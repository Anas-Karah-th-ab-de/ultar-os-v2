//! The encrypted channel between two devices: Noise XX (X25519, ChaCha20-Poly1305,
//! BLAKE2s) over one TCP connection. Both sides prove their static key; one Noise message
//! carries one frame, prefixed with its length.

use crate::wire::{self, Frame};
use snow::{Builder, StatelessTransportState};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const MAX_MESSAGE: usize = 65_535;
const HANDSHAKE_TIME: Duration = Duration::from_secs(10);

/// Why a channel failed.
#[derive(Debug)]
pub enum ChannelError {
    /// The connection.
    Io(std::io::Error),
    /// The other side did not follow the protocol, or the keys are wrong.
    Protocol,
}

impl From<std::io::Error> for ChannelError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<snow::Error> for ChannelError {
    fn from(_: snow::Error) -> Self {
        Self::Protocol
    }
}

/// This device's static key pair.
#[derive(Clone)]
pub struct Keys {
    /// The private half (never leaves the device).
    pub private: Vec<u8>,
    /// The public half (the device id is derived from it).
    pub public: Vec<u8>,
}

impl Keys {
    /// A fresh pair.
    pub fn generate() -> Option<Self> {
        let pair = Builder::new(PATTERN.parse().ok()?)
            .generate_keypair()
            .ok()?;
        Some(Self {
            private: pair.private,
            public: pair.public,
        })
    }
}

/// The sending half of a connection that carries whole messages.
pub trait Tx: Send {
    /// Sends one message.
    fn send(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    /// Closes the connection.
    fn close(&mut self);
}

/// The receiving half of a connection that carries whole messages.
pub trait Rx: Send {
    /// Waits for the next message.
    fn recv(&mut self) -> std::io::Result<Vec<u8>>;
    /// How long [`Rx::recv`] waits (`None`: for ever).
    fn limit(&mut self, time: Option<Duration>);
}

struct Tcp(TcpStream);

impl Tx for Tcp {
    fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let len = u16::try_from(bytes.len()).map_err(|_| std::io::ErrorKind::InvalidInput)?;
        self.0.write_all(&len.to_be_bytes())?;
        self.0.write_all(bytes)
    }

    fn close(&mut self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}

impl Rx for Tcp {
    fn recv(&mut self) -> std::io::Result<Vec<u8>> {
        let mut len = [0u8; 2];
        self.0.read_exact(&mut len)?;
        let mut bytes = vec![0u8; usize::from(u16::from_be_bytes(len))];
        self.0.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn limit(&mut self, time: Option<Duration>) {
        let _ = self.0.set_read_timeout(time);
    }
}

/// The sending half of a channel (shared between threads).
#[derive(Clone)]
pub struct Sender {
    state: Arc<StatelessTransportState>,
    stream: Arc<Mutex<(Box<dyn Tx>, u64)>>,
}

impl Sender {
    /// Sends one frame; `false` when it does not fit or the connection is gone.
    pub fn send(&self, frame: &Frame) -> bool {
        let Some(plain) = wire::pack(frame) else {
            return false;
        };
        let Ok(mut guard) = self.stream.lock() else {
            return false;
        };
        let mut cipher = vec![0u8; plain.len() + 16];
        let nonce = guard.1;
        let Ok(n) = self.state.write_message(nonce, &plain, &mut cipher) else {
            return false;
        };
        guard.1 += 1;
        guard.0.send(&cipher[..n]).is_ok()
    }

