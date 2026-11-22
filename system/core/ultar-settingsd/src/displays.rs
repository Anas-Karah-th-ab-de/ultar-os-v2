//! Display combinations: stable identifiers for displays and for the set of
//! connected displays, and the sensible default arrangement of a set that
//! is seen for the first time. Pure logic, no hardware access.
//!
//! Display settings are remembered per combination, so the dock at home
//! and the projector at the office each get their own arrangement back.

use crate::schema::{sanitize_segment, DefaultProvider, SettingDef};
use crate::value::Value;
use std::fmt;
use ultar_bus::messages::settings::DisplayDescriptor;

/// Most displays one combination may have.
pub const MAX_DISPLAYS: usize = 16;
/// Pixel density at which elements are shown at 100 %.
pub const BASE_DPI: u32 = 96;

/// One connected display with its stable identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayInfo {
    /// Stable identifier: the `<id>` of `display.<id>.*` keys.
    pub id: String,
    /// Built into the device.
    pub internal: bool,
    /// Preferred width in pixels (`0` = unknown).
    pub width_px: u32,
    /// Preferred height in pixels (`0` = unknown).
    pub height_px: u32,
    /// Pixel density (`0` = unknown).
    pub dpi: u16,
    /// Preferred refresh rate in millihertz (`0` = unknown).
    pub refresh_mhz: u32,
}

impl DisplayInfo {
    /// A display of which only the identifier is known.
    pub fn bare(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            internal: false,
            width_px: 0,
            height_px: 0,
            dpi: 0,
            refresh_mhz: 0,
        }
    }
}

/// Why a set of displays was not accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayError {
    /// More than [`MAX_DISPLAYS`] displays.
    TooMany,
}

impl fmt::Display for DisplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("too many displays")
    }
}

impl std::error::Error for DisplayError {}

/// The identifier a display gets from its own identification data:
/// manufacturer, product and serial number when the display reports any of
/// them, else the connector name. The same display therefore keeps its
/// identifier whichever port it is plugged into.
pub fn base_display_id(descriptor: &DisplayDescriptor) -> String {
    let parts = [
        &descriptor.manufacturer,
        &descriptor.product,
        &descriptor.serial,
    ];
    let joined = parts
        .iter()
        .filter_map(|part| sanitize_segment(part))
        .collect::<Vec<_>>()
        .join("-");
    sanitize_segment(&joined)
        .or_else(|| sanitize_segment(&descriptor.connector))
        .unwrap_or_else(|| "display".to_owned())
}

fn shorten(mut id: String, room: usize) -> String {
    let keep = crate::schema::MAX_SEGMENT_LEN.saturating_sub(room);
    while id.len() > keep {
        id.pop();
    }
    id
}

/// Gives every display its stable identifier, in request order.
///
/// Displays that cannot be told apart by their own data (two identical
/// monitors without serial numbers) get the connector appended, and a
/// running number as last resort, so identifiers are always distinct.
pub fn identify(descriptors: &[DisplayDescriptor]) -> Result<Vec<DisplayInfo>, DisplayError> {
    if descriptors.len() > MAX_DISPLAYS {
        return Err(DisplayError::TooMany);
    }
    let mut ids: Vec<String> = descriptors.iter().map(base_display_id).collect();
    let clashes =
        |ids: &[String], index: usize| ids.iter().filter(|id| **id == ids[index]).count() > 1;
    let ambiguous: Vec<usize> = (0..ids.len())
        .filter(|index| clashes(&ids, *index))
        .collect();
    for index in &ambiguous {
        if let Some(connector) = sanitize_segment(&descriptors[*index].connector) {
            let base = shorten(ids[*index].clone(), connector.len() + 1);
            ids[*index] = format!("{base}-{connector}");
        }
    }
    for index in 0..ids.len() {
        let mut number = 2;
        while ids[..index].contains(&ids[index]) {
            let base = shorten(base_display_id(&descriptors[index]), 4);
            ids[index] = format!("{base}-{number}");
            number += 1;
        }
    }
    Ok(descriptors
        .iter()
        .zip(ids)
        .map(|(descriptor, id)| DisplayInfo {
            id,
            internal: descriptor.internal,
            width_px: descriptor.width_px,
            height_px: descriptor.height_px,
            dpi: descriptor.dpi,
            refresh_mhz: descriptor.refresh_mhz,
        })
        .collect())
}

