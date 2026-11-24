//! Oberfläche des installierten Ultar OS auf dem Ultar-Kern: ruhiges Startbild beim Hochfahren und danach die
//! Startseite – alles in der Systemsprache (Arabisch von rechts nach links). Das Startprotokoll läuft weiter über die
//! serielle Schnittstelle und `protokoll`; die Konsole ist eine Kachel („Terminal“), `oberflaeche` führt zurück.

use crate::install::{
    self as gui, Canvas, Eingabe, Lang, Taste, BAR, BG, CARD, DARK, GOLD, GOLD2, GREEN, LINE,
    MUTED, RED, TEXT,
};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

fn lang() -> Lang {
    match crate::sprache::get() {
        crate::sprache::AR => Lang::Ar,
        crate::sprache::EN => Lang::En,
        _ => Lang::De,
    }
}

fn s(k: &str) -> &'static str {
    use crate::sprache::{AR, EN};
    match (k, crate::sprache::get()) {
        ("starte", AR) => concat!("جارٍ تشغيل ", ultar_brand::product!()),
        ("starte", EN) => concat!("Starting ", ultar_brand::product!()),
        ("starte", _) => concat!(ultar_brand::product!(), " startet"),
        ("p_kern", AR) => "تهيئة النواة",
        ("p_kern", EN) => "Preparing the kernel",
        ("p_kern", _) => "Kern wird vorbereitet",
        ("p_hw", AR) => "التعرف على الأجهزة",
        ("p_hw", EN) => "Detecting devices",
        ("p_hw", _) => "Geräte werden erkannt",
        ("p_net", AR) => "الاتصال بالشبكة",
        ("p_net", EN) => "Connecting to the network",
        ("p_net", _) => "Netzwerk wird verbunden",
        ("p_disk", AR) => "تجهيز الأقراص",
        ("p_disk", EN) => "Mounting drives",
        ("p_disk", _) => "Laufwerke werden eingebunden",
        ("p_fertig", AR) => "جاهز",
        ("p_fertig", EN) => "Ready",
        ("p_fertig", _) => "Bereit",
        ("unter", _) => crate::sprache::marke("brand.boot.subline"),
        ("terminal", AR) => "الطرفية",
        ("terminal", _) => "Terminal",
        ("geraete", AR) => "الأجهزة",
        ("geraete", EN) => "Devices",
        ("geraete", _) => "Geräte",
        ("netz", AR) => "الشبكة",
        ("netz", EN) => "Network",
        ("netz", _) => "Netzwerk",
        ("sprache", AR) => "اللغة: العربية",
        ("sprache", EN) => "Language: English",
        ("sprache", _) => "Sprache: Deutsch",
        ("neustart", AR) => "إعادة التشغيل",
        ("neustart", EN) => "Restart",
        ("neustart", _) => "Neu starten",
        ("aus", AR) => "إيقاف التشغيل",
        ("aus", EN) => "Shut down",
        ("aus", _) => "Ausschalten",
        ("zurueck", AR) => "رجوع",
        ("zurueck", EN) => "Back",
        ("zurueck", _) => "Zurück",
        ("weiter", AR) => "التالي",
        ("weiter", EN) => "Next",
        ("weiter", _) => "Weiter",
        ("prozessor", AR) => "المعالج",
        ("prozessor", EN) => "Processor",
        ("prozessor", _) => "Prozessor",
        ("speicher", AR) => "الذاكرة",
        ("speicher", EN) => "Memory",
        ("speicher", _) => "Arbeitsspeicher",
        ("kerne", AR) => "أنوية",
        ("kerne", EN) => "cores",
        ("kerne", _) => "Kerne",
        ("laufwerke", AR) => "الأقراص",
        ("laufwerke", EN) => "Drives",
        ("laufwerke", _) => "Laufwerke",
        ("frei", AR) => "متاح",
        ("frei", EN) => "free",
        ("frei", _) => "frei",
        ("mit_treiber", AR) => "بتعريف",
        ("mit_treiber", EN) => "with driver",
        ("mit_treiber", _) => "mit Treiber",
        ("seite", AR) => "صفحة",
        ("seite", EN) => "page",
        ("seite", _) => "Seite",
        ("wlan", AR) => "بطاقة WLAN موجودة – تعريفها قيد التطوير، استخدم الكابل حالياً",
        ("wlan", EN) => "Wi-Fi card found – driver in progress, please use a cable for now",
        ("wlan", _) => "WLAN-Karte erkannt – Treiber in Arbeit, vorerst bitte Kabel nutzen",
        ("term_hinweis", AR) => "اكتب «واجهة» للعودة إلى الواجهة و«مساعدة» لقائمة الأوامر",
        ("term_hinweis", EN) => "type `ui` to return to the interface, `help` for commands",
        ("term_hinweis", _) => "`oberflaeche` = zurück zur Oberfläche, `hilfe` = Befehle",
        (_, _) => "",
    }
}

