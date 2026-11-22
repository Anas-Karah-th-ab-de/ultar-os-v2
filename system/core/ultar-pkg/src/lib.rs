//! Ultar-App-Paketformat `.uap` — packen, signieren, prüfen, installieren.
//!
//! Harte Regel: **ohne gültige Signatur keine Installation.** Jede App wird mit dem privaten
//! Schlüssel von KM Code Craft (bzw. einem freigegebenen Herausgeber) signiert; das Gerät prüft
//! gegen die eingebrannten öffentlichen Schlüssel in `/etc/ultar/trusted-keys/*.ed25519`. Schlägt
//! die Prüfung fehl oder fehlt die Signatur, wird abgelehnt — unabhängig davon, was der Server sagt.
//!
//! ## Aufbau einer `.uap` (äußeres tar, unkomprimiert)
//! ```text
//! ultar-manifest.toml      Metadaten (id, name, version, arch, kind, services, permissions …)
//! ultar-payload.tar.gz     die eigentlichen Dateien der App
//! ultar-sig.ed25519        "<keyid>\n<base64-signatur>"  über die unten definierte Nachricht
//! ```
//! Signierte Nachricht (kanonisch, keine Mehrdeutigkeit):
//! ```text
//! ultar-pkg-v1\n<sha256(manifest)>\n<sha256(payload.tar.gz)>\n
//! ```

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

pub const MAGIC: &str = "ultar-pkg-v1";
pub const M_MANIFEST: &str = "ultar-manifest.toml";
pub const M_PAYLOAD: &str = "ultar-payload.tar.gz";
pub const M_SIG: &str = "ultar-sig.ed25519";
pub const TRUSTED_DIR: &str = "/etc/ultar/trusted-keys";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default = "arch_any")]
    pub arch: String,
    /// "native" (Dateien nach /), "compat-deb" (.deb im Ubuntu-Bereich) oder "data" (nur /data/ultar/apps/<id>)
    pub kind: String,
    #[serde(default)]
    pub exec: String,
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub min_os: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub publisher: String,
}

