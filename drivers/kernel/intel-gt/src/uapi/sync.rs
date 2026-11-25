//! Sync objects: named places that hold "when is this job done". A program hangs the end of
//! a job on one (exec, bind) and waits for it or makes another job wait for it. Requests and
//! layouts: `include/uapi/drm/drm.h` (`DRM_IOCTL_SYNCOBJ_*`), semantics as Linux'
//! `drm_syncobj.c`. A binary object holds one such point, a timeline object one per number.
//!
//! Not there: passing sync objects between programs as file descriptors.

use super::drm::User;
use super::file::{Backend, File};
use super::Errno;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// When something is done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Point {
    /// Done.
    Signaled,
    /// When job `seqno` of queue `queue` (the file's id) has finished.
    Job { queue: u32, seqno: u64 },
}

/// One sync object.
#[derive(Default)]
pub(super) struct SyncObj {
    binary: Option<Point>,
    timeline: BTreeMap<u64, Point>,
}

/// A sync entry of an exec or bind request.
pub(super) enum Sync {
    /// Store `value` at the program's address `addr` when done.
    UserFence { addr: u64, value: u64 },
    /// Sync object `handle`, point `point` (0: binary): `signal` – it gets the request's
    /// end; otherwise the request waits for it.
    Object {
        handle: u32,
        point: u64,
        signal: bool,
    },
}

const MAX_SYNCS: u32 = 16;
const MAX_OBJECTS: usize = 4096;
const MAX_HANDLES: u32 = 64;

const WAIT_ALL: u32 = 1;
const WAIT_FOR_SUBMIT: u32 = 2;
const WAIT_AVAILABLE: u32 = 4;

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn u64_at(b: &[u8], at: usize) -> u64 {
    u32_at(b, at) as u64 | (u32_at(b, at + 4) as u64) << 32
}

/// Reads `count` sync entries (48 bytes each) of a request.
pub(super) fn read(user: &mut impl User, addr: u64, count: u32) -> Result<Vec<Sync>, Errno> {
    if count > MAX_SYNCS {
        return Err(Errno::Inval);
    }
    let mut out = Vec::new();
    for i in 0..count as u64 {
        let s = user.read(addr + i * 48, 48).ok_or(Errno::Fault)?;
        if u64_at(&s, 0) != 0 || s[32..].iter().any(|x| *x != 0) {
            return Err(Errno::Inval);
        }
        let (kind, flags, target, value) = (
            u32_at(&s, 8),
            u32_at(&s, 12),
            u64_at(&s, 16),
            u64_at(&s, 24),
        );
        if flags & !1 != 0 {
            return Err(Errno::Inval);
        }
        let signal = flags & 1 != 0;
        out.push(match kind {
            // binary (no point) and timeline sync objects; a timeline entry that signals
            // needs a point
            0 | 1 if kind == 0 && value != 0 || kind == 1 && signal && value == 0 => {
                return Err(Errno::Inval)
            }
            0 | 1 => Sync::Object {
                handle: target as u32,
                point: value,
                signal,
            },
            2 if signal && target.is_multiple_of(8) => Sync::UserFence {
                addr: target,
                value,
            },
            _ => return Err(Errno::Inval),
        });
    }
    Ok(out)
}

impl SyncObj {
    /// The point a waiter for `point` looks at: the binary one, or the first of the timeline
    /// at or after `point`, with everything before it.
    fn done(&self, point: u64, finished: &mut impl FnMut(Point) -> bool) -> Option<bool> {
        if point == 0 {
            return self.binary.map(finished);
        }
        let upto = *self.timeline.range(point..).next()?.0;
        Some(self.timeline.range(..=upto).all(|(_, p)| finished(*p)))
    }

    fn set(&mut self, point: u64, to: Point) {
        if point == 0 {
            self.binary = Some(to);
        } else {
            self.timeline.insert(point, to);
        }
    }
}

impl<B: Backend> File<B> {
    /// Whether what `p` stands for is over.
    fn point_finished(&self, be: &mut B, p: Point) -> bool {
        match p {
            Point::Signaled => true,
            Point::Job { queue, seqno } => match self.queue_backend(queue) {
                Some(q) => be.queue_done(q) >= seqno,
                // the queue is gone: its jobs will not finish any more
                None => true,
            },
        }
    }

    /// `Some(done)` for a sync object's point, `None` if nothing was hung on it yet.
    fn sync_state(&self, be: &mut B, handle: u32, point: u64) -> Result<Option<bool>, Errno> {
        let obj = self.syncs.get(&handle).ok_or(Errno::NoEnt)?;
        Ok(obj.done(point, &mut |p| self.point_finished(be, p)))
    }

