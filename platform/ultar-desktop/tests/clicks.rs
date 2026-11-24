//! Every click of CLICKS.md that belongs to the shell's surfaces, with its effect
//! (DESKTOP-SPEC §2.2d): the table is read from `docs/ui/CLICKS.md`; an action of these
//! boards without a check here – and without a reason why it belongs to another program –
//! fails the test.

mod common;

use common::*;
use std::collections::BTreeSet;
use ultar_bus::messages::settings::SettingValue;
use ultar_desktop::model::quick::{PowerMode, TileId};
use ultar_desktop::model::search::Hit;
use ultar_desktop::model::services::Call;
use ultar_desktop::model::{Open, QuickPage};
use ultar_desktop::shell::{Effect, Opening};
use ultar_desktop::view::Target;
use ultar_i18n::Lang;
use ultar_surface::proto::WindowAction;
use ultar_ui::event::{ActionKind, Key};
use ultar_ui::tokens::ThemeKind;

/// Boards whose clicks are (partly) the shell's.
const BOARDS: [&str; 6] = [
    "Main",
    "DesktopBottom",
    "QuickSettings",
    "CalendarSheet",
    "Overview",
    "Projection",
];

/// Actions on these boards that belong to another program, with the reason.
const ELSEWHERE: [(&str, &str); 4] = [
    ("files.", "the Files app (window content on the board)"),
    ("browser.", "the Browser app (window content on the board)"),
    (
        "win.",
        "drawn and handled by ultar-compositor (decor.rs, tests/input_routing.rs)",
    ),
    (
        "desk.icon",
        "desktop icons belong to the desktop surface of Files (no board item of the bar)",
    ),
];

fn actions() -> BTreeSet<(String, String)> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/ui/CLICKS.md");
    let text = std::fs::read_to_string(path).unwrap();
    let mut board = String::new();
    let mut out = BTreeSet::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            board = rest.split(' ').next().unwrap_or("").to_owned();
        } else if line.starts_with("| ") && BOARDS.contains(&board.as_str()) {
            let cells: Vec<&str> = line.split('|').collect();
            if let Some(a) = cells.get(3).map(|c| c.trim().trim_matches('`')) {
                if a.contains('.') && !a.contains(' ') {
                    out.insert((board.clone(), a.to_owned()));
                }
            }
        }
    }
    out
}

fn h() -> Harness {
    Harness::new(Lang::Ar, ThemeKind::Dark)
}

fn launched(h: &mut Harness) -> Vec<String> {
    h.take()
        .into_iter()
        .filter_map(|e| match e {
            Effect::Launch { app, .. } => Some(app),
            _ => None,
        })
        .collect()
}

fn check(action: &str) -> bool {
    match action {
        "shell.trash" => {
            let mut h = h();
            h.click_bar(Target::Trash);
            h.take()
                == vec![Effect::Launch {
                    app: "org.ultar.files".into(),
                    open: Opening::uri("trash:///"),
                }]
        }
        "shell.overview" => {
            let mut h = h();
            h.click_bar(Target::Overview);
            let open = h.shell.d.open == Open::Overview && h.popup_target(&Target::SearchField);
            h.click_bar(Target::Overview);
            open && h.shell.d.open == Open::None && h.popup.is_none()
        }
        "shell.apps" => {
            let mut h = h();
            h.click_bar(Target::AllApps);
            matches!(h.shell.d.open, Open::Apps(_))
                && h.popup_target(&Target::AppItem("org.ultar.files".into()))
        }
        "shell.search" => {
            let mut h = h();
            h.click_bar(Target::Search);
            let focused = h.popup.as_ref().and_then(|p| p.view.ui.focused());
            let field = h
                .popup
                .as_ref()
                .and_then(|p| p.view.node(&Target::SearchField));
            h.type_popup("الطرفية");
            h.shell.d.open == Open::Overview
                && focused == field
                && h.popup_target(&Target::Result(Hit::App("org.ultar.terminal".into())))
        }
        "shell.app" => shell_app(),
        "shell.drive" => false,
        a if a.starts_with("qs.") => quick(a),
        a if a.starts_with("note.") || a.starts_with("cal.") => calendar(a),
        a if a.starts_with("overview.") => overview(a),
        "projection.mode" => projection(),
        _ => bar_menus(action),
    }
}

