//! One admitted client connection inside the broker: a reader thread that
//! feeds the router and a writer thread that drains the bounded queue. The
//! handshake before it runs in the listener's gate (`gate.rs`).

use super::observe::BrokerEvent;
use super::route::{handle, Rules};
use super::state::{ConnEntry, ConnId, Frame, LinkControl};
use super::{LiveGuard, Shared};
use crate::error::{BusError, ErrorCode};
use crate::identity::Identity;
use crate::message::encode;
use crate::sync::{lock, read};
use crate::wire::{read_envelope, write_envelope, Envelope, Kind, Welcome, PROTOCOL_VERSION};
use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;
use std::thread;

/// Stack size of the per-connection reader thread.
const READER_STACK: usize = 256 * 1024;
/// Stack size of the per-connection writer thread.
const WRITER_STACK: usize = 128 * 1024;

/// Tells a peer that passed the preamble why it is turned away.
pub(crate) fn reject(mut stream: &UnixStream, err: &BusError) {
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

/// What a session needs after admission: id, control, outbox, queued bytes.
pub(crate) type Admitted = (ConnId, Arc<LinkControl>, Receiver<Frame>, Arc<AtomicUsize>);

/// Adds the connection to the routing state and queues its `Welcome`;
/// `limit` is the most connections of `listener`.
pub(crate) fn admit(
    shared: &Shared,
    stream: &Arc<UnixStream>,
    identity: Identity,
    listener: usize,
    limit: usize,
) -> Result<Admitted, BusError> {
    let welcome = Welcome {
        identity: identity.as_str().to_owned(),
        max_frame: shared.config.max_frame,
    };
    let welcome = Envelope::new(Kind::Welcome, 0).with_payload(PROTOCOL_VERSION, encode(&welcome)?);
    // One descriptor per connection (review round 3, R3-B1): reader,
    // writer and control share it.
    let control = Arc::new(LinkControl::new(stream.clone()));
    let (queue, outbox) = sync_channel(shared.config.queue_capacity.max(2));
    let conn = shared.next_conn.fetch_add(1, Ordering::SeqCst);
    let mut state = lock(&shared.state);
    if shared.stopping.load(Ordering::SeqCst) {
        return Err(BusError::Remote(ErrorCode::ShuttingDown));
    }
    // Per listener: the app socket cannot use up the slots of the system socket.
    let same_listener = state
        .conns
        .values()
        .filter(|entry| entry.listener == listener)
        .count();
    if same_listener >= limit {
        return Err(BusError::Remote(ErrorCode::LimitExceeded));
    }
    let queued = Arc::new(AtomicUsize::new(0));
    let entry = ConnEntry {
        identity,
        queue,
        control: control.clone(),
        listener,
        queued: queued.clone(),
        subscriptions: Vec::new(),
        services: Vec::new(),
    };
    state.conns.insert(conn, entry);
    state.send(conn, &welcome);
    Ok((conn, control, outbox, queued))
}

/// Starts the reader thread of an admitted connection. A thread that
/// cannot be started refuses this connection; nothing panics.
pub(crate) fn start_session(
    shared: &Arc<Shared>,
    stream: Arc<UnixStream>,
    admitted: Admitted,
    listener: usize,
) {
    let conn = admitted.0;
    *lock(&shared.live_threads) += 1;
    let guard = LiveGuard(shared.clone());
    let session = stream.clone();
    let started = thread::Builder::new()
        .name("ultar-bus-conn".into())
        .stack_size(READER_STACK)
        .spawn(move || {
            run_session(&guard.0, &session, admitted, listener);
            drop(guard);
        });
    if started.is_err() {
        // The closure and with it the guard are gone already.
        lock(&shared.state).remove_conn(conn);
        refuse(shared, &stream, listener);
    }
}

/// Turns a client away because a thread could not be started.
fn refuse(shared: &Shared, stream: &UnixStream, listener: usize) {
    shared.observe(&BrokerEvent::ConnectionLimit { listener });
    reject(stream, &BusError::Remote(ErrorCode::LimitExceeded));
}

fn run_session(
    shared: &Arc<Shared>,
    stream: &Arc<UnixStream>,
    admitted: Admitted,
    listener: usize,
) {
    let (conn, control, outbox, queued) = admitted;
    let writer = stream.set_read_timeout(None).and_then(|()| {
        let (control, write_half) = (control.clone(), stream.clone());
        thread::Builder::new()
            .name("ultar-bus-writer".into())
            .stack_size(WRITER_STACK)
            .spawn(move || write_loop(&write_half, &outbox, &control, &queued))
    });
    if writer.is_ok() {
        read_loop(shared, stream, conn, &control);
    }
    lock(&shared.state).remove_conn(conn);
    match writer {
        Ok(handle) => {
            let _ = handle.join();
        }
        Err(_) => refuse(shared, stream, listener),
    }
    let _ = stream.shutdown(Shutdown::Both);
}

/// Reads envelopes and routes them until the connection ends.
fn read_loop(shared: &Shared, mut stream: &UnixStream, conn: ConnId, control: &LinkControl) {
    loop {
        let envelope = match read_envelope(&mut stream, shared.config.max_frame) {
            Ok(envelope) => envelope,
            Err(err @ (BusError::FrameTooLarge { .. } | BusError::Malformed)) => {
                control.set_reason(err.code());
                protocol_error(shared, conn, err.code());
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
            drop(state);
            protocol_error(shared, conn, code);
            return;
        }
    }
}

/// Reports a client that is disconnected for breaking the protocol.
fn protocol_error(shared: &Shared, conn: ConnId, code: ErrorCode) {
    let state = lock(&shared.state);
    if let Some(entry) = state.conns.get(&conn) {
        state.observe(&BrokerEvent::ProtocolError {
            identity: entry.identity.as_str(),
            code,
        });
    }
}

/// Writes queued frames; ends when the queue is closed, the peer stops
/// accepting data, or the connection is marked as closing. The reason for
/// closing is sent as last frame when the peer can still take it.
fn write_loop(
    mut stream: &UnixStream,
    outbox: &Receiver<Frame>,
    control: &LinkControl,
    queued: &AtomicUsize,
) {
    let mut writable = true;
    while let Ok(frame) = outbox.recv() {
        if control.is_closing() {
            break;
        }
        let written = stream.write_all(&frame);
        queued.fetch_sub(frame.len(), Ordering::SeqCst);
        if written.is_err() {
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
