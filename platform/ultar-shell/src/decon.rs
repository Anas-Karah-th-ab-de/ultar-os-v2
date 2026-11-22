//! DECON-Anzeigeweg für Samsung-Exynos-Geräte (z. B. Galaxy A30).
//!
//! Der klassische fbdev-Weg (`write` + `FBIOPAN_DISPLAY`) schiebt auf diesen Geräten kein Bild aufs
//! Panel – Android nutzt stattdessen DECONs eigenen Befehl `S3CFB_WIN_CONFIG` mit einem ION-Puffer
//! (dmabuf), den die Anzeige-Hardware per IOMMU liest. Genau das macht dieses Modul:
//!   1. ION-Puffer anlegen (`/dev/ion`, System-Heap), als dmabuf teilen und in den Speicher abbilden.
//!   2. In den Puffer zeichnen.
//!   3. Puffer per `S3CFB_WIN_CONFIG` als Vollbild-Fenster anzeigen (Standardfenster 3, Kanal IDMA_G0).
//!
//! Die Strukturen sind bytegenau wie im A30-Kernel (CONFIG_DPU_20=y, CONFIG_SUPPORT_MASK_LAYER=y).

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;

extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    fn munmap(addr: *mut u8, len: usize) -> i32;
    fn close(fd: i32) -> i32;
}

const fn ioc(dir: u32, typ: u32, nr: u32, size: u32) -> u64 {
    ((dir << 30) | (size << 16) | (typ << 8) | nr) as u64
}
const DIR_W: u32 = 1;
const DIR_RW: u32 = 3;

// ----- ION (Legacy-Schnittstelle, Kernel 4.4) -----
#[repr(C)]
#[derive(Default)]
struct IonAllocationData {
    len: u64,
    align: u64,
    heap_id_mask: u32,
    flags: u32,
    handle: i32,
}
#[repr(C)]
#[derive(Default)]
struct IonFdData {
    handle: i32,
    fd: i32,
}
fn ion_alloc(magic: u32, nr: u32) -> u64 {
    ioc(DIR_RW, magic, nr, std::mem::size_of::<IonAllocationData>() as u32)
}
fn ion_share(magic: u32, nr: u32) -> u64 {
    ioc(DIR_RW, magic, nr, std::mem::size_of::<IonFdData>() as u32)
}

// ----- DECON-Fensterkonfiguration -----
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
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
/// Inhalt der C-Union für ein Puffer-Fenster. Eigene Struktur, damit die 8-Byte-Ausrichtung (wegen der
/// 64-Bit-Adressen in `dpp_parm`) stimmt – so sitzt sie im Gesamtbild an derselben Stelle wie im Kernel.
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
    block_area: Rect,
    transparent_area: Rect,
    opaque_area: Rect,
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
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WinConfigExtra {
    remained_frames: i32,
    reserved: [u32; 7],
}
const MAX_WIN: usize = 4;
#[repr(C)]
struct WinConfigData {
    fence: i32,
    fd_odma: i32,
    config: [WinConfig; MAX_WIN + 1],
    extra: WinConfigExtra,
}

const WIN_STATE_DISABLED: i32 = 0;
const WIN_STATE_BUFFER: i32 = 2;
// Per Kamera-Test am A30 bestätigt: für die Zeichenfläche 0x00RRGGBB zeigt BGRX_8888 (7) Rot/Grün/Blau
// korrekt (die anderen Formate vertauschen Kanäle – XRGB zeigte Rot als Grün).
const FMT_BGRX_8888: i32 = 7;
const IDMA_G0: i32 = 0;
const DFT_WIN: usize = 3; // Standard-Fenster laut Treiber (decon->dt.dft_win = 3)

/// Ein geöffneter DECON-Ausgang mit eigenem ION-Bildpuffer.
pub struct Decon {
    fb: File,
    _ion: File,
    /// zwei Puffer: in einen wird gezeichnet, der andere wird gerade angezeigt
    bufs: [(i32, *mut u8); 2],
    back: usize,
    buf_fd: i32,
    map: *mut u8,
    size: usize,
    pub w: u32,
    pub h: u32,
    line: usize, // Bytes pro Zeile im Puffer
}

