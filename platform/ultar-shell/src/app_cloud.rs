//! Einstellungen → Konto (Nummer), VPN und Store/Updates. Backend: crate `ultar-cloud` (+ ultar-vpn).
//!
//! Netzaufrufe laufen beim Tippen auf Knöpfe (kurze Anfragen). Für lange Aktionen (Store-Download,
//! OTA) wäre ein Hintergrund-Thread besser — als nächster Schliff am Gerät vermerkt.
use super::{App, Screen};
use crate::gfx::{rgb, Rect};
use crate::ui::{Al, Btn, Ui};

#[derive(Default)]
pub struct CloudState {
    pub apps: Vec<ultar_cloud::AppMeta>,
    pub apps_loaded: bool,
}

impl App {
    // ---------- Konto + eindeutige Nummer ----------
    pub(super) fn s_account(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Account"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let m = ui.dp(22.0);

        let signed_in = ultar_cloud::token().is_some();
        if let Some(num) = ultar_cloud::my_number() {
            ui.section(&mut y, self.t("Your Ultar number"));
            ui.label(
                Rect::new(m, y, ui.w - 2.0 * m, ui.dp(34.0)),
                &num,
                24.0,
                ui.th.text,
                true,
                Al::Start,
            );
            y += ui.dp(42.0);
            self.caption(
                ui,
                &mut y,
                self.t("Others with Ultar OS can call you on this number."),
            );
        }

        if signed_in {
            ui.section(&mut y, self.t("Signed in"));
            if ultar_cloud::my_number().is_none() {
                if ui.button(
                    Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                    self.t("Get my number"),
                    Btn::Primary,
                ) {
                    match ultar_cloud::register_number() {
                        Ok(n) => self.toast(format!("{}: {n}", self.t("Number"))),
                        Err(e) => self.toast(e.to_string()),
                    }
                }
                y += ui.dp(56.0);
            }
        } else {
            ui.section(&mut y, self.t("Sign in or create account"));
            self.caption(ui, &mut y, self.t("One account for calls, store and VPN."));
            ui.field(
                Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                "acc_user",
                &self.f("acc_user"),
                self.t("Username"),
            );
            y += ui.dp(56.0);
            ui.field(
                Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                "acc_pw",
                &self.f("acc_pw"),
                self.t("Password"),
            );
            y += ui.dp(58.0);
            let bw = (ui.w - 2.0 * m - ui.dp(12.0)) / 2.0;
            if ui.button(
                Rect::new(m, y, bw, ui.dp(48.0)),
                self.t("Sign in"),
                Btn::Primary,
            ) {
                self.cloud_login(false);
            }
            if ui.button(
                Rect::new(m + bw + ui.dp(12.0), y, bw, ui.dp(48.0)),
                self.t("Create account"),
                Btn::Plain,
            ) {
                self.cloud_login(true);
            }
            y += ui.dp(56.0);
        }
        self.caption(
            ui,
            &mut y,
            &format!("{}: {}", self.t("Server"), ultar_cloud::base_url()),
        );
        ui.scroll_end(area, (y + *sc) - area.y + ui.dp(20.0), sc);
    }

    fn cloud_login(&mut self, create: bool) {
        let (u, p) = (self.f("acc_user"), self.f("acc_pw"));
        if u.len() < 3 || p.len() < 8 {
            self.toast(self.t("Username >=3, password >=8").into());
            return;
        }
        let r = if create {
            ultar_cloud::register(&u, &p)
        } else {
            ultar_cloud::login(&u, &p)
        };
        match r {
            Ok(_) => {
                // direkt Nummer holen
                let _ = ultar_cloud::register_number();
                self.toast(self.t("Signed in").into());
                self.fields.insert("acc_pw", String::new());
                self.focus = None;
            }
            Err(e) => self.toast(e.to_string()),
        }
    }

