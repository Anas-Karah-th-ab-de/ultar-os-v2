//! The phone-size shell (boards CompactHome, CompactApps, CompactQuick): every `mobile.*`
//! and `qs.mobile*` click of CLICKS.md with its effect, and the boards' geometry.

mod common;

use common::*;
use ultar_bus::messages::settings::SettingValue;
use ultar_desktop::model::quick::TileId;
use ultar_desktop::model::services::Call;
use ultar_desktop::model::{Form, Open, QuickPage};
use ultar_desktop::shell::{bar_panel, second_panel, Effect, Opening, Outcome};
use ultar_desktop::view::{apps::item, Target};
use ultar_i18n::Lang;
use ultar_surface::proto::SwipeEdge;
use ultar_surface::proto::{Edge, WindowAction};
use ultar_ui::event::{ActionKind, Key};
use ultar_ui::geom::{Point, Rect};
use ultar_ui::tokens::ThemeKind;

fn h() -> Harness {
    Harness::phone(Lang::Ar, ThemeKind::Dark)
}

fn rect(view: &ultar_desktop::view::View, t: &Target) -> Rect {
    view.ui.rect(view.node(t).unwrap()).unwrap()
}

fn app(id: &str) -> Target {
    Target::AppItem(id.into())
}

#[test]
fn a_phone_screen_gets_the_phone_shell() {
    assert_eq!(Form::choose(None, PHONE.w), Form::Compact);
    let h = h();
    assert_eq!(bar_panel(h.shell.shape), (Edge::Top, 32.0));
    assert_eq!(
        second_panel(h.shell.shape),
        Some((Edge::Bottom, 44.0, false))
    );
    assert!(h.home.is_some() && h.second.is_some() && h.popup.is_none());
}

#[test]
fn the_home_screen_has_the_boards_geometry() {
    let h = h();
    let home = h.home.as_ref().unwrap();
    // Search entry: 16 from both sides, 32 high.
    let search = rect(home, &Target::Search);
    assert_eq!((search.x, search.w, search.h), (16.0, 328.0, 32.0));
    // Four columns of cells 96 high with a gap of 8; Arabic: the first app at the right.
    let files = rect(home, &app("org.ultar.files"));
    let editor = rect(home, &app("org.ultar.editor"));
    assert_eq!((files.w, files.h), (76.0, 96.0));
    assert_eq!(files.right(), 344.0);
    assert_eq!(files.x - editor.right(), 8.0);
    assert_eq!(files.y - search.bottom(), 16.0);
    // Favourites: a card 8 inside, the last cell is "all apps", its bottom 12 above the
    // gesture bar.
    let all = rect(home, &Target::AllApps);
    let browser = rect(home, &app("org.ultar.browser"));
    assert_eq!(all.x, 16.0 + 8.0);
    assert_eq!(browser.right(), 344.0 - 8.0);
    assert_eq!(all.bottom(), 800.0 - 44.0 - 12.0 - 8.0);
    assert!(home.node(&app("org.ultar.terminal")).is_none());
    // Status bar: the status button at the end, 32 high; gesture bar: all of its 44.
    let status = rect(&h.bar, &Target::Status);
    assert_eq!((status.x, status.h), (12.0, 32.0));
    let handle = rect(h.second.as_ref().unwrap(), &Target::Home);
    assert_eq!((handle.w, handle.h), (360.0, 44.0));
    let en = Harness::phone(Lang::En, ThemeKind::Light);
    let files = rect(en.home.as_ref().unwrap(), &app("org.ultar.files"));
    assert_eq!(files.x, 16.0);
}

#[test]
fn mobile_status_opens_and_closes_the_quick_settings() {
    let mut h = h();
    h.click_bar(Target::Status);
    assert_eq!(h.shell.d.open, Open::Quick(QuickPage::Tiles));
    let p = h.popup.as_ref().unwrap();
    assert_eq!((p.offset, p.size.w, p.size.h), ((0, 0), 360.0, 756.0));
    // sheet.close: the handle, the dimmed part, a swipe up on the handle, Esc.
    h.click_popup(Target::SheetClose);
    assert_eq!(h.shell.d.open, Open::None);
    // A swipe down on the status bar opens it too; a short drag does not.
    let drag = |y: f32| ActionKind::DragEnded {
        delta: Point::new(0.0, y),
        target: None,
    };
    h.act(Target::Status, drag(10.0));
    assert_eq!(h.shell.d.open, Open::None);
    h.act(Target::Status, drag(60.0));
    assert_eq!(h.shell.d.open, Open::Quick(QuickPage::Tiles));
    h.act(Target::SheetClose, drag(-60.0));
    assert_eq!(h.shell.d.open, Open::None);
    h.click_bar(Target::Status);
    h.key_popup(Key::Escape);
    assert_eq!(h.shell.d.open, Open::None);
    h.click_bar(Target::Status);
    h.click_side(Target::Home);
    assert_eq!(h.shell.d.open, Open::None, "home closes the sheet");
}