fn leinwand() -> Option<(crate::efi::Framebuffer, Canvas)> {
    let fb = crate::console::framebuffer()?;
    let (w, h) = (fb.width as usize, fb.height as usize);
    Some((
        fb,
        Canvas {
            w,
            h,
            px: vec![BG; w * h],
            rtl: lang() == Lang::Ar,
        },
    ))
}

/// Kopfleiste mit Marke (wie in der Installation)
fn kopf(cv: &mut Canvas) {
    let w = cv.w as i32;
    cv.rect(0, 0, w, 60, BAR);
    cv.rect(0, 58, w, 2, GOLD);
    let (marke, unter) = (ultar_brand::product_caps!(), ultar_brand::vendor!());
    if cv.rtl {
        let mw = Canvas::text_w(marke, 2);
        cv.text(w - 30 - mw, 14, marke, 2, GOLD2);
        cv.text(30, 22, unter, 1, MUTED);
    } else {
        cv.text(30, 14, marke, 2, GOLD2);
        let uw = Canvas::text_w(unter, 1);
        cv.text(w - 30 - uw, 22, unter, 1, MUTED);
    }
}

// ------------------------------------------------------------------ Startbild

/// Startbild mit Fortschritt (0–100) und Schritt – nur auf dem installierten System (Sprache fest)
pub fn startbild(prozent: u32, schritt: &str) {
    if !crate::sprache::FEST.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Some((fb, mut cv)) = leinwand() else {
        return;
    };
    let (w, h) = (cv.w as i32, cv.h as i32);
    let my = h / 2 - 90;
    cv.text_c(w / 2, my, ultar_brand::product_caps!(), 6, GOLD2);
    cv.text_c(w / 2, my + 96, s("unter"), 1, MUTED);
    let bw = 360.min(w - 80);
    let bx = (w - bw) / 2;
    let by = my + 150;
    cv.rrect(bx, by, bw, 8, 4, LINE);
    let fw = (bw * prozent.min(100) as i32 / 100).max(8);
    // Fortschritt wächst bei Arabisch von rechts
    let fx = if cv.rtl { bx + bw - fw } else { bx };
    cv.rrect(fx, by, fw, 8, 4, GOLD);
    let txt = alloc::format!("{} …", s(schritt));
    cv.text_c(w / 2, by + 26, &txt, 1, TEXT);
    gui::blit(&fb, &cv);
}

// ------------------------------------------------------------------ Startseite

#[derive(Clone, Copy, PartialEq)]
enum Aktion {
    Terminal,
    Geraete,
    Netz,
    Sprache,
    Neustart,
    Aus,
    Zurueck,
    Seite(i32),
}

struct Knopf {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    a: Aktion,
}

