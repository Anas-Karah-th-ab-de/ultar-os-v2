//! ultar-sshd – Entwickler-Zugang (SSH) für Ultar OS.
//!
//! Während der Entwicklung steuert der PC (und Claude auf dem PC) das Gerät darüber: Befehle ausführen,
//! Kernprotokoll und Dienst-Protokolle lesen, Dateien kopieren (SFTP/scp), Gerätedienste weiterleiten
//! (`ssh -L 8080:127.0.0.1:8080`). Läuft gleich auf dem Ultar-Kern (eigener Kern, Linux-ABI) und auf dem Handy.
//!
//! Sicherheit: nur Schlüssel (kein Kennwort), nur wenn eingeschaltet (`/data/ultar/entwickler/an`,
//! `/etc/ultar/entwickler` oder Startparameter `ultar.entwickler=1`), Weiterleitung nur zu 127.0.0.1.

mod sftp;
mod steuern;

use russh::keys::ssh_key::{private::Ed25519Keypair, HashAlg, LineEnding};
use russh::keys::{PrivateKey, PublicKey};
use russh::server::{Auth, Msg, PublicKeyOffer, Server as _, Session, VerifiedPublicKey};
use russh::{Channel, ChannelId, ChannelMsg};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};

const STANDARD_PORT: u16 = 2222;

fn log(s: &str) {
    eprintln!("[entwickler-ssh] {s}");
}

// ------------------------------------------------------------------ Einstellungen

