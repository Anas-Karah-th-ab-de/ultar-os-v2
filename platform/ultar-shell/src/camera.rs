//! Kamera-Aufnahme über V4L2 (Exynos FIMC-IS). Unterstützt Single-Plane und MPLANE.
//!
//! Ablauf: open -> QUERYCAP -> S_FMT (NV21/YUYV) -> REQBUFS(MMAP) -> QUERYBUF+mmap ->
//! QBUF -> STREAMON -> (DQBUF/QBUF Schleife) -> ein Frame -> nach RGB (0x00RRGGBB) wandeln.
//! `Camera::open("/dev/videoN")`, dann `capture_frame()` liefert (Breite, Höhe, Vec<u32>).
//!
//! Hinweis: Auf dem A30 muss die richtige Capture-Node gefunden werden (viele /dev/video*).
//! Das Werkzeug tools/camctl.c listet sie (QUERYCAP). Manche Exynos-Pipelines brauchen zuerst
//! media-ctl, um Sensor->CSIS->MCSC zu verschalten; die MCSC-Capture-Node reicht dann hier.

#![allow(dead_code)]

use nix::libc;
use std::os::fd::RawFd;

// ---- ioctl-Nummern (asm-generic, type 'V' = 0x56) ----
const fn ioc(dir: u64, nr: u64, size: u64) -> libc::c_ulong {
    ((dir << 30) | (size << 16) | (0x56u64 << 8) | nr) as libc::c_ulong
}
const VIDIOC_QUERYCAP: libc::c_ulong = ioc(2, 0, 104);
const VIDIOC_G_FMT: libc::c_ulong = ioc(3, 4, 208);
const VIDIOC_S_FMT: libc::c_ulong = ioc(3, 5, 208);
const VIDIOC_REQBUFS: libc::c_ulong = ioc(3, 8, 20);
const VIDIOC_QUERYBUF: libc::c_ulong = ioc(3, 9, 88);
const VIDIOC_QBUF: libc::c_ulong = ioc(3, 15, 88);
const VIDIOC_DQBUF: libc::c_ulong = ioc(3, 17, 88);
const VIDIOC_STREAMON: libc::c_ulong = ioc(1, 18, 4);
const VIDIOC_STREAMOFF: libc::c_ulong = ioc(1, 19, 4);

const CAP_VIDEO_CAPTURE: u32 = 0x0000_0001;
const CAP_VIDEO_CAPTURE_MPLANE: u32 = 0x0000_1000;
const BUF_TYPE_CAPTURE: u32 = 1;
const BUF_TYPE_CAPTURE_MPLANE: u32 = 9;
const MEMORY_MMAP: u32 = 1;
const FIELD_ANY: u32 = 0;

const fn fourcc(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
}
const PIX_YUYV: u32 = fourcc(b'Y', b'U', b'Y', b'V');
const PIX_NV21: u32 = fourcc(b'N', b'V', b'2', b'1');
const PIX_NV12: u32 = fourcc(b'N', b'V', b'1', b'2');
const PIX_RGB24: u32 = fourcc(b'R', b'G', b'B', b'3');

// ---- Strukturen ----
#[repr(C)]
struct Capability {
    driver: [u8; 16],
    card: [u8; 32],
    bus_info: [u8; 32],
    version: u32,
    capabilities: u32,
    device_caps: u32,
    reserved: [u32; 3],
}

// v4l2_format als type + 204-Byte-Union (Gesamtgröße 208). Felder per Offset.
#[repr(C)]
struct Format {
    type_: u32,
    data: [u8; 204],
}

#[repr(C)]
struct RequestBuffers {
    count: u32,
    type_: u32,
    memory: u32,
    capabilities: u32,
    flags: u8,
    reserved: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Plane {
    bytesused: u32,
    length: u32,
    m: u64, // union: mem_offset(u32) / userptr / fd
    data_offset: u32,
    reserved: [u32; 11],
}

#[repr(C)]
struct Buffer {
    index: u32,
    type_: u32,
    bytesused: u32,
    flags: u32,
    field: u32,
    _pad: u32,
    timestamp: [i64; 2], // sec, usec
    timecode: [u8; 16],
    sequence: u32,
    memory: u32,
    m: u64, // union: offset(u32) / planes(ptr) / userptr / fd
    length: u32,
    reserved2: u32,
    request_fd: i32,
    reserved: u32,
}

// Größen zur Bauzeit absichern
const _A: () = assert!(std::mem::size_of::<Capability>() == 104);
const _B: () = assert!(std::mem::size_of::<Format>() == 208);
const _C: () = assert!(std::mem::size_of::<RequestBuffers>() == 20);
const _D: () = assert!(std::mem::size_of::<Plane>() == 64);
const _E: () = assert!(std::mem::size_of::<Buffer>() == 88);

fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_ne_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}
fn put_u32(d: &mut [u8], o: usize, v: u32) {
    d[o..o + 4].copy_from_slice(&v.to_ne_bytes());
}

