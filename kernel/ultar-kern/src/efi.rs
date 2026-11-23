//! Minimale UEFI-Anbindung – nur das, was der Kern bis zum Verlassen der Firmware braucht:
//! Textausgabe, Grafik (GOP), Speicherkarte, ACPI-Tabelle, ExitBootServices.
//! Danach gehört die Maschine allein dem Ultar-Kern.

use core::ffi::c_void;

pub type Handle = *mut c_void;
pub type Status = usize;

#[repr(C)]
pub struct TableHeader {
    pub signature: u64,
    pub revision: u32,
    pub size: u32,
    pub crc32: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct SimpleTextOutput {
    pub reset: usize,
    pub output_string: extern "efiapi" fn(*mut SimpleTextOutput, *const u16) -> Status,
}

#[repr(C)]
pub struct ConfigTable {
    pub guid: Guid,
    pub table: *mut c_void,
}

#[repr(C)]
pub struct SystemTable {
    pub hdr: TableHeader,
    pub firmware_vendor: *const u16,
    pub firmware_revision: u32,
    pub con_in_handle: Handle,
    pub con_in: *mut c_void,
    pub con_out_handle: Handle,
    pub con_out: *mut SimpleTextOutput,
    pub stderr_handle: Handle,
    pub stderr: *mut c_void,
    pub runtime: *mut RuntimeServices,
    pub boot: *mut BootServices,
    pub n_config: usize,
    pub config: *const ConfigTable,
}

/// Boot-Dienste als Tabelle von Funktionszeigern (Reihenfolge laut UEFI-Spezifikation 2.10, Abschnitt 4.4)
#[repr(C)]
pub struct BootServices {
    pub hdr: TableHeader,
    pub f: [usize; 44],
}

#[repr(C)]
pub struct RuntimeServices {
    pub hdr: TableHeader,
    pub f: [usize; 14],
}

const BS_ALLOCATE_PAGES: usize = 2;
const BS_GET_MEMORY_MAP: usize = 4;
const BS_ALLOCATE_POOL: usize = 5;
const BS_EXIT_BOOT_SERVICES: usize = 26;
const BS_SET_WATCHDOG: usize = 29;
const BS_LOCATE_PROTOCOL: usize = 37;
/// ResetSystem in den Laufzeitdiensten
const RT_RESET_SYSTEM: usize = 11;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Guid(pub u32, pub u16, pub u16, pub [u8; 8]);

pub const GOP_GUID: Guid = Guid(0x9042a9de, 0x23dc, 0x4a38, [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a]);
pub const ACPI20_GUID: Guid = Guid(0x8868e871, 0xe4f1, 0x11d3, [0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81]);
pub const SMBIOS_GUID: Guid = Guid(0xeb9d2d31, 0x2d88, 0x11d3, [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d]);
pub const SMBIOS3_GUID: Guid = Guid(0xf2fd1544, 0x9794, 0x4a2c, [0x99, 0x2e, 0xe5, 0xbb, 0xcf, 0x20, 0xe3, 0x94]);

#[repr(C)]
pub struct GopModeInfo {
    pub version: u32,
    pub hres: u32,
    pub vres: u32,
    /// 0 = RGB(X), 1 = BGR(X), 2 = Bitmaske, 3 = nur Blt
    pub pixel_format: u32,
    pub masks: [u32; 4],
    pub stride: u32,
}

#[repr(C)]
pub struct GopMode {
    pub max_mode: u32,
    pub mode: u32,
    pub info: *const GopModeInfo,
    pub info_size: usize,
    pub fb_base: u64,
    pub fb_size: usize,
}

#[repr(C)]
pub struct Gop {
    pub query_mode: usize,
    pub set_mode: usize,
    pub blt: usize,
    pub mode: *const GopMode,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemDesc {
    pub typ: u32,
    pub pad: u32,
    pub phys: u64,
    pub virt: u64,
    pub pages: u64,
    pub attr: u64,
}

/// Speicherarten, die nach ExitBootServices frei sind
pub fn usable(typ: u32) -> bool {
    // 1/2 Lader-Code/-Daten (enthält den Kern selbst → nicht frei), 3/4 Boot-Dienste, 7 konventionell
    matches!(typ, 3 | 4 | 7)
}

/// Bildschirm, den die Firmware eingerichtet hat
#[derive(Clone, Copy)]
pub struct Framebuffer {
    pub base: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgr: bool,
}

pub struct Boot {
    pub fb: Option<Framebuffer>,
    pub rsdp: u64,
    /// SMBIOS-Einstiegspunkt (3.x bevorzugt), 0 = keiner
    pub smbios: u64,
    pub mem: &'static [u8],
    pub desc_size: usize,
    pub runtime: *mut RuntimeServices,
}

impl Boot {
    pub fn regions(&self) -> impl Iterator<Item = MemDesc> + '_ {
        self.mem.chunks_exact(self.desc_size).map(|c| unsafe { core::ptr::read_unaligned(c.as_ptr() as *const MemDesc) })
    }
}

pub fn print(st: *mut SystemTable, s: &str) {
    unsafe {
        let out = (*st).con_out;
        if out.is_null() {
            return;
        }
        let mut buf = [0u16; 128];
        let mut i = 0;
        for c in s.encode_utf16() {
            if c == b'\n' as u16 {
                buf[i] = b'\r' as u16;
                i += 1;
            }
            buf[i] = c;
            i += 1;
            if i >= buf.len() - 3 {
                buf[i] = 0;
                ((*out).output_string)(out, buf.as_ptr());
                i = 0;
            }
        }
        buf[i] = 0;
        ((*out).output_string)(out, buf.as_ptr());
    }
}

unsafe fn bs(st: *mut SystemTable, idx: usize) -> usize {
    (*(*st).boot).f[idx]
}

/// Alles Nötige von der Firmware holen und sie danach verlassen.
pub unsafe fn take_over(image: Handle, st: *mut SystemTable) -> Boot {
    // Wachhund der Firmware aus (sonst Neustart nach 5 Minuten)
    let wd: extern "efiapi" fn(usize, u64, usize, *const u16) -> Status = core::mem::transmute(bs(st, BS_SET_WATCHDOG));
    wd(0, 0, 0, core::ptr::null());

    // Grafik
    let locate: extern "efiapi" fn(*const Guid, *mut c_void, *mut *mut c_void) -> Status = core::mem::transmute(bs(st, BS_LOCATE_PROTOCOL));
    let mut gop: *mut c_void = core::ptr::null_mut();
    let fb = if locate(&GOP_GUID, core::ptr::null_mut(), &mut gop) == 0 && !gop.is_null() {
        let m = &*(*(gop as *const Gop)).mode;
        let info = &*m.info;
        (info.pixel_format <= 1).then_some(Framebuffer { base: m.fb_base, width: info.hres, height: info.vres, stride: info.stride, bgr: info.pixel_format == 1 })
    } else {
        None
    };

    // ACPI-Wurzel (für Ausschalten, Prozessoren, PCIe-Bereich)
    let mut rsdp = 0u64;
    let mut smbios = 0u64;
    for i in 0..(*st).n_config {
        let c = &*(*st).config.add(i);
        if c.guid == ACPI20_GUID {
            rsdp = c.table as u64;
        }
        if c.guid == SMBIOS3_GUID || (c.guid == SMBIOS_GUID && smbios == 0) {
            smbios = c.table as u64;
        }
    }

    // Puffer für die Speicherkarte (bleibt nach dem Verlassen gültig: Lader-Daten)
    let alloc_pool: extern "efiapi" fn(u32, usize, *mut *mut u8) -> Status = core::mem::transmute(bs(st, BS_ALLOCATE_POOL));
    let get_map: extern "efiapi" fn(*mut usize, *mut u8, *mut usize, *mut usize, *mut u32) -> Status = core::mem::transmute(bs(st, BS_GET_MEMORY_MAP));
    let exit: extern "efiapi" fn(Handle, usize) -> Status = core::mem::transmute(bs(st, BS_EXIT_BOOT_SERVICES));
    let cap = 64 * 1024;
    let mut buf: *mut u8 = core::ptr::null_mut();
    if alloc_pool(2, cap, &mut buf) != 0 {
        print(st, "Ultar-Kern: kein Speicher für die Speicherkarte\n");
        loop {}
    }
    let runtime = (*st).runtime;
    let mut tries = 0;
    loop {
        let (mut size, mut key, mut dsize, mut dver) = (cap, 0usize, 0usize, 0u32);
        if get_map(&mut size, buf, &mut key, &mut dsize, &mut dver) != 0 {
            print(st, "Ultar-Kern: GetMemoryMap fehlgeschlagen\n");
            loop {}
        }
        // Ab hier keine Firmware-Ausgabe mehr: jeder Aufruf kann die Karte ändern
        if exit(image, key) == 0 {
            return Boot { fb, rsdp, smbios, mem: core::slice::from_raw_parts(buf, size), desc_size: dsize, runtime };
        }
        tries += 1;
        if tries > 4 {
            loop {}
        }
    }
}

// ------------------------------------------------------------------ Kettenstart vom Installationsmedium

pub const LOADED_IMAGE_GUID: Guid = Guid(0x5b1b31a1, 0x9562, 0x11d2, [0x8e, 0x3f, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b]);
pub const SIMPLE_FS_GUID: Guid = Guid(0x964e5b22, 0x6459, 0x11d2, [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b]);
pub const DEVICE_PATH_GUID: Guid = Guid(0x09576e91, 0x6d3f, 0x11d2, [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b]);
const BS_HANDLE_PROTOCOL: usize = 16;
const BS_LOAD_IMAGE: usize = 22;
const BS_START_IMAGE: usize = 23;
const BS_STALL: usize = 28;
const BS_LOCATE_HANDLE_BUFFER: usize = 36;

#[repr(C)]
struct FileProto {
    revision: u64,
    open: extern "efiapi" fn(*mut FileProto, *mut *mut FileProto, *const u16, u64, u64) -> Status,
    close: extern "efiapi" fn(*mut FileProto) -> Status,
    delete: usize,
    read: extern "efiapi" fn(*mut FileProto, *mut usize, *mut c_void) -> Status,
}

#[repr(C)]
struct SimpleFs {
    revision: u64,
    open_volume: extern "efiapi" fn(*mut SimpleFs, *mut *mut FileProto) -> Status,
}

#[repr(C)]
struct SimpleTextIn {
    reset: usize,
    read_key: extern "efiapi" fn(*mut SimpleTextIn, *mut [u16; 2]) -> Status,
}

fn u16z(s: &str, buf: &mut [u16; 64]) -> *const u16 {
    let mut i = 0;
    for c in s.encode_utf16().take(63) {
        buf[i] = c;
        i += 1;
    }
    buf[i] = 0;
    buf.as_ptr()
}

unsafe fn protokoll(st: *mut SystemTable, h: Handle, g: &Guid) -> *mut c_void {
    let hp: extern "efiapi" fn(Handle, *const Guid, *mut *mut c_void) -> Status = core::mem::transmute(bs(st, BS_HANDLE_PROTOCOL));
    let mut p: *mut c_void = core::ptr::null_mut();
    if hp(h, g, &mut p) != 0 {
        return core::ptr::null_mut();
    }
    p
}

/// Gibt es die Datei auf dem Dateisystem dieses Geräts?
unsafe fn datei_da(st: *mut SystemTable, h: Handle, pfad: &str) -> bool {
    let fs = protokoll(st, h, &SIMPLE_FS_GUID) as *mut SimpleFs;
    if fs.is_null() {
        return false;
    }
    let mut root: *mut FileProto = core::ptr::null_mut();
    if ((*fs).open_volume)(fs, &mut root) != 0 || root.is_null() {
        return false;
    }
    let mut b = [0u16; 64];
    let mut f: *mut FileProto = core::ptr::null_mut();
    let ok = ((*root).open)(root, &mut f, u16z(pfad, &mut b), 1, 0) == 0 && !f.is_null();
    if ok {
        ((*f).close)(f);
    }
    ((*root).close)(root);
    ok
}

/// Gerätepfad bis zum ersten Datenträger-Knoten (Typ 4 = Medium/Partition): kennzeichnet die Platte
unsafe fn platten_praefix(st: *mut SystemTable, h: Handle) -> Option<(*const u8, usize)> {
    let dp = protokoll(st, h, &DEVICE_PATH_GUID) as *const u8;
    if dp.is_null() {
        return None;
    }
    let mut len = 0usize;
    loop {
        let t = *dp.add(len);
        let l = u16::from_le_bytes([*dp.add(len + 2), *dp.add(len + 3)]) as usize;
        if t == 0x7f || t == 4 || len > 4096 {
            break;
        }
        len += l.max(4);
    }
    Some((dp, len))
}

unsafe fn gleiche_platte(st: *mut SystemTable, a: Handle, b: Handle) -> bool {
    match (platten_praefix(st, a), platten_praefix(st, b)) {
        (Some((pa, la)), Some((pb, lb))) => la == lb && core::slice::from_raw_parts(pa, la) == core::slice::from_raw_parts(pb, lb),
        _ => false,
    }
}

/// Vom Installationsmedium (Datei \ultar-installer) gestartet und eine installierte Platte vorhanden
/// (\ultar\installiert oder \ultar\vmlinuz)? Dann nach 3 Sekunden deren Startprogramm starten –
/// Taste „i“ bleibt in der Installation.
/// vom Installationsmedium gestartet (Datei \ultar-installer auf der eigenen Startpartition)
pub static VOM_INSTALLER: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub unsafe fn kettenstart(image: Handle, st: *mut SystemTable) {
    let li = protokoll(st, image, &LOADED_IMAGE_GUID);
    if li.is_null() {
        return;
    }
    let eigenes = *((li as *const u8).add(24) as *const Handle);
    let marke = datei_da(st, eigenes, "\\ultar-installer");
    if !marke {
        return;
    }
    VOM_INSTALLER.store(true, core::sync::atomic::Ordering::Relaxed);
    let lhb: extern "efiapi" fn(u32, *const Guid, *mut c_void, *mut usize, *mut *mut Handle) -> Status = core::mem::transmute(bs(st, BS_LOCATE_HANDLE_BUFFER));
    let (mut n, mut hs): (usize, *mut Handle) = (0, core::ptr::null_mut());
    if lhb(2, &SIMPLE_FS_GUID, core::ptr::null_mut(), &mut n, &mut hs) != 0 {
        return;
    }
    let mut ziel: Handle = core::ptr::null_mut();
    for i in 0..n {
        let h = *hs.add(i);
        if h == eigenes || gleiche_platte(st, h, eigenes) {
            continue; // eigene Platte (Nutzlast-Partition des Mediums) überspringen
        }
        if datei_da(st, h, "\\EFI\\BOOT\\BOOTX64.EFI") && (datei_da(st, h, "\\ultar\\installiert") || datei_da(st, h, "\\ultar\\vmlinuz")) {
            ziel = h;
            break;
        }
    }
    if ziel.is_null() {
        return;
    }
    print(st, "\nUltar OS ist bereits installiert – Start in 3 s.   Taste I: Installation\nUltar OS installed - starting in 3 s.   Key I: installer\nUltar OS مثبت – يبدأ خلال 3 ثوانٍ. اضغط I للتثبيت\n");
    let stall: extern "efiapi" fn(usize) -> Status = core::mem::transmute(bs(st, BS_STALL));
    let cin = (*st).con_in as *mut SimpleTextIn;
    for _ in 0..30 {
        if !cin.is_null() {
            let mut k = [0u16; 2];
            if ((*cin).read_key)(cin, &mut k) == 0 && (k[1] == b'i' as u16 || k[1] == b'I' as u16) {
                print(st, "-> Installation\n");
                return;
            }
        }
        stall(100_000);
    }
    // Gerätepfad des Ziels + Dateipfad-Knoten + Ende
    let dp = protokoll(st, ziel, &DEVICE_PATH_GUID) as *const u8;
    if dp.is_null() {
        return;
    }
    let mut len = 0usize;
    loop {
        let t = *dp.add(len);
        let st_ = *dp.add(len + 1);
        let l = u16::from_le_bytes([*dp.add(len + 2), *dp.add(len + 3)]) as usize;
        if t == 0x7f && st_ == 0xff {
            break;
        }
        len += l.max(4);
        if len > 4096 {
            return;
        }
    }
    // (noch kein Kern-Speicher: alles auf dem Stapel)
    let mut datei = [0u16; 32];
    let mut dn = 0;
    for c in "\\EFI\\BOOT\\BOOTX64.EFI".encode_utf16() {
        datei[dn] = c;
        dn += 1;
    }
    dn += 1; // Endnull
    let datei = &datei[..dn];
    let fl = 4 + datei.len() * 2;
    if len + fl + 4 > 2048 {
        return;
    }
    let mut pfad = [0u8; 2048];
    core::ptr::copy_nonoverlapping(dp, pfad.as_mut_ptr(), len);
    pfad[len] = 4;
    pfad[len + 1] = 4;
    pfad[len + 2..len + 4].copy_from_slice(&(fl as u16).to_le_bytes());
    for (i, c) in datei.iter().enumerate() {
        pfad[len + 4 + i * 2..len + 6 + i * 2].copy_from_slice(&c.to_le_bytes());
    }
    let e = len + fl;
    pfad[e] = 0x7f;
    pfad[e + 1] = 0xff;
    pfad[e + 2] = 4;
    let load: extern "efiapi" fn(u8, Handle, *const u8, *const c_void, usize, *mut Handle) -> Status = core::mem::transmute(bs(st, BS_LOAD_IMAGE));
    let start: extern "efiapi" fn(Handle, *mut usize, *mut *mut u16) -> Status = core::mem::transmute(bs(st, BS_START_IMAGE));
    let mut neu: Handle = core::ptr::null_mut();
    if load(0, image, pfad.as_ptr(), core::ptr::null(), 0, &mut neu) == 0 {
        start(neu, core::ptr::null_mut(), core::ptr::null_mut());
    }
    print(st, "Start der installierten Platte fehlgeschlagen – Installation\n");
}

/// The load options this image was started with (UTF-16 units, as the parent set them), copied
/// into `out`; empty when there are none.
// SAFETY: callers pass the image handle and system table the firmware gave and call it before
// ExitBootServices (boot services and the loaded image protocol are valid then).
pub unsafe fn load_options(image: Handle, st: *mut SystemTable, out: &mut [u16]) -> usize {
    let li = protokoll(st, image, &LOADED_IMAGE_GUID) as *const u8;
    if li.is_null() {
        return 0;
    }
    // SAFETY: EFI_LOADED_IMAGE_PROTOCOL (UEFI 2.10 §9.1): LoadOptionsSize (u32) at offset 48,
    // LoadOptions (pointer) at offset 56; the firmware keeps both valid while the image runs.
    let size = core::ptr::read_unaligned(li.add(48) as *const u32) as usize;
    let ptr = core::ptr::read_unaligned(li.add(56) as *const *const u16);
    if ptr.is_null() {
        return 0;
    }
    let n = (size / 2).min(out.len());
    for (i, slot) in out.iter_mut().take(n).enumerate() {
        // SAFETY: inside the LoadOptionsSize bytes the firmware gave (n ≤ size / 2).
        *slot = core::ptr::read_unaligned(ptr.add(i));
    }
    n
}

static RT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Systemsprache von der eigenen Startpartition (\ultar\sprache: ar/de/en) – noch vor ExitBootServices,
/// damit schon das erste Bild in der gewählten Sprache erscheint.
pub unsafe fn sprache_lesen(image: Handle, st: *mut SystemTable) -> Option<u8> {
    let li = protokoll(st, image, &LOADED_IMAGE_GUID);
    if li.is_null() {
        return None;
    }
    let eigenes = *((li as *const u8).add(24) as *const Handle);
    let fs = protokoll(st, eigenes, &SIMPLE_FS_GUID) as *mut SimpleFs;
    if fs.is_null() {
        return None;
    }
    let mut root: *mut FileProto = core::ptr::null_mut();
    if ((*fs).open_volume)(fs, &mut root) != 0 || root.is_null() {
        return None;
    }
    let mut b = [0u16; 64];
    let mut f: *mut FileProto = core::ptr::null_mut();
    let mut erg = None;
    if ((*root).open)(root, &mut f, u16z("\\ultar\\sprache", &mut b), 1, 0) == 0 && !f.is_null() {
        let mut buf = [0u8; 16];
        let mut n = buf.len();
        if ((*f).read)(f, &mut n, buf.as_mut_ptr() as *mut c_void) == 0 {
            if let Ok(s) = core::str::from_utf8(&buf[..n.min(buf.len())]) {
                erg = crate::sprache::aus_code(s);
            }
        }
        ((*f).close)(f);
    }
    ((*root).close)(root);
    erg
}

pub fn set_runtime(rt: *mut RuntimeServices) {
    RT.store(rt as usize, core::sync::atomic::Ordering::Relaxed);
}

pub fn runtime() -> *mut RuntimeServices {
    RT.load(core::sync::atomic::Ordering::Relaxed) as *mut RuntimeServices
}

/// Neustart (0) oder Ausschalten (2) über die Laufzeitdienste der Firmware
pub unsafe fn reset(rt: *mut RuntimeServices, kind: u32) -> ! {
    if !rt.is_null() {
        let f: extern "efiapi" fn(u32, Status, usize, *const c_void) -> ! = core::mem::transmute((*rt).f[RT_RESET_SYSTEM]);
        f(kind, 0, 0, core::ptr::null());
    }
    loop {
        core::arch::asm!("hlt");
    }
}

pub fn _unused() -> usize {
    BS_ALLOCATE_PAGES
}
