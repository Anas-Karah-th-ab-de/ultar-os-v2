//! The command buffers (CTB): two rings in memory shared with the GuC – H2G for requests of the
//! host, G2H for responses and events of the GuC. They replace the slow register channel for
//! everything after the start; submission of work runs through them. Layout and rules follow
//! `xe_guc_ct.c` and `abi/guc_communication_ctb_abi.h` of the Linux `xe` driver (MIT).
//!
//! ```text
//! | H2G descriptor | G2H descriptor | H2G ring 16K | G2H ring 128K |     one buffer in the GGTT
//! 0                2K               4K             20K             148K
//! ```
//! A message is one header word (fence, format, length) and an HXG message (`hxg.rs`). Each ring
//! has a descriptor with head, tail and status; the writer moves the tail, the reader the head.
//!
//! Interrupts are not used yet: [`Ct::request`] polls the G2H ring.

use crate::hxg;
use crate::regs::GUC_HOST_INTERRUPT;
use crate::{Error, Hw, Result};
use alloc::vec::Vec;

/// The shared buffer, addressed in 32-bit words. The kernel's implementation writes through to
/// memory (the GuC does not see the CPU's caches) and reads from memory.
pub trait Shared {
    /// Reads word `index`.
    fn read(&mut self, index: usize) -> u32;
    /// Writes word `index`.
    fn write(&mut self, index: usize, val: u32);
}

const H2G_DESC: usize = 0;
const G2H_DESC: usize = 2048 / 4;
const H2G_CMDS: usize = 4096 / 4;
/// Words of the H2G ring (16 KiB).
pub const H2G_WORDS: u32 = (16 << 10) / 4;
const G2H_CMDS: usize = H2G_CMDS + H2G_WORDS as usize;
/// Words of the G2H ring (128 KiB).
pub const G2H_WORDS: u32 = (128 << 10) / 4;
/// Size of the shared buffer in bytes.
pub const SIZE_BYTES: u32 = (G2H_CMDS as u32 + G2H_WORDS) * 4;
/// Longest action (HXG words) this driver sends.
pub const MAX_ACTION_WORDS: usize = 64;

const DESC_HEAD: usize = 0;
const DESC_TAIL: usize = 1;
const DESC_STATUS: usize = 2;
const DESC_WORDS: usize = 16;

const ACTION_SELF_CFG: u32 = 0x0508;
const ACTION_CONTROL_CTB: u32 = 0x4509;
const KLV_H2G_CTB_ADDR: u32 = 0x0902;
const KLV_H2G_CTB_DESCRIPTOR_ADDR: u32 = 0x0903;
const KLV_H2G_CTB_SIZE: u32 = 0x0904;
const KLV_G2H_CTB_ADDR: u32 = 0x0905;
const KLV_G2H_CTB_DESCRIPTOR_ADDR: u32 = 0x0906;
const KLV_G2H_CTB_SIZE: u32 = 0x0907;

const HXG_ORIGIN_GUC: u32 = 1 << 31;
const HXG_TYPE_SHIFT: u32 = 28;
const HXG_TYPE_FAST_REQUEST: u32 = 2 << HXG_TYPE_SHIFT;
const HXG_DATA: u32 = 0x0fff_ffff;

/// How often a request the GuC asks to retry is sent again.
const MAX_RETRIES: u32 = 3;

/// A message of the GuC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum G2h {
    /// The request with this fence succeeded.
    Response {
        /// Fence of the request.
        fence: u16,
        /// The 28 data bits of the response header.
        data0: u32,
        /// Further words of the response.
        payload: Vec<u32>,
    },
    /// The request with this fence failed.
    Failure {
        /// Fence of the request.
        fence: u16,
        /// The GuC's error code.
        error: u32,
        /// The GuC's hint to the error.
        hint: u32,
    },
    /// The GuC asks to send the request with this fence again.
    Retry {
        /// Fence of the request.
        fence: u16,
    },
    /// The GuC is still working on the request with this fence.
    Busy {
        /// Fence of the request.
        fence: u16,
    },
    /// Something the GuC reports on its own.
    Event {
        /// What happened.
        action: u32,
        /// Its details.
        payload: Vec<u32>,
    },
}

/// The host's side of the two rings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ct {
    tail: u32,
    head: u32,
    fence: u16,
    /// Events that arrived while another one was waited for (action, payload), oldest first;
    /// at most [`BACKLOG`] are kept. The owner takes them out with [`Ct::take_backlog`].
    backlog: Vec<(u32, Vec<u32>)>,
}

/// Number of events kept for the owner while it waits for another one.
pub const BACKLOG: usize = 64;

fn self_cfg(hw: &mut impl Hw, key: u32, len: u32, val: u64) -> Result<()> {
    let request = [
        ACTION_SELF_CFG,
        key << 16 | len,
        val as u32,
        (val >> 32) as u32,
    ];
    // the GuC answers with the number of keys it took
    match hxg::send_recv(hw, &request)?.data0 {
        1 => Ok(()),
        n => Err(Error::GucProtocol {
            request: key,
            reply: n,
        }),
    }
}

