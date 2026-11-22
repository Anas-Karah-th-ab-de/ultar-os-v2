//! WireGuard-Client. Bringt den Tunnel hoch, routet ALLES darüber (0.0.0.0/0) und sperrt per
//! nftables jeden Verkehr außerhalb des Tunnels (Killswitch). Nutzt das geprüfte `wg`-Werkzeug;
//! wo das Kernelmodul fehlt, der boringtun-Userspace (geprüftes Rust) über /dev/net/tun.
//!
//! Die Schlüssel entstehen auf dem Gerät; der private Schlüssel verlässt es NIE. Der öffentliche
//! Schlüssel geht per ultar-cloud an /v1/vpn/peer; die Antwort füllt `WgConfig`.

use std::process::Command;

pub const WG: &str = "/usr/lib/ultar/wg";
pub const IFACE: &str = "wg0";
pub const CONF: &str = "/run/ultar/wg0.conf";

#[derive(Clone, Debug, Default)]
pub struct WgConfig {
    pub private_key: String,
    pub server_pubkey: String,
    pub endpoint: String,    // host:port
    pub address: String,     // 10.80.x.y/32
    pub dns: String,         // 10.80.0.1
    pub allowed_ips: String, // 0.0.0.0/0, ::/0
}

#[derive(Default, Debug, Clone)]
pub struct VpnStatus {
    pub up: bool,
    pub address: String,
    pub endpoint: String,
    pub rx: u64,
    pub tx: u64,
    pub last_handshake: u64,
}

fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(format!(
            "{cmd} {:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Schlüsselpaar erzeugen (privat, öffentlich).
pub fn genkeys() -> Result<(String, String), String> {
    let priv_k = run(WG, &["genkey"])?.trim().to_string();
    // pubkey aus privkey über stdin
    use std::io::Write;
    use std::process::Stdio;
    let mut c = Command::new(WG)
        .arg("pubkey")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    c.stdin
        .as_mut()
        .unwrap()
        .write_all(priv_k.as_bytes())
        .map_err(|e| e.to_string())?;
    let out = c.wait_with_output().map_err(|e| e.to_string())?;
    let pub_k = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((priv_k, pub_k))
}

fn write_conf(cfg: &WgConfig) -> Result<(), String> {
    let _ = std::fs::create_dir_all("/run/ultar");
    let txt = format!(
        "[Interface]\nPrivateKey = {}\nAddress = {}\nDNS = {}\n\n[Peer]\nPublicKey = {}\nEndpoint = {}\nAllowedIPs = {}\nPersistentKeepalive = 25\n",
        cfg.private_key, cfg.address, cfg.dns, cfg.server_pubkey, cfg.endpoint, cfg.allowed_ips
    );
    std::fs::write(CONF, txt).map_err(|e| e.to_string())
}

/// Killswitch: außer Tunnel + Handshake zum Server-Endpunkt wird alles verworfen.
fn killswitch_on(endpoint: &str) -> Result<(), String> {
    let host_port = endpoint.rsplit_once(':');
    let (host, port) = host_port.unwrap_or((endpoint, "51820"));
    // ipv4; Endpunkt-Host muss aufgelöst sein (ultar-cloud liefert i. d. R. eine IP)
    let rules = format!(
        "add table inet ultarks\n\
         add chain inet ultarks out {{ type filter hook output priority 0; policy drop; }}\n\
         add rule inet ultarks out oifname \"lo\" accept\n\
         add rule inet ultarks out oifname \"{IFACE}\" accept\n\
         add rule inet ultarks out ip daddr {host} udp dport {port} accept\n\
         add rule inet ultarks out ip daddr 10.80.0.0/16 accept\n"
    );
    use std::io::Write;
    use std::process::Stdio;
    let mut c = Command::new("nft")
        .arg("-f")
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    c.stdin
        .as_mut()
        .unwrap()
        .write_all(rules.as_bytes())
        .map_err(|e| e.to_string())?;
    if c.wait().map_err(|e| e.to_string())?.success() {
        Ok(())
    } else {
        Err("Killswitch (nft) fehlgeschlagen".into())
    }
}

fn killswitch_off() {
    let _ = Command::new("nft")
        .args(["delete", "table", "inet", "ultarks"])
        .status();
}

/// Tunnel hochbringen und gesamten Verkehr hindurchleiten.
pub fn connect(cfg: &WgConfig) -> Result<(), String> {
    write_conf(cfg)?;
    // Schnittstelle anlegen (Kernel-WireGuard). Fehlt das Modul -> boringtun-Userspace.
    if run("ip", &["link", "add", IFACE, "type", "wireguard"]).is_err() {
        // boringtun erzeugt die wg0 selbst
        let _ = Command::new("/usr/lib/ultar/boringtun")
            .args(["--disable-drop-privileges", IFACE])
            .status();
    }
    run(WG, &["setconf", IFACE, CONF])?;
    run("ip", &["address", "add", &cfg.address, "dev", IFACE]).ok();
    run("ip", &["link", "set", IFACE, "up"])?;
    // Standardroute über den Tunnel (0.0.0.0/0 in zwei Hälften, damit die Default-Route bleibt)
    run("ip", &["route", "add", "0.0.0.0/1", "dev", IFACE]).ok();
    run("ip", &["route", "add", "128.0.0.0/1", "dev", IFACE]).ok();
    // DNS über den Tunnel
    let _ = std::fs::write(
        "/run/ultar/resolv.conf",
        format!("nameserver {}\n", cfg.dns),
    );
    let _ = std::fs::copy("/run/ultar/resolv.conf", "/etc/resolv.conf");
    killswitch_on(&cfg.endpoint)?;
    Ok(())
}

/// Tunnel trennen und Killswitch entfernen.
pub fn disconnect() -> Result<(), String> {
    killswitch_off();
    let _ = run("ip", &["link", "set", IFACE, "down"]);
    let _ = run("ip", &["link", "del", IFACE]);
    let _ = Command::new("pkill").args(["-f", "boringtun"]).status();
    Ok(())
}

/// Gespeicherte Peer-Konfiguration (/run/ultar/wg-peer.conf, von ultar-cloud geschrieben) laden.
pub fn load_saved() -> Option<WgConfig> {
    let s = std::fs::read_to_string("/run/ultar/wg-peer.conf").ok()?;
    let mut c = WgConfig::default();
    for l in s.lines() {
        if let Some((k, v)) = l.split_once('=') {
            let v = v.trim().to_string();
            match k.trim() {
                "private_key" => c.private_key = v,
                "server_pubkey" => c.server_pubkey = v,
                "endpoint" => c.endpoint = v,
                "address" => c.address = v,
                "dns" => c.dns = v,
                "allowed_ips" => c.allowed_ips = v,
                _ => {}
            }
        }
    }
    if c.private_key.is_empty() || c.server_pubkey.is_empty() {
        None
    } else {
        Some(c)
    }
}

/// Tunnel aus der gespeicherten Konfiguration hochbringen.
pub fn connect_saved() -> Result<(), String> {
    let cfg = load_saved().ok_or("keine VPN-Konfiguration (erst Konto/VPN einrichten)")?;
    connect(&cfg)
}

/// Status aus `wg show` lesen.
pub fn status() -> VpnStatus {
    let mut s = VpnStatus::default();
    if let Ok(out) = run(WG, &["show", IFACE, "dump"]) {
        // Zeile 2 = Peer: pubkey, psk, endpoint, allowed, last-handshake, rx, tx, keepalive
        for (i, line) in out.lines().enumerate() {
            let f: Vec<&str> = line.split('\t').collect();
            if i == 1 && f.len() >= 8 {
                s.up = true;
                s.endpoint = f[2].to_string();
                s.last_handshake = f[4].parse().unwrap_or(0);
                s.rx = f[5].parse().unwrap_or(0);
                s.tx = f[6].parse().unwrap_or(0);
            }
        }
    }
    if let Ok(a) = run("ip", &["-o", "-4", "addr", "show", "dev", IFACE]) {
        if let Some(p) = a.split_whitespace().skip_while(|x| *x != "inet").nth(1) {
            s.address = p.to_string();
        }
    }
    s
}
