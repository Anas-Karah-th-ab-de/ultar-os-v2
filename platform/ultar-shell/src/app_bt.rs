//! Bluetooth-Bildschirm: Ein/Aus, eigene Adresse, Geräte in der Nähe suchen (klassisch + Low Energy).
//! Dahinter steht `ultar-bt` (eigener HCI-Host über /dev/scsc_h4_0 – der Kernel des A30 hat kein Bluetooth).
//! Koppeln und Profile (Audio, Tastatur/Maus) fehlen noch und werden hier ehrlich so angezeigt.

use super::{App, Screen};
use crate::gfx::Rect;
use crate::ui::{Btn, Ic, Ui};
use std::sync::Mutex;

/// Schalter: Datei vorhanden = Bluetooth an (Geräte ohne rfkill, z. B. A30).
pub const BT_FLAG: &str = "/data/ultar/bt.on";
const BIN: &[&str] = &["/usr/bin/ultar-bt", "/data/ultar/ultar-bt"];

#[derive(Clone, Default)]
pub struct BtDev {
    pub addr: String,
    pub le: bool,
    pub rssi: Option<i32>,
    pub class: u32,
    pub name: String,
}

#[derive(Default)]
struct BtState {
    scanning: bool,
    addr: String,
    devs: Vec<BtDev>,
    error: String,
    scanned: bool,
}

static STATE: Mutex<Option<BtState>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut BtState) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(BtState::default))
}

pub fn available() -> bool {
    std::path::Path::new("/dev/scsc_h4_0").exists() && BIN.iter().any(|b| std::path::Path::new(b).exists())
}

pub fn enabled() -> bool {
    std::path::Path::new(BT_FLAG).exists()
}

pub fn set_enabled(on: bool) {
    if on {
        let _ = std::fs::write(BT_FLAG, "1");
    } else {
        let _ = std::fs::remove_file(BT_FLAG);
        with(|s| {
            s.devs.clear();
            s.scanned = false;
        });
    }
}

/// Ausgabe von `ultar-bt scan` lesen: "adresse\t…", "ADDR\tle|br\tRSSI\tKLASSE\tNAME".
pub fn parse_scan(out: &str) -> (String, Vec<BtDev>) {
    let (mut own, mut devs) = (String::new(), Vec::new());
    for l in out.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        match f.as_slice() {
            ["adresse", a] => own = a.to_string(),
            [a, kind @ ("le" | "br"), rssi, class, name] if a.len() == 17 => devs.push(BtDev {
                addr: a.to_string(),
                le: *kind == "le",
                rssi: rssi.parse().ok(),
                class: u32::from_str_radix(class, 16).unwrap_or(0),
                name: name.to_string(),
            }),
            _ => {}
        }
    }
    (own, devs)
}

/// Geräteart aus der Geräteklasse (Hauptklasse, Bits 8–12).
fn kind_key(d: &BtDev) -> &'static str {
    match (d.class >> 8) & 0x1f {
        1 => "Computer",
        2 => "Phone",
        4 => "Audio",
        5 => "Keyboard / mouse",
        7 => "Wearable",
        _ if d.le => "Low Energy device",
        _ => "Device",
    }
}

fn start_scan() {
    if with(|s| std::mem::replace(&mut s.scanning, true)) {
        return;
    }
    std::thread::spawn(|| {
        let bin = BIN.iter().find(|b| std::path::Path::new(b).exists()).copied().unwrap_or(BIN[0]);
        let out = std::process::Command::new(bin).args(["scan", "10"]).output();
        with(|s| {
            s.scanning = false;
            s.scanned = true;
            match out {
                Ok(o) if o.status.success() => {
                    let (own, devs) = parse_scan(&String::from_utf8_lossy(&o.stdout));
                    s.addr = own;
                    s.devs = devs;
                    s.error.clear();
                }
                Ok(o) => s.error = String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("").to_string(),
                Err(e) => s.error = e.to_string(),
            }
        });
    });
}

impl App {
    pub(super) fn s_bluetooth(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Bluetooth"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        if !available() {
            self.caption(ui, &mut y, self.t("No Bluetooth chip found on this device"));
            ui.scroll_end(area, y + *sc - area.y, sc);
            return;
        }
        if !crate::privacy::allowed(&self.set, crate::privacy::Cap::Bluetooth) {
            self.caption(ui, &mut y, self.t("Turned off in privacy settings"));
            ui.scroll_end(area, y + *sc - area.y, sc);
            return;
        }
        let mut on = enabled();
        if ui.toggle_row(&mut y, self.t("Bluetooth"), None, &mut on) {
            set_enabled(on);
            if on {
                start_scan();
            }
        }
        if !on {
            ui.scroll_end(area, y + *sc - area.y, sc);
            return;
        }
        let (scanning, addr, devs, error, scanned) = with(|s| (s.scanning, s.addr.clone(), s.devs.clone(), s.error.clone(), s.scanned));
        if !addr.is_empty() {
            ui.row(&mut y, Some(Ic::Chip), self.t("This device"), None, Some(&addr), false);
        }
        y += ui.dp(8.0);
        let label = if scanning { self.t("Searching…") } else { self.t("Search for devices") };
        if ui.button(Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(48.0)), label, if scanning { Btn::Plain } else { Btn::Primary }) && !scanning {
            start_scan();
        }
        y += ui.dp(60.0);
        if !error.is_empty() {
            self.caption(ui, &mut y, &error);
        }
        if !scanning && !scanned {
            start_scan();
        }
        ui.section(&mut y, self.t("Devices nearby"));
        if devs.is_empty() {
            self.caption(ui, &mut y, self.t(if scanning { "Searching…" } else { "No devices found" }));
        }
        for d in &devs {
            let title = if d.name.is_empty() { d.addr.clone() } else { d.name.clone() };
            let sub = format!("{}{}", self.t(kind_key(d)), if d.name.is_empty() { String::new() } else { format!("  ·  {}", d.addr) });
            let val = d.rssi.map(|r| format!("{r} dBm"));
            ui.row(&mut y, Some(Ic::Wifi), &title, Some(&sub), val.as_deref(), false);
        }
        y += ui.dp(10.0);
        self.caption(ui, &mut y, self.t("Pairing, audio and keyboards are not available yet"));
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    #[allow(dead_code)]
    pub(super) fn bluetooth_open(&self) -> bool {
        matches!(self.stack.last().map(|s| &s.0), Some(Screen::Bluetooth))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tool_output() {
        let out = "adresse\tAE:6C:AC:46:8F:CE\nversion\thci=9 hersteller=0x0075 lmp_sub=0x0000\n23:28:C3:C9:F9:16\tle\t-52\t000000\t\n11:22:33:44:55:66\tbr\t-60\t240404\tBox\ngefunden\t2\n";
        let (own, d) = parse_scan(out);
        assert_eq!(own, "AE:6C:AC:46:8F:CE");
        assert_eq!(d.len(), 2);
        assert!(d[0].le && d[0].rssi == Some(-52) && d[0].name.is_empty());
        assert_eq!((d[1].name.as_str(), kind_key(&d[1])), ("Box", "Audio"));
    }
}
