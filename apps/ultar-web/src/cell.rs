//! The cell of the page processes (plan §3.7, `docs/integration/kernel-isolation-design.md`
//! §5): the engine host builds it before it loads the engine, and every web and network
//! process of the engine inherits it. The browser decides **what** the cell contains (this
//! module, tested); the host only carries it out (`external/wpe-webkit/host/cell.c`) and
//! refuses to start when it cannot (fail closed).
//!
//! A page cell sees: the engine bundle read-only, the engine's own folder (cookies, cache, and
//! the browser's Tor socket) and the downloads folder read-write, a private `/tmp`. **No
//! network**: the cell has a private loopback only, and the one way out is the Unix socket of
//! the browser's Tor client in the engine folder (`--proxy-socket`, [`proxy_socket`]). Nothing
//! else of the user – no home folder, no system data, no device nodes, not the browser's own
//! profile with the Tor state.

use std::ffi::OsString;
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// Most folders a cell lists.
pub const MAX_BINDS: usize = 8;
/// Default memory limit of a page cell (MiB).
pub const DEFAULT_MEMORY_MB: u32 = 2048;
/// File name of the browser's Tor socket in the engine's folder.
pub const PROXY_SOCKET: &str = "tor.sock";
/// File name of the browser's sound socket in the engine's folder (`media::relay`).
pub const AUDIO_SOCKET: &str = "audio.sock";

/// Where the browser makes its Tor socket for the engine in `engine` (the engine's folder,
/// which the cell sees read-write).
pub fn proxy_socket(engine: &Path) -> PathBuf {
    engine.join(PROXY_SOCKET)
}

/// Where the browser makes the sound socket of the engine in `engine` (the engine's folder):
/// the way of the cell's sound to the sound server.
pub fn audio_socket(engine: &Path) -> PathBuf {
    engine.join(AUDIO_SOCKET)
}

/// Why a cell description is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellError {
    /// A path is relative, contains `.`/`..` or is the root.
    BadPath(PathBuf),
    /// Two entries overlap (one lies inside the other).
    Overlap(PathBuf, PathBuf),
    /// More than [`MAX_BINDS`] folders.
    TooMany,
    /// Memory limit 0 or above 64 GiB.
    BadMemory,
}

impl fmt::Display for CellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CellError::BadPath(p) => write!(f, "cell path not allowed: {}", p.display()),
            CellError::Overlap(a, b) => {
                write!(f, "cell paths overlap: {} and {}", a.display(), b.display())
            }
            CellError::TooMany => f.write_str("too many cell paths"),
            CellError::BadMemory => f.write_str("cell memory limit out of range"),
        }
    }
}

impl std::error::Error for CellError {}

/// What a cell contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellSpec {
    /// Network access. A page cell has none: its only way out is the proxy socket.
    pub network: bool,
    /// Folders visible read-only (same path inside).
    pub read_only: Vec<PathBuf>,
    /// Folders visible read-write (same path inside).
    pub read_write: Vec<PathBuf>,
    /// Memory limit of the whole cell in MiB.
    pub memory_mb: u32,
}

fn clean(path: &Path) -> Result<(), CellError> {
    let bad = || CellError::BadPath(path.to_path_buf());
    if !path.is_absolute() || path.parent().is_none() {
        return Err(bad());
    }
    let text = path.to_string_lossy();
    let raw_ok = text
        .split('/')
        .skip(1)
        .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    let ok = raw_ok
        && path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)));
    if ok {
        Ok(())
    } else {
        Err(bad())
    }
}

impl CellSpec {
    /// The cell of the page processes: `bundle` read-only, the engine's folder `engine` (with
    /// the proxy socket, see [`proxy_socket`]) and `downloads` read-write, **without network**.
    pub fn page(bundle: &Path, engine: &Path, downloads: &Path) -> Result<CellSpec, CellError> {
        let spec = CellSpec {
            network: false,
            read_only: vec![bundle.to_path_buf()],
            read_write: vec![engine.to_path_buf(), downloads.to_path_buf()],
            memory_mb: DEFAULT_MEMORY_MB,
        };
        spec.validate()?;
        Ok(spec)
    }

    /// Checks the description: absolute clean paths, no root, no overlaps, limits.
    pub fn validate(&self) -> Result<(), CellError> {
        let all: Vec<&PathBuf> = self.read_only.iter().chain(&self.read_write).collect();
        if all.len() > MAX_BINDS {
            return Err(CellError::TooMany);
        }
        if self.memory_mb == 0 || self.memory_mb > 64 * 1024 {
            return Err(CellError::BadMemory);
        }
        for (i, a) in all.iter().enumerate() {
            clean(a)?;
            for b in &all[i + 1..] {
                if a.starts_with(b) || b.starts_with(a) {
                    return Err(CellError::Overlap((*a).clone(), (*b).clone()));
                }
            }
        }
        Ok(())
    }

    /// The host's command-line options for this cell.
    pub fn args(&self) -> Vec<OsString> {
        let mut args = Vec::new();
        for p in &self.read_only {
            args.push(OsString::from("--cell-ro"));
            args.push(p.clone().into_os_string());
        }
        for p in &self.read_write {
            args.push(OsString::from("--cell-rw"));
            args.push(p.clone().into_os_string());
        }
        if self.network {
            args.push(OsString::from("--cell-net"));
        }
        args.push(OsString::from("--cell-memory-mb"));
        args.push(OsString::from(self.memory_mb.to_string()));
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_cell_and_its_arguments() {
        let spec = CellSpec::page(
            Path::new("/usr/lib/ultar/web"),
            Path::new("/home/u/.local/share/ultar-web"),
            Path::new("/home/u/Downloads"),
        )
        .unwrap();
        assert!(!spec.network, "a page cell has no network of its own");
        assert_eq!(
            proxy_socket(Path::new("/home/u/.local/share/ultar-web")),
            PathBuf::from("/home/u/.local/share/ultar-web/tor.sock")
        );
        let args: Vec<String> = spec
            .args()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--cell-ro",
                "/usr/lib/ultar/web",
                "--cell-rw",
                "/home/u/.local/share/ultar-web",
                "--cell-rw",
                "/home/u/Downloads",
                "--cell-memory-mb",
                "2048"
            ]
        );
    }

    #[test]
    fn dangerous_cells_are_refused() {
        let p =
            |a: &str, b: &str, c: &str| CellSpec::page(Path::new(a), Path::new(b), Path::new(c));
        assert!(matches!(p("/", "/p", "/d"), Err(CellError::BadPath(_))));
        assert!(matches!(p("rel", "/p", "/d"), Err(CellError::BadPath(_))));
        assert!(matches!(
            p("/b/../etc", "/p", "/d"),
            Err(CellError::BadPath(_))
        ));
        assert!(matches!(
            p("/b/./x", "/p", "/d"),
            Err(CellError::BadPath(_))
        ));
        assert!(matches!(
            p("/home/u", "/home/u/p", "/d"),
            Err(CellError::Overlap(_, _))
        ));
        assert!(matches!(
            p("/b", "/home", "/home/Downloads"),
            Err(CellError::Overlap(_, _))
        ));
        let mut s = p("/b", "/p", "/d").unwrap();
        s.memory_mb = 0;
        assert_eq!(s.validate(), Err(CellError::BadMemory));
        s.memory_mb = 1;
        s.read_only = (0..MAX_BINDS)
            .map(|i| PathBuf::from(format!("/r{i}")))
            .collect();
        assert_eq!(s.validate(), Err(CellError::TooMany));
        assert!(!CellError::TooMany.to_string().is_empty());
    }
}