fn arch_any() -> String {
    "aarch64".into()
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

fn sign_message(manifest: &[u8], payload: &[u8]) -> String {
    format!(
        "{MAGIC}\n{}\n{}\n",
        sha256_hex(manifest),
        sha256_hex(payload)
    )
}

// ---------- Schlüssel ----------

/// Neues Schlüsselpaar erzeugen → (privat hex, öffentlich hex). Privaten Schlüssel offline halten!
pub fn keygen() -> (String, String) {
    let sk = SigningKey::generate(&mut rand::rngs::OsRng);
    let vk = sk.verifying_key();
    (hex::encode(sk.to_bytes()), hex::encode(vk.to_bytes()))
}

/// Kurz-Kennung eines öffentlichen Schlüssels (für Zuordnung Signatur ↔ Schlüssel).
pub fn key_id(pubkey: &VerifyingKey) -> String {
    sha256_hex(&pubkey.to_bytes())[..16].to_string()
}

fn load_signing(hex_priv: &str) -> Result<SigningKey, String> {
    let b = hex::decode(hex_priv.trim()).map_err(|e| format!("Schlüssel hex: {e}"))?;
    let arr: [u8; 32] = b
        .as_slice()
        .try_into()
        .map_err(|_| "privater Schlüssel muss 32 Byte sein".to_string())?;
    Ok(SigningKey::from_bytes(&arr))
}

fn load_verifying(hex_pub: &str) -> Option<VerifyingKey> {
    let b = hex::decode(hex_pub.trim()).ok()?;
    let arr: [u8; 32] = b.as_slice().try_into().ok()?;
    VerifyingKey::from_bytes(&arr).ok()
}

// ---------- Payload (tar.gz) ----------

fn make_payload_targz(src_dir: &Path) -> Result<Vec<u8>, String> {
    let enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tb = tar::Builder::new(enc);
    tb.append_dir_all(".", src_dir)
        .map_err(|e| format!("Payload packen: {e}"))?;
    let enc = tb.into_inner().map_err(|e| e.to_string())?;
    enc.finish().map_err(|e| e.to_string())
}

fn extract_payload_targz(data: &[u8], dest: &Path) -> Result<(), String> {
    let dec = flate2::read::GzDecoder::new(data);
    let mut ar = tar::Archive::new(dec);
    ar.unpack(dest)
        .map_err(|e| format!("Payload entpacken: {e}"))
}

// ---------- Packen + signieren ----------

/// Aus Manifest + Quellordner ein signiertes `.uap` schreiben.
pub fn pack_and_sign(
    manifest_path: &Path,
    src_dir: &Path,
    out_uap: &Path,
    hex_priv: &str,
) -> Result<(), String> {
    let sk = load_signing(hex_priv)?;
    let manifest_bytes =
        std::fs::read(manifest_path).map_err(|e| format!("Manifest lesen: {e}"))?;
    // Manifest validieren
    let _m: Manifest = toml::from_str(&String::from_utf8_lossy(&manifest_bytes))
        .map_err(|e| format!("Manifest ungültig: {e}"))?;
    let payload = make_payload_targz(src_dir)?;

    let msg = sign_message(&manifest_bytes, &payload);
    let sig: Signature = sk.sign(msg.as_bytes());
    let kid = key_id(&sk.verifying_key());
    let sig_blob = format!("{kid}\n{}", B64.encode(sig.to_bytes()));

    // äußeres tar (store/unkomprimiert)
    let out = std::fs::File::create(out_uap).map_err(|e| e.to_string())?;
    let mut tb = tar::Builder::new(out);
    append_bytes(&mut tb, M_MANIFEST, &manifest_bytes)?;
    append_bytes(&mut tb, M_PAYLOAD, &payload)?;
    append_bytes(&mut tb, M_SIG, sig_blob.as_bytes())?;
    tb.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn append_bytes<W: Write>(tb: &mut tar::Builder<W>, name: &str, data: &[u8]) -> Result<(), String> {
    let mut h = tar::Header::new_gnu();
    h.set_size(data.len() as u64);
    h.set_mode(0o644);
    h.set_cksum();
    tb.append_data(&mut h, name, data)
        .map_err(|e| format!("tar {name}: {e}"))
}

// ---------- Lesen + prüfen ----------

struct Parts {
    manifest: Vec<u8>,
    payload: Vec<u8>,
    sig_blob: String,
}

fn read_parts(uap: &Path) -> Result<Parts, String> {
    let f = std::fs::File::open(uap).map_err(|e| format!("{}: {e}", uap.display()))?;
    let mut ar = tar::Archive::new(f);
    let (mut manifest, mut payload, mut sig_blob) = (None, None, None);
    for e in ar.entries().map_err(|e| e.to_string())? {
        let mut e = e.map_err(|e| e.to_string())?;
        let path = e
            .path()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .to_string();
        let mut buf = Vec::new();
        e.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        match path.as_str() {
            M_MANIFEST => manifest = Some(buf),
            M_PAYLOAD => payload = Some(buf),
            M_SIG => sig_blob = Some(String::from_utf8_lossy(&buf).to_string()),
            _ => {}
        }
    }
    Ok(Parts {
        manifest: manifest.ok_or("Manifest fehlt im Paket")?,
        payload: payload.ok_or("Payload fehlt im Paket")?,
        sig_blob: sig_blob.ok_or("Signatur fehlt im Paket")?,
    })
}

/// Vertrauenswürdige öffentliche Schlüssel laden (Dateien mit hex-Pubkey in /etc/ultar/trusted-keys/).
fn trusted_keys(dir: &Path) -> Vec<VerifyingKey> {
    let mut keys = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if let Ok(s) = std::fs::read_to_string(e.path()) {
                for line in s.lines() {
                    let line = line.trim();
                    if !line.is_empty() {
                        if let Some(k) = load_verifying(line) {
                            keys.push(k);
                        }
                    }
                }
            }
        }
    }
    keys
}

