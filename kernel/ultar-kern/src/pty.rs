//! Pseudo-terminals: `/dev/ptmx` gives a master end, `/dev/pts/<n>` the slave end of terminal `n`
//! (apt logs dpkg through one, terminal apps such as foot run their shell on one).
//!
//! A terminal is two pipes – input (master writes, slave reads) and output (slave writes, master
//! reads) – plus its settings: termios, window size, lock, session and foreground group. A small line
//! discipline works on the bytes as they are written: input gets `ICRNL`, `ISIG` (^C ^\ ^Z), `ICANON`
//! (a line goes to the slave at its end; erase and kill edit it) and `ECHO`; output gets `OPOST|ONLCR`.
//!
//! Terminals are not per mount namespace (Linux has devpts instances): a slave opens only for the user
//! that opened its master (or a holder of `FILE_ADMIN`), and `/dev/pts` lists only the caller's own –
//! apps of different cells have different users and so never reach each other's terminals.

use crate::pipe::{Pipe, PIPE_DEFAULT};
use alloc::collections::BTreeMap;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use spin::Mutex;

const EIO: i64 = 5;
const EACCES: i64 = 13;
const ENOENT: i64 = 2;
const ENOSPC: i64 = 28;
const EINVAL: i64 = 22;
const ENOTTY: i64 = 25;

/// At most this many terminals at once.
const MAX_PTYS: u32 = 256;

// termios flags used here
const ICRNL: u32 = 0x100;
const OPOST: u32 = 0x1;
const ONLCR: u32 = 0x4;
const ISIG: u32 = 0x1;
const ICANON: u32 = 0x2;
const ECHO: u32 = 0x8;
// indices into c_cc (offset 17 in struct termios)
const VINTR: usize = 0;
const VQUIT: usize = 1;
const VERASE: usize = 2;
const VKILL: usize = 3;
const VEOF: usize = 4;
const VSUSP: usize = 10;

/// Linux's defaults for a new terminal (36 bytes of `struct termios`).
fn default_termios() -> [u8; 36] {
    let mut b = [0u8; 36];
    b[0..4].copy_from_slice(&(ICRNL | 0x400).to_le_bytes()); // ICRNL|IXON
    b[4..8].copy_from_slice(&(OPOST | ONLCR).to_le_bytes());
    b[8..12].copy_from_slice(&0xbfu32.to_le_bytes()); // B38400|CS8|CREAD
    b[12..16].copy_from_slice(&0x8a3bu32.to_le_bytes()); // ISIG|ICANON|ECHO|ECHOE|ECHOK|ECHOCTL|ECHOKE|IEXTEN
    let cc = [3u8, 28, 127, 21, 4, 0, 1, 0, 17, 19, 26, 0, 18, 15, 23, 22];
    b[17..17 + cc.len()].copy_from_slice(&cc);
    b
}

struct State {
    termios: [u8; 36],
    winsize: [u8; 8],
    locked: bool,
    /// the first slave took over the placeholder ends the master held for it
    slave_seen: bool,
    /// foreground process group and session (0: none yet)
    pgrp: u64,
    sid: u64,
    /// canonical mode: the line being typed
    line: Vec<u8>,
}

impl State {
    fn flag(&self, at: usize) -> u32 {
        let mut w = [0u8; 4];
        w.copy_from_slice(&self.termios[at..at + 4]);
        u32::from_le_bytes(w)
    }
    fn cc(&self, i: usize) -> u8 {
        self.termios[17 + i]
    }
}

pub struct Pty {
    pub num: u32,
    owner: u32,
    /// master → slave
    inp: Arc<Pipe>,
    /// slave → master
    out: Arc<Pipe>,
    state: Mutex<State>,
}

/// One open end; the descriptor kind is `FdKind::Pty(rx, tx, end)`.
pub struct End {
    pub pty: Arc<Pty>,
    pub master: bool,
}

static PTYS: Mutex<BTreeMap<u32, Weak<Pty>>> = Mutex::new(BTreeMap::new());

