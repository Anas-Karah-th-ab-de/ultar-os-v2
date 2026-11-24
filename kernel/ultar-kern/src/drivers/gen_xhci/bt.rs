//! USB Bluetooth controllers of the xHCI driver (Bluetooth Core Vol 4 Part B, "USB Transport
//! Layer"): every interface of class `E0/01/01` (wireless controller, RF, Bluetooth primary
//! controller) is bound here and offered to the Bluetooth service as a raw HCI node
//! `/dev/ultar/hciN` (`svcdev/hci.rs` is the node, this file is the transport).
//!
//! * HCI commands go out as class requests on endpoint 0 (`bmRequestType` 0x20, `bRequest` 0);
//! * HCI events come from the interrupt IN endpoint, ACL data from bulk IN, ACL data goes out on
//!   bulk OUT. SCO (isochronous, interface 1) is not offered.
//!
//! Both IN endpoints are read as byte streams and cut into packets by their headers (an event:
//! 2 + length octets, ACL: 4 + length octets), so packets split over several USB transfers and
//! several packets in one transfer both come out right. Every finished packet becomes one record
//! with the H4 type octet in front (`0x04` event, `0x02` ACL) – what the Linux HCI user channel
//! hands out. Records are kept only while the node is open, at most [`MAX_RECORDS`] /
//! [`MAX_BYTES`]; more are dropped and counted.
//!
//! The event endpoint is read **one USB packet per transfer** (like Linux `btusb`), with several
//! transfers queued at once: a controller does not end an event whose length is a multiple of the
//! endpoint's packet size with a zero-length packet, so a longer transfer would hold such an event
//! back until the next one arrives (during a scan: advertising reports and inquiry results that
//! never show up). The driver has no interrupts and is pumped every few milliseconds, so the
//! queue (up to [`EV_TDS`] packets) keeps a 257-octet inquiry result from needing 17 rounds.
//! The ACL endpoint has [`ACL_TDS`] transfers of 1 KiB queued.

use super::{config_ep, w32, Desc, Hc, TRB_NORMAL};
use crate::mem::alloc_pages;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;

/// H4 type octets.
pub const H4_COMMAND: u8 = 1;
/// ACL data.
pub const H4_ACL: u8 = 2;
/// Event.
pub const H4_EVENT: u8 = 4;
/// Most controllers at once (nodes `hci0`..`hci3`).
pub const MAX_CONTROLLERS: usize = 4;
/// Records kept for the reader at most.
pub const MAX_RECORDS: usize = 256;
/// Bytes kept for the reader at most.
pub const MAX_BYTES: usize = 128 * 1024;
/// Largest ACL payload accepted in either direction (the controller's buffer is smaller; this is
/// the bound of a hostile or broken device).
pub const MAX_ACL_DATA: usize = 1021;
/// Event transfers queued at most (each one USB packet).
pub const EV_TDS: usize = 32;
/// ACL transfers queued.
pub const ACL_TDS: usize = 4;
/// Bytes asked for per ACL transfer.
const ACL_TD: u32 = 1024;

/// An IN endpoint read through several queued transfers, each into its own part of one DMA page.
#[derive(Clone, Copy)]
struct InEp {
    dci: u32,
    /// DMA page.
    buf: u64,
    /// Bytes per transfer.
    td: u32,
    /// Transfers queued.
    n: usize,
    /// TRB address of each queued transfer (to find the part a completion belongs to).
    trb: [u64; EV_TDS],
    /// The part the next completion is expected for (in ring order).
    next: usize,
}

impl InEp {
    fn new(dci: u32, buf: u64, td: u32, n: usize) -> Self {
        Self { dci, buf, td, n: n.clamp(1, EV_TDS), trb: [0; EV_TDS], next: 0 }
    }

