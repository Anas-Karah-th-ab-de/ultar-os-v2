//! Client side, used by the publish scripts: compute digests, ask the service, write files.
//!
//! Every output file is written atomically (temporary file, `fsync`, rename).

use crate::error::SignError;
use crate::keys::Role;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use sha2::{Digest, Sha256, Sha512};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

/// Largest manifest read (`ultar-manifest.toml`).
pub const MAX_MANIFEST: u64 = 64 * 1024;
/// Largest payload read into memory for packing.
pub const MAX_PAYLOAD: u64 = 1024 * 1024 * 1024;
/// Largest catalogue body.
pub const MAX_CATALOG: u64 = 4 * 1024 * 1024;
/// Largest answer line read from the service.
const MAX_ANSWER: u64 = 8 * 1024;

/// Sends one request line and returns the text after `ok `.
pub fn request(socket: &Path, line: &str) -> Result<String, SignError> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    stream.write_all(format!("{line}\n").as_bytes())?;
    let mut answer = String::new();
    stream.take(MAX_ANSWER).read_to_string(&mut answer)?;
    let answer = answer.trim_end_matches('\n');
    if let Some(rest) = answer.strip_prefix("ok ") {
        return Ok(rest.to_string());
    }
    match answer.strip_prefix("err ") {
        Some(code) => Err(SignError::Refused(code.to_string())),
        None => Err(SignError::Protocol),
    }
}

/// Splits `"<key-id> <base64>"`.
fn split_signature(answer: &str) -> Result<(String, Vec<u8>), SignError> {
    let (kid, b64) = answer.split_once(' ').ok_or(SignError::Protocol)?;
    let bytes = B64.decode(b64).map_err(|_| SignError::Protocol)?;
    Ok((kid.to_string(), bytes))
}

/// Reads a whole file of at most `max` bytes.
pub fn read_capped(path: &Path, max: u64) -> Result<Vec<u8>, SignError> {
    let mut out = Vec::new();
    std::fs::File::open(path)?
        .take(max + 1)
        .read_to_end(&mut out)?;
    if out.len() as u64 > max {
        return Err(SignError::BadPackage);
    }
    Ok(out)
}

/// Writes `data` to `path` atomically.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), SignError> {
    let mut tmp: PathBuf = path.to_path_buf();
    tmp.set_extension("part");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn append(tb: &mut tar::Builder<Vec<u8>>, name: &str, data: &[u8]) -> Result<(), SignError> {
    let mut h = tar::Header::new_gnu();
    h.set_size(data.len() as u64);
    h.set_mode(0o644);
    h.set_mtime(0);
    h.set_uid(0);
    h.set_gid(0);
    h.set_cksum();
    tb.append_data(&mut h, name, data)?;
    Ok(())
}

/// Builds the `.uap` (outer tar: manifest, payload, signature – the `ultar-pkg-v1` layout).
pub fn build_uap(manifest: &[u8], payload: &[u8], sig_blob: &str) -> Result<Vec<u8>, SignError> {
    let mut tb = tar::Builder::new(Vec::new());
    append(&mut tb, "ultar-manifest.toml", manifest)?;
    append(&mut tb, "ultar-payload.tar.gz", payload)?;
    append(&mut tb, "ultar-sig.ed25519", sig_blob.as_bytes())?;
    Ok(tb.into_inner()?)
}

/// Signs a package (`role` = system or store) and writes the `.uap` to `out`.
/// Answers the key id that signed it.
pub fn sign_uap(
    socket: &Path,
    role: Role,
    manifest_path: &Path,
    payload_path: &Path,
    out: &Path,
) -> Result<String, SignError> {
    let op = match role {
        Role::System => "pkg-system",
        Role::Store => "pkg-store",
        Role::Os => return Err(SignError::BadRequest),
    };
    let manifest = read_capped(manifest_path, MAX_MANIFEST)?;
    let payload = read_capped(payload_path, MAX_PAYLOAD)?;
    if manifest.is_empty() || payload.is_empty() {
        return Err(SignError::BadPackage);
    }
    let answer = request(
        socket,
        &format!("{op} {} {}", sha256_hex(&manifest), sha256_hex(&payload)),
    )?;
    let (kid, sig) = split_signature(&answer)?;
    if sig.len() != 64 {
        return Err(SignError::Protocol);
    }
    let blob = format!("{kid}\n{}", B64.encode(&sig));
    write_atomic(out, &build_uap(&manifest, &payload, &blob)?)?;
    Ok(kid)
}

/// Signs a catalogue body; writes `<file>.sig` = `"<key-id>\n<base64>"`.
pub fn sign_catalog(socket: &Path, file: &Path) -> Result<PathBuf, SignError> {
    let body = read_capped(file, MAX_CATALOG)?;
    let answer = request(socket, &format!("catalog {}", sha256_hex(&body)))?;
    let (kid, sig) = split_signature(&answer)?;
    if sig.len() != 64 {
        return Err(SignError::Protocol);
    }
    let out = sig_path(file);
    write_atomic(&out, format!("{kid}\n{}\n", B64.encode(&sig)).as_bytes())?;
    Ok(out)
}

/// Signs a release file (streamed SHA-512, any size); writes `<file>.sig` (SSHSIG armor).
pub fn sign_release(socket: &Path, file: &Path) -> Result<PathBuf, SignError> {
    let mut f = std::fs::File::open(file)?;
    let mut h = Sha512::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let answer = request(socket, &format!("release {}", hex::encode(h.finalize())))?;
    let (_kid, armored) = split_signature(&answer)?;
    if !armored.starts_with(b"-----BEGIN SSH SIGNATURE-----\n") {
        return Err(SignError::Protocol);
    }
    let out = sig_path(file);
    write_atomic(&out, &armored)?;
    Ok(out)
}

/// `<file>.sig`.
pub fn sig_path(file: &Path) -> PathBuf {
    let mut s = file.as_os_str().to_os_string();
    s.push(".sig");
    PathBuf::from(s)
}

/// The hex public key of a role, as the device's trusted-keys files hold it.
pub fn public_key(socket: &Path, role: Role) -> Result<String, SignError> {
    let hex_key = request(socket, &format!("pubkey {}", role.name()))?;
    if hex_key.len() != 64 || hex::decode(&hex_key).is_err() {
        return Err(SignError::Protocol);
    }
    Ok(hex_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uap_layout_has_three_members_in_order() {
        let bytes = build_uap(b"id = \"x\"\n", b"payload", "kid\nsig").unwrap();
        let mut ar = tar::Archive::new(bytes.as_slice());
        let names: Vec<String> = ar
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().display().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "ultar-manifest.toml",
                "ultar-payload.tar.gz",
                "ultar-sig.ed25519"
            ]
        );
    }

    #[test]
    fn helpers_refuse_bad_input() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("big");
        std::fs::write(&f, vec![0u8; 20]).unwrap();
        assert_eq!(read_capped(&f, 10), Err(SignError::BadPackage));
        assert_eq!(read_capped(&f, 20).unwrap().len(), 20);
        assert_eq!(split_signature("nospace"), Err(SignError::Protocol));
        assert_eq!(split_signature("k !!!"), Err(SignError::Protocol));
        assert_eq!(
            sig_path(Path::new("/a/b.img")),
            PathBuf::from("/a/b.img.sig")
        );
        assert_eq!(
            request(&dir.path().join("none.sock"), "pubkey os"),
            Err(SignError::Io)
        );
        assert_eq!(
            sign_uap(&dir.path().join("s"), Role::Os, &f, &f, &f),
            Err(SignError::BadRequest)
        );
    }
}
