//! The broker: accepts connections on Unix domain sockets, authenticates
//! them, enforces the policy and routes requests and events.
//!
//! The broker uses blocking I/O only. Every thread sleeps in `accept`,
//! `read` or a queue until there is work, so an idle bus causes no wakeups.

mod conn;
mod observe;
mod route;
#[cfg(test)]
mod route_tests;
mod state;

use crate::error::{BusError, ErrorCode};
use crate::identity::PeerIdentity;
use crate::policy::Policy;
use crate::sync::{lock, replace, wait_timeout};
use crate::wire::DEFAULT_MAX_FRAME;
pub use observe::{BrokerEvent, BrokerObserver, Operation};
use state::State;
use std::io::ErrorKind;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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
    /// Bytes that may wait for one client (one frame always fits).
    pub queue_bytes: usize,
    /// Most authenticated connections at the same time, per listener.
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
            queue_bytes: 8 << 20,
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
    observer: Option<Arc<dyn BrokerObserver>>,
    live_threads: Mutex<usize>,
    all_done: Condvar,
}

impl Shared {
    /// Hands `event` to the observer, if there is one.
    pub(crate) fn observe(&self, event: &BrokerEvent<'_>) {
        if let Some(observer) = &self.observer {
            observer.event(event);
        }
    }
}

/// Who may reach a socket file: permission bits, owner and group. Fields
/// left `None` keep what `bind` produced under the process's umask.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SocketAccess {
    /// Permission bits, e.g. `0o660`.
    pub mode: Option<u32>,
    /// Owning user id.
    pub owner: Option<u32>,
    /// Owning group id.
    pub group: Option<u32>,
}

impl SocketAccess {
    /// Only the permission bits.
    pub const fn mode(mode: u32) -> Self {
        Self {
            mode: Some(mode),
            owner: None,
            group: None,
        }
    }
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
    access: SocketAccess,
}

/// Collects the settings of a broker before it starts.
pub struct BrokerBuilder {
    policy: Policy,
    config: BrokerConfig,
    listeners: Vec<ListenerSpec>,
    observer: Option<Arc<dyn BrokerObserver>>,
}

impl BrokerBuilder {
    /// Reports refused operations, dropped clients and limits to
    /// `observer` (see [`BrokerObserver`] for what it must not do).
    pub fn observer(mut self, observer: Arc<dyn BrokerObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Like [`Self::listen`], and sets permission bits, owner and group of
    /// the socket file. Owner and group are set before the mode, so the
    /// file is never reachable by more than the final setting allows.
    pub fn listen_with_access(
        self,
        path: &Path,
        auth: Arc<dyn PeerIdentity>,
        access: SocketAccess,
    ) -> Self {
        self.add(path, auth, access)
    }

    /// Replaces the default limits and timeouts.
    pub fn config(mut self, config: BrokerConfig) -> Self {
        self.config = config;
        self
    }

    /// Adds a socket; peers connecting to it are identified by `auth`.
    pub fn listen(self, path: &Path, auth: Arc<dyn PeerIdentity>) -> Self {
        self.add(path, auth, SocketAccess::default())
    }

    /// Like [`Self::listen`], and sets the permission bits of the socket
    /// file (e.g. `0o600`) so the kernel restricts who may connect.
    pub fn listen_with_mode(self, path: &Path, auth: Arc<dyn PeerIdentity>, mode: u32) -> Self {
        self.add(path, auth, SocketAccess::mode(mode))
    }

    fn add(mut self, path: &Path, auth: Arc<dyn PeerIdentity>, access: SocketAccess) -> Self {
        self.listeners.push(ListenerSpec {
            path: path.to_owned(),
            auth,
            access,
        });
        self
    }

    /// Binds all sockets and starts serving.
    pub fn start(self) -> Result<Broker, BusError> {
        let mut state = State::new();
        state.queue_bytes = self.config.queue_bytes;
        state.observer = self.observer.clone();
        let shared = Arc::new(Shared {
            observer: self.observer,
            config: self.config,
            policy: RwLock::new(self.policy),
            state: Mutex::new(state),
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
///
/// Only a **socket** is ever removed: a regular file, a directory or a
/// symbolic link at the path (a typo in a service file, or somebody's
/// attempt to make a privileged broker delete a file) is left alone and
/// reported as [`BusError::NotASocket`].
fn bind(path: &Path) -> Result<UnixListener, BusError> {
    match UnixListener::bind(path) {
        Ok(listener) => Ok(listener),
        Err(err) if err.kind() == ErrorKind::AddrInUse => {
            // `symlink_metadata`: a link is judged itself, not its target.
            let meta = std::fs::symlink_metadata(path).map_err(BusError::Io)?;
            if !meta.file_type().is_socket() {
                return Err(BusError::NotASocket);
            }
            if UnixStream::connect(path).is_ok() {
                return Err(BusError::AddressInUse);
            }
            std::fs::remove_file(path).map_err(BusError::Io)?;
            UnixListener::bind(path).map_err(BusError::Io)
        }
        Err(err) => Err(BusError::Io(err)),
    }
}

/// Applies owner, group and mode to a freshly bound socket file.
fn restrict(path: &Path, access: &SocketAccess) -> Result<(), BusError> {
    if access.owner.is_some() || access.group.is_some() {
        std::os::unix::fs::chown(path, access.owner, access.group).map_err(BusError::Io)?;
    }
    if let Some(mode) = access.mode {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(BusError::Io)?;
    }
    Ok(())
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
            observer: None,
        }
    }

    fn open(&mut self, spec: ListenerSpec) -> Result<(), BusError> {
        let listener = bind(&spec.path)?;
        self.paths.push(spec.path.clone());
        restrict(&spec.path, &spec.access)?;
        let shared = self.shared.clone();
        let index = self.paths.len();
        let acceptor = thread::Builder::new()
            .name("ultar-bus-accept".into())
            .spawn(move || accept_loop(&shared, &listener, &spec.auth, index))
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

fn accept_loop(
    shared: &Arc<Shared>,
    listener: &UnixListener,
    auth: &Arc<dyn PeerIdentity>,
    index: usize,
) {
    // Threads of this listener only: one socket cannot starve the others.
    let own = Arc::new(AtomicUsize::new(0));
    for incoming in listener.incoming() {
        if shared.stopping.load(Ordering::SeqCst) {
            return;
        }
        match incoming {
            Ok(stream) => spawn_connection(shared, stream, auth, index, &own),
            Err(_) => thread::sleep(ACCEPT_RETRY),
        }
    }
}

struct OwnGuard(Arc<AtomicUsize>);

impl Drop for OwnGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn spawn_connection(
    shared: &Arc<Shared>,
    stream: UnixStream,
    auth: &Arc<dyn PeerIdentity>,
    index: usize,
    own: &Arc<AtomicUsize>,
) {
    let limit = shared
        .config
        .max_connections
        .saturating_add(HANDSHAKE_SLACK);
    if own.fetch_add(1, Ordering::SeqCst) >= limit {
        own.fetch_sub(1, Ordering::SeqCst);
        return;
    }
    let own_guard = OwnGuard(own.clone());
    *lock(&shared.live_threads) += 1;
    let guard = LiveGuard(shared.clone());
    let auth = auth.clone();
    // If the thread cannot be created the closure is dropped, and with it
    // the guard and the stream.
    let _ = thread::Builder::new()
        .name("ultar-bus-conn".into())
        .stack_size(READER_STACK)
        .spawn(move || {
            conn::serve(&guard.0, stream, auth.as_ref(), index);
            drop(own_guard);
            drop(guard);
        });
}
