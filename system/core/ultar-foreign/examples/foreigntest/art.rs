//! The Android runtime of the area (component "Android apps", stage P5) under the kernel:
//! ART runs a program of its own (`hello.dex` of `make-android-runtime.sh`), chrooted into the
//! area as root – what ART needs of the kernel, before any cell. ART's data (`ANDROID_DATA`) is
//! a tmpfs: the check asks the kernel for ART, writing the area's disk is `root-writes-area`.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DATA: &str = "/tmp/ultar-art";
const HELLO: [&str; 3] = ["-cp", "/tmp/ultar-art/hello.dex", "Hello"];

fn command(program: &str, args: &[&str]) -> Command {
    let mut c = Command::new(program);
    c.args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("ANDROID_DATA", DATA)
        .stdin(Stdio::null());
    c
}

/// A program of the area, stopped after `secs` (ART can hang on a kernel): how it ended, its
/// output and its errors (through files on the tmpfs).
fn run_limited(program: &str, args: &[&str], secs: u64) -> (String, String, String) {
    let (out, err) = (format!("{DATA}/out"), format!("{DATA}/err"));
    let (Ok(o), Ok(e)) = (std::fs::File::create(&out), std::fs::File::create(&err)) else {
        return ("no output files".into(), String::new(), String::new());
    };
    let mut child = match command(program, args).stdout(o).stderr(e).spawn() {
        Ok(c) => c,
        Err(e) => return (format!("start: {e}"), String::new(), String::new()),
    };
    let end = Instant::now() + Duration::from_secs(secs);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break format!("exit {:?}", s.code()),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(200)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break format!("still running after {secs} s");
            }
            Err(e) => break format!("wait: {e}"),
        }
    };
    let read = |f: &str| std::fs::read_to_string(f).unwrap_or_default();
    (status, read(&out), read(&err))
}

fn last_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().rev().take(3).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join(" | ")
}

fn done(out: &str) -> bool {
    out.lines().any(|l| l == "ultar-art-ok 42")
}

/// Runs `hello.dex` with ART; on failure what went wrong, and whether ART gets further when
/// the loader is named the program itself (glibc's `$ORIGIN` needs the kernel's view of it).
fn hello(proc_mounted: bool, tmpfs: bool) -> Result<(), String> {
    let (end, out, err) = run_limited("/usr/bin/dalvikvm", &HELLO, 300);
    if done(&out) {
        return Ok(());
    }
    let mut through_loader = vec!["/usr/bin/dalvikvm"];
    through_loader.extend(HELLO);
    let (l_end, l_out, l_err) = run_limited("/lib64/ld-linux-x86-64.so.2", &through_loader, 300);
    let loader = if done(&l_out) {
        "ok".to_owned()
    } else {
        format!("{l_end}, {}", last_lines(&l_err))
    };
    Err(format!(
        "(ART runs a program: dalvikvm {end}, proc {}, data {}; through ld.so: {loader}) {}",
        if proc_mounted {
            "mounted"
        } else {
            "NOT mounted"
        },
        if tmpfs { "tmpfs" } else { "on the area's disk" },
        last_lines(&err)
    ))
}

/// `--check NAME`: the ART check with `--dex`, otherwise the other checks (`raw`).
pub fn or_raw(
    name: &str,
    area: &Path,
    dex: Option<String>,
    raw: fn(&str, &Path) -> Result<(), String>,
) -> Result<(), String> {
    match (name, dex) {
        ("android-art", Some(dex)) => art(area, Path::new(&dex)),
        _ => raw(name, area),
    }
}

/// The ART check in a process of its own (it changes its root): `me --check android-art`.
pub fn apart(me: &Path, area: &Path, dex: &str) -> Result<(), String> {
    let out = Command::new(me)
        .args(["--check", "android-art", "--area"])
        .arg(area)
        .args(["--dex", dex])
        .output();
    match out {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_owned()),
        Err(e) => Err(e.to_string()),
    }
}

fn art(area: &Path, dex: &Path) -> Result<(), String> {
    let code = std::fs::read(dex).map_err(|e| format!("read {e}"))?;
    std::fs::create_dir_all(area.join("tmp/ultar-art")).map_err(|e| format!("mkdir {e}"))?;
    std::os::unix::fs::chroot(area).map_err(|e| format!("chroot {e}"))?;
    std::env::set_current_dir("/").map_err(|e| e.to_string())?;
    let mount = |args: &[&str]| {
        command("/usr/bin/mount", args)
            .status()
            .is_ok_and(|s| s.success())
    };
    let tmpfs = mount(&["-t", "tmpfs", "-o", "size=256m", "tmpfs", DATA]);
    std::fs::create_dir_all(format!("{DATA}/dalvik-cache")).map_err(|e| format!("mkdir {e}"))?;
    std::fs::write(format!("{DATA}/hello.dex"), &code).map_err(|e| format!("write {e}"))?;
    let proc_mounted = mount(&["-t", "proc", "proc", "/proc"]);
    let result = hello(proc_mounted, tmpfs);
    if proc_mounted {
        let _ = command("/usr/bin/umount", &["/proc"]).status();
    }
    if tmpfs {
        let _ = command("/usr/bin/umount", &[DATA]).status();
    }
    let _ = std::fs::remove_dir_all(DATA);
    result
}
