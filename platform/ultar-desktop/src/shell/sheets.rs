//! Clicks in the sheets of the bar (CLICKS.md: QuickSettings, CalendarSheet, Overview,
//! Projection, all apps).

use super::bar::{launch, SETTINGS};
use super::keys::confirm_projection;
use super::{Effect, Outcome, Shell};
use crate::model::calendar::Month;
use crate::model::quick::{PowerMode, TileId};
use crate::model::search::{Command, Hit};
use crate::model::{Open, QuickPage};
use crate::view::Target;
use ultar_bus::messages::settings::SettingValue;
use ultar_surface::proto::WindowAction;
use ultar_ui::event::ActionKind;

pub(super) fn on_popup(s: &mut Shell, target: &Target, kind: &ActionKind) -> Outcome {
    match (target, kind) {
        (Target::Tile(id), ActionKind::Activated) => tile(s, *id),
        (Target::TileMore(id), ActionKind::Activated) => more(s, *id),
        (Target::QuickBack, ActionKind::Activated) => page(s, QuickPage::Tiles),
        (Target::PowerModeChoice(_), ActionKind::Selected(i)) => match PowerMode::ALL.get(*i) {
            Some(m) => setting(s, "power.mode", SettingValue::Text(m.word().into())),
            None => Outcome::none(),
        },
        (Target::Brightness, ActionKind::ValueChanged(v)) => brightness(s, *v),
        (Target::Battery, ActionKind::Activated) => launch(s, SETTINGS, &["power"]),
        (Target::Settings, ActionKind::Activated) => launch(s, SETTINGS, &[]),
        (Target::Lock, ActionKind::Activated) => {
            s.d.open = Open::None;
            Outcome::effect_and_close(Effect::Lock)
        }
        (Target::Power, ActionKind::Activated) => {
            s.d.open = Open::PowerMenu;
            Outcome::surfaces()
        }
        (Target::Dnd, ActionKind::Toggled(on)) => {
            setting(s, "notifications.do_not_disturb", SettingValue::Flag(*on))
        }
        _ => calendar_and_more(s, target, kind),
    }
}

fn calendar_and_more(s: &mut Shell, target: &Target, kind: &ActionKind) -> Outcome {
    let now = s.clock.now_ms();
    match (target, kind) {
        (Target::ClearAll, ActionKind::Activated) => {
            s.d.notes.clear(now);
            popup_only()
        }
        (Target::Undo, ActionKind::Activated) => {
            s.d.notes.undo(now);
            popup_only()
        }
        (Target::Dismiss(id), ActionKind::Activated) => {
            s.d.notes.dismiss(*id);
            popup_only()
        }
        (Target::PrevMonth | Target::NextMonth, ActionKind::Activated) => {
            let step = if *target == Target::PrevMonth { -1 } else { 1 };
            let today = s.local_time().map(|t| Month::of(t.date));
            let current = s.d.calendar.map(|c| c.0).or(today);
            if let Some(m) = current {
                s.d.calendar = Some((m.step(step), usize::MAX));
            }
            popup_only()
        }
        (Target::Days, ActionKind::Selected(i)) => {
            let today = s.local_time().map(|t| Month::of(t.date));
            if let Some(m) = s.d.calendar.map(|c| c.0).or(today) {
                s.d.calendar = Some((m, *i));
            }
            popup_only()
        }
        _ => overview_and_apps(s, target, kind),
    }
}

fn overview_and_apps(s: &mut Shell, target: &Target, kind: &ActionKind) -> Outcome {
    match (target, kind) {
        (Target::AppItem(app), ActionKind::Activated) => open_app(s, &app.clone()),
        (Target::Result(hit), ActionKind::Activated) => result(s, &hit.clone()),
        (Target::Window(id), ActionKind::Activated) => {
            s.d.open = Open::None;
            Outcome::effect_and_close(Effect::Window(*id, WindowAction::Activate))
        }
        (Target::CloseWindow(id), ActionKind::Activated) => {
            Outcome::effect(Effect::Window(*id, WindowAction::Close))
        }
        // The current workspace: back to it (the compositor has one workspace).
        (Target::Workspace(1), ActionKind::Activated) => s.close(),
        (Target::Mode(i), ActionKind::Activated) => {
            s.d.projection = *i;
            confirm_projection(s)
        }
        (Target::SearchField, ActionKind::Submitted) => first_result(s),
        _ => Outcome::none(),
    }
}

