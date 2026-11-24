//! The network side: devices announce themselves in the local network, connect to each
//! other and open the encrypted channel. Everything that happens is reported as a
//! [`NetEvent`]; while the mesh is off no socket is open.

use crate::device;
use crate::secure::{self, Keys, Sender};
use crate::wire::Frame;
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Port of the announcements (UDP) and, by default, of the channels (TCP).
pub const PORT: u16 = 47_810;
/// How often a device announces itself.
pub const ANNOUNCE_EVERY: Duration = Duration::from_secs(2);
const CONNECT_TIME: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(300);

/// What happened on the network.
pub enum NetEvent {
    /// A channel to `peer` is open.
    Up {
        /// The device (id derived from the key it proved).
        peer: String,
        /// Its static public key.
        key: Vec<u8>,
        /// Number of this channel.
        link: u64,
        /// Sends frames to it.
        sender: Sender,
    },
    /// A frame arrived on channel `link`.
    Frame {
        /// The device.
        peer: String,
        /// The channel.
        link: u64,
        /// The frame.
        frame: Frame,
    },
    /// Channel `link` to `peer` ended.
    Down {
        /// The device.
        peer: String,
        /// The channel.
        link: u64,
    },
    /// A job this device ran for `peer` ended.
    Ran {
        /// Who asked ([`crate::compute::LOCAL`]: this device).
        peer: String,
        /// The job.
        id: String,
        /// Exit code.
        code: Option<i32>,
        /// What it printed.
        output: String,
    },
}

#[derive(Serialize, Deserialize)]
struct Announce {
    mesh: u16,
    id: String,
    port: u16,
}

struct Shared {
    keys: Keys,
    id: String,
    events: mpsc::Sender<NetEvent>,
    stop: AtomicBool,
    links: AtomicU64,
    linked: Mutex<Vec<String>>,
}

/// The open sockets of a device.
pub struct Net {
    shared: Arc<Shared>,
    port: u16,
}

fn serve(shared: &Arc<Shared>, stream: TcpStream, initiator: bool) {
    let Ok(channel) = secure::open(stream, &shared.keys, initiator) else {
        return;
    };
    let peer = device::id_of(&channel.remote);
    if peer == shared.id || shared.stop.load(Ordering::SeqCst) {
        channel.sender.close();
        return;
    }
    let link = shared.links.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut linked) = shared.linked.lock() {
        linked.push(peer.clone());
    }
    let up = NetEvent::Up {
        peer: peer.clone(),
        key: channel.remote,
        link,
        sender: channel.sender,
    };
    let mut receiver = channel.receiver;
    let mut open = shared.events.send(up).is_ok();
    while open {
        let Some(frame) = receiver.receive() else {
            break;
        };
        let event = NetEvent::Frame {
            peer: peer.clone(),
            link,
            frame,
        };
        open = shared.events.send(event).is_ok();
    }
    if let Ok(mut linked) = shared.linked.lock() {
        if let Some(at) = linked.iter().position(|p| p == &peer) {
            linked.remove(at);
        }
    }
    let _ = shared.events.send(NetEvent::Down { peer, link });
}

fn spawn(work: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new().spawn(work);
}

fn accept(shared: Arc<Shared>, listener: TcpListener) {
    for stream in listener.incoming() {
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        if let Ok(stream) = stream {
            let shared = shared.clone();
            spawn(move || serve(&shared, stream, false));
        }
    }
}

fn dial(shared: &Arc<Shared>, addr: SocketAddr) {
    let shared = shared.clone();
    spawn(move || {
        if let Ok(stream) = TcpStream::connect_timeout(&addr, CONNECT_TIME) {
            serve(&shared, stream, true);
        }
    });
}

fn discover(shared: Arc<Shared>, socket: UdpSocket, announce_to: SocketAddr, port: u16) {
    let me = Announce {
        mesh: crate::wire::VERSION,
        id: shared.id.clone(),
        port,
    };
    let Ok(text) = serde_json::to_vec(&me) else {
        return;
    };
    let mut buffer = [0u8; 512];
    let mut waited = ANNOUNCE_EVERY;
    while !shared.stop.load(Ordering::SeqCst) {
        if waited >= ANNOUNCE_EVERY {
            let _ = socket.send_to(&text, announce_to);
            waited = Duration::ZERO;
        }
        waited += POLL;
        let Ok((n, from)) = socket.recv_from(&mut buffer) else {
            continue;
        };
        let Ok(other) = serde_json::from_slice::<Announce>(&buffer[..n]) else {
            continue;
        };
        // The device with the smaller id connects, so two devices open one channel.
        let known = shared.linked.lock().map_or(true, |l| l.contains(&other.id));
        if device::is_id(&other.id) && shared.id < other.id && !known {
            dial(&shared, SocketAddr::new(from.ip(), other.port));
        }
    }
}

impl Net {
    /// Opens the channel port on `bind` (port 0: any) and, with `announce`, the
    /// announcement socket on that UDP port.
    pub fn start(
        keys: Keys,
        bind: SocketAddr,
        announce: Option<u16>,
        events: mpsc::Sender<NetEvent>,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind(bind)?;
        let port = listener.local_addr()?.port();
        let shared = Arc::new(Shared {
            id: device::id_of(&keys.public),
            keys,
            events,
            stop: AtomicBool::new(false),
            links: AtomicU64::new(1),
            linked: Mutex::new(Vec::new()),
        });
        let for_accept = shared.clone();
        spawn(move || accept(for_accept, listener));
        if let Some(udp) = announce {
            let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, udp))?;
            socket.set_broadcast(true)?;
            socket.set_read_timeout(Some(POLL))?;
            let to = SocketAddr::from((Ipv4Addr::BROADCAST, udp));
            let for_discovery = shared.clone();
            spawn(move || discover(for_discovery, socket, to, port));
        }
        Ok(Self { shared, port })
    }

    /// The TCP port the channels arrive on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Connects to a device at a known address (networks without broadcast).
    pub fn connect(&self, addr: SocketAddr) {
        dial(&self.shared, addr);
    }

    /// Closes the sockets (the open channels are closed by their owner).
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // Wakes the accepting thread.
        let _ = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)),
            Duration::from_millis(200),
        );
    }
}

impl Drop for Net {
    fn drop(&mut self) {
        self.stop();
    }
}
