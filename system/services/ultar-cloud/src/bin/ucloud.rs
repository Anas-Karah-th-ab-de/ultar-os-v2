//! ucloud — the Ultar cloud from the shell.
//!
//! `register|login <user> [--password-file <file>]` · `number` · `devices` ·
//! `device-rename <id> <name>` · `device-remove <id>` · `device-approve <id>` · `device-reset` ·
//! `alerts` · `not-me` · `vpn` · `store [search]` · `app <id>` · `install <id>` ·
//! `update [channel]` · `whoami`. Store and update commands work signed-out.
//!
//! The password is never taken from the command line (other users can read it there, and it
//! ends up in shell histories): it is read from standard input (first line) or from a file.
//! All output goes through message keys (Arabic, English, German).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::ExitCode;
use ultar_cloud::{messages, AppAccess, CatalogQuery, CloudError, Subject};
use ultar_i18n::{Arg, Lang, Translator};

type Failure = (CloudError, Subject);

fn general(error: CloudError) -> Failure {
    (error, Subject::General)
}

fn app_error(error: CloudError) -> Failure {
    (error, Subject::App)
}

fn store(tr: &Translator, search: &str) -> Result<(), Failure> {
    let cloud = ultar_cloud::client();
    let query = CatalogQuery {
        text: search.to_string(),
        per_page: 100,
        ..CatalogQuery::default()
    };
    let page = cloud.catalog(&query).map_err(app_error)?;
    let mine = if cloud.is_signed_in() {
        cloud.entitlements().ok()
    } else {
        None
    };
    if page.items.is_empty() {
        println!("{}", tr.t("cloud.store.empty"));
    }
    for app in &page.items {
        let state = AppAccess::of(app, mine.as_ref());
        println!(
            "{}  {}  {}  [{}]",
            app.id,
            app.name,
            app.version,
            tr.t(state.message_key())
        );
    }
    Ok(())
}

fn app(tr: &Translator, id: &str) -> Result<(), Failure> {
    let cloud = ultar_cloud::client();
    let details = cloud.app(id).map_err(app_error)?;
    let mine = if cloud.is_signed_in() {
        cloud.entitlements().ok()
    } else {
        None
    };
    let state = AppAccess::of(&details.summary, mine.as_ref());
    let s = &details.summary;
    println!(
        "{}  {}  {}  [{}]",
        s.id,
        s.name,
        s.version,
        tr.t(state.message_key())
    );
    if s.publisher.verified {
        println!(
            "{} ({})",
            s.publisher.name,
            tr.t("cloud.store.publisher.verified")
        );
    } else {
        println!("{}", s.publisher.name);
    }
    println!("{}", details.permissions.join(" "));
    Ok(())
}

fn install(tr: &Translator, id: &str) -> Result<(), Failure> {
    let work = Path::new("/data/ultar/dl");
    let trusted = Path::new(ultar_pkg::TRUSTED_DIR);
    let manifest = ultar_cloud::client()
        .install_app(id, work, trusted)
        .map_err(app_error)?;
    let args = [
        ("name", Arg::Str(&manifest.name)),
        ("version", Arg::Str(&manifest.version)),
    ];
    println!("{}", tr.t_args("cloud.store.installed", &args));
    Ok(())
}

fn update(tr: &Translator, channel: &str) -> Result<(), Failure> {
    let found = ultar_cloud::update_check(channel).map_err(|e| (e, Subject::UpdateChannel))?;
    match found {
        Some(release) => {
            let args = [
                ("version", Arg::Str(&release.version)),
                ("channel", Arg::Str(&release.channel)),
            ];
            println!("{}", tr.t_args("cloud.update.available", &args));
        }
        None => println!("{}", tr.t("cloud.update.none")),
    }
    Ok(())
}

fn devices(tr: &Translator) -> Result<(), Failure> {
    let list = ultar_cloud::client().devices().map_err(general)?;
    if list.is_empty() {
        println!("{}", tr.t("cloud.device.none"));
    }
    for device in &list {
        let state = if device.state == "legacy" {
            "cloud.device.state.legacy"
        } else if device.manage {
            "cloud.device.state.active"
        } else {
            "cloud.device.state.waiting"
        };
        let mark = if device.current {
            format!("  ({})", tr.t("cloud.device.this_device"))
        } else {
            String::new()
        };
        println!(
            "{}  {}  {}  [{}]{mark}",
            device.device_id,
            device.number.as_deref().unwrap_or("-"),
            device.name,
            tr.t(state)
        );
    }
    Ok(())
}

/// Rename or remove a device of the account, authorised by this device's key.
fn device_change(tr: &Translator, id: &str, new_name: Option<&str>) -> Result<(), Failure> {
    let key = ultar_cloud::DeviceHome::system()
        .key_store()
        .map_err(general)?;
    let cloud = ultar_cloud::client();
    match new_name {
        Some(name) => {
            cloud.rename_device(&key, id, name).map_err(general)?;
            println!("{}", tr.t("cloud.device.renamed"));
        }
        None => {
            cloud.remove_device(&key, id).map_err(general)?;
            println!("{}", tr.t("cloud.device.removed"));
        }
    }
    Ok(())
}

