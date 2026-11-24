//! Grafische Installation von Ultar OS – läuft direkt auf dem Ultar-Kern (ohne Linux, ohne Fenstersystem).
//!
//! Ablauf: Sprache → Zielplatte → Bestätigen → Installation (Partitionen kopieren, prüfen, GPT schreiben) → Fertig.
//! Bedienung mit Maus (PS/2), Tastatur (Pfeile/Tab/Enter/Esc) oder über die serielle Konsole (Tests).
//! Quelle ist das Startmedium (GPT mit `ULTAR-INSTALL` oder `ULTAR-EFI` und `ultar-compat`); auf der Zielplatte heißt die
//! Startpartition `ULTAR-EFI` – das installierte System startet deshalb nicht erneut in die Installation.

#[path = "install_ab.rs"]
mod ab;

use crate::driver::BLOCK;
use crate::efi::Framebuffer;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ------------------------------------------------------------------ Sprache

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Lang {
    Ar,
    De,
    En,
}

pub(crate) fn t(l: Lang, k: &str) -> &'static str {
    use Lang::*;
    match (k, l) {
        ("welcome", Ar) => concat!("مرحباً بك في ", ultar_brand::product!()),
        ("welcome", De) => concat!("Willkommen bei ", ultar_brand::product!()),
        ("welcome", En) => concat!("Welcome to ", ultar_brand::product!()),
        ("welcome_sub", Ar) => "اختر لغة التثبيت",
        ("welcome_sub", De) => "Sprache der Installation wählen",
        ("welcome_sub", En) => "Choose the installation language",
        ("s1", Ar) => "اللغة",
        ("s1", De) => "Sprache",
        ("s1", En) => "Language",
        ("s2", Ar) => "القرص",
        ("s2", De) => "Zielplatte",
        ("s2", En) => "Target disk",
        ("s3", Ar) => "التأكيد",
        ("s3", De) => "Bestätigen",
        ("s3", En) => "Confirm",
        ("s4", Ar) => "التثبيت",
        ("s4", De) => "Installation",
        ("s4", En) => "Installation",
        ("s5", Ar) => "انتهى",
        ("s5", De) => "Fertig",
        ("s5", En) => "Done",
        ("step", Ar) => "الخطوة",
        ("step", De) => "Schritt",
        ("step", En) => "Step",
        ("of", Ar) => "من",
        ("of", De) => "von",
        ("of", En) => "of",
        ("disk_title", Ar) => concat!("أين نثبت ", ultar_brand::product!(), "؟"),
        ("disk_title", De) => concat!("Wohin soll ", ultar_brand::product!(), "?"),
        ("disk_title", En) => concat!("Where should ", ultar_brand::product!(), " go?"),
        ("disk_sub", Ar) => "اختر القرص. سيتم مسح كل محتواه.",
        ("disk_sub", De) => "Platte wählen. Ihr ganzer Inhalt wird gelöscht.",
        ("disk_sub", En) => "Choose a disk. All of its contents will be erased.",
        ("no_disk", Ar) => "لا يوجد قرص مناسب. وصّل قرصاً ثم ابحث من جديد.",
        ("no_disk", De) => "Keine passende Platte gefunden. Platte anschließen und neu suchen.",
        ("no_disk", En) => "No suitable disk found. Connect a disk and search again.",
        ("search", Ar) => "بحث من جديد",
        ("search", De) => "Neu suchen",
        ("search", En) => "Search again",
        ("back", Ar) => "رجوع",
        ("back", De) => "Zurück",
        ("back", En) => "Back",
        ("next", Ar) => "التالي",
        ("next", De) => "Weiter",
        ("next", En) => "Next",
        ("cancel", Ar) => "إلغاء",
        ("cancel", De) => "Abbrechen",
        ("cancel", En) => "Cancel",
        ("confirm_title", Ar) => "تأكيد التثبيت",
        ("confirm_title", De) => "Installation bestätigen",
        ("confirm_title", En) => "Confirm installation",
        ("confirm_warn", Ar) => "سيتم مسح كل البيانات على هذا القرص نهائياً:",
        ("confirm_warn", De) => "Alle Daten auf dieser Platte werden endgültig gelöscht:",
        ("confirm_warn", En) => "All data on this disk will be permanently erased:",
        ("confirm_btn", Ar) => "مسح وتثبيت",
        ("confirm_btn", De) => "Löschen und installieren",
        ("confirm_btn", En) => "Erase and install",
        ("inst_title", Ar) => concat!("جارٍ تثبيت ", ultar_brand::product!()),
        ("inst_title", De) => concat!(ultar_brand::product!(), " wird installiert"),
        ("inst_title", En) => concat!("Installing ", ultar_brand::product!()),
        ("inst_wait", Ar) => "لا تطفئ الجهاز.",
        ("inst_wait", De) => "Gerät nicht ausschalten.",
        ("inst_wait", En) => "Do not turn off the device.",
        ("ph_copy", Ar) => "نسخ",
        ("ph_copy", De) => "Kopieren:",
        ("ph_copy", En) => "Copying",
        ("ph_verify", Ar) => "تحقق",
        ("ph_verify", De) => "Prüfen:",
        ("ph_verify", En) => "Verifying",
        ("ph_gpt", Ar) => "كتابة جدول الأقسام",
        ("ph_gpt", De) => "Partitionstabelle schreiben",
        ("ph_gpt", En) => "Writing partition table",
        ("done_title", Ar) => concat!("تم تثبيت ", ultar_brand::product!()),
        ("done_title", De) => concat!(ultar_brand::product!(), " ist installiert"),
        ("done_title", En) => concat!(ultar_brand::product!(), " is installed"),
        ("done_text", Ar) => "أعد التشغيل الآن – حتى مع بقاء وسيط التثبيت يبدأ النظام المثبت.",
        ("done_text", De) => "Jetzt neu starten – auch mit eingestecktem Medium startet das installierte System.",
        ("done_text", En) => "Restart now – the installed system starts even with the medium attached.",
        ("restart", Ar) => "إعادة التشغيل",
        ("restart", De) => "Neu starten",
        ("restart", En) => "Restart",
        ("console", Ar) => "إلى الطرفية",
        ("console", De) => "Zur Konsole",
        ("console", En) => "To console",
        ("dev_title", Ar) => "الأجهزة المكتشفة",
        ("dev_title", De) => "Erkannte Geräte",
        ("dev_title", En) => "Detected devices",
        ("dev_sub", Ar) => "تحقق Ultar من العتاد والشبكة قبل التثبيت.",
        ("dev_sub", De) => "Ultar hat Hardware und Netz vorab geprüft.",
        ("dev_sub", En) => "Ultar checked hardware and network in advance.",
        ("cat_net", Ar) => "شبكة سلكية",
        ("cat_net", De) => "Netzwerk (Kabel)",
        ("cat_net", En) => "Network (wired)",
        ("cat_wlan", Ar) => "شبكة لاسلكية WLAN",
        ("cat_wlan", De) => "WLAN",
        ("cat_wlan", En) => "Wi-Fi",
        ("cat_disk", Ar) => "التخزين",
        ("cat_disk", De) => "Speicher",
        ("cat_disk", En) => "Storage",
        ("cat_usb", Ar) => "USB",
        ("cat_usb", De) => "USB",
        ("cat_usb", En) => "USB",
        ("cat_gpu", Ar) => "الرسوميات",
        ("cat_gpu", De) => "Grafik",
        ("cat_gpu", En) => "Graphics",
        ("cat_audio", Ar) => "الصوت",
        ("cat_audio", De) => "Ton",
        ("cat_audio", En) => "Audio",
        ("cat_bridge", Ar) => "الشرائح والجسور",
        ("cat_bridge", De) => "Chipsatz/Brücken",
        ("cat_bridge", En) => "Chipset/bridges",
        ("cat_other", Ar) => "أجهزة أخرى",
        ("cat_other", De) => "Weitere Geräte",
        ("cat_other", En) => "Other devices",
        ("all_dev", Ar) => "كل الأجهزة",
        ("all_dev", De) => "Alle Geräte",
        ("all_dev", En) => "All devices",
        ("page", Ar) => "صفحة",
        ("page", De) => "Seite",
        ("page", En) => "Page",
        ("drv_ok", Ar) => "التعريف يعمل",
        ("drv_ok", De) => "Treiber aktiv",
        ("drv_ok", En) => "driver active",
        ("drv_missing", Ar) => "بلا تعريف بعد (الورشة)",
        ("drv_missing", De) => "noch ohne Treiber (Werkstatt)",
        ("drv_missing", En) => "no driver yet (workshop)",
        ("fb", Ar) => "عبر ذاكرة الشاشة",
        ("fb", De) => "über den Bildspeicher der Firmware",
        ("fb", En) => "via firmware framebuffer",
        ("connect", Ar) => "اتصال بالشبكة",
        ("connect", De) => "Netz verbinden",
        ("connect", En) => "Connect",
        ("net_no", Ar) => "غير متصل بالشبكة",
        ("net_no", De) => "Nicht verbunden",
        ("net_no", En) => "Not connected",
        ("net_dev_no", Ar) => "لا توجد بطاقة شبكة بتعريف",
        ("net_dev_no", De) => "Keine Netzwerkkarte mit Treiber",
        ("net_dev_no", En) => "No network card with driver",
        ("inet_ok", Ar) => "الإنترنت متاح",
        ("inet_ok", De) => "Internet erreichbar",
        ("inet_ok", En) => "Internet reachable",
        ("inet_no", Ar) => "لا يوجد إنترنت",
        ("inet_no", De) => "kein Internet",
        ("inet_no", En) => "no Internet",
        ("wlan_hint", Ar) => "تسجيل الدخول إلى WLAN يأتي مع تعريف البطاقة.",
        ("wlan_hint", De) => "WLAN-Anmeldung folgt mit dem Treiber der Karte.",
        ("wlan_hint", En) => "Wi-Fi sign-in follows with the card's driver.",
        ("err_title", Ar) => "فشل التثبيت",
        ("err_title", De) => "Installation fehlgeschlagen",
        ("err_title", En) => "Installation failed",
        ("hint", Ar) => "الأسهم أو Tab للتنقل، Enter للاختيار، والفأرة تعمل أيضاً",
        ("hint", De) => "Pfeile/Tab wechseln · Enter wählt · Maus geht auch",
        ("hint", En) => "Arrows/Tab to move · Enter to select · mouse works too",
        ("source", Ar) => "المصدر",
        ("source", De) => "Quelle",
        ("source", En) => "Source",
        ("verified", Ar) => "تم التحقق من كل البيانات.",
        ("verified", De) => "Alle Daten wurden geprüft.",
        ("verified", En) => "All data was verified.",
        _ => "",
    }
}

