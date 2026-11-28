//! The check `windows-programs` of `foreigntest`: Wine of the area's component „Windows
//! programs“.

use super::private;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use ultar_sys::cell::{self as sys_cell, BindMode};

/// Wine of the area's component „Windows programs“ under the Linux ABI: `wine --version`, a
/// new prefix (`wineboot -i`, which starts `wineserver`) and a Windows program (`cmd`). In a
/// private mount namespace with the area as root, its own `/proc`, `/tmp` and device nodes
/// (like the cells, without the network namespace the Ultar kernel lacks).
pub fn windows_programs(area: &Path) -> Result<(), String> {
    private()?;
    wine_mounts(area)?;
    std::os::unix::fs::chroot(area).map_err(|e| format!("chroot {e}"))?;
    std::env::set_current_dir("/").map_err(|e| e.to_string())?;
    let steps: [(&[&str], &str, u64); 3] = [
        (&["/usr/bin/wine", "--version"], "wine-", 300),
        (&["/usr/bin/wineboot", "-i"], "", 1500),
        (
            &["/usr/bin/wine", "cmd", "/c", "echo", "ultar-wine"],
            "ultar-wine",
            900,
        ),
    ];
    let mut result = Ok(());
    for (program, expect, secs) in steps {
        match run_for(program, secs) {
            Ok(out) if out.contains(expect) => {}
            Ok(out) => {
                result = Err(format!("{}: {}", program.join(" "), tail(&out)));
                break;
            }
            Err(e) => {
                result = Err(format!("{}: {e}", program.join(" ")));
                break;
            }
        }
    }
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
    result.map_err(with_prefix)
}

/// The area's own `/proc`, `/tmp` and device nodes for Wine.
fn wine_mounts(area: &Path) -> Result<(), String> {
    let errno = |what: &'static str| move |e: ultar_sys::Errno| format!("{what} {e:?}");
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
    Ok(())
}

/// `e` with how far the new prefix got.
fn with_prefix(e: String) -> String {
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
    let (Some(mut out), Some(mut err)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("no output pipes".into());
    };
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
