//! The real `ultar-logd` and `ulog`: stop signals, recovery after a kill,
//! tamper detection by the tool, a flood through the socket, query, follow
//! and export.

mod common;

use common::world::{send_signal, stop, World, ULOG};
use common::WAIT;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use ultar_bus::messages::log::Stats;
use ultar_service::log::{Level, Record};

/// Writes `count` records and one last marker record, and waits until
/// the marker is stored.
fn fill(world: &World, key: &'static str, count: u64) {
    let one = world.logger("ingest.sock");
    for n in 0..count {
        one.emit(Record::new(Level::Info, key).count("n", n));
    }
    one.emit(Record::new(Level::Info, "fill.done"));
    assert!(one.flush(WAIT));
    world.wait_for_key(&world.client("admin.support"), "fill.done");
}

/// The value after `name=` in the first line of a verify report.
fn number(report: &str, name: &str) -> u64 {
    report
        .split_whitespace()
        .find_map(|word| word.strip_prefix(name)?.strip_prefix('=')?.parse().ok())
        .unwrap()
}

#[test]
fn sigterm_writes_everything_out_removes_the_sockets_and_exits_with_zero() {
    let world = World::new("term");
    let child = world.start(&[]);
    let limits = std::fs::read_to_string(format!("/proc/{}/limits", child.id())).unwrap();
    assert!(limits
        .lines()
        .any(|l| l.starts_with("Max open files") && l.contains(" 256 ")));
    assert!(limits
        .lines()
        .any(|l| l.starts_with("Max core file size") && l.contains(" 0 ")));
    fill(&world, "one.event", 500);
    let (code, stderr) = stop(child);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stderr.ends_with("The log service stopped.\n"), "{stderr}");
    assert!(!world.path("ingest.sock").exists() && !world.path("two.sock").exists());
    // Everything is on disk and the chain verifies with the key. With
    // `--fsync rotate` the head was written by the orderly stop.
    let report = world.verify(true);
    assert!(report.ends_with("The log is intact.\n"), "{report}");
    let last = number(&report, "last");
    assert!(last >= 502, "{report}");
    assert!(report.contains(&format!("head={last}")), "{report}");
}

#[test]
fn after_sigkill_the_service_recovers_and_continues_the_same_log() {
    let world = World::new("kill");
    let child = world.start(&[]);
    fill(&world, "first.life", 300);
    assert_eq!(stop(child).0, Some(0));
    let last = number(&world.verify(true), "last");
    // Second life, ended by SIGKILL right after a burst.
    let mut child = world.start(&[]);
    fill(&world, "second.life", 300);
    send_signal(child.id(), "KILL");
    assert_eq!(child.wait().unwrap().code(), None);
    // The socket files are stale now; the next start replaces them, cuts
    // a torn record if there is one and goes on with new positions.
    let child = world.start(&[]);
    let stats = world.stats(&world.client("admin.support"));
    assert!(stats.last_seq > last, "{} > {last}", stats.last_seq);
    assert!(!stats.degraded);
    assert_eq!(stop(child).0, Some(0));
    world.verify(true);
}

#[test]
fn the_tool_finds_a_record_that_was_changed_on_disk() {
    let world = World::new("tamper");
    let child = world.start(&[]);
    fill(&world, "one.event", 100);
    assert_eq!(stop(child).0, Some(0));
    world.verify(true);
    let segment = std::fs::read_dir(world.path("log"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "ulog"))
        .unwrap();
    let mut data = std::fs::read(&segment).unwrap();
    let middle = data.len() / 2;
    data[middle] ^= 0x40;
    std::fs::write(&segment, data).unwrap();
    let report = world.verify(false);
    assert!(report.contains("The log is NOT intact!"), "{report}");
    assert!(report.contains("problem="), "{report}");
}

#[test]
fn a_flood_through_the_socket_is_limited_and_bounded_and_never_blocks_the_writer() {
    let world = World::new("flood");
    let child = world.start(&["--rate-burst", "100", "--rate-per-second", "10"]);
    let one = world.logger("ingest.sock");
    for n in 0..200_000u64 {
        // Returns at once whatever the log service does with the record.
        one.emit(Record::new(Level::Info, "flood.event").count("n", n));
    }
    assert!(one.flush(WAIT));
    let quiet = world.logger("two.sock");
    quiet.log(Level::Notice, "two.after_flood");
    assert!(quiet.flush(WAIT));
    let admin = world.client("admin.support");
    world.wait_for_key(&admin, "two.after_flood");
    let sent = one.stats();
    assert_eq!(sent.accepted, 200_000);
    // The writer's own drop markers travel as records.
    assert_eq!(
        sent.sent + sent.buffer_dropped,
        sent.accepted + sent.markers,
        "{sent:?}"
    );
    // Nothing is unaccounted for: everything the writer handed over is
    // stored or counted as dropped by the service. (The service may still
    // be reading the last bytes of the flood: ask until the sum is there.)
    let started = std::time::Instant::now();
    let (stats, flood) = loop {
        let stats = admin.call(&Stats {}, WAIT).unwrap();
        let flood = stats
            .sources
            .iter()
            .find(|s| s.service == "app.one")
            .cloned()
            .unwrap();
        if flood.stored + flood.dropped == sent.sent {
            break (stats, flood);
        }
        assert!(started.elapsed() < WAIT, "{flood:?} {sent:?}");
        std::thread::yield_now();
    };
    // Burst plus what the rate allowed while the flood ran: far below the
    // flood.
    assert!(flood.stored >= 100 && flood.stored < 2_000, "{flood:?}");
    assert!(stats.disk_bytes <= stats.disk_limit);
    // The quiet source was not affected by the noisy one.
    let two = stats
        .sources
        .iter()
        .find(|s| s.service == "app.two")
        .unwrap();
    assert_eq!((two.stored, two.dropped), (1, 0));
    assert_eq!(stop(child).0, Some(0));
    let size: u64 = std::fs::read_dir(world.path("log"))
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    assert!(size <= 131_072 + 128, "{size}");
}

