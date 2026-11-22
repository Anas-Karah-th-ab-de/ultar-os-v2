//! ultar-net – Netzwerkdienst von Ultar OS.
//!
//! Kabelgebundene Schnittstellen: DHCP (eigener Client) oder statische Adresse.
//! Konfiguration: /data/ultar/net.toml (vom Nutzer, über die Oberfläche), sonst /etc/ultar/net.toml.
//! Status für die Oberfläche: /run/ultar/net/<schnittstelle>

use nix::libc;
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use nix::sys::signal::{SigSet, Signal};
use nix::sys::signalfd::{SfdFlags, SignalFd};
use serde::Deserialize;
use std::collections::HashSet;
use std::net::{Ipv4Addr, UdpSocket};
use std::os::fd::AsFd;
use std::os::fd::FromRawFd;
use std::time::{Duration, Instant};

#[derive(Deserialize, Clone)]
#[serde(default)]
struct Cfg {
    dhcp: bool,
    address: String,
    prefix: u8,
    gateway: String,
    dns: String,
    /// Schnittstellen, die der Dienst nicht anfassen soll
    ignore: Vec<String>,
}

impl Default for Cfg {
    fn default() -> Self {
        Self {
            dhcp: true,
            address: String::new(),
            prefix: 24,
            gateway: String::new(),
            dns: String::new(),
            ignore: Vec::new(),
        }
    }
}

fn load_cfg() -> Cfg {
    for p in ["/data/ultar/net.toml", "/etc/ultar/net.toml"] {
        if let Ok(s) = std::fs::read_to_string(p) {
            match toml::from_str(&s) {
                Ok(c) => return c,
                Err(e) => log(&format!("{p} fehlerhaft: {e}")),
            }
        }
    }
    Cfg::default()
}

fn log(m: &str) {
    println!("[ultar-net] {m}");
}

struct Lease {
    ip: Ipv4Addr,
    mask: Ipv4Addr,
    gw: Option<Ipv4Addr>,
    dns: Vec<Ipv4Addr>,
    secs: u32,
}

fn main() {
    let _ = std::fs::create_dir_all("/run/ultar/net");
    let cfg = load_cfg();
    log(&format!(
        "Start ({})",
        if cfg.dhcp { "DHCP" } else { "statisch" }
    ));
    // SIGTERM/SIGINT gezielt abholen (in allen Threads blockiert), damit beim Stoppen
    // der Status für die Oberfläche entfernt wird.
    let mut mask = SigSet::empty();
    mask.add(Signal::SIGTERM);
    mask.add(Signal::SIGINT);
    let _ = mask.thread_block();
    let sfd = SignalFd::with_flags(&mask, SfdFlags::SFD_NONBLOCK | SfdFlags::SFD_CLOEXEC).ok();
    let mut known: HashSet<String> = HashSet::new();
    loop {
        if let Ok(rd) = std::fs::read_dir("/sys/class/net") {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name == "lo" || known.contains(&name) || cfg.ignore.contains(&name) {
                    continue;
                }
                // nur echte Ethernet-Geräte (type 1), kein WLAN
                let ty = std::fs::read_to_string(e.path().join("type")).unwrap_or_default();
                if ty.trim() != "1" {
                    continue;
                }
                known.insert(name.clone());
                let wifi = e.path().join("wireless").exists() || e.path().join("phy80211").exists();
                // Nebenschnittstellen des WLAN-Chips (Wi-Fi Direct, Hotspot, NAN) nicht verwalten –
                // sonst startet ein zweites wpa_supplicant samt DHCP auf p2p0 (Samsung SCSC legt p2p0 an).
                if wifi
                    && ["p2p", "swlan", "ap", "aware", "nan"]
                        .iter()
                        .any(|p| name.starts_with(p))
                {
                    known.insert(name);
                    continue;
                }
                let mut c = cfg.clone();
                if wifi {
                    c.dhcp = true; // WLAN bezieht die Adresse immer vom Zugangspunkt
                }
                std::thread::spawn(move || run_iface(name, c, wifi));
            }
        }
        match &sfd {
            Some(fd) => {
                let mut p = [PollFd::new(fd.as_fd(), PollFlags::POLLIN)];
                let _ = poll(&mut p, PollTimeout::from(3000u16));
                if let Ok(Some(_)) = fd.read_signal() {
                    // eigene wpa_supplicant-Instanzen mitnehmen, sonst laufen sie verwaist weiter
                    for n in &known {
                        kill_stale_wpa(n);
                    }
                    log("beendet");
                    let _ = std::fs::remove_dir_all("/run/ultar/net");
                    std::process::exit(0);
                }
            }
            None => std::thread::sleep(Duration::from_secs(3)),
        }
    }
}