struct Seite {
    fb: crate::efi::Framebuffer,
    cv: Canvas,
    knoepfe: Vec<Knopf>,
    fokus: usize,
    /// Geräteliste offen (Seite)
    geraete: Option<usize>,
    netz: String,
    internet: Option<bool>,
    zeiger: (i32, i32),
}

impl Seite {
    fn rtl(&self) -> bool {
        lang() == Lang::Ar
    }

    fn netz_pruefen(&mut self, verbinden: bool) {
        let l = lang();
        if crate::driver::NET.lock().is_empty() {
            self.netz = String::from(gui::t(l, "net_dev_no"));
            self.internet = Some(false);
            return;
        }
        if verbinden || crate::net::CONFIG.lock().is_none() {
            crate::shell::dhcp();
        }
        let cfg = *crate::net::CONFIG.lock();
        match cfg {
            Some(c) => {
                let nic = crate::net::active_nic();
                let ping =
                    crate::net::ping(nic, [1, 1, 1, 1]).or_else(|| crate::net::ping(nic, c.gw));
                self.internet = Some(ping.is_some());
                self.netz = alloc::format!(
                    "IP {}  ·  {}{}",
                    gui::ip_text(c.ip),
                    gui::t(l, if ping.is_some() { "inet_ok" } else { "inet_no" }),
                    ping.map(|ms| alloc::format!(" ({ms} ms)"))
                        .unwrap_or_default()
                );
            }
            None => {
                self.netz = String::from(gui::t(l, "net_no"));
                self.internet = Some(false);
            }
        }
    }

    /// Kachel: gerahmte Fläche mit Titel und Untertitel (primär = Gold)
    fn kachel(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        titel: &str,
        unter: &str,
        a: Aktion,
        primaer: bool,
    ) {
        let fokus = self.knoepfe.len() == self.fokus;
        if primaer {
            self.cv
                .rrect(x, y, w, h, 16, if fokus { GOLD2 } else { GOLD });
        } else {
            self.cv.frame(
                x,
                y,
                w,
                h,
                16,
                if fokus { GOLD2 } else { LINE },
                CARD,
                if fokus { 2 } else { 1 },
            );
        }
        let (c1, c2) = if primaer {
            (DARK, 0x3A3010)
        } else {
            (TEXT, MUTED)
        };
        let sc = Canvas::passend(titel, 2, w - 28);
        let ty = if unter.is_empty() {
            y + h / 2 - 14
        } else {
            y + h / 2 - 26
        };
        self.cv.text_c(x + w / 2, ty, titel, sc, c1);
        if !unter.is_empty() {
            let sc2 = Canvas::passend(unter, 1, w - 24);
            self.cv.text_c(x + w / 2, ty + 36, unter, sc2, c2);
        }
        self.knoepfe.push(Knopf { x, y, w, h, a });
    }

    /// Zeile in einer Karte, bei Arabisch rechtsbündig
    fn zeile(&mut self, kx: i32, kw: i32, y: i32, t: &str, scale: i32, col: u32) {
        let sc = Canvas::passend(t, scale, kw - 64);
        let tw = Canvas::text_w(t, sc);
        let x = if self.rtl() {
            kx + kw - 32 - tw
        } else {
            kx + 32
        };
        self.cv.text(x, y, t, sc, col);
    }

