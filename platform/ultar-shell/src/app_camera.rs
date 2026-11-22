//! Kamera-App: Sucher im Vollbild, Foto / Quadrat / Video, Selbstauslöser, Raster, Zoom, Licht,
//! Front-/Rückkamera, Bilddrehung und Vorschau der letzten Aufnahme (öffnet die Galerie).
//! Untermodul von `app`. Nutzt `crate::camera::CameraWorker` (v4l2 im Hintergrund-Thread).
//!
//! WICHTIG: Öffnen und Aufnahme laufen ausschließlich im Worker-Thread. Auf Exynos/fimc-is kann der
//! Kernel beim open()/S_FMT/STREAMON im request_firmware bis ~60 s blockieren, wenn die ISP-Firmware
//! fehlt (/vendor nicht gemountet). Die Oberfläche bleibt bedienbar und zeigt nach kurzer Zeit
//! „Kamera nicht verfügbar". Speichern (JPEG) und Video (Motion-JPEG) laufen ebenfalls in eigenen Threads.
//!
//! Aufnahmen landen in /data/DCIM/Camera als IMG_JJJJMMTT_HHMMSS.jpg bzw. VID_….avi.

use super::app_gallery::camera_dir;
use super::tools::{decode, Pic};
use super::App;
use crate::camera::{CamStatus, CameraWorker};
use crate::gfx::{rgb, Rect};
use crate::mjpeg;
use crate::scan;
use crate::sys;
use crate::ui::{Al, Ic, Ui};
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Frame = (u32, u32, Vec<u32>);
type Slot = Arc<Mutex<Option<Result<String, String>>>>;

/// Dauerhafte Kamera-Vorgaben
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct CamCfg {
    pub grid: bool,
    pub timer_s: u8,
    /// Bilddrehung in Vierteldrehungen je Kamera (Rück, Front) – Sensoren sind je Gerät anders eingebaut
    pub rot: [u8; 2],
    pub mirror_front: bool,
    /// Scanner: 0 Farbe, 1 Grau, 2 Schwarz-Weiß
    pub scan_filter: u8,
}

impl Default for CamCfg {
    fn default() -> Self {
        // Samsung-Telefone: Rücksensor um 90°, Frontsensor um 270° gedreht eingebaut
        let rot = if crate::camera::samsung_available() {
            [1, 3]
        } else {
            [0, 0]
        };
        Self {
            grid: false,
            timer_s: 0,
            rot,
            mirror_front: true,
            scan_filter: 0,
        }
    }
}

/// Aufgenommenes Dokument vor dem Speichern: Standbild und (verschiebbare) Ecken.
struct Review {
    frame: Frame,
    quad: scan::Quad,
    grab: Option<usize>,
    found: bool,
}

pub(super) struct Recorder {
    tx: SyncSender<Frame>,
    started: Instant,
}

#[derive(Default)]
pub struct CameraState {
    pub worker: Option<CameraWorker>,
    /// 0 = Rückkamera, 1 = Frontkamera
    pub facing: u8,
    pub err: Option<String>,
    /// Zuletzt empfangenes Bild, bereits gedreht
    pub last_frame: Option<Frame>,
    pub opened_at: Option<Instant>,
    /// 0 Foto, 1 Quadrat, 2 Video, 3 Scan (Dokumente)
    pub mode: u8,
    /// Scanner: zuletzt erkanntes Blatt (geglättet) und Zeitpunkt der letzten Suche
    /// Fernbedienung (Tür): beim nächsten fertigen Bild auslösen
    pub remote_shot: bool,
    /// Fernbedienung (Tür): offene Scan-Kontrolle übernehmen
    pub remote_accept: bool,
    /// Scanner: aufgenommenes Blatt zur Kontrolle (Ecken ziehen, dann übernehmen)
    review: Option<Review>,
    scan_quad: Option<scan::Quad>,
    scan_at: Option<Instant>,
    /// Scanner: bereits aufgenommene Seiten (JPEG, Breite, Höhe) für das nächste PDF
    scan_pages: Arc<Mutex<Vec<(Vec<u8>, u32, u32)>>>,
    pub zoom: f32,
    pub torch: bool,
    pub cfg: Option<CamCfg>,
    pub(super) countdown: Option<Instant>,
    flash_fx: Option<Instant>,
    saved: Slot,
    pub(super) rec: Option<Recorder>,
    thumb: Option<(String, Option<Pic>)>,
    pub(super) busy: bool,
}

fn rotate(f: Frame, quarter: u8) -> Frame {
    let (w, h, px) = f;
    let (wu, hu) = (w as usize, h as usize);
    match quarter % 4 {
        1 => {
            let mut o = vec![0u32; px.len()];
            for y in 0..hu {
                for x in 0..wu {
                    o[x * hu + (hu - 1 - y)] = px[y * wu + x];
                }
            }
            (h, w, o)
        }
        2 => (w, h, px.into_iter().rev().collect()),
        3 => {
            let mut o = vec![0u32; px.len()];
            for y in 0..hu {
                for x in 0..wu {
                    o[(wu - 1 - x) * hu + y] = px[y * wu + x];
                }
            }
            (h, w, o)
        }
        _ => (w, h, px),
    }
}

/// Probebild ohne Kamera (Gestaltungsprüfung auf dem Entwicklungsrechner, ULTAR_DRY).
fn demo_frame() -> Frame {
    let (w, h) = (480u32, 640u32);
    let mut px = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let sky = fy < 0.62;
            let (mut r, mut g, mut b) = if sky {
                (40.0 + 180.0 * fy, 90.0 + 120.0 * fy, 190.0 - 40.0 * fy)
            } else {
                (38.0 + 30.0 * fx, 70.0 - 40.0 * (fy - 0.62), 52.0)
            };
            let d = ((fx - 0.7).powi(2) + (fy - 0.28).powi(2) * 1.6).sqrt();
            if d < 0.09 {
                (r, g, b) = (255.0, 226.0, 150.0);
            }
            let hill = 0.62 - 0.12 * ((fx * 6.0).sin() * 0.5 + 0.5) * (1.0 - (fx - 0.3).abs());
            if fy > hill && sky {
                (r, g, b) = (30.0, 52.0, 60.0);
            }
            px.push((r as u32).min(255) << 16 | (g as u32).min(255) << 8 | (b as u32).min(255));
        }
    }
    (w, h, px)
}