fn popup_only() -> Outcome {
    Outcome {
        effects: Vec::new(),
        bar: false,
        popup: true,
    }
}

fn page(s: &mut Shell, page: QuickPage) -> Outcome {
    s.d.open = Open::Quick(page);
    popup_only()
}

fn setting(s: &mut Shell, key: &str, value: SettingValue) -> Outcome {
    let _ = s;
    let mut out = popup_only();
    out.effects.push(Effect::Setting(key.to_owned(), value));
    out
}

/// qs.toggle / qs.powermode: the tiles that work through the settings service switch it;
/// the new state shows when the service announces it (`settings.changed`).
fn tile(s: &mut Shell, id: TileId) -> Outcome {
    let v = s.d.values;
    match id {
        TileId::NightLight => setting(
            s,
            "display.night_light.enabled",
            SettingValue::Flag(!v.night),
        ),
        TileId::Dark => {
            let theme = if v.dark { "light" } else { "dark" };
            setting(s, "appearance.theme", SettingValue::Text(theme.into()))
        }
        TileId::Dnd => setting(
            s,
            "notifications.do_not_disturb",
            SettingValue::Flag(!v.dnd),
        ),
        TileId::PowerMode => setting(
            s,
            "power.mode",
            SettingValue::Text(v.power.next().word().into()),
        ),
        // qs.wired: no switch, opens Settings > Network.
        TileId::Wired => launch(s, SETTINGS, &["network"]),
        // Wi-Fi, Bluetooth, airplane, VPN, Mesh: disabled until their service exists.
        _ => Outcome::none(),
    }
}

fn more(s: &mut Shell, id: TileId) -> Outcome {
    match id {
        TileId::PowerMode => page(s, QuickPage::PowerModes),
        _ => Outcome::none(),
    }
}

fn brightness(s: &mut Shell, value: f32) -> Outcome {
    let Some(display) = s.d.main_display.clone() else {
        return Outcome::none();
    };
    let level = value.round().clamp(0.0, 100.0) as i64;
    let mut out = setting(
        s,
        &format!("display.{display}.brightness"),
        SettingValue::Integer(level),
    );
    out.popup = false;
    out
}

pub(super) fn open_app(s: &mut Shell, app: &str) -> Outcome {
    let tiles = s.d.bar_apps();
    match tiles
        .iter()
        .find(|t| t.app.id == app)
        .and_then(|t| t.windows.last())
    {
        Some(w) => {
            let id = w.id;
            s.d.open = Open::None;
            Outcome::effect_and_close(Effect::Window(id, WindowAction::Activate))
        }
        None => launch(s, app, &[]),
    }
}

fn result(s: &mut Shell, hit: &Hit) -> Outcome {
    match hit {
        Hit::App(id) => open_app(s, id),
        Hit::Setting(panel) => launch(s, SETTINGS, &[panel]),
        // A calculation result stays; selecting it changes nothing (copy needs the clipboard).
        Hit::Calc(_) => Outcome::none(),
        Hit::Command(Command::Lock) => {
            s.d.open = Open::None;
            Outcome::effect_and_close(Effect::Lock)
        }
        Hit::Command(Command::Power) => {
            s.d.open = Open::PowerMenu;
            Outcome::surfaces()
        }
        Hit::Command(Command::DisplayMode) => {
            s.d.open = Open::Projection;
            Outcome::surfaces()
        }
        Hit::Command(Command::QuickSettings) => {
            s.d.open = Open::Quick(QuickPage::Tiles);
            Outcome::surfaces()
        }
    }
}

fn first_result(s: &mut Shell) -> Outcome {
    if let Open::Apps(category) = s.d.open {
        let list = crate::view::apps::listed(&s.d, category, &s.d.query, &s.tr);
        return match list.first() {
            Some(app) => open_app(s, &app.id.clone()),
            None => Outcome::none(),
        };
    }
    let hits = crate::model::search::search(&s.d.query, &s.d.catalog, &s.tr);
    match hits.first() {
        Some(hit) => result(s, &hit.clone()),
        None => Outcome::none(),
    }
}