    fn zeichnen(&mut self) {
        self.cv.rtl = self.rtl();
        self.knoepfe.clear();
        let (w, h) = (self.cv.w as i32, self.cv.h as i32);
        self.cv.rect(0, 0, w, h, BG);
        kopf(&mut self.cv);
        let l = lang();
        let kw = (w - 60).min(980);
        let kx = (w - kw) / 2;
        if let Some(seite) = self.geraete {
            self.zeichne_geraete(kx, kw, seite);
            return;
        }
        // Begrüßung und Zustand
        let ky = 84;
        self.cv.frame(kx, ky, kw, 214, 18, LINE, CARD, 1);
        let gruss = gui::t(l, "welcome");
        let sc = Canvas::passend(gruss, 3, kw - 64);
        self.zeile(kx, kw, ky + 22, gruss, sc, TEXT);
        let mut nb = [0u8; 48];
        let (total, _, _, _) = crate::mem::stats();
        let (on, _) = crate::apic::cpu_count();
        let cpu = alloc::format!(
            "{}: {} · {on} {}",
            s("prozessor"),
            crate::arch::cpu_name(&mut nb).trim(),
            s("kerne")
        );
        self.zeile(kx, kw, ky + 86, &cpu, 1, MUTED);
        let mut lw = String::new();
        for p in gui::platten() {
            if !lw.is_empty() {
                lw.push_str("  ·  ");
            }
            lw.push_str(&alloc::format!("{} {} GB", p.name, (p.mb + 512) / 1024));
        }
        let mem = alloc::format!(
            "{}: {} MB {}  ·  {}: {}",
            s("speicher"),
            total >> 20,
            s("frei"),
            s("laufwerke"),
            lw
        );
        self.zeile(kx, kw, ky + 116, &mem, 1, MUTED);
        let netz = alloc::format!("{}: {}", s("netz"), self.netz);
        let farbe = match self.internet {
            Some(true) => GREEN,
            Some(false) => RED,
            None => MUTED,
        };
        let netz_c = netz.clone();
        self.zeile(kx, kw, ky + 146, &netz_c, 1, farbe);
        let wlan = gui::geraete_gruppen()
            .iter()
            .any(|g| g.0 == "cat_wlan" && g.2 < g.1);
        if wlan {
            self.zeile(kx, kw, ky + 176, s("wlan"), 1, GOLD);
        }
        // Kacheln 3×2 (bei Arabisch von rechts)
        let ty = ky + 214 + 24;
        let gap = 18;
        let spalten = if kw >= 640 { 3 } else { 2 };
        let tw = (kw - gap * (spalten - 1)) / spalten;
        let th = ((h - ty - 30 - gap) / 2).clamp(80, 130);
        let ip = self.netz.clone();
        let kacheln: [(&str, String, Aktion, bool); 6] = [
            (s("terminal"), String::new(), Aktion::Terminal, true),
            (
                s("geraete"),
                alloc::format!("{}", crate::driver::DEVICES.lock().len()),
                Aktion::Geraete,
                false,
            ),
            (s("netz"), ip, Aktion::Netz, false),
            (s("sprache"), String::new(), Aktion::Sprache, false),
            (s("neustart"), String::new(), Aktion::Neustart, false),
            (s("aus"), String::new(), Aktion::Aus, false),
        ];
        for (i, (titel, unter, a, primaer)) in kacheln.iter().enumerate() {
            let (sp, ze) = ((i as i32) % spalten, (i as i32) / spalten);
            let sp_vis = if self.rtl() { spalten - 1 - sp } else { sp };
            let x = kx + sp_vis * (tw + gap);
            let y = ty + ze * (th + gap);
            self.kachel(x, y, tw, th, titel, unter, *a, *primaer);
        }
    }

