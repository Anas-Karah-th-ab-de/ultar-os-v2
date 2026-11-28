//! `foreigntest` – what the kernel under it offers the apps from other systems
//! (`tools/kern-test/foreign-e2e.sh` runs it on the Ultar kernel in QEMU; on Linux as
//! root it should say `FOREIGN: ALLES OK` but for the network barrier of stage P0).
//!
//! ```text
//! foreigntest --helper /bin/ultar-foreign --area /data/foreign/linux [--deb FILE]
//! ```
//!
//! With the area's component „Windows programs“ (`components=windows`) it also runs Wine
//! (`windows-programs`), between the lines `FOREIGN-WINDOWS: start` and `… end`.
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
const RAW: [(&str, &str); 15] = [
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
        "root-writes-area" | "chown-outside-cell" => {
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
        "windows-programs" => windows_programs(area, private),
        _ => Err("unknown check".into()),
    }
}

/// Wine of the area's component „Windows programs“ under the Linux ABI: `wine --version`, a
/// new prefix (`wineboot -i`, which starts `wineserver`) and a Windows program (`cmd`). In a
/// private mount namespace with the area as root, its own `/proc`, `/tmp` and device nodes
/// (like the cells, without the network namespace the Ultar kernel lacks).
fn windows_programs(area: &Path, private: impl Fn() -> Result<(), String>) -> Result<(), String> {
    let errno = |what: &'static str| move |e: ultar_sys::Errno| format!("{what} {e:?}");
    private()?;
    sys_cell::mount_proc(&area.join("proc")).map_err(errno("proc"))?;
    sys_cell::mount_tmpfs(&area.join("tmp"), "size=805306368,mode=1777").map_err(errno("tmpfs"))?;
    let dev = area.join("dev");
    sys_cell::mount_tmpfs(&dev, "size=65536,mode=0755").map_err(errno("dev"))?;
    for node in sys_cell::DEV_NODES {
        let at = dev.join(node);
        std::fs::File::create(&at).map_err(|e| e.to_string())?;
        if sys_cell::bind(&Path::new(sys_cell::DEV).join(node), &at, BindMode::Device).is_err() {
            // the Ultar kernel cannot bind a single node yet: its gated device tree
            sys_cell::detach(&dev).map_err(errno("dev detach"))?;
            sys_cell::mount_devtmpfs(&dev).map_err(errno("devtmpfs"))?;
            break;
        }
    }
    std::os::unix::fs::chroot(area).map_err(|e| format!("chroot {e}"))?;
    std::env::set_current_dir("/").map_err(|e| e.to_string())?;
    let mut steps: Vec<(&[&str], &str, u64)> = vec![
        (&["/usr/bin/wine", "--version"] as &[&str], "wine-", 300),
        (&["/usr/bin/wineboot", "-i"] as &[&str], "", 1500),
        (
            &["/usr/bin/wine", "cmd", "/c", "echo", "ultar-wine"] as &[&str],
            "ultar-wine",
            900,
        ),
    ];
    // a real program when the test put one into the area (foreign-e2e.sh WIN_MSI=…): 7-Zip's MSI
    if Path::new("/opt/ultar-test/program.msi").exists() {
        steps.push((
            &[
                "/usr/bin/wine",
                "msiexec",
                "/i",
                "Z:\\opt\\ultar-test\\program.msi",
                "/qn",
            ],
            "",
            1500,
        ));
        steps.push((
            &["/usr/bin/wine", "C:\\Program Files\\7-Zip\\7z.exe", "i"],
            "7-Zip",
            600,
        ));
    }
    // every step runs (a prefix that stops half-way can still run a 64-bit program); each says how it went
    let mut failed: Vec<String> = Vec::new();
    for (program, expect, secs) in steps {
        let why = match run_for(program, secs) {
            Ok(out) if out.contains(expect) => None,
            Ok(out) => Some(tail(&out)),
            Err(e) => Some(e),
        };
        println!(
            "FOREIGN-WINDOWS: {} {}",
            program.join(" "),
            if why.is_none() { "ok" } else { "FEHLT" }
        );
        if let Some(why) = why {
            failed.push(format!("{}: {why}", program.join(" ")));
        }
    }
    let result = if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join(" || "))
    };
    // Ubuntu keeps the server out of PATH
    if let Some(server) = [
        "/usr/lib/x86_64-linux-gnu/wine/wineserver",
        "/usr/bin/wineserver",
    ]
    .into_iter()
    .find(|s| Path::new(s).exists())
    {
        let _ = run_for(&[server, "-k"], 60);
    }
    // how far the new prefix got
    result.map_err(|e| {
        let made: Vec<String> = ["system.reg", "drive_c/windows/system32/kernel32.dll"]
            .iter()
            .filter(|p| Path::new("/tmp/wine").join(p).exists())
            .map(|p| (*p).to_owned())
            .collect();
        format!(
            "{e} [prefix: {}]",
            if made.is_empty() {
                "-".into()
            } else {
                made.join(", ")
            }
        )
    })
}

