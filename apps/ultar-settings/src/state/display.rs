//! State of the Displays page: the displays (settingsd values joined with the display
//! service's names and modes by display id) and the page-wide rows.

use ultar_bus::messages::display_service::{Mode, Output};
use ultar_bus::messages::settings::{ItemInfo, PanelInfo, SettingValue};

/// How several displays are used (`display.mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayMode {
    /// One desktop across all displays.
    #[default]
    Extend,
    /// The same picture on the chosen displays.
    Mirror,
    /// Only one display is on.
    Single,
}

impl DisplayMode {
    /// The modes in board order.
    pub const ALL: [DisplayMode; 3] = [
        DisplayMode::Extend,
        DisplayMode::Mirror,
        DisplayMode::Single,
    ];

    /// The settingsd choice id.
    pub fn id(self) -> &'static str {
        match self {
            DisplayMode::Extend => "extend",
            DisplayMode::Mirror => "mirror",
            DisplayMode::Single => "single",
        }
    }

    /// From the settingsd choice id.
    pub fn from_id(id: &str) -> Option<DisplayMode> {
        DisplayMode::ALL.into_iter().find(|m| m.id() == id)
    }
}

/// Orientation choices of settingsd, in the order offered.
pub const ORIENTATIONS: [&str; 4] = [
    "landscape",
    "portrait",
    "landscape_flipped",
    "portrait_flipped",
];
/// Element size choices (`display.<id>.scale`: 100–300 in steps of 25).
pub const SCALES: [i64; 9] = [100, 125, 150, 175, 200, 225, 250, 275, 300];

/// What a display can do, as the display service reports it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Caps {
    /// Offered modes (empty when the service does not report).
    pub modes: Vec<Mode>,
    /// Preferred mode (recommended).
    pub preferred: Option<Mode>,
    /// HDR supported.
    pub hdr: bool,
    /// Brightness can be set.
    pub brightness: bool,
    /// Colour profiles offered (ids).
    pub profiles: Vec<String>,
}

/// One connected display.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DisplayEntry {
    /// settingsd id.
    pub id: String,
    /// Number shown on the tile and by "show display numbers" (1-based).
    pub number: usize,
    /// Product name (empty: unknown).
    pub name: String,
    /// Built-in panel.
    pub internal: bool,
    /// Diagonal in tenths of an inch (0 = unknown).
    pub diagonal_tenths: u16,
    /// Position in the arrangement (horizontal, vertical).
    pub pos: (i64, i64),
    /// What the display service reported it can do.
    pub caps: Caps,
    /// `display.<id>.resolution` (`auto` or `WxH`).
    pub resolution: String,
    /// `display.<id>.scale` in percent.
    pub scale: i64,
    /// Recommended scale (computed default of settingsd).
    pub scale_default: i64,
    /// `display.<id>.refresh_rate` in Hz (0 = automatic).
    pub refresh: f64,
    /// `display.<id>.orientation`.
    pub orientation: String,
    /// `display.<id>.hdr`.
    pub hdr: bool,
    /// `display.<id>.brightness`.
    pub brightness: i64,
    /// `display.<id>.color_profile` (empty = automatic).
    pub profile: String,
}

impl DisplayEntry {
    /// The resolution in effect as `(w, h)`: the stored one, else the preferred mode.
    pub fn resolution_px(&self) -> Option<(u32, u32)> {
        parse_resolution(&self.resolution).or(self.caps.preferred.map(|m| (m.width, m.height)))
    }

    /// The distinct resolutions offered, largest first.
    pub fn resolutions(&self) -> Vec<(u32, u32)> {
        let mut list: Vec<(u32, u32)> = Vec::new();
        for m in &self.caps.modes {
            if !list.contains(&(m.width, m.height)) {
                list.push((m.width, m.height));
            }
        }
        if let Some(current) = self.resolution_px() {
            if !list.contains(&current) {
                list.insert(0, current);
            }
        }
        list
    }

