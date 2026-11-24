//! PS/2-Tastatur (i8042, Abfragebetrieb): Laptops intern meist PS/2-kompatibel angebunden, VMs immer.
//! Belegung umschaltbar: deutsch (Standard) oder US.

use crate::arch::inb;
use core::sync::atomic::{AtomicBool, Ordering};

static SHIFT: AtomicBool = AtomicBool::new(false);
static ALTGR: AtomicBool = AtomicBool::new(false);
static E0: AtomicBool = AtomicBool::new(false);
static ALT: AtomicBool = AtomicBool::new(false);
/// Pfeiltasten (privater Unicode-Bereich, die Konsole ignoriert sie)
pub const HOCH: char = '\u{E000}';
pub const RUNTER: char = '\u{E001}';
pub const LINKS: char = '\u{E002}';
pub const RECHTS: char = '\u{E003}';
pub static GERMAN: AtomicBool = AtomicBool::new(true);
/// Arabische Belegung (Standard-PC-Belegung „Arabisch 101“); Ziffern bleiben 0–9. Umschalten: Alt+Shift oder `tastatur ar`.
pub static ARABIC: AtomicBool = AtomicBool::new(false);
/// zweites Zeichen einer Taste (لا, لأ, لإ, لآ)
static PENDING: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// US-Zeichen → arabisches Zeichen (zweites Zeichen für Lam-Alef in PENDING)
pub fn arabisch(c: char) -> char {
    let (a, b): (char, Option<char>) = match c {
        'q' => ('ض', None),
        'w' => ('ص', None),
        'e' => ('ث', None),
        'r' => ('ق', None),
        't' => ('ف', None),
        'y' => ('غ', None),
        'u' => ('ع', None),
        'i' => ('ه', None),
        'o' => ('خ', None),
        'p' => ('ح', None),
        '[' => ('ج', None),
        ']' => ('د', None),
        'a' => ('ش', None),
        's' => ('س', None),
        'd' => ('ي', None),
        'f' => ('ب', None),
        'g' => ('ل', None),
        'h' => ('ا', None),
        'j' => ('ت', None),
        'k' => ('ن', None),
        'l' => ('م', None),
        ';' => ('ك', None),
        '\'' => ('ط', None),
        '`' => ('ذ', None),
        'z' => ('ئ', None),
        'x' => ('ء', None),
        'c' => ('ؤ', None),
        'v' => ('ر', None),
        'b' => ('ل', Some('ا')),
        'n' => ('ى', None),
        'm' => ('ة', None),
        ',' => ('و', None),
        '.' => ('ز', None),
        '/' => ('ظ', None),
        'Q' => ('\u{64e}', None),
        'W' => ('\u{64b}', None),
        'E' => ('\u{64f}', None),
        'R' => ('\u{64c}', None),
        'T' => ('ل', Some('إ')),
        'Y' => ('إ', None),
        'U' => ('‘', None),
        'I' => ('÷', None),
        'O' => ('×', None),
        'P' => ('؛', None),
        '{' => ('<', None),
        '}' => ('>', None),
        'A' => ('\u{650}', None),
        'S' => ('\u{64d}', None),
        'D' => (']', None),
        'F' => ('[', None),
        'G' => ('ل', Some('أ')),
        'H' => ('أ', None),
        'J' => ('ـ', None),
        'K' => ('،', None),
        'L' => ('/', None),
        '~' => ('\u{651}', None),
        'Z' => ('~', None),
        'X' => ('\u{652}', None),
        'C' => ('}', None),
        'V' => ('{', None),
        'B' => ('ل', Some('آ')),
        'N' => ('آ', None),
        'M' => ('\'', None),
        '<' => (',', None),
        '>' => ('.', None),
        '?' => ('؟', None),
        _ => (c, None),
    };
    if let Some(b) = b {
        PENDING.store(b as u32, Ordering::Relaxed);
    }
    a
}

fn umschalten() {
    let v = !ARABIC.load(Ordering::Relaxed);
    ARABIC.store(v, Ordering::Relaxed);
}

