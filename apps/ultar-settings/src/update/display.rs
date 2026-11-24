//! Clicks of the Displays page (`display.*`, CLICKS.md SettingsDisplay; DESKTOP-SPEC §8, §13).

use super::{load_panels, set, Ctl, Outcome};
use crate::action::Act;
use crate::service::Ports;
use crate::state::display::{DisplayMode, ORIENTATIONS, SCALES};
use crate::state::{State, SubPage};
use ultar_bus::messages::settings::SettingValue;
use ultar_bus::{Text, TextArg};
use ultar_ui::event::ActionKind;
use ultar_ui::ui::NodeId;
use ultar_ui::widgets::Tone;

/// Reads settings and the display service.
pub fn load(state: &mut State, ports: &mut Ports) {
    match ports.display.outputs() {
        Ok(outputs) => state.display.apply_outputs(&outputs),
        Err(_) => state.display.outputs_known = false,
    }
}

/// `true` for the actions of this page.
pub fn owns(act: Act) -> bool {
    matches!(
        act,
        Act::DisplaySelect
            | Act::DisplayIdentify
            | Act::DisplayDetect
            | Act::DisplayMode
            | Act::DisplayMirrorPick(_)
            | Act::DisplaySingle
            | Act::DisplayNightlight
            | Act::DisplayNightlightMore
            | Act::DisplayRemember
            | Act::DisplayBar
            | Act::DisplayLid
            | Act::DisplayMakePrimary
            | Act::DisplayResolution
            | Act::DisplayScale
            | Act::DisplayRefresh
            | Act::DisplayOrientation
            | Act::DisplayHdr
            | Act::DisplayProfile
            | Act::DisplayBrightness
            | Act::DisplayAutoBrightness
            | Act::NightSchedule
            | Act::NightWarmth
            | Act::NightBack
    )
}

fn current_key(state: &State, field: &str) -> Option<String> {
    state
        .display
        .current()
        .map(|d| format!("display.{}.{field}", d.id))
}

/// Handles a click.
pub fn handle(c: &mut Ctl<'_>, act: Act, node: NodeId, kind: ActionKind) -> Outcome {
    use ActionKind::{Activated, Selected, Toggled, ValueChanged};
    match (act, kind) {
        (Act::DisplaySelect, Selected(i)) => select(c.state, i),
        (Act::DisplaySelect, ActionKind::Arranged(_)) => arranged(c, node),
        (Act::DisplayIdentify, Activated) => identify(c),
        (Act::DisplayDetect, Activated) => detect(c, node),
        (Act::DisplayMode, Selected(i)) => mode(c, i),
        (Act::DisplayMirrorPick(i), Toggled(on)) => mirror(c, i, on),
        (Act::DisplaySingle, Selected(i)) => {
            let id = c.state.display.displays.get(i).map(|d| d.id.clone());
            id.map_or_else(Outcome::default, |id| {
                c.state.display.single.clone_from(&id);
                set(c, "display.single_display", SettingValue::Text(id))
            })
        }
        (Act::DisplayNightlight, Toggled(on)) => {
            c.state.display.night = on;
            set(c, "display.night_light.enabled", SettingValue::Flag(on))
        }
        (Act::DisplayNightlightMore, Activated) => sub(c.state, SubPage::NightLight),
        (Act::NightBack, Activated) => sub(c.state, SubPage::None),
        (Act::DisplayMakePrimary, Activated) => primary(c),
        (Act::DisplayBrightness, ValueChanged(v)) => brightness(c, v),
        (a, Selected(i)) if is_per_display(a) => per_display_choice(c, a, i),
        (Act::DisplayHdr, Toggled(on)) => per_display(c, "hdr", SettingValue::Flag(on)),
        _ => general(c, act, kind),
    }
}

