//! ultar-ril – Werkzeug zum Prüfen des Modem-IPC-Transports am Gerät.
//!
//!   ultar-ril --probe [sekunden]   Öffnet FMT/RFS/Router, liest rohe IPC-Frames (Timeout, blockiert nie),
//!                                   gibt sie als Hexdump aus. Nach CP-ONLINE sieht man, was der CP sendet.
//!   ultar-ril --send <hex>         Sendet rohe Hex-Nutzdaten auf FMT_0 (/dev/umts_ipc0) und wartet kurz auf Antwort.
//!   ultar-ril --nodes              Zeigt die Kanal->Knoten-Zuordnung.
//!   ultar-ril --daemon             Funkdienst: schreibt /run/ultar/radio/state für die Oberfläche.

mod daemon;

use ultar_ril::link::{ModemLink, UmtsLink, CH_FMT_0, CH_RFS_0, CH_ROUTER};

fn hexdump(label: &str, data: &[u8]) {
    print!("{label} ({} B):", data.len());
    for (i, b) in data.iter().enumerate() {
        if i % 16 == 0 {
            print!("\n  {:04x}: ", i);
        }
        print!("{:02x} ", b);
    }
    println!();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("--probe");

    match mode {
        "--daemon" => daemon::run(match args.get(2).map(String::as_str) {
            Some("--bus") => args.get(3).map(std::path::PathBuf::from),
            _ => None,
        }),
        "--nodes" => {
            println!("Kanal -> Knoten:");
            println!("  FMT_0  235 -> /dev/umts_ipc0  (RIL: SIM/Netz)");
            println!("  FMT_1  236 -> /dev/umts_ipc1");
            println!("  RFS_0  245 -> /dev/umts_rfs0");
            println!("  ROUTER  25 -> /dev/umts_router");
            println!("  PDP  10..24 -> rmnet0..7 (Netzgeräte, kein Zeichenknoten)");
        }
        "--send" => {
            let hex = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let payload = parse_hex(hex);
            if payload.is_empty() {
                eprintln!("Nutzdaten (hex) angeben, z.B. --send 01020304");
                std::process::exit(2);
            }
            let mut link = match UmtsLink::open() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("umts_ipc0 öffnen fehlgeschlagen: {e} (CP online?)");
                    std::process::exit(1);
                }
            };
            if let Err(e) = link.send(CH_FMT_0, &payload) {
                eprintln!("Senden fehlgeschlagen: {e}");
                std::process::exit(1);
            }
            hexdump("gesendet FMT_0", &payload);
            // kurz auf Antwort warten
            for _ in 0..10 {
                match link.recv(300) {
                    Ok(Some(f)) => hexdump(&format!("empfangen ch{}", f.channel), &f.payload),
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("recv: {e}");
                        break;
                    }
                }
            }
        }
        _ => {
            // --probe
            let secs: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
            let mut link = UmtsLink::empty();
            let mut opened = Vec::new();
            for ch in [CH_FMT_0, CH_RFS_0, CH_ROUTER] {
                match link.ensure(ch) {
                    Ok(()) => opened.push(ch),
                    Err(e) => eprintln!("Kanal {ch} nicht offen: {e}"),
                }
            }
            if opened.is_empty() {
                eprintln!("Kein IPC-Knoten offen – CP vermutlich nicht gebootet/ONLINE.");
                std::process::exit(1);
            }
            eprintln!("Lausche {secs}s auf Kanäle {opened:?} …");
            let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
            let mut count = 0;
            while std::time::Instant::now() < end {
                match link.recv(300) {
                    Ok(Some(f)) => {
                        hexdump(&format!("ch{}", f.channel), &f.payload);
                        count += 1;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("recv: {e}");
                        break;
                    }
                }
            }
            eprintln!("{count} Frame(s) empfangen.");
        }
    }
}

fn parse_hex(s: &str) -> Vec<u8> {
    let clean: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    (0..clean.len() / 2)
        .filter_map(|i| u8::from_str_radix(&clean[2 * i..2 * i + 2], 16).ok())
        .collect()
}