    // ---------- VPN ----------
    /// Geräteverbund: an/aus, Erlaubnisse, gefundene Geräte, Kopplungscode (bedient `ultar-verbund`)
    pub(super) fn s_verbund(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Device cluster (Ultar-Verbund)"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let m = ui.dp(22.0);
        self.caption(ui, &mut y, self.t("Connect your Ultar devices: they find each other on the same network, help each other and pool their power. Elsewhere they connect end-to-end encrypted via ultar.code-craft.tech."));
        y += ui.dp(8.0);
        let cfg = verbund_cfg();
        let get = |k: &str, d: bool| cfg.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
        let (mut an, mut steuern, mut rechnen) = (
            get("aktiv", false),
            get("steuern_erlauben", true),
            get("rechnen_erlauben", true),
        );
        if ui.toggle_row(&mut y, self.t("Cluster on"), None, &mut an) {
            verbund_cli(&[if an { "an" } else { "aus" }]);
        }
        if ui.toggle_row(
            &mut y,
            self.t("Allow control from paired devices"),
            None,
            &mut steuern,
        ) {
            verbund_cli(&["erlauben", "steuern", if steuern { "an" } else { "aus" }]);
        }
        if ui.toggle_row(&mut y, self.t("Share computing power"), None, &mut rechnen) {
            verbund_cli(&["erlauben", "rechnen", if rechnen { "an" } else { "aus" }]);
        }
        let (mut bild, mut ton, mut dateien) = (
            get("bildschirm_erlauben", true),
            get("ton_erlauben", true),
            get("dateien_erlauben", true),
        );
        if ui.toggle_row(
            &mut y,
            self.t("Allow screen sharing and remote control"),
            None,
            &mut bild,
        ) {
            verbund_cli(&["erlauben", "bildschirm", if bild { "an" } else { "aus" }]);
        }
        if ui.toggle_row(
            &mut y,
            self.t("Allow listening and announcements"),
            None,
            &mut ton,
        ) {
            verbund_cli(&["erlauben", "ton", if ton { "an" } else { "aus" }]);
        }
        if ui.toggle_row(
            &mut y,
            self.t("Accept files from paired devices"),
            None,
            &mut dateien,
        ) {
            verbund_cli(&["erlauben", "dateien", if dateien { "an" } else { "aus" }]);
        }
        ui.section(&mut y, self.t("Devices"));
        let z: serde_json::Value =
            std::fs::read_to_string(format!("{}/zustand.json", verbund_dir()))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
        let liste = z["geraete"].as_array().cloned().unwrap_or_default();
        if liste.is_empty() || !an {
            self.caption(ui, &mut y, self.t("No devices found yet."));
        }
        if an {
            for g in liste {
                let titel = format!(
                    "{} · {}",
                    g["name"].as_str().unwrap_or("?"),
                    g["ausgabe"].as_str().unwrap_or("")
                );
                let sub = format!(
                    "{}/{} {} · {}{}",
                    g["frei"].as_u64().unwrap_or(0),
                    g["kerne"].as_u64().unwrap_or(0),
                    self.t("cores free"),
                    if g["vertraut"].as_bool().unwrap_or(false) {
                        self.t("paired")
                    } else {
                        self.t("not paired")
                    },
                    if g["weg"] == "relais" {
                        format!(" · {}", self.t("via relay"))
                    } else {
                        String::new()
                    }
                );
                let vertraut = g["vertraut"].as_bool().unwrap_or(false);
                if ui.row(
                    &mut y,
                    None,
                    &titel,
                    Some(&sub),
                    if vertraut {
                        Some(self.t("View screen"))
                    } else {
                        None
                    },
                    vertraut,
                ) && vertraut
                {
                    if let Some(k) = g["kennung"].as_str() {
                        verbund_hintergrund(&["anzeigen", k, "--oeffnen"]);
                        self.toast(self.t("Opening remote screen …").into());
                    }
                }
            }
        }
        self.caption(ui, &mut y, self.t("Tip: hold anywhere, then swipe up – to hand over this app or the open picture to another device."));
        y += ui.dp(8.0);
        if an
            && ui.button(
                Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                self.t("Pair a device"),
                Btn::Primary,
            )
        {
            let code = format!(
                "{:06}",
                (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0)
                    ^ std::process::id().wrapping_mul(2654435761))
                    % 1_000_000
            );
            let _ = std::process::Command::new("ultar-verbund")
                .arg("koppeln")
                .env("ULTAR_VERBUND_DIR", verbund_dir())
                .env("ULTAR_VERBUND_CODE", &code)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            self.toast(format!(
                "{} {code}",
                self.t("Pairing code (5 min) – enter on the other device:")
            ));
        }
        y += ui.dp(56.0);