pub const WIFI_CONF: &str = "/data/ultar/wifi.conf";
/// Steuerschnittstelle von wpa_supplicant: die Oberfläche sucht, verbindet und vergisst Netze darüber,
/// ohne den Netzdienst neu zu starten. `update_config=1` erlaubt das Speichern (SAVE_CONFIG).
const WPA_HDR: &str = "ctrl_interface=DIR=/run/wpa_supplicant\nupdate_config=1\n";

/// Native statische wpa_supplicant-Binary von Ultar, sonst Ubuntu-Bereich.
fn wpa_native() -> Option<&'static str> {
    for p in [
        "/usr/lib/ultar/wpa_supplicant",
        "/sbin/wpa_supplicant",
        "/usr/sbin/wpa_supplicant",
    ] {
        if std::path::Path::new(p).exists() {
            return Some(p);
        }
    }
    None
}
const WPA_COMPAT: &str = "/compat/usr/sbin/wpa_supplicant";

/// Wartet, bis /data eingebunden ist (ultar-init bindet es im Hintergrund ein). Sonst würde die
/// WLAN-Konfiguration auf dem Wurzel-Dateisystem landen und später vom Mount verdeckt.
fn wait_data(max: Duration) {
    let t0 = Instant::now();
    while t0.elapsed() < max {
        let m = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
        if m.lines()
            .any(|l| l.split_whitespace().nth(1) == Some("/data"))
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Konfiguration anlegen bzw. ältere Dateien (ohne Steuerschnittstelle) ergänzen. Nur für root lesbar.
fn ensure_wifi_conf() {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let cur = std::fs::read_to_string(WIFI_CONF).unwrap_or_default();
    if cur
        .lines()
        .any(|l| l.trim_start().starts_with("ctrl_interface="))
    {
        return;
    }
    let _ = std::fs::create_dir_all("/data/ultar");
    let tmp = format!("{WIFI_CONF}.tmp");
    let r = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| {
            f.write_all(WPA_HDR.as_bytes())?;
            f.write_all(cur.as_bytes())?;
            f.sync_all()
        });
    match r.and_then(|_| std::fs::rename(&tmp, WIFI_CONF)) {
        Ok(()) => log("WLAN-Konfiguration vorbereitet"),
        Err(e) => log(&format!("WLAN-Konfiguration nicht schreibbar: {e}")),
    }
}

/// Schnittstelle einschalten, bis es klappt. Auf Handys lädt das Einschalten die Funk-Firmware
/// (A30: /vendor/etc/wifi/mx140.bin); solange sie nicht erreichbar ist oder der Funk per rfkill
/// gesperrt ist, scheitert es (z. B. os error 22) – daher mit wachsendem Abstand wiederholen.
fn wifi_up(name: &str) {
    let mut backoff = 1u64;
    loop {
        match set_flags(name, true) {
            Ok(()) => return,
            Err(e) => {
                log(&format!(
                    "{name}: Einschalten fehlgeschlagen: {e} – neuer Versuch in {backoff} s"
                ));
                status(name, "wlan-start-fehler", None);
            }
        }
        std::thread::sleep(Duration::from_secs(backoff));
        backoff = (backoff * 2).min(30);
    }
}

/// Alte wpa_supplicant-Prozesse für diese Schnittstelle beenden. Zwei Instanzen auf derselben
/// Schnittstelle stören sich gegenseitig: beim A30 bleibt dann der „controlled port" des Treibers
/// gesperrt – WLAN zeigt „verbunden", aber außer DHCP/ARP geht kein Paket durch (kein Internet).
fn kill_stale_wpa(iface: &str) -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else {
                continue;
            };
            if pid as u32 == std::process::id() {
                continue;
            }
            let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            let args: Vec<String> = cmd
                .split(|b| *b == 0)
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect();
            let is_wpa = args.first().is_some_and(|a| a.ends_with("wpa_supplicant"))
                || args.iter().any(|a| a.ends_with("/wpa_supplicant"));
            let ours = args.windows(2).any(|w| w[0] == "-i" && w[1] == iface)
                || args.iter().any(|a| *a == format!("-i{iface}"));
            if is_wpa && ours {
                // erst sauber beenden (meldet sich beim Zugangspunkt ab), dann notfalls hart
                unsafe { libc::kill(pid, libc::SIGTERM) };
                let t0 = Instant::now();
                while std::path::Path::new(&format!("/proc/{pid}/cmdline")).exists()
                    && t0.elapsed() < Duration::from_secs(3)
                {
                    if std::fs::read_to_string(format!("/proc/{pid}/stat"))
                        .map(|s| s.contains(") Z "))
                        .unwrap_or(false)
                    {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                unsafe { libc::kill(pid, libc::SIGKILL) };
                n += 1;
            }
        }
    }
    if n > 0 {
        log(&format!(
            "{iface}: {n} alte wpa_supplicant-Instanz(en) beendet"
        ));
        // dem Treiber Zeit für die Abmeldung lassen
        std::thread::sleep(Duration::from_millis(1500));
    }
    n
}

