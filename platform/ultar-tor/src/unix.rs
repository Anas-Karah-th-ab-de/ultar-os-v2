//! The third front: the same HTTP `CONNECT` protocol on a Unix socket whose path the app gives.
//! For an engine in a cell **without any network** (the browser's page cell): the only thing it
//! can reach is this file. No secret is asked: the file mode (0600, made in one step) and the
//! folder it lies in protect it, and the engine's cell is the only one that sees that folder.
//! The refusals are the same as on the other fronts (this device, the local network).

use crate::http;
use std::future::Future;
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Component, Path};
use tokio::io::{AsyncRead, AsyncWrite};

/// Longest socket path (the address field of a Unix socket holds 108 bytes).
pub const MAX_PATH: usize = 100;

fn refuse(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason.to_string())
}

/// Checks the path of the socket: absolute, clean (no `.`, `..`, empty parts), short enough,
/// below an existing folder.
pub fn check_path(path: &Path) -> io::Result<()> {
    if !path.is_absolute() || path.as_os_str().len() > MAX_PATH {
        return Err(refuse("socket path must be absolute and short"));
    }
    let clean = path
        .components()
        .all(|c| matches!(c, Component::RootDir | Component::Normal(_)));
    // `Path` hides `.` and `//` inside a path: look at the text itself
    let text = path.to_string_lossy();
    let parts_ok = text
        .split('/')
        .skip(1)
        .all(|part| !part.is_empty() && part != "." && part != "..");
    if !clean || !parts_ok {
        return Err(refuse("socket path must be clean"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| refuse("socket path has no folder"))?;
    if !std::fs::metadata(parent)?.is_dir() {
        return Err(refuse("socket folder is not a folder"));
    }
    Ok(())
}

/// Makes the socket at `path` with mode 0600 and no moment with a wider one: it is bound inside
/// a private folder (mode 0700, made beside it), given its mode there and then moved to `path`
/// (a rename keeps the socket). No process-wide creation mask is touched, so other threads of
/// the app are not affected. A left-over socket of an earlier run is replaced; anything else
/// at that path is refused.
pub fn bind(path: &Path) -> io::Result<UnixListener> {
    check_path(path)?;
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_socket() => std::fs::remove_file(path)?,
        Ok(_) => return Err(io::Error::from(io::ErrorKind::AlreadyExists)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let parent = path
        .parent()
        .ok_or_else(|| refuse("socket path has no folder"))?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let private = parent.join(format!(".ultar-tor-{}-{nanos}", std::process::id()));
    std::fs::DirBuilder::new().mode(0o700).create(&private)?;
    let made = (|| {
        let inside = private.join("s");
        let listener = UnixListener::bind(&inside)?;
        std::fs::set_permissions(&inside, std::fs::Permissions::from_mode(0o600))?;
        std::fs::rename(&inside, path)?;
        listener.set_nonblocking(true)?;
        Ok(listener)
    })();
    let _ = std::fs::remove_dir_all(&private);
    made
}
/// Serves `CONNECT` on `listener` until the runtime ends; `open(host, port)` gives the Tor
/// stream of a request.
pub async fn serve<F, Fut, S>(listener: tokio::net::UnixListener, open: F)
where
    F: Fn(String, u16) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<S, &'static str>> + Send + 'static,
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let open = open.clone();
        tokio::spawn(async move {
            let _ = http::handle(stream, None, open).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};

    fn folder(name: &str) -> io::Result<std::path::PathBuf> {
        let dir =
            std::env::temp_dir().join(format!("ultar-tor-unix-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    #[test]
    fn paths_are_checked_like_the_other_ports() -> io::Result<()> {
        let dir = folder("paths")?;
        assert!(check_path(&dir.join("t.sock")).is_ok());
        assert!(check_path(Path::new("t.sock")).is_err(), "relative");
        assert!(check_path(&dir.join("../t.sock")).is_err(), "..");
        assert!(check_path(&dir.join("./t.sock")).is_err(), ".");
        assert!(check_path(&dir.join("a/b.sock")).is_err(), "no such folder");
        assert!(check_path(Path::new("/")).is_err());
        let long = dir.join("x".repeat(MAX_PATH));
        assert!(check_path(&long).is_err(), "too long");
        std::fs::remove_dir_all(dir)
    }

    #[test]
    fn the_socket_is_private_and_a_stale_one_is_replaced() -> io::Result<()> {
        let dir = folder("mode")?;
        let path = dir.join("t.sock");
        let first = bind(&path)?;
        assert_eq!(std::fs::metadata(&path)?.mode() & 0o777, 0o600);
        // nothing of the private folder it was made in is left beside it
        assert_eq!(std::fs::read_dir(&dir)?.count(), 1);
        drop(first);
        // the file stays after the listener is gone: the next start replaces it
        assert!(bind(&path).is_ok());
        // a regular file is never replaced
        let other = dir.join("file");
        std::fs::write(&other, b"x")?;
        assert_eq!(
            bind(&other).err().map(|e| e.kind()),
            Some(io::ErrorKind::AlreadyExists)
        );
        std::fs::remove_dir_all(dir)
    }

    async fn ask(path: &Path, head: &str) -> io::Result<String> {
        let mut c = tokio::net::UnixStream::connect(path).await?;
        c.write_all(head.as_bytes()).await?;
        let mut buf = vec![0u8; 64];
        let n = c.read(&mut buf).await.unwrap_or(0);
        let mut text = String::from_utf8_lossy(&buf[..n]).into_owned();
        if text.starts_with("HTTP/1.1 200") {
            c.write_all(b"ping").await?;
            let mut back = [0u8; 4];
            c.read_exact(&mut back).await?;
            text.push_str(&String::from_utf8_lossy(&back));
        }
        Ok(text)
    }

    #[tokio::test]
    async fn connect_without_a_secret_goes_through_and_local_targets_do_not() -> io::Result<()> {
        let dir = folder("serve")?;
        let path = dir.join("t.sock");
        let listener = tokio::net::UnixListener::from_std(bind(&path)?)?;
        tokio::spawn(serve(listener, |_host, _port| async {
            let (a, mut b) = duplex(64);
            tokio::spawn(async move {
                let mut x = [0u8; 4];
                if b.read_exact(&mut x).await.is_ok() {
                    let _ = b.write_all(&x).await;
                }
            });
            Ok(a)
        }));
        let ok = ask(&path, "CONNECT example.org:443 HTTP/1.1\r\n\r\n").await?;
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        assert!(ok.ends_with("ping"), "{ok}");
        let local = ask(&path, "CONNECT 127.0.0.1:22 HTTP/1.1\r\n\r\n").await?;
        assert!(local.starts_with("HTTP/1.1 403"), "{local}");
        let lan = ask(&path, "CONNECT 192.168.1.1:80 HTTP/1.1\r\n\r\n").await?;
        assert!(lan.starts_with("HTTP/1.1 403"), "{lan}");
        let get = ask(&path, "GET http://example.org/ HTTP/1.1\r\n\r\n").await?;
        assert!(get.starts_with("HTTP/1.1 405"), "{get}");
        std::fs::remove_dir_all(dir)
    }
}
