//! Eingabe über evdev: Touchscreen (Single-/Multitouch), Tablet/Maus, Tastatur.

use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use std::fs::File;
use std::io::Read;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ev {
    /// Position normalisiert 0..1 in physischen Bildschirmkoordinaten
    Down(f32, f32),
    Move(f32, f32),
    Up,
    Char(char),
    Backspace,
    Enter,
    Back,
    VolUp,
    VolDown,
    Power,
    /// Mausrad (Schritte, positiv = nach oben)
    Rad(i32),
}

static ARABISCH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Grundbelegung der Hardware-Tastatur: 0 = US (EN), 1 = Deutsch (QWERTZ); gemerkt in /data/ultar/tastatur
static DEUTSCH: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);

fn tastatur_datei() -> String {
    format!("{}/tastatur", crate::sys::data_dir())
}

pub fn deutsch() -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    let v = DEUTSCH.load(Relaxed);
    if v == 2 {
        // noch nicht geladen: gemerkte Wahl, sonst nach Systemsprache
        let w = std::fs::read_to_string(tastatur_datei()).ok();
        let de = match w.as_deref().map(str::trim) {
            Some("de") => true,
            Some("us") | Some("en") => false,
            _ => std::fs::read_to_string(format!("{}/settings.toml", crate::sys::data_dir())).map(|s| s.contains("lang = \"de\"")).unwrap_or(false),
        };
        DEUTSCH.store(de as u8, Relaxed);
        return de;
    }
    v == 1
}

/// Belegung reihum: EN → DE → Arabisch → EN
pub fn belegung_weiter() {
    use std::sync::atomic::Ordering::Relaxed;
    if arabisch() {
        ARABISCH.store(false, Relaxed);
        DEUTSCH.store(0, Relaxed);
    } else if deutsch() {
        ARABISCH.store(true, Relaxed);
    } else {
        DEUTSCH.store(1, Relaxed);
    }
    let _ = std::fs::write(tastatur_datei(), if deutsch() { "de" } else { "us" });
}

/// Kurzname der aktiven Belegung für die Leiste
pub fn belegung_name() -> &'static str {
    if arabisch() {
        "ع"
    } else if deutsch() {
        "DE"
    } else {
        "EN"
    }
}

/// Hardware-Tastatur mit arabischer Belegung (Umschalten: Alt+Umschalt oder Knopf in der Leiste)
pub fn arabisch() -> bool {
    ARABISCH.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn arabisch_umschalten() {
    ARABISCH.fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
}

/// US-Taste → arabisches Zeichen (Belegung „Arabisch 101“, wie der Ultar-Kern); zweites Zeichen für Lam-Alef
fn arabisch_zeichen(c: char) -> (char, Option<char>) {
    match c {
        'q' => ('ض', None), 'w' => ('ص', None), 'e' => ('ث', None), 'r' => ('ق', None), 't' => ('ف', None),
        'y' => ('غ', None), 'u' => ('ع', None), 'i' => ('ه', None), 'o' => ('خ', None), 'p' => ('ح', None),
        '[' => ('ج', None), ']' => ('د', None),
        'a' => ('ش', None), 's' => ('س', None), 'd' => ('ي', None), 'f' => ('ب', None), 'g' => ('ل', None),
        'h' => ('ا', None), 'j' => ('ت', None), 'k' => ('ن', None), 'l' => ('م', None), ';' => ('ك', None),
        '\'' => ('ط', None), '`' => ('ذ', None),
        'z' => ('ئ', None), 'x' => ('ء', None), 'c' => ('ؤ', None), 'v' => ('ر', None), 'b' => ('ل', Some('ا')),
        'n' => ('ى', None), 'm' => ('ة', None), ',' => ('و', None), '.' => ('ز', None), '/' => ('ظ', None),
        'T' => ('ل', Some('إ')), 'Y' => ('إ', None), 'G' => ('ل', Some('أ')), 'H' => ('أ', None), 'B' => ('ل', Some('آ')),
        'N' => ('آ', None), 'K' => ('،', None), 'P' => ('؛', None), '?' => ('؟', None), 'J' => ('ـ', None),
        _ => (c, None),
    }
}

struct Dev {
    f: File,
    path: String,
    ax: Option<(i32, i32)>,
    ay: Option<(i32, i32)>,
    x: i32,
    y: i32,
    down: bool,
    was_down: bool,
    moved: bool,
    shift: bool,
    alt: bool,
    altgr: bool,
}

pub struct Input {
    devs: Vec<Dev>,
    last_scan: Option<Instant>,
    /// Zeigerposition für relative Mäuse (normalisiert)
    pub cursor: (f32, f32),
    pub has_mouse: bool,
    pub has_touch: bool,
}

extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
}