fn dir() -> PathBuf {
    std::env::var("ULTAR_ENTWICKLER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| "/data/ultar/entwickler".into())
}

fn eingeschaltet() -> bool {
    // „aus“ in den Einstellungen gilt auch gegen das Entwicklungs-Abbild (/etc/ultar/entwickler)
    !dir().join("aus").exists()
        && (dir().join("an").exists()
            || Path::new("/etc/ultar/entwickler").exists()
            || std::fs::read_to_string("/proc/cmdline")
                .unwrap_or_default()
                .split_whitespace()
                .any(|a| a == "ultar.entwickler=1"))
}

fn port() -> u16 {
    std::fs::read_to_string(dir().join("port"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(STANDARD_PORT)
}

/// Erlaubte Schlüssel: auf der Datenpartition (änderbar) und im Abbild (Bauzeit, `ENTWICKLER_SCHLUESSEL`)
fn schluessel_dateien() -> Vec<PathBuf> {
    vec![
        dir().join("authorized_keys"),
        PathBuf::from("/etc/ultar/entwickler.keys"),
    ]
}

fn erlaubte_schluessel() -> Vec<PublicKey> {
    let mut v = Vec::new();
    for f in schluessel_dateien() {
        for z in std::fs::read_to_string(f).unwrap_or_default().lines() {
            let z = z.trim();
            if z.is_empty() || z.starts_with('#') {
                continue;
            }
            if let Ok(k) = PublicKey::from_openssh(z) {
                v.push(k);
            }
        }
    }
    v
}

/// Offen ist der Zugang nur eingeschaltet **und** mit mindestens einem erlaubten Schlüssel.
fn aktiv() -> bool {
    eingeschaltet() && !erlaubte_schluessel().is_empty()
}

/// Schreibt ins Protokoll, warum der Zugang gerade zu ist.
fn melde_zu() {
    if !eingeschaltet() {
        log("aus (einschalten: ultar-sshd an, /etc/ultar/entwickler oder Startparameter ultar.entwickler=1)");
        return;
    }
    log(&format!(
        "kein erlaubter Schlüssel ({}): Zugang bleibt zu",
        schluessel_dateien()
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ));
}

/// So oft sieht der Dienst nach, ob der Zugang auf- oder zugehen soll (nur Dateien, kein Netz).
const PRUEFTAKT: Duration = Duration::from_secs(2);
/// Ende, wenn der Zugang zugeschaltet wurde: ultar-init startet den Dienst neu (`restart =
/// "on-failure"`), der neue wartet wieder – alle offenen Sitzungen enden mit dem alten Prozess.
const EXIT_ZU: i32 = 75;

/// Wartet ohne offenen Port, bis der Zugang eingeschaltet ist und ein Schlüssel erlaubt ist –
/// etwa nach dem ersten Start, der den Schlüssel aus dem Installer erst einträgt, oder nach
/// `ultar-sshd an` / einem Schlüssel aus der Web-Verwaltung. Kein Neustart nötig.
fn warten_bis_aktiv() {
    let mut gemeldet = false;
    while !aktiv() {
        if !gemeldet {
            melde_zu();
            gemeldet = true;
        }
        std::thread::sleep(PRUEFTAKT);
    }
}

fn schluessel_erlaubt(k: &PublicKey) -> bool {
    erlaubte_schluessel()
        .iter()
        .any(|e| e.key_data() == k.key_data())
}

/// Wirtsschlüssel laden oder einmalig erzeugen (bleibt über Neustarts gleich, damit der PC ihn wiedererkennt)
fn wirtsschluessel() -> anyhow::Result<PrivateKey> {
    let mut orte = vec![
        dir().join("host_ed25519"),
        PathBuf::from("/etc/ultar/entwickler.host"),
    ];
    orte.push(PathBuf::from("/run/ultar/entwickler-host_ed25519"));
    for o in &orte {
        if let Ok(s) = std::fs::read_to_string(o) {
            if let Ok(k) = PrivateKey::from_openssh(&s) {
                return Ok(k);
            }
        }
    }
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| anyhow::anyhow!("Zufall: {e}"))?;
    let k = PrivateKey::from(Ed25519Keypair::from_seed(&seed));
    let text = k.to_openssh(LineEnding::LF)?;
    for o in [&orte[0], &orte[2]] {
        if let Some(d) = o.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if std::fs::write(o, text.as_bytes()).is_ok() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(o, std::fs::Permissions::from_mode(0o600));
            log(&format!("neuer Wirtsschlüssel: {}", o.display()));
            break;
        }
    }
    Ok(k)
}

// ------------------------------------------------------------------ Eingebaute Befehle

/// Kernprotokoll: Ultar-Kern (/proc/ultar/protokoll) oder Linux (/dev/kmsg, nicht blockierend)
fn kernlog() -> String {
    if let Ok(s) = std::fs::read_to_string("/proc/ultar/protokoll") {
        return s;
    }
    if let Ok(f) = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags_nb()
        .open("/dev/kmsg")
    {
        let mut f = f;
        let mut out = String::new();
        let mut b = vec![0u8; 8192];
        while let Ok(n) = f.read(&mut b) {
            if n == 0 {
                break;
            }
            let z = String::from_utf8_lossy(&b[..n]);
            // "6,1234,567890,-;Nachricht" → "[   0.567890] Nachricht"
            let (kopf, text) = z.split_once(';').unwrap_or(("", &z));
            let us: u64 = kopf
                .split(',')
                .nth(2)
                .and_then(|t| t.parse().ok())
                .unwrap_or(0);
            out += &format!("[{:>5}.{:06}] {}", us / 1_000_000, us % 1_000_000, text);
            if !text.ends_with('\n') {
                out.push('\n');
            }
        }
        return out;
    }
    "kein Kernprotokoll lesbar (/proc/ultar/protokoll, /dev/kmsg)\n".into()
}

trait NichtBlockierend {
    fn custom_flags_nb(&mut self) -> &mut Self;
}
impl NichtBlockierend for std::fs::OpenOptions {
    fn custom_flags_nb(&mut self) -> &mut Self {
        use std::os::unix::fs::OpenOptionsExt;
        self.custom_flags(libc::O_NONBLOCK)
    }
}

fn lies(p: &str) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn info() -> String {
    let mut s = String::new();
    s += &format!("system: {}", lies("/proc/version"));
    if let Ok(h) = std::fs::read_to_string("/etc/hostname") {
        s += &format!("name: {}", h);
    }
    let up = lies("/proc/uptime");
    s += &format!(
        "laufzeit: {} s\n",
        up.split_whitespace().next().unwrap_or("?")
    );
    for z in lies("/proc/meminfo").lines().take(3) {
        s += &format!("{z}\n");
    }
    s += &format!(
        "kerne: {}\n",
        lies("/proc/cpuinfo").matches("processor").count()
    );
    let sich = lies("/proc/ultar/sicherheit");
    if !sich.is_empty() {
        s += "--- sicherheit ---\n";
        s += &sich;
    }
    let fw = lies("/proc/ultar/firewall");
    if !fw.is_empty() {
        s += "--- firewall ---\n";
        s += &fw;
    }
    s += &format!(
        "entwickler-ssh: port {}, {} schlüssel\n",
        port(),
        erlaubte_schluessel().len()
    );
    s
}

fn hilfe() -> String {
    "Ultar OS – Entwickler-Zugang\n\
     eingebaut (gehen immer, auch ohne Shell):\n\
     \x20 kernlog          Kernprotokoll (Ultar-Kern: /proc/ultar/protokoll, Linux: /dev/kmsg)\n\
     \x20 info             Gerät: Kern, Speicher, Laufzeit, Brandmauer, Sicherheit\n\
     \x20 ultar-hilfe      diese Hilfe\n\
     Gerät steuern:\n\
     \x20 bildschirm       Bildschirmfoto → /data/ultar/screen.png (danach: scp ultar:/data/ultar/screen.png .)\n\
     \x20 tippen X Y       tippen (Bildpunkte des Fotos oder Anteile 0.0–1.0)\n\
     \x20 wischen X1 Y1 X2 Y2 [ms]\n\
     \x20 text \"…\"         Text eingeben\n\
     \x20 taste NAME       enter, zurueck, loeschen, lauter, leiser, tab, hoch, runter, links, rechts, home, ein\n\
     \x20 oeffne SEITE [sprache]  Seite der Oberfläche öffnen (home, settings, office, agent …)\n\
     \x20 helfer \"…\"       Frage an den Helfer/System-Assistenten\n\
     sonst: jeder Befehl über /bin/sh (z. B. ultar-ctl status, ultar-ctl log ui, cat /proc/ultar/sicherheit)\n\
     Dateien: scp/sftp; Dienste weiterleiten: ssh -L 8080:127.0.0.1:8080 ultar\n"
        .into()
}

const WERKZEUGE: [&str; 12] = [
    "kernlog",
    "info",
    "ultar-hilfe",
    "bildschirm",
    "tippen",
    "wischen",
    "text",
    "taste",
    "oeffne",
    "helfer",
    "tap",
    "swipe",
];

/// einfacher Befehl ohne Shell-Zeichen (sonst übernimmt /bin/sh mit den Werkzeug-Skripten)
fn einfach(cmd: &str) -> bool {
    !cmd.chars().any(|c| {
        matches!(
            c,
            ';' | '|' | '&' | '>' | '<' | '`' | '$' | '(' | ')' | '\n'
        )
    })
}

/// Werkzeuge als kleine Skripte, damit sie auch in Shell-Zeilen gehen (`tippen 10 20 && bildschirm`)
fn werkzeuge_anlegen() -> Option<String> {
    let exe = std::env::current_exe().ok()?.to_string_lossy().into_owned();
    let sh = shell()?.0;
    let d = format!(
        "{}/entwickler-bin",
        std::env::var("ULTAR_RUN").unwrap_or_else(|_| "/run/ultar".into())
    );
    std::fs::create_dir_all(&d).ok()?;
    for w in WERKZEUGE {
        let p = format!("{d}/{w}");
        let inhalt = format!("#!{sh}\nexec {exe} werkzeug {w} \"$@\"\n");
        if std::fs::write(&p, inhalt).is_ok() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
        }
    }
    Some(d)
}

