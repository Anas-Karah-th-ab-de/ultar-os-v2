//! The page cell of the engine host: **no network of its own**, the only way out is the browser's
//! Tor socket. Built by the host itself before Servo starts, in two stages like the app launcher
//! (`system/core/ultar-launcher/src/cell`), without `unsafe` (the calls are `ultar-sys`):
//!
//! 1. *Outside* (the process the browser started): checks the plan, joins a memory group,
//!    creates new mount, pid, ipc and **network** namespaces (a user namespace first when it is
//!    not root), starts itself again as stage 2 (`--cell-init ROOT`) and waits for it.
//! 2. *Inside* (pid 1 of the cell): builds the view ([`view`]), switches the root, brings up the
//!    private loopback, forbids new privileges, drops all capabilities and Ultar rights, starts
//!    the relay ([`crate::relay`]) and goes on as the engine, with Servo's proxy pointed at the
//!    relay's loopback port.
//!
//! Every failing step ends the process before Servo exists (fail closed). Where the kernel
//! cannot make a network namespace (the Ultar kernel today) the host refuses to start with a
//! cell: it never runs with network instead (`docs/integration/servo-cell.md`).

mod selftest;
mod stage;
pub mod view;

use crate::args::Args;
use std::fmt;
use std::path::{Path, PathBuf};
use ultar_web::cell::{CellError, CellSpec, DEFAULT_MEMORY_MB};

/// Longest path of the proxy socket (the address field of a Unix socket holds 108 bytes).
pub const MAX_SOCKET_PATH: usize = 100;

/// What the host has to build, from the command line.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// Folders visible read-only (the engine bundle, system libraries).
    pub read_only: Vec<PathBuf>,
    /// Folders visible read-write (the engine's profile, downloads).
    pub read_write: Vec<PathBuf>,
    /// Memory limit of the whole cell in MiB.
    pub memory_mb: u32,
    /// The browser's Unix socket for HTTP `CONNECT`: inside one of the read-write folders.
    pub socket: PathBuf,
}

/// Why the command line is not a cell the host will build.
#[derive(Debug, PartialEq, Eq)]
pub enum PlanError {
    /// `--cell-net`: a page cell never gets network.
    Network,
    /// A cell needs `--proxy-socket`.
    NeedsSocket,
    /// `--proxy` (a TCP port) next to a cell: the engine would have to reach it by network.
    ProxyInCell,
    /// A cell option (`--proxy-socket`, `--cell-init`, `--cell-selftest`) without a cell.
    WithoutCell,
    /// The socket path is relative, unclean or too long.
    BadSocket(PathBuf),
    /// The socket is not inside a read-write folder of the cell.
    SocketOutside(PathBuf),
    /// The folders or the memory limit are not acceptable (`ultar_web::cell`).
    Spec(CellError),
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::Network => f.write_str("a page cell never gets network (--cell-net)"),
            PlanError::NeedsSocket => f.write_str("a cell needs --proxy-socket"),
            PlanError::ProxyInCell => f.write_str("--proxy does not go with a cell (no network)"),
            PlanError::WithoutCell => f.write_str("cell options without a cell"),
            PlanError::BadSocket(p) => write!(f, "bad proxy socket path: {}", p.display()),
            PlanError::SocketOutside(p) => {
                write!(
                    f,
                    "proxy socket outside the cell's folders: {}",
                    p.display()
                )
            }
            PlanError::Spec(e) => write!(f, "{e}"),
        }
    }
}

/// Why the cell could not be built.
#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    /// The command line.
    Plan(PlanError),
    /// A step of the build (its name, what the system said).
    Step(&'static str, String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Plan(e) => write!(f, "cell refused: {e}"),
            Failure::Step(step, why) => write!(f, "cell failed at {step}: {why}"),
        }
    }
}

/// What [`enter`] decided.
#[derive(Debug, PartialEq, Eq)]
pub enum Entered {
    /// No cell (`--no-cell`, development): go on with `--proxy`.
    Uncelled,
    /// Inside the finished cell: the proxy address Servo must use (the relay's loopback port).
    Inside(String),
    /// The work is done here (stage 1 after the cell ended, or the self-test): exit with this code.
    Finished(u8),
}

fn clean_socket(path: &Path) -> bool {
    let text = path.to_string_lossy();
    path.is_absolute()
        && text.len() <= MAX_SOCKET_PATH
        && text
            .split('/')
            .skip(1)
            .all(|s| !s.is_empty() && s != "." && s != "..")
}

/// Makes the plan from the command line: `Ok(None)` for development without a cell. Pure: it
/// looks at nothing on the system.
pub fn plan(args: &Args) -> Result<Option<Plan>, PlanError> {
    let internal = args.proxy_socket.is_some() || args.cell_init.is_some() || args.selftest;
    if args.cell.none {
        return if internal {
            Err(PlanError::WithoutCell)
        } else {
            Ok(None)
        };
    }
    if args.cell.network {
        return Err(PlanError::Network);
    }
    if args.proxy.is_some() {
        return Err(PlanError::ProxyInCell);
    }
    let socket = args.proxy_socket.clone().ok_or(PlanError::NeedsSocket)?;
    let spec = CellSpec {
        network: false,
        read_only: args.cell.read_only.clone(),
        read_write: args.cell.read_write.clone(),
        memory_mb: args.cell.memory_mb.unwrap_or(DEFAULT_MEMORY_MB),
    };
    spec.validate().map_err(PlanError::Spec)?;
    if !clean_socket(&socket) {
        return Err(PlanError::BadSocket(socket));
    }
    if !spec.read_write.iter().any(|dir| socket.starts_with(dir)) {
        return Err(PlanError::SocketOutside(socket));
    }
    Ok(Some(Plan {
        read_only: spec.read_only,
        read_write: spec.read_write,
        memory_mb: spec.memory_mb,
        socket,
    }))
}

