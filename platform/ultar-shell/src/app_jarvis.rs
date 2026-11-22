//! KM als System-Assistent „wie Jarvis" (Untermodul von `app`):
//! - **sieht** den Bildschirm (Text jeder Seite + Bildschirmfoto der zuletzt benutzten Seite), auf Wunsch den
//!   Nutzer (Frontkamera, mit Rückfrage) und alles, was der Nutzer ihm bringt (Fotos, Bildschirm, Dateien);
//! - hat **Root-Zugriff** auf das System (Prozesse, Dateien, Netz, Protokolle, Befehle) – Root-, Sicherheits-,
//!   Kamera- und Datei-Eingriffe laufen nur nach Bestätigung (`agent::Call::always_confirm`);
//! - handelt **nur auf Anfrage** – Werkzeuge laufen ausschließlich innerhalb einer vom Nutzer begonnenen Runde;
//! - denkt mit dem Sprachmodell **direkt im Ultar-Kern** (Dienst `ultar-llm`, kein Ubuntu-Bereich). Modelle kommen
//!   von Hugging Face oder Ollama, passend zum Gerät empfohlen (`ultar_llm::models`).

use super::{App, Job, Screen};
use crate::agent::{self, Call};
use crate::gfx::{rgb, Rect};
use crate::sys::{self, Msg};
use crate::ui::{Al, Ic, Ui};
use std::time::{Duration, Instant};
use ultar_llm::models;

/// Ordner für Bilder an den Assistenten (privat, im Speicher)
fn img_dir() -> String {
    let d = if std::path::Path::new("/run/ultar").is_dir() { "/run/ultar/agent".to_string() } else { std::env::temp_dir().join("ultar-agent").to_string_lossy().into_owned() };
    let _ = std::fs::create_dir_all(&d);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
    }
    d
}

/// Ordner der Modelle (wie Vorgabe von ultar-llm)
pub fn model_dir() -> String {
    std::env::var("ULTAR_MODEL_DIR").unwrap_or_else(|_| format!("{}/models", sys::data_dir()))
}

#[derive(Default)]
pub struct Jarvis {
    /// Letzte vom Nutzer benutzte Seite (nicht der Assistent selbst): verkleinertes Bild (XRGB), Text, Titel
    pub view: Option<(u32, u32, Vec<u32>)>,
    pub view_text: Vec<String>,
    view_scr: Option<Screen>,
    view_at: Option<Instant>,
    /// Mitgebrachte Dinge für die nächste Nachricht (Dateipfade)
    pub attach: Vec<String>,
    pub attach_open: bool,
    /// Quelle für Modelle: 0 Hugging Face, 1 Ollama
    pub src: u8,
    /// Gerade geladenes Modell (Kennung)
    pub pulling: Option<String>,
}

fn is_img(p: &str) -> bool {
    let l = p.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".webp", ".bmp"].iter().any(|e| l.ends_with(e))
}

/// XRGB-Bild als PNG speichern
fn save_png(path: &str, w: u32, h: u32, px: &[u32]) -> Result<(), String> {
    let mut buf = Vec::with_capacity((w * h * 3) as usize);
    for p in px {
        buf.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    image::save_buffer(path, &buf, w, h, image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())
}

fn read_trim(p: &str) -> String {
    std::fs::read_to_string(p).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// Prozesse aus /proc: (pid, Name, RSS in KB, CPU-Zeit in Ticks, Befehlszeile)
fn procs() -> Vec<(u32, String, u64, u64, String)> {
    let mut v = Vec::new();
    let Ok(rd) = std::fs::read_dir("/proc") else { return v };
    for e in rd.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        let (Some(a), Some(b)) = (stat.find('('), stat.rfind(')')) else { continue };
        let name = stat[a + 1..b].to_string();
        let f: Vec<&str> = stat[b + 2..].split_whitespace().collect();
        let ticks = f.get(11).and_then(|x| x.parse::<u64>().ok()).unwrap_or(0) + f.get(12).and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
        let rss = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default().lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| l.split_whitespace().nth(1)).and_then(|x| x.parse().ok()).unwrap_or(0);
        let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).map(|b| String::from_utf8_lossy(&b).replace('\0', " ").trim().to_string()).unwrap_or_default();
        v.push((pid, name, rss, ticks, cmd));
    }
    v
}

/// Socket-Inode → Prozess (pid, Name)
fn socket_owners() -> std::collections::HashMap<u64, (u32, String)> {
    let mut m = std::collections::HashMap::new();
    for (pid, name, ..) in procs() {
        let Ok(rd) = std::fs::read_dir(format!("/proc/{pid}/fd")) else { continue };
        for fd in rd.flatten() {
            if let Ok(t) = std::fs::read_link(fd.path()) {
                let t = t.to_string_lossy();
                if let Some(i) = t.strip_prefix("socket:[").and_then(|x| x.strip_suffix(']')).and_then(|x| x.parse::<u64>().ok()) {
                    m.insert(i, (pid, name.clone()));
                }
            }
        }
    }
    m
}

