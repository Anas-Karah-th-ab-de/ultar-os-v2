//! The real backend: a HAL display (DRM/KMS on Linux, the framebuffer of
//! the Ultar kernel) and evdev input.

use super::{Backend, BackendOutput, Hotplug};
use crate::geom::Rect;
use crate::pixels::Image;
use std::collections::HashMap;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::PathBuf;
use ultar_hal::{
    Display, DisplayEvent, DisplayId, FrameView, HalError, Input, InputEvent, InputEventKind,
    PixelFormat, Size,
};
use ultar_hal_gfx::decon::{self, Decon};
use ultar_hal_gfx::edid;
use ultar_hal_gfx::evdev::{Evdev, SeatPolicy};
use ultar_hal_gfx::fbdev::FbDev;
use ultar_hal_gfx::kms::Kms;
use ultar_hal_gfx::uevent::UeventSocket;
use ultar_hal_gfx::GfxError;
use ultar_surface::proto::{OutputIdentity, OutputMode};

/// Which display backend to open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputKind {
    /// DRM/KMS first, then the framebuffer.
    Auto,
    /// A DRM card (None = default card).
    Kms(Option<PathBuf>),
    /// A framebuffer device (None = default device).
    Framebuffer(Option<PathBuf>),
    /// Samsung Exynos DECON through a framebuffer device (None = default device).
    Decon(Option<PathBuf>),
}

/// HAL display + evdev input with hot-plug (kernel uevents; a rescan on
/// SIGHUP where there are none).
pub struct HalBackend {
    display: Box<dyn Display>,
    input: Evdev,
    uevents: Option<UeventSocket>,
    input_fds: Vec<OwnedFd>,
    staging: HashMap<u32, Vec<u8>>,
}

impl std::fmt::Debug for HalBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HalBackend")
            .field("input_fds", &self.input_fds.len())
            .field("uevents", &self.uevents.is_some())
            .finish()
    }
}

/// DRM/KMS first; on an Exynos phone DECON (plain fbdev shows nothing there); else the framebuffer.
fn auto_display() -> Result<Box<dyn Display>, GfxError> {
    if let Ok(kms) = Kms::open(None) {
        return Ok(Box::new(kms));
    }
    if decon::present() {
        if let Ok(d) = Decon::open(None) {
            return Ok(Box::new(d));
        }
    }
    Ok(Box::new(FbDev::open(None)?))
}

impl HalBackend {
    /// Opens the display and the seat's input devices (`seat`: which
    /// event nodes belong to the session; input is optional).
    pub fn open(kind: &OutputKind, seat: SeatPolicy) -> Result<HalBackend, GfxError> {
        let display: Box<dyn Display> = match kind {
            OutputKind::Kms(path) => Box::new(Kms::open(path.as_deref())?),
            OutputKind::Framebuffer(path) => Box::new(FbDev::open(path.as_deref())?),
            OutputKind::Decon(path) => Box::new(Decon::open(path.as_deref())?),
            OutputKind::Auto => auto_display()?,
        };
        let input = Evdev::open_seat(seat).unwrap_or_else(|_| Evdev::empty());
        let input_fds = input.poll_handles();
        Ok(HalBackend {
            display,
            input,
            uevents: UeventSocket::open().ok(),
            input_fds,
            staging: HashMap::new(),
        })
    }

    /// Refreshes the poll set after devices came or went.
    fn track(&mut self, events: &[InputEvent]) {
        let changed = events.iter().any(|e| {
            matches!(
                e.kind,
                InputEventKind::DeviceAdded | InputEventKind::DeviceRemoved
            )
        });
        if changed {
            self.input_fds = self.input.poll_handles();
        }
    }
}

