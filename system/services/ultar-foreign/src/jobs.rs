//! The work of the service, run on a worker thread (the service loop stays free):
//! inspecting a file, installing it after the administrator confirmed, removing an app.
//!
//! Order of an installation of a file (each step published on `foreign.progress`):
//!
//! 1. **Prepare** – a copy of the file in the inbox; its fields read by the Linux area's
//!    `dpkg-deb` in the inspection cell (no network, `nobody`, read-only).
//! 2. **Confirm** – the system dialog asks for an administrator's password, naming what
//!    the file says it is (unverified), that it comes from another system, and what the
//!    app will get. Without "authorized" nothing is installed.
//! 3. **Install** – `apt-get install` of the copy in the installation cell (writable
//!    Linux area, no kernel rights, no network until the kernel's barrier exists).
//! 4. **Register** – its visible programs become `app.foreign` apps in the launcher.
//!
//! A failing step leaves nothing behind: the package is purged again, registered apps
//! are removed, the copy is deleted.

use crate::cellrun::{self, CellJob, Mode, INBOX_IN_CELL};
use crate::desktop;
use crate::detect;
use crate::harvest;
use crate::layout::Layout;
use crate::state::{AppRecord, State};
use std::path::{Path, PathBuf};
use std::time::Duration;
use ultar_bus::messages::foreign::{
    Choices, Failure, InspectOutcome, Inspected, NetworkChoice, Progress, Source, Step,
};
use ultar_bus::messages::launcher::{InstallForeign, Uninstall};
use ultar_bus::messages::sysdialog::{AuthResult, Authenticate, CALL_TIMEOUT_MS};
use ultar_bus::{Client, Text, TextArg};

/// Longest wait for reading a package's fields.
const INSPECT_LIMIT: Duration = Duration::from_secs(60);
/// Longest wait for the package manager.
const INSTALL_LIMIT: Duration = Duration::from_secs(30 * 60);
/// Longest wait for the launcher.
const LAUNCHER_WAIT: Duration = Duration::from_secs(20);

/// What a job needs.
#[derive(Clone, Debug)]
pub struct Ctx {
    /// The service's places.
    pub layout: Layout,
    /// This program (the cells' helper).
    pub helper: PathBuf,
    /// Where files may come from (`detect::SOURCES`).
    pub sources: Vec<PathBuf>,
}

/// The fields of a `.deb` and its copy in the inbox.
struct Copied {
    dir: PathBuf,
    file: String,
    control: desktop::Control,
}

impl Drop for Copied {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Copies the opened file into a fresh inbox directory readable by the inspection
/// cell's `nobody` (`0755`/`0644`; only the service writes there). The copy is what is
/// read and installed afterwards, whatever happens to the original meanwhile.
pub(crate) fn copy_in(
    ctx: &Ctx,
    source: &mut std::fs::File,
    job: u64,
    file: &str,
) -> std::io::Result<PathBuf> {
    use std::io::Read;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let dir = ctx.layout.inbox().join(job.to_string());
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))?;
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(dir.join(file))?;
    // no more than the limit, even if the original grows while it is copied
    let copied = std::io::copy(&mut source.take(detect::MAX_FILE + 1), &mut out)?;
    if copied > detect::MAX_FILE {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(std::io::Error::from(std::io::ErrorKind::FileTooLarge));
    }
    out.sync_all()?;
    std::fs::set_permissions(dir.join(file), std::fs::Permissions::from_mode(0o644))?;
    Ok(dir)
}

/// Reads the fields of the opened `.deb` in the inspection cell.
fn read_deb(ctx: &Ctx, mut opened: detect::Opened, job: u64) -> Result<Copied, Failure> {
    let file = "package.deb".to_owned();
    let dir = copy_in(ctx, &mut opened.file, job, &file).map_err(|_| Failure::Space)?;
    let cell = CellJob {
        mode: Mode::Inspect,
        runtime: ctx.layout.runtime(),
        inbox: Some(dir.clone()),
        program: [
            "/usr/bin/dpkg-deb",
            "--field",
            &format!("{INBOX_IN_CELL}/{file}"),
            "Package",
            "Version",
            "Maintainer",
        ]
        .map(str::to_owned)
        .to_vec(),
    };
    let copied = |control| Copied {
        dir: dir.clone(),
        file: file.clone(),
        control,
    };
    let ran = cellrun::run(&ctx.helper, &cell, INSPECT_LIMIT).map_err(|_| Failure::Internal);
    let control = ran
        .ok()
        .filter(|r| r.code == Some(0))
        .and_then(|r| desktop::parse_control(&String::from_utf8_lossy(&r.output)));
    match control {
        Some(c) => Ok(copied(c)),
        None => {
            drop(copied(desktop::Control::default()));
            Err(Failure::Damaged)
        }
    }
}

