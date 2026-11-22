//! Netz der Zellen: jede Zelle hat einen eigenen Netz-Namensraum mit einer Punkt-zu-Punkt-Leitung zum Gerät
//! (10.88.x.y/30). Alles läuft über die Weiterleitung des Geräts – dort entscheidet nftables:
//! * Zellen sehen sich gegenseitig nicht – außer sie sind verknüpft.
//! * **ueber** (VPN-Zelle): der Verkehr der Zelle geht nur zur VPN-Zelle (Richtlinien-Routing), sonst nirgends hin.
//!   Die VPN-Zelle leitet nur durch ihren Tunnel weiter. Fällt der Tunnel oder die VPN-Zelle aus: kein Verkehr
//!   (Killswitch), kein Umweg über das normale Netz.
//! * **wireguard**: eigener Tunnel in der Zelle; am Tunnel vorbei darf nur der Endpunkt erreicht werden.
//! * **aus**: nur Loopback.
//! Zellen erreichen keine Dienste des Geräts selbst (Eingang gesperrt).

use crate::befehl;
use crate::profil::Profil;
use std::net::Ipv4Addr;

pub const TABELLE: &str = "ultar_zellen";

/// /30-Netz der Zelle: (Geräteseite, Zellenseite). Index stabil aus dem Namen, Kollisionen aufgelöst.
pub fn adressen(name: &str) -> (Ipv4Addr, Ipv4Addr) {
    let i = index(name);
    let basis = u32::from(Ipv4Addr::new(10, 88, 0, 0)) + i * 4;
    (Ipv4Addr::from(basis + 1), Ipv4Addr::from(basis + 2))
}

fn index(name: &str) -> u32 {
    // gespeicherter Index (bei Kollision vergeben) – sonst aus dem Namen
    let f = crate::profil::ordner(name).join("netz-index");
    if let Some(i) = std::fs::read_to_string(&f)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
    {
        return i;
    }
    let belegt: Vec<u32> = Profil::alle()
        .iter()
        .filter(|p| p.name != name)
        .filter_map(|p| {
            std::fs::read_to_string(crate::profil::ordner(&p.name).join("netz-index"))
                .ok()?
                .trim()
                .parse()
                .ok()
        })
        .collect();
    let mut i = 1 + crate::kurzhash(name) % 16000;
    while belegt.contains(&i) {
        i = 1 + (i % 16000);
    }
    let _ = std::fs::write(f, i.to_string());
    i
}

pub fn ns(name: &str) -> String {
    format!("z-{name}")
}

fn veth(name: &str) -> String {
    format!("zh{:08x}", crate::kurzhash(name))
}

fn in_ns(name: &str, args: &[&str]) -> Result<String, String> {
    let mut a = vec!["netns", "exec", &ns_s(name)]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    a.extend(args.iter().map(|s| s.to_string()));
    let r: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
    befehl("ip", &r)
}

fn ns_s(name: &str) -> String {
    ns(name)
}

pub fn besteht(name: &str) -> bool {
    std::path::Path::new("/run/netns").join(ns(name)).exists()
}

