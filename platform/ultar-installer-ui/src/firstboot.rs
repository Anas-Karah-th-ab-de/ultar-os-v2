//! First start of an installed system: the installer's answers record (`UIAN` in
//! `ULTAR-BOOTCTL`, [`crate::engine::answers`]) becomes the first account, then the record is
//! wiped so it is applied exactly once.
//!
//! The account goes into the account file of the authentication service (`ultar-authd`,
//! directory `/data/ultar/auth`, file `users`): first line `#ultar-auth crc32=<8 hex>` over
//! the rest, then `user <name> admin=1 display=<hex utf-8> password=<phc> pin=-`. The installer
//! stored only the Argon2id hash of the password, never the password.
//!
//! Rules: the record is hostile input (length, checksum and every value checked; nothing is
//! guessed); an existing account file is never overwritten (a start after an interruption
//! only wipes the record); the file is written atomically (temp file, fsync, rename,
//! directory fsync) before the record is wiped.

use crate::engine::answers::{Answers, AnswersError, MAX_BYTES, OFFSET};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use ultar_slots::crc32;

/// Header word of the account file (`ultar-authd`).
pub const KIND: &str = "ultar-auth";
/// Name of the account file in its directory.
pub const FILE: &str = "users";
/// Longest user name (`ultar-authd`).
pub const MAX_USER: usize = 32;
/// Longest shown name in bytes (`ultar-authd`).
pub const MAX_DISPLAY: usize = 128;
/// Longest password hash accepted from the record.
pub const MAX_HASH: usize = 256;
/// Largest account file read by `--list`.
pub const MAX_FILE: u64 = 256 * 1024;

/// What the first start did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No record (none written or already applied): nothing to do.
    Nothing,
    /// The account was created and the record wiped.
    Created(String),
    /// An account file existed already: it stays, the record was wiped.
    Kept,
}

/// Why the first start could not apply the record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirstBootError {
    /// Reading or writing failed (operation, error kind).
    Io(&'static str, ErrorKind),
    /// The record is damaged (it stays for an administrator to look at).
    Record(AnswersError),
    /// A value of the record is not acceptable (key).
    Value(&'static str),
}

impl std::fmt::Display for FirstBootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FirstBootError::Io(op, kind) => write!(f, "{op}: {kind:?}"),
            FirstBootError::Record(e) => write!(f, "answers record damaged: {e:?}"),
            FirstBootError::Value(key) => write!(f, "answers record: value of {key} refused"),
        }
    }
}

impl std::error::Error for FirstBootError {}

fn io(op: &'static str) -> impl Fn(std::io::Error) -> FirstBootError {
    move |e| FirstBootError::Io(op, e.kind())
}

