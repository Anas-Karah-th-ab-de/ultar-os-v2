//! Permissions: what the profile grants at installation, what the user
//! grants and takes back, and how `ui.*` permissions reach the compositor.

mod common;

use common::{wait_state, World};
use ultar_bus::messages::launcher::{InstanceState as S, Role};
use ultar_launcher::error::LaunchError;

#[test]
fn grants_reach_the_compositor_and_a_revoke_stops_the_app() {
    let mut world = World::new("grants");
    let launcher = world.open_default();
    let extra = "[permissions]\nnetwork = \"ask\"\nui = [\"ui.capture\", \"ui.overlay\"]\n";
    let reply = launcher
        .install(&world.package("good", "1.0.0", extra, true))
        .unwrap();
    let granted: Vec<(String, bool)> = reply
        .declared
        .iter()
        .map(|p| (p.name.clone(), p.granted))
        .collect();
    assert_eq!(
        granted,
        [
            ("network".into(), true),
            ("ui.capture".into(), true),
            ("ui.overlay".into(), true)
        ]
    );
    let rights = || std::fs::read_to_string(world.path("share/compositor-apps.toml")).unwrap();
    assert!(
        rights().contains("20000 = \"ui.capture,ui.overlay\""),
        "{}",
        rights()
    );
    launcher.start("good", Role::Ui, None).unwrap();
    let revoked = launcher
        .set_permission("good", "ui.capture", false)
        .unwrap();
    assert_eq!((revoked.granted, revoked.stopped), (false, 1));
    wait_state(&launcher, "good", Role::Ui, &[S::Stopped]);
    assert!(rights().contains("20000 = \"ui.overlay\""), "{}", rights());
    assert_eq!(
        launcher.set_permission("good", "background", true),
        Err(LaunchError::NotDeclared)
    );
    assert_eq!(
        launcher.set_permission("good", "root", true),
        Err(LaunchError::NotDeclared)
    );
    launcher.uninstall("good", false).unwrap();
    assert!(!rights().contains("20000"), "{}", rights());
}

#[test]
fn store_apps_get_nothing_without_the_user() {
    let mut world = World::new("store");
    let launcher = world.open_default();
    let store = launcher
        .install(&world.package(
            "store.other",
            "1.0.0",
            "[permissions]\nnetwork = \"allowed\"\n",
            false,
        ))
        .unwrap();
    assert!(
        !store.declared[0].granted,
        "store apps get nothing without the user"
    );
    let permissions = launcher.permissions("store.other").unwrap();
    assert_eq!(
        (permissions.declared.len(), permissions.declared[0].granted),
        (1, false)
    );
    assert!(
        launcher
            .set_permission("store.other", "network", true)
            .unwrap()
            .granted
    );
    assert!(launcher.permissions("store.other").unwrap().declared[0].granted);
    assert_eq!(
        launcher.permissions("nothing"),
        Err(LaunchError::UnknownApp)
    );
}
