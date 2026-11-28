//! Tests of the state file and the id allocation.

use super::*;
use crate::perm::Permission;

const RANGE: UidRange = UidRange {
    first: 20000,
    last: 20002,
};

fn id(text: &str) -> AppId {
    AppId::new(text).unwrap()
}

fn entry(uid: u32, installed: bool) -> Entry {
    Entry {
        uid,
        profile: ProfileName::System,
        installed,
        version: "1.4.0".into(),
        digest: digest(b"m"),
        granted: PermissionSet::of(&[Permission::Network, Permission::UiCapture]),
        signer: "0123456789abcdef".into(),
    }
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ulaunch-store-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn ids_are_stable_never_reused_and_bounded() {
    let mut state = State::new(RANGE);
    let a = state.uid_for(&id("a")).unwrap();
    state.put(id("a"), entry(a, true));
    let b = state.uid_for(&id("b")).unwrap();
    state.put(id("b"), entry(b, true));
    assert_eq!((a, b), (20000, 20001));
    // uninstalled: the id stays with the app, a reinstall gets it back
    state.get_mut(&id("a")).unwrap().installed = false;
    assert_eq!(state.uid_for(&id("a")), Ok(20000));
    let c = state.uid_for(&id("c")).unwrap();
    assert_eq!(c, 20002);
    state.put(id("c"), entry(c, true));
    assert_eq!(state.uid_for(&id("d")), Err(StateError::Exhausted));
    assert_eq!(state.uid_for(&id("c")), Ok(20002));
}

#[test]
fn the_state_file_round_trips() {
    let mut state = State::new(UidRange {
        first: 20000,
        last: 49999,
    });
    for (n, name) in ["camera", "km.notes", "x"].iter().enumerate() {
        let uid = state.uid_for(&id(name)).unwrap();
        let mut e = entry(uid, n != 1);
        if n == 2 {
            e.granted = PermissionSet::EMPTY;
            e.profile = ProfileName::Standard;
        }
        state.put(id(name), e);
    }
    let text = state.render();
    assert!(text.contains("app x 20002 app.standard 1 1.4.0"));
    assert!(text.ends_with(" - 0123456789abcdef\n"));
    let back = State::parse(&text, state.range()).unwrap();
    assert_eq!(back, state);
    assert_eq!(back.apps().count(), 3);
    assert_eq!(State::parse(&text, RANGE), Err(StateError::Range));
}

#[test]
fn damaged_state_is_detected() {
    let mut state = State::new(UidRange {
        first: 20000,
        last: 49999,
    });
    let uid = state.uid_for(&id("a")).unwrap();
    state.put(id("a"), entry(uid, true));
    let good = state.render();
    let range = state.range();
    let cases = [
        good.replace(MAGIC, "ultar-launcher-state 1"),
        good.replace("next 20001", "next 20000"),
        good.replace("next 20001", "next nineteen"),
        good.replace("next 20001", "next 50001"),
        good.replace("app a ", "app A "),
        good.replace("app a ", "ap a "),
        good.replace(" app.system ", " app.root "),
        good.replace(" 1 1.4.0 ", " 2 1.4.0 "),
        good.replace(" 1.4.0 ", " 1 4 "),
        good.replace("network,ui.capture", "network,sudo"),
        good.replace(&digest(b"m"), "xyz"),
        good.replace(&digest(b"m"), &digest(b"m").to_uppercase()),
        good.replace("0123456789abcdef", "0123456789ABCDEF"),
        good.replace(" 0123456789abcdef", ""),
        format!("{good}{}", good.lines().last().unwrap()),
        good.replace("app a 20000", "app a 19999"),
        format!(
            "{good}app b 20000 app.system 1 1 {} - 0123456789abcdef\n",
            digest(b"m")
        ),
        format!("{good}\n"),
        String::new(),
    ];
    for (n, text) in cases.iter().enumerate() {
        let parsed = State::parse(text, range);
        assert!(
            matches!(parsed, Err(StateError::Corrupt(_)) | Err(StateError::Range)),
            "case {n}: {parsed:?}"
        );
    }
    // fuzz with a fixed seed: never a panic
    let mut seed: u64 = 77;
    for _ in 0..2000 {
        let mut bytes = good.clone().into_bytes();
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let at = (seed >> 33) as usize % bytes.len();
        bytes[at] = (seed >> 13) as u8;
        if let Ok(text) = std::str::from_utf8(&bytes) {
            let _ = State::parse(text, range);
        }
    }
}

#[test]
fn the_store_persists_state_and_manifests() {
    let dir = scratch("s");
    let range = UidRange {
        first: 20000,
        last: 49999,
    };
    let mut store = Store::open(&dir, range).unwrap();
    let uid = store.state.uid_for(&id("camera")).unwrap();
    let digest = store.save_manifest(&id("camera"), b"manifest").unwrap();
    store.state.put(
        id("camera"),
        Entry {
            digest,
            ..entry(uid, true)
        },
    );
    store.save().unwrap();
    let again = Store::open(&dir, range).unwrap();
    assert_eq!(again.state, store.state);
    assert_eq!(again.load_manifest(&id("camera")).unwrap(), b"manifest");
    // a changed manifest copy is detected
    std::fs::write(dir.join("manifests/camera.toml"), b"evil").unwrap();
    assert_eq!(again.load_manifest(&id("camera")), Err(StateError::Digest));
    assert_eq!(again.load_manifest(&id("other")), Err(StateError::Digest));
    again.remove_manifest(&id("camera")).unwrap();
    again.remove_manifest(&id("camera")).unwrap();
    // another range or a damaged file: refused, not reset
    assert_eq!(Store::open(&dir, RANGE).err(), Some(StateError::Range));
    std::fs::write(dir.join(STATE_FILE), b"garbage").unwrap();
    assert!(matches!(
        Store::open(&dir, range),
        Err(StateError::Corrupt(_))
    ));
    assert!(StateError::Digest
        .to_string()
        .starts_with("launcher.state."));
    let _ = std::fs::remove_dir_all(&dir);
}

/// R2-2: a file of format 2 (short signer ids) is read and written as format 3; full
/// signer identities round-trip.
#[test]
fn format_2_is_read_and_full_signers_round_trip() {
    let range = UidRange {
        first: 20000,
        last: 20009,
    };
    let old = format!(
        "ultar-launcher-state 2\nrange 20000 20009\nnext 20001\napp a 20000 app.system 1 1 {} - 0123456789abcdef\n",
        digest(b"m")
    );
    let state = State::parse(&old, range).unwrap();
    let text = state.render();
    assert!(text.starts_with("ultar-launcher-state 3\n"));
    assert_eq!(State::parse(&text, range).unwrap(), state);
    let full = "0123456789abcdef".repeat(4);
    let new = text.replace(" 0123456789abcdef\n", &format!(" {full}\n"));
    assert_eq!(State::parse(&new, range).unwrap().render(), new);
    let bad = text.replace(" 0123456789abcdef\n", " 0123456789abcdef0\n");
    assert!(State::parse(&bad, range).is_err());
}