unsafe fn ioctl(fd: RawFd, req: libc::c_ulong, arg: *mut libc::c_void) -> i32 {
    loop {
        let r = libc::ioctl(fd, req, arg);
        if r < 0 && *libc::__errno_location() == libc::EINTR {
            continue;
        }
        return r;
    }
}

struct Buf {
    ptr: *mut libc::c_void,
    len: usize,
}

pub struct Camera {
    fd: RawFd,
    mplane: bool,
    btype: u32,
    pub width: u32,
    pub height: u32,
    pix: u32,
    stride: u32,
    bufs: Vec<Buf>,
    streaming: bool,
}

impl Camera {
    /// Kamera öffnen und Streaming vorbereiten (bevorzugtes Format NV21/MPLANE bzw. YUYV).
    pub fn open(path: &str) -> Result<Camera, String> {
        let cpath = std::ffi::CString::new(path).map_err(|_| "pfad".to_string())?;
        let fd = unsafe { libc::open(cpath.as_ptr(), libc::O_RDWR | libc::O_NONBLOCK) };
        if fd < 0 {
            return Err(format!("{path}: {}", std::io::Error::last_os_error()));
        }
        let mut cam = Camera {
            fd,
            mplane: false,
            btype: BUF_TYPE_CAPTURE,
            width: 0,
            height: 0,
            pix: 0,
            stride: 0,
            bufs: Vec::new(),
            streaming: false,
        };
        if let Err(e) = cam.init() {
            cam.close();
            return Err(e);
        }
        Ok(cam)
    }

