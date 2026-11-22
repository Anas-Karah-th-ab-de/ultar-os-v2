//! Zelle starten, abschotten, stoppen.
//!
//! Ablauf: `zelle start` (root) baut Netz, Grafikwahl, cgroup und Ersatzdateien, startet ggf. den Wächter und dann
//! `zelle innen` in neuem Mount-, PID-, UTS- und IPC-Namensraum im Netz der Zelle. Dort richtet der erste Prozess
//! (PID 1) die Sicht ein – nur eigener Speicher, nur erlaubte Geräte, keine Geräteinfo – und startet die App mit
//! eigener Benutzerkennung ohne Sonderrechte (no_new_privs).

use crate::gpu;
use crate::netz;
use crate::profil::{ordner, Profil};
use nix::mount::{mount, umount2, MntFlags, MsFlags};
use std::ffi::CString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

fn m(quelle: Option<&str>, ziel: &Path, typ: Option<&str>, flags: MsFlags, daten: Option<&str>) -> Result<(), String> {
    mount(quelle, ziel, typ, flags, daten).map_err(|e| format!("mount {}: {e}", ziel.display()))
}

fn binden(q: &Path, z: &Path, nur_lesen: bool) -> Result<(), String> {
    if q.is_dir() {
        std::fs::create_dir_all(z).map_err(|e| e.to_string())?;
    } else {
        if let Some(p) = z.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        if !z.exists() {
            std::fs::write(z, b"").map_err(|e| format!("{}: {e}", z.display()))?;
        }
    }
    m(Some(&q.to_string_lossy()), z, None, MsFlags::MS_BIND | MsFlags::MS_REC, None)?;
    if nur_lesen {
        m(None, z, None, MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY | MsFlags::MS_NOSUID | MsFlags::MS_NODEV, None)?;
    }
    Ok(())
}

fn tmpfs(z: &Path, modus: &str) -> Result<(), String> {
    std::fs::create_dir_all(z).map_err(|e| e.to_string())?;
    m(Some("tmpfs"), z, Some("tmpfs"), MsFlags::MS_NOSUID | MsFlags::MS_NODEV, Some(&format!("mode={modus}")))
}

fn unter(wurzel: &Path, p: &str) -> PathBuf {
    wurzel.join(p.trim_start_matches('/'))
}

