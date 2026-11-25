//! Galerie: Fotos und Videos nach Datum, Alben, Favoriten, Mehrfachauswahl und ein Vollbild-Betrachter
//! (wischen, doppelt tippen = vergrößern, drehen, als Hintergrund setzen, Details, löschen, Video abspielen).
//! Untermodul von `app`.
//!
//! Vorschaubilder entstehen in einem Hintergrund-Thread und liegen danach als kleine JPEGs unter
//! `<daten>/cache/thumbs` – die Oberfläche bleibt beim Blättern flüssig, auch bei großen Fotos.

use super::tools::{decode, is_image, Pic};
use super::{App, Screen};
use crate::gfx::{rgb, Rect};
use crate::mjpeg;
use crate::sys;
use crate::ui::{Al, Ic, Ui};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Wurzel des Nutzerbereichs (auf dem Gerät /data; für Tests und Probeläufe über ULTAR_MEDIA umlenkbar).
pub fn media_root() -> String {
    std::env::var("ULTAR_MEDIA").unwrap_or_else(|_| "/data".into())
}

/// Ordner für Aufnahmen der Kamera.
pub fn camera_dir() -> String {
    format!("{}/DCIM/Camera", media_root())
}

const MEDIA_DIRS: [&str; 5] = ["DCIM", "Pictures", "media", "Download", "ultar/media"];
const THUMB: u32 = 320;
#[allow(dead_code)] // not used by anything yet; kept, see docs/OFFEN-WARNUNGEN.md
pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

#[derive(Clone, PartialEq)]
pub struct Item {
    pub path: String,
    pub mtime: i64,
    pub size: u64,
    pub video: bool,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct GalCfg {
    pub favorites: Vec<String>,
}

struct Loader {
    tx: Sender<Item>,
    out: Arc<Mutex<Vec<(String, Option<Pic>)>>>,
}

/// Laufende Video-Wiedergabe im Betrachter
pub struct Player {
    rd: mjpeg::Reader,
    /// Wiedergabe läuft seit (Zeitpunkt, Bild beim Start)
    since: Option<(Instant, usize)>,
    cur: usize,
    shown: Option<usize>,
    frame: Option<Pic>,
}

#[derive(Default)]
pub struct Gal {
    pub items: Vec<Item>,
    pub scanned: bool,
    /// 0 Fotos (alles nach Datum), 1 Alben, 2 Favoriten, 3 Videos
    pub tab: usize,
    pub album: Option<String>,
    thumbs: HashMap<String, Option<Pic>>,
    asked: HashSet<String>,
    loader: Option<Loader>,
    pub cfg: GalCfg,
    cfg_loaded: bool,
    pub select: Option<HashSet<String>>,
    // Betrachter
    list: Vec<String>,
    full: Option<(String, Result<Pic, String>)>,
    chrome_off: bool,
    zoom: f32,
    pan: (f32, f32),
    last_tap: Option<Instant>,
    info: bool,
    del_armed: bool,
    player: Option<(String, Player)>,
}

fn fnv(s: &str, a: i64, b: u64) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for x in s.bytes().chain(a.to_le_bytes()).chain(b.to_le_bytes()) {
        h = (h ^ x as u64).wrapping_mul(0x100000001b3);
    }
    h
}

fn scan(dir: &str, depth: u8, out: &mut Vec<Item>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let s = p.to_string_lossy().into_owned();
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            if depth > 0
                && !s.ends_with("/cache")
                && !e.file_name().to_string_lossy().starts_with('.')
            {
                scan(&s, depth - 1, out);
            }
        } else if is_image(&s) || mjpeg::is_video(&s) {
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            out.push(Item {
                video: mjpeg::is_video(&s),
                path: s,
                mtime,
                size: md.len(),
            });
        }
    }
}

fn shrink(p: Pic, max: u32) -> Pic {
    if p.w <= max && p.h <= max {
        return p;
    }
    let Some(im) = image::RgbaImage::from_raw(p.w, p.h, p.rgba) else {
        return Pic {
            w: 1,
            h: 1,
            rgba: vec![0, 0, 0, 255],
        };
    };
    let t = image::DynamicImage::ImageRgba8(im)
        .thumbnail(max, max)
        .to_rgba8();
    Pic {
        w: t.width(),
        h: t.height(),
        rgba: t.into_raw(),
    }
}

fn save_jpeg(p: &Pic, path: &str, q: u8) -> Result<(), String> {
    let rgb: Vec<u8> = p
        .rgba
        .chunks_exact(4)
        .flat_map(|c| [c[0], c[1], c[2]])
        .collect();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q)
        .encode(&rgb, p.w, p.h, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, out)
        .and_then(|_| std::fs::rename(&tmp, path))
        .map_err(|e| e.to_string())
}

