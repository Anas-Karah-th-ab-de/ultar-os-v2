//! The account file `users` in the service's directory and the pictures in
//! `pictures/`. No secret is stored here: passwords live only in the
//! authentication service. Writes are atomic (temporary file, `fsync`,
//! rename, directory `fsync`); a file whose check line does not match, or
//! with any line that does not fit, is *damaged*: the service then refuses
//! every change until an administrator repairs it (fail closed).

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use ultar_bus::messages::account_local::{
    PictureInfo, UserKind, MAX_LOCAL_USERS, MAX_PICTURE_BYTES,
};

/// File name of the account file.
pub const FILE: &str = "users";
/// Directory of the pictures.
pub const PICTURES: &str = "pictures";
/// Largest account file accepted (bytes).
pub const MAX_FILE: u64 = 64 * 1024;
const HEADER: &str = "#ultar-accounts crc32=";

/// One local account as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserRecord {
    /// Account name.
    pub name: String,
    /// Kernel user id.
    pub uid: u32,
    /// Display name.
    pub display: String,
    /// Administrator or standard.
    pub kind: UserKind,
    /// Picture.
    pub picture: PictureInfo,
    /// Signs in by itself.
    pub auto_login: bool,
}

/// Everything in the account file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stored {
    /// The accounts.
    pub users: Vec<UserRecord>,
    /// Ids of removed accounts whose files were kept (never given again).
    pub retired: BTreeSet<u32>,
    /// Number of the last own picture.
    pub pictures: u32,
}

/// Why reading or writing failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    /// The file exists but cannot be trusted.
    Damaged,
    /// The disk refused (read or write).
    Io,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            StoreError::Damaged => "account file damaged",
            StoreError::Io => "account file could not be read or written",
        })
    }
}

impl std::error::Error for StoreError {}

/// CRC-32 (IEEE) of the file body: detects a cut or changed file.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

fn hex(text: &str) -> String {
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<String> {
    if !text.len().is_multiple_of(2) || text.len() > 512 {
        return None;
    }
    let bytes = (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
}

fn picture_word(p: PictureInfo) -> String {
    match p {
        PictureInfo::Initials => "initials".into(),
        PictureInfo::Builtin(n) => format!("builtin:{n}"),
        PictureInfo::Custom(v) => format!("custom:{v}"),
    }
}

fn picture_of(word: &str) -> Option<PictureInfo> {
    if word == "initials" {
        return Some(PictureInfo::Initials);
    }
    if let Some(n) = word.strip_prefix("builtin:") {
        return n.parse().ok().map(PictureInfo::Builtin);
    }
    word.strip_prefix("custom:")?
        .parse()
        .ok()
        .map(PictureInfo::Custom)
}

/// The file body.
pub fn encode(s: &Stored) -> String {
    let mut out = String::new();
    for u in &s.users {
        let kind = match u.kind {
            UserKind::Administrator => "admin",
            UserKind::Standard => "standard",
        };
        out.push_str(&format!(
            "user {} uid={} kind={kind} display={} picture={} auto={}\n",
            u.name,
            u.uid,
            hex(&u.display),
            picture_word(u.picture),
            u8::from(u.auto_login)
        ));
    }
    for uid in &s.retired {
        out.push_str(&format!("retired {uid}\n"));
    }
    out.push_str(&format!("pictures {}\n", s.pictures));
    out
}

fn field<'a>(part: Option<&'a str>, key: &str) -> Option<&'a str> {
    part?.strip_prefix(key)?.strip_prefix('=')
}

fn decode_user(rest: &str) -> Option<UserRecord> {
    let mut parts = rest.split(' ');
    let name = parts.next()?.to_owned();
    let uid = field(parts.next(), "uid")?.parse().ok()?;
    let kind = match field(parts.next(), "kind")? {
        "admin" => UserKind::Administrator,
        "standard" => UserKind::Standard,
        _ => return None,
    };
    let display = unhex(field(parts.next(), "display")?)?;
    let picture = picture_of(field(parts.next(), "picture")?)?;
    let auto_login = match field(parts.next(), "auto")? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    let ok = parts.next().is_none()
        && crate::names::valid_name(&name)
        && crate::names::valid_display(&display);
    ok.then_some(UserRecord {
        name,
        uid,
        display,
        kind,
        picture,
        auto_login,
    })
}