fn stamp(tz_min: i32) -> String {
    let d = sys::from_unix(sys::unix_now(), tz_min);
    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        d.y, d.mo, d.d, d.h, d.mi, d.s
    )
}

fn unique(prefix: &str, tz_min: i32, ext: &str) -> String {
    let dir = camera_dir();
    let _ = std::fs::create_dir_all(&dir);
    let base = format!("{dir}/{prefix}_{}", stamp(tz_min));
    let mut p = format!("{base}.{ext}");
    let mut n = 1;
    while std::path::Path::new(&p).exists() {
        n += 1;
        p = format!("{base}_{n}.{ext}");
    }
    p
}

/// Bildausschnitt für Zielseitenverhältnis `aspect` (Breite/Höhe) und Zoom, mittig.
fn crop(w: u32, h: u32, aspect: f32, zoom: f32) -> Rect {
    let (wf, hf) = (w as f32, h as f32);
    let (mut cw, mut ch) = if wf / hf > aspect {
        (hf * aspect, hf)
    } else {
        (wf, wf / aspect)
    };
    cw /= zoom.max(1.0);
    ch /= zoom.max(1.0);
    Rect::new((wf - cw) / 2.0, (hf - ch) / 2.0, cw, ch)
}

impl App {
    fn cam_cfg(&mut self) -> &mut CamCfg {
        self.cam.cfg.get_or_insert_with(|| sys::load("camera.toml"))
    }

    fn cam_save(&mut self) {
        if let Some(c) = &self.cam.cfg {
            let _ = sys::save("camera.toml", c);
        }
    }

    pub(super) fn camera_animating(&self) -> bool {
        self.cam.worker.is_some()
            || self.cam.countdown.is_some()
            || self.cam.flash_fx.is_some()
            || self.cam.busy
    }

    /// Beim Verlassen der Kamera: alles freigeben (Sensor, Licht, laufende Aufnahme abschließen).
    pub(super) fn camera_leave(&mut self) {
        self.cam.review = None;
        self.cam.remote_shot = false;
        if self.cam.worker.is_none()
            && self.cam.rec.is_none()
            && !self.cam.torch
            && self.cam.last_frame.is_none()
        {
            return;
        }
        self.cam.rec = None; // Kanal schließt -> Recorder schreibt das Video fertig
        if let Some(w) = self.cam.worker.take() {
            w.stop();
        }
        if self.cam.torch {
            crate::quick::set_torch(false);
            self.cam.torch = false;
        }
        self.cam.countdown = None;
        self.cam.last_frame = None;
        self.cam.opened_at = None;
        self.cam.err = None;
    }

