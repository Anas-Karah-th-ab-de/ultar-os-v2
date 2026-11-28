//! `ultar-firstboot` – the first start of an installed system applies the installer's answers.
//!
//! ```text
//! ultar-firstboot --bootctl PATH [--auth-dir DIR] [--owner UID]
//!                 [--settings-dir DIR] [--settings-user NAME] [--install-dir DIR]
//!                                                                  apply the record once
//! ultar-firstboot --list [--auth-dir DIR]                          print the accounts
//! ```
//! The service file names the raw node of `ULTAR-BOOTCTL` (`firstboot.toml`); `--auth-dir`
//! defaults to `/data/ultar/auth`. With `--settings-dir` the other choices (language,
//! keyboard, time zone, accessibility, device name) go into the settings service's store
//! (`user-<--settings-user, default "default">.toml` / `system.toml`) and edition and
//! components into `<--install-dir, default /data/ultar/install>/selection.toml`. Lines go to standard error
//! (`[firstboot] …`, never a hash). Exit status: 0 done or nothing to do, 1 failure, 2 usage.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::ExitCode;
use ultar_installer_ui::firstboot::{accounts, apply_all, Extras, Outcome};
use ultar_installer_ui::firstboot_settings::SettingsReport;

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

/// One line of what part 2 did (key names only, never a value).
fn report_settings(r: &SettingsReport) {
    eprintln!(
        "[firstboot] settings stored={} kept={} refused=[{}] damaged=[{}] selection={}",
        r.stored.len(),
        r.kept.len(),
        r.refused.join(","),
        r.damaged.join(","),
        r.selection
    );
}

const USAGE: &str = "[firstboot] usage: ultar-firstboot --bootctl PATH [--auth-dir DIR] [--owner UID] [--settings-dir DIR] [--settings-user NAME] [--install-dir DIR] | --list";

/// The command line.
struct Args {
    bootctl: Option<PathBuf>,
    auth: PathBuf,
    owner: Option<u32>,
    list: bool,
    settings: Option<PathBuf>,
    settings_user: String,
    install: PathBuf,
}

/// Reads the command line; `None` on a usage error.
fn parse(mut args: impl Iterator<Item = String>) -> Option<Args> {
    let mut out = Args {
        bootctl: None,
        auth: PathBuf::from("/data/ultar/auth"),
        owner: None,
        list: false,
        settings: None,
        settings_user: String::from("default"),
        install: PathBuf::from("/data/ultar/install"),
    };
    while let Some(a) = args.next() {
        if a == "--list" {
            out.list = true;
            continue;
        }
        let value = args.next()?;
        match a.as_str() {
            "--bootctl" => out.bootctl = Some(value.into()),
            "--auth-dir" => out.auth = value.into(),
            "--owner" => out.owner = Some(value.parse::<u32>().ok()?),
            "--settings-dir" => out.settings = Some(value.into()),
            "--settings-user" => out.settings_user = value,
            "--install-dir" => out.install = value.into(),
            _ => return None,
        }
    }
    Some(out)
}

fn main() -> ExitCode {
    let Some(args) = parse(std::env::args().skip(1)) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if args.list {
        return list_accounts(&args.auth);
    }
    let Some(bootctl) = args.bootctl else {
        eprintln!("[firstboot] usage: --bootctl PATH is needed");
        return ExitCode::from(2);
    };
    let extras = args.settings.map(|settings_dir| Extras {
        settings_dir,
        settings_user: args.settings_user,
        install_dir: args.install,
    });
    let result = apply_all(&bootctl, &args.auth, args.owner, extras.as_ref());
    match result {
        Ok((Outcome::Nothing, _)) => eprintln!("[firstboot] nothing to apply"),
        Ok((outcome, report)) => {
            if extras.is_some() {
                report_settings(&report);
            }
            match outcome {
                Outcome::Created(user) => {
                    eprintln!("[firstboot] account {user} created, answers record wiped")
                }
                _ => eprintln!("[firstboot] account file exists, kept; answers record wiped"),
            }
        }
        Err(e) => {
            eprintln!("[firstboot] failed: {e}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