static WERKZEUG_DIR: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// eingebauter Befehl → (Ausgabe, Rückgabewert)
fn eingebaut(cmd: &str) -> Option<(String, u32)> {
    if !einfach(cmd) {
        return None;
    }
    let w: Vec<&str> = cmd.split_whitespace().collect();
    if let Some(r) = steuern::befehl(&w, cmd) {
        return Some(match r {
            Ok(t) => (t, 0),
            Err(e) => (format!("fehler: {e}\n"), 1),
        });
    }
    let t = match w.as_slice() {
        ["kernlog"] => Some(kernlog()),
        ["kernlog", n] => {
            let n: usize = n.trim_start_matches('-').parse().unwrap_or(50);
            let k = kernlog();
            let z: Vec<&str> = k.lines().collect();
            Some(z[z.len().saturating_sub(n)..].join("\n") + "\n")
        }
        ["info"] => Some(info()),
        ["ultar-hilfe"] => Some(hilfe()),
        _ => None,
    };
    t.map(|t| (t, 0))
}

// ------------------------------------------------------------------ Prozesse (Rohr oder Pseudo-Terminal)

fn shell() -> Option<(String, Vec<String>)> {
    for s in ["/bin/bash", "/bin/sh", "/usr/bin/bash", "/usr/bin/sh"] {
        if Path::new(s).exists() {
            return Some((s.into(), vec![]));
        }
    }
    for b in ["/bin/busybox", "/usr/bin/busybox"] {
        if Path::new(b).exists() {
            return Some((b.into(), vec!["sh".into()]));
        }
    }
    None
}

#[derive(Clone, Default)]
struct Pty {
    term: String,
    cols: u16,
    rows: u16,
}

enum Ereignis {
    Aus(Vec<u8>),
    Err(Vec<u8>),
    Zu,
    Ende(u32),
}

struct Lauf {
    ein: Option<std::sync::mpsc::Sender<Vec<u8>>>,
    master: Option<i32>,
    pid: i32,
    rx: mpsc::Receiver<Ereignis>,
    stroeme: usize,
}