impl Decon {
    /// DECON öffnen: Framebuffer-Gerät (für den Befehl) + ION-Puffer in Bildschirmgröße.
    pub fn open(fb_path: &str) -> Result<Self, String> {
        let fb = OpenOptions::new().read(true).write(true).open(fb_path).map_err(|e| format!("{fb_path}: {e}"))?;
        // Größe aus dem fbdev-Var-Screeninfo lesen (FBIOGET_VSCREENINFO = 0x4600)
        let mut var = [0u32; 40];
        if unsafe { ioctl(fb.as_raw_fd(), 0x4600, var.as_mut_ptr()) } != 0 {
            return Err("FBIOGET_VSCREENINFO fehlgeschlagen".into());
        }
        let (w, h) = (var[0], var[1]);
        if w == 0 || h == 0 {
            return Err("Bildschirmgröße 0".into());
        }
        let line = (w * 4) as usize;
        let size = line * h as usize;

        let ion = OpenOptions::new().read(true).write(true).open("/dev/ion").map_err(|e| format!("/dev/ion: {e}"))?;
        let alloc = |ion: &File| -> Result<(i32, *mut u8), String> {
            // ION-Puffer anlegen (System-Heap, Maske 1)
            let mut a = IonAllocationData { len: size as u64, align: 0, heap_id_mask: 1, flags: 0, handle: 0 };
            if unsafe { ioctl(ion.as_raw_fd(), ion_alloc(b'I' as u32, 0), &mut a as *mut _) } < 0 {
                return Err("ION_IOC_ALLOC fehlgeschlagen".into());
            }
            // dmabuf-Dateizeiger holen
            let mut fdd = IonFdData { handle: a.handle, fd: -1 };
            if unsafe { ioctl(ion.as_raw_fd(), ion_share(b'I' as u32, 4), &mut fdd as *mut _) } < 0 || fdd.fd < 0 {
                return Err("ION_IOC_SHARE fehlgeschlagen".into());
            }
            // in den Speicher abbilden (MAP_SHARED=1, PROT_READ|WRITE=3)
            let map = unsafe { mmap(std::ptr::null_mut(), size, 3, 1, fdd.fd, 0) };
            if map.is_null() || map as isize == -1 {
                return Err("mmap des ION-Puffers fehlgeschlagen".into());
            }
            Ok((fdd.fd, map))
        };
        let b0 = alloc(&ion)?;
        // zweiter Puffer optional: ohne ihn bleibt es beim Einzelpuffer
        let b1 = alloc(&ion).unwrap_or(b0);
        Ok(Self { fb, _ion: ion, bufs: [b0, b1], back: 0, buf_fd: b0.0, map: b0.1, size, w, h, line })
    }

    /// Zugriff auf den Bildpuffer als 32-Bit-Pixel (0x00RRGGBB).
    pub fn buffer(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.map as *mut u32, self.size / 4) }
    }

    pub fn line_bytes(&self) -> usize {
        self.line
    }

    /// Den aktuellen Pufferinhalt anzeigen (ein Vollbild-Fenster über S3CFB_WIN_CONFIG) und danach in den
    /// anderen Puffer zeichnen (Doppelpuffer: kein Reißen, Zeichnen läuft nicht in das angezeigte Bild).
    pub fn flip(&mut self) {
        self.flip_inner();
        self.back ^= 1;
        self.buf_fd = self.bufs[self.back].0;
        self.map = self.bufs[self.back].1;
    }

    fn flip_inner(&mut self) {
        let mut d: WinConfigData = unsafe { std::mem::zeroed() };
        d.fence = -1;
        d.fd_odma = -1;
        for c in d.config.iter_mut() {
            c.state = WIN_STATE_DISABLED;
            c.buf.fd_idma = [-1, -1, -1];
            c.buf.fence_fd = -1;
            c.buf.rel_fence = -1;
        }
        let c = &mut d.config[DFT_WIN];
        c.state = WIN_STATE_BUFFER;
        c.buf.fd_idma = [self.buf_fd, -1, -1];
        c.buf.fence_fd = -1;
        c.buf.rel_fence = -1;
        c.buf.plane_alpha = 255;
        c.buf.blending = 0;
        c.buf.idma_type = IDMA_G0;
        c.buf.format = FMT_BGRX_8888;
        c.buf.src = Frame { x: 0, y: 0, w: self.w, h: self.h, f_w: self.w, f_h: self.h };
        c.dst = Frame { x: 0, y: 0, w: self.w, h: self.h, f_w: self.w, f_h: self.h };
        let sz = std::mem::size_of::<WinConfigData>() as u32;
        let req = ioc(DIR_W, b'F' as u32, 209, sz);
        let r = unsafe { ioctl(self.fb.as_raw_fd(), req, &mut d as *mut _) };
        // Der Kernel legt je Bild Sync-Dateien an und gibt ihre Nummern zurück: eine fürs ganze Bild (`fence`)
        // und eine Freigabe-Fence je aktivem Fenster (`rel_fence`). Wir warten auf keine davon – also alle
        // schließen, sonst läuft die Dateitabelle voll („decon_create_release_fences: failed to get unused fd").
        // Alle Felder standen vorher auf -1; was jetzt >= 3 ist, hat der Kernel eingetragen.
        let _ = r;
        let mut seen = [-1i32; MAX_WIN + 2];
        let mut n = 0;
        for fd in std::iter::once(d.fence).chain(d.config.iter().map(|c| c.buf.rel_fence)) {
            if fd >= 3 && fd != self.buf_fd && !seen[..n].contains(&fd) {
                seen[n] = fd;
                n += 1;
                unsafe { close(fd) };
            }
        }
    }
}

impl Drop for Decon {
    fn drop(&mut self) {
        unsafe {
            let same = self.bufs[0].0 == self.bufs[1].0;
            for (i, (fd, map)) in self.bufs.iter().enumerate() {
                if i == 1 && same {
                    break;
                }
                if !map.is_null() {
                    munmap(*map, self.size);
                }
                if *fd >= 0 {
                    close(*fd);
                }
            }
        }
    }
}

// Der Zeiger in `map` wird nur aus dem UI-Thread benutzt; Display ist nicht Send/Sync-kritisch hier.
unsafe impl Send for Decon {}

// Größen bytegenau wie im Kernel absichern (sonst stimmt der ioctl-Befehl nicht).
const _: () = assert!(std::mem::size_of::<WinConfig>() == 200);
const _: () = assert!(std::mem::size_of::<WinConfigData>() == 1040);
const _: () = assert!(std::mem::size_of::<IonAllocationData>() == 32);
