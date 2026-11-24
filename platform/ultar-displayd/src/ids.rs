//! What the settings service and the apps learn about a connected display:
//! the descriptor that `settingsd` turns into the display's stable id
//! (manufacturer, product, serial from the display's own data – the same
//! monitor keeps its settings on any port), and the `display.outputs` entry.

use ultar_bus::messages::display_service::{Mode, Output, MAX_MODES, MAX_NAME};
use ultar_bus::messages::settings::DisplayDescriptor;
use ultar_surface::proto::{OutputDetails, OutputMode};

/// Millimetres per inch × 10 (for tenths of an inch).
const MM_PER_TENTH_INCH: f64 = 2.54;

/// The descriptor `settings.set_displays` needs for one display.
pub fn descriptor(d: &OutputDetails) -> DisplayDescriptor {
    let identity = &d.identity;
    let known = !identity.manufacturer.is_empty();
    DisplayDescriptor {
        manufacturer: identity.manufacturer.clone(),
        product: if known {
            format!("{:04x}", identity.product)
        } else {
            String::new()
        },
        serial: if known && identity.serial != 0 {
            identity.serial.to_string()
        } else {
            String::new()
        },
        connector: d.connector.to_ascii_lowercase(),
        internal: d.internal,
        width_px: d.preferred.width,
        height_px: d.preferred.height,
        dpi: dpi(d),
        refresh_mhz: d.preferred.refresh_mhz,
    }
}

/// Pixel density from the preferred width and the physical width (0 = unknown).
pub fn dpi(d: &OutputDetails) -> u16 {
    if d.size_mm.0 == 0 || d.preferred.width == 0 {
        return 0;
    }
    let dpi = f64::from(d.preferred.width) * 25.4 / f64::from(d.size_mm.0);
    dpi.round().clamp(0.0, f64::from(u16::MAX)) as u16
}

/// Diagonal in tenths of an inch (0 = unknown).
pub fn diagonal_tenths(d: &OutputDetails) -> u16 {
    let (w, h) = (f64::from(d.size_mm.0), f64::from(d.size_mm.1));
    if w == 0.0 || h == 0.0 {
        return 0;
    }
    ((w * w + h * h).sqrt() / MM_PER_TENTH_INCH)
        .round()
        .clamp(0.0, f64::from(u16::MAX)) as u16
}

fn mode(m: OutputMode) -> Mode {
    Mode {
        width: m.width,
        height: m.height,
        refresh_mhz: m.refresh_mhz,
    }
}

/// The `display.outputs` entry of a display with its settings id.
pub fn output(d: &OutputDetails, id: &str) -> Output {
    let mut modes: Vec<Mode> = d.modes.iter().copied().map(mode).collect();
    // Largest first (area, then refresh rate); no doubles.
    modes.sort_by(|a, b| {
        (u64::from(b.width) * u64::from(b.height), b.refresh_mhz)
            .cmp(&(u64::from(a.width) * u64::from(a.height), a.refresh_mhz))
    });
    modes.dedup();
    modes.truncate(MAX_MODES);
    let mut name: String = d.identity.name.clone();
    while name.len() > MAX_NAME {
        name.pop();
    }
    Output {
        id: id.to_owned(),
        connector: d.connector.to_ascii_lowercase(),
        name,
        internal: d.internal,
        diagonal_tenths: diagonal_tenths(d),
        preferred: mode(d.preferred),
        current: mode(d.current),
        modes,
        hdr: false,
        brightness: false,
        profiles: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_surface::proto::{OutputId, OutputIdentity};

    pub(crate) fn details(i: u32, internal: bool) -> OutputDetails {
        let m = |w, h, r| OutputMode {
            width: w,
            height: h,
            refresh_mhz: r,
        };
        OutputDetails {
            id: OutputId(i),
            connector: format!("HDMI-A-{i}"),
            identity: OutputIdentity {
                manufacturer: "DEL".into(),
                product: 0xa0c4,
                serial: 7 + i,
                name: "DELL U2422H".into(),
            },
            internal,
            size_mm: (527, 296),
            preferred: m(1920, 1080, 60_000),
            current: m(1920, 1080, 60_000),
            modes: vec![
                m(1280, 720, 60_000),
                m(1920, 1080, 60_000),
                m(1920, 1080, 75_000),
            ],
        }
    }

    #[test]
    fn descriptors_carry_the_identity_or_only_the_connector() {
        let d = details(1, false);
        let desc = descriptor(&d);
        assert_eq!(
            (
                desc.manufacturer.as_str(),
                desc.product.as_str(),
                desc.serial.as_str()
            ),
            ("DEL", "a0c4", "8")
        );
        assert_eq!(desc.connector, "hdmi-a-1");
        assert_eq!(desc.dpi, 93);
        let mut bare = d.clone();
        bare.identity = OutputIdentity::default();
        bare.size_mm = (0, 0);
        let desc = descriptor(&bare);
        assert!(desc.manufacturer.is_empty() && desc.product.is_empty() && desc.serial.is_empty());
        assert_eq!(desc.dpi, 0);
        assert_eq!(diagonal_tenths(&bare), 0);
    }

    #[test]
    fn outputs_list_modes_largest_first() {
        let o = output(&details(1, false), "del-a0c4-8");
        assert_eq!(o.id, "del-a0c4-8");
        assert_eq!(o.diagonal_tenths, 238);
        assert_eq!(
            o.modes
                .iter()
                .map(|m| (m.width, m.refresh_mhz))
                .collect::<Vec<_>>(),
            vec![(1920, 75_000), (1920, 60_000), (1280, 60_000)]
        );
        let mut long = details(2, true);
        long.identity.name = "x".repeat(200);
        assert_eq!(output(&long, "a").name.len(), MAX_NAME);
    }
}