    /// Whether every sync object a request waits for is done ([`Errno::Again`] otherwise).
    pub(super) fn syncs_ready(&self, be: &mut B, syncs: &[Sync]) -> Result<(), Errno> {
        for s in syncs {
            if let Sync::Object {
                handle,
                point,
                signal: false,
            } = s
            {
                // waiting for something that was never submitted cannot end
                match self.sync_state(be, *handle, *point)? {
                    Some(true) => {}
                    Some(false) => return Err(Errno::Again),
                    None => return Err(Errno::Inval),
                }
            } else if let Sync::Object { handle, .. } = s {
                if !self.syncs.contains_key(handle) {
                    return Err(Errno::NoEnt);
                }
            }
        }
        Ok(())
    }

    /// Hangs `to` on every sync object the request signals.
    pub(super) fn syncs_signal(&mut self, syncs: &[Sync], to: Point) {
        for s in syncs {
            if let Sync::Object {
                handle,
                point,
                signal: true,
            } = s
            {
                if let Some(obj) = self.syncs.get_mut(handle) {
                    obj.set(*point, to);
                }
            }
        }
    }

    /// `DRM_IOCTL_SYNCOBJ_CREATE` (flag 1: created signaled).
    pub fn syncobj_create(&mut self, arg: &mut [u8]) -> Result<(), Errno> {
        let b = arg.get(..8).ok_or(Errno::Inval)?;
        let flags = u32_at(b, 4);
        if flags & !1 != 0 {
            return Err(Errno::Inval);
        }
        if self.syncs.len() >= MAX_OBJECTS {
            return Err(Errno::NoMem);
        }
        let mut obj = SyncObj::default();
        if flags == 1 {
            obj.binary = Some(Point::Signaled);
        }
        let handle = self.new_id();
        self.syncs.insert(handle, obj);
        arg[..4].copy_from_slice(&handle.to_le_bytes());
        Ok(())
    }

    /// `DRM_IOCTL_SYNCOBJ_DESTROY`.
    pub fn syncobj_destroy(&mut self, arg: &[u8]) -> Result<(), Errno> {
        let b = arg.get(..8).ok_or(Errno::Inval)?;
        if u32_at(b, 4) != 0 {
            return Err(Errno::Inval);
        }
        self.syncs
            .remove(&u32_at(b, 0))
            .map(|_| ())
            .ok_or(Errno::Inval)
    }

    fn handles(user: &mut impl User, addr: u64, count: u32) -> Result<Vec<u32>, Errno> {
        if count == 0 || count > MAX_HANDLES {
            return Err(Errno::Inval);
        }
        let b = user.read(addr, count as usize * 4).ok_or(Errno::Fault)?;
        Ok(b.chunks_exact(4).map(|c| u32_at(c, 0)).collect())
    }

    fn points(user: &mut impl User, addr: u64, count: u32) -> Result<Vec<u64>, Errno> {
        let b = user.read(addr, count as usize * 8).ok_or(Errno::Fault)?;
        Ok(b.chunks_exact(8).map(|c| u64_at(c, 0)).collect())
    }

    /// `DRM_IOCTL_SYNCOBJ_WAIT` and `…_TIMELINE_WAIT` (`timeline`): until one or all of the
    /// objects are done or the absolute time `timeout` (ns, monotonic) is reached.
    /// [`Errno::Again`]: not yet, ask again.
    pub fn syncobj_wait(
        &mut self,
        be: &mut B,
        arg: &mut [u8],
        user: &mut impl User,
        timeline: bool,
    ) -> Result<(), Errno> {
        let at = if timeline { 8 } else { 0 };
        let b = arg.get(..32 + at).ok_or(Errno::Inval)?;
        let (timeout, count, flags) = (
            u64_at(b, 8 + at) as i64,
            u32_at(b, 16 + at),
            u32_at(b, 20 + at),
        );
        if flags & !(WAIT_ALL | WAIT_FOR_SUBMIT | WAIT_AVAILABLE) != 0 {
            return Err(Errno::Inval);
        }
        let handles = Self::handles(user, u64_at(b, 0), count)?;
        let points = match timeline {
            true => Self::points(user, u64_at(b, 8), count)?,
            false => alloc::vec![0; count as usize],
        };
        let mut first = None;
        let mut all = true;
        for (i, (h, p)) in handles.iter().zip(&points).enumerate() {
            let done = match self.sync_state(be, *h, *p)? {
                None if flags & WAIT_FOR_SUBMIT == 0 => return Err(Errno::Inval),
                None => false,
                Some(done) => done || flags & WAIT_AVAILABLE != 0,
            };
            if done && first.is_none() {
                first = Some(i as u32);
            }
            all &= done;
        }
        let passed = if flags & WAIT_ALL != 0 {
            all
        } else {
            first.is_some()
        };
        if passed {
            arg[24 + at..28 + at].copy_from_slice(&first.unwrap_or(0).to_le_bytes());
            return Ok(());
        }
        if timeout <= 0 || be.now_ns() >= timeout as u64 {
            return Err(Errno::Time);
        }
        Err(Errno::Again)
    }

