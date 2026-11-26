//! Samsung Exynos DECON output (Galaxy A30 with its Linux 4.4 kernel).
//!
//! On these panels the plain framebuffer path (`pwrite` + pan) shows nothing:
//! Android, and this backend, hand the display hardware an ION buffer
//! (dmabuf) through DECON's own `S3CFB_WIN_CONFIG` as one full-screen window.
//! Tested on the device with the first Ultar shell (`platform/ultar-shell`,
//! `docs` of the A30); the structures match the A30 kernel byte for byte
//! (`CONFIG_DPU_20=y`, `CONFIG_SUPPORT_MASK_LAYER=y`).
//!
//! 1. At open: `FBIOBLANK` POWERDOWN then UNBLANK (DECON starts in the
//!    bootloader's INIT state and skips the panel power sequence otherwise),
//!    one ION buffer of screen size (system heap), shared as dmabuf, mapped.
//! 2. Each frame: the damaged rows are copied into the buffer, then one
//!    `S3CFB_WIN_CONFIG` shows it; the fences the kernel returns are closed.

use crate::fbdev::{write_damage, FbGeometry};
use crate::GfxError;
use rustix::ioctl::{opcode, IntegerSetter, Opcode, Updater};
use rustix::mm::{mmap, munmap, MapFlags, ProtFlags};
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Mutex;
use ultar_hal::{
    Display, DisplayEvent, DisplayId, DisplayInfo, DisplayMode, FrameView, HalError, HalResult,
    Level, PixelFormat, Rect, Rotation,
};

/// The ION allocator of the Exynos 4.4 kernels.
pub const ION_PATH: &str = "/dev/ion";

#[repr(C)]
#[derive(Default)]
struct IonAllocation {
    len: u64,
    align: u64,
    heap_id_mask: u32,
    flags: u32,
    handle: i32,
}

#[repr(C)]
struct IonFd {
    handle: i32,
    fd: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Area {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Frame {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    f_w: u32,
    f_h: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DppParams {
    addr: [u64; 3],
    flip: i32,
    eq_mode: i32,
    comp_src: i32,
    hdr_std: i32,
    min_luminance: u32,
    max_luminance: u32,
}

/// The buffer member of the kernel's union (8-byte aligned like it).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WinBuf {
    fd_idma: [i32; 3],
    fence_fd: i32,
    rel_fence: i32,
    plane_alpha: i32,
    blending: i32,
    idma_type: i32,
    format: i32,
    dpp_parm: DppParams,
    block_area: Area,
    transparent_area: Area,
    opaque_area: Area,
    src: Frame,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WinConfig {
    state: i32,
    buf: WinBuf,
    dst: Frame,
    protection: u8,
    compression: u8,
}

const MAX_WIN: usize = 4;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WinConfigData {
    fence: i32,
    fd_odma: i32,
    config: [WinConfig; MAX_WIN + 1],
    remained_frames: i32,
    reserved: [u32; 7],
}

const _: () = assert!(std::mem::size_of::<WinConfig>() == 200);
const _: () = assert!(std::mem::size_of::<WinConfigData>() == 1040);
const _: () = assert!(std::mem::size_of::<IonAllocation>() == 32);

const ION_IOC_ALLOC: Opcode = opcode::read_write::<IonAllocation>(b'I', 0);
const ION_IOC_SHARE: Opcode = opcode::read_write::<IonFd>(b'I', 4);
const S3CFB_WIN_CONFIG: Opcode = opcode::write::<WinConfigData>(b'F', 209);
const FBIOGET_VSCREENINFO: Opcode = 0x4600;
const FBIOBLANK: Opcode = 0x4611;
const FB_BLANK_UNBLANK: usize = 0;
const FB_BLANK_POWERDOWN: usize = 4;
const WIN_STATE_BUFFER: i32 = 2;
/// Confirmed on the A30: for pixels 0x00RRGGBB in memory (B, G, R, X) BGRX shows true colours.
const FMT_BGRX_8888: i32 = 7;
/// DECON's default window (`decon->dt.dft_win`).
const DEFAULT_WINDOW: usize = 3;
const ASSUMED_REFRESH_HZ: u32 = 60;

/// The mapped ION buffer.
struct Mapping {
    ptr: NonNull<c_void>,
    len: usize,
}

// SAFETY: the mapping is owned by this value and only reached through the
// `Mutex` in `Decon`, like a `Vec<u8>`.
unsafe impl Send for Mapping {}

impl Mapping {
    fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: `ptr` maps `len` readable and writable bytes for the life
        // of `self`; the exclusive borrow excludes any other access here.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr().cast::<u8>(), self.len) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: exactly the mapping created in `Decon::open`.
        let _ = unsafe { munmap(self.ptr.as_ptr(), self.len) };
    }
}

/// A DECON display with its own ION buffer (always one output, id 0).
pub struct Decon {
    fb: File,
    _ion: File,
    buffer: OwnedFd,
    mapping: Mutex<Mapping>,
    geometry: FbGeometry,
}

impl std::fmt::Debug for Decon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decon")
            .field("geometry", &self.geometry)
            .finish_non_exhaustive()
    }
}