/// `open("/dev/ptmx")`: a new terminal, its master end (rx, tx, end). Until a slave opens, the master
/// holds placeholder ends for it, so its reads wait and its writes are kept (as on Linux).
pub fn open_master(uid: u32) -> Result<(Arc<Pipe>, Arc<Pipe>, Arc<End>), i64> {
    let mut table = PTYS.lock();
    table.retain(|_, w| w.strong_count() > 0);
    let num = (0..MAX_PTYS).find(|n| !table.contains_key(n)).ok_or(-ENOSPC)?;
    let (inp, out) = (Pipe::with_capacity(PIPE_DEFAULT), Pipe::with_capacity(PIPE_DEFAULT));
    for p in [&inp, &out] {
        p.readers.store(1, Ordering::SeqCst);
        p.writers.store(1, Ordering::SeqCst);
    }
    let state = State { termios: default_termios(), winsize: [0; 8], locked: true, slave_seen: false, pgrp: 0, sid: 0, line: Vec::new() };
    let pty = Arc::new(Pty { num, owner: uid, inp: inp.clone(), out: out.clone(), state: Mutex::new(state) });
    table.insert(num, Arc::downgrade(&pty));
    Ok((out, inp, Arc::new(End { pty, master: true })))
}

/// `open("/dev/pts/<n>")` (or `TIOCGPTPEER`): the slave end of terminal `n` – unlocked, and only for the
/// user that opened the master (or `admin`).
pub fn open_slave(num: u32, uid: u32, admin: bool) -> Result<(Arc<Pipe>, Arc<Pipe>, Arc<End>), i64> {
    let pty = PTYS.lock().get(&num).and_then(Weak::upgrade).ok_or(-ENOENT)?;
    open_slave_of(&pty, uid, admin)
}

fn open_slave_of(pty: &Arc<Pty>, uid: u32, admin: bool) -> Result<(Arc<Pipe>, Arc<Pipe>, Arc<End>), i64> {
    if pty.owner != uid && !admin {
        return Err(-EACCES);
    }
    let mut st = pty.state.lock();
    if st.locked {
        return Err(-EIO);
    }
    if pty.inp.write_closed() {
        // the master is gone: the terminal hung up
        return Err(-EIO);
    }
    if st.slave_seen {
        pty.inp.readers.fetch_add(1, Ordering::SeqCst);
        pty.out.writers.fetch_add(1, Ordering::SeqCst);
    }
    st.slave_seen = true;
    Ok((pty.inp.clone(), pty.out.clone(), Arc::new(End { pty: pty.clone(), master: false })))
}

/// Numbers of all live terminals (the directory `/dev/pts`; opening one checks its owner).
pub fn all() -> Vec<u32> {
    PTYS.lock().values().filter_map(Weak::upgrade).map(|p| p.num).collect()
}

/// The user owning live terminal `num` (the one who opened its master): `/dev/pts/N` belongs to them.
pub fn owner(num: u32) -> Option<u32> {
    PTYS.lock().get(&num).and_then(Weak::upgrade).map(|p| p.owner)
}

/// What a write to one end becomes: the bytes for the other end, an echo back to the writer (master
/// only) and a signal for the foreground group (`(group, signal)`).
pub struct Cooked {
    pub data: Vec<u8>,
    pub echo: Vec<u8>,
    pub signal: Option<(u64, u64)>,
}

/// Line discipline for a write to `end`.
pub fn cook(end: &End, bytes: &[u8]) -> Cooked {
    let mut st = end.pty.state.lock();
    if !end.master {
        return Cooked { data: output(&st, bytes), echo: Vec::new(), signal: None };
    }
    input(&mut st, bytes)
}

/// Output processing (slave → master): `OPOST|ONLCR` turns `\n` into `\r\n`.
fn output(st: &State, bytes: &[u8]) -> Vec<u8> {
    let o = st.flag(4);
    if o & OPOST == 0 || o & ONLCR == 0 {
        return bytes.to_vec();
    }
    let mut v = Vec::with_capacity(bytes.len() + 8);
    for &b in bytes {
        if b == b'\n' {
            v.push(b'\r');
        }
        v.push(b);
    }
    v
}

/// Input processing (master → slave): ICRNL, ISIG, ICANON with erase/kill, ECHO.
fn input(st: &mut State, bytes: &[u8]) -> Cooked {
    let (i, l) = (st.flag(0), st.flag(12));
    let mut c = Cooked { data: Vec::new(), echo: Vec::new(), signal: None };
    for &raw in bytes {
        let b = if raw == b'\r' && i & ICRNL != 0 { b'\n' } else { raw };
        if l & ISIG != 0 {
            let sig = [(VINTR, 2u64), (VQUIT, 3), (VSUSP, 20)].into_iter().find(|(k, _)| b != 0 && b == st.cc(*k)).map(|(_, s)| s);
            if let Some(s) = sig {
                st.line.clear();
                c.signal = Some((st.pgrp, s));
                if l & ECHO != 0 {
                    c.echo.extend_from_slice(&[b'^', b ^ 0x40, b'\r', b'\n']);
                }
                continue;
            }
        }
        if l & ICANON == 0 {
            c.data.push(b);
            if l & ECHO != 0 {
                c.echo.push(b);
            }
            continue;
        }
        canonical(st, b, l & ECHO != 0, &mut c);
    }
    c
}