    /// Closes the connection (the receiving half ends too).
    pub fn close(&self) {
        if let Ok(mut guard) = self.stream.lock() {
            guard.0.close();
        }
    }
}

/// The receiving half of a channel.
pub struct Receiver {
    state: Arc<StatelessTransportState>,
    stream: Box<dyn Rx>,
    nonce: AtomicU64,
}

impl Receiver {
    /// Waits for the next frame; `None` when the connection ended or was tampered with.
    pub fn receive(&mut self) -> Option<Frame> {
        let cipher = self.stream.recv().ok()?;
        let mut plain = vec![0u8; cipher.len()];
        let nonce = self.nonce.fetch_add(1, Ordering::SeqCst);
        let n = self.state.read_message(nonce, &cipher, &mut plain).ok()?;
        wire::unpack(&plain[..n])
    }
}

/// An open channel and the static key the other side proved.
pub struct Channel {
    /// Send frames.
    pub sender: Sender,
    /// Receive frames.
    pub receiver: Receiver,
    /// The other side's static public key.
    pub remote: Vec<u8>,
}

/// Runs the handshake on a TCP connection (`initiator`: this side connected).
pub fn open(stream: TcpStream, keys: &Keys, initiator: bool) -> Result<Channel, ChannelError> {
    stream.set_nodelay(true)?;
    let writer = stream.try_clone()?;
    open_on(
        Box::new(Tcp(writer)),
        Box::new(Tcp(stream)),
        keys,
        initiator,
    )
}

/// Runs the handshake on any connection that carries whole messages.
pub fn open_on(
    mut tx: Box<dyn Tx>,
    mut rx: Box<dyn Rx>,
    keys: &Keys,
    initiator: bool,
) -> Result<Channel, ChannelError> {
    rx.limit(Some(HANDSHAKE_TIME));
    let builder = Builder::new(PATTERN.parse()?).local_private_key(&keys.private);
    let mut noise = if initiator {
        builder.build_initiator()?
    } else {
        builder.build_responder()?
    };
    let mut buffer = vec![0u8; MAX_MESSAGE];
    let mut mine = initiator;
    while !noise.is_handshake_finished() {
        if mine {
            let n = noise.write_message(&[], &mut buffer)?;
            tx.send(&buffer[..n])?;
        } else {
            let message = rx.recv()?;
            noise.read_message(&message, &mut buffer)?;
        }
        mine = !mine;
    }
    let remote = noise
        .get_remote_static()
        .ok_or(ChannelError::Protocol)?
        .to_vec();
    let state = Arc::new(noise.into_stateless_transport_mode()?);
    rx.limit(None);
    Ok(Channel {
        sender: Sender {
            state: state.clone(),
            stream: Arc::new(Mutex::new((tx, 0))),
        },
        receiver: Receiver {
            state,
            stream: rx,
            nonce: AtomicU64::new(0),
        },
        remote,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn pair() -> (Channel, Channel, Keys, Keys) {
        let (a, b) = (Keys::generate().unwrap(), Keys::generate().unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let kb = b.clone();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            open(stream, &kb, false).unwrap()
        });
        let client = open(TcpStream::connect(addr).unwrap(), &a, true).unwrap();
        (client, server.join().unwrap(), a, b)
    }

    #[test]
    fn both_sides_prove_their_key_and_frames_arrive_in_order() {
        let (mut client, mut server, a, b) = pair();
        assert_eq!(client.remote, b.public);
        assert_eq!(server.remote, a.public);
        for seq in 0..50 {
            assert!(client.sender.send(&Frame::Clip {
                seq,
                text: "x".repeat(1000),
            }));
        }
        for seq in 0..50 {
            match server.receiver.receive() {
                Some(Frame::Clip { seq: got, .. }) => assert_eq!(got, seq),
                other => panic!("{other:?}"),
            }
        }
        assert!(server.sender.send(&Frame::PairRefused));
        assert_eq!(client.receiver.receive(), Some(Frame::PairRefused));
    }

    #[test]
    fn a_closed_channel_ends_the_other_side() {
        let (client, mut server, _, _) = pair();
        client.sender.close();
        assert_eq!(server.receiver.receive(), None);
        assert!(!client.sender.send(&Frame::PairRefused));
    }

    #[test]
    fn plain_bytes_are_refused() {
        let b = Keys::generate().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            open(stream, &b, false).is_err()
        });
        let mut raw = TcpStream::connect(addr).unwrap();
        raw.write_all(&[0, 4, 1, 2, 3, 4]).unwrap();
        assert!(server.join().unwrap());
    }
}