    fn init(&mut self) -> Result<(), String> {
        // QUERYCAP
        let mut cap: Capability = unsafe { std::mem::zeroed() };
        if unsafe { ioctl(self.fd, VIDIOC_QUERYCAP, &mut cap as *mut _ as *mut _) } < 0 {
            return Err(format!("QUERYCAP: {}", std::io::Error::last_os_error()));
        }
        let caps = if cap.device_caps != 0 { cap.device_caps } else { cap.capabilities };
        self.mplane = (caps & CAP_VIDEO_CAPTURE_MPLANE) != 0 && (caps & CAP_VIDEO_CAPTURE) == 0;
        self.btype = if self.mplane { BUF_TYPE_CAPTURE_MPLANE } else { BUF_TYPE_CAPTURE };

        // aktuelles Format lesen
        let mut fmt: Format = unsafe { std::mem::zeroed() };
        fmt.type_ = self.btype;
        unsafe { ioctl(self.fd, VIDIOC_G_FMT, &mut fmt as *mut _ as *mut _) };

        // bevorzugtes Format setzen
        if self.mplane {
            // pix_mp: width@0 height@4 pixelformat@8 field@12 colorspace@16
            if u32_at(&fmt.data, 0) == 0 {
                put_u32(&mut fmt.data, 0, 640);
                put_u32(&mut fmt.data, 4, 480);
            }
            put_u32(&mut fmt.data, 8, PIX_NV21);
            put_u32(&mut fmt.data, 12, FIELD_ANY);
        } else {
            // pix: width@0 height@4 pixelformat@8 field@12 bytesperline@16
            if u32_at(&fmt.data, 0) == 0 {
                put_u32(&mut fmt.data, 0, 640);
                put_u32(&mut fmt.data, 4, 480);
            }
            put_u32(&mut fmt.data, 8, PIX_YUYV);
            put_u32(&mut fmt.data, 12, FIELD_ANY);
        }
        unsafe { ioctl(self.fd, VIDIOC_S_FMT, &mut fmt as *mut _ as *mut _) };

        // zurückgelesene Werte übernehmen
        self.width = u32_at(&fmt.data, 0);
        self.height = u32_at(&fmt.data, 4);
        self.pix = u32_at(&fmt.data, 8);
        if self.mplane {
            // plane_fmt[0].bytesperline @ (16 colorspace) -> plane_fmt beginnt @20: sizeimage@20, bytesperline@24
            self.stride = u32_at(&fmt.data, 24);
        } else {
            self.stride = u32_at(&fmt.data, 16); // bytesperline
        }
        if self.width == 0 || self.height == 0 {
            return Err("Format ohne Groesse".into());
        }
        if self.stride == 0 {
            self.stride = if self.pix == PIX_YUYV { self.width * 2 } else { self.width };
        }

        // Puffer anfordern
        let mut req: RequestBuffers = unsafe { std::mem::zeroed() };
        req.count = 4;
        req.type_ = self.btype;
        req.memory = MEMORY_MMAP;
        if unsafe { ioctl(self.fd, VIDIOC_REQBUFS, &mut req as *mut _ as *mut _) } < 0 {
            return Err(format!("REQBUFS: {}", std::io::Error::last_os_error()));
        }
        if req.count < 1 {
            return Err("keine Puffer".into());
        }

        for i in 0..req.count {
            let mut b: Buffer = unsafe { std::mem::zeroed() };
            let mut planes: [Plane; 8] = unsafe { std::mem::zeroed() };
            b.type_ = self.btype;
            b.memory = MEMORY_MMAP;
            b.index = i;
            if self.mplane {
                b.m = planes.as_mut_ptr() as u64;
                b.length = 8;
            }
            if unsafe { ioctl(self.fd, VIDIOC_QUERYBUF, &mut b as *mut _ as *mut _) } < 0 {
                return Err(format!("QUERYBUF: {}", std::io::Error::last_os_error()));
            }
            let (len, off) = if self.mplane {
                (planes[0].length as usize, (planes[0].m & 0xffff_ffff) as i64)
            } else {
                (b.length as usize, (b.m & 0xffff_ffff) as i64)
            };
            let ptr = unsafe {
                libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, self.fd, off)
            };
            if ptr == libc::MAP_FAILED {
                return Err(format!("mmap: {}", std::io::Error::last_os_error()));
            }
            self.bufs.push(Buf { ptr, len });

            // wieder einreihen
            let mut qb: Buffer = unsafe { std::mem::zeroed() };
            let mut qp: [Plane; 8] = unsafe { std::mem::zeroed() };
            qb.type_ = self.btype;
            qb.memory = MEMORY_MMAP;
            qb.index = i;
            if self.mplane {
                qb.m = qp.as_mut_ptr() as u64;
                qb.length = 8;
            }
            if unsafe { ioctl(self.fd, VIDIOC_QBUF, &mut qb as *mut _ as *mut _) } < 0 {
                return Err(format!("QBUF: {}", std::io::Error::last_os_error()));
            }
        }

