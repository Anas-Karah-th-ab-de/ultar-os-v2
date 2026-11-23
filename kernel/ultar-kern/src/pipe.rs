//! Pipes: a bounded byte queue between a writing and a reading end. Also the two directions of a
//! connected Unix socket.
//!
//! The buffer has a capacity (64 KiB for a pipe like Linux, 256 KiB per direction of a Unix
//! socket). A writer that finds it full waits (or gets `EAGAIN` with `O_NONBLOCK`); a write of at
//! most [`PIPE_BUF`] bytes is all-or-nothing. `F_SETPIPE_SZ` changes the capacity up to
//! [`max_size`], which a holder of the `MANAGE_GROUPS` right can set through
//! `/proc/sys/fs/pipe-max-size`. The buffered bytes are charged to the writer's resource group.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

/// Default capacity of a pipe.
pub const PIPE_DEFAULT: u64 = 64 * 1024;
/// Capacity of each direction of a Unix socket connection.
pub const UNIX_DEFAULT: u64 = 256 * 1024;
/// Writes up to this size are atomic.
pub const PIPE_BUF: usize = 4096;
/// Smallest capacity (one page).
pub const PIPE_MIN: u64 = 4096;
/// Hard ceiling of the configurable maximum.
pub const PIPE_MAX_CEILING: u64 = 64 << 20;

static MAX_SIZE: AtomicU64 = AtomicU64::new(1 << 20);

/// Largest capacity `F_SETPIPE_SZ` grants.
pub fn max_size() -> u64 {
    MAX_SIZE.load(Ordering::Relaxed)
}

/// Set the largest capacity; `false` if `bytes` is outside one page … 64 MiB.
pub fn set_max_size(bytes: u64) -> bool {
    let ok = (PIPE_MIN..=PIPE_MAX_CEILING).contains(&bytes);
    if ok {
        MAX_SIZE.store(bytes, Ordering::Relaxed);
    }
    ok
}

/// Why nothing could be written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PushError {
    /// No room (for a write of at most [`PIPE_BUF`] bytes: not enough room for all of it).
    Full,
    /// The buffer could not grow: no memory for the writer's group.
    NoMemory,
}

/// One direction of data flow.
pub struct Pipe {
    /// Buffered bytes.
    pub buf: Mutex<VecDeque<u8>>,
    /// Open reading ends.
    pub readers: AtomicU64,
    /// Open writing ends.
    pub writers: AtomicU64,
    cap: AtomicU64,
    /// Unix socket: identity of the process that writes into this direction – its process number
    /// (0 = not set: a plain pipe) and user id / group id packed into one word.
    writer: [AtomicU64; 2],
    /// Packet mode (device events): lengths of the messages in `buf`, in order; `None`: a stream.
    /// Always locked after `buf`.
    lens: Mutex<Option<VecDeque<u32>>>,
}

impl Pipe {
    /// Empty pipe with the default capacity and no open ends.
    pub fn new() -> Arc<Pipe> {
        Pipe::with_capacity(PIPE_DEFAULT)
    }

    /// Empty pipe with the given capacity and no open ends.
    pub fn with_capacity(cap: u64) -> Arc<Pipe> {
        Arc::new(Pipe { buf: Mutex::new(VecDeque::new()), readers: AtomicU64::new(0), writers: AtomicU64::new(0), cap: AtomicU64::new(cap), writer: [AtomicU64::new(0), AtomicU64::new(0)], lens: Mutex::new(None) })
    }

    /// Empty pipe in packet mode: every [`Pipe::push_packet`] is read back as one message.
    pub fn packets(cap: u64) -> Arc<Pipe> {
        let p = Pipe::with_capacity(cap);
        *p.lens.lock() = Some(VecDeque::new());
        p
    }

