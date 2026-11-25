//! Ausgabe: serielle Schnittstelle COM1 (Protokoll, Fernsteuerung) und Bildschirm (Bildspeicher der Firmware)
//! mit eingebetteter PSF-Schrift. Beide bekommen jede Zeile; `kprintln!` schreibt in beide.

use crate::arch::{inb, outb};
use crate::efi::Framebuffer;
use core::fmt::{self, Write};

const COM1: u16 = 0x3f8;

pub fn serial_init() {
    unsafe {
        outb(COM1 + 1, 0x00); // keine Interrupts
        outb(COM1 + 3, 0x80); // Teiler setzen
        outb(COM1, 0x01); // 115200 Baud
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x03); // 8N1
        outb(COM1 + 2, 0xc7); // FIFO
        outb(COM1 + 4, 0x0b);
    }
}

fn serial_put(b: u8) {
    unsafe {
        let mut n = 0;
        // wait for the transmitter (an emulator holds it while its output is not taken); a byte is
        // dropped only after a long wait, so a slow reader does not lose log lines
        while inb(COM1 + 5) & 0x20 == 0 && n < 2_000_000 {
            n += 1;
        }
        outb(COM1, b);
    }
}

/// Ein Zeichen von COM1, falls eines da ist
pub fn serial_get() -> Option<u8> {
    unsafe { (inb(COM1 + 5) & 1 != 0).then(|| inb(COM1)) }
}

// ------------------------------------------------------------------ PSF-Schrift

pub struct Font {
    data: &'static [u8],
    glyphs: usize,
    height: usize,
    /// Unicode → Glyphe (aus der Unicode-Tabelle der PSF-Datei), sortiert für Binärsuche
    map: [(u16, u16); 1024],
    n: usize,
}

impl Font {
    pub const fn empty() -> Font {
        Font { data: &[], glyphs: 0, height: 16, map: [(0, 0); 1024], n: 0 }
    }
    /// PSF1 mit Unicode-Tabelle (Modus 2/3)
    pub fn load(data: &'static [u8]) -> Font {
        let mut f = Font::empty();
        if data.len() < 4 || data[0] != 0x36 || data[1] != 0x04 {
            return f;
        }
        let mode = data[2];
        f.height = data[3] as usize;
        f.glyphs = if mode & 1 != 0 { 512 } else { 256 };
        f.data = &data[4..4 + f.glyphs * f.height];
        if mode & 2 != 0 {
            let mut p = 4 + f.glyphs * f.height;
            let mut g = 0u16;
            while p + 1 < data.len() && (g as usize) < f.glyphs {
                let u = u16::from_le_bytes([data[p], data[p + 1]]);
                p += 2;
                match u {
                    0xffff => g += 1,
                    0xfffe => {} // Folgen (kombinierende Zeichen) überspringen wir
                    _ => {
                        if f.n < f.map.len() {
                            f.map[f.n] = (u, g);
                            f.n += 1;
                        }
                    }
                }
            }
            f.map[..f.n].sort_unstable_by_key(|e| e.0);
        }
        f
    }
    pub fn glyph(&self, c: char) -> Option<&[u8]> {
        if self.data.is_empty() {
            return None;
        }
        let cp = c as u32;
        let g = if self.n > 0 {
            if cp > 0xffff {
                return None;
            }
            let i = self.map[..self.n].binary_search_by_key(&(cp as u16), |e| e.0).ok()?;
            self.map[i].1 as usize
        } else if (cp as usize) < self.glyphs {
            cp as usize
        } else {
            return None;
        };
        Some(&self.data[g * self.height..(g + 1) * self.height])
    }
}

static mut LATIN: Font = Font::empty();

/// Glyphe (8×16) für ein Zeichen: lateinisch, sonst arabisch
pub fn glyph(c: char) -> Option<&'static [u8]> {
    let lat = unsafe { &*core::ptr::addr_of!(LATIN) };
    let ara = unsafe { &*core::ptr::addr_of!(ARABIC) };
    lat.glyph(c).or_else(|| ara.glyph(c))
}

pub fn framebuffer() -> Option<Framebuffer> {
    unsafe { (*core::ptr::addr_of!(SCREEN)).fb }
}

