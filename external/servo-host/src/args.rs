//! The command line of the host, exactly as `engine::remote::HostConfig::args` of the browser
//! writes it. Anything else is refused: the host is never started by hand in a release.

use std::ffi::OsString;
use std::path::PathBuf;

/// What the browser asked for.
#[derive(Debug, Default, PartialEq)]
pub struct Args {
    /// Profile folder of the engine (nothing is kept there: storage is temporary).
    pub profile: Option<PathBuf>,
    /// Downloads folder.
    pub downloads: Option<PathBuf>,
    /// Product token of the browser (only logged; pages see the persona's user agent).
    pub product: Option<String>,
    /// Compiled content-blocker list.
    pub filter: Option<PathBuf>,
    /// The only way out: the browser's Tor port for HTTP `CONNECT`.
    pub proxy: Option<String>,
    /// The only way out of a cell without network: the browser's Unix socket for HTTP
    /// `CONNECT` (`--proxy-socket`). The relay inside the cell leads Servo's proxy to it.
    pub proxy_socket: Option<PathBuf>,
    /// The cell the host must build before Servo starts.
    pub cell: Cell,
    /// Internal, set by the host itself for its second stage (`--cell-init ROOT`): the folder
    /// the cell's root is built on. Only accepted as the first process of a new pid namespace.
    pub cell_init: Option<PathBuf>,
    /// Test only (`--cell-selftest`): inside the finished cell check what the cell allows and
    /// report it, instead of starting Servo.
    pub selftest: bool,
}

/// The cell options (`--cell-ro`, `--cell-rw`, `--cell-net`, `--cell-memory-mb`, `--no-cell`).
#[derive(Debug, Default, PartialEq)]
pub struct Cell {
    /// Development only: no cell at all.
    pub none: bool,
    /// Folders visible read-only.
    pub read_only: Vec<PathBuf>,
    /// Folders visible read-write.
    pub read_write: Vec<PathBuf>,
    /// Network asked for (the host still reaches only the proxy).
    pub network: bool,
    /// Memory limit in MiB.
    pub memory_mb: Option<u32>,
}

impl Cell {
    /// Whether any cell option was given.
    pub fn requested(&self) -> bool {
        !self.read_only.is_empty()
            || !self.read_write.is_empty()
            || self.network
            || self.memory_mb.is_some()
    }
}

/// Parses the options; `None` for anything unknown or incomplete.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Option<Args> {
    let mut a = Args::default();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let text = arg.to_str()?;
        match text {
            "--profile" => a.profile = Some(it.next()?.into()),
            "--downloads" => a.downloads = Some(it.next()?.into()),
            "--user-agent" => a.product = Some(it.next()?.into_string().ok()?),
            "--filter" => a.filter = Some(it.next()?.into()),
            "--proxy" => a.proxy = Some(it.next()?.into_string().ok()?),
            "--proxy-socket" => a.proxy_socket = Some(it.next()?.into()),
            "--cell-init" => a.cell_init = Some(it.next()?.into()),
            "--cell-selftest" => a.selftest = true,
            "--cell-ro" => a.cell.read_only.push(it.next()?.into()),
            "--cell-rw" => a.cell.read_write.push(it.next()?.into()),
            "--cell-net" => a.cell.network = true,
            "--cell-memory-mb" => a.cell.memory_mb = Some(it.next()?.to_str()?.parse().ok()?),
            "--no-cell" => a.cell.none = true,
            _ => return None,
        }
    }
    // either a cell or explicitly none, never both
    (a.cell.none != a.cell.requested()).then_some(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn what_the_browser_writes_is_understood() {
        let a = parse(os(&[
            "--profile",
            "/p",
            "--downloads",
            "/d",
            "--user-agent",
            "UltarWeb/0.8",
            "--proxy",
            "http://u:p@127.0.0.1:9",
            "--cell-ro",
            "/usr/lib/ultar/web",
            "--cell-rw",
            "/p",
            "--cell-net",
            "--cell-memory-mb",
            "2048",
        ]))
        .unwrap();
        assert_eq!(a.proxy.as_deref(), Some("http://u:p@127.0.0.1:9"));
        assert_eq!(a.cell.memory_mb, Some(2048));
        assert!(a.cell.network && !a.cell.none);
        assert!(parse(os(&["--no-cell"])).unwrap().cell.none);
    }

    #[test]
    fn the_cell_options_of_the_second_stage_and_the_socket_are_understood() {
        let a = parse(os(&[
            "--proxy-socket",
            "/p/tor.sock",
            "--cell-rw",
            "/p",
            "--cell-init",
            "/tmp/root",
            "--cell-selftest",
        ]))
        .unwrap();
        assert_eq!(
            a.proxy_socket.as_deref(),
            Some(std::path::Path::new("/p/tor.sock"))
        );
        assert_eq!(
            a.cell_init.as_deref(),
            Some(std::path::Path::new("/tmp/root"))
        );
        assert!(a.selftest && a.proxy.is_none());
        assert!(parse(os(&["--cell-init"])).is_none());
    }

    #[test]
    fn anything_else_is_refused() {
        assert!(parse(os(&["--debug"])).is_none());
        assert!(parse(os(&["--profile"])).is_none());
        assert!(parse(os(&[])).is_none(), "neither a cell nor --no-cell");
        assert!(parse(os(&["--no-cell", "--cell-net"])).is_none());
        assert!(parse(os(&["--cell-memory-mb", "x"])).is_none());
    }
}
