//! Mock display outputs and renderer.

use super::cell::MockCell;
use crate::display::{
    Display, DisplayEvent, DisplayInfo, DisplayMode, FrameView, Renderer, RendererKind,
};
use crate::error::{HalError, HalResult};
use crate::types::{DisplayId, Level, PixelFormat, Rect, Rotation, Size};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug)]
struct Output {
    info: DisplayInfo,
    modes: Vec<DisplayMode>,
    current: DisplayMode,
    brightness: Level,
    rotation: Rotation,
    frames: u64,
    last_damage: Vec<Rect>,
}

#[derive(Debug, Default)]
struct DisplayState {
    outputs: Vec<Output>,
    events: Vec<DisplayEvent>,
    next_id: u32,
}

impl DisplayState {
    fn output(&mut self, id: DisplayId) -> HalResult<&mut Output> {
        self.outputs
            .iter_mut()
            .find(|output| output.info.id == id)
            .ok_or(HalError::NotFound)
    }
}

/// Scriptable display outputs. Starts without any display.
#[derive(Debug, Default)]
pub struct MockDisplay {
    cell: MockCell<DisplayState>,
}

impl MockDisplay {
    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }

    /// Connects a display and queues the hotplug event. The first mode is the
    /// preferred and current one; an empty mode list gets one 1024x768 mode.
    pub fn plug(
        &self,
        connector: &str,
        internal: bool,
        physical_mm: Size,
        modes: &[DisplayMode],
    ) -> DisplayId {
        let mut state = self.cell.state();
        let id = DisplayId(state.next_id);
        state.next_id += 1;
        let modes = if modes.is_empty() {
            vec![DisplayMode::new(1024, 768, 60)]
        } else {
            modes.to_vec()
        };
        let preferred = modes.first().copied().unwrap_or_default();
        state.outputs.push(Output {
            info: DisplayInfo {
                id,
                connector: connector.to_string(),
                internal,
                physical_mm,
                preferred_mode: preferred,
            },
            modes,
            current: preferred,
            brightness: Level::MAX,
            rotation: Rotation::Deg0,
            frames: 0,
            last_damage: Vec::new(),
        });
        state.events.push(DisplayEvent::Connected(id));
        id
    }

    /// Removes a display and queues the hotplug event.
    pub fn unplug(&self, id: DisplayId) -> HalResult<()> {
        let mut state = self.cell.state();
        let before = state.outputs.len();
        state.outputs.retain(|output| output.info.id != id);
        if state.outputs.len() == before {
            return Err(HalError::NotFound);
        }
        state.events.push(DisplayEvent::Disconnected(id));
        Ok(())
    }

    /// Number of frames presented on a display so far.
    pub fn presented_frames(&self, id: DisplayId) -> u64 {
        self.cell
            .state()
            .output(id)
            .map(|output| output.frames)
            .unwrap_or(0)
    }

    /// Damage of the last presented frame, clipped to the display.
    pub fn last_damage(&self, id: DisplayId) -> Vec<Rect> {
        self.cell
            .state()
            .output(id)
            .map(|output| output.last_damage.clone())
            .unwrap_or_default()
    }
}

impl Display for MockDisplay {
    fn displays(&self) -> HalResult<Vec<DisplayInfo>> {
        self.cell.call(|state| {
            Ok(state
                .outputs
                .iter()
                .map(|output| output.info.clone())
                .collect())
        })
    }

    fn modes(&self, id: DisplayId) -> HalResult<Vec<DisplayMode>> {
        self.cell.call(|state| Ok(state.output(id)?.modes.clone()))
    }

    fn current_mode(&self, id: DisplayId) -> HalResult<DisplayMode> {
        self.cell.call(|state| Ok(state.output(id)?.current))
    }

    fn set_mode(&self, id: DisplayId, mode: DisplayMode) -> HalResult<()> {
        self.cell.call(|state| {
            let output = state.output(id)?;
            if !output.modes.contains(&mode) {
                return Err(HalError::InvalidArgument);
            }
            if output.current != mode {
                output.current = mode;
                state.events.push(DisplayEvent::ModeChanged(id));
            }
            Ok(())
        })
    }

    fn present(&self, id: DisplayId, frame: &FrameView<'_>, damage: &[Rect]) -> HalResult<()> {
        self.cell.call(|state| {
            let output = state.output(id)?;
            let size = output.current.size;
            if frame.size() != size {
                return Err(HalError::InvalidArgument);
            }
            output.last_damage = if damage.is_empty() {
                vec![Rect::from_size(size)]
            } else {
                damage
                    .iter()
                    .filter_map(|rect| rect.clipped_to(size))
                    .collect()
            };
            output.frames += 1;
            Ok(())
        })
    }

    fn brightness(&self, id: DisplayId) -> HalResult<Level> {
        self.cell.call(|state| Ok(state.output(id)?.brightness))
    }

    fn set_brightness(&self, id: DisplayId, level: Level) -> HalResult<()> {
        self.cell.call(|state| {
            state.output(id)?.brightness = level;
            Ok(())
        })
    }

    fn rotation(&self, id: DisplayId) -> HalResult<Rotation> {
        self.cell.call(|state| Ok(state.output(id)?.rotation))
    }

    fn set_rotation(&self, id: DisplayId, rotation: Rotation) -> HalResult<()> {
        self.cell.call(|state| {
            state.output(id)?.rotation = rotation;
            Ok(())
        })
    }

    fn poll_events(&self) -> HalResult<Vec<DisplayEvent>> {
        self.cell
            .call(|state| Ok(std::mem::take(&mut state.events)))
    }
}

/// Scriptable renderer description. Software by default.
#[derive(Debug, Default)]
pub struct MockRenderer {
    gpu: AtomicBool,
}

impl MockRenderer {
    /// Switches between a GPU and the software renderer.
    pub fn set_gpu(&self, gpu: bool) {
        self.gpu.store(gpu, Ordering::SeqCst);
    }
}

impl Renderer for MockRenderer {
    fn kind(&self) -> RendererKind {
        if self.gpu.load(Ordering::SeqCst) {
            RendererKind::Gpu
        } else {
            RendererKind::Software
        }
    }

    fn max_surface_size(&self) -> u32 {
        match self.kind() {
            RendererKind::Gpu => 16_384,
            RendererKind::Software => 8_192,
        }
    }

    fn supports_format(&self, format: PixelFormat) -> bool {
        match self.kind() {
            RendererKind::Gpu => format.bytes_per_pixel().is_some(),
            RendererKind::Software => {
                matches!(format, PixelFormat::Xrgb8888 | PixelFormat::Argb8888)
            }
        }
    }
}