#[test]
fn mobile_search_and_apps_open_over_the_home_screen() {
    let mut h = h();
    h.click_side(Target::Search);
    assert_eq!(h.shell.d.open, Open::Overview);
    let p = h.popup.as_ref().unwrap();
    assert_eq!((p.offset, p.size.h), ((0, 32), 724.0));
    assert_eq!(
        p.view.ui.focused(),
        p.view.node(&Target::SearchField),
        "the caret is in the field"
    );
    h.type_popup("متصفح");
    assert!(
        h.popup_target(&Target::Result(ultar_desktop::model::search::Hit::App(
            "org.ultar.browser".into()
        )))
    );
    h.key_popup(Key::Enter);
    assert_eq!(
        h.take(),
        vec![Effect::Launch {
            app: "org.ultar.browser".into(),
            open: Opening::start()
        }]
    );
    assert_eq!(h.shell.d.open, Open::None);
    // mobile.apps.
    h.click_side(Target::AllApps);
    assert_eq!(h.shell.d.open, Open::Apps(None));
    let p = h.popup.as_ref().unwrap();
    assert_eq!((p.offset, p.size.h), ((0, 32), 724.0));
    let settings = rect(&p.view, &app("org.ultar.settings"));
    assert_eq!(
        (settings.w, settings.h, settings.right()),
        (76.0, 96.0, 344.0)
    );
    h.click_popup(app("org.ultar.terminal"));
    assert_eq!(
        h.take(),
        vec![Effect::Launch {
            app: "org.ultar.terminal".into(),
            open: Opening::start()
        }]
    );
    assert_eq!(h.shell.d.open, Open::None);
}

#[test]
fn mobile_app_starts_the_app_and_a_long_press_opens_its_menu() {
    let mut h = h();
    h.click_side(app("org.ultar.files"));
    assert_eq!(
        h.take(),
        vec![Effect::Launch {
            app: "org.ultar.files".into(),
            open: Opening::start()
        }]
    );
    // The menu's "favourites" entry (a long press is the secondary click of touch).
    assert!(h.right_home(app("org.ultar.files")));
    h.act(app("org.ultar.files"), ActionKind::MenuChosen(item::PIN));
    assert!(h.shell.d.pinned.contains(&"org.ultar.files".to_owned()));
    h.act(app("org.ultar.browser"), ActionKind::MenuChosen(item::PIN));
    assert!(!h.shell.d.pinned.contains(&"org.ultar.browser".to_owned()));
    assert!(h
        .home
        .as_ref()
        .unwrap()
        .node(&app("org.ultar.browser"))
        .is_none());
}

#[test]
fn mobile_home_puts_the_apps_away_and_held_shows_the_recent_ones() {
    let mut h = h();
    let out = h.shell.on_windows(vec![
        window(1, "org.ultar.files", "الملفات", false),
        window(2, "org.ultar.browser", "المتصفح", true),
    ]);
    // Every app is full screen: the compositor's phone mode does it (no shell effect).
    assert!(out.effects.is_empty());
    h.apply(out);
    h.take();
    h.click_side(Target::Home);
    assert_eq!(
        h.take(),
        vec![
            Effect::Window(1, WindowAction::Minimize),
            Effect::Window(2, WindowAction::Minimize)
        ]
    );
    // A swipe up does the same.
    h.act(
        Target::Home,
        ActionKind::DragEnded {
            delta: Point::new(0.0, -40.0),
            target: None,
        },
    );
    assert_eq!(h.take().len(), 2);
    // Held: the recent apps; choosing one brings it back.
    let out = h.shell.on_bar_menu(&Target::Home);
    h.apply(out);
    assert_eq!(h.shell.d.open, Open::Overview);
    h.click_popup(Target::Window(1));
    assert_eq!(h.take(), vec![Effect::Window(1, WindowAction::Activate)]);
}

#[test]
fn the_quick_settings_have_the_boards_tiles_and_reasons() {
    let mut h = h();
    h.click_bar(Target::Status);
    let p = h.popup.as_ref().unwrap();
    // Two columns of tiles 56 high, 16 from the sides, gap 8.
    let wifi = rect(&p.view, &Target::Tile(TileId::Wifi));
    let more = rect(&p.view, &Target::TileMore(TileId::Wifi));
    let mobile = rect(&p.view, &Target::Tile(TileId::Mobile));
    assert_eq!((wifi.h, more.w, wifi.right()), (56.0, 32.0, 344.0));
    assert_eq!(
        more.x - rect(&p.view, &Target::TileMore(TileId::Mobile)).right(),
        8.0 + mobile.w
    );
    assert_eq!(
        mobile
            .x
            .min(rect(&p.view, &Target::TileMore(TileId::Mobile)).x),
        16.0
    );
    for id in TileId::COMPACT {
        assert!(p.view.node(&Target::Tile(id)).is_some(), "{id:?}");
    }
    assert!(p.view.node(&Target::Tile(TileId::Vpn)).is_none());
    // qs.wifi / qs.mobile / qs.bluetooth / airplane and their lists: no service on the
    // bus – disabled with the reason, never a click without effect.
    for t in [
        Target::Tile(TileId::Wifi),
        Target::TileMore(TileId::Wifi),
        Target::Tile(TileId::Mobile),
        Target::TileMore(TileId::Mobile),
        Target::Tile(TileId::Bluetooth),
        Target::TileMore(TileId::Bluetooth),
        Target::Tile(TileId::Airplane),
        Target::Volume,
        Target::Output,
    ] {
        let node = p.view.node(&t).unwrap();
        assert!(p.view.ui.is_disabled(node), "{t:?}");
        assert!(p.view.ui.tooltip(node).is_some(), "{t:?} says why");
    }
}

