//! `foreigntest` – what the kernel under it offers the apps from other systems
//! (`tools/kern-test/foreign-e2e.sh` runs it on the Ultar kernel in QEMU; on Linux as
//! root it should say `FOREIGN: ALLES OK`).
//!
//! ```text
//! foreigntest --helper /bin/ultar-foreign --area /data/foreign/linux [--deb FILE] [--dex FILE]
//! ```
//!
//! With the area's component „Windows programs“ (`components=windows`) it also runs Wine
//! (`windows-programs`), between the lines `FOREIGN-WINDOWS: start` and `… end`.
//!
//! Each kernel feature is tried in its own child process; the cells are the service's
//! own (`cellrun`), with the Ubuntu programs of the area. Every line is
//! `FOREIGN: <check> ok` or `FOREIGN: <check> FEHLT <detail>`; the last line is
//! `FOREIGN: ALLES OK` or `FOREIGN: FEHLENDE KERN-FUNKTIONEN: <list>`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use ultar_foreign::cellrun::{self, CellJob, Mode, SetupStep, INBOX_IN_CELL};
use ultar_sys::cell::{self as sys_cell, BindMode, Namespaces};

// ART of the component "Android apps" (check android-art)
mod art;
mod wine;

/// The raw checks, each in a child (`--check NAME`): name and what it needs.
const RAW: [(&str, &str); 15] = [
    ("namespaces", "mount and pid namespaces"),
    (
        "network-namespace",
        "a cell without network (own network namespace)",
    ),
    (
        "file-bind",
        "binding a single device node (the null device into the cell)",
    ),
    ("bind-read-only", "a read-only bind of the area"),
    (
        "remount-nosuid",
        "remount of a bind without set-id and devices",
    ),
    (
        "parent-death-signal",
        "the cell ends with its parent (PR_SET_PDEATHSIG)",
    ),
    ("no-new-privileges", "PR_SET_NO_NEW_PRIVS"),
    (
        "switch-to-nobody",
        "setresuid/setresgid to nobody (inspection cell)",
    ),
    (
        "root-writes-area",
        "root of the cell without rights writes the area's root-owned files",
    ),
    (
        "chown-outside-cell",
        "outside a cell, root without rights gives no file another owner (EPERM)",
    ),
    (
        "network-guard",
        "the barrier \"internet only\" in a network namespace of its own",
    ),
    ("tmpfs", "tmpfs for /run and /tmp"),
    ("proc-mount", "an own /proc"),
    ("pivot-root", "the root switch into the cell's view"),
    (
        "ubuntu-programs",
        "the area's Ubuntu programs (glibc, ld.so) under the Linux ABI",
    ),
];

fn errno(e: ultar_sys::Errno) -> String {
    format!("{e:?}")
}

/// A mount namespace of its own whose mounts do not leak out.
fn private() -> Result<(), String> {
    sys_cell::unshare(Namespaces {
        mount: true,
        ..Namespaces::default()
    })
    .map_err(errno)?;
    sys_cell::make_mounts_private().map_err(errno)
}

fn raw(name: &str, area: &Path) -> Result<(), String> {
    let scratch = std::env::temp_dir().join(format!("ft-{name}"));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    match name {
        "namespaces"
        | "network-namespace"
        | "parent-death-signal"
        | "no-new-privileges"
        | "switch-to-nobody"
        | "network-guard" => process_check(name),
        "file-bind" | "bind-read-only" | "remount-nosuid" | "tmpfs" | "proc-mount"
        | "pivot-root" => mount_check(name, &scratch),
        "root-writes-area" | "chown-outside-cell" => rights_check(name, area),
        "ubuntu-programs" => ubuntu_programs(area),
        "windows-programs" => wine::windows_programs(area),
        _ => Err("unknown check".into()),
    }
}