/// A user name as `ultar-authd` accepts it: `a-z` first, then `a-z 0-9 - _ .`, 1–32 bytes.
pub fn valid_user(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && name.len() <= MAX_USER
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

fn valid_display(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= MAX_DISPLAY && !name.chars().any(char::is_control)
}

/// An Argon2id PHC string without spaces or control characters.
fn valid_hash(hash: &str) -> bool {
    hash.starts_with("$argon2id$")
        && hash.len() <= MAX_HASH
        && hash.bytes().all(|b| b.is_ascii_graphic())
}

/// The account line of the record's answers.
pub fn account_line(a: &Answers) -> Result<(String, String), FirstBootError> {
    let user = a.get("user").ok_or(FirstBootError::Value("user"))?;
    if !valid_user(user) {
        return Err(FirstBootError::Value("user"));
    }
    let full = a
        .get("full_name")
        .filter(|n| !n.trim().is_empty())
        .unwrap_or(user);
    if !valid_display(full) {
        return Err(FirstBootError::Value("full_name"));
    }
    let hash = a
        .get("password_hash")
        .ok_or(FirstBootError::Value("password_hash"))?;
    if !valid_hash(hash) {
        return Err(FirstBootError::Value("password_hash"));
    }
    let display: String = full.bytes().map(|b| format!("{b:02x}")).collect();
    Ok((
        user.to_owned(),
        format!("user {user} admin=1 display={display} password={hash} pin=-\n"),
    ))
}

/// `O_NOFOLLOW` (the generic Linux value on x86_64; aarch64 has its own).
const fn libc_o_nofollow() -> i32 {
    if cfg!(target_arch = "aarch64") {
        0o100_000
    } else {
        0o400_000
    }
}

/// The account file's content for `body`.
pub fn account_file(body: &str) -> String {
    format!("#{KIND} crc32={:08x}\n{body}", crc32(body.as_bytes()))
}

/// Writes `content` to `dir/FILE` atomically (mode 0600), owned by `owner` when given.
///
/// `dir` belongs to the account service, so whatever lies at the temporary name is removed
/// and the file is made new without following a link, and ownership is changed on the open
/// descriptor: a link planted there can never get a root file chowned (review S-10).
fn write_atomic(dir: &Path, content: &str, owner: Option<u32>) -> Result<(), FirstBootError> {
    let temp = dir.join(format!(".{FILE}.tmp"));
    match fs::remove_file(&temp) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io("remove")(e)),
        _ => {}
    }
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc_o_nofollow())
        .mode(0o600)
        .open(&temp)
        .map_err(io("create"))?;
    f.write_all(content.as_bytes()).map_err(io("write"))?;
    if let Some(uid) = owner {
        std::os::unix::fs::fchown(&f, Some(uid), Some(uid)).map_err(io("chown"))?;
    }
    f.sync_all().map_err(io("fsync"))?;
    drop(f);
    fs::rename(&temp, dir.join(FILE)).map_err(io("rename"))?;
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(io("fsync dir"))
}

/// Applies the record on `bootctl` (a file or the raw partition node): the account goes to
/// `auth_dir`, then the record is wiped.
pub fn apply(
    bootctl: &Path,
    auth_dir: &Path,
    owner: Option<u32>,
) -> Result<Outcome, FirstBootError> {
    let mut dev = OpenOptions::new()
        .read(true)
        .write(true)
        .open(bootctl)
        .map_err(io("open"))?;
    let mut raw = vec![0u8; MAX_BYTES];
    dev.seek(SeekFrom::Start(OFFSET as u64))
        .map_err(io("seek"))?;
    dev.read_exact(&mut raw).map_err(io("read"))?;
    let answers = match Answers::decode(&raw) {
        Ok(a) => a,
        Err(AnswersError::Magic) => return Ok(Outcome::Nothing),
        Err(e) => return Err(FirstBootError::Record(e)),
    };
    let outcome = if auth_dir.join(FILE).exists() {
        Outcome::Kept
    } else {
        let (user, line) = account_line(&answers)?;
        fs::create_dir_all(auth_dir).map_err(io("mkdir"))?;
        write_atomic(auth_dir, &account_file(&line), owner)?;
        Outcome::Created(user)
    };
    dev.seek(SeekFrom::Start(OFFSET as u64))
        .map_err(io("seek"))?;
    dev.write_all(&vec![0u8; MAX_BYTES]).map_err(io("wipe"))?;
    dev.sync_all().map_err(io("fsync"))?;
    Ok(outcome)
}

