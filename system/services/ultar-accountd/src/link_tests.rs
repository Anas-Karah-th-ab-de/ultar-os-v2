use super::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use ultar_bus::messages::account_service::{
    AccountService, ChangePassword, Create, Delete, DeviceRemove, SetServer, SetService, SignIn,
};

/// An account server in memory: accounts, one device per sign-in, reachable or not.
#[derive(Default)]
struct World {
    up: bool,
    accounts: HashMap<String, String>,
    tokens: HashMap<String, String>,
    devices: Vec<(String, String)>,
    removed: Vec<String>,
    next: u32,
}

#[derive(Clone, Default)]
struct Fake(Rc<RefCell<World>>);

impl Fake {
    fn up() -> Self {
        let f = Fake::default();
        f.0.borrow_mut().up = true;
        f
    }
    fn token(&self, user: &str) -> String {
        let mut w = self.0.borrow_mut();
        w.next += 1;
        let t = format!("t{}", w.next);
        w.tokens.insert(t.clone(), user.to_owned());
        t
    }
    fn reach(&self) -> Result<(), LinkError> {
        if self.0.borrow().up {
            Ok(())
        } else {
            Err(LinkError::Unreachable)
        }
    }
    fn user_of(&self, token: &str) -> Result<String, LinkError> {
        self.reach()?;
        self.0
            .borrow()
            .tokens
            .get(token)
            .cloned()
            .ok_or(LinkError::SessionEnded)
    }
}

impl Server for Fake {
    fn health(&self, server: &str) -> Result<(), LinkError> {
        self.reach()?;
        if server == "nobody.example" {
            return Err(LinkError::Unreachable);
        }
        Ok(())
    }
    fn login(&self, _: &str, user: &str, password: &str) -> Result<String, LinkError> {
        self.reach()?;
        if self.0.borrow().accounts.get(user).map(String::as_str) != Some(password) {
            return Err(LinkError::WrongPassword);
        }
        Ok(self.token(user))
    }
    fn register(&self, _: &str, user: &str, password: &str) -> Result<String, LinkError> {
        self.reach()?;
        if self.0.borrow().accounts.contains_key(user) {
            return Err(LinkError::NameTaken);
        }
        self.0
            .borrow_mut()
            .accounts
            .insert(user.to_owned(), password.to_owned());
        Ok(self.token(user))
    }
    fn enroll(&self, _: &str, token: &str, name: &str) -> Result<Enrolled, LinkError> {
        self.user_of(token)?;
        let mut w = self.0.borrow_mut();
        if !w.devices.iter().any(|(id, _)| id == "me") {
            w.devices.push(("me".into(), name.into()));
        }
        Ok(Enrolled {
            number: "UOS-AAAA-0001".into(),
            device_token: format!("device-{token}"),
        })
    }
    fn devices(&self, _: &str, token: &str) -> Result<Vec<RemoteDevice>, LinkError> {
        self.user_of(token)?;
        Ok(self
            .0
            .borrow()
            .devices
            .iter()
            .map(|(id, name)| RemoteDevice {
                id: id.clone(),
                name: name.clone(),
                current: id == "me",
                last_seen: 7,
            })
            .collect())
    }
    fn change_password(
        &self,
        _: &str,
        token: &str,
        old: &str,
        new: &str,
    ) -> Result<String, LinkError> {
        let user = self.user_of(token)?;
        if self.0.borrow().accounts.get(&user).map(String::as_str) != Some(old) {
            return Err(LinkError::WrongPassword);
        }
        let mut w = self.0.borrow_mut();
        w.accounts.insert(user.clone(), new.to_owned());
        w.tokens.clear();
        drop(w);
        Ok(self.token(&user))
    }
    fn remove_device(&self, _: &str, token: &str, device: &str) -> Result<(), LinkError> {
        self.user_of(token)?;
        let mut w = self.0.borrow_mut();
        w.devices.retain(|(id, _)| id != device);
        w.removed.push(device.to_owned());
        Ok(())
    }
}

struct T {
    dir: tempfile::TempDir,
    fake: Fake,
}