/// The checks of the process: namespaces, signals, rights, identity and the network barrier.
fn process_check(name: &str) -> Result<(), String> {
    match name {
        "namespaces" => sys_cell::unshare(Namespaces {
            mount: true,
            pid: true,
            ..Namespaces::default()
        })
        .map_err(errno),
        "network-namespace" => sys_cell::unshare(Namespaces {
            net: true,
            ..Namespaces::default()
        })
        .map_err(errno),
        "parent-death-signal" => sys_cell::kill_with_parent().map_err(errno),
        "no-new-privileges" => ultar_sys::set_no_new_privileges().map_err(errno),
        "switch-to-nobody" => {
            let nobody = ultar_sys::Identity {
                uid: 65534,
                gid: 65534,
                groups: Vec::new(),
            };
            ultar_sys::switch_identity(&nobody).map_err(|e| format!("{e:?}"))?;
            if ultar_sys::euid() == 65534 {
                Ok(())
            } else {
                Err(format!("euid {}", ultar_sys::euid()))
            }
        }
        "network-guard" => {
            sys_cell::unshare(Namespaces {
                net: true,
                ..Namespaces::default()
            })
            .map_err(|e| format!("namespace {e:?}"))?;
            ultar_sys::ultar_net_guard().map_err(|e| format!("guard {e:?}"))?;
            match ultar_sys::ultar_net_guarded() {
                Ok(true) => Ok(()),
                other => Err(format!("read back {other:?}")),
            }
        }
        _ => Err("unknown check".into()),
    }
}

/// The checks of the mounts, each in a private mount namespace on `scratch`.
fn mount_check(name: &str, scratch: &Path) -> Result<(), String> {
    private()?;
    match name {
        "file-bind" => {
            let node = scratch.join("null");
            std::fs::File::create(&node).map_err(|e| e.to_string())?;
            sys_cell::bind(
                &Path::new(sys_cell::DEV).join("null"),
                &node,
                BindMode::Device,
            )
            .map_err(errno)
        }
        "bind-read-only" => {
            sys_cell::bind(scratch, scratch, BindMode::ReadOnly).map_err(errno)?;
            match std::fs::write(scratch.join("x"), b"x") {
                Ok(()) => Err("written through the read-only bind".into()),
                Err(_) => Ok(()),
            }
        }
        "remount-nosuid" => {
            use rustix::mount::{mount_bind, mount_remount, MountFlags};
            mount_bind(scratch, scratch).map_err(|e| format!("bind {e:?}"))?;
            mount_remount(
                scratch,
                MountFlags::BIND | MountFlags::NOSUID | MountFlags::NODEV,
                "",
            )
            .map_err(|e| format!("remount {e:?}"))
        }
        "tmpfs" => sys_cell::mount_tmpfs(scratch, "size=65536,mode=0755").map_err(errno),
        "proc-mount" => sys_cell::mount_proc(scratch).map_err(errno),
        "pivot-root" => {
            let new = scratch.join("new");
            std::fs::create_dir_all(new.join("old")).map_err(|e| e.to_string())?;
            sys_cell::bind(&new, &new, BindMode::ReadOnly).map_err(|e| format!("bind {e:?}"))?;
            sys_cell::pivot_root(&new, &new.join("old")).map_err(|e| format!("pivot {e:?}"))?;
            sys_cell::detach(Path::new("/old")).map_err(|e| format!("detach {e:?}"))
        }
        _ => Err("unknown check".into()),
    }
}

/// Root without rights: may it write the area's root-owned files, may it give a file
/// another owner outside a cell?
fn rights_check(name: &str, area: &Path) -> Result<(), String> {
    let file = area.join("var/lib/dpkg").join(format!("ultar-{name}"));
    std::fs::write(&file, b"x").map_err(|e| format!("prepare {e}"))?;
    let ultar = ultar_sys::ultar_rights().is_ok();
    if ultar {
        ultar_sys::ultar_drop_rights(u64::MAX).map_err(errno)?;
    }
    let result = if name == "root-writes-area" {
        std::fs::write(&file, b"y").map_err(|e| e.to_string())
    } else {
        match rustix::fs::chown(&file, Some(rustix::fs::Uid::from_raw(42)), None) {
            Err(rustix::io::Errno::PERM) => Ok(()),
            // Linux: root keeps CAP_CHOWN outside a cell (the cells cut it)
            Ok(()) if !ultar => Ok(()),
            other => Err(format!("{other:?} (expected EPERM)")),
        }
    };
    let _ = std::fs::remove_file(&file);
    result
}

/// The area's Ubuntu programs under the Linux ABI, with the area as root.
fn ubuntu_programs(area: &Path) -> Result<(), String> {
    std::os::unix::fs::chroot(area).map_err(|e| format!("chroot {e}"))?;
    std::env::set_current_dir("/").map_err(|e| e.to_string())?;
    for program in [
        ["/usr/bin/dpkg-deb", "--version"],
        ["/usr/bin/apt-get", "--version"],
    ] {
        let out = Command::new(program[0])
            .arg(program[1])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            // the area has no device nodes (the cells bring their own): no null device here
            .stdin(std::process::Stdio::piped())
            .output()
            .map_err(|e| format!("{}: {e}", program[0]))?;
        if !out.status.success() {
            return Err(format!("{}: {:?}", program[0], out.status.code()));
        }
    }
    Ok(())
}