fn absinfo(f: &File, code: u64) -> Option<(i32, i32)> {
    let mut a = [0i32; 6];
    let r = unsafe { ioctl(f.as_raw_fd(), 0x8018_4540 + code, a.as_mut_ptr()) };
    (r == 0 && a[2] > a[1]).then_some((a[1], a[2]))
}

impl Input {
    pub fn new() -> Self {
        Self { devs: Vec::new(), last_scan: None, cursor: (0.5, 0.5), has_mouse: false, has_touch: false }
    }

    fn scan(&mut self) {
        self.last_scan = Some(Instant::now());
        let Ok(rd) = std::fs::read_dir("/dev/input") else { return };
        for e in rd.flatten() {
            let path = e.path().to_string_lossy().into_owned();
            if !e.file_name().to_string_lossy().starts_with("event") || self.devs.iter().any(|d| d.path == path) {
                continue;
            }
            let Ok(f) = std::fs::OpenOptions::new().read(true).custom_flags(0o4000 /*O_NONBLOCK*/ | 0o2000000 /*O_CLOEXEC*/).open(&path) else { continue };
            // Multitouch-Achsen bevorzugen, sonst ABS_X/ABS_Y
            let ax = absinfo(&f, 0x35).or_else(|| absinfo(&f, 0));
            let ay = absinfo(&f, 0x36).or_else(|| absinfo(&f, 1));
            if ax.is_some() && ay.is_some() {
                self.has_touch = true;
            }
            self.devs.push(Dev { f, path, ax, ay, x: 0, y: 0, down: false, was_down: false, moved: false, shift: false, alt: false, altgr: false });
        }
    }

    /// Wartet bis zu `timeout_ms` auf Eingaben oder das Weck-Signal.
    pub fn wait(&mut self, timeout_ms: u16, wake: BorrowedFd) -> Vec<Ev> {
        if self.last_scan.map_or(true, |t| t.elapsed().as_secs() >= 3) {
            self.scan();
        }
        let mut fds: Vec<PollFd> = self.devs.iter().map(|d| PollFd::new(d.f.as_fd(), PollFlags::POLLIN)).collect();
        fds.push(PollFd::new(wake, PollFlags::POLLIN));
        let _ = poll(&mut fds, PollTimeout::from(timeout_ms));
        drop(fds);

        let mut out = Vec::new();
        let mut dead = Vec::new();
        let mut buf = [0u8; 24 * 64];
        for (di, d) in self.devs.iter_mut().enumerate() {
            loop {
                let n = match d.f.read(&mut buf) {
                    Ok(0) => {
                        dead.push(di);
                        break;
                    }
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        dead.push(di);
                        break;
                    }
                };
                for ev in buf[..n].chunks_exact(24) {
                    let ty = u16::from_ne_bytes([ev[16], ev[17]]);
                    let code = u16::from_ne_bytes([ev[18], ev[19]]);
                    let val = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                    match (ty, code) {
                        (3, 0x00) | (3, 0x35) => {
                            d.x = val;
                            d.moved = true;
                        }
                        (3, 0x01) | (3, 0x36) => {
                            d.y = val;
                            d.moved = true;
                        }
                        (3, 0x39) => d.down = val >= 0,             // ABS_MT_TRACKING_ID
                        (1, 0x14a) | (1, 0x110) => d.down = val != 0, // BTN_TOUCH / BTN_LEFT
                        (2, 0) => {
                            self.cursor.0 = (self.cursor.0 + val as f32 / 1000.0).clamp(0.0, 1.0);
                            d.moved = true;
                            self.has_mouse = true;
                        }
                        (2, 1) => {
                            self.cursor.1 = (self.cursor.1 + val as f32 / 1000.0).clamp(0.0, 1.0);
                            d.moved = true;
                            self.has_mouse = true;
                        }
                        (1, 42) | (1, 54) => {
                            // Alt+Umschalt: arabische/englische Belegung
                            if val == 1 && d.alt {
                                arabisch_umschalten();
                            }
                            d.shift = val != 0;
                        }
                        (1, 56) => {
                            if val == 1 && d.shift {
                                arabisch_umschalten();
                            }
                            d.alt = val != 0;
                        }
                        (1, 100) => d.altgr = val != 0,
                        (2, 8) => out.push(Ev::Rad(val)),
                        // Lautstärke (mit Autorepeat) und Power-Taste
                        (1, 115) if val != 0 => out.push(Ev::VolUp),
                        (1, 114) if val != 0 => out.push(Ev::VolDown),
                        (1, 116) if val == 1 => out.push(Ev::Power),
                        (1, k) if val != 0 && !d.alt => {
                            let ev = if arabisch() { key(k, d.shift) } else { key_belegung(k, d.shift, deutsch(), d.altgr) };
                            match ev {
                                Some(Ev::Char(c)) if arabisch() => {
                                    let (a, b) = arabisch_zeichen(c);
                                    out.push(Ev::Char(a));
                                    if let Some(b) = b {
                                        out.push(Ev::Char(b));
                                    }
                                }
                                Some(e) => out.push(e),
                                None => {}
                            }
                        }
                        (0, 0) => {
                            let pos = match (d.ax, d.ay) {
                                (Some((x0, x1)), Some((y0, y1))) => {
                                    let p = ((d.x - x0) as f32 / (x1 - x0) as f32, (d.y - y0) as f32 / (y1 - y0) as f32);
                                    self.cursor = p;
                                    p
                                }
                                _ => self.cursor,
                            };
                            if d.down && !d.was_down {
                                out.push(Ev::Down(pos.0, pos.1));
                            } else if !d.down && d.was_down {
                                out.push(Ev::Up);
                            } else if d.moved {
                                out.push(Ev::Move(pos.0, pos.1));
                            }
                            d.was_down = d.down;
                            d.moved = false;
                        }
                        _ => {}
                    }
                }
            }
        }
        for i in dead.into_iter().rev() {
            self.devs.remove(i);
        }
        out
    }
}