    pub(super) fn s_camera(&mut self, ui: &mut Ui, area: Rect) {
        let white = rgb(0xFFFFFF);
        let gold = ui.th.accent;
        ui.cv.rect(Rect::new(0.0, 0.0, ui.w, ui.h), rgb(0x050506));
        if !crate::privacy::allowed(&self.set, crate::privacy::Cap::Camera) {
            self.camera_leave();
            let r = Rect::new(
                ui.dp(24.0),
                area.cy() - ui.dp(70.0),
                ui.w - ui.dp(48.0),
                ui.dp(140.0),
            );
            ui.cv
                .circle(r.cx(), r.y + ui.dp(30.0), ui.dp(30.0), white.alpha(0.08));
            ui.icon(Ic::Shield, r.cx(), r.y + ui.dp(30.0), ui.dp(28.0), gold);
            ui.label(
                Rect::new(r.x, r.y + ui.dp(76.0), r.w, ui.dp(26.0)),
                self.t("Camera is switched off"),
                17.0,
                white,
                true,
                Al::Center,
            );
            ui.label(
                Rect::new(r.x, r.y + ui.dp(104.0), r.w, ui.dp(22.0)),
                self.t("Turned off in privacy settings"),
                13.0,
                white.alpha(0.6),
                false,
                Al::Center,
            );
            return;
        }
        let dry = std::env::var("ULTAR_DRY").is_ok();
        // Kamera startet von selbst
        if self.cam.worker.is_none() && self.cam.err.is_none() && !dry {
            self.start_camera();
        }
        if self.cam.zoom < 1.0 {
            self.cam.zoom = 1.0;
        }
        let cfg = self.cam_cfg().clone();
        let facing = self.cam.facing as usize & 1;

        // Neues Bild abholen (nicht blockierend), drehen, ggf. an die Videoaufnahme weiterreichen
        let mut fresh = false;
        if let Some(w) = &self.cam.worker {
            if let Some(f) = w.take_frame() {
                self.cam.last_frame = Some(rotate(f, cfg.rot[facing]));
                fresh = true;
            }
            if let Some(e) = w.error() {
                self.cam.err = Some(e);
            }
        } else if dry {
            // Probelauf ohne Kamera: Probebild; während einer Videoaufnahme zählt jedes Zeichnen als neues Bild
            if self.cam.last_frame.is_none() {
                self.cam.last_frame = Some(rotate(demo_frame(), cfg.rot[facing]));
            }
            fresh = self.cam.rec.is_some();
        }
        if self.cam.mode == 3 {
            // höchstens ~6x je Sekunde suchen; die Vorschau ist klein, das kostet wenige Millisekunden
            let due = self
                .cam
                .scan_at
                .map(|t| t.elapsed() > Duration::from_millis(160))
                .unwrap_or(true);
            if fresh && due {
                if let Some((w, h, px)) = &self.cam.last_frame {
                    self.cam.scan_quad =
                        scan::detect(px, *w, *h).map(|q| scan::smooth(self.cam.scan_quad, q));
                    self.cam.scan_at = Some(Instant::now());
                }
            }
        } else {
            self.cam.scan_quad = None;
        }
        if fresh {
            if let (Some(rec), Some(f)) = (&self.cam.rec, &self.cam.last_frame) {
                let _ = rec.tx.try_send(f.clone());
            }
        }
        // Ergebnis des Speicherns einsammeln
        let done = self.cam.saved.lock().ok().and_then(|mut s| s.take());
        if let Some(res) = done {
            self.cam.busy = false;
            match res {
                Ok(p) => {
                    self.cam.thumb = None;
                    self.gal.rescan();
                    self.tl.files_for.clear();
                    let _ = p;
                }
                Err(e) => self.toast(e),
            }
            ui.dirty = true;
        }

        // ---------- Aufteilung ----------
        let wide = ui.w > ui.h;
        let (view, ctrl) = if wide {
            let cw = ui.dp(132.0);
            (
                Rect::new(0.0, area.y, ui.w - cw, area.h),
                Rect::new(ui.w - cw, area.y, cw, area.h),
            )
        } else {
            let ch = ui.dp(158.0).min(area.h * 0.3);
            (
                Rect::new(0.0, area.y + ui.dp(52.0), ui.w, area.h - ch - ui.dp(52.0)),
                Rect::new(0.0, area.bottom() - ch, ui.w, ch),
            )
        };
        let top = Rect::new(view.x, area.y, view.w, ui.dp(52.0));
        let mirror = facing == 1 && cfg.mirror_front;

        // Scanner: Kontrollansicht ersetzt Sucher und Bedienfeld, bis die Seite übernommen oder verworfen ist
        if let Some(rv) = self.cam.review.take() {
            if self.cam.mode == 3 {
                self.scan_review_ui(ui, view, ctrl, rv);
                return;
            }
        }

        // ---------- Sucher ----------
        let mut have = false;
        let mut pic = view.inset(ui.dp(if wide { 6.0 } else { 0.0 }));
        if let Some((w, h, px)) = &self.cam.last_frame {
            let (w, h) = (*w, *h);
            if w > 0 && h > 0 {
                let aspect = if self.cam.mode == 1 {
                    1.0
                } else {
                    w as f32 / h as f32
                };
                let src = crop(
                    w,
                    h,
                    aspect,
                    if self.cam.mode == 3 {
                        1.0
                    } else {
                        self.cam.zoom
                    },
                );
                let k = (view.w / aspect).min(view.h);
                let (dw, dh) = (k * aspect, k);
                pic = Rect::new(view.cx() - dw / 2.0, view.cy() - dh / 2.0, dw, dh);
                ui.cv.blit_xrgb(px, w, h, src, pic, mirror);
                ui.cv.round_corners(pic, ui.dp(18.0), rgb(0x050506));
                have = true;
            }
        }
        let on = self.cam.worker.is_some();
        if !have {
            let status = self.cam.worker.as_ref().map(|w| w.status());
            let waited = self
                .cam
                .opened_at
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);
            let failed =
                self.cam.err.is_some() || status == Some(CamStatus::Failed) || (on && waited >= 8);
            ui.cv.rrect(pic, ui.dp(18.0), rgb(0x0E0F12));
            ui.cv.circle(
                pic.cx(),
                pic.cy() - ui.dp(34.0),
                ui.dp(34.0),
                white.alpha(0.06),
            );
            ui.icon(
                Ic::Camera,
                pic.cx(),
                pic.cy() - ui.dp(34.0),
                ui.dp(32.0),
                if failed { white.alpha(0.45) } else { gold },
            );
            let msg = if failed {
                self.t("Camera not available").to_string()
            } else {
                self.t("Starting camera…").to_string()
            };
            ui.label(
                Rect::new(pic.x, pic.cy() + ui.dp(10.0), pic.w, ui.dp(24.0)),
                &msg,
                15.0,
                white,
                true,
                Al::Center,
            );
            if failed {
                if let Some(e) = self.cam.err.clone() {
                    let e = ui.fit(&e, 12.0, false, pic.w - ui.dp(40.0));
                    ui.label(
                        Rect::new(pic.x, pic.cy() + ui.dp(36.0), pic.w, ui.dp(20.0)),
                        &e,
                        12.0,
                        white.alpha(0.5),
                        false,
                        Al::Center,
                    );
                }
                let label = self.t("Try again");
                let pw = ui.text_w(label, 13.5, false) + ui.dp(26.0);
                let (_, tapped) =
                    ui.pill(pic.cx() - pw / 2.0, pic.cy() + ui.dp(66.0), label, false);
                if tapped {
                    if let Some(w) = self.cam.worker.take() {
                        w.stop();
                    }
                    // gemerkten Knoten verwerfen, damit die Suche neu beginnt
                    let _ = std::fs::remove_file(format!("{}/camera.node", sys::data_dir()));
                    self.cam.err = None;
                    self.start_camera();
                    ui.dirty = true;
                }
            }
        }

        // Scanner: erkanntes Blatt einrahmen
        if have && self.cam.mode == 3 {
            match self.cam.scan_quad {
                Some(q) => {
                    let pt = |c: (f32, f32)| (pic.x + c.0 * pic.w, pic.y + c.1 * pic.h);
                    for i in 0..4 {
                        let (a, b) = (pt(q[i]), pt(q[(i + 1) % 4]));
                        ui.cv.line(a.0, a.1, b.0, b.1, ui.dp(3.0), gold);
                    }
                    for c in q {
                        let (x, y) = pt(c);
                        ui.cv.circle(x, y, ui.dp(7.0), gold);
                        ui.cv.circle(x, y, ui.dp(3.5), white);
                    }
                }
                None => {
                    let msg = self.t("Place the document in view");
                    let tw = ui.text_w(msg, 13.0, true) + ui.dp(28.0);
                    let r = Rect::new(pic.cx() - tw / 2.0, pic.cy() - ui.dp(15.0), tw, ui.dp(30.0));
                    ui.cv.rrect(r, ui.dp(15.0), rgb(0).alpha(0.5));
                    ui.label(r, msg, 13.0, white, true, Al::Center);
                }
            }
            // gesammelte Seiten → PDF
            let n = self.cam.scan_pages.lock().map(|p| p.len()).unwrap_or(0);
            if n > 0 {
                let label = format!(
                    "PDF · {n} {}",
                    self.t(if n == 1 { "page" } else { "pages" })
                );
                let pw = ui.text_w(&label, 13.5, true) + ui.dp(26.0);
                let th0 = ui.th;
                ui.th.card = rgb(0x17181C);
                ui.th.line = rgb(0x2A2C33);
                ui.th.text = white;
                let (_, tapped) = ui.pill(pic.cx() - pw / 2.0, pic.y + ui.dp(12.0), &label, true);
                ui.th = th0;
                if tapped {
                    self.save_scan_pdf();
                    ui.dirty = true;
                }
            }
        }

