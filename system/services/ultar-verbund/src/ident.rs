//! Geräte-Identität: statisches X25519-Schlüsselpaar (Noise). Kennung = „UV-“ + 8 Hex-Zeichen aus SHA-256.

use sha2::{Digest, Sha256};
use std::path::Path;

pub const NOISE: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";

#[derive(Clone)]
pub struct Ident {
    pub privat: Vec<u8>,
    pub oeffentlich: Vec<u8>,
}

impl Ident {
    /// laden oder beim ersten Start erzeugen (Datei nur für den Besitzer lesbar)
    pub fn laden(dir: &Path) -> std::io::Result<Ident> {
        let f = dir.join("schluessel");
        if let Ok(b) = std::fs::read(&f) {
            if b.len() == 64 {
                return Ok(Ident {
                    privat: b[..32].to_vec(),
                    oeffentlich: b[32..].to_vec(),
                });
            }
        }
        std::fs::create_dir_all(dir)?;
        let kp = snow::Builder::new(NOISE.parse().unwrap())
            .generate_keypair()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let mut b = kp.private.clone();
        b.extend_from_slice(&kp.public);
        std::fs::write(&f, &b)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600));
        }
        Ok(Ident {
            privat: kp.private,
            oeffentlich: kp.public,
        })
    }

    pub fn kennung(&self) -> String {
        kennung(&self.oeffentlich)
    }

    pub fn pk_hex(&self) -> String {
        hex(&self.oeffentlich)
    }
}

pub fn kennung(pk: &[u8]) -> String {
    let h = Sha256::digest(pk);
    format!("UV-{:02x}{:02x}-{:02x}{:02x}", h[0], h[1], h[2], h[3])
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}