/// The user names of the account file in `auth_dir` (header and checksum checked).
pub fn accounts(auth_dir: &Path) -> Result<Vec<String>, FirstBootError> {
    let mut raw = Vec::new();
    File::open(auth_dir.join(FILE))
        .map_err(io("open"))?
        .take(MAX_FILE + 1)
        .read_to_end(&mut raw)
        .map_err(io("read"))?;
    let damaged = FirstBootError::Value("account file");
    let text = String::from_utf8(raw).map_err(|_| damaged.clone())?;
    let (head, body) = text.split_once('\n').ok_or(damaged.clone())?;
    let want = head
        .strip_prefix('#')
        .and_then(|h| h.strip_prefix(KIND))
        .and_then(|h| h.strip_prefix(" crc32="))
        .and_then(|h| u32::from_str_radix(h, 16).ok())
        .ok_or(damaged.clone())?;
    if crc32(body.as_bytes()) != want {
        return Err(damaged);
    }
    Ok(body
        .lines()
        .filter_map(|l| {
            l.strip_prefix("user ")?
                .split(' ')
                .next()
                .map(str::to_owned)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pairs: &[(&str, &str)]) -> Vec<u8> {
        let mut a = Answers::default();
        for (k, v) in pairs {
            a.set(k, v);
        }
        let mut img = vec![0u8; OFFSET + MAX_BYTES];
        let r = a.encode();
        img[OFFSET..OFFSET + r.len()].copy_from_slice(&r);
        img
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ultar-firstboot-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A shown name beyond the limit (the record itself drops control characters).
    const LONG: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaA";

    #[test]
    fn the_record_becomes_one_account_once() {
        let d = scratch("once");
        let ctl = d.join("bootctl");
        fs::write(
            &ctl,
            record(&[
                ("full_name", "محمد أنس"),
                ("user", "anas"),
                ("password_hash", HASH),
            ]),
        )
        .unwrap();
        let auth = d.join("auth");
        assert_eq!(
            apply(&ctl, &auth, None).unwrap(),
            Outcome::Created("anas".into())
        );
        assert_eq!(accounts(&auth).unwrap(), vec!["anas".to_owned()]);
        let text = fs::read_to_string(auth.join(FILE)).unwrap();
        assert!(text.contains(&format!("password={HASH} pin=-")) && text.contains("admin=1"));
        assert!(
            fs::read(&ctl).unwrap()[OFFSET..].iter().all(|b| *b == 0),
            "record wiped"
        );
        assert_eq!(apply(&ctl, &auth, None).unwrap(), Outcome::Nothing);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn an_existing_account_file_is_kept_and_bad_records_change_nothing() {
        let d = scratch("kept");
        let ctl = d.join("bootctl");
        let auth = d.join("auth");
        fs::create_dir_all(&auth).unwrap();
        fs::write(
            auth.join(FILE),
            account_file("user old admin=1 display=6f password=x pin=-\n"),
        )
        .unwrap();
        fs::write(&ctl, record(&[("user", "anas"), ("password_hash", HASH)])).unwrap();
        assert_eq!(apply(&ctl, &auth, None).unwrap(), Outcome::Kept);
        assert_eq!(accounts(&auth).unwrap(), vec!["old".to_owned()]);
        let auth2 = d.join("auth2");
        for bad in [
            vec![("user", "Anas"), ("password_hash", HASH)],
            vec![("user", "a b"), ("password_hash", HASH)],
            vec![("user", "anas"), ("password_hash", "plain-password")],
            vec![("user", "anas"), ("password_hash", "$argon2id$x y")],
            vec![("user", "anas")],
            vec![
                ("user", "anas"),
                ("full_name", LONG),
                ("password_hash", HASH),
            ],
        ] {
            fs::write(&ctl, record(&bad)).unwrap();
            assert!(
                matches!(apply(&ctl, &auth2, None), Err(FirstBootError::Value(_))),
                "{bad:?}"
            );
            assert!(!auth2.join(FILE).exists());
            assert!(
                fs::read(&ctl).unwrap()[OFFSET..OFFSET + 4] == *b"UIAN",
                "a refused record stays"
            );
        }
        let mut img = record(&[("user", "anas"), ("password_hash", HASH)]);
        img[OFFSET + 14] ^= 1;
        fs::write(&ctl, &img).unwrap();
        assert!(matches!(
            apply(&ctl, &auth2, None),
            Err(FirstBootError::Record(AnswersError::Checksum))
        ));
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn hostile_records_never_panic_or_create_odd_names() {
        let mut seed = 0x9e37_79b9_u32;
        let base = record(&[("user", "anas"), ("password_hash", HASH)]);
        for _ in 0..3000 {
            let mut junk = base[OFFSET..OFFSET + 200].to_vec();
            for _ in 0..3 {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let i = seed as usize % junk.len();
                junk[i] = (seed >> 16) as u8;
            }
            if let Ok(a) = Answers::decode(&junk) {
                if let Ok((user, line)) = account_line(&a) {
                    assert!(valid_user(&user) && line.lines().count() == 1);
                }
            }
        }
    }
}