    /// Queue transfer `i` (into its part of the page).
    fn arm(&mut self, hc: &mut Hc, slot: u32, i: usize) {
        let ring = hc.ring(slot, self.dci);
        self.trb[i] = ring.push(self.buf + i as u64 * self.td as u64, self.td, (TRB_NORMAL << 10) | (1 << 5) | (1 << 2));
    }

    /// Queue every transfer and ring the doorbell (after attach and after a reset).
    fn arm_all(&mut self, hc: &mut Hc, slot: u32) {
        self.next = 0;
        for i in 0..self.n {
            self.arm(hc, slot, i);
        }
        w32(hc.db + slot as u64 * 4, self.dci);
    }

    /// The part a completion of TRB `trb` belongs to.
    fn part(&self, trb: u64) -> usize {
        self.trb[..self.n].iter().position(|&t| t == trb).unwrap_or(self.next)
    }
}

/// One controller.
struct Bt {
    /// Serial number (never reused; descriptors hold it).
    id: u64,
    /// Node number `N` of `hciN`.
    index: u8,
    hc: usize,
    slot: u32,
    /// Interrupt IN (events).
    ev: InEp,
    /// Bulk IN (ACL).
    acl: InEp,
    /// Bulk OUT and the page of the outgoing packet.
    out: (u32, u64),
    ev_asm: Vec<u8>,
    acl_asm: Vec<u8>,
    queue: VecDeque<Vec<u8>>,
    queued_bytes: usize,
    dropped: u64,
    open: bool,
    /// IN endpoints that stopped with an error and wait for a reset (bit 0 events, bit 1 ACL).
    halted: u8,
}

static BTS: Mutex<Vec<Bt>> = Mutex::new(Vec::new());
static NEXT_ID: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

/// Whether the interface is a Bluetooth primary controller.
pub(super) fn is_bt(d: &Desc) -> bool {
    (d.class, d.sub, d.proto) == (0xE0, 0x01, 0x01)
}

/// Whether controller `hc` has a Bluetooth controller (its events must be pumped).
pub(super) fn any(hc: usize) -> bool {
    BTS.lock().iter().any(|b| b.hc == hc)
}

/// Bind interface `d` of the device in `slot`: endpoints, buffers, first IN transfers. Returns the
/// node number.
pub(super) fn attach(hc: &mut Hc, slot: u32, ctx: (u64, u64, u32), d: &Desc) -> Result<u8, &'static str> {
    let find = |dir_in: bool, typ: u8| d.eps.iter().find(|e| (e.0 & 0x80 != 0) == dir_in && e.1 & 3 == typ).copied();
    let (Some(ev), Some(bin), Some(bout)) = (find(true, 3), find(true, 2), find(false, 2)) else { return Err("Bluetooth: endpoints missing") };
    let index = {
        let list = BTS.lock();
        if list.len() >= MAX_CONTROLLERS {
            return Err("Bluetooth: too many controllers");
        }
        (0..MAX_CONTROLLERS as u8).find(|i| !list.iter().any(|b| b.index == *i)).ok_or("Bluetooth: no node")?
    };
    let (ev_dci, in_dci, out_dci) = ((ev.0 as u32 & 0xf) * 2 + 1, (bin.0 as u32 & 0xf) * 2 + 1, (bout.0 as u32 & 0xf) * 2);
    config_ep(hc, slot, ctx.0, ctx.1, ctx.2, &[(ev_dci, 7, ev.2 as u32, ev.3), (in_dci, 6, bin.2 as u32, 0), (out_dci, 2, bout.2 as u32, 0)]).map_err(|_| "Bluetooth: endpoints not configured")?;
    let pages = [alloc_pages(1), alloc_pages(1), alloc_pages(1)];
    let [Some(ev_buf), Some(in_buf), Some(out_buf)] = pages else {
        pages.into_iter().flatten().for_each(crate::mem::free_page);
        return Err("Bluetooth: no memory");
    };
    let id = NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    // one USB packet per event transfer (wMaxPacketSize without the high-speed multiplier bits)
    let ev_mps = (ev.2 & 0x7ff).clamp(8, 1024) as u32;
    let mut bt = Bt {
        id,
        index,
        hc: hc.index,
        slot,
        ev: InEp::new(ev_dci, ev_buf, ev_mps, (4096 / ev_mps as usize).min(EV_TDS)),
        acl: InEp::new(in_dci, in_buf, ACL_TD, ACL_TDS),
        out: (out_dci, out_buf),
        ev_asm: Vec::new(),
        acl_asm: Vec::new(),
        queue: VecDeque::new(),
        queued_bytes: 0,
        dropped: 0,
        open: false,
        halted: 0,
    };
    if !crate::heap::kernel(|| BTS.lock().try_reserve(1).is_ok()) {
        [ev_buf, in_buf, out_buf].into_iter().for_each(crate::mem::free_page);
        return Err("Bluetooth: no memory");
    }
    bt.ev.arm_all(hc, slot);
    bt.acl.arm_all(hc, slot);
    BTS.lock().push(bt);
    crate::kprintln!("[bt] hci{index}: USB Bluetooth controller (slot {slot}, events {ev_mps} x {})", (4096 / ev_mps as usize).min(EV_TDS));
    Ok(index)
}

