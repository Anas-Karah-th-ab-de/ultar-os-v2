//! The publisher's side: make a manifest from payload files, sign it, rotate keys, build and
//! verify bundles. Used by the `uupdate-sign` program. Secret keys are read from files only –
//! never from the command line or the environment.

use crate::bundle::Header;
use crate::error::{Malformed, UpdateError};
use crate::fsutil::sync_parent;
use crate::keys::{read_key_file, KeySet, Rotation};
use crate::manifest::{Manifest, Payload, Role, MAX_MANIFEST};
use crate::sign::{self, public_of, PublicKey, MANIFEST_DOMAIN, MANIFEST_MAGIC};
use crate::stage::{PayloadCheck, CHUNK};
use crate::verify::{authenticate, Authentic};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// A payload file for a manifest or bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadFile {
    /// Its role.
    pub role: Role,
    /// Where it is.
    pub path: PathBuf,
    /// dm-verity root hash of the image, when it has one.
    pub verity_root: Option<[u8; 32]>,
}

/// Parses `role=path` or `role=path:verityroot` (64 hex characters).
pub fn parse_payload_arg(text: &str) -> Result<PayloadFile, UpdateError> {
    let (role, rest) = text.split_once('=').ok_or(UpdateError::InvalidArgument)?;
    let role = Role::from_name(role).ok_or(UpdateError::InvalidArgument)?;
    let (path, verity_root) = match rest.rsplit_once(':') {
        Some((path, hex)) if hex.len() == 64 => {
            let mut root = [0u8; 32];
            hex::decode_to_slice(hex, &mut root).map_err(|_| UpdateError::InvalidArgument)?;
            (path, Some(root))
        }
        _ => (rest, None),
    };
    if path.is_empty() {
        return Err(UpdateError::InvalidArgument);
    }
    Ok(PayloadFile {
        role,
        path: PathBuf::from(path),
        verity_root,
    })
}

/// Creates a new key pair from the system's random source: (secret seed, public key).
pub fn keygen() -> Result<([u8; 32], PublicKey), UpdateError> {
    let mut secret = [0u8; 32];
    getrandom::getrandom(&mut secret)
        .map_err(|e| UpdateError::Io(std::io::Error::other(e.to_string())))?;
    Ok((secret, public_of(&secret)))
}

/// Writes a key as one hex line. Refuses to overwrite; a secret key is created with mode 0600.
pub fn write_key_file(path: &Path, key: &[u8; 32], secret: bool) -> Result<(), UpdateError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if secret {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(format!("{}\n", hex::encode(key)).as_bytes())?;
    file.sync_all()?;
    Ok(sync_parent(path)?)
}

fn hash_file(path: &Path) -> Result<(u64, [u8; 32]), UpdateError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok((size, hasher.finalize().into()));
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
}

/// Fills the payload list of `manifest` from the files (sizes and SHA-256) and returns the
/// canonical manifest bytes. Everything else in `manifest` is taken as given.
pub fn build_manifest(
    mut manifest: Manifest,
    files: &[PayloadFile],
) -> Result<Vec<u8>, UpdateError> {
    let mut payloads = Vec::new();
    for file in files {
        let (size, sha256) = hash_file(&file.path)?;
        payloads.push(Payload {
            role: file.role,
            size,
            sha256,
            verity_root: file.verity_root,
        });
    }
    payloads.sort_by_key(|p| p.role);
    manifest.payloads = payloads;
    Ok(manifest.encode()?)
}

/// Signs canonical manifest bytes (refused when they are not canonical).
pub fn sign_manifest(manifest: &[u8], secret: &[u8; 32]) -> Result<Vec<u8>, UpdateError> {
    Manifest::parse(manifest)?;
    sign::seal(&MANIFEST_MAGIC, MANIFEST_DOMAIN, manifest, secret)
        .ok_or(UpdateError::Malformed(Malformed::Length))
}

/// Builds a signed rotation to the key set `public_keys`.
pub fn make_rotation(
    serial: u64,
    created: u64,
    public_keys: &[PublicKey],
    secret: &[u8; 32],
) -> Result<Vec<u8>, UpdateError> {
    let mut keys = public_keys.to_vec();
    keys.sort_unstable();
    keys.dedup();
    let rotation = Rotation {
        set: KeySet { serial, keys },
        created,
    };
    Ok(rotation.sign(secret)?)
}

/// Writes a bundle to `out` (via a temporary file) and verifies while writing that every
/// payload file is exactly what the signed manifest says. Returns the bundle size.
pub fn make_bundle(
    signed_manifest: &[u8],
    rotations: &[Vec<u8>],
    files: &[PayloadFile],
    out: &Path,
) -> Result<u64, UpdateError> {
    let envelope = sign::open_exact(&MANIFEST_MAGIC, signed_manifest, MAX_MANIFEST)?;
    let manifest = Manifest::parse(envelope.body)?;
    let header = Header::new(rotations, signed_manifest)?;
    let mut sources = Vec::new();
    for payload in &manifest.payloads {
        let file = files
            .iter()
            .find(|f| f.role == payload.role)
            .ok_or(UpdateError::PayloadSetMismatch)?;
        sources.push(File::open(&file.path)?);
    }
    if sources.len() != files.len() {
        return Err(UpdateError::PayloadSetMismatch);
    }
    let mut tmp = out.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let written = write_checked(&tmp, &header, &manifest, &mut sources);
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    let total = written?;
    fs::rename(&tmp, out)?;
    sync_parent(out)?;
    Ok(total)
}

fn write_checked(
    tmp: &Path,
    header: &Header,
    manifest: &Manifest,
    sources: &mut [File],
) -> Result<u64, UpdateError> {
    // One checker over the whole payload stream, fed by whichever file is being copied.
    let mut check = PayloadCheck::new(manifest);
    let mut file = File::create(tmp)?;
    file.write_all(&header.raw)?;
    let mut total = header.len();
    for source in sources {
        total += copy_checked(source, &mut file, &mut check)?;
    }
    if !check.complete() {
        return Err(UpdateError::Truncated);
    }
    file.sync_all()?;
    Ok(total)
}

fn copy_checked(
    from: &mut File,
    to: &mut File,
    check: &mut PayloadCheck,
) -> Result<u64, UpdateError> {
    let mut buf = vec![0u8; CHUNK];
    let mut copied = 0u64;
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            return Ok(copied);
        }
        check.feed(&buf[..n])?;
        to.write_all(&buf[..n])?;
        copied += n as u64;
    }
}

/// Verifies a bundle file completely: header shape, signature against `keys`, every payload
/// hash, and that the file ends exactly after the last payload. Rotations in the bundle are
/// **not** applied (`keys` must already contain the signing key).
pub fn verify_bundle(path: &Path, keys: &[PublicKey]) -> Result<Authentic, UpdateError> {
    let mut file = File::open(path)?;
    let header = Header::read(&mut file)?;
    let authentic = authenticate(&header.signed_manifest, keys)?;
    let mut check = PayloadCheck::new(authentic.manifest());
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        check.feed(&buf[..n])?;
    }
    if !check.complete() {
        return Err(UpdateError::Truncated);
    }
    Ok(authentic)
}

/// Reads the public keys of all `paths` (each a key file).
pub fn read_public_keys(paths: &[&str]) -> Result<Vec<PublicKey>, UpdateError> {
    let mut keys = Vec::new();
    for path in paths {
        keys.extend(read_key_file(Path::new(path))?);
    }
    Ok(keys)
}

#[cfg(test)]
#[path = "signtool_tests.rs"]
mod tests;
