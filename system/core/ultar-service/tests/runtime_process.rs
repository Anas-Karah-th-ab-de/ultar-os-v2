//! The runtime in a real process: signals, deadline, limits read back,
//! inherited descriptors, readiness pipe, secrets, panic hook.

mod common;

use common::{base_args, temp_dir, Probe, PROBE, WAIT};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use ultar_service::ready::{wait_ready, ReadyEvent};
use ultar_service::{exit, READY_FD_ENV};

#[test]
fn sigterm_stops_cleanly_with_exit_code_zero() {
    let mut probe = Probe::start(&[]);
    probe.until("ready");
    probe.signal("TERM");
    probe.until("stopping=term");
    let (code, stderr) = probe.finish();
    assert_eq!(code, Some(exit::OK), "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn sigint_stops_and_sighup_reloads() {
    let mut probe = Probe::start(&[]);
    probe.until("ready");
    probe.signal("HUP");
    probe.until("reload");
    probe.signal("HUP");
    probe.until("reload");
    probe.signal("INT");
    probe.until("stopping=int");
    assert_eq!(probe.finish().0, Some(exit::OK));
}

#[test]
fn a_service_that_does_not_stop_is_ended_at_the_deadline() {
    let mut probe = Probe::start(&["--hang", "--deadline-ms", "50"]);
    probe.until("ready");
    probe.signal("TERM");
    probe.until("stopping=term");
    assert_eq!(probe.finish().0, Some(exit::DEADLINE));
}

#[test]
fn a_second_signal_forces_the_exit() {
    let mut probe = Probe::start(&["--hang", "--deadline-ms", "3600000"]);
    probe.until("ready");
    probe.signal("TERM");
    // The first signal has been handled completely once this line is out.
    probe.until("stopping=term");
    probe.signal("TERM");
    assert_eq!(probe.finish().0, Some(exit::FORCED));
}

#[test]
fn limits_umask_and_working_directory_are_applied_and_read_back() {
    let mut probe = Probe::start(&["--nofile", "97"]);
    probe.until("ready");
    assert_eq!(probe.value("platform"), Some("Linux"));
    assert_eq!(probe.value("limit.nofile"), Some("97/97"));
    assert_eq!(probe.value("limit.as"), Some("536870912/536870912"));
    assert_eq!(probe.value("limit.nproc"), Some("128/128"));
    assert_eq!(probe.value("limit.core"), Some("0/0"));
    assert_eq!(probe.value("cwd"), Some("/"));
    for step in [
        "close_fds",
        "umask",
        "workdir",
        "no_new_privs",
        "limit_core",
        "limit_as",
        "limit_nofile",
        "limit_nproc",
    ] {
        assert_eq!(
            probe.value(&format!("step.{step}")),
            Some("Applied"),
            "{step}"
        );
    }
    // Linux has no Ultar rights: reported, not pretended.
    assert_eq!(probe.value("step.rights"), Some("Unsupported"));
    probe.signal("TERM");
    assert_eq!(probe.finish().0, Some(exit::OK));
}

#[test]
fn the_kernel_enforces_the_descriptor_limit() {
    // With 8 descriptors the probe cannot even open what it needs beyond
    // the standard streams and the signal descriptor – but it must not
    // crash: it reports and keeps running.
    let mut probe = Probe::start(&["--nofile", "8"]);
    probe.until("ready");
    assert_eq!(probe.value("limit.nofile"), Some("8/8"));
    probe.signal("TERM");
    assert_eq!(probe.finish().0, Some(exit::OK));
}

#[test]
fn limits_can_be_turned_off_only_explicitly() {
    let mut probe = Probe::start(&["--no-limits"]);
    probe.until("ready");
    assert_eq!(probe.value("step.limit_nofile"), Some("Disabled"));
    assert_ne!(probe.value("limit.nofile"), Some("256/256"));
    probe.signal("TERM");
    assert_eq!(probe.finish().0, Some(exit::OK));
}

#[test]
fn root_is_refused_unless_declared() {
    if ultar_service::effective_uid() != 0 {
        // As an ordinary user the check passes; covered by every other test.
        return;
    }
    let output = Command::new(PROBE).output().unwrap();
    assert_eq!(output.status.code(), Some(exit::HARDENING));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "service.start.root_refused"
    );
    assert!(output.stdout.is_empty());
}

/// Runs the probe through `sh` so that the shell can set up extra
/// descriptors (`redirects`) before it `exec`s the probe (same pid).
fn through_shell(redirects: &str, args: &[&str]) -> Command {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(format!("exec \"$0\" \"$@\" {redirects}"))
        .arg(PROBE)
        .args(base_args())
        .args(args);
    command
}

/// Descriptors the environment itself hands every child (test runner, CI
/// agent, jobserver). The checks below count only what they planted.
fn leaked_by_environment() -> u64 {
    let mut probe = Probe::spawn(through_shell("", &[]));
    probe.until("ready");
    let n = probe
        .value("closed_fds")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    probe.signal("TERM");
    assert_eq!(probe.finish().0, Some(exit::OK));
    n
}

fn closed_count(probe: &Probe) -> u64 {
    probe
        .value("closed_fds")
        .and_then(|v| v.parse().ok())
        .unwrap_or(u64::MAX)
}

#[test]
fn inherited_descriptors_are_closed() {
    let base = leaked_by_environment();
    let dir = temp_dir("fds");
    let file = dir.join("leak");
    std::fs::write(&file, b"x").unwrap();
    let path = file.to_str().unwrap();
    let redirects = format!("5<'{path}' 6<'{path}' 9<'{path}'");
    let mut probe = Probe::spawn(through_shell(&redirects, &[]));
    probe.until("ready");
    assert_eq!(closed_count(&probe), base + 3);
    probe.signal("TERM");
    assert_eq!(probe.finish().0, Some(exit::OK));
}