// ------------------------------------------------------------------ Zeichnen

pub(crate) const BG: u32 = 0x0B0D12;
pub(crate) const BAR: u32 = 0x141821;
pub(crate) const CARD: u32 = 0x12151C;
pub(crate) const LINE: u32 = 0x232836;
pub(crate) const TEXT: u32 = 0xE6E8EE;
pub(crate) const MUTED: u32 = 0x9AA0AC;
pub(crate) const GOLD: u32 = 0xC9A227;
pub(crate) const GOLD2: u32 = 0xE6C65A;
pub(crate) const DARK: u32 = 0x10100A;
pub(crate) const RED: u32 = 0xE5484D;
pub(crate) const GREEN: u32 = 0x46A758;

// ------------------------------------------------------------------ Vektorschrift

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};

static NOTO: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf");
static NOTO_FETT: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Bold.ttf");
static NOTO_AR: &[u8] = include_bytes!("../../../assets/fonts/NotoSansArabic-Regular.ttf");

struct Bild {
    w: i32,
    h: i32,
    ox: i32,
    oy: i32,
    adv: i32,
    cov: Vec<u8>,
}

struct Schriften {
    lat: FontRef<'static>,
    fett: FontRef<'static>,
    ar: FontRef<'static>,
    cache: alloc::collections::BTreeMap<(char, u16, bool), Option<Bild>>,
}

static SCHRIFT: spin::Mutex<Option<Schriften>> = spin::Mutex::new(None);

fn px_groesse(scale: i32) -> u16 {
    match scale {
        1 => 17,
        2 => 25,
        3 => 38,
        s => (s * 13) as u16,
    }
}

fn ist_ar(c: char) -> bool {
    matches!(c as u32, 0x0600..=0x06ff | 0xfb50..=0xfdff | 0xfe70..=0xfeff)
}

/// Glyphe als Deckungsbild (0–255), Abstand zur Oberkante der Zeile und Vorschub
fn glyphe<R>(c: char, px: u16, fett: bool, f: impl FnOnce(Option<&Bild>) -> R) -> R {
    let mut g = SCHRIFT.lock();
    if g.is_none() {
        *g = Some(Schriften {
            lat: FontRef::try_from_slice(NOTO).expect("Noto Sans"),
            fett: FontRef::try_from_slice(NOTO_FETT).expect("Noto Sans Bold"),
            ar: FontRef::try_from_slice(NOTO_AR).expect("Noto Sans Arabic"),
            cache: alloc::collections::BTreeMap::new(),
        });
    }
    let s = g.as_mut().unwrap();
    if !s.cache.contains_key(&(c, px, fett)) {
        let lat = if fett { &s.fett } else { &s.lat };
        let font = if ist_ar(c) && s.ar.glyph_id(c).0 != 0 { &s.ar } else { lat };
        let gid = font.glyph_id(c);
        let bild = if gid.0 == 0 && c != ' ' {
            None
        } else {
            let lat_sc = PxScale::from(px as f32);
            let sc = if core::ptr::eq(font, &s.ar) { PxScale::from(px as f32 * 1.22) } else { lat_sc };
            // gemeinsame Grundlinie: Aufstieg der lateinischen Schrift
            let asc = lat.as_scaled(lat_sc).ascent();
            let adv = font.as_scaled(sc).h_advance(gid);
            let glyph = gid.with_scale_and_position(sc, ab_glyph::point(0.0, asc));
            match font.outline_glyph(glyph) {
                Some(o) => {
                    let b = o.px_bounds();
                    let (w, h) = ((b.max.x - b.min.x) as i32 + 1, (b.max.y - b.min.y) as i32 + 1);
                    let mut cov = vec![0u8; (w * h) as usize];
                    o.draw(|x, y, v| {
                        let i = (y as i32 * w + x as i32) as usize;
                        if i < cov.len() {
                            cov[i] = (v.clamp(0.0, 1.0) * 255.0) as u8;
                        }
                    });
                    Some(Bild { w, h, ox: b.min.x as i32, oy: b.min.y as i32, adv: (adv + 0.5) as i32, cov })
                }
                None => Some(Bild { w: 0, h: 0, ox: 0, oy: 0, adv: (adv + 0.5) as i32, cov: Vec::new() }),
            }
        };
        s.cache.insert((c, px, fett), bild);
    }
    f(s.cache.get(&(c, px, fett)).and_then(|b| b.as_ref()))
}

fn mischen(a: u32, b: u32, t: u32) -> u32 {
    let ch = |s: u32| -> u32 {
        let x = (a >> s) & 0xff;
        let y = (b >> s) & 0xff;
        ((x * (255 - t) + y * t) / 255) << s
    };
    ch(16) | ch(8) | ch(0)
}

pub(crate) struct Canvas {
    pub(crate) w: usize,
    pub(crate) h: usize,
    pub(crate) px: Vec<u32>,
    pub(crate) rtl: bool,
}

fn sichtbar(s: &str, rtl: bool) -> Vec<char> {
    if rtl { crate::console::visual_line_rtl(s) } else { crate::console::visual_line(s) }
}