        // Raster (Drittel)
        if have && cfg.grid {
            let lc = white.alpha(0.28);
            for i in 1..3 {
                let x = pic.x + pic.w * i as f32 / 3.0;
                let y = pic.y + pic.h * i as f32 / 3.0;
                ui.cv.rect(Rect::new(x, pic.y, 1.0, pic.h), lc);
                ui.cv.rect(Rect::new(pic.x, y, pic.w, 1.0), lc);
            }
        }

        // ---------- obere Leiste: Licht, Selbstauslöser, Raster, Drehen ----------
        let recording = self.cam.rec.is_some();
        {
            let n = 4.0;
            let step = (top.w / n).min(ui.dp(96.0));
            let x0 = top.cx() - step * (n - 1.0) / 2.0;
            let r = ui.dp(17.0);
            let items: [(Ic, bool, u8); 4] = [
                (Ic::Flash, self.cam.torch, 0),
                (Ic::Timer, cfg.timer_s > 0, 1),
                (Ic::Thirds, cfg.grid, 2),
                (Ic::Rotate, false, 3),
            ];
            for (i, (ic, active, id)) in items.into_iter().enumerate() {
                let i = if ui.rtl { 3 - i } else { i };
                let cx = x0 + step * i as f32;
                let fg = if active { gold } else { white };
                let old = ui.enabled;
                ui.enabled = old && (!recording || id == 0);
                if ui.icon_btn(
                    cx,
                    top.cy(),
                    r,
                    ic,
                    white.alpha(if active { 0.14 } else { 0.07 }),
                    fg,
                ) {
                    match id {
                        0 => {
                            self.cam.torch = !self.cam.torch;
                            crate::quick::set_torch(self.cam.torch);
                        }
                        1 => {
                            let c = self.cam_cfg();
                            c.timer_s = match c.timer_s {
                                0 => 3,
                                3 => 10,
                                _ => 0,
                            };
                            self.cam_save();
                        }
                        2 => {
                            let c = self.cam_cfg();
                            c.grid = !c.grid;
                            self.cam_save();
                        }
                        _ => {
                            let c = self.cam_cfg();
                            c.rot[facing] = (c.rot[facing] + 1) % 4;
                            self.cam_save();
                            self.cam.last_frame = None;
                        }
                    }
                    ui.dirty = true;
                }
                ui.enabled = old;
                if id == 1 && cfg.timer_s > 0 {
                    let old = ui.rtl;
                    ui.rtl = false;
                    ui.label(
                        Rect::new(
                            cx + r * 0.5,
                            top.cy() - r - ui.dp(2.0),
                            ui.dp(22.0),
                            ui.dp(14.0),
                        ),
                        &cfg.timer_s.to_string(),
                        10.5,
                        gold,
                        true,
                        Al::Center,
                    );
                    ui.rtl = old;
                }
            }
        }

        // Aufnahme-Anzeige
        if let Some(rec) = &self.cam.rec {
            let s = rec.started.elapsed().as_secs();
            let t = format!("{:02}:{:02}", s / 60, s % 60);
            let tw = ui.text_w(&t, 13.0, true) + ui.dp(34.0);
            let r = Rect::new(pic.cx() - tw / 2.0, pic.y + ui.dp(12.0), tw, ui.dp(28.0));
            ui.cv.rrect(r, ui.dp(14.0), rgb(0).alpha(0.55));
            if s % 2 == 0 {
                ui.cv
                    .circle(r.x + ui.dp(14.0), r.cy(), ui.dp(4.5), rgb(0xF43F5E));
            }
            let old = ui.rtl;
            ui.rtl = false;
            ui.label(
                Rect::new(r.x + ui.dp(24.0), r.y, r.w - ui.dp(30.0), r.h),
                &t,
                13.0,
                white,
                true,
                Al::Start,
            );
            ui.rtl = old;
        }

        // Scanner: Aufbereitung wählen (Farbe / Grau / Schwarz-Weiß)
        if have && self.cam.mode == 3 {
            let names = ["Color", "Gray", "B/W"];
            let ws: Vec<f32> = names
                .iter()
                .map(|m| ui.text_w(self.t(m), 12.5, true) + ui.dp(22.0))
                .collect();
            let total: f32 = ws.iter().sum::<f32>() + ui.dp(8.0);
            let hgt = ui.dp(32.0);
            let cy = pic.bottom() - ui.dp(28.0);
            ui.cv.rrect(
                Rect::new(
                    pic.cx() - total / 2.0,
                    cy - hgt / 2.0 - ui.dp(3.0),
                    total,
                    hgt + ui.dp(6.0),
                ),
                hgt / 2.0 + ui.dp(3.0),
                rgb(0).alpha(0.45),
            );
            let mut x = pic.cx() - total / 2.0 + ui.dp(4.0);
            let order: Vec<usize> = if ui.rtl { vec![2, 1, 0] } else { vec![0, 1, 2] };
            for i in order {
                let hit = Rect::new(x, cy - hgt / 2.0, ws[i], hgt);
                let active = cfg.scan_filter as usize == i;
                if active {
                    ui.cv.rrect(hit, hgt / 2.0, white.alpha(0.92));
                }
                ui.label(
                    hit,
                    self.t(names[i]),
                    12.5,
                    if active { rgb(0x101010) } else { white },
                    true,
                    Al::Center,
                );
                if ui.tap(hit) {
                    self.cam_cfg().scan_filter = i as u8;
                    self.cam_save();
                    ui.dirty = true;
                }
                x += ws[i];
            }
        }