impl T {
    fn new(fake: Fake) -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            fake,
        }
    }
    fn relay(&self) -> PathBuf {
        self.dir.path().join("relay/relay.token")
    }
    fn link(&self) -> Link<Fake> {
        let store = Store::new(self.dir.path(), Some(self.relay()), None);
        Link::open(self.fake.clone(), DEFAULT_SERVER, store, "Laptop", "laptop")
    }
}

fn sign_in(user: &str, password: &str) -> LinkCall {
    LinkCall::SignIn(SignIn {
        user: user.into(),
        password: password.into(),
    })
}

fn key(r: &Reply) -> String {
    r.outcome.clone().unwrap_err().reason.key
}

#[test]
fn creating_an_account_signs_in_registers_the_device_and_hands_the_relay_its_token() {
    let t = T::new(Fake::up());
    let now = Instant::now();
    let mut link = t.link();
    assert_eq!(link.state(now).phase, Phase::SignedOut);
    assert_eq!(link.state(now).server, DEFAULT_SERVER);
    let create = LinkCall::Create(Create {
        user: "anas".into(),
        password: "secret-1".into(),
        email: None,
    });
    let st = link.call(create, now).outcome.unwrap();
    assert_eq!(
        (st.phase, st.user.as_str(), st.number.as_str()),
        (Phase::SignedIn, "anas", "UOS-AAAA-0001")
    );
    assert_eq!(st.devices.len(), 1);
    assert!(st.devices[0].this_device);
    assert_eq!(st.devices[0].kind, "laptop");
    assert_eq!(std::fs::read_to_string(t.relay()).unwrap(), "device-t1");
    // a second start knows the account and asks the server again
    drop(link);
    let mut again = t.link();
    assert_eq!(again.state(now).phase, Phase::Offline);
    again.refresh();
    let st = again.state(now);
    assert_eq!((st.phase, st.user.as_str()), (Phase::SignedIn, "anas"));
    // signing out forgets the token and the relay's token
    let st = again.call(LinkCall::SignOut, now).outcome.unwrap();
    assert_eq!((st.phase, st.user.as_str()), (Phase::SignedOut, ""));
    assert!(!t.relay().exists());
    assert_eq!(t.link().state(now).phase, Phase::SignedOut);
}

#[test]
fn only_a_wrong_password_uses_up_attempts_and_then_sign_in_waits() {
    let t = T::new(Fake::default());
    let now = Instant::now();
    let mut link = t.link();
    // server away: no attempt used
    let r = link.call(sign_in("anas", "x"), now);
    assert_eq!(key(&r), "accountd.link.unreachable");
    assert_eq!(link.state(now).attempts_remaining, SIGN_IN_ATTEMPTS);
    t.fake.0.borrow_mut().up = true;
    t.fake
        .0
        .borrow_mut()
        .accounts
        .insert("anas".into(), "right".into());
    for left in (1..SIGN_IN_ATTEMPTS).rev() {
        let r = link.call(sign_in("anas", "wrong"), now);
        assert_eq!(key(&r), "accountd.link.wrong_password");
        assert_eq!(link.state(now).attempts_remaining, left);
    }
    link.call(sign_in("anas", "wrong"), now);
    let st = link.state(now);
    assert_eq!((st.phase, st.locked_minutes), (Phase::Locked, LOCK_MINUTES));
    assert_eq!(
        key(&link.call(sign_in("anas", "right"), now)),
        "accountd.link.locked"
    );
    let later = now + Duration::from_secs(u64::from(LOCK_MINUTES) * 60 + 1);
    let st = link.call(sign_in("anas", "right"), later).outcome.unwrap();
    assert_eq!(
        (st.phase, st.attempts_remaining),
        (Phase::SignedIn, SIGN_IN_ATTEMPTS)
    );
}

