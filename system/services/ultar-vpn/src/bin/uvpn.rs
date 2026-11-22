//! uvpn — VPN-Steuerung über Tür/Shell.
//! Aufrufe: genkeys | up (liest /run/ultar/wg-peer.conf) | down | status
//!
//! /run/ultar/wg-peer.conf (von ultar-cloud nach der Provisionierung geschrieben):
//!   private_key=..\nserver_pubkey=..\nendpoint=host:port\naddress=10.80.x.y/32\ndns=10.80.0.1\nallowed_ips=0.0.0.0/0, ::/0
use ultar_vpn::{self as vpn, WgConfig};

fn load_cfg() -> Option<WgConfig> {
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
    if c.private_key.is_empty() || c.server_pubkey.is_empty() { None } else { Some(c) }
}

fn main() {
    let cmd = std::env::args().nth(1).unwrap_or_else(|| "status".into());
    match cmd.as_str() {
        "genkeys" => match vpn::genkeys() {
            Ok((p, pb)) => {
                println!("private={p}");
                println!("public={pb}");
            }
            Err(e) => {
                eprintln!("FEHLER: {e}");
                std::process::exit(1);
            }
        },
        "up" => match load_cfg() {
            Some(cfg) => match vpn::connect(&cfg) {
                Ok(()) => println!("OK verbunden"),
                Err(e) => {
                    eprintln!("FEHLER: {e}");
                    std::process::exit(1);
                }
            },
            None => {
                eprintln!("keine Peer-Konfiguration (erst Konto/VPN einrichten)");
                std::process::exit(2);
            }
        },
        "down" => {
            let _ = vpn::disconnect();
            println!("OK getrennt");
        }
        "status" => {
            let s = vpn::status();
            println!("up={}", s.up);
            println!("address={}", s.address);
            println!("endpoint={}", s.endpoint);
            println!("rx={} tx={}", s.rx, s.tx);
            println!("last_handshake={}", s.last_handshake);
        }
        _ => {
            eprintln!("unbekannt: {cmd}");
            std::process::exit(2);
        }
    }
}
