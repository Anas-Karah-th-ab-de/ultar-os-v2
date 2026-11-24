//! From settings to an arrangement: which displays take part, which is the
//! main display, which mode each runs, where it sits. Pure; the same input
//! always gives the same setup, so a setup that the compositor already has
//! is recognised and not sent again.

use crate::values::{DisplayValues, ModeWord, PerDisplay};
use ultar_surface::proto::{Arrangement, OutputDetails, OutputMode, OutputSetup, Setup, Transform};

/// One connected display with its settings id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connected {
    /// What the compositor reported.
    pub details: OutputDetails,
    /// The id `settingsd` gave it.
    pub id: String,
}

/// The mode a display runs for its settings: the chosen resolution (else
/// the preferred one) at the chosen rate (0: the preferred rate at the
/// preferred resolution, else the highest; otherwise the nearest offered).
pub fn pick_mode(d: &OutputDetails, per: &PerDisplay) -> OutputMode {
    let (w, h) = per
        .resolution
        .unwrap_or((d.preferred.width, d.preferred.height));
    let candidates: Vec<OutputMode> = d
        .modes
        .iter()
        .copied()
        .filter(|m| m.width == w && m.height == h)
        .collect();
    if candidates.is_empty() {
        return d.preferred;
    }
    if per.refresh_hz <= 0.0 {
        if candidates.contains(&d.preferred) {
            return d.preferred;
        }
        return candidates
            .iter()
            .copied()
            .max_by_key(|m| m.refresh_mhz)
            .unwrap_or(d.preferred);
    }
    let want = (per.refresh_hz * 1000.0).round() as i64;
    candidates
        .iter()
        .copied()
        .min_by_key(|m| (i64::from(m.refresh_mhz) - want).abs())
        .unwrap_or(d.preferred)
}

/// The main display: the chosen one if connected, else the built-in one,
/// else the first.
pub fn main_display<'a>(
    connected: &'a [Connected],
    values: &DisplayValues,
) -> Option<&'a Connected> {
    connected
        .iter()
        .find(|c| !values.primary.is_empty() && c.id == values.primary)
        .or_else(|| connected.iter().find(|c| c.details.internal))
        .or_else(|| connected.first())
}

fn logical(mode: OutputMode, scale: u32) -> (i32, i32) {
    let s = f64::from(scale) / 1000.0;
    (
        (f64::from(mode.width) / s).ceil() as i32,
        (f64::from(mode.height) / s).ceil() as i32,
    )
}

fn overlap(a: &OutputSetup, b: &OutputSetup) -> bool {
    let (aw, ah) = logical(a.mode, a.scale_milli);
    let (bw, bh) = logical(b.mode, b.scale_milli);
    a.position.0 < b.position.0 + bw
        && b.position.0 < a.position.0 + aw
        && a.position.1 < b.position.1 + bh
        && b.position.1 < a.position.1 + ah
}

/// Places the included displays side by side from the main display when the
/// stored positions overlap (first use, a mode change that grew a display).
fn fix_positions(outputs: &mut [OutputSetup]) {
    let included: Vec<usize> = (0..outputs.len())
        .filter(|&i| outputs[i].included)
        .collect();
    let clash = included.iter().enumerate().any(|(n, &i)| {
        included[n + 1..]
            .iter()
            .any(|&j| overlap(&outputs[i], &outputs[j]))
    });
    if !clash {
        return;
    }
    let mut order = included;
    order.sort_by_key(|&i| (!outputs[i].primary, outputs[i].output));
    let mut x = 0;
    for i in order {
        outputs[i].position = (x, 0);
        x += logical(outputs[i].mode, outputs[i].scale_milli).0;
    }
}

fn scale_milli(percent: u32) -> u32 {
    let stepped = (percent + 12) / 25 * 25;
    stepped.clamp(100, 300) * 10
}

/// Which displays take part and which is the main one.
fn membership(connected: &[Connected], values: &DisplayValues) -> (Arrangement, Vec<bool>, usize) {
    let main = main_display(connected, values)
        .and_then(|m| connected.iter().position(|c| c.id == m.id))
        .unwrap_or(0);
    if connected.len() < 2 {
        return (Arrangement::Extend, vec![true; connected.len()], main);
    }
    match values.mode {
        ModeWord::Extend => (Arrangement::Extend, vec![true; connected.len()], main),
        ModeWord::Single => {
            let single = connected
                .iter()
                .position(|c| !values.single.is_empty() && c.id == values.single)
                .unwrap_or(main);
            let included = (0..connected.len()).map(|i| i == single).collect();
            (Arrangement::Single, included, single)
        }
        ModeWord::Mirror => {
            let chosen: Vec<bool> = connected
                .iter()
                .map(|c| values.mirror.contains(&c.id))
                .collect();
            if chosen.iter().filter(|&&b| b).count() < 2 {
                return (Arrangement::Mirror, vec![true; connected.len()], main);
            }
            let main = if chosen[main] {
                main
            } else {
                chosen.iter().position(|&b| b).unwrap_or(main)
            };
            (Arrangement::Mirror, chosen, main)
        }
    }
}

/// The setup for the connected displays and the settings.
pub fn plan(
    connected: &[Connected],
    values: &DisplayValues,
    serial: u32,
    provisional_ms: u32,
) -> Setup {
    let (arrangement, included, main) = membership(connected, values);
    let mut outputs: Vec<OutputSetup> = connected
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let per = values.displays.get(&c.id).cloned().unwrap_or_default();
            let position = if arrangement == Arrangement::Extend {
                per.position
            } else {
                (0, 0)
            };
            OutputSetup {
                output: c.details.id,
                included: included[i],
                primary: i == main,
                mode: pick_mode(&c.details, &per),
                scale_milli: scale_milli(per.scale),
                position,
                transform: per.transform,
            }
        })
        .collect();
    if arrangement == Arrangement::Extend {
        fix_positions(&mut outputs);
    }
    Setup {
        serial,
        arrangement,
        provisional_ms,
        outputs,
    }
}

/// True when two setups arrange the displays the same way (serial and
/// deadline aside).
pub fn same_arrangement(a: &Setup, b: &Setup) -> bool {
    a.arrangement == b.arrangement && a.outputs == b.outputs
}

/// The compositor draws only upright pictures today: a turned display is
/// shown upright (the setting stays; the service logs that it could not).
pub fn upright(setup: &mut Setup) -> bool {
    let mut turned = false;
    for o in &mut setup.outputs {
        if o.transform != Transform::Normal {
            o.transform = Transform::Normal;
            turned = true;
        }
    }
    turned
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
