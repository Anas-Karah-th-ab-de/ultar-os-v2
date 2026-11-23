//! Linux DRM/KMS output with dumb buffers – for developing the compositor on
//! a Linux machine (from a text console; a running desktop holds the DRM
//! master and `set_crtc` is refused).
//!
//! Every connected connector gets its own CRTC and one dumb buffer that is
//! scanned out directly; [`Display::present`] copies the damaged rows into it
//! and tells the driver which part changed (`DIRTYFB`). Hot-plug is found by
//! [`Display::poll_events`], which probes the connectors again; the
//! compositor calls it on demand (there is no uevent socket here).

use crate::GfxError;
use drm::buffer::{Buffer as _, DrmFourcc};
use drm::control::{
    connector, crtc, dumbbuffer::DumbBuffer, framebuffer, ClipRect, Device as ControlDevice, Mode,
    ModeTypeFlags, ResourceHandles,
};
use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, BorrowedFd};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use ultar_hal::{
    Display, DisplayEvent, DisplayId, DisplayInfo, DisplayMode, FrameView, HalError, HalResult,
    Level, PixelFormat, Rect, Rotation, Size,
};

/// The card the compositor tries when no path is given.
pub const DEFAULT_CARD: &str = "/dev/dri/card0";

struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl drm::Device for Card {}
impl ControlDevice for Card {}

struct Output {
    id: DisplayId,
    connector: connector::Handle,
    crtc: crtc::Handle,
    info: DisplayInfo,
    mode: DisplayMode,
    buffer: DumbBuffer,
    framebuffer: framebuffer::Handle,
}

/// A DRM/KMS card with all connected outputs lit.
pub struct Kms {
    card: Card,
    path: PathBuf,
    outputs: Mutex<Vec<Output>>,
}

impl std::fmt::Debug for Kms {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kms").field("path", &self.path).finish()
    }
}

/// The preferred mode of a connector, else its first one.
pub fn pick_mode(modes: &[(u32, u32, u32, bool)]) -> Option<usize> {
    modes
        .iter()
        .position(|m| m.3)
        .or(if modes.is_empty() { None } else { Some(0) })
}