/// Befehl und Umgebung der Zelle (eigener Befehl oder installierte App aus uapp)
pub fn programm(p: &Profil) -> Result<(Vec<String>, Vec<(String, String)>, String), String> {
    if !p.befehl.is_empty() {
        return Ok((p.befehl.clone(), p.umgebung.clone(), p.wurzel.clone()));
    }
    if p.app.is_empty() {
        return Err("weder befehl noch app angegeben".into());
    }
    let pfad = std::env::var("ULTAR_DATA").map(PathBuf::from).unwrap_or_else(|_| "/data/ultar".into()).join("apps.d").join(format!("{}.toml", p.app));
    let s = std::fs::read_to_string(&pfad).map_err(|_| format!("App „{}“ ist nicht installiert", p.app))?;
    let v: toml::Value = toml::from_str(&s).map_err(|e| e.to_string())?;
    let run: Vec<String> = v.get("run").and_then(|r| r.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    if run.is_empty() {
        return Err(format!("App „{}“ hat keinen Startbefehl", p.app));
    }
    let mut env: Vec<(String, String)> = v
        .get("env")
        .and_then(|e| e.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_array()).filter_map(|kv| Some((kv.first()?.as_str()?.to_string(), kv.get(1)?.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    // eigener Wine-Bereich je Zelle (Kopie beim ersten Start, siehe innen)
    let mut ursprung = None;
    for (k, val) in env.iter_mut() {
        if k == "WINEPREFIX" {
            ursprung = Some(val.clone());
            *val = "/data/zelle/wine".into();
        }
    }
    if let Some(u) = ursprung {
        env.push(("WINEPREFIX_URSPRUNG".into(), u));
    }
    env.extend(p.umgebung.clone());
    let nativ = v.get("format").and_then(|f| f.as_str()) == Some("uap");
    let wurzel = if nativ { p.wurzel.clone() } else { "/compat".into() };
    Ok((run, env, wurzel))
}

fn ersatzdateien(p: &Profil) -> Result<PathBuf, String> {
    let d = ordner(&p.name).join(".zelle");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    // eigene Geräte-ID je Zelle (bleibt gleich, verrät aber nichts über das Gerät)
    let mid = d.join("machine-id");
    if !mid.exists() {
        let mut b = [0u8; 16];
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            use std::io::Read;
            let _ = f.read_exact(&mut b);
        }
        std::fs::write(&mid, format!("{}\n", b.iter().map(|x| format!("{x:02x}")).collect::<String>())).map_err(|e| e.to_string())?;
    }
    let kerne = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let mut cpu = String::new();
    for i in 0..kerne {
        cpu += &format!("processor\t: {i}\nvendor_id\t: Ultar\nmodel name\t: Ultar Prozessor\ncpu MHz\t\t: 2000.000\ncache size\t: 1024 KB\nflags\t\t: fpu sse sse2\n\n");
    }
    std::fs::write(d.join("cpuinfo"), cpu).map_err(|e| e.to_string())?;
    std::fs::write(d.join("hostname"), format!("{}\n", p.privat.rechnername)).map_err(|e| e.to_string())?;
    let mut hosts = format!("127.0.0.1\tlocalhost {}\n::1\tlocalhost\n", p.privat.rechnername);
    for v in &p.verknuepft {
        let (_, ip) = netz::adressen(v);
        hosts += &format!("{ip}\t{v} {v}.zelle\n");
    }
    std::fs::write(d.join("hosts"), hosts).map_err(|e| e.to_string())?;
    let dns: String = netz::dns().iter().map(|a| format!("nameserver {a}\n")).collect();
    std::fs::write(d.join("resolv.conf"), dns).map_err(|e| e.to_string())?;
    // vorgegebene Identität: Zeitzone und Ort (die App sieht nur diese Werte)
    std::fs::write(d.join("timezone"), format!("{}\n", p.privat.zeitzone)).map_err(|e| e.to_string())?;
    std::fs::write(d.join("gnss.nmea"), nmea(&p.privat.ort)).map_err(|e| e.to_string())?;
    Ok(d)
}

/// NMEA-Sätze (GGA + RMC) für einen vorgegebenen Ort – leer ohne Ort
pub fn nmea(ort: &str) -> String {
    let Some((b, l)) = ort.split_once(',').and_then(|(b, l)| Some((b.trim().parse::<f64>().ok()?, l.trim().parse::<f64>().ok()?))) else {
        return String::new();
    };
    let grad = |x: f64, stellen: usize| {
        let g = x.abs().trunc();
        format!("{:0w$}{:07.4}", g as u32, (x.abs() - g) * 60.0, w = stellen)
    };
    let (ns, ew) = (if b < 0.0 { "S" } else { "N" }, if l < 0.0 { "W" } else { "E" });
    let satz = |s: String| {
        let sum = s.bytes().fold(0u8, |a, c| a ^ c);
        format!("${s}*{sum:02X}\r\n")
    };
    satz(format!("GPGGA,120000.00,{},{ns},{},{ew},1,08,0.9,100.0,M,0.0,M,,", grad(b, 2), grad(l, 3)))
        + &satz(format!("GPRMC,120000.00,A,{},{ns},{},{ew},0.0,0.0,010126,,,A", grad(b, 2), grad(l, 3)))
}

/// Locale aus einer Kurzangabe („ar“ → „ar_SA.UTF-8“)
pub fn locale(s: &str) -> String {
    if s.contains('_') || s.contains('.') {
        return s.to_string();
    }
    match s {
        "ar" => "ar_SA.UTF-8",
        "de" => "de_DE.UTF-8",
        "en" => "en_US.UTF-8",
        "fr" => "fr_FR.UTF-8",
        "tr" => "tr_TR.UTF-8",
        "es" => "es_ES.UTF-8",
        _ => "C.UTF-8",
    }
    .into()
}

/// Firefox-Einstellungen im Profil der Zelle: vorgegebener Ort statt echter Ortung, sonst Ortung aus
fn firefox_vorgaben(home: &Path, p: &Profil) {
    let mut prefs = String::from("// von Ultar-Zellen gesetzt\nuser_pref(\"geo.provider.use_geoclue\", false);\nuser_pref(\"geo.provider.use_gpsd\", false);\nuser_pref(\"geo.provider.network.logging.enabled\", false);\n");
    if let Some((b, l)) = p.privat.ort.split_once(',') {
        prefs += &format!("user_pref(\"geo.enabled\", true);\nuser_pref(\"geo.provider.network.url\", \"data:application/json,{{\\\"location\\\":{{\\\"lat\\\":{b},\\\"lng\\\":{l}}},\\\"accuracy\\\":25}}\");\n");
    } else if !p.privat.standort {
        prefs += "user_pref(\"geo.enabled\", false);\n";
    }
    if !p.privat.sprache.is_empty() {
        let l = locale(&p.privat.sprache);
        let kurz = l.split('.').next().unwrap_or("").replace('_', "-");
        prefs += &format!("user_pref(\"intl.accept_languages\", \"{kurz}\");\nuser_pref(\"intl.locale.requested\", \"{kurz}\");\n");
    }
    let basis = home.join(".mozilla/firefox");
    let mut profile: Vec<PathBuf> = std::fs::read_dir(&basis).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.join("prefs.js").exists() || p.join("times.json").exists()).collect()).unwrap_or_default();
    if profile.is_empty() {
        let d = basis.join("ultar.default");
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(basis.join("profiles.ini"), "[General]\nStartWithLastProfile=1\n\n[Profile0]\nName=default\nIsRelative=1\nPath=ultar.default\nDefault=1\n");
        profile.push(d);
    }
    for d in profile {
        let _ = std::fs::write(d.join("user.js"), &prefs);
    }
}

/// gemeinsamer Ordner zweier verknüpfter Zellen (Reihenfolge egal)
pub fn gemeinsam(a: &str, b: &str) -> PathBuf {
    let (x, y) = if a < b { (a, b) } else { (b, a) };
    crate::wurzel().join("_gemeinsam").join(format!("{x}+{y}"))
}

fn cgroup(p: &Profil) -> Option<PathBuf> {
    let basis = Path::new("/sys/fs/cgroup");
    if !basis.join("cgroup.controllers").exists() {
        return None; // kein cgroup v2
    }
    let _ = std::fs::write(basis.join("cgroup.subtree_control"), "+memory +cpu +pids");
    let eltern = basis.join("ultar-zellen");
    let _ = std::fs::create_dir_all(&eltern);
    let _ = std::fs::write(eltern.join("cgroup.subtree_control"), "+memory +cpu +pids");
    let g = eltern.join(&p.name);
    std::fs::create_dir_all(&g).ok()?;
    // dynamisch: keine feste Grenze (nimmt, was frei ist); fest: harte Grenze
    let max = if p.speicher.ram_mb == 0 { "max".to_string() } else { (p.speicher.ram_mb << 20).to_string() };
    let _ = std::fs::write(g.join("memory.max"), max);
    let _ = std::fs::write(g.join("memory.low"), (p.speicher.ram_garantiert_mb << 20).to_string());
    let _ = std::fs::write(g.join("pids.max"), "4096");
    Some(g)
}

#[derive(serde::Serialize, Default)]
pub struct Bericht {
    pub name: String,
    pub pid: u32,
    pub gpu: String,
    pub netz: String,
    pub ip: String,
}

/// Zelle starten (als root); liefert den laufenden Prozess
pub fn starten(p: &Profil, extra: &[String]) -> Result<(std::process::Child, Bericht), String> {
    if !nix::unistd::geteuid().is_root() {
        return Err("Zellen startet nur root (über ultar-init/uapp)".into());
    }
    let (mut befehl, umgebung, wurzel) = programm(p)?;
    befehl.extend(extra.iter().cloned());
    let d = ordner(&p.name);
    std::fs::create_dir_all(d.join("daten")).map_err(|e| e.to_string())?;
    let uid = p.uid();
    let _ = nix::unistd::chown(&d.join("daten"), Some(uid.into()), Some(uid.into()));
    ersatzdateien(p)?;
    for v in &p.verknuepft {
        let g = gemeinsam(&p.name, v);
        let _ = std::fs::create_dir_all(&g);
        // gemeinsame Gruppe: beide Zellen dürfen schreiben
        let _ = std::fs::set_permissions(&g, std::os::unix::fs::PermissionsExt::from_mode(0o1777));
    }
    netz::aufbauen(p)?;
    let modus = if p.gpu.is_empty() { gpu::vorgabe() } else { p.gpu.clone() };
    let w = gpu::waehlen(&modus, &gpu::karten());
    if p.protokoll.verbindungen {
        crate::waechter::regeln_setzen(p)?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        std::process::Command::new("ip")
            .args(["netns", "exec", &netz::ns(&p.name)])
            .arg(&exe)
            .args(["waechter", &p.name])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Wächter: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    let cg = cgroup(p);
    let auftrag = serde_json::json!({"befehl": befehl, "umgebung": umgebung, "wurzel": wurzel, "geraete": w.geraete, "gpu_umgebung": w.umgebung});
    std::fs::write(d.join(".zelle/auftrag.json"), auftrag.to_string()).map_err(|e| e.to_string())?;
    let ns_datei = std::fs::File::open(Path::new("/run/netns").join(netz::ns(&p.name))).map_err(|e| format!("Netz der Zelle: {e}"))?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["innen", &p.name]);
    let cg2 = cg.clone();
    unsafe {
        cmd.pre_exec(move || {
            use std::os::fd::AsRawFd;
            if libc::setns(ns_datei.as_raw_fd(), libc::CLONE_NEWNET) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::unshare(libc::CLONE_NEWNS | libc::CLONE_NEWUTS | libc::CLONE_NEWIPC | libc::CLONE_NEWPID) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if let Some(g) = &cg2 {
                let _ = std::fs::write(g.join("cgroup.procs"), "0");
            }
            Ok(())
        });
    }
    let k = cmd.spawn().map_err(|e| format!("Zelle starten: {e}"))?;
    let _ = std::fs::write(d.join(".zelle/pid"), k.id().to_string());
    let (_, ip) = netz::adressen(&p.name);
    let pid = k.id();
    Ok((k, Bericht { name: p.name.clone(), pid, gpu: w.begruendung, netz: p.netz.art.clone(), ip: if p.netz.art == "aus" { String::new() } else { ip.to_string() } }))
}

/// läuft in der Zelle: Sicht einrichten (PID 1) und die App starten
pub fn innen(name: &str) -> i32 {
    let p = match Profil::laden(name) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("zelle: {e}");
            return 1;
        }
    };
    // erster Kindprozess = PID 1 im neuen PID-Namensraum
    match unsafe { nix::unistd::fork() } {
        Ok(nix::unistd::ForkResult::Parent { child }) => match nix::sys::wait::waitpid(child, None) {
            Ok(nix::sys::wait::WaitStatus::Exited(_, c)) => c,
            _ => 1,
        },
        Ok(nix::unistd::ForkResult::Child) => {
            let c = match einrichten_und_starten(&p) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("zelle {name}: {e}");
                    126
                }
            };
            std::process::exit(c)
        }
        Err(e) => {
            eprintln!("zelle: fork: {e}");
            1
        }
    }
}