fn openpty(p: &Pty) -> std::io::Result<(OwnedFd, OwnedFd)> {
    let (mut m, mut s) = (0, 0);
    let ws = libc::winsize {
        ws_row: p.rows.max(1),
        ws_col: p.cols.max(1),
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let r = unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws) };
    if r != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(m), OwnedFd::from_raw_fd(s)) })
}

fn leser(mut r: impl Read + Send + 'static, tx: mpsc::Sender<Ereignis>, err: bool) {
    std::thread::spawn(move || {
        let mut b = vec![0u8; 32 * 1024];
        loop {
            match r.read(&mut b) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let d = b[..n].to_vec();
                    if tx
                        .blocking_send(if err {
                            Ereignis::Err(d)
                        } else {
                            Ereignis::Aus(d)
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
        let _ = tx.blocking_send(Ereignis::Zu);
    });
}

fn schreiber(mut w: impl Write + Send + 'static) -> std::sync::mpsc::Sender<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        while let Ok(d) = rx.recv() {
            if w.write_all(&d).and_then(|_| w.flush()).is_err() {
                break;
            }
        }
    });
    tx
}

fn starten(
    cmd: Option<&str>,
    pty: Option<&Pty>,
    env: &HashMap<String, String>,
) -> std::io::Result<Lauf> {
    let (prog, mut args) = match shell() {
        Some(s) => s,
        None => {
            // ohne Shell: Befehl direkt ausführen
            let w: Vec<String> = cmd
                .unwrap_or("")
                .split_whitespace()
                .map(String::from)
                .collect();
            if w.is_empty() {
                return Err(std::io::Error::other(
                    "keine Shell auf dem Gerät (/bin/sh) – nur eingebaute Befehle: ultar-hilfe",
                ));
            }
            (w[0].clone(), w[1..].to_vec())
        }
    };
    if shell().is_some() {
        match cmd {
            Some(c) => args.extend(["-c".into(), c.into()]),
            None => args.push("-i".into()),
        }
    }
    let mut c = Command::new(&prog);
    c.args(&args).current_dir(if Path::new("/root").is_dir() {
        "/root"
    } else {
        "/"
    });
    c.env("HOME", "/root")
        .env("USER", "root")
        .env("LOGNAME", "root")
        .env("SHELL", &prog)
        .env("ULTAR_ENTWICKLER", "1");
    let pfad = std::env::var("PATH")
        .unwrap_or_else(|_| "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into());
    match WERKZEUG_DIR.get().cloned().flatten() {
        Some(d) => c.env("PATH", format!("{d}:{pfad}")),
        None => c.env("PATH", pfad),
    };
    for (k, v) in env {
        if k == "LANG" || k.starts_with("LC_") || k == "TERM" {
            c.env(k, v);
        }
    }
    let (tx, rx) = mpsc::channel(64);
    if let Some(p) = pty {
        if let Ok((m, s)) = openpty(p) {
            c.env(
                "TERM",
                if p.term.is_empty() {
                    "xterm-256color"
                } else {
                    &p.term
                },
            );
            let s0 = s.try_clone()?;
            let s1 = s.try_clone()?;
            c.stdin(Stdio::from(s0))
                .stdout(Stdio::from(s1))
                .stderr(Stdio::from(s));
            unsafe {
                use std::os::unix::process::CommandExt;
                c.pre_exec(|| {
                    libc::setsid();
                    libc::ioctl(0, libc::TIOCSCTTY as _, 0);
                    Ok(())
                });
            }
            let mut kind = c.spawn()?;
            drop(c);
            let mfd = m.as_raw_fd();
            let rf = std::fs::File::from(m.try_clone()?);
            let wf = std::fs::File::from(m);
            leser(rf, tx.clone(), false);
            let pid = kind.id() as i32;
            let txe = tx;
            std::thread::spawn(move || {
                let code = kind.wait().ok().and_then(|s| s.code()).unwrap_or(255) as u32;
                let _ = txe.blocking_send(Ereignis::Ende(code));
            });
            return Ok(Lauf {
                ein: Some(schreiber(wf)),
                master: Some(mfd),
                pid,
                rx,
                stroeme: 1,
            });
        }
        // kein Pseudo-Terminal (Ultar-Kern): weiter mit Rohren, Ausgabe bleibt lesbar
    }
    c.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut kind = c.spawn()?;
    let ein = kind.stdin.take().map(schreiber);
    leser(kind.stdout.take().unwrap(), tx.clone(), false);
    leser(kind.stderr.take().unwrap(), tx.clone(), true);
    let pid = kind.id() as i32;
    std::thread::spawn(move || {
        let code = kind.wait().ok().and_then(|s| s.code()).unwrap_or(255) as u32;
        let _ = tx.blocking_send(Ereignis::Ende(code));
    });
    Ok(Lauf {
        ein,
        master: None,
        pid,
        rx,
        stroeme: 2,
    })
}

/// Kanal bedienen, bis der Prozess endet und seine Ausgabe vollständig übertragen ist
async fn bedienen(channel: Channel<Msg>, mut l: Lauf) {
    let (mut lesen, schreiben) = channel.split();
    let mut ende: Option<u32> = None;
    let mut offen = l.stroeme;
    loop {
        if offen == 0 {
            if let Some(c) = ende {
                let _ = schreiben.exit_status(c).await;
                break;
            }
        }
        tokio::select! {
            m = lesen.wait() => match m {
                Some(ChannelMsg::Data { data }) => {
                    if let Some(e) = &l.ein {
                        let _ = e.send(data.to_vec());
                    }
                }
                Some(ChannelMsg::Eof) => {
                    // ohne Terminal: Eingabe schließen (cat > datei endet); mit Terminal ignorieren
                    if l.master.is_none() {
                        l.ein = None;
                    }
                }
                Some(ChannelMsg::WindowChange { col_width, row_height, .. }) => {
                    if let Some(m) = l.master {
                        let ws = libc::winsize { ws_row: row_height as u16, ws_col: col_width as u16, ws_xpixel: 0, ws_ypixel: 0 };
                        unsafe { libc::ioctl(m, libc::TIOCSWINSZ as _, &ws) };
                    }
                }
                Some(ChannelMsg::Signal { .. }) => unsafe {
                    libc::kill(l.pid, libc::SIGTERM);
                },
                Some(_) => {}
                None => {
                    // Gegenseite weg: Prozess beenden
                    unsafe { libc::kill(l.pid, libc::SIGHUP) };
                    unsafe { libc::kill(l.pid, libc::SIGKILL) };
                    return;
                }
            },
            e = l.rx.recv() => match e {
                Some(Ereignis::Aus(d)) => { let _ = schreiben.data_bytes(d).await; }
                Some(Ereignis::Err(d)) => { let _ = schreiben.extended_data_bytes(1, d).await; }
                Some(Ereignis::Zu) => offen = offen.saturating_sub(1),
                Some(Ereignis::Ende(c)) => {
                    ende = Some(c);
                    if l.master.is_some() {
                        // Terminal: Nachfahren können den Leser offen halten – kurz nachlaufen lassen
                        let frist = tokio::time::sleep(Duration::from_millis(300));
                        tokio::pin!(frist);
                        loop {
                            tokio::select! {
                                _ = &mut frist => break,
                                e = l.rx.recv() => match e {
                                    Some(Ereignis::Aus(d)) => { let _ = schreiben.data_bytes(d).await; }
                                    Some(Ereignis::Zu) | None => break,
                                    _ => {}
                                },
                            }
                        }
                        offen = 0;
                    }
                }
                None => offen = 0,
            },
        }
    }
    let _ = schreiben.eof().await;
    let _ = schreiben.close().await;
}

/// SFTP über eine Brücke: so bekommt der Client am Ende einen Rückgabewert (scp prüft ihn)
async fn sftp_bruecke(ch: Channel<Msg>, b: tokio::io::DuplexStream) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (mut lesen, schreiben) = ch.split();
    let (mut br, mut bw) = tokio::io::split(b);
    let mut buf = vec![0u8; 64 * 1024];
    let mut ein_offen = true;
    loop {
        tokio::select! {
            m = lesen.wait(), if ein_offen => match m {
                Some(ChannelMsg::Data { data }) => {
                    if bw.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Some(ChannelMsg::Eof) | None => {
                    ein_offen = false;
                    let _ = bw.shutdown().await;
                }
                Some(_) => {}
            },
            n = br.read(&mut buf) => match n {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if schreiben.data_bytes(buf[..n].to_vec()).await.is_err() {
                        return;
                    }
                }
            },
        }
    }
    let _ = schreiben.exit_status(0).await;
    let _ = schreiben.eof().await;
    let _ = schreiben.close().await;
}