fn arg(words: &[String], name: &str) -> Option<String> {
    let at = words.iter().position(|w| w == name)?;
    words.get(at + 1).cloned()
}

fn cell(
    helper: &Path,
    mode: Mode,
    area: &Path,
    inbox: Option<PathBuf>,
    program: &[&str],
) -> Result<String, String> {
    let job = CellJob {
        mode,
        runtime: area.to_path_buf(),
        inbox,
        program: program.iter().map(|w| (*w).to_owned()).collect(),
    };
    let ran = cellrun::run(helper, &job, Duration::from_secs(900)).map_err(|e| e.to_string())?;
    let out = String::from_utf8_lossy(&ran.output).into_owned();
    match ran.code {
        Some(0) => Ok(out),
        Some(code) => match SetupStep::of_code(code) {
            Some(step) => Err(format!("cell step {step:?}")),
            None => {
                let tail: Vec<&str> = out.lines().rev().take(20).collect();
                let tail: Vec<&str> = tail.into_iter().rev().collect();
                Err(format!("exit {code}:\n    {}", tail.join("\n    ")))
            }
        },
        None => Err("killed or timed out".into()),
    }
}

/// The lines `FOREIGN: <check> ok|FEHLT`; what is missing.
#[derive(Default)]
struct Report {
    missing: Vec<String>,
}

impl Report {
    fn line(&mut self, name: &str, result: Result<(), String>) {
        match result {
            Ok(()) => println!("FOREIGN: {name} ok"),
            Err(detail) => {
                println!("FOREIGN: {name} FEHLT {detail}");
                self.missing.push(name.to_owned());
            }
        }
    }
}

/// `Ok` when the output contains `needle`, else the output.
fn shows(out: String, needle: &str) -> Result<(), String> {
    if out.contains(needle) {
        Ok(())
    } else {
        Err(out)
    }
}

/// The raw checks, each in a child of this program.
fn raw_checks(me: &Path, area: &Path, report: &mut Report) {
    for (name, what) in RAW {
        let out = Command::new(me)
            .args(["--check", name, "--area"])
            .arg(area)
            .output();
        let result = match out {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(format!(
                "({what}): {}",
                String::from_utf8_lossy(&o.stderr).trim()
            )),
            Err(e) => Err(format!("({what}): {e}")),
        };
        report.line(name, result);
    }
}

