//! The accessibility view page, changed through its rows: the magnifier and the
//! colour filter reach the compositor through the display service
//! (`VisualSettings` -> `Request::ConfigureVisual`); the visual alert and the bar
//! button reach the shell (`apply_setting`) – a banner owes a screen flash and the
//! quick settings show the accessibility button.

use super::setup::Run;
use super::wired::{change, choose, page, whole, Follower};
use ultar_bus::messages::settings::SettingValue;
use ultar_bus::Client;
use ultar_desktop::app::desktop::link::apply_setting;
use ultar_desktop::model::{Desktop, QuickPage};
use ultar_desktop::view::{quick, Look, Target};
use ultar_displayd::visual::VisualSettings;
use ultar_i18n::Lang;
use ultar_settings::App;
use ultar_surface::proto::{ColourFilter, Request};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Size;

/// Everything above.
pub fn check(
    bus: &Client,
    admin: &Client,
    run: &Run,
    desktop: &Follower,
    displayd: &Follower,
) -> Result<(), String> {
    let mut app = page(bus, run, "accessibility")?;
    view_compositor(&mut app, admin, displayd)?;
    view_shell(&mut app, admin, desktop)?;
    println!(
        "SETTINGS: accessibility page -> compositor and shell: magnifier and colour filter, \
         visual-alert flash, accessibility button in the quick settings"
    );
    Ok(())
}

/// The display service turns the view settings into the compositor request.
fn follow(
    v: &mut VisualSettings,
    displayd: &Follower,
    key: &str,
    value: SettingValue,
) -> Result<(), String> {
    displayd.told(key, &value)?;
    v.on_changed(key, &value)
        .then_some(())
        .ok_or(format!("the display service ignores {key}={value:?}"))
}

/// Zoom on, a larger zoom factor, the deuteranopia filter: the request carries them.
fn view_compositor(app: &mut App, admin: &Client, displayd: &Follower) -> Result<(), String> {
    let mut v = VisualSettings::default();
    let on = change((app, admin), ("accessibility", "accessibility.zoom"), None)?;
    follow(&mut v, displayd, "accessibility.zoom", on)?;
    let pct = change(
        (app, admin),
        ("accessibility", "accessibility.zoom_percent"),
        Some(0.5),
    )?;
    let n = whole("accessibility.zoom_percent", &pct)?;
    follow(&mut v, displayd, "accessibility.zoom_percent", pct)?;
    // The list starts at the selected option (none): three steps down is
    // greyscale -> protanopia -> deuteranopia.
    let filter = choose(
        (app, admin),
        ("accessibility", "accessibility.colour_filter"),
        3,
    )?;
    follow(&mut v, displayd, "accessibility.colour_filter", filter)?;
    let Request::ConfigureVisual {
        zoom,
        zoom_percent,
        filter,
    } = v.requests()[0].clone()
    else {
        return Err("no view configuration".into());
    };
    if !zoom || i64::from(zoom_percent) != n || filter != ColourFilter::Deuteranopia {
        return Err(format!(
            "the view became {zoom}, {zoom_percent} %, {filter:?}"
        ));
    }
    Ok(())
}

/// The shell follows the visual alert (a banner owes a flash) and show_in_bar (the
/// quick settings show the accessibility button).
fn view_shell(app: &mut App, admin: &Client, desktop: &Follower) -> Result<(), String> {
    let mut desk = Desktop::new(Lang::En);
    let on = change(
        (app, admin),
        ("accessibility", "accessibility.visual_alert"),
        None,
    )?;
    desktop.told("accessibility.visual_alert", &on)?;
    apply_setting(&mut desk, "accessibility.visual_alert", &on);
    if !desk.notes.visual_alert {
        return Err("the shell did not follow the visual alert".into());
    }
    // A banner that appears now owes a flash (the shell's own notice shows one).
    desk.notes
        .post_notice("org.ultar.shell", "desk.service.failed", 0);
    if !desk.notes.take_flash() {
        return Err("a banner did not owe a visual-alert flash".into());
    }
    let on = change(
        (app, admin),
        ("accessibility", "accessibility.show_in_bar"),
        None,
    )?;
    desktop.told("accessibility.show_in_bar", &on)?;
    apply_setting(&mut desk, "accessibility.show_in_bar", &on);
    if !desk.values.show_in_bar {
        return Err("the shell did not follow show_in_bar".into());
    }
    // The quick settings show the accessibility button (the launcher is there).
    desk.presence.launcher = true;
    let tr = ultar_desktop::i18n::translator(Lang::En).map_err(|e| format!("{e:?}"))?;
    let look = Look::new(Lang::En, ThemeKind::Dark);
    let (view, _) = quick::build(&desk, QuickPage::Tiles, look, Size::new(420.0, 760.0), &tr)
        .map_err(|e| format!("{e:?}"))?;
    if view.node(&Target::AccessSettings).is_none() {
        return Err("the quick settings have no accessibility button".into());
    }
    Ok(())
}