/// A stopped daemon whose log holds one service record and two kernel lines.
fn stopped_with_one_warning(tag: &str) -> World {
    let world = World::new(tag);
    let child = world.start(&[]);
    let one = world.logger("ingest.sock");
    one.emit(
        Record::new(Level::Warn, "one.warned")
            .count("n", 7)
            .word("why", "limit"),
    );
    assert!(one.flush(WAIT));
    world.wait_for_key(&world.client("admin.support"), "one.warned");
    assert_eq!(stop(child).0, Some(0));
    world
}

#[test]
fn ulog_queries_follows_shows_statistics_and_is_refused_without_permission() {
    let world = World::new("tools");
    let child = world.start(&[]);
    let one = world.logger("ingest.sock");
    one.emit(
        Record::new(Level::Warn, "one.warned")
            .count("n", 7)
            .word("why", "limit"),
    );
    assert!(one.flush(WAIT));
    world.wait_for_key(&world.client("admin.support"), "one.warned");

    let query = world.ulog("app.one", &["query"]);
    assert!(query.status.success());
    let text = String::from_utf8(query.stdout).unwrap();
    let columns: Vec<&str> = text.lines().next().unwrap().split('\t').collect();
    assert_eq!(
        &columns[2..],
        ["service", "app.one", "warn", "one.warned", "n=7 why=limit"]
    );
    assert_eq!(text.lines().count(), 1);

    let all = world.ulog("admin.support", &["query", "--level", "info"]);
    let text = String::from_utf8(all.stdout).unwrap();
    let kernel = "\tkernel\tkernel\tinfo\tkernel.line\t\t[kern] nvme0 ok";
    assert!(text.lines().any(|line| line.contains(kernel)), "{text}");
    assert!(text.lines().count() > 3);

    let follow = world.ulog("app.one", &["follow", "--count", "1"]);
    assert_eq!(String::from_utf8(follow.stdout).unwrap().lines().count(), 1);
    let stats = world.ulog("admin.support", &["stats"]);
    let text = String::from_utf8(stats.stdout).unwrap();
    assert!(text.starts_with("last_seq=") && text.contains("source=app.one stored=1"));

    let refused = world.ulog("other.mute", &["query"]);
    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(refused.stderr).unwrap(),
        "You are not permitted to do this.\n"
    );
    let usage = Command::new(ULOG)
        .args(["--lang", "en", "frob"])
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(2));
    let text = String::from_utf8(usage.stderr).unwrap();
    assert!(text.starts_with("Invalid arguments.\nUsage:"), "{text}");
    assert_eq!(stop(child).0, Some(0));
}

/// Runs `ulog export`; returns the exported text and the tool's summary.
fn export(world: &World, extra: &[&str], name: &str) -> (String, String) {
    let out = world.path(name);
    let output = Command::new(ULOG)
        .args(["--lang", "en", "export"])
        .arg(world.path("log"))
        .arg(&out)
        .args(extra)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mode = std::fs::metadata(&out).unwrap().permissions().mode();
    assert_eq!(mode & 0o077, 0, "an export is private");
    (
        std::fs::read_to_string(&out).unwrap(),
        String::from_utf8(output.stdout).unwrap(),
    )
}

#[test]
fn an_export_withholds_kernel_text_unless_asked_and_states_the_chain_result() {
    let world = stopped_with_one_warning("export");
    let key = world.path("chain.key");
    let key = key.to_str().unwrap();
    let (redacted, summary) = export(&world, &["--key-file", key], "support.txt");
    assert!(summary.contains("Kernel lines withheld: 2"), "{summary}");
    assert!(summary.ends_with("The log is intact.\n"));
    assert!(redacted.starts_with("# ultar log export v1\n# chain: intact (keyed)\n"));
    assert!(!redacted.contains("nvme0"));
    assert!(redacted.contains("\tservice\tapp.one\twarn\tone.warned\tn=7 why=limit\n"));
    assert!(redacted.contains("# kernel text withheld: 2\n# kernel text included: 0\n"));
    assert!(redacted.contains("# source app.one: 1\n"));
    let asked = ["--key-file", key, "--include-kernel-text"];
    let (full, summary) = export(&world, &asked, "full.txt");
    assert!(summary.contains("Kernel lines withheld: 0"));
    assert!(full.contains("\t[kern] nvme0 ok\n"));
    // Without the key the chain does not verify, and the export says so.
    let (unverified, summary) = export(&world, &[], "nokey.txt");
    assert!(unverified.contains("# chain: NOT INTACT (unkeyed)"));
    assert!(summary.ends_with("The log is NOT intact!\n"));
    let missing = Command::new(ULOG)
        .args(["--lang", "en", "verify"])
        .arg(world.path("no-such-dir"))
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
}
