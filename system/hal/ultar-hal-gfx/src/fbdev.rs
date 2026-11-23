//! Framebuffer output (`fb0`): the display of the Ultar kernel (UEFI GOP /
//! Bochs) and Linux fbdev as a fallback.
//!
//! Frames are written with `pwrite` row by row, only the damaged rows and
//! columns. The Ultar kernel copies each write straight into the visible
//! framebuffer (`eingabe::fb_schreiben`), so there is no page flip and no
//! vertical blank event; the compositor paces frames with its own timer.

use crate::GfxError;
use std::fs::{File, OpenOptions};
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::Mutex;
use ultar_hal::{
    Display, DisplayEvent, DisplayId, DisplayInfo, DisplayMode, FrameView, HalError, HalResult,
    Level, PixelFormat, Rect, Rotation, Size,
};

/// The framebuffer device of the Ultar kernel and of most Linux systems.
pub const DEFAULT_PATH: &str = "/dev/fb0";
/// `FBIOGET_VSCREENINFO`.
const FBIOGET_VSCREENINFO: rustix::ioctl::Opcode = 0x4600;
/// `FBIOGET_FSCREENINFO`.
const FBIOGET_FSCREENINFO: rustix::ioctl::Opcode = 0x4602;
/// Refresh rate reported for a framebuffer without timing information.
const ASSUMED_REFRESH_HZ: u32 = 60;

/// Geometry of a framebuffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FbGeometry {
    /// Visible width in pixels.
    pub width: u32,
    /// Visible height in pixels.
    pub height: u32,
    /// Bytes from one row to the next.
    pub line_bytes: u32,
    /// Red is in the lowest byte (RGBX) instead of the third (BGRX).
    pub swap_red_blue: bool,
}

impl FbGeometry {
    /// Reads the geometry from the two `FBIOGET` answers (`var` = 40 words of
    /// `fb_var_screeninfo`, `line_length` from `fb_fix_screeninfo`).
    pub fn from_screeninfo(var: &[u32; 40], line_length: u32) -> Result<FbGeometry, GfxError> {
        let (width, height, bits) = (var[0], var[1], var[6]);
        if bits != 32 {
            return Err(GfxError::UnsupportedFormat);
        }
        if width == 0 || height == 0 || u64::from(line_length) < u64::from(width) * 4 {
            return Err(GfxError::InvalidArgument);
        }
        // var[8] = red offset, var[14] = blue offset.
        let swap_red_blue = match (var[8], var[14]) {
            (16, 0) => false,
            (0, 16) => true,
            _ => return Err(GfxError::UnsupportedFormat),
        };
        Ok(FbGeometry {
            width,
            height,
            line_bytes: line_length,
            swap_red_blue,
        })
    }
}

/// A framebuffer display (always one output, id 0).
#[derive(Debug)]
pub struct FbDev {
    file: File,
    geometry: FbGeometry,
    scratch: Mutex<Vec<u8>>,
}

impl FbDev {
    /// Opens a framebuffer device; `None` means [`DEFAULT_PATH`].
    pub fn open(path: Option<&Path>) -> Result<FbDev, GfxError> {
        let path = path.unwrap_or(Path::new(DEFAULT_PATH));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => GfxError::NoDevice,
                _ => GfxError::from(e),
            })?;
        let mut var = [0u32; 40];
        let mut fix = [0u8; 80];
        // SAFETY: FBIOGET_VSCREENINFO writes a `struct fb_var_screeninfo`
        // (160 bytes = 40 u32) into the pointer of the `Updater`.
        unsafe {
            rustix::ioctl::ioctl(
                &file,
                rustix::ioctl::Updater::<FBIOGET_VSCREENINFO, [u32; 40]>::new(&mut var),
            )
        }?;
        // SAFETY: FBIOGET_FSCREENINFO writes a `struct fb_fix_screeninfo`
        // (80 bytes on 64-bit targets) into the pointer of the `Updater`.
        unsafe {
            rustix::ioctl::ioctl(
                &file,
                rustix::ioctl::Updater::<FBIOGET_FSCREENINFO, [u8; 80]>::new(&mut fix),
            )
        }?;
        let line_length = u32::from_ne_bytes([fix[48], fix[49], fix[50], fix[51]]);
        let geometry = FbGeometry::from_screeninfo(&var, line_length)?;
        Ok(FbDev {
            file,
            geometry,
            scratch: Mutex::new(Vec::new()),
        })
    }

    /// The geometry read at open.
    pub fn geometry(&self) -> FbGeometry {
        self.geometry
    }

    fn mode(&self) -> DisplayMode {
        DisplayMode::new(
            self.geometry.width,
            self.geometry.height,
            ASSUMED_REFRESH_HZ,
        )
    }

    fn check(&self, id: DisplayId) -> HalResult<()> {
        if id.0 == 0 {
            Ok(())
        } else {
            Err(HalError::NotFound)
        }
    }
}

/// Copies the damaged rows of `frame` into a framebuffer laid out as
/// `geometry` through `write_at(offset, bytes)`. Shared by the device and
/// the tests.
pub fn write_damage(
    geometry: &FbGeometry,
    frame: &FrameView<'_>,
    damage: &[Rect],
    scratch: &mut Vec<u8>,
    mut write_at: impl FnMut(u64, &[u8]) -> std::io::Result<()>,
) -> Result<(), GfxError> {
    let screen = Size::new(
        geometry.width.min(frame.size().width),
        geometry.height.min(frame.size().height),
    );
    let whole = [Rect::from_size(screen)];
    let rects = if damage.is_empty() {
        &whole[..]
    } else {
        damage
    };
    for rect in rects {
        let Some(area) = rect.clipped_to(screen) else {
            continue;
        };
        let (x, width) = (area.x as usize, area.width as usize);
        for row in area.y as usize..area.y as usize + area.height as usize {
            let start = row * frame.stride() as usize + x * 4;
            let source = frame
                .data()
                .get(start..start + width * 4)
                .ok_or(GfxError::InvalidArgument)?;
            let bytes = if geometry.swap_red_blue {
                scratch.clear();
                scratch.extend_from_slice(source);
                for px in scratch.chunks_exact_mut(4) {
                    px.swap(0, 2);
                }
                &scratch[..]
            } else {
                source
            };
            let offset = row as u64 * u64::from(geometry.line_bytes) + x as u64 * 4;
            write_at(offset, bytes)?;
        }
    }
    Ok(())
}