impl Canvas {
    pub(crate) fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        let (x0, y0) = (x.max(0) as usize, y.max(0) as usize);
        let (x1, y1) = (((x + w).max(0) as usize).min(self.w), ((y + h).max(0) as usize).min(self.h));
        for yy in y0..y1 {
            self.px[yy * self.w + x0..yy * self.w + x1.max(x0)].fill(c);
        }
    }
    /// Rechteck mit runden Ecken
    pub(crate) fn rrect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, c: u32) {
        for yy in 0..h {
            let dy = if yy < r { r - yy } else if yy >= h - r { yy - (h - r - 1) } else { 0 };
            let inset = if dy > 0 { r - isqrt(r * r - dy * dy) } else { 0 };
            self.rect(x + inset, y + yy, w - 2 * inset, 1, c);
        }
    }
    /// gerahmtes Feld: Rahmen in `b`, Fläche in `c`
    pub(crate) fn frame(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, b: u32, c: u32, dicke: i32) {
        self.rrect(x, y, w, h, r, b);
        self.rrect(x + dicke, y + dicke, w - 2 * dicke, h - 2 * dicke, (r - dicke).max(0), c);
    }
    fn glyph(&mut self, x: i32, y: i32, c: char, s: i32, col: u32) {
        let Some(g) = crate::console::glyph(c) else { return };
        for (gy, &bits) in g.iter().enumerate() {
            for gx in 0..8 {
                if bits & (0x80 >> gx) != 0 {
                    self.rect(x + gx * s, y + gy as i32 * s, s, s, col);
                }
            }
        }
    }
    /// Text in Bildschirmreihenfolge (Arabisch verbunden und gespiegelt), Vektorschrift; liefert die Breite
    pub(crate) fn text(&mut self, x: i32, y: i32, s: &str, scale: i32, col: u32) -> i32 {
        let vis = sichtbar(s, self.rtl);
        let px = px_groesse(scale);
        let fett = scale >= 3;
        let mut pen = x;
        for &c in vis.iter() {
            let mut fehlt = false;
            let (cw, ch) = (self.w as i32, self.h as i32);
            let px_buf = &mut self.px;
            let adv = glyphe(c, px, fett, |b| match b {
                Some(b) => {
                    for gy in 0..b.h {
                        let yy = y + b.oy + gy;
                        if yy < 0 || yy >= ch {
                            continue;
                        }
                        for gx in 0..b.w {
                            let xx = pen + b.ox + gx;
                            if xx < 0 || xx >= cw {
                                continue;
                            }
                            let t = b.cov[(gy * b.w + gx) as usize] as u32;
                            if t > 0 {
                                let i = yy as usize * cw as usize + xx as usize;
                                px_buf[i] = mischen(px_buf[i], col, t);
                            }
                        }
                    }
                    b.adv
                }
                None => {
                    fehlt = true;
                    8 * scale
                }
            });
            if fehlt {
                self.glyph(pen, y, c, scale, col);
            }
            pen += adv;
        }
        pen - x
    }
    pub(crate) fn text_w(s: &str, scale: i32) -> i32 {
        let px = px_groesse(scale);
        let fett = scale >= 3;
        crate::console::visual_line(s).iter().map(|&c| glyphe(c, px, fett, |b| b.map(|b| b.adv).unwrap_or(8 * scale))).sum()
    }
    pub(crate) fn text_c(&mut self, cx: i32, y: i32, s: &str, scale: i32, col: u32) {
        let w = Self::text_w(s, scale);
        self.text(cx - w / 2, y, s, scale, col);
    }
    /// größte Schrift (bis `max`), die in `breite` passt
    pub(crate) fn passend(s: &str, max: i32, breite: i32) -> i32 {
        let mut sc = max;
        while sc > 1 && Self::text_w(s, sc) > breite {
            sc -= 1;
        }
        sc
    }
    fn cursor(&mut self, x: i32, y: i32) {
        // Pfeil 12×19 aus Zeilenbreiten
        const B: [i32; 17] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 7, 7, 4, 4, 3, 2];
        for (i, &w) in B.iter().enumerate() {
            self.rect(x - 1, y + i as i32, w + 2, 1, 0x000000);
        }
        for (i, &w) in B.iter().enumerate().take(16) {
            if w > 1 {
                self.rect(x, y + i as i32 + 1, w - 1, 1, 0xFFFFFF);
            }
        }
    }
}

fn isqrt(v: i32) -> i32 {
    if v <= 0 {
        return 0;
    }
    let mut r = 0;
    while (r + 1) * (r + 1) <= v {
        r += 1;
    }
    r
}

fn farbe(fb: &Framebuffer, v: u32) -> u32 {
    if fb.bgr { v } else { ((v & 0xff) << 16) | (v & 0xff00) | ((v >> 16) & 0xff) }
}

/// Ausschnitt der Leinwand auf den Bildschirm (z. B. unter dem alten Mauszeiger)
pub(crate) fn blit_rect(fb: &Framebuffer, cv: &Canvas, x: i32, y: i32, w: i32, h: i32) {
    let (x0, y0) = (x.max(0) as usize, y.max(0) as usize);
    let x1 = ((x + w).max(0) as usize).min(cv.w).min(fb.width as usize);
    let y1 = ((y + h).max(0) as usize).min(cv.h).min(fb.height as usize);
    let base = fb.base as *mut u32;
    for yy in y0..y1 {
        for xx in x0..x1 {
            unsafe { core::ptr::write_volatile(base.add(yy * fb.stride as usize + xx), farbe(fb, cv.px[yy * cv.w + xx])) };
        }
    }
}

/// Mauszeiger direkt auf den Bildschirm (die Leinwand bleibt ohne Zeiger)
const ZEIGER: [u16; 19] = [
    0b1000000000000000, 0b1100000000000000, 0b1110000000000000, 0b1111000000000000, 0b1111100000000000,
    0b1111110000000000, 0b1111111000000000, 0b1111111100000000, 0b1111111110000000, 0b1111111111000000,
    0b1111111111100000, 0b1111111000000000, 0b1110111000000000, 0b1100011100000000, 0b1000011100000000,
    0b0000001110000000, 0b0000001110000000, 0b0000000111000000, 0b0000000000000000,
];

pub(crate) fn zeiger_malen(fb: &Framebuffer, x: i32, y: i32) {
    let base = fb.base as *mut u32;
    let (w, h) = (fb.width as i32, fb.height as i32);
    let gesetzt = |zx: i32, zy: i32| zy >= 0 && (zy as usize) < ZEIGER.len() && (0..16).contains(&zx) && ZEIGER[zy as usize] & (0x8000 >> zx) != 0;
    for zy in -1..20 {
        for zx in -1..17 {
            let innen = gesetzt(zx, zy);
            let rand = !innen && (gesetzt(zx - 1, zy) || gesetzt(zx + 1, zy) || gesetzt(zx, zy - 1) || gesetzt(zx, zy + 1));
            if !(innen || rand) {
                continue;
            }
            let (px, py) = (x + zx, y + zy);
            if px < 0 || py < 0 || px >= w || py >= h {
                continue;
            }
            let c = if innen { 0xFFFFFF } else { 0x000000 };
            unsafe { core::ptr::write_volatile(base.add(py as usize * fb.stride as usize + px as usize), farbe(fb, c)) };
        }
    }
}