/// Netz der Zelle aufbauen (idempotent)
pub fn aufbauen(p: &Profil) -> Result<(), String> {
    let n = &p.name;
    if !besteht(n) {
        befehl("ip", &["netns", "add", &ns(n)])?;
    }
    in_ns(n, &["ip", "link", "set", "lo", "up"])?;
    if p.netz.art == "aus" {
        return regeln();
    }
    let (g, z) = adressen(n);
    let v = veth(n);
    if befehl("ip", &["link", "show", &v]).is_err() {
        befehl(
            "ip",
            &["link", "add", &v, "type", "veth", "peer", "name", "zcell0"],
        )?;
        befehl("ip", &["link", "set", "zcell0", "netns", &ns(n)])?;
        in_ns(n, &["ip", "link", "set", "zcell0", "name", "e0"])?;
    }
    let _ = befehl("ip", &["addr", "add", &format!("{g}/30"), "dev", &v]);
    befehl("ip", &["link", "set", &v, "up"])?;
    let _ = in_ns(n, &["ip", "addr", "add", &format!("{z}/30"), "dev", "e0"]);
    in_ns(n, &["ip", "link", "set", "e0", "up"])?;
    let _ = std::fs::write("/proc/sys/net/ipv4/ip_forward", "1");
    match p.netz.art.as_str() {
        "wireguard" => wireguard(p, g)?,
        _ => {
            let _ = in_ns(
                n,
                &["ip", "route", "replace", "default", "via", &g.to_string()],
            );
        }
    }
    // „ueber“: Richtlinien-Routing auf dem Gerät – alles von dieser Zelle zur VPN-Zelle
    if p.netz.art == "ueber" {
        let vpn = Profil::laden(&p.netz.ueber)?;
        if !vpn.netz.vpn_zelle {
            return Err(format!("„{}“ ist keine VPN-Zelle", vpn.name));
        }
        let (_, vz) = adressen(&vpn.name);
        let t = (1000 + index(n)).to_string();
        let _ = befehl("ip", &["rule", "del", "from", &z.to_string(), "table", &t]);
        befehl(
            "ip",
            &[
                "rule",
                "add",
                "from",
                &z.to_string(),
                "table",
                &t,
                "priority",
                "100",
            ],
        )?;
        // Rückfall „unerreichbar“: fällt die VPN-Zelle weg, endet die Suche hier – kein Umweg über das normale Netz
        befehl(
            "ip",
            &[
                "route",
                "replace",
                "unreachable",
                "default",
                "metric",
                "4000",
                "table",
                &t,
            ],
        )?;
        let _ = befehl(
            "ip",
            &[
                "route",
                "replace",
                "default",
                "via",
                &vz.to_string(),
                "dev",
                &veth(&vpn.name),
                "metric",
                "10",
                "table",
                &t,
            ],
        );
        befehl(
            "ip",
            &[
                "route",
                "replace",
                &format!("{z}/32"),
                "dev",
                &v,
                "table",
                &t,
            ],
        )?;
    }
    // VPN-Zelle: weiterleiten, aber nur durch den Tunnel
    if p.netz.vpn_zelle {
        in_ns(n, &["sh", "-c", "echo 1 > /proc/sys/net/ipv4/ip_forward"])?;
        in_ns(
            n,
            &[
                "ip",
                "route",
                "replace",
                "10.88.0.0/16",
                "via",
                &g.to_string(),
            ],
        )?;
        let iface = &p.netz.vpn_schnittstelle;
        let regel = format!(
            "table inet ultar_vpn {{ chain weiter {{ type filter hook forward priority 0; policy drop; ct state established,related accept; iifname \"e0\" oifname \"{iface}\" accept; }}\n \
             chain nat {{ type nat hook postrouting priority 100; oifname \"{iface}\" masquerade; }}\n}}\n"
        );
        let _ = in_ns(n, &["nft", "delete", "table", "inet", "ultar_vpn"]);
        nft_in(n, &regel)?;
    }
    regeln()
}