/// `foreign.inspect`.
pub fn inspect(ctx: &Ctx, path: &str, job: u64) -> InspectOutcome {
    if !detect::path_ok(path) {
        return InspectOutcome::Invalid;
    }
    let file = Path::new(path);
    let opened = match detect::open_source(&ctx.sources, path) {
        Ok(opened) => opened,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return InspectOutcome::Invalid
        }
        Err(_) => return InspectOutcome::Unreadable,
    };
    let (size, source) = (opened.size, opened.source);
    if size > detect::MAX_FILE {
        return InspectOutcome::TooLarge;
    }
    let runtime = ctx.layout.runtime_installed();
    if source == Source::Apk && runtime {
        return match crate::apkjobs::read(ctx, opened, job) {
            Ok(read) => InspectOutcome::Found(crate::apkjobs::inspected(
                &read,
                size,
                ctx.layout.android_installed(),
            )),
            Err(_) => InspectOutcome::Unreadable,
        };
    }
    let (mut name, mut version, mut publisher) = (detect::stem(file), String::new(), String::new());
    let mut installable = source.installable() && runtime;
    if matches!(source, Source::Exe | Source::Msi) {
        match crate::windows::describe(&opened, file) {
            Ok(d) => {
                (name, version, publisher) = (d.name, d.version, d.publisher);
                installable &= ctx.layout.windows_installed();
            }
            Err(_) => return InspectOutcome::Unreadable,
        }
    }
    if source == Source::Deb && runtime {
        match read_deb(ctx, opened, job) {
            Ok(c) => {
                name = c.control.package.clone();
                version = c.control.version.clone();
                publisher = c.control.maintainer.clone();
            }
            Err(_) => return InspectOutcome::Unreadable,
        }
    }
    InspectOutcome::Found(Inspected {
        source,
        name,
        version,
        publisher,
        size_bytes: size,
        verified: false,
        installable,
        suggested: Choices::default(),
        signer: String::new(),
        refusal: None,
        not_granted: Vec::new(),
    })
}

/// Publishes one step of `job`.
pub fn step(client: &Client, job: u64, step: Step, percent: u8, apps: Vec<String>) {
    let _ = client.publish(&Progress {
        job,
        step,
        percent,
        apps,
    });
}

/// The dialog's question for installing `name` of `source` with `choices`.
pub(crate) fn question(name: &str, source: Source, choices: &Choices, archive: bool) -> Text {
    let net = match (choices.network, choices.lan) {
        (NetworkChoice::None, _) => "foreign.net.none",
        (NetworkChoice::Internet, false) => "foreign.net.internet",
        (NetworkChoice::Internet, true) => "foreign.net.lan",
    };
    let folders = if choices.folders.is_empty() {
        TextArg::Key("foreign.folders.none".into())
    } else {
        TextArg::Verbatim(choices.folders.join(", "))
    };
    Text {
        key: if archive {
            "foreign.confirm.archive"
        } else {
            "foreign.confirm.install"
        }
        .into(),
        args: vec![
            TextArg::Verbatim(name.chars().take(64).collect()),
            TextArg::Key(format!("foreign.source.{}", source.word())),
            TextArg::Key(net.into()),
            folders,
        ],
    }
}

/// Asks an administrator; `Ok(true)`: authorised.
pub(crate) fn confirm(client: &Client, caller: &str, text: Text) -> Result<bool, Failure> {
    let ask = Authenticate {
        app_id: caller.to_owned(),
        app_name: caller.to_owned(),
        action: text,
    };
    match client.call(&ask, Duration::from_millis(CALL_TIMEOUT_MS)) {
        Ok(r) if r.result == AuthResult::Authorized => Ok(true),
        Ok(r) if r.result == AuthResult::Cancelled => Ok(false),
        _ => Err(Failure::NoConfirmation),
    }
}

