//! upkg-sign — ein Ultar-App-Paket (.uap) packen und signieren (auf Server/Entwicklerrechner).
//! Aufruf: upkg-sign <manifest.toml> <quellordner> <ausgabe.uap> <privkey.hex-datei>
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 5 {
        eprintln!("Aufruf: upkg-sign <manifest.toml> <quellordner> <ausgabe.uap> <privkey-datei>");
        std::process::exit(2);
    }
    let priv_hex = std::fs::read_to_string(&a[4]).unwrap_or_default();
    match ultar_pkg::pack_and_sign(Path::new(&a[1]), Path::new(&a[2]), Path::new(&a[3]), priv_hex.trim()) {
        Ok(()) => println!("OK signiert: {}", a[3]),
        Err(e) => {
            eprintln!("FEHLER: {e}");
            std::process::exit(1);
        }
    }
}
