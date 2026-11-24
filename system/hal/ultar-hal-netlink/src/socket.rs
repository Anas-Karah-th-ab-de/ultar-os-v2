//! A netlink socket: requests answered by acknowledgement or dump, and the
//! events (multicast, or unicast to this socket like EAPOL frames of the
//! nl80211 control port) that arrive in between, kept in a bounded queue.

use crate::attr::{self, Builder, F_ACK, F_MULTI, NLMSG_DONE, NLMSG_ERROR};
use crate::{sys, NetError};
use rustix::event::{poll, PollFd, PollFlags, Timespec};
use rustix::net::netlink::SocketAddrNetlink;
use rustix::net::{AddressFamily, Protocol, RecvFlags, SendFlags, SocketFlags, SocketType};
use std::collections::VecDeque;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::time::{Duration, Instant};

/// Netlink protocol of rtnetlink.
pub const ROUTE: u32 = 0;
/// Netlink protocol of generic netlink.
pub const GENERIC: u32 = 16;
/// Events kept while a request waits.
pub const MAX_EVENTS: usize = 128;
/// Answers of one dump accepted.
pub const MAX_ANSWERS: usize = 4096;
/// Receive buffer.
const BUF: usize = 64 * 1024;
/// Default wait for an answer.
pub const ANSWER_WAIT: Duration = Duration::from_secs(5);

/// One message that was not the answer to a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    /// Message type (generic netlink: the family id).
    pub kind: u16,
    /// Payload after the netlink header.
    pub payload: Vec<u8>,
}

/// A netlink socket.
#[derive(Debug)]
pub struct Socket {
    fd: OwnedFd,
    seq: u32,
    events: VecDeque<Event>,
    buf: Vec<u8>,
}

impl Socket {
    /// Opens a socket of netlink `protocol`.
    pub fn open(protocol: u32) -> Result<Socket, NetError> {
        let proto = std::num::NonZeroU32::new(protocol).map(Protocol::from_raw);
        let fd = rustix::net::socket_with(
            AddressFamily::NETLINK,
            SocketType::RAW,
            SocketFlags::CLOEXEC,
            proto,
        )?;
        rustix::net::bind(&fd, &SocketAddrNetlink::new(0, 0))?;
        Ok(Socket {
            fd,
            seq: 1,
            events: VecDeque::new(),
            buf: vec![0; BUF],
        })
    }

    /// The descriptor (to wait on it).
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }

    /// Joins a multicast group.
    pub fn join(&self, group: u32) -> Result<(), NetError> {
        sys::add_membership(self.fd.as_fd(), group).map_err(NetError::from)
    }

    fn send(&mut self, b: Builder) -> Result<u32, NetError> {
        self.seq = self.seq.wrapping_add(1).max(1);
        let bytes = b.finish(self.seq);
        rustix::net::sendto(
            &self.fd,
            &bytes,
            SendFlags::empty(),
            &SocketAddrNetlink::new(0, 0),
        )?;
        Ok(self.seq)
    }

    /// Reads one datagram, waiting at most `wait` (`None`: no wait).
    fn read(&mut self, wait: Option<Duration>) -> Result<Option<Vec<u8>>, NetError> {
        let timeout = wait.map(|d| Timespec {
            tv_sec: d.as_secs() as i64,
            tv_nsec: i64::from(d.subsec_nanos()),
        });
        let mut fds = [PollFd::new(&self.fd, PollFlags::IN)];
        let zero = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        if poll(&mut fds, Some(timeout.as_ref().unwrap_or(&zero)))? == 0 {
            return Ok(None);
        }
        let (n, full) = rustix::net::recv(&self.fd, &mut self.buf[..], RecvFlags::DONTWAIT)?;
        if full > n {
            return Err(NetError::Malformed);
        }
        Ok(Some(self.buf[..n].to_vec()))
    }

    fn keep(&mut self, kind: u16, payload: &[u8]) {
        if self.events.len() >= MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(Event {
            kind,
            payload: payload.to_vec(),
        });
    }

    /// Sends a request and collects its answers (a dump's parts, or nothing
    /// for an acknowledged change). Events arriving meanwhile are kept.
    pub fn request(&mut self, b: Builder) -> Result<Vec<Vec<u8>>, NetError> {
        self.request_within(b, ANSWER_WAIT)
    }

    /// [`Socket::request`] with its own wait.
    pub fn request_within(&mut self, b: Builder, wait: Duration) -> Result<Vec<Vec<u8>>, NetError> {
        let seq = self.send(b)?;
        let until = Instant::now() + wait;
        let mut answers = Vec::new();
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(NetError::Timeout);
            }
            let Some(data) = self.read(Some(left))? else {
                return Err(NetError::Timeout);
            };
            for m in attr::messages(&data) {
                let m = m?;
                if m.seq != seq {
                    if m.kind != NLMSG_ERROR && m.kind != NLMSG_DONE {
                        self.keep(m.kind, m.payload);
                    }
                    continue;
                }
                match m.kind {
                    NLMSG_ERROR => {
                        let code = attr::error_code(m.payload)?;
                        return if code == 0 {
                            Ok(answers)
                        } else {
                            Err(NetError::Kernel(code))
                        };
                    }
                    NLMSG_DONE => return Ok(answers),
                    _ => {
                        if answers.len() >= MAX_ANSWERS {
                            return Err(NetError::Malformed);
                        }
                        answers.push(m.payload.to_vec());
                        if m.flags & F_MULTI == 0 {
                            // A single answer; its acknowledgement follows
                            // when one was asked for.
                            continue;
                        }
                    }
                }
            }
        }
    }

    /// Sends a change and waits for its acknowledgement.
    pub fn change(&mut self, b: Builder) -> Result<(), NetError> {
        self.request(b).map(|_| ())
    }

    /// The next event, waiting at most `wait` (`None`: only what is there).
    pub fn next_event(&mut self, wait: Option<Duration>) -> Result<Option<Event>, NetError> {
        if let Some(e) = self.events.pop_front() {
            return Ok(Some(e));
        }
        let Some(data) = self.read(wait)? else {
            return Ok(None);
        };
        for m in attr::messages(&data) {
            let m = m?;
            if m.kind != NLMSG_ERROR && m.kind != NLMSG_DONE {
                self.keep(m.kind, m.payload);
            }
        }
        Ok(self.events.pop_front())
    }
}

/// Flags of a change that wants an acknowledgement.
pub const CHANGE: u16 = attr::F_REQUEST | F_ACK;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attr::{F_DUMP, F_REQUEST};

    #[test]
    fn route_dump_of_links_works_on_linux() {
        // rtnetlink exists on every Linux host: dump the links (lo at least).
        let mut s = Socket::open(ROUTE).unwrap();
        let answers = s
            .request(Builder::new(18, F_REQUEST | F_DUMP).raw(&[0u8; 16]))
            .unwrap();
        assert!(!answers.is_empty());
        assert!(s.next_event(None).unwrap().is_none());
        let _ = s.fd();
    }

    #[test]
    fn errors_come_back_typed() {
        let mut s = Socket::open(ROUTE).unwrap();
        // Delete an address of interface index 999999: no such device.
        let r = s.change(
            Builder::new(21, CHANGE)
                .raw(&[2, 24, 0, 0, 0x3f, 0x42, 0x0f, 0x00])
                .attr(2, &[10, 9, 9, 9]),
        );
        assert!(matches!(r, Err(NetError::Kernel(_))), "{r:?}");
        assert!(Socket::open(9999).is_err());
    }
}
