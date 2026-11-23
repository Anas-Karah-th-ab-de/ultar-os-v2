use super::*;
use std::io::Read;
use std::os::unix::net::UnixListener;
use ultar_service::log::output::decode_body;
use ultar_service::log::wire::FrameReader;

#[test]
fn bytes_are_cut_into_lines_and_long_lines_are_cut_once() {
    let mut cutter = LineCutter::default();
    let mut lines = Vec::new();
    cutter.push(b"first\nsec", |line| lines.push(line));
    cutter.push(b"ond\n", |line| lines.push(line));
    let long = vec![b'x'; MAX_TEXT + 100];
    cutter.push(&long, |line| lines.push(line));
    cutter.push(b"tail of the long line\nlast", |line| lines.push(line));
    cutter.finish(|line| lines.push(line));
    assert_eq!(lines.len(), 4);
    assert_eq!((lines[0].as_str(), lines[1].as_str()), ("first", "second"));
    assert_eq!(lines[2].len(), MAX_TEXT);
    assert_eq!(lines[3], "last");
}

#[test]
fn the_ring_is_bounded_and_counts_what_it_lost() {
    let mut forwarder = Forwarder::new(None);
    for n in 0..(RING_LINES + 10) {
        forwarder.push("svc", Stream::Out, &format!("line {n}"));
    }
    assert_eq!(forwarder.waiting(), RING_LINES);
    assert_eq!(forwarder.dropped(), 10);
    let big = "y".repeat(MAX_TEXT);
    for _ in 0..(RING_BYTES / MAX_TEXT + 5) {
        forwarder.push("svc", Stream::Err, &big);
    }
    assert!(forwarder.waiting() * MAX_TEXT <= RING_BYTES);
    // Without a log service nothing leaves, nothing blocks.
    assert!(forwarder.pump(Instant::now()));
    assert!(!forwarder.connected());
}

#[test]
fn waiting_lines_reach_the_log_service_once_it_listens() {
    let dir = std::env::temp_dir().join(format!("ultar-init-out-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("init.sock");
    let mut forwarder = Forwarder::new(Some(path.clone()));
    forwarder.push("logd", Stream::Out, "before the log service");
    assert!(forwarder.pump(Instant::now()));
    let listener = UnixListener::bind(&path).unwrap();
    forwarder.push("bus", Stream::Err, "after");
    let deadline = Instant::now() + Duration::from_secs(5);
    while forwarder.pump(Instant::now()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let (mut peer, _) = listener.accept().unwrap();
    drop(forwarder);
    let mut bytes = Vec::new();
    peer.read_to_end(&mut bytes).unwrap();
    let mut lines = Vec::new();
    FrameReader::new()
        .push(&bytes, |body| lines.push(decode_body(body).unwrap().text))
        .unwrap();
    assert_eq!(lines, ["before the log service", "after"]);
    std::fs::remove_dir_all(dir).unwrap();
}
