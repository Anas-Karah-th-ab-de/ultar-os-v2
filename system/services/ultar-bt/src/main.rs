//! ultar-bt – Bluetooth-Werkzeug für die Oberfläche und für Tests.
//!   ultar-bt info            Chip einschalten, Adresse und Version zeigen
//!   ultar-bt scan [sek]      Geräte suchen (klassisch + LE); Zeilen: ADRESSE<TAB>le|br<TAB>RSSI<TAB>KLASSE<TAB>NAME
//! Ergebnis der Suche zusätzlich in /run/ultar/bt/scan.tsv (für die Oberfläche).

use ultar_bt::*;

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty() && s != "(none)" && s != "localhost")
        .unwrap_or_else(|| "Ultar".into())
}

fn run() -> Result<(), Error> {
    let a: Vec<String> = std::env::args().collect();
    let dev = std::env::var("ULTAR_BT_DEV").unwrap_or_else(|_| H4_DEV.into());
    match a.get(1).map(|s| s.as_str()) {
        Some("info") | Some("scan") => {}
        _ => {
            println!("ultar-bt info | scan [sekunden]");
            return Ok(());
        }
    }
    let mut h = Hci::open(&dev).map_err(|e| {
        eprintln!("{dev}: {e}");
        Error::Io(e)
    })?;
    h.init(&hostname())?;
    let addr = h.bd_addr()?;
    let (ver, manu, sub) = h.version()?;
    println!("adresse\t{}", addr_str(&addr));
    println!("version\thci={ver} hersteller=0x{manu:04X} lmp_sub=0x{sub:04X}");
    if a[1] == "scan" {
        let secs = a
            .get(2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(10u64)
            .clamp(2, 60);
        let list = h.scan(secs, |_| {})?;
        let mut tsv = String::new();
        for f in &list {
            let line = format!(
                "{}\t{}\t{}\t{:06X}\t{}\n",
                addr_str(&f.addr),
                if f.le { "le" } else { "br" },
                f.rssi.map(|r| r.to_string()).unwrap_or_default(),
                f.class,
                f.name.clone().unwrap_or_default()
            );
            print!("{line}");
            tsv.push_str(&line);
        }
        println!("gefunden\t{}", list.len());
        let _ = std::fs::create_dir_all("/run/ultar/bt");
        let _ = std::fs::write("/run/ultar/bt/scan.tsv", tsv);
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("ultar-bt: {e}");
        std::process::exit(1);
    }
}
