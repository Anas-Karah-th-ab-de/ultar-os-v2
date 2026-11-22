//! Systemmonitor: Auslastung von Prozessor, Arbeitsspeicher, Grafik-/KI-Chip und Datenverkehr (besonders Upload).
//! Untermodul von `app`. Messwerte kommen direkt aus /proc und /sys – kein zusätzlicher Dienst.

use super::{App, Screen};
use crate::gfx::Rect;
use crate::sys;
use crate::ui::{Al, Ui};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Instant;

const HIST: usize = 120;

#[derive(Default)]
pub struct Mon {
    last: Option<Instant>,
    cpu_prev: Vec<(u64, u64)>, // je Zeile aus /proc/stat: (gesamt, leerlauf)
    pub cpu: Vec<f32>,         // [0] = alle Kerne, danach je Kern (0..1)
    pub cpu_hist: Vec<f32>,
    pub mem: (u64, u64),       // gesamt, verfügbar (Bytes)
    pub gpu: Option<(String, f32)>,
    pub npu: Option<(String, f32)>,
    net_prev: BTreeMap<String, (u64, u64)>,
    /// je Schnittstelle: (Download B/s, Upload B/s, empfangen gesamt, gesendet gesamt)
    pub net: BTreeMap<String, (f32, f32, u64, u64)>,
    pub up_hist: Vec<f32>,
    pub down_hist: Vec<f32>,
    pub top: Vec<(String, u64)>,
    top_at: Option<Instant>,
    pub usage: Usage,
    usage_saved: Option<Instant>,
}

/// Verbrauch je Tag (dauerhaft gespeichert): Tag -> (empfangen, gesendet) in Bytes, ohne lo.
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Usage {
    pub days: BTreeMap<String, (u64, u64)>,
}

fn read(p: &str) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// /proc/stat lesen: je cpu-Zeile (gesamt, leerlauf)
pub fn parse_stat(s: &str) -> Vec<(u64, u64)> {
    s.lines()
        .filter(|l| l.starts_with("cpu"))
        .map(|l| {
            let v: Vec<u64> = l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
            let idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
            (v.iter().take(8).sum(), idle)
        })
        .collect()
}

/// /proc/net/dev lesen: Schnittstelle -> (empfangen, gesendet)
pub fn parse_netdev(s: &str) -> BTreeMap<String, (u64, u64)> {
    s.lines()
        .filter_map(|l| {
            let (name, rest) = l.split_once(':')?;
            let v: Vec<u64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            Some((name.trim().to_string(), (*v.first()?, *v.get(8)?)))
        })
        .collect()
}

fn first_number(s: &str) -> Option<f32> {
    let t: String = s.trim().chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    t.parse().ok()
}

/// Auslastung des Grafikchips: bekannte Stellen je Hersteller der Reihe nach (erste lesbare gilt).
fn gpu_load() -> Option<(String, f32)> {
    let fixed = [
        ("Mali", "/sys/class/misc/mali0/device/utilization"),
        ("Mali", "/sys/devices/platform/mali.0/utilization"),
        ("Adreno", "/sys/class/kgsl/kgsl-3d0/gpu_busy_percentage"),
        ("GPU", "/sys/kernel/gpu/gpu_busy"),
        ("GPU", "/sys/class/drm/card0/device/gpu_busy_percent"),
    ];
    for (n, p) in fixed {
        if let Some(v) = first_number(&read(p)) {
            return Some((n.into(), (v / 100.0).clamp(0.0, 1.0)));
        }
    }
    // devfreq (z. B. Mali/Panfrost, Rockchip): "load" = "<prozent>@<frequenz>Hz"
    for e in std::fs::read_dir("/sys/class/devfreq").ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.contains("gpu") || name.contains("mali") {
            if let Some(v) = first_number(&read(&format!("/sys/class/devfreq/{name}/load"))) {
                return Some(("GPU".into(), (v / 100.0).clamp(0.0, 1.0)));
            }
        }
    }
    None
}

/// Auslastung des KI-Chips (NPU), soweit der Kernel sie anbietet (Rockchip).
fn npu_load() -> Option<(String, f32)> {
    for p in ["/sys/kernel/debug/rknpu/load", "/proc/rknpu/load"] {
        let s = read(p);
        if let Some(i) = s.find(|c: char| c.is_ascii_digit()) {
            if let Some(v) = first_number(&s[i..]) {
                return Some(("NPU".into(), (v / 100.0).clamp(0.0, 1.0)));
            }
        }
    }
    None
}

