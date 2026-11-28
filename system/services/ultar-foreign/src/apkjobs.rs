//! The jobs for Android apps (`.apk`, stage P5 of `docs/FREMD-APPS.md`).
//!
//! 1. **Prepare** – a copy in the inbox, read by the service's APK reader in the
//!    inspection cell ([`crate::apk`]): package, names, version, native code, the
//!    signature (APK Signature Scheme v2/v3, verified there).
//! 2. **Check** – unsigned, v1-only, broken signatures and ARM-only apps are refused;
//!    an update must carry a signer of the installed app and must not go back.
//! 3. **Confirm** – the administrator in the system dialog, like every file of the person.
//! 4. **Install** – the installation cell puts `base.apk` and its x86-64 libraries into
//!    `/opt/ultar-android/apps/<package>` of the Linux area (read-only for the app).
//! 5. **Register** – one `app.foreign` app (`compat-apk`) that the Android runtime starts.

use crate::apk::{self, internal, Report};
use crate::cellrun::{self, CellJob, Mode, INBOX_IN_CELL};
use crate::jobs::{confirm, copy_in, question, register_one, step, Ctx, Request};
use crate::state::{AppRecord, State};
use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::foreign::{
    Choices, Failure, Inspected, NetworkChoice, Refusal, Source, Step, ANDROID_NOT_GRANTED,
};
use ultar_bus::Client;

/// Longest wait for reading an APK (signature digests of a large file).
const READ_LIMIT: Duration = Duration::from_secs(120);
/// Longest wait for putting it into the area.
const PLACE_LIMIT: Duration = Duration::from_secs(300);
/// Name of the copy in the inbox.
const COPY: &str = "app.apk";

/// The read APK and its copy (deleted when dropped).
pub struct Read {
    dir: PathBuf,
    /// What it says.
    pub report: Report,
}

impl Drop for Read {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Copies the opened APK into the inbox and reads it in the inspection cell.
pub fn read(ctx: &Ctx, mut opened: crate::detect::Opened, job: u64) -> Result<Read, Failure> {
    let dir = copy_in(ctx, &mut opened.file, job, COPY).map_err(|_| Failure::Space)?;
    let cell = CellJob {
        mode: Mode::Inspect,
        runtime: ctx.layout.runtime(),
        inbox: Some(dir.clone()),
        program: vec![internal::INSPECT.into(), format!("{INBOX_IN_CELL}/{COPY}")],
    };
    let report = cellrun::run(&ctx.helper, &cell, READ_LIMIT)
        .ok()
        .filter(|r| r.code == Some(0))
        .and_then(|r| Report::from_lines(&String::from_utf8_lossy(&r.output)));
    match report {
        Some(report) => Ok(Read { dir, report }),
        None => {
            let _ = std::fs::remove_dir_all(&dir);
            Err(Failure::Damaged)
        }
    }
}

/// The words of the permissions it asks for that it never gets here.
fn not_granted(report: &Report) -> Vec<String> {
    let mut words: Vec<String> = ANDROID_NOT_GRANTED
        .iter()
        .filter(|(p, _)| report.permissions.iter().any(|x| x == p))
        .map(|(_, w)| (*w).to_owned())
        .collect();
    words.dedup();
    words
}

/// What the dialog offers first: the internet only when the app asks for it.
fn suggested(report: &Report) -> Choices {
    let wants = report
        .permissions
        .iter()
        .any(|p| p == "android.permission.INTERNET");
    Choices {
        network: if wants {
            NetworkChoice::Internet
        } else {
            NetworkChoice::None
        },
        ..Choices::default()
    }
}

/// `foreign.inspect` of an APK (`android`: the component is in the area).
pub fn inspected(read: &Read, size: u64, android: bool) -> Inspected {
    let r = &read.report;
    let refusal = r
        .refusal()
        .or((!android).then_some(Refusal::RuntimeMissing));
    let signer = r
        .signers
        .first()
        .map(|(fp, cn)| format!("{fp} {cn}").trim_end().to_owned())
        .unwrap_or_default();
    Inspected {
        source: Source::Apk,
        name: r.label.clone(),
        version: r.version.clone(),
        publisher: r
            .signers
            .first()
            .map(|(_, cn)| cn.clone())
            .unwrap_or_default(),
        size_bytes: size,
        verified: false,
        installable: refusal.is_none(),
        suggested: suggested(r),
        signer,
        refusal,
        not_granted: not_granted(r),
    }
}

/// The installed app of the same package, if any.
fn installed<'a>(state: &'a State, package: &str) -> Option<&'a AppRecord> {
    state
        .apps
        .iter()
        .find(|a| a.source == Source::Apk.word() && a.package == package)
}

/// Whether `report` may replace `old` (same signer, not older).
pub fn update_check(old: Option<&AppRecord>, report: &Report) -> Result<(), Failure> {
    let Some(old) = old else {
        return Ok(());
    };
    if !report
        .fingerprints()
        .iter()
        .any(|f| old.signers.contains(f))
    {
        return Err(Failure::SignerChanged);
    }
    if report.version_code < old.version_code {
        return Err(Failure::Older);
    }
    Ok(())
}

fn install_cell(
    ctx: &Ctx,
    inbox: Option<PathBuf>,
    program: Vec<String>,
    limit: Duration,
) -> Result<(), Failure> {
    let cell = CellJob {
        mode: Mode::Install,
        runtime: ctx.layout.runtime(),
        inbox,
        program,
    };
    match cellrun::run(&ctx.helper, &cell, limit) {
        Ok(r) if r.code == Some(0) => Ok(()),
        Ok(_) => Err(Failure::Refused),
        Err(_) => Err(Failure::Internal),
    }
}