async fn text_senden(channel: Channel<Msg>, text: String, code: u32) {
    let _ = channel
        .data_bytes(text.replace("\n", "\r\n").into_bytes())
        .await;
    let _ = channel.exit_status(code).await;
    let _ = channel.eof().await;
    let _ = channel.close().await;
}

// ------------------------------------------------------------------ SSH-Server

#[derive(Clone)]
struct Server;

impl russh::server::Server for Server {
    type Handler = Verbindung;
    fn new_client(&mut self, adr: Option<std::net::SocketAddr>) -> Verbindung {
        Verbindung {
            adr: adr.map(|a| a.to_string()).unwrap_or_default(),
            ..Default::default()
        }
    }
}

#[derive(Default)]
struct Verbindung {
    adr: String,
    kanaele: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
    pty: HashMap<ChannelId, Pty>,
    env: HashMap<String, String>,
}

impl Verbindung {
    async fn nimm(&self, id: ChannelId) -> Option<Channel<Msg>> {
        self.kanaele.lock().await.remove(&id)
    }
}

impl russh::server::Handler for Verbindung {
    type Error = anyhow::Error;

    async fn auth_publickey_offered(
        &mut self,
        _u: &str,
        o: PublicKeyOffer<'_>,
    ) -> Result<Auth, Self::Error> {
        Ok(if schluessel_erlaubt(o.public_key) {
            Auth::Accept
        } else {
            Auth::reject()
        })
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        k: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        if schluessel_erlaubt(k.public_key) {
            log(&format!(
                "angemeldet: {user} von {} ({})",
                self.adr,
                k.public_key.fingerprint(HashAlg::Sha256)
            ));
            Ok(Auth::Accept)
        } else {
            log(&format!(
                "abgelehnt: {user} von {} ({})",
                self.adr,
                k.public_key.fingerprint(HashAlg::Sha256)
            ));
            Ok(Auth::reject())
        }
    }