        if unsafe { ioctl(self.fd, VIDIOC_STREAMON, &self.btype as *const _ as *mut _) } < 0 {
            return Err(format!("STREAMON: {}", std::io::Error::last_os_error()));
        }
        self.streaming = true;
        Ok(())
    }

    /// Einen Frame holen und als RGB (0x00RRGGBB) liefern. Verwirft einige Frames (Belichtung).
    pub fn capture_frame(&mut self) -> Result<Vec<u32>, String> {
        let mut out: Option<Vec<u8>> = None;
        for tries in 0..30 {
            let mut pfd = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
            let pr = unsafe { libc::poll(&mut pfd, 1, 2000) };
            if pr <= 0 {
                if tries > 3 {
                    break;
                }
                continue;
            }
            let mut b: Buffer = unsafe { std::mem::zeroed() };
            let mut planes: [Plane; 8] = unsafe { std::mem::zeroed() };
            b.type_ = self.btype;
            b.memory = MEMORY_MMAP;
            if self.mplane {
                b.m = planes.as_mut_ptr() as u64;
                b.length = 8;
            }
            if unsafe { ioctl(self.fd, VIDIOC_DQBUF, &mut b as *mut _ as *mut _) } < 0 {
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() == Some(libc::EAGAIN) {
                    continue;
                }
                return Err(format!("DQBUF: {e}"));
            }
            if tries >= 5 {
                let buf = &self.bufs[b.index as usize];
                let slice = unsafe { std::slice::from_raw_parts(buf.ptr as *const u8, buf.len) };
                out = Some(slice.to_vec());
            }
            // zurückgeben
            unsafe { ioctl(self.fd, VIDIOC_QBUF, &mut b as *mut _ as *mut _) };
            if out.is_some() {
                break;
            }
        }
        let frame = out.ok_or_else(|| "kein Frame".to_string())?;
        Ok(self.to_rgb(&frame))
    }

    fn to_rgb(&self, src: &[u8]) -> Vec<u32> {
        let (w, h, stride) = (self.width as usize, self.height as usize, self.stride as usize);
        let mut px = vec![0u32; w * h];
        match self.pix {
            PIX_YUYV => {
                for y in 0..h {
                    let line = &src[y * stride..];
                    for x in (0..w).step_by(2) {
                        let i = x * 2;
                        if i + 3 >= line.len() {
                            break;
                        }
                        let (y0, u, y1, v) = (line[i] as i32, line[i + 1] as i32, line[i + 2] as i32, line[i + 3] as i32);
                        px[y * w + x] = yuv(y0, u, v);
                        if x + 1 < w {
                            px[y * w + x + 1] = yuv(y1, u, v);
                        }
                    }
                }
            }
            PIX_NV21 | PIX_NV12 => {
                let nv21 = self.pix == PIX_NV21;
                let csrc = stride * h;
                for y in 0..h {
                    let yl = y * stride;
                    let cl = csrc + (y / 2) * stride;
                    for x in 0..w {
                        let yy = src[yl + x] as i32;
                        let ci = cl + (x / 2) * 2;
                        if ci + 1 >= src.len() {
                            break;
                        }
                        let (u, v) = if nv21 { (src[ci + 1] as i32, src[ci] as i32) } else { (src[ci] as i32, src[ci + 1] as i32) };
                        px[y * w + x] = yuv(yy, u, v);
                    }
                }
            }
            PIX_RGB24 => {
                for y in 0..h {
                    for x in 0..w {
                        let i = y * stride + x * 3;
                        if i + 2 < src.len() {
                            px[y * w + x] = ((src[i] as u32) << 16) | ((src[i + 1] as u32) << 8) | src[i + 2] as u32;
                        }
                    }
                }
            }
            _ => {}
        }
        px
    }

    pub fn close(&mut self) {
        if self.streaming {
            unsafe { ioctl(self.fd, VIDIOC_STREAMOFF, &self.btype as *const _ as *mut _) };
            self.streaming = false;
        }
        for b in self.bufs.drain(..) {
            unsafe { libc::munmap(b.ptr, b.len) };
        }
        if self.fd >= 0 {
            unsafe { libc::close(self.fd) };
            self.fd = -1;
        }
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        self.close();
    }
}

#[inline]
fn clamp8(v: i32) -> u32 {
    v.clamp(0, 255) as u32
}
/// BT.601 YUV -> 0x00RRGGBB
#[inline]
fn yuv(y: i32, u: i32, v: i32) -> u32 {
    let c = y - 16;
    let d = u - 128;
    let e = v - 128;
    let r = clamp8((298 * c + 409 * e + 128) >> 8);
    let g = clamp8((298 * c - 100 * d - 208 * e + 128) >> 8);
    let b = clamp8((298 * c + 516 * d + 128) >> 8);
    (r << 16) | (g << 8) | b
}

/// Erste Kamera-Capture-Node finden (einfacher Fall). Liefert z. B. "/dev/video0".
pub fn find_capture_node() -> Option<String> {
    for i in 0..260 {
        let path = format!("/dev/video{i}");
        let cpath = std::ffi::CString::new(path.clone()).ok()?;
        let fd = unsafe { libc::open(cpath.as_ptr(), libc::O_RDWR | libc::O_NONBLOCK) };
        if fd < 0 {
            continue;
        }
        let mut cap: Capability = unsafe { std::mem::zeroed() };
        let ok = unsafe { ioctl(fd, VIDIOC_QUERYCAP, &mut cap as *mut _ as *mut _) } == 0;
        unsafe { libc::close(fd) };
        if ok {
            let caps = if cap.device_caps != 0 { cap.device_caps } else { cap.capabilities };
            if caps & (CAP_VIDEO_CAPTURE | CAP_VIDEO_CAPTURE_MPLANE) != 0 {
                return Some(path);
            }
        }
    }
    None
}