fn key(code: u16, shift: bool) -> Option<Ev> {
    key_belegung(code, shift, false, false)
}

/// Taste → Zeichen: US-Belegung oder deutsche (QWERTZ, mit AltGr)
fn key_belegung(code: u16, shift: bool, de: bool, altgr: bool) -> Option<Ev> {
    const US: [(u16, &str, &str); 4] = [
        (2, "1234567890-=", "!@#$%^&*()_+"),
        (16, "qwertyuiop[]", "QWERTYUIOP{}"),
        (30, "asdfghjkl;'`", "ASDFGHJKL:\"~"),
        (44, "zxcvbnm,./", "ZXCVBNM<>?"),
    ];
    const DE: [(u16, &str, &str); 4] = [
        (2, "1234567890ß´", "!\"§$%&/()=?`"),
        (16, "qwertzuiopü+", "QWERTZUIOPÜ*"),
        (30, "asdfghjklöä^", "ASDFGHJKLÖÄ°"),
        (44, "yxcvbnm,.-", "YXCVBNM;:_"),
    ];
    match code {
        1 => return Some(Ev::Back),
        14 => return Some(Ev::Backspace),
        28 | 96 => return Some(Ev::Enter),
        57 => return Some(Ev::Char(' ')),
        _ => {}
    }
    if de && altgr {
        let c = match code {
            16 => '@',
            18 => '€',
            3 => '²',
            4 => '³',
            8 => '{',
            9 => '[',
            10 => ']',
            11 => '}',
            12 => '\\',
            27 => '~',
            86 => '|',
            50 => 'µ',
            _ => return None,
        };
        return Some(Ev::Char(c));
    }
    match (code, de) {
        (43, false) => return Some(Ev::Char(if shift { '|' } else { '\\' })),
        (43, true) => return Some(Ev::Char(if shift { '\'' } else { '#' })),
        (86, true) => return Some(Ev::Char(if shift { '>' } else { '<' })),
        (86, false) => return Some(Ev::Char(if shift { '|' } else { '\\' })),
        _ => {}
    }
    for (start, lo, up) in if de { DE } else { US } {
        if code >= start && ((code - start) as usize) < lo.chars().count() {
            let s = if shift { up } else { lo };
            return s.chars().nth((code - start) as usize).map(Ev::Char);
        }
    }
    None
}