fn hex_addr(h: &str, v6: bool) -> String {
    let (ip, port) = h.split_once(':').unwrap_or((h, "0"));
    let port = u16::from_str_radix(port, 16).unwrap_or(0);
    if !v6 {
        let n = u32::from_str_radix(ip, 16).unwrap_or(0).to_le_bytes();
        format!("{}.{}.{}.{}:{port}", n[0], n[1], n[2], n[3])
    } else {
        let mut b = Vec::new();
        for i in 0..4 {
            let w = u32::from_str_radix(ip.get(i * 8..i * 8 + 8).unwrap_or("0"), 16).unwrap_or(0).to_le_bytes();
            b.extend_from_slice(&w);
        }
        let a: [u8; 16] = b.try_into().unwrap_or([0; 16]);
        let addr = std::net::Ipv6Addr::from(a);
        match addr.to_ipv4_mapped() {
            Some(v4) => format!("{v4}:{port}"),
            None => format!("[{addr}]:{port}"),
        }
    }
}

/// Offene Verbindungen und lauschende Ports mit Programm
pub fn connections() -> String {
    let owners = socket_owners();
    let mut est = Vec::new();
    let mut listen = Vec::new();
    for (file, proto, v6) in [("/proc/net/tcp", "tcp", false), ("/proc/net/tcp6", "tcp", true), ("/proc/net/udp", "udp", false), ("/proc/net/udp6", "udp", true)] {
        let t = std::fs::read_to_string(file).unwrap_or_default();
        for l in t.lines().skip(1) {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 10 {
                continue;
            }
            let (local, remote, st) = (hex_addr(f[1], v6), hex_addr(f[2], v6), f[3]);
            let inode: u64 = f[9].parse().unwrap_or(0);
            let who = owners.get(&inode).map(|(p, n)| format!("{n}[{p}]")).unwrap_or_else(|| "?".into());
            if (proto == "tcp" && st == "0A") || (proto == "udp" && remote.ends_with(":0") && st == "07") {
                listen.push(format!("{proto} {local} {who}"));
            } else if proto == "tcp" && st == "01" {
                est.push(format!("{who} {local} -> {remote}"));
            }
        }
    }
    est.sort();
    est.dedup();
    listen.sort();
    listen.dedup();
    format!("ESTABLISHED ({}):\n{}\nLISTENING ({}):\n{}", est.len(), if est.is_empty() { "none".into() } else { est.join("\n") }, listen.len(), if listen.is_empty() { "none".into() } else { listen.join("\n") })
}

impl App {
    /// Vor dem Zeichnen: soll dieses Bild für den Assistenten festgehalten werden?
    pub(super) fn jarvis_capture_due(&self) -> bool {
        let Some(top) = self.stack.last().map(|s| &s.0) else { return false };
        if matches!(top, Screen::Agent | Screen::AgentSet | Screen::Job | Screen::Setup(_) | Screen::WifiPw(..)) || self.sx.locked || self.pending.is_some() {
            return false;
        }
        self.jv.view_scr.as_ref() != Some(top) || self.jv.view_at.map_or(true, |t| t.elapsed() > Duration::from_millis(1500))
    }

    /// Nach dem Zeichnen: Text und verkleinertes Bild der Seite merken (höchstens ~1×/1,5 s, sonst bei Seitenwechsel)
    pub(super) fn jarvis_store(&mut self, ui: &mut Ui) {
        let text = ui.seen.take().unwrap_or_default();
        self.jv.view_text = text;
        self.jv.view_scr = self.stack.last().map(|s| s.0.clone());
        self.jv.view_at = Some(Instant::now());
        // halbe Auflösung reicht für Bild-Modelle (die verkleinern ohnehin auf ~448–1024 px)
        let (w, h) = (ui.cv.w.max(1) as u32, ui.cv.h.max(1) as u32);
        let k = if w > 800 { 2 } else { 1 };
        let (dw, dh) = (w / k, h / k);
        let mut px = Vec::with_capacity((dw * dh) as usize);
        for y in 0..dh {
            let row = (y * k * w) as usize;
            for x in 0..dw {
                px.push(ui.cv.px[row + (x * k) as usize]);
            }
        }
        self.jv.view = Some((dw, dh, px));
    }

