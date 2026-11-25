//! The real `ultar-sessiond` with stand-ins for the authentication service, the lock
//! screen and the account service.

use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::account_local::{LocalUsers, SetSession, UsersReply as AccountReply};
use ultar_bus::messages::auth::{UserInfo, Users as AuthUsers, UsersReply as AuthReply};
use ultar_bus::messages::lock::{Lock, LockReply};
use ultar_bus::messages::notify::{Post, PostReply};
use ultar_bus::messages::session::{
    SessionOutcome, SessionState, SignOut, SignedIn, SwitchUser, Users,
};
use ultar_bus::messages::settings::{Changed, Get, GetReply, SettingValue};
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.sessiond"
register = ["session"]
publish = ["session.state"]
subscribe = ["account.users", "settings.changed"]
call = ["auth.users", "lock.lock", "account.session", "settings.get", "notify.post"]

[[rule]]
identity = "system.settingsd"
register = ["settings"]
publish = ["settings.changed"]

[[rule]]
identity = "system.notifyd"
register = ["notify"]

[[rule]]
identity = "system.authd"
register = ["auth"]

[[rule]]
identity = "system.lock"
register = ["lock"]
call = ["session.signed_in", "session.signin", "session.users", "session.system"]

[[rule]]
identity = "system.accountd"
register = ["account"]
publish = ["account.users"]

[[rule]]
identity = "system.shell"
call = ["session.switch_user", "session.sign_out", "session.users", "session.system", "session.signed_in"]
subscribe = ["session.state"]

[[rule]]
identity = "app.notes"
call = ["session.sign_out"]
"#;

const IDS: [&str; 8] = [
    "system.sessiond",
    "system.settingsd",
    "system.notifyd",
    "system.authd",
    "system.lock",
    "system.accountd",
    "system.shell",
    "app.notes",
];

#[derive(Default)]
struct Seen {
    locks: u32,
    lock_ok: bool,
    told: Vec<Option<String>>,
    /// What the settings stand-in answers for `settings.get`.
    settings: std::collections::BTreeMap<String, SettingValue>,
    /// How often `settings.get` was asked (the start read happened).
    queries: u32,
    /// The notifications the session service posted.
    posts: Vec<Post>,
}

fn setup(tag: &str) -> (World, Arc<Mutex<Seen>>) {
    let world = World::new(tag, POLICY, &IDS);
    let seen = Arc::new(Mutex::new(Seen {
        lock_ok: true,
        ..Seen::default()
    }));
    world.serve("system.authd", "auth", |r| {
        let _ = r.respond::<AuthUsers>(|_| {
            Ok(AuthReply {
                users: ["amal", "badr"]
                    .iter()
                    .map(|n| UserInfo {
                        name: (*n).into(),
                        display_name: format!("{n}!"),
                        has_pin: false,
                        // badr is the administrator; amal a standard user.
                        admin: *n == "badr",
                    })
                    .collect(),
            })
        });
    });
    serve_settings_notify(&world, &seen);
    let s = seen.clone();
    world.serve("system.lock", "lock", move |r| {
        let _ = r.respond::<Lock>(|_| {
            let mut s = s.lock().unwrap();
            s.locks += 1;
            Ok(LockReply { locked: s.lock_ok })
        });
    });
    let s = seen.clone();
    world.serve("system.accountd", "account", move |r| {
        let _ = r.respond::<SetSession>(|c| {
            s.lock().unwrap().told.push(c.user);
            Ok(AccountReply {
                outcome: Ok(LocalUsers {
                    users: vec![],
                    credentials_available: true,
                }),
            })
        });
    });
    (world, seen)
}

/// The settings stand-in (answers `settings.get` from `Seen::settings`) and the
/// notification stand-in (records the posts).
fn serve_settings_notify(world: &World, seen: &Arc<Mutex<Seen>>) {
    let s = seen.clone();
    world.serve("system.settingsd", "settings", move |r| {
        let _ = r.respond::<Get>(|g| {
            let mut s = s.lock().unwrap();
            s.queries += 1;
            Ok(GetReply {
                value: s.settings.get(&g.key).cloned(),
            })
        });
    });
    let s = seen.clone();
    world.serve("system.notifyd", "notify", move |r| {
        let _ = r.respond::<Post>(|p| {
            s.lock().unwrap().posts.push(p);
            Ok(PostReply { id: 1 })
        });
    });
}