/// Wine, when the area has the component „Windows programs“ (stage P4); the kernel's
/// "noch nicht umgesetzt" lines between the two marks are what Wine asked for first.
fn windows_check(me: &Path, area: &Path) {
    let runtime = std::fs::read_to_string(area.join("etc/ultar-runtime")).unwrap_or_default();
    let windows = runtime.lines().any(|l| {
        l.strip_prefix("components=")
            .is_some_and(|c| c.split(',').any(|c| c.trim() == "windows"))
    });
    if !windows {
        return;
    }
    println!("FOREIGN-WINDOWS: start");
    let out = Command::new(me)
        .args(["--check", "windows-programs", "--area"])
        .arg(area)
        .output();
    println!("FOREIGN-WINDOWS: end");
    // reported on its own line: Wine on the Ultar kernel is the next kernel stage and does
    // not decide `FOREIGN` (the Linux programs) – docs/integration/ultar-foreign.md
    match out {
        Ok(o) if o.status.success() => println!("FOREIGN-WINDOWS: ALLES OK"),
        Ok(o) => println!(
            "FOREIGN-WINDOWS: FEHLT (Wine: wine --version, wineboot, cmd): {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => println!("FOREIGN-WINDOWS: FEHLT (Wine): {e}"),
    }
}

/// The service's cells with the area's Ubuntu programs.
fn cell_checks(helper: &Path, area: &Path, report: &mut Report) {
    let version = ["/usr/bin/dpkg-deb", "--version"];
    let inspected = cell(helper, Mode::Inspect, area, None, &version);
    report.line(
        "inspection-cell",
        inspected.and_then(|o| shows(o, "dpkg-deb")),
    );
    let writes = "touch /var/lib/dpkg/ultar-w && rm /var/lib/dpkg/ultar-w && dpkg --version";
    let installed = cell(
        helper,
        Mode::Install,
        area,
        None,
        &["/bin/sh", "-c", writes],
    );
    report.line(
        "installation-cell",
        installed.and_then(|o| shows(o, "dpkg")),
    );
    // dpkg gives files their owners (apt's `_apt` is 42): in the installation cell, which
    // keeps the right cell_chown, on the area; the set-id bits stay off
    let chown = "f=/var/lib/dpkg/ultar-chown; touch $f && chmod 4755 $f && chown 42:42 $f && \
                 s=$(stat -c '%u:%g %a' $f) && rm -f $f && echo \"chown $s\"";
    let chowned = cell(helper, Mode::Install, area, None, &["/bin/sh", "-c", chown]);
    report.line(
        "cell-chown",
        chowned.and_then(|o| shows(o, "chown 42:42 755")),
    );
}

/// What dpkg's control area looks like after dpkg-deb unpacked it (diagnostics: printed).
fn control_diagnostics(helper: &Path, area: &Path, inbox: &Path) {
    let script = "rm -rf /var/tmp/ft && mkdir -p /var/tmp/ft && cd /var/tmp/ft && \
                 dpkg-deb --control /run/inbox/package.deb ci; echo \"code $?\"; \
                 echo \"slash: $(ls ci/ | tr '\\n' ' ')\"; echo \"noslash: $(ls ci | tr '\\n' ' ')\"; \
                 echo \"lstat: $(stat -c '%F' ci/control) / $(stat -c '%F' /var/tmp/ft/ci/control)\"; \
                 echo \"glob: $(echo ci/*)\"; \
                 rm -rf /var/tmp/ft";
    let control = cell(
        helper,
        Mode::Install,
        area,
        Some(inbox.to_path_buf()),
        &["/bin/sh", "-c", script],
    );
    match &control {
        Ok(out) | Err(out) => {
            for line in out.lines().take(20) {
                println!("FOREIGN:   control {line}");
            }
        }
    }
}

/// A real package `deb`: its fields, its installation with apt and its removal.
fn package_checks(helper: &Path, area: &Path, deb: &str, report: &mut Report) {
    let inbox = std::env::temp_dir().join("ft-inbox");
    let _ = std::fs::remove_dir_all(&inbox);
    let copied = std::fs::create_dir_all(&inbox)
        .and_then(|()| std::fs::copy(deb, inbox.join("package.deb")));
    if let Err(e) = copied {
        report.line("package-fields", Err(format!("inbox: {e}")));
        return;
    }
    let target = format!("{INBOX_IN_CELL}/package.deb");
    let fields = cell(
        helper,
        Mode::Inspect,
        area,
        Some(inbox.clone()),
        &["/usr/bin/dpkg-deb", "--field", &target, "Package"],
    );
    report.line("package-fields", fields.map(|_| ()));
    control_diagnostics(helper, area, &inbox);
    let apt = cell(
        helper,
        Mode::Install,
        area,
        Some(inbox.clone()),
        &[
            "/usr/bin/apt-get",
            "-o",
            "APT::Sandbox::User=root",
            "install",
            "-y",
            "--no-install-recommends",
            &target,
        ],
    );
    report.line("package-install", apt.map(|_| ()));
    let purge = ["/usr/bin/dpkg", "--purge", "mousepad"];
    let _ = cell(helper, Mode::Install, area, None, &purge);
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let area = PathBuf::from(arg(&words, "--area").unwrap_or_else(|| "/data/foreign/linux".into()));
    if let Some(name) = arg(&words, "--check") {
        std::process::exit(match art::or_raw(&name, &area, arg(&words, "--dex"), raw) {
            Ok(()) => 0,
            Err(detail) => {
                eprintln!("{detail}");
                1
            }
        });
    }
    let helper =
        PathBuf::from(arg(&words, "--helper").unwrap_or_else(|| "/bin/ultar-foreign".into()));
    let me = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/bin/foreigntest"));
    let mut report = Report::default();
    raw_checks(&me, &area, &mut report);
    windows_check(&me, &area);
    // the Android runtime (stage P5), when the area has it
    if let Some(dex) = arg(&words, "--dex") {
        report.line("android-art", art::apart(&me, &area, &dex));
    }
    cell_checks(&helper, &area, &mut report);
    if let Some(deb) = arg(&words, "--deb") {
        package_checks(&helper, &area, &deb, &mut report);
    }
    if report.missing.is_empty() {
        println!("FOREIGN: ALLES OK");
    } else {
        println!(
            "FOREIGN: FEHLENDE KERN-FUNKTIONEN: {}",
            report.missing.join(", ")
        );
    }
}