// ===========================================================================
// Hintergrund-Kamera (verhindert UI-Freeze): Öffnen + Capture laufen in einem
// eigenen Thread. Auf Exynos/fimc-is kann open()/S_FMT/STREAMON im Kernel-
// request_firmware bis zu ~60 s blockieren, wenn ISP-Firmware fehlt. Im Thread
// friert dadurch nur der Worker ein, nie die Oberfläche. Der Worker wird beim
// Stop NICHT gejoint (evtl. im Kernel hängend), sondern mit einem Stopp-Flag
// abgelöst; der fd wird geschlossen, sobald der Kernelaufruf zurückkehrt.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq)]
pub enum CamStatus {
    Opening,
    Live,
    Failed,
}

#[derive(Default)]
struct Shared {
    frame: Mutex<Option<(u32, u32, Vec<u32>)>>,
    status: Mutex<u8>, // 0 Opening, 1 Live, 2 Failed
    err: Mutex<Option<String>>,
}

pub struct CameraWorker {
    sh: Arc<Shared>,
    stop: Arc<AtomicBool>,
    photo: Arc<AtomicBool>,
    hires: bool,
    pub facing: u8,
}

/// Samsung (Exynos fimc-is, z. B. Galaxy A30): Die Kamera läuft nur über Samsungs Bildkette.
/// Das übernimmt der Helfer `ultar-cam` (tools/a30-cam) und liefert fertige RGB-Bilder als Strom.
const SAMSUNG_HELPER: &str = "/usr/bin/ultar-cam";
pub fn samsung_available() -> bool {
    std::path::Path::new("/sys/class/camera/rear/fw_update").exists() && std::path::Path::new(SAMSUNG_HELPER).exists()
}

impl CameraWorker {
    /// Startet die Kamera im Hintergrund. `facing`: 0 Rück, 1 Front (nur Hinweis,
    /// Knotenwahl bestimmt der gespeicherte camera.node / die Autosuche).
    pub fn start(facing: u8, node_hint: Option<String>, data_dir: String) -> CameraWorker {
        let sh = Arc::new(Shared { status: Mutex::new(0), ..Default::default() });
        let stop = Arc::new(AtomicBool::new(false));
        let photo = Arc::new(AtomicBool::new(false));
        let sh2 = sh.clone();
        let stop2 = stop.clone();
        let photo2 = photo.clone();
        let hires = samsung_available();
        std::thread::Builder::new()
            .name("ultar-cam".into())
            .spawn(move || {
                if hires {
                    samsung_main(sh2, stop2, photo2, facing)
                } else {
                    worker_main(sh2, stop2, node_hint, data_dir)
                }
            })
            .ok();
        CameraWorker { sh, stop, photo, hires, facing }
    }

    /// Kann der Worker auf Anforderung ein Bild in voller Fotoauflösung liefern?
    pub fn hires(&self) -> bool {
        self.hires
    }

    /// Nächstes Bild in Fotoauflösung anfordern (kommt wie gewohnt über `take_frame`, nur größer).
    pub fn request_photo(&self) {
        self.photo.store(true, Ordering::SeqCst);
    }

    /// Neuestes Bild herausnehmen (konsumierend – danach wartet der Worker auf das nächste).
    pub fn take_frame(&self) -> Option<(u32, u32, Vec<u32>)> {
        self.sh.frame.lock().ok().and_then(|mut f| f.take())
    }

    pub fn status(&self) -> CamStatus {
        match self.sh.status.lock().map(|s| *s).unwrap_or(2) {
            0 => CamStatus::Opening,
            1 => CamStatus::Live,
            _ => CamStatus::Failed,
        }
    }

    pub fn error(&self) -> Option<String> {
        self.sh.err.lock().ok().and_then(|e| e.clone())
    }

