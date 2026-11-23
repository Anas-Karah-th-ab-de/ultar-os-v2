//! The real backend: a HAL display (DRM/KMS on Linux, the framebuffer of
//! the Ultar kernel) and evdev input.

use super::{Backend, BackendOutput, Hotplug};
use crate::geom::Rect;
use crate::pixels::Image;
use std::collections::HashMap;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::PathBuf;
use ultar_hal::{
    Display, DisplayEvent, DisplayId, FrameView, HalError, Input, InputEvent, PixelFormat, Size,
};
use ultar_hal_gfx::evdev::Evdev;
use ultar_hal_gfx::fbdev::FbDev;
use ultar_hal_gfx::kms::Kms;
use ultar_hal_gfx::GfxError;

/// Which display backend to open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputKind {
    /// DRM/KMS first, then the framebuffer.
    Auto,
    /// A DRM card (None = default card).
    Kms(Option<PathBuf>),
    /// A framebuffer device (None = default device).
    Framebuffer(Option<PathBuf>),
}

/// HAL display + evdev input.
pub struct HalBackend {
    display: Box<dyn Display>,
    input: Option<Evdev>,
    input_fds: Vec<OwnedFd>,
    staging: HashMap<u32, Vec<u8>>,
}

impl std::fmt::Debug for HalBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HalBackend")
            .field("input", &self.input.is_some())
            .finish()
    }
}

impl HalBackend {
    /// Opens the display and every input device (input is optional).
    pub fn open(kind: &OutputKind) -> Result<HalBackend, GfxError> {
        let display: Box<dyn Display> = match kind {
            OutputKind::Kms(path) => Box::new(Kms::open(path.as_deref())?),
            OutputKind::Framebuffer(path) => Box::new(FbDev::open(path.as_deref())?),
            OutputKind::Auto => match Kms::open(None) {
                Ok(kms) => Box::new(kms),
                Err(_) => Box::new(FbDev::open(None)?),
            },
        };
        let input = Evdev::open_all().ok();
        let input_fds = input.as_ref().map(Evdev::poll_handles).unwrap_or_default();
        Ok(HalBackend {
            display,
            input,
            input_fds,
            staging: HashMap::new(),
        })
    }
}

impl Backend for HalBackend {
    fn outputs(&self) -> Vec<BackendOutput> {
        self.display
            .displays()
            .unwrap_or_default()
            .into_iter()
            .map(|d| BackendOutput {
                hal: d.id.0,
                name: d.connector,
                width: d.preferred_mode.size.width,
                height: d.preferred_mode.size.height,
                refresh_millihertz: d.preferred_mode.refresh_millihertz,
                width_mm: d.physical_mm.width,
            })
            .collect()
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
        self.input_fds.iter().map(|f| f.as_fd()).collect()
    }

    fn read_input(&mut self) -> Vec<InputEvent> {
        self.input
            .as_ref()
            .and_then(|i| i.poll(0).ok())
            .unwrap_or_default()
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