/// Cut finished packets off the front of a stream (`hdr`: header length, `len_at`/`wide`: where
/// the length field is and whether it has two octets, `max`: largest payload).
fn cut(asm: &mut Vec<u8>, hdr: usize, len_at: usize, wide: bool, max: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while asm.len() >= hdr {
        let len = if wide { u16::from_le_bytes([asm[len_at], asm[len_at + 1]]) as usize } else { asm[len_at] as usize };
        if len > max {
            // a broken or hostile device: nothing after this can be trusted to line up
            asm.clear();
            break;
        }
        if asm.len() < hdr + len {
            break;
        }
        let rest = asm.split_off(hdr + len);
        out.push(core::mem::replace(asm, rest));
    }
    out
}

impl Bt {
    fn ev_dci(&self) -> u32 {
        self.ev.dci
    }
    fn in_dci(&self) -> u32 {
        self.acl.dci
    }

    /// A finished packet of type `h4` for the reader (dropped while nobody reads or when full).
    fn keep(&mut self, h4: u8, pkt: Vec<u8>) {
        if !self.open {
            return;
        }
        if self.queue.len() >= MAX_RECORDS || self.queued_bytes + pkt.len() + 1 > MAX_BYTES {
            self.dropped += 1;
            return;
        }
        let mut rec = Vec::new();
        if crate::heap::kernel(|| rec.try_reserve_exact(pkt.len() + 1).is_err() || self.queue.try_reserve(1).is_err()) {
            self.dropped += 1;
            return;
        }
        rec.push(h4);
        rec.extend_from_slice(&pkt);
        self.queued_bytes += rec.len();
        self.queue.push_back(rec);
    }

    /// `n` octets arrived on endpoint `dci` in part `part` of its DMA page.
    fn arrived(&mut self, dci: u32, part: usize, n: usize) {
        let ev = dci == self.ev_dci();
        let (ep, asm) = if ev { (self.ev, &mut self.ev_asm) } else { (self.acl, &mut self.acl_asm) };
        // SAFETY: the part is inside this controller's DMA page; the controller wrote `n` <= td
        // octets into it and the transfer is not queued again before the copy below.
        let data = unsafe { core::slice::from_raw_parts((ep.buf + part as u64 * ep.td as u64) as *const u8, n.min(ep.td as usize)) };
        if crate::heap::kernel(|| asm.try_reserve(data.len()).is_err()) {
            asm.clear();
            self.dropped += 1;
            return;
        }
        asm.extend_from_slice(data);
        let pkts = if ev { cut(asm, 2, 1, false, 255) } else { cut(asm, 4, 2, true, MAX_ACL_DATA) };
        for p in pkts {
            self.keep(if ev { H4_EVENT } else { H4_ACL }, p);
        }
    }
}

