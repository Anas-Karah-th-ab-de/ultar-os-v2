//! The account file `users` in the service's directory: one line per user,
//! `user <name> admin=<0|1> display=<hex utf-8> password=<phc> pin=<phc|->`, checked by
//! its CRC line. Any line that does not fit makes the whole file damaged.

use crate::file::{self, FileError};
use crate::hash;
use crate::name::{valid_display, valid_user};
use std::path::{Path, PathBuf};
use ultar_bus::messages::auth::{UserInfo, MAX_USERS};

/// Header word of the account file.
pub const KIND: &str = "ultar-auth";
/// File name in the directory.
pub const FILE: &str = "users";

/// One stored account.
#[derive(Clone, PartialEq, Eq)]
pub struct Account {
    /// Name.
    pub name: String,
    /// Shown name.
    pub display: String,
    /// Administrator.
    pub admin: bool,
    /// Argon2id hash of the password.
    pub password: String,
    /// Argon2id hash of the PIN, if one is set up.
    pub pin: Option<String>,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("name", &self.name)
            .field("admin", &self.admin)
            .field("has_pin", &self.pin.is_some())
            .finish_non_exhaustive()
    }
}

impl Account {
    /// What the bus may see.
    pub fn info(&self) -> UserInfo {
        UserInfo {
            name: self.name.clone(),
            display_name: self.display.clone(),
            has_pin: self.pin.is_some(),
            admin: self.admin,
        }
    }
}

fn hex(text: &str) -> String {
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<String> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes = (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
}

/// Turns accounts into the file body.
pub fn encode(accounts: &[Account]) -> String {
    accounts
        .iter()
        .map(|a| {
            format!(
                "user {} admin={} display={} password={} pin={}\n",
                a.name,
                u8::from(a.admin),
                hex(&a.display),
                a.password,
                a.pin.as_deref().unwrap_or("-")
            )
        })
        .collect()
}

fn field<'a>(part: Option<&'a str>, key: &str) -> Option<&'a str> {
    part?.strip_prefix(key)?.strip_prefix('=')
}

fn decode_line(line: &str) -> Option<Account> {
    let mut parts = line.split(' ');
    (parts.next()? == "user").then_some(())?;
    let name = parts.next()?.to_owned();
    let admin = match field(parts.next(), "admin")? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    let display = unhex(field(parts.next(), "display")?)?;
    let password = field(parts.next(), "password")?.to_owned();
    let pin = match field(parts.next(), "pin")? {
        "-" => None,
        p => Some(p.to_owned()),
    };
    let ok = parts.next().is_none()
        && valid_user(&name)
        && valid_display(&display)
        && hash::looks_like_phc(&password)
        && pin.as_deref().is_none_or(hash::looks_like_phc);
    ok.then_some(Account {
        name,
        display,
        admin,
        password,
        pin,
    })
}

/// Reads accounts from a file body; `Damaged` for anything unexpected (also a name twice
/// or more than [`MAX_USERS`] accounts).
pub fn decode(body: &str) -> Result<Vec<Account>, FileError> {
    let mut out: Vec<Account> = Vec::new();
    for line in body.lines() {
        let account = decode_line(line).ok_or(FileError::Damaged)?;
        if out.iter().any(|a| a.name == account.name) || out.len() >= MAX_USERS {
            return Err(FileError::Damaged);
        }
        out.push(account);
    }
    Ok(out)
}

/// The account file of a directory.
pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// Loads the accounts (none when the file does not exist yet).
pub fn load(dir: &Path) -> Result<Vec<Account>, FileError> {
    match file::read(&path(dir), KIND)? {
        Some(body) => decode(&body),
        None => Ok(Vec::new()),
    }
}

/// Stores the accounts atomically.
pub fn save(dir: &Path, accounts: &[Account]) -> Result<(), FileError> {
    file::write(&path(dir), KIND, &encode(accounts))
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "$argon2id$v=19$m=64,t=1,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaA";

    fn account(name: &str) -> Account {
        Account {
            name: name.into(),
            display: "أنس قرة محمد".into(),
            admin: true,
            password: H.into(),
            pin: Some(H.into()),
        }
    }

    #[test]
    fn accounts_round_trip_and_hide_hashes_in_debug() {
        let list = vec![
            account("anas"),
            Account {
                pin: None,
                admin: false,
                ..account("leila")
            },
        ];
        let body = encode(&list);
        assert_eq!(decode(&body).unwrap(), list);
        let shown = format!("{:?}", list[0]);
        assert!(
            !shown.contains("argon2") && shown.contains("anas"),
            "{shown}"
        );
        assert_eq!(list[0].info().display_name, "أنس قرة محمد");
        assert!(list[0].info().has_pin && !list[1].info().has_pin);
    }

    #[test]
    fn anything_unexpected_is_damage() {
        let good = encode(&[account("anas")]);
        for bad in [
            good.replace("admin=1", "admin=2"),
            good.replace("user anas", "user Anas"),
            good.replace("display=", "display=zz"),
            good.replace("$argon2id$", "$argon2i$"),
            good.replace('\n', " extra\n"),
            format!("{good}{good}"),
            "user\n".to_owned(),
            "group x\n".to_owned(),
        ] {
            assert_eq!(decode(&bad), Err(FileError::Damaged), "{bad}");
        }
        let many: Vec<Account> = (0..=MAX_USERS).map(|i| account(&format!("u{i}"))).collect();
        assert_eq!(decode(&encode(&many)), Err(FileError::Damaged));
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("zz"), None);
    }
}