fn nft_in(name: &str, regel: &str) -> Result<(), String> {
    use std::io::Write;
    let mut k = std::process::Command::new("ip")
        .args(["netns", "exec", &ns(name), "nft", "-f", "-"])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    k.stdin
        .take()
        .unwrap()
        .write_all(regel.as_bytes())
        .map_err(|e| e.to_string())?;
    let o = k.wait_with_output().map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        Err(format!(
            "nft: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ))
    }
}

pub fn nft(regel: &str) -> Result<(), String> {
    use std::io::Write;
    let mut k = std::process::Command::new("nft")
        .args(["-f", "-"])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    k.stdin
        .take()
        .unwrap()
        .write_all(regel.as_bytes())
        .map_err(|e| e.to_string())?;
    let o = k.wait_with_output().map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        Err(format!(
            "nft: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ))
    }
}

fn wireguard(p: &Profil, g: Ipv4Addr) -> Result<(), String> {
    let n = &p.name;
    if p.netz.wg_conf.is_empty() || p.netz.wg_endpunkt.is_empty() {
        return Err("wireguard: wg_conf und wg_endpunkt angeben".into());
    }
    let ep_ip = p
        .netz
        .wg_endpunkt
        .rsplit_once(':')
        .map(|x| x.0)
        .unwrap_or(&p.netz.wg_endpunkt)
        .trim_matches(['[', ']'])
        .to_string();
    if in_ns(n, &["ip", "link", "show", "wg0"]).is_err() {
        in_ns(n, &["ip", "link", "add", "wg0", "type", "wireguard"])?;
    }
    in_ns(n, &["wg", "setconf", "wg0", &p.netz.wg_conf])?;
    if !p.netz.wg_adresse.is_empty() {
        let _ = in_ns(n, &["ip", "addr", "add", &p.netz.wg_adresse, "dev", "wg0"]);
    }
    in_ns(n, &["ip", "link", "set", "wg0", "up"])?;
    // nur der Endpunkt am Tunnel vorbei, alles andere durch den Tunnel – ohne Tunnel: nichts
    in_ns(
        n,
        &[
            "ip",
            "route",
            "replace",
            &format!("{ep_ip}/32"),
            "via",
            &g.to_string(),
        ],
    )?;
    in_ns(n, &["ip", "route", "replace", "default", "dev", "wg0"])?;
    Ok(())
}

/// Netz der Zelle abbauen
pub fn abbauen(name: &str) {
    let (_, z) = adressen(name);
    let t = (1000 + index(name)).to_string();
    let _ = befehl("ip", &["rule", "del", "from", &z.to_string(), "table", &t]);
    let _ = befehl("ip", &["route", "flush", "table", &t]);
    if let Ok(pids) = befehl("ip", &["netns", "pids", &ns(name)]) {
        for p in pids.split_whitespace() {
            let _ = befehl("kill", &["-9", p]);
        }
    }
    let _ = befehl("ip", &["link", "del", &veth(name)]);
    let _ = befehl("ip", &["netns", "del", &ns(name)]);
    let _ = regeln();
}

/// nftables-Regeln des Geräts aus allen Zellen neu erzeugen
pub fn regeln() -> Result<(), String> {
    let alle = Profil::alle();
    let mut fwd = String::new();
    let mut zwischen = String::new();
    for p in alle
        .iter()
        .filter(|p| p.netz.art != "aus" && besteht(&p.name))
    {
        let (_, z) = adressen(&p.name);
        match p.netz.art.as_str() {
            "ueber" => {
                if let Ok(vpn) = Profil::laden(&p.netz.ueber) {
                    let (_, vz) = adressen(&vpn.name);
                    let (va, vv) = (veth(&p.name), veth(&vpn.name));
                    // Killswitch: nur über die Leitung der VPN-Zelle, nie direkt hinaus oder herein
                    fwd += &format!("    iifname \"{va}\" oifname != \"{vv}\" drop\n    oifname \"{va}\" iifname != \"{vv}\" drop\n");
                    zwischen += &format!("    ip saddr {z} ip daddr {vz} accept\n    ip saddr {vz} ip daddr {z} accept\n");
                } else {
                    fwd += &format!("    ip saddr {z} drop\n");
                }
            }
            "wireguard" => {
                let ep = p
                    .netz
                    .wg_endpunkt
                    .rsplit_once(':')
                    .map(|x| x.0.to_string())
                    .unwrap_or_default();
                if !ep.is_empty() {
                    fwd += &format!("    ip saddr {z} ip daddr != {ep} drop\n");
                }
            }
            _ => {}
        }
        for v in &p.verknuepft {
            if Profil::laden(v).is_ok() {
                let (_, vz) = adressen(v);
                zwischen += &format!("    ip saddr {z} ip daddr {vz} accept\n");
            }
        }
    }
    let regel = format!(
        "table inet {TABELLE} {{\n  chain zwischen {{\n{zwischen}    drop\n  }}\n  chain weiter {{\n    type filter hook forward priority -10; policy accept;\n{fwd}    ip saddr 10.88.0.0/16 ip daddr 10.88.0.0/16 jump zwischen\n  }}\n  chain ein {{\n    type filter hook input priority -10; policy accept;\n    iifname \"zh*\" ct state established,related accept\n    iifname \"zh*\" drop\n  }}\n  chain nat {{\n    type nat hook postrouting priority 100;\n    ip saddr 10.88.0.0/16 oifname != \"zh*\" masquerade\n  }}\n}}\n"
    );
    let _ = befehl("nft", &["delete", "table", "inet", TABELLE]);
    nft(&regel)
}

/// DNS-Server für die Zelle: die des Geräts ohne lokale (127.*), sonst Quad9
pub fn dns() -> Vec<String> {
    if let Ok(v) = std::env::var("ULTAR_ZELLEN_DNS") {
        return v.split(',').map(String::from).collect();
    }
    let mut v: Vec<String> = [
        "/run/ultar/resolv.conf",
        "/etc/resolv.conf",
        "/run/systemd/resolve/resolv.conf",
    ]
    .iter()
    .filter_map(|p| std::fs::read_to_string(p).ok())
    .flat_map(|s| {
        s.lines()
            .filter_map(|l| l.strip_prefix("nameserver").map(|x| x.trim().to_string()))
            .collect::<Vec<_>>()
    })
    .filter(|a| !a.starts_with("127.") && a != "::1")
    .collect();
    v.dedup();
    if v.is_empty() {
        v.push("9.9.9.9".into());
    }
    v
}
