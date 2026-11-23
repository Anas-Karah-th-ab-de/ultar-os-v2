//! Stop and reload signals without a C library.
//!
//! The signals `SIGTERM`, `SIGINT` and `SIGHUP` are **blocked** in the main
//! thread before any other thread exists (every later thread inherits the
//! mask) and read as data from a `signalfd`. No signal handler ever runs,
//! so there is no async-signal-safety problem and no `unsafe` in this crate
//! (the calls come from the HAL crate `ultar-sys`).
//!
//! * first `SIGTERM`/`SIGINT` → [`Lifecycle::request_stop`]; a watchdog
//!   ends the process with [`crate::exit::DEADLINE`] if the service has not
//!   reached `Stopped` within the configured deadline;
//! * second `SIGTERM`/`SIGINT` → the process ends at once with
//!   [`crate::exit::FORCED`];
//! * `SIGHUP` → [`Lifecycle::reload`].
//!
//! # Why the main thread reads the signals
//!
//! Both mechanisms exist in the Ultar kernel (`rt_sigprocmask` = call 14,
//! `signalfd`/`signalfd4` = calls 282/289, `poll`/`ppoll` on the
//! descriptor). But where Linux keeps a signal that every thread blocks
//! pending for the whole process, the Ultar kernel (state 23.11.2026,
//! `signal.rs::send_locked`) marks it pending on the **first thread of the
//! group** only, and `read`/`poll` on a `signalfd` look at the pending set
//! of the calling thread. A dedicated signal thread – the usual design –
//! would never see a signal there. The first thread of the group is the
//! main thread, so the runtime reads the descriptor **on the main thread**
//! ([`MainSource`]) and runs the service's work on a second thread. That
//! is correct on Linux as well.

use crate::exit;
use crate::lifecycle::{Lifecycle, Signal, State, StopCause};
use std::fs::File;
use std::io::{PipeReader, PipeWriter, Read, Write};
use std::os::fd::AsFd;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use ultar_sys::{self as sys, Errno};

/// Size of one `signalfd_siginfo` record.
const SIGINFO_BYTES: usize = 128;

/// Where signals come from. The real source is a `signalfd`; tests use a
/// channel.
pub trait SignalSource: Send {
    /// Blocks until the next signal; `None` ends the watcher.
    fn next(&mut self) -> Option<Signal>;
}

/// Ends the process. Replaced in tests.
pub type Exiter = Arc<dyn Fn(i32) + Send + Sync>;

/// How the watcher reacts.
#[derive(Clone)]
pub struct SignalConfig {
    /// Time the service has from the stop signal to state `Stopped`.
    pub deadline: Duration,
    /// Called with the exit code when the deadline passes or a second stop
    /// signal arrives.
    pub exit: Exiter,
}

impl SignalConfig {
    /// The given deadline and a real process exit.
    pub fn new(deadline: Duration) -> Self {
        Self {
            deadline,
            exit: Arc::new(|code| std::process::exit(code)),
        }
    }
}

/// The kernel-backed signal source.
pub struct SignalFd {
    file: File,
}

impl SignalFd {
    /// Blocks the three signals in the calling thread and opens the
    /// descriptor. **Call before the first thread is spawned.**
    pub fn open() -> Result<Self, Errno> {
        let mask = [Signal::Hangup, Signal::Interrupt, Signal::Terminate]
            .iter()
            .fold(0u64, |mask, signal| mask | 1 << (signal.number() - 1));
        sys::block_signals(mask)?;
        Ok(Self {
            file: File::from(sys::signal_fd(mask)?),
        })
    }
}

impl SignalSource for SignalFd {
    fn next(&mut self) -> Option<Signal> {
        loop {
            let mut info = [0u8; SIGINFO_BYTES];
            self.file.read_exact(&mut info).ok()?;
            let number = u32::from_le_bytes([info[0], info[1], info[2], info[3]]);
            if let Some(signal) = Signal::from_number(number) {
                return Some(signal);
            }
        }
    }
}

/// Waits for the service to stop; ends the process when it takes too long.
fn watchdog(lifecycle: &Lifecycle, config: &SignalConfig) {
    if !lifecycle.wait_for_timeout(State::Stopped, config.deadline) {
        (config.exit)(exit::DEADLINE);
    }
}

/// Handles signals from `source` until it ends. The runtime calls this on
/// the main thread; it is public so that it can be driven with a scripted
/// source.
pub fn watch(mut source: impl SignalSource, lifecycle: &Lifecycle, config: &SignalConfig) {
    let mut stop_seen = false;
    while let Some(signal) = source.next() {
        match signal {
            Signal::Hangup => {
                lifecycle.reload();
            }
            Signal::Interrupt | Signal::Terminate if stop_seen => {
                (config.exit)(exit::FORCED);
            }
            Signal::Interrupt | Signal::Terminate => {
                stop_seen = true;
                let (life, watch_config) = (lifecycle.clone(), config.clone());
                // Started before the hooks run: a hook that hangs must not
                // keep the deadline from being enforced.
                let spawned = thread::Builder::new()
                    .name("ultar-stop-deadline".into())
                    .spawn(move || watchdog(&life, &watch_config));
                lifecycle.request_stop(StopCause::Signal(signal));
                if spawned.is_err() && !lifecycle.wait_for_timeout(State::Stopped, config.deadline)
                {
                    (config.exit)(exit::DEADLINE);
                }
            }
        }
    }
}