fn top_processes() -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = std::fs::read_dir("/proc")
        .map(|d| {
            d.flatten()
                .filter_map(|e| {
                    let pid = e.file_name().to_string_lossy().into_owned();
                    if !pid.chars().all(|c| c.is_ascii_digit()) {
                        return None;
                    }
                    let rss = read(&format!("/proc/{pid}/statm")).split_whitespace().nth(1)?.parse::<u64>().ok()? * 4096;
                    if rss == 0 {
                        return None;
                    }
                    Some((read(&format!("/proc/{pid}/comm")).trim().to_string(), rss))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v.truncate(6);
    v
}

fn push(h: &mut Vec<f32>, v: f32) {
    h.push(v);
    if h.len() > HIST {
        h.remove(0);
    }
}

pub fn rate(b: f32) -> String {
    if b >= 1_048_576.0 {
        format!("{:.1} MB/s", b / 1_048_576.0)
    } else if b >= 1024.0 {
        format!("{:.0} kB/s", b / 1024.0)
    } else {
        format!("{b:.0} B/s")
    }
}

pub fn amount(b: u64) -> String {
    let f = b as f64;
    if f >= 1_073_741_824.0 {
        format!("{:.2} GB", f / 1_073_741_824.0)
    } else if f >= 1_048_576.0 {
        format!("{:.1} MB", f / 1_048_576.0)
    } else {
        format!("{:.0} kB", f / 1024.0)
    }
}

impl Mon {
    /// Messung. Läuft auch im Hintergrund weiter (Verbrauch je Tag), dann seltener.
    pub fn sample(&mut self, tz_min: i32) {
        let now = Instant::now();
        let dt = self.last.map(|t| now.duration_since(t).as_secs_f32()).unwrap_or(0.0);
        if self.last.is_some() && dt < 0.4 {
            return;
        }
        self.last = Some(now);
        // Prozessor
        let cur = parse_stat(&read("/proc/stat"));
        if cur.len() == self.cpu_prev.len() && !cur.is_empty() {
            self.cpu = cur.iter().zip(&self.cpu_prev).map(|(c, p)| {
                let (dt, di) = (c.0.saturating_sub(p.0), c.1.saturating_sub(p.1));
                if dt == 0 { 0.0 } else { (1.0 - di as f32 / dt as f32).clamp(0.0, 1.0) }
            }).collect();
            push(&mut self.cpu_hist, self.cpu[0]);
        }
        self.cpu_prev = cur;
        self.mem = sys::meminfo();
        self.gpu = gpu_load();
        self.npu = npu_load();
        // Datenverkehr
        let cur = parse_netdev(&read("/proc/net/dev"));
        let (mut up, mut down, mut d_rx, mut d_tx) = (0.0, 0.0, 0u64, 0u64);
        let mut net = BTreeMap::new();
        for (n, (rx, tx)) in &cur {
            if n == "lo" {
                continue;
            }
            let (prx, ptx) = self.net_prev.get(n).copied().unwrap_or((*rx, *tx));
            let (drx, dtx) = (rx.saturating_sub(prx), tx.saturating_sub(ptx));
            let (r, t) = if dt > 0.0 { (drx as f32 / dt, dtx as f32 / dt) } else { (0.0, 0.0) };
            net.insert(n.clone(), (r, t, *rx, *tx));
            down += r;
            up += t;
            d_rx += drx;
            d_tx += dtx;
        }
        if !self.net_prev.is_empty() {
            push(&mut self.up_hist, up);
            push(&mut self.down_hist, down);
            let d = sys::from_unix(sys::unix_now(), tz_min);
            let e = self.usage.days.entry(format!("{:04}-{:02}-{:02}", d.y, d.mo, d.d)).or_default();
            e.0 += d_rx;
            e.1 += d_tx;
            while self.usage.days.len() > 31 {
                let k = self.usage.days.keys().next().cloned().unwrap();
                self.usage.days.remove(&k);
            }
            // höchstens einmal je Minute sichern (schont den Speicher)
            if (d_rx + d_tx > 0) && self.usage_saved.map_or(true, |t| t.elapsed().as_secs() >= 60) {
                let _ = sys::save("net-usage.toml", &self.usage);
                self.usage_saved = Some(now);
            }
        }
        self.net_prev = cur;
        self.net = net;
        if self.top_at.map_or(true, |t| t.elapsed().as_secs() >= 3) {
            self.top = top_processes();
            self.top_at = Some(now);
        }
    }
}

impl App {
    /// Balken mit Beschriftung links und Wert rechts
    fn meter(&mut self, ui: &mut Ui, y: &mut f32, title: &str, value: &str, v: Option<f32>) {
        let m = ui.dp(12.0);
        let r = Rect::new(m, *y, ui.w - 2.0 * m, ui.dp(if v.is_some() { 66.0 } else { 48.0 }));
        *y += r.h + ui.dp(8.0);
        if !ui.visible(r) {
            return;
        }
        ui.card(r);
        let (c, cs) = (ui.th.text, ui.th.sub);
        let lr = Rect::new(r.x + ui.dp(14.0), r.y + ui.dp(8.0), r.w - ui.dp(28.0), ui.dp(26.0));
        ui.label(lr, title, 15.0, c, false, Al::Start);
        ui.label(lr, value, 14.0, cs, false, Al::End);
        if let Some(v) = v {
            ui.progress(Rect::new(r.x + ui.dp(14.0), r.y + ui.dp(42.0), r.w - ui.dp(28.0), ui.dp(10.0)), v);
        }
    }

    /// Verlaufslinie (Werte 0..max) in einer Karte
    fn graph(&mut self, ui: &mut Ui, y: &mut f32, a: &[f32], b: Option<&[f32]>, max: f32) {
        let m = ui.dp(12.0);
        let r = Rect::new(m, *y, ui.w - 2.0 * m, ui.dp(110.0));
        *y += r.h + ui.dp(8.0);
        if !ui.visible(r) {
            return;
        }
        ui.card(r);
        let inner = r.inset(ui.dp(10.0));
        let max = max.max(1e-6);
        let (ca, cb) = (ui.th.accent, ui.th.sub);
        let th = ui.dp(2.0);
        let mut draw = |h: &[f32], col| {
            let n = h.len();
            for i in 1..n {
                let x0 = inner.x + inner.w * (HIST - n + i - 1) as f32 / (HIST - 1) as f32;
                let x1 = inner.x + inner.w * (HIST - n + i) as f32 / (HIST - 1) as f32;
                let y0 = inner.bottom() - inner.h * (h[i - 1] / max).clamp(0.0, 1.0);
                let y1 = inner.bottom() - inner.h * (h[i] / max).clamp(0.0, 1.0);
                ui.cv.line(x0, y0, x1, y1, th, col);
            }
        };
        if let Some(b) = b {
            draw(b, cb);
        }
        draw(a, ca);
    }

    pub(super) fn s_monitor(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Monitor"));
        self.mon.sample(self.set.tz_min);
        let mut y = area.y;
        let tabs = [self.t("Load"), self.t("Data traffic"), self.t("Processes")];
        if let Some(i) = ui.chips(&mut y, &tabs, self.mon_tab) {
            self.mon_tab = i;
            *sc = 0.0;
            ui.dirty = true;
        }
        let body = Rect::new(area.x, y, area.w, (area.bottom() - y).max(0.0));
        ui.scroll_begin(body, sc);
        let mut y = body.y - *sc;
        let na = self.t("not available");
        if self.mon_tab == 0 {
            // wie in der Web-Demo: Prozessor-Karte mit Verlauf und Kernen, dann vier kleine Karten
            let m = ui.dp(16.0);
            let (c, cs) = (ui.th.text, ui.th.sub);
            let cpu = self.mon.cpu.first().copied().unwrap_or(0.0);
            let cores: Vec<f32> = self.mon.cpu.iter().skip(1).copied().take(8).collect();
            let core_rows = cores.len().div_ceil(4) as f32;
            let ch = ui.dp(28.0 + 64.0 + 8.0) + core_rows * ui.dp(22.0) + ui.dp(10.0);
            let r = Rect::new(m, y, ui.w - 2.0 * m, ch);
            ui.card(r);
            let inn = Rect::new(r.x + ui.dp(14.0), r.y + ui.dp(12.0), r.w - ui.dp(28.0), ui.dp(20.0));
            ui.label(inn, self.t("Processor"), 15.0, c, true, Al::Start);
            ui.label(inn, &format!("{:.0} %", cpu * 100.0), 15.0, ui.th.accent, false, Al::End);
            // Verlauf als Linie
            let gr = Rect::new(inn.x, inn.y + ui.dp(28.0), inn.w, ui.dp(60.0));
            let hist = self.mon.cpu_hist.clone();
            if hist.len() >= 2 {
                let n = hist.len();
                let ac = ui.th.accent;
                let pts: Vec<(f32, f32)> = hist.iter().enumerate().map(|(i, v)| {
                    let x = if ui.rtl { gr.right() - gr.w * i as f32 / (n - 1) as f32 } else { gr.x + gr.w * i as f32 / (n - 1) as f32 };
                    (x, gr.bottom() - gr.h * v.clamp(0.0, 1.0))
                }).collect();
                for w in pts.windows(2) {
                    ui.cv.line(w[0].0, w[0].1, w[1].0, w[1].1, ui.dp(2.0), ac);
                }
            }
            // Kerne
            let cw = (inn.w - 3.0 * ui.dp(6.0)) / 4.0;
            for (i, v) in cores.iter().enumerate() {
                let col = if ui.rtl { 3 - i % 4 } else { i % 4 };
                let cx = inn.x + col as f32 * (cw + ui.dp(6.0));
                let cy = gr.bottom() + ui.dp(8.0) + (i / 4) as f32 * ui.dp(22.0);
                ui.label(Rect::new(cx, cy, cw, ui.dp(12.0)), &format!("C{i}"), 10.0, cs, false, Al::Start);
                let br = Rect::new(cx, cy + ui.dp(14.0), cw, ui.dp(5.0));
                ui.cv.rrect(br, ui.dp(2.5), ui.th.line);
                let fw = (br.w * v.clamp(0.0, 1.0)).max(ui.dp(5.0));
                ui.cv.rrect(if ui.rtl { Rect::new(br.right() - fw, br.y, fw, br.h) } else { Rect::new(br.x, br.y, fw, br.h) }, ui.dp(2.5), ui.th.accent);
            }
            y += ch + ui.dp(12.0);
            // Vier Karten: RAM, Grafik, NPU, Temperatur
            let (mt, ma) = self.mon.mem;
            let used = mt.saturating_sub(ma);
            let temp = std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp").ok().and_then(|t| t.trim().parse::<f32>().ok()).map(|t| if t > 1000.0 { t / 1000.0 } else { t });
            let gpu = self.mon.gpu.clone();
            let npu = self.mon.npu.clone();
            let tiles: [(String, String, Option<f32>); 4] = [
                ("RAM".into(), format!("{} / {}", sys::human(used), sys::human(mt)), Some(if mt > 0 { used as f32 / mt as f32 } else { 0.0 })),
                (gpu.as_ref().map(|g| format!("GPU ({})", g.0)).unwrap_or_else(|| "GPU".into()), gpu.as_ref().map(|g| format!("{:.0} %", g.1 * 100.0)).unwrap_or_else(|| na.to_string()), gpu.map(|g| g.1)),
                ("NPU".into(), npu.as_ref().map(|g| format!("{:.0} %", g.1 * 100.0)).unwrap_or_else(|| na.to_string()), npu.map(|g| g.1)),
                (self.t("Temperature").into(), temp.map(|t| format!("{t:.0} °C")).unwrap_or_else(|| na.to_string()), temp.map(|t| t / 100.0)),
            ];
            let gap = ui.dp(10.0);
            let tw = (ui.w - 2.0 * m - gap) / 2.0;
            let th = ui.dp(76.0);
            for (i, (lab, val, f)) in tiles.iter().enumerate() {
                let col = if ui.rtl { 1 - i % 2 } else { i % 2 };
                let r = Rect::new(m + col as f32 * (tw + gap), y + (i / 2) as f32 * (th + gap), tw, th);
                ui.card(r);
                let inn = r.inset(ui.dp(12.0));
                let lf = ui.fit(lab, 12.0, false, inn.w);
                ui.label(Rect::new(inn.x, inn.y, inn.w, ui.dp(16.0)), &lf, 12.0, cs, false, Al::Start);
                let vs = if ui.text_w(val, 18.0, true) <= inn.w { 18.0 } else { 14.0 };
                let vf = ui.fit(val, vs, true, inn.w);
                ui.label(Rect::new(inn.x, inn.y + ui.dp(18.0), inn.w, ui.dp(24.0)), &vf, vs, c, true, Al::Start);
                let br = Rect::new(inn.x, inn.bottom() - ui.dp(5.0), inn.w, ui.dp(5.0));
                ui.cv.rrect(br, ui.dp(2.5), ui.th.line);
                if let Some(f) = f {
                    let fw = (br.w * f.clamp(0.0, 1.0)).max(ui.dp(5.0));
                    ui.cv.rrect(if ui.rtl { Rect::new(br.right() - fw, br.y, fw, br.h) } else { Rect::new(br.x, br.y, fw, br.h) }, ui.dp(2.5), ui.th.accent);
                }
            }
            y += 2.0 * (th + gap) + ui.dp(4.0);
            let note = self.t("The interface itself needs ~65 MB RAM – Android over 1 GB.");
            y += ui.para(m + ui.dp(4.0), y, ui.w - 2.0 * m - ui.dp(8.0), note, 12.5, cs) + ui.dp(16.0);
        } else if self.mon_tab == 2 {
            ui.kb.row_end.set(-1.0);
            for (name, rss) in self.mon.top.clone() {
                ui.row(&mut y, None, &name, None, Some(&sys::human(rss)), false);
            }
        } else {
            let up: f32 = self.mon.net.values().map(|v| v.1).sum();
            let down: f32 = self.mon.net.values().map(|v| v.0).sum();
            let peak = self.mon.up_hist.iter().chain(self.mon.down_hist.iter()).cloned().fold(1024.0, f32::max);
            self.meter(ui, &mut y, self.t("Upload"), &rate(up), Some(up / peak));
            self.meter(ui, &mut y, self.t("Download"), &rate(down), Some(down / peak));
            let (uh, dh) = (self.mon.up_hist.clone(), self.mon.down_hist.clone());
            self.graph(ui, &mut y, &uh, Some(&dh), peak);
            let s = format!("{} · max. {}", self.t("Upload coloured, download grey, last minute"), rate(peak));
            self.caption(ui, &mut y, &s);
            ui.section(&mut y, self.t("Per connection (since start)"));
            for (n, (r, t, rx, tx)) in self.mon.net.clone() {
                let sub = format!("{}: {} ({})  ·  {}: {} ({})", self.t("Upload"), rate(t), amount(tx), self.t("Download"), rate(r), amount(rx));
                ui.row(&mut y, None, &n, Some(&sub), None, false);
            }
            if self.mon.net.is_empty() {
                self.caption(ui, &mut y, self.t("No network device found"));
            }
            ui.section(&mut y, self.t("Per day"));
            for (day, (rx, tx)) in self.mon.usage.days.clone().into_iter().rev().take(7) {
                let sub = format!("{}: {}  ·  {}: {}", self.t("Upload"), amount(tx), self.t("Download"), amount(rx));
                ui.row(&mut y, None, &day, Some(&sub), None, false);
            }
        }
        ui.scroll_end(body, y + *sc - body.y, sc);
    }

    pub(super) fn monitor_init(&mut self) {
        self.mon.usage = sys::load("net-usage.toml");
    }

    /// Im Hintergrund weiterzählen (Verbrauch je Tag); true = Monitor ist offen und soll neu gezeichnet werden.
    pub(super) fn monitor_tick(&mut self) -> bool {
        let open = matches!(self.stack.last().map(|s| &s.0), Some(Screen::Monitor));
        self.mon.sample(self.set.tz_min);
        open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_and_netdev() {
        let s = parse_stat("cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 60 0 20 400 20 0 0 0\nintr 5\n");
        assert_eq!(s, vec![(1000, 850), (500, 420)]);
        let n = parse_netdev("Inter-|   Receive\n face |bytes\n    lo: 10 1 0 0 0 0 0 0 10 1 0 0 0 0 0 0\n  eth0: 5000 9 0 0 0 0 0 0 1200 7 0 0 0 0 0 0\n");
        assert_eq!(n["eth0"], (5000, 1200));
        assert_eq!(n["lo"], (10, 10));
        assert_eq!(rate(2048.0), "2 kB/s");
        assert_eq!(amount(3 * 1_048_576), "3.0 MB");
    }
}
