//! The desktop shell's request: a start may carry a URI (handed to a fresh
//! instance as data) and ask for another window; both are checked strictly.

mod common;

use common::{wait_state, Collector, World};
use ultar_bus::messages::launcher::{InstanceState as S, Role, MAX_URI};
use ultar_launcher::error::LaunchError;
use ultar_launcher::open::Opening;

const SCHEMES: &str = "[open]\nschemes = [\"trash\", \"settings\"]\n";

fn uri(text: &str) -> Opening {
    Opening {
        token: Some(4),
        uri: Some(text.to_owned()),
        new_window: false,
    }
}

fn new_window() -> Opening {
    Opening {
        new_window: true,
        ..Opening::default()
    }
}

#[test]
fn a_fresh_instance_gets_the_uri_as_data() {
    let mut world = World::new("open-uri");
    let collector = Collector::listen(&world.path("out.sock"));
    let launcher = world.open_default();
    launcher
        .install(&world.package_args("viewer", &["info"], SCHEMES, true))
        .unwrap();
    // spaces, quotes and `$` stay data: no shell, no command line
    let text = "settings://network?name=a b'$(true)";
    let reply = launcher
        .start_opening("viewer", Role::Ui, &uri(text))
        .unwrap();
    assert!(reply.opened && !reply.already, "{reply:?}");
    let line = collector
        .wait_for("app-viewer", "TESTAPP uid=")
        .expect("no output line");
    assert!(line.contains(&format!("uri=[{text}]")), "{line}");
    assert!(line.contains("token=4"), "{line}");
    wait_state(&launcher, "viewer", Role::Ui, &[S::Stopped, S::Crashed]);
    // a start without a URI hands none
    let reply = launcher.start("viewer", Role::Ui, None).unwrap();
    assert!(!reply.opened);
}

#[test]
fn uris_are_checked_before_anything_starts() {
    let mut world = World::new("open-refused");
    let launcher = world.open_default();
    launcher
        .install(&world.package("viewer", "1.0.0", SCHEMES, true))
        .unwrap();
    launcher
        .install(&world.package("plain", "1.0.0", "", true))
        .unwrap();
    let long = format!("trash:///{}", "a".repeat(MAX_URI));
    let refused = [
        ("viewer", Role::Ui, "http://example.org/".to_owned()),
        ("viewer", Role::Ui, "TRASH:///".to_owned()),
        ("viewer", Role::Ui, "trash:///a\nb".to_owned()),
        ("viewer", Role::Ui, "trash:///\u{1b}[2J".to_owned()),
        ("viewer", Role::Ui, "trash:///\0".to_owned()),
        ("viewer", Role::Ui, "trash:///\u{7f}".to_owned()),
        ("viewer", Role::Ui, "trash:///\u{85}".to_owned()),
        ("viewer", Role::Ui, String::new()),
        ("viewer", Role::Ui, "no-scheme".to_owned()),
        ("viewer", Role::Ui, long),
        ("viewer", Role::Service, "trash:///".to_owned()),
        ("plain", Role::Ui, "trash:///".to_owned()),
    ];
    for (app, role, text) in refused {
        assert!(
            matches!(
                launcher.start_opening(app, role, &uri(&text)),
                Err(LaunchError::Invalid)
            ),
            "{app} {text:?}"
        );
    }
    assert!(
        launcher.list().iter().all(|a| a.instances.is_empty()),
        "nothing was started"
    );
    assert!(matches!(
        launcher.start_opening("nothing", Role::Ui, &uri("trash:///")),
        Err(LaunchError::UnknownApp)
    ));
    let longest = format!("trash:///{}", "a".repeat(MAX_URI - "trash:///".len()));
    let reply = launcher
        .start_opening("viewer", Role::Ui, &uri(&longest))
        .unwrap();
    assert!(reply.opened);
    launcher.stop("viewer", Role::Ui).unwrap();
    wait_state(&launcher, "viewer", Role::Ui, &[S::Stopped]);
}

#[test]
fn a_running_instance_is_raised_or_the_open_is_refused() {
    let mut world = World::new("open-running");
    let launcher = world.open_default();
    launcher
        .install(&world.package("single", "1.0.0", SCHEMES, true))
        .unwrap();
    let multi = format!("{SCHEMES}multi_window = true\n");
    launcher
        .install(&world.package("multi", "1.0.0", &multi, true))
        .unwrap();
    for app in ["single", "multi"] {
        let first = launcher.start(app, Role::Ui, None).unwrap();
        assert!(!first.already && !first.opened);
    }
    // one window only: the running one is raised
    let raised = launcher
        .start_opening("single", Role::Ui, &new_window())
        .unwrap();
    assert!(raised.already && !raised.opened, "{raised:?}");
    // a URI or a second window would have to reach the running app
    for (app, opening) in [
        ("single", uri("trash:///")),
        ("multi", uri("settings://x")),
        ("multi", new_window()),
    ] {
        assert!(
            matches!(
                launcher.start_opening(app, Role::Ui, &opening),
                Err(LaunchError::NoOpen)
            ),
            "{app} {opening:?}"
        );
    }
    // the check comes first: an undeclared scheme stays invalid
    assert!(matches!(
        launcher.start_opening("single", Role::Ui, &uri("http://x")),
        Err(LaunchError::Invalid)
    ));
    assert_eq!(LaunchError::NoOpen.code(), 12);
    for app in ["single", "multi"] {
        launcher.stop(app, Role::Ui).unwrap();
        wait_state(&launcher, app, Role::Ui, &[S::Stopped]);
    }
}