        // ---- Mining im Leerlauf ----
        ui.section(&mut y, self.t("Mining while idle"));
        let mk: serde_json::Value =
            std::fs::read_to_string(format!("{}/mining.json", verbund_dir()))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
        let mut mining = mk["aktiv"].as_bool().unwrap_or(false);
        let grund = z["mining"].as_str().unwrap_or("").to_string();
        self.caption(ui, &mut y, self.t("Unused devices mine Monero (XMR) – only when idle, on power and not too warm. Any use stops it at once. Ethereum can no longer be mined (proof of stake since 2022)."));
        if ui.toggle_row(
            &mut y,
            self.t("Mine Monero while idle"),
            if grund.is_empty() { None } else { Some(&grund) },
            &mut mining,
        ) {
            if mining
                && self.f("mine_wallet").trim().is_empty()
                && mk["wallet"].as_str().unwrap_or("").is_empty()
            {
                self.toast(self.t("Enter your Monero wallet address first").into());
            } else {
                if !self.f("mine_wallet").trim().is_empty() {
                    verbund_cli(&["mining", "wallet", self.f("mine_wallet").trim()]);
                }
                verbund_hintergrund(&["mining", if mining { "an" } else { "aus" }]);
            }
        }
        let wallet_alt = mk["wallet"].as_str().unwrap_or("").to_string();
        let hint = if wallet_alt.is_empty() {
            self.t("Monero wallet address (4… or 8…)").to_string()
        } else {
            format!("{}…", &wallet_alt[..wallet_alt.len().min(16)])
        };
        ui.field(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
            "mine_wallet",
            &self.f("mine_wallet"),
            &hint,
        );
        y += ui.dp(56.0);
        if ui.button(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
            self.t("Set up on all devices (one click)"),
            Btn::Primary,
        ) {
            let w = self.f("mine_wallet").trim().to_string();
            if w.is_empty() && wallet_alt.is_empty() {
                self.toast(self.t("Enter your Monero wallet address first").into());
            } else {
                if !w.is_empty() {
                    verbund_cli(&["mining", "wallet", &w]);
                }
                verbund_cli(&["mining", "an"]);
                verbund_hintergrund(&["mining", "alle"]);
                self.toast(
                    self.t("Mining is being set up on all paired devices")
                        .into(),
                );
            }
        }
        y += ui.dp(56.0);
        ui.scroll_end(area, (y + *sc) - area.y + ui.dp(20.0), sc);
    }

    /// Weitergeben (Geste): Ziel wählen – Datei dort öffnen oder diese App dort zeigen
    pub(super) fn s_weitergeben(
        &mut self,
        ui: &mut Ui,
        area: Rect,
        datei: Option<String>,
        sc: &mut f32,
    ) {
        let area = self.header(ui, area, self.t("Hand over to …"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let was = match &datei {
            Some(p) => format!("{}: {}", self.t("File"), p.rsplit('/').next().unwrap_or(p)),
            None => self
                .t("This app – keep working on the other device, it keeps running here")
                .to_string(),
        };
        self.caption(ui, &mut y, &was);
        y += ui.dp(6.0);
        let z: serde_json::Value =
            std::fs::read_to_string(format!("{}/zustand.json", verbund_dir()))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
        let liste: Vec<serde_json::Value> = z["geraete"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|g| g["vertraut"].as_bool().unwrap_or(false))
            .collect();
        if liste.is_empty() {
            self.caption(
                ui,
                &mut y,
                self.t("No paired device reachable. Settings → Device cluster."),
            );
        }
        for g in liste {
            let titel = format!(
                "{} · {}",
                g["name"].as_str().unwrap_or("?"),
                g["ausgabe"].as_str().unwrap_or("")
            );
            if ui.row(&mut y, None, &titel, None, Some("→"), true) {
                if let Some(k) = g["kennung"].as_str() {
                    match &datei {
                        Some(p) => verbund_hintergrund(&["weitergeben", k, p]),
                        None => verbund_hintergrund(&["weitergeben", k]),
                    }
                    self.toast(format!(
                        "{} {}",
                        self.t("Handed over to"),
                        g["name"].as_str().unwrap_or("?")
                    ));
                    self.back();
                }
            }
        }
        ui.scroll_end(area, (y + *sc) - area.y + ui.dp(20.0), sc);
    }

    pub(super) fn s_vpn(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("VPN (all traffic via your server)"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let m = ui.dp(22.0);

        if ultar_cloud::token().is_none() {
            self.caption(ui, &mut y, self.t("Sign in first (Account)."));
            ui.scroll_end(area, (y + *sc) - area.y + ui.dp(20.0), sc);
            return;
        }

        let st = crate::sys::vpn_status();
        let state = if st.up {
            self.t("Connected")
        } else {
            self.t("Disconnected")
        };
        ui.section(&mut y, self.t("Status"));
        ui.label(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(28.0)),
            state,
            18.0,
            if st.up { ui.th.text } else { ui.th.sub },
            true,
            Al::Start,
        );
        y += ui.dp(34.0);
        if st.up {
            self.caption(
                ui,
                &mut y,
                &format!("{}  ↓{} ↑{}", st.address, st.rx, st.tx),
            );
        }
        self.caption(ui, &mut y, self.t("When on, nothing leaves the device directly — all traffic goes through your server."));

        if st.up {
            if ui.button(
                Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                self.t("Disconnect"),
                Btn::Danger,
            ) {
                let _ = {
                    crate::sys::memo_clear();
                    ultar_vpn::disconnect()
                };
                self.toast(self.t("Disconnected").into());
            }
            y += ui.dp(56.0);
        } else if ui.button(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
            self.t("Connect"),
            Btn::Primary,
        ) {
            // provisionieren (falls nötig) + Tunnel hoch
            if ultar_vpn::load_saved().is_none() {
                if let Err(e) = ultar_cloud::vpn_provision() {
                    self.toast(e.to_string());
                    return;
                }
            }
            match {
                crate::sys::memo_clear();
                ultar_vpn::connect_saved()
            } {
                Ok(()) => self.toast(self.t("Connected").into()),
                Err(e) => self.toast(e),
            }
            y += ui.dp(56.0);
        }
        ui.scroll_end(area, (y + *sc) - area.y + ui.dp(20.0), sc);
    }

    // ---------- Store ----------
    /// Ultar Store wie in der Web-Demo: Empfohlen-Karte, App-Liste mit Installieren, darunter Ubuntu (apt)
    pub(super) fn s_store_cloud(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        use crate::ui::Ic;
        let area = self.header(ui, area, self.t("Ultar Store"));
        ui.scroll_begin(area, sc);
        let m = ui.dp(16.0);
        let mut y = area.y - *sc;
        let (c, cs) = (ui.th.text, ui.th.sub);
        let signed = ultar_cloud::token().is_some();
        let apps = self.cloud.apps.clone();
        // Empfohlen
        let fr = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(104.0));
        let ac = ui.th.accent;
        ui.cv.rrect(fr, ui.dp(20.0), ui.th.line);
        ui.cv.rrect(fr.inset(1.0), ui.dp(19.0), ui.th.card);
        ui.cv.rrect(fr.inset(1.0), ui.dp(19.0), ac.alpha(0.12));
        let inn = Rect::new(
            fr.x + ui.dp(18.0),
            fr.y + ui.dp(16.0),
            fr.w - ui.dp(36.0),
            ui.dp(18.0),
        );
        ui.label(inn, self.t("Featured"), 12.0, cs, false, Al::Start);
        let feat = apps
            .first()
            .map(|a| format!("{} {}", a.name, a.version))
            .unwrap_or_else(|| "KM POS".into());
        ui.label(
            Rect::new(inn.x, inn.y + ui.dp(18.0), inn.w, ui.dp(26.0)),
            &feat,
            20.0,
            c,
            true,
            Al::Start,
        );
        ui.para(
            inn.x,
            inn.y + ui.dp(46.0),
            inn.w,
            self.t("Every app is signed with Ed25519 and verified on the device."),
            13.0,
            cs,
        );
        y += fr.h + ui.dp(14.0);
        // Apps
        let rh = ui.dp(70.0);
        let rows: usize = if !signed || !self.cloud.apps_loaded || apps.is_empty() {
            1
        } else {
            apps.len()
        };
        let lr = Rect::new(m, y, ui.w - 2.0 * m, rh * rows as f32);
        ui.card(lr);
        if !signed || !self.cloud.apps_loaded || apps.is_empty() {
            let r = Rect::new(m, y, ui.w - 2.0 * m, rh);
            let (msg, act) = if !signed {
                (self.t("Sign in first (Account)."), self.t("Account"))
            } else if !self.cloud.apps_loaded {
                (self.t("Apps from the Ultar server"), self.t("Load apps"))
            } else {
                (self.t("No apps available yet."), "")
            };
            ui.label(
                Rect::new(r.x + ui.dp(16.0), r.y, r.w - ui.dp(140.0), r.h),
                msg,
                14.0,
                cs,
                false,
                Al::Start,
            );
            if !act.is_empty() {
                let bw = ui.text_w(act, 13.5, true) + ui.dp(28.0);
                let br = Rect::new(
                    if ui.rtl {
                        r.x + ui.dp(14.0)
                    } else {
                        r.right() - ui.dp(14.0) - bw
                    },
                    r.cy() - ui.dp(18.0),
                    bw,
                    ui.dp(36.0),
                );
                if ui.button(br, act, Btn::Primary) {
                    if !signed {
                        self.go(Screen::Account);
                    } else {
                        match ultar_cloud::store_list() {
                            Ok(a) => {
                                self.cloud.apps = a;
                                self.cloud.apps_loaded = true;
                            }
                            Err(e) => self.toast(e.to_string()),
                        }
                    }
                    ui.dirty = true;
                }
            }
            y += rh;
        } else {
            for (i, app) in apps.iter().enumerate() {
                let r = Rect::new(m, y, ui.w - 2.0 * m, rh);
                if i > 0 {
                    ui.cv
                        .rect(Rect::new(r.x + 1.0, r.y, r.w - 2.0, 1.0), ui.th.line);
                }
                let ix = if ui.rtl {
                    r.right() - ui.dp(14.0) - ui.dp(46.0)
                } else {
                    r.x + ui.dp(14.0)
                };
                let ir = Rect::new(ix, r.cy() - ui.dp(23.0), ui.dp(46.0), ui.dp(46.0));
                ui.cv.rrect(ir, ui.dp(14.0), rgb(0x2A2D35));
                ui.icon(Ic::Box, ir.cx(), ir.cy(), ui.dp(22.0), ui.th.accent);
                let bt = self.t("Install");
                let bw = ui.text_w(bt, 13.5, true) + ui.dp(28.0);
                let br = Rect::new(
                    if ui.rtl {
                        r.x + ui.dp(14.0)
                    } else {
                        r.right() - ui.dp(14.0) - bw
                    },
                    r.cy() - ui.dp(18.0),
                    bw,
                    ui.dp(36.0),
                );
                let tx = if ui.rtl {
                    br.right() + ui.dp(10.0)
                } else {
                    ir.right() + ui.dp(12.0)
                };
                let tw = r.w - ui.dp(46.0) - bw - ui.dp(50.0);
                let nf = ui.fit(&app.name, 15.0, false, tw);
                ui.label(
                    Rect::new(tx, r.y + ui.dp(15.0), tw, ui.dp(20.0)),
                    &nf,
                    15.0,
                    c,
                    false,
                    Al::Start,
                );
                let sub = format!(
                    "v{} · {}",
                    app.version,
                    crate::sys::human(app.size.max(0) as u64)
                );
                ui.label(
                    Rect::new(tx, r.y + ui.dp(37.0), tw, ui.dp(18.0)),
                    &sub,
                    12.5,
                    cs,
                    false,
                    Al::Start,
                );
                if ui.button(br, bt, Btn::Primary) {
                    match ultar_cloud::store_install(app) {
                        Ok(name) => {
                            ultar_notify::post(
                                "app",
                                "store",
                                &format!("{}: {name}", self.t("Installed")),
                                "",
                                ultar_notify::Urgency::Normal,
                            );
                            self.toast(format!("{}: {name}", self.t("Installed")));
                        }
                        Err(e) => self.toast(e.to_string()),
                    }
                    ui.dirty = true;
                }
                y += rh;
            }
        }
        y += ui.dp(14.0);
        // Ubuntu (apt)
        ui.section(&mut y, "Ubuntu (apt)");
        ui.kb.row_end.set(-1.0);
        for p in ["htop", "ffmpeg", "python3", "git", "gimp"] {
            let y0 = y;
            ui.kb.row_bg.set(Some(rgb(0x5A2A1C)));
            ui.row(
                &mut y,
                Some(Ic::Box),
                p,
                Some("Ubuntu 24.04"),
                Some("          "),
                false,
            );
            let r = Rect::new(m, y0, ui.w - 2.0 * m, y - y0);
            let bw = ui.dp(54.0);
            let br = Rect::new(
                if ui.rtl {
                    r.x + ui.dp(14.0)
                } else {
                    r.right() - ui.dp(14.0) - bw
                },
                r.cy() - ui.dp(18.0),
                bw,
                ui.dp(36.0),
            );
            if ui.button(br, "apt", Btn::Plain) {
                self.go(Screen::Pkg(p.to_string(), String::new()));
                ui.dirty = true;
            }
        }
        ui.kb.row_bg.set(Some(rgb(0x2A2D35)));
        if ui.row(
            &mut y,
            Some(Ic::Search),
            self.t("All Ubuntu packages"),
            None,
            None,
            true,
        ) {
            self.go(Screen::Store);
            ui.dirty = true;
        }
        y += ui.dp(20.0);
        ui.scroll_end(area, (y + *sc) - area.y, sc);
    }

    // ---------- Updates ----------
    pub(super) fn s_updates(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("System update"));
        let mut y = area.y + ui.dp(8.0);
        let m = ui.dp(22.0);
        if ultar_cloud::token().is_none() {
            self.caption(ui, &mut y, self.t("Sign in first (Account)."));
            return;
        }
        if ui.button(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
            self.t("Check for updates"),
            Btn::Primary,
        ) {
            match ultar_cloud::update_check("stable") {
                Ok(Some(rel)) => {
                    ultar_notify::post(
                        "system",
                        "update",
                        &format!("{} v{}", self.t("Update available"), rel.version),
                        "",
                        ultar_notify::Urgency::Normal,
                    );
                    self.toast(format!("{} v{}", self.t("Update available"), rel.version));
                }
                Ok(None) => self.toast(self.t("System is up to date").into()),
                Err(e) => self.toast(e.to_string()),
            }
        }
        y += ui.dp(58.0);
        self.caption(
            ui,
            &mut y,
            self.t("Updates are signed and hash-verified before they are applied."),
        );
    }
}