pub(crate) fn blit(fb: &Framebuffer, cv: &Canvas) {
    let stride = fb.stride as usize;
    let base = fb.base as *mut u32;
    for y in 0..cv.h.min(fb.height as usize) {
        let row = &cv.px[y * cv.w..(y + 1) * cv.w];
        unsafe {
            let dst = base.add(y * stride);
            if fb.bgr {
                core::ptr::copy_nonoverlapping(row.as_ptr(), dst, cv.w.min(fb.width as usize));
            } else {
                for (x, &v) in row.iter().enumerate().take(fb.width as usize) {
                    core::ptr::write_volatile(dst.add(x), ((v & 0xff) << 16) | (v & 0xff00) | ((v >> 16) & 0xff));
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Platten und GPT

#[derive(Clone)]
struct Teil {
    first: u64,
    last: u64,
    name: String,
    entry: [u8; 128],
}

#[derive(Clone)]
pub(crate) struct Platte {
    pub(crate) idx: usize,
    pub(crate) name: String,
    pub(crate) mb: u64,
    sectors: u64,
    ss: usize,
    teile: Vec<Teil>,
}

/// 1 MiB per copy step
const BLOCK_SEKT: u64 = 2048;

fn rd(idx: usize, lba: u64, n: usize) -> Option<Vec<u8>> {
    let mut b = BLOCK.lock();
    let d = b.get_mut(idx)?;
    let mut buf = vec![0u8; n * d.sector_size()];
    d.read(lba, &mut buf).ok()?;
    Some(buf)
}

fn le64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

pub(crate) fn platten() -> Vec<Platte> {
    let n = BLOCK.lock().len();
    let mut v = Vec::new();
    for idx in 0..n {
        let (name, sectors, ss) = {
            let b = BLOCK.lock();
            let d = &b[idx];
            (d.name(), d.sectors(), d.sector_size())
        };
        let mut teile = Vec::new();
        if ss == 512 {
            if let Some(h) = rd(idx, 1, 1) {
                if &h[0..8] == b"EFI PART" {
                    let lba = le64(&h, 72);
                    let cnt = (le32(&h, 80) as usize).min(128);
                    let esz = le32(&h, 84) as usize;
                    if esz == 128 {
                        if let Some(tb) = rd(idx, lba, (cnt * esz).div_ceil(512)) {
                            for i in 0..cnt {
                                let e = &tb[i * 128..(i + 1) * 128];
                                if e[..16].iter().all(|&x| x == 0) {
                                    continue;
                                }
                                let nm: String = char::decode_utf16(e[56..128].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0))
                                    .map(|r| r.unwrap_or('?'))
                                    .collect();
                                let mut entry = [0u8; 128];
                                entry.copy_from_slice(e);
                                teile.push(Teil { first: le64(e, 32), last: le64(e, 40), name: nm, entry });
                            }
                        }
                    }
                }
            }
        }
        v.push(Platte { idx, name, mb: sectors * ss as u64 >> 20, sectors, ss, teile });
    }
    v
}

fn ist_quelle(p: &Platte) -> bool {
    p.teile.iter().any(|t| t.name == "ULTAR-INSTALL" || t.name == "ULTAR-EFI") && p.teile.iter().any(|t| t.name == "ultar-compat")
}

/// Sectors the target needs for medium `q` (A/B layout or the single-root layout).
fn bedarf(q: &Platte) -> u64 {
    if ab::is_source(q) {
        ab::plan(q).map(|p| ab::needed(&p)).unwrap_or(u64::MAX)
    } else {
        benoetigt(q)
    }
}

/// Startmedium mit Installationspartition? (dann startet die Installation von selbst)
pub fn startmedium_ist_installer() -> bool {
    crate::efi::VOM_INSTALLER.load(core::sync::atomic::Ordering::Relaxed)
}

fn quelle(v: &[Platte]) -> Option<Platte> {
    v.iter().find(|p| p.teile.iter().any(|t| t.name == "ULTAR-INSTALL")).or_else(|| v.iter().find(|p| ist_quelle(p))).cloned()
}

/// Was installiert wird: (Quellpartition, erster Sektor auf dem Ziel). Hat das Medium neben ULTAR-INSTALL eine eigene
/// Startpartition ULTAR-EFI (Desktop-Ausgabe), wird die Installationspartition nicht mitkopiert.
fn plan(q: &Platte) -> Vec<(Teil, u64)> {
    let eigene_efi = q.teile.iter().any(|t| t.name == "ULTAR-EFI");
    let mut v = Vec::new();
    let mut pos = 2048u64;
    for t in &q.teile {
        if eigene_efi && t.name == "ULTAR-INSTALL" {
            continue;
        }
        v.push((t.clone(), pos));
        pos += (t.last - t.first + 1 + 2047) / 2048 * 2048;
    }
    v
}

fn benoetigt(q: &Platte) -> u64 {
    plan(q).iter().map(|(t, z)| z + (t.last - t.first)).max().unwrap_or(0) + 34
}

fn crc32(d: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in d {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

fn zufall16() -> [u8; 16] {
    let mut s = crate::arch::rdtsc() ^ 0x9E37_79B9_7F4A_7C15;
    let mut g = [0u8; 16];
    for b in g.iter_mut() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        *b = s as u8;
    }
    g[7] = (g[7] & 0x0f) | 0x40; // Version 4
    g[8] = (g[8] & 0x3f) | 0x80;
    g
}

fn name_setzen(e: &mut [u8; 128], name: &str) {
    e[56..128].fill(0);
    for (i, u) in name.encode_utf16().take(36).enumerate() {
        e[56 + i * 2..58 + i * 2].copy_from_slice(&u.to_le_bytes());
    }
}

/// GPT (Schutz-MBR, Kopf + Tabelle vorne und hinten) für die Zielplatte
fn gpt_bauen(q: &Platte, ziel_sektoren: u64) -> (Vec<u8>, Vec<u8>, u64) {
    let n = ziel_sektoren;
    let mut tab = vec![0u8; 128 * 128];
    for (i, (t, ziel)) in plan(q).iter().enumerate().take(128) {
        let mut e = t.entry;
        e[16..32].copy_from_slice(&zufall16());
        e[32..40].copy_from_slice(&ziel.to_le_bytes());
        e[40..48].copy_from_slice(&(ziel + (t.last - t.first)).to_le_bytes());
        if t.name == "ULTAR-INSTALL" {
            name_setzen(&mut e, "ULTAR-EFI");
        }
        tab[i * 128..(i + 1) * 128].copy_from_slice(&e);
    }
    let tcrc = crc32(&tab);
    let disk = zufall16();
    let kopf = |cur: u64, alt: u64, tab_lba: u64| -> Vec<u8> {
        let mut h = vec![0u8; 512];
        h[0..8].copy_from_slice(b"EFI PART");
        h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        h[12..16].copy_from_slice(&92u32.to_le_bytes());
        h[24..32].copy_from_slice(&cur.to_le_bytes());
        h[32..40].copy_from_slice(&alt.to_le_bytes());
        h[40..48].copy_from_slice(&34u64.to_le_bytes());
        h[48..56].copy_from_slice(&(n - 34).to_le_bytes());
        h[56..72].copy_from_slice(&disk);
        h[72..80].copy_from_slice(&tab_lba.to_le_bytes());
        h[80..84].copy_from_slice(&128u32.to_le_bytes());
        h[84..88].copy_from_slice(&128u32.to_le_bytes());
        h[88..92].copy_from_slice(&tcrc.to_le_bytes());
        let c = crc32(&h[..92]);
        h[16..20].copy_from_slice(&c.to_le_bytes());
        h
    };
    // vorne: Schutz-MBR (LBA 0), Kopf (LBA 1), Tabelle (LBA 2..33)
    let mut vorne = vec![0u8; 34 * 512];
    let m = &mut vorne[0..512];
    m[446 + 1..446 + 4].copy_from_slice(&[0x00, 0x02, 0x00]);
    m[446 + 4] = 0xEE;
    m[446 + 5..446 + 8].copy_from_slice(&[0xff, 0xff, 0xff]);
    m[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    m[446 + 12..446 + 16].copy_from_slice(&((n - 1).min(0xffff_ffff) as u32).to_le_bytes());
    m[510] = 0x55;
    m[511] = 0xAA;
    vorne[512..1024].copy_from_slice(&kopf(1, n - 1, 2));
    vorne[1024..].copy_from_slice(&tab);
    // hinten: Tabelle (n-33..n-2), Kopf (n-1)
    let mut hinten = vec![0u8; 33 * 512];
    hinten[..128 * 128].copy_from_slice(&tab);
    hinten[32 * 512..].copy_from_slice(&kopf(n - 1, 1, n - 33));
    (vorne, hinten, n - 33)
}

// ------------------------------------------------------------------ Eingabe

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Taste {
    Hoch,
    Runter,
    Links,
    Rechts,
    Tab,
    Enter,
    Esc,
    Zeichen(char),
}

pub(crate) struct Eingabe {
    pub(crate) esc: u8,
}

impl Eingabe {
    pub(crate) fn lesen(&mut self) -> Option<Taste> {
        // serielle Konsole (Fernsteuerung/Tests): Enter, Tab, Pfeile als ESC [ A..D, Ziffern
        if let Some(b) = crate::console::serial_get() {
            match (self.esc, b) {
                (0, 0x1b) => self.esc = 1,
                (1, b'[') => self.esc = 2,
                (2, b'A') => { self.esc = 0; return Some(Taste::Hoch); }
                (2, b'B') => { self.esc = 0; return Some(Taste::Runter); }
                (2, b'C') => { self.esc = 0; return Some(Taste::Rechts); }
                (2, b'D') => { self.esc = 0; return Some(Taste::Links); }
                (1, _) => { self.esc = 0; return Some(Taste::Esc); }
                (_, b'\r') | (_, b'\n') => { self.esc = 0; return Some(Taste::Enter); }
                (_, b'\t') => { self.esc = 0; return Some(Taste::Tab); }
                (_, c) => { self.esc = 0; return Some(Taste::Zeichen(c as char)); }
            }
            return None;
        }
        let c = crate::drivers::ps2kbd::poll()?;
        Some(match c {
            crate::drivers::ps2kbd::HOCH => Taste::Hoch,
            crate::drivers::ps2kbd::RUNTER => Taste::Runter,
            crate::drivers::ps2kbd::LINKS => Taste::Links,
            crate::drivers::ps2kbd::RECHTS => Taste::Rechts,
            '\t' => Taste::Tab,
            '\n' | '\r' => Taste::Enter,
            '\x1b' => Taste::Esc,
            c => Taste::Zeichen(c),
        })
    }
}

// ------------------------------------------------------------------ Ablauf

#[derive(Clone, Copy, PartialEq)]
enum Schritt {
    Sprache,
    Geraete,
    Ziel,
    Bestaetigen,
    Installieren,
    Fertig,
    Fehler,
}

#[derive(Clone, Copy, PartialEq)]
enum Aktion {
    Sprache(Lang),
    Platte(usize),
    Suchen,
    Weiter,
    Zurueck,
    Abbrechen,
    Loeschen,
    Neustart,
    Konsole,
    Verbinden,
    AlleGeraete,
    Seite(i32),
}

struct Knopf {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    a: Aktion,
}

struct Ui {
    fb: Framebuffer,
    cv: Canvas,
    lang: Lang,
    schritt: Schritt,
    knoepfe: Vec<Knopf>,
    fokus: usize,
    quelle: Option<Platte>,
    ziele: Vec<Platte>,
    wahl: Option<usize>,
    fortschritt: (u64, u64),
    phase: String,
    fehler: String,
    /// wo der Mauszeiger zuletzt gemalt wurde
    zeiger: (i32, i32),
    /// Netzwerkzustand (Text) und Internet erreichbar?
    netz: String,
    internet: Option<bool>,
    /// Liste aller Geräte zeigen (Seite)
    alle: Option<usize>,
}

/// Gruppe eines PCI-Geräts (Schlüssel für t())
pub(crate) fn kategorie(class: u8, subclass: u8) -> &'static str {
    match (class, subclass) {
        (0x02, 0x80) | (0x0d, _) => "cat_wlan",
        (0x02, _) => "cat_net",
        (0x01, _) => "cat_disk",
        (0x0c, 0x03) => "cat_usb",
        (0x03, _) => "cat_gpu",
        (0x04, _) => "cat_audio",
        (0x06, _) => "cat_bridge",
        _ => "cat_other",
    }
}

/// Gerätegruppen für die Übersicht: (Schlüssel, Anzahl, mit Treiber, Beschreibung)
pub(crate) fn geraete_gruppen() -> Vec<(&'static str, usize, usize, String)> {
    let mut g: Vec<(&'static str, usize, usize, String)> = Vec::new();
    for b in crate::driver::DEVICES.lock().iter() {
        let d = &b.dev;
        let k = kategorie(d.class, d.subclass);
        let txt = alloc::format!("{:04x}:{:04x}{}", d.vendor, d.device, b.driver.map(|n| alloc::format!(" {n}")).unwrap_or_default());
        match g.iter_mut().find(|e| e.0 == k) {
            Some(e) => {
                e.1 += 1;
                e.2 += b.driver.is_some() as usize;
                if e.3.len() < 60 {
                    e.3.push_str(" · ");
                    e.3.push_str(&txt);
                }
            }
            None => g.push((k, 1, b.driver.is_some() as usize, txt)),
        }
    }
    let ordnung = ["cat_net", "cat_wlan", "cat_disk", "cat_usb", "cat_gpu", "cat_audio", "cat_bridge", "cat_other"];
    g.sort_by_key(|e| ordnung.iter().position(|o| *o == e.0).unwrap_or(9));
    g
}

pub(crate) fn ip_text(a: [u8; 4]) -> String {
    alloc::format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3])
}

pub enum Ende {
    Konsole,
    Neustart,
}

impl Ui {
    fn rtl(&self) -> bool {
        self.lang == Lang::Ar
    }

    fn suchen(&mut self) {
        let alle = platten();
        self.quelle = quelle(&alle);
        let q = self.quelle.as_ref().map(|q| q.idx);
        let need = self.quelle.as_ref().map(bedarf).unwrap_or(u64::MAX);
        self.ziele = alle.into_iter().filter(|p| Some(p.idx) != q && p.ss == 512 && p.sectors >= need).collect();
        self.wahl = None;
    }

    fn knopf(&mut self, x: i32, y: i32, w: i32, h: i32, a: Aktion) {
        self.knoepfe.push(Knopf { x, y, w, h, a });
    }

    /// Schaltfläche zeichnen (primär = Gold) und registrieren
    fn schaltflaeche(&mut self, x: i32, y: i32, w: i32, text: &str, a: Aktion, primaer: bool) {
        let h = 52;
        let fokus = self.knoepfe.len() == self.fokus;
        let cv = &mut self.cv;
        if primaer {
            cv.rrect(x, y, w, h, 12, if fokus { GOLD2 } else { GOLD });
        } else {
            cv.frame(x, y, w, h, 12, if fokus { GOLD2 } else { LINE }, CARD, if fokus { 2 } else { 1 });
        }
        let sc = Canvas::passend(text, 2, w - 24);
        cv.text_c(x + w / 2, y + if sc == 2 { 10 } else { 18 }, text, sc, if primaer { DARK } else { TEXT });
        self.knopf(x, y, w, h, a);
    }

    /// Zeile mit Text links (oder rechts bei Arabisch) innerhalb einer Karte
    fn zeile(&mut self, kx: i32, kw: i32, y: i32, s: &str, scale: i32, col: u32) {
        let scale = Canvas::passend(s, scale, kw - 80);
        let w = Canvas::text_w(s, scale);
        let x = if self.rtl() { kx + kw - 40 - w } else { kx + 40 };
        self.cv.text(x, y, s, scale, col);
    }

    /// zentrierter Text, der in die Karte passt
    fn mitte(&mut self, y: i32, s: &str, scale: i32, col: u32) {
        let w = self.cv.w as i32;
        let kw = (w - 80).min(820);
        let sc = Canvas::passend(s, scale, kw - 60);
        self.cv.text_c(w / 2, y + (scale - sc) * 8, s, sc, col);
    }

    fn zeichnen(&mut self) {
        self.cv.rtl = self.rtl();
        self.knoepfe.clear();
        let (w, h) = (self.cv.w as i32, self.cv.h as i32);
        self.cv.rect(0, 0, w, h, BG);
        // Kopfleiste mit Marke
        self.cv.rect(0, 0, w, 60, BAR);
        self.cv.rect(0, 58, w, 2, GOLD);
        let marke = ultar_brand::product_caps!();
        let unter = ultar_brand::vendor!();
        if self.rtl() {
            let mw = Canvas::text_w(marke, 2);
            self.cv.text(w - 30 - mw, 14, marke, 2, GOLD2);
            self.cv.text(30, 22, unter, 1, MUTED);
        } else {
            self.cv.text(30, 14, marke, 2, GOLD2);
            let uw = Canvas::text_w(unter, 1);
            self.cv.text(w - 30 - uw, 22, unter, 1, MUTED);
        }
        // Schrittanzeige
        let n = match self.schritt {
            Schritt::Sprache => 1,
            Schritt::Geraete => 2,
            Schritt::Ziel => 3,
            Schritt::Bestaetigen => 4,
            Schritt::Installieren => 5,
            Schritt::Fertig | Schritt::Fehler => 6,
        };
        let sk = ["s1", "dev_title", "s2", "s3", "s4", "s5"];
        let stufe = alloc::format!("{} {} {} 6 · {}", t(self.lang, "step"), n, t(self.lang, "of"), t(self.lang, sk[n - 1]));
        let pw = 6 * 46;
        let px0 = (w - pw) / 2;
        for i in 0..6 {
            let i_vis = if self.rtl() { 5 - i } else { i };
            let farbe = if (i as usize) < n { GOLD } else { LINE };
            self.cv.rrect(px0 + i_vis * 46, 84, 38, 6, 3, farbe);
        }
        self.cv.text_c(w / 2, 100, &stufe, 1, MUTED);
        // Karte
        let kw = (w - 80).min(820);
        let kx = (w - kw) / 2;
        let ky = 130;
        let kh = (h - ky - 60).min(560);
        self.cv.frame(kx, ky, kw, kh, 18, LINE, CARD, 1);
        let unten = ky + kh - 52 - 28;
        match self.schritt {
            Schritt::Sprache => {
                self.mitte(ky + 36, t(self.lang, "welcome"), 3, TEXT);
                self.mitte(ky + 100, t(self.lang, "welcome_sub"), 2, MUTED);
                let bw = 360;
                let mut y = ky + 160;
                for (l, s) in [(Lang::Ar, "العربية"), (Lang::De, "Deutsch"), (Lang::En, "English")] {
                    let fokus = self.knoepfe.len() == self.fokus;
                    let aktiv = self.lang == l;
                    let (b, f) = if fokus { (GOLD2, 0x1A1E28) } else if aktiv { (GOLD, CARD) } else { (LINE, CARD) };
                    self.cv.frame(w / 2 - bw / 2, y, bw, 56, 14, b, f, if fokus || aktiv { 2 } else { 1 });
                    self.cv.text_c(w / 2, y + 12, s, 2, if aktiv { GOLD2 } else { TEXT });
                    self.knopf(w / 2 - bw / 2, y, bw, 56, Aktion::Sprache(l));
                    y += 70;
                }
                let bx = if self.rtl() { kx + 28 } else { kx + kw - 28 - 200 };
                self.schaltflaeche(bx, unten, 200, t(self.lang, "next"), Aktion::Weiter, true);
            }
            Schritt::Geraete if self.alle.is_some() => {
                let seite = self.alle.unwrap_or(0);
                let liste: Vec<(String, bool)> = crate::driver::DEVICES
                    .lock()
                    .iter()
                    .map(|b| {
                        let d = &b.dev;
                        (
                            alloc::format!("{}  {:04x}:{:04x}  {}  ·  {}", d.id(), d.vendor, d.device, d.class_name(), b.driver.unwrap_or("–")),
                            b.driver.is_some(),
                        )
                    })
                    .collect();
                let je = 11usize;
                let seiten = liste.len().div_ceil(je).max(1);
                let seite = seite.min(seiten - 1);
                let titel = alloc::format!("{} ({})  ·  {} {}/{}", t(self.lang, "all_dev"), liste.len(), t(self.lang, "page"), seite + 1, seiten);
                self.zeile(kx, kw, ky + 24, &titel, 2, TEXT);
                let mut y = ky + 70;
                let rtl_alt = self.cv.rtl;
                self.cv.rtl = false; // Gerätezeilen sind technisch (links→rechts)
                for (txt, ok) in liste.iter().skip(seite * je).take(je) {
                    self.cv.rrect(kx + 40, y + 7, 9, 9, 4, if *ok { GREEN } else { GOLD });
                    let sc = Canvas::passend(txt, 1, kw - 100);
                    self.cv.text(kx + 60, y, txt, sc, if *ok { TEXT } else { MUTED });
                    y += 28;
                }
                self.cv.rtl = rtl_alt;
                let (lx, mx, rx) = if self.rtl() { (kx + kw - 28 - 200, kx + kw / 2 - 100, kx + 28) } else { (kx + 28, kx + kw / 2 - 100, kx + kw - 28 - 200) };
                self.schaltflaeche(lx, unten, 200, t(self.lang, "back"), Aktion::Seite(-1), false);
                self.schaltflaeche(mx, unten, 200, "‹ ›", Aktion::Seite(1), false);
                self.schaltflaeche(rx, unten, 200, t(self.lang, "next"), Aktion::AlleGeraete, true);
            }
            Schritt::Geraete => {
                self.zeile(kx, kw, ky + 26, t(self.lang, "dev_title"), 3, TEXT);
                self.zeile(kx, kw, ky + 80, t(self.lang, "dev_sub"), 1, MUTED);
                let mut y = ky + 108;
                let gruppen = geraete_gruppen();
                let hat_wlan = gruppen.iter().any(|e| e.0 == "cat_wlan");
                let anz_gesamt: usize = gruppen.iter().map(|e| e.1).sum();
                for (k, anz, mit, txt) in gruppen.into_iter().take(6) {
                    let (farbe, status) = if k == "cat_gpu" && mit == 0 {
                        (GOLD, t(self.lang, "fb"))
                    } else if mit == anz {
                        (GREEN, t(self.lang, "drv_ok"))
                    } else {
                        (GOLD, t(self.lang, "drv_missing"))
                    };
                    let kopf = alloc::format!("{} ({})  ·  {}", t(self.lang, k), anz, status);
                    let rtl = self.rtl();
                    let (pkt_x, txt_x) = if rtl { (kx + kw - 48, kx + 40) } else { (kx + 40, kx + 64) };
                    self.cv.rrect(pkt_x, y + 6, 12, 12, 6, farbe);
                    let kw2 = kw - 110;
                    let sc = Canvas::passend(&kopf, 2, kw2);
                    let tw = Canvas::text_w(&kopf, sc);
                    let dw = Canvas::text_w(&txt, 1);
                    if rtl {
                        self.cv.text(kx + kw - 64 - tw, y, &kopf, sc, TEXT);
                        self.cv.text(kx + kw - 64 - dw.min(kw2), y + 32, &txt, 1, MUTED);
                    } else {
                        self.cv.text(txt_x, y, &kopf, sc, TEXT);
                        self.cv.text(txt_x, y + 32, &txt, 1, MUTED);
                    }
                    y += 52;
                }
                // Netzwerkzustand
                let farbe = match self.internet { Some(true) => GREEN, Some(false) => GOLD, None => MUTED };
                let nz = self.netz.clone();
                self.cv.rrect(kx + 30, y + 4, kw - 60, 40, 10, 0x171B24);
                self.zeile(kx, kw, y + 14, &nz, 1, farbe);
                y += 50;
                if hat_wlan {
                    self.zeile(kx, kw, y, t(self.lang, "wlan_hint"), 1, MUTED);
                    y += 26;
                }
                let _ = y;
                let (lx, mx, rx) = if self.rtl() { (kx + kw - 28 - 200, kx + kw / 2 - 120, kx + 28) } else { (kx + 28, kx + kw / 2 - 120, kx + kw - 28 - 200) };
                self.schaltflaeche(lx, unten, 200, t(self.lang, "back"), Aktion::Zurueck, false);
                self.schaltflaeche(mx, unten, 240, t(self.lang, "connect"), Aktion::Verbinden, false);
                self.schaltflaeche(rx, unten, 200, t(self.lang, "next"), Aktion::Weiter, true);
                let alle_txt = alloc::format!("{} ({})", t(self.lang, "all_dev"), anz_gesamt);
                let aw = (Canvas::text_w(&alle_txt, 1) + 40).max(160);
                let ax = if self.rtl() { kx + 28 } else { kx + kw - 28 - aw };
                self.schaltflaeche(ax, ky + 20, aw, &alle_txt, Aktion::AlleGeraete, false);
            }
            Schritt::Ziel => {
                self.zeile(kx, kw, ky + 30, t(self.lang, "disk_title"), 3, TEXT);
                self.zeile(kx, kw, ky + 90, t(self.lang, "disk_sub"), 2, MUTED);
                let mut y = ky + 140;
                if let Some(q) = self.quelle.clone() {
                    let s = alloc::format!("{}: {} · {} MB", t(self.lang, "source"), q.name, q.mb);
                    self.zeile(kx, kw, y, &s, 1, MUTED);
                }
                y += 28;
                if self.ziele.is_empty() {
                    self.zeile(kx, kw, y + 10, t(self.lang, "no_disk"), 2, GOLD2);
                }
                for i in 0..self.ziele.len().min(4) {
                    let p = self.ziele[i].clone();
                    let fokus = self.knoepfe.len() == self.fokus;
                    let gew = self.wahl == Some(i);
                    let (b, f) = if fokus { (GOLD2, 0x1A1E28) } else if gew { (GOLD, CARD) } else { (LINE, CARD) };
                    self.cv.frame(kx + 30, y, kw - 60, 64, 14, b, f, if fokus || gew { 2 } else { 1 });
                    let alt = if p.teile.is_empty() {
                        String::from("–")
                    } else {
                        p.teile.iter().map(|t| if t.name.is_empty() { String::from("?") } else { t.name.clone() }).collect::<Vec<_>>().join(", ")
                    };
                    let titel = alloc::format!("{}  ·  {} MB", p.name, p.mb);
                    let tw = Canvas::text_w(&titel, 2);
                    let aw = Canvas::text_w(&alt, 1);
                    if self.rtl() {
                        self.cv.text(kx + kw - 56 - tw, y + 8, &titel, 2, if gew { GOLD2 } else { TEXT });
                        self.cv.text(kx + kw - 56 - aw, y + 42, &alt, 1, MUTED);
                    } else {
                        self.cv.text(kx + 56, y + 8, &titel, 2, if gew { GOLD2 } else { TEXT });
                        self.cv.text(kx + 56, y + 42, &alt, 1, MUTED);
                    }
                    self.knopf(kx + 30, y, kw - 60, 64, Aktion::Platte(i));
                    y += 76;
                }
                let (lx, rx) = if self.rtl() { (kx + kw - 28 - 200, kx + 28) } else { (kx + 28, kx + kw - 28 - 200) };
                self.schaltflaeche(lx, unten, 200, t(self.lang, "back"), Aktion::Zurueck, false);
                if self.ziele.is_empty() {
                    self.schaltflaeche(rx, unten, 200, t(self.lang, "search"), Aktion::Suchen, true);
                } else {
                    self.schaltflaeche(rx, unten, 200, t(self.lang, "next"), Aktion::Weiter, true);
                }
            }
            Schritt::Bestaetigen => {
                self.zeile(kx, kw, ky + 30, t(self.lang, "confirm_title"), 3, TEXT);
                self.cv.rrect(kx + 30, ky + 100, kw - 60, 150, 14, 0x2A1416);
                self.zeile(kx, kw, ky + 120, t(self.lang, "confirm_warn"), 2, RED);
                if let Some(p) = self.wahl.and_then(|i| self.ziele.get(i)).cloned() {
                    let s = alloc::format!("{}  ·  {} MB", p.name, p.mb);
                    self.zeile(kx, kw, ky + 170, &s, 2, TEXT);
                }
                if let Some(q) = self.quelle.clone() {
                    let s = alloc::format!("{}: {} · {}", t(self.lang, "source"), q.name, q.teile.iter().map(|t| t.name.clone()).collect::<Vec<_>>().join(" + "));
                    self.zeile(kx, kw, ky + 275, &s, 1, MUTED);
                }
                let bw = (Canvas::text_w(t(self.lang, "confirm_btn"), 2) + 56).clamp(240, kw - 290);
                let (lx, rx) = if self.rtl() { (kx + kw - 28 - 200, kx + 28) } else { (kx + 28, kx + kw - 28 - bw) };
                self.schaltflaeche(lx, unten, 200, t(self.lang, "back"), Aktion::Zurueck, false);
                self.schaltflaeche(rx, unten, bw, t(self.lang, "confirm_btn"), Aktion::Loeschen, true);
            }
            Schritt::Installieren => {
                self.mitte(ky + 40, t(self.lang, "inst_title"), 3, TEXT);
                self.mitte(ky + 100, t(self.lang, "inst_wait"), 2, MUTED);
                let (a, b) = self.fortschritt;
                let pct = if b == 0 { 0 } else { (a * 100 / b) as i32 };
                let bw = kw - 120;
                self.cv.rrect(kx + 60, ky + 180, bw, 22, 11, LINE);
                let fw = (bw * pct / 100).max(22);
                if self.rtl() {
                    self.cv.rrect(kx + 60 + bw - fw, ky + 180, fw, 22, 11, GOLD);
                } else {
                    self.cv.rrect(kx + 60, ky + 180, fw, 22, 11, GOLD);
                }
                self.cv.text_c(w / 2, ky + 220, &alloc::format!("{pct} %"), 2, GOLD2);
                let ph = self.phase.clone();
                self.mitte(ky + 270, &ph, 2, TEXT);
            }
            Schritt::Fertig => {
                self.cv.rrect(w / 2 - 40, ky + 40, 80, 80, 40, GREEN);
                self.cv.text_c(w / 2, ky + 56, "OK", 3, 0xFFFFFF);
                self.mitte(ky + 150, t(self.lang, "done_title"), 3, TEXT);
                self.mitte(ky + 210, t(self.lang, "verified"), 2, MUTED);
                self.mitte(ky + 250, t(self.lang, "done_text"), 2, GOLD2);
                let (lx, rx) = if self.rtl() { (kx + kw - 28 - 240, kx + 28) } else { (kx + 28, kx + kw - 28 - 240) };
                self.schaltflaeche(lx, unten, 240, t(self.lang, "console"), Aktion::Konsole, false);
                self.schaltflaeche(rx, unten, 240, t(self.lang, "restart"), Aktion::Neustart, true);
            }
            Schritt::Fehler => {
                self.cv.rrect(w / 2 - 40, ky + 40, 80, 80, 40, RED);
                self.cv.text_c(w / 2, ky + 56, "!", 3, 0xFFFFFF);
                self.mitte(ky + 150, t(self.lang, "err_title"), 3, TEXT);
                let f = self.fehler.clone();
                self.mitte(ky + 210, &f, 1, GOLD2);
                let (lx, rx) = if self.rtl() { (kx + kw - 28 - 240, kx + 28) } else { (kx + 28, kx + kw - 28 - 240) };
                self.schaltflaeche(lx, unten, 240, t(self.lang, "console"), Aktion::Konsole, false);
                self.schaltflaeche(rx, unten, 240, t(self.lang, "back"), Aktion::Zurueck, true);
            }
        }
        // Hinweis unten
        if self.schritt != Schritt::Installieren {
            self.cv.text_c(w / 2, h - 34, t(self.lang, "hint"), 1, MUTED);
        }
        if self.fokus >= self.knoepfe.len() && !self.knoepfe.is_empty() {
            self.fokus = self.knoepfe.len() - 1;
        }
        blit(&self.fb, &self.cv);
        if crate::drivers::ps2maus::aktiv() {
            let (mx, my, _, _) = crate::drivers::ps2maus::zustand();
            zeiger_malen(&self.fb, mx, my);
            self.zeiger = (mx, my);
        }
    }

    fn ausfuehren(&mut self, a: Aktion) -> Option<Ende> {
        match a {
            Aktion::Sprache(l) => {
                self.lang = l;
                crate::sprache::set(match l {
                    Lang::Ar => crate::sprache::AR,
                    Lang::De => crate::sprache::DE,
                    Lang::En => crate::sprache::EN,
                });
            }
            Aktion::Verbinden => self.netz_pruefen(true),
            Aktion::AlleGeraete => {
                self.alle = if self.alle.is_some() { None } else { Some(0) };
                self.fokus = 0;
            }
            Aktion::Seite(d) => {
                let n = crate::driver::DEVICES.lock().len().div_ceil(11).max(1) as i32;
                let s = self.alle.unwrap_or(0) as i32;
                if d < 0 && s == 0 {
                    self.alle = None;
                } else {
                    self.alle = Some(((s + d).rem_euclid(n)) as usize);
                }
            }
            Aktion::Platte(i) => {
                self.wahl = Some(i);
            }
            Aktion::Suchen => self.suchen(),
            Aktion::Weiter => match self.schritt {
                Schritt::Sprache => {
                    // Sprache dauerhaft speichern – wird mit installiert
                    crate::sprache::set(match self.lang {
                        Lang::Ar => crate::sprache::AR,
                        Lang::De => crate::sprache::DE,
                        Lang::En => crate::sprache::EN,
                    });
                    crate::sprache::speichern();
                    self.netz_pruefen(false);
                    self.schritt = Schritt::Geraete;
                    self.fokus = 2;
                }
                Schritt::Geraete => {
                    self.suchen();
                    self.schritt = Schritt::Ziel;
                    self.fokus = 0;
                }
                Schritt::Ziel => {
                    if self.wahl.is_none() && self.ziele.len() == 1 {
                        self.wahl = Some(0);
                    }
                    if self.wahl.is_some() {
                        self.schritt = Schritt::Bestaetigen;
                        self.fokus = 0;
                    }
                }
                _ => {}
            },
            Aktion::Zurueck => {
                self.schritt = match self.schritt {
                    Schritt::Geraete => Schritt::Sprache,
                    Schritt::Ziel => Schritt::Geraete,
                    Schritt::Bestaetigen | Schritt::Fehler => {
                        self.suchen();
                        Schritt::Ziel
                    }
                    s => s,
                };
                self.fokus = 0;
            }
            Aktion::Abbrechen | Aktion::Konsole => return Some(Ende::Konsole),
            Aktion::Neustart => return Some(Ende::Neustart),
            Aktion::Loeschen => {
                self.schritt = Schritt::Installieren;
                self.zeichnen();
                match self.installieren() {
                    Ok(()) => self.schritt = Schritt::Fertig,
                    Err(e) => {
                        crate::kprintln!("[installation] Fehler: {e}");
                        self.fehler = e;
                        self.schritt = Schritt::Fehler;
                    }
                }
                self.fokus = 1;
            }
        }
        None
    }

    fn phase_setzen(&mut self, s: String) {
        self.phase = s;
        self.zeichnen();
    }

    /// Partitionen kopieren, prüfen, GPT schreiben
    fn installieren(&mut self) -> Result<(), String> {
        let q = self.quelle.clone().ok_or("kein Installationsmedium gefunden")?;
        let z = self.wahl.and_then(|i| self.ziele.get(i)).cloned().ok_or("keine Zielplatte gewählt")?;
        if z.idx == q.idx {
            return Err(String::from("Zielplatte ist das Installationsmedium"));
        }
        if ab::is_source(&q) {
            return self.installieren_ab(q, z);
        }
        if z.sectors < benoetigt(&q) {
            return Err(alloc::format!("Zielplatte zu klein ({} MB)", z.mb));
        }
        crate::kprintln!("[installation] {} -> {} ({} MB)", q.name, z.name, z.mb);
        let auftrag = plan(&q);
        let gesamt: u64 = auftrag.iter().map(|(t, _)| t.last - t.first + 1).sum::<u64>() * 2 + 1;
        let mut erledigt = 0u64;
        // alte GPT vorne zerstören, damit eine abgebrochene Installation nicht als gültig gilt
        {
            let mut b = BLOCK.lock();
            b[z.idx].write(0, &vec![0u8; 34 * 512]).map_err(|e| alloc::format!("{}: {e}", z.name))?;
        }
        for (teil, ziel_first) in &auftrag {
            let versatz = *ziel_first as i64 - teil.first as i64;
            let anzeige = if teil.name == "ULTAR-INSTALL" { String::from("ULTAR-EFI") } else { teil.name.clone() };
            for pruefen in [false, true] {
                let label = alloc::format!("{} {}", t(self.lang, if pruefen { "ph_verify" } else { "ph_copy" }), anzeige);
                self.phase_setzen(label);
                let mut lba = teil.first;
                let mut zuletzt = crate::arch::uptime_ms();
                while lba <= teil.last {
                    let n = (teil.last - lba + 1).min(BLOCK_SEKT);
                    let mut buf = vec![0u8; n as usize * 512];
                    {
                        let mut b = BLOCK.lock();
                        b[q.idx].read(lba, &mut buf).map_err(|e| alloc::format!("{} lesen: {e}", q.name))?;
                        if pruefen {
                            let mut neu = vec![0u8; buf.len()];
                            b[z.idx].read((lba as i64 + versatz) as u64, &mut neu).map_err(|e| alloc::format!("{} lesen: {e}", z.name))?;
                            if neu != buf {
                                return Err(alloc::format!("Prüfung fehlgeschlagen: {anzeige}, Sektor {lba}"));
                            }
                        } else {
                            b[z.idx].write((lba as i64 + versatz) as u64, &buf).map_err(|e| alloc::format!("{} schreiben: {e}", z.name))?;
                        }
                    }
                    lba += n;
                    erledigt += n;
                    let jetzt = crate::arch::uptime_ms();
                    if jetzt.wrapping_sub(zuletzt) >= 120 {
                        zuletzt = jetzt;
                        self.fortschritt = (erledigt, gesamt);
                        self.zeichnen();
                    }
                }
            }
        }
        self.phase_setzen(String::from(t(self.lang, "ph_gpt")));
        let (vorne, hinten, hinten_lba) = gpt_bauen(&q, z.sectors);
        {
            let mut b = BLOCK.lock();
            b[z.idx].write(hinten_lba, &hinten).map_err(|e| alloc::format!("GPT hinten: {e}"))?;
            b[z.idx].write(0, &vorne).map_err(|e| alloc::format!("GPT vorne: {e}"))?;
        }
        // GPT zurücklesen und prüfen
        let h = rd(z.idx, 1, 1).ok_or("GPT lesen fehlgeschlagen")?;
        if &h[0..8] != b"EFI PART" {
            return Err(String::from("GPT nach dem Schreiben ungültig"));
        }
        self.fortschritt = (gesamt, gesamt);
        crate::kprintln!("[installation] fertig: {} Partition(en) kopiert und geprüft, GPT geschrieben", auftrag.len());
        Ok(())
    }

    /// Netz prüfen: Adresse (DHCP, falls nötig) und Internet (Ping 1.1.1.1)
    fn netz_pruefen(&mut self, verbinden: bool) {
        if crate::driver::NET.lock().is_empty() {
            self.netz = String::from(t(self.lang, "net_dev_no"));
            self.internet = Some(false);
            return;
        }
        if verbinden || crate::net::CONFIG.lock().is_none() {
            self.netz = String::from("…");
            self.zeichnen();
            crate::shell::dhcp();
        }
        let cfg = *crate::net::CONFIG.lock();
        match cfg {
            Some(c) => {
                let nic = crate::net::active_nic();
                let ping = crate::net::ping(nic, [1, 1, 1, 1]).or_else(|| crate::net::ping(nic, c.gw));
                self.internet = Some(ping.is_some());
                self.netz = alloc::format!(
                    "IP {}  ·  Router {}  ·  {}{}",
                    ip_text(c.ip),
                    ip_text(c.gw),
                    t(self.lang, if ping.is_some() { "inet_ok" } else { "inet_no" }),
                    ping.map(|ms| alloc::format!(" ({ms} ms)")).unwrap_or_default()
                );
            }
            None => {
                self.netz = String::from(t(self.lang, "net_no"));
                self.internet = Some(false);
            }
        }
    }

    fn treffer(&self, x: i32, y: i32) -> Option<usize> {
        self.knoepfe.iter().position(|k| x >= k.x && x < k.x + k.w && y >= k.y && y < k.y + k.h)
    }
}

/// Installation anzeigen, bis der Nutzer sie verlässt
pub fn run(rt: *mut crate::efi::RuntimeServices) {
    let Some(fb) = crate::console::framebuffer() else {
        crate::kprintln!("[installation] kein Bildschirm – Installation braucht eine Grafikausgabe");
        return;
    };
    let (w, h) = (fb.width as usize, fb.height as usize);
    crate::drivers::ps2maus::init(w, h);
    crate::console::grafik(true);
    crate::kprintln!("[installation] gestartet ({}x{}, bgr={}, stride={})", w, h, fb.bgr, fb.stride);
    let mut ui = Ui {
        fb,
        cv: Canvas { w, h, px: vec![BG; w * h], rtl: true },
        // gespeicherte Systemsprache übernehmen (ohne Einstellung: Arabisch zuerst)
        lang: if crate::fs::read_all("/etc/ultar/sprache").is_some() {
            match crate::sprache::get() {
                crate::sprache::DE => Lang::De,
                crate::sprache::EN => Lang::En,
                _ => Lang::Ar,
            }
        } else {
            Lang::Ar
        },
        schritt: Schritt::Sprache,
        knoepfe: Vec::new(),
        fokus: 0,
        quelle: None,
        ziele: Vec::new(),
        wahl: None,
        fortschritt: (0, 0),
        phase: String::new(),
        fehler: String::new(),
        zeiger: (0, 0),
        netz: String::new(),
        internet: None,
        alle: None,
    };
    ui.zeichnen();
    let mut ein = Eingabe { esc: 0 };
    let mut letzte_maus = crate::drivers::ps2maus::zustand();
    let mut taste_war = false;
    let ende = loop {
        let mut neu = false;
        let mut aktion = None;
        if let Some(k) = ein.lesen() {
            neu = true;
            let n = ui.knoepfe.len().max(1);
            match k {
                Taste::Tab | Taste::Runter => ui.fokus = (ui.fokus + 1) % n,
                Taste::Hoch => ui.fokus = (ui.fokus + n - 1) % n,
                Taste::Rechts => ui.fokus = if ui.rtl() { (ui.fokus + n - 1) % n } else { (ui.fokus + 1) % n },
                Taste::Links => ui.fokus = if ui.rtl() { (ui.fokus + 1) % n } else { (ui.fokus + n - 1) % n },
                Taste::Enter => aktion = ui.knoepfe.get(ui.fokus).map(|k| k.a),
                Taste::Esc => {
                    if ui.schritt == Schritt::Sprache {
                        aktion = Some(Aktion::Abbrechen);
                    } else if ui.schritt != Schritt::Installieren {
                        aktion = Some(Aktion::Zurueck);
                    }
                }
                Taste::Zeichen(c) => {
                    // Ziffern wählen das n-te Element (Tests über die serielle Konsole)
                    if let Some(d) = c.to_digit(10) {
                        if d >= 1 && (d as usize) <= ui.knoepfe.len() {
                            ui.fokus = d as usize - 1;
                            aktion = ui.knoepfe.get(ui.fokus).map(|k| k.a);
                        }
                    }
                }
            }
        }
        crate::drivers::ps2maus::abfragen();
        let m = crate::drivers::ps2maus::zustand();
        if m.3 != letzte_maus.3 {
            // nur der Zeiger hat sich bewegt: alten Platz wiederherstellen, neu malen – kein ganzes Bild
            let fokus_neu = ui.treffer(m.0, m.1).filter(|&i| i != ui.fokus);
            if let Some(i) = fokus_neu {
                ui.fokus = i;
                neu = true;
            } else if (m.0, m.1) != ui.zeiger {
                let (ox, oy) = ui.zeiger;
                blit_rect(&ui.fb, &ui.cv, ox - 2, oy - 2, 20, 23);
                zeiger_malen(&ui.fb, m.0, m.1);
                ui.zeiger = (m.0, m.1);
            }
            // Klick beim Loslassen der linken Taste
            if taste_war && !m.2 {
                if let Some(i) = ui.treffer(m.0, m.1) {
                    aktion = ui.knoepfe.get(i).map(|k| k.a);
                }
            }
            taste_war = m.2;
            letzte_maus = m;
        }
        if let Some(a) = aktion {
            if let Some(e) = ui.ausfuehren(a) {
                break e;
            }
            neu = true;
        }
        if neu {
            ui.zeichnen();
        } else {
            for _ in 0..64 {
                crate::arch::pause();
            }
        }
    };
    crate::console::grafik(false);
    match ende {
        Ende::Neustart => {
            let _ = rt;
            crate::power::neustart()
        }
        Ende::Konsole => crate::kprintln!("[installation] beendet"),
    }
}
