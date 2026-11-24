//! `ultar-firstboot` – the first start of an installed system applies the installer's answers.
//!
//! ```text
//! ultar-firstboot --bootctl PATH [--auth-dir DIR] [--owner UID]     apply the record once
//! ultar-firstboot --list [--auth-dir DIR]                          print the accounts
//! ```
//! The service file names the raw node of `ULTAR-BOOTCTL` (`firstboot.toml`); `--auth-dir`
//! defaults to `/data/ultar/auth`. Lines go to standard error
//! (`[firstboot] …`, never a hash). Exit status: 0 done or nothing to do, 1 failure, 2 usage.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::ExitCode;
use ultar_installer_ui::firstboot::{accounts, apply, Outcome};

/// `--list`: the accounts of the account file, one line each.
fn list_accounts(auth: &std::path::Path) -> ExitCode {
    match accounts(auth) {
        Ok(names) => {
            for n in names {
                eprintln!("[firstboot] account {n}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("[firstboot] no accounts: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let mut bootctl: Option<PathBuf> = None;
    let mut auth = PathBuf::from("/data/ultar/auth");
    let mut owner = None;
    let mut list = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let value = match a.as_str() {
            "--list" => {
                list = true;
                continue;
            }
            "--bootctl" | "--auth-dir" | "--owner" => args.next(),
            _ => None,
        };
        let Some(value) = value else {
            eprintln!("[firstboot] usage: ultar-firstboot --bootctl PATH [--auth-dir DIR] [--owner UID] | --list");
            return ExitCode::from(2);
        };
        match a.as_str() {
            "--bootctl" => bootctl = Some(value.into()),
            "--auth-dir" => auth = value.into(),
            _ => match value.parse::<u32>() {
                Ok(uid) => owner = Some(uid),
                Err(_) => return ExitCode::from(2),
            },
        }
    }
    if list {
        return list_accounts(&auth);
    }
    let Some(bootctl) = bootctl else {
        eprintln!("[firstboot] usage: --bootctl PATH is needed");
        return ExitCode::from(2);
    };
    match apply(&bootctl, &auth, owner) {
        Ok(Outcome::Nothing) => eprintln!("[firstboot] nothing to apply"),
        Ok(Outcome::Created(user)) => {
            eprintln!("[firstboot] account {user} created, answers record wiped")
        }
        Ok(Outcome::Kept) => {
            eprintln!("[firstboot] account file exists, kept; answers record wiped")
        }
        Err(e) => {
            eprintln!("[firstboot] failed: {e}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
