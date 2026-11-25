//! Spotify-App für Ultar – zwei legitime Wege, kein DRM-Bypass:
//!  1) Web-Player (open.spotify.com) über den Ultar-Browser (braucht Widevine im WebKit; ohne das
//!     kann die Wiedergabe ausbleiben – dann Weg 2).
//!  2) **librespot** (offener Spotify-Connect-Client, braucht ein PREMIUM-Konto): läuft als
//!     Connect-Gerät „Ultar"; gesteuert wird aus der offiziellen Spotify-App auf einem anderen
//!     Gerät ODER aus dem Web-Player. Start/Stopp hier.
//!
//! Untermodul von `app`. Keine privaten Methoden anderer Module.

use super::App;
use crate::gfx::Rect;
use crate::ui::{Btn, Ui};

#[derive(Default)]
pub struct SpotifyState {
    #[allow(dead_code)] // written but never read; kept, see docs/OFFEN-WARNUNGEN.md
    pub note: Option<String>,
}

const LIBRESPOT_SH: &str = "/usr/lib/ultar/librespot.sh";
const LIBRESPOT_BIN: &str = "/usr/lib/ultar/librespot";

fn connect_running() -> bool {
    // grob: läuft ein librespot-Prozess?
    std::fs::read_dir("/proc")
        .map(|rd| {
            rd.flatten().any(|e| {
                let p = e.path().join("comm");
                std::fs::read_to_string(p)
                    .map(|s| s.trim() == "librespot")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

impl App {
    pub(super) fn s_spotify(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Spotify"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;

        if !crate::privacy::allowed(&self.set, crate::privacy::Cap::Network) {
            self.caption(ui, &mut y, self.t("Turned off in privacy settings"));
            ui.scroll_end(area, y + *sc - area.y, sc);
            return;
        }

        // Vorschaltseite wie in der Web-Demo, darunter das Connect-Gerät
        let txt = self.t("Official Spotify Connect: the device appears as speaker \"Ultar\".");
        y = self.webapp_page(
            ui,
            area,
            sc,
            "Spotify",
            crate::ui::Ic::Music,
            0x1DB954,
            "https://open.spotify.com/",
            txt,
        );

        // Weg 2: librespot Connect-Gerät
        ui.section(&mut y, self.t("Spotify Connect (librespot)"));
        let have = std::path::Path::new(LIBRESPOT_BIN).exists();
        if !have {
            self.caption(
                ui,
                &mut y,
                self.t("librespot is not installed yet (see docs)."),
            );
        } else {
            self.caption(ui, &mut y, self.t("Runs as the Connect device \"Ultar\". Needs Spotify Premium. Control it from the Spotify app or the web player."));
            let running = connect_running();
            let label = if running {
                self.t("Stop Connect device")
            } else {
                self.t("Start Connect device")
            };
            let kind = if running { Btn::Danger } else { Btn::Primary };
            if ui.button(
                Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
                label,
                kind,
            ) {
                if running {
                    let _ = std::process::Command::new("/system/bin/sh")
                        .arg("-c")
                        .arg("kill $(pidof librespot) 2>/dev/null")
                        .status();
                    self.toast(self.t("Stopped").into());
                } else {
                    let r = std::process::Command::new("/system/bin/sh")
                        .arg(LIBRESPOT_SH)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                    match r {
                        Ok(_) => self.toast(self.t("Starting…").into()),
                        Err(e) => self.toast(format!("{e}")),
                    }
                }
                ui.dirty = true;
            }
            y += ui.dp(64.0);
            let st = if running {
                self.t("Connect device is running")
            } else {
                self.t("Connect device is off")
            };
            self.caption(ui, &mut y, st);
        }

        ui.scroll_end(area, y + *sc - area.y, sc);
    }
}