/// Takes the app's files out of the Linux area.
pub fn take_out(ctx: &Ctx, package: &str) -> Result<(), Failure> {
    if !apk::package_ok(package) {
        return Err(Failure::Internal);
    }
    install_cell(
        ctx,
        None,
        vec![internal::REMOVE.into(), package.into()],
        PLACE_LIMIT,
    )
}

fn write_icon(icon: Option<&[u8]>, icons: &std::path::Path, id: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let Some(bytes) = icon else {
        return String::new();
    };
    let (tmp, target) = (
        icons.join(format!(".{id}.tmp")),
        icons.join(format!("{id}.png")),
    );
    let written = std::fs::write(&tmp, bytes)
        .and_then(|()| std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644)))
        .and_then(|()| std::fs::rename(&tmp, &target));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return String::new();
    }
    target.to_string_lossy().into_owned()
}

fn record(ctx: &Ctx, state: &State, r: &Report, choices: &Choices) -> Result<AppRecord, Failure> {
    let id = installed(state, &r.package)
        .map(|a| a.id.clone())
        .or_else(|| state.id_for(Source::Apk, &r.package, &r.package))
        .ok_or(Failure::Launcher)?;
    let mut rec = AppRecord {
        icon: write_icon(r.icon.as_deref(), &ctx.layout.icons(), &id),
        id,
        name: r.label.clone(),
        names: r.labels.clone(),
        source: Source::Apk.word().into(),
        package: r.package.clone(),
        entry: r.package.clone(),
        version: r.version.clone(),
        exec: apk::RUN.into(),
        args: vec![r.package.clone(), r.activity.clone()],
        network: false,
        lan: false,
        folders: Vec::new(),
        signers: r.fingerprints(),
        version_code: r.version_code,
    };
    rec.set_choices(choices);
    Ok(rec)
}

/// Installs an APK of the person. `Ok(None)`: not confirmed.
pub fn install(
    ctx: &Ctx,
    client: &Client,
    state: &State,
    req: &Request,
    opened: crate::detect::Opened,
) -> Result<Option<Vec<AppRecord>>, Failure> {
    if !ctx.layout.android_installed() {
        return Err(Failure::Refused);
    }
    let read = read(ctx, opened, req.job)?;
    let r = read.report.clone();
    if r.refusal().is_some() {
        return Err(Failure::Damaged);
    }
    let old = installed(state, &r.package).cloned();
    update_check(old.as_ref(), &r)?;
    step(client, req.job, Step::Confirm, 15, Vec::new());
    if !confirm(
        client,
        &req.caller,
        question(&r.label, Source::Apk, &req.choices, false),
    )? {
        return Ok(None);
    }
    step(client, req.job, Step::Install, 40, Vec::new());
    let place = vec![
        internal::PLACE.into(),
        format!("{INBOX_IN_CELL}/{COPY}"),
        r.package.clone(),
    ];
    install_cell(ctx, Some(read.dir.clone()), place, PLACE_LIMIT)?;
    step(client, req.job, Step::Register, 85, Vec::new());
    let rec = record(ctx, state, &r, &req.choices)?;
    if register_one(client, &rec) {
        return Ok(Some(vec![rec]));
    }
    if old.is_none() {
        let _ = take_out(ctx, &r.package);
        if !rec.icon.is_empty() {
            let _ = std::fs::remove_file(&rec.icon);
        }
    }
    Err(Failure::Launcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apk::{Abi, Sig};

    fn report(code: u32, fp: &str) -> Report {
        Report {
            package: "org.example.calc".into(),
            label: "Calculator".into(),
            labels: Vec::new(),
            version: "1".into(),
            version_code: code,
            min_sdk: 21,
            activity: "org/example/calc/Main".into(),
            abi: Abi::Java,
            sig: Sig::Valid(2),
            signers: vec![(fp.repeat(64), String::new())],
            permissions: vec![
                "android.permission.INTERNET".into(),
                "android.permission.CAMERA".into(),
                "android.permission.ACCESS_FINE_LOCATION".into(),
                "android.permission.ACCESS_COARSE_LOCATION".into(),
            ],
            icon: None,
        }
    }

    #[test]
    fn updates_keep_their_signer_and_never_go_back() {
        let old = AppRecord {
            id: "foreign.apk-calc".into(),
            name: "Calculator".into(),
            names: Vec::new(),
            source: "apk".into(),
            package: "org.example.calc".into(),
            entry: "org.example.calc".into(),
            version: "1".into(),
            icon: String::new(),
            exec: apk::RUN.into(),
            args: Vec::new(),
            network: false,
            lan: false,
            folders: Vec::new(),
            signers: vec!["a".repeat(64)],
            version_code: 5,
        };
        assert_eq!(update_check(None, &report(1, "b")), Ok(()));
        assert_eq!(update_check(Some(&old), &report(6, "a")), Ok(()));
        assert_eq!(update_check(Some(&old), &report(5, "a")), Ok(()));
        assert_eq!(
            update_check(Some(&old), &report(6, "b")),
            Err(Failure::SignerChanged)
        );
        assert_eq!(
            update_check(Some(&old), &report(4, "a")),
            Err(Failure::Older)
        );
        assert!(old.manifest().contains("kind = \"compat-apk\""));
    }

    #[test]
    fn what_the_dialog_offers_and_withholds() {
        let r = report(1, "a");
        assert_eq!(suggested(&r).network, NetworkChoice::Internet);
        assert_eq!(
            not_granted(&r),
            vec!["camera".to_owned(), "location".to_owned()]
        );
        let mut quiet = r.clone();
        quiet.permissions.clear();
        assert_eq!(suggested(&quiet), Choices::default());
    }
}