fn control(hw: &mut impl Hw, enable: bool) -> Result<()> {
    hxg::send_recv(hw, &[ACTION_CONTROL_CTB, enable as u32]).map(|_| ())
}

impl Ct {
    /// Clears both descriptors, tells the GuC where descriptors and rings are (register channel)
    /// and switches the buffers on. `ggtt` is where the shared buffer of [`SIZE_BYTES`] is mapped.
    pub fn enable(hw: &mut impl Hw, mem: &mut impl Shared, ggtt: u32) -> Result<Ct> {
        if !ggtt.is_multiple_of(4096) || ggtt.checked_add(SIZE_BYTES).is_none() {
            return Err(Error::Invalid("CTB address"));
        }
        for i in 0..DESC_WORDS {
            mem.write(H2G_DESC + i, 0);
            mem.write(G2H_DESC + i, 0);
        }
        let at = |word: usize| (ggtt + word as u32 * 4) as u64;
        self_cfg(hw, KLV_H2G_CTB_DESCRIPTOR_ADDR, 2, at(H2G_DESC))?;
        self_cfg(hw, KLV_H2G_CTB_ADDR, 2, at(H2G_CMDS))?;
        self_cfg(hw, KLV_H2G_CTB_SIZE, 1, (H2G_WORDS * 4) as u64)?;
        self_cfg(hw, KLV_G2H_CTB_DESCRIPTOR_ADDR, 2, at(G2H_DESC))?;
        self_cfg(hw, KLV_G2H_CTB_ADDR, 2, at(G2H_CMDS))?;
        self_cfg(hw, KLV_G2H_CTB_SIZE, 1, (G2H_WORDS * 4) as u64)?;
        control(hw, true)?;
        Ok(Ct {
            tail: 0,
            head: 0,
            fence: 0,
            backlog: Vec::new(),
        })
    }

    /// Switches the buffers off (register channel).
    pub fn disable(self, hw: &mut impl Hw) -> Result<()> {
        control(hw, false)
    }

    /// Writes a request (word 0: the action) into the H2G ring and notifies the GuC. Returns the
    /// fence the response will carry.
    pub fn send(&mut self, hw: &mut impl Hw, mem: &mut impl Shared, action: &[u32]) -> Result<u16> {
        if action.first().is_some_and(|a| a & !HXG_DATA != 0) {
            return Err(Error::Invalid("CTB request"));
        }
        self.send_words(hw, mem, action)
    }

    fn send_words(
        &mut self,
        hw: &mut impl Hw,
        mem: &mut impl Shared,
        action: &[u32],
    ) -> Result<u16> {
        if action.is_empty() || action.len() > MAX_ACTION_WORDS {
            return Err(Error::Invalid("CTB request"));
        }
        if mem.read(H2G_DESC + DESC_STATUS) != 0 {
            return Err(Error::Ctb("H2G ring reports an error"));
        }
        let head = mem.read(H2G_DESC + DESC_HEAD);
        if head >= H2G_WORDS {
            return Err(Error::Ctb("H2G head out of range"));
        }
        let full = action.len() as u32 + 1;
        // one word always stays free, so a full ring differs from an empty one
        let free = head.wrapping_sub(self.tail + 1) & (H2G_WORDS - 1);
        let to_end = H2G_WORDS - self.tail;
        let wrap = full > to_end;
        if full + if wrap { to_end } else { 0 } > free {
            return Err(Error::Ctb("H2G ring full"));
        }
        if wrap {
            // a message never wraps: the rest of the ring is filled with empty words
            for i in self.tail..H2G_WORDS {
                mem.write(H2G_CMDS + i as usize, 0);
            }
            self.tail = 0;
        }
        self.fence = self.fence.wrapping_add(1);
        let base = H2G_CMDS + self.tail as usize;
        mem.write(base, (self.fence as u32) << 16 | action.len() as u32);
        for (i, w) in action.iter().enumerate() {
            mem.write(base + 1 + i, *w);
        }
        self.tail = (self.tail + full) % H2G_WORDS;
        mem.write(H2G_DESC + DESC_TAIL, self.tail);
        hw.write(GUC_HOST_INTERRUPT, 0);
        Ok(self.fence)
    }

    /// Sends a request the GuC does not answer ("fast request"): it only reports a failure, and
    /// what the request caused arrives as an event (see [`Ct::wait_event`]).
    pub fn post(&mut self, hw: &mut impl Hw, mem: &mut impl Shared, action: &[u32]) -> Result<u16> {
        let mut words = action.to_vec();
        if let Some(first) = words.first_mut() {
            if *first & !HXG_DATA != 0 {
                return Err(Error::Invalid("CTB request"));
            }
            *first |= HXG_TYPE_FAST_REQUEST;
        }
        self.send_words(hw, mem, &words)
    }

