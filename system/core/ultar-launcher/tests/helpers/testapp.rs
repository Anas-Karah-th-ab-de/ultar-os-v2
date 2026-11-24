//! Test app of the launcher's process tests and of the QEMU run (not
//! shipped). It behaves well (`info`, `run`, `exit N`, `crash`) or attacks
//! its cell (`attack …`) and prints one line per attempt.
//!
//! ```text
//! ultar-launcher-testapp info                  print who and where it is, then end
//! ultar-launcher-testapp run                   print "TESTAPP running", then wait to be stopped
//! ultar-launcher-testapp exit N | crash        end with code N | by SIGABRT
//! ultar-launcher-testapp attack [--victim-data DIR] [--victim-pid N] [--socket PATH]...
//!                               [--port N] [--memory] [--forks N]
//! ```

#![forbid(unsafe_code)]

use std::io::Write;
use std::time::Duration;

fn out(line: &str) {
    let mut stdout = std::io::stdout();
    let _ = writeln!(stdout, "{line}");
    let _ = stdout.flush();
}

fn info() {
    let rights = ultar_sys::ultar_rights().map_or("none".to_owned(), |r| format!("{r:x}"));
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let data_ok =
        std::fs::write("data-probe", b"x").is_ok() && std::fs::remove_file("data-probe").is_ok();
    let tmp = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".into());
    let tmp_ok = std::fs::write(format!("{tmp}/probe"), b"x").is_ok();
    let root_ro = std::fs::write("/probe", b"x").is_err();
    out(&format!(
        "TESTAPP uid={} gid={} pid={} rights={rights} cwd={cwd} data_writable={data_ok} tmp_writable={tmp_ok} root_readonly={root_ro} token={} app={} uri=[{}] args=[{}]",
        ultar_sys::uid(),
        ultar_sys::gid(),
        std::process::id(),
        std::env::var("ULTAR_ACTIVATION_TOKEN").unwrap_or_default(),
        std::env::var("ULTAR_APP_ID").unwrap_or_default(),
        std::env::var("ULTAR_OPEN_URI").unwrap_or_default(),
        std::env::args().skip(2).collect::<Vec<_>>().join(" "),
    ));
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "TESTAPP stderr line");
}

fn run() {
    out("TESTAPP running");
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

mod attack;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("info") => info(),
        Some("run") => run(),
        Some("exit") => std::process::exit(args.get(1).and_then(|n| n.parse().ok()).unwrap_or(1)),
        Some("crash") => std::process::abort(),
        Some("attack") => std::process::exit(attack::main(&args[1..])),
        Some("memhog") => attack::memhog(),
        Some("child") => std::thread::sleep(Duration::from_secs(60)),
        _ => std::process::exit(2),
    }
}