/// Builds the cell around this process if the command line asks for one (see the module).
pub fn enter(args: &Args) -> Result<Entered, Failure> {
    let Some(plan) = plan(args).map_err(Failure::Plan)? else {
        return Ok(Entered::Uncelled);
    };
    match &args.cell_init {
        None => stage::outside(&plan).map(Entered::Finished),
        Some(root) => stage::inside(args, &plan, root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::parse;
    use std::ffi::OsString;

    fn args(words: &[&str]) -> Args {
        parse(words.iter().map(OsString::from)).unwrap()
    }

    fn cell(extra: &[&str]) -> Args {
        let mut w = vec![
            "--cell-ro",
            "/usr/lib/ultar/web",
            "--cell-rw",
            "/p/engine",
            "--cell-rw",
            "/p/Downloads",
            "--cell-memory-mb",
            "2048",
            "--proxy-socket",
            "/p/engine/tor.sock",
        ];
        w.extend_from_slice(extra);
        args(&w)
    }

    #[test]
    fn the_cell_the_browser_asks_for_is_planned() {
        let p = plan(&cell(&[])).unwrap().unwrap();
        assert_eq!(p.memory_mb, 2048);
        assert_eq!(p.socket, PathBuf::from("/p/engine/tor.sock"));
        assert_eq!(p.read_only, [PathBuf::from("/usr/lib/ultar/web")]);
        assert_eq!(p.read_write.len(), 2);
        // without a memory option the default of the browser's page cell applies
        let mut a = cell(&[]);
        a.cell.memory_mb = None;
        assert_eq!(plan(&a).unwrap().unwrap().memory_mb, DEFAULT_MEMORY_MB);
    }

    #[test]
    fn development_without_a_cell_is_planned_as_none_and_never_with_cell_options() {
        assert_eq!(
            plan(&args(&["--no-cell", "--proxy", "http://u:p@127.0.0.1:9"])),
            Ok(None)
        );
        assert_eq!(
            plan(&args(&["--no-cell", "--proxy-socket", "/p/s"])),
            Err(PlanError::WithoutCell)
        );
        assert_eq!(
            plan(&args(&["--no-cell", "--cell-selftest"])),
            Err(PlanError::WithoutCell)
        );
        assert_eq!(
            plan(&args(&["--no-cell", "--cell-init", "/tmp/r"])),
            Err(PlanError::WithoutCell)
        );
    }

    #[test]
    fn a_cell_never_gets_network_and_needs_its_socket() {
        assert_eq!(plan(&cell(&["--cell-net"])), Err(PlanError::Network));
        assert_eq!(
            plan(&cell(&["--proxy", "http://u:p@127.0.0.1:9"])),
            Err(PlanError::ProxyInCell)
        );
        let mut a = cell(&[]);
        a.proxy_socket = None;
        assert_eq!(plan(&a), Err(PlanError::NeedsSocket));
    }

    #[test]
    fn the_socket_must_be_a_clean_path_inside_a_writable_folder_of_the_cell() {
        let with = |socket: &str| {
            let mut a = cell(&[]);
            a.proxy_socket = Some(socket.into());
            plan(&a)
        };
        assert!(matches!(with("tor.sock"), Err(PlanError::BadSocket(_))));
        assert!(matches!(
            with("/p/engine/../x"),
            Err(PlanError::BadSocket(_))
        ));
        let long = format!("/p/engine/{}", "x".repeat(120));
        assert!(matches!(with(&long), Err(PlanError::BadSocket(_))));
        assert!(matches!(
            with("/tmp/tor.sock"),
            Err(PlanError::SocketOutside(_))
        ));
        // a read-only folder is not a place for the socket
        assert!(matches!(
            with("/usr/lib/ultar/web/tor.sock"),
            Err(PlanError::SocketOutside(_))
        ));
        assert!(with("/p/Downloads/tor.sock").is_ok());
    }

    #[test]
    fn bad_folders_and_limits_are_refused_by_the_browser_rules() {
        let mut a = cell(&[]);
        a.cell.read_only = vec!["/".into()];
        assert!(matches!(
            plan(&a),
            Err(PlanError::Spec(CellError::BadPath(_)))
        ));
        let mut a = cell(&[]);
        a.cell.memory_mb = Some(0);
        assert_eq!(plan(&a), Err(PlanError::Spec(CellError::BadMemory)));
        let mut a = cell(&[]);
        a.cell.read_write = vec!["/p".into(), "/p/engine".into()];
        assert!(matches!(
            plan(&a),
            Err(PlanError::Spec(CellError::Overlap(_, _)))
        ));
        assert!(!Failure::Plan(PlanError::Network).to_string().is_empty());
        assert!(Failure::Step("mounts", "x".into())
            .to_string()
            .contains("mounts"));
    }

    #[test]
    fn a_plan_without_a_cell_does_not_enter_one() {
        assert_eq!(
            enter(&args(&["--no-cell", "--proxy", "http://u:p@127.0.0.1:9"])),
            Ok(Entered::Uncelled)
        );
        assert_eq!(
            enter(&cell(&["--cell-net"])),
            Err(Failure::Plan(PlanError::Network))
        );
    }
}
