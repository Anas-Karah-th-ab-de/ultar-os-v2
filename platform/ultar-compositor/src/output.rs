//! Outputs (displays) and how they are arranged: extend, mirror, single;
//! one primary output; a scale per output.

use crate::backend::BackendOutput;
use crate::geom::Rect;
use std::collections::BTreeMap;
use ultar_surface::proto::{OutputId, OutputInfo, Transform};

/// Smallest and largest scale (DESKTOP-SPEC §5.2: 100–300 %).
pub const MIN_SCALE_MILLI: u32 = 1000;
/// Largest scale.
pub const MAX_SCALE_MILLI: u32 = 3000;
/// Step of user-chosen scales (25 %).
pub const SCALE_STEP_MILLI: u32 = 250;

/// How several outputs are used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayMode {
    /// One large desktop over all outputs.
    Extend,
    /// Every output shows what the primary shows.
    Mirror,
    /// Only this output is on.
    Single(OutputId),
}

/// One output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    /// Id given to clients.
    pub id: OutputId,
    /// Id of the backend display.
    pub hal: u32,
    /// Connector name.
    pub name: String,
    /// Width in device pixels.
    pub width: u32,
    /// Height in device pixels.
    pub height: u32,
    /// Refresh rate in millihertz.
    pub refresh_millihertz: u32,
    /// Device pixels per logical point × 1000 (in effect).
    pub scale_milli: u32,
    /// The scale the user chose (mirror mode overrides it for a while).
    pub user_scale_milli: u32,
    /// Position in the global layout (logical points).
    pub x: i32,
    /// Position in the global layout (logical points).
    pub y: i32,
    /// The primary output.
    pub primary: bool,
    /// Shown (false: switched off by the display mode).
    pub enabled: bool,
    /// What the display service chose beyond mode and scale.
    pub placement: Placement,
}

/// Where an output sits and whether it takes part (set by the display
/// service; the defaults arrange outputs left to right, all included).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// Fixed position in logical points (`None`: placed automatically).
    pub position: Option<(i32, i32)>,
    /// Turn of the picture (only `Normal` is drawn today).
    pub transform: Transform,
    /// Part of the arrangement (false: switched off, e.g. not mirrored).
    pub included: bool,
}

impl Default for Placement {
    fn default() -> Self {
        Placement {
            position: None,
            transform: Transform::Normal,
            included: true,
        }
    }
}

impl Output {
    /// Scale as a factor.
    pub fn scale(&self) -> f64 {
        f64::from(self.scale_milli) / 1000.0
    }

    /// Size in logical points.
    pub fn logical_size(&self) -> (i32, i32) {
        let s = self.scale();
        (
            (f64::from(self.width) / s).ceil() as i32,
            (f64::from(self.height) / s).ceil() as i32,
        )
    }

    /// Area in the global layout (logical points).
    pub fn logical_rect(&self) -> Rect {
        let (w, h) = self.logical_size();
        Rect::new(self.x, self.y, w, h)
    }

    /// Time between two frames in microseconds.
    pub fn frame_interval_us(&self) -> u64 {
        1_000_000_000 / u64::from(self.refresh_millihertz.max(1000))
    }

    /// What clients learn about the output.
    pub fn info(&self) -> OutputInfo {
        OutputInfo {
            id: self.id,
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            scale_milli: self.scale_milli,
            refresh_millihertz: self.refresh_millihertz,
            primary: self.primary,
            name: self.name.clone(),
        }
    }
}

/// Suggests a scale from the pixel density: 96 dpi = 100 %, rounded to
/// 25 % steps, 100–300 %. Without a physical size: 100 %.
pub fn suggested_scale(width_px: u32, width_mm: u32) -> u32 {
    if width_mm == 0 {
        return MIN_SCALE_MILLI;
    }
    let dpi = f64::from(width_px) * 25.4 / f64::from(width_mm);
    let raw = dpi / 96.0 * 1000.0;
    let stepped = (raw / f64::from(SCALE_STEP_MILLI)).round() as u32 * SCALE_STEP_MILLI;
    stepped.clamp(MIN_SCALE_MILLI, MAX_SCALE_MILLI)
}

/// All outputs and their arrangement.
#[derive(Clone, Debug)]
pub struct Layout {
    pub(crate) outputs: Vec<Output>,
    pub(crate) mode: DisplayMode,
    next_id: u32,
    /// What the backend reported about each output (modes, identity).
    pub(crate) details: BTreeMap<OutputId, BackendOutput>,
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            outputs: Vec::new(),
            mode: DisplayMode::Extend,
            next_id: 0,
            details: BTreeMap::new(),
        }
    }
}

