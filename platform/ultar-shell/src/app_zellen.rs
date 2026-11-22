//! Zellen & Container: Apps abgeschottet (eigener Speicher, eigenes Netz, VPN mit Killswitch, Protokoll),
//! verknüpfen statt doppelt installieren, Grafikkarten-Verteilung. Bedient das Werkzeug `zelle`.
use super::{App, Screen};
use crate::gfx::Rect;
use crate::ui::{Al, Btn, Ui};

fn zelle(args: &[&str]) -> Option<String> {
    let o = std::process::Command::new("zelle")
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn json(args: &[&str]) -> serde_json::Value {
    zelle(args)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Text eines Werkzeugs (Deutsch) in die Sprache der Oberfläche
fn sp(app: &App, s: &str) -> String {
    let z = match app.set.lang.as_str() {
        "ar" => ultar_sprache::Sprache::Ar,
        "en" => ultar_sprache::Sprache::En,
        _ => ultar_sprache::Sprache::De,
    };
    ultar_sprache::in_sprache(s, z)
}

fn absatz(ui: &mut Ui, y: &mut f32, s: &str) {
    let c = ui.th.sub;
    *y += ui.para(ui.dp(22.0), *y, ui.w - ui.dp(44.0), s, 13.0, c) + ui.dp(10.0);
}

fn liste() -> Vec<serde_json::Value> {
    crate::sys::memo("zellen", 1500, || {
        json(&["liste", "--json"])
            .as_array()
            .cloned()
            .unwrap_or_default()
    })
}

/// Empfehlung vor einer Installation (None = nichts Vergleichbares vorhanden)
pub(crate) fn empfehlung(datei: &str) -> Option<serde_json::Value> {
    let n = std::path::Path::new(datei)
        .file_name()?
        .to_string_lossy()
        .into_owned();
    let v = json(&["empfehlung", &n, "--json"]);
    v.get("familie").is_some().then_some(v)
}

/// Kurzstatus für die Einstellungsliste
pub(crate) fn kurz(app: &App) -> String {
    let l = liste();
    if l.is_empty() {
        return String::new();
    }
    let an = l
        .iter()
        .filter(|z| z["laeuft"].as_bool().unwrap_or(false))
        .count();
    format!("{} · {an} {}", l.len(), app.t("running"))
}

impl App {
    pub(super) fn s_zellen(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Cells & containers"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        absatz(ui, &mut y, self.t("Each cell runs an app or container sealed off: own storage that grows as needed, no access to other data, own network (optionally only via VPN with kill switch) and an optional log of what it sends."));
        ui.section(&mut y, self.t("Internet rules"));
        let r = crate::sys::memo("netzregeln", 1500, || json(&["netz", "--json"]));
        let mut geraet = r["geraet"].as_bool().unwrap_or(true);
        if ui.toggle_row(&mut y, self.t("Internet for the device itself"), Some(self.t("Off: the system, updates and apps without a cell stay offline – only allowed cells and services below get internet")), &mut geraet) {
            let _ = zelle(&["netz", "geraet", if geraet { "an" } else { "aus" }]);
            crate::sys::memo_clear();
        }
        let vorgabe = r["vorgabe"].as_str().unwrap_or("frei");
        y += ui.dp(8.0);
        absatz(ui, &mut y, self.t("Network of new cells"));
        if let Some(i) = ui.chips(
            &mut y,
            &[self.t("Unrestricted"), self.t("No network")],
            if vorgabe == "aus" { 1 } else { 0 },
        ) {
            let _ = zelle(&["netz", "vorgabe", if i == 0 { "frei" } else { "aus" }]);
            crate::sys::memo_clear();
        }
        if !geraet {
            absatz(ui, &mut y, self.t("System services allowed online (their remote commands, e.g. package installs, stay offline):"));
            let erlaubt: Vec<String> = r["erlaubt"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let mut namen: Vec<String> = crate::sys::memo("svc_namen", 5000, || {
                crate::sys::services()
                    .into_iter()
                    .map(|s| s.name)
                    .filter(|n| n != "ui" && n != "netzregeln")
                    .collect::<Vec<_>>()
            });
            for e in &erlaubt {
                if !namen.contains(e) {
                    namen.push(e.clone());
                }
            }
            for n in namen {
                let mut on = erlaubt.contains(&n);
                let titel = if n == "verbund" {
                    format!("{n} · {}", self.t("Device cluster"))
                } else {
                    n.clone()
                };
                if ui.toggle_row(&mut y, &titel, None, &mut on) {
                    let _ = zelle(&["netz", if on { "erlauben" } else { "verbieten" }, &n]);
                    crate::sys::memo_clear();
                }
            }
        }
        ui.section(&mut y, self.t("Cells & containers"));
        let l = liste();
        if l.is_empty() {
            absatz(ui, &mut y, self.t("No cells yet. When installing, Ultar suggests a cell or linking an existing app."));
        }
        for z in &l {
            let name = z["name"].as_str().unwrap_or("").to_string();
            let netz = z["netz"].as_str().unwrap_or("");
            let netz = match netz {
                "frei" => self.t("Unrestricted").to_string(),
                "aus" => self.t("No network").to_string(),
                n => n
                    .strip_prefix("über ")
                    .map(|v| format!("VPN {v}"))
                    .unwrap_or(n.to_string()),
            };
            let mut sub = format!("{netz} · {} MB", z["belegt_mb"].as_u64().unwrap_or(0));
            if z["protokoll"].as_bool().unwrap_or(false) {
                sub += &format!(" · {}", self.t("Log"));
            }
            if let Some(v) = z["verknuepft"].as_array().filter(|v| !v.is_empty()) {
                sub += &format!(
                    " · {} {}",
                    self.t("Linked with"),
                    v.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            let val = if z["laeuft"].as_bool().unwrap_or(false) {
                self.t("running")
            } else {
                ""
            };
            if ui.row(
                &mut y,
                None,
                &format!("{name} · {}", z["app"].as_str().unwrap_or("")),
                Some(&sub),
                Some(val),
                true,
            ) {
                self.go(Screen::Zelle(name));
            }
        }
        ui.section(&mut y, self.t("Graphics cards"));
        let g = crate::sys::memo("gpu", 3000, || json(&["gpu", "--json"]));
        let n = g["karten"].as_array().map(|a| a.len()).unwrap_or(0);
        if ui.row(
            &mut y,
            None,
            self.t("Graphics cards"),
            Some(&sp(self, g["wahl"]["begruendung"].as_str().unwrap_or(""))),
            Some(&n.to_string()),
            true,
        ) {
            self.go(Screen::Gpu);
        }
        ui.section(&mut y, self.t("Containers (Kiri / Docker)"));
        let k = crate::sys::memo("kiri", 5000, || json(&["kiri", "status", "--json"]));
        if let Some(v) = k["dienst"].as_str() {
            let n = k["container"].as_u64().unwrap_or(0).to_string();
            ui.row(
                &mut y,
                None,
                v,
                Some(self.t("Container service running")),
                Some(&n),
                false,
            );
        } else if k["installiert"].as_bool().unwrap_or(false) {
            absatz(
                ui,
                &mut y,
                self.t("Kiri is installed; start the service under Services (kiri)."),
            );
        } else {
            absatz(ui, &mut y, self.t("Kiri is not installed. Install it to run Docker containers natively (Rust, no daemon VM)."));
            let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0));
            if ui.button(r, self.t("Install Kiri"), Btn::Primary) {
                self.job = Some(super::Job {
                    title: "Kiri".into(),
                    lines: Vec::new(),
                    done: None,
                });
                self.bg.run(
                    "/usr/bin/zelle",
                    vec!["kiri".into(), "einrichten".into()],
                    crate::sys::Msg::JobLine,
                    crate::sys::Msg::JobDone,
                );
                self.go(Screen::Job);
            }
            y += ui.dp(58.0);
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    pub(super) fn s_zelle(&mut self, ui: &mut Ui, area: Rect, name: String, sc: &mut f32) {
        let area = self.header(ui, area, &name);
        let l = liste();
        let Some(z) = l.iter().find(|z| z["name"] == name.as_str()).cloned() else {
            return;
        };
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let setze = |feld: &str, wert: &str| {
            let _ = zelle(&["setze", &name, feld, wert]);
            crate::sys::memo_clear();
        };
        let laeuft = z["laeuft"].as_bool().unwrap_or(false);
        let bw = ui.w - ui.dp(32.0);
        if ui.button(
            Rect::new(ui.dp(16.0), y, bw, ui.dp(46.0)),
            self.t(if laeuft { "Stop" } else { "Start" }),
            if laeuft { Btn::Danger } else { Btn::Primary },
        ) {
            if laeuft {
                let _ = zelle(&["stopp", &name]);
            } else {
                self.launch = Some(format!("exec:zelle start '{}'", name.replace('\'', "")));
            }
            crate::sys::memo_clear();
        }
        y += ui.dp(58.0);
        ui.section(&mut y, self.t("Network"));
        let vpns: Vec<String> = l
            .iter()
            .filter(|v| v["vpn_zelle"].as_bool().unwrap_or(false))
            .filter_map(|v| v["name"].as_str().map(String::from))
            .collect();
        let netz = z["netz"].as_str().unwrap_or("frei").to_string();
        let mut opts: Vec<String> =
            vec![self.t("Unrestricted").into(), self.t("No network").into()];
        opts.extend(vpns.iter().map(|v| format!("{} {v}", self.t("Only via"))));
        let sel = if netz == "aus" {
            1
        } else {
            vpns.iter()
                .position(|v| netz == format!("über {v}"))
                .map(|i| i + 2)
                .unwrap_or(0)
        };
        let o: Vec<&str> = opts.iter().map(|s| s.as_str()).collect();
        if let Some(i) = ui.chips(&mut y, &o, sel) {
            match i {
                0 => setze("netz.art", "frei"),
                1 => setze("netz.art", "aus"),
                i => {
                    setze("netz.ueber", &vpns[i - 2]);
                    setze("netz.art", "ueber");
                }
            }
        }
        if vpns.is_empty() {
            absatz(ui, &mut y, self.t("For a VPN with kill switch: create a VPN cell (zelle neu vpn --vpn-zelle wg0) or a WireGuard cell. If the VPN drops, the app has no network at all – nothing leaks."));
        }
        ui.section(&mut y, self.t("Privacy"));
        let toml_s = std::fs::read_to_string(format!(
            "{}/{name}/zelle.toml",
            std::env::var("ULTAR_ZELLEN").unwrap_or_else(|_| "/data/ultar/zellen".into())
        ))
        .unwrap_or_default();
        let t: toml::Value =
            toml::from_str(&toml_s).unwrap_or(toml::Value::Table(Default::default()));
        let b = |a: &str, k: &str| {
            t.get(a)
                .and_then(|x| x.get(k))
                .and_then(|x| x.as_bool())
                .unwrap_or(false)
        };
        for (feld, titel, sub) in [
            (
                "privat.standort",
                "Location",
                "GPS receiver and location sensors",
            ),
            (
                "privat.geraeteinfo",
                "Device info",
                "Real processor, device ID, serial numbers, battery",
            ),
            ("privat.kamera", "Camera", ""),
            ("privat.mikrofon", "Microphone", ""),
            ("privat.usb", "USB devices", ""),
        ] {
            let (a, k) = feld.split_once('.').unwrap();
            let mut on = b(a, k);
            if ui.toggle_row(
                &mut y,
                self.t(titel),
                if sub.is_empty() {
                    None
                } else {
                    Some(self.t(sub))
                },
                &mut on,
            ) {
                setze(feld, if on { "true" } else { "false" });
            }
        }
        ui.section(&mut y, self.t("Identity shown to the app"));
        absatz(ui, &mut y, self.t("The app only sees these values: location (also for Firefox and the Ultar location service), time zone and language. Empty = none / device setting."));
        let s_ = |a: &str, k: &str| {
            t.get(a)
                .and_then(|x| x.get(k))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string()
        };
        let fw = ui.w - ui.dp(32.0);
        for (id, feld, hint) in [
            ("z_ort", "privat.ort", "33.5138,36.2765"),
            ("z_tz", "privat.zeitzone", "Asia/Damascus"),
            ("z_lang", "privat.sprache", "ar"),
        ] {
            let (a, k) = feld.split_once('.').unwrap();
            let jetzt = s_(a, k);
            let titel = match id {
                "z_ort" => self.t("Location (latitude,longitude)"),
                "z_tz" => self.t("Time zone"),
                _ => self.t("Language"),
            };
            ui.label(
                Rect::new(ui.dp(22.0), y, fw, ui.dp(20.0)),
                titel,
                12.0,
                ui.th.sub,
                false,
                Al::Start,
            );
            y += ui.dp(22.0);
            let wert = if self.fields.contains_key(id) {
                self.f(id)
            } else {
                jetzt.clone()
            };
            ui.field(Rect::new(ui.dp(16.0), y, fw, ui.dp(46.0)), id, &wert, hint);
            y += ui.dp(54.0);
        }
        if ui.button(
            Rect::new(ui.dp(16.0), y, fw, ui.dp(44.0)),
            self.t("Apply"),
            Btn::Primary,
        ) {
            let mut ok = true;
            for (id, feld) in [
                ("z_ort", "privat.ort"),
                ("z_tz", "privat.zeitzone"),
                ("z_lang", "privat.sprache"),
            ] {
                if let Some(v) = self.fields.get(id).cloned() {
                    let v = v.trim().replace(' ', "");
                    if id == "z_ort"
                        && !v.is_empty()
                        && !v.split_once(',').is_some_and(|(b, l)| {
                            b.parse::<f64>().is_ok_and(|x| x.abs() <= 90.0)
                                && l.parse::<f64>().is_ok_and(|x| x.abs() <= 180.0)
                        })
                    {
                        ok = false;
                        continue;
                    }
                    setze(feld, &v);
                }
            }
            for id in ["z_ort", "z_tz", "z_lang"] {
                self.fields.remove(id);
            }
            self.focus = None;
            self.toast(self.t(if ok { "Saved" } else { "Invalid input" }).into());
        }
        y += ui.dp(56.0);
        ui.section(&mut y, self.t("Log"));
        let (mut verb, mut up) = (b("protokoll", "verbindungen"), b("protokoll", "uploads"));
        if ui.toggle_row(
            &mut y,
            self.t("Log connections"),
            Some(self.t("Destination, name (DNS/SNI) and bytes sent per connection")),
            &mut verb,
        ) {
            setze(
                "protokoll.verbindungen",
                if verb { "true" } else { "false" },
            );
        }
        if ui.toggle_row(
            &mut y,
            self.t("Log uploads"),
            Some(self.t("Start of every unencrypted upload; for HTTPS only destination and size")),
            &mut up,
        ) {
            setze("protokoll.uploads", if up { "true" } else { "false" });
            if up {
                setze("protokoll.verbindungen", "true");
            }
        }
        if verb {
            for zeile in sp(
                self,
                &zelle(&["protokoll", &name, "--n", "15"]).unwrap_or_default(),
            )
            .lines()
            {
                ui.label(
                    Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(20.0)),
                    zeile,
                    11.0,
                    ui.th.sub,
                    false,
                    Al::Start,
                );
                y += ui.dp(20.0);
            }
        }
        ui.section(&mut y, self.t("Storage"));
        absatz(
            ui,
            &mut y,
            &format!(
                "{} MB {} · {}",
                z["belegt_mb"].as_u64().unwrap_or(0),
                self.t("used"),
                self.t("Grows as needed while storage is free; no access to other cells.")
            ),
        );
        let ram = z["ram_mb"].as_u64().unwrap_or(0);
        let stufen = [0u64, 512, 1024, 2048, 4096, 8192];
        let lab: Vec<String> = stufen
            .iter()
            .map(|s| {
                if *s == 0 {
                    self.t("RAM dynamic").to_string()
                } else {
                    format!("{} GB", *s as f32 / 1024.0)
                }
            })
            .collect();
        let o: Vec<&str> = lab.iter().map(|s| s.as_str()).collect();
        if let Some(i) = ui.chips(
            &mut y,
            &o,
            stufen.iter().position(|s| *s == ram).unwrap_or(0),
        ) {
            setze("speicher.ram_mb", &stufen[i].to_string());
        }
        ui.section(&mut y, self.t("Linked with"));
        absatz(ui, &mut y, self.t("Linked cells share a folder (/data/gemeinsam/<name>) and can call each other by name."));
        let verkn: Vec<String> = z["verknuepft"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        for o in l
            .iter()
            .filter_map(|o| o["name"].as_str())
            .filter(|o| *o != name)
        {
            let mut on = verkn.iter().any(|v| v == o);
            if ui.toggle_row(&mut y, o, None, &mut on) {
                let _ = zelle(&[if on { "verknuepfen" } else { "trennen" }, &name, o]);
                crate::sys::memo_clear();
            }
        }
        ui.section(&mut y, self.t("Graphics cards"));
        let gpu = t
            .get("gpu")
            .and_then(|x| x.as_str())
            .unwrap_or("auto")
            .to_string();
        let gm = ["auto", "alle", "aus", "0", "1"];
        let gl = [
            self.t("Automatic"),
            self.t("All cards"),
            self.t("None"),
            "1",
            "2",
        ];
        if let Some(i) = ui.chips(&mut y, &gl, gm.iter().position(|m| *m == gpu).unwrap_or(0)) {
            setze("gpu", gm[i]);
        }
        y += ui.dp(10.0);
        if ui.button(
            Rect::new(ui.dp(16.0), y, bw, ui.dp(46.0)),
            self.t("Delete cell"),
            Btn::Danger,
        ) {
            let _ = zelle(&["loeschen", &name]);
            crate::sys::memo_clear();
            self.back();
        }
        y += ui.dp(58.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    pub(super) fn s_gpu(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Graphics cards"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let g = crate::sys::memo("gpu", 3000, || json(&["gpu", "--json"]));
        let karten = g["karten"].as_array().cloned().unwrap_or_default();
        for k in &karten {
            let sub = format!(
                "{} · {} / {} MB {} · {} %",
                k["treiber"].as_str().unwrap_or(""),
                k["vram_frei_mb"].as_u64().unwrap_or(0),
                k["vram_mb"].as_u64().unwrap_or(0),
                self.t("free"),
                k["last"].as_u64().unwrap_or(0)
            );
            ui.row(
                &mut y,
                None,
                &format!(
                    "{} · {}",
                    k["nr"].as_u64().unwrap_or(0) + 1,
                    k["name"].as_str().unwrap_or("?")
                ),
                Some(&sub),
                None,
                false,
            );
        }
        ui.section(&mut y, self.t("Distribution"));
        let modus = g["vorgabe"].as_str().unwrap_or("auto").to_string();
        let mut m: Vec<String> = vec!["auto".into(), "alle".into(), "aus".into()];
        m.extend((0..karten.len()).map(|i| i.to_string()));
        let mut lab: Vec<String> = vec![
            self.t("Automatic").into(),
            self.t("All cards").into(),
            self.t("Off").into(),
        ];
        lab.extend((0..karten.len()).map(|i| format!("{} {}", self.t("Only card"), i + 1)));
        let o: Vec<&str> = lab.iter().map(|s| s.as_str()).collect();
        if let Some(i) = ui.chips(&mut y, &o, m.iter().position(|x| *x == modus).unwrap_or(0)) {
            let _ = zelle(&["gpu", "modus", &m[i]]);
            crate::sys::memo_clear();
        }
        let b = sp(self, g["wahl"]["begruendung"].as_str().unwrap_or(""));
        absatz(ui, &mut y, &b);
        absatz(ui, &mut y, self.t("Combining adds up memory and computing power for AI and compute jobs (e.g. models split across both cards). Games and the desktop use one card at a time – no driver can merge two cards into one for rendering. Each cell can override this."));
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    /// vor der Installation: Vorhandenes verknüpfen oder trotzdem doppelt installieren
    pub(super) fn s_empfehlung(&mut self, ui: &mut Ui, area: Rect, datei: String, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Already installed"));
        let Some(e) = crate::sys::memo("empf", 60_000, || empfehlung(&datei)) else {
            self.install_file_direkt(&datei);
            return;
        };
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let fam = e["familie"].as_str().unwrap_or("");
        absatz(ui, &mut y, &format!("{} ({fam}) – {}", e["neu"].as_str().unwrap_or(""), self.t("you already have something similar. Instead of installing twice you can use it in a separate cell with its own profile (own data, network, VPN) or link it.")));
        let bw = ui.w - ui.dp(32.0);
        for v in e["vorhanden"].as_array().cloned().unwrap_or_default() {
            let art = v["art"].as_str().unwrap_or("");
            let kenn = v["kennung"].as_str().unwrap_or("").to_string();
            ui.row(
                &mut y,
                None,
                v["name"].as_str().unwrap_or(""),
                Some(match art {
                    "app" => "App",
                    "zelle" => self.t("Cell"),
                    _ => "Container",
                }),
                None,
                false,
            );
            let label = match art {
                "app" => self.t("Use in a new cell"),
                "container" => self.t("Link container"),
                _ => self.t("Open cell"),
            };
            if ui.button(
                Rect::new(ui.dp(16.0), y, bw, ui.dp(44.0)),
                label,
                Btn::Primary,
            ) {
                let neu: String = std::path::Path::new(&datei)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default()
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .take(20)
                    .collect();
                let neu = if neu.is_empty() {
                    format!("{kenn}-2")
                } else {
                    neu
                };
                crate::sys::memo_clear();
                self.back();
                match art {
                    "app" => {
                        let ok = zelle(&["neu", &neu, "--app", &kenn]).is_some();
                        self.toast(self.t(if ok { "Saved" } else { "Failed" }).into());
                        if ok {
                            self.go(Screen::Zelle(neu));
                        }
                    }
                    "zelle" => self.go(Screen::Zelle(kenn)),
                    _ => self.go(Screen::Zellen),
                }
                return;
            }
            y += ui.dp(56.0);
        }
        if ui.button(
            Rect::new(ui.dp(16.0), y, bw, ui.dp(44.0)),
            self.t("Install anyway (duplicate)"),
            Btn::Plain,
        ) {
            crate::sys::memo_clear();
            self.back();
            self.install_file_direkt(&datei);
        }
        y += ui.dp(56.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }
}