/// The page-wide rows (night light page, remember, bar, lid, automatic brightness).
fn general(c: &mut Ctl<'_>, act: Act, kind: ActionKind) -> Outcome {
    use ActionKind::{Selected, Toggled, ValueChanged};
    match (act, kind) {
        (Act::NightSchedule, Selected(i)) => {
            let ids = ["sunset_to_sunrise", "custom", "always"];
            let id = ids.get(i).copied().unwrap_or(ids[0]);
            c.state.display.night_schedule = id.to_owned();
            set(
                c,
                "display.night_light.schedule",
                SettingValue::Text(id.to_owned()),
            )
        }
        (Act::NightWarmth, ValueChanged(v)) => {
            c.state.display.night_temperature = v.round() as i64;
            set(
                c,
                "display.night_light.temperature",
                SettingValue::Integer(v.round() as i64),
            )
        }
        (Act::DisplayRemember, Toggled(on)) => {
            c.state.display.remember = on;
            set(
                c,
                "display.remember_window_positions",
                SettingValue::Flag(on),
            )
        }
        (Act::DisplayBar, Selected(i)) => {
            c.state.display.bar_all = i == 1;
            let v = if i == 1 { "all" } else { "primary" };
            set(c, "bar.displays", SettingValue::Text(v.to_owned()))
        }
        (Act::DisplayLid, Selected(i)) => {
            c.state.display.lid_stay = i == 0;
            let v = if i == 0 { "stay_on" } else { "suspend" };
            set(
                c,
                "display.lid_close_with_external",
                SettingValue::Text(v.to_owned()),
            )
        }
        (Act::DisplayAutoBrightness, Toggled(on)) => {
            c.state.display.auto_brightness = Some(on);
            set(c, "display.auto_brightness", SettingValue::Flag(on))
        }
        _ => Outcome::default(),
    }
}

fn is_per_display(a: Act) -> bool {
    matches!(
        a,
        Act::DisplayResolution
            | Act::DisplayScale
            | Act::DisplayRefresh
            | Act::DisplayOrientation
            | Act::DisplayProfile
    )
}

fn sub(state: &mut State, page: SubPage) -> Outcome {
    state.sub = page;
    Outcome::rebuild()
}

fn select(state: &mut State, i: usize) -> Outcome {
    let order = state.display.arrangement();
    match order.get(i) {
        Some(&index) if index != state.display.selected => {
            state.display.selected = index;
            Outcome::rebuild()
        }
        _ => Outcome::default(),
    }
}

fn identify(c: &mut Ctl<'_>) -> Outcome {
    let order = c
        .state
        .display
        .displays
        .iter()
        .map(|d| d.id.clone())
        .collect();
    match c.ports.display.identify(order) {
        Ok(_) => Outcome::default(),
        Err(e) => Outcome::error(&e),
    }
}

fn detect(c: &mut Ctl<'_>, node: NodeId) -> Outcome {
    let _ = c.ui.set_loading(node, true);
    let result = c.ports.display.detect();
    let _ = c.ui.set_loading(node, false);
    match result {
        Ok(reply) => {
            load_panels(c.state, c.ports);
            load(c.state, c.ports);
            let text = if reply.new == 0 {
                Text::key("app.settings.display.detect.none")
            } else {
                Text::key("app.settings.display.detect.found")
                    .with(TextArg::Number(i64::from(reply.found)))
            };
            Outcome {
                toast: Some((Tone::Info, text)),
                ..Outcome::rebuild()
            }
        }
        Err(e) => Outcome::error(&e),
    }
}

fn mode(c: &mut Ctl<'_>, i: usize) -> Outcome {
    let Some(&mode) = DisplayMode::ALL.get(i) else {
        return Outcome::default();
    };
    if mode == c.state.display.mode {
        return Outcome::default();
    }
    c.state.display.mode = mode;
    let out = set(c, "display.mode", SettingValue::Text(mode.id().to_owned()));
    Outcome {
        rebuild: true,
        ..out
    }
}

fn mirror(c: &mut Ctl<'_>, i: usize, on: bool) -> Outcome {
    let st = &mut c.state.display;
    let Some(id) = st.displays.get(i).map(|d| d.id.clone()) else {
        return Outcome::default();
    };
    if st.mirror.is_empty() {
        st.mirror = st.displays.iter().map(|d| d.id.clone()).collect();
    }
    st.mirror.retain(|m| *m != id);
    if on {
        st.mirror.push(id);
    }
    let list = st.mirror.clone();
    set(c, "display.mirror_displays", SettingValue::List(list))
}