fn canonical(st: &mut State, b: u8, echo: bool, c: &mut Cooked) {
    if b == st.cc(VERASE) || b == 8 {
        if st.line.pop().is_some() && echo {
            c.echo.extend_from_slice(b"\x08 \x08");
        }
    } else if b == st.cc(VKILL) {
        if echo {
            c.echo.extend(core::iter::repeat_n(b"\x08 \x08", st.line.len()).flatten());
        }
        st.line.clear();
    } else if b == st.cc(VEOF) {
        // ^D: the line goes as it is (an empty one reads as end of file)
        c.data.append(&mut st.line);
    } else {
        st.line.push(b);
        if echo {
            if b == b'\n' {
                c.echo.extend_from_slice(b"\r\n");
            } else {
                c.echo.push(b);
            }
        }
        if b == b'\n' || st.line.len() >= 4095 {
            c.data.append(&mut st.line);
        }
    }
}

/// Terminal requests. `peer` opens the slave for `TIOCGPTPEER` (the caller adds the descriptor);
/// `caller_pgid`: the caller's process group (session and foreground group default to it).
pub enum Answer {
    Val(i64),
    Bytes(Vec<u8>),
    OpenPeer,
    /// the window size changed: SIGWINCH for this group
    Resized(u64),
}

pub fn ioctl(end: &End, req: u64, arg_in: Option<&[u8]>, caller_pgid: u64, pending: usize) -> Answer {
    let mut st = end.pty.state.lock();
    let word = |b: Option<&[u8]>| b.and_then(|b| b.get(0..4)).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]));
    match req {
        0x5401 | 0x802c_542a => {
            let mut b = st.termios.to_vec();
            if req != 0x5401 {
                b.extend_from_slice(&38400u32.to_le_bytes());
                b.extend_from_slice(&38400u32.to_le_bytes());
            }
            Answer::Bytes(b)
        }
        0x5402..=0x5404 | 0x402c_542b..=0x402c_542d => match arg_in {
            Some(b) if b.len() >= 36 => {
                st.termios.copy_from_slice(&b[..36]);
                if st.flag(12) & ICANON == 0 && !st.line.is_empty() {
                    // leaving canonical mode: what was typed goes to the reader
                    let line = core::mem::take(&mut st.line);
                    drop(st);
                    let _ = end.pty.inp.push(&line);
                }
                Answer::Val(0)
            }
            _ => Answer::Val(-EINVAL),
        },
        0x5413 => Answer::Bytes(st.winsize.to_vec()),
        0x5414 => match arg_in {
            Some(b) if b.len() >= 8 => {
                st.winsize.copy_from_slice(&b[..8]);
                Answer::Resized(st.pgrp)
            }
            _ => Answer::Val(-EINVAL),
        },
        0x8004_5430 if end.master => Answer::Bytes(end.pty.num.to_le_bytes().to_vec()),
        0x4004_5431 if end.master => {
            st.locked = word(arg_in).unwrap_or(1) != 0;
            Answer::Val(0)
        }
        0x5441 if end.master => Answer::OpenPeer,
        // TIOCSCTTY: becomes the session's terminal; TIOCNOTTY
        0x540E => {
            st.sid = caller_pgid;
            st.pgrp = caller_pgid;
            Answer::Val(0)
        }
        0x5422 => Answer::Val(0),
        0x540F => Answer::Bytes((if st.pgrp != 0 { st.pgrp } else { caller_pgid } as u32).to_le_bytes().to_vec()),
        0x5410 => {
            st.pgrp = word(arg_in).unwrap_or(0) as u64;
            Answer::Val(0)
        }
        0x5429 => Answer::Bytes((if st.sid != 0 { st.sid } else { caller_pgid } as u32).to_le_bytes().to_vec()),
        0x541B => Answer::Bytes((pending as u32).to_le_bytes().to_vec()),
        // TCSBRK, TCXONC, TCFLSH
        0x5409..=0x540B => Answer::Val(0),
        _ => Answer::Val(-ENOTTY),
    }
}
