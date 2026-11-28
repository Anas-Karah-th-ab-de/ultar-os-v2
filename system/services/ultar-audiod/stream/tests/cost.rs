//! What it costs to carry sound over the stream socket (the measurement behind the choice of
//! raw PCM over a Unix socket instead of a shared-memory ring, `docs/integration/ultar-audiod.md`):
//! ten seconds of 48 kHz stereo (the hardest case: the card's own format) are pushed as fast as
//! the socket takes them and read on the other side; the time per second of sound is printed.

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::time::Instant;
use ultar_audio_stream::{Class, Direction, Hello, Status, Stream, HELLO_LEN};

#[test]
fn a_second_of_stereo_sound_costs_a_few_milliseconds_to_carry() {
    let dir = std::env::temp_dir().join(format!("uas-cost-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut hello = [0u8; HELLO_LEN];
        s.read_exact(&mut hello).unwrap();
        s.write_all(&Status::Ok.encode()).unwrap();
        let mut total = 0usize;
        let mut buf = vec![0u8; 65_536];
        loop {
            match s.read(&mut buf) {
                Ok(0) | Err(_) => return total,
                Ok(n) => total += n,
            }
        }
    });
    let mut stream =
        Stream::connect(&path, Hello::new(Class::Media, Direction::Play, 48_000, 2)).unwrap();
    // 20 ms of 48 kHz stereo
    let chunk: Vec<i16> = (0..1_920).map(|i| (i % 200) as i16).collect();
    let seconds = 10;
    let mut sent = 0usize;
    let start = Instant::now();
    while sent < seconds * 96_000 {
        let took = stream.push(&chunk).unwrap();
        sent += took;
        if took < chunk.len() {
            std::thread::yield_now();
        }
    }
    while stream.backlog() > 0 {
        stream.flush().unwrap();
        std::thread::yield_now();
    }
    let spent = start.elapsed();
    drop(stream);
    let received = server.join().unwrap();
    let per_second = spent.as_secs_f64() * 1000.0 / seconds as f64;
    println!("carried {received} bytes: {per_second:.2} ms per second of 48 kHz stereo sound");
    assert_eq!(received, sent * 2);
    assert!(per_second < 100.0, "{per_second} ms per second of sound");
    let _ = std::fs::remove_dir_all(&dir);
}
