use super::*;
use crate::names::parse_ids;
use std::cell::RefCell;
use std::collections::BTreeMap;
use ultar_bus::messages::account_local::{
    CreateUser, RemoveUser, RenameUser, SetAutoLogin, SetKind, SetPassword, SetPicture,
};

#[derive(Default)]
struct FakeCreds {
    users: RefCell<BTreeMap<String, (String, String, bool)>>,
    down: RefCell<bool>,
    log: RefCell<Vec<String>>,
}

impl Credentials for FakeCreds {
    fn available(&self) -> bool {
        !*self.down.borrow()
    }
    fn verify(&self, user: &str, password: &Secret) -> Check {
        if *self.down.borrow() {
            return Check::Unavailable;
        }
        match self.users.borrow().get(user) {
            Some((p, _, _)) if p == password.expose() => Check::Granted,
            _ => Check::Denied(1_000),
        }
    }
    fn set_password(&self, user: &str, display: &str, password: &Secret) -> Result<(), CredError> {
        if *self.down.borrow() {
            return Err(CredError::Unavailable);
        }
        self.log.borrow_mut().push(format!("set {user}"));
        let mut users = self.users.borrow_mut();
        let admin = users.get(user).is_some_and(|u| u.2);
        users.insert(
            user.into(),
            (password.expose().into(), display.into(), admin),
        );
        Ok(())
    }
    fn update(
        &self,
        user: &str,
        display: Option<&str>,
        admin: Option<bool>,
    ) -> Result<(), CredError> {
        let mut users = self.users.borrow_mut();
        let u = users.get_mut(user).ok_or(CredError::Rejected)?;
        if let Some(d) = display {
            u.1 = d.into();
        }
        if let Some(a) = admin {
            u.2 = a;
        }
        Ok(())
    }
    fn remove(&self, user: &str) -> Result<(), CredError> {
        self.log.borrow_mut().push(format!("remove {user}"));
        self.users
            .borrow_mut()
            .remove(user)
            .map(|_| ())
            .ok_or(CredError::Rejected)
    }
}

#[derive(Default)]
struct FakeHomes(RefCell<Vec<String>>, RefCell<bool>);

impl Homes for FakeHomes {
    fn create(&self, name: &str, uid: u32) -> std::io::Result<()> {
        if *self.1.borrow() {
            return Err(std::io::Error::other("full"));
        }
        self.0.borrow_mut().push(format!("create {name} {uid}"));
        Ok(())
    }
    fn retire(&self, name: &str, uid: u32) -> std::io::Result<()> {
        self.0.borrow_mut().push(format!("retire {name} {uid}"));
        Ok(())
    }
    fn delete(&self, name: &str) -> std::io::Result<()> {
        self.0.borrow_mut().push(format!("delete {name}"));
        Ok(())
    }
}

struct Rig {
    creds: FakeCreds,
    homes: FakeHomes,
    dir: PathBuf,
}