/// Ends a [`MainSource`]: the service's work is done, the main thread
/// can stop waiting for signals. Dropping the waker wakes as well, so a
/// work thread that ends in any way releases the main thread.
pub struct Waker {
    writer: PipeWriter,
}

impl Waker {
    /// Wakes the main thread.
    pub fn wake(mut self) {
        let _ = self.writer.write_all(b"x");
    }
}

/// The signal source of the main thread: the `signalfd`, and a pipe that
/// ends the wait when the service's work is done.
pub struct MainSource {
    signals: SignalFd,
    done: PipeReader,
}

impl MainSource {
    /// Blocks the three signals in the calling thread and opens the
    /// descriptors. **Call on the main thread before any other thread is
    /// spawned.**
    pub fn open() -> Result<(Self, Waker), SignalError> {
        let signals = SignalFd::open().map_err(SignalError::Kernel)?;
        let (done, writer) = std::io::pipe().map_err(|_| SignalError::Pipe)?;
        Ok((Self { signals, done }, Waker { writer }))
    }
}

impl SignalSource for MainSource {
    fn next(&mut self) -> Option<Signal> {
        loop {
            let (signal, done) =
                sys::wait_readable(self.signals.file.as_fd(), self.done.as_fd()).ok()?;
            // A signal that arrived together with the end is still
            // handled: the caller asks again and then sees the end.
            if signal {
                return self.signals.next();
            }
            if done {
                // The work is done (a byte, or the waker was dropped).
                return None;
            }
        }
    }
}

/// Why signal handling could not be set up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignalError {
    /// The kernel refused `rt_sigprocmask` or `signalfd`.
    Kernel(Errno),
    /// The pipe that ends the wait could not be created.
    Pipe,
}

impl std::fmt::Display for SignalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kernel(errno) => write!(f, "signal setup refused by the kernel: {errno}"),
            Self::Pipe => f.write_str("wake pipe could not be created"),
        }
    }
}

impl std::error::Error for SignalError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Receiver};
    use std::sync::Mutex;

    struct Scripted(Receiver<Signal>);

    impl SignalSource for Scripted {
        fn next(&mut self) -> Option<Signal> {
            self.0.recv().ok()
        }
    }

    fn recording(deadline: Duration) -> (SignalConfig, Arc<Mutex<Vec<i32>>>) {
        let codes = Arc::new(Mutex::new(Vec::new()));
        let sink = codes.clone();
        let config = SignalConfig {
            deadline,
            exit: Arc::new(move |code| sink.lock().unwrap().push(code)),
        };
        (config, codes)
    }

    #[test]
    fn terminate_requests_a_stop_and_a_clean_stop_needs_no_exit() {
        let life = Lifecycle::new();
        let (config, codes) = recording(Duration::from_secs(3600));
        let (send, receive) = channel();
        let watcher = {
            let life = life.clone();
            thread::spawn(move || watch(Scripted(receive), &life, &config))
        };
        send.send(Signal::Terminate).unwrap();
        assert_eq!(life.wait_stop(), StopCause::Signal(Signal::Terminate));
        life.mark_stopped();
        drop(send);
        watcher.join().unwrap();
        assert!(codes.lock().unwrap().is_empty());
    }

    #[test]
    fn a_second_stop_signal_forces_the_exit() {
        let life = Lifecycle::new();
        let (config, codes) = recording(Duration::from_secs(3600));
        let (send, receive) = channel();
        send.send(Signal::Interrupt).unwrap();
        send.send(Signal::Terminate).unwrap();
        drop(send);
        watch(Scripted(receive), &life, &config);
        assert_eq!(*codes.lock().unwrap(), vec![exit::FORCED]);
        assert_eq!(
            life.stop_cause(),
            Some(StopCause::Signal(Signal::Interrupt))
        );
        life.mark_stopped();
    }

    #[test]
    fn a_missed_deadline_ends_the_process_with_its_own_code() {
        let life = Lifecycle::new();
        let (config, codes) = recording(Duration::ZERO);
        watchdog(&life, &config);
        assert_eq!(*codes.lock().unwrap(), vec![exit::DEADLINE]);
        // A stopped service is left alone.
        life.mark_stopped();
        watchdog(&life, &config);
        assert_eq!(codes.lock().unwrap().len(), 1);
    }

    #[test]
    fn hangup_reloads_and_does_not_stop() {
        let life = Lifecycle::new();
        let (config, codes) = recording(Duration::from_secs(3600));
        let (send, receive) = channel();
        send.send(Signal::Hangup).unwrap();
        send.send(Signal::Hangup).unwrap();
        drop(send);
        watch(Scripted(receive), &life, &config);
        assert_eq!(life.reloads(), 2);
        assert_eq!(life.state(), State::Starting);
        assert!(codes.lock().unwrap().is_empty());
    }

    #[test]
    fn errors_have_text() {
        assert!(!SignalError::Pipe.to_string().is_empty());
        assert!(!SignalError::Kernel(Errno::EPERM).to_string().is_empty());
        let _ = SignalConfig::new(Duration::from_secs(1));
    }
}