/// Stable identifier of a set of displays: independent of the order in
/// which they are listed or were plugged in.
pub fn combination_id<S: AsRef<str>>(display_ids: &[S]) -> String {
    let mut sorted: Vec<&str> = display_ids.iter().map(AsRef::as_ref).collect();
    sorted.sort_unstable();
    sorted.dedup();
    // FNV-1a, 64 bit, with a separator byte no identifier contains.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for id in sorted {
        for byte in id.bytes().chain([0u8]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("c-{hash:016x}")
}

/// Element size in percent recommended for a pixel density: the density
/// relative to [`BASE_DPI`], rounded to steps of 25 within 100–300.
pub fn recommended_scale(dpi: u16) -> i64 {
    if dpi == 0 {
        return 100;
    }
    let percent = u32::from(dpi) * 100 / BASE_DPI;
    let stepped = (percent + 12) / 25 * 25;
    i64::from(stepped.clamp(100, 300))
}

/// Order of displays in the default arrangement: the primary display
/// first, the others by identifier.
fn arrangement_order(displays: &[DisplayInfo]) -> Vec<&DisplayInfo> {
    let mut order: Vec<&DisplayInfo> = displays.iter().collect();
    order.sort_by(|a, b| b.internal.cmp(&a.internal).then_with(|| a.id.cmp(&b.id)));
    order
}

/// The default primary display: the built-in one, else the first by
/// identifier. `None` without displays.
pub fn default_primary(displays: &[DisplayInfo]) -> Option<&str> {
    arrangement_order(displays)
        .first()
        .map(|display| display.id.as_str())
}

/// Default position of a display: all displays side by side in one row,
/// the primary first, each as wide as its logical width at the recommended
/// element size (1920 when the width is unknown).
pub fn default_position(displays: &[DisplayInfo], id: &str) -> Option<(i64, i64)> {
    let mut x = 0i64;
    for display in arrangement_order(displays) {
        if display.id == id {
            return Some((x, 0));
        }
        let width = if display.width_px == 0 {
            1920
        } else {
            i64::from(display.width_px)
        };
        x += width * 100 / recommended_scale(display.dpi);
    }
    None
}

/// Defaults of the display settings computed from the connected displays.
#[derive(Clone, Copy, Debug)]
pub struct DisplayDefaults<'a>(pub &'a [DisplayInfo]);

impl DefaultProvider for DisplayDefaults<'_> {
    fn computed_default(&self, def: &SettingDef, instance: Option<&str>) -> Option<Value> {
        let displays = self.0;
        let Some(id) = instance else {
            return match def.key {
                "display.primary" | "display.single_display" => {
                    default_primary(displays).map(Value::text)
                }
                _ => None,
            };
        };
        let display = displays.iter().find(|display| display.id == id)?;
        match def.key {
            "display.<id>.scale" => {
                (display.dpi > 0).then(|| Value::Int(recommended_scale(display.dpi)))
            }
            "display.<id>.resolution" => (display.width_px > 0 && display.height_px > 0)
                .then(|| Value::Str(format!("{}x{}", display.width_px, display.height_px))),
            "display.<id>.refresh_rate" => (display.refresh_mhz > 0)
                .then(|| Value::Float(f64::from(display.refresh_mhz) / 1000.0)),
            "display.<id>.position_x" => default_position(displays, id).map(|(x, _)| Value::Int(x)),
            "display.<id>.position_y" => default_position(displays, id).map(|(_, y)| Value::Int(y)),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "displays_tests.rs"]
mod tests;