        // Zoom-Stufen im Sucher
        if have && self.cam.mode != 3 {
            let zs = [1.0f32, 2.0, 4.0];
            let r = ui.dp(17.0);
            let gap = ui.dp(44.0);
            let cy = pic.bottom() - ui.dp(28.0);
            let x0 = pic.cx() - gap;
            let bw = gap * 2.0 + r * 2.0 + ui.dp(10.0);
            ui.cv.rrect(
                Rect::new(
                    pic.cx() - bw / 2.0,
                    cy - r - ui.dp(5.0),
                    bw,
                    2.0 * r + ui.dp(10.0),
                ),
                r + ui.dp(5.0),
                rgb(0).alpha(0.38),
            );
            for (i, z) in zs.iter().enumerate() {
                let cx = x0 + gap * i as f32;
                let active = (self.cam.zoom - z).abs() < 0.05;
                let hit = Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r);
                if active {
                    ui.cv.circle(cx, cy, r, white.alpha(0.92));
                }
                let old = ui.rtl;
                ui.rtl = false;
                ui.label(
                    hit,
                    &format!("{}×", *z as u32),
                    12.0,
                    if active { rgb(0x101010) } else { white },
                    true,
                    Al::Center,
                );
                ui.rtl = old;
                if ui.tap(hit) {
                    self.cam.zoom = *z;
                    ui.dirty = true;
                }
            }
        }

        // Selbstauslöser läuft
        if let Some(end) = self.cam.countdown {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                // noch kein Bild da (Kamera startet): kurz weiter warten, dann aufgeben
                if self.capture_photo() || end.elapsed() > Duration::from_secs(15) {
                    self.cam.countdown = None;
                }
            } else {
                let n = left.as_secs() + 1;
                ui.cv
                    .circle(pic.cx(), pic.cy(), ui.dp(58.0), rgb(0).alpha(0.4));
                ui.label(
                    Rect::new(pic.x, pic.cy() - ui.dp(50.0), pic.w, ui.dp(100.0)),
                    &n.to_string(),
                    72.0,
                    white,
                    true,
                    Al::Center,
                );
            }
        }

        // ---------- Bedienfeld: Modi, Vorschau, Auslöser, Kamera wechseln ----------
        let modes = ["Photo", "Square", "Video", "Scan"];
        let (sx, sy, tx, ty, fx, fy) = if wide {
            (
                ctrl.cx(),
                ctrl.cy(),
                ctrl.cx(),
                ctrl.y + ui.dp(54.0),
                ctrl.cx(),
                ctrl.bottom() - ui.dp(54.0),
            )
        } else {
            let y = ctrl.y + ui.dp(44.0) + (ctrl.h - ui.dp(44.0)) / 2.0;
            let side = (ui.w * 0.3).min(ui.dp(130.0));
            let (a, b) = (ctrl.cx() - side, ctrl.cx() + side);
            if ui.rtl {
                (ctrl.cx(), y, b, y, a, y)
            } else {
                (ctrl.cx(), y, a, y, b, y)
            }
        };
        if !recording {
            // Modus-Leiste wie in der Web-Demo: Text-Reiter (Video, Foto, Scan, Quadrat), aktiver gold mit Punkt
            let order_demo: [usize; 4] = [2, 0, 3, 1];
            let ws: Vec<f32> = order_demo
                .iter()
                .map(|i| ui.text_w(self.t(modes[*i]), 14.0, false) + ui.dp(28.0))
                .collect();
            let total: f32 = ws.iter().sum();
            let (mut x, y) = if wide {
                (view.cx() - total / 2.0, view.y + ui.dp(60.0))
            } else {
                (ctrl.cx() - total / 2.0, ctrl.y + ui.dp(6.0))
            };
            let seq: Vec<usize> = if ui.rtl {
                (0..4).rev().collect()
            } else {
                (0..4).collect()
            };
            for k in seq {
                let i = order_demo[k];
                let r = Rect::new(x, y, ws[k], ui.dp(32.0));
                let on = self.cam.mode == i as u8;
                ui.label(
                    r,
                    self.t(modes[i]),
                    14.0,
                    if on { rgb(0xD8C870) } else { white.alpha(0.7) },
                    on,
                    Al::Center,
                );
                if on {
                    ui.cv
                        .circle(r.cx(), r.bottom() + ui.dp(2.0), ui.dp(2.0), rgb(0xD8C870));
                }
                if ui.tap(r) {
                    self.cam.mode = i as u8;
                    self.cam.countdown = None;
                    ui.dirty = true;
                }
                x += ws[k];
            }
        }

        // Vorschau der letzten Aufnahme
        let tr = ui.dp(25.0);
        let newest = self.gal.newest();
        let thumb_rect = Rect::new(tx - tr, ty - tr, 2.0 * tr, 2.0 * tr);
        ui.cv.circle(tx, ty, tr + ui.dp(2.0), white.alpha(0.85));
        ui.cv.circle(tx, ty, tr, rgb(0x17181C));
        if let Some(it) = &newest {
            if self.cam.thumb.as_ref().map(|t| t.0.as_str()) != Some(it.path.as_str()) {
                let p = if it.video {
                    mjpeg::Reader::open(&it.path)
                        .ok()
                        .and_then(|mut r| r.frame(0))
                        .map(|(w, h, rgba)| Pic { w, h, rgba })
                } else {
                    decode(&it.path, 160).ok()
                };
                self.cam.thumb = Some((it.path.clone(), p));
            }
            if let Some((_, Some(p))) = &self.cam.thumb {
                let k = (thumb_rect.w / p.w as f32).max(thumb_rect.h / p.h as f32);
                let (w, h) = (p.w as f32 * k, p.h as f32 * k);
                ui.cv.set_clip(Some(thumb_rect));
                ui.cv.blit_rgba(
                    &p.rgba,
                    p.w,
                    p.h,
                    Rect::new(tx - w / 2.0, ty - h / 2.0, w, h),
                );
                ui.cv.set_clip(None);
                ui.cv.round_corners(thumb_rect, tr, rgb(0));
                ui.cv
                    .ring(tx, ty, tr + ui.dp(1.0), ui.dp(2.0), white.alpha(0.85));
            }
        } else {
            ui.icon(Ic::Image, tx, ty, ui.dp(22.0), white.alpha(0.5));
        }
        if ui.tap(thumb_rect.inset(-ui.dp(8.0))) && !recording {
            match newest {
                Some(it) => {
                    let list: Vec<String> = self.gal.items.iter().map(|i| i.path.clone()).collect();
                    self.open_photo(it.path, list);
                }
                None => self.go(super::Screen::Gallery),
            }
            ui.dirty = true;
            return;
        }

        // Auslöser
        let (ro, ri) = (ui.dp(37.0), ui.dp(29.0));
        let hit = Rect::new(sx - ro, sy - ro, 2.0 * ro, 2.0 * ro);
        let held = ui.held(hit);
        let can = have && !self.cam.busy;
        if self.cam.remote_shot
            && can
            && self
                .cam
                .opened_at
                .map(|t| t.elapsed() > Duration::from_millis(3000))
                .unwrap_or(true)
        {
            self.cam.remote_shot = false;
            if self.cam.mode == 3 {
                self.capture_scan();
            } else {
                self.capture_photo();
            }
        }
        ui.cv.ring(
            sx,
            sy,
            ro - ui.dp(2.0),
            ui.dp(4.0),
            if can { white } else { white.alpha(0.35) },
        );
        let video = self.cam.mode == 2;
        let inner = if video { rgb(0xF43F5E) } else { white };
        let inner = if can { inner } else { inner.alpha(0.35) };
        if recording {
            let s = ui.dp(24.0);
            ui.cv.rrect(
                Rect::new(sx - s / 2.0, sy - s / 2.0, s, s),
                ui.dp(6.0),
                inner,
            );
        } else {
            ui.cv.circle(
                sx,
                sy,
                if held && can { ri - ui.dp(4.0) } else { ri },
                inner,
            );
        }
        if self.cam.countdown.is_some() {
            ui.icon(Ic::Close, sx, sy, ui.dp(20.0), rgb(0x101010));
        }
        if can && ui.tap(hit) {
            if self.cam.countdown.take().is_some() {
                // Selbstauslöser abgebrochen
            } else if video {
                if recording {
                    self.cam.rec = None;
                    self.cam.busy = true;
                } else {
                    self.start_recording();
                }
            } else if self.cam.mode == 3 {
                self.capture_scan();
            } else if cfg.timer_s > 0 {
                self.cam.countdown = Some(Instant::now() + Duration::from_secs(cfg.timer_s as u64));
            } else {
                self.capture_photo();
            }
            ui.dirty = true;
        }

        // Front/Rück
        let old = ui.enabled;
        ui.enabled = old && !recording;
        if ui.icon_btn(fx, fy, ui.dp(24.0), Ic::Flip, white.alpha(0.1), white) {
            self.cam.facing ^= 1;
            if let Some(w) = self.cam.worker.take() {
                w.stop();
            }
            self.cam.err = None;
            self.cam.last_frame = None;
            ui.dirty = true;
        }
        ui.enabled = old;

        // kurzer Lichtblitz als Rückmeldung beim Auslösen
        if let Some(t) = self.cam.flash_fx {
            let a = 1.0 - t.elapsed().as_millis() as f32 / 180.0;
            if a <= 0.0 {
                self.cam.flash_fx = None;
            } else {
                ui.cv.rrect(pic, ui.dp(18.0), white.alpha(a * 0.85));
            }
        }
    }

    /// Vom Agenten: Foto nach `secs` Sekunden auslösen (Kamera ist dann geöffnet).
    pub(super) fn camera_auto_shot(&mut self, secs: u64) {
        self.cam.countdown = Some(Instant::now() + Duration::from_secs(secs));
    }

    fn start_camera(&mut self) {
        self.cam.err = None;
        self.cam.last_frame = None;
        self.cam.opened_at = Some(Instant::now());
        // gemerkter Knoten je Kamera: camera.node (Rück) / camera-front.node
        let file = if self.cam.facing == 1 {
            "camera-front.node"
        } else {
            "camera.node"
        };
        let hint = std::fs::read_to_string(format!("{}/{file}", sys::data_dir()))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if self.cam.facing == 1 && hint.is_none() {
            // Ohne bekannten Front-Knoten bleibt es bei der Rückkamera (Knoten in camera-front.node eintragen)
            self.toast(self.t("Front camera node not known yet").into());
        }
        self.cam.worker = Some(CameraWorker::start(self.cam.facing, hint, sys::data_dir()));
    }

    /// Für Foto und Scan ein Bild in voller Auflösung holen (Samsung-Kette), sonst bleibt das Vorschaubild.
    fn grab_hires(&mut self) {
        // Samsung-Kette: für das Foto ein Bild in voller Auflösung anfordern und kurz darauf warten.
        let rot = {
            let facing = self.cam.facing as usize & 1;
            self.cam_cfg().rot[facing]
        };
        if let Some(wk) = self.cam.worker.as_ref().filter(|w| w.hires()) {
            let base = self
                .cam
                .last_frame
                .as_ref()
                .map(|f| f.0.max(f.1))
                .unwrap_or(0);
            wk.request_photo();
            let until = Instant::now() + Duration::from_millis(1500);
            while Instant::now() < until {
                if let Some(f) = wk.take_frame() {
                    let big = f.0.max(f.1) > base;
                    self.cam.last_frame = Some(rotate(f, rot));
                    if big {
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(8));
            }
        }
    }

    /// Dokument aufnehmen: Blatt suchen und zur Kontrolle anzeigen (Ecken lassen sich ziehen).
    pub(super) fn capture_scan(&mut self) -> bool {
        self.grab_hires();
        let Some(frame) = self.cam.last_frame.clone() else {
            return false;
        };
        let (w, h) = (frame.0, frame.1);
        if w == 0 || h == 0 || (frame.2.len() as u32) < w * h {
            return false;
        }
        // im großen Bild neu suchen (genauer); sonst die Ecken aus der Vorschau; sonst ein Rahmen zum Zurechtziehen
        let found = scan::detect(&frame.2, w, h).or(self.cam.scan_quad);
        let quad = found.unwrap_or([(0.08, 0.08), (0.92, 0.08), (0.92, 0.92), (0.08, 0.92)]);
        self.cam.flash_fx = Some(Instant::now());
        if self.set.sound {
            crate::audio::play(vec![(1800.0, 25), (1200.0, 35)], self.set.volume * 0.5);
        }
        self.cam.review = Some(Review {
            frame,
            quad,
            grab: None,
            found: found.is_some(),
        });
        true
    }

    /// Kontrollierte Seite entzerren, aufbereiten, als JPEG speichern und für das PDF merken – im Hintergrund.
    fn accept_scan(&mut self, rv: Review) {
        let (w, h, px) = rv.frame;
        let q = rv.quad;
        let filter = scan::Filter::from(self.cam_cfg().scan_filter);
        let path = unique("SCAN", self.set.tz_min, "jpg");
        let (slot, bg, pages) = (
            self.cam.saved.clone(),
            self.bg.clone(),
            self.cam.scan_pages.clone(),
        );
        self.cam.busy = true;
        std::thread::spawn(move || {
            let (ow, oh, mut out) = scan::warp(&px, w, h, &q, 2200);
            drop(px);
            scan::enhance(&mut out, ow, oh, filter);
            let res = match mjpeg::encode_jpeg(&out, ow, oh, 88) {
                Some(j) => match std::fs::write(&path, &j) {
                    Ok(()) => {
                        if let Ok(mut p) = pages.lock() {
                            // Speicher begrenzen: höchstens 40 Seiten je PDF
                            if p.len() < 40 {
                                p.push((j, ow, oh));
                            }
                        }
                        Ok(path)
                    }
                    Err(e) => Err(e.to_string()),
                },
                None => Err("JPEG".to_string()),
            };
            if let Ok(mut s) = slot.lock() {
                *s = Some(res);
            }
            bg.poke();
        });
    }

    /// Kontrollansicht nach der Aufnahme: Standbild, Ecken ziehen, Filter wählen, verwerfen oder übernehmen.
    fn scan_review_ui(&mut self, ui: &mut Ui, view: Rect, ctrl: Rect, mut rv: Review) {
        let white = rgb(0xFFFFFF);
        let gold = ui.th.accent;
        let (w, h) = (rv.frame.0, rv.frame.1);
        let aspect = w as f32 / h as f32;
        let avail = Rect::new(
            view.x + ui.dp(18.0),
            view.y + ui.dp(40.0),
            view.w - ui.dp(36.0),
            view.h - ui.dp(100.0),
        );
        let k = (avail.w / aspect).min(avail.h);
        let (dw, dh) = (k * aspect, k);
        let pic = Rect::new(avail.cx() - dw / 2.0, avail.cy() - dh / 2.0, dw, dh);
        ui.cv.blit_xrgb(
            &rv.frame.2,
            w,
            h,
            Rect::new(0.0, 0.0, w as f32, h as f32),
            pic,
            false,
        );
        let hint = if rv.found {
            self.t("Check the corners")
        } else {
            self.t("Drag the corners to the page edges")
        };
        ui.label(
            Rect::new(view.x, view.y + ui.dp(8.0), view.w, ui.dp(24.0)),
            hint,
            14.0,
            white,
            true,
            Al::Center,
        );

        // Ecken ziehen
        let pt = |c: (f32, f32)| (pic.x + c.0 * pic.w, pic.y + c.1 * pic.h);
        if ui.p.pressed {
            let reach = ui.dp(46.0);
            let mut best = (reach * reach, None);
            for (i, c) in rv.quad.iter().enumerate() {
                let (x, y) = pt(*c);
                let d = (x - ui.p.x).powi(2) + (y - ui.p.y).powi(2);
                if d < best.0 {
                    best = (d, Some(i));
                }
            }
            rv.grab = best.1;
        }
        if ui.p.down {
            if let Some(i) = rv.grab {
                rv.quad[i] = (
                    ((ui.p.x - pic.x) / pic.w).clamp(0.0, 1.0),
                    ((ui.p.y - pic.y) / pic.h).clamp(0.0, 1.0),
                );
                ui.dirty = true;
            }
        } else {
            rv.grab = None;
        }
        let ok = scan::valid(&rv.quad);
        let lc = if ok { gold } else { rgb(0xF43F5E) };
        for i in 0..4 {
            let (a, b) = (pt(rv.quad[i]), pt(rv.quad[(i + 1) % 4]));
            ui.cv.line(a.0, a.1, b.0, b.1, ui.dp(3.0), lc);
        }
        for (i, c) in rv.quad.iter().enumerate() {
            let (x, y) = pt(*c);
            let big = rv.grab == Some(i);
            ui.cv
                .circle(x, y, ui.dp(if big { 20.0 } else { 13.0 }), lc.alpha(0.35));
            ui.cv.circle(x, y, ui.dp(8.0), lc);
            ui.cv.circle(x, y, ui.dp(4.0), white);
        }

        // Filterwahl
        let cfg_filter = self.cam_cfg().scan_filter as usize;
        let names = ["Color", "Gray", "B/W"];
        let ws: Vec<f32> = names
            .iter()
            .map(|m| ui.text_w(self.t(m), 12.5, true) + ui.dp(22.0))
            .collect();
        let total: f32 = ws.iter().sum::<f32>() + ui.dp(8.0);
        let hgt = ui.dp(32.0);
        let cy = view.bottom() - ui.dp(26.0);
        ui.cv.rrect(
            Rect::new(
                view.cx() - total / 2.0,
                cy - hgt / 2.0 - ui.dp(3.0),
                total,
                hgt + ui.dp(6.0),
            ),
            hgt / 2.0 + ui.dp(3.0),
            rgb(0x17181C),
        );
        let mut x = view.cx() - total / 2.0 + ui.dp(4.0);
        let order: Vec<usize> = if ui.rtl { vec![2, 1, 0] } else { vec![0, 1, 2] };
        for i in order {
            let hit = Rect::new(x, cy - hgt / 2.0, ws[i], hgt);
            if cfg_filter == i {
                ui.cv.rrect(hit, hgt / 2.0, white.alpha(0.92));
            }
            ui.label(
                hit,
                self.t(names[i]),
                12.5,
                if cfg_filter == i {
                    rgb(0x101010)
                } else {
                    white
                },
                true,
                Al::Center,
            );
            if rv.grab.is_none() && ui.tap(hit) {
                self.cam_cfg().scan_filter = i as u8;
                self.cam_save();
                ui.dirty = true;
            }
            x += ws[i];
        }

        // Verwerfen / Übernehmen
        let y = ctrl.cy();
        let side = (ui.w * 0.22).min(ui.dp(96.0));
        let (xl, xr) = if ui.rtl {
            (ctrl.cx() + side, ctrl.cx() - side)
        } else {
            (ctrl.cx() - side, ctrl.cx() + side)
        };
        let discard = ui.icon_btn(xl, y, ui.dp(26.0), Ic::Close, white.alpha(0.12), white);
        let accept = ui.icon_btn(
            xr,
            y,
            ui.dp(32.0),
            Ic::Check,
            if ok { gold } else { white.alpha(0.12) },
            if ok { rgb(0x101010) } else { white.alpha(0.4) },
        );
        let remote = std::mem::take(&mut self.cam.remote_accept);
        if discard && rv.grab.is_none() {
            ui.dirty = true;
            return;
        }
        if (accept || remote) && ok && rv.grab.is_none() {
            self.accept_scan(rv);
            ui.dirty = true;
            return;
        }
        self.cam.review = Some(rv);
    }

    /// Gesammelte Scan-Seiten als ein PDF unter Documents ablegen.
    fn save_scan_pdf(&mut self) {
        let pages = match self.cam.scan_pages.lock() {
            Ok(mut p) => std::mem::take(&mut *p),
            Err(_) => return,
        };
        if pages.is_empty() {
            return;
        }
        let dir = format!("{}/Documents", super::app_gallery::media_root());
        let _ = std::fs::create_dir_all(&dir);
        let name = format!("SCAN_{}.pdf", stamp(self.set.tz_min));
        let path = format!("{dir}/{name}");
        match std::fs::write(&path, scan::pdf(&pages)) {
            Ok(()) => {
                self.tl.files_for.clear();
                self.toast(format!("{}: Documents/{name}", self.t("PDF saved")));
            }
            Err(e) => {
                // Seiten nicht verlieren
                if let Ok(mut p) = self.cam.scan_pages.lock() {
                    *p = pages;
                }
                self.toast(e.to_string());
            }
        }
    }

    /// Sichtbaren Ausschnitt (Modus, Zoom, Spiegelung) als JPEG speichern – im Hintergrund.
    pub(super) fn capture_photo(&mut self) -> bool {
        self.grab_hires();
        let Some((w, h, px)) = self.cam.last_frame.clone() else {
            return false;
        };
        if w == 0 || h == 0 || (px.len() as u32) < w * h {
            return false;
        }
        let facing = self.cam.facing as usize & 1;
        let mirror = facing == 1 && self.cam_cfg().mirror_front;
        let aspect = if self.cam.mode == 1 {
            1.0
        } else {
            w as f32 / h as f32
        };
        let c = crop(w, h, aspect, self.cam.zoom);
        let (cx, cy, cw, ch) = (
            c.x as u32,
            c.y as u32,
            (c.w as u32).clamp(1, w),
            (c.h as u32).clamp(1, h),
        );
        let path = unique("IMG", self.set.tz_min, "jpg");
        let (slot, bg) = (self.cam.saved.clone(), self.bg.clone());
        self.cam.flash_fx = Some(Instant::now());
        self.cam.busy = true;
        if self.set.sound {
            crate::audio::play(vec![(1800.0, 25), (1200.0, 35)], self.set.volume * 0.5);
        }
        std::thread::spawn(move || {
            let mut out = Vec::with_capacity((cw * ch) as usize);
            for y in cy..(cy + ch).min(h) {
                let row = &px[(y * w + cx) as usize..(y * w + (cx + cw).min(w)) as usize];
                if mirror {
                    out.extend(row.iter().rev());
                } else {
                    out.extend_from_slice(row);
                }
            }
            let res = mjpeg::encode_jpeg(&out, cw, ch, 93)
                .ok_or_else(|| "JPEG".to_string())
                .and_then(|j| std::fs::write(&path, j).map_err(|e| e.to_string()))
                .map(|_| path);
            if let Ok(mut s) = slot.lock() {
                *s = Some(res);
            }
            bg.poke();
        });
        true
    }

    fn start_recording(&mut self) {
        let Some((w, h, _)) = &self.cam.last_frame else {
            return;
        };
        let (w, h) = (*w, *h);
        let path = unique("VID", self.set.tz_min, "avi");
        let (tx, rx) = sync_channel::<Frame>(2);
        let (slot, bg) = (self.cam.saved.clone(), self.bg.clone());
        std::thread::spawn(move || {
            let res = (|| -> Result<String, String> {
                let mut wr = mjpeg::Writer::create(&path, w, h).map_err(|e| e.to_string())?;
                let t0 = Instant::now();
                while let Ok((fw, fh, px)) = rx.recv() {
                    if (fw, fh) != (w, h) {
                        continue;
                    }
                    if let Some(j) = mjpeg::encode_jpeg(&px, w, h, 82) {
                        wr.push(&j).map_err(|e| e.to_string())?;
                    }
                }
                let n = wr.frames().max(1) as u128;
                if wr.frames() == 0 {
                    let _ = std::fs::remove_file(&path);
                    return Err("Keine Bilder aufgenommen".into());
                }
                wr.finish((t0.elapsed().as_micros() / n) as u32)
                    .map_err(|e| e.to_string())?;
                Ok(path.clone())
            })();
            if let Ok(mut s) = slot.lock() {
                *s = Some(res);
            }
            bg.poke();
        });
        self.cam.rec = Some(Recorder {
            tx,
            started: Instant::now(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drehen_und_ausschnitt() {
        let f: Frame = (3, 2, vec![1, 2, 3, 4, 5, 6]);
        // Vierteldrehung im Uhrzeigersinn: 3x2 -> 2x3
        assert_eq!(rotate(f.clone(), 1), (2, 3, vec![4, 1, 5, 2, 6, 3]));
        assert_eq!(rotate(f.clone(), 2), (3, 2, vec![6, 5, 4, 3, 2, 1]));
        assert_eq!(rotate(rotate(f.clone(), 1), 3), f);
        let c = crop(640, 480, 1.0, 1.0);
        assert_eq!((c.x, c.y, c.w, c.h), (80.0, 0.0, 480.0, 480.0));
        let c = crop(640, 480, 640.0 / 480.0, 2.0);
        assert_eq!((c.x, c.y, c.w, c.h), (160.0, 120.0, 320.0, 240.0));
    }
}