/// Text in Bildschirmreihenfolge (Arabisch verbunden, von rechts nach links)
pub fn visual_line(s: &str) -> alloc::vec::Vec<char> {
    let log: alloc::vec::Vec<char> = s.chars().take(LINE).collect();
    let mut out = [' '; LINE];
    let n = visual(&log, &mut out);
    out[..n].to_vec()
}

/// Solange eine grafische Anwendung (Installation) den Bildschirm hat, schreibt die Konsole nur seriell
static GRAFIK: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn grafik(an: bool) {
    let war = GRAFIK.swap(an, core::sync::atomic::Ordering::SeqCst);
    if !an && war {
        // Konsole neu aufbauen
        let s = unsafe { &mut *core::ptr::addr_of_mut!(SCREEN) };
        if let Some(f) = s.fb {
            fill(&f, 0, 0, f.width as usize, f.height as usize, s.bg);
            fill(&f, 0, 0, f.width as usize, CH * TOP - 4, 0x141821);
            fill(&f, 0, CH * TOP - 4, f.width as usize, 2, 0xC9A227);
            let title = concat!(ultar_brand::product_caps!(), "  ·  ", ultar_brand::vendor!(), "  ·  Rust");
            for (i, ch) in title.chars().enumerate() {
                draw_char(&f, 1 + i, 0, ch, 0xE6C454, 0x141821, 8);
            }
            s.row = TOP;
            s.col = 0;
            s.len = 0;
            s.drawn = 0;
        }
    }
}
static mut ARABIC: Font = Font::empty();

// ------------------------------------------------------------------ Bildschirm

pub struct Screen {
    fb: Option<Framebuffer>,
    col: usize,
    row: usize,
    cols: usize,
    rows: usize,
    pub fg: u32,
    pub bg: u32,
    esc: u8,
    /// logischer Inhalt der aktuellen Zeile (für Arabisch: Formen verbinden und von rechts nach links zeigen)
    cells: [char; LINE],
    len: usize,
    /// so viele Zellen hat die letzte Neuzeichnung belegt
    drawn: usize,
}

const LINE: usize = 256;

pub static mut SCREEN: Screen = Screen {
    fb: None, col: 0, row: 0, cols: 0, rows: 0, fg: 0xE6E8EE, bg: 0x0B0D12, esc: 0,
    cells: [' '; LINE], len: 0, drawn: 0,
};

/// Spalten und Zeilen der Konsole (Bildschirm, sonst 80×25)
pub fn size() -> (usize, usize) {
    let s = unsafe { &*core::ptr::addr_of!(SCREEN) };
    if s.fb.is_some() {
        (s.cols, s.rows - TOP)
    } else {
        (80, 25)
    }
}

const CW: usize = 8;
const CH: usize = 16;
/// Kopfzeile mit der Marke
const TOP: usize = 2;

pub fn screen_init(fb: Option<Framebuffer>) {
    unsafe {
        LATIN = Font::load(include_bytes!("../fonts/terminus16.psf"));
        ARABIC = Font::load(include_bytes!("../fonts/arabic16.psf"));
        let s = &mut *core::ptr::addr_of_mut!(SCREEN);
        s.fb = fb;
        if let Some(f) = fb {
            s.cols = f.width as usize / CW;
            s.rows = f.height as usize / CH;
            s.row = TOP;
            fill(&f, 0, 0, f.width as usize, f.height as usize, s.bg);
            // Kopfleiste: Gold der Marke KM Code Craft
            fill(&f, 0, 0, f.width as usize, CH * TOP - 4, 0x141821);
            fill(&f, 0, CH * TOP - 4, f.width as usize, 2, 0xC9A227);
            let title = concat!(ultar_brand::product_caps!(), "  ·  ", ultar_brand::vendor!(), "  ·  Rust");
            for (i, ch) in title.chars().enumerate() {
                draw_char(&f, 1 + i, 0, ch, 0xE6C454, 0x141821, 8);
            }
        }
    }
}

#[inline]
fn px(f: &Framebuffer, x: usize, y: usize, rgb: u32) {
    let v = if f.bgr { rgb } else { ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff) };
    unsafe { core::ptr::write_volatile((f.base as *mut u32).add(y * f.stride as usize + x), v) };
}

