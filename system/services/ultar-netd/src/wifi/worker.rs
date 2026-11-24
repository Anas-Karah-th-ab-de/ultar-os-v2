//! The Wi-Fi core on its own thread: it sleeps in `poll` on the radio's
//! socket, the DHCP socket and a wake-up socket until the next deadline,
//! and tells the service loop whenever the connection changed.

use super::core::{Command, WifiCore};
use rustix::event::{poll, PollFd, PollFlags, Timespec};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use ultar_bus::messages::net::WifiLink;

/// Longest sleep without anything due.
const IDLE_MS: u64 = 60_000;

/// Called with the new connection state.
pub type OnChange = Box<dyn Fn(WifiLink) + Send>;

/// The service's side of the worker.
pub struct WifiHandle {
    tx: Sender<Command>,
    wake: Arc<Mutex<UnixStream>>,
    stop: Arc<AtomicBool>,
}

impl WifiHandle {
    /// Hands a command to the worker; when it has ended the command is
    /// answered as refused.
    pub fn send(&self, command: Command) {
        if let Err(e) = self.tx.send(command) {
            e.0.refuse();
        }
        self.poke();
    }

    fn poke(&self) {
        if let Ok(mut w) = self.wake.lock() {
            let _ = w.write(&[1]);
        }
    }
}

impl Drop for WifiHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.poke();
    }
}

/// Starts the worker with `core`; `changed` hears every new state (the first
/// right away).
pub fn start(core: WifiCore, changed: OnChange) -> std::io::Result<WifiHandle> {
    let (tx, rx) = channel();
    let (wake_tx, wake_rx) = UnixStream::pair()?;
    wake_tx.set_nonblocking(true)?;
    wake_rx.set_nonblocking(true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    std::thread::Builder::new()
        .name("wifi".into())
        .stack_size(256 * 1024)
        .spawn(move || run(core, &rx, wake_rx, &thread_stop, &changed))?;
    Ok(WifiHandle {
        tx,
        wake: Arc::new(Mutex::new(wake_tx)),
        stop,
    })
}

fn wait(core: &WifiCore, wake: &UnixStream, ms: u64) {
    let timeout = Timespec {
        tv_sec: (ms / 1000) as i64,
        tv_nsec: ((ms % 1000) * 1_000_000) as i64,
    };
    let mut fds = vec![PollFd::new(wake, PollFlags::IN)];
    if let Some(fd) = core.driver.as_ref().and_then(|d| d.fd()) {
        fds.push(PollFd::from_borrowed_fd(fd, PollFlags::IN));
    }
    if let Some(fd) = core.ip.fd() {
        fds.push(PollFd::from_borrowed_fd(fd, PollFlags::IN));
    }
    let _ = poll(&mut fds, Some(&timeout));
}

fn run(
    mut core: WifiCore,
    rx: &Receiver<Command>,
    mut wake: UnixStream,
    stop: &AtomicBool,
    changed: &OnChange,
) {
    let started = Instant::now();
    let now = || started.elapsed().as_millis() as u64;
    let mut last = core.status();
    changed(last.clone());
    while !stop.load(Ordering::SeqCst) {
        let t = now();
        let due = core
            .next_deadline()
            .map_or(IDLE_MS, |d| d.saturating_sub(t).min(IDLE_MS));
        wait(&core, &wake, due);
        let mut buf = [0u8; 64];
        while matches!(wake.read(&mut buf), Ok(n) if n > 0) {}
        let t = now();
        for command in rx.try_iter() {
            core.command(command, t);
        }
        for _ in 0..64 {
            let Some(event) = core
                .driver
                .as_mut()
                .and_then(|d| d.next_event().ok().flatten())
            else {
                break;
            };
            core.event(event, t);
        }
        core.tick(t);
        let status = core.status();
        if status != last {
            changed(status.clone());
            last = status;
        }
    }
    core.leave(ultar_bus::messages::net::JoinOutcome::Failed);
}