/// Samsung SCSC: gesperrter „controlled port" trotz Verbindung? (None = anderer Treiber)
fn port_blocked() -> Option<bool> {
    let s = std::fs::read_to_string("/proc/driver/unifi0/fcq").ok()?;
    let l = s.lines().find(|l| l.contains("Controlled port"))?;
    Some(l.contains("Blocked"))
}

/// Befehl an wpa_supplicant (Steuerschnittstelle).
fn wpa_cmd(iface: &str, cmd: &str) -> Option<String> {
    use std::os::unix::net::UnixDatagram;
    let local = format!("/run/ultar/net-wpa-{}", std::process::id());
    let _ = std::fs::remove_file(&local);
    let s = UnixDatagram::bind(&local).ok()?;
    let r = (|| {
        s.connect(format!("/run/wpa_supplicant/{iface}")).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
        s.send(cmd.as_bytes()).ok()?;
        let mut b = [0u8; 2048];
        let n = s.recv(&mut b).ok()?;
        Some(String::from_utf8_lossy(&b[..n]).into_owned())
    })();
    let _ = std::fs::remove_file(&local);
    r
}

/// Selbstheilung: Verbindung steht (carrier), aber der Treiber lässt keine Daten durch →
/// sauber trennen und neu verbinden (höchstens alle 60 s).
fn heal_port(iface: &str, last: &mut Option<Instant>) {
    if port_blocked() != Some(true) || last.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) {
        return;
    }
    *last = Some(Instant::now());
    log(&format!(
        "{iface}: Datenport des Treibers gesperrt – trenne und verbinde neu"
    ));
    let _ = wpa_cmd(iface, "DISCONNECT");
    std::thread::sleep(Duration::from_secs(2));
    let _ = wpa_cmd(iface, "RECONNECT");
}

/// wpa_supplicant starten und überwachen: endet es, wird es mit wachsendem Abstand neu gestartet.
fn wpa_supervise(name: String, native: Option<&'static str>) {
    std::thread::spawn(move || {
        let mut backoff = 1u64;
        loop {
            let _ = std::fs::create_dir_all("/run/wpa_supplicant");
            kill_stale_wpa(&name);
            let _ = std::fs::remove_file(format!("/run/wpa_supplicant/{name}")); // alte Steuer-Datei
            let mut cmd = match native {
                Some(bin) => {
                    let mut c = std::process::Command::new(bin);
                    c.args(["-i", &name, "-c", WIFI_CONF, "-D", "nl80211"]);
                    c
                }
                None => {
                    let mut c = std::process::Command::new("/usr/bin/ultar-compat");
                    c.args([
                        "run",
                        "--",
                        "/usr/sbin/wpa_supplicant",
                        "-i",
                        &name,
                        "-c",
                        WIFI_CONF,
                        "-D",
                        "nl80211,wext",
                    ]);
                    c
                }
            };
            cmd.stdin(std::process::Stdio::null());
            // SAVE_CONFIG schreibt die Datei neu – Kennwörter nur für root lesbar
            unsafe {
                use std::os::unix::process::CommandExt;
                cmd.pre_exec(|| {
                    libc::umask(0o077);
                    Ok(())
                });
            }
            let t0 = Instant::now();
            match cmd.spawn() {
                Ok(mut ch) => {
                    let st = ch.wait();
                    log(&format!("{name}: wpa_supplicant beendet ({st:?})"));
                }
                Err(e) => {
                    log(&format!("{name}: wpa_supplicant nicht startbar: {e}"));
                    status(&name, "fehler", None);
                }
            }
            if t0.elapsed() > Duration::from_secs(60) {
                backoff = 1;
            }
            std::thread::sleep(Duration::from_secs(backoff));
            backoff = (backoff * 2).min(30);
        }
    });
}

