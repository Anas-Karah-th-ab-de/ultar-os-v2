//! The real `ultar-accountd` on a bus: the first administrator, the system
//! dialog for everything else, what a standard user may and may not do,
//! a crash while the dialog is open, a damaged account file, the Ultar
//! account link state. Runs as root and as another user (umask 002 too).

mod common;

use common::*;
use std::os::unix::fs::MetadataExt;
use std::time::Duration;
use ultar_accountd::store::{self, Stored, UserRecord};
use ultar_bus::messages::account_local::{
    CreateUser, PictureInfo, RemoveData, RemoveUser, RenameUser, Secret, SetKind, SetPassword,
    SetSession, UserKind,
};
use ultar_bus::messages::account_service::{Phase, SignIn, Status};
use ultar_bus::{BusError, ErrorCode};

fn create(name: &str, kind: UserKind) -> CreateUser {
    CreateUser {
        name: name.into(),
        display_name: format!("User {name}"),
        kind,
        password: Secret::new("sesame-1".into()),
        confirm: Secret::new("sesame-1".into()),
    }
}

fn refused<T: std::fmt::Debug>(
    r: Result<T, ultar_bus::messages::account_service::Refusal>,
    key: &str,
) {
    match r {
        Err(refusal) => assert_eq!(refusal.reason.key, key),
        Ok(v) => panic!("expected {key}, got {v:?}"),
    }
}

#[test]
fn first_administrator_then_the_dialog_for_everything_else() {
    let w = World::new("first");
    let _d = w.start();
    let installer = connect(&w.socket, "system.installer");
    let app = connect(&w.socket, "app.settings");
    let (first, _) = w.uids();
    let made = installer
        .call(&create("anas", UserKind::Administrator), WAIT)
        .unwrap()
        .outcome
        .unwrap();
    assert_eq!(made.users[0].uid, first);
    assert!(
        w.asked().is_empty(),
        "the very first administrator needs no dialog"
    );
    assert!(w.cred("anas").unwrap().admin);
    let home = std::fs::metadata(w.home("anas")).unwrap();
    assert_eq!((home.uid(), home.mode() & 0o777), (first, 0o700));
    // Not empty any more: the set-up identity gets the dialog like anybody.
    refused(
        installer
            .call(&create("omar", UserKind::Administrator), WAIT)
            .unwrap()
            .outcome,
        "accountd.refused.not_authorized",
    );
    assert_eq!(w.asked(), vec!["accountd.action.create"]);
    refused(
        app.call(&create("leila", UserKind::Standard), WAIT)
            .unwrap()
            .outcome,
        "accountd.refused.not_authorized",
    );
    assert!(w.cred("leila").is_none() && !w.home("leila").exists());
    w.answer(Answer::Yes);
    let reply = app
        .call(&create("leila", UserKind::Standard), WAIT)
        .unwrap()
        .outcome;
    if me() == 0 {
        let users = reply.unwrap();
        assert_eq!(users.users[1].uid, first + 1);
        assert_eq!(std::fs::metadata(w.home("leila")).unwrap().uid(), first + 1);
        assert!(!w.cred("leila").unwrap().admin);
    } else {
        // Without the right to hand a folder to another id, nothing is made.
        refused(reply, "accountd.refused.home_failed");
        assert!(w.cred("leila").is_none());
    }
    let bad = app
        .call(&create("root", UserKind::Standard), WAIT)
        .unwrap()
        .outcome;
    refused(bad, "accountd.refused.reserved_name");
    assert!(
        w.asked().len() == 2,
        "no dialog for a change that is refused anyway"
    );
}

fn seeded(tag: &str) -> World {
    let w = World::new(tag);
    let (first, _) = w.uids();
    let record = |name: &str, uid, kind| UserRecord {
        name: name.into(),
        uid,
        display: name.into(),
        kind,
        picture: PictureInfo::Initials,
        auto_login: false,
    };
    let stored = Stored {
        users: vec![
            record("anas", first, UserKind::Administrator),
            record("leila", first + 1, UserKind::Standard),
        ],
        ..Stored::default()
    };
    store::save(&w.dir.join("state"), &stored).unwrap();
    w.seed("anas", "anas-pass", true);
    w.seed("leila", "leila-pass", false);
    w
}

#[test]
fn a_standard_user_cannot_change_others_or_become_an_administrator() {
    let w = seeded("standard");
    let _d = w.start();
    let lock = connect(&w.socket, "system.lock");
    let app = connect(&w.socket, "app.settings");
    lock.call(
        &SetSession {
            user: Some("leila".into()),
        },
        WAIT,
    )
    .unwrap()
    .outcome
    .unwrap();
    let own = RenameUser {
        user: "leila".into(),
        display_name: "ليلى".into(),
    };
    let after = app.call(&own, WAIT).unwrap().outcome.unwrap();
    assert!(after.users[1].current && after.users[1].display_name == "ليلى");
    assert!(w.asked().is_empty(), "own name: no dialog");
    let other = RenameUser {
        user: "anas".into(),
        display_name: "x".into(),
    };
    refused(
        app.call(&other, WAIT).unwrap().outcome,
        "accountd.refused.not_authorized",
    );
    let up = SetKind {
        user: "leila".into(),
        kind: UserKind::Administrator,
    };
    refused(
        app.call(&up, WAIT).unwrap().outcome,
        "accountd.refused.not_authorized",
    );
    assert_eq!(
        w.asked(),
        vec!["accountd.action.rename", "accountd.action.kind"]
    );
    assert!(!w.cred("leila").unwrap().admin && w.cred("anas").unwrap().display == "anas");
}

