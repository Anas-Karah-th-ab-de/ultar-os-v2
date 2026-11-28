//! A raw Wayland client for the tests: the wire format by hand (no
//! library), so the tests also send what a library never would.
#![allow(dead_code)]

use rustix::net::{
    recvmsg, sendmsg, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags,
};
use std::collections::VecDeque;
use std::io::{IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::Path;

/// One argument of a request.
pub enum Arg<'a> {
    U(u32),
    I(i32),
    F(f64),
    S(&'a str),
    A(&'a [u8]),
}

/// An event as it came.
#[derive(Clone, Debug)]
pub struct Ev {
    pub obj: u32,
    pub op: u16,
    pub body: Vec<u8>,
    pub fd: Option<std::sync::Arc<OwnedFd>>,
}

/// Reads arguments out of an event body.
pub struct Args<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Args<'a> {
    pub fn of(ev: &'a Ev) -> Args<'a> {
        Args { b: &ev.body, at: 0 }
    }
    pub fn u(&mut self) -> u32 {
        let v = u32::from_ne_bytes(self.b[self.at..self.at + 4].try_into().unwrap());
        self.at += 4;
        v
    }
    pub fn i(&mut self) -> i32 {
        self.u() as i32
    }
    pub fn f(&mut self) -> f64 {
        f64::from(self.i()) / 256.0
    }
    pub fn s(&mut self) -> String {
        let n = self.u() as usize;
        let s =
            String::from_utf8_lossy(&self.b[self.at..self.at + n.saturating_sub(1)]).into_owned();
        self.at += (n + 3) & !3;
        s
    }
    pub fn a(&mut self) -> Vec<u8> {
        let n = self.u() as usize;
        let v = self.b[self.at..self.at + n].to_vec();
        self.at += (n + 3) & !3;
        v
    }
}

/// The client.
pub struct Wl {
    pub sock: UnixStream,
    next: u32,
    inbuf: Vec<u8>,
    fds: VecDeque<OwnedFd>,
    pub events: Vec<Ev>,
    pub dead: bool,
    /// Event opcodes that carry a descriptor: (object, opcode).
    pub fd_events: Vec<(u32, u16)>,
}

impl Wl {
    pub fn connect(path: &Path) -> Wl {
        // The bridge binds before it listens: retry a moment.
        let mut tries = 0;
        let sock = loop {
            match UnixStream::connect(path) {
                Ok(s) => break s,
                Err(e) if tries < 200 => {
                    tries += 1;
                    let _ = e;
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => panic!("connect {}: {e}", path.display()),
            }
        };
        sock.set_nonblocking(true).unwrap();
        Wl {
            sock,
            next: 1,
            inbuf: Vec::new(),
            fds: VecDeque::new(),
            events: Vec::new(),
            dead: false,
            fd_events: Vec::new(),
        }
    }

    pub fn new_id(&mut self) -> u32 {
        self.next += 1;
        self.next
    }

    pub fn send(&mut self, obj: u32, op: u16, args: &[Arg<'_>]) {
        self.send_fd(obj, op, args, None)
    }

    pub fn send_fd(&mut self, obj: u32, op: u16, args: &[Arg<'_>], fd: Option<BorrowedFd<'_>>) {
        let mut body = Vec::new();
        for a in args {
            match a {
                Arg::U(v) => body.extend_from_slice(&v.to_ne_bytes()),
                Arg::I(v) => body.extend_from_slice(&v.to_ne_bytes()),
                Arg::F(v) => body.extend_from_slice(&((v * 256.0) as i32).to_ne_bytes()),
                Arg::S(s) => {
                    body.extend_from_slice(&(s.len() as u32 + 1).to_ne_bytes());
                    body.extend_from_slice(s.as_bytes());
                    body.push(0);
                    while body.len() % 4 != 0 {
                        body.push(0);
                    }
                }
                Arg::A(a) => {
                    body.extend_from_slice(&(a.len() as u32).to_ne_bytes());
                    body.extend_from_slice(a);
                    while body.len() % 4 != 0 {
                        body.push(0);
                    }
                }
            }
        }
        let mut msg = Vec::new();
        msg.extend_from_slice(&obj.to_ne_bytes());
        msg.extend_from_slice(&(((body.len() as u32 + 8) << 16) | u32::from(op)).to_ne_bytes());
        msg.extend_from_slice(&body);
        self.raw(&msg, fd);
    }

    /// Writes bytes as they are (with an optional descriptor).
    pub fn raw(&mut self, msg: &[u8], fd: Option<BorrowedFd<'_>>) {
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut anc = SendAncillaryBuffer::new(&mut space);
        let fds;
        if let Some(fd) = fd {
            fds = [fd];
            assert!(anc.push(SendAncillaryMessage::ScmRights(&fds)), "fd fits");
        }
        let mut sent = 0;
        let mut first = true;
        while sent < msg.len() {
            let r = if first {
                sendmsg(
                    &self.sock,
                    &[IoSlice::new(&msg[sent..])],
                    &mut anc,
                    SendFlags::NOSIGNAL,
                )
            } else {
                sendmsg(
                    &self.sock,
                    &[IoSlice::new(&msg[sent..])],
                    &mut SendAncillaryBuffer::default(),
                    SendFlags::NOSIGNAL,
                )
            };
            match r {
                Ok(n) => {
                    sent += n;
                    first = false;
                }
                Err(rustix::io::Errno::AGAIN) => {
                    std::thread::sleep(std::time::Duration::from_millis(1))
                }
                Err(e) => {
                    eprintln!("sendmsg: {e:?}");
                    self.dead = true;
                    return;
                }
            }
        }
    }

    /// Reads what is there (non-blocking).
    pub fn read(&mut self) {
        loop {
            let mut buf = [0u8; 65536];
            let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(8))];
            let mut anc = RecvAncillaryBuffer::new(&mut space);
            let mut iov = [IoSliceMut::new(&mut buf)];
            match recvmsg(&self.sock, &mut iov, &mut anc, RecvFlags::CMSG_CLOEXEC) {
                Ok(r) if r.bytes == 0 => {
                    self.dead = true;
                    return;
                }
                Ok(r) => {
                    for m in anc.drain() {
                        if let RecvAncillaryMessage::ScmRights(fds) = m {
                            self.fds.extend(fds);
                        }
                    }
                    let n = r.bytes;
                    self.inbuf.extend_from_slice(&buf[..n]);
                }
                Err(rustix::io::Errno::AGAIN) => break,
                Err(e) => {
                    eprintln!("recvmsg: {e:?}");
                    self.dead = true;
                    return;
                }
            }
        }
        self.split();
    }

    fn split(&mut self) {
        while self.inbuf.len() >= 8 {
            let obj = u32::from_ne_bytes(self.inbuf[0..4].try_into().unwrap());
            let w = u32::from_ne_bytes(self.inbuf[4..8].try_into().unwrap());
            let (size, op) = ((w >> 16) as usize, (w & 0xFFFF) as u16);
            if size < 8 || self.inbuf.len() < size {
                break;
            }
            let body = self.inbuf[8..size].to_vec();
            self.inbuf.drain(..size);
            let fd = if self.fd_events.contains(&(obj, op)) {
                self.fds.pop_front().map(std::sync::Arc::new)
            } else {
                None
            };
            self.events.push(Ev { obj, op, body, fd });
        }
    }

    /// Takes the events of `obj` with opcode `op`.
    pub fn take(&mut self, obj: u32, op: u16) -> Vec<Ev> {
        let (hit, rest): (Vec<Ev>, Vec<Ev>) = self
            .events
            .drain(..)
            .partition(|e| e.obj == obj && e.op == op);
        self.events = rest;
        hit
    }

    pub fn has(&self, obj: u32, op: u16) -> bool {
        self.events.iter().any(|e| e.obj == obj && e.op == op)
    }

    /// wl_display.error, if any: (object, code, message).
    pub fn error(&self) -> Option<(u32, u32, String)> {
        let e = self.events.iter().find(|e| e.obj == 1 && e.op == 0)?;
        let mut a = Args::of(e);
        Some((a.u(), a.u(), a.s()))
    }

    pub fn fd_of(ev: &Ev) -> BorrowedFd<'_> {
        ev.fd.as_ref().unwrap().as_fd()
    }
}