fn fill(f: &Framebuffer, x: usize, y: usize, w: usize, h: usize, rgb: u32) {
    for yy in y..(y + h).min(f.height as usize) {
        for xx in x..(x + w).min(f.width as usize) {
            px(f, xx, yy, rgb);
        }
    }
}

fn draw_char(f: &Framebuffer, col: usize, row: usize, c: char, fg: u32, bg: u32, yoff: usize) {
    let (x0, y0) = (col * CW, row * CH + yoff / 2);
    let lat = unsafe { &*core::ptr::addr_of!(LATIN) };
    let ara = unsafe { &*core::ptr::addr_of!(ARABIC) };
    let g = lat.glyph(c).or_else(|| ara.glyph(c)).or_else(|| lat.glyph('?'));
    for y in 0..CH {
        let bits = g.map(|g| g.get(y).copied().unwrap_or(0)).unwrap_or(0);
        for x in 0..CW {
            px(f, x0 + x, y0 + y, if bits & (0x80 >> x) != 0 { fg } else { bg });
        }
    }
}

impl Screen {
    fn newline_reset(&mut self) {
        self.len = 0;
        self.drawn = 0;
    }
    /// Aktuelle Zeile neu zeichnen: arabische Buchstaben verbunden (Darstellungsformen) und von rechts nach links
    fn redraw(&mut self, f: &Framebuffer) {
        let has_ar = self.cells[..self.len].iter().any(|&c| is_arabic(c));
        if !has_ar {
            for x in 0..self.len.min(self.cols) {
                draw_char(f, x, self.row, self.cells[x], self.fg, self.bg, 0);
            }
            for x in self.len..self.drawn.min(self.cols) {
                draw_char(f, x, self.row, ' ', self.fg, self.bg, 0);
            }
            self.drawn = 0;
            return;
        }
        let mut vis = [' '; LINE];
        let n = visual(&self.cells[..self.len], &mut vis);
        for x in 0..n.min(self.cols) {
            draw_char(f, x, self.row, vis[x], self.fg, self.bg, 0);
        }
        for x in n..self.drawn.max(self.len).min(self.cols) {
            draw_char(f, x, self.row, ' ', self.fg, self.bg, 0);
        }
        self.drawn = n.max(1);
    }
    fn scroll(&mut self, f: &Framebuffer) {
        let top = TOP * CH;
        let stride = f.stride as usize;
        let h = self.rows * CH;
        unsafe {
            let base = f.base as *mut u32;
            core::ptr::copy(base.add((top + CH) * stride), base.add(top * stride), (h - top - CH) * stride);
        }
        fill(f, 0, h - CH, f.width as usize, CH, self.bg);
        self.row = self.rows - 1;
    }
    fn put(&mut self, c: char) {
        let Some(f) = self.fb else { return };
        // ANSI-Steuerfolgen (ESC [ … Buchstabe) der Programme: nur „Zeile löschen“ und „Bild löschen“ umsetzen
        match self.esc {
            1 => {
                self.esc = if c == '[' { 2 } else { 0 };
                return;
            }
            2 => {
                if ('\x40'..='\x7e').contains(&c) {
                    self.esc = 0;
                    if c == 'K' {
                        for x in self.col..self.cols {
                            draw_char(&f, x, self.row, ' ', self.fg, self.bg, 0);
                        }
                        self.len = self.len.min(self.col);
                        if self.drawn > 0 {
                            self.redraw(&f);
                        }
                    } else if c == 'J' {
                        fill(&f, 0, TOP * CH, f.width as usize, (self.rows - TOP) * CH, self.bg);
                        self.row = TOP;
                        self.col = 0;
                        self.newline_reset();
                    }
                }
                return;
            }
            _ => {}
        }
        if c == '\x1b' {
            self.esc = 1;
            return;
        }
        if c == '\x07' {
            return;
        }
        if c == '\t' {
            for _ in 0..(8 - self.col % 8) {
                self.put(' ');
            }
            return;
        }
        match c {
            '\n' => {
                self.col = 0;
                self.row += 1;
                self.newline_reset();
            }
            '\r' => self.col = 0,
            '\x08' => {
                if self.col > 0 {
                    self.col -= 1;
                    if self.drawn > 0 {
                        // Zelle bleibt bis zum Überschreiben im Puffer, sichtbar wird neu gezeichnet
                        self.redraw(&f);
                    } else {
                        draw_char(&f, self.col, self.row, ' ', self.fg, self.bg, 0);
                    }
                }
            }
            _ => {
                if self.col >= self.cols || self.col >= LINE {
                    self.col = 0;
                    self.row += 1;
                    self.newline_reset();
                }
                if self.row >= self.rows {
                    self.scroll(&f);
                }
                self.cells[self.col] = c;
                if self.col >= self.len {
                    self.len = self.col + 1;
                }
                self.col += 1;
                if self.drawn > 0 || is_arabic(c) {
                    self.redraw(&f);
                } else {
                    draw_char(&f, self.col - 1, self.row, c, self.fg, self.bg, 0);
                }
            }
        }
        if self.row >= self.rows {
            self.scroll(&f);
        }
    }
}

