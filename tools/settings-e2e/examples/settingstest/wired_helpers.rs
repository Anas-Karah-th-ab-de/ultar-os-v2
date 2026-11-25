//! The keyboard-helpers check of the e2e run (split from `wired.rs` for size): the four
//! helpers of the accessibility page and the compose key of the keyboard page reach the
//! compositor through the display service's own code (`Request::ConfigureKeyHelpers`).

use super::{change, choose, page, Display, Follower};
use crate::setup::Run;
use ultar_bus::Client;
use ultar_displayd::input::InputSettings;
use ultar_surface::proto::{ComposeKey, Request};

/// Accessibility and keyboard pages: the four keyboard helpers and the compose key reach the
/// compositor through the display service. Restores them afterwards.
pub(super) fn key_helpers(
    bus: &Client,
    admin: &Client,
    run: &Run,
    displayd: &Follower,
) -> Result<(), String> {
    let mut d = Display {
        told: displayd,
        service: InputSettings::default(),
    };
    let keys = [
        "accessibility.sticky_keys",
        "accessibility.slow_keys",
        "accessibility.bounce_keys",
        "accessibility.mouse_keys",
    ];
    let mut app = page(bus, run, "accessibility")?;
    for key in keys {
        let on = change((&mut app, admin), ("accessibility", key), None)?;
        d.follow(key, on)?;
    }
    // The compose key is a choice on the keyboard page: none -> alt_gr -> menu (two steps).
    let mut kbd = page(bus, run, "keyboard")?;
    let compose = choose((&mut kbd, admin), ("keyboard", "keyboard.compose_key"), 2)?;
    d.follow("keyboard.compose_key", compose)?;
    let Request::ConfigureKeyHelpers {
        sticky_keys,
        slow_keys,
        bounce_keys,
        mouse_keys,
        compose_key,
    } = d.service.requests()[2].clone()
    else {
        return Err("no keyboard-helpers configuration".into());
    };
    if !(sticky_keys && slow_keys && bounce_keys && mouse_keys) || compose_key != ComposeKey::Menu {
        return Err(format!(
            "the helpers became {sticky_keys} {slow_keys} {bounce_keys} {mouse_keys} {compose_key:?}"
        ));
    }
    // Back to the defaults for whoever runs after this (the menu wraps: menu -> caps_lock -> none).
    for key in keys {
        change((&mut app, admin), ("accessibility", key), None)?;
    }
    choose((&mut kbd, admin), ("keyboard", "keyboard.compose_key"), 2)?;
    println!(
        "SETTINGS: accessibility and keyboard pages -> compositor: sticky, slow, bounce and mouse \
         keys on, compose key menu"
    );
    Ok(())
}
