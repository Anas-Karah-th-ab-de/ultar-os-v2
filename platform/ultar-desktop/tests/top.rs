//! The variant "bar at the top" (board DesktopTop): the top bar and the dock with the
//! boards' geometry, `shell.appmenu`, and where the sheets and menus go.

mod common;

use common::*;
use ultar_desktop::model::{BarPlace, Open, QuickPage, Shape};
use ultar_desktop::shell::{bar_panel, second_panel, Effect, Opening};
use ultar_desktop::view::Target;
use ultar_i18n::Lang;
use ultar_surface::proto::{Edge, WindowAction};
use ultar_ui::event::ActionKind;
use ultar_ui::geom::Rect;
use ultar_ui::tokens::ThemeKind;

fn h() -> Harness {
    Harness::top(Lang::Ar, ThemeKind::Dark)
}

fn rect(view: &ultar_desktop::view::View, t: &Target) -> Rect {
    view.ui.rect(view.node(t).unwrap()).unwrap()
}

#[test]
fn the_bottom_bar_is_the_standard_and_the_setting_chooses_the_top_bar() {
    assert_eq!(Shape::default().bar, BarPlace::Bottom);
    assert_eq!(bar_panel(Shape::default()), (Edge::Bottom, 56.0));
    assert_eq!(BarPlace::parse("top"), Some(BarPlace::Top));
    let h = h();
    assert_eq!(bar_panel(h.shell.shape), (Edge::Top, 32.0));
    assert_eq!(
        second_panel(h.shell.shape),
        Some((Edge::Bottom, 68.0, true))
    );
    assert!(h.second.is_some() && h.home.is_none());
    assert!(Harness::new(Lang::Ar, ThemeKind::Dark).second.is_none());
}

#[test]
fn top_bar_and_dock_have_the_boards_geometry() {
    let h = h();
    // Top bar: 32 high controls, 8 from the edges; Arabic: overview at the right.
    let overview = rect(&h.bar, &Target::Overview);
    assert_eq!(
        (overview.w, overview.h, overview.right()),
        (32.0, 32.0, 1432.0)
    );
    let menu = rect(&h.bar, &Target::AppMenu);
    assert_eq!((menu.h, overview.x - menu.right()), (32.0, 8.0));
    let clock = rect(&h.bar, &Target::Clock);
    assert!((clock.x + clock.w / 2.0 - 720.0).abs() <= 1.0, "centred");
    let status = rect(&h.bar, &Target::Status);
    let kbd = rect(&h.bar, &Target::Keyboard);
    assert_eq!((status.x, kbd.x - status.right()), (8.0, 4.0));
    // Dock: 56 high, 12 above the bottom edge, centred; tiles 44 with a gap of 8.
    let dock = h.second.as_ref().unwrap();
    let apps = rect(dock, &Target::AllApps);
    let first = rect(dock, &Target::App(0));
    let trash = rect(dock, &Target::Trash);
    assert_eq!((apps.w, apps.h, first.w, trash.w), (44.0, 44.0, 44.0, 44.0));
    assert_eq!(apps.y, (56.0 - 44.0) / 2.0);
    // apps | separator | tiles: 8 + 1 + 8 between "all apps" and the first tile.
    assert_eq!(apps.x - first.right(), 17.0);
    let centre = (apps.right() + trash.x) / 2.0;
    assert!((centre - 720.0).abs() <= 1.0, "the dock is centred");
    for i in 0..6 {
        assert!(dock.node(&Target::App(i)).is_some());
    }
}

#[test]
fn shell_appmenu_is_the_menu_of_the_active_app() {
    let mut h = h();
    h.click_bar(Target::AppMenu);
    assert_eq!(h.shell.d.open, Open::AppMenu);
    let p = h.popup.as_ref().unwrap();
    // Below the top bar (panel at the top: offsets are screen coordinates).
    assert_eq!(p.offset.1 as f32 + p.inset_top, 32.0 + 8.0);
    // Quit closes the app's windows (Files has the focus on the board).
    let out = h.shell.on_menu(103);
    h.apply(out);
    assert_eq!(h.take(), vec![Effect::Window(2, WindowAction::Close)]);
    assert_eq!(h.shell.d.open, Open::None);
    // New window.
    h.click_bar(Target::AppMenu);
    let out = h.shell.on_menu(101);
    h.apply(out);
    assert_eq!(
        h.take(),
        vec![Effect::Launch {
            app: "org.ultar.files".into(),
            open: Opening::new_window()
        }]
    );
    // About: the app's page in the Store; App settings: its page in Settings.
    h.click_bar(Target::AppMenu);
    let out = h.shell.on_menu(106);
    h.apply(out);
    assert_eq!(
        h.take(),
        vec![Effect::Launch {
            app: "org.ultar.store".into(),
            open: Opening::store_page("org.ultar.files", false)
        }]
    );
    h.click_bar(Target::AppMenu);
    let out = h.shell.on_menu(107);
    h.apply(out);
    assert_eq!(
        h.take(),
        vec![Effect::Launch {
            app: "org.ultar.settings".into(),
            open: Opening::settings_page("apps/org.ultar.files")
        }]
    );
    // Not present when no app is active.
    let mut d = board_desktop(Lang::Ar, ThemeKind::Dark);
    d.windows.clear();
    let none = Harness::shaped(d, Lang::Ar, ThemeKind::Dark, Shape::TOP, SCREEN);
    assert!(none.bar.node(&Target::AppMenu).is_none());
}

#[test]
fn sheets_hang_below_the_top_bar_and_the_dock_works_like_the_bar() {
    let mut h = h();
    h.click_bar(Target::Status);
    assert_eq!(h.shell.d.open, Open::Quick(QuickPage::Tiles));
    let p = h.popup.as_ref().unwrap();
    assert_eq!(p.offset.1 as f32 + p.inset_top, 40.0);
    h.click_bar(Target::Clock);
    assert_eq!(h.shell.d.open, Open::Calendar);
    assert_eq!(
        h.popup.as_ref().unwrap().offset.1 as f32 + h.popup.as_ref().unwrap().inset_top,
        40.0
    );
    h.click_bar(Target::Overview);
    let p = h.popup.as_ref().unwrap();
    assert_eq!((p.offset, p.size.h), ((0, 32), 900.0 - 32.0 - 68.0));
    // The dock: all apps, an app tile, the trash, middle click.
    h.click_side(Target::AllApps);
    assert_eq!(h.shell.d.open, Open::Apps(None));
    h.click_side(Target::App(1));
    h.click_side(Target::Trash);
    h.act(Target::App(1), ActionKind::MiddleClicked);
    let launched: Vec<String> = h
        .take()
        .into_iter()
        .filter_map(|e| match e {
            Effect::Launch { app, .. } => Some(app),
            _ => None,
        })
        .collect();
    assert_eq!(
        launched,
        ["org.ultar.browser", "org.ultar.files", "org.ultar.browser"]
    );
    // A tile's menu stands above the dock.
    let out = h.shell.on_bar_menu(&Target::App(0));
    h.apply(out);
    let p = h.popup.as_ref().unwrap();
    assert!(p.offset.1 as f32 + p.size.h <= 900.0 - 68.0 + 48.0);
    assert!(p.offset.1 > 450, "near the dock, not below the top bar");
}