pub struct Out;

/// Sperre für die Ausgabe (mehrere Kerne); derselbe Kern darf erneut hinein (z. B. Ausnahme beim Schreiben)
static OWNER: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(usize::MAX);

/// Held while one piece of output is written: (this guard took the lock, interrupts were on before).
/// Interrupts stay off meanwhile, so a timer or device interrupt on the same core cannot print into the
/// middle of a line; other cores wait for the whole piece.
struct Guard(bool, bool);
impl Drop for Guard {
    fn drop(&mut self) {
        if self.0 {
            OWNER.store(usize::MAX, core::sync::atomic::Ordering::Release);
        }
        if self.1 {
            // SAFETY: interrupts were on when the guard was taken; turning them on again restores that state.
            unsafe { core::arch::asm!("sti", options(nomem, nostack)) };
        }
    }
}

/// Whether interrupts are on, and turn them off.
fn interrupts_off() -> bool {
    let flags: u64;
    // SAFETY: reads RFLAGS through the stack and clears IF; no memory of the program is touched.
    unsafe { core::arch::asm!("pushfq", "pop {}", "cli", out(reg) flags) };
    flags & (1 << 9) != 0
}

fn lock() -> Guard {
    let was_on = interrupts_off();
    let me = crate::percpu::id();
    if OWNER.load(core::sync::atomic::Ordering::Acquire) == me {
        return Guard(false, was_on);
    }
    let mut n = 0u64;
    while OWNER.compare_exchange(usize::MAX, me, core::sync::atomic::Ordering::Acquire, core::sync::atomic::Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
        n += 1;
        if n > 200_000_000 {
            // Notausgang: festgefahrene Sperre übernehmen
            OWNER.store(me, core::sync::atomic::Ordering::Release);
            break;
        }
    }
    Guard(true, was_on)
}

impl Write for Out {
    /// A formatted message is written under one lock: its pieces and its line end stay together.
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        let _g = lock();
        fmt::write(self, args)
    }

    fn write_str(&mut self, s: &str) -> fmt::Result {
        let _g = lock();
        for b in s.bytes() {
            if b == b'\n' {
                serial_put(b'\r');
            }
            serial_put(b);
        }
        if !GRAFIK.load(core::sync::atomic::Ordering::Relaxed) {
            let scr = unsafe { &mut *core::ptr::addr_of_mut!(SCREEN) };
            for c in s.chars() {
                scr.put(c);
            }
        }
        crate::klog::record(s);
        Ok(())
    }
}

/// Ausgabe von Benutzerprogrammen: Bildschirm und serielle Schnittstelle, aber NICHT das Kernprotokoll.
/// Sonst kann jedes Programm Zeilen wie `[treiber] …` in `/proc/ultar/protokoll` schreiben,
/// die `ultar-devd` als Aussage des Kerns liest.
pub struct UserOut;

impl Write for UserOut {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let _g = lock();
        for b in s.bytes() {
            if b == b'\n' {
                serial_put(b'\r');
            }
            serial_put(b);
        }
        if !GRAFIK.load(core::sync::atomic::Ordering::Relaxed) {
            let scr = unsafe { &mut *core::ptr::addr_of_mut!(SCREEN) };
            for c in s.chars() {
                scr.put(c);
            }
        }
        // The remote hardware test has no serial line to read: its only evidence is the log file, so program output is kept there too.
        #[cfg(feature = "remote-test")]
        crate::klog::record(s);
        Ok(())
    }
}