/// Runs the package manager in the installation cell.
fn apt(ctx: &Ctx, inbox: Option<PathBuf>, args: &[&str]) -> Result<(), Failure> {
    // the cell is apt's sandbox: no switch to `_apt` (no set-id in the cell)
    let mut program = vec![
        "/usr/bin/apt-get".to_owned(),
        "-o".to_owned(),
        "APT::Sandbox::User=root".to_owned(),
    ];
    program.extend(args.iter().map(|a| (*a).to_owned()));
    let cell = CellJob {
        mode: Mode::Install,
        runtime: ctx.layout.runtime(),
        inbox,
        program,
    };
    match cellrun::run(&ctx.helper, &cell, INSTALL_LIMIT) {
        Ok(r) if r.code == Some(0) => Ok(()),
        Ok(_) => Err(Failure::Refused),
        Err(_) => Err(Failure::Internal),
    }
}

/// Takes what `record`'s package put into the Linux area out again (the package
/// manager for Linux packages, the APK reader's helper for Android apps).
pub fn take_out(ctx: &Ctx, record: &AppRecord) -> Result<(), Failure> {
    if record.source == Source::Apk.word() {
        crate::apkjobs::take_out(ctx, &record.package)
    } else {
        purge(ctx, &record.package)
    }
}

/// Removes `package` from the Linux area (best effort, used when undoing).
pub fn purge(ctx: &Ctx, package: &str) -> Result<(), Failure> {
    if !desktop::package_name_ok(package) {
        return Err(Failure::Internal);
    }
    apt(ctx, None, &["remove", "--purge", "-y", package])
}

/// The apps a package's programs become; registered with the launcher one by one.
/// Registers (or replaces) one app with the launcher; true when the launcher took it as
/// exactly this app of profile `app.foreign`.
pub fn register_one(client: &Client, record: &AppRecord) -> bool {
    let call = InstallForeign {
        manifest: record.manifest(),
    };
    client
        .call(&call, LAUNCHER_WAIT)
        .is_ok_and(|r| r.app == record.id && r.profile == "app.foreign")
}

/// `foreign.set_choices` that grants more: an administrator confirms, then the launcher
/// gets the app with its new permissions. `Ok(None)`: not confirmed.
pub fn change(
    client: &Client,
    caller: &str,
    job: u64,
    record: &AppRecord,
) -> Result<Option<AppRecord>, Failure> {
    step(client, job, Step::Confirm, 20, Vec::new());
    let source = Source::from_word(&record.source).unwrap_or(Source::Unknown);
    let mut text = question(&record.name, source, &record.choices(), false);
    text.key = "foreign.confirm.change".into();
    if !confirm(client, caller, text)? {
        return Ok(None);
    }
    step(client, job, Step::Register, 80, Vec::new());
    if register_one(client, record) {
        Ok(Some(record.clone()))
    } else {
        Err(Failure::Launcher)
    }
}

fn register(
    ctx: &Ctx,
    client: &Client,
    state: &State,
    package: &str,
    version: &str,
    choices: &Choices,
) -> Result<Vec<AppRecord>, Failure> {
    let runtime = ctx.layout.runtime();
    let programs = harvest::programs(&runtime, package);
    if programs.is_empty() {
        return Err(Failure::NoPrograms);
    }
    let mut done: Vec<AppRecord> = Vec::new();
    for p in programs {
        let Some(id) = state
            .id_for(Source::Deb, package, &p.entry.file)
            .filter(|id| !done.iter().any(|d| &d.id == id))
        else {
            continue;
        };
        let mut record = AppRecord {
            icon: harvest::copy_icon(p.icon.as_deref(), &ctx.layout.icons(), &id),
            id,
            name: p.entry.name,
            names: p.entry.names,
            source: Source::Deb.word().into(),
            package: package.into(),
            entry: p.entry.file,
            version: version.into(),
            exec: p.exec,
            args: p.entry.args,
            network: false,
            lan: false,
            folders: Vec::new(),
            signers: Vec::new(),
            version_code: 0,
        };
        record.set_choices(choices);
        if !register_one(client, &record) {
            unregister(client, &done);
            return Err(Failure::Launcher);
        }
        done.push(record);
    }
    if done.is_empty() {
        return Err(Failure::Launcher);
    }
    Ok(done)
}

/// Removes `apps` from the launcher (their data too).
pub fn unregister(client: &Client, apps: &[AppRecord]) {
    for a in apps {
        let _ = client.call(
            &Uninstall {
                app: a.id.clone(),
                keep_data: false,
            },
            LAUNCHER_WAIT,
        );
    }
}

/// What an installation asks for.
pub struct Request {
    /// The job.
    pub job: u64,
    /// Who asked (for the dialog).
    pub caller: String,
    /// The file.
    pub path: String,
    /// What the person grants.
    pub choices: Choices,
}

