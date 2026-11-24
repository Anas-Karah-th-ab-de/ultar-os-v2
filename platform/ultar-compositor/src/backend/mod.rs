//! Where frames go and input comes from: the HAL displays and input
//! devices on a machine, or nothing at all (headless, for tests).

mod hal;
mod headless;

pub use hal::{HalBackend, OutputKind};
pub use headless::Headless;

use crate::geom::Rect;
use crate::pixels::Image;
use std::os::fd::BorrowedFd;
use ultar_hal::{HalError, InputEvent};
use ultar_surface::proto::{OutputIdentity, OutputMode};

/// A display as the backend reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackendOutput {
    /// Backend id.
    pub hal: u32,
    /// Connector name.
    pub name: String,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Refresh rate in millihertz.
    pub refresh_millihertz: u32,
    /// Physical width in millimetres (0 = unknown).
    pub width_mm: u32,
    /// Physical height in millimetres (0 = unknown).
    pub height_mm: u32,
    /// Built into the device.
    pub internal: bool,
    /// What the display says about itself (empty without data).
    pub identity: OutputIdentity,
    /// Every mode it offers (empty: only the one above).
    pub modes: Vec<OutputMode>,
}

impl BackendOutput {
    /// The mode it runs now.
    pub fn current(&self) -> OutputMode {
        OutputMode {
            width: self.width,
            height: self.height,
            refresh_mhz: self.refresh_millihertz,
        }
    }

    /// Its modes; at least the current one.
    pub fn mode_list(&self) -> Vec<OutputMode> {
        if self.modes.is_empty() {
            vec![self.current()]
        } else {
            self.modes.clone()
        }
    }
}

/// A change of the connected displays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hotplug {
    /// A display was connected.
    Added(BackendOutput),
    /// A display was removed.
    Removed(u32),
}

/// Output and input of the compositor.
pub trait Backend {
    /// Connected displays.
    fn outputs(&self) -> Vec<BackendOutput>;
    /// Shows the damaged parts of a frame.
    fn present(&mut self, hal: u32, frame: &Image, damage: &[Rect]) -> Result<(), HalError>;
    /// Descriptors that become readable when input arrives.
    fn input_fds(&self) -> Vec<BorrowedFd<'_>>;
    /// Input that arrived (does not block).
    fn read_input(&mut self) -> Vec<InputEvent>;
    /// Display changes since the last call.
    fn hotplug(&mut self) -> Vec<Hotplug>;
    /// Switches a display to one of its modes. Backends that cannot switch
    /// accept only the mode in use.
    fn set_mode(&mut self, hal: u32, mode: OutputMode) -> Result<(), HalError> {
        let current = self.outputs().into_iter().find(|o| o.hal == hal);
        match current {
            Some(o) if o.current() == mode => Ok(()),
            Some(_) => Err(HalError::NotSupported),
            None => Err(HalError::NotFound),
        }
    }
    /// Looks for input devices that came or went (SIGHUP, kernels without
    /// uevents); returns `DeviceAdded`/`DeviceRemoved` and releases.
    fn rescan_input(&mut self) -> Vec<InputEvent> {
        Vec::new()
    }
}