fn run_iface(name: String, cfg: Cfg, wifi: bool) {
    log(&format!(
        "{name}: gefunden{}",
        if wifi { " (WLAN)" } else { "" }
    ));
    if wifi {
        let native = wpa_native();
        if native.is_none() && !std::path::Path::new(WPA_COMPAT).exists() {
            status(&name, "wlan-programme-fehlen", None);
            return;
        }
        status(&name, "wlan-start", None);
        wifi_up(&name);
        wait_data(Duration::from_secs(60));
        ensure_wifi_conf();
        // wpa_supplicant läuft immer (auch ohne gespeichertes Netz), damit die Oberfläche
        // über seine Steuerschnittstelle suchen und verbinden kann.
        wpa_supervise(name.clone(), native);
    } else if let Err(e) = set_flags(&name, true) {
        log(&format!("{name}: aktivieren fehlgeschlagen: {e}"));
    }
    let mut backoff = 1u64;
    loop {
        if !carrier(&name) {
            status(
                &name,
                if wifi { "wlan-getrennt" } else { "kein-kabel" },
                None,
            );
            while !carrier(&name) {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        let lease = if cfg.dhcp {
            status(&name, "dhcp", None);
            match dhcp(&name) {
                Ok(l) => l,
                Err(e) => {
                    log(&format!("{name}: DHCP fehlgeschlagen: {e}"));
                    status(&name, "dhcp-fehler", None);
                    std::thread::sleep(Duration::from_secs(backoff));
                    backoff = (backoff * 2).min(30);
                    continue;
                }
            }
        } else {
            match static_lease(&cfg) {
                Some(l) => l,
                None => {
                    log(&format!("{name}: statische Konfiguration ungültig"));
                    status(&name, "konfig-fehler", None);
                    return;
                }
            }
        };
        backoff = 1;
        match apply(&name, &lease) {
            Ok(()) => {
                log(&format!(
                    "{name}: {}/{} gw {:?} dns {:?}",
                    lease.ip,
                    mask_prefix(lease.mask),
                    lease.gw,
                    lease.dns
                ));
                status(&name, "verbunden", Some(&lease));
            }
            Err(e) => {
                log(&format!("{name}: Adresse setzen fehlgeschlagen: {e}"));
                status(&name, "fehler", None);
            }
        }
        // Bis zur Erneuerung (halbe Lease-Zeit) Verbindung überwachen
        let renew = if cfg.dhcp {
            Duration::from_secs((lease.secs / 2).clamp(60, 43200) as u64)
        } else {
            Duration::from_secs(u32::MAX as u64)
        };
        let t0 = Instant::now();
        let mut healed: Option<Instant> = None;
        while t0.elapsed() < renew {
            std::thread::sleep(Duration::from_secs(2));
            if wifi && t0.elapsed() > Duration::from_secs(12) {
                heal_port(&name, &mut healed);
            }
            if !carrier(&name) {
                log(&format!("{name}: Verbindung getrennt"));
                let _ = set_addr(&name, libc::SIOCSIFADDR, Ipv4Addr::UNSPECIFIED);
                break;
            }
        }
    }
}

fn carrier(name: &str) -> bool {
    std::fs::read_to_string(format!("/sys/class/net/{name}/carrier"))
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

fn mac(name: &str) -> [u8; 6] {
    let mut m = [0u8; 6];
    if let Ok(s) = std::fs::read_to_string(format!("/sys/class/net/{name}/address")) {
        for (i, p) in s.trim().split(':').take(6).enumerate() {
            m[i] = u8::from_str_radix(p, 16).unwrap_or(0);
        }
    }
    m
}

fn mask_prefix(m: Ipv4Addr) -> u32 {
    u32::from(m).count_ones()
}

fn prefix_mask(p: u8) -> Ipv4Addr {
    Ipv4Addr::from(if p == 0 {
        0
    } else {
        u32::MAX << (32 - p.min(32) as u32)
    })
}

fn static_lease(c: &Cfg) -> Option<Lease> {
    Some(Lease {
        ip: c.address.trim().parse().ok()?,
        mask: prefix_mask(c.prefix),
        gw: c.gateway.trim().parse().ok(),
        dns: c
            .dns
            .split([',', ' '])
            .filter_map(|d| d.trim().parse().ok())
            .collect(),
        secs: u32::MAX,
    })
}

fn status(name: &str, state: &str, l: Option<&Lease>) {
    let m = mac(name);
    let wifi = std::path::Path::new(&format!("/sys/class/net/{name}/phy80211")).exists();
    let mut s = format!(
        "kind={}\nstate={state}\nmac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
        if wifi { "wlan" } else { "kabel" },
        m[0],
        m[1],
        m[2],
        m[3],
        m[4],
        m[5]
    );
    if let Some(l) = l {
        s += &format!("ip={}/{}\n", l.ip, mask_prefix(l.mask));
        if let Some(g) = l.gw {
            s += &format!("gw={g}\n");
        }
        let d: Vec<String> = l.dns.iter().map(|d| d.to_string()).collect();
        s += &format!("dns={}\n", d.join(","));
    }
    let tmp = format!("/run/ultar/net/.{name}.tmp");
    let _ = std::fs::create_dir_all("/run/ultar/net");
    if std::fs::write(&tmp, s).is_ok() {
        let _ = std::fs::rename(&tmp, format!("/run/ultar/net/{name}"));
    }
}

// ---------- Schnittstellen-Konfiguration (ioctl) ----------

fn ctl_sock() -> std::io::Result<i32> {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

fn ifreq(name: &str) -> libc::ifreq {
    let mut r: libc::ifreq = unsafe { std::mem::zeroed() };
    for (i, b) in name.bytes().take(libc::IFNAMSIZ - 1).enumerate() {
        r.ifr_name[i] = b as libc::c_char;
    }
    r
}

fn sockaddr(ip: Ipv4Addr) -> libc::sockaddr {
    let sin = libc::sockaddr_in {
        sin_family: libc::AF_INET as u16,
        sin_port: 0,
        sin_addr: libc::in_addr {
            s_addr: u32::from(ip).to_be(),
        },
        sin_zero: [0; 8],
    };
    unsafe { std::mem::transmute(sin) }
}

fn io<T>(fd: i32, ret: i32, v: T) -> std::io::Result<T> {
    let e = std::io::Error::last_os_error();
    unsafe { libc::close(fd) };
    if ret < 0 {
        Err(e)
    } else {
        Ok(v)
    }
}

fn set_flags(name: &str, up: bool) -> std::io::Result<()> {
    let fd = ctl_sock()?;
    let mut r = ifreq(name);
    unsafe {
        if libc::ioctl(fd, libc::SIOCGIFFLAGS, &mut r) < 0 {
            return io(fd, -1, ());
        }
        if up {
            r.ifr_ifru.ifru_flags |= libc::IFF_UP as i16; // IFF_RUNNING setzt nur der Treiber
        } else {
            r.ifr_ifru.ifru_flags &= !(libc::IFF_UP as i16);
        }
        let ret = libc::ioctl(fd, libc::SIOCSIFFLAGS, &r);
        io(fd, ret, ())
    }
}

fn set_addr(name: &str, req: libc::Ioctl, ip: Ipv4Addr) -> std::io::Result<()> {
    let fd = ctl_sock()?;
    let mut r = ifreq(name);
    r.ifr_ifru.ifru_addr = sockaddr(ip);
    let ret = unsafe { libc::ioctl(fd, req, &r) };
    io(fd, ret, ())
}

fn set_default_route(name: &str, gw: Ipv4Addr) -> std::io::Result<()> {
    let fd = ctl_sock()?;
    let mut dev: Vec<u8> = name.bytes().collect();
    dev.push(0);
    let mut rt: libc::rtentry = unsafe { std::mem::zeroed() };
    rt.rt_dst = sockaddr(Ipv4Addr::UNSPECIFIED);
    rt.rt_genmask = sockaddr(Ipv4Addr::UNSPECIFIED);
    rt.rt_gateway = sockaddr(gw);
    rt.rt_flags = libc::RTF_UP | libc::RTF_GATEWAY;
    rt.rt_dev = dev.as_mut_ptr() as *mut libc::c_char;
    unsafe {
        // alte Standardroute dieser Schnittstelle entfernen (Fehler egal)
        let mut old = rt;
        old.rt_gateway = sockaddr(Ipv4Addr::UNSPECIFIED);
        old.rt_flags = libc::RTF_UP;
        libc::ioctl(fd, libc::SIOCDELRT, &old);
        let ret = libc::ioctl(fd, libc::SIOCADDRT, &rt);
        io(fd, ret, ())
    }
}

fn apply(name: &str, l: &Lease) -> std::io::Result<()> {
    set_addr(name, libc::SIOCSIFADDR, l.ip)?;
    set_addr(name, libc::SIOCSIFNETMASK, l.mask)?;
    set_flags(name, true)?;
    if let Some(gw) = l.gw {
        if let Err(e) = set_default_route(name, gw) {
            if e.raw_os_error() != Some(libc::EEXIST) {
                return Err(e);
            }
        }
    }
    // Eigene DNS-Server des Nutzers (Oberfläche: Netzwerk-Werkzeuge) haben Vorrang vor DHCP
    let own: Vec<Ipv4Addr> = std::fs::read_to_string("/data/ultar/dns.override")
        .unwrap_or_default()
        .split([',', ' ', '\n'])
        .filter_map(|x| x.trim().parse().ok())
        .collect();
    let dns = if own.is_empty() { l.dns.clone() } else { own };
    if !dns.is_empty() {
        let s: String = dns.iter().map(|d| format!("nameserver {d}\n")).collect();
        let _ = std::fs::write("/run/ultar/resolv.conf", &s);
        let _ = std::fs::write("/etc/resolv.conf", &s); // schlägt bei schreibgeschütztem Root fehl – dann gilt /run
    }
    Ok(())
}

// ---------- DHCP ----------

fn dhcp_socket(name: &str) -> std::io::Result<UdpSocket> {
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let one: libc::c_int = 1;
        let sz = std::mem::size_of::<libc::c_int>() as u32;
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            &one as *const _ as *const _,
            sz,
        );
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_BROADCAST,
            &one as *const _ as *const _,
            sz,
        );
        if libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_BINDTODEVICE,
            name.as_ptr() as *const _,
            name.len() as u32,
        ) < 0
        {
            let e = std::io::Error::last_os_error();
            libc::close(fd);
            return Err(e);
        }
        let mut a: libc::sockaddr_in = std::mem::zeroed();
        a.sin_family = libc::AF_INET as u16;
        a.sin_port = 68u16.to_be();
        if libc::bind(
            fd,
            &a as *const _ as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_in>() as u32,
        ) < 0
        {
            let e = std::io::Error::last_os_error();
            libc::close(fd);
            return Err(e);
        }
        Ok(UdpSocket::from_raw_fd(fd))
    }
}