/// Reads a body; anything unexpected (also a name or id twice, two
/// automatic sign-ins, too many accounts) is damage.
pub fn decode(body: &str) -> Result<Stored, StoreError> {
    let mut s = Stored::default();
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("user ") {
            let u = decode_user(rest).ok_or(StoreError::Damaged)?;
            let twice = s.users.iter().any(|o| o.name == u.name || o.uid == u.uid);
            if twice || s.users.len() >= MAX_LOCAL_USERS {
                return Err(StoreError::Damaged);
            }
            s.users.push(u);
        } else if let Some(uid) = line.strip_prefix("retired ") {
            s.retired
                .insert(uid.parse().map_err(|_| StoreError::Damaged)?);
        } else if let Some(n) = line.strip_prefix("pictures ") {
            s.pictures = n.parse().map_err(|_| StoreError::Damaged)?;
        } else {
            return Err(StoreError::Damaged);
        }
    }
    if s.users.iter().filter(|u| u.auto_login).count() > 1 {
        return Err(StoreError::Damaged);
    }
    Ok(s)
}

/// Loads the account file (nothing stored yet: empty).
pub fn load(dir: &Path) -> Result<Stored, StoreError> {
    let path = dir.join(FILE);
    let file = match File::open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Stored::default()),
        Err(_) => return Err(StoreError::Io),
    };
    let mut text = String::new();
    file.take(MAX_FILE + 1)
        .read_to_string(&mut text)
        .map_err(|_| StoreError::Damaged)?;
    if text.len() as u64 > MAX_FILE {
        return Err(StoreError::Damaged);
    }
    let (head, body) = text.split_once('\n').ok_or(StoreError::Damaged)?;
    let crc = head.strip_prefix(HEADER).ok_or(StoreError::Damaged)?;
    let crc = u32::from_str_radix(crc, 16).map_err(|_| StoreError::Damaged)?;
    if crc != crc32(body.as_bytes()) {
        return Err(StoreError::Damaged);
    }
    decode(body)
}

/// Writes `data` to `path` atomically (mode 0600).
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), StoreError> {
    let dir = path.parent().ok_or(StoreError::Io)?;
    let name = path.file_name().ok_or(StoreError::Io)?.to_string_lossy();
    let tmp: PathBuf = dir.join(format!(".{name}.tmp"));
    let io = |_| StoreError::Io;
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(io)?;
    f.write_all(data).map_err(io)?;
    f.sync_all().map_err(io)?;
    drop(f);
    fs::rename(&tmp, path).map_err(io)?;
    File::open(dir).and_then(|d| d.sync_all()).map_err(io)
}

/// Stores the account file atomically.
pub fn save(dir: &Path, s: &Stored) -> Result<(), StoreError> {
    let body = encode(s);
    let text = format!("{HEADER}{:08x}\n{body}", crc32(body.as_bytes()));
    write_atomic(&dir.join(FILE), text.as_bytes())
}

/// Path of a user's own picture.
pub fn picture_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(PICTURES).join(name)
}

/// Stores a user's own picture.
pub fn save_picture(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), StoreError> {
    let pictures = dir.join(PICTURES);
    fs::create_dir_all(&pictures).map_err(|_| StoreError::Io)?;
    write_atomic(&picture_path(dir, name), bytes)
}

/// Reads a user's own picture (bounded).
pub fn load_picture(dir: &Path, name: &str) -> Option<Vec<u8>> {
    let f = File::open(picture_path(dir, name)).ok()?;
    let mut bytes = Vec::new();
    f.take(MAX_PICTURE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= MAX_PICTURE_BYTES).then_some(bytes)
}

/// Removes a user's own picture (absent is fine).
pub fn remove_picture(dir: &Path, name: &str) {
    let _ = fs::remove_file(picture_path(dir, name));
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
