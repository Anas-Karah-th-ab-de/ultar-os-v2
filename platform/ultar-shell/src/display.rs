//! Bildschirmausgabe: DRM/KMS (Dumb-Buffer), Rückfall auf fbdev.

use drm::buffer::Buffer as _;
use drm::control::{
    connector, dumbbuffer::DumbBuffer, framebuffer, ClipRect, Device as ControlDevice,
};
use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::FileExt;

struct Card(File);
impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}
impl drm::Device for Card {}
impl ControlDevice for Card {}

enum Backend {
    Drm {
        card: Card,
        db: DumbBuffer,
        fb: framebuffer::Handle,
    },
    /// fbdev; swap_rb: Bildspeicher erwartet Rot und Blau vertauscht (RGBX statt BGRX)
    Fb {
        f: File,
        line: usize,
        swap_rb: bool,
        var: [u32; 40],
    },
    /// DECON (Samsung Exynos): ION-Puffer + S3CFB_WIN_CONFIG – der einzige Weg, der auf diesen Panels Bild zeigt
    Decon(crate::decon::Decon),
    /// kein Anzeigegerät: Bild nur für das Teilen (Ultar-Verbund) – Geräte ohne Bildschirm
    Virtuell,
}

pub struct Display {
    be: Backend,
    pub w: u32,
    pub h: u32,
    pub name: String,
    scratch: Vec<u8>,
}

/// Physischer Punkt -> logischer Punkt bei Drehung (0/90/180/270).
pub fn phys_to_logical(rot: u16, px: i32, py: i32, pw: i32, ph: i32) -> (i32, i32) {
    match rot {
        90 => (py, pw - 1 - px),
        180 => (pw - 1 - px, ph - 1 - py),
        270 => (ph - 1 - py, px),
        _ => (px, py),
    }
}

impl Display {
    /// virtueller Bildschirm (ohne Anzeigegerät), sichtbar über das Teilen im Ultar-Verbund
    pub fn virtuell(w: u32, h: u32) -> Self {
        Display {
            be: Backend::Virtuell,
            w,
            h,
            name: format!("virtuell {w}x{h}"),
            scratch: Vec::new(),
        }
    }

    pub fn ist_virtuell(&self) -> bool {
        matches!(self.be, Backend::Virtuell)
    }

    /// Merkdatei: welcher Bildschirm-Fall auf diesem Gerät funktioniert hat (z. B. "drm:/dev/dri/card0").
    const CASE_FILE: &'static str = "/data/ultar/display.case";

    fn open_case(case: &str) -> Result<Self, String> {
        match case.split_once(':') {
            Some(("drm", p)) => Self::open_drm(p),
            Some(("decon", p)) => Self::open_decon(p),
            Some(("fbdev", p)) => Self::open_fb(p),
            _ => Err(format!("unbekannter Fall {case}")),
        }
    }

    /// Bildschirm öffnen. Erster Start: alle Fälle der Reihe nach (DRM-Karten, dann Framebuffer-Geräte); der erste,
    /// der funktioniert, wird gemerkt. Jeder weitere Start: nur der gemerkte Fall – erst wenn er versagt, wird neu gesucht.
    pub fn open() -> Result<Self, String> {
        let mut errs = Vec::new();
        let remembered = std::fs::read_to_string(Self::CASE_FILE)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(case) = &remembered {
            match Self::open_case(case) {
                Ok(d) => return Ok(d),
                Err(e) => errs.push(format!("gemerkt {case}: {e}")),
            }
        }
        let mut cases: Vec<String> = (0..4).map(|i| format!("drm:/dev/dri/card{i}")).collect();
        // DECON vor fbdev versuchen: auf Samsung-Exynos ist das der einzige Weg, der Bild zeigt.
        // Nur wenn /dev/ion existiert (sonst ist es kein Exynos-DECON-Gerät).
        if std::path::Path::new("/dev/ion").exists() {
            cases.extend(["decon:/dev/fb0", "decon:/dev/graphics/fb0"].map(String::from));
        }
        cases.extend(
            [
                "fbdev:/dev/fb0",
                "fbdev:/dev/graphics/fb0",
                "fbdev:/dev/fb1",
            ]
            .map(String::from),
        );
        for case in cases {
            let path = case.split_once(':').map(|x| x.1).unwrap_or("");
            if remembered.as_deref() == Some(case.as_str()) || !std::path::Path::new(path).exists()
            {
                continue;
            }
            match Self::open_case(&case) {
                Ok(d) => {
                    // Merken (nur wenn es den Datenordner gibt; Fehler sind unkritisch – dann wird nächstes Mal neu gesucht)
                    if std::path::Path::new("/data/ultar").is_dir() {
                        let _ = std::fs::write(Self::CASE_FILE, &case);
                    }
                    return Ok(d);
                }
                Err(e) => errs.push(format!("{path}: {e}")),
            }
        }
        if errs.is_empty() {
            errs.push("kein Bildschirmgerät gefunden (/dev/dri/card*, /dev/fb*)".into());
        }
        Err(errs.join("; "))
    }