fn packet(kind: u8, xid: u32, mac: [u8; 6], req: Option<(Ipv4Addr, Ipv4Addr)>) -> Vec<u8> {
    let mut p = vec![0u8; 240];
    p[0] = 1; // BOOTREQUEST
    p[1] = 1; // Ethernet
    p[2] = 6;
    p[4..8].copy_from_slice(&xid.to_be_bytes());
    p[10] = 0x80; // Broadcast-Flag: Antwort als Broadcast (wir haben noch keine Adresse)
    p[28..34].copy_from_slice(&mac);
    p[236..240].copy_from_slice(&[99, 130, 83, 99]);
    p.extend_from_slice(&[53, 1, kind]);
    p.extend_from_slice(&[61, 7, 1]);
    p.extend_from_slice(&mac);
    if let Some((ip, server)) = req {
        p.extend_from_slice(&[50, 4]);
        p.extend_from_slice(&ip.octets());
        p.extend_from_slice(&[54, 4]);
        p.extend_from_slice(&server.octets());
    }
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    let host = host.trim().as_bytes();
    if !host.is_empty() && host.len() < 64 {
        p.extend_from_slice(&[12, host.len() as u8]);
        p.extend_from_slice(host);
    }
    p.extend_from_slice(&[55, 4, 1, 3, 6, 51]);
    p.push(255);
    p
}

