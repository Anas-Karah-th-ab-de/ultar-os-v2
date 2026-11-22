//! Ultar-Desktop: Leiste (Start, Fensterliste, Uhr, Akku, Modus), Startmenü mit Suche, Hintergrund, Fensterverwaltung
//! (Minimieren, Maximieren, Andocken links/rechts, Alt+Tab) und Desktop-/Tablet-Modus.
//!
//! Alles wird hier in Rust auf der CPU gezeichnet (nur bei Änderungen) und als Speicherpuffer an den Renderer gegeben;
//! Fensterinhalte selbst zeichnet die GPU (bzw. pixman ohne GPU).

pub mod apps;
pub mod canvas;
pub mod text;

use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    process::Command,
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
    time::Instant,
};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::element::memory::MemoryRenderBuffer,
    },
    desktop::{layer_map_for_output, Space, WindowSurface},
    output::Output,
    reexports::wayland_protocols::xdg::shell::server::xdg_toplevel,
    utils::{IsAlive, Logical, Point, Rectangle, Size, Transform, SERIAL_COUNTER},
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};

use crate::{focus::KeyboardFocusTarget, shell::WindowElement, state::Backend, AnvilState};
use apps::{App, Config};
use canvas::{Canvas, Img, Rgba};
use text::Text;

// ---------- Farben (wie die Ultar-Oberfläche) ----------
const BG: Rgba = Rgba::hex(0x0B0D12);
const BG2: Rgba = Rgba::hex(0x07080B);
const CARD: Rgba = Rgba::hex(0x161922);
const CARD_HI: Rgba = Rgba::hex(0x232735);
const LINE: Rgba = Rgba::hex(0x2C303C);
const TEXT: Rgba = Rgba::hex(0xECE8DF);
const SUB: Rgba = Rgba::hex(0x9AA0AD);
const GOLD: Rgba = Rgba::hex(0xC9A227);
const GOLD2: Rgba = Rgba::hex(0xE8C75E);
const RED: Rgba = Rgba::hex(0xE5484D);

static LOGO_PNG: &[u8] = include_bytes!("../../../../assets/brand/logo.png");
static MARK_PNG: &[u8] = include_bytes!("../../../../assets/brand/mark-circuit.png");

// ---------- Globale Werte, die auch die Fensterplatzierung braucht ----------
static TABLET: AtomicBool = AtomicBool::new(false);
static PANEL_H: AtomicI32 = AtomicI32::new(48);

pub fn tablet() -> bool {
    TABLET.load(Ordering::Relaxed)
}
pub fn panel_h() -> i32 {
    PANEL_H.load(Ordering::Relaxed)
}
fn set_tablet(on: bool) {
    TABLET.store(on, Ordering::Relaxed);
    PANEL_H.store(if on { 60 } else { 48 }, Ordering::Relaxed);
}

/// Nutzbare Fläche eines Bildschirms: ohne Layer-Shell-Zonen und ohne die Ultar-Leiste unten.
pub fn usable_area(space: &Space<WindowElement>, output: &Output) -> Rectangle<i32, Logical> {
    let Some(geo) = space.output_geometry(output) else {
        return Rectangle::from_size((800, 600).into());
    };
    let zone = layer_map_for_output(output).non_exclusive_zone();
    let mut r = Rectangle::new(geo.loc + zone.loc, zone.size);
    let bottom_gap = (geo.loc.y + geo.size.h) - (r.loc.y + r.size.h);
    r.size.h -= (panel_h() - bottom_gap).max(0);
    r
}

// ---------- Texte ----------
fn tr(lang: &str, k: &str) -> &'static str {
    match (lang, k) {
        ("ar", "search") => "ابحث عن تطبيق…",
        ("de", "search") => "Apps suchen…",
        (_, "search") => "Search apps…",
        ("ar", "apps") => "التطبيقات",
        ("de", "apps") => "Apps",
        (_, "apps") => "Apps",
        ("ar", "none") => "لا توجد نتائج",
        ("de", "none") => "Keine Treffer",
        (_, "none") => "No results",
        ("ar", "logout") => "العودة إلى Ultar",
        ("de", "logout") => "Zurück zu Ultar",
        (_, "logout") => "Back to Ultar",
        ("ar", "tablet") => "وضع اللوحي",
        ("de", "tablet") => "Tablet-Modus",
        (_, "tablet") => "Tablet mode",
        ("ar", "desktop") => "وضع سطح المكتب",
        ("de", "desktop") => "Desktop-Modus",
        (_, "desktop") => "Desktop mode",
        _ => "",
    }
}

const WEEKDAYS: [[&str; 7]; 3] = [
    ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"],
    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
    ["الإثنين", "الثلاثاء", "الأربعاء", "الخميس", "الجمعة", "السبت", "الأحد"],
];

// ---------- Fensterliste ----------
#[derive(Clone)]
pub struct WinInfo {
    pub elem: WindowElement,
    pub title: String,
    pub active: bool,
    pub minimized: bool,
}

/// Gemerkte Position eines minimierten Fensters.
struct MinLoc(std::cell::Cell<Point<i32, Logical>>);

pub fn window_title(w: &WindowElement) -> String {
    match w.0.underlying_surface() {
        WindowSurface::Wayland(t) => with_states(t.wl_surface(), |s| {
            s.data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|d| {
                    let d = d.lock().ok()?;
                    d.title.clone().filter(|t| !t.is_empty()).or_else(|| d.app_id.clone())
                })
                .unwrap_or_default()
        }),
        #[cfg(feature = "xwayland")]
        WindowSurface::X11(x) => {
            let t = x.title();
            if t.is_empty() {
                x.class()
            } else {
                t
            }
        }
    }
}

pub fn is_activated(w: &WindowElement) -> bool {
    match w.0.underlying_surface() {
        WindowSurface::Wayland(t) => t
            .current_state()
            .states
            .contains(xdg_toplevel::State::Activated),
        #[cfg(feature = "xwayland")]
        WindowSurface::X11(x) => x.is_activated(),
    }
}

