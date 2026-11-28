//! The relay inside the page cell. The cell has no network except a private loopback; Servo's
//! proxy setting points at a port of that loopback, and this thread copies every connection to
//! the browser's Unix socket (`--proxy-socket`), where the browser's Tor client takes it. So the
//! engine can reach exactly one thing: that socket. The bytes are not looked at (TLS stays
//! between Servo and the site).

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Most connections copied at the same time (each takes two threads); more are closed at once.
pub const MAX_CONNECTIONS: usize = 128;

/// Starts the relay on `listener` (the private loopback): every accepted connection is
/// connected to the Unix socket `socket` and copied both ways until one side ends.
pub fn start(listener: TcpListener, socket: PathBuf) -> io::Result<JoinHandle<()>> {
    let open = Arc::new(AtomicUsize::new(0));
    std::thread::Builder::new()
        .name("ultar-relay".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    open.fetch_sub(1, Ordering::SeqCst);
                    continue; // dropped: closed
                }
                let (socket, open) = (socket.clone(), open.clone());
                let spawned = std::thread::Builder::new()
                    .name("ultar-relay-c".into())
                    .spawn({
                        let open = open.clone();
                        move || {
                            let _ = connection(stream, &socket);
                            open.fetch_sub(1, Ordering::SeqCst);
                        }
                    });
                if spawned.is_err() {
                    open.fetch_sub(1, Ordering::SeqCst);
                }
            }
        })
}

/// One connection: to the socket, then both directions.
fn connection(client: TcpStream, socket: &std::path::Path) -> io::Result<()> {
    let upstream = UnixStream::connect(socket)?;
    let (mut client_read, mut client_write) = (client.try_clone()?, client);
    let (mut up_read, mut up_write) = (upstream.try_clone()?, upstream);
    let back = std::thread::Builder::new()
        .name("ultar-relay-r".into())
        .spawn(move || {
            let _ = copy(&mut up_read, &mut client_write);
            let _ = client_write.shutdown(Shutdown::Both);
            let _ = up_read.shutdown(Shutdown::Both);
        })?;
    let _ = copy(&mut client_read, &mut up_write);
    let _ = up_write.shutdown(Shutdown::Both);
    let _ = client_read.shutdown(Shutdown::Both);
    let _ = back.join();
    Ok(())
}

fn copy(from: &mut impl Read, to: &mut impl Write) -> io::Result<()> {
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        to.write_all(&buf[..n])?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::time::Duration;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ultar-relay-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn relay_to(socket: PathBuf) -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        start(listener, socket).unwrap();
        port
    }

    #[test]
    fn bytes_travel_both_ways_to_the_socket_and_it_is_all_the_relay_reaches() {
        let dir = folder("echo");
        let path = dir.join("t.sock");
        let server = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            for stream in server.incoming().flatten() {
                std::thread::spawn(move || {
                    let (mut r, mut w) = (stream.try_clone().unwrap(), stream);
                    let _ = copy(&mut r, &mut w);
                });
            }
        });
        let port = relay_to(path);
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        // a large message (several buffers) comes back whole
        let sent: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let mut writer = c.try_clone().unwrap();
        let copy_of = sent.clone();
        let t = std::thread::spawn(move || writer.write_all(&copy_of).unwrap());
        let mut back = vec![0u8; sent.len()];
        c.read_exact(&mut back).unwrap();
        t.join().unwrap();
        assert_eq!(back, sent);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_socket_closes_the_connection() {
        let dir = folder("missing");
        let port = relay_to(dir.join("none.sock"));
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut buf = [0u8; 8];
        assert!(matches!(c.read(&mut buf), Ok(0) | Err(_)));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_end_of_one_side_ends_the_other() {
        let dir = folder("close");
        let path = dir.join("t.sock");
        let server = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = server.accept() {
                let _ = s.write_all(b"bye");
            }
        });
        let port = relay_to(path);
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut all = Vec::new();
        c.read_to_end(&mut all).unwrap();
        assert_eq!(all, b"bye");
        let _ = std::fs::remove_dir_all(dir);
    }
}