pub fn set_color(fg: u32) {
    unsafe { (*core::ptr::addr_of_mut!(SCREEN)).fg = fg };
}

#[macro_export]
macro_rules! kprint {
    ($($a:tt)*) => {{ let _ = core::fmt::Write::write_fmt(&mut $crate::console::Out, format_args!($($a)*)); }};
}
#[macro_export]
macro_rules! kprintln {
    () => { $crate::kprint!("\n") };
    // one call: the line and its end are written under the console lock together
    ($($a:tt)*) => {{ $crate::kprint!("{}\n", format_args!($($a)*)); }};
}

// ------------------------------------------------------------------ Arabisch: Formen und Leserichtung

fn is_arabic(c: char) -> bool {
    matches!(c as u32, 0x0600..=0x06ff | 0xfb50..=0xfdff | 0xfe70..=0xfeff)
}

fn is_mark(c: char) -> bool {
    matches!(c as u32, 0x064b..=0x0652 | 0x0670)
}

/// (isolierte Form, verbindet nach links = beidseitig)
fn forms(c: char) -> Option<(u32, bool)> {
    Some(match c as u32 {
        0x0621 => (0xfe80, false),
        0x0622 => (0xfe81, false),
        0x0623 => (0xfe83, false),
        0x0624 => (0xfe85, false),
        0x0625 => (0xfe87, false),
        0x0626 => (0xfe89, true),
        0x0627 => (0xfe8d, false),
        0x0628 => (0xfe8f, true),
        0x0629 => (0xfe93, false),
        0x062a => (0xfe95, true),
        0x062b => (0xfe99, true),
        0x062c => (0xfe9d, true),
        0x062d => (0xfea1, true),
        0x062e => (0xfea5, true),
        0x062f => (0xfea9, false),
        0x0630 => (0xfeab, false),
        0x0631 => (0xfead, false),
        0x0632 => (0xfeaf, false),
        0x0633 => (0xfeb1, true),
        0x0634 => (0xfeb5, true),
        0x0635 => (0xfeb9, true),
        0x0636 => (0xfebd, true),
        0x0637 => (0xfec1, true),
        0x0638 => (0xfec5, true),
        0x0639 => (0xfec9, true),
        0x063a => (0xfecd, true),
        0x0641 => (0xfed1, true),
        0x0642 => (0xfed5, true),
        0x0643 => (0xfed9, true),
        0x0644 => (0xfedd, true),
        0x0645 => (0xfee1, true),
        0x0646 => (0xfee5, true),
        0x0647 => (0xfee9, true),
        0x0648 => (0xfeed, false),
        0x0649 => (0xfeef, false),
        0x064a => (0xfef1, true),
        _ => return None,
    })
}

/// verbindet dieser Buchstabe zum folgenden (links)?
fn joins_left(c: char) -> bool {
    c == '\u{640}' || forms(c).map(|f| f.1).unwrap_or(false)
}
/// kann dieser Buchstabe mit dem vorigen (rechts) verbunden werden?
fn joins_right(c: char) -> bool {
    c == '\u{640}' || (forms(c).is_some() && c != '\u{621}')
}

fn lam_alef(c: char) -> Option<u32> {
    Some(match c {
        '\u{622}' => 0xfef5,
        '\u{623}' => 0xfef7,
        '\u{625}' => 0xfef9,
        '\u{627}' => 0xfefb,
        _ => return None,
    })
}