fn primary(c: &mut Ctl<'_>) -> Outcome {
    let Some(id) = c.state.display.current().map(|d| d.id.clone()) else {
        return Outcome::default();
    };
    c.state.display.primary.clone_from(&id);
    let out = set(c, "display.primary", SettingValue::Text(id));
    Outcome {
        rebuild: true,
        ..out
    }
}

fn brightness(c: &mut Ctl<'_>, v: f32) -> Outcome {
    let value = v.round().clamp(0.0, 100.0) as i64;
    let selected = c.state.display.selected;
    if let Some(d) = c.state.display.displays.get_mut(selected) {
        d.brightness = value;
    }
    per_display(c, "brightness", SettingValue::Integer(value))
}

/// A display was dragged on the arrangement canvas: the new left-to-right order of the
/// tiles becomes the displays' positions (side by side, top edges level, in logical pixels).
fn arranged(c: &mut Ctl<'_>, node: NodeId) -> Outcome {
    let order = c.state.display.arrangement();
    let mut tiles: Vec<(f32, usize)> = order
        .iter()
        .enumerate()
        .filter_map(|(n, &i)| {
            let r = c.ui.arranged_rect(node, n).ok()?;
            Some((r.x + r.w / 2.0, i))
        })
        .collect();
    tiles.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut x = 0i64;
    let mut out = Outcome::default();
    for (_, i) in tiles {
        let Some(d) = c.state.display.displays.get_mut(i) else {
            continue;
        };
        let (w, _) = d.resolution_px().unwrap_or((1920, 1080));
        let logical = i64::from(w) * 100 / d.scale.max(25);
        let (id, moved) = (d.id.clone(), d.pos != (x, 0));
        d.pos = (x, 0);
        if moved {
            let key = format!("display.{id}.position_x");
            let o = set(c, &key, SettingValue::Integer(x));
            out.toast = out.toast.or(o.toast);
            let key = format!("display.{id}.position_y");
            let o = set(c, &key, SettingValue::Integer(0));
            out.toast = out.toast.or(o.toast);
        }
        x += logical;
    }
    out.rebuild = true;
    out
}

fn per_display(c: &mut Ctl<'_>, field: &str, value: SettingValue) -> Outcome {
    match current_key(c.state, field) {
        Some(key) => set(c, &key, value),
        None => Outcome::default(),
    }
}

fn per_display_choice(c: &mut Ctl<'_>, act: Act, i: usize) -> Outcome {
    let Some(d) = c.state.display.current().cloned() else {
        return Outcome::default();
    };
    let (field, value) = match act {
        Act::DisplayResolution => match d.resolutions().get(i) {
            Some((w, h)) => ("resolution", SettingValue::Text(format!("{w}x{h}"))),
            None => return Outcome::default(),
        },
        Act::DisplayScale => match SCALES.get(i) {
            Some(s) => ("scale", SettingValue::Integer(*s)),
            None => return Outcome::default(),
        },
        Act::DisplayRefresh => match d.refresh_rates().get(i) {
            Some(mhz) => (
                "refresh_rate",
                SettingValue::Decimal(f64::from(*mhz) / 1000.0),
            ),
            None => return Outcome::default(),
        },
        Act::DisplayOrientation => match ORIENTATIONS.get(i) {
            Some(o) => ("orientation", SettingValue::Text((*o).to_owned())),
            None => return Outcome::default(),
        },
        Act::DisplayProfile => {
            let id = if i == 0 {
                Some(String::new())
            } else {
                d.caps.profiles.get(i - 1).cloned()
            };
            match id {
                Some(id) => ("color_profile", SettingValue::Text(id)),
                None => return Outcome::default(),
            }
        }
        _ => return Outcome::default(),
    };
    let out = per_display(c, field, value);
    if out.toast.is_none() {
        load_panels(c.state, c.ports);
    }
    out
}
