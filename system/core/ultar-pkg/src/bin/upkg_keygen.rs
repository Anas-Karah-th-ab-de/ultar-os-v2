//! upkg-keygen — Herausgeber-Schlüsselpaar für den Ultar-Store erzeugen.
//! Ausgabe: privater Schlüssel (GEHEIM, offline halten) + öffentlicher Schlüssel (ins Gerät-Image).
fn main() {
    let (priv_hex, pub_hex) = ultar_pkg::keygen();
    eprintln!("# PRIVATER Schlüssel — geheim halten, nur zum Signieren (upkg-sign):");
    println!("{priv_hex}");
    eprintln!("# ÖFFENTLICHER Schlüssel — nach /etc/ultar/trusted-keys/<name>.ed25519 ins Image:");
    eprintln!("{pub_hex}");
}