    fn open_drm(path: &str) -> Result<Self, String> {
        let card = Card(
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|e| e.to_string())?,
        );
        let res = card
            .resource_handles()
            .map_err(|e| format!("resources: {e}"))?;
        let mut found = None;
        for &c in res.connectors() {
            let info = card
                .get_connector(c, true)
                .map_err(|e| format!("connector: {e}"))?;
            if info.state() == connector::State::Connected && !info.modes().is_empty() {
                found = Some(info);
                break;
            }
        }
        let conn = found.ok_or("kein angeschlossener Bildschirm")?;
        // bevorzugten Modus wählen, sonst den ersten
        let mode = *conn
            .modes()
            .iter()
            .find(|m| {
                m.mode_type()
                    .contains(drm::control::ModeTypeFlags::PREFERRED)
            })
            .unwrap_or(&conn.modes()[0]);
        let crtc = conn
            .current_encoder()
            .and_then(|e| card.get_encoder(e).ok())
            .and_then(|e| e.crtc())
            .or_else(|| {
                conn.encoders()
                    .iter()
                    .filter_map(|&e| card.get_encoder(e).ok())
                    .find_map(|e| res.filter_crtcs(e.possible_crtcs()).first().copied())
            })
            .or_else(|| res.crtcs().first().copied())
            .ok_or("kein CRTC")?;
        let (w, h) = (mode.size().0 as u32, mode.size().1 as u32);
        let db = card
            .create_dumb_buffer((w, h), drm::buffer::DrmFourcc::Xrgb8888, 32)
            .map_err(|e| format!("dumb buffer: {e}"))?;
        let fb = card
            .add_framebuffer(&db, 24, 32)
            .map_err(|e| format!("framebuffer: {e}"))?;
        card.set_crtc(crtc, Some(fb), (0, 0), &[conn.handle()], Some(mode))
            .map_err(|e| format!("set_crtc: {e}"))?;
        Ok(Self {
            be: Backend::Drm { card, db, fb },
            w,
            h,
            name: format!("DRM {path}"),
            scratch: Vec::new(),
        })
    }

    fn open_fb(path: &str) -> Result<Self, String> {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        let mut var = [0u32; 40];
        let mut fix = [0u8; 80];
        extern "C" {
            fn ioctl(fd: i32, req: u64, ...) -> i32;
        }
        use std::os::fd::AsRawFd;
        unsafe {
            if ioctl(f.as_raw_fd(), 0x4600, var.as_mut_ptr()) != 0
                || ioctl(f.as_raw_fd(), 0x4602, fix.as_mut_ptr()) != 0
            {
                return Err("fbdev-ioctl fehlgeschlagen".into());
            }
        }
        if var[6] != 32 {
            return Err(format!("{} Bit Farbtiefe nicht unterstützt", var[6]));
        }
        let line = u32::from_ne_bytes([fix[48], fix[49], fix[50], fix[51]]) as usize;
        // var[8] = Bit-Position von Rot: 16 = BGRX (wie unsere Zeichenfläche), 0 = RGBX
        let swap_rb = var[8] == 0 && var[14] == 16;
        // Bildschirm einschalten. Exynos/DECON startet nach dem Bootloader im Zustand INIT und
        // überspringt beim bloßen UNBLANK die Panel-Einschaltsequenz (Bildschirm bliebe schwarz).
        // Darum erst POWERDOWN (Zustand->OFF), dann UNBLANK: das erzwingt die volle DSIM/Panel-Initialisierung.
        unsafe {
            ioctl(
                f.as_raw_fd(),
                0x4611, /* FBIOBLANK */
                4,      /* POWERDOWN */
            );
            std::thread::sleep(std::time::Duration::from_millis(60));
            ioctl(
                f.as_raw_fd(),
                0x4611, /* FBIOBLANK */
                0,      /* UNBLANK */
            );
        }
        Ok(Self {
            be: Backend::Fb {
                f,
                line,
                swap_rb,
                var,
            },
            w: var[0],
            h: var[1],
            name: format!("fbdev {path}"),
            scratch: Vec::new(),
        })
    }

    fn open_decon(path: &str) -> Result<Self, String> {
        let d = crate::decon::Decon::open(path)?;
        let (w, h) = (d.w, d.h);
        Ok(Self {
            be: Backend::Decon(d),
            w,
            h,
            name: format!("DECON {path}"),
            scratch: Vec::new(),
        })
    }

    /// Logische Größe bei gegebener Drehung.
    pub fn logical(&self, rot: u16) -> (i32, i32) {
        if rot == 90 || rot == 270 {
            (self.h as i32, self.w as i32)
        } else {
            (self.w as i32, self.h as i32)
        }
    }

    /// Bild ausgeben. `bright` 0..1 dunkelt per Software ab (wenn keine Hintergrundbeleuchtung steuerbar ist).
    /// Bei DRM wird direkt in den Bildspeicher geschrieben, ohne Zwischenkopie.
    pub fn present(&mut self, src: &[u32], lw: i32, rot: u16, bright: f32) {
        // Bild teilen (Ultar-Verbund) – kostet nur etwas, wenn wirklich geteilt wird
        crate::fern::bild(src, lw);
        let (pw, ph) = (self.w as usize, self.h as usize);
        match &mut self.be {
            Backend::Virtuell => {}
            Backend::Drm { card, db, fb } => {
                let pitch = db.pitch() as usize;
                if let Ok(mut map) = card.map_dumb_buffer(db) {
                    blit(&mut map, pitch, src, lw, pw, ph, rot, bright);
                }
                let _ = card.dirty_framebuffer(*fb, &[ClipRect::new(0, 0, pw as u16, ph as u16)]);
            }
            Backend::Decon(d) => {
                let line = d.line_bytes();
                let buf = d.buffer();
                let dst = unsafe {
                    std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, buf.len() * 4)
                };
                // Format XRGB_8888 entspricht der Zeichenfläche (0x00RRGGBB) – kein Rot/Blau-Tausch nötig.
                blit(dst, line, src, lw, pw, ph, rot, bright);
                d.flip();
            }
            Backend::Fb {
                f,
                line,
                swap_rb,
                var,
            } => {
                self.scratch.resize(*line * ph, 0);
                blit(&mut self.scratch, *line, src, lw, pw, ph, rot, bright);
                if *swap_rb {
                    for px in self.scratch.chunks_exact_mut(4) {
                        px.swap(0, 2);
                    }
                }
                let _ = f.write_all_at(&self.scratch, 0);
                // Treiber, die nur auf Anforderung neu anzeigen (z. B. Exynos DECON): Bild anstoßen
                use std::os::fd::AsRawFd;
                extern "C" {
                    fn ioctl(fd: i32, req: u64, ...) -> i32;
                }
                let mut v = *var;
                v[4] = 0; // xoffset
                v[5] = 0; // yoffset
                unsafe {
                    ioctl(
                        f.as_raw_fd(),
                        0x4606, /* FBIOPAN_DISPLAY */
                        v.as_mut_ptr(),
                    )
                };
            }
        }
    }
}

