//! First start, part 3: the public SSH key typed in the installer (answer `ssh_key`) becomes an
//! allowed key of the developer access – `<dir>/authorized_keys` (`/data/ultar/entwickler`, read
//! by `ultar-sshd`). The record is hostile input: the line is checked again
//! ([`crate::ssh_key::valid`]); the directory is made 0700, the file 0600, a key that is there
//! already is not added twice, and a link at the file's place is never followed.

use crate::engine::answers::Answers;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

/// Name of the key file in the developer directory (as `ultar-sshd` reads it).
pub const FILE: &str = "authorized_keys";

/// What happened to the answer `ssh_key`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SshKey {
    /// No key was typed.
    #[default]
    None,
    /// The key was added.
    Added,
    /// The key was there already.
    Present,
    /// The value is not a valid key line (nothing written).
    Refused,
    /// Writing failed (nothing or not everything written).
    Failed,
}

/// Adds the answer's key to `dir/authorized_keys`. Never fails the first start.
pub fn apply_ssh_key(a: &Answers, dir: &Path) -> SshKey {
    let Some(line) = a.get("ssh_key").map(str::trim).filter(|l| !l.is_empty()) else {
        return SshKey::None;
    };
    if !crate::ssh_key::valid(line) {
        return SshKey::Refused;
    }
    match add(dir, line) {
        Ok(true) => SshKey::Added,
        Ok(false) => SshKey::Present,
        Err(_) => SshKey::Failed,
    }
}

/// Type and key data of a key line (the comment does not make another key).
fn key(line: &str) -> Vec<&str> {
    line.split(' ').filter(|p| !p.is_empty()).take(2).collect()
}

fn add(dir: &Path, line: &str) -> std::io::Result<bool> {
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    let path = dir.join(FILE);
    let have = match fs::symlink_metadata(&path) {
        Ok(m) if !m.file_type().is_file() => {
            return Err(std::io::Error::other("authorized_keys is not a plain file"))
        }
        Ok(_) => fs::read_to_string(&path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    if have.lines().any(|l| key(l) == key(line)) {
        return Ok(false);
    }
    let mut f = OpenOptions::new()
        .append(true)
        .create(true)
        .custom_flags(o_nofollow())
        .mode(0o600)
        .open(&path)?;
    let sep = if have.is_empty() || have.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    f.write_all(format!("{sep}{line}\n").as_bytes())?;
    f.set_permissions(fs::Permissions::from_mode(0o600))?;
    f.sync_all()?;
    Ok(true)
}

/// `O_NOFOLLOW` (the generic Linux value on x86_64; aarch64 has its own).
const fn o_nofollow() -> i32 {
    if cfg!(target_arch = "aarch64") {
        0o100_000
    } else {
        0o400_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl anas@pc";

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ultar-fb-ssh-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn answers(key: Option<&str>) -> Answers {
        let mut a = Answers::default();
        if let Some(k) = key {
            a.set("ssh_key", k);
        }
        a
    }

    #[test]
    fn key_is_added_once_with_tight_modes() {
        let d = tmp("add").join("entwickler");
        assert_eq!(apply_ssh_key(&answers(Some(ED)), &d), SshKey::Added);
        assert_eq!(
            apply_ssh_key(&answers(Some(&format!(" {ED} "))), &d),
            SshKey::Present
        );
        let text = fs::read_to_string(d.join(FILE)).unwrap();
        assert_eq!(text, format!("{ED}\n"));
        assert_eq!(
            fs::metadata(&d).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(d.join(FILE)).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = fs::remove_dir_all(d.parent().unwrap());
    }

    #[test]
    fn existing_keys_stay_and_the_new_one_follows() {
        let d = tmp("keep");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(FILE), "ssh-ed25519 AAAAother pc2").unwrap();
        assert_eq!(apply_ssh_key(&answers(Some(ED)), &d), SshKey::Added);
        let text = fs::read_to_string(d.join(FILE)).unwrap();
        assert_eq!(text, format!("ssh-ed25519 AAAAother pc2\n{ED}\n"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn no_key_bad_key_and_a_link_write_nothing() {
        let d = tmp("bad");
        assert_eq!(apply_ssh_key(&answers(None), &d), SshKey::None);
        assert_eq!(
            apply_ssh_key(&answers(Some("ssh-rsa AAAA x")), &d),
            SshKey::Refused
        );
        assert!(!d.join(FILE).exists());
        fs::create_dir_all(&d).unwrap();
        let target = d.join("elsewhere");
        std::os::unix::fs::symlink(&target, d.join(FILE)).unwrap();
        assert_eq!(apply_ssh_key(&answers(Some(ED)), &d), SshKey::Failed);
        assert!(!target.exists());
        let _ = fs::remove_dir_all(&d);
    }
}