fn start(world: &World) -> Daemon {
    let state = world.subdir("state");
    let policy = world.policy_file();
    world.spawn(
        env!("CARGO_BIN_EXE_ultar-sessiond"),
        "system.sessiond",
        &[
            "--policy",
            policy.to_str().unwrap(),
            "--state",
            state.to_str().unwrap(),
        ],
    )
}

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(10));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

fn outcome<C: Call<Reply = ultar_bus::messages::session::SessionReply>>(
    client: &Client,
    c: &C,
) -> SessionOutcome {
    call(client, c).unwrap().outcome
}

fn signed_in(user: &str) -> SignedIn {
    SignedIn { user: user.into() }
}

#[test]
fn sign_in_switch_and_sign_out_with_the_lock_screen() {
    let (world, seen) = setup("flow");
    let mut daemon = start(&world);
    let shell = world.client("system.shell");
    let lock = world.client("system.lock");
    let state = shell.subscribe::<SessionState>().unwrap();
    assert_eq!(state.recv_timeout(WAIT).unwrap(), SessionState::default());
    // Only the sign-in screen reports sign-ins, and only of existing accounts.
    assert_eq!(outcome(&shell, &signed_in("amal")), SessionOutcome::Refused);
    assert_eq!(outcome(&lock, &signed_in("ghost")), SessionOutcome::Unknown);
    assert_eq!(outcome(&lock, &signed_in("amal")), SessionOutcome::Done);
    assert_eq!(
        state.recv_timeout(WAIT).unwrap().user.as_deref(),
        Some("amal")
    );
    // Switching locks the screen; amal stays signed in behind it.
    assert_eq!(
        outcome(
            &shell,
            &SwitchUser {
                user: "nobody".into()
            }
        ),
        SessionOutcome::Unknown
    );
    assert_eq!(
        outcome(
            &shell,
            &SwitchUser {
                user: "badr".into()
            }
        ),
        SessionOutcome::Done
    );
    let s = state.recv_timeout(WAIT).unwrap();
    assert_eq!((s.user, s.signed_in.len()), (None, 1));
    assert_eq!(outcome(&lock, &signed_in("badr")), SessionOutcome::Done);
    let users = call(&shell, &Users {}).unwrap().users;
    assert_eq!(users.len(), 2);
    assert!(users
        .iter()
        .any(|u| u.name == "badr" && u.active && u.display_name == "badr!"));
    // Signing out needs the lock screen; without it nothing changes.
    seen.lock().unwrap().lock_ok = false;
    assert_eq!(outcome(&shell, &SignOut {}), SessionOutcome::Unavailable);
    seen.lock().unwrap().lock_ok = true;
    assert_eq!(outcome(&shell, &SignOut {}), SessionOutcome::Done);
    assert!(wait_until(WAIT, || state
        .try_recv()
        .ok()
        .flatten()
        .is_some_and(|s| s.user.is_none() && s.signed_in == ["amal"])));
    assert_eq!(outcome(&shell, &SignOut {}), SessionOutcome::Invalid);
    assert!(wait_until(WAIT, || seen.lock().unwrap().told.last() == Some(&None)));
    assert!(seen.lock().unwrap().locks >= 3);
    assert_eq!(daemon.terminate(), Some(0));
}