    /// Refresh rates (mHz) offered at the resolution in effect, highest first.
    pub fn refresh_rates(&self) -> Vec<u32> {
        let res = self.resolution_px();
        let mut rates: Vec<u32> = self
            .caps
            .modes
            .iter()
            .filter(|m| Some((m.width, m.height)) == res)
            .map(|m| m.refresh_mhz)
            .collect();
        rates.sort_unstable_by(|a, b| b.cmp(a));
        rates.dedup();
        if rates.is_empty() && self.refresh > 0.0 {
            rates.push((self.refresh * 1000.0).round() as u32);
        }
        rates
    }
}

/// `WxH` → pixels.
pub fn parse_resolution(text: &str) -> Option<(u32, u32)> {
    let (w, h) = text.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// The whole page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DisplayState {
    /// Displays in number order.
    pub displays: Vec<DisplayEntry>,
    /// Index of the display whose card is shown.
    pub selected: usize,
    /// `display.mode`.
    pub mode: DisplayMode,
    /// `display.mirror_displays`.
    pub mirror: Vec<String>,
    /// `display.single_display`.
    pub single: String,
    /// `display.primary`.
    pub primary: String,
    /// `display.night_light.enabled`.
    pub night: bool,
    /// `display.night_light.schedule`.
    pub night_schedule: String,
    /// `display.night_light.temperature`.
    pub night_temperature: i64,
    /// `display.remember_window_positions`.
    pub remember: bool,
    /// `bar.displays` = `all`.
    pub bar_all: bool,
    /// `display.lid_close_with_external` = `stay_on`.
    pub lid_stay: bool,
    /// `display.auto_brightness` – present only when settingsd offers it (light sensor).
    pub auto_brightness: Option<bool>,
    /// The display service reported the displays.
    pub outputs_known: bool,
    /// settingsd answered.
    pub settings_known: bool,
}

fn text(v: &SettingValue) -> Option<&str> {
    match v {
        SettingValue::Text(t) => Some(t),
        _ => None,
    }
}

fn int(v: &SettingValue) -> Option<i64> {
    match v {
        SettingValue::Integer(i) => Some(*i),
        _ => None,
    }
}

fn flag(v: &SettingValue) -> Option<bool> {
    match v {
        SettingValue::Flag(b) => Some(*b),
        _ => None,
    }
}

fn float(v: &SettingValue) -> Option<f64> {
    match v {
        SettingValue::Decimal(f) => Some(*f),
        SettingValue::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

/// `display.<id>.<field>` → (id, field).
fn split_display_key(key: &str) -> Option<(&str, &str)> {
    let rest = key.strip_prefix("display.")?;
    let (id, field) = rest.rsplit_once('.')?;
    if id.contains('.') || id.contains('<') || id == "night_light" {
        return None;
    }
    Some((id, field))
}

fn entry_mut<'a>(list: &'a mut Vec<DisplayEntry>, id: &str) -> &'a mut DisplayEntry {
    if let Some(i) = list.iter().position(|d| d.id == id) {
        return &mut list[i];
    }
    list.push(DisplayEntry {
        id: id.to_owned(),
        orientation: ORIENTATIONS[0].to_owned(),
        scale: 100,
        scale_default: 100,
        brightness: 80,
        ..DisplayEntry::default()
    });
    let last = list.len() - 1;
    &mut list[last]
}

fn apply_display_item(list: &mut Vec<DisplayEntry>, item: &ItemInfo) {
    let Some((id, field)) = split_display_key(&item.key) else {
        return;
    };
    let d = entry_mut(list, id);
    let v = &item.value;
    match field {
        "resolution" => d.resolution = text(v).unwrap_or("auto").to_owned(),
        "scale" => {
            d.scale = int(v).unwrap_or(100);
            d.scale_default = int(&item.default).unwrap_or(d.scale);
        }
        "refresh_rate" => d.refresh = float(v).unwrap_or(0.0),
        "orientation" => d.orientation = text(v).unwrap_or(ORIENTATIONS[0]).to_owned(),
        "hdr" => d.hdr = flag(v).unwrap_or(false),
        "brightness" => d.brightness = int(v).unwrap_or(80),
        "color_profile" => d.profile = text(v).unwrap_or("").to_owned(),
        "position_x" => d.pos.0 = int(v).unwrap_or(0),
        "position_y" => d.pos.1 = int(v).unwrap_or(0),
        _ => {}
    }
}