impl Kms {
    /// Opens a card (`None` = [`DEFAULT_CARD`]) and lights every connected
    /// output. Fails with [`GfxError::NoDevice`] when nothing is connected.
    pub fn open(path: Option<&Path>) -> Result<Kms, GfxError> {
        let path = path.unwrap_or(Path::new(DEFAULT_CARD)).to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => GfxError::NoDevice,
                _ => GfxError::from(e),
            })?;
        let kms = Kms {
            card: Card(file),
            path,
            outputs: Mutex::new(Vec::new()),
        };
        kms.rescan()?;
        let lit = kms.outputs.lock().map(|o| o.len()).unwrap_or(0);
        if lit == 0 {
            return Err(GfxError::NoDevice);
        }
        Ok(kms)
    }

    /// Probes the connectors; lights new ones, drops removed ones.
    fn rescan(&self) -> Result<Vec<DisplayEvent>, GfxError> {
        let res = self.card.resource_handles()?;
        let mut outputs = self.outputs.lock().map_err(|_| GfxError::Sys(16))?;
        let mut events = Vec::new();
        let mut connected = Vec::new();
        for &handle in res.connectors() {
            let info = self.card.get_connector(handle, true)?;
            if info.state() == connector::State::Connected && !info.modes().is_empty() {
                connected.push(info);
            }
        }
        // Removed outputs.
        let mut kept = Vec::new();
        for output in outputs.drain(..) {
            if connected.iter().any(|c| c.handle() == output.connector) {
                kept.push(output);
            } else {
                events.push(DisplayEvent::Disconnected(output.id));
                let _ = self.card.destroy_framebuffer(output.framebuffer);
                let _ = self.card.destroy_dumb_buffer(output.buffer);
            }
        }
        *outputs = kept;
        for info in connected {
            if outputs.iter().any(|o| o.connector == info.handle()) {
                continue;
            }
            let used: Vec<crtc::Handle> = outputs.iter().map(|o| o.crtc).collect();
            if let Some(output) = self.light(&res, &info, &used, next_id(&outputs))? {
                events.push(DisplayEvent::Connected(output.id));
                outputs.push(output);
            }
        }
        Ok(events)
    }

    fn light(
        &self,
        res: &ResourceHandles,
        info: &connector::Info,
        used: &[crtc::Handle],
        id: DisplayId,
    ) -> Result<Option<Output>, GfxError> {
        let list = mode_list(info);
        let Some(index) = pick_mode(&list) else {
            return Ok(None);
        };
        let mode: Mode = info.modes()[index];
        let crtc = info
            .encoders()
            .iter()
            .filter_map(|&e| self.card.get_encoder(e).ok())
            .find_map(|e| {
                res.filter_crtcs(e.possible_crtcs())
                    .into_iter()
                    .find(|c| !used.contains(c))
            })
            .ok_or(GfxError::NoDevice)?;
        let (width, height) = (list[index].0, list[index].1);
        let buffer = self
            .card
            .create_dumb_buffer((width, height), DrmFourcc::Xrgb8888, 32)?;
        let framebuffer = self.card.add_framebuffer(&buffer, 24, 32)?;
        self.card.set_crtc(
            crtc,
            Some(framebuffer),
            (0, 0),
            &[info.handle()],
            Some(mode),
        )?;
        let display_mode = DisplayMode::new(width, height, list[index].2.max(1));
        let (mm_w, mm_h) = info.size().unwrap_or((0, 0));
        let internal = matches!(
            info.interface(),
            connector::Interface::EmbeddedDisplayPort | connector::Interface::LVDS
        );
        Ok(Some(Output {
            id,
            connector: info.handle(),
            crtc,
            info: DisplayInfo {
                id,
                connector: format!("{:?}-{}", info.interface(), info.interface_id()),
                internal,
                physical_mm: Size::new(mm_w, mm_h),
                preferred_mode: display_mode,
            },
            mode: display_mode,
            buffer,
            framebuffer,
        }))
    }

    fn with_output<T>(
        &self,
        id: DisplayId,
        f: impl FnOnce(&mut Output) -> HalResult<T>,
    ) -> HalResult<T> {
        let mut outputs = self.outputs.lock().map_err(|_| HalError::Busy)?;
        let output = outputs
            .iter_mut()
            .find(|o| o.id == id)
            .ok_or(HalError::NotFound)?;
        f(output)
    }
}

/// (width, height, refresh in Hz, preferred) of every mode of a connector.
fn mode_list(info: &connector::Info) -> Vec<(u32, u32, u32, bool)> {
    info.modes()
        .iter()
        .map(|m| {
            let (w, h) = m.size();
            let preferred = m.mode_type().contains(ModeTypeFlags::PREFERRED);
            (u32::from(w), u32::from(h), m.vrefresh(), preferred)
        })
        .collect()
}

fn next_id(outputs: &[Output]) -> DisplayId {
    DisplayId(outputs.iter().map(|o| o.id.0 + 1).max().unwrap_or(0))
}

impl Display for Kms {
    fn displays(&self) -> HalResult<Vec<DisplayInfo>> {
        let outputs = self.outputs.lock().map_err(|_| HalError::Busy)?;
        Ok(outputs.iter().map(|o| o.info.clone()).collect())
    }

    fn modes(&self, id: DisplayId) -> HalResult<Vec<DisplayMode>> {
        self.with_output(id, |o| Ok(vec![o.mode]))
    }

    fn current_mode(&self, id: DisplayId) -> HalResult<DisplayMode> {
        self.with_output(id, |o| Ok(o.mode))
    }

    fn set_mode(&self, id: DisplayId, mode: DisplayMode) -> HalResult<()> {
        self.with_output(id, |o| {
            if o.mode.size == mode.size {
                Ok(())
            } else {
                Err(HalError::NotSupported)
            }
        })
    }

