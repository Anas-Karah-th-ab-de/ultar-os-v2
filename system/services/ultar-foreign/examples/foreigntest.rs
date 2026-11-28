//! `foreigntest` – what the kernel under it offers the apps from other systems
//! (`tools/kern-test/foreign-e2e.sh` runs it on the Ultar kernel in QEMU; on Linux as
//! root it should say `FOREIGN: ALLES OK` but for the network barrier of stage P0).
//!
//! ```text
//! foreigntest --helper /bin/ultar-foreign --area /data/foreign/linux [--deb FILE] [--dex FILE]
//! ```
//!
//! Each kernel feature is tried in its own child process; the cells are the service's
//! own (`cellrun`), with the Ubuntu programs of the area. Every line is
//! `FOREIGN: <check> ok` or `FOREIGN: <check> FEHLT <detail>`; the last line is
//! `FOREIGN: ALLES OK` or `FOREIGN: FEHLENDE KERN-FUNKTIONEN: <list>`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use ultar_foreign::cellrun::{self, CellJob, Mode, SetupStep, INBOX_IN_CELL};
use ultar_sys::cell::{self as sys_cell, BindMode, Namespaces};

/// The raw checks, each in a child (`--check NAME`): name and what it needs.
const RAW: [(&str, &str); 14] = [
    ("namespaces", "mount and pid namespaces"),
    (
        "network-namespace",
        "a cell without network (own network namespace)",
    ),
    (
        "file-bind",
        "binding a single device node (/dev/null into the cell)",
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
        "root-chown",
        "root of the cell without rights gives files to another owner (dpkg, apt's _apt)",
    ),
    ("tmpfs", "tmpfs for /run and /tmp"),
    ("proc-mount", "an own /proc"),
    ("pivot-root", "the root switch into the cell's view"),
    (
        "ubuntu-programs",
        "the area's Ubuntu programs (glibc, ld.so) under the Linux ABI",
    ),
];

fn raw(name: &str, area: &Path) -> Result<(), String> {
    let errno = |e: ultar_sys::Errno| format!("{e:?}");
    let private = || -> Result<(), String> {
        sys_cell::unshare(Namespaces {
            mount: true,
            ..Namespaces::default()
        })
        .map_err(errno)?;
        sys_cell::make_mounts_private().map_err(errno)
    };
    let scratch = std::env::temp_dir().join(format!("ft-{name}"));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
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
        "file-bind" => {
            private()?;
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
            private()?;
            sys_cell::bind(&scratch, &scratch, BindMode::ReadOnly).map_err(errno)?;
            match std::fs::write(scratch.join("x"), b"x") {
                Ok(()) => Err("written through the read-only bind".into()),
                Err(_) => Ok(()),
            }
        }
        "remount-nosuid" => {
            use rustix::mount::{mount_bind, mount_remount, MountFlags};
            private()?;
            mount_bind(&scratch, &scratch).map_err(|e| format!("bind {e:?}"))?;
            mount_remount(
                &scratch,
                MountFlags::BIND | MountFlags::NOSUID | MountFlags::NODEV,
                "",
            )
            .map_err(|e| format!("remount {e:?}"))
        }
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
        "root-writes-area" | "root-chown" => {
            let file = area.join("var/lib/dpkg").join(format!("ultar-{name}"));
            std::fs::write(&file, b"x").map_err(|e| format!("prepare {e}"))?;
            if ultar_sys::ultar_rights().is_ok() {
                ultar_sys::ultar_drop_rights(u64::MAX).map_err(errno)?;
            }
            let result = if name == "root-writes-area" {
                std::fs::write(&file, b"y").map_err(|e| e.to_string())
            } else {
                rustix::fs::chown(&file, Some(rustix::fs::Uid::from_raw(42)), None)
                    .map_err(|e| format!("{e:?}"))
            };
            let _ = std::fs::remove_file(&file);
            result
        }
        "tmpfs" => {
            private()?;
            sys_cell::mount_tmpfs(&scratch, "size=65536,mode=0755").map_err(errno)
        }
        "proc-mount" => {
            private()?;
            sys_cell::mount_proc(&scratch).map_err(errno)
        }
        "pivot-root" => {
            private()?;
            let new = scratch.join("new");
            std::fs::create_dir_all(new.join("old")).map_err(|e| e.to_string())?;
            sys_cell::bind(&new, &new, BindMode::ReadOnly).map_err(|e| format!("bind {e:?}"))?;
            sys_cell::pivot_root(&new, &new.join("old")).map_err(|e| format!("pivot {e:?}"))?;
            sys_cell::detach(Path::new("/old")).map_err(|e| format!("detach {e:?}"))
        }
        "ubuntu-programs" => {
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
                    // the area has no /dev (the cells bring their own): no /dev/null here
                    .stdin(std::process::Stdio::piped())
                    .output()
                    .map_err(|e| format!("{}: {e}", program[0]))?;
                if !out.status.success() {
                    return Err(format!("{}: {:?}", program[0], out.status.code()));
                }
            }
            Ok(())
        }
        _ => Err("unknown check".into()),
    }
}

