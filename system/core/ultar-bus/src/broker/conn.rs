//! One client connection inside the broker: handshake, a reader thread that
//! feeds the router and a writer thread that drains the bounded queue.

use super::route::{handle, Rules};
use super::state::{ConnEntry, ConnId, Frame, LinkControl};
use super::Shared;
use crate::error::{BusError, ErrorCode};
use crate::identity::{Identity, PeerIdentity, PeerInfo};
use crate::message::{decode, encode};
use crate::sync::{lock, read};
use crate::wire::{
    read_envelope, read_preamble, write_envelope, write_preamble, Envelope, Hello, Kind, Welcome,
    PROTOCOL_VERSION,
};
use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;
use std::thread;

/// Largest frame accepted before the peer is authenticated.
const HANDSHAKE_MAX_FRAME: u32 = 4096;
/// Stack size of the per-connection writer thread.
const WRITER_STACK: usize = 128 * 1024;

/// Serves one accepted connection until it ends.
pub(crate) fn serve(shared: &Arc<Shared>, stream: UnixStream, auth: &dyn PeerIdentity) {
    let identity = match handshake(shared, &stream, auth) {
        Ok(identity) => identity,
        Err(err) => {
            reject(&stream, &err);
            return;
        }
    };
    if let Err(err) = run_session(shared, &stream, identity) {
        reject(&stream, &err);
    }
    let _ = stream.shutdown(Shutdown::Both);
}

/// Tells a peer that passed the preamble why it is turned away.
fn reject(mut stream: &UnixStream, err: &BusError) {
    let silent = matches!(
        err,
        BusError::BadMagic
            | BusError::ProtocolMismatch { .. }
            | BusError::Closed
            | BusError::Timeout
            | BusError::Io(_)
    );
    if !silent {
        let _ = write_envelope(&mut stream, &Envelope::error(0, err.code()), u32::MAX);
    }
    let _ = stream.shutdown(Shutdown::Both);
}

fn handshake(
    shared: &Shared,
    mut stream: &UnixStream,
    auth: &dyn PeerIdentity,
) -> Result<Identity, BusError> {
    stream.set_read_timeout(Some(shared.config.handshake_timeout))?;
    stream.set_write_timeout(Some(shared.config.write_timeout))?;
    let theirs = read_preamble(&mut stream)?;
    write_preamble(&mut stream, PROTOCOL_VERSION)?;
    if theirs != PROTOCOL_VERSION {
        return Err(BusError::ProtocolMismatch {
            ours: PROTOCOL_VERSION,
            theirs,
        });
    }
    let envelope = read_envelope(&mut stream, HANDSHAKE_MAX_FRAME)?;
    if envelope.kind != Kind::Hello {
        return Err(BusError::Remote(ErrorCode::ProtocolViolation));
    }
    let hello: Hello = decode(&envelope.payload)?;
    let peer = PeerInfo {
        stream,
        client_name: &hello.client_name,
        token: hello.token.as_deref(),
    };
    auth.identify(&peer).map_err(|_| BusError::AuthFailed)
}

/// Adds the connection to the routing state and queues its `Welcome`.
fn admit(
    shared: &Shared,
    stream: &UnixStream,
    identity: Identity,
) -> Result<(ConnId, Arc<LinkControl>, Receiver<Frame>), BusError> {
    let welcome = Welcome {
        identity: identity.as_str().to_owned(),
        max_frame: shared.config.max_frame,
    };
    let welcome = Envelope::new(Kind::Welcome, 0).with_payload(PROTOCOL_VERSION, encode(&welcome)?);
    let control = Arc::new(LinkControl::new(stream.try_clone()?));
    let (queue, outbox) = sync_channel(shared.config.queue_capacity.max(2));
    let conn = shared.next_conn.fetch_add(1, Ordering::SeqCst);
    let mut state = lock(&shared.state);
    if shared.stopping.load(Ordering::SeqCst) {
        return Err(BusError::Remote(ErrorCode::ShuttingDown));
    }
    if state.conns.len() >= shared.config.max_connections {
        return Err(BusError::Remote(ErrorCode::LimitExceeded));
    }
    let entry = ConnEntry {
        identity,
        queue,
        control: control.clone(),
        subscriptions: Vec::new(),
        services: Vec::new(),
    };
    state.conns.insert(conn, entry);
    state.send(conn, &welcome);
    Ok((conn, control, outbox))
}

fn run_session(
    shared: &Arc<Shared>,
    stream: &UnixStream,
    identity: Identity,
) -> Result<(), BusError> {
    let (conn, control, outbox) = admit(shared, stream, identity)?;
    let writer = stream.try_clone().and_then(|write_half| {
        stream.set_read_timeout(None)?;
        let control = control.clone();
        thread::Builder::new()
            .name("ultar-bus-writer".into())
            .stack_size(WRITER_STACK)
            .spawn(move || write_loop(write_half, &outbox, &control))
    });
    if writer.is_ok() {
        read_loop(shared, stream, conn, &control);
    }
    lock(&shared.state).remove_conn(conn);
    match writer {
        Ok(handle) => {
            let _ = handle.join();
            Ok(())
        }
        Err(err) => Err(BusError::Io(err)),
    }
}

/// Reads envelopes and routes them until the connection ends.
fn read_loop(shared: &Shared, mut stream: &UnixStream, conn: ConnId, control: &LinkControl) {
    loop {
        let envelope = match read_envelope(&mut stream, shared.config.max_frame) {
            Ok(envelope) => envelope,
            Err(err @ (BusError::FrameTooLarge { .. } | BusError::Malformed)) => {
                control.set_reason(err.code());
                return;
            }
            Err(_) => return,
        };
        let policy = read(&shared.policy);
        let rules = Rules {
            policy: &policy,
            config: &shared.config,
        };
        let mut state = lock(&shared.state);
        if let Err(code) = handle(&mut state, &rules, conn, envelope) {
            control.set_reason(code);
            return;
        }
    }
}

/// Writes queued frames; ends when the queue is closed, the peer stops
/// accepting data, or the connection is marked as closing. The reason for
/// closing is sent as last frame when the peer can still take it.
fn write_loop(mut stream: UnixStream, outbox: &Receiver<Frame>, control: &LinkControl) {
    let mut writable = true;
    while let Ok(frame) = outbox.recv() {
        if control.is_closing() {
            break;
        }
        if stream.write_all(&frame).is_err() {
            writable = false;
            break;
        }
    }
    if writable {
        if let Some(code) = control.reason() {
            let _ = write_envelope(&mut stream, &Envelope::error(0, code), u32::MAX);
        }
    }
    control.hang_up();
    let _ = stream.shutdown(Shutdown::Both);
}
