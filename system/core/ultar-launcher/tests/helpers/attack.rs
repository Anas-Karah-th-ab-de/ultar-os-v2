//! The attacks of the test app: each tries to leave its cell and reports
//! `ATTACK <name> blocked` or `ATTACK <name> ESCAPED <what>`.

use std::io::Read;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use ultar_sys::cell::{self, Namespaces, StopSignal};

/// Memory the hog tries to touch before it says it escaped.
const HOG_LIMIT: usize = 2 << 30;

struct Report {
    escaped: usize,
}

impl Report {
    fn check(&mut self, name: &str, blocked: bool, what: &str) {
        if blocked {
            super::out(&format!("ATTACK {name} blocked"));
        } else {
            self.escaped += 1;
            super::out(&format!("ATTACK {name} ESCAPED {what}"));
        }
    }
}

fn read_other(report: &mut Report, dir: &str) {
    let listed = std::fs::read_dir(dir).is_ok();
    let mut secret = String::new();
    let read = std::fs::File::open(format!("{dir}/secret"))
        .and_then(|mut f| f.read_to_string(&mut secret))
        .is_ok();
    let climbed = std::fs::read_dir(format!("/data/../../..{dir}")).is_ok();
    report.check("read_other_data", !listed && !read && !climbed, dir);
}

fn kill_other(report: &mut Report, pid: u32) {
    let term = cell::send_signal(pid, StopSignal::Terminate).is_err();
    let kill = cell::send_signal(pid, StopSignal::Kill).is_err();
    report.check("kill_other_app", term && kill, &pid.to_string());
}

fn escalate(report: &mut Report) {
    let root = ultar_sys::Identity {
        uid: 0,
        gid: 0,
        groups: Vec::new(),
    };
    report.check(
        "become_root",
        ultar_sys::switch_identity(&root).is_err(),
        "setuid(0)",
    );
    let rights = ultar_sys::ultar_rights().map_or(true, |r| r == 0);
    report.check("kernel_rights", rights, "rights");
    let ns = Namespaces {
        mount: true,
        ..Namespaces::default()
    };
    report.check("new_namespace", cell::unshare(ns).is_err(), "unshare");
    let mounted = cell::mount_tmpfs(std::path::Path::new("/tmp"), "size=4096").is_ok();
    report.check("mount", !mounted, "mount");
    let wrote_system =
        std::fs::write("/usr/escape", b"x").is_ok() || std::fs::write("/bin/escape", b"x").is_ok();
    report.check("write_system", !wrote_system, "/usr");
    report.check("write_root", std::fs::write("/escape", b"x").is_err(), "/");
}

fn sockets(report: &mut Report, paths: &[String]) {
    for path in paths {
        report.check(
            "privileged_socket",
            UnixStream::connect(path).is_err(),
            path,
        );
    }
}

fn network(report: &mut Report, port: u16) {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let reached = std::net::TcpStream::connect_timeout(&address, Duration::from_secs(2)).is_ok();
    report.check("network", !reached, &port.to_string());
}

fn memory(report: &mut Report) {
    let me = std::env::current_exe().unwrap_or_default();
    let status = Command::new(me)
        .arg("memhog")
        .stdout(Stdio::null())
        .status();
    // ended by the kernel (signal) or refused memory (abort): blocked
    let blocked = status.as_ref().map_or(true, |s| !s.success());
    let how = status.map_or("not started".to_owned(), |s| {
        use std::os::unix::process::ExitStatusExt;
        format!("code={:?} signal={:?}", s.code(), s.signal())
    });
    super::out(&format!("MEMORY hog ended {how}"));
    report.check("fill_memory", blocked, "2 GiB");
}

/// Touches memory until refused or [`HOG_LIMIT`]; exit 0 = it got it all.
pub fn memhog() {
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let mut total = 0;
    while total < HOG_LIMIT {
        let mut chunk = vec![0u8; 16 << 20];
        for page in chunk.chunks_mut(4096) {
            page[0] = 1;
        }
        total += chunk.len();
        chunks.push(chunk);
    }
    super::out(&format!("MEMHOG {} MiB", total >> 20));
}

fn forks(report: &mut Report, wanted: usize) {
    let me = std::env::current_exe().unwrap_or_default();
    let mut children: Vec<Child> = Vec::new();
    while children.len() < wanted {
        match Command::new(&me).arg("child").stdout(Stdio::null()).spawn() {
            Ok(child) => children.push(child),
            Err(_) => break,
        }
    }
    let got = children.len();
    for child in &mut children {
        let _ = child.kill();
        let _ = child.wait();
    }
    report.check("fork_bomb", got < wanted, &got.to_string());
    super::out(&format!("FORKS {got}"));
}

/// Runs the attacks named on the command line; exit code 0 = all blocked.
pub fn main(args: &[String]) -> i32 {
    let mut report = Report { escaped: 0 };
    let mut socket_paths = Vec::new();
    let mut words = args.iter();
    let mut plan: Vec<(String, String)> = Vec::new();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--memory" => plan.push(("memory".into(), String::new())),
            "--socket" => socket_paths.push(words.next().cloned().unwrap_or_default()),
            option => plan.push((option.to_owned(), words.next().cloned().unwrap_or_default())),
        }
    }
    escalate(&mut report);
    sockets(&mut report, &socket_paths);
    for (option, value) in plan {
        match option.as_str() {
            "--victim-data" => read_other(&mut report, &value),
            "--victim-pid" => kill_other(&mut report, value.parse().unwrap_or(1)),
            "--port" => network(&mut report, value.parse().unwrap_or(1)),
            "--forks" => forks(&mut report, value.parse().unwrap_or(64)),
            "memory" => memory(&mut report),
            _ => report.check("arguments", false, &option),
        }
    }
    if report.escaped == 0 {
        super::out("ATTACKS: ALL BLOCKED");
        0
    } else {
        super::out(&format!("ATTACKS: ESCAPED {}", report.escaped));
        3
    }
}