    async fn channel_open_session(
        &mut self,
        ch: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _s: &mut Session,
    ) -> Result<(), Self::Error> {
        self.kanaele.lock().await.insert(ch.id(), ch);
        reply.accept().await;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        id: ChannelId,
        term: &str,
        cols: u32,
        rows: u32,
        _pw: u32,
        _ph: u32,
        _m: &[(russh::Pty, u32)],
        s: &mut Session,
    ) -> Result<(), Self::Error> {
        self.pty.insert(
            id,
            Pty {
                term: term.into(),
                cols: cols as u16,
                rows: rows as u16,
            },
        );
        s.channel_success(id)?;
        Ok(())
    }

    async fn env_request(
        &mut self,
        id: ChannelId,
        k: &str,
        v: &str,
        s: &mut Session,
    ) -> Result<(), Self::Error> {
        self.env.insert(k.into(), v.into());
        s.channel_success(id)?;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        id: ChannelId,
        data: &[u8],
        s: &mut Session,
    ) -> Result<(), Self::Error> {
        let cmd = String::from_utf8_lossy(data).into_owned();
        log(&format!(
            "befehl ({}): {}",
            self.adr,
            cmd.chars().take(200).collect::<String>()
        ));
        let Some(ch) = self.nimm(id).await else {
            return Ok(s.channel_failure(id)?);
        };
        s.channel_success(id)?;
        if let Some((t, code)) = eingebaut(&cmd) {
            tokio::spawn(async move {
                let _ = ch.data_bytes(t.into_bytes()).await;
                let _ = ch.exit_status(code).await;
                let _ = ch.eof().await;
                let _ = ch.close().await;
            });
            return Ok(());
        }
        let pty = self.pty.get(&id).cloned();
        match starten(Some(&cmd), pty.as_ref(), &self.env) {
            Ok(l) => {
                tokio::spawn(bedienen(ch, l));
            }
            Err(e) => {
                tokio::spawn(text_senden(ch, format!("ultar-sshd: {e}\n"), 127));
            }
        }
        Ok(())
    }