fn is_task_window(w: &WindowElement) -> bool {
    #[cfg(feature = "xwayland")]
    if let Some(x) = w.0.x11_surface() {
        return !x.is_override_redirect();
    }
    true
}

// ---------- Trefferflächen ----------
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hit {
    Start,
    Task(usize),
    Mode,
    Launch(usize),
    Logout,
    Search,
    LauncherBg,
    Panel,
}

/// Ein fertiger Puffer zum Zeichnen: Position bezogen auf den Bildschirm (logisch), oben = über den Fenstern.
#[derive(Clone)]
pub struct Layer {
    pub buf: MemoryRenderBuffer,
    pub loc: Point<i32, Logical>,
    pub top: bool,
}

struct Cached {
    key: u64,
    buf: MemoryRenderBuffer,
}

#[derive(Default)]
struct OutCache {
    wall: Option<Cached>,
    panel: Option<Cached>,
    launcher: Option<Cached>,
}

impl std::fmt::Debug for Ui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ui")
    }
}

pub struct Ui {
    pub cfg: Config,
    apps: Vec<App>,
    apps_loaded: Instant,
    text: Text,
    logo: Option<Img>,
    mark: Option<Img>,
    pub launcher: bool,
    pub query: String,
    scroll: usize,
    pub minimized: Vec<WindowElement>,
    order: Vec<WindowElement>,
    cache: HashMap<String, OutCache>,
    /// Trefferflächen (global, logisch) aus dem letzten Zeichnen
    hits: Vec<(Rectangle<i32, Logical>, Hit)>,
    /// Gefilterte App-Liste (Indizes in `apps`) wie zuletzt gezeichnet
    shown: Vec<usize>,
    hover: Option<Hit>,
    /// Gedrückte Taste/Berührung wurde von der Leiste übernommen -> Loslassen nicht an Fenster geben
    pub swallow_release: bool,
    pub rtl: bool,
    pub autostarted: bool,
}

impl Ui {
    pub fn new() -> Self {
        let cfg = apps::load_config();
        let start_tablet = match cfg.mode.as_str() {
            "tablet" => true,
            _ => false,
        };
        set_tablet(start_tablet);
        let apps = apps::load_apps(&cfg);
        tracing::info!("Ultar-Desktop: {} Apps im Startmenü, Sprache {}", apps.len(), cfg.lang);
        let rtl = cfg.lang == "ar";
        Self {
            apps,
            apps_loaded: Instant::now(),
            cfg,
            text: Text::new(),
            logo: Img::from_png(LOGO_PNG),
            mark: Img::from_png(MARK_PNG),
            launcher: false,
            query: String::new(),
            scroll: 0,
            minimized: Vec::new(),
            order: Vec::new(),
            cache: HashMap::new(),
            hits: Vec::new(),
            shown: Vec::new(),
            hover: None,
            swallow_release: false,
            rtl,
            autostarted: false,
        }
    }

    /// Bei "auto": Tablet-Modus auf schmalen Bildschirmen (Handy, kleines Tablet).
    pub fn output_added(&mut self, logical_w: i32) {
        if self.cfg.mode == "auto" {
            set_tablet(logical_w < 900);
        }
    }