    fn zeichne_geraete(&mut self, kx: i32, kw: i32, seite: usize) {
        let l = lang();
        let h = self.cv.h as i32;
        let ky = 84;
        let kh = h - ky - 24;
        self.cv.frame(kx, ky, kw, kh, 18, LINE, CARD, 1);
        // Gruppen
        let mut y = ky + 20;
        for (k, n, mit, _) in gui::geraete_gruppen() {
            let t = alloc::format!("{}:  {mit}/{n} {}", gui::t(l, k), s("mit_treiber"));
            self.zeile(kx, kw, y, &t, 1, if mit == n { TEXT } else { GOLD });
            y += 28;
        }
        y += 8;
        self.cv.rect(kx + 24, y, kw - 48, 1, LINE);
        y += 12;
        // alle Geräte, seitenweise
        let liste: Vec<(String, bool, String)> = crate::driver::DEVICES
            .lock()
            .iter()
            .map(|b| {
                let d = &b.dev;
                // Geräteart in der Systemsprache (Gruppe), Kennungen und Treiber unverändert
                let art = if l == Lang::De {
                    String::from(d.class_name())
                } else {
                    String::from(gui::t(l, gui::kategorie(d.class, d.subclass)))
                };
                (
                    alloc::format!(
                        "{}  {:04x}:{:04x}  ·  {}",
                        d.id(),
                        d.vendor,
                        d.device,
                        b.driver.unwrap_or("–")
                    ),
                    b.driver.is_some(),
                    art,
                )
            })
            .collect();
        let unten = ky + kh - 52 - 20;
        let je = (((unten - 16 - y) / 26).max(1)) as usize;
        let seiten = liste.len().div_ceil(je).max(1);
        let seite = if seite >= seiten { 0 } else { seite };
        self.geraete = Some(seite);
        for (t, ok, art) in liste.iter().skip(seite * je).take(je) {
            let col = if *ok { TEXT } else { MUTED };
            // Art in der Sprache (Spalte am Rand), Kennungen immer von links
            let aw = Canvas::text_w(art, 1);
            let (ax, ix) = if self.rtl() {
                (kx + kw - 32 - aw, kx + 32)
            } else {
                (kx + 32, kx + 260)
            };
            self.cv.text(ax, y, art, 1, col);
            self.cv.rtl = false;
            let iw = if self.rtl() {
                kw - 64 - aw - 20
            } else {
                kw - 260 - 32
            };
            let sc = Canvas::passend(t, 1, iw);
            self.cv.text(ix, y, t, sc, col);
            self.cv.rtl = lang() == Lang::Ar;
            y += 26;
        }
        // Knöpfe: Zurück, Seite weiter
        let bw = 190;
        let seite_t = alloc::format!("{} {}/{}", s("seite"), seite + 1, seiten);
        let (zx, wx) = if self.rtl() {
            (kx + kw - 24 - bw, kx + 24)
        } else {
            (kx + 24, kx + kw - 24 - bw)
        };
        self.kachel(zx, unten, bw, 52, s("zurueck"), "", Aktion::Zurueck, false);
        if seiten > 1 {
            self.kachel(wx, unten, bw, 52, s("weiter"), "", Aktion::Seite(1), true);
        }
        self.cv.text_c(kx + kw / 2, unten + 16, &seite_t, 1, MUTED);
    }

    fn treffer(&self, x: i32, y: i32) -> Option<usize> {
        self.knoepfe
            .iter()
            .position(|k| x >= k.x && x < k.x + k.w && y >= k.y && y < k.y + k.h)
    }

    /// Aktion ausführen; true = Startseite verlassen (Terminal)
    fn ausfuehren(&mut self, a: Aktion) -> bool {
        match a {
            Aktion::Terminal => return true,
            Aktion::Geraete => {
                self.geraete = Some(0);
                self.fokus = 0;
            }
            Aktion::Zurueck => {
                self.geraete = None;
                self.fokus = 1;
            }
            Aktion::Seite(d) => {
                // zeichne_geraete() beginnt nach der letzten Seite wieder von vorn
                self.geraete = Some((self.geraete.unwrap_or(0) as i32 + d).max(0) as usize);
            }
            Aktion::Netz => {
                self.netz = String::from("…");
                self.zeichnen();
                gui::blit(&self.fb, &self.cv);
                self.netz_pruefen(true);
            }
            Aktion::Sprache => {
                let neu = match crate::sprache::get() {
                    crate::sprache::AR => crate::sprache::DE,
                    crate::sprache::DE => crate::sprache::EN,
                    _ => crate::sprache::AR,
                };
                crate::sprache::set(neu);
                crate::sprache::speichern();
                self.netz_pruefen(false);
            }
            Aktion::Neustart => crate::power::neustart(),
            Aktion::Aus => crate::power::aus(),
        }
        false
    }
}

