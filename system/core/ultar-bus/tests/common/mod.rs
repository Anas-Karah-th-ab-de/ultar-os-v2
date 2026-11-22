//! Helpers shared by the integration tests.
#![allow(dead_code)]

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_bus::wire::{
    read_envelope, read_preamble, write_envelope, write_preamble, Envelope, Hello, Kind, Welcome,
    PROTOCOL_VERSION,
};
use ultar_bus::{
    decode, encode, Broker, BrokerConfig, BusError, Client, ClientOptions, FixedIdentity, Identity,
    Policy, TokenIdentity,
};

/// Generous upper bound for anything that should happen "immediately".
pub const WAIT: Duration = Duration::from_secs(10);
/// Time after which something that must *not* happen counts as not happened.
pub const QUIET: Duration = Duration::from_millis(150);

static NEXT_SOCKET: AtomicUsize = AtomicUsize::new(0);

/// A unique socket path below the system temp directory.
pub fn socket_path(tag: &str) -> PathBuf {
    let number = NEXT_SOCKET.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!("ubus-{}-{number}-{tag}.sock", std::process::id()))
}

pub fn id(name: &str) -> Identity {
    Identity::new(name).unwrap()
}

/// Broker where every peer is `test.peer`.
pub fn start(tag: &str, policy: Policy, config: BrokerConfig) -> (Broker, PathBuf) {
    let path = socket_path(tag);
    let broker = start_at(&path, policy, config);
    (broker, path)
}

pub fn start_at(path: &Path, policy: Policy, config: BrokerConfig) -> Broker {
    let auth = Arc::new(FixedIdentity::new(id("test.peer")));
    Broker::builder(policy)
        .config(config)
        .listen(path, auth)
        .start()
        .unwrap()
}

/// Open broker with default limits.
pub fn open_broker(tag: &str) -> (Broker, PathBuf) {
    start(tag, Policy::allow_all(), BrokerConfig::default())
}

/// Broker that identifies peers by the tokens `system.settingsd`,
/// `app.phone` and `app.thirdparty` (token = identity name).
pub fn token_broker(tag: &str, policy: Policy) -> (Broker, PathBuf) {
    let path = socket_path(tag);
    let mut auth = TokenIdentity::new();
    for name in [
        "system.settingsd",
        "system.devd",
        "app.phone",
        "app.thirdparty",
    ] {
        auth = auth.with_token(name, id(name));
    }
    let broker = Broker::builder(policy)
        .listen(&path, Arc::new(auth))
        .start()
        .unwrap();
    (broker, path)
}

pub fn client(path: &Path, name: &str) -> Client {
    Client::connect(path, ClientOptions::named(name)).unwrap()
}

pub fn client_with_token(path: &Path, token: &str) -> Result<Client, BusError> {
    let options = ClientOptions {
        token: Some(token.to_owned()),
        ..ClientOptions::named(token)
    };
    Client::connect(path, options)
}

/// Polls `condition` until it holds or `WAIT` passed.
pub fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A hand-driven peer for protocol level tests.
pub struct RawPeer {
    pub stream: UnixStream,
}

impl RawPeer {
    /// Connects without sending anything.
    pub fn open(path: &Path) -> Self {
        let stream = UnixStream::connect(path).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.set_write_timeout(Some(WAIT)).unwrap();
        Self { stream }
    }

    /// Connects and completes the handshake.
    pub fn connect(path: &Path) -> Self {
        let mut peer = Self::open(path);
        write_preamble(&mut peer.stream, PROTOCOL_VERSION).unwrap();
        assert_eq!(read_preamble(&mut peer.stream).unwrap(), PROTOCOL_VERSION);
        let hello = Hello {
            client_name: "raw".into(),
            token: None,
        };
        let hello = Envelope::new(Kind::Hello, 0).with_payload(1, encode(&hello).unwrap());
        peer.send(&hello);
        let welcome = peer.recv().unwrap();
        assert_eq!(welcome.kind, Kind::Welcome);
        decode::<Welcome>(&welcome.payload).unwrap();
        peer
    }

    pub fn send(&mut self, envelope: &Envelope) {
        write_envelope(&mut self.stream, envelope, u32::MAX).unwrap();
    }

    pub fn recv(&mut self) -> Result<Envelope, BusError> {
        read_envelope(&mut self.stream, u32::MAX)
    }

    /// Subscribes and consumes the acknowledgement.
    pub fn subscribe(&mut self, id: u64, pattern: &str) {
        self.send(&Envelope::new(Kind::Subscribe, id).with_target(pattern));
        let ack = self.recv().unwrap();
        assert_eq!((ack.kind, ack.id), (Kind::Response, id));
    }
}

/// Deterministic pseudo random numbers (xorshift64).
pub struct XorShift(pub u64);

impl XorShift {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound.max(1)
    }

    /// Between `min` and `min + spread - 1` random bytes.
    pub fn some_bytes(&mut self, min: usize, spread: u64) -> Vec<u8> {
        let len = min + self.below(spread) as usize;
        self.bytes(len)
    }

    pub fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next() as u8).collect()
    }
}