    pub fn hit(&self, p: Point<f64, Logical>) -> Option<Hit> {
        let p = p.to_i32_floor();
        // Startmenü liegt über der Leiste: zuerst prüfen (Liste ist von hinten nach vorn gefüllt)
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h).or(if self.launcher {
            Some(Hit::LauncherBg)
        } else {
            None
        })
    }

    pub fn set_hover(&mut self, p: Point<f64, Logical>) {
        let h = self.hits.iter().rev().find(|(r, _)| r.contains(p.to_i32_floor())).map(|(_, h)| *h);
        self.hover = h;
    }

    pub fn toggle_launcher(&mut self) {
        self.launcher = !self.launcher;
        self.query.clear();
        self.scroll = 0;
        // Neu installierte Programme aufnehmen (höchstens alle 5 s neu einlesen)
        if self.launcher && self.apps_loaded.elapsed().as_secs() > 5 {
            self.apps = apps::load_apps(&self.cfg);
            self.apps_loaded = Instant::now();
        }
    }

    pub fn scroll_launcher(&mut self, down: bool) {
        if down {
            self.scroll += 1;
        } else {
            self.scroll = self.scroll.saturating_sub(1);
        }
    }

    pub fn app_exec(&self, idx: usize) -> Option<String> {
        self.apps.get(idx).map(|a| a.exec.clone())
    }

    /// Erste passende App (Enter im Suchfeld).
    pub fn first_shown(&self) -> Option<usize> {
        self.filtered().first().copied()
    }

    fn filtered(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        self.apps
            .iter()
            .enumerate()
            .filter(|(_, a)| q.is_empty() || a.name.to_lowercase().contains(&q) || a.keywords.contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    /// Reihenfolge der Leiste mit dem Raum abgleichen (neue Fenster hinten anhängen, tote entfernen).
    pub fn sync_order(&mut self, space: &Space<WindowElement>) {
        self.order.retain(|w| w.alive());
        self.minimized.retain(|w| w.alive());
        for w in space.elements() {
            if is_task_window(w) && !self.order.contains(w) {
                self.order.push(w.clone());
            }
        }
        let min = &self.minimized;
        self.order.retain(|w| space.elements().any(|e| e == w) || min.contains(w));
    }

    pub fn order(&self) -> &[WindowElement] {
        &self.order
    }

    /// Puffer für einen Bildschirm erzeugen bzw. aus dem Zwischenspeicher holen.
    pub fn layers(&mut self, name: &str, geo: Rectangle<i32, Logical>, scale: i32, wins: &[WinInfo]) -> Vec<Layer> {
        let s = scale.max(1);
        let (w, h) = (geo.size.w, geo.size.h);
        let mut out = Vec::new();
        self.hits.retain(|_| false);
        let mut cache = self.cache.remove(name).unwrap_or_default();

        // Hintergrund
        let key = hash(&(w, h, s, 1u8));
        if cache.wall.as_ref().map(|c| c.key) != Some(key) {
            let cv = self.draw_wall(w, h, s);
            cache.wall = Some(Cached { key, buf: to_buf(cv, s, true) });
        }
        out.push(Layer { buf: cache.wall.as_ref().unwrap().buf.clone(), loc: (0, 0).into(), top: false });

        // Leiste
        let ph = panel_h();
        let now = chrono::Local::now();
        let minute = now.format("%H:%M %d.%m.%Y %u").to_string();
        let bat = battery();
        let tasks: Vec<(String, bool, bool)> = wins.iter().map(|w| (w.title.clone(), w.active, w.minimized)).collect();
        let key = hash(&(w, ph, s, &minute, &tasks, bat, self.launcher, self.hover, tablet(), self.rtl));
        let mut panel_hits = Vec::new();
        if cache.panel.as_ref().map(|c| c.key) != Some(key) {
            let cv = self.draw_panel(w, ph, s, wins, &now, bat, &mut panel_hits);
            cache.panel = Some(Cached { key, buf: to_buf(cv, s, false) });
        } else {
            self.panel_hits(w, ph, wins.len(), &mut panel_hits);
        }
        let py = h - ph;
        for (r, hit) in panel_hits {
            self.hits.push((Rectangle::new(geo.loc + Point::from((r.loc.x, r.loc.y + py)), r.size), hit));
        }
        out.push(Layer { buf: cache.panel.as_ref().unwrap().buf.clone(), loc: (0, py).into(), top: true });

        // Startmenü
        if self.launcher {
            let rect = self.launcher_rect(w, h);
            self.shown = self.filtered();
            let names: Vec<&str> = self.shown.iter().map(|&i| self.apps[i].name.as_str()).collect();
            let key = hash(&((rect.size.w, rect.size.h), s, &self.query, &names, self.scroll, self.hover, self.rtl));
            let mut lh = Vec::new();
            if cache.launcher.as_ref().map(|c| c.key) != Some(key) {
                let cv = self.draw_launcher(rect.size.w, rect.size.h, s, &mut lh);
                cache.launcher = Some(Cached { key, buf: to_buf(cv, s, false) });
            } else {
                // Trefferflächen ohne Neuzeichnen neu berechnen
                let mut dummy = Canvas::new(1, 1);
                let _ = self.launcher_content(&mut dummy, rect.size.w, rect.size.h, 0, &mut lh);
            }
            self.hits.push((Rectangle::new(geo.loc + rect.loc, rect.size), Hit::Panel));
            for (r, hit) in lh {
                self.hits.push((Rectangle::new(geo.loc + rect.loc + r.loc, r.size), hit));
            }
            out.push(Layer { buf: cache.launcher.as_ref().unwrap().buf.clone(), loc: rect.loc, top: true });
        }
        self.cache.insert(name.to_string(), cache);
        out
    }

    fn launcher_rect(&self, w: i32, h: i32) -> Rectangle<i32, Logical> {
        let ph = panel_h();
        if tablet() {
            return Rectangle::new((0, 0).into(), (w, h - ph).into());
        }
        let lw = (w - 32).min(780);
        let lh = (h - ph - 32).min(580);
        let x = if self.rtl { w - lw - 12 } else { 12 };
        Rectangle::new((x, h - ph - lh - 10).into(), (lw, lh).into())
    }

    // ---------- Zeichnen ----------

    fn draw_wall(&mut self, w: i32, h: i32, s: i32) -> Canvas {
        let mut cv = Canvas::new(w * s, h * s);
        cv.vgrad(BG, BG2);
        let (fw, fh) = ((w * s) as f32, (h * s) as f32);
        cv.glow(fw * 0.5, fh * 0.42, fw.min(fh) * 0.62, GOLD.a(26));
        cv.glow(fw * 0.85, fh * 0.1, fw.min(fh) * 0.35, Rgba::hex(0x3B4A7A).a(20));
        if let Some(logo) = self.logo.clone() {
            let lw = ((w as f32 * 0.32).min(440.0) * s as f32) as i32;
            let lh = (lw as f32 * logo.h as f32 / logo.w as f32) as i32;
            let (x, y) = ((w * s - lw) / 2, (fh * 0.40) as i32 - lh / 2);
            cv.image(&logo, x, y, lw, lh, 0.92);
            let t = "Ultar OS";
            let sz = 15.0 * s as f32;
            let tw = self.text.width(t, sz, true);
            self.text.draw(&mut cv, t, (w * s) as f32 / 2.0 - tw / 2.0, (y + lh) as f32 + 18.0 * s as f32, sz, SUB, true);
        }
        cv
    }

    /// Rechtecke der Leiste (logisch, relativ zur Leiste); bei Arabisch gespiegelt.
    fn panel_layout(&self, w: i32, ph: i32, n: usize) -> (Rectangle<i32, Logical>, Vec<Rectangle<i32, Logical>>, Rectangle<i32, Logical>, Rectangle<i32, Logical>) {
        let pad = 6;
        let bs = ph - pad * 2;
        let start = Rectangle::new((pad + 2, pad).into(), (bs, bs).into());
        let clock_w = if tablet() { 104 } else { 92 };
        let clock = Rectangle::new((w - clock_w - 8, 0).into(), (clock_w, ph).into());
        let mode = Rectangle::new((clock.loc.x - bs - 74, pad).into(), (bs, bs).into());
        let x0 = start.loc.x + bs + 10;
        let avail = (mode.loc.x - 10 - x0).max(0);
        let tw = if n == 0 { 0 } else { (avail / n as i32).min(if tablet() { 240 } else { 220 }) };
        let tasks = (0..n)
            .map(|i| Rectangle::new((x0 + i as i32 * tw, pad).into(), ((tw - 6).max(0), bs).into()))
            .collect();
        let m = |r: Rectangle<i32, Logical>| if self.rtl { Rectangle::new((w - r.loc.x - r.size.w, r.loc.y).into(), r.size) } else { r };
        let tasks: Vec<_> = Vec::into_iter(tasks).map(m).collect();
        (m(start), tasks, m(mode), m(clock))
    }

    fn panel_hits(&self, w: i32, ph: i32, n: usize, hits: &mut Vec<(Rectangle<i32, Logical>, Hit)>) {
        let (start, tasks, mode, _) = self.panel_layout(w, ph, n);
        hits.push((Rectangle::new((0, 0).into(), (w, ph).into()), Hit::Panel));
        hits.push((start, Hit::Start));
        hits.push((mode, Hit::Mode));
        for (i, r) in tasks.into_iter().enumerate() {
            hits.push((r, Hit::Task(i)));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_panel(
        &mut self,
        w: i32,
        ph: i32,
        s: i32,
        wins: &[WinInfo],
        now: &chrono::DateTime<chrono::Local>,
        bat: Option<(u8, bool)>,
        hits: &mut Vec<(Rectangle<i32, Logical>, Hit)>,
    ) -> Canvas {
        let k = |v: i32| v * s;
        let kf = |v: f32| v * s as f32;
        let mut cv = Canvas::new(k(w), k(ph));
        cv.fill(0, 0, k(w), k(ph), Rgba::hex(0x0E1016).a(238));
        cv.fill(0, 0, k(w), s.max(1), LINE);
        let (start, tasks, mode, clock) = self.panel_layout(w, ph, wins.len());
        self.panel_hits(w, ph, wins.len(), hits);

        // Start
        let hov = |h: Hit| self.hover == Some(h);
        let sbg = if self.launcher { GOLD.a(70) } else if hov(Hit::Start) { CARD_HI } else { CARD };
        cv.rrect(k(start.loc.x), k(start.loc.y), k(start.size.w), k(start.size.h), kf(10.0), sbg);
        if let Some(mark) = self.mark.clone() {
            let iw = k(start.size.w - 10);
            let ih = iw * mark.h as i32 / mark.w as i32;
            cv.image(&mark, k(start.loc.x) + (k(start.size.w) - iw) / 2, k(start.loc.y) + (k(start.size.h) - ih) / 2, iw, ih, 1.0);
        }

        // Fenster
        let fsz = kf(if tablet() { 14.0 } else { 13.0 });
        for (i, (r, win)) in tasks.iter().zip(wins).enumerate() {
            if r.size.w < 24 {
                continue;
            }
            let bg = if win.active {
                CARD_HI
            } else if hov(Hit::Task(i)) {
                Rgba::hex(0x1C2029)
            } else {
                CARD.a(200)
            };
            cv.rrect(k(r.loc.x), k(r.loc.y), k(r.size.w), k(r.size.h), kf(9.0), bg);
            if win.active {
                let bw = r.size.w * 2 / 5;
                cv.rrect(k(r.loc.x + (r.size.w - bw) / 2), k(r.loc.y + r.size.h - 3), k(bw), k(3), kf(1.5), GOLD);
            } else if !win.minimized {
                cv.circle(kf(r.loc.x as f32 + r.size.w as f32 / 2.0), kf((r.loc.y + r.size.h - 3) as f32), kf(1.6), SUB);
            }
            // Symbol: Anfangsbuchstabe auf farbiger Fläche
            let isz = (r.size.h - 14).min(26);
            let ix = if self.rtl { r.loc.x + r.size.w - 8 - isz } else { r.loc.x + 8 };
            let iy = r.loc.y + (r.size.h - isz) / 2;
            let hue = hue_for(&win.title);
            cv.rrect(k(ix), k(iy), k(isz), k(isz), kf(7.0), hue.a(if win.minimized { 110 } else { 230 }));
            let letter: String = win.title.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
            let lsz = kf(isz as f32 * 0.55);
            let lw = self.text.width(&letter, lsz, true);
            self.text.draw(&mut cv, &letter, kf(ix as f32 + isz as f32 / 2.0) - lw / 2.0, kf(iy as f32) + (kf(isz as f32) - Text::line_h(lsz)) / 2.0, lsz, Rgba::hex(0x101114), true);
            let tx0 = if self.rtl { r.loc.x + 8 } else { ix + isz + 8 };
            let maxw = kf((r.size.w - isz - 24) as f32);
            if maxw > kf(12.0) {
                let t = self.text.fit(&win.title, fsz, false, maxw);
                let tw = self.text.width(&t, fsz, false);
                let tx = if self.rtl { kf((ix - 8) as f32) - tw } else { kf(tx0 as f32) };
                let col = if win.minimized { SUB } else { TEXT };
                self.text.draw(&mut cv, &t, tx, kf(r.loc.y as f32) + (kf(r.size.h as f32) - Text::line_h(fsz)) / 2.0, fsz, col, false);
            }
        }

        // Modus Desktop/Tablet
        let mb = if hov(Hit::Mode) { CARD_HI } else { CARD };
        cv.rrect(k(mode.loc.x), k(mode.loc.y), k(mode.size.w), k(mode.size.h), kf(10.0), mb);
        let (mx, my, mw, mh) = (kf(mode.loc.x as f32), kf(mode.loc.y as f32), kf(mode.size.w as f32), kf(mode.size.h as f32));
        let th = kf(1.6);
        if tablet() {
            // Symbol Bildschirm (wechselt zu Desktop)
            let (x0, y0, x1, y1) = (mx + mw * 0.24, my + mh * 0.28, mx + mw * 0.76, my + mh * 0.62);
            for (a, b, c, d) in [(x0, y0, x1, y0), (x1, y0, x1, y1), (x1, y1, x0, y1), (x0, y1, x0, y0)] {
                cv.line(a, b, c, d, th, TEXT);
            }
            cv.line(mx + mw * 0.5, y1, mx + mw * 0.5, my + mh * 0.74, th, TEXT);
            cv.line(mx + mw * 0.38, my + mh * 0.74, mx + mw * 0.62, my + mh * 0.74, th, TEXT);
        } else {
            // Symbol Tablet (wechselt zu Tablet)
            let (x0, y0, x1, y1) = (mx + mw * 0.32, my + mh * 0.22, mx + mw * 0.68, my + mh * 0.78);
            for (a, b, c, d) in [(x0, y0, x1, y0), (x1, y0, x1, y1), (x1, y1, x0, y1), (x0, y1, x0, y0)] {
                cv.line(a, b, c, d, th, TEXT);
            }
            cv.circle(mx + mw * 0.5, y1 - mh * 0.08, kf(1.4), TEXT);
        }

        // Akku
        if let Some((pct, charging)) = bat {
            // Platz zwischen Modus-Knopf und Uhr (bei Arabisch gespiegelt: links vom Modus-Knopf)
            let bx = if self.rtl { mode.loc.x - 70 } else { mode.loc.x + mode.size.w + 10 };
            let by = ph / 2 - 6;
            let (bw, bh) = (24, 12);
            let col = if charging { Rgba::hex(0x4CC38A) } else if pct < 15 { RED } else { TEXT };
            cv.rrect_line(k(bx), k(by), k(bw), k(bh), kf(3.0), col);
            cv.fill(k(bx + bw), k(by + 4), k(2), k(4), col);
            let fill = ((bw - 4) as f32 * pct as f32 / 100.0).round() as i32;
            cv.rrect(k(bx + 2), k(by + 2), k(fill.max(1)), k(bh - 4), kf(1.5), col);
            let t = format!("{pct}%");
            let sz = kf(11.0);
            self.text.draw(&mut cv, &t, kf((bx + bw + 6) as f32), kf((ph / 2) as f32) - Text::line_h(sz) / 2.0, sz, SUB, false);
        }

        // Uhr
        let lang = self.cfg.lang.clone();
        let time = now.format("%H:%M").to_string();
        let wd = now.format("%u").to_string().parse::<usize>().unwrap_or(1).clamp(1, 7) - 1;
        let li = match lang.as_str() {
            "de" => 0,
            "ar" => 2,
            _ => 1,
        };
        let date = format!("{} {}", WEEKDAYS[li][wd], now.format("%d.%m.%Y"));
        let (tsz, dsz) = (kf(if tablet() { 17.0 } else { 15.0 }), kf(10.5));
        let total = Text::line_h(tsz) + Text::line_h(dsz) - kf(4.0);
        let y0 = kf(ph as f32) / 2.0 - total / 2.0;
        let cx = kf(clock.loc.x as f32 + clock.size.w as f32 / 2.0);
        let tw = self.text.width(&time, tsz, true);
        self.text.draw(&mut cv, &time, cx - tw / 2.0, y0, tsz, TEXT, true);
        let dw = self.text.width(&date, dsz, false);
        self.text.draw(&mut cv, &date, cx - dw / 2.0, y0 + Text::line_h(tsz) - kf(4.0), dsz, SUB, false);
        cv
    }

    fn draw_launcher(&mut self, w: i32, h: i32, s: i32, hits: &mut Vec<(Rectangle<i32, Logical>, Hit)>) -> Canvas {
        let mut cv = Canvas::new(w * s, h * s);
        let kf = |v: f32| v * s as f32;
        if tablet() {
            cv.vgrad(Rgba::hex(0x0D0F15).a(246), Rgba::hex(0x07080B).a(250));
        } else {
            cv.rrect(0, 0, w * s, h * s, kf(18.0), Rgba::hex(0x12141B).a(246));
            cv.rrect_line(0, 0, w * s, h * s, kf(18.0), LINE);
        }
        self.launcher_content(&mut cv, w, h, s, hits);
        cv
    }

    /// Inhalt des Startmenüs; mit s = 0 werden nur die Trefferflächen berechnet.
    fn launcher_content(&mut self, cv: &mut Canvas, w: i32, h: i32, s: i32, hits: &mut Vec<(Rectangle<i32, Logical>, Hit)>) -> Option<()> {
        let draw = s > 0;
        let s = s.max(1);
        let k = |v: i32| v * s;
        let kf = |v: f32| v * s as f32;
        let lang = self.cfg.lang.clone();
        let pad = if tablet() { 28 } else { 22 };

        // Kopf: Logo + Suchfeld
        let head_h = 44;
        let logo_w = if w > 520 { 110 } else { 0 };
        if draw && logo_w > 0 {
            if let Some(logo) = self.logo.clone() {
                let lh = 40;
                let lw = lh * logo.w as i32 / logo.h as i32;
                let lx = if self.rtl { w - pad - lw } else { pad };
                cv.image(&logo, k(lx), k(pad + (head_h - lh) / 2), k(lw), k(lh), 1.0);
            }
        }
        let sx = if self.rtl { pad } else { pad + logo_w };
        let sw = w - pad * 2 - logo_w;
        let search = Rectangle::new((sx, pad).into(), (sw, head_h).into());
        hits.push((search, Hit::Search));
        if draw {
            cv.rrect(k(sx), k(pad), k(sw), k(head_h), kf(12.0), CARD);
            cv.rrect_line(k(sx), k(pad), k(sw), k(head_h), kf(12.0), if self.query.is_empty() { LINE } else { GOLD.a(160) });
            // Lupe
            let (lx, ly) = if self.rtl { (kf((sx + sw - 24) as f32), kf((pad + head_h / 2) as f32)) } else { (kf((sx + 22) as f32), kf((pad + head_h / 2) as f32)) };
            cv.circle(lx - kf(2.0), ly - kf(2.0), kf(6.5), SUB);
            cv.circle(lx - kf(2.0), ly - kf(2.0), kf(4.8), CARD);
            cv.line(lx + kf(2.5), ly + kf(2.5), lx + kf(7.0), ly + kf(7.0), kf(2.0), SUB);
            let fsz = kf(15.0);
            let (t, col) = if self.query.is_empty() { (tr(&lang, "search").to_string(), SUB) } else { (format!("{}|", self.query), TEXT) };
            let tw = self.text.width(&t, fsz, false);
            let tx = if self.rtl { kf((sx + sw - 44) as f32) - tw } else { kf((sx + 42) as f32) };
            self.text.draw(cv, &t, tx, kf((pad + head_h / 2) as f32) - Text::line_h(fsz) / 2.0, fsz, col, false);
        }

        // Raster
        let top = pad + head_h + 38;
        let foot_h = 48;
        let (tile_w, tile_h) = if w < 520 { (104, 112) } else if tablet() { (132, 124) } else { (116, 108) };
        let grid_w = w - pad * 2;
        let cols = (grid_w / tile_w).max(1);
        let rows_vis = ((h - top - foot_h - pad) / tile_h).max(1);
        let n = self.shown.len();
        let total_rows = (n as i32 + cols - 1) / cols;
        let max_scroll = (total_rows - rows_vis).max(0) as usize;
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
        let xoff = pad + (grid_w - cols * tile_w) / 2;
        if draw {
            let sz = kf(12.0);
            let label = tr(&lang, "apps");
            let lw = self.text.width(label, sz, true);
            let lx = if self.rtl { kf((w - pad) as f32) - lw } else { kf(pad as f32) };
            self.text.draw(cv, label, lx, kf((top - 24) as f32), sz, GOLD2, true);
            if n == 0 {
                let t = tr(&lang, "none");
                let fsz = kf(15.0);
                let tw = self.text.width(t, fsz, false);
                self.text.draw(cv, t, kf(w as f32 / 2.0) - tw / 2.0, kf((top + 40) as f32), fsz, SUB, false);
            }
        }
        let shown = self.shown.clone();
        for (slot, &ai) in shown.iter().enumerate().skip(self.scroll * cols as usize).take((rows_vis * cols) as usize) {
            let vis = slot - self.scroll * cols as usize;
            let (c, r) = ((vis as i32) % cols, (vis as i32) / cols);
            let c = if self.rtl { cols - 1 - c } else { c };
            let rect = Rectangle::new((xoff + c * tile_w + 4, top + r * tile_h).into(), (tile_w - 8, tile_h - 6).into());
            hits.push((rect, Hit::Launch(ai)));
            if !draw {
                continue;
            }
            let hovered = self.hover == Some(Hit::Launch(ai)) || (slot == 0 && !self.query.is_empty());
            if hovered {
                cv.rrect(k(rect.loc.x), k(rect.loc.y), k(rect.size.w), k(rect.size.h), kf(14.0), CARD_HI);
            }
            let isz = if w < 520 { 50 } else if tablet() { 60 } else { 52 };
            let ix = rect.loc.x + (rect.size.w - isz) / 2;
            let iy = rect.loc.y + 10;
            let app = self.apps[ai].clone();
            if let Some(icon) = &app.icon {
                cv.image(icon, k(ix), k(iy), k(isz), k(isz), 1.0);
            } else {
                let hue = hue_for(&app.name);
                cv.rrect(k(ix), k(iy), k(isz), k(isz), kf(14.0), hue);
                cv.rrect(k(ix), k(iy), k(isz), k(isz / 2), kf(14.0), Rgba(255, 255, 255, 22));
                let letter: String = app.name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
                let lsz = kf(isz as f32 * 0.5);
                let lw = self.text.width(&letter, lsz, true);
                self.text.draw(cv, &letter, kf(ix as f32 + isz as f32 / 2.0) - lw / 2.0, kf(iy as f32) + (kf(isz as f32) - Text::line_h(lsz)) / 2.0, lsz, Rgba::hex(0x101114), true);
            }
            let fsz = kf(12.0);
            let t = self.text.fit(&app.name, fsz, false, kf((rect.size.w - 8) as f32));
            let tw = self.text.width(&t, fsz, false);
            self.text.draw(cv, &t, kf(rect.loc.x as f32 + rect.size.w as f32 / 2.0) - tw / 2.0, kf((iy + isz + 7) as f32), fsz, TEXT, false);
        }

        // Fuß: Marke links, „Zurück zu Ultar“ rechts
        let fy = h - pad - 36;
        let lt = tr(&lang, "logout");
        let fsz = kf(13.0);
        let bw = (self.text.width(lt, fsz, false) / s as f32) as i32 + 46;
        let bx = if self.rtl { pad } else { w - pad - bw };
        let btn = Rectangle::new((bx, fy).into(), (bw, 36).into());
        hits.push((btn, Hit::Logout));
        if draw {
            cv.fill(k(pad), k(fy - 12), k(w - pad * 2), s, LINE);
            let brand = "KM Code Craft · Ultar OS";
            let bsz = kf(11.5);
            let bwid = self.text.width(brand, bsz, false);
            let brx = if self.rtl { kf((w - pad) as f32) - bwid } else { kf(pad as f32) };
            self.text.draw(cv, brand, brx, kf((fy + 18) as f32) - Text::line_h(bsz) / 2.0, bsz, SUB, false);
            let hb = self.hover == Some(Hit::Logout);
            cv.rrect(k(bx), k(fy), k(bw), k(36), kf(10.0), if hb { RED.a(60) } else { CARD });
            // Ein/Aus-Symbol
            let (cx, cy) = (kf((bx + 18) as f32), kf((fy + 18) as f32));
            cv.circle(cx, cy, kf(7.0), if hb { RED } else { SUB });
            cv.circle(cx, cy, kf(5.2), if hb { Rgba::hex(0x3A1E22) } else { CARD });
            cv.line(cx, cy - kf(8.0), cx, cy - kf(1.0), kf(1.8), if hb { RED } else { SUB });
            self.text.draw(cv, lt, kf((bx + 32) as f32), cy - Text::line_h(fsz) / 2.0, fsz, TEXT, false);
        }
        Some(())
    }
}

fn to_buf(cv: Canvas, scale: i32, opaque: bool) -> MemoryRenderBuffer {
    let opaque_regions = opaque.then(|| vec![Rectangle::from_size((cv.w, cv.h).into())]);
    MemoryRenderBuffer::from_slice(&cv.px, Fourcc::Argb8888, (cv.w, cv.h), scale, Transform::Normal, opaque_regions)
}

fn hash<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// Gleichbleibende, gut lesbare Farbe je Name.
fn hue_for(name: &str) -> Rgba {
    const P: [u32; 8] = [0xC9A227, 0x4F8EF7, 0x4CC38A, 0xE07A5F, 0x9B7BEA, 0x3DB8C9, 0xE6A23C, 0xD3667E];
    let mut h = 0u32;
    for b in name.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    Rgba::hex(P[(h % 8) as usize])
}

/// Akku (Prozent, lädt) aus /sys/class/power_supply – auf Desktops ohne Akku: keiner.
fn battery() -> Option<(u8, bool)> {
    let rd = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for e in rd.flatten() {
        let p = e.path();
        let typ = std::fs::read_to_string(p.join("type")).unwrap_or_default();
        if typ.trim() != "Battery" {
            continue;
        }
        let cap = std::fs::read_to_string(p.join("capacity")).ok()?.trim().parse::<u8>().ok()?;
        let st = std::fs::read_to_string(p.join("status")).unwrap_or_default();
        return Some((cap.min(100), st.trim() == "Charging" || st.trim() == "Full"));
    }
    None
}

// ---------- Fensterverwaltung im Zustand des Compositors ----------

impl<BackendData: Backend> AnvilState<BackendData> {
    /// Fensterliste für die Leiste.
    pub fn ui_windows(&mut self) -> Vec<WinInfo> {
        self.ui.sync_order(&self.space);
        let active = self.focused_window();
        self.ui
            .order()
            .iter()
            .map(|w| WinInfo {
                elem: w.clone(),
                title: {
                    let t = window_title(w);
                    if t.is_empty() {
                        "Fenster".into()
                    } else {
                        t
                    }
                },
                active: !self.ui.minimized.contains(w) && active.as_ref() == Some(w),
                minimized: self.ui.minimized.contains(w),
            })
            .collect()
    }

    /// Puffer der Oberfläche für einen Bildschirm.
    pub fn ui_layers(&mut self, output: &Output) -> Vec<Layer> {
        let Some(geo) = self.space.output_geometry(output) else { return Vec::new() };
        let scale = output.current_scale().integer_scale();
        self.ui.set_hover(self.pointer.current_location());
        let wins = self.ui_windows();
        self.ui.layers(&output.name(), geo, scale, &wins)
    }

    /// Klick/Berührung auf der Oberfläche. Liefert true, wenn die Eingabe übernommen wurde.
    pub fn ui_press(&mut self, pos: Point<f64, Logical>) -> bool {
        let Some(hit) = self.ui.hit(pos) else { return false };
        let launcher_was_open = self.ui.launcher;
        match hit {
            Hit::Start => self.ui.toggle_launcher(),
            Hit::Task(i) => {
                let wins = self.ui_windows();
                if let Some(w) = wins.get(i) {
                    if w.active && !w.minimized {
                        self.minimize_window(&w.elem);
                    } else {
                        self.focus_window(&w.elem);
                    }
                }
                self.ui.launcher = false;
            }
            Hit::Mode => {
                set_tablet(!tablet());
                if tablet() {
                    self.maximize_all();
                }
            }
            Hit::Launch(ai) => {
                if let Some(exec) = self.ui.app_exec(ai) {
                    self.spawn_shell(&exec);
                }
                self.ui.launcher = false;
            }
            Hit::Logout => {
                tracing::info!("Sitzung beendet – zurück zur Ultar-Oberfläche");
                self.running.store(false, Ordering::SeqCst);
            }
            Hit::LauncherBg => self.ui.launcher = false,
            Hit::Search | Hit::Panel => {}
        }
        if launcher_was_open && !self.ui.launcher {
            self.ui.query.clear();
        }
        self.ui.swallow_release = true;
        true
    }

    /// Befehl über die Shell starten (mit den Umgebungsvariablen dieser Sitzung).
    pub fn spawn_shell(&mut self, cmd: &str) {
        tracing::info!(cmd, "Programm starten");
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(format!("exec {cmd}"));
        if let Some(s) = &self.socket_name {
            c.env("WAYLAND_DISPLAY", s);
        }
        #[cfg(feature = "xwayland")]
        if let Some(d) = self.xdisplay {
            c.env("DISPLAY", format!(":{d}"));
        }
        c.env("XDG_CURRENT_DESKTOP", "Ultar")
            .env("XDG_SESSION_TYPE", "wayland")
            // Electron-Apps (VS Code, Discord …) nativ unter Wayland, Qt/GTK ebenso
            .env("ELECTRON_OZONE_PLATFORM_HINT", "auto")
            .env("QT_QPA_PLATFORM", "wayland;xcb")
            .env("GDK_BACKEND", "wayland,x11")
            .env("MOZ_ENABLE_WAYLAND", "1")
            .env("SDL_VIDEODRIVER", "wayland,x11");
        if let Err(e) = c.spawn() {
            tracing::error!(cmd, err = %e, "Programm konnte nicht starten");
        }
    }

    pub fn focus_window(&mut self, w: &WindowElement) {
        if let Some(i) = self.ui.minimized.iter().position(|m| m == w) {
            self.ui.minimized.remove(i);
            let loc = w.user_data().get::<MinLoc>().map(|l| l.0.get()).unwrap_or_default();
            self.space.map_element(w.clone(), loc, true);
        }
        self.space.raise_element(w, true);
        #[cfg(feature = "xwayland")]
        if let Some(x) = w.0.x11_surface() {
            if let Some(xwm) = self.xwm.as_mut() {
                let _ = xwm.raise_window(x);
            }
        }
        let serial = SERIAL_COUNTER.next_serial();
        if let Some(kb) = self.seat.get_keyboard() {
            kb.set_focus(self, Some(w.clone().into()), serial);
        }
        self.flush_activation();
    }

    /// Geänderte Aktivierung (Fensterkopf hell/dunkel) sofort an alle Wayland-Fenster senden.
    pub fn flush_activation(&mut self) {
        for e in self.space.elements() {
            if let Some(t) = e.0.toplevel() {
                if t.is_initial_configure_sent() {
                    t.send_pending_configure();
                }
            }
        }
    }

    pub fn minimize_window(&mut self, w: &WindowElement) {
        let Some(loc) = self.space.element_location(w) else { return };
        w.user_data().insert_if_missing(|| MinLoc(std::cell::Cell::new(loc)));
        if let Some(m) = w.user_data().get::<MinLoc>() {
            m.0.set(loc);
        }
        self.space.unmap_elem(w);
        if !self.ui.minimized.contains(w) {
            self.ui.minimized.push(w.clone());
        }
        // Fokus an das nächste sichtbare Fenster
        let next = self.space.elements().rev().find(|e| is_task_window(e)).cloned();
        let serial = SERIAL_COUNTER.next_serial();
        if let Some(kb) = self.seat.get_keyboard() {
            kb.set_focus(self, next.map(Into::into), serial);
        }
    }

    /// Aktives Fenster: Tastaturfokus, sonst das aktivierte oberste Fenster.
    pub fn focused_window(&self) -> Option<WindowElement> {
        if let Some(KeyboardFocusTarget::Window(w)) = self.seat.get_keyboard().and_then(|k| k.current_focus()) {
            if let Some(e) = self.space.elements().find(|e| e.0 == w) {
                return Some(e.clone());
            }
        }
        self.space.elements().rev().find(|e| is_task_window(e) && is_activated(e)).cloned()
    }

    /// Alt+Tab: nächstes Fenster der Leiste nach vorn holen (auch minimierte).
    pub fn cycle_windows(&mut self) {
        let wins = self.ui_windows();
        if wins.is_empty() {
            return;
        }
        let cur = wins.iter().position(|w| w.active).unwrap_or(wins.len() - 1);
        let next = wins[(cur + 1) % wins.len()].elem.clone();
        self.focus_window(&next);
    }

    /// Fenster auf eine Fläche setzen (Andocken, Maximieren), für Wayland- und X11-Fenster.
    pub fn place_window(&mut self, w: &WindowElement, rect: Rectangle<i32, Logical>, maximized: bool) {
        // erst einordnen (aktiviert), dann die Größe senden – so kommt beides in derselben Konfiguration an
        self.space.map_element(w.clone(), rect.loc, true);
        match w.0.underlying_surface() {
            WindowSurface::Wayland(t) => {
                let mut size = rect.size;
                if w.decoration_state().is_ssd {
                    size.h -= crate::shell::ssd::HEADER_BAR_HEIGHT;
                }
                t.with_pending_state(|st| {
                    if maximized {
                        st.states.set(xdg_toplevel::State::Maximized);
                    } else {
                        st.states.unset(xdg_toplevel::State::Maximized);
                    }
                    st.size = Some(size);
                });
                if t.is_initial_configure_sent() {
                    t.send_pending_configure();
                }
            }
            #[cfg(feature = "xwayland")]
            WindowSurface::X11(x) => {
                let _ = x.set_maximized(maximized);
                let mut r = rect;
                if w.decoration_state().is_ssd {
                    r.loc.y += crate::shell::ssd::HEADER_BAR_HEIGHT;
                    r.size.h -= crate::shell::ssd::HEADER_BAR_HEIGHT;
                }
                let _ = x.configure(r);
            }
        }
    }

    fn output_for(&self, w: &WindowElement) -> Option<Output> {
        self.space
            .outputs_for_element(w)
            .first()
            .cloned()
            .or_else(|| self.space.outputs().next().cloned())
    }

    /// Super+Pfeil: 0 = links, 1 = rechts, 2 = maximieren/wiederherstellen, 3 = minimieren.
    pub fn snap_focused(&mut self, how: u8) {
        let Some(w) = self.focused_window() else { return };
        self.snap_window(&w, how);
    }

    pub fn snap_window(&mut self, w: &WindowElement, how: u8) {
        let w = w.clone();
        let Some(out) = self.output_for(&w) else { return };
        let area = usable_area(&self.space, &out);
        let half = Size::from((area.size.w / 2, area.size.h));
        match how {
            0 => self.place_window(&w, Rectangle::new(area.loc, half), false),
            1 => self.place_window(&w, Rectangle::new(area.loc + Point::from((area.size.w - half.w, 0)), half), false),
            2 => {
                let is_max = match w.0.underlying_surface() {
                    WindowSurface::Wayland(t) => t.current_state().states.contains(xdg_toplevel::State::Maximized),
                    #[cfg(feature = "xwayland")]
                    WindowSurface::X11(x) => x.is_maximized(),
                };
                if is_max {
                    let r = Rectangle::new(
                        area.loc + Point::from((area.size.w / 8, area.size.h / 10)),
                        (area.size.w * 3 / 4, area.size.h * 4 / 5).into(),
                    );
                    self.place_window(&w, r, false);
                } else {
                    self.place_window(&w, area, true);
                }
            }
            _ => self.minimize_window(&w),
        }
    }

    pub fn close_focused(&mut self) {
        let Some(w) = self.focused_window() else { return };
        match w.0.underlying_surface() {
            WindowSurface::Wayland(t) => t.send_close(),
            #[cfg(feature = "xwayland")]
            WindowSurface::X11(x) => {
                let _ = x.close();
            }
        }
    }

    /// Tablet-Modus: alle Fenster füllen den Bildschirm.
    pub fn maximize_all(&mut self) {
        let wins: Vec<WindowElement> = self.space.elements().filter(|w| is_task_window(w)).cloned().collect();
        for w in wins {
            if let Some(out) = self.output_for(&w) {
                let area = usable_area(&self.space, &out);
                self.place_window(&w, area, true);
            }
        }
    }

    /// Programme aus der Einstellung `autostart` starten (einmal je Sitzung).
    pub fn run_autostart_once(&mut self) {
        if self.ui.autostarted {
            return;
        }
        self.ui.autostarted = true;
        for cmd in self.ui.cfg.autostart.clone() {
            self.spawn_shell(&cmd);
        }
    }

    pub fn terminal_cmd(&self) -> String {
        self.ui.cfg.terminal.clone()
    }
}