fn verbund_dir() -> String {
    std::env::var("ULTAR_VERBUND_DIR")
        .unwrap_or_else(|_| format!("{}/verbund", crate::sys::data_dir()))
}

fn verbund_cfg() -> serde_json::Value {
    std::fs::read_to_string(format!("{}/verbund.json", verbund_dir()))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// lange laufende Verbund-Befehle (Ansicht, Weitergeben, Mining einrichten) ohne die Oberfläche zu blockieren
fn verbund_hintergrund(args: &[&str]) {
    let _ = std::process::Command::new("ultar-verbund")
        .args(args)
        .env("ULTAR_VERBUND_DIR", verbund_dir())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// „auf <Gerät> <Anweisung>“ / „on <device> …“ / „على <جهاز> …“ → an den Helfer des Geräts (auch per Stimme)
pub(crate) fn verbund_weiterleiten(text: &str) -> Option<String> {
    let low = text.trim().to_lowercase();
    let z: serde_json::Value = std::fs::read_to_string(format!("{}/zustand.json", verbund_dir()))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())?;
    for g in z["geraete"]
        .as_array()?
        .iter()
        .filter(|g| g["vertraut"].as_bool().unwrap_or(false))
    {
        let name = g["name"].as_str()?.to_lowercase();
        for pre in ["auf ", "an ", "on ", "على ", "في "] {
            let kopf = format!("{pre}{name}");
            if let Some(rest) = low.strip_prefix(&kopf) {
                let rest = rest.trim_start_matches([':', ',', ' ']).trim();
                if rest.is_empty() {
                    return None;
                }
                // Originalschreibung behalten
                let orig: String = text
                    .trim()
                    .chars()
                    .skip(kopf.chars().count())
                    .collect::<String>()
                    .trim_start_matches([':', ',', ' '])
                    .trim()
                    .to_string();
                let k = g["kennung"].as_str()?.to_string();
                verbund_hintergrund(&["bildschirm", &k, "helfer", &orig]);
                let _ = rest;
                return Some(g["name"].as_str()?.to_string());
            }
        }
    }
    None
}

fn verbund_cli(args: &[&str]) {
    let _ = std::process::Command::new("ultar-verbund")
        .args(args)
        .env("ULTAR_VERBUND_DIR", verbund_dir())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Kurzstatus für die Einstellungsliste
pub(crate) fn verbund_kurz(app: &super::App) -> String {
    if !verbund_cfg()["aktiv"].as_bool().unwrap_or(false) {
        return app.t("Off").into();
    }
    let z: serde_json::Value = std::fs::read_to_string(format!("{}/zustand.json", verbund_dir()))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let n = z["geraete"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|g| g["vertraut"].as_bool().unwrap_or(false))
                .count()
        })
        .unwrap_or(0);
    format!("{n} {}", app.t("Devices"))
}