/// Text für einen Absatz mit Grundrichtung rechts→links (arabische Oberfläche): Abschnitte in umgekehrter Folge,
/// lateinische Wörter/Zahlen bleiben links→rechts.
pub fn visual_line_rtl(s: &str) -> alloc::vec::Vec<char> {
    let log: alloc::vec::Vec<char> = s.chars().take(LINE).collect();
    let shaped = shape(&log);
    let ltr = |c: char| !is_arabic(c) && c.is_alphanumeric();
    // Abschnitte: L = lateinisch/Ziffern (Leerzeichen/Satzzeichen dazwischen gehören dazu, wenn danach wieder L folgt)
    let mut runs: alloc::vec::Vec<(bool, alloc::vec::Vec<char>)> = alloc::vec::Vec::new();
    let mut i = 0;
    while i < shaped.len() {
        if ltr(shaped[i]) {
            let mut end = i + 1;
            let mut j = i + 1;
            while j < shaped.len() && !is_arabic(shaped[j]) {
                if ltr(shaped[j]) {
                    end = j + 1;
                }
                j += 1;
            }
            runs.push((true, shaped[i..end].to_vec()));
            i = end;
        } else {
            let mut j = i;
            while j < shaped.len() && !ltr(shaped[j]) {
                j += 1;
            }
            runs.push((false, shaped[i..j].to_vec()));
            i = j;
        }
    }
    let mut out = alloc::vec::Vec::with_capacity(shaped.len());
    for (l, r) in runs.into_iter().rev() {
        if l {
            out.extend(r);
        } else {
            out.extend(r.into_iter().rev().map(|c| match c {
                '(' => ')',
                ')' => '(',
                '[' => ']',
                ']' => '[',
                '<' => '>',
                '>' => '<',
                '«' => '»',
                '»' => '«',
                c => c,
            }));
        }
    }
    out
}

/// Formen bilden (Zeichen ohne Zelle: Vokalzeichen; Lam+Alef wird eine Zelle)
fn shape(log: &[char]) -> alloc::vec::Vec<char> {
    let letters: alloc::vec::Vec<(usize, char)> = log.iter().copied().enumerate().filter(|(_, c)| !is_mark(*c)).collect();
    let mut shaped: alloc::vec::Vec<char> = alloc::vec::Vec::with_capacity(log.len());
    let mut i = 0;
    while i < letters.len() {
        let c = letters[i].1;
        let prev = if i > 0 { Some(letters[i - 1].1) } else { None };
        let next = letters.get(i + 1).map(|x| x.1);
        let jp = prev.map(joins_left).unwrap_or(false) && joins_right(c);
        if c == '\u{644}' {
            if let Some(lig) = next.and_then(lam_alef) {
                shaped.push(char::from_u32(lig + jp as u32).unwrap_or(c));
                i += 2;
                continue;
            }
        }
        match forms(c) {
            Some((iso, dual)) => {
                let jn = dual && next.map(joins_right).unwrap_or(false);
                let f = match (jp, jn) {
                    (false, false) => iso,
                    (true, false) => iso + 1,
                    (false, true) => iso + 2,
                    (true, true) => iso + 3,
                };
                shaped.push(char::from_u32(f).unwrap_or(c));
            }
            None => shaped.push(c),
        }
        i += 1;
    }
    shaped
}

/// logische Zeile → Bildschirmzellen (Formen verbunden, arabische Abschnitte gespiegelt). Liefert die Zellenzahl.
fn visual(log: &[char], out: &mut [char; LINE]) -> usize {
    let shaped = shape(log);
    let n = shaped.len().min(LINE);
    out[..n].copy_from_slice(&shaped[..n]);
    // 2) Leserichtung: Grundrichtung links→rechts; Abschnitte aus Arabisch (mit Leerzeichen/Ziffern dazwischen) spiegeln
    let mut k = 0;
    while k < n {
        if !is_arabic(out[k]) {
            k += 1;
            continue;
        }
        let start = k;
        let mut end = k + 1; // exklusiv, endet nach dem letzten arabischen Zeichen
        let mut j = k + 1;
        while j < n {
            let c = out[j];
            if is_arabic(c) {
                end = j + 1;
            } else if c.is_ascii_alphabetic() {
                break;
            }
            j += 1;
        }
        out[start..end].reverse();
        // Ziffernfolgen im gespiegelten Abschnitt wieder richtig herum, Klammern spiegeln
        let mut d = start;
        while d < end {
            if out[d].is_ascii_digit() {
                let ds = d;
                while d < end && (out[d].is_ascii_digit() || out[d] == '.' || out[d] == ',') {
                    d += 1;
                }
                out[ds..d].reverse();
            } else {
                out[d] = match out[d] {
                    '(' => ')',
                    ')' => '(',
                    '[' => ']',
                    ']' => '[',
                    '<' => '>',
                    '>' => '<',
                    '{' => '}',
                    '}' => '{',
                    c => c,
                };
                d += 1;
            }
        }
        k = end;
    }
    n
}
