//! The page cell against a built host (`ultar-web-engine`): started with a cell, it checks from
//! the inside what the cell allows (`--cell-selftest`, `src/cell/selftest.rs`) and the test reads
//! the report. Needs a Linux that lets this process make namespaces and a memory group (root,
//! cgroup v2). Runs only when `ULTAR_WEB_ENGINE` names a built host; otherwise it says
//! `SKIPPED` and does nothing. The refusal test needs no privileges.

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const CHECKS: [&str; 13] = [
    "no-home",
    "root-holds-only-the-view",
    "only-own-processes",
    "no-capabilities-no-new-privileges",
    "only-the-loopback-device",
    "no-route-to-1.1.1.1",
    "no-route-to-the-lan",
    "no-route-to-the-lan-10",
    "no-route-by-ipv6",
    "folders-as-declared",
    "tmp-private-and-writable",
    "only-harmless-devices",
    "relay-reaches-the-browser-socket",
];

fn engine() -> Option<PathBuf> {
    std::env::var_os("ULTAR_WEB_ENGINE").map(PathBuf::from)
}

fn work(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ultar-cell-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("engine")).unwrap();
    std::fs::create_dir_all(dir.join("downloads")).unwrap();
    dir
}

/// The browser's side of the socket: answers every `CONNECT` with `200` and echoes after it.
fn serve_socket(path: &Path) {
    let listener = UnixListener::bind(path).unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let (mut r, mut w) = (stream.try_clone().unwrap(), stream);
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if r.read(&mut byte).unwrap_or(0) == 0 {
                        return;
                    }
                    head.push(byte[0]);
                }
                let _ = w.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
                let mut buf = [0u8; 4096];
                while let Ok(n) = r.read(&mut buf) {
                    if n == 0 || w.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
            });
        }
    });
}

fn run(program: &Path, args: &[String]) -> Output {
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the host did not end");
        std::thread::sleep(Duration::from_millis(50));
    }
    child.wait_with_output().unwrap()
}

/// What a host that ended must not leave: its root folder and its memory group.
fn leftovers() -> usize {
    let count = |dir: &str, prefix: &str| {
        std::fs::read_dir(dir)
            .map(|d| {
                d.flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
                    .count()
            })
            .unwrap_or(0)
    };
    count("/sys/fs/cgroup", "ultar-web-")
        + count(&std::env::temp_dir().to_string_lossy(), "ultar-web-cell-")
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_cell_has_no_network_no_home_and_only_the_relay_out() {
    let Some(engine) = engine() else {
        eprintln!("SKIPPED: ULTAR_WEB_ENGINE does not name a built host");
        return;
    };
    let dir = work("inside");
    let (profile, downloads) = (dir.join("engine"), dir.join("downloads"));
    let socket = profile.join("tor.sock");
    serve_socket(&socket);
    let before = leftovers();
    let text = |p: &Path| p.display().to_string();
    let mut args = vec![
        "--profile".to_string(),
        text(&profile),
        "--downloads".to_string(),
        text(&downloads),
    ];
    // system libraries the host needs, read-only, as the bundle would hold them
    for system in ["/usr", "/lib", "/lib64", "/bin"] {
        if Path::new(system).is_dir() {
            args.extend(words(&["--cell-ro", system]));
        }
    }
    args.extend([
        "--cell-rw".to_string(),
        text(&profile),
        "--cell-rw".to_string(),
        text(&downloads),
        "--cell-memory-mb".to_string(),
        "2048".to_string(),
        "--proxy-socket".to_string(),
        text(&socket),
        "--cell-selftest".to_string(),
    ]);
    let out = run(&engine, &args);
    let report = String::from_utf8_lossy(&out.stdout).into_owned();
    let errors = String::from_utf8_lossy(&out.stderr).into_owned();
    println!("{report}{errors}");
    assert!(!report.contains("selftest FAIL"), "{report}\n{errors}");
    for check in CHECKS {
        assert!(
            report.contains(&format!("selftest ok {check}\n")),
            "check {check} did not report ok:\n{report}\n{errors}"
        );
    }
    assert!(out.status.success(), "{:?}\n{errors}", out.status);
    assert_eq!(
        leftovers(),
        before,
        "the host left its root folder or memory group"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Runs the host with a cell over `engine/` and `downloads/` of `dir` plus `extra`; returns its
/// exit code and what it said on standard error.
fn refused(engine: &Path, dir: &Path, extra: &[&str]) -> (Option<i32>, String) {
    let d = |s: &str| dir.join(s).display().to_string();
    let mut args = words(&["--cell-ro", "/usr"]);
    args.extend(["--cell-rw".to_string(), d("engine")]);
    args.extend(["--cell-rw".to_string(), d("downloads")]);
    args.extend(words(extra));
    let out = run(engine, &args);
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_cell_never_gets_network_a_tcp_proxy_or_a_missing_socket() {
    let Some(engine) = engine() else {
        eprintln!("SKIPPED: ULTAR_WEB_ENGINE does not name a built host");
        return;
    };
    let dir = work("refuse-net");
    let socket = dir.join("engine/tor.sock").display().to_string();
    let (code, said) = refused(&engine, &dir, &["--cell-net", "--proxy-socket", &socket]);
    assert_eq!(code, Some(1));
    assert!(said.contains("never gets network"), "{said}");
    let (code, said) = refused(&engine, &dir, &[]);
    assert_eq!(code, Some(1));
    assert!(said.contains("--proxy-socket"), "{said}");
    let tcp = [
        "--proxy",
        "http://u:p@127.0.0.1:9",
        "--proxy-socket",
        &socket,
    ];
    assert_eq!(refused(&engine, &dir, &tcp).0, Some(1));
    // development without a cell still needs the proxy
    let out = run(&engine, &words(&["--no-cell"]));
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no proxy"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_socket_must_lie_in_the_cell_and_the_second_stage_only_runs_as_pid_1() {
    let Some(engine) = engine() else {
        eprintln!("SKIPPED: ULTAR_WEB_ENGINE does not name a built host");
        return;
    };
    let dir = work("refuse-socket");
    let socket = dir.join("engine/tor.sock").display().to_string();
    // the socket outside the folders the cell sees
    let (code, said) = refused(&engine, &dir, &["--proxy-socket", "/tmp/elsewhere.sock"]);
    assert_eq!(code, Some(1));
    assert!(said.contains("outside"), "{said}");
    // the internal second stage outside a new pid namespace
    let init = dir.join("x").display().to_string();
    let (code, said) = refused(
        &engine,
        &dir,
        &["--proxy-socket", &socket, "--cell-init", &init],
    );
    assert_eq!(code, Some(1));
    assert!(said.contains("first process"), "{said}");
    let _ = std::fs::remove_dir_all(dir);
}
