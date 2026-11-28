//! Clicks in the sheets of the bar (CLICKS.md: QuickSettings, CalendarSheet, Overview,
//! Projection, all apps).

use super::bar::{launch, CALENDAR, SETTINGS, STORE};
use super::history::{open_record, remember};
use super::keys::confirm_projection;
use super::Opening;
use super::{Effect, Outcome, Shell};
use crate::model::calendar::Month;
use crate::model::quick::{self, Avail, PowerMode, TileId};
use crate::model::search::{Command, Hit};
use crate::model::services::Call;
use crate::model::{Open, QuickPage};
use crate::view::Target;
use ultar_bus::messages::settings::SettingValue;
use ultar_surface::proto::WindowAction;
use ultar_ui::event::ActionKind;

pub(super) fn on_popup(s: &mut Shell, target: &Target, kind: &ActionKind) -> Outcome {
    if let Some(out) = super::network::on_popup(s, target, kind) {
        return out;
    }
    match (target, kind) {
        (Target::Tile(id), ActionKind::Activated) => tile(s, *id),
        (Target::TileMore(id), ActionKind::Activated) => more(s, *id),
        (Target::QuickBack, ActionKind::Activated) => page(s, QuickPage::Tiles),
        (Target::PowerModeChoice(_), ActionKind::Selected(i)) => match PowerMode::ALL.get(*i) {
            Some(m) => setting(s, "power.mode", SettingValue::Text(m.word().into())),
            None => Outcome::none(),
        },
        (Target::Brightness, ActionKind::ValueChanged(v)) => brightness(s, *v),
        (Target::BtScan | Target::BtDevice(_) | Target::BtForget(_), _) => {
            bluetooth(s, target, kind)
        }
        // qs.volume / qs.output: the audio service.
        (Target::Volume, ActionKind::ValueChanged(v)) => {
            service(Some(Call::SetVolume(v.round().clamp(0.0, 100.0) as u8)))
        }
        (Target::Output, ActionKind::Activated) => {
            s.d.open = Open::OutputMenu;
            let mut out = Outcome::surfaces();
            out.effects.push(Effect::Service(Call::Outputs));
            out
        }
        // qs.shot: the screenshot service saves it into the person's Pictures.
        (Target::Screenshot, ActionKind::Activated) => {
            s.d.open = Open::None;
            Outcome::effect_and_close(Effect::Service(Call::Screenshot))
        }
        (Target::Battery, ActionKind::Activated) => {
            launch(s, SETTINGS, Opening::settings_page("power"))
        }
        (Target::Settings, ActionKind::Activated) => launch(s, SETTINGS, Opening::start()),
        // qs.access: the accessibility button opens Settings > Accessibility.
        (Target::AccessSettings, ActionKind::Activated) => {
            launch(s, SETTINGS, Opening::settings_page("accessibility"))
        }
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

/// qs.bluetooth.list: the devices page (bluetooth v2).
fn bluetooth(s: &Shell, target: &Target, kind: &ActionKind) -> Outcome {
    let call =
        match (target, kind) {
            (Target::BtScan, ActionKind::Toggled(on)) => Some(Call::BtScan(*on)),
            (Target::BtDevice(id), ActionKind::Activated) => {
                s.d.svc.bt_devices.iter().find(|d| &d.id == id).map(|d| {
                    match (d.paired, d.connected) {
                        (false, _) => Call::BtPair(d.id.clone()),
                        (true, connected) => Call::BtConnect(d.id.clone(), !connected),
                    }
                })
            }
            (Target::BtForget(id), ActionKind::Activated) => Some(Call::BtForget(id.clone())),
            _ => None,
        };
    service(call)
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
        // note.dismiss: gone here and at the notification service.
        (Target::Dismiss(id), ActionKind::Activated) => {
            s.d.notes.dismiss(*id);
            service(Some(Call::NoteDismiss(*id)))
        }
        // note.action: the button goes back to the sender through the service; the
        // notification is done with.
        (Target::NoteAction(id, index), ActionKind::Activated) => {
            let action = s.d.notes.action(*id, usize::from(*index));
            s.d.notes.dismiss(*id);
            let mut out = service(action.map(|a| Call::NoteAction(*id, a)));
            out.bar = true;
            out
        }
        // cal.event: the appointment in the Calendar app.
        (Target::Event(id), ActionKind::Activated) => {
            launch(s, CALENDAR, Opening::uri(format!("calendar://event/{id}")))
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
            let mut out = popup_only();
            out.effects.extend(s.day_request());
            out
        }
        _ => overview_and_apps(s, target, kind),
    }
}

fn overview_and_apps(s: &mut Shell, target: &Target, kind: &ActionKind) -> Outcome {
    match (target, kind) {
        (Target::AppItem(app), ActionKind::Activated) if s.d.places.picking => {
            super::desk::picked(s, &app.clone())
        }
        (Target::AppItem(app), ActionKind::Activated) => open_app(s, &app.clone()),
        (Target::Result(hit), ActionKind::Activated) => result(s, &hit.clone()),
        (Target::Window(id), ActionKind::Activated) => {
            s.d.open = Open::None;
            Outcome::effect_and_close(Effect::Window(*id, WindowAction::Activate))
        }
        (Target::CloseWindow(id), ActionKind::Activated) => {
            Outcome::effect(Effect::Window(*id, WindowAction::Close))
        }
        // overview.workspace: the shown one closes the overview, another is shown, the
        // last card adds one (the compositor bounds the number).
        (Target::Workspace(i), ActionKind::Activated) => {
            let current = s.d.overview.workspaces.map_or(0, |w| w.0);
            match u8::try_from(*i) {
                Ok(i) if i == current => s.close(),
                Ok(i) => Outcome::effect(Effect::Workspace(i)),
                Err(_) => Outcome::none(),
            }
        }
        (Target::Mode(i), ActionKind::Activated) => {
            s.d.open = Open::Projection(*i);
            confirm_projection(s)
        }
        (Target::SearchField, ActionKind::Submitted) if s.d.open == Open::Helper => {
            super::helper::send(s)
        }
        (Target::SearchField, ActionKind::Submitted) => first_result(s),
        (Target::HelperSend, ActionKind::Activated) => super::helper::send(s),
        (Target::HelperStop, ActionKind::Activated) => super::helper::stop(s),
        (Target::HelperSettings, ActionKind::Activated) => super::helper::settings(s),
        (Target::Assistant, ActionKind::Activated) => super::helper::toggle(s),
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

/// One call to a system service (`None`: the service never reported – nothing to do).
pub(super) fn service(call: Option<Call>) -> Outcome {
    let mut out = popup_only();
    out.effects.extend(call.map(Effect::Service));
    out
}

fn setting(s: &mut Shell, key: &str, value: SettingValue) -> Outcome {
    let _ = s;
    let mut out = popup_only();
    out.effects.push(Effect::Setting(key.to_owned(), value));
    out
}

/// qs.toggle / qs.powermode: the tiles that work through the settings service switch it;
/// the new state shows when the service announces it (`settings.changed`). A tile that
/// cannot switch now (no service, no device, airplane mode) is never a dead click: it
/// opens its page, which says why, or its place in Settings.
fn tile(s: &mut Shell, id: TileId) -> Outcome {
    let shown = quick::tile_of(id, &s.d.presence, &s.d.values, &s.d.svc);
    if shown.is_some_and(|t| matches!(t.avail, Avail::Disabled(_))) {
        return unavailable(s, id);
    }
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
        TileId::Wired => settings_of(s, TileId::Wired),
        // The radios through their services; the state shows when the service reports it.
        TileId::Wifi => match s.d.svc.net.as_ref() {
            Some(n) => service(Some(Call::SetWifi(!n.wifi_on))),
            None => more(s, TileId::Wifi),
        },
        TileId::Airplane => match s.d.svc.net.as_ref() {
            Some(n) => service(Some(Call::SetAirplane(!n.airplane))),
            None => settings_of(s, TileId::Airplane),
        },
        TileId::Bluetooth => match s.d.svc.bluetooth.as_ref() {
            Some(b) => service(Some(Call::SetBluetooth(!b.on))),
            None => more(s, TileId::Bluetooth),
        },
        TileId::Vpn => super::network::vpn_tile(s),
        TileId::Mesh => match s.d.svc.mesh.state.as_ref() {
            Some(m) => service(Some(Call::SetMesh(!m.enabled))),
            None => unavailable(s, id),
        },
    }
}

/// A tile that cannot switch now: its page (with the reason) or its place in Settings.
fn unavailable(s: &mut Shell, id: TileId) -> Outcome {
    match id {
        TileId::Wifi | TileId::Vpn | TileId::Bluetooth => more(s, id),
        _ => settings_of(s, id),
    }
}

/// The tile's area of the Settings app.
fn settings_of(s: &mut Shell, id: TileId) -> Outcome {
    launch(s, SETTINGS, Opening::settings_page(id.settings_area()))
}

/// qs.*.list: the arrow of a tile always opens its page (the page says what is missing);
/// tiles without a page of their own open their place in Settings.
fn more(s: &mut Shell, id: TileId) -> Outcome {
    if let Some(out) = super::network::more(s, id) {
        return out;
    }
    match id {
        TileId::PowerMode => page(s, QuickPage::PowerModes),
        // qs.bluetooth.list: the devices page asks the service for its list.
        TileId::Bluetooth => {
            let mut out = page(s, QuickPage::Bluetooth);
            if s.d.svc.bluetooth.is_some() {
                out.effects.push(Effect::Service(Call::BtDevices));
            }
            out
        }
        // The mesh tile's arrow: continue what is in front on another device (the phone's
        // way to it); with nobody to send to, its page in Settings.
        TileId::Mesh if crate::model::mesh::can_send(&s.d.svc) => super::handoff::send(s, None),
        _ => settings_of(s, id),
    }
}

fn brightness(s: &mut Shell, value: f32) -> Outcome {
    let Some(display) = s.d.svc.main_display.clone() else {
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
        None => launch(s, app, Opening::start()),
    }
}

fn result(s: &mut Shell, hit: &Hit) -> Outcome {
    match hit {
        Hit::App(id) => {
            let mut out = open_app(s, id);
            out.effects
                .extend(remember(s, crate::model::history::Record::App(id.clone())));
            out
        }
        // An app from the Store's catalogue that is not installed: open the Store on it.
        Hit::Store(id) => launch(s, STORE, Opening::store_page(id, false)),
        Hit::Setting(panel) => {
            let area = crate::model::search::panel_area(panel);
            let mut out = launch(s, SETTINGS, Opening::settings_page(area));
            out.effects.extend(remember(
                s,
                crate::model::history::Record::Setting((*panel).to_owned()),
            ));
            out
        }
        // A calculation result stays; selecting it changes nothing (copy needs the clipboard).
        Hit::Calc(_) => Outcome::none(),
        // A character goes into the search field (the picker's one field).
        Hit::Character(ch) => {
            s.d.overview.query.push_str(ch);
            Outcome::surfaces()
        }
        // A remembered entry runs again (and is remembered again, newest first).
        Hit::History(record) => open_record(s, record),
        Hit::Helper(text) => super::helper::ask_from_search(s, text),
        Hit::Command(Command::Lock) => {
            s.d.open = Open::None;
            Outcome::effect_and_close(Effect::Lock)
        }
        Hit::Command(Command::Power) => {
            s.d.open = Open::PowerMenu;
            Outcome::surfaces()
        }
        Hit::Command(Command::DisplayMode) => {
            s.d.open = Open::Projection(s.projection);
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
        let list = crate::view::apps::listed(&s.d, category, &s.d.overview.query, &s.tr);
        return match list.first() {
            Some(app) => open_app(s, &app.id.clone()),
            None => Outcome::none(),
        };
    }
    let hits = crate::model::search::search_in_with_history(
        &s.d.overview.query,
        &s.d.catalog,
        &s.tr,
        s.d.overview.sources,
        &s.d.overview.history,
        s.d.overview.store.as_ref(),
    );
    let mut out = match hits.first() {
        Some(hit) => result(s, &hit.clone()),
        None => Outcome::none(),
    };
    // The submitted search itself is remembered too.
    let query = s.d.overview.query.clone();
    out.effects
        .extend(remember(s, crate::model::history::Record::Search(query)));
    out
}