impl Layout {
    /// No outputs, extend mode.
    pub fn new() -> Layout {
        Layout::default()
    }

    /// Adds a connected display; the first one becomes primary.
    pub fn add(
        &mut self,
        hal: u32,
        name: &str,
        size: (u32, u32),
        refresh_millihertz: u32,
        width_mm: u32,
    ) -> OutputId {
        let id = OutputId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        let primary = self.outputs.is_empty();
        self.outputs.push(Output {
            id,
            hal,
            name: name.chars().take(32).collect(),
            width: size.0,
            height: size.1,
            refresh_millihertz: refresh_millihertz.max(1000),
            scale_milli: suggested_scale(size.0, width_mm),
            user_scale_milli: suggested_scale(size.0, width_mm),
            x: 0,
            y: 0,
            primary,
            enabled: true,
            placement: Placement::default(),
        });
        self.arrange();
        id
    }

    /// Adds a connected display with everything the backend knows of it.
    pub fn add_output(&mut self, b: &BackendOutput) -> OutputId {
        let id = self.add(
            b.hal,
            &b.name,
            (b.width, b.height),
            b.refresh_millihertz,
            b.width_mm,
        );
        self.details.insert(id, b.clone());
        id
    }

    /// Removes a display; another becomes primary if needed.
    pub fn remove(&mut self, id: OutputId) -> bool {
        let before = self.outputs.len();
        self.outputs.retain(|o| o.id != id);
        self.details.remove(&id);
        if self.mode == DisplayMode::Single(id) {
            self.mode = DisplayMode::Extend;
        }
        if !self.outputs.iter().any(|o| o.primary) {
            if let Some(first) = self.outputs.first_mut() {
                first.primary = true;
            }
        }
        self.arrange();
        before != self.outputs.len()
    }

    /// Sets the scale of an output (rounded to 25 % steps, 100–300 %).
    pub fn set_scale(&mut self, id: OutputId, scale_milli: u32) -> bool {
        let stepped = ((scale_milli + SCALE_STEP_MILLI / 2) / SCALE_STEP_MILLI * SCALE_STEP_MILLI)
            .clamp(MIN_SCALE_MILLI, MAX_SCALE_MILLI);
        let Some(o) = self.outputs.iter_mut().find(|o| o.id == id) else {
            return false;
        };
        o.scale_milli = stepped;
        o.user_scale_milli = stepped;
        self.arrange();
        true
    }

    /// Makes `id` the primary output.
    pub fn set_primary(&mut self, id: OutputId) -> bool {
        if !self.outputs.iter().any(|o| o.id == id) {
            return false;
        }
        for o in &mut self.outputs {
            o.primary = o.id == id;
        }
        self.arrange();
        true
    }

    /// Switches extend / mirror / single.
    pub fn set_mode(&mut self, mode: DisplayMode) -> bool {
        if let DisplayMode::Single(id) = mode {
            if !self.outputs.iter().any(|o| o.id == id) {
                return false;
            }
        }
        self.mode = mode;
        self.arrange();
        true
    }

    /// The display mode.
    pub fn mode(&self) -> DisplayMode {
        self.mode
    }

    pub(crate) fn arrange(&mut self) {
        // Primary first, then the others in connection order, left to right
        // (physical arrangement), unless the display service placed them.
        self.outputs.sort_by_key(|o| (!o.primary, o.id));
        for o in &mut self.outputs {
            o.scale_milli = o.user_scale_milli;
        }
        let primary_size = self.outputs.first().map(|o| o.logical_size());
        let placed = self
            .outputs
            .iter()
            .filter(|o| o.placement.included)
            .all(|o| o.placement.position.is_some());
        let mut x = 0;
        for o in &mut self.outputs {
            o.enabled = o.placement.included
                && match self.mode {
                    DisplayMode::Single(id) => o.id == id,
                    _ => true,
                };
            o.y = 0;
            match self.mode {
                DisplayMode::Extend => match o.placement.position {
                    Some((px, py)) if placed => (o.x, o.y) = (px, py),
                    _ => {
                        o.x = x;
                        if o.enabled {
                            x += o.logical_size().0;
                        }
                    }
                },
                DisplayMode::Mirror => {
                    o.x = 0;
                    if let (false, Some((lw, lh))) = (o.primary, primary_size) {
                        // Fit the primary's logical area (letterboxed).
                        let fit = (f64::from(o.width) / f64::from(lw.max(1)))
                            .min(f64::from(o.height) / f64::from(lh.max(1)));
                        o.scale_milli = ((fit * 1000.0) as u32).clamp(250, 8000);
                    }
                }
                DisplayMode::Single(_) => o.x = 0,
            }
        }
    }