fn blit(
    dst: &mut [u8],
    pitch: usize,
    src: &[u32],
    lw: i32,
    pw: usize,
    ph: usize,
    rot: u16,
    bright: f32,
) {
    let m = (bright.clamp(0.0, 1.0) * 256.0) as u32;
    for y in 0..ph {
        if (y + 1) * pitch > dst.len() + pitch - pw * 4 {
            break;
        }
        let row = &mut dst[y * pitch..y * pitch + pw * 4];
        if rot == 0 && m >= 256 {
            // XRGB8888 Little-Endian entspricht dem Speicherformat der Zeichenfläche: Zeile am Stück kopieren
            let s = &src[y * pw..(y + 1) * pw];
            #[cfg(target_endian = "little")]
            row.copy_from_slice(unsafe {
                std::slice::from_raw_parts(s.as_ptr() as *const u8, pw * 4)
            });
            #[cfg(not(target_endian = "little"))]
            for (d, v) in row.chunks_exact_mut(4).zip(s) {
                d.copy_from_slice(&v.to_le_bytes());
            }
            continue;
        }
        for x in 0..pw {
            let (lx, ly) = phys_to_logical(rot, x as i32, y as i32, pw as i32, ph as i32);
            let mut v = src[(ly * lw + lx) as usize];
            if m < 256 {
                v = (((v >> 16) & 255) * m >> 8) << 16
                    | (((v >> 8) & 255) * m >> 8) << 8
                    | ((v & 255) * m >> 8);
            }
            row[x * 4..x * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
}