    /// Polls for the event `action` (at most `timeout_us`) and returns its payload. A failure
    /// the GuC reports meanwhile ends the wait with that failure; other messages are dropped.
    pub fn wait_event(
        &mut self,
        hw: &mut impl Hw,
        mem: &mut impl Shared,
        action: u32,
        timeout_us: u32,
    ) -> Result<Vec<u32>> {
        let start = hw.now_us();
        loop {
            match self.recv(mem)? {
                Some(G2h::Event { action: a, payload }) if a == action => return Ok(payload),
                Some(G2h::Failure { error, hint, .. }) => {
                    return Err(Error::GucRequest {
                        request: action,
                        error,
                        hint,
                    })
                }
                Some(G2h::Event { action: a, payload }) => {
                    if self.backlog.len() == BACKLOG {
                        self.backlog.remove(0);
                    }
                    self.backlog.push((a, payload));
                }
                Some(_) => {}
                None => {
                    if hw.now_us().saturating_sub(start) >= timeout_us as u64 {
                        return Err(Error::GucProtocol {
                            request: action,
                            reply: 0,
                        });
                    }
                    hw.delay_us(100);
                }
            }
        }
    }

    /// The events that arrived while others were waited for (see [`Ct::wait_event`]).
    pub fn take_backlog(&mut self) -> Vec<(u32, Vec<u32>)> {
        core::mem::take(&mut self.backlog)
    }

    /// Takes the next message out of the G2H ring; `None` when it is empty.
    pub fn recv(&mut self, mem: &mut impl Shared) -> Result<Option<G2h>> {
        if mem.read(G2H_DESC + DESC_STATUS) != 0 {
            return Err(Error::Ctb("G2H ring reports an error"));
        }
        let tail = mem.read(G2H_DESC + DESC_TAIL);
        if tail >= G2H_WORDS {
            return Err(Error::Ctb("G2H tail out of range"));
        }
        let avail = tail.wrapping_sub(self.head) & (G2H_WORDS - 1);
        if avail == 0 {
            return Ok(None);
        }
        let word = |mem: &mut dyn Shared, i: u32| mem.read(G2H_CMDS + (i % G2H_WORDS) as usize);
        let header = word(mem, self.head);
        let len = header & 0xff;
        if len == 0 || len + 1 > avail {
            return Err(Error::Ctb("G2H message cut off"));
        }
        let hxg0 = word(mem, self.head + 1);
        let payload: Vec<u32> = (2..=len).map(|i| word(mem, self.head + i)).collect();
        self.head = (self.head + len + 1) % G2H_WORDS;
        mem.write(G2H_DESC + DESC_HEAD, self.head);
        if hxg0 & HXG_ORIGIN_GUC == 0 {
            return Err(Error::Ctb("G2H message not from the GuC"));
        }
        let fence = (header >> 16) as u16;
        Ok(Some(match (hxg0 >> HXG_TYPE_SHIFT) & 0x7 {
            7 => G2h::Response {
                fence,
                data0: hxg0 & HXG_DATA,
                payload,
            },
            6 => G2h::Failure {
                fence,
                error: hxg0 & 0xffff,
                hint: (hxg0 >> 16) & 0xfff,
            },
            5 => G2h::Retry { fence },
            3 => G2h::Busy { fence },
            1 => G2h::Event {
                action: hxg0 & 0xffff,
                payload,
            },
            _ => return Err(Error::Ctb("unknown G2H message type")),
        }))
    }

    /// Sends a request and polls for its response (at most `timeout_us`). Returns the response's
    /// data bits and the events that arrived meanwhile.
    pub fn request(
        &mut self,
        hw: &mut impl Hw,
        mem: &mut impl Shared,
        action: &[u32],
        timeout_us: u32,
    ) -> Result<(u32, Vec<G2h>)> {
        let mut events = Vec::new();
        let mut fence = self.send(hw, mem, action)?;
        let mut retries = 0;
        let start = hw.now_us();
        loop {
            match self.recv(mem)? {
                Some(G2h::Response {
                    fence: f, data0, ..
                }) if f == fence => return Ok((data0, events)),
                Some(G2h::Failure {
                    fence: f,
                    error,
                    hint,
                }) if f == fence => {
                    return Err(Error::GucRequest {
                        request: action[0],
                        error,
                        hint,
                    })
                }
                Some(G2h::Retry { fence: f }) if f == fence && retries < MAX_RETRIES => {
                    retries += 1;
                    hw.delay_us(1000 << retries);
                    fence = self.send(hw, mem, action)?;
                }
                Some(G2h::Busy { .. }) => {}
                Some(other) => events.push(other),
                None => {
                    if hw.now_us().saturating_sub(start) >= timeout_us as u64 {
                        return Err(Error::GucProtocol {
                            request: action[0],
                            reply: 0,
                        });
                    }
                    hw.delay_us(100);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
