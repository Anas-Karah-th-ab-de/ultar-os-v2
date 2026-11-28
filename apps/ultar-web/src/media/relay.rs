//! The way of the page cell's sound to the sound server: a Unix socket in the engine's folder
//! (which the cell sees, like the Tor socket) that the browser forwards byte for byte to its
//! own stream socket of `ultar-audiod`. The sound server therefore always sees the browser's
//! user id (the launcher's, asked at run time), never the cell's, and the cell never sees
//! `/run/ultar/audio` with the sockets of the other programs.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// The browser's stream socket of the sound server.
pub const AUDIO_SOCKET: &str = "/run/ultar/audio/app.browser.sock";
/// Connections of the cell at the same time at most (a page makes a few streams).
pub const MAX_CONNECTIONS: usize = 8;

/// Listens on `listen` (mode 0600) and forwards every connection to `upstream`. The threads
/// end with the process; a connection that cannot reach `upstream` is closed.
pub fn spawn(listen: &Path, upstream: PathBuf) -> io::Result<()> {
    let _ = std::fs::remove_file(listen);
    let listener = UnixListener::bind(listen)?;
    std::fs::set_permissions(listen, std::fs::Permissions::from_mode(0o600))?;
    let open = Arc::new(AtomicUsize::new(0));
    std::thread::Builder::new()
        .name("audio-relay".into())
        .spawn(move || {
            for cell in listener.incoming().flatten() {
                if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    open.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let (upstream, open) = (upstream.clone(), open.clone());
                let _ = std::thread::Builder::new()
                    .name("audio-relay-conn".into())
                    .spawn(move || {
                        let _ = forward(cell, &upstream);
                        open.fetch_sub(1, Ordering::SeqCst);
                    });
            }
        })?;
    Ok(())
}

/// Copies both ways until one side ends, then ends the other.
fn forward(cell: UnixStream, upstream: &Path) -> io::Result<()> {
    let server = UnixStream::connect(upstream)?;
    let (mut cell_in, mut server_out) = (cell.try_clone()?, server.try_clone()?);
    let up = std::thread::Builder::new()
        .name("audio-relay-up".into())
        .spawn(move || {
            let _ = io::copy(&mut cell_in, &mut server_out);
            let _ = server_out.shutdown(std::net::Shutdown::Both);
        })?;
    let (mut server_in, mut cell_out) = (server, cell);
    let _ = io::copy(&mut server_in, &mut cell_out);
    let _ = cell_out.shutdown(std::net::Shutdown::Both);
    let _ = up.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("relay-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn bytes_go_both_ways_and_the_end_of_one_side_ends_the_other() {
        let d = dir("both");
        let server = UnixListener::bind(d.join("server.sock")).unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = server.accept().unwrap();
            let mut hello = [0u8; 4];
            s.read_exact(&mut hello).unwrap();
            s.write_all(b"ok").unwrap();
            let mut rest = Vec::new();
            s.read_to_end(&mut rest).unwrap();
            assert_eq!(rest, b"pcm-pcm");
        });
        spawn(&d.join("cell.sock"), d.join("server.sock")).unwrap();
        let mut c = UnixStream::connect(d.join("cell.sock")).unwrap();
        c.write_all(b"UAS1").unwrap();
        let mut reply = [0u8; 2];
        c.read_exact(&mut reply).unwrap();
        assert_eq!(&reply, b"ok");
        c.write_all(b"pcm-pcm").unwrap();
        c.shutdown(std::net::Shutdown::Write).unwrap();
        let mut end = Vec::new();
        c.read_to_end(&mut end).unwrap();
        assert!(end.is_empty(), "the server's end reaches the cell");
        let mode = std::fs::metadata(d.join("cell.sock"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_connection_without_a_sound_server_is_closed() {
        let d = dir("none");
        spawn(&d.join("cell.sock"), d.join("missing.sock")).unwrap();
        let mut c = UnixStream::connect(d.join("cell.sock")).unwrap();
        let mut b = [0u8; 1];
        assert_eq!(c.read(&mut b).unwrap_or(0), 0);
    }
}