//                      0    1    2    3    4    5    6    7    8    9    a    b    c    d    e    f
const DE: &[u8; 58] =
    b"\0\x1b1234567890\xdf\xb4\x08\tqwertzuiop\xfc+\n\0asdfghjkl\xf6\xe4^\0#yxcvbnm,.-\0*\0 ";
const DE_S: &[u8; 58] =
    b"\0\x1b!\"\xa7$%&/()=?`\x08\tQWERTZUIOP\xdc*\n\0ASDFGHJKL\xd6\xc4\xb0\0'YXCVBNM;:_\0*\0 ";
const US: &[u8; 58] = b"\0\x1b1234567890-=\x08\tqwertyuiop[]\n\0asdfghjkl;'`\0\\zxcvbnm,./\0*\0 ";
const US_S: &[u8; 58] = b"\0\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\n\0ASDFGHJKL:\"~\0|ZXCVBNM<>?\0*\0 ";

/// Ein Zeichen, falls eine Taste gedrückt wurde (Latin-1 → char)
pub fn poll() -> Option<char> {
    if let Some(c) = char::from_u32(PENDING.swap(0, Ordering::Relaxed)).filter(|&c| c != '\0') {
        return Some(c);
    }
    let ar = ARABIC.load(Ordering::Relaxed);
    // USB-Tastaturen (xHCI) zuerst – bei arabischer Belegung liefern sie US-Zeichen, die hier umgesetzt werden
    if let Some(c) = crate::drivers::gen_xhci::poll_key() {
        return Some(if ar { arabisch(c) } else { c });
    }
    unsafe {
        let st = inb(0x64);
        // 0xFF = kein Tastaturcontroller (moderne Rechner ohne i8042, z. B. Beelink GTi15): nichts lesen
        if st == 0xff || st & 1 == 0 {
            return None;
        }
        if st & 0x20 != 0 {
            crate::drivers::ps2maus::byte(inb(0x60)); // Byte der Maus
            return None;
        }
        let sc = inb(0x60);
        if sc == 0xe0 {
            E0.store(true, Ordering::Relaxed);
            return None;
        }
        let e0 = E0.swap(false, Ordering::Relaxed);
        let rel = sc & 0x80 != 0;
        let code = sc & 0x7f;
        match code {
            0x2a | 0x36 => {
                if !rel && !SHIFT.load(Ordering::Relaxed) && ALT.load(Ordering::Relaxed) {
                    umschalten();
                }
                SHIFT.store(!rel, Ordering::Relaxed);
                return None;
            }
            0x38 if e0 => {
                ALTGR.store(!rel, Ordering::Relaxed);
                return None;
            }
            0x38 => {
                if !rel && !ALT.load(Ordering::Relaxed) && SHIFT.load(Ordering::Relaxed) {
                    umschalten();
                }
                ALT.store(!rel, Ordering::Relaxed);
                return None;
            }
            _ => {}
        }
        if rel {
            return None;
        }
        if e0 {
            return match code {
                0x48 => Some(HOCH),
                0x50 => Some(RUNTER),
                0x4b => Some(LINKS),
                0x4d => Some(RECHTS),
                0x1c => Some('\n'),
                _ => None,
            };
        }
        let de = GERMAN.load(Ordering::Relaxed) && !ar;
        if ALTGR.load(Ordering::Relaxed) && de {
            return match code {
                0x10 => Some('@'),
                0x1b => Some('~'),
                0x56 => Some('|'),
                0x08 => Some('{'),
                0x09 => Some('['),
                0x0a => Some(']'),
                0x0b => Some('}'),
                0x0c => Some('\\'),
                0x12 => Some('€'),
                _ => None,
            };
        }
        if code == 0x56 {
            return Some(if SHIFT.load(Ordering::Relaxed) {
                '>'
            } else {
                '<'
            });
        }
        let t = match (de, SHIFT.load(Ordering::Relaxed)) {
            (true, false) => DE,
            (true, true) => DE_S,
            (false, false) => US,
            (false, true) => US_S,
        };
        let b = *t.get(code as usize)?;
        if b == 0 {
            return None;
        }
        let c = b as char;
        Some(
            if ar && !c.is_ascii_digit() && !c.is_control() && c != ' ' {
                arabisch(c)
            } else {
                c
            },
        )
    }
}