/// The Android runtime of the area (component "Android apps", stage P5): ART runs a
/// program of its own (`hello.dex` of `make-android-runtime.sh`), chrooted into the area as
/// root – what ART needs of the kernel, before any cell.
fn art(area: &Path, dex: &Path) -> Result<(), String> {
    let work = area.join("tmp/ultar-art");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(work.join("dalvik-cache")).map_err(|e| format!("mkdir {e}"))?;
    std::fs::copy(dex, work.join("hello.dex")).map_err(|e| format!("copy {e}"))?;
    std::os::unix::fs::chroot(area).map_err(|e| format!("chroot {e}"))?;
    std::env::set_current_dir("/").map_err(|e| e.to_string())?;
    let run = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("ANDROID_DATA", "/tmp/ultar-art")
            .stdin(std::process::Stdio::piped())
            .output()
    };
    let proc_mounted =
        run("/usr/bin/mount", &["-t", "proc", "proc", "/proc"]).is_ok_and(|o| o.status.success());
    let out = run(
        "/usr/bin/dalvikvm",
        &["-cp", "/tmp/ultar-art/hello.dex", "Hello"],
    );
    if proc_mounted {
        let _ = run("/usr/bin/umount", &["/proc"]);
    }
    let _ = std::fs::remove_dir_all("/tmp/ultar-art");
    let out = out.map_err(|e| format!("dalvikvm: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    if text.lines().any(|l| l == "ultar-art-ok 42") {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let last: Vec<&str> = err.lines().rev().take(4).collect();
    Err(format!(
        "(ART runs a program: dalvikvm {:?}, /proc {}) {}",
        out.status.code(),
        if proc_mounted {
            "mounted"
        } else {
            "NOT mounted"
        },
        last.into_iter().rev().collect::<Vec<_>>().join(" | ")
    ))
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
            None => Err(format!("exit {code}: {}", out.lines().last().unwrap_or(""))),
        },
        None => Err("killed or timed out".into()),
    }
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let area = PathBuf::from(arg(&words, "--area").unwrap_or_else(|| "/data/foreign/linux".into()));
    if let Some(name) = arg(&words, "--check") {
        let result = match (name.as_str(), arg(&words, "--dex")) {
            ("android-art", Some(dex)) => art(&area, Path::new(&dex)),
            _ => raw(&name, &area),
        };
        std::process::exit(match result {
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
    let mut missing: Vec<String> = Vec::new();
    let mut report = |name: &str, result: Result<(), String>| match result {
        Ok(()) => println!("FOREIGN: {name} ok"),
        Err(detail) => {
            println!("FOREIGN: {name} FEHLT {detail}");
            missing.push(name.to_owned());
        }
    };
    for (name, what) in RAW {
        let out = Command::new(&me)
            .args(["--check", name, "--area"])
            .arg(&area)
            .output();
        let result = match out {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(format!(
                "({what}): {}",
                String::from_utf8_lossy(&o.stderr).trim()
            )),
            Err(e) => Err(format!("({what}): {e}")),
        };
        report(name, result);
    }
    // the Android runtime (stage P5), when the area has it
    if let Some(dex) = arg(&words, "--dex") {
        let out = Command::new(&me)
            .args(["--check", "android-art", "--area"])
            .arg(&area)
            .args(["--dex", &dex])
            .output();
        report(
            "android-art",
            match out {
                Ok(o) if o.status.success() => Ok(()),
                Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_owned()),
                Err(e) => Err(e.to_string()),
            },
        );
    }
    // the kernel's barrier "internet only" (stage P0) has no interface yet
    report(
        "network-barrier",
        Err("(internet only for an app with network: the kernel call of stage P0 does not exist yet)".into()),
    );
    // the service's cells with the area's Ubuntu programs
    let inspected = cell(
        &helper,
        Mode::Inspect,
        &area,
        None,
        &["/usr/bin/dpkg-deb", "--version"],
    );
    report(
        "inspection-cell",
        inspected.and_then(|o| {
            if o.contains("dpkg-deb") {
                Ok(())
            } else {
                Err(o)
            }
        }),
    );
    let installed = cell(
        &helper,
        Mode::Install,
        &area,
        None,
        &[
            "/bin/sh",
            "-c",
            "touch /var/lib/dpkg/ultar-w && rm /var/lib/dpkg/ultar-w && dpkg --version",
        ],
    );
    report(
        "installation-cell",
        installed.and_then(|o| if o.contains("dpkg") { Ok(()) } else { Err(o) }),
    );
    if let Some(deb) = arg(&words, "--deb") {
        let inbox = std::env::temp_dir().join("ft-inbox");
        let _ = std::fs::remove_dir_all(&inbox);
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::copy(&deb, inbox.join("package.deb")).unwrap();
        let target = format!("{INBOX_IN_CELL}/package.deb");
        let fields = cell(
            &helper,
            Mode::Inspect,
            &area,
            Some(inbox.clone()),
            &["/usr/bin/dpkg-deb", "--field", &target, "Package"],
        );
        report("package-fields", fields.map(|_| ()));
        let apt = cell(
            &helper,
            Mode::Install,
            &area,
            Some(inbox.clone()),
            &[
                "/usr/bin/apt-get",
                "install",
                "-y",
                "--no-install-recommends",
                &target,
            ],
        );
        report("package-install", apt.map(|_| ()));
        let _ = cell(
            &helper,
            Mode::Install,
            &area,
            None,
            &["/usr/bin/dpkg", "--purge", "mousepad"],
        );
    }
    if missing.is_empty() {
        println!("FOREIGN: ALLES OK");
    } else {
        println!("FOREIGN: FEHLENDE KERN-FUNKTIONEN: {}", missing.join(", "));
    }
}