/// Paket prüfen. Gibt das Manifest nur zurück, wenn die Signatur gegen einen vertrauten Schlüssel passt.
/// Fehlt die Signatur, ist sie ungültig oder passt kein Schlüssel → Fehler (Installation verboten).
pub fn verify(uap: &Path, trusted_dir: &Path) -> Result<Manifest, String> {
    let p = read_parts(uap)?;
    let msg = sign_message(&p.manifest, &p.payload);

    // Signatur-Blob: "<keyid>\n<base64 sig>"
    let sig_b64 = p.sig_blob.lines().nth(1).unwrap_or("").trim();
    let sig_bytes = B64
        .decode(sig_b64)
        .map_err(|_| "Signatur nicht lesbar".to_string())?;
    let sig_arr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "Signaturlänge ungültig".to_string())?;
    let sig = Signature::from_bytes(&sig_arr);

    let keys = trusted_keys(trusted_dir);
    if keys.is_empty() {
        return Err("keine vertrauenswürdigen Schlüssel auf dem Gerät (/etc/ultar/trusted-keys) — Installation abgelehnt".into());
    }
    let ok = keys.iter().any(|k| k.verify(msg.as_bytes(), &sig).is_ok());
    if !ok {
        return Err(
            "Signatur ungültig oder unbekannter Herausgeber — Installation abgelehnt".into(),
        );
    }
    let m: Manifest = toml::from_str(&String::from_utf8_lossy(&p.manifest))
        .map_err(|e| format!("Manifest ungültig: {e}"))?;
    Ok(m)
}

/// Nur-Info (prüft NICHT die Signatur gegen Gerätevertrauen — nur Anzeige).
pub fn info(uap: &Path) -> Result<Manifest, String> {
    let p = read_parts(uap)?;
    toml::from_str(&String::from_utf8_lossy(&p.manifest))
        .map_err(|e| format!("Manifest ungültig: {e}"))
}

// ---------- Installieren ----------

/// Installiert ein Paket — NUR nach erfolgreicher Signaturprüfung.
pub fn install(uap: &Path, trusted_dir: &Path) -> Result<Manifest, String> {
    install_at(uap, trusted_dir, Path::new("/"))
}

/// True for package ids that are safe as a directory name: 1–128 characters of
/// `a-z A-Z 0-9 . _ -`, starting with a letter or digit, without `..`.
pub fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.first().is_some_and(|b| b.is_ascii_alphanumeric())
        && bytes.len() <= 128
        && !id.contains("..")
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Like [`install`], but with `root` in place of `/` (tests, offline images). The signature is
/// verified first; on failure nothing is written. `compat-deb` packages and the service reload
/// need the live system and are only supported with `root` = `/`.
pub fn install_at(uap: &Path, trusted_dir: &Path, root: &Path) -> Result<Manifest, String> {
    let m = verify(uap, trusted_dir)?; // harte Sperre
    if !valid_id(&m.id) {
        return Err("Paketkennung ungültig — Installation abgelehnt".into());
    }
    let live = root == Path::new("/");
    let p = read_parts(uap)?;
    let stage = root.join("data/ultar/apps").join(&m.id);

    match m.kind.as_str() {
        "native" => {
            // Dateien direkt ins Wurzeldateisystem (Payload-Pfade sind unter usr/, etc/ … gewurzelt)
            extract_payload_targz(&p.payload, root)?;
        }
        "compat-deb" => {
            if !live {
                return Err("compat-deb nur auf dem laufenden System installierbar".into());
            }
            // Payload in Staging, enthaltenes .deb im Ubuntu-Bereich installieren
            std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
            extract_payload_targz(&p.payload, &stage)?;
            let deb = find_first_deb(&stage).ok_or("kein .deb im Paket")?;
            let st = std::process::Command::new("/usr/bin/ultar-compat")
                .args(["dpkg", "-i", &deb])
                .status()
                .map_err(|e| format!("ultar-compat: {e}"))?;
            if !st.success() {
                return Err("dpkg-Installation fehlgeschlagen".into());
            }
        }
        _ => {
            std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
            extract_payload_targz(&p.payload, &stage)?;
        }
    }

    // Dienste übernehmen
    if live && !m.services.is_empty() {
        let _ = std::process::Command::new("/usr/bin/ultar-ctl")
            .arg("reload")
            .status();
    }
    // Installationsvermerk
    let installed = root.join("data/ultar/installed");
    let _ = std::fs::create_dir_all(&installed);
    let _ = std::fs::write(
        installed.join(format!("{}.toml", m.id)),
        toml::to_string(&m).unwrap_or_default(),
    );
    Ok(m)
}

fn find_first_deb(dir: &Path) -> Option<String> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.extension().map(|x| x == "deb").unwrap_or(false) {
            return Some(p.to_string_lossy().to_string());
        }
    }
    None
}