/// The bar's language menu and its two sheets.
fn bar_menus(action: &str) -> bool {
    match action {
        "shell.kbd" => {
            let mut h = h();
            h.click_bar(Target::Keyboard);
            let open = h.shell.d.open == Open::KeyboardMenu;
            h.dispatch_popup(vec![ultar_ui::event::Action {
                node: h.popup.as_ref().unwrap().view.node(&Target::Menu).unwrap(),
                kind: ActionKind::MenuChosen(1),
            }]);
            open && h.take() == vec![Effect::Layout(1)]
        }
        "shell.status" | "shell.clock" => {
            let (t, o) = if action == "shell.status" {
                (Target::Status, Open::Quick(QuickPage::Tiles))
            } else {
                (Target::Clock, Open::Calendar)
            };
            let mut h = h();
            h.click_bar(t.clone());
            let opened = h.shell.d.open == o
                && h.popup
                    .as_ref()
                    .is_some_and(|p| p.view.ui.focused().is_some());
            h.key_popup(Key::Escape);
            let esc = h.shell.d.open == Open::None;
            h.click_bar(Target::Status);
            h.click_bar(Target::Clock);
            opened && esc && h.shell.d.open == Open::Calendar
        }
        _ => false,
    }
}

fn shell_app() -> bool {
    let mut h = h();
    // Files: its only window is in front – a click minimises it (taskbar habit).
    h.click_bar(Target::App(0));
    let minimise = h.take() == vec![Effect::Window(2, WindowAction::Minimize)];
    // Terminal: raise.
    h.click_bar(Target::App(2));
    let raise = h.take() == vec![Effect::Window(1, WindowAction::Activate)];
    // Browser: not running – start.
    h.click_bar(Target::App(1));
    let start = launched(&mut h) == ["org.ultar.browser"];
    h.middle_bar(Target::App(0));
    let new_window = h.take()
        == vec![Effect::Launch {
            app: "org.ultar.files".into(),
            open: Opening::new_window(),
        }];
    h.right_bar(Target::App(0));
    let menu = matches!(h.shell.d.open, Open::TileMenu(0)) && h.popup.is_some();
    minimise && raise && start && new_window && menu
}

/// The rest of the quick settings sheet (brightness to power).
fn quick_more(a: &str, h: &mut Harness) -> bool {
    match a {
        "qs.brightness" => {
            let p = h.popup.as_mut().unwrap();
            let n = p.view.node(&Target::Brightness).unwrap();
            let r = p.view.ui.handle(ultar_ui::event::Event::Key {
                key: Key::ArrowRight,
                modifiers: Default::default(),
            });
            let _ = r;
            p.view.ui.focus(n).unwrap();
            let actions = p
                .view
                .ui
                .handle(ultar_ui::event::Event::Key {
                    key: Key::ArrowRight,
                    modifiers: Default::default(),
                })
                .actions;
            h.dispatch_popup(actions);
            h.take().iter().any(|e| matches!(e, Effect::Setting(k, SettingValue::Integer(_)) if k == "display.edp-1.brightness"))
        }
        "qs.battery" => {
            // Without a battery (power.state) the button is absent; with one it opens Settings > Power.
            h.shell.d.values.battery = Some((87, false));
            h.shell.d.open = Open::Quick(QuickPage::Tiles);
            h.apply(ultar_desktop::shell::Outcome::surfaces());
            h.click_popup(Target::Battery);
            h.take()
                == vec![Effect::Launch {
                    app: "org.ultar.settings".into(),
                    open: Opening::settings_page("power"),
                }]
        }
        "qs.assistant" => !h.popup_target(&Target::Menu),
        "qs.shot" => disabled(h, Target::Screenshot),
        "qs.settings" => {
            h.click_popup(Target::Settings);
            launched(h) == ["org.ultar.settings"]
        }
        "qs.lock" => {
            h.click_popup(Target::Lock);
            h.take() == vec![Effect::Lock] && h.popup.is_none()
        }
        "qs.power" => {
            h.click_popup(Target::Power);
            h.shell.d.open == Open::PowerMenu && h.popup.is_some()
        }
        _ => false,
    }
}

/// Shown, disabled and with the reason as its tooltip.
fn disabled(h: &Harness, t: Target) -> bool {
    let p = h.popup.as_ref().unwrap();
    p.view
        .node(&t)
        .is_some_and(|n| p.view.ui.is_disabled(n) && p.view.ui.tooltip(n).is_some())
}

fn tile_effect(h: &mut Harness, id: TileId) -> Vec<Effect> {
    h.click_popup(Target::Tile(id));
    h.take()
}

fn quick(a: &str) -> bool {
    let mut h = h();
    h.click_bar(Target::Status);
    match a {
        "qs.wifi" | "qs.bluetooth" | "qs.vpn" => {
            let id = match a {
                "qs.wifi" => TileId::Wifi,
                "qs.bluetooth" => TileId::Bluetooth,
                _ => TileId::Vpn,
            };
            disabled(&h, Target::Tile(id)) && tile_effect(&mut h, id).is_empty()
        }
        "qs.wifi.list" | "qs.bluetooth.list" | "qs.vpn.list" => {
            let id = match a {
                "qs.wifi.list" => TileId::Wifi,
                "qs.bluetooth.list" => TileId::Bluetooth,
                _ => TileId::Vpn,
            };
            let p = h.popup.as_ref().unwrap();
            p.view
                .node(&Target::TileMore(id))
                .is_some_and(|n| p.view.ui.is_disabled(n))
        }
        "qs.wired" => {
            tile_effect(&mut h, TileId::Wired)
                == vec![Effect::Launch {
                    app: "org.ultar.settings".into(),
                    open: Opening::settings_page("network"),
                }]
        }
        _ => quick_tiles(a, &mut h),
    }
}