impl HalBackend {
    /// A HAL display as the compositor sees it: mode in use, every mode, and
    /// the identity from the display's own data.
    fn describe(&self, d: ultar_hal::DisplayInfo) -> BackendOutput {
        let id = d.id;
        let current = self.display.current_mode(id).unwrap_or(d.preferred_mode);
        let to_mode = |m: ultar_hal::DisplayMode| OutputMode {
            width: m.size.width,
            height: m.size.height,
            refresh_mhz: m.refresh_millihertz,
        };
        let edid = self
            .display
            .identification(id)
            .ok()
            .flatten()
            .and_then(|bytes| edid::parse(&bytes));
        let identity = edid
            .as_ref()
            .map_or_else(OutputIdentity::default, |e| OutputIdentity {
                manufacturer: e.manufacturer.clone(),
                product: e.product,
                serial: e.serial,
                name: e.name.clone(),
            });
        let (width_mm, height_mm) = match (d.physical_mm, &edid) {
            (s, _) if s.width > 0 => (s.width, s.height),
            (_, Some(e)) => e.size_mm,
            _ => (0, 0),
        };
        let mut modes: Vec<OutputMode> = self
            .display
            .modes(id)
            .unwrap_or_default()
            .into_iter()
            .map(to_mode)
            .collect();
        modes.truncate(ultar_surface::proto::MAX_OUTPUT_MODES);
        BackendOutput {
            hal: id.0,
            name: d.connector,
            width: current.size.width,
            height: current.size.height,
            refresh_millihertz: current.refresh_millihertz,
            width_mm,
            height_mm,
            internal: d.internal,
            identity,
            modes,
        }
    }
}

impl Backend for HalBackend {
    fn outputs(&self) -> Vec<BackendOutput> {
        self.display
            .displays()
            .unwrap_or_default()
            .into_iter()
            .map(|d| self.describe(d))
            .collect()
    }

    fn set_mode(&mut self, hal: u32, mode: OutputMode) -> Result<(), HalError> {
        let mode = ultar_hal::DisplayMode {
            size: Size::new(mode.width, mode.height),
            refresh_millihertz: mode.refresh_mhz,
        };
        self.display.set_mode(DisplayId(hal), mode)
    }

    fn present(&mut self, hal: u32, frame: &Image, damage: &[Rect]) -> Result<(), HalError> {
        let (w, h) = (frame.width() as usize, frame.height() as usize);
        let staging = self.staging.entry(hal).or_default();
        let full = staging.len() != w * h * 4;
        if full {
            staging.clear();
            staging.resize(w * h * 4, 0);
        }
        let whole = [frame.bounds()];
        let rects = if full { &whole[..] } else { damage };
        let mut hal_rects = Vec::with_capacity(rects.len());
        for r in rects {
            let Some(r) = r.intersect(&frame.bounds()) else {
                continue;
            };
            for y in r.y..r.max_y() {
                let row = &frame.data()
                    [y as usize * w + r.x as usize..y as usize * w + r.max_x() as usize];
                let start = (y as usize * w + r.x as usize) * 4;
                for (dst, px) in staging[start..start + row.len() * 4]
                    .chunks_exact_mut(4)
                    .zip(row)
                {
                    dst.copy_from_slice(&px.to_le_bytes());
                }
            }
            hal_rects.push(ultar_hal::Rect::new(r.x, r.y, r.w as u32, r.h as u32));
        }
        let view = FrameView::new(
            Size::new(w as u32, h as u32),
            (w * 4) as u32,
            PixelFormat::Xrgb8888,
            staging,
        )?;
        self.display.present(DisplayId(hal), &view, &hal_rects)
    }

    fn input_fds(&self) -> Vec<BorrowedFd<'_>> {
        let devices = self.input_fds.iter().map(|f| f.as_fd());
        devices
            .chain(self.uevents.as_ref().map(|u| u.fd()))
            .collect()
    }

    fn read_input(&mut self) -> Vec<InputEvent> {
        let mut events = Vec::new();
        if self
            .uevents
            .as_ref()
            .is_some_and(UeventSocket::input_changed)
        {
            events.extend(self.input.rescan());
        }
        events.extend(self.input.poll(0).unwrap_or_default());
        self.track(&events);
        events
    }

    fn rescan_input(&mut self) -> Vec<InputEvent> {
        let events = self.input.rescan();
        self.track(&events);
        events
    }

    fn hotplug(&mut self) -> Vec<Hotplug> {
        let events = self.display.poll_events().unwrap_or_default();
        let outputs = self.outputs();
        events
            .into_iter()
            .filter_map(|e| match e {
                DisplayEvent::Connected(id) => outputs
                    .iter()
                    .find(|o| o.hal == id.0)
                    .cloned()
                    .map(Hotplug::Added),
                DisplayEvent::Disconnected(id) => Some(Hotplug::Removed(id.0)),
                DisplayEvent::ModeChanged(_) => None,
            })
            .collect()
    }
}