struct Reply {
    kind: u8,
    yiaddr: Ipv4Addr,
    server: Option<Ipv4Addr>,
    mask: Option<Ipv4Addr>,
    gw: Option<Ipv4Addr>,
    dns: Vec<Ipv4Addr>,
    secs: u32,
}

fn parse(b: &[u8], xid: u32, mac: [u8; 6]) -> Option<Reply> {
    if b.len() < 240
        || b[0] != 2
        || b[4..8] != xid.to_be_bytes()
        || b[28..34] != mac
        || b[236..240] != [99, 130, 83, 99]
    {
        return None;
    }
    let ip4 = |v: &[u8]| (v.len() >= 4).then(|| Ipv4Addr::new(v[0], v[1], v[2], v[3]));
    let mut r = Reply {
        kind: 0,
        yiaddr: Ipv4Addr::new(b[16], b[17], b[18], b[19]),
        server: None,
        mask: None,
        gw: None,
        dns: vec![],
        secs: 3600,
    };
    let mut i = 240;
    while i < b.len() {
        let t = b[i];
        if t == 255 {
            break;
        }
        if t == 0 {
            i += 1;
            continue;
        }
        let len = *b.get(i + 1)? as usize;
        let v = b.get(i + 2..i + 2 + len)?;
        match t {
            53 => r.kind = *v.first()?,
            1 => r.mask = ip4(v),
            3 => r.gw = ip4(v),
            6 => {
                r.dns = v
                    .chunks_exact(4)
                    .map(|c| Ipv4Addr::new(c[0], c[1], c[2], c[3]))
                    .collect()
            }
            51 if len == 4 => r.secs = u32::from_be_bytes([v[0], v[1], v[2], v[3]]),
            54 => r.server = ip4(v),
            _ => {}
        }
        i += 2 + len;
    }
    Some(r)
}