#[test]
fn services_and_the_server_follow_the_rules() {
    let t = T::new(Fake::up());
    let now = Instant::now();
    let mut link = t.link();
    let calls = LinkCall::SetService(SetService {
        service: AccountService::Calls,
        on: true,
    });
    assert_eq!(
        key(&link.call(calls.clone(), now)),
        "accountd.link.needs_account"
    );
    // another server: checked before it is kept
    let bad = LinkCall::SetServer(SetServer {
        server: "nobody.example".into(),
    });
    assert_eq!(key(&link.call(bad, now)), "accountd.link.unreachable");
    let odd = LinkCall::SetServer(SetServer {
        server: "a b".into(),
    });
    assert_eq!(key(&link.call(odd, now)), "accountd.link.rejected");
    let own = LinkCall::SetServer(SetServer {
        server: "Cloud.Example".into(),
    });
    assert_eq!(link.call(own, now).outcome.unwrap().server, "cloud.example");
    t.fake
        .0
        .borrow_mut()
        .accounts
        .insert("anas".into(), "old".into());
    link.call(sign_in("anas", "old"), now).outcome.unwrap();
    assert_eq!(
        key(&link.call(sign_in("anas", "old"), now)),
        "accountd.link.signed_in"
    );
    let st = link.call(calls, now).outcome.unwrap();
    assert_eq!(st.services, vec![AccountService::Calls]);
}

#[test]
fn the_password_and_the_devices_change_on_the_server() {
    let t = T::new(Fake::up());
    let now = Instant::now();
    let mut link = t.link();
    t.fake
        .0
        .borrow_mut()
        .accounts
        .insert("anas".into(), "old".into());
    link.call(sign_in("anas", "old"), now).outcome.unwrap();
    // the password: a wrong old one changes nothing; the new one keeps the device signed in
    let wrong = LinkCall::ChangePassword(ChangePassword {
        old: "nope".into(),
        new: "new".into(),
    });
    assert_eq!(key(&link.call(wrong, now)), "accountd.link.wrong_password");
    let right = LinkCall::ChangePassword(ChangePassword {
        old: "old".into(),
        new: "new".into(),
    });
    assert_eq!(
        link.call(right, now).outcome.unwrap().phase,
        Phase::SignedIn
    );
    assert!(std::fs::read_to_string(t.relay())
        .unwrap()
        .starts_with("device-"));
    // another device goes; this one leaves only by signing out
    t.fake
        .0
        .borrow_mut()
        .devices
        .push(("tab".into(), "Tablet".into()));
    let gone = LinkCall::DeviceRemove(DeviceRemove {
        device: "tab".into(),
    });
    link.refresh();
    assert_eq!(link.state(now).devices.len(), 2);
    assert_eq!(link.call(gone, now).outcome.unwrap().devices.len(), 1);
    let me = LinkCall::DeviceRemove(DeviceRemove {
        device: "me".into(),
    });
    assert_eq!(key(&link.call(me, now)), "accountd.link.not_offered");
    let delete = LinkCall::Delete(Delete {
        password: "new".into(),
    });
    assert_eq!(key(&link.call(delete, now)), "accountd.link.not_offered");
}

#[test]
fn a_session_the_server_ended_signs_the_device_out_with_the_reason() {
    let t = T::new(Fake::up());
    let now = Instant::now();
    let mut link = t.link();
    t.fake
        .0
        .borrow_mut()
        .accounts
        .insert("anas".into(), "pw".into());
    link.call(sign_in("anas", "pw"), now).outcome.unwrap();
    t.fake.0.borrow_mut().tokens.clear();
    link.refresh();
    let st = link.state(now);
    assert_eq!(st.phase, Phase::SignedOut);
    assert_eq!(st.error.unwrap().key, "accountd.link.session_ended");
    assert!(!t.relay().exists());
    // server away while signed in: offline, nothing lost
    link.call(sign_in("anas", "pw"), now).outcome.unwrap();
    t.fake.0.borrow_mut().up = false;
    link.refresh();
    let st = link.state(now);
    assert_eq!((st.phase, st.user.as_str()), (Phase::Offline, "anas"));
    assert!(t.relay().exists());
}

#[test]
fn every_link_method_is_known() {
    for m in [
        "status",
        "sign_in",
        "sign_out",
        "create",
        "set_server",
        "set_service",
        "change_password",
        "delete",
        "device_sign_out",
        "device_remove",
    ] {
        assert!(is_link_method(m), "{m}");
    }
    assert!(!is_link_method("user_create"));
}
