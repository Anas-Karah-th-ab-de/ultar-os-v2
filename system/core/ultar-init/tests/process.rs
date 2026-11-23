//! The real `ultar-init` (not PID 1) with real services: order and
//! readiness, output to the log service, memory groups, identities,
//! restarts, the ordered shutdown.

mod common;

use common::{is_root, journal, stop, World};
use std::time::Duration;

/// (`dash` redirects only to descriptors 0–9; the pipe has a higher number.)
const READY_AFTER_A_WHILE: &str = r#"
exec = "/bin/sh"
notify = true
args = ["-c", "echo starting; sleep 0.3; echo READY $$ > /proc/self/fd/$ULTAR_READY_FD; echo first is up; exec sleep 60"]
"#;

#[test]
fn services_start_after_readiness_and_stop_in_reverse_order() {
    let world = World::new("order");
    world.service("first", READY_AFTER_A_WHILE);
    world.service(
        "second",
        "exec = \"/bin/sh\"\nafter = [\"first\"]\nargs = [\"-c\", \"echo second runs; exec sleep 60\"]",
    );
    let child = world.start("");
    world.wait_status("second", |c| c[1] == "running");
    // Output of both reached the log service under their names.
    world.wait_line("first", "first is up");
    world.wait_line("second", "second runs");
    let starting = world.wait_line("first", "starting");
    assert_eq!(starting.stream, ultar_service::log::output::Stream::Out);
    let (code, stderr) = stop(child);
    assert_eq!(code, Some(0), "{stderr}");
    let lines = journal(&stderr);
    let position = |prefix: &str| {
        lines
            .iter()
            .position(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("{prefix} missing: {stderr}"))
    };
    // Started in order, readiness first …
    assert!(position("ready first") < position("start second"));
    // … stopped in reverse order.
    assert!(position("exit second") < position("exit first"), "{stderr}");
}

#[test]
fn a_service_is_moved_into_its_memory_group_with_its_limits() {
    let world = World::new("groups");
    let groups = world.path("groups");
    // A tree like the kernel's (files appear in new groups by themselves
    // there; here they are made beforehand).
    let dir = groups.join("system/limited");
    std::fs::create_dir_all(&dir).unwrap();
    for file in ["cgroup.procs", "memory.max", "pids.max"] {
        std::fs::write(groups.join(file), "").unwrap();
        std::fs::write(dir.join(file), "").unwrap();
    }
    world.service(
        "limited",
        "exec = \"/bin/sleep\"\nargs = [\"60\"]\n[cgroup]\n\"memory.max\" = \"32M\"\n\"pids.max\" = \"8\"",
    );
    // A limit the tree has no file for is refused: the service does not start.
    std::fs::create_dir_all(groups.join("system/unlimited")).unwrap();
    world.service(
        "unlimited",
        "exec = \"/bin/sleep\"\nargs = [\"60\"]\nrestart = \"never\"\n[cgroup]\n\"memory.max\" = \"8M\"",
    );
    let child = world.start(&format!("group_root = \"{}\"", groups.display()));
    let status = world.wait_status("limited", |c| c[1] == "running");
    let read = |file: &str| std::fs::read_to_string(dir.join(file)).unwrap();
    assert_eq!(
        (read("memory.max").as_str(), read("pids.max").as_str()),
        ("32M", "8")
    );
    // The launcher moved itself in before it became the service.
    assert_eq!(read("cgroup.procs").trim(), status[2]);
    world.wait_status("unlimited", |c| c[1] == "failed" || c[1] == "backoff");
    let (code, stderr) = stop(child);
    assert_eq!(code, Some(0));
    assert!(
        stderr.contains("refused unlimited reason=group"),
        "{stderr}"
    );
}

#[test]
fn as_root_a_service_runs_as_its_user_with_its_groups_and_without_new_privileges() {
    if !is_root() {
        eprintln!("skipped: needs root");
        return;
    }
    let world = World::new("ids");
    let state = world.path("state");
    world.service(
        "nobody",
        &format!(
            "exec = \"/bin/sh\"\nuser = 65534\ngroups = [65533]\nrestart = \"never\"\n\
             dirs = [\"{}\"]\n\
             args = [\"-c\", \"id -u; id -G; grep NoNewPrivs /proc/self/status; touch {}/x && echo wrote\"]",
            state.display(),
            state.display()
        ),
    );
    let child = world.start("");
    world.wait_line("nobody", "wrote");
    let texts = world.texts("nobody");
    assert_eq!(texts[0], "65534");
    assert_eq!(texts[1], "65534 65533");
    assert!(texts[2].ends_with('1'), "{texts:?}");
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(&state).unwrap();
    assert_eq!((meta.uid(), meta.mode() & 0o777), (65534, 0o700));
    let (code, _) = stop(child);
    assert_eq!(code, Some(0));
}

#[test]
fn without_root_another_user_is_refused_and_init_carries_on() {
    if is_root() {
        eprintln!("skipped: needs a user without root");
        return;
    }
    let world = World::new("noroot");
    let other = ultar_service::effective_uid().wrapping_add(1);
    world.service(
        "other",
        &format!("exec = \"/bin/sleep\"\nargs = [\"60\"]\nuser = {other}"),
    );
    world.service("mine", "exec = \"/bin/sleep\"\nargs = [\"60\"]");
    let child = world.start("");
    world.wait_status("other", |c| c[1] == "failed");
    world.wait_status("mine", |c| c[1] == "running");
    let (code, stderr) = stop(child);
    assert_eq!(code, Some(0));
    assert!(
        stderr.contains("refused other reason=identity_needs_root"),
        "{stderr}"
    );
}

#[test]
fn a_crashing_service_is_restarted_with_back_off_and_a_request_works() {
    let world = World::new("restart");
    world.service(
        "crash",
        "exec = \"/bin/sh\"\nargs = [\"-c\", \"echo up; exit 4\"]",
    );
    world.service("calm", "exec = \"/bin/sleep\"\nargs = [\"60\"]");
    let child = world.start("");
    world.wait_status("crash", |c| c[3].parse::<u32>().unwrap_or(0) >= 2);
    let before = world.wait_status("calm", |c| c[1] == "running");
    assert!(world.ctl("restart calm").contains("restart"));
    world.wait_status("calm", |c| c[1] == "running" && c[2] != before[2]);
    assert!(world.ctl("stop nonexistent").starts_with("error"));
    let (code, stderr) = stop(child);
    assert_eq!(code, Some(0));
    assert!(stderr.contains("exit crash"), "{stderr}");
    assert!(stderr.contains("code=4"), "{stderr}");
}

#[test]
fn a_service_without_readiness_in_time_is_ended() {
    let world = World::new("deadline");
    world.service(
        "slow",
        "exec = \"/bin/sleep\"\nargs = [\"60\"]\nnotify = true\nstart_timeout_ms = 300\nrestart = \"never\"",
    );
    let child = world.start("");
    world.wait_status("slow", |c| c[1] == "done" || c[1] == "failed");
    std::thread::sleep(Duration::from_millis(100));
    let (code, stderr) = stop(child);
    assert_eq!(code, Some(0));
    assert!(
        stderr.contains("refused slow reason=ready_timeout"),
        "{stderr}"
    );
}
