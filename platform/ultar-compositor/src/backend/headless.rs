//! A backend without hardware: outputs of a chosen size, frames counted,
//! input fed by the caller. Used by the tests and for running the
//! compositor where there is no display.

use super::{Backend, BackendOutput, Hotplug};
use crate::geom::Rect;
use crate::pixels::Image;
use std::collections::VecDeque;
use std::os::fd::BorrowedFd;
use ultar_hal::{HalError, InputEvent};
use ultar_surface::proto::OutputMode;

/// Headless outputs.
#[derive(Debug, Default)]
pub struct Headless {
    outputs: Vec<BackendOutput>,
    input: VecDeque<InputEvent>,
    hotplug: Vec<Hotplug>,
    refused: Vec<OutputMode>,
    /// Frames presented so far.
    pub presented: u64,
    /// Damage of the last present.
    pub last_damage: Vec<Rect>,
}

impl Headless {
    /// One output per size, named `headless-N`, 60 Hz. Each offers its size
    /// and the common smaller modes, and identifies itself as manufacturer
    /// `UHL` (Ultar headless), product 1, serial N + 1 – so the display
    /// service can be run end to end without hardware.
    pub fn new(sizes: &[(u32, u32)]) -> Headless {
        let outputs = sizes
            .iter()
            .enumerate()
            .map(|(i, &(width, height))| BackendOutput {
                hal: i as u32,
                name: format!("headless-{i}"),
                width,
                height,
                refresh_millihertz: 60_000,
                identity: ultar_surface::proto::OutputIdentity {
                    manufacturer: "UHL".into(),
                    product: 1,
                    serial: i as u32 + 1,
                    name: "Headless".into(),
                },
                modes: headless_modes(width, height),
                ..BackendOutput::default()
            })
            .collect();
        Headless {
            outputs,
            ..Headless::default()
        }
    }

    /// Queues input as if a device produced it.
    pub fn push_input(&mut self, events: &[InputEvent]) {
        self.input.extend(events.iter().copied());
    }

    /// Simulates plugging in a display.
    pub fn plug(&mut self, output: BackendOutput) {
        self.outputs.push(output.clone());
        self.hotplug.push(Hotplug::Added(output));
    }

    /// Gives a headless output identity data and a mode list (tests of the
    /// display service).
    pub fn describe(&mut self, hal: u32, describe: impl FnOnce(&mut BackendOutput)) {
        if let Some(o) = self.outputs.iter_mut().find(|o| o.hal == hal) {
            describe(o);
        }
    }

    /// Modes the next `set_mode` refuses (a display that fails to switch).
    pub fn refuse_mode(&mut self, mode: OutputMode) {
        self.refused.push(mode);
    }

    /// Simulates removing a display.
    pub fn unplug(&mut self, hal: u32) {
        self.outputs.retain(|o| o.hal != hal);
        self.hotplug.push(Hotplug::Removed(hal));
    }
}

/// The size itself, then the common smaller modes, all at 60 Hz.
fn headless_modes(width: u32, height: u32) -> Vec<OutputMode> {
    let mut modes = vec![OutputMode {
        width,
        height,
        refresh_mhz: 60_000,
    }];
    for (w, h) in [
        (1920, 1080),
        (1280, 800),
        (1280, 720),
        (1024, 768),
        (800, 600),
    ] {
        if w < width && h <= height || w <= width && h < height {
            modes.push(OutputMode {
                width: w,
                height: h,
                refresh_mhz: 60_000,
            });
        }
    }
    modes
}

impl Backend for Headless {
    fn outputs(&self) -> Vec<BackendOutput> {
        self.outputs.clone()
    }

    fn present(&mut self, hal: u32, _frame: &Image, damage: &[Rect]) -> Result<(), HalError> {
        if !self.outputs.iter().any(|o| o.hal == hal) {
            return Err(HalError::NotFound);
        }
        self.presented += 1;
        self.last_damage = damage.to_vec();
        Ok(())
    }

    fn input_fds(&self) -> Vec<BorrowedFd<'_>> {
        Vec::new()
    }

    fn read_input(&mut self) -> Vec<InputEvent> {
        self.input.drain(..).collect()
    }

    fn hotplug(&mut self) -> Vec<Hotplug> {
        std::mem::take(&mut self.hotplug)
    }

    fn set_mode(&mut self, hal: u32, mode: OutputMode) -> Result<(), HalError> {
        let refused = self.refused.contains(&mode);
        let o = self
            .outputs
            .iter_mut()
            .find(|o| o.hal == hal)
            .ok_or(HalError::NotFound)?;
        if o.current() == mode {
            return Ok(());
        }
        if refused || !o.mode_list().contains(&mode) {
            return Err(HalError::NotSupported);
        }
        if o.modes.is_empty() {
            o.modes = vec![o.current()];
        }
        o.width = mode.width;
        o.height = mode.height;
        o.refresh_millihertz = mode.refresh_mhz;
        Ok(())
    }
}
