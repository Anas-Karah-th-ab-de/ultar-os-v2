//! Tests of the logger: rate limit, markers, the bounded buffer and a log
//! service that is away or comes back.

use super::*;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicU64, Ordering};

struct ManualClock(AtomicU64);

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn manual(config: LoggerConfig) -> (Logger, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock(AtomicU64::new(0)));
    (Logger::start_with_clock(config, clock.clone()), clock)
}

fn socket_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ultar-log-{}-{tag}.sock", std::process::id()))
}

fn read_keys(stream: &mut UnixStream, count: usize) -> Vec<OwnedRecord> {
    let mut reader = wire::FrameReader::new();
    let mut records = Vec::new();
    let mut buffer = [0u8; 4096];
    while records.len() < count {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "log stream ended early");
        reader
            .push(&buffer[..read], |body| {
                records.push(wire::decode_body(body).unwrap());
            })
            .unwrap();
    }
    records
}

#[test]
fn rate_limit_drops_and_reports_with_a_marker() {
    let mut config = LoggerConfig::to(LogTarget::Discard);
    config.rate = RateLimit {
        burst: 3,
        per_second: 1,
    };
    let (logger, clock) = manual(config);
    for _ in 0..10 {
        logger.log(Level::Info, "tick");
    }
    let stats = logger.stats();
    assert_eq!((stats.accepted, stats.rate_dropped), (3, 7));
    clock.0.store(1_000, Ordering::SeqCst);
    logger.log(Level::Info, "tick");
    // The record that passes is preceded by the marker.
    let stats = logger.stats();
    assert_eq!(
        (stats.accepted, stats.markers, stats.rate_dropped),
        (4, 1, 7)
    );
}

#[test]
fn levels_below_the_minimum_and_bad_records_are_counted_not_sent() {
    let (logger, _clock) = manual(LoggerConfig::to(LogTarget::Discard));
    logger.log(Level::Debug, "quiet");
    logger.log(Level::Info, "Bad Key");
    logger.log(Level::Info, "fine");
    let stats = logger.stats();
    assert_eq!((stats.filtered, stats.rejected, stats.accepted), (1, 1, 1));
    assert!(logger.flush(Duration::ZERO));
}

#[test]
fn records_reach_the_socket_in_order_with_the_drop_marker() {
    let path = socket_path("order");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    let mut config = LoggerConfig::to(LogTarget::Socket(path.clone()));
    config.rate = RateLimit {
        burst: 2,
        per_second: 1,
    };
    let (logger, clock) = manual(config);
    logger.emit(Record::new(Level::Info, "one").count("n", 1));
    logger.log(Level::Info, "two");
    logger.log(Level::Info, "lost");
    logger.log(Level::Info, "lost");
    clock.0.store(1_000, Ordering::SeqCst);
    logger.log(Level::Error, "three");
    let (mut stream, _) = listener.accept().unwrap();
    let records = read_keys(&mut stream, 4);
    let keys: Vec<&str> = records.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, ["one", "two", DROPPED_KEY, "three"]);
    assert_eq!(
        records[2].fields,
        vec![
            ("rate".to_owned(), OwnedValue::Count(2)),
            ("buffer".to_owned(), OwnedValue::Count(0)),
        ]
    );
    assert!(logger.flush(Duration::from_secs(30)));
    assert_eq!(logger.stats().sent, 4);
    logger.close();
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn without_a_log_service_the_buffer_stays_bounded_and_nothing_blocks() {
    let path = socket_path("absent");
    let _ = std::fs::remove_file(&path);
    let mut config = LoggerConfig::to(LogTarget::Socket(path.clone()));
    config.buffer_records = 8;
    config.rate = RateLimit {
        burst: 1_000_000,
        per_second: 0,
    };
    config.retry_min = Duration::from_millis(1);
    config.retry_max = Duration::from_millis(5);
    let (logger, _clock) = manual(config);
    for index in 0..100_000u64 {
        logger.emit(Record::new(Level::Info, "flood").count("n", index));
        assert!(logger.stats().queued <= 8);
    }
    let stats = logger.stats();
    assert_eq!((stats.accepted, stats.markers), (100_000, 0));
    assert!(stats.buffer_dropped >= 100_000 - 9, "{stats:?}");
    assert!(!logger.flush(Duration::from_millis(1)));
    // The log service appears: the newest records arrive; the next
    // record is led by the marker of what was lost.
    let listener = UnixListener::bind(&path).unwrap();
    let (mut stream, _) = listener.accept().unwrap();
    assert!(logger.flush(Duration::from_secs(30)));
    logger.log(Level::Info, "after");
    assert!(logger.flush(Duration::from_secs(30)));
    let stats = logger.stats();
    let records = read_keys(&mut stream, usize::try_from(stats.sent).unwrap());
    assert!(records.len() <= 11, "{}", records.len());
    let tail: Vec<&str> = records[records.len() - 2..]
        .iter()
        .map(|r| r.key.as_str())
        .collect();
    assert_eq!(tail, [DROPPED_KEY, "after"]);
    let marker = &records[records.len() - 2];
    assert_eq!(
        marker.fields[1],
        ("buffer".to_owned(), OwnedValue::Count(stats.buffer_dropped))
    );
    logger.close();
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn a_log_service_that_goes_away_and_returns_is_survived() {
    let path = socket_path("restart");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    let mut config = LoggerConfig::to(LogTarget::Socket(path.clone()));
    config.retry_min = Duration::from_millis(1);
    config.retry_max = Duration::from_millis(5);
    let (logger, _clock) = manual(config);
    logger.log(Level::Info, "first");
    let (mut stream, _) = listener.accept().unwrap();
    assert_eq!(read_keys(&mut stream, 1)[0].key, "first");
    drop(stream);
    drop(listener);
    std::fs::remove_file(&path).unwrap();
    // Writes into the dead connection fail at some point; the records
    // are kept and delivered to the new instance.
    for _ in 0..50 {
        logger.log(Level::Info, "during");
    }
    let listener = UnixListener::bind(&path).unwrap();
    let (mut stream, _) = listener.accept().unwrap();
    logger.log(Level::Info, "last");
    assert!(logger.flush(Duration::from_secs(30)));
    let mut seen: Vec<String> = Vec::new();
    while seen.last().map(String::as_str) != Some("last") {
        // One read may deliver several records.
        seen.extend(read_keys(&mut stream, 1).into_iter().map(|r| r.key));
    }
    assert!(seen.len() >= 2 && seen.iter().any(|key| key == "during"));
    logger.close();
    logger.log(Level::Info, "ignored");
    std::fs::remove_file(&path).unwrap();
}
