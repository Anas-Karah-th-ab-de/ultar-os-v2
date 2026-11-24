//! The authentication service end to end: accounts, checks, back-off across restarts,
//! damaged files (fail closed), setting secrets, the bus service with the policy, and
//! hostile file contents (fixed-seed fuzz).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use ultar_auth::hash::Cost;
use ultar_auth::server::{self, AdminList};
use ultar_auth::{store, throttle, Auth, AuthConfig, ManualClock};
use ultar_bus::messages::auth::{
    Method, Outcome, Secret, SetOutcome, SetSecret, Users, Verify, SERVICE,
};
use ultar_bus::{Broker, Client, ClientOptions, FixedIdentity, Identity, Policy};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn dir(tag: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("ultar-auth-{}-{n}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn open(dir: &std::path::Path, clock: &Arc<ManualClock>) -> Auth {
    Auth::open(AuthConfig {
        dir: dir.to_path_buf(),
        clock: clock.clone(),
        cost: Cost::TEST,
    })
}

fn secret(s: &str) -> Secret {
    Secret::new(s.to_owned())
}

fn create(auth: &mut Auth, user: &str, password: &str) {
    let request = SetSecret {
        user: user.into(),
        display_name: format!("{user} name"),
        method: Method::Password,
        current: None,
        new: secret(password),
    };
    assert_eq!(auth.set_secret(&request, true), SetOutcome::Done);
}

#[test]
fn right_password_grants_and_wrong_ones_wait_longer_each_time() {
    let d = dir("verify");
    let clock = Arc::new(ManualClock::new(1_000_000));
    let mut auth = open(&d, &clock);
    create(&mut auth, "anas", "sesam-öffne-dich");
    create(&mut auth, "leila", "zweites");
    let users = auth.users();
    assert_eq!(users.len(), 2);
    assert!(
        users[0].admin && !users[1].admin,
        "the first account administers"
    );
    assert_eq!(
        auth.verify("anas", Method::Password, &secret("sesam-öffne-dich")),
        Outcome::Granted
    );
    assert_eq!(
        auth.verify("anas", Method::Password, &secret("falsch")),
        Outcome::Denied {
            retry_after_ms: 1_000
        }
    );
    // During the wait not even the right password is checked.
    assert_eq!(
        auth.verify("anas", Method::Password, &secret("sesam-öffne-dich")),
        Outcome::Throttled {
            retry_after_ms: 1_000
        }
    );
    // Another user is not affected.
    assert_eq!(
        auth.verify("leila", Method::Password, &secret("zweites")),
        Outcome::Granted
    );
    for wait in [1_000, 2_000] {
        clock.advance(wait);
        let denied = Outcome::Denied {
            retry_after_ms: 2 * wait,
        };
        assert_eq!(
            auth.verify("anas", Method::Password, &secret("falsch")),
            denied
        );
    }
    clock.advance(4_000);
    assert_eq!(
        auth.verify("anas", Method::Password, &secret("sesam-öffne-dich")),
        Outcome::Granted
    );
    assert_eq!(
        auth.verify("anas", Method::Password, &secret("falsch")),
        Outcome::Denied {
            retry_after_ms: 1_000
        },
        "a success starts counting again"
    );
}

#[test]
fn unknown_users_and_missing_pins_look_like_wrong_secrets() {
    let d = dir("unknown");
    let clock = Arc::new(ManualClock::new(5));
    let mut auth = open(&d, &clock);
    create(&mut auth, "anas", "geheim1");
    assert_eq!(
        auth.verify("nobody", Method::Password, &secret("x")),
        Outcome::Denied {
            retry_after_ms: 1_000
        }
    );
    assert!(
        matches!(
            auth.verify("other", Method::Password, &secret("x")),
            Outcome::Throttled { .. }
        ),
        "unknown names share one back-off"
    );
    assert_eq!(
        auth.verify("anas", Method::Pin, &secret("1234")),
        Outcome::Denied {
            retry_after_ms: 1_000
        }
    );
    let debug = format!("{auth:?}");
    assert!(!debug.contains("argon2"), "{debug}");
}

#[test]
fn the_back_off_survives_a_restart_and_a_damaged_counter_file() {
    let d = dir("restart");
    let clock = Arc::new(ManualClock::new(10_000));
    let mut auth = open(&d, &clock);
    create(&mut auth, "anas", "geheim1");
    for _ in 0..5 {
        let _ = auth.verify("anas", Method::Password, &secret("x"));
        clock.advance(throttle::MAX_WAIT_MS);
    }
    let _ = auth.verify("anas", Method::Password, &secret("x"));
    drop(auth);
    let mut again = open(&d, &clock);
    assert!(matches!(
        again.verify("anas", Method::Password, &secret("geheim1")),
        Outcome::Throttled { retry_after_ms } if retry_after_ms == 32_000
    ));
    drop(again);
    std::fs::write(d.join(throttle::FILE), b"garbage").unwrap();
    let mut damaged = open(&d, &clock);
    assert_eq!(
        damaged.verify("anas", Method::Password, &secret("geheim1")),
        Outcome::Throttled {
            retry_after_ms: throttle::MAX_WAIT_MS
        },
        "damaging the counters does not reset them"
    );
}

#[test]
fn a_damaged_account_file_lets_nobody_in() {
    let d = dir("damaged");
    let clock = Arc::new(ManualClock::new(0));
    let mut auth = open(&d, &clock);
    create(&mut auth, "anas", "geheim1");
    let path = store::path(&d);
    let mut bytes = std::fs::read(&path).unwrap();
    let n = bytes.len() - 3;
    bytes[n] ^= 0x20;
    std::fs::write(&path, bytes).unwrap();
    let mut broken = open(&d, &clock);
    assert!(!broken.usable());
    assert!(broken.users().is_empty());
    assert_eq!(
        broken.verify("anas", Method::Password, &secret("geheim1")),
        Outcome::Unavailable
    );
    let request = SetSecret {
        user: "anas".into(),
        display_name: "A".into(),
        method: Method::Password,
        current: None,
        new: secret("neu-geheim"),
    };
    assert_eq!(broken.set_secret(&request, true), SetOutcome::Unavailable);
}

/// Bad user names and new accounts without a password or a display name are refused.
fn refuses_bad_accounts(auth: &mut Auth, pin: &SetSecret) {
    let bad_name = SetSecret {
        user: "Bad Name".into(),
        method: Method::Password,
        new: secret("long-enough"),
        ..pin.clone()
    };
    assert_eq!(auth.set_secret(&bad_name, true), SetOutcome::Rejected);
    let new_with_pin = SetSecret {
        user: "neu".into(),
        display_name: "Neu".into(),
        ..pin.clone()
    };
    assert_eq!(
        auth.set_secret(&new_with_pin, true),
        SetOutcome::Rejected,
        "a new account starts with a password"
    );
    let no_display = SetSecret {
        user: "neu".into(),
        display_name: " ".into(),
        method: Method::Password,
        new: secret("long-enough"),
        current: None,
    };
    assert_eq!(auth.set_secret(&no_display, true), SetOutcome::Rejected);
}

#[test]
fn setting_secrets_needs_the_current_password_or_an_administrator() {
    let d = dir("set");
    let clock = Arc::new(ManualClock::new(0));
    let mut auth = open(&d, &clock);
    create(&mut auth, "anas", "geheim1");
    let mut pin = SetSecret {
        user: "anas".into(),
        display_name: String::new(),
        method: Method::Pin,
        current: Some(secret("wrong")),
        new: secret("2468"),
    };
    assert_eq!(
        auth.set_secret(&pin, false),
        SetOutcome::Denied {
            retry_after_ms: 1_000
        }
    );
    clock.advance(1_000);
    pin.current = Some(secret("geheim1"));
    assert_eq!(auth.set_secret(&pin, false), SetOutcome::Done);
    assert!(auth.users()[0].has_pin);
    assert_eq!(
        auth.verify("anas", Method::Pin, &secret("2468")),
        Outcome::Granted
    );
    for (method, new) in [
        (Method::Pin, "12a4"),
        (Method::Pin, "123"),
        (Method::Pin, "1234567890123"),
        (Method::Password, "abc"),
    ] {
        let bad = SetSecret {
            method,
            new: secret(new),
            ..pin.clone()
        };
        assert_eq!(auth.set_secret(&bad, true), SetOutcome::Rejected, "{new}");
    }
    refuses_bad_accounts(&mut auth, &pin);
    // The file holds hashes only.
    let file = std::fs::read_to_string(store::path(&d)).unwrap();
    assert!(!file.contains("geheim1") && !file.contains("2468") && file.contains("$argon2id$"));
}

fn broker(tag: &str, policy: Policy, identity: &str) -> (Broker, PathBuf) {
    let path = std::env::temp_dir().join(format!("ultar-auth-{}-{tag}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let auth = Arc::new(FixedIdentity::new(Identity::new(identity).unwrap()));
    let broker = Broker::builder(policy).listen(&path, auth).start().unwrap();
    (broker, path)
}

#[test]
fn the_bus_service_answers_with_the_policy() {
    let policy = Policy::from_toml_str(
        "[[rule]]\nidentity = \"ui.lock\"\nregister = [\"auth\"]\ncall = [\"auth.users\", \"auth.verify\", \"auth.set_secret\", \"auth.admin\"]\n",
    )
    .unwrap();
    let (_broker, path) = broker("bus", policy.clone(), "ui.lock");
    let d = dir("bus");
    let clock = Arc::new(ManualClock::new(0));
    let mut auth = open(&d, &clock);
    create(&mut auth, "anas", "geheim1");
    let service = Client::connect(&path, ClientOptions::named("authd")).unwrap();
    let endpoint = service.register(SERVICE).unwrap();
    let caller = Client::connect(&path, ClientOptions::named("lock")).unwrap();
    let worker = std::thread::spawn(move || {
        for _ in 0..2 {
            let request = endpoint.recv_timeout(Duration::from_secs(10)).unwrap();
            server::handle(&mut auth, &policy, request).unwrap();
        }
    });
    let users = caller.call(&Users {}, Duration::from_secs(10)).unwrap();
    assert_eq!(users.users[0].name, "anas");
    let reply = caller
        .call(
            &Verify {
                user: "anas".into(),
                method: Method::Password,
                secret: secret("geheim1"),
            },
            Duration::from_secs(10),
        )
        .unwrap();
    assert_eq!(reply.outcome, Outcome::Granted);
    worker.join().unwrap();
    use ultar_auth::server::AdminCheck;
    assert!(
        Policy::from_toml_str("[[rule]]\nidentity = \"ui.lock\"\ncall = [\"auth.admin\"]\n")
            .unwrap()
            .is_admin("ui.lock")
    );
    assert!(!Policy::allow_all().is_admin("bad name with spaces"));
    assert!(AdminList(vec!["app.installer".into()]).is_admin("app.installer"));
    assert!(!AdminList::default().is_admin("app.installer"));
}

/// Fixed-seed fuzz: random and mutated file contents never panic and never yield an
/// account that was not written.
#[test]
fn hostile_file_contents_are_refused_not_trusted() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let good = "user anas admin=1 display=616e6173 password=$argon2id$v=19$m=64,t=1,p=1$c2FsdA$aGFzaA pin=-\n";
    for _ in 0..3000 {
        let mut bytes = good.as_bytes().to_vec();
        for _ in 0..(next() % 4 + 1) {
            let i = (next() as usize) % bytes.len();
            bytes[i] = (next() & 0xff) as u8;
        }
        if let Ok(text) = String::from_utf8(bytes) {
            if let Ok(list) = store::decode(&text) {
                assert!(list.len() <= 1);
            }
            let _ = throttle::Throttle::decode(&text);
        }
        let random: String = (0..(next() % 200))
            .map(|_| char::from((next() % 96 + 32) as u8))
            .collect();
        assert!(store::decode(&random).is_err() || random.is_empty());
        let _ = throttle::Throttle::decode(&random);
    }
}

/// Security review S-6: after `PIN_LOCKOUT` wrong tries in a row the PIN is no longer
/// checked (even the right one is "wrong"); the password still works and clears the lock.
#[test]
fn a_pin_is_locked_after_too_many_failures_until_the_password_is_used() {
    let d = dir("pinlock");
    let clock = Arc::new(ManualClock::new(0));
    let mut auth = open(&d, &clock);
    create(&mut auth, "anas", "sesam-öffne-dich");
    let pin = SetSecret {
        user: "anas".into(),
        display_name: "anas name".into(),
        method: Method::Pin,
        current: Some(secret("sesam-öffne-dich")),
        new: secret("4711"),
    };
    assert_eq!(auth.set_secret(&pin, false), SetOutcome::Done);
    for _ in 0..throttle::PIN_LOCKOUT {
        clock.advance(throttle::MAX_WAIT_MS);
        assert!(matches!(
            auth.verify("anas", Method::Pin, &secret("0000")),
            Outcome::Denied { .. }
        ));
    }
    clock.advance(throttle::MAX_WAIT_MS);
    assert!(
        matches!(
            auth.verify("anas", Method::Pin, &secret("4711")),
            Outcome::Denied { .. }
        ),
        "the right PIN after the lockout"
    );
    clock.advance(throttle::MAX_WAIT_MS);
    assert_eq!(
        auth.verify("anas", Method::Password, &secret("sesam-öffne-dich")),
        Outcome::Granted
    );
    assert_eq!(
        auth.verify("anas", Method::Pin, &secret("4711")),
        Outcome::Granted
    );
}

/// Security review S-6: an unreadable counter file (not only a damaged one) holds every
/// user back instead of starting without a back-off.
#[test]
fn an_unreadable_counter_file_holds_everyone_back() {
    let d = dir("unreadable");
    let clock = Arc::new(ManualClock::new(0));
    let mut auth = open(&d, &clock);
    create(&mut auth, "anas", "sesam-öffne-dich");
    drop(auth);
    // a directory where the file goes: reading it fails with an I/O error
    std::fs::create_dir(d.join(throttle::FILE)).unwrap();
    let mut auth = open(&d, &clock);
    assert!(matches!(
        auth.verify("anas", Method::Password, &secret("sesam-öffne-dich")),
        Outcome::Throttled { .. }
    ));
}