impl Rig {
    fn new(name: &str) -> (Rig, Accounts) {
        let dir = std::env::temp_dir().join(format!("ultar-acc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let system = parse_ids("[users]\nroot = 0\ndaemon = 1000\n").unwrap();
        let config = Config {
            dir: dir.clone(),
            uids: (1000, 1003),
            system,
        };
        let rig = Rig {
            creds: FakeCreds::default(),
            homes: FakeHomes::default(),
            dir,
        };
        (rig, Accounts::open(config))
    }
    fn ports(&self) -> Ports<'_> {
        Ports {
            credentials: &self.creds,
            homes: &self.homes,
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn pw(s: &str) -> Secret {
    Secret::new(s.into())
}

fn create(name: &str, kind: UserKind) -> Op {
    Op::Create(CreateUser {
        name: name.into(),
        display_name: format!("User {name}"),
        kind,
        password: pw("sesame-1"),
        confirm: pw("sesame-1"),
    })
}

fn reason(r: Result<(), Refusal>) -> String {
    r.err().map(|e| e.reason.key).unwrap_or_default()
}

#[test]
fn creating_users_checks_names_ids_and_passwords() {
    let (rig, mut acc) = Rig::new("create");
    assert!(acc.empty());
    assert!(acc.needs_admin(&create("anas", UserKind::Administrator)));
    acc.perform(create("anas", UserKind::Administrator), &rig.ports(), 0)
        .unwrap();
    let view = acc.view(true);
    assert_eq!(view.users[0].uid, 1001, "1000 belongs to the system");
    assert!(
        rig.creds.users.borrow()["anas"].2,
        "administrator in the store"
    );
    assert_eq!(rig.homes.0.borrow()[0], "create anas 1001");
    for (op, key) in [
        (create("Anas", UserKind::Standard), Key::InvalidName),
        (create("root", UserKind::Standard), Key::ReservedName),
        (create("anas", UserKind::Standard), Key::NameTaken),
    ] {
        assert_eq!(reason(acc.precheck(&op)), key.key());
    }
    let mut differ = create("leila", UserKind::Standard);
    if let Op::Create(c) = &mut differ {
        c.confirm = pw("sesame-2");
    }
    assert_eq!(reason(acc.precheck(&differ)), Key::PasswordsDiffer.key());
    let mut blank = create("leila", UserKind::Standard);
    if let Op::Create(c) = &mut blank {
        c.display_name = "\u{202e}x".into();
    }
    assert_eq!(reason(acc.precheck(&blank)), Key::InvalidDisplayName.key());
    acc.perform(create("leila", UserKind::Standard), &rig.ports(), 0)
        .unwrap();
    acc.perform(create("omar", UserKind::Standard), &rig.ports(), 0)
        .unwrap();
    assert_eq!(
        reason(acc.perform(create("zaid", UserKind::Standard), &rig.ports(), 0)),
        Key::NoFreeId.key()
    );
    drop(acc);
    let (_, again) = (
        0,
        Accounts::open(Config {
            dir: rig.dir.clone(),
            uids: (1000, 1003),
            system: parse_ids("").unwrap(),
        }),
    );
    assert_eq!(again.view(true).users.len(), 3, "stored");
}

#[test]
fn a_failed_step_leaves_nothing_behind() {
    let (rig, mut acc) = Rig::new("rollback");
    *rig.creds.down.borrow_mut() = true;
    assert_eq!(
        reason(acc.perform(create("anas", UserKind::Administrator), &rig.ports(), 0)),
        Key::CredentialsUnavailable.key()
    );
    assert_eq!(
        *rig.homes.0.borrow(),
        vec!["create anas 1001", "delete anas"]
    );
    assert!(acc.empty());
    *rig.creds.down.borrow_mut() = false;
    *rig.homes.1.borrow_mut() = true;
    assert_eq!(
        reason(acc.perform(create("anas", UserKind::Administrator), &rig.ports(), 0)),
        Key::HomeFailed.key()
    );
    assert!(
        rig.creds.users.borrow().is_empty(),
        "no password stored without a home"
    );
}

#[test]
fn own_changes_need_no_dialog_others_do() {
    let (rig, mut acc) = Rig::new("own");
    acc.perform(create("anas", UserKind::Administrator), &rig.ports(), 0)
        .unwrap();
    acc.perform(create("leila", UserKind::Standard), &rig.ports(), 0)
        .unwrap();
    assert_eq!(
        reason(acc.set_session(Some("ghost".into()))),
        Key::UnknownUser.key()
    );
    acc.set_session(Some("leila".into())).unwrap();
    let rename = |u: &str| {
        Op::Rename(RenameUser {
            user: u.into(),
            display_name: "ليلى".into(),
        })
    };
    let picture = |u: &str| {
        Op::Picture(SetPicture {
            user: u.into(),
            picture: Picture::Builtin(3),
        })
    };
    let password = |u: &str, current: Option<&str>| {
        Op::Password(SetPassword {
            user: u.into(),
            current: current.map(pw),
            new: pw("new-pass"),
            confirm: pw("new-pass"),
        })
    };
    assert!(!acc.needs_admin(&rename("leila")));
    assert!(!acc.needs_admin(&picture("leila")));
    assert!(!acc.needs_admin(&password("leila", Some("x"))));
    assert!(
        acc.needs_admin(&password("leila", None)),
        "without the current one"
    );
    assert!(acc.needs_admin(&rename("anas")), "another user");
    assert!(acc.needs_admin(&Op::Kind(SetKind {
        user: "leila".into(),
        kind: UserKind::Administrator
    })));
    assert!(acc.needs_admin(&Op::AutoLogin(SetAutoLogin {
        user: Some("leila".into())
    })));
    acc.perform(rename("leila"), &rig.ports(), 0).unwrap();
    assert_eq!(rig.creds.users.borrow()["leila"].1, "ليلى");
    acc.perform(picture("leila"), &rig.ports(), 0).unwrap();
}

#[test]
fn own_pictures_are_checked_and_numbered() {
    let (rig, mut acc) = Rig::new("pictures");
    acc.perform(create("anas", UserKind::Administrator), &rig.ports(), 0)
        .unwrap();
    acc.perform(create("leila", UserKind::Standard), &rig.ports(), 0)
        .unwrap();
    acc.set_session(Some("leila".into())).unwrap();
    let bad = Op::Picture(SetPicture {
        user: "leila".into(),
        picture: Picture::Image(b"GIF89a".to_vec()),
    });
    assert_eq!(reason(acc.precheck(&bad)), Key::PictureInvalid.key());
    let png = Op::Picture(SetPicture {
        user: "leila".into(),
        picture: Picture::Image(crate::picture::tests::png_bytes()),
    });
    acc.perform(png, &rig.ports(), 0).unwrap();
    assert_eq!(acc.view(true).users[1].picture, PictureInfo::Custom(1));
    assert!(acc.view(true).users[1].current);
}

#[test]
fn a_wrong_current_password_waits_longer_each_time() {
    let (rig, mut acc) = Rig::new("password");
    acc.perform(create("anas", UserKind::Administrator), &rig.ports(), 0)
        .unwrap();
    acc.set_session(Some("anas".into())).unwrap();
    let change = |current: &str| {
        Op::Password(SetPassword {
            user: "anas".into(),
            current: Some(pw(current)),
            new: pw("better-1"),
            confirm: pw("better-1"),
        })
    };
    let r = acc.perform(change("wrong"), &rig.ports(), 0).unwrap_err();
    assert_eq!(r.reason.key, Key::WrongPassword.key());
    let r = acc
        .perform(change("sesame-1"), &rig.ports(), 500)
        .unwrap_err();
    assert_eq!(
        r.reason.key,
        Key::Throttled.key(),
        "even the right one waits"
    );
    acc.perform(change("wrong"), &rig.ports(), 1_000)
        .unwrap_err();
    assert!(acc
        .perform(change("sesame-1"), &rig.ports(), 2_500)
        .is_err());
    acc.perform(change("sesame-1"), &rig.ports(), 3_000)
        .unwrap();
    assert_eq!(rig.creds.users.borrow()["anas"].0, "better-1");
    let format = format!("{:?}", change("sesame-1"));
    assert!(
        !format.contains("sesame") && !format.contains("better"),
        "{format}"
    );
}

#[test]
fn there_is_always_an_administrator_and_removal_keeps_or_deletes() {
    let (rig, mut acc) = Rig::new("remove");
    acc.perform(create("anas", UserKind::Administrator), &rig.ports(), 0)
        .unwrap();
    acc.perform(create("leila", UserKind::Standard), &rig.ports(), 0)
        .unwrap();
    let demote = Op::Kind(SetKind {
        user: "anas".into(),
        kind: UserKind::Standard,
    });
    assert_eq!(reason(acc.precheck(&demote)), Key::LastAdmin.key());
    let remove = |u: &str, data| {
        Op::Remove(RemoveUser {
            user: u.into(),
            data,
        })
    };
    assert_eq!(
        reason(acc.precheck(&remove("anas", RemoveData::Keep))),
        Key::LastAdmin.key()
    );
    acc.set_session(Some("leila".into())).unwrap();
    assert_eq!(
        reason(acc.precheck(&remove("leila", RemoveData::Keep))),
        Key::CurrentUser.key()
    );
    acc.set_session(None).unwrap();
    let promote = Op::Kind(SetKind {
        user: "leila".into(),
        kind: UserKind::Administrator,
    });
    acc.perform(promote, &rig.ports(), 0).unwrap();
    assert!(rig.creds.users.borrow()["leila"].2);
    acc.perform(demote, &rig.ports(), 0).unwrap();
    let auto = |u: Option<&str>| {
        Op::AutoLogin(SetAutoLogin {
            user: u.map(str::to_owned),
        })
    };
    acc.perform(auto(Some("anas")), &rig.ports(), 0).unwrap();
    acc.perform(auto(Some("leila")), &rig.ports(), 0).unwrap();
    let flags: Vec<bool> = acc.view(true).users.iter().map(|u| u.auto_login).collect();
    assert_eq!(flags, vec![false, true], "only one signs in by itself");
    acc.perform(remove("anas", RemoveData::Keep), &rig.ports(), 0)
        .unwrap();
    assert_eq!(rig.homes.0.borrow().last().unwrap(), "retire anas 1001");
    acc.perform(create("omar", UserKind::Standard), &rig.ports(), 0)
        .unwrap();
    assert_eq!(
        acc.view(true).users[1].uid,
        1003,
        "1001 is never given again"
    );
    acc.perform(remove("omar", RemoveData::Delete), &rig.ports(), 0)
        .unwrap();
    assert_eq!(rig.homes.0.borrow().last().unwrap(), "delete omar");
    assert!(acc.take_warnings().is_empty());
}

#[test]
fn a_damaged_account_file_refuses_everything() {
    let (rig, acc) = Rig::new("damaged");
    drop(acc);
    std::fs::write(
        rig.dir.join(crate::store::FILE),
        b"#ultar-accounts crc32=00000000\nuser x\n",
    )
    .unwrap();
    let mut acc = Accounts::open(Config {
        dir: rig.dir.clone(),
        uids: (1000, 1003),
        system: Default::default(),
    });
    assert!(!acc.usable());
    assert_eq!(
        reason(acc.perform(create("anas", UserKind::Administrator), &rig.ports(), 0)),
        Key::StoreDamaged.key()
    );
    assert!(acc.view(false).users.is_empty());
    assert!(rig.homes.0.borrow().is_empty());
}