fn einrichten_und_starten(p: &Profil) -> Result<i32, String> {
    let d = ordner(&p.name);
    let a: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(d.join(".zelle/auftrag.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let liste = |k: &str| -> Vec<String> { a[k].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() };
    let paare = |k: &str| -> Vec<(String, String)> { a[k].as_array().map(|v| v.iter().filter_map(|x| Some((x[0].as_str()?.to_string(), x[1].as_str()?.to_string()))).collect()).unwrap_or_default() };
    let befehl = liste("befehl");
    let geraete = liste("geraete");
    let mut umgebung = paare("umgebung");
    umgebung.extend(paare("gpu_umgebung"));
    let wurzel = PathBuf::from(a["wurzel"].as_str().unwrap_or("/"));

    // 1. alles privat: nichts dringt hinaus oder herein
    m(None, Path::new("/"), None, MsFlags::MS_REC | MsFlags::MS_PRIVATE, None)?;
    // 2. eigene Daten und verknüpfte Ordner vorab an einen Zwischenplatz (gleich wird /data verdeckt)
    let zwischen = PathBuf::from("/run/zellen-zwischen").join(&p.name);
    binden(&d.join("daten"), &zwischen.join("daten"), false)?;
    binden(&d.join(".zelle"), &zwischen.join("ersatz"), true)?;
    let ersatz = zwischen.join("ersatz");
    for v in &p.verknuepft {
        binden(&gemeinsam(&p.name, v), &zwischen.join("gemeinsam").join(v), false)?;
    }
    // Wine: eigener Bereich je Zelle – beim ersten Start Kopie (reflink, wo das Dateisystem es kann)
    if let Some((_, alt)) = programm(p).ok().and_then(|(_, env, _)| env.into_iter().find(|(k, _)| k == "WINEPREFIX_URSPRUNG")) {
        let ziel = d.join("daten/wine");
        if !ziel.exists() && Path::new(&alt).exists() {
            let _ = std::process::Command::new("cp").args(["-a", "--reflink=auto", &alt, &ziel.to_string_lossy()]).status();
            let _ = std::process::Command::new("chown").args(["-R", &format!("{0}:{0}", p.uid()), &ziel.to_string_lossy()]).status();
        }
    }
    // 3. frisches /proc für den neuen PID-Raum, /tmp leer
    m(Some("proc"), &unter(&wurzel, "/proc"), Some("proc"), MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC, None)?;
    tmpfs(&unter(&wurzel, "/tmp"), "1777")?;
    // 4. fremden Speicher verdecken
    for v in ["/data", "/home", "/root", "/media", "/mnt", "/srv", "/run/ultar", "/var/lib/ultar", "/run/user", "/var/log"] {
        let z = unter(&wurzel, v);
        if z.is_dir() {
            tmpfs(&z, "755")?;
        }
    }
    // 5. eigene Daten + Verknüpfungen einhängen
    let home = unter(&wurzel, "/data/zelle");
    binden(&zwischen.join("daten"), &home, false)?;
    for v in &p.verknuepft {
        binden(&zwischen.join("gemeinsam").join(v), &unter(&wurzel, &format!("/data/gemeinsam/{v}")), false)?;
    }
    // 6. Geräte: nur das Nötigste + was erlaubt ist
    let dev = unter(&wurzel, "/dev");
    let hostdev = zwischen.join("hostdev");
    binden(Path::new("/dev"), &hostdev, false)?;
    tmpfs(&dev, "755")?;
    let mut erlaubt: Vec<String> = ["null", "zero", "full", "random", "urandom", "tty"].iter().map(|s| format!("/dev/{s}")).collect();
    erlaubt.extend(geraete);
    if p.privat.mikrofon || p.privat.lautsprecher {
        erlaubt.push("/dev/snd".into());
    }
    if p.privat.kamera {
        for e in std::fs::read_dir("/dev").map(|d| d.flatten().collect::<Vec<_>>()).unwrap_or_default() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("video") || n.starts_with("media") {
                erlaubt.push(format!("/dev/{n}"));
            }
        }
    }
    if p.privat.usb {
        erlaubt.push("/dev/bus/usb".into());
    }
    if p.privat.standort {
        for e in std::fs::read_dir("/dev").map(|d| d.flatten().collect::<Vec<_>>()).unwrap_or_default() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("ttyACM") || n.starts_with("gnss") {
                erlaubt.push(format!("/dev/{n}"));
            }
        }
    }
    for g in &erlaubt {
        let q = unter(&hostdev, g.trim_start_matches("/dev"));
        if q.exists() {
            binden(&q, &unter(&wurzel, g), false)?;
        }
    }
    let pts = dev.join("pts");
    std::fs::create_dir_all(&pts).map_err(|e| e.to_string())?;
    let _ = m(Some("devpts"), &pts, Some("devpts"), MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC, Some("newinstance,ptmxmode=0666,mode=620"));
    let _ = std::os::unix::fs::symlink("pts/ptmx", dev.join("ptmx"));
    tmpfs(&dev.join("shm"), "1777")?;
    for (n, z) in [("fd", "/proc/self/fd"), ("stdin", "/proc/self/fd/0"), ("stdout", "/proc/self/fd/1"), ("stderr", "/proc/self/fd/2")] {
        let _ = std::os::unix::fs::symlink(z, dev.join(n));
    }
    // 7. Geräteinfo: neutraler Prozessor, eigene Geräte-ID, keine DMI/Firmware/Akku-Daten
    let sys = unter(&wurzel, "/sys");
    std::fs::create_dir_all(&sys).map_err(|e| e.to_string())?;
    let _ = m(Some("sysfs"), &sys, Some("sysfs"), MsFlags::MS_RDONLY | MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC, None);
    if !p.privat.geraeteinfo {
        binden(&ersatz.join("cpuinfo"), &unter(&wurzel, "/proc/cpuinfo"), true)?;
        for v in ["/sys/class/dmi", "/sys/devices/virtual/dmi", "/sys/firmware", "/sys/class/power_supply", "/sys/class/hwmon", "/sys/bus/usb", "/sys/class/bluetooth", "/proc/device-tree", "/sys/class/ieee80211"] {
            let z = unter(&wurzel, v);
            if z.is_dir() {
                let _ = tmpfs(&z, "555");
            }
        }
        if !p.privat.standort {
            let _ = tmpfs(&unter(&wurzel, "/sys/class/gnss"), "555");
        }
        for f in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
            if unter(&wurzel, f).exists() {
                binden(&ersatz.join("machine-id"), &unter(&wurzel, f), true)?;
            }
        }
    }
    for (q, z) in [("hostname", "/etc/hostname"), ("hosts", "/etc/hosts"), ("resolv.conf", "/etc/resolv.conf")] {
        let ziel = unter(&wurzel, z);
        if ziel.is_symlink() {
            let _ = std::fs::remove_file(&ziel); // z. B. resolv.conf → systemd: eigene Datei
        }
        let _ = binden(&ersatz.join(q), &ziel, true);
    }
    // vorgegebene Zeitzone und Ort
    if !p.privat.zeitzone.is_empty() && !p.privat.zeitzone.contains("..") {
        let zi = unter(&wurzel, &format!("/usr/share/zoneinfo/{}", p.privat.zeitzone));
        let lt = unter(&wurzel, "/etc/localtime");
        if zi.is_file() && (lt.exists() || lt.is_symlink()) {
            let _ = binden(&zi, &std::fs::canonicalize(&lt).unwrap_or(lt), true);
        }
        let tz = unter(&wurzel, "/etc/timezone");
        if tz.exists() {
            let _ = binden(&ersatz.join("timezone"), &tz, true);
        }
    }
    if !p.privat.ort.is_empty() {
        // Ultar-Ortung (/run/ultar/gnss.nmea) liefert in der Zelle nur den vorgegebenen Ort
        let run = unter(&wurzel, "/run/ultar");
        if run.is_dir() {
            let _ = binden(&ersatz.join("gnss.nmea"), &run.join("gnss.nmea"), true);
        }
    }
    nix::unistd::sethostname(&p.privat.rechnername).map_err(|e| format!("Rechnername: {e}"))?;
    // Zwischenplätze lösen (die Einhängungen bleiben an ihren Zielen)
    let _ = umount2(&zwischen.join("ersatz"), MntFlags::MNT_DETACH);
    let _ = umount2(&zwischen.join("hostdev"), MntFlags::MNT_DETACH);
    let _ = umount2(&zwischen.join("daten"), MntFlags::MNT_DETACH);
    for v in &p.verknuepft {
        let _ = umount2(&zwischen.join("gemeinsam").join(v), MntFlags::MNT_DETACH);
    }
    // 8. Wurzel wechseln, Rechte abgeben
    if wurzel != Path::new("/") {
        nix::unistd::chroot(&wurzel).map_err(|e| format!("chroot: {e}"))?;
    }
    std::env::set_current_dir("/data/zelle").map_err(|e| e.to_string())?;
    let uid = p.uid();
    nix::unistd::setgroups(&[]).map_err(|e| format!("setgroups: {e}"))?;
    nix::unistd::setgid(uid.into()).map_err(|e| format!("setgid: {e}"))?;
    nix::unistd::setuid(uid.into()).map_err(|e| format!("setuid: {e}"))?;
    unsafe {
        libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
    }
    // 9. App als Kind von PID 1 starten, PID 1 räumt verwaiste Prozesse ab
    let mut cmd = std::process::Command::new(&befehl[0]);
    cmd.args(&befehl[1..]).env_clear();
    cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin:/usr/games").env("HOME", "/data/zelle").env("USER", "zelle").env("LANG", std::env::var("LANG").unwrap_or_else(|_| "C.UTF-8".into()));
    for k in ["DISPLAY", "WAYLAND_DISPLAY", "TERM"] {
        if let Ok(v) = std::env::var(k) {
            cmd.env(k, v);
        }
    }
    if !p.privat.zeitzone.is_empty() {
        cmd.env("TZ", &p.privat.zeitzone);
    }
    if !p.privat.sprache.is_empty() {
        let l = locale(&p.privat.sprache);
        cmd.env("LANG", &l).env("LC_ALL", &l).env("LANGUAGE", l.split('.').next().unwrap_or(""));
    }
    if !p.privat.ort.is_empty() {
        cmd.env("ULTAR_STANDORT", &p.privat.ort);
    }
    firefox_vorgaben(Path::new("/data/zelle"), p);
    for (k, v) in umgebung {
        cmd.env(k, v);
    }
    let mut kind = cmd.spawn().map_err(|e| format!("{}: {e}", befehl[0]))?;
    let haupt = kind.id() as i32;
    loop {
        match nix::sys::wait::wait() {
            Ok(nix::sys::wait::WaitStatus::Exited(pid, c)) if pid.as_raw() == haupt => {
                let _ = kind.try_wait();
                return Ok(c);
            }
            Ok(nix::sys::wait::WaitStatus::Signaled(pid, s, _)) if pid.as_raw() == haupt => return Ok(128 + s as i32),
            Ok(_) => continue,
            Err(_) => return Ok(0),
        }
    }
}

/// Zelle beenden: alle Prozesse im Netz/cgroup der Zelle, Netz abbauen
pub fn stoppen(name: &str) {
    let g = Path::new("/sys/fs/cgroup/ultar-zellen").join(name);
    if let Ok(s) = std::fs::read_to_string(g.join("cgroup.procs")) {
        for p in s.split_whitespace() {
            let _ = crate::befehl("kill", &["-9", p]);
        }
    }
    if let Ok(pid) = std::fs::read_to_string(ordner(name).join(".zelle/pid")) {
        let _ = crate::befehl("kill", &["-9", pid.trim()]);
    }
    netz::abbauen(name);
    let _ = std::fs::remove_dir(&g);
}

pub fn laeuft(name: &str) -> bool {
    std::fs::read_to_string(ordner(name).join(".zelle/pid")).ok().and_then(|s| s.trim().parse::<i32>().ok()).map(|p| Path::new(&format!("/proc/{p}")).exists()).unwrap_or(false)
}

#[allow(dead_code)]
fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}