    fn present(&self, id: DisplayId, frame: &FrameView<'_>, damage: &[Rect]) -> HalResult<()> {
        if !matches!(
            frame.format(),
            PixelFormat::Xrgb8888 | PixelFormat::Argb8888
        ) {
            return Err(HalError::NotSupported);
        }
        let card = &self.card;
        self.with_output(id, |o| {
            let screen = Size::new(
                o.mode.size.width.min(frame.size().width),
                o.mode.size.height.min(frame.size().height),
            );
            let pitch = o.buffer.pitch() as usize;
            let framebuffer = o.framebuffer;
            let mut map = card
                .map_dumb_buffer(&mut o.buffer)
                .map_err(|e| HalError::from(GfxError::from(e)))?;
            let whole = [Rect::from_size(screen)];
            let rects = if damage.is_empty() {
                &whole[..]
            } else {
                damage
            };
            let mut clips = Vec::with_capacity(rects.len());
            for rect in rects {
                let Some(area) = rect.clipped_to(screen) else {
                    continue;
                };
                copy_rect(frame, &area, &mut map, pitch).ok_or(HalError::InvalidArgument)?;
                clips.push(ClipRect::new(
                    area.x as u16,
                    area.y as u16,
                    (area.x as u32 + area.width) as u16,
                    (area.y as u32 + area.height) as u16,
                ));
            }
            drop(map);
            // Drivers without DIRTYFB scan out the buffer continuously.
            let _ = card.dirty_framebuffer(framebuffer, &clips);
            Ok(())
        })
    }

    fn brightness(&self, _id: DisplayId) -> HalResult<Level> {
        Err(HalError::NotSupported)
    }

    fn set_brightness(&self, _id: DisplayId, _level: Level) -> HalResult<()> {
        Err(HalError::NotSupported)
    }

    fn rotation(&self, id: DisplayId) -> HalResult<Rotation> {
        self.with_output(id, |_| Ok(Rotation::default()))
    }

    fn set_rotation(&self, _id: DisplayId, _rotation: Rotation) -> HalResult<()> {
        Err(HalError::NotSupported)
    }

    fn poll_events(&self) -> HalResult<Vec<DisplayEvent>> {
        Ok(self.rescan()?)
    }
}

fn copy_rect(frame: &FrameView<'_>, area: &Rect, dst: &mut [u8], pitch: usize) -> Option<()> {
    let (x, width) = (area.x as usize, area.width as usize * 4);
    for row in area.y as usize..area.y as usize + area.height as usize {
        let src_start = row * frame.stride() as usize + x * 4;
        let dst_start = row * pitch + x * 4;
        let source = frame.data().get(src_start..src_start + width)?;
        dst.get_mut(dst_start..dst_start + width)?
            .copy_from_slice(source);
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_mode_wins_else_first() {
        assert_eq!(pick_mode(&[]), None);
        assert_eq!(
            pick_mode(&[(800, 600, 60, false), (1920, 1080, 60, true)]),
            Some(1)
        );
        assert_eq!(pick_mode(&[(800, 600, 60, false)]), Some(0));
    }

    #[test]
    fn rect_copy_respects_pitch_and_bounds() {
        let pixels: Vec<u8> = (0..32).collect();
        let frame = FrameView::new(Size::new(2, 4), 8, PixelFormat::Xrgb8888, &pixels).unwrap();
        let mut dst = vec![0u8; 4 * 12];
        copy_rect(&frame, &Rect::new(1, 2, 1, 2), &mut dst, 12).unwrap();
        assert_eq!(&dst[28..32], &[20, 21, 22, 23]);
        assert_eq!(&dst[40..44], &[28, 29, 30, 31]);
        let mut short = vec![0u8; 8];
        assert!(copy_rect(&frame, &Rect::new(0, 0, 2, 2), &mut short, 12).is_none());
    }

    #[test]
    fn missing_card_is_reported() {
        let missing = Path::new("/dev/dri/ultar-no-such-card");
        assert_eq!(Kms::open(Some(missing)).err(), Some(GfxError::NoDevice));
    }
}