    /// Packet mode: append the whole message or nothing (`false`: no room or no memory).
    pub fn push_packet(&self, data: &[u8]) -> bool {
        let mut b = self.buf.lock();
        let mut lens = self.lens.lock();
        let Some(lens) = lens.as_mut() else { return false };
        if b.len() as u64 + data.len() as u64 > self.capacity() || data.len() > u32::MAX as usize {
            return false;
        }
        if crate::heap::fallible(|| b.try_reserve(data.len()).and(lens.try_reserve(1))).is_err() {
            return false;
        }
        b.extend(data);
        lens.push_back(data.len() as u32);
        true
    }

    /// Take what one `read` of at most `max` bytes gets: up to `max` bytes of a stream, or the
    /// next message in packet mode (cut to `max`, the rest of it dropped). `buf` is this pipe's
    /// locked buffer. `None` if memory for the copy is refused – then nothing was taken.
    pub fn take(&self, buf: &mut VecDeque<u8>, max: usize) -> Option<alloc::vec::Vec<u8>> {
        let mut lens = self.lens.lock();
        let Some(lens) = lens.as_mut() else { return Pipe::pull(buf, max) };
        let whole = lens.front().map_or(buf.len(), |&l| l as usize);
        let out = Pipe::pull(buf, max.min(whole))?;
        buf.drain(..whole - out.len());
        lens.pop_front();
        Some(out)
    }

    /// Unix socket: remember who writes into this direction (see `uds.rs`).
    pub fn set_writer(&self, who: crate::uds::Peer) {
        self.writer[1].store((who.uid as u64) << 32 | who.gid as u64, Ordering::Relaxed);
        self.writer[0].store(who.pid, Ordering::Relaxed);
    }

    /// Unix socket: who writes into this direction, if it was recorded.
    pub fn writer(&self) -> Option<crate::uds::Peer> {
        let (pid, ids) = (self.writer[0].load(Ordering::Relaxed), self.writer[1].load(Ordering::Relaxed));
        (pid != 0).then_some(crate::uds::Peer { pid, uid: (ids >> 32) as u32, gid: ids as u32 })
    }

    /// Capacity in bytes.
    pub fn capacity(&self) -> u64 {
        self.cap.load(Ordering::Relaxed)
    }

    /// Change the capacity (rounded up to a page). Returns the new capacity, or `None` if more
    /// bytes are buffered than would fit.
    pub fn set_capacity(&self, bytes: u64) -> Option<u64> {
        let cap = bytes.max(PIPE_MIN).div_ceil(4096) * 4096;
        let b = self.buf.lock();
        if b.len() as u64 > cap {
            return None;
        }
        self.cap.store(cap, Ordering::Relaxed);
        Some(cap)
    }

    /// Whether a writer would get at least [`PIPE_BUF`] bytes in (what `poll` reports as writable).
    pub fn writable(&self) -> bool {
        let len = self.buf.lock().len() as u64;
        let cap = self.capacity();
        cap.saturating_sub(len) >= (PIPE_BUF as u64).min(cap)
    }

    /// Take up to `max` bytes out of the front of the queue (in bulk); `None` if memory for the
    /// copy is refused – then nothing was taken.
    pub fn pull(buf: &mut VecDeque<u8>, max: usize) -> Option<alloc::vec::Vec<u8>> {
        let k = max.min(buf.len());
        let mut out = crate::mem::try_with_capacity(k)?;
        let (front, back) = buf.as_slices();
        let first = k.min(front.len());
        out.extend_from_slice(&front[..first]);
        out.extend_from_slice(&back[..k - first]);
        buf.drain(..k);
        Some(out)
    }

    /// Append as much of `data` as fits and return that count (never 0 for non-empty data).
    pub fn push(&self, data: &[u8]) -> Result<usize, PushError> {
        let mut b = self.buf.lock();
        let room = self.capacity().saturating_sub(b.len() as u64) as usize;
        let n = room.min(data.len());
        if n == 0 && !data.is_empty() || data.len() <= PIPE_BUF && n < data.len() {
            return Err(PushError::Full);
        }
        if crate::heap::fallible(|| b.try_reserve(n)).is_err() {
            return Err(PushError::NoMemory);
        }
        // from a slice the queue copies in bulk
        b.extend(&data[..n]);
        Ok(n)
    }
}