#[test]
fn readiness_is_reported_over_the_inherited_pipe() {
    // The shell makes descriptor 3 the pipe the test reads and sends the
    // probe's own standard output to standard error.
    let mut command = through_shell("3>&1 1>&2", &[]);
    let mut child = command
        .env(READY_FD_ENV, "3")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pipe = child.stdout.take().unwrap();
    let (pid, mut events) = wait_ready(pipe, WAIT).unwrap();
    assert_eq!(pid, child.id());
    common::send_signal(child.id(), "TERM");
    assert_eq!(events.next_event().unwrap(), Some(ReadyEvent::Stopping));
    assert_eq!(
        events.next_event().unwrap(),
        Some(ReadyEvent::Stopped { code: 0 })
    );
    assert_eq!(events.next_event().unwrap(), None);
    assert_eq!(child.wait().unwrap().code(), Some(exit::OK));
}

#[test]
fn a_service_that_dies_during_start_closes_the_readiness_pipe() {
    let mut command = through_shell("3>&1 1>&2", &["--bogus-option"]);
    let mut child = command
        .env(READY_FD_ENV, "3")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pipe = child.stdout.take().unwrap();
    assert!(matches!(
        wait_ready(pipe, WAIT),
        Err(ultar_service::ReadyError::Closed)
    ));
    assert_eq!(child.wait().unwrap().code(), Some(exit::USAGE));
}

#[test]
fn secrets_come_from_a_descriptor_or_a_checked_file_never_printed() {
    let base = leaked_by_environment();
    let dir = temp_dir("secret");
    let file = dir.join("token");
    std::fs::write(&file, b"correct horse battery\n").unwrap();
    let chmod = |mode| {
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    chmod(0o600);
    let path = file.to_str().unwrap().to_owned();

    let mut by_fd = Probe::spawn(through_shell(&format!("4<'{path}'"), &["--secret-fd", "4"]));
    by_fd.until("ready");
    assert_eq!(by_fd.value("secret.len"), Some("21"));
    // Descriptor 4 was kept for the secret, nothing else was inherited.
    assert_eq!(closed_count(&by_fd), base);
    by_fd.signal("TERM");
    let seen = by_fd.seen.join("\n");
    let (code, stderr) = by_fd.finish();
    assert_eq!(code, Some(exit::OK));
    assert!(!seen.contains("horse") && !stderr.contains("horse"));

    let mut by_file = Probe::start(&["--secret-file", &path]);
    by_file.until("ready");
    assert_eq!(by_file.value("secret.len"), Some("21"));
    by_file.signal("TERM");
    assert_eq!(by_file.finish().0, Some(exit::OK));

    chmod(0o640);
    let mut open = Probe::start(&["--secret-file", &path]);
    open.until("ready");
    assert_eq!(open.value("secret.error"), Some("secret.too_permissive"));
    open.signal("TERM");
    assert_eq!(open.finish().0, Some(exit::OK));
}

#[test]
fn a_panic_is_reported_with_its_own_exit_code_and_without_its_message() {
    let mut probe = Probe::start(&["--panic"]);
    probe.until("ready");
    let (code, stderr) = probe.finish();
    assert_eq!(code, Some(exit::PANIC));
    assert!(stderr.starts_with("service.panic "), "{stderr}");
    assert!(stderr.contains("probe.rs:"));
    assert!(!stderr.contains("secret-looking"), "{stderr}");
}

#[test]
fn sigkill_cannot_be_handled_and_is_visible_as_such() {
    let mut probe = Probe::start(&[]);
    probe.until("ready");
    probe.signal("KILL");
    // Killed by a signal: no exit code at all.
    assert_eq!(probe.finish().0, None);
}

#[test]
fn bad_command_lines_are_usage_errors() {
    let output = Command::new(PROBE).arg("--nofile").output().unwrap();
    assert_eq!(output.status.code(), Some(exit::USAGE));
}

#[test]
fn the_main_thread_waits_for_signals_and_the_work_runs_on_a_second_thread() {
    // The Ultar kernel marks a blocked signal pending on the first thread
    // of the process only, so it has to be the main thread that reads the
    // signal descriptor (see `ultar_service::signal`).
    let mut probe = Probe::start(&[]);
    probe.until("ready");
    let pid = probe.child.id();
    let mut names = Vec::new();
    for entry in std::fs::read_dir(format!("/proc/{pid}/task")).unwrap() {
        let tid: u32 = entry
            .unwrap()
            .file_name()
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        let comm = std::fs::read_to_string(format!("/proc/{pid}/task/{tid}/comm")).unwrap();
        names.push((tid == pid, comm.trim().to_owned()));
    }
    let work = names.iter().find(|(_, name)| name == "probe-work").unwrap();
    assert!(
        !work.0,
        "the work must not run on the main thread: {names:?}"
    );
    // The main thread sits in `ppoll` on the signal descriptor and the
    // wake pipe (the system call number is the first field).
    let ppoll = if cfg!(target_arch = "x86_64") {
        "271"
    } else {
        "73"
    };
    let started = std::time::Instant::now();
    // (The work thread may print `ready` an instant before the main
    // thread has entered its wait: look until it is there.)
    while let Ok(syscall) = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/syscall")) {
        if syscall.split_whitespace().next() == Some(ppoll) {
            break;
        }
        assert!(started.elapsed() < WAIT, "main thread is in: {syscall}");
        std::thread::yield_now();
    }
    probe.signal("TERM");
    assert_eq!(probe.finish().0, Some(exit::OK));
}
