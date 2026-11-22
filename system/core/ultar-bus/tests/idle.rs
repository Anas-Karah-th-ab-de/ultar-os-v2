//! Evidence for "zero wakeups when idle" (plan §3.6). Kept in its own test
//! binary so no other test's threads run in the measured process.
#![cfg(target_os = "linux")]

mod common;

use common::{client, open_broker, WAIT};
use std::time::Duration;

/// Context switches of every thread of this process, by thread id.
fn context_switches() -> Vec<(String, u64)> {
    let mut counts = Vec::new();
    for task in std::fs::read_dir("/proc/self/task").unwrap() {
        let task = task.unwrap();
        let Ok(status) = std::fs::read_to_string(task.path().join("status")) else {
            continue;
        };
        let total = status
            .lines()
            .filter(|line| line.contains("ctxt_switches"))
            .filter_map(|line| line.split_whitespace().last()?.parse::<u64>().ok())
            .sum();
        counts.push((task.file_name().to_string_lossy().into_owned(), total));
    }
    counts
}

#[test]
fn idle_bus_does_not_wake_any_thread() {
    let (broker, path) = open_broker("idle");
    let service = client(&path, "service");
    let _endpoint = service.register("idle").unwrap();
    let watcher = client(&path, "watcher");
    let stream = watcher.subscribe_pattern("*").unwrap();
    let publisher = client(&path, "publisher");
    publisher
        .publish_raw("warm.up", 1, true, vec![1], true)
        .unwrap();
    stream.recv_timeout(WAIT).unwrap();
    assert_eq!(broker.stats().connections, 3);
    // Let every thread reach its blocking call.
    std::thread::sleep(Duration::from_millis(200));

    let own = std::fs::read_link("/proc/thread-self")
        .ok()
        .and_then(|link| {
            link.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });
    let before = context_switches();
    std::thread::sleep(Duration::from_millis(1500));
    let after = context_switches();

    // 1 acceptor + 3 × (reader + writer) in the broker, 3 client readers.
    assert!(
        before.len() >= 11,
        "expected the bus threads to exist, found {}",
        before.len()
    );
    for (thread_id, count_before) in &before {
        if Some(thread_id) == own.as_ref() {
            continue; // this thread slept and woke up
        }
        let count_after = after
            .iter()
            .find(|(id, _)| id == thread_id)
            .map(|(_, count)| *count);
        assert_eq!(
            count_after,
            Some(*count_before),
            "thread {thread_id} woke up while idle"
        );
    }
    // And the bus still reacts immediately afterwards.
    publisher
        .publish_raw("still.alive", 1, false, vec![2], true)
        .unwrap();
    assert_eq!(stream.recv_timeout(WAIT).unwrap().payload, vec![2]);
}