/// A transfer of endpoint `(slot, dci)` completed (`trb`: its TRB, `code`, `left` octets not
/// transferred): if it is a Bluetooth IN endpoint, take the data, queue the transfer again and
/// return true.
pub(super) fn report(hc: &mut Hc, (slot, dci): (u32, u32), trb: u64, code: u32, left: u32) -> bool {
    {
        let mut list = BTS.lock();
        let Some(b) = list.iter_mut().find(|b| b.hc == hc.index && b.slot == slot && (b.ev_dci() == dci || b.in_dci() == dci)) else { return false };
        let ev = dci == b.ev_dci();
        let (part, td) = {
            let ep = if ev { &mut b.ev } else { &mut b.acl };
            let part = ep.part(trb);
            ep.next = (part + 1) % ep.n;
            (part, ep.td)
        };
        if !matches!(code, 1 | 13) {
            // halted (stall, babble, transaction error): reset later in `recover`, not here – a
            // reset waits for a command completion, which runs this function again
            b.halted |= if ev { 1 } else { 2 };
            return true;
        }
        b.arrived(dci, part, td.saturating_sub(left) as usize);
        let ep = if ev { &mut b.ev } else { &mut b.acl };
        ep.arm(hc, slot, part);
    }
    w32(hc.db + slot as u64 * 4, dci);
    crate::socket::bump();
    true
}

/// The device in `slot` left: its controllers go away (open descriptors read `ENODEV`).
pub(super) fn remove(hc: &mut Hc, slot: u32) {
    let mut list = BTS.lock();
    for b in list.iter().filter(|b| b.hc == hc.index && b.slot == slot) {
        crate::kprintln!("[bt] hci{}: removed", b.index);
        [b.ev.buf, b.acl.buf, b.out.1].into_iter().for_each(crate::mem::free_page);
    }
    list.retain(|b| !(b.hc == hc.index && b.slot == slot));
}

// ------------------------------------------------------------------ for the node (svcdev/hci.rs)

/// Errors of the node operations.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BtError {
    /// No such controller (it left).
    Gone,
    /// Already open.
    Busy,
    /// Not a valid packet.
    Invalid,
    /// The controller did not take it.
    Io,
}

/// Node numbers of the controllers that are present, ascending.
pub fn nodes() -> Vec<u8> {
    let mut v: Vec<u8> = BTS.lock().iter().map(|b| b.index).collect();
    v.sort_unstable();
    v
}

/// Open node `index` (one reader at a time): returns the controller's serial number.
pub fn open(index: u8) -> Result<u64, BtError> {
    let mut list = BTS.lock();
    let b = list.iter_mut().find(|b| b.index == index).ok_or(BtError::Gone)?;
    if b.open {
        return Err(BtError::Busy);
    }
    b.open = true;
    b.queue.clear();
    b.queued_bytes = 0;
    Ok(b.id)
}

/// The descriptor of controller `id` was closed.
pub fn close(id: u64) {
    if let Some(b) = BTS.lock().iter_mut().find(|b| b.id == id) {
        if b.dropped > 0 {
            crate::kprintln!("[bt] hci{}: {} records dropped while open", b.index, b.dropped);
        }
        b.open = false;
        b.dropped = 0;
        b.queue.clear();
        b.queued_bytes = 0;
    }
}

/// Process what the host controllers have (the driver works without interrupts). Quick: no
/// command is waited for (safe where the caller holds other locks).
pub fn poll() {
    let Some(mut g) = super::HC.try_lock() else { return };
    for hc in g.iter_mut().filter(|h| any(h.index)) {
        hc.pump();
    }
}