    /// Hat das Gerät ein Bild-Modell (oder ist Claude gewählt, das Bilder sieht)?
    fn jarvis_can_see(&self) -> bool {
        if self.agent.backend == "claude" {
            return true;
        }
        let dir = model_dir();
        std::fs::read_dir(&dir).map(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().starts_with("mmproj"))).unwrap_or(false)
            || std::fs::read_dir(agent::MODEL_DIR).map(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().starts_with("mmproj"))).unwrap_or(false)
    }

    /// Bildschirmfoto der letzten Seite als Datei
    fn jarvis_screen_png(&self) -> Result<String, String> {
        let (w, h, px) = self.jv.view.as_ref().ok_or("no screen seen yet")?;
        let p = format!("{}/screen-{}.png", img_dir(), sys::unix_now());
        save_png(&p, *w, *h, px)?;
        Ok(p)
    }

    /// Ergebnis eines Werkzeugs mit Bild an das Modell geben
    pub(super) fn tool_result_img(&mut self, res: String, img: String) {
        let res: String = res.chars().take(1500).collect();
        self.history.push(agent::user_msg(&format!("TOOL_RESULT: {res}"), &[img]));
        self.request();
    }

    /// Werkzeuge des System-Assistenten. None = hier nicht bekannt. Bild-Ergebnisse gehen über `tool_result_img`.
    pub(super) fn exec_jarvis(&mut self, c: &Call) -> Option<Result<String, ()>> {
        let bg = self.bg.clone();
        let res = match c.tool.as_str() {
            "see_screen" => {
                let title = self.jv.view_scr.as_ref().map(|s| self.screen_label(s)).unwrap_or_default();
                if self.jv.view_text.is_empty() {
                    "The user has not looked at any app yet (only this conversation).".into()
                } else {
                    let mut t = format!("App/screen: {title}\nVisible text (top to bottom):\n");
                    for l in &self.jv.view_text {
                        if t.len() > 2600 {
                            t.push_str("…");
                            break;
                        }
                        t.push_str(l);
                        t.push('\n');
                    }
                    t
                }
            }
            "look_screen" => {
                if !self.jarvis_can_see() {
                    return Some(Ok("error: no vision model installed – use see_screen (text) instead, or install one in Settings → Local agent".into()));
                }
                match self.jarvis_screen_png() {
                    Ok(p) => {
                        let q = c.arg("question");
                        self.tool_result_img(format!("screenshot of the screen the user was looking at. {q}"), p);
                        return Some(Err(()));
                    }
                    Err(e) => format!("error: {e}"),
                }
            }
            "look_at_user" => {
                if !crate::privacy::allowed(&self.set, crate::privacy::Cap::Camera) {
                    return Some(Ok("error: the camera is switched off in privacy settings".into()));
                }
                if !self.jarvis_can_see() {
                    return Some(Ok("error: no vision model installed".into()));
                }
                let node = std::fs::read_to_string(format!("{}/camera-front.node", sys::data_dir())).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
                let data = sys::data_dir();
                std::thread::spawn(move || {
                    let w = crate::camera::CameraWorker::start(1, node, data);
                    let t0 = Instant::now();
                    let mut got = None;
                    while t0.elapsed() < Duration::from_secs(12) {
                        if let Some(f) = w.take_frame() {
                            // erste Bilder sind oft dunkel (Belichtung) – ein paar abwarten
                            got = Some(f);
                            if t0.elapsed() > Duration::from_millis(1500) {
                                break;
                            }
                        }
                        if w.status() == crate::camera::CamStatus::Failed {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(120));
                    }
                    let err = w.error();
                    w.stop();
                    match got {
                        Some((fw, fh, px)) => {
                            let p = format!("{}/user-{}.png", img_dir(), sys::unix_now());
                            match save_png(&p, fw, fh, &px) {
                                Ok(_) => bg.send(Msg::AgentToolImg("front camera photo of the user (taken just now, with permission)".into(), p)),
                                Err(e) => bg.send(Msg::AgentTool(format!("error: {e}"))),
                            }
                        }
                        None => bg.send(Msg::AgentTool(format!("error: camera gave no picture{}", err.map(|e| format!(" ({e})")).unwrap_or_default()))),
                    }
                });
                return Some(Err(()));
            }
            "system_info" => {
                let (mt, ma) = sys::meminfo();
                let load = read_trim("/proc/loadavg");
                let up = read_trim("/proc/uptime").split_whitespace().next().and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0) as u64;
                let temp = ultar_llm::platform::cpu_temp().map(|t| format!("{t:.0} °C")).unwrap_or_else(|| "n/a".into());
                let disk = |p: &str| sys::disk(p).map(|d| format!("{} free of {}", sys::human(d.1), sys::human(d.0))).unwrap_or_else(|| "n/a".into());
                let net: Vec<String> = sys::net_status().iter().map(|n| format!("{} {} {}", n.name, n.state, n.ip)).collect();
                let mut ps = procs();
                ps.sort_by(|a, b| b.2.cmp(&a.2));
                let top: Vec<String> = ps.iter().take(8).map(|p| format!("{} [{}] {} MB", p.1, p.0, p.2 / 1024)).collect();
                format!(
                    "kernel {}; uptime {}h{}m; load {load}; cpus {}; temp {temp}; ram {} free of {}; battery {:?}; storage system {}, user {}; network [{}]; processes {}; top memory: {}",
                    read_trim("/proc/sys/kernel/osrelease"),
                    up / 3600,
                    up % 3600 / 60,
                    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
                    sys::human(ma),
                    sys::human(mt),
                    sys::battery(),
                    disk("/"),
                    disk("/data"),
                    net.join(", "),
                    ps.len(),
                    top.join(", ")
                )
            }
            "processes" => {
                let mut ps = procs();
                if c.arg("sort") == "cpu" {
                    ps.sort_by(|a, b| b.3.cmp(&a.3));
                } else {
                    ps.sort_by(|a, b| b.2.cmp(&a.2));
                }
                ps.iter().take(25).map(|p| format!("{:>6} {:<18} {:>6} MB  cpu {:>7}  {}", p.0, p.1, p.2 / 1024, p.3, p.4.chars().take(60).collect::<String>())).collect::<Vec<_>>().join("\n")
            }
            "kill_process" => {
                let pid: i32 = c.arg("pid").trim().parse().unwrap_or(0);
                let me = std::process::id() as i32;
                if pid <= 1 || pid == me {
                    "error: this process cannot be stopped (init or the interface itself)".into()
                } else {
                    let sig = if c.arg("signal").eq_ignore_ascii_case("kill") { nix::sys::signal::Signal::SIGKILL } else { nix::sys::signal::Signal::SIGTERM };
                    match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), sig) {
                        Ok(_) => format!("ok: signal {sig:?} sent to {pid}"),
                        Err(e) => format!("error: {e}"),
                    }
                }
            }
            "network_connections" => connections(),
            "security_status" => {
                let s = &self.set;
                let groups = [("network", s.priv_net), ("camera", s.priv_cam), ("microphone", s.priv_mic), ("location", s.priv_loc), ("sensors", s.priv_sensors), ("bluetooth", s.priv_bt)];
                let g: Vec<String> = groups.iter().map(|(n, on)| format!("{n} {}", if s.priv_all && *on { "allowed" } else { "blocked" })).collect();
                let v = ultar_vpn::status();
                let vpn = if v.up { format!("on via {}", v.endpoint) } else { "off".to_string() };
                let listen = connections().split("LISTENING").nth(1).unwrap_or("").lines().take(12).collect::<Vec<_>>().join("; ");
                format!(
                    "device switch {}; {}; vpn {}; screen lock {}; assistant model {}; listening{}",
                    if s.priv_all { "on" } else { "OFF (everything blocked)" },
                    g.join(", "),
                    vpn.chars().take(120).collect::<String>(),
                    if self.sx.cfg.lock { "on" } else { "off" },
                    if self.agent.backend == "claude" { "Claude (cloud – data leaves the device)" } else { "local (nothing leaves the device)" },
                    listen
                )
            }
            "set_privacy" => {
                let on = matches!(c.arg("on").as_str(), "true" | "1" | "on");
                let s = &mut self.set;
                match c.arg("group").as_str() {
                    "all" => s.priv_all = on,
                    "network" => s.priv_net = on,
                    "camera" => s.priv_cam = on,
                    "microphone" => s.priv_mic = on,
                    "location" => s.priv_loc = on,
                    "sensors" => s.priv_sensors = on,
                    "bluetooth" => s.priv_bt = on,
                    _ => return Some(Ok("error: unknown group".into())),
                }
                crate::privacy::apply(&self.set);
                self.save();
                format!("ok: {} {}", c.arg("group"), if on { "allowed" } else { "blocked" })
            }
            "logs" => {
                let n: usize = c.arg("lines").trim().parse().unwrap_or(40).clamp(5, 200);
                let v = if c.arg("source") == "services" { crate::sysx::service_log(n) } else { crate::sysx::kernel_log(n) };
                if v.is_empty() { "(no log lines)".into() } else { v.join("\n") }
            }
            "list_dir" => {
                let p = c.arg("path");
                let p = if p.trim().is_empty() { "/".to_string() } else { p };
                match std::fs::read_dir(&p) {
                    Ok(rd) => {
                        let mut v: Vec<String> = rd
                            .flatten()
                            .take(200)
                            .map(|e| {
                                let m = e.metadata().ok();
                                let d = m.as_ref().is_some_and(|m| m.is_dir());
                                format!("{}{}{}", e.file_name().to_string_lossy(), if d { "/" } else { "" }, m.filter(|m| !m.is_dir()).map(|m| format!("  {}", sys::human(m.len()))).unwrap_or_default())
                            })
                            .collect();
                        v.sort();
                        if v.is_empty() { "(empty)".into() } else { v.join("\n") }
                    }
                    Err(e) => format!("error: {e}"),
                }
            }
            "read_any" => {
                let p = c.arg("path");
                match std::fs::read(&p) {
                    Ok(b) if p.starts_with("/proc/") || p.starts_with("/sys/") || ultar_office::looks_text(&b) => {
                        let max: usize = c.arg("max").trim().parse().unwrap_or(3000).clamp(200, 12000);
                        let t = String::from_utf8_lossy(&b);
                        let t: String = t.chars().take(max).collect();
                        if b.len() > max { format!("{t}\n… ({} bytes total)", b.len()) } else { t }
                    }
                    Ok(_) => match ultar_office::read_text(&p, 3000) {
                        Ok(t) => t,
                        Err(_) => format!("binary file, {} bytes", std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0)),
                    },
                    Err(e) => format!("error: {e}"),
                }
            }
            "write_file" => {
                let (p, content) = (c.arg("path"), c.arg("content"));
                if p.trim().is_empty() || !p.starts_with('/') {
                    "error: absolute path needed".into()
                } else {
                    let r = if matches!(c.arg("append").as_str(), "true" | "1") {
                        std::fs::OpenOptions::new().create(true).append(true).open(&p).and_then(|mut f| std::io::Write::write_all(&mut f, content.as_bytes()))
                    } else {
                        if let Some(d) = std::path::Path::new(&p).parent() {
                            let _ = std::fs::create_dir_all(d);
                        }
                        std::fs::write(&p, content.as_bytes())
                    };
                    match r {
                        Ok(_) => format!("ok: {} bytes written to {p}", content.len()),
                        Err(e) => format!("error: {e}"),
                    }
                }
            }
            "delete_path" => {
                let p = c.arg("path");
                let protected = ["/", "/data", "/data/ultar", "/usr", "/etc", "/sbin", "/bin", "/boot", "/proc", "/sys", "/dev", "/vendor", "/system"];
                if protected.contains(&p.trim_end_matches('/')) || p.trim().is_empty() || !p.starts_with('/') {
                    "error: this path is protected".into()
                } else {
                    let path = std::path::Path::new(&p);
                    let r = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
                    match r {
                        Ok(_) => format!("ok: deleted {p}"),
                        Err(e) => format!("error: {e}"),
                    }
                }
            }
            "run_command" => {
                let cmd = c.arg("command");
                let ubuntu = c.arg("where") == "ubuntu";
                std::thread::spawn(move || {
                    let wrapped = format!("({cmd}) 2>&1");
                    let out = if ubuntu {
                        if sys::compat_ready() { sys::capture(sys::COMPAT, &["run", "--", "/bin/bash", "-c", &wrapped]) } else { "error: Ubuntu area not available".into() }
                    } else if std::path::Path::new("/bin/sh").exists() {
                        // direkt im Ultar-System als root (kein Ubuntu-Bereich)
                        sys::capture("/bin/sh", &["-c", &wrapped])
                    } else if sys::compat_ready() {
                        sys::capture(sys::COMPAT, &["run", "--", "/bin/bash", "-c", &wrapped])
                    } else {
                        "error: no shell available".into()
                    };
                    let out: String = if out.chars().count() > 3000 { out.chars().rev().take(3000).collect::<Vec<_>>().into_iter().rev().collect() } else { out };
                    bg.send(Msg::AgentTool(if out.trim().is_empty() { "(no output)".into() } else { out }))
                });
                return Some(Err(()));
            }
            "model_status" => {
                let p = ultar_llm::platform::detect();
                let r = models::recommend(&p);
                let installed: Vec<String> = std::fs::read_dir(model_dir()).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".gguf")).collect()).unwrap_or_default();
                format!(
                    "device: {} ({}); recommended chat model {} and vision model {}; installed: [{}]; engine: ultar-llm on the Ultar core{}",
                    r.why,
                    r.cpu,
                    r.chat,
                    r.vision.unwrap_or("none (too little memory)"),
                    installed.join(", "),
                    if self.agent.backend == "claude" { "; currently using Claude (cloud)" } else { "" }
                )
            }
            "model_download" => {
                let id = c.arg("id");
                let spec = if c.arg("source") == "ollama" { format!("{id}@ollama") } else { id.clone() };
                if models::parse_spec(&spec).is_err() {
                    return Some(Ok(format!("error: unknown model {id} – known: {}", models::CATALOG.iter().map(|e| e.id).collect::<Vec<_>>().join(", "))));
                }
                self.jarvis_pull(spec);
                format!("ok: download of {id} started – progress is shown on screen; tell the user it can take a while")
            }
            _ => return None,
        };
        Some(Ok(res))
    }

    /// Name der Seite für die Beschreibung
    fn screen_label(&self, s: &Screen) -> String {
        let name = self.all_apps().into_iter().find(|a| a.2.as_ref() == Some(s)).map(|a| a.1.to_string());
        name.unwrap_or_else(|| match s {
            Screen::Home => "Home screen".into(),
            Screen::Settings => "Settings".into(),
            Screen::Office(x) => format!("{x:?}"),
            Screen::Viewer(p) | Screen::Photo(p) => format!("viewer {p}"),
            Screen::Web(u) => format!("browser {u}"),
            _ => "app".into(),
        })
    }

    /// Modell im Hintergrund laden (Fortschritt im Auftragsfenster), danach Dienst neu starten
    pub(super) fn jarvis_pull(&mut self, spec: String) {
        if self.job_running() {
            self.toast(self.t("Please wait").into());
            return;
        }
        let id = models::parse_spec(&spec).map(|x| x.0).unwrap_or_else(|_| spec.clone());
        let vision = models::find(&id).is_some_and(|e| e.vision);
        self.jv.pulling = Some(id.clone());
        self.job = Some(Job { title: format!("{} – {id}", self.t("Language model")), lines: vec![format!("{} {}", self.t("Downloading"), spec)], done: None });
        self.go(Screen::Job);
        let bg = self.bg.clone();
        let dir = model_dir();
        std::thread::spawn(move || {
            let mut last = Instant::now() - Duration::from_secs(5);
            let mut lastp = u64::MAX;
            let r = models::pull(&spec, &dir, &mut |label, done, total| {
                let pct = if total > 0 { done * 100 / total } else { 0 };
                if last.elapsed() > Duration::from_secs(2) || (pct != lastp && pct % 10 == 0) {
                    last = Instant::now();
                    lastp = pct;
                    bg.send(Msg::JobLine(if total > 0 { format!("{label}: {} / {} MB ({pct} %)", done >> 20, total >> 20) } else { format!("{label}: {} MB", done >> 20) }));
                }
            });
            match r {
                Ok(p) => {
                    // Gesprächsmodell als Standard des Dienstes festlegen (Bild-Modelle lädt der Dienst bei Bedarf)
                    if !vision {
                        let cfgp = format!("{}/llm.toml", sys::data_dir());
                        let mut t: toml::Table = std::fs::read_to_string(&cfgp).ok().and_then(|s| s.parse().ok()).unwrap_or_default();
                        t.insert("model".into(), toml::Value::String(p.model.clone()));
                        let _ = std::fs::write(&cfgp, toml::to_string(&t).unwrap_or_default());
                    }
                    bg.send(Msg::JobLine(format!("OK: {}{}", p.model, p.mmproj.map(|m| format!(" + {m}")).unwrap_or_default())));
                    sys::ctl("restart llm");
                    bg.send(Msg::JobDone(0));
                    bg.send(Msg::AgentUp(false));
                }
                Err(e) => {
                    bg.send(Msg::JobLine(format!("error: {e}")));
                    bg.send(Msg::JobDone(1));
                }
            }
        });
    }

    /// Abschnitt „Sprachmodell auf dem Gerät": Empfehlung, Quelle (Hugging Face / Ollama), Katalog
    pub(super) fn agent_model_rows(&mut self, ui: &mut Ui, y: &mut f32) {
        ui.section(y, self.t("Language model on the device"));
        let p = ultar_llm::platform::detect();
        let r = models::recommend(&p);
        let installed: Vec<String> = std::fs::read_dir(model_dir()).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().trim_end_matches(".gguf").to_string()).collect()).unwrap_or_default();
        let cs = ui.th.sub;
        let dev = format!("{}: {:.1} GB RAM · {} · {} {}", self.t("This device"), r.mem_gb, r.cpu, self.t("GPU"), r.gpu.clone().unwrap_or_else(|| "–".into()));
        *y += ui.para(ui.dp(22.0), *y, ui.w - ui.dp(44.0), &dev, 12.5, cs) + ui.dp(4.0);
        let rec_chat = models::find(r.chat).map(|e| e.name).unwrap_or(r.chat);
        let rec_vis = r.vision.and_then(models::find).map(|e| e.name).unwrap_or("–");
        let rec = format!("★ {}: {rec_chat} · {}: {rec_vis}", self.t("Recommended"), self.t("Vision"));
        let ac = ui.th.accent;
        *y += ui.para(ui.dp(22.0), *y, ui.w - ui.dp(44.0), &rec, 13.5, ac) + ui.dp(8.0);
        self.caption(ui, y, self.t("Source"));
        if let Some(i) = ui.chips(y, &["Hugging Face", "Ollama"], self.jv.src as usize) {
            self.jv.src = i as u8;
        }
        let old = ui.enabled;
        ui.enabled = old && !self.job_running();
        let mut pick = None;
        for e in models::CATALOG {
            let have = installed.iter().any(|i| i.eq_ignore_ascii_case(e.id));
            let star = e.id == r.chat || Some(e.id) == r.vision;
            let sub = format!("{} · {} MB · {} MB RAM · {}", self.t(if e.vision { "sees images" } else { "chat and tools" }), e.size_mb, e.ram_mb, e.langs);
            let val = if have { self.t("Installed") } else if star { self.t("recommended") } else { "" };
            ui.kb.row_gold.set(star && !have);
            if ui.row(y, Some(if e.vision { Ic::Camera } else { Ic::Spark }), e.name, Some(&sub), Some(val), !have) && !have {
                pick = Some(e);
            }
        }
        ui.enabled = old;
        if let Some(e) = pick {
            if self.jv.src == 1 && e.ollama.is_empty() {
                self.toast(self.t("Only on Hugging Face – switched source").into());
                self.jv.src = 0;
            }
            let spec = if self.jv.src == 1 { format!("{}@ollama", e.id) } else { e.id.to_string() };
            self.agent.enabled = true;
            self.agent.backend = "local".into();
            self.save_agent();
            self.jarvis_pull(spec);
            ui.dirty = true;
        }
        let msg = if sys::online() { self.t("Downloads go directly into the Ultar core (no Ubuntu area). Vision models load only when you show the assistant something.") } else { self.t("Offline: copy a .gguf model (and its mmproj file) into /data/ultar/models.") };
        *y += ui.para(ui.dp(22.0), *y, ui.w - ui.dp(44.0), msg, 12.5, cs) + ui.dp(12.0);
    }

    // ------------------------------------------------------------------ Mitbringen

    /// Leiste über dem Eingabefeld: mitgebrachte Dinge (Chips) und – geöffnet – die Auswahl
    pub(super) fn jarvis_attach_ui(&mut self, ui: &mut Ui, bottom: f32) -> f32 {
        let m = ui.dp(14.0);
        let mut y = bottom;
        // Auswahl
        if self.jv.attach_open {
            let mut opts: Vec<(Ic, String, Option<String>)> = Vec::new();
            if self.jv.view.is_some() {
                opts.push((Ic::Svg("img"), self.t("Current screen").into(), Some("screen:".into())));
            }
            opts.push((Ic::Camera, self.t("Take a photo").into(), Some("camera:".into())));
            // neueste Bilder und Dokumente
            let media = std::env::var("ULTAR_MEDIA").unwrap_or_else(|_| "/data".into());
            let mut recent: Vec<(String, u64)> = Vec::new();
            for d in ["DCIM/Camera", "Pictures", "Download", "Documents", "Pictures/Screenshots"] {
                if let Ok(rd) = std::fs::read_dir(format!("{media}/{d}")) {
                    for e in rd.flatten() {
                        let p = e.path().to_string_lossy().into_owned();
                        let ok = is_img(&p) || !matches!(ultar_office::Kind::of(&p), ultar_office::Kind::Other);
                        if ok && e.path().is_file() {
                            let mt = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
                            recent.push((p, mt));
                        }
                    }
                }
            }
            recent.sort_by(|a, b| b.1.cmp(&a.1));
            for (p, _) in recent.into_iter().take(5) {
                let n = std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                opts.push((if is_img(&p) { Ic::Image } else { Ic::Doc }, n, Some(p)));
            }
            let rh = ui.dp(46.0);
            let h = rh * opts.len() as f32 + ui.dp(12.0);
            y -= h + ui.dp(6.0);
            let r = Rect::new(m, y, ui.w - 2.0 * m, h);
            ui.cv.soft_shadow(r, ui.dp(18.0), rgb(0));
            ui.card(r);
            let mut yy = r.y + ui.dp(6.0);
            let mut pick = None;
            for (ic, label, val) in &opts {
                let rr = Rect::new(r.x + ui.dp(6.0), yy, r.w - ui.dp(12.0), rh);
                if ui.held(rr) {
                    ui.cv.rrect(rr, ui.dp(12.0), ui.th.press);
                }
                let ix = if ui.rtl { rr.right() - ui.dp(24.0) } else { rr.x + ui.dp(24.0) };
                let ac = ui.th.accent;
                ui.icon(*ic, ix, rr.cy(), ui.dp(18.0), ac);
                let tx = if ui.rtl { rr.x + ui.dp(10.0) } else { rr.x + ui.dp(48.0) };
                let lf = ui.fit(label, 14.5, false, rr.w - ui.dp(60.0));
                let c = ui.th.text;
                ui.label(Rect::new(tx, rr.y, rr.w - ui.dp(58.0), rh), &lf, 14.5, c, false, Al::Start);
                if ui.tap(rr) {
                    pick = val.clone();
                }
                yy += rh;
            }
            if let Some(v) = pick {
                self.jv.attach_open = false;
                match v.as_str() {
                    "screen:" => match self.jarvis_screen_png() {
                        Ok(p) => self.jv.attach.push(p),
                        Err(e) => self.toast(e),
                    },
                    "camera:" => {
                        // Foto mit der Kamera-App; zurück im Gespräch wird das neueste Foto angehängt
                        self.jv.attach.push("camera:".into());
                        self.cam.mode = 0;
                        self.go(Screen::Camera);
                    }
                    p => self.jv.attach.push(p.to_string()),
                }
                ui.dirty = true;
            }
        }
        // „camera:" durch das neueste Foto ersetzen, sobald wir zurück sind
        if self.jv.attach.iter().any(|a| a == "camera:") && matches!(self.stack.last().map(|s| &s.0), Some(Screen::Agent)) {
            let dir = super::app_gallery::camera_dir();
            let newest = std::fs::read_dir(&dir).ok().and_then(|rd| rd.flatten().filter(|e| is_img(&e.path().to_string_lossy())).max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()))).map(|e| e.path().to_string_lossy().into_owned());
            self.jv.attach.retain(|a| a != "camera:");
            if let Some(n) = newest {
                self.jv.attach.push(n);
            }
        }
        // Chips der mitgebrachten Dinge
        if !self.jv.attach.is_empty() {
            let ch = ui.dp(34.0);
            y -= ch + ui.dp(6.0);
            let mut x = m;
            let mut remove = None;
            for (i, a) in self.jv.attach.clone().iter().enumerate() {
                let n = std::path::Path::new(a).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let n = if n.starts_with("screen-") { self.t("Screen").to_string() } else { n };
                let lf = ui.fit(&n, 12.5, false, ui.dp(150.0));
                let w = ui.text_w(&lf, 12.5, false) + ui.dp(58.0);
                let px = if ui.rtl { ui.w - x - w } else { x };
                let r = Rect::new(px, y, w, ch);
                let ac = ui.th.accent;
                ui.cv.rrect(r, ch / 2.0, ac.alpha(0.16));
                ui.icon(if is_img(a) { Ic::Image } else { Ic::Doc }, r.x + ui.dp(16.0), r.cy(), ui.dp(13.0), ac);
                let c = ui.th.text;
                ui.label(Rect::new(r.x + ui.dp(28.0), r.y, w - ui.dp(54.0), ch), &lf, 12.5, c, false, Al::Start);
                let xr = Rect::new(r.right() - ui.dp(28.0), r.y, ui.dp(28.0), ch);
                let cs = ui.th.sub;
                ui.icon(Ic::Close, xr.cx(), xr.cy(), ui.dp(10.0), cs);
                if ui.tap(xr) {
                    remove = Some(i);
                }
                x += w + ui.dp(6.0);
            }
            if let Some(i) = remove {
                self.jv.attach.remove(i);
                ui.dirty = true;
            }
        }
        y
    }

    /// Nachricht mit mitgebrachten Dingen bauen: Bilder als Bilder, Dokumente als Text (gekürzt)
    pub(super) fn jarvis_take_attachments(&mut self, text: &str) -> (serde_json::Value, String) {
        let items = std::mem::take(&mut self.jv.attach);
        let mut imgs = Vec::new();
        let mut body = text.to_string();
        let mut shown = text.to_string();
        for a in items.into_iter().filter(|a| a != "camera:") {
            let name = std::path::Path::new(&a).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if is_img(&a) {
                imgs.push(a.clone());
                shown.push_str(&format!("\n📎 {name}"));
            } else {
                match ultar_office::read_text(&a, 6000) {
                    Ok(t) => body.push_str(&format!("\n\n[The user brought the file {a}:]\n{t}")),
                    Err(e) => body.push_str(&format!("\n\n[The user brought the file {a} – could not read it: {e}]")),
                }
                shown.push_str(&format!("\n📎 {name}"));
            }
        }
        if !imgs.is_empty() && !self.jarvis_can_see() {
            body.push_str(&format!("\n\n[{} image(s) attached, but no vision model is installed – tell the user to install one in Settings → Local agent]", imgs.len()));
            imgs.clear();
        }
        (agent::user_msg(&body, &imgs), shown)
    }

    /// Muss diese Aktion bestätigt werden? Root/Sicherheit/Kamera/Dateien immer; Bild an die Cloud (Claude) auch.
    pub(super) fn jarvis_needs_confirm(&self, c: &Call) -> bool {
        if c.always_confirm() {
            return true;
        }
        if self.agent.backend == "claude" && matches!(c.tool.as_str(), "look_screen" | "see_screen" | "read_any" | "network_connections" | "processes") {
            // Inhalte verlassen das Gerät → erst fragen
            return true;
        }
        c.risky() && self.agent.confirm
    }

    pub(super) fn jarvis_button(&mut self, ui: &mut Ui, cx: f32, cy: f32, r: f32) {
        let open = self.jv.attach_open;
        let (bg, fg) = if open { (ui.th.accent, ui.th.on_accent) } else { (ui.th.card, ui.th.text) };
        if ui.icon_btn(cx, cy, r, Ic::Plus, bg, fg) {
            self.jv.attach_open = !open;
            ui.dirty = true;
        }
    }
}