/// Runs a program of the area for at most `secs`; its output (stdout and stderr) when it ends
/// with 0.
fn run_for(program: &[&str], secs: u64) -> Result<String, String> {
    use std::io::Read;
    let mut child = Command::new(program[0])
        .args(&program[1..])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/tmp")
        // no TMPDIR: Ubuntu's Wine 10 server breaks with it (as in ultar-wine-app)
        .env("WINEPREFIX", "/tmp/wine")
        // Wine's own errors, not its notes about what it does not implement
        .env("WINEDEBUG", "err+all,warn-all,fixme-all")
        .env("WINEDLLOVERRIDES", "mscoree,mshtml=")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = out.read_to_string(&mut text);
        let mut more = String::new();
        let _ = err.read_to_string(&mut more);
        text + &more
    });
    let end = std::time::Instant::now() + Duration::from_secs(secs);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break Some(status);
        }
        if std::time::Instant::now() > end {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let text = if status.is_some() {
        reader.join().unwrap_or_default()
    } else {
        String::new()
    };
    match status {
        Some(s) if s.success() => Ok(text),
        Some(s) => Err(format!("exit {:?}: {}", s.code(), tail(&text))),
        None => Err(format!("still running after {secs} s")),
    }
}

/// The last lines of a program's output, short enough for one report line.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut t = lines[lines.len().saturating_sub(3)..].join(" | ");
    if t.len() > 300 {
        let mut cut = t.len() - 300;
        while !t.is_char_boundary(cut) {
            cut += 1;
        }
        t = t[cut..].to_owned();
    }
    t
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

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let area = PathBuf::from(arg(&words, "--area").unwrap_or_else(|| "/data/foreign/linux".into()));
    if let Some(name) = arg(&words, "--check") {
        std::process::exit(match raw(&name, &area) {
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
    // Wine, when the area has the component „Windows programs“ (stage P4); the kernel's
    // "noch nicht umgesetzt" lines between the two marks are what Wine asked for first
    let runtime = std::fs::read_to_string(area.join("etc/ultar-runtime")).unwrap_or_default();
    if runtime.lines().any(|l| {
        l.strip_prefix("components=")
            .is_some_and(|c| c.split(',').any(|c| c.trim() == "windows"))
    }) {
        println!("FOREIGN-WINDOWS: start");
        let out = Command::new(&me)
            .args(["--check", "windows-programs", "--area"])
            .arg(&area)
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
    // dpkg gives files their owners (apt's `_apt` is 42): in the installation cell, which
    // keeps the right cell_chown, on the area; the set-id bits stay off
    let chowned = cell(
        &helper,
        Mode::Install,
        &area,
        None,
        &[
            "/bin/sh",
            "-c",
            "f=/var/lib/dpkg/ultar-chown; touch $f && chmod 4755 $f && chown 42:42 $f && \
             s=$(stat -c '%u:%g %a' $f) && rm -f $f && echo \"chown $s\"",
        ],
    );
    report(
        "cell-chown",
        chowned.and_then(|o| {
            if o.contains("chown 42:42 755") {
                Ok(())
            } else {
                Err(o)
            }
        }),
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
        // what dpkg's control area looks like after dpkg-deb unpacked it (diagnostics: printed)
        let control = cell(
            &helper,
            Mode::Install,
            &area,
            Some(inbox.clone()),
            &[
                "/bin/sh",
                "-c",
                "rm -rf /var/tmp/ft && mkdir -p /var/tmp/ft && cd /var/tmp/ft && \
                 dpkg-deb --control /run/inbox/package.deb ci; echo \"code $?\"; \
                 echo \"slash: $(ls ci/ | tr '\\n' ' ')\"; echo \"noslash: $(ls ci | tr '\\n' ' ')\"; \
                 echo \"lstat: $(stat -c '%F' ci/control) / $(stat -c '%F' /var/tmp/ft/ci/control)\"; \
                 echo \"glob: $(echo ci/*)\"; \
                 rm -rf /var/tmp/ft",
            ],
        );
        match &control {
            Ok(out) | Err(out) => {
                for line in out.lines().take(20) {
                    println!("FOREIGN:   control {line}");
                }
            }
        }
        let apt = cell(
            &helper,
            Mode::Install,
            &area,
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
