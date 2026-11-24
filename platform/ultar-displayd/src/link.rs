//! The connection to the compositor on its own thread: it sleeps in `poll`
//! on the compositor socket and a wake-up socket (no timer while nothing
//! happens), forwards events to the service loop and sends the loop's
//! requests. When the compositor goes away it tells the loop and connects
//! again with a growing pause.

use crate::ports::CompositorPort;
use rustix::event::{poll, PollFd, PollFlags};
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_surface::client::Client;
use ultar_surface::proto::{Event, Request};

/// App id the service gives the compositor (a hint; rights come from its uid).
pub const APP_ID: &str = "org.ultar.displayd";
/// Requests waiting for the link thread at once.
pub const MAX_QUEUED: usize = 64;
/// Longest pause between connection attempts.
const MAX_PAUSE: Duration = Duration::from_secs(2);

/// What the link thread tells the loop.
#[derive(Debug)]
pub enum LinkEvent {
    /// Connected (again); events follow.
    Connected,
    /// An event of the compositor.
    Event(Event),
    /// The connection ended.
    Gone,
}

/// The loop's side: sends requests to the link thread.
#[derive(Clone)]
pub struct LinkHandle {
    requests: SyncSender<Request>,
    wake: Arc<Mutex<UnixStream>>,
    connected: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl LinkHandle {
    /// Ends the link thread.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.poke();
    }

    fn poke(&self) {
        if let Ok(mut w) = self.wake.lock() {
            let _ = w.write(&[1]);
        }
    }
}

impl CompositorPort for LinkHandle {
    fn send(&self, request: Request) -> bool {
        if !self.connected.load(Ordering::SeqCst) {
            return false;
        }
        match self.requests.try_send(request) {
            Ok(()) => {
                self.poke();
                true
            }
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
        }
    }
}

struct Thread {
    path: PathBuf,
    requests: Receiver<Request>,
    wake: UnixStream,
    events: Sender<crate::daemon::Input>,
    connected: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

/// Starts the link thread for the compositor socket at `path`.
pub fn start(
    path: PathBuf,
    events: Sender<crate::daemon::Input>,
) -> std::io::Result<(LinkHandle, std::thread::JoinHandle<()>)> {
    let (tx, rx) = std::sync::mpsc::sync_channel(MAX_QUEUED);
    let (wake_tx, wake_rx) = UnixStream::pair()?;
    wake_tx.set_nonblocking(true)?;
    wake_rx.set_nonblocking(true)?;
    let connected = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let handle = LinkHandle {
        requests: tx,
        wake: Arc::new(Mutex::new(wake_tx)),
        connected: connected.clone(),
        stop: stop.clone(),
    };
    let thread = Thread {
        path,
        requests: rx,
        wake: wake_rx,
        events,
        connected,
        stop,
    };
    let join = std::thread::Builder::new()
        .name("compositor".into())
        .stack_size(256 * 1024)
        .spawn(move || thread.run())?;
    Ok((handle, join))
}

impl Thread {
    fn run(mut self) {
        let mut pause = Duration::from_millis(50);
        while !self.stop.load(Ordering::SeqCst) {
            match Client::connect(&self.path, APP_ID) {
                Ok(client) => {
                    pause = Duration::from_millis(50);
                    self.connected.store(true, Ordering::SeqCst);
                    let _ = self.events.send(LinkEvent::Connected.into());
                    self.serve(client);
                    self.connected.store(false, Ordering::SeqCst);
                    while self.requests.try_recv().is_ok() {}
                    let _ = self.events.send(LinkEvent::Gone.into());
                }
                Err(_) => {
                    self.sleep(pause);
                    pause = (pause * 2).min(MAX_PAUSE);
                }
            }
        }
    }

    /// Waits on the wake-up socket only (a pause that a stop ends at once).
    fn sleep(&mut self, pause: Duration) {
        let mut fds = [PollFd::new(&self.wake, PollFlags::IN)];
        let timeout = rustix::event::Timespec {
            tv_sec: pause.as_secs() as i64,
            tv_nsec: i64::from(pause.subsec_nanos()),
        };
        let _ = poll(&mut fds, Some(&timeout));
        self.drain_wake();
    }

    fn drain_wake(&mut self) {
        let mut buf = [0u8; 64];
        while matches!(self.wake.read(&mut buf), Ok(n) if n > 0) {}
    }

    /// One connection: until it ends or the service stops.
    fn serve(&mut self, mut client: Client) {
        loop {
            loop {
                match client.next_event(Some(Duration::ZERO)) {
                    Ok(Some(event)) => {
                        if self.events.send(LinkEvent::Event(event).into()).is_err() {
                            return;
                        }
                    }
                    Ok(None) => break,
                    Err(_) => return,
                }
            }
            while let Ok(request) = self.requests.try_recv() {
                if client.send(&request).is_err() {
                    return;
                }
            }
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            let wake_in = {
                let (cfd, wfd) = (client.fd(), self.wake.as_fd());
                let mut fds = [
                    PollFd::new(&cfd, PollFlags::IN),
                    PollFd::new(&wfd, PollFlags::IN),
                ];
                if poll(&mut fds, None).is_err() {
                    return;
                }
                fds[1].revents()
            };
            if wake_in.contains(PollFlags::IN) {
                self.drain_wake();
            }
        }
    }
}