/// The password: first line of the given file, or of standard input.
fn read_password(file: Option<&str>) -> Result<String, CloudError> {
    let mut line = String::new();
    match file {
        Some(path) => line = std::fs::read_to_string(path)?,
        None => {
            std::io::stdin().read_line(&mut line)?;
        }
    }
    let password = line.lines().next().unwrap_or_default().to_string();
    if password.is_empty() {
        return Err(CloudError::InvalidCredentials);
    }
    Ok(password)
}

/// Shows what needs the user's attention: a pending move of a number and new security notices.
fn alerts(tr: &Translator) -> Result<(), Failure> {
    let watch = ultar_cloud::SecurityWatch::new(ultar_cloud::DeviceHome::system());
    let state = watch.check(&ultar_cloud::client()).map_err(general)?;
    if !state.needs_attention() {
        println!("{}", tr.t("cloud.alert.none"));
    }
    if let (Some(recovery), Some(hours)) = (&state.recovery, state.recovery_hours()) {
        let device: String = recovery.old_device.chars().take(8).collect();
        let args = [("device", Arg::Str(&device)), ("hours", Arg::Int(hours))];
        println!(
            "{}",
            tr.t_args(ultar_cloud::watch::KEY_RECOVERY_PENDING, &args)
        );
        let hint = if state.manage {
            ultar_cloud::watch::KEY_NOT_ME
        } else {
            "cloud.alert.cannot_cancel"
        };
        println!("{}", tr.t(hint));
    }
    for notice in &state.notices {
        if let Some(key) = ultar_cloud::watch::notice_key(&notice.event) {
            let args = [("device", Arg::Str(&notice.device))];
            println!("{}", tr.t_args(key, &args));
        }
    }
    watch.acknowledge(&state).map_err(general)
}

fn whoami(tr: &Translator) {
    let server = ultar_cloud::base_url();
    println!(
        "{}",
        tr.t_args("cloud.cli.server", &[("server", Arg::Str(&server))])
    );
    let key = if ultar_cloud::token().is_some() {
        "cloud.cli.account.signed_in"
    } else {
        "cloud.cli.account.signed_out"
    };
    println!("{}", tr.t(key));
    if let Some(number) = ultar_cloud::my_number() {
        println!(
            "{}",
            tr.t_args("cloud.cli.number", &[("number", Arg::Str(&number))])
        );
    }
}

fn run(tr: &Translator, args: &[&str]) -> Result<bool, Failure> {
    match args {
        ["register", user] | ["register", user, "--password-file", _] => {
            let password = read_password(args.get(3).copied()).map_err(general)?;
            ultar_cloud::register(user, &password).map_err(general)?;
            println!("{}", tr.t("cloud.cli.signed_in"));
        }
        ["login", user] | ["login", user, "--password-file", _] => {
            let password = read_password(args.get(3).copied()).map_err(general)?;
            ultar_cloud::login(user, &password).map_err(general)?;
            println!("{}", tr.t("cloud.cli.signed_in"));
        }
        ["device-approve", id] => {
            let key = ultar_cloud::DeviceHome::system()
                .key_store()
                .map_err(general)?;
            ultar_cloud::client()
                .approve_device(&key, id)
                .map_err(general)?;
            println!("{}", tr.t("cloud.device.approved"));
        }
        ["alerts"] => alerts(tr)?,
        ["not-me"] => {
            let watch = ultar_cloud::SecurityWatch::new(ultar_cloud::DeviceHome::system());
            watch.not_me(&ultar_cloud::client()).map_err(general)?;
            println!("{}", tr.t(ultar_cloud::watch::KEY_RECOVERY_CANCELLED));
        }
        ["number"] => {
            let number = ultar_cloud::register_number().map_err(general)?;
            println!(
                "{}",
                tr.t_args("cloud.cli.number", &[("number", Arg::Str(&number))])
            );
        }
        ["devices"] => devices(tr)?,
        ["device-rename", id, name] => device_change(tr, id, Some(name))?,
        ["device-remove", id] => device_change(tr, id, None)?,
        ["device-reset"] => {
            ultar_cloud::DeviceHome::system().reset().map_err(general)?;
            println!("{}", tr.t("cloud.device.reset"));
        }
        ["vpn"] => {
            ultar_cloud::vpn_provision().map_err(general)?;
            println!("{}", tr.t("cloud.cli.vpn_ready"));
        }
        ["store"] => store(tr, "")?,
        ["store", search] => store(tr, search)?,
        ["app", id] => app(tr, id)?,
        ["install", id] => install(tr, id)?,
        ["update"] => update(tr, "stable")?,
        ["update", channel] => update(tr, channel)?,
        [] | ["whoami"] => whoami(tr),
        _ => return Ok(false),
    }
    Ok(true)
}

fn main() -> ExitCode {
    let mut tr = Translator::empty(Lang::from_env());
    if messages::register(&mut tr).is_err() {
        return ExitCode::from(3);
    }
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = owned.iter().map(String::as_str).collect();
    match run(&tr, &args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            let command = args.first().copied().unwrap_or_default();
            let unknown = [("command", Arg::Str(command))];
            eprintln!("{}", tr.t_args("cloud.cli.unknown_command", &unknown));
            eprintln!("{}", tr.t("cloud.cli.usage"));
            ExitCode::from(2)
        }
        Err((error, subject)) => {
            eprintln!("{}", messages::describe(&tr, &error, subject));
            ExitCode::from(1)
        }
    }
}
