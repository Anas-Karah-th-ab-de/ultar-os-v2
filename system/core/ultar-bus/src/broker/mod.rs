//! The broker: accepts connections on Unix domain sockets, authenticates
//! them, enforces the policy and routes requests and events.
//!
//! The broker uses blocking I/O only. Every thread sleeps in `accept`,
//! `read` or a queue until there is work, so an idle bus causes no wakeups.

mod conn;
mod route;
#[cfg(test)]
mod route_tests;
mod state;

use crate::error::{BusError, ErrorCode};
use crate::identity::PeerIdentity;
use crate::policy::Policy;
use crate::sync::{lock, replace, wait_timeout};
use crate::wire::DEFAULT_MAX_FRAME;
use state::State;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Stack size of the per-connection reader thread.
const READER_STACK: usize = 256 * 1024;
/// Pause after a failed `accept` (e.g. out of file descriptors).
const ACCEPT_RETRY: Duration = Duration::from_millis(50);
/// Connections that may be in the handshake beyond `max_connections`.
const HANDSHAKE_SLACK: usize = 16;

/// Limits and timeouts of a broker.
#[derive(Clone, Debug)]
pub struct BrokerConfig {
    /// Largest accepted frame body in bytes.
    pub max_frame: u32,
    /// Frames that may wait for one client before it counts as too slow.
    pub queue_capacity: usize,
    /// Most authenticated connections at the same time.
    pub max_connections: usize,
    /// Most subscriptions per connection.
    pub max_subscriptions: usize,
    /// Most topics with a retained value.
    pub max_retained: usize,
    /// How long a new connection may take to authenticate.
    pub handshake_timeout: Duration,
    /// How long one write to a client may block before it is dropped.
    pub write_timeout: Duration,
    /// How long the broker remembers an unanswered request.
    pub pending_ttl: Duration,
}

impl Default for BrokerConfig {
    fn default() -> Self {
        Self {
            max_frame: DEFAULT_MAX_FRAME,
            queue_capacity: 1024,
            max_connections: 256,
            max_subscriptions: 256,
            max_retained: 256,
            handshake_timeout: Duration::from_secs(2),
            write_timeout: Duration::from_secs(2),
            pending_ttl: Duration::from_secs(60),
        }
    }
}

/// Counters describing a running broker.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BrokerStats {
    /// Authenticated connections.
    pub connections: usize,
    /// Registered services.
    pub services: usize,
    /// Active subscriptions.
    pub subscriptions: usize,
    /// Topics with a retained value.
    pub retained: usize,
    /// Requests waiting for an answer.
    pub pending_requests: usize,
    /// Clients dropped because they were too slow, since start.
    pub dropped_slow: u64,
}

/// State shared by all broker threads.
pub(crate) struct Shared {
    pub(crate) config: BrokerConfig,
    pub(crate) policy: RwLock<Policy>,
    pub(crate) state: Mutex<State>,
    pub(crate) stopping: AtomicBool,
    pub(crate) next_conn: AtomicU64,
    live_threads: Mutex<usize>,
    all_done: Condvar,
}

/// Decrements the live connection thread count when dropped.
struct LiveGuard(Arc<Shared>);

impl Drop for LiveGuard {
    fn drop(&mut self) {
        let mut live = lock(&self.0.live_threads);
        *live = live.saturating_sub(1);
        self.0.all_done.notify_all();
    }
}

struct ListenerSpec {
    path: PathBuf,
    auth: Arc<dyn PeerIdentity>,
    mode: Option<u32>,
}

/// Collects the settings of a broker before it starts.
pub struct BrokerBuilder {
    policy: Policy,
    config: BrokerConfig,
    listeners: Vec<ListenerSpec>,
}

impl BrokerBuilder {
    /// Replaces the default limits and timeouts.
    pub fn config(mut self, config: BrokerConfig) -> Self {
        self.config = config;
        self
    }

    /// Adds a socket; peers connecting to it are identified by `auth`.
    pub fn listen(self, path: &Path, auth: Arc<dyn PeerIdentity>) -> Self {
        self.add(path, auth, None)
    }

    /// Like [`Self::listen`], and sets the permission bits of the socket
    /// file (e.g. `0o600`) so the kernel restricts who may connect.
    pub fn listen_with_mode(self, path: &Path, auth: Arc<dyn PeerIdentity>, mode: u32) -> Self {
        self.add(path, auth, Some(mode))
    }

    fn add(mut self, path: &Path, auth: Arc<dyn PeerIdentity>, mode: Option<u32>) -> Self {
        self.listeners.push(ListenerSpec {
            path: path.to_owned(),
            auth,
            mode,
        });
        self
    }