#[test]
fn a_standard_user_changes_the_own_password_with_the_current_one() {
    let w = seeded("password");
    let _d = w.start();
    let lock = connect(&w.socket, "system.lock");
    let app = connect(&w.socket, "app.settings");
    lock.call(
        &SetSession {
            user: Some("leila".into()),
        },
        WAIT,
    )
    .unwrap()
    .outcome
    .unwrap();
    let pw = |current: &str| SetPassword {
        user: "leila".into(),
        current: Some(Secret::new(current.into())),
        new: Secret::new("better-pass".into()),
        confirm: Secret::new("better-pass".into()),
    };
    refused(
        app.call(&pw("guess"), WAIT).unwrap().outcome,
        "accountd.refused.wrong_password",
    );
    std::thread::sleep(Duration::from_millis(1100));
    app.call(&pw("leila-pass"), WAIT).unwrap().outcome.unwrap();
    assert_eq!(w.cred("leila").unwrap().password, "better-pass");
    // The signed-in user cannot be removed, even by an administrator.
    w.answer(Answer::Yes);
    let remove = RemoveUser {
        user: "leila".into(),
        data: RemoveData::Keep,
    };
    refused(
        app.call(&remove, WAIT).unwrap().outcome,
        "accountd.refused.current_user",
    );
}

#[test]
fn the_broker_stops_apps_without_the_right_and_the_link_is_offline() {
    let w = seeded("broker");
    let _d = w.start();
    let lock = connect(&w.socket, "system.lock");
    let app = connect(&w.socket, "app.settings");
    lock.call(
        &SetSession {
            user: Some("leila".into()),
        },
        WAIT,
    )
    .unwrap()
    .outcome
    .unwrap();
    // Apps without the right are stopped by the broker.
    let evil = connect(&w.socket, "app.evil");
    let denied = evil.call(
        &RenameUser {
            user: "anas".into(),
            display_name: "x".into(),
        },
        WAIT,
    );
    assert!(matches!(
        denied,
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    let session = app.call(
        &SetSession {
            user: Some("anas".into()),
        },
        WAIT,
    );
    assert!(matches!(
        session,
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    // The Ultar account: signed out, with the reason; signing in is refused.
    let state = app.call(&Status {}, WAIT).unwrap().outcome.unwrap();
    assert_eq!(state.phase, Phase::SignedOut);
    assert_eq!(state.error.unwrap().key, "accountd.link.no_server");
    let sign_in = SignIn {
        user: "x".into(),
        password: "y".into(),
    };
    refused(
        app.call(&sign_in, WAIT).unwrap().outcome,
        "accountd.link.no_server",
    );
}

#[test]
fn an_administrator_removes_with_the_files_kept_or_deleted() {
    let w = seeded("remove");
    let _d = w.start();
    let app = connect(&w.socket, "app.settings");
    std::fs::create_dir_all(w.home("leila")).unwrap();
    std::fs::write(w.home("leila").join("note"), b"x").unwrap();
    let (first, _) = w.uids();
    w.answer(Answer::Yes);
    let remove = RemoveUser {
        user: "leila".into(),
        data: RemoveData::Keep,
    };
    let left = app.call(&remove, WAIT).unwrap().outcome.unwrap();
    assert_eq!(left.users.len(), 1);
    assert!(w.cred("leila").is_none());
    let kept = w
        .dir
        .join("home/.removed")
        .join(format!("leila-{}", first + 1))
        .join("note");
    assert!(kept.exists());
    w.answer(Answer::Yes);
    let last = RemoveUser {
        user: "anas".into(),
        data: RemoveData::Delete,
    };
    refused(
        app.call(&last, WAIT).unwrap().outcome,
        "accountd.refused.last_admin",
    );
    assert!(w.cred("anas").is_some());
}

#[test]
fn a_crash_while_the_dialog_is_open_changes_nothing() {
    let w = seeded("crash");
    let d = w.start();
    let app = connect(&w.socket, "app.settings");
    w.answer(Answer::Hang);
    let socket = w.socket.clone();
    let waiting = std::thread::spawn(move || {
        let app = connect(&socket, "app.settings");
        app.call(
            &SetKind {
                user: "leila".into(),
                kind: UserKind::Administrator,
            },
            Duration::from_secs(10),
        )
    });
    let start = std::time::Instant::now();
    while w.shared.lock().unwrap().asked.is_empty() {
        assert!(start.elapsed() < WAIT);
        std::thread::sleep(Duration::from_millis(20));
    }
    // Meanwhile the service still answers others.
    assert_eq!(users(&app).users.len(), 2);
    let busy = app.call(
        &RenameUser {
            user: "anas".into(),
            display_name: "y".into(),
        },
        WAIT,
    );
    refused(busy.unwrap().outcome, "accountd.refused.busy");
    drop(d);
    assert!(
        waiting.join().unwrap().is_err(),
        "the asking app gets no success"
    );
    let _d = w.start();
    let after = users(&app);
    assert_eq!(after.users[1].kind, UserKind::Standard);
    assert!(!w.cred("leila").unwrap().admin);
}

#[test]
fn a_damaged_account_file_fails_closed() {
    let w = World::new("damaged");
    std::fs::write(
        w.dir.join("state/users"),
        b"#ultar-accounts crc32=deadbeef\nuser x\n",
    )
    .unwrap();
    let _d = w.start();
    let installer = connect(&w.socket, "system.installer");
    assert!(users(&installer).users.is_empty());
    let r = installer
        .call(&create("anas", UserKind::Administrator), WAIT)
        .unwrap()
        .outcome;
    refused(r, "accountd.refused.store_damaged");
    assert!(w.cred("anas").is_none() && !w.home("anas").exists());
}