#[test]
fn the_quick_settings_switch_set_and_clear() {
    let mut h = h();
    phone_note(&mut h.shell.d, Lang::Ar);
    h.click_bar(Target::Status);
    // qs.toggle.
    h.click_popup(Target::Tile(TileId::NightLight));
    h.click_popup(Target::Tile(TileId::Dnd));
    assert_eq!(
        h.take(),
        vec![
            Effect::Setting(
                "display.night_light.enabled".into(),
                SettingValue::Flag(false)
            ),
            Effect::Setting(
                "notifications.do_not_disturb".into(),
                SettingValue::Flag(true)
            ),
        ]
    );
    // qs.brightness.
    h.dispatch_popup(vec![ultar_ui::event::Action {
        node: h
            .popup
            .as_ref()
            .unwrap()
            .view
            .node(&Target::Brightness)
            .unwrap(),
        kind: ActionKind::ValueChanged(40.0),
    }]);
    assert_eq!(
        h.take(),
        vec![Effect::Setting(
            "display.edp-1.brightness".into(),
            SettingValue::Integer(40)
        )]
    );
    // note.dismiss, note.clear with undo.
    assert!(h.popup_target(&Target::Dismiss(7)));
    h.click_popup(Target::ClearAll);
    assert!(h.shell.d.notes.list().is_empty() && h.popup_target(&Target::Undo));
    h.click_popup(Target::Undo);
    h.click_popup(Target::Dismiss(7));
    assert!(h.shell.d.notes.list().is_empty());
    // The notification service learns the dismissal.
    assert_eq!(h.take(), vec![Effect::Service(Call::NoteDismiss(7))]);
    // qs.battery, qs.settings, qs.power.
    h.click_popup(Target::Battery);
    h.click_bar(Target::Status);
    h.click_popup(Target::Settings);
    let launched: Vec<_> = h.take();
    assert_eq!(launched.len(), 2);
    h.click_bar(Target::Status);
    h.click_popup(Target::Power);
    assert_eq!(h.shell.d.open, Open::PowerMenu);
    assert!(h.popup.is_some());
}

#[test]
fn without_a_modem_there_is_no_mobile_tile_and_no_cell_icon() {
    let mut d = board_phone(Lang::Ar, ThemeKind::Dark);
    d.presence.modem = Some(false);
    let mut h = Harness::shaped(
        d,
        Lang::Ar,
        ThemeKind::Dark,
        ultar_desktop::model::Shape::COMPACT,
        PHONE,
    );
    h.click_bar(Target::Status);
    assert!(!h.popup_target(&Target::Tile(TileId::Mobile)));
    assert!(h.popup_target(&Target::Tile(TileId::Wifi)));
}

#[test]
fn the_compositors_edge_gestures_are_home_recent_apps_and_back() {
    let mut h = h();
    let out = h.shell.on_windows(vec![
        window(1, "org.ultar.files", "الملفات", false),
        window(2, "org.ultar.browser", "المتصفح", true),
    ]);
    h.apply(out);
    // The compositor's edge gestures (phone mode): up from the bottom edge = home, held =
    // recent apps, from a side = back (the shell's sheet first, then the focused app).
    let out = h.shell.on_edge(SwipeEdge::Bottom, false);
    assert_eq!(out.effects.len(), 2, "both windows minimised");
    h.apply(out);
    let out = h.shell.on_edge(SwipeEdge::Bottom, true);
    h.apply(out);
    assert_eq!(h.shell.d.open, Open::Overview);
    let out = h.shell.on_edge(SwipeEdge::Side, false);
    h.apply(out);
    assert_eq!(h.shell.d.open, Open::None, "back closes the recent apps");
    h.take();
    let out = h.shell.on_edge(SwipeEdge::Side, false);
    assert_eq!(out.effects, vec![Effect::Window(2, WindowAction::Back)]);
    h.shell.d.open = Open::Quick(ultar_desktop::model::QuickPage::Wifi);
    let out = h.shell.on_edge(SwipeEdge::Side, false);
    assert!(out.effects.is_empty());
    assert_eq!(
        h.shell.d.open,
        Open::Quick(ultar_desktop::model::QuickPage::Tiles)
    );
    // On a desktop the shell ignores them.
    let mut desk = Harness::new(Lang::Ar, ThemeKind::Dark);
    assert_eq!(
        desk.shell.on_edge(SwipeEdge::Bottom, false),
        Outcome::none()
    );
    desk.take();
}