#[test]
fn apps_are_refused_and_sessions_survive_a_crash_but_not_removed_accounts() {
    let (world, _seen) = setup("crash");
    let mut daemon = start(&world);
    let app = world.client("app.notes");
    assert_eq!(outcome(&app, &SignOut {}), SessionOutcome::Refused);
    assert!(matches!(
        app.call(&Users {}, Duration::from_secs(5)),
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    let lock = world.client("system.lock");
    assert_eq!(outcome(&lock, &signed_in("amal")), SessionOutcome::Done);
    assert_eq!(outcome(&lock, &signed_in("badr")), SessionOutcome::Done);
    daemon.crash();
    let mut again = start(&world);
    let shell = world.client("system.shell");
    let state = shell.subscribe::<SessionState>().unwrap();
    let s = state.recv_timeout(WAIT).unwrap();
    assert_eq!((s.user.as_deref(), s.signed_in.len()), (Some("badr"), 2));
    // An administrator removed badr: the session goes too.
    let accounts = world.client("system.accountd");
    accounts
        .publish(&LocalUsers {
            users: vec![],
            credentials_available: true,
        })
        .unwrap();
    assert!(wait_until(WAIT, || state
        .try_recv()
        .ok()
        .flatten()
        .is_some_and(|s| s.signed_in.is_empty() && s.user.is_none())));
    assert_eq!(again.terminate(), Some(0));
    // A damaged state file: nobody is signed in.
    std::fs::write(world.dir.join("state/sessions"), b"ULTARSTATE1\ngarbage").unwrap();
    let mut third = start(&world);
    assert!(call(&shell, &Users {}).unwrap().users.is_empty());
    assert_eq!(third.terminate(), Some(0));
}

/// Minutes since midnight of the wall clock (the service's basis).
fn now_min() -> u16 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    u16::try_from((secs % 86_400) / 60).unwrap()
}

fn hhmm(m: u16) -> String {
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// The settings service tells sessiond a new value (and answers the next read with it).
fn tell(world: &World, seen: &Arc<Mutex<Seen>>, key: &str, value: SettingValue) {
    seen.lock()
        .unwrap()
        .settings
        .insert(key.into(), value.clone());
    world
        .client("system.settingsd")
        .publish(&Changed {
            key: key.into(),
            value,
        })
        .unwrap();
}

/// One warning for `name`, with the name in its body.
fn assert_warned(seen: &Arc<Mutex<Seen>>, name: &str) {
    let s = seen.lock().unwrap();
    assert_eq!(s.posts.len(), 1, "one warning, not one per tick");
    let body = match &s.posts[0].body {
        ultar_bus::messages::notify::Body::Localized(t) => t,
        other => panic!("warning body: {other:?}"),
    };
    assert!(body
        .args
        .iter()
        .any(|a| matches!(a, ultar_bus::TextArg::Verbatim(v) if v == name)));
}

/// users.parental: a standard user signs in only in the window; an open session is
/// warned shortly before the window ends and signed out at its end (the screen locks
/// first); an administrator is never restricted.
#[test]
fn parental_window_warns_and_ends_standard_sessions() {
    let (world, seen) = setup("parental");
    let mut daemon = start(&world);
    let lock = world.client("system.lock");
    let shell = world.client("system.shell");
    let state = shell.subscribe::<SessionState>().unwrap();
    assert_eq!(state.recv_timeout(WAIT).unwrap(), SessionState::default());
    assert!(wait_until(WAIT, || seen.lock().unwrap().queries >= 3));
    // Both sign in; amal (standard) is in front.
    assert_eq!(outcome(&lock, &signed_in("badr")), SessionOutcome::Done);
    assert_eq!(outcome(&lock, &signed_in("amal")), SessionOutcome::Done);
    assert!(wait_until(WAIT, || state
        .try_recv()
        .ok()
        .flatten()
        .is_some_and(
            |s| s.user.as_deref() == Some("amal") && s.signed_in.len() == 2
        )));
    // A window that began an hour ago and ends in three minutes: the warning is due.
    let m = now_min();
    let from = SettingValue::Text(hhmm((m + 1440 - 60) % 1440));
    let until = SettingValue::Text(hhmm((m + 3) % 1440));
    tell(&world, &seen, "users.parental.from", from);
    tell(&world, &seen, "users.parental.until", until);
    tell(&world, &seen, "users.parental", SettingValue::Flag(true));
    assert!(wait_until(WAIT, || seen
        .lock()
        .unwrap()
        .posts
        .iter()
        .any(|p| p.title.key == "sessiond.parental.title")));
    assert_warned(&seen, "amal");
    // The warning alone changes no session.
    assert!(state.try_recv().ok().flatten().is_none());
    // The window ends now: amal's session ends (screen locked first), badr stays.
    let end = SettingValue::Text(hhmm(now_min()));
    tell(&world, &seen, "users.parental.until", end);
    assert!(wait_until(WAIT, || state
        .try_recv()
        .ok()
        .flatten()
        .is_some_and(|s| s.user.is_none() && s.signed_in == ["badr"])));
    assert!(
        seen.lock().unwrap().locks >= 1,
        "locked before the sign-out"
    );
    // Outside the window a standard user cannot sign in; an administrator can.
    assert_eq!(outcome(&lock, &signed_in("amal")), SessionOutcome::Refused);
    assert_eq!(outcome(&lock, &signed_in("badr")), SessionOutcome::Done);
    assert_eq!(daemon.terminate(), Some(0));
}