impl DisplayState {
    /// Reads the settingsd panels `display` and `desktop_bar`.
    pub fn apply_panels(&mut self, panels: &[PanelInfo]) {
        let mut displays = Vec::new();
        for item in panels.iter().flat_map(|p| p.items.iter()) {
            let v = &item.value;
            match item.key.as_str() {
                "display.mode" => {
                    self.mode = text(v).and_then(DisplayMode::from_id).unwrap_or_default()
                }
                "display.mirror_displays" => {
                    if let SettingValue::List(l) = v {
                        self.mirror = l.clone();
                    }
                }
                "display.single_display" => self.single = text(v).unwrap_or("").to_owned(),
                "display.primary" => self.primary = text(v).unwrap_or("").to_owned(),
                "display.night_light.enabled" => self.night = flag(v).unwrap_or(false),
                "display.night_light.schedule" => {
                    self.night_schedule = text(v).unwrap_or("sunset_to_sunrise").to_owned()
                }
                "display.night_light.temperature" => {
                    self.night_temperature = int(v).unwrap_or(4000)
                }
                "display.remember_window_positions" => self.remember = flag(v).unwrap_or(true),
                "display.lid_close_with_external" => self.lid_stay = text(v) == Some("stay_on"),
                "bar.displays" => self.bar_all = text(v) == Some("all"),
                "display.auto_brightness" => self.auto_brightness = flag(v),
                _ if !item.template => apply_display_item(&mut displays, item),
                _ => {}
            }
        }
        self.settings_known = true;
        self.merge_displays(displays);
    }

    fn merge_displays(&mut self, mut fresh: Vec<DisplayEntry>) {
        for d in &mut fresh {
            if let Some(old) = self.displays.iter().find(|o| o.id == d.id) {
                d.name.clone_from(&old.name);
                d.internal = old.internal;
                d.diagonal_tenths = old.diagonal_tenths;
                d.caps.clone_from(&old.caps);
            }
        }
        self.displays = fresh;
        self.renumber();
    }

    /// Joins the display service's report by id.
    pub fn apply_outputs(&mut self, outputs: &[Output]) {
        for o in outputs {
            let d = entry_mut(&mut self.displays, &o.id);
            d.name.clone_from(&o.name);
            d.internal = o.internal;
            d.diagonal_tenths = o.diagonal_tenths;
            d.caps = Caps {
                modes: o.modes.clone(),
                preferred: Some(o.preferred),
                hdr: o.hdr,
                brightness: o.brightness,
                profiles: o.profiles.clone(),
            };
        }
        self.outputs_known = true;
        self.renumber();
    }

    /// Numbers: built-in displays first, then by id (stable while the set stays).
    fn renumber(&mut self) {
        let selected = self.displays.get(self.selected).map(|d| d.id.clone());
        self.displays
            .sort_by(|a, b| b.internal.cmp(&a.internal).then_with(|| a.id.cmp(&b.id)));
        for (i, d) in self.displays.iter_mut().enumerate() {
            d.number = i + 1;
        }
        self.selected = selected
            .and_then(|id| self.displays.iter().position(|d| d.id == id))
            .unwrap_or(0);
    }

    /// The display whose card is shown.
    pub fn current(&self) -> Option<&DisplayEntry> {
        self.displays.get(self.selected)
    }

    /// Indices of the displays in physical order (left to right by position).
    pub fn arrangement(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.displays.len()).collect();
        order.sort_by_key(|&i| (self.displays[i].pos, self.displays[i].number));
        order
    }

    /// The id of the main display (settingsd value, else the first built-in/numbered one).
    pub fn primary_id(&self) -> Option<&str> {
        if self.displays.iter().any(|d| d.id == self.primary) {
            return Some(&self.primary);
        }
        self.displays.first().map(|d| d.id.as_str())
    }
}

#[cfg(test)]
#[path = "display_tests.rs"]
mod tests;