    async fn shell_request(&mut self, id: ChannelId, s: &mut Session) -> Result<(), Self::Error> {
        log(&format!("shell ({})", self.adr));
        let Some(ch) = self.nimm(id).await else {
            return Ok(s.channel_failure(id)?);
        };
        s.channel_success(id)?;
        let pty = self.pty.get(&id).cloned();
        let gruss = format!(
            "Ultar OS – Entwickler-Zugang ({}). „ultar-hilfe“ zeigt die eingebauten Befehle.\r\n",
            if pty.is_some() {
                "Terminal"
            } else {
                "ohne Terminal"
            }
        );
        let _ = ch.data_bytes(gruss.into_bytes()).await;
        match starten(None, pty.as_ref(), &self.env) {
            Ok(l) => {
                tokio::spawn(bedienen(ch, l));
            }
            Err(e) => {
                tokio::spawn(text_senden(ch, format!("ultar-sshd: {e}\n"), 127));
            }
        }
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        id: ChannelId,
        name: &str,
        s: &mut Session,
    ) -> Result<(), Self::Error> {
        if name != "sftp" {
            return Ok(s.channel_failure(id)?);
        }
        let Some(ch) = self.nimm(id).await else {
            return Ok(s.channel_failure(id)?);
        };
        s.channel_success(id)?;
        log(&format!("sftp ({})", self.adr));
        let home = if Path::new("/root").is_dir() {
            "/root"
        } else {
            "/"
        };
        let (a, b) = tokio::io::duplex(512 * 1024);
        russh_sftp::server::run(a, sftp::Sitzung::neu(home)).await;
        tokio::spawn(sftp_bruecke(ch, b));
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        ch: Channel<Msg>,
        host: &str,
        port: u32,
        _oa: &str,
        _op: u32,
        reply: russh::server::ChannelOpenHandle,
        _s: &mut Session,
    ) -> Result<(), Self::Error> {
        // nur Dienste auf dem Gerät selbst (Sprachmodell, Web-Verwaltung …) – kein Sprungbrett ins Netz
        if !matches!(host, "127.0.0.1" | "localhost" | "::1") || port == 0 || port > 65535 {
            reply
                .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        }
        let ziel = format!("127.0.0.1:{port}");
        match tokio::net::TcpStream::connect(&ziel).await {
            Ok(mut tcp) => {
                reply.accept().await;
                log(&format!("weiterleitung ({}) → {ziel}", self.adr));
                tokio::spawn(async move {
                    let mut st = ch.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut st, &mut tcp).await;
                });
            }
            Err(_) => reply.reject(russh::ChannelOpenFailure::ConnectFailed).await,
        }
        Ok(())
    }

    async fn channel_eof(&mut self, id: ChannelId, s: &mut Session) -> Result<(), Self::Error> {
        // Kanäle ohne laufenden Prozess (noch in der Tabelle) schließen
        if self.kanaele.lock().await.remove(&id).is_some() {
            s.close(id)?;
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ Befehlszeile

fn status() -> String {
    let k = wirtsschluessel()
        .ok()
        .map(|k| k.public_key().fingerprint(HashAlg::Sha256).to_string())
        .unwrap_or_else(|| "?".into());
    format!(
        "entwickler-ssh: {}\nport: {}\nwirtsschlüssel: {}\nerlaubte schlüssel: {}\nordner: {}\n",
        if eingeschaltet() { "an" } else { "aus" },
        port(),
        k,
        erlaubte_schluessel().len(),
        dir().display()
    )
}

fn schluessel_hinzu(z: &str) -> anyhow::Result<()> {
    let k = PublicKey::from_openssh(z.trim())
        .map_err(|e| anyhow::anyhow!("kein gültiger öffentlicher Schlüssel: {e}"))?;
    if schluessel_erlaubt(&k) {
        println!("schon erlaubt: {}", k.fingerprint(HashAlg::Sha256));
        return Ok(());
    }
    std::fs::create_dir_all(dir())?;
    let p = dir().join("authorized_keys");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)?;
    writeln!(f, "{}", k.to_openssh()?)?;
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    println!("erlaubt: {}", k.fingerprint(HashAlg::Sha256));
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(|s| s.as_str()) {
        Some("status") => {
            print!("{}", status());
            return Ok(());
        }
        Some("an") => {
            std::fs::create_dir_all(dir())?;
            let _ = std::fs::remove_file(dir().join("aus"));
            std::fs::write(dir().join("an"), b"")?;
            println!(
                "Entwickler-Zugang an (Port {}); wirksam nach wenigen Sekunden (Stand: ultar-sshd status).",
                port()
            );
            return Ok(());
        }
        Some("aus") => {
            let _ = std::fs::remove_file(dir().join("an"));
            std::fs::create_dir_all(dir())?;
            std::fs::write(dir().join("aus"), b"")?;
            println!("Entwickler-Zugang aus; offene Sitzungen enden in wenigen Sekunden.");
            return Ok(());
        }
        Some("schluessel") => {
            let z = a[1..].join(" ");
            if z.is_empty() {
                anyhow::bail!("ultar-sshd schluessel \"ssh-ed25519 AAAA… name\"");
            }
            return schluessel_hinzu(&z);
        }
        Some("kernlog") => {
            print!("{}", kernlog());
            return Ok(());
        }
        Some("werkzeug") => {
            // aus den Werkzeug-Skripten: ultar-sshd werkzeug <name> <argumente…>
            let zeile = a[1..]
                .iter()
                .map(|x| {
                    if x.contains(' ') && a[1] != "text" && a[1] != "helfer" {
                        format!("\"{x}\"")
                    } else {
                        x.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            let w: Vec<&str> = a[1..].iter().map(|s| s.as_str()).collect();
            let r = steuern::befehl(&w, &zeile).map(|r| r.map_err(|e| format!("fehler: {e}\n")));
            let (t, code) = match r {
                Some(Ok(t)) => (t, 0),
                Some(Err(t)) => (t, 1),
                None => match eingebaut(&zeile) {
                    Some(x) => x,
                    None => (format!("unbekannt: {zeile}\n"), 2),
                },
            };
            if code == 0 {
                print!("{t}")
            } else {
                eprint!("{t}")
            }
            std::process::exit(code as i32);
        }
        Some("-h") | Some("--help") | Some("hilfe") => {
            println!("ultar-sshd [--immer] [--port N] | status | an | aus | schluessel \"<öffentlicher Schlüssel>\" | kernlog");
            return Ok(());
        }
        _ => {}
    }
    let immer = a.iter().any(|x| x == "--immer");
    let port = a
        .iter()
        .position(|x| x == "--port")
        .and_then(|i| a.get(i + 1))
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(port);
    if immer {
        if erlaubte_schluessel().is_empty() {
            melde_zu();
            return Ok(());
        }
    } else {
        warten_bis_aktiv();
    }
    let key = wirtsschluessel()?;
    let _ = WERKZEUG_DIR.set(werkzeuge_anlegen());
    log(&format!(
        "Port {port}, Wirtsschlüssel {}",
        key.public_key().fingerprint(HashAlg::Sha256)
    ));
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let cfg = russh::server::Config {
            keys: vec![key],
            methods: russh::MethodSet::from(&[russh::MethodKind::PublicKey][..]),
            auth_rejection_time: Duration::from_secs(2),
            auth_rejection_time_initial: Some(Duration::from_millis(0)),
            inactivity_timeout: None,
            keepalive_interval: Some(Duration::from_secs(30)),
            server_id: russh::SshId::Standard("SSH-2.0-UltarOS_Entwickler".into()),
            ..Default::default()
        };
        let mut s = Server;
        let dienst = s.run_on_address(Arc::new(cfg), ("0.0.0.0", port));
        if immer {
            return dienst.await;
        }
        // zugeschaltet (`aus`, Schlüssel entfernt): Port und alle Sitzungen schließen
        let waechter = async {
            loop {
                tokio::time::sleep(PRUEFTAKT).await;
                if !aktiv() {
                    melde_zu();
                    std::process::exit(EXIT_ZU);
                }
            }
        };
        tokio::select! {
            r = dienst => r,
            _ = waechter => Ok(()),
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eingebaute_befehle() {
        assert!(eingebaut("ultar-hilfe").unwrap().0.contains("kernlog"));
        assert_eq!(eingebaut("taste quatsch").unwrap().1, 1);
        assert!(eingebaut("info").is_some());
        assert!(eingebaut("ls -la").is_none());
        assert!(
            eingebaut("text a; bildschirm").is_none(),
            "Shell-Zeile geht an /bin/sh"
        );
    }

    #[test]
    fn schluessel_pruefen() {
        let t = std::env::temp_dir().join(format!("ussh-{}", std::process::id()));
        std::fs::create_dir_all(&t).unwrap();
        std::env::set_var("ULTAR_ENTWICKLER_DIR", &t);
        let k = PrivateKey::from(Ed25519Keypair::from_seed(&[7u8; 32]));
        let fremd = PrivateKey::from(Ed25519Keypair::from_seed(&[8u8; 32]));
        assert!(!schluessel_erlaubt(k.public_key()));
        schluessel_hinzu(&k.public_key().to_openssh().unwrap()).unwrap();
        assert!(schluessel_erlaubt(k.public_key()));
        assert!(!schluessel_erlaubt(fremd.public_key()));
        assert!(schluessel_hinzu("ssh-ed25519 kaputt").is_err());
        let w1 = wirtsschluessel().unwrap();
        let w2 = wirtsschluessel().unwrap();
        assert_eq!(
            w1.public_key().key_data(),
            w2.public_key().key_data(),
            "Wirtsschlüssel bleibt gleich"
        );
        // an/aus: „aus“ gewinnt auch gegen /etc/ultar/entwickler; offen nur mit Schlüssel
        std::fs::write(t.join("an"), b"").unwrap();
        assert!(eingeschaltet());
        assert!(aktiv(), "an und ein Schlüssel: offen");
        std::fs::write(t.join("authorized_keys"), b"").unwrap();
        if !std::path::Path::new("/etc/ultar/entwickler.keys").exists() {
            assert!(!aktiv(), "an, aber kein Schlüssel: zu");
        }
        std::fs::write(t.join("aus"), b"").unwrap();
        assert!(!eingeschaltet());
        assert!(!aktiv());

        let _ = std::fs::remove_dir_all(&t);
    }
}
