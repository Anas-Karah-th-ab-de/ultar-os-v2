//! Reviewer tests, round 2 (`launcher-runde-2.md`): attempts on the
//! changed code of round 1 – the publisher prefix, the signer, the opened
//! URI, the revoke when the state cannot be saved, and code run from the
//! private `/tmp` of a **store** app (round 1's test now uses a system app).

mod common;

use common::{root, wait_state, Collector, World};
use ultar_bus::messages::launcher::{InstanceState as S, Role};
use ultar_launcher::error::{LaunchError, PackageError};
use ultar_launcher::open::{check, UriError};

fn refused(result: Result<ultar_bus::messages::launcher::InstallReply, LaunchError>) -> bool {
    matches!(result, Err(LaunchError::Package(_)))
}

/// Ids that only look like the store key's prefix: case, Unicode
/// look-alikes, extra dots, an empty app part, a trailing dot. None may be
/// installed by the `store` key, and none becomes another app's id.
#[test]
fn the_publisher_prefix_cannot_be_dressed_up() {
    let mut world = World::new("r2prefix");
    let launcher = world.open_default();
    for id in [
        "Store.game",
        "STORE.game",
        "\u{0455}tore.game", // Cyrillic dze
        "store\u{200b}.game",
        "store..game",
        "store.",
        ".store",
        "store.game.x",
        "store.game/../camera",
        "store-x.game",
        "stor.game",
        "store",
    ] {
        let text = World::manifest_text(id, "1.0.0", "");
        let package = world.sign(&format!("p{}", id.len()), &text, false);
        assert!(refused(launcher.install(&package)), "{id:?} was accepted");
    }
    // a store key whose file name is not a valid publisher can publish nothing
    for name in ["Acme", "ac_me", "x--y"] {
        let key = world.another_key("keys", name);
        let lower = name.to_ascii_lowercase();
        let text = World::manifest_text(&format!("{lower}.game"), "1.0.0", "");
        let package = world.sign_with(&format!("k{name}"), &text, &key);
        assert!(refused(launcher.install(&package)), "key {name:?}");
    }
}

/// A second key file with the same publisher name (`store.old.ed25519` →
/// `store`) passes the prefix test but is another signer: it cannot update
/// or take over the first key's app, installed or uninstalled with kept data.
#[test]
fn a_second_key_of_the_same_publisher_name_is_another_signer() {
    let mut world = World::new("r2samename");
    let launcher = world.open_default();
    let first = launcher
        .install(&world.package("store.game", "1.0.0", "", false))
        .unwrap();
    let twin = world.another_key("keys", "store.old");
    let text = World::manifest_text("store.game", "2.0.0", "");
    let package = world.sign_with("twin", &text, &twin);
    assert_eq!(
        launcher.install(&package),
        Err(LaunchError::Package(PackageError::Signer))
    );
    launcher.uninstall("store.game", true).unwrap();
    assert_eq!(
        launcher.install(&package),
        Err(LaunchError::Package(PackageError::Signer))
    );
    // the right signer gets its id back, never downwards
    let back = launcher
        .install(&world.package("store.game", "1.0.0", "", false))
        .unwrap();
    assert_eq!(back.uid, first.uid);
    assert_eq!(
        launcher.install(&world.package("store.game", "0.9.0", "", false)),
        Err(LaunchError::Package(PackageError::Downgrade))
    );
}

/// The opened URI: control characters of every kind, a scheme that only
/// differs in case or by a look-alike, a scheme hidden behind a slash.
#[test]
fn the_uri_check_has_no_way_around() {
    let schemes = vec!["settings".to_owned()];
    assert_eq!(check("settings://network", &schemes), Ok(()));
    for (uri, why) in [
        ("settings://a\nb", UriError::Control),
        ("settings://a\u{0}", UriError::Control),
        ("settings://a\u{7f}", UriError::Control),
        ("settings://a\u{85}", UriError::Control),
        ("Settings://network", UriError::Scheme),
        ("\u{0455}ettings://x", UriError::Scheme),
        ("/settings:x", UriError::Scheme),
        ("settings", UriError::Scheme),
        ("file:///etc/shadow", UriError::Undeclared),
        ("", UriError::Length),
    ] {
        assert_eq!(check(uri, &schemes), Err(why), "{uri:?}");
    }
    let long = format!("settings:{}", "a".repeat(2048));
    assert_eq!(check(&long, &schemes), Err(UriError::Length));
}

/// Fail closed also when the **state** cannot be saved: the revoke is
/// reported as failed, the app is stopped and the compositor's file no
/// longer lists the right (before the fix the file kept it: the save
/// failed first and the file was not touched).
#[test]
fn a_revoke_whose_state_cannot_be_saved_takes_the_right_off_file() {
    let mut world = World::new("r2revoke");
    let launcher = world.open_default();
    let extra = "[permissions]\nui = [\"ui.capture\"]\n";
    launcher
        .install(&world.package("good", "1.0.0", extra, true))
        .unwrap();
    let file = world.path("share/compositor-apps.toml");
    assert!(std::fs::read_to_string(&file)
        .unwrap()
        .contains("ui.capture"));
    launcher.start("good", Role::Ui, None).unwrap();
    // the state's temporary name is taken by a directory: the save fails
    let blocker = world.path("state/.apps.state.new");
    std::fs::create_dir(&blocker).unwrap();
    std::fs::write(blocker.join("x"), b"x").unwrap();
    let revoked = launcher.set_permission("good", "ui.capture", false);
    assert!(revoked.is_err(), "{revoked:?}");
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(
        !text.contains("ui.capture"),
        "the right is still on file after a failed revoke: {text:?}"
    );
    wait_state(&launcher, "good", Role::Ui, &[S::Stopped]);
    // a grant whose state cannot be saved is taken back, also on file
    assert!(launcher.set_permission("good", "ui.capture", true).is_err());
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(!text.contains("ui.capture"), "{text:?}");
}

/// Round 1's L-2 test with a **store** app (its id carries the publisher):
/// neither `/data` nor `/tmp` runs what the app wrote.
#[test]
fn a_store_app_cannot_execute_what_it_wrote() {
    if !root() {
        eprintln!("skipped: needs real cells (root)");
        return;
    }
    let mut world = World::new("r2tmpexec");
    let collector = Collector::listen(&world.path("out.sock"));
    let launcher = world.open_default();
    let text = "id = \"store.evil\"\nversion = \"1.0.0\"\nkind = \"native\"\nname = \"app.evil.name\"\n\
                exec = \"/bin/sh\"\nargs = [\"-c\", \"cp /bin/true /data/t 2>/dev/null; /data/t && echo DATA-EXEC-OK; \
                cp /bin/true /tmp/t && /tmp/t && echo TMP-EXEC-OK; echo DONE\"]\n";
    launcher
        .install(&world.sign("store-evil", text, false))
        .unwrap();
    launcher.start("store.evil", Role::Ui, None).unwrap();
    wait_state(&launcher, "store.evil", Role::Ui, &[S::Stopped, S::Crashed]);
    assert!(collector.wait_for("app-store_evil", "DONE").is_some());
    let lines = collector.of("app-store_evil");
    assert!(
        !lines
            .iter()
            .any(|l| l.contains("DATA-EXEC-OK") || l.contains("TMP-EXEC-OK")),
        "{lines:?}"
    );
}
