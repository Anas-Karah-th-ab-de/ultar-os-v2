//! The text control socket of earlier releases (`ultar-ctl`, the admin
//! web page, scripts): one line in, one answer out. Root only (mode
//! `0600`); kept for those tools – the bus service `init` is the
//! interface for new ones.
//!
//! `status | start <s> | stop <s> | restart <s> | reload | uptime |
//! poweroff | reboot`; anything else goes to the caller's hook (the
//! emergency exit `rescue` of PID 1).

use crate::daemon::Halt;
use crate::supervisor::{ControlError, Supervisor};
use std::io::{BufRead, BufReader, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::{Duration, Instant};
use ultar_bus::messages::init::Action;

/// Longest time a client may take to send its line.
const READ_TIMEOUT: Duration = Duration::from_millis(300);
/// Longest accepted line.
const MAX_LINE: u64 = 256;

/// The listening socket.
pub struct Ctl {
    listener: UnixListener,
}

impl Ctl {
    /// Binds `path` (a stale socket there is replaced), mode `0600`.
    pub fn bind(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if std::fs::symlink_metadata(path).is_ok_and(|meta| {
            use std::os::unix::fs::FileTypeExt;
            meta.file_type().is_socket()
        }) {
            std::fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        Ok(Self { listener })
    }

    /// The descriptor, for `poll`.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.listener.as_fd()
    }

    /// The connections that wait (at most a few per wake-up).
    pub fn accept_all(&self) -> Vec<UnixStream> {
        (0..8)
            .map_while(|_| self.listener.accept().ok().map(|(stream, _)| stream))
            .collect()
    }
}

fn uptime() -> String {
    let seconds = ultar_sys::machine::uptime()
        .unwrap_or_default()
        .as_secs_f64();
    format!("{seconds:.2}")
}

fn control_reply(supervisor: &mut Supervisor, action: Action, name: &str, now: Instant) -> String {
    match supervisor.control(action, name, now) {
        Ok(state) => format!("{name}: {} {}\n", action.word(), state.word()),
        Err(ControlError::Unknown) => format!("error: unknown service {name}\n"),
        Err(ControlError::Already) => format!("error: {name} already {}\n", action.word()),
        Err(ControlError::ShuttingDown) => "error: shutting down\n".to_owned(),
    }
}

/// What a command asks of the loop besides its answer.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// End init.
    pub halt: Option<Halt>,
    /// Read policy and service files again.
    pub reload: bool,
}

/// Reads one command from `stream` and answers it.
pub fn handle(
    stream: UnixStream,
    supervisor: &mut Supervisor,
    now: Instant,
    extra: &mut impl FnMut(&str, &str) -> Option<(String, Option<Halt>)>,
    pid1: bool,
) -> Outcome {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let mut line = String::new();
    if BufReader::new((&stream).take_limited())
        .read_line(&mut line)
        .is_err()
    {
        return Outcome::default();
    }
    let mut words = line.split_whitespace();
    let (command, argument) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    let mut halt = None;
    let mut reload = false;
    let reply = match command {
        "status" => supervisor.status_text(),
        "reload" => {
            reload = true;
            "reload\n".to_owned()
        }
        "start" => control_reply(supervisor, Action::Start, argument, now),
        "stop" => control_reply(supervisor, Action::Stop, argument, now),
        "restart" => control_reply(supervisor, Action::Restart, argument, now),
        "uptime" => format!("{}\n", uptime()),
        "poweroff" | "reboot" => {
            halt = Some(match (pid1, command) {
                (false, _) => Halt::Exit,
                (true, "poweroff") => Halt::PowerOff,
                (true, _) => Halt::Reboot,
            });
            format!("{command} …\n")
        }
        other => match extra(other, argument) {
            Some((text, requested)) => {
                halt = requested;
                text
            }
            None => "commands: status | start <s> | stop <s> | restart <s> | reload | uptime | poweroff | reboot\n".to_owned(),
        },
    };
    let _ = (&stream).write_all(reply.as_bytes());
    Outcome { halt, reload }
}

trait TakeLimited: Sized {
    fn take_limited(self) -> std::io::Take<Self>;
}

impl<'a> TakeLimited for &'a UnixStream {
    fn take_limited(self) -> std::io::Take<&'a UnixStream> {
        std::io::Read::take(self, MAX_LINE)
    }
}