    /// Signalisiert dem Worker das Ende. Kein join() – falls er noch im Kernel
    /// hängt, soll die Oberfläche nicht mitwarten.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for CameraWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn worker_main(sh: Arc<Shared>, stop: Arc<AtomicBool>, node_hint: Option<String>, data_dir: String) {
    // Knoten bestimmen (kann blockieren – aber nur hier im Thread).
    let hinted = node_hint.as_ref().is_some_and(|s| !s.is_empty());
    let node = node_hint
        .filter(|s| !s.is_empty())
        .or_else(find_capture_node);
    let Some(path) = node else {
        *sh.status.lock().unwrap() = 2;
        *sh.err.lock().unwrap() = Some("Keine Kamera gefunden".into());
        return;
    };
    if stop.load(Ordering::SeqCst) {
        return;
    }
    // Öffnen (kann im Kernel blockieren). Ergebnis erst danach bewerten.
    let mut cam = match Camera::open(&path) {
        Ok(c) => c,
        Err(e) => {
            *sh.status.lock().unwrap() = 2;
            *sh.err.lock().unwrap() = Some(e);
            return;
        }
    };
    // Funktionierenden Knoten merken, damit der nächste Start sofort trifft.
    // (Nur bei selbst gefundenem Knoten – ein vorgegebener, z. B. Frontkamera, überschreibt ihn nicht.)
    if !hinted {
        let _ = std::fs::write(format!("{data_dir}/camera.node"), &path);
    }

    let mut live = false;
    while !stop.load(Ordering::SeqCst) {
        match cam.capture_frame() {
            Ok(px) => {
                let (w, h) = (cam.width, cam.height);
                if w > 0 && h > 0 && px.len() as u32 >= w * h {
                    if !live {
                        *sh.status.lock().unwrap() = 1;
                        live = true;
                    }
                    if let Ok(mut f) = sh.frame.lock() {
                        *f = Some((w, h, px));
                    }
                }
            }
            Err(e) => {
                // Vereinzelte Timeouts tolerieren, dauerhaften Fehler melden.
                if !live {
                    *sh.status.lock().unwrap() = 2;
                    *sh.err.lock().unwrap() = Some(e);
                    break;
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    cam.close();
}

/// Bildstrom des Helfers lesen: Satz = "UCF1", Breite, Höhe, Nummer (je u32 LE), dann Breite*Höhe mal 0x00RRGGBB.
fn samsung_main(sh: Arc<Shared>, stop: Arc<AtomicBool>, photo: Arc<AtomicBool>, facing: u8) {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(SAMSUNG_HELPER);
    cmd.arg("stream");
    if facing == 1 {
        cmd.arg("front");
    }
    let child = cmd.env("UCAM_STEP", "3").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            *sh.status.lock().unwrap() = 2;
            *sh.err.lock().unwrap() = Some(format!("ultar-cam: {e}"));
            return;
        }
    };
    let (Some(mut cin), Some(mut cout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        return;
    };
    let mut live = false;
    let mut bytes: Vec<u8> = Vec::new();
    while !stop.load(Ordering::SeqCst) {
        if photo.swap(false, Ordering::SeqCst) {
            let _ = cin.write_all(b"p");
            let _ = cin.flush();
        }
        let mut hd = [0u8; 16];
        if cout.read_exact(&mut hd).is_err() {
            break;
        }
        let (w, h) = (u32_at(&hd, 4), u32_at(&hd, 8));
        if &hd[0..4] != b"UCF1" || w == 0 || h == 0 || w > 4700 || h > 3600 {
            break;
        }
        let n = (w * h) as usize;
        bytes.resize(n * 4, 0);
        if cout.read_exact(&mut bytes).is_err() {
            break;
        }
        let px: Vec<u32> = bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        if !live {
            *sh.status.lock().unwrap() = 1;
            live = true;
        }
        if let Ok(mut f) = sh.frame.lock() {
            // Ein noch nicht abgeholtes Foto nicht durch ein Vorschaubild ersetzen
            let keep = matches!(&*f, Some((ow, _, _)) if *ow > w);
            if !keep {
                *f = Some((w, h, px));
            }
        }
    }
    // Sauber beenden: der Helfer schaltet Sensor und Bildkette ab, sobald er 'q' oder das Ende der Eingabe sieht.
    let _ = cin.write_all(b"q");
    drop(cin);
    drop(cout);
    for _ in 0..40 {
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    if !live {
        *sh.status.lock().unwrap() = 2;
        *sh.err.lock().unwrap() = Some(if facing == 1 { "Frontkamera liefert kein Bild".into() } else { "Kamera liefert kein Bild".into() });
    }
}
