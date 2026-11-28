//! Starts the chain on the Ultar kernel, where no shell runs programs side by
//! side: `ultar-compositor`, then `ultar-wayland` with `uwl-demo`. Prints
//! `uwl-kern: started` and keeps the compositor running for the screenshot.
//!
//!     uwl-kern [bin-dir]   (default /bin)

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

const SOCKET: &str = "/tmp/uwl/compositor";
const RUN: &str = "/tmp/uwl/run";

/// Fork + exec: the Ultar kernel releases a vfork parent only when the child
/// exits (kernel gap K10); setting the unchanged uid makes std fork.
fn spawn(program: &Path, args: &[&str]) -> std::io::Result<Child> {
    Command::new(program)
        .args(args)
        .uid(rustix::process::getuid().as_raw())
        .spawn()
}

fn main() {
    let bin = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "/bin".into()));
    let _ = std::fs::create_dir_all(RUN);
    let comp_args = [
        "--socket",
        SOCKET,
        "--allow-root",
        "--lang",
        "en",
        "--no-cursor",
    ];
    let Ok(_compositor) = spawn(&bin.join("ultar-compositor"), &comp_args) else {
        println!("uwl-kern: compositor did not start");
        std::process::exit(1);
    };
    for _ in 0..300 {
        if Path::new(SOCKET).exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let demo = bin.join("uwl-demo").display().to_string();
    let bridge_args = [
        "--compositor",
        SOCKET,
        "--runtime-dir",
        RUN,
        "--app-id",
        "org.ultar.uwl-demo",
        "--",
        &demo,
        "640",
        "400",
    ];
    match spawn(&bin.join("ultar-wayland"), &bridge_args) {
        Ok(mut bridge) => {
            println!("uwl-kern: started");
            let status = bridge.wait();
            println!("uwl-kern: bridge ended: {status:?}");
        }
        Err(e) => println!("uwl-kern: bridge did not start: {e}"),
    }
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