/// Startseite zeigen, bis „Terminal“ gewählt wird
pub fn run() {
    let Some((fb, cv)) = leinwand() else { return };
    crate::drivers::ps2maus::init(cv.w, cv.h);
    crate::console::grafik(true);
    let mut sei = Seite {
        fb,
        cv,
        knoepfe: Vec::new(),
        fokus: 0,
        geraete: None,
        netz: String::new(),
        internet: None,
        zeiger: (0, 0),
    };
    sei.netz_pruefen(false);
    sei.zeichnen();
    gui::blit(&sei.fb, &sei.cv);
    crate::kprintln!("[oberflaeche] Startseite ({})", crate::sprache::code());
    let mut ein = Eingabe { esc: 0 };
    let mut letzte = crate::drivers::ps2maus::zustand();
    let mut taste_war = false;
    loop {
        let mut neu = false;
        let mut aktion = None;
        if let Some(k) = ein.lesen() {
            neu = true;
            let n = sei.knoepfe.len().max(1);
            let rtl = sei.rtl();
            match k {
                Taste::Tab | Taste::Runter => sei.fokus = (sei.fokus + 1) % n,
                Taste::Hoch => sei.fokus = (sei.fokus + n - 1) % n,
                Taste::Rechts => {
                    sei.fokus = if rtl {
                        (sei.fokus + n - 1) % n
                    } else {
                        (sei.fokus + 1) % n
                    }
                }
                Taste::Links => {
                    sei.fokus = if rtl {
                        (sei.fokus + 1) % n
                    } else {
                        (sei.fokus + n - 1) % n
                    }
                }
                Taste::Enter => aktion = sei.knoepfe.get(sei.fokus).map(|k| k.a),
                Taste::Esc => aktion = sei.geraete.map(|_| Aktion::Zurueck),
                Taste::Zeichen(c) => {
                    if let Some(d) = c.to_digit(10) {
                        if d >= 1 && (d as usize) <= sei.knoepfe.len() {
                            sei.fokus = d as usize - 1;
                            aktion = sei.knoepfe.get(sei.fokus).map(|k| k.a);
                        }
                    }
                }
            }
        }
        crate::drivers::ps2maus::abfragen();
        let m = crate::drivers::ps2maus::zustand();
        if m.3 != letzte.3 {
            if let Some(i) = sei.treffer(m.0, m.1).filter(|&i| i != sei.fokus) {
                sei.fokus = i;
                neu = true;
            } else if (m.0, m.1) != sei.zeiger {
                let (ox, oy) = sei.zeiger;
                gui::blit_rect(&sei.fb, &sei.cv, ox - 2, oy - 2, 20, 23);
                gui::zeiger_malen(&sei.fb, m.0, m.1);
                sei.zeiger = (m.0, m.1);
            }
            if taste_war && !m.2 {
                if let Some(i) = sei.treffer(m.0, m.1) {
                    aktion = sei.knoepfe.get(i).map(|k| k.a);
                }
            }
            taste_war = m.2;
            letzte = m;
        }
        if let Some(a) = aktion {
            if sei.ausfuehren(a) {
                break;
            }
            neu = true;
        }
        if neu {
            sei.zeichnen();
            gui::blit(&sei.fb, &sei.cv);
            gui::zeiger_malen(&sei.fb, sei.zeiger.0, sei.zeiger.1);
        } else {
            crate::shell::leerlauf();
            for _ in 0..64 {
                crate::arch::pause();
            }
        }
    }
    crate::console::grafik(false);
    crate::kprint!("\x1b[2J\x1b[H");
    crate::kprintln!(concat!(ultar_brand::product!(), " · {}"), s("unter"));
    crate::kprintln!("{}", s("term_hinweis"));
}