/// Installs a `.deb` of the person. Returns the new apps or why it failed (`Ok(None)`:
/// not confirmed).
pub fn install_file(
    ctx: &Ctx,
    client: &Client,
    state: &State,
    req: &Request,
) -> Result<Option<Vec<AppRecord>>, Failure> {
    step(client, req.job, Step::Prepare, 5, Vec::new());
    let opened = detect::open_source(&ctx.sources, &req.path).map_err(|_| Failure::Damaged)?;
    let source = opened.source;
    if opened.size > detect::MAX_FILE {
        return Err(Failure::Damaged);
    }
    if source == Source::Apk {
        return crate::apkjobs::install(ctx, client, state, req, opened);
    }
    if source != Source::Deb {
        return Err(Failure::Damaged);
    }
    let copied = read_deb(ctx, opened, req.job)?;
    step(client, req.job, Step::Confirm, 15, Vec::new());
    let text = question(&copied.control.package, source, &req.choices, false);
    if !confirm(client, &req.caller, text)? {
        return Ok(None);
    }
    step(client, req.job, Step::Install, 30, Vec::new());
    let target = format!("{INBOX_IN_CELL}/{}", copied.file);
    apt(
        ctx,
        Some(copied.dir.clone()),
        &["install", "-y", "--no-install-recommends", &target],
    )?;
    step(client, req.job, Step::Register, 85, Vec::new());
    let package = copied.control.package.clone();
    match register(
        ctx,
        client,
        state,
        &package,
        &copied.control.version,
        &req.choices,
    ) {
        Ok(apps) => Ok(Some(apps)),
        Err(failure) => {
            let _ = purge(ctx, &package);
            Err(failure)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_question_names_the_file_and_what_the_app_gets() {
        let choices = Choices {
            network: NetworkChoice::Internet,
            lan: true,
            folders: vec!["documents".into(), "downloads".into()],
        };
        let t = question("mousepad", Source::Deb, &choices, false);
        assert_eq!(t.key, "foreign.confirm.install");
        assert_eq!(t.args[0], TextArg::Verbatim("mousepad".into()));
        assert_eq!(t.args[1], TextArg::Key("foreign.source.deb".into()));
        assert_eq!(t.args[2], TextArg::Key("foreign.net.lan".into()));
        assert_eq!(t.args[3], TextArg::Verbatim("documents, downloads".into()));
        let none = question(&"x".repeat(200), Source::Deb, &Choices::default(), true);
        assert_eq!(none.key, "foreign.confirm.archive");
        assert_eq!(none.args[2], TextArg::Key("foreign.net.none".into()));
        assert_eq!(none.args[3], TextArg::Key("foreign.folders.none".into()));
        assert!(matches!(&none.args[0], TextArg::Verbatim(n) if n.len() == 64));
    }

    #[test]
    fn inspecting_refuses_odd_paths_and_reads_heads() {
        let dir = std::env::temp_dir().join(format!("ufj-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = Ctx {
            layout: Layout::new(Path::new("/nonexistent/foreign")),
            helper: "/nonexistent/helper".into(),
            sources: vec![dir.clone()],
        };
        assert_eq!(inspect(&ctx, "relative.deb", 1), InspectOutcome::Invalid);
        // outside the person's areas and drives: refused, not merely unreadable
        assert_eq!(inspect(&ctx, "/etc/hostname", 1), InspectOutcome::Invalid);
        let missing = dir.join("missing.deb");
        assert_eq!(
            inspect(&ctx, missing.to_str().unwrap(), 1),
            InspectOutcome::Unreadable
        );
        let exe = dir.join("setup.exe");
        std::fs::write(&exe, crate::pe::tests::sample(&[], &[])).unwrap();
        // a head that only looks like a program is unreadable, not a program
        let fake = dir.join("fake.exe");
        let mut head = vec![0u8; 256];
        head[..2].copy_from_slice(b"MZ");
        head[0x3c..0x40].copy_from_slice(&128u32.to_le_bytes());
        head[128..132].copy_from_slice(b"PE\0\0");
        std::fs::write(&fake, &head).unwrap();
        assert_eq!(
            inspect(&ctx, fake.to_str().unwrap(), 1),
            InspectOutcome::Unreadable
        );
        match inspect(&ctx, exe.to_str().unwrap(), 1) {
            InspectOutcome::Found(i) => {
                assert_eq!(i.source, Source::Exe);
                assert!(!i.installable && !i.verified);
                assert_eq!(i.name, "setup");
            }
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