/// Reset IN endpoints that stopped with an error and read on (waits for the host controller:
/// call it only from a plain system call path).
pub fn recover() {
    let Some(mut g) = super::HC.try_lock() else { return };
    for hc in g.iter_mut().filter(|h| any(h.index)) {
        let halted: Vec<(u32, u32)> = {
            let mut list = BTS.lock();
            let mut v = Vec::new();
            for b in list.iter_mut().filter(|b| b.hc == hc.index && b.halted != 0) {
                for (bit, dci) in [(1u8, b.ev.dci), (2, b.acl.dci)] {
                    if b.halted & bit != 0 {
                        v.push((b.slot, dci));
                    }
                }
                b.halted = 0;
            }
            v
        };
        for (slot, dci) in halted {
            // the reset drops every transfer still queued; queue them all again
            hc.reset_ep(slot, dci);
            let ep = BTS.lock().iter().find(|b| b.hc == hc.index && b.slot == slot).map(|b| if b.ev.dci == dci { b.ev } else { b.acl });
            if let Some(mut ep) = ep {
                ep.arm_all(hc, slot);
                if let Some(b) = BTS.lock().iter_mut().find(|b| b.hc == hc.index && b.slot == slot) {
                    if b.ev.dci == dci {
                        b.ev = ep;
                    } else {
                        b.acl = ep;
                    }
                }
            }
        }
    }
}

/// The next record of controller `id`; `Ok(None)`: nothing yet.
pub fn take(id: u64) -> Result<Option<Vec<u8>>, BtError> {
    let mut list = BTS.lock();
    let b = list.iter_mut().find(|b| b.id == id).ok_or(BtError::Gone)?;
    let r = b.queue.pop_front();
    if let Some(r) = &r {
        b.queued_bytes -= r.len();
    }
    Ok(r)
}

/// Put a record back in front (the reader's buffer was too small).
pub fn untake(id: u64, rec: Vec<u8>) {
    if let Some(b) = BTS.lock().iter_mut().find(|b| b.id == id) {
        b.queued_bytes += rec.len();
        b.queue.push_front(rec);
    }
}

/// Whether a read would not wait (a record is there, or the controller left).
pub fn ready(id: u64) -> bool {
    BTS.lock().iter().find(|b| b.id == id).is_none_or(|b| !b.queue.is_empty())
}

/// Check one H4 record from the service: a command (3 + length octets) or ACL data (4 + length
/// octets, at most [`MAX_ACL_DATA`]); SCO and anything else is refused.
pub fn valid(rec: &[u8]) -> bool {
    match rec.split_first() {
        Some((&H4_COMMAND, p)) => p.len() >= 3 && p.len() == 3 + p[2] as usize,
        Some((&H4_ACL, p)) => p.len() >= 4 && {
            let n = u16::from_le_bytes([p[2], p[3]]) as usize;
            n <= MAX_ACL_DATA && p.len() == 4 + n
        },
        _ => false,
    }
}

/// Send one H4 record (checked with [`valid`]) to controller `id`.
pub fn send(id: u64, rec: &[u8]) -> Result<(), BtError> {
    if !valid(rec) {
        return Err(BtError::Invalid);
    }
    let mut g = super::HC.lock();
    let (hci, slot, out_dci, buf) = {
        let list = BTS.lock();
        let b = list.iter().find(|b| b.id == id).ok_or(BtError::Gone)?;
        (b.hc, b.slot, b.out.0, b.out.1)
    };
    let hc = g.iter_mut().find(|h| h.index == hci).ok_or(BtError::Gone)?;
    let pkt = &rec[1..];
    // SAFETY: `buf` is this controller's DMA page; `pkt` is at most 4 + MAX_ACL_DATA < 4096 octets.
    unsafe { core::ptr::copy_nonoverlapping(pkt.as_ptr(), buf as *mut u8, pkt.len()) };
    let done = if rec[0] == H4_COMMAND {
        hc.control(slot, 0x20, 0, 0, 0, pkt.len() as u16, buf)
    } else {
        hc.transfer(slot, out_dci, buf, pkt.len(), 2000)
    };
    match done {
        Some(n) if n == pkt.len() => Ok(()),
        _ => Err(BtError::Io),
    }
}