/// The switch tiles of the quick settings.
fn quick_tiles(a: &str, h: &mut Harness) -> bool {
    match a {
        "qs.toggle" => {
            let night = tile_effect(h, TileId::NightLight)
                == vec![Effect::Setting(
                    "display.night_light.enabled".into(),
                    SettingValue::Flag(true),
                )];
            let dark = tile_effect(h, TileId::Dark)
                == vec![Effect::Setting(
                    "appearance.theme".into(),
                    SettingValue::Text("light".into()),
                )];
            let dnd = tile_effect(h, TileId::Dnd)
                == vec![Effect::Setting(
                    "notifications.do_not_disturb".into(),
                    SettingValue::Flag(true),
                )];
            night && dark && dnd && disabled(h, Target::Tile(TileId::Airplane))
        }
        "qs.powermode" => {
            tile_effect(h, TileId::PowerMode)
                == vec![Effect::Setting(
                    "power.mode".into(),
                    SettingValue::Text("saver".into()),
                )]
        }
        "qs.powermode.list" => {
            h.click_popup(Target::TileMore(TileId::PowerMode));
            let page = h.shell.d.open == Open::Quick(QuickPage::PowerModes);
            h.click_popup(Target::QuickBack);
            page && h.shell.d.open == Open::Quick(QuickPage::Tiles)
        }
        "qs.mesh" | "qs.mesh.list" => h
            .popup
            .as_ref()
            .unwrap()
            .view
            .node(&Target::Tile(TileId::Mesh))
            .is_none(),
        "qs.volume" | "qs.output" => disabled(
            h,
            if a == "qs.volume" {
                Target::Volume
            } else {
                Target::Output
            },
        ),
        _ => quick_more(a, h),
    }
}

fn calendar(a: &str) -> bool {
    let mut d = board_desktop(Lang::Ar, ThemeKind::Dark);
    for (i, app) in ["org.ultar.update", "org.ultar.files"].iter().enumerate() {
        d.notes.add(ultar_bus::messages::notify::Posted {
            id: i as u64 + 1,
            timestamp_ms: BOARD_MS,
            post: ultar_bus::messages::notify::Post {
                urgency: Default::default(),
                category: Default::default(),
                app_id: (*app).into(),
                title: ultar_bus::Text::key("desk.cal.notes"),
                body: ultar_bus::messages::notify::Body::Verbatim("x".into()),
                actions: vec![ultar_bus::messages::notify::Action {
                    id: "open".into(),
                    label: ultar_bus::Text::key("desk.menu.open"),
                }],
            },
            sender: format!("app.{app}"),
        });
    }
    let mut h = Harness::with(d, Lang::Ar, ThemeKind::Dark);
    h.click_bar(Target::Clock);
    // Opening asks the calendar service for the day's appointments.
    let asked = matches!(
        h.take().as_slice(),
        [Effect::Service(Call::Day(2026, 11, 22))]
    );
    if !asked {
        return false;
    }
    match a {
        "note.dnd" => {
            h.click_popup(Target::Dnd);
            h.take()
                == vec![Effect::Setting(
                    "notifications.do_not_disturb".into(),
                    SettingValue::Flag(true),
                )]
        }
        "note.clear" => {
            h.click_popup(Target::ClearAll);
            let cleared = h.shell.d.notes.list().is_empty() && h.popup_target(&Target::Undo);
            h.click_popup(Target::Undo);
            cleared && h.shell.d.notes.list().len() == 2
        }
        "note.dismiss" => {
            h.click_popup(Target::Dismiss(2));
            h.shell.d.notes.list().len() == 1
        }
        // A notification's button goes back to its sender through the service.
        "note.action" => {
            h.click_popup(Target::NoteAction(1, 0));
            h.take() == vec![Effect::Service(Call::NoteAction(1, "open".into()))]
                && h.shell.d.notes.list().len() == 1
        }
        _ => calendar_grid(a, &mut h),
    }
}

