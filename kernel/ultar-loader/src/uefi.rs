//! The few UEFI services the loader uses (UEFI 2.10), as thin wrappers. Every raw call is here.

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};

/// Opaque firmware handle.
pub type Handle = *mut c_void;
/// UEFI status code (0 = success).
pub type Status = usize;
/// `EFI_NOT_FOUND`.
pub const NOT_FOUND: Status = (1 << 63) | 14;
/// `EFI_LOAD_ERROR`.
pub const LOAD_ERROR: Status = (1 << 63) | 1;

/// A protocol or table GUID.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Guid(pub u32, pub u16, pub u16, pub [u8; 8]);

/// `EFI_LOADED_IMAGE_PROTOCOL`.
pub const LOADED_IMAGE: Guid = Guid(
    0x5b1b31a1,
    0x9562,
    0x11d2,
    [0x8e, 0x3f, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
);
/// `EFI_DEVICE_PATH_PROTOCOL`.
pub const DEVICE_PATH: Guid = Guid(
    0x09576e91,
    0x6d3f,
    0x11d2,
    [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
);
/// `EFI_BLOCK_IO_PROTOCOL`.
pub const BLOCK_IO: Guid = Guid(
    0x964e5b21,
    0x6459,
    0x11d2,
    [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
);

#[repr(C)]
struct TableHeader {
    signature: u64,
    revision: u32,
    size: u32,
    crc32: u32,
    reserved: u32,
}

#[repr(C)]
struct TextOut {
    reset: usize,
    output_string: extern "efiapi" fn(*mut TextOut, *const u16) -> Status,
}

/// `EFI_SYSTEM_TABLE` (the fields the loader reads).
#[repr(C)]
pub struct SystemTable {
    hdr: TableHeader,
    firmware_vendor: *const u16,
    firmware_revision: u32,
    con_in_handle: Handle,
    con_in: *mut c_void,
    con_out_handle: Handle,
    con_out: *mut TextOut,
    stderr_handle: Handle,
    stderr: *mut c_void,
    runtime: *mut Services<14>,
    boot: *mut Services<44>,
}

#[repr(C)]
struct Services<const N: usize> {
    hdr: TableHeader,
    f: [usize; N],
}

const ALLOCATE_PAGES: usize = 2;
const FREE_PAGES: usize = 3;
const HANDLE_PROTOCOL: usize = 16;
const LOAD_IMAGE: usize = 22;
const START_IMAGE: usize = 23;
const UNLOAD_IMAGE: usize = 25;
const STALL: usize = 28;
const SET_WATCHDOG: usize = 29;
const LOCATE_HANDLE_BUFFER: usize = 36;
/// UEFI 2.10 §4.5: GetTime 0 … GetNextHighMonotonicCount 9, ResetSystem 10 (11 is UpdateCapsule).
const RT_RESET_SYSTEM: usize = 10;

static SYSTEM: AtomicPtr<SystemTable> = AtomicPtr::new(core::ptr::null_mut());

/// Remembers the system table for the wrappers below; called once at entry.
pub fn init(st: *mut SystemTable) {
    SYSTEM.store(st, Ordering::Relaxed);
}

fn boot_fn(index: usize) -> usize {
    let st = SYSTEM.load(Ordering::Relaxed);
    // SAFETY: `st` is the system table the firmware passed to the entry point (init); boot
    // services stay valid because the loader never calls ExitBootServices.
    unsafe { (*(*st).boot).f[index] }
}

/// Writes `text` on the firmware console (best effort).
pub fn print(text: &str) {
    let st = SYSTEM.load(Ordering::Relaxed);
    // SAFETY: see `boot_fn`; ConOut may be null on headless firmware, which is checked.
    let out = unsafe { (*st).con_out };
    if out.is_null() {
        return;
    }
    let mut buf = [0u16; 128];
    let mut n = 0;
    for unit in text.encode_utf16() {
        buf[n] = unit;
        n += 1;
        if n == buf.len() - 1 {
            buf[n] = 0;
            // SAFETY: `out` is the firmware's text output protocol, `buf` is 0-terminated.
            unsafe { ((*out).output_string)(out, buf.as_ptr()) };
            n = 0;
        }
    }
    buf[n] = 0;
    // SAFETY: as above.
    unsafe { ((*out).output_string)(out, buf.as_ptr()) };
}

/// The protocol interface `guid` on `handle`, or null.
pub fn protocol(handle: Handle, guid: &Guid) -> *mut c_void {
    let f = boot_fn(HANDLE_PROTOCOL);
    // SAFETY: HandleProtocol's signature per UEFI 2.10 §7.3; `out` is a valid pointer.
    let hp: extern "efiapi" fn(Handle, *const Guid, *mut *mut c_void) -> Status =
        unsafe { core::mem::transmute(f) };
    let mut out = core::ptr::null_mut();
    if hp(handle, guid, &mut out) != 0 {
        return core::ptr::null_mut();
    }
    out
}

type LocateHandleBuffer =
    extern "efiapi" fn(u32, *const Guid, *mut c_void, *mut usize, *mut *mut Handle) -> Status;

/// All handles that carry `guid` (firmware-owned buffer; it is not freed – the loader is
/// short-lived and the buffer is tiny).
pub fn handles(guid: &Guid) -> &'static [Handle] {
    let f = boot_fn(LOCATE_HANDLE_BUFFER);
    // SAFETY: LocateHandleBuffer's signature per UEFI 2.10 §7.3 (search type 2 = ByProtocol).
    let lhb: LocateHandleBuffer = unsafe { core::mem::transmute(f) };
    let (mut n, mut list) = (0usize, core::ptr::null_mut());
    if lhb(2, guid, core::ptr::null_mut(), &mut n, &mut list) != 0 || list.is_null() {
        return &[];
    }
    // SAFETY: the firmware returned `n` handles at `list`, valid until freed (never).
    unsafe { core::slice::from_raw_parts(list, n) }
}

/// Firmware pages that are freed on drop.
pub struct Pages {
    addr: u64,
    count: usize,
}

impl Pages {
    /// `bytes` rounded up to whole 4 KiB pages, zeroed. `None` when the firmware has no memory.
    pub fn new(bytes: usize) -> Option<Self> {
        let count = bytes.div_ceil(4096).max(1);
        let f = boot_fn(ALLOCATE_PAGES);
        // SAFETY: AllocatePages per UEFI 2.10 §7.2 (AllocateAnyPages, EfiLoaderData).
        let ap: extern "efiapi" fn(u32, u32, usize, *mut u64) -> Status =
            unsafe { core::mem::transmute(f) };
        let mut addr = 0u64;
        if ap(0, 2, count, &mut addr) != 0 || addr == 0 {
            return None;
        }
        let mut pages = Self { addr, count };
        pages.bytes().fill(0);
        Some(pages)
    }

    /// The memory as bytes.
    pub fn bytes(&mut self) -> &mut [u8] {
        // SAFETY: the firmware gave us `count` pages at `addr`, owned by this value.
        unsafe { core::slice::from_raw_parts_mut(self.addr as *mut u8, self.count * 4096) }
    }
}

impl Drop for Pages {
    fn drop(&mut self) {
        let f = boot_fn(FREE_PAGES);
        // SAFETY: FreePages per UEFI 2.10 §7.2 on pages this value allocated.
        let fp: extern "efiapi" fn(u64, usize) -> Status = unsafe { core::mem::transmute(f) };
        fp(self.addr, self.count);
    }
}

/// Loads a PE image from memory; `path` is the device path the image is said to come from.
pub fn load_image(parent: Handle, path: *const u8, image: &[u8]) -> Result<Handle, Status> {
    let f = boot_fn(LOAD_IMAGE);
    // SAFETY: LoadImage per UEFI 2.10 §7.4 with a source buffer (BootPolicy = FALSE).
    let li: extern "efiapi" fn(u8, Handle, *const u8, *const c_void, usize, *mut Handle) -> Status =
        unsafe { core::mem::transmute(f) };
    let mut child = core::ptr::null_mut();
    match li(
        0,
        parent,
        path,
        image.as_ptr().cast(),
        image.len(),
        &mut child,
    ) {
        0 => Ok(child),
        e => Err(e),
    }
}

/// Starts a loaded image; returns only if it exits or fails.
pub fn start_image(child: Handle) -> Status {
    let f = boot_fn(START_IMAGE);
    // SAFETY: StartImage per UEFI 2.10 §7.4 on a handle returned by LoadImage.
    let si: extern "efiapi" fn(Handle, *mut usize, *mut *mut u16) -> Status =
        unsafe { core::mem::transmute(f) };
    si(child, core::ptr::null_mut(), core::ptr::null_mut())
}

/// Unloads an image that was loaded but did not run to the end.
pub fn unload_image(child: Handle) {
    let f = boot_fn(UNLOAD_IMAGE);
    // SAFETY: UnloadImage per UEFI 2.10 §7.4.
    let ui: extern "efiapi" fn(Handle) -> Status = unsafe { core::mem::transmute(f) };
    ui(child);
}

/// Arms (seconds > 0) or disarms (0) the firmware watchdog.
pub fn watchdog(seconds: usize) {
    let f = boot_fn(SET_WATCHDOG);
    // SAFETY: SetWatchdogTimer per UEFI 2.10 §7.5; code 0x10001 is outside the reserved range.
    let wd: extern "efiapi" fn(usize, u64, usize, *const u16) -> Status =
        unsafe { core::mem::transmute(f) };
    wd(seconds, 0x1_0001, 0, core::ptr::null());
}

/// Waits `micros` microseconds.
pub fn stall(micros: usize) {
    let f = boot_fn(STALL);
    // SAFETY: Stall per UEFI 2.10 §7.5.
    let s: extern "efiapi" fn(usize) -> Status = unsafe { core::mem::transmute(f) };
    s(micros);
}

/// Cold reset through the runtime services (used after a loader panic).
pub fn reset() -> ! {
    let st = SYSTEM.load(Ordering::Relaxed);
    if !st.is_null() {
        // SAFETY: ResetSystem per UEFI 2.10 §8.5.1; runtime services stay valid.
        let rs: extern "efiapi" fn(u32, Status, usize, *const c_void) -> Status =
            unsafe { core::mem::transmute((*(*st).runtime).f[RT_RESET_SYSTEM]) };
        let _ = rs(0, 0, 0, core::ptr::null());
    }
    loop {
        core::hint::spin_loop();
    }
}

/// The `EFI_LOADED_IMAGE_PROTOCOL` fields the loader touches.
#[repr(C)]
pub struct LoadedImage {
    /// Revision.
    pub revision: u32,
    /// Parent image.
    pub parent: Handle,
    /// System table.
    pub system: *mut SystemTable,
    /// Device the image was loaded from.
    pub device: Handle,
    /// File path of the image.
    pub file_path: *const u8,
    /// Reserved.
    pub reserved: *mut c_void,
    /// Size of the load options in bytes.
    pub options_size: u32,
    /// The load options.
    pub options: *const c_void,
}