fn exchange(s: &UdpSocket, pkt: &[u8], xid: u32, mac: [u8; 6], want: u8) -> std::io::Result<Reply> {
    let mut buf = [0u8; 1500];
    for attempt in 0..4u32 {
        s.send_to(pkt, (Ipv4Addr::BROADCAST, 67))?;
        let deadline = Instant::now() + Duration::from_secs(2 << attempt);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            s.set_read_timeout(Some(left))?;
            match s.recv_from(&mut buf) {
                Ok((n, _)) => {
                    if let Some(r) = parse(&buf[..n], xid, mac) {
                        if r.kind == want {
                            return Ok(r);
                        }
                        if r.kind == 6 {
                            return Err(std::io::Error::other("Server lehnt ab (NAK)"));
                        }
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    break
                }
                Err(e) => return Err(e),
            }
        }
    }
    Err(std::io::Error::other("keine Antwort vom DHCP-Server"))
}

fn dhcp(name: &str) -> std::io::Result<Lease> {
    let s = dhcp_socket(name)?;
    let m = mac(name);
    let mut x = [0u8; 4];
    let _ = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut x));
    let xid = u32::from_be_bytes(x);
    let offer = exchange(&s, &packet(1, xid, m, None), xid, m, 2)?;
    let server = offer.server.unwrap_or(Ipv4Addr::BROADCAST);
    let ack = exchange(
        &s,
        &packet(3, xid, m, Some((offer.yiaddr, server))),
        xid,
        m,
        5,
    )?;
    Ok(Lease {
        ip: ack.yiaddr,
        mask: ack.mask.unwrap_or(Ipv4Addr::new(255, 255, 255, 0)),
        gw: ack.gw,
        dns: ack.dns,
        secs: ack.secs,
    })
}