/// The month grid of the calendar sheet.
fn calendar_grid(a: &str, h: &mut Harness) -> bool {
    match a {
        "cal.day" => {
            let p = h.popup.as_mut().unwrap();
            let grid = p.view.node(&Target::Days).unwrap();
            p.view.ui.focus(grid).unwrap();
            let actions = p
                .view
                .ui
                .handle(ultar_ui::event::Event::Key {
                    key: Key::ArrowDown,
                    modifiers: Default::default(),
                })
                .actions;
            h.dispatch_popup(actions);
            matches!(h.shell.d.calendar, Some((_, 29)))
        }
        "cal.month" => {
            h.click_popup(Target::NextMonth);
            let next = h.shell.d.calendar.map(|c| c.0.month) == Some(12);
            h.click_popup(Target::PrevMonth);
            h.click_popup(Target::PrevMonth);
            next && h.shell.d.calendar.map(|c| c.0.month) == Some(10)
        }
        // The day's appointments (calendar.day); a click opens one in the Calendar app.
        "cal.event" => {
            let event = ultar_bus::messages::calendar::DayEvent {
                id: 42,
                start_minute: 9 * 60 + 30,
                duration_minutes: 60,
                title: "اجتماع فريق التصميم".into(),
            };
            h.shell.d.svc.set_day((2026, 11, 22), vec![event]);
            h.apply(ultar_desktop::shell::Outcome::surfaces());
            h.click_popup(Target::Event(42));
            h.take()
                == vec![Effect::Launch {
                    app: "org.ultar.calendar".into(),
                    open: Opening::uri("calendar://event/42"),
                }]
        }
        _ => false,
    }
}

fn overview(a: &str) -> bool {
    let mut h = h();
    h.click_bar(Target::Overview);
    // Opening asks the compositor for copies of the shown windows.
    let asked = h.take() == vec![Effect::Thumbnails(vec![1, 2], (512, 384))];
    if !asked {
        return false;
    }
    match a {
        "overview.search" => {
            h.type_popup("12*3");
            h.popup_target(&Target::Result(Hit::Calc("36".into())))
        }
        "overview.window" => {
            h.click_popup(Target::Window(1));
            h.take() == vec![Effect::Window(1, WindowAction::Activate)] && h.popup.is_none()
        }
        "overview.close" => {
            h.click_popup(Target::CloseWindow(1));
            h.take() == vec![Effect::Window(1, WindowAction::Close)]
                && h.shell.d.open == Open::Overview
        }
        "overview.workspace" => {
            // Board: two workspaces, the first shown, and "new workspace".
            h.shell.d.overview.workspaces = Some((0, 2));
            h.apply(ultar_desktop::shell::Outcome::surfaces());
            h.click_popup(Target::Workspace(1));
            let other = h.take() == vec![Effect::Workspace(1)];
            h.click_popup(Target::Workspace(2));
            let new = h.take() == vec![Effect::Workspace(2)];
            // Full: "new" is disabled with its reason.
            h.shell.d.overview.workspaces = Some((0, 8));
            h.apply(ultar_desktop::shell::Outcome::surfaces());
            let p = h.popup.as_ref().unwrap();
            let full = p
                .view
                .ui
                .is_disabled(p.view.node(&Target::Workspace(8)).unwrap());
            h.click_popup(Target::Workspace(0));
            other && new && full && h.popup.is_none()
        }
        _ => false,
    }
}

fn projection() -> bool {
    let mut h = h();
    let out = h.shell.on_shell_key(25);
    h.apply(out);
    let open = matches!(h.shell.d.open, Open::Projection(_));
    let out = h.shell.on_shell_key(25);
    h.apply(out);
    h.key_popup(Key::Enter);
    let stepped = h.take() == vec![Effect::DisplayMode(1)];
    let out = h.shell.on_shell_key(25);
    h.apply(out);
    h.click_popup(Target::Mode(2));
    let clicked = h.take() == vec![Effect::DisplayMode(2)];
    let out = h.shell.on_shell_key(25);
    h.apply(out);
    h.key_popup(Key::Escape);
    open && stepped && clicked && h.take().is_empty() && h.popup.is_none()
}

/// What is not implemented, with the reason (listed in desktop-shell.md as open points).
const NOT_YET: [(&str, &str); 2] = [
    (
        "shell.drive",
        "tested in tests/services.rs (the tile exists only while a drive is mounted)",
    ),
    (
        "qs.assistant",
        "the Assistant component is not installed: the button is absent (spec)",
    ),
];

#[test]
fn every_click_of_the_shell_boards_has_its_effect() {
    let mut missing = Vec::new();
    let mut covered = 0;
    for (board, action) in actions() {
        if ELSEWHERE.iter().any(|(p, _)| action.starts_with(p)) {
            continue;
        }
        if NOT_YET.iter().any(|(a, _)| *a == action) {
            continue;
        }
        if check(&action) {
            covered += 1;
        } else {
            missing.push(format!("{board}: {action}"));
        }
    }
    assert!(missing.is_empty(), "clicks without effect: {missing:?}");
    assert!(covered >= 40, "{covered}");
    let _ = PowerMode::Saver;
}