    /// Binds all sockets and starts serving.
    pub fn start(self) -> Result<Broker, BusError> {
        let shared = Arc::new(Shared {
            config: self.config,
            policy: RwLock::new(self.policy),
            state: Mutex::new(State::new()),
            stopping: AtomicBool::new(false),
            next_conn: AtomicU64::new(1),
            live_threads: Mutex::new(0),
            all_done: Condvar::new(),
        });
        let mut broker = Broker {
            shared,
            acceptors: Vec::new(),
            paths: Vec::new(),
        };
        for spec in self.listeners {
            // On failure `broker` is dropped, which stops what already runs.
            broker.open(spec)?;
        }
        Ok(broker)
    }
}

/// Binds `path`, replacing a socket file no broker listens on any more.
fn bind(path: &Path) -> Result<UnixListener, BusError> {
    match UnixListener::bind(path) {
        Ok(listener) => Ok(listener),
        Err(err) if err.kind() == ErrorKind::AddrInUse => {
            if UnixStream::connect(path).is_ok() {
                return Err(BusError::AddressInUse);
            }
            std::fs::remove_file(path).map_err(BusError::Io)?;
            UnixListener::bind(path).map_err(BusError::Io)
        }
        Err(err) => Err(BusError::Io(err)),
    }
}

/// A running broker. Dropping it shuts it down.
pub struct Broker {
    shared: Arc<Shared>,
    acceptors: Vec<JoinHandle<()>>,
    paths: Vec<PathBuf>,
}

impl Broker {
    /// Starts describing a broker that enforces `policy`.
    pub fn builder(policy: Policy) -> BrokerBuilder {
        BrokerBuilder {
            policy,
            config: BrokerConfig::default(),
            listeners: Vec::new(),
        }
    }

    fn open(&mut self, spec: ListenerSpec) -> Result<(), BusError> {
        let listener = bind(&spec.path)?;
        self.paths.push(spec.path.clone());
        if let Some(mode) = spec.mode {
            std::fs::set_permissions(&spec.path, std::fs::Permissions::from_mode(mode))
                .map_err(BusError::Io)?;
        }
        let shared = self.shared.clone();
        let acceptor = thread::Builder::new()
            .name("ultar-bus-accept".into())
            .spawn(move || accept_loop(&shared, &listener, &spec.auth))
            .map_err(BusError::Io)?;
        self.acceptors.push(acceptor);
        Ok(())
    }

    /// Socket paths this broker serves.
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Replaces the policy; it applies to all following operations.
    pub fn set_policy(&self, policy: Policy) {
        replace(&self.shared.policy, policy);
    }

    /// Current counters.
    pub fn stats(&self) -> BrokerStats {
        let state = lock(&self.shared.state);
        BrokerStats {
            connections: state.conns.len(),
            services: state.services.len(),
            subscriptions: state.subscription_count(),
            retained: state.retained.len(),
            pending_requests: state.pending.len(),
            dropped_slow: state.dropped_slow,
        }
    }

    /// Stops accepting, tells every client `ShuttingDown`, waits for the
    /// connection threads and removes the socket files.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        if self.shared.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        for entry in lock(&self.shared.state).conns.values() {
            entry.control.close_with(ErrorCode::ShuttingDown);
        }
        for path in &self.paths {
            // Wakes the acceptor out of its blocking `accept`.
            let _ = UnixStream::connect(path);
        }
        for acceptor in self.acceptors.drain(..) {
            let _ = acceptor.join();
        }
        let config = &self.shared.config;
        let deadline = Instant::now() + config.write_timeout + config.handshake_timeout;
        let mut live = lock(&self.shared.live_threads);
        while *live > 0 {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            live = wait_timeout(&self.shared.all_done, live, deadline - now);
        }
        drop(live);
        for path in self.paths.drain(..) {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept_loop(shared: &Arc<Shared>, listener: &UnixListener, auth: &Arc<dyn PeerIdentity>) {
    for incoming in listener.incoming() {
        if shared.stopping.load(Ordering::SeqCst) {
            return;
        }
        match incoming {
            Ok(stream) => spawn_connection(shared, stream, auth),
            Err(_) => thread::sleep(ACCEPT_RETRY),
        }
    }
}

fn spawn_connection(shared: &Arc<Shared>, stream: UnixStream, auth: &Arc<dyn PeerIdentity>) {
    {
        let mut live = lock(&shared.live_threads);
        if *live
            >= shared
                .config
                .max_connections
                .saturating_add(HANDSHAKE_SLACK)
        {
            return;
        }
        *live += 1;
    }
    let guard = LiveGuard(shared.clone());
    let auth = auth.clone();
    // If the thread cannot be created the closure is dropped, and with it
    // the guard and the stream.
    let _ = thread::Builder::new()
        .name("ultar-bus-conn".into())
        .stack_size(READER_STACK)
        .spawn(move || {
            conn::serve(&guard.0, stream, auth.as_ref());
            drop(guard);
        });
}
