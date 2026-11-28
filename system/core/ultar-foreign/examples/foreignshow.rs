//! `foreignshow` – a GTK program of the Linux area on the screen of the Ultar kernel:
//! `ultar-compositor`, then `ultar-wayland` with the program, which runs with the area as its
//! root (own mount namespace, the harmless device nodes bound one by one, own `/proc`). Used by
//! `tools/kern-test/foreign-gtk-kern.sh` for a screenshot; no shell is needed on the kernel.
//!
//! ```text
//! foreignshow [--bin /bin] [--area /data/foreign/linux] -- PROGRAM [ARGS…]
//! foreignshow --child AREA PROGRAM [ARGS…]      (started by ultar-wayland)
//! ```
//!
//! Not a cell of the launcher (no own user, no limits): it shows what the area's programs need
//! of the kernel and the bridge. Prints `foreignshow: started` once the bridge runs.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;
use ultar_sys::cell::{self as sys_cell, BindMode, Namespaces};

const SOCKET: &str = "/tmp/fshow/compositor";
/// The bridge's directory, inside the area (the program sees it as `/tmp/.run`).
const RUN_IN_AREA: &str = "tmp/.run";

/// Fork + exec (the Ultar kernel releases a vfork parent only when the child exits).
fn spawn(program: &Path, args: &[String]) -> std::io::Result<Child> {
    Command::new(program)
        .args(args)
        .uid(rustix::process::getuid().as_raw())
        .spawn()
}

fn child(area: &Path, program: &[String]) -> ! {
    let fail = |what: &str, e: String| -> ! {
        eprintln!("foreignshow: child: {what}: {e}");
        std::process::exit(120);
    };
    sys_cell::unshare(Namespaces {
        mount: true,
        ..Namespaces::default()
    })
    .unwrap_or_else(|e| fail("unshare", format!("{e:?}")));
    sys_cell::make_mounts_private().unwrap_or_else(|e| fail("private", format!("{e:?}")));
    for node in sys_cell::DEV_NODES.iter().chain(["tty"].iter()) {
        let source = Path::new(sys_cell::DEV).join(node);
        if std::fs::symlink_metadata(&source).is_err() {
            continue;
        }
        let at = area.join("dev").join(node);
        let _ = std::fs::File::create(&at);
        if let Err(e) = sys_cell::bind(&source, &at, BindMode::Device) {
            eprintln!("foreignshow: child: bind {}: {e:?}", source.display());
        }
    }
    if let Err(e) = sys_cell::mount_proc(&area.join("proc")) {
        eprintln!("foreignshow: child: proc: {e:?}");
    }
    std::os::unix::fs::chroot(area).unwrap_or_else(|e| fail("chroot", e.to_string()));
    std::env::set_current_dir("/").unwrap_or_else(|e| fail("chdir", e.to_string()));
    let e = Command::new(&program[0])
        .args(&program[1..])
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", "/tmp")
        .env("XDG_RUNTIME_DIR", "/tmp/.run")
        .env("WAYLAND_DISPLAY", "wayland-0")
        .env("GDK_BACKEND", "wayland")
        .env("GSK_RENDERER", "cairo")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("NO_AT_BRIDGE", "1")
        .env("GTK_A11Y", "none")
        .env("LANG", "en_US.UTF-8")
        .env("LC_ALL", "en_US.UTF-8")
        .exec();
    fail("exec", e.to_string())
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    if words.first().map(String::as_str) == Some("--child") {
        let area = PathBuf::from(&words[1]);
        child(&area, &words[2..]);
    }
    let value = |name: &str, default: &str| {
        words
            .iter()
            .position(|w| w == name)
            .and_then(|i| words.get(i + 1))
            .cloned()
            .unwrap_or_else(|| default.to_owned())
    };
    let bin = PathBuf::from(value("--bin", "/bin"));
    let area = PathBuf::from(value("--area", "/data/foreign/linux"));
    let Some(dash) = words.iter().position(|w| w == "--") else {
        println!("foreignshow: usage: foreignshow [--bin DIR] [--area DIR] -- PROGRAM [ARGS]");
        std::process::exit(2);
    };
    let program = &words[dash + 1..];
    let run = area.join(RUN_IN_AREA);
    let _ = std::fs::create_dir_all(&run);
    start_compositor(&bin);
    let me = std::env::current_exe().unwrap_or_else(|_| bin.join("foreignshow"));
    let mut bridge_args = bridge_args(&run, &me, &area);
    bridge_args.extend(program.iter().cloned());
    match spawn(&bin.join("ultar-wayland"), &bridge_args) {
        Ok(mut bridge) => {
            println!("foreignshow: started");
            let status = bridge.wait();
            println!("foreignshow: bridge ended: {status:?}");
        }
        Err(e) => println!("foreignshow: bridge did not start: {e}"),
    }
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

/// Starts `ultar-compositor` and waits (up to 30 s) for its socket.
fn start_compositor(bin: &Path) {
    if let Some(dir) = Path::new(SOCKET).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let comp: Vec<String> = [
        "--socket",
        SOCKET,
        "--allow-root",
        "--lang",
        "en",
        "--no-cursor",
    ]
    .map(str::to_owned)
    .to_vec();
    // the compositor runs as long as this program (its child is never waited for)
    if spawn(&bin.join("ultar-compositor"), &comp).is_err() {
        println!("foreignshow: compositor did not start");
        std::process::exit(1);
    }
    for _ in 0..300 {
        if Path::new(SOCKET).exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The bridge's words up to the program: its runtime directory `run` and this program
/// `me` as the child in `area`.
fn bridge_args(run: &Path, me: &Path, area: &Path) -> Vec<String> {
    [
        "--compositor",
        SOCKET,
        "--runtime-dir",
        &run.display().to_string(),
        "--app-id",
        "org.ultar.foreign-show",
        "--",
        &me.display().to_string(),
        "--child",
        &area.display().to_string(),
    ]
    .map(|s| s.to_owned())
    .to_vec()
}
