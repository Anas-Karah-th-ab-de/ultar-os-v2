//! upkg — Ultar-Paket auf dem Gerät prüfen/installieren (über Tür oder Shell).
//! Aufruf: upkg info <datei.uap> | verify <datei.uap> | install <datei.uap>
use std::path::Path;
use ultar_pkg::TRUSTED_DIR;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let cmd = a.get(1).map(|s| s.as_str()).unwrap_or("");
    let file = a.get(2).map(|s| s.as_str()).unwrap_or("");
    if file.is_empty() {
        eprintln!(
            "{}",
            ultar_sprache::tr("Aufruf: upkg info|verify|install <datei.uap>")
        );
        std::process::exit(2);
    }
    let trusted_s = std::env::var("ULTAR_TRUSTED").unwrap_or_else(|_| TRUSTED_DIR.to_string());
    let trusted = Path::new(&trusted_s);
    match cmd {
        "info" => match ultar_pkg::info(Path::new(file)) {
            Ok(m) => println!(
                "{} {} v{} ({}, {})",
                m.id, m.name, m.version, m.arch, m.kind
            ),
            Err(e) => fail(e),
        },
        "verify" => match ultar_pkg::verify(Path::new(file), trusted) {
            Ok(m) => println!("OK signiert: {} v{}", m.id, m.version),
            Err(e) => fail(e),
        },
        "install" => match ultar_pkg::install(Path::new(file), trusted) {
            Ok(m) => println!("OK installiert: {} v{}", m.id, m.version),
            Err(e) => fail(e),
        },
        _ => {
            eprintln!("unbekannt: {cmd}");
            std::process::exit(2);
        }
    }
}

fn fail(e: String) -> ! {
    eprintln!("ABGELEHNT: {e}");
    std::process::exit(1);
}