fn open_rw(path: &Path) -> Result<File, GfxError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => GfxError::NoDevice,
            _ => GfxError::from(e),
        })
}

/// Whether this device has the Exynos ION allocator (then DECON is worth a try).
pub fn present() -> bool {
    Path::new(ION_PATH).exists()
}

impl Decon {
    /// Opens DECON through the framebuffer device `fb` (`None`: `/dev/fb0`).
    pub fn open(fb: Option<&Path>) -> Result<Decon, GfxError> {
        let ion = open_rw(Path::new(ION_PATH))?;
        let fb = open_rw(fb.unwrap_or(Path::new(crate::fbdev::DEFAULT_PATH)))?;
        let mut var = [0u32; 40];
        // SAFETY: FBIOGET_VSCREENINFO writes a `struct fb_var_screeninfo` (40 u32).
        unsafe {
            rustix::ioctl::ioctl(
                &fb,
                Updater::<FBIOGET_VSCREENINFO, [u32; 40]>::new(&mut var),
            )
        }?;
        let (width, height) = (var[0], var[1]);
        if width == 0 || height == 0 {
            return Err(GfxError::InvalidArgument);
        }
        power_on(&fb);
        let geometry = FbGeometry {
            width,
            height,
            line_bytes: width * 4,
            swap_red_blue: false,
            origin: 0,
        };
        let len = geometry.line_bytes as usize * height as usize;
        let buffer = ion_buffer(&ion, len)?;
        // SAFETY: a fresh shared mapping (kernel chooses the address) of the
        // `len`-byte dmabuf just allocated; nothing else in this process maps it.
        let raw = unsafe {
            mmap(
                std::ptr::null_mut(),
                len,
                ProtFlags::READ | ProtFlags::WRITE,
                MapFlags::SHARED,
                &buffer,
                0,
            )
        }?;
        let ptr = NonNull::new(raw).ok_or(GfxError::Sys(12))?;
        Ok(Decon {
            fb,
            _ion: ion,
            buffer,
            mapping: Mutex::new(Mapping { ptr, len }),
            geometry,
        })
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

    /// Shows the buffer as one full-screen window and closes the fences the kernel hands back.
    fn flip(&self) -> Result<(), GfxError> {
        let mut data = window_config(self.buffer.as_raw_fd(), &self.geometry);
        // SAFETY: S3CFB_WIN_CONFIG reads a `decon_win_config_data` (1040 bytes,
        // checked above) and writes fence descriptors back into it.
        let result = unsafe {
            rustix::ioctl::ioctl(
                &self.fb,
                Updater::<S3CFB_WIN_CONFIG, WinConfigData>::new(&mut data),
            )
        };
        close_fences(&data, self.buffer.as_raw_fd());
        result.map_err(GfxError::from)
    }
}

/// POWERDOWN then UNBLANK: forces the full DSIM/panel power sequence.
fn power_on(fb: &File) {
    for (mode, pause) in [(FB_BLANK_POWERDOWN, 60), (FB_BLANK_UNBLANK, 0)] {
        // SAFETY: FBIOBLANK takes the blank mode as an integer argument.
        let _ = unsafe { rustix::ioctl::ioctl(fb, IntegerSetter::<FBIOBLANK>::new_usize(mode)) };
        std::thread::sleep(std::time::Duration::from_millis(pause));
    }
}

/// One ION buffer of `len` bytes from the system heap, as a dmabuf descriptor.
fn ion_buffer(ion: &File, len: usize) -> Result<OwnedFd, GfxError> {
    let mut alloc = IonAllocation {
        len: len as u64,
        heap_id_mask: 1,
        ..IonAllocation::default()
    };
    // SAFETY: ION_IOC_ALLOC reads and writes a `struct ion_allocation_data` (32 bytes).
    unsafe {
        rustix::ioctl::ioctl(
            ion,
            Updater::<ION_IOC_ALLOC, IonAllocation>::new(&mut alloc),
        )
    }?;
    let mut share = IonFd {
        handle: alloc.handle,
        fd: -1,
    };
    // SAFETY: ION_IOC_SHARE reads the handle and writes a new descriptor into `fd`.
    unsafe { rustix::ioctl::ioctl(ion, Updater::<ION_IOC_SHARE, IonFd>::new(&mut share)) }?;
    if share.fd < 0 {
        return Err(GfxError::NoDevice);
    }
    // SAFETY: the kernel just created `share.fd` for this process; nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(share.fd) })
}

/// The window list: every window off except the default one, which shows `buffer` full screen.
fn window_config(buffer: i32, g: &FbGeometry) -> WinConfigData {
    let mut data = WinConfigData {
        fence: -1,
        fd_odma: -1,
        ..WinConfigData::default()
    };
    for c in &mut data.config {
        c.buf.fd_idma = [-1; 3];
        c.buf.fence_fd = -1;
        c.buf.rel_fence = -1;
    }
    let full = Frame {
        x: 0,
        y: 0,
        w: g.width,
        h: g.height,
        f_w: g.width,
        f_h: g.height,
    };
    let c = &mut data.config[DEFAULT_WINDOW];
    c.state = WIN_STATE_BUFFER;
    c.buf.fd_idma[0] = buffer;
    c.buf.plane_alpha = 255;
    c.buf.format = FMT_BGRX_8888;
    c.buf.src = full;
    c.dst = full;
    data
}

/// Closes every descriptor the kernel wrote into `data` (all fields were -1 before).
fn close_fences(data: &WinConfigData, buffer: i32) {
    let mut seen = Vec::with_capacity(MAX_WIN + 2);
    let fences = std::iter::once(data.fence).chain(data.config.iter().map(|c| c.buf.rel_fence));
    for fd in fences {
        if fd >= 3 && fd != buffer && !seen.contains(&fd) {
            seen.push(fd);
            // SAFETY: the kernel created these sync files for this process
            // and nothing else refers to them; dropping closes them once.
            drop(unsafe { OwnedFd::from_raw_fd(fd) });
        }
    }
}

impl Display for Decon {
    fn displays(&self) -> HalResult<Vec<DisplayInfo>> {
        Ok(vec![DisplayInfo {
            id: DisplayId(0),
            connector: "decon".into(),
            internal: true,
            physical_mm: ultar_hal::Size::new(0, 0),
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
        {
            let mut mapping = self.mapping.lock().map_err(|_| HalError::Busy)?;
            let screen = mapping.bytes_mut();
            let mut scratch = Vec::new();
            write_damage(
                &self.geometry,
                frame,
                damage,
                &mut scratch,
                |offset, bytes| {
                    let start = usize::try_from(offset).unwrap_or(usize::MAX);
                    screen
                        .get_mut(start..start.saturating_add(bytes.len()))
                        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?
                        .copy_from_slice(bytes);
                    Ok(())
                },
            )?;
        }
        self.flip()?;
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

    #[test]
    fn only_the_default_window_shows_the_buffer_full_screen() {
        let g = FbGeometry {
            width: 1080,
            height: 2340,
            line_bytes: 4320,
            swap_red_blue: false,
            origin: 0,
        };
        let d = window_config(9, &g);
        assert_eq!((d.fence, d.fd_odma), (-1, -1));
        for (i, c) in d.config.iter().enumerate() {
            if i == DEFAULT_WINDOW {
                assert_eq!(c.state, WIN_STATE_BUFFER);
                assert_eq!(c.buf.fd_idma, [9, -1, -1]);
                assert_eq!((c.buf.format, c.buf.plane_alpha), (FMT_BGRX_8888, 255));
                assert_eq!((c.dst.w, c.dst.h, c.buf.src.f_h), (1080, 2340, 2340));
            } else {
                assert_eq!(c.state, 0);
                assert_eq!(c.buf.fd_idma, [-1; 3]);
            }
            assert_eq!((c.buf.fence_fd, c.buf.rel_fence), (-1, -1));
        }
    }

    #[test]
    fn the_opcodes_match_the_a30_kernel() {
        // _IOW('F', 209, 1040 bytes) and _IOWR('I', 0/4, …) as in the tested first shell.
        assert_eq!(
            S3CFB_WIN_CONFIG as u64,
            (1 << 30) | (1040 << 16) | (0x46 << 8) | 209
        );
        assert_eq!(ION_IOC_ALLOC as u64, (3 << 30) | (32 << 16) | (0x49 << 8));
        assert_eq!(
            ION_IOC_SHARE as u64,
            (3 << 30) | (8 << 16) | (0x49 << 8) | 4
        );
    }

    #[test]
    fn without_ion_there_is_no_decon() {
        if !present() {
            assert_eq!(Decon::open(None).err(), Some(GfxError::NoDevice));
        }
    }
}