impl Display for FbDev {
    fn displays(&self) -> HalResult<Vec<DisplayInfo>> {
        Ok(vec![DisplayInfo {
            id: DisplayId(0),
            connector: "fb0".into(),
            internal: true,
            physical_mm: Size::new(0, 0),
            preferred_mode: self.mode(),
        }])
    }

    fn modes(&self, id: DisplayId) -> HalResult<Vec<DisplayMode>> {
        self.check(id)?;
        Ok(vec![self.mode()])
    }

    fn current_mode(&self, id: DisplayId) -> HalResult<DisplayMode> {
        self.check(id)?;
        Ok(self.mode())
    }

    fn set_mode(&self, id: DisplayId, mode: DisplayMode) -> HalResult<()> {
        self.check(id)?;
        if mode.size == self.mode().size {
            Ok(())
        } else {
            Err(HalError::NotSupported)
        }
    }

    fn present(&self, id: DisplayId, frame: &FrameView<'_>, damage: &[Rect]) -> HalResult<()> {
        self.check(id)?;
        if !matches!(
            frame.format(),
            PixelFormat::Xrgb8888 | PixelFormat::Argb8888
        ) {
            return Err(HalError::NotSupported);
        }
        let mut scratch = self.scratch.lock().map_err(|_| HalError::Busy)?;
        write_damage(
            &self.geometry,
            frame,
            damage,
            &mut scratch,
            |offset, bytes| self.file.write_all_at(bytes, offset),
        )?;
        Ok(())
    }

    fn brightness(&self, _id: DisplayId) -> HalResult<Level> {
        Err(HalError::NotSupported)
    }

    fn set_brightness(&self, _id: DisplayId, _level: Level) -> HalResult<()> {
        Err(HalError::NotSupported)
    }

    fn rotation(&self, id: DisplayId) -> HalResult<Rotation> {
        self.check(id)?;
        Ok(Rotation::default())
    }

    fn set_rotation(&self, _id: DisplayId, _rotation: Rotation) -> HalResult<()> {
        Err(HalError::NotSupported)
    }

    fn poll_events(&self) -> HalResult<Vec<DisplayEvent>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(width: u32, height: u32, red: u32, blue: u32) -> [u32; 40] {
        let mut v = [0u32; 40];
        v[0] = width;
        v[1] = height;
        v[6] = 32;
        v[8] = red;
        v[14] = blue;
        v
    }

    #[test]
    fn geometry_is_validated() {
        let g = FbGeometry::from_screeninfo(&var(4, 2, 16, 0), 32).unwrap();
        assert!(!g.swap_red_blue);
        assert!(
            FbGeometry::from_screeninfo(&var(4, 2, 0, 16), 16)
                .unwrap()
                .swap_red_blue
        );
        let mut bits16 = var(4, 2, 16, 0);
        bits16[6] = 16;
        assert_eq!(
            FbGeometry::from_screeninfo(&bits16, 32),
            Err(GfxError::UnsupportedFormat)
        );
        assert_eq!(
            FbGeometry::from_screeninfo(&var(4, 2, 8, 0), 32),
            Err(GfxError::UnsupportedFormat)
        );
        assert_eq!(
            FbGeometry::from_screeninfo(&var(4, 2, 16, 0), 8),
            Err(GfxError::InvalidArgument)
        );
        assert_eq!(
            FbGeometry::from_screeninfo(&var(0, 2, 16, 0), 32),
            Err(GfxError::InvalidArgument)
        );
    }

    #[test]
    fn only_damaged_rows_are_written_with_channel_swap() {
        let geometry = FbGeometry {
            width: 4,
            height: 3,
            line_bytes: 20,
            swap_red_blue: true,
        };
        let pixels: Vec<u8> = (0..48).collect();
        let frame = FrameView::new(Size::new(4, 3), 16, PixelFormat::Xrgb8888, &pixels).unwrap();
        let mut screen = [0u8; 60];
        let mut scratch = Vec::new();
        write_damage(
            &geometry,
            &frame,
            &[Rect::new(1, 1, 2, 1), Rect::new(10, 10, 1, 1)],
            &mut scratch,
            |offset, bytes| {
                let o = offset as usize;
                screen[o..o + bytes.len()].copy_from_slice(bytes);
                Ok(())
            },
        )
        .unwrap();
        // Row 1, pixels 1 and 2: source bytes 20..28, red and blue swapped.
        assert_eq!(&screen[24..32], &[22, 21, 20, 23, 26, 25, 24, 27]);
        assert!(screen[..24].iter().all(|&b| b == 0));
        assert!(screen[32..].iter().all(|&b| b == 0));
        // Empty damage = everything.
        let mut writes = 0;
        write_damage(&geometry, &frame, &[], &mut scratch, |_, _| {
            writes += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(writes, 3);
    }

    #[test]
    fn missing_device_is_reported() {
        let missing = Path::new("/dev/ultar-no-such-fb");
        assert_eq!(FbDev::open(Some(missing)).err(), Some(GfxError::NoDevice));
    }
}
