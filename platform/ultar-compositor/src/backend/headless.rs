//! A backend without hardware: outputs of a chosen size, frames counted,
//! input fed by the caller. Used by the tests and for running the
//! compositor where there is no display.

use super::{Backend, BackendOutput, Hotplug};
use crate::geom::Rect;
use crate::pixels::Image;
use std::collections::VecDeque;
use std::os::fd::BorrowedFd;
use ultar_hal::{HalError, InputEvent};

/// Headless outputs.
#[derive(Debug, Default)]
pub struct Headless {
    outputs: Vec<BackendOutput>,
    input: VecDeque<InputEvent>,
    hotplug: Vec<Hotplug>,
    /// Frames presented so far.
    pub presented: u64,
    /// Damage of the last present.
    pub last_damage: Vec<Rect>,
}

impl Headless {
    /// One output per size, named `headless-N`, 60 Hz.
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
                width_mm: 0,
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

    /// Simulates removing a display.
    pub fn unplug(&mut self, hal: u32) {
        self.outputs.retain(|o| o.hal != hal);
        self.hotplug.push(Hotplug::Removed(hal));
    }
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
}