fn make_thumb(it: &Item, cache_dir: &str) -> Option<Pic> {
    let cache = format!("{cache_dir}/{:016x}.jpg", fnv(&it.path, it.mtime, it.size));
    if let Ok(p) = decode(&cache, THUMB * 2) {
        return Some(p);
    }
    let pic = if it.video {
        let (w, h, rgba) = mjpeg::Reader::open(&it.path).ok()?.frame(0)?;
        shrink(Pic { w, h, rgba }, THUMB)
    } else {
        decode(&it.path, THUMB).ok()?
    };
    let _ = std::fs::create_dir_all(cache_dir);
    let _ = save_jpeg(&pic, &cache, 82);
    Some(pic)
}

/// Schreiben/Löschen ist nur im Nutzerbereich erlaubt.
fn user_file(p: &str) -> bool {
    p.starts_with(&format!("{}/", media_root())) && !p.contains("/../")
}

fn album_of(path: &str) -> String {
    std::path::Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn base(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Gal {
    pub fn rescan(&mut self) {
        self.scanned = false;
    }

    fn ensure(&mut self) {
        if !self.cfg_loaded {
            self.cfg = sys::load("gallery.toml");
            self.cfg_loaded = true;
        }
        if self.scanned {
            return;
        }
        let mut v = Vec::new();
        let root = media_root();
        for d in MEDIA_DIRS {
            scan(&format!("{root}/{d}"), 4, &mut v);
        }
        v.sort_by(|a, b| b.mtime.cmp(&a.mtime).then(b.path.cmp(&a.path)));
        v.dedup_by(|a, b| a.path == b.path);
        v.truncate(5000);
        self.items = v;
        self.scanned = true;
        self.asked.clear();
    }

    /// Neuestes Bild (für die Vorschau in der Kamera).
    pub fn newest(&mut self) -> Option<Item> {
        self.ensure();
        self.items.first().cloned()
    }

    fn is_fav(&self, p: &str) -> bool {
        self.cfg.favorites.iter().any(|f| f == p)
    }

    fn filtered(&self) -> Vec<Item> {
        self.items
            .iter()
            .filter(|i| match (&self.album, self.tab) {
                (Some(a), _) => &album_of(&i.path) == a,
                (None, 2) => self.is_fav(&i.path),
                (None, 3) => i.video,
                _ => true,
            })
            .cloned()
            .collect()
    }

    /// Vorschaubild holen; fehlt es, wird es im Hintergrund angefordert.
    fn thumb(&mut self, it: &Item, bg: &sys::Bg) -> Option<&Pic> {
        if !self.thumbs.contains_key(&it.path) && self.asked.insert(it.path.clone()) {
            let ld = self.loader.get_or_insert_with(|| {
                let (tx, rx) = channel::<Item>();
                let out: Arc<Mutex<Vec<(String, Option<Pic>)>>> = Arc::default();
                let (o2, bg2) = (out.clone(), bg.clone());
                let dir = format!("{}/cache/thumbs", sys::data_dir());
                let _ = std::thread::Builder::new()
                    .name("ultar-thumbs".into())
                    .spawn(move || {
                        while let Ok(it) = rx.recv() {
                            let p = make_thumb(&it, &dir);
                            if let Ok(mut o) = o2.lock() {
                                o.push((it.path, p));
                            }
                            bg2.poke();
                        }
                    });
                Loader { tx, out }
            });
            let _ = ld.tx.send(it.clone());
        }
        self.thumbs.get(&it.path).and_then(|p| p.as_ref())
    }

    /// Fertige Vorschaubilder übernehmen; true = es kam etwas Neues.
    fn collect(&mut self) -> bool {
        let Some(ld) = &self.loader else { return false };
        let got: Vec<(String, Option<Pic>)> = ld
            .out
            .lock()
            .map(|mut o| o.drain(..).collect())
            .unwrap_or_default();
        let any = !got.is_empty();
        for (p, t) in got {
            self.thumbs.insert(p, t);
        }
        any
    }

    pub fn pending(&self) -> bool {
        self.asked.len() > self.thumbs.len()
    }

    fn forget(&mut self, path: &str) {
        self.thumbs.remove(path);
        self.asked.remove(path);
        if self.full.as_ref().is_some_and(|f| f.0 == path) {
            self.full = None;
        }
    }
}

/// Bild so zeichnen, dass es die Zelle füllt (mittig zugeschnitten).
fn cover(ui: &mut Ui, t: &Pic, cell: Rect) {
    let k = (cell.w / t.w as f32).max(cell.h / t.h as f32);
    let (w, h) = (t.w as f32 * k, t.h as f32 * k);
    let old = ui.clip;
    let clip = match old {
        Some(o) => {
            let (x0, y0) = (cell.x.max(o.x), cell.y.max(o.y));
            Rect::new(
                x0,
                y0,
                (cell.right().min(o.right()) - x0).max(0.0),
                (cell.bottom().min(o.bottom()) - y0).max(0.0),
            )
        }
        None => cell,
    };
    ui.set_clip(Some(clip));
    ui.cv.blit_rgba(
        &t.rgba,
        t.w,
        t.h,
        Rect::new(cell.cx() - w / 2.0, cell.cy() - h / 2.0, w, h),
    );
    ui.set_clip(old);
}

fn fmt_dur(ms: u64) -> String {
    format!("{}:{:02}", ms / 60000, (ms / 1000) % 60)
}

impl App {
    fn date_label(&self, mtime: i64) -> String {
        let tz = self.set.tz_min as i64 * 60;
        let day = (mtime + tz).div_euclid(86400);
        let today = (sys::unix_now() + tz).div_euclid(86400);
        if day == today {
            return self.t("Today").into();
        }
        if day == today - 1 {
            return self.t("Yesterday").into();
        }
        // wie in der Demo: Heute, Gestern, Diese Woche, Früher
        if day > today - 8 {
            return self.t("This week").into();
        }
        self.t("Earlier").into()
    }

    pub(super) fn gallery_pause(&mut self) {
        if let Some((_, pl)) = &mut self.gal.player {
            pl.since = None;
        }
    }

    pub(super) fn gallery_needs_tick(&self) -> bool {
        match self.stack.last().map(|s| &s.0) {
            Some(Screen::Gallery) => self.gal.pending(),
            Some(Screen::Photo(_)) => self
                .gal
                .player
                .as_ref()
                .is_some_and(|p| p.1.since.is_some()),
            _ => false,
        }
    }

    pub(super) fn s_gallery(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        self.gal.ensure();
        if self.gal.collect() {
            ui.dirty = true;
        }
        let m = ui.dp(16.0);
        let (c, cs) = (ui.th.text, ui.th.sub);
        // Kopf: Titel + Anzahl, rechts Kamera-Knopf; im Auswahlmodus Anzahl + Löschen + Abbrechen
        let hh = ui.dp(64.0);
        let head = Rect::new(m, area.y, ui.w - 2.0 * m, hh);
        let list = self.gal.filtered();
        let _ = cs;
        if let Some(sel) = self.gal.select.clone() {
            let s = format!("{} {}", sel.len(), self.t("selected"));
            ui.label(
                Rect::new(head.x, head.y, head.w - ui.dp(110.0), hh),
                &s,
                22.0,
                c,
                true,
                Al::Start,
            );
            let (bx1, bx2) = if ui.rtl {
                (head.x + ui.dp(22.0), head.x + ui.dp(72.0))
            } else {
                (head.right() - ui.dp(22.0), head.right() - ui.dp(72.0))
            };
            if ui.icon_btn(bx1, head.cy(), ui.dp(20.0), Ic::Close, ui.th.card, c) {
                self.gal.select = None;
                ui.dirty = true;
            }
            let dc = ui.th.danger;
            if ui.icon_btn(bx2, head.cy(), ui.dp(20.0), Ic::Trash, dc.alpha(0.18), dc)
                && !sel.is_empty()
            {
                if self.gal.del_armed {
                    let mut n = 0;
                    for p in &sel {
                        if user_file(p) && std::fs::remove_file(p).is_ok() {
                            n += 1;
                            self.gal.forget(p);
                            self.gal.cfg.favorites.retain(|f| f != p);
                        }
                    }
                    let _ = sys::save("gallery.toml", &self.gal.cfg);
                    self.gal.select = None;
                    self.gal.del_armed = false;
                    self.gal.rescan();
                    self.toast(format!("{n} {}", self.t("Deleted")));
                } else {
                    self.gal.del_armed = true;
                    self.toast(self.t("Tap again to delete").into());
                }
                ui.dirty = true;
            }
        }
        // Kopf wie in der Web-Demo: Zurück, Titel, Kamera-Knopf; darunter Reiter
        let mut y = area.y + hh + ui.dp(4.0);
        if self.gal.select.is_none() {
            let title = match &self.gal.album {
                Some(a) => base(a),
                None => self.t("Gallery").to_string(),
            };
            let a2 = self.header(ui, area, &title);
            if std::mem::take(&mut self.hdr_tap) {
                self.go(Screen::Camera);
                ui.dirty = true;
                return;
            }
            y = a2.y;
            let tabs = [
                self.t("Photos"),
                self.t("Albums"),
                self.t("Favorites"),
                self.t("Videos"),
            ];
            let sel = if self.gal.album.is_none() {
                self.gal.tab
            } else {
                usize::MAX
            };
            if let Some(i) = ui.chips(&mut y, &tabs, sel) {
                self.gal.tab = i;
                self.gal.album = None;
                *sc = 0.0;
                ui.dirty = true;
            }
        }
        let body = Rect::new(area.x, y, area.w, (area.bottom() - y).max(0.0));

        if self.gal.items.is_empty() {
            ui.label(
                Rect::new(m, body.y + ui.dp(40.0), ui.w - 2.0 * m, ui.dp(24.0)),
                self.t("Nothing here"),
                14.0,
                cs,
                false,
                Al::Center,
            );
            return;
        }

        ui.scroll_begin(body, sc);
        if self.gal.tab == 1 && self.gal.album.is_none() {
            self.gallery_albums(ui, body, sc);
            return;
        }
        if list.is_empty() {
            let msg = self.t(if self.gal.tab == 2 {
                "No favorites yet. Tap the heart on a photo."
            } else {
                "Nothing here yet"
            });
            ui.label(
                Rect::new(m, body.y + ui.dp(40.0), ui.w - 2.0 * m, ui.dp(24.0)),
                msg,
                14.0,
                cs,
                false,
                Al::Center,
            );
            ui.scroll_end(body, 0.0, sc);
            return;
        }
        // Raster wie in der Demo: 3 Spalten (breite Bildschirme mehr), 3 dp Abstand, Ränder 16 dp
        let cols = ((ui.w / ui.dp(130.0)) as usize).clamp(3, 9);
        let gap = ui.dp(3.0);
        let side = ui.dp(16.0);
        let cw = (ui.w - 2.0 * side - gap * (cols - 1) as f32) / cols as f32;
        let mut cy = body.y - *sc;
        let mut i = 0;
        let bgc = ui.th.bg.mix(ui.th.bg2, 0.5);
        let mut open: Option<String> = None;
        while i < list.len() {
            // Abschnitt: alle Einträge mit derselben Datumsüberschrift
            let label = self.date_label(list[i].mtime);
            let mut j = i;
            while j < list.len() && self.date_label(list[j].mtime) == label {
                j += 1;
            }
            let hr = Rect::new(
                side + ui.dp(8.0),
                cy + ui.dp(8.0),
                ui.w - 2.0 * side - ui.dp(16.0),
                ui.dp(20.0),
            );
            ui.label(hr, &label.to_uppercase(), 12.0, cs, false, Al::Start);
            cy += ui.dp(34.0);
            for (k, it) in list[i..j].iter().enumerate() {
                let col = k % cols;
                let col = if ui.rtl { cols - 1 - col } else { col };
                let cell = Rect::new(
                    side + col as f32 * (cw + gap),
                    cy + (k / cols) as f32 * (cw + gap),
                    cw,
                    cw,
                );
                if !ui.visible(cell) {
                    continue;
                }
                let bgref = self.bg.clone();
                match self.gal.thumb(it, &bgref) {
                    Some(t) => {
                        cover(ui, t, cell);
                        ui.cv.round_corners(cell, ui.dp(4.0), bgc);
                    }
                    None => ui.cv.rrect(cell, ui.dp(4.0), ui.th.card),
                }
                if it.video {
                    ui.cv
                        .circle(cell.cx(), cell.cy(), ui.dp(17.0), rgb(0).alpha(0.5));
                    ui.icon(Ic::Play, cell.cx(), cell.cy(), ui.dp(17.0), rgb(0xFFFFFF));
                }
                if self.gal.is_fav(&it.path) {
                    let hx = if ui.rtl {
                        cell.x + ui.dp(12.0)
                    } else {
                        cell.right() - ui.dp(12.0)
                    };
                    ui.icon(
                        Ic::Heart,
                        hx,
                        cell.bottom() - ui.dp(11.0),
                        ui.dp(12.0),
                        rgb(0xFFFFFF),
                    );
                }
                if let Some(sel) = &self.gal.select {
                    let on = sel.contains(&it.path);
                    if on {
                        ui.cv.rrect(cell, ui.dp(7.0), ui.th.accent.alpha(0.32));
                    }
                    let (kx, ky) = (cell.x + ui.dp(17.0), cell.y + ui.dp(17.0));
                    ui.cv.circle(
                        kx,
                        ky,
                        ui.dp(11.0),
                        if on { ui.th.accent } else { rgb(0).alpha(0.4) },
                    );
                    ui.cv.ring(kx, ky, ui.dp(10.0), ui.dp(1.5), rgb(0xFFFFFF));
                    if on {
                        let oc = ui.th.on_accent;
                        ui.icon(Ic::Check, kx, ky, ui.dp(13.0), oc);
                    }
                }
                if self.long_press(ui, cell) && self.gal.select.is_none() {
                    self.gal.select = Some(HashSet::from([it.path.clone()]));
                    self.gal.del_armed = false;
                    ui.dirty = true;
                } else if ui.tap(cell) {
                    match &mut self.gal.select {
                        Some(sel) => {
                            if !sel.remove(&it.path) {
                                sel.insert(it.path.clone());
                            }
                            self.gal.del_armed = false;
                        }
                        None => open = Some(it.path.clone()),
                    }
                    ui.dirty = true;
                }
            }
            cy += (j - i).div_ceil(cols) as f32 * (cw + gap) + ui.dp(6.0);
            i = j;
        }
        ui.scroll_end(body, cy + *sc - body.y + ui.dp(12.0), sc);
        if let Some(p) = open {
            self.open_photo(p, list.iter().map(|i| i.path.clone()).collect());
        }
    }

    fn gallery_albums(&mut self, ui: &mut Ui, body: Rect, sc: &mut f32) {
        // Alben = Ordner; Titelbild ist das neueste Bild
        let mut albums: Vec<(String, Item, usize)> = Vec::new();
        for it in &self.gal.items {
            let a = album_of(&it.path);
            match albums.iter_mut().find(|x| x.0 == a) {
                Some(x) => x.2 += 1,
                None => albums.push((a, it.clone(), 1)),
            }
        }
        let cols = ((ui.w / ui.dp(170.0)) as usize).clamp(2, 6);
        let side = ui.dp(14.0);
        let gap = ui.dp(12.0);
        let cw = (ui.w - 2.0 * side - gap * (cols - 1) as f32) / cols as f32;
        let ch = cw + ui.dp(50.0);
        let (c, cs) = (ui.th.text, ui.th.sub);
        let bgc = ui.th.bg.mix(ui.th.bg2, 0.5);
        for (k, (dir, cover_it, n)) in albums.iter().enumerate() {
            let col = k % cols;
            let col = if ui.rtl { cols - 1 - col } else { col };
            let cell = Rect::new(
                side + col as f32 * (cw + gap),
                body.y - *sc + ui.dp(4.0) + (k / cols) as f32 * (ch + gap),
                cw,
                ch,
            );
            if !ui.visible(cell) {
                continue;
            }
            let pic = Rect::new(cell.x, cell.y, cw, cw);
            let bgref = self.bg.clone();
            match self.gal.thumb(cover_it, &bgref) {
                Some(t) => {
                    cover(ui, t, pic);
                    ui.cv.round_corners(pic, ui.dp(16.0), bgc);
                }
                None => ui.cv.rrect(pic, ui.dp(16.0), ui.th.card),
            }
            let name = if *dir == camera_dir() {
                self.t("Camera").to_string()
            } else {
                base(dir)
            };
            ui.label(
                Rect::new(
                    cell.x + ui.dp(4.0),
                    pic.bottom() + ui.dp(6.0),
                    cw - ui.dp(8.0),
                    ui.dp(22.0),
                ),
                &name,
                14.5,
                c,
                true,
                Al::Start,
            );
            ui.label(
                Rect::new(
                    cell.x + ui.dp(4.0),
                    pic.bottom() + ui.dp(27.0),
                    cw - ui.dp(8.0),
                    ui.dp(18.0),
                ),
                &n.to_string(),
                12.0,
                cs,
                false,
                Al::Start,
            );
            if ui.tap(cell) {
                self.gal.album = Some(dir.clone());
                *sc = 0.0;
                ui.dirty = true;
            }
        }
        let rows = albums.len().div_ceil(cols);
        ui.scroll_end(body, rows as f32 * (ch + gap) + ui.dp(12.0), sc);
    }

    /// Betrachter für `path` öffnen; `list` ist die Reihenfolge zum Blättern.
    pub(super) fn open_photo(&mut self, path: String, list: Vec<String>) {
        self.gal.list = if list.is_empty() {
            vec![path.clone()]
        } else {
            list
        };
        self.gal.zoom = 1.0;
        self.gal.pan = (0.0, 0.0);
        self.gal.info = false;
        self.gal.del_armed = false;
        self.gal.chrome_off = false;
        self.gal.player = None;
        self.go(Screen::Photo(path));
    }

    /// „Zurück" in der Galerie: erst Auswahl bzw. Album schließen. true = verbraucht.
    pub(super) fn gallery_back(&mut self) -> bool {
        if !matches!(self.stack.last().map(|s| &s.0), Some(Screen::Gallery)) {
            return false;
        }
        if self.gal.select.take().is_some() {
            return true;
        }
        self.gal.album.take().is_some()
    }

    fn photo_step(&mut self, path: &str, d: i32) {
        let Some(i) = self.gal.list.iter().position(|p| p == path) else {
            return;
        };
        let j = i as i32 + d;
        if j < 0 || j >= self.gal.list.len() as i32 {
            return;
        }
        let next = self.gal.list[j as usize].clone();
        self.gal.zoom = 1.0;
        self.gal.pan = (0.0, 0.0);
        self.gal.del_armed = false;
        self.gal.player = None;
        if let Some(l) = self.stack.last_mut() {
            l.0 = Screen::Photo(next);
        }
    }

    pub(super) fn s_photo(&mut self, ui: &mut Ui, full: Rect, path: &str) {
        self.gal.ensure();
        let white = rgb(0xFFFFFF);
        ui.cv.rect(Rect::new(0.0, 0.0, ui.w, ui.h), rgb(0));
        let video = mjpeg::is_video(path);
        let view = full;
        let mut pic_rect = view;

        if video {
            if self.gal.player.as_ref().map(|p| p.0.as_str()) != Some(path) {
                self.gal.player = mjpeg::Reader::open(path).ok().map(|rd| {
                    (
                        path.to_string(),
                        Player {
                            rd,
                            since: None,
                            cur: 0,
                            shown: None,
                            frame: None,
                        },
                    )
                });
            }
            if let Some((_, pl)) = &mut self.gal.player {
                let n = pl.rd.index.len();
                if let Some((t0, f0)) = pl.since {
                    let f = f0 + (t0.elapsed().as_micros() / pl.rd.usec.max(1) as u128) as usize;
                    if f >= n {
                        pl.since = None;
                        pl.cur = 0;
                    } else {
                        pl.cur = f;
                    }
                    ui.dirty = true;
                }
                if pl.shown != Some(pl.cur) {
                    if let Some((w, h, rgba)) = pl.rd.frame(pl.cur) {
                        pl.frame = Some(Pic { w, h, rgba });
                    }
                    pl.shown = Some(pl.cur);
                }
                if let Some(p) = &pl.frame {
                    let k = (view.w / p.w as f32).min(view.h / p.h as f32);
                    let (w, h) = (p.w as f32 * k, p.h as f32 * k);
                    pic_rect = Rect::new(view.cx() - w / 2.0, view.cy() - h / 2.0, w, h);
                    ui.cv.blit_rgba(&p.rgba, p.w, p.h, pic_rect);
                }
            } else {
                ui.label(
                    view,
                    self.t("Cannot play this video"),
                    15.0,
                    white,
                    false,
                    Al::Center,
                );
            }
        } else {
            if self.gal.full.as_ref().map(|v| v.0.as_str()) != Some(path) {
                let max = (ui.w.max(ui.h) as u32 * 2).clamp(960, 4096);
                self.gal.full = Some((path.to_string(), decode(path, max)));
            }
            match &self.gal.full.as_ref().unwrap().1 {
                Ok(p) => {
                    let k = (view.w / p.w as f32).min(view.h / p.h as f32) * self.gal.zoom;
                    let (w, h) = (p.w as f32 * k, p.h as f32 * k);
                    // Verschieben begrenzen, damit das Bild nicht aus dem Blick rutscht
                    let (mx, my) = (((w - view.w) / 2.0).max(0.0), ((h - view.h) / 2.0).max(0.0));
                    self.gal.pan = (self.gal.pan.0.clamp(-mx, mx), self.gal.pan.1.clamp(-my, my));
                    pic_rect = Rect::new(
                        view.cx() - w / 2.0 + self.gal.pan.0,
                        view.cy() - h / 2.0 + self.gal.pan.1,
                        w,
                        h,
                    );
                    ui.cv.set_clip(Some(view));
                    ui.cv.blit_rgba(&p.rgba, p.w, p.h, pic_rect);
                    ui.cv.set_clip(None);
                }
                Err(e) => {
                    let e = e.clone();
                    ui.label(view, &e, 15.0, white, false, Al::Center);
                }
            }
        }
        let _ = pic_rect;

        let chrome = !self.gal.chrome_off;
        let top_h = ui.dp(64.0);
        let bot_h = ui.dp(84.0);
        let top = Rect::new(0.0, full.y, ui.w, top_h);
        let bot = Rect::new(0.0, full.bottom() - bot_h, ui.w, bot_h);
        let mut handled = false;
        let idx = self.gal.list.iter().position(|p| p == path);

        if chrome {
            ui.cv.veil(
                Rect::new(0.0, 0.0, ui.w, top.bottom() + ui.dp(30.0)),
                rgb(0).alpha(0.72),
                rgb(0).alpha(0.0),
            );
            ui.cv.veil(
                Rect::new(0.0, bot.y - ui.dp(30.0), ui.w, bot_h + ui.dp(30.0)),
                rgb(0).alpha(0.0),
                rgb(0).alpha(0.78),
            );
            // oben: zurück, Datum/Name, Zähler
            let bx = if ui.rtl {
                ui.w - ui.dp(30.0)
            } else {
                ui.dp(30.0)
            };
            if ui.icon_btn(
                bx,
                top.cy(),
                ui.dp(19.0),
                Ic::Back,
                rgb(0xFFFFFF).alpha(0.14),
                white,
            ) {
                self.back();
                ui.dirty = true;
                return;
            }
            let it = self.gal.items.iter().find(|i| i.path == path).cloned();
            let title = it
                .as_ref()
                .map(|i| self.date_label(i.mtime))
                .unwrap_or_else(|| base(path));
            let sub = it
                .as_ref()
                .map(|i| {
                    let d = sys::from_unix(i.mtime, self.set.tz_min);
                    format!("{:02}.{:02}.{} · {:02}:{:02}", d.d, d.mo, d.y, d.h, d.mi)
                })
                .unwrap_or_default();
            let tr = Rect::new(
                ui.dp(62.0),
                top.y + ui.dp(10.0),
                ui.w - ui.dp(124.0),
                ui.dp(24.0),
            );
            ui.label(tr, &title, 16.0, white, true, Al::Center);
            ui.label(
                Rect::new(tr.x, tr.bottom(), tr.w, ui.dp(20.0)),
                &sub,
                12.0,
                white.alpha(0.7),
                false,
                Al::Center,
            );
            if let Some(i) = idx {
                let s = format!("{}/{}", i + 1, self.gal.list.len());
                let old = ui.rtl;
                ui.rtl = false;
                let r = if old {
                    Rect::new(ui.dp(14.0), top.y, ui.dp(60.0), top_h)
                } else {
                    Rect::new(ui.w - ui.dp(74.0), top.y, ui.dp(60.0), top_h)
                };
                ui.label(
                    r,
                    &s,
                    12.5,
                    white.alpha(0.75),
                    false,
                    if old { Al::Start } else { Al::End },
                );
                ui.rtl = old;
            }

            // unten: Aktionen
            let fav = self.gal.is_fav(path);
            let mut acts: Vec<(Ic, u8)> = vec![(Ic::Heart, 0)];
            if !video && user_file(path) {
                acts.push((Ic::Rotate, 1));
            }
            if !video {
                acts.push((Ic::Wall, 2));
            }
            acts.push((Ic::Info, 3));
            if user_file(path) {
                acts.push((Ic::Trash, 4));
            }
            let step = ui.w / acts.len() as f32;
            let acc = ui.th.accent;
            let dng = ui.th.danger;
            for (k, (ic, id)) in acts.iter().enumerate() {
                let k = if ui.rtl { acts.len() - 1 - k } else { k };
                let cx = step * (k as f32 + 0.5);
                let col = match id {
                    0 if fav => acc,
                    4 if self.gal.del_armed => dng,
                    3 if self.gal.info => acc,
                    _ => white,
                };
                if ui.icon_btn(
                    cx,
                    bot.cy() - ui.dp(4.0),
                    ui.dp(21.0),
                    *ic,
                    rgb(0xFFFFFF).alpha(0.12),
                    col,
                ) {
                    handled = true;
                    ui.dirty = true;
                    match id {
                        0 => {
                            if fav {
                                self.gal.cfg.favorites.retain(|f| f != path);
                            } else {
                                self.gal.cfg.favorites.push(path.to_string());
                            }
                            let _ = sys::save("gallery.toml", &self.gal.cfg);
                        }
                        1 => {
                            let res = image::open(path)
                                .map_err(|e| e.to_string())
                                .and_then(|im| im.rotate90().save(path).map_err(|e| e.to_string()));
                            match res {
                                Ok(()) => {
                                    self.gal.forget(path);
                                    self.gal.rescan();
                                }
                                Err(e) => self.toast(e),
                            }
                        }
                        2 => {
                            let res =
                                decode(path, (ui.w.max(ui.h) as u32).max(720)).and_then(|p| {
                                    save_jpeg(&p, &format!("{}/wallpaper.jpg", sys::data_dir()), 88)
                                });
                            self.wall = None;
                            self.toast(match res {
                                Ok(()) => self.t("Wallpaper set").to_string(),
                                Err(e) => e,
                            });
                        }
                        3 => self.gal.info = !self.gal.info,
                        _ => {
                            if self.gal.del_armed {
                                let msg = match std::fs::remove_file(path) {
                                    Ok(()) => self.t("Deleted").to_string(),
                                    Err(e) => e.to_string(),
                                };
                                self.gal.forget(path);
                                self.gal.cfg.favorites.retain(|f| f != path);
                                let _ = sys::save("gallery.toml", &self.gal.cfg);
                                self.gal.rescan();
                                self.tl.files_for.clear();
                                let i = idx.unwrap_or(0);
                                self.gal.list.retain(|p| p != path);
                                self.gal.del_armed = false;
                                self.gal.player = None;
                                match self
                                    .gal
                                    .list
                                    .get(i)
                                    .or_else(|| self.gal.list.last())
                                    .cloned()
                                {
                                    Some(n) => {
                                        if let Some(l) = self.stack.last_mut() {
                                            l.0 = Screen::Photo(n);
                                        }
                                    }
                                    None => self.back(),
                                }
                                self.toast(msg);
                                return;
                            } else {
                                self.gal.del_armed = true;
                                self.toast(self.t("Tap again to delete").into());
                            }
                        }
                    }
                }
            }
        }

        // Video: Abspielknopf in der Mitte + Fortschritt
        if video {
            if let Some((_, pl)) = &mut self.gal.player {
                let n = pl.rd.index.len().max(1);
                let playing = pl.since.is_some();
                if chrome || !playing {
                    let pr = ui.dp(34.0);
                    if ui.icon_btn(
                        view.cx(),
                        view.cy(),
                        pr,
                        if playing { Ic::Pause } else { Ic::Play },
                        rgb(0).alpha(0.55),
                        white,
                    ) {
                        pl.since = if playing {
                            None
                        } else {
                            Some((Instant::now(), if pl.cur + 1 >= n { 0 } else { pl.cur }))
                        };
                        handled = true;
                        ui.dirty = true;
                    }
                }
                if chrome {
                    let bar = Rect::new(
                        ui.dp(22.0),
                        bot.y - ui.dp(18.0),
                        ui.w - ui.dp(44.0),
                        ui.dp(4.0),
                    );
                    ui.cv.rrect(bar, ui.dp(2.0), white.alpha(0.25));
                    ui.cv.rrect(
                        Rect::new(bar.x, bar.y, bar.w * (pl.cur as f32 / n as f32), bar.h),
                        ui.dp(2.0),
                        ui.th.accent,
                    );
                    let t = format!(
                        "{} / {}",
                        fmt_dur(pl.cur as u64 * pl.rd.usec as u64 / 1000),
                        fmt_dur(pl.rd.duration_ms())
                    );
                    let old = ui.rtl;
                    ui.rtl = false;
                    ui.label(
                        Rect::new(bar.x, bar.y - ui.dp(24.0), bar.w, ui.dp(20.0)),
                        &t,
                        12.0,
                        white.alpha(0.8),
                        false,
                        Al::Start,
                    );
                    ui.rtl = old;
                }
            }
        }

        // Details als Karte über der unteren Leiste
        if self.gal.info {
            let it = self.gal.items.iter().find(|i| i.path == path).cloned();
            let dims = if video {
                self.gal.player.as_ref().map(|p| (p.1.rd.w, p.1.rd.h))
            } else {
                image::image_dimensions(path).ok()
            };
            let mut rows: Vec<(String, String)> = vec![
                (self.t("Name").into(), base(path)),
                (self.t("Folder").into(), album_of(path)),
            ];
            if let Some(i) = &it {
                rows.push((self.t("Size").into(), sys::human(i.size)));
            }
            if let Some((w, h)) = dims {
                rows.push((self.t("Resolution").into(), format!("{w} × {h}")));
            }
            if let (true, Some(p)) = (video, &self.gal.player) {
                rows.push((self.t("Duration").into(), fmt_dur(p.1.rd.duration_ms())));
            }
            let rh = ui.dp(30.0);
            let w = (ui.w - ui.dp(28.0)).min(ui.dp(460.0));
            let r = Rect::new(
                (ui.w - w) / 2.0,
                bot.y - ui.dp(46.0) - rows.len() as f32 * rh - ui.dp(24.0),
                w,
                rows.len() as f32 * rh + ui.dp(24.0),
            );
            ui.cv.rrect(r, ui.dp(18.0), rgb(0x15171C).alpha(0.94));
            let mut y = r.y + ui.dp(12.0);
            for (k, v) in rows {
                let line = Rect::new(r.x + ui.dp(18.0), y, r.w - ui.dp(36.0), rh);
                ui.label(
                    Rect::new(line.x, line.y, line.w * 0.34, rh),
                    &k,
                    12.5,
                    white.alpha(0.6),
                    false,
                    Al::Start,
                );
                let v = ui.fit(&v, 13.5, false, line.w * 0.64);
                ui.label(line, &v, 13.5, white, false, Al::End);
                y += rh;
            }
        }

        // Gesten: ziehen = verschieben (vergrößert), wischen = blättern / schließen, tippen = Leisten, doppelt = Zoom
        if ui.enabled && ui.p.down && ui.p.moved && self.gal.zoom > 1.0 {
            self.gal.pan.0 += ui.p.dx;
            self.gal.pan.1 += ui.p.dy;
            ui.p.dx = 0.0;
            ui.p.dy = 0.0;
            ui.dirty = true;
        }
        if ui.enabled && ui.p.released && !handled {
            let (dx, dy) = (ui.p.x - ui.p.px, ui.p.y - ui.p.py);
            let far = ui.dp(70.0);
            let edge = ui.dp(24.0);
            let inner = ui.p.px > edge && ui.p.px < ui.w - edge;
            if ui.p.moved {
                if self.gal.zoom <= 1.0 && inner && dx.abs() > far && dx.abs() > dy.abs() * 1.4 {
                    self.photo_step(path, if dx < 0.0 { 1 } else { -1 });
                    ui.dirty = true;
                } else if self.gal.zoom <= 1.0
                    && dy > far * 1.6
                    && dy.abs() > dx.abs() * 1.5
                    && ui.p.py > ui.dp(60.0)
                {
                    self.back();
                    ui.dirty = true;
                }
            } else if view.contains(ui.p.x, ui.p.y)
                && !(chrome && (top.contains(ui.p.x, ui.p.y) || bot.contains(ui.p.x, ui.p.y)))
            {
                let dbl = self
                    .gal
                    .last_tap
                    .is_some_and(|t| t.elapsed().as_millis() < 320);
                if dbl && !video {
                    if self.gal.zoom > 1.0 {
                        self.gal.zoom = 1.0;
                        self.gal.pan = (0.0, 0.0);
                    } else {
                        self.gal.zoom = 2.5;
                        // auf den angetippten Punkt hin vergrößern
                        self.gal.pan = ((view.cx() - ui.p.x) * 1.5, (view.cy() - ui.p.y) * 1.5);
                    }
                    self.gal.chrome_off = true;
                    self.gal.last_tap = None;
                } else {
                    self.gal.chrome_off = !self.gal.chrome_off;
                    self.gal.last_tap = Some(Instant::now());
                }
                self.gal.info = false;
                ui.dirty = true;
            }
        }
    }
}
