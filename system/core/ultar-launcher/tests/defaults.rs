//! Default apps and autostart: the launcher resolves a URI through `apps.default.*`
//! (read at start, followed at runtime), lists the installed apps that declare a kind,
//! and hands out the autostart list once after a sign-in. No cell is started here.

mod common;

use common::World;
use ultar_bus::messages::launcher::Kind;
use ultar_bus::messages::session::SessionState;
use ultar_bus::messages::settings::SettingValue;

const WEB: &str = "[open]\nschemes = [\"http\", \"https\"]\n";
const MAIL: &str = "[open]\nschemes = [\"mailto\"]\n";
const FILE: &str = "[open]\nschemes = [\"file\"]\n";

fn text(value: &str) -> SettingValue {
    SettingValue::Text(value.to_owned())
}

fn signed_in() -> SessionState {
    SessionState {
        user: Some("amal".into()),
        signed_in: vec!["amal".into()],
    }
}

#[test]
fn resolve_follows_the_default_apps_and_falls_back() {
    let mut world = World::new("defaults-resolve");
    let launcher = world.open_default();
    for (id, open, system) in [
        ("browser", WEB, true),
        ("mail", MAIL, true),
        ("media", FILE, true),
        ("images", FILE, true),
        ("store.web", WEB, false),
    ] {
        launcher
            .install(&world.package_args(id, &["info"], open, system))
            .unwrap();
    }
    // the schema's defaults resolve to the installed system apps
    assert_eq!(
        launcher.resolve("https://example.org/a").as_deref(),
        Some("browser")
    );
    assert_eq!(
        launcher.resolve("mailto:a@example.org").as_deref(),
        Some("mail")
    );
    assert_eq!(
        launcher.resolve("file:///m/u/song.mp3").as_deref(),
        Some("media")
    );
    assert_eq!(
        launcher.resolve("file:///m/u/pic.png").as_deref(),
        Some("images")
    );
    // a runtime change reaches the resolution
    launcher.follow_setting("apps.default.web", &text("store.web"));
    assert_eq!(
        launcher.resolve("https://example.org").as_deref(),
        Some("store.web")
    );
    // an invalid value falls back to the system default
    launcher.follow_setting("apps.default.web", &text("Not An Id!"));
    assert_eq!(
        launcher.resolve("https://example.org").as_deref(),
        Some("browser")
    );
    // a valid id that is not installed falls back too
    launcher.follow_setting("apps.default.web", &text("ghost"));
    assert_eq!(
        launcher.resolve("https://example.org").as_deref(),
        Some("browser")
    );
    // a URI no installed app declares resolves to nothing
    assert_eq!(launcher.resolve("file:///m/u/folder"), None);
    assert_eq!(launcher.resolve("trash:///"), None);
    assert_eq!(launcher.resolve("https://example.org/\n"), None);
    world.close();
}

#[test]
fn candidates_are_the_installed_apps_that_declare_the_kind() {
    let mut world = World::new("defaults-candidates");
    let launcher = world.open_default();
    for (id, open, system) in [
        ("browser", WEB, true),
        ("media", FILE, true),
        ("images", FILE, true),
        ("store.web", WEB, false),
    ] {
        launcher
            .install(&world.package_args(id, &["info"], open, system))
            .unwrap();
    }
    assert_eq!(launcher.candidates(Kind::Web), ["browser", "store.web"]);
    assert_eq!(launcher.candidates(Kind::Photos), ["images", "media"]);
    assert_eq!(launcher.candidates(Kind::Music), ["images", "media"]);
    assert!(launcher.candidates(Kind::Mail).is_empty());
    // an uninstalled app is no candidate
    launcher.uninstall("store.web", false).unwrap();
    assert_eq!(launcher.candidates(Kind::Web), ["browser"]);
    world.close();
}

#[test]
fn autostart_hands_out_the_list_once_after_sign_in() {
    let mut world = World::new("defaults-autostart");
    let launcher = world.open_default();
    launcher.follow_setting(
        "apps.autostart",
        &SettingValue::List(vec!["browser".into(), "media".into(), "Bad Id".into()]),
    );
    assert_eq!(launcher.autostart_due(&signed_in()), ["browser", "media"]);
    assert!(launcher.autostart_due(&signed_in()).is_empty());
    assert!(launcher.autostart_due(&SessionState::default()).is_empty());
    assert_eq!(launcher.autostart_due(&signed_in()), ["browser", "media"]);
    world.close();
}