    /// `DRM_IOCTL_SYNCOBJ_RESET` (`to` = `None`) and `…_SIGNAL` (`to` = signaled) for the
    /// binary point of each object.
    pub(super) fn syncobj_set(
        &mut self,
        arg: &[u8],
        user: &mut impl User,
        to: Option<Point>,
    ) -> Result<(), Errno> {
        let b = arg.get(..16).ok_or(Errno::Inval)?;
        let handles = Self::handles(user, u64_at(b, 0), u32_at(b, 8))?;
        if handles.iter().any(|h| !self.syncs.contains_key(h)) {
            return Err(Errno::NoEnt);
        }
        for h in handles {
            if let Some(obj) = self.syncs.get_mut(&h) {
                obj.binary = to;
                if to.is_none() {
                    obj.timeline.clear();
                }
            }
        }
        Ok(())
    }

    /// `DRM_IOCTL_SYNCOBJ_TIMELINE_SIGNAL`: point `points[i]` of object `handles[i]` is done.
    pub fn syncobj_timeline_signal(
        &mut self,
        arg: &[u8],
        user: &mut impl User,
    ) -> Result<(), Errno> {
        let b = arg.get(..24).ok_or(Errno::Inval)?;
        let count = u32_at(b, 16);
        let handles = Self::handles(user, u64_at(b, 0), count)?;
        let points = Self::points(user, u64_at(b, 8), count)?;
        if u32_at(b, 20) != 0 || handles.iter().any(|h| !self.syncs.contains_key(h)) {
            return Err(Errno::NoEnt);
        }
        for (h, p) in handles.iter().zip(points) {
            if let Some(obj) = self.syncs.get_mut(h) {
                obj.set(p, Point::Signaled);
            }
        }
        Ok(())
    }

    /// `DRM_IOCTL_SYNCOBJ_QUERY`: the last point of each timeline that is done.
    pub fn syncobj_query(
        &mut self,
        be: &mut B,
        arg: &[u8],
        user: &mut impl User,
    ) -> Result<(), Errno> {
        let b = arg.get(..24).ok_or(Errno::Inval)?;
        let (out, count) = (u64_at(b, 8), u32_at(b, 16));
        let handles = Self::handles(user, u64_at(b, 0), count)?;
        for (i, h) in handles.iter().enumerate() {
            let obj = self.syncs.get(h).ok_or(Errno::NoEnt)?;
            let mut last = 0u64;
            for (point, p) in &obj.timeline {
                if !self.point_finished(be, *p) {
                    break;
                }
                last = *point;
            }
            if !user.write(out + i as u64 * 8, &last.to_le_bytes()) {
                return Err(Errno::Fault);
            }
        }
        Ok(())
    }

    /// `DRM_IOCTL_SYNCOBJ_TRANSFER`: what hangs on a point of one object also hangs on a
    /// point of another.
    pub fn syncobj_transfer(&mut self, arg: &[u8]) -> Result<(), Errno> {
        let b = arg.get(..32).ok_or(Errno::Inval)?;
        let (src, dst, from, to) = (u32_at(b, 0), u32_at(b, 4), u64_at(b, 8), u64_at(b, 16));
        if u32_at(b, 24) & !WAIT_FOR_SUBMIT != 0 || u32_at(b, 28) != 0 {
            return Err(Errno::Inval);
        }
        let source = self.syncs.get(&src).ok_or(Errno::NoEnt)?;
        let found = match from {
            0 => source.binary,
            _ => source.timeline.range(from..).next().map(|(_, p)| *p),
        };
        let point = found.ok_or(Errno::Inval)?;
        self.syncs.get_mut(&dst).ok_or(Errno::NoEnt)?.set(to, point);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