    /// All outputs (primary first).
    pub fn outputs(&self) -> &[Output] {
        &self.outputs
    }

    /// The outputs that are on.
    pub fn enabled(&self) -> impl Iterator<Item = &Output> {
        self.outputs.iter().filter(|o| o.enabled)
    }

    /// One output.
    pub fn get(&self, id: OutputId) -> Option<&Output> {
        self.outputs.iter().find(|o| o.id == id)
    }

    /// The output with a backend display id.
    pub fn by_hal(&self, hal: u32) -> Option<&Output> {
        self.outputs.iter().find(|o| o.hal == hal)
    }

    /// The primary output (if any output is enabled).
    pub fn primary(&self) -> Option<&Output> {
        self.enabled()
            .find(|o| o.primary)
            .or_else(|| self.enabled().next())
    }

    /// The enabled output containing a logical point, else the primary.
    pub fn output_at(&self, x: i32, y: i32) -> Option<&Output> {
        self.enabled()
            .find(|o| o.logical_rect().contains(x, y))
            .or_else(|| self.primary())
    }

    /// The union of all enabled outputs (logical points).
    pub fn bounds(&self) -> Rect {
        self.enabled()
            .fold(Rect::default(), |acc, o| acc.union(&o.logical_rect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_suggestion_follows_density() {
        assert_eq!(suggested_scale(1920, 0), 1000);
        assert_eq!(suggested_scale(1920, 508), 1000); // 96 dpi
        assert_eq!(suggested_scale(3840, 344), 3000); // 284 dpi laptop -> capped
        assert_eq!(suggested_scale(2560, 344), 2000); // 189 dpi
        assert_eq!(suggested_scale(10, 10_000), 1000);
    }

    #[test]
    fn extend_mirror_single_and_primary() {
        let mut layout = Layout::new();
        let a = layout.add(0, "eDP-1", (1920, 1080), 60_000, 0);
        let b = layout.add(1, "HDMI-A-1", (3840, 2160), 60_000, 0);
        assert!(layout.set_scale(b, 2000));
        assert_eq!(layout.get(b).map(|o| o.scale_milli), Some(2000));
        assert_eq!(layout.get(b).map(|o| o.x), Some(1920));
        assert_eq!(layout.bounds(), Rect::new(0, 0, 3840, 1080));
        assert_eq!(layout.output_at(2000, 10).map(|o| o.id), Some(b));
        assert_eq!(layout.output_at(-50, -50).map(|o| o.id), Some(a));
        assert!(layout.set_primary(b));
        assert_eq!(layout.outputs()[0].id, b);
        assert_eq!(layout.get(a).map(|o| o.x), Some(1920));
        assert!(layout.set_mode(DisplayMode::Mirror));
        assert!(layout.outputs().iter().all(|o| o.x == 0));
        // a mirrors b's 1920x1080 logical area at scale 1.
        assert_eq!(layout.get(a).map(|o| o.scale_milli), Some(1000));
        assert!(layout.set_mode(DisplayMode::Single(a)));
        assert_eq!(layout.enabled().count(), 1);
        assert_eq!(layout.primary().map(|o| o.id), Some(a));
        assert!(!layout.set_mode(DisplayMode::Single(OutputId(99))));
        assert!(!layout.set_primary(OutputId(99)));
        assert!(!layout.set_scale(OutputId(99), 1000));
        assert!(layout.remove(a));
        assert_eq!(layout.mode(), DisplayMode::Extend);
        assert_eq!(layout.primary().map(|o| o.id), Some(b));
        assert!(!layout.remove(a));
        assert!(layout.set_scale(b, 1130));
        assert_eq!(layout.get(b).map(|o| o.scale_milli), Some(1250));
        assert!(layout.set_scale(b, 9000));
        assert_eq!(layout.get(b).map(|o| o.scale_milli), Some(3000));
        let o = layout.get(b).cloned().unwrap_or_else(|| unreachable!());
        assert_eq!(o.frame_interval_us(), 16_666);
        assert_eq!(o.info().scale_milli, 3000);
    }
}
