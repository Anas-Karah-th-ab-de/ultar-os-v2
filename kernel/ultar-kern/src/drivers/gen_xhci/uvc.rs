//! USB cameras of the xHCI driver (USB Video Class 1.1/1.5, payload format MJPEG): the first
//! camera is offered to the call service as `/dev/ultar/kamera` (`svcdev/kamera.rs` is the node,
//! this file is the transport).
//!
//! * At attach only the descriptors are read: the video streaming interface, its MJPEG format and
//!   the frame size used (exactly 320×240, else the smallest one at least as large, else the
//!   largest), and the alternate settings with an isochronous IN endpoint.
//! * Opening the node negotiates the stream (VS_PROBE / VS_COMMIT, 15 pictures a second), picks the
//!   smallest alternate setting that carries the camera's payload, configures the endpoint and
//!   queues [`TDS`] isochronous transfers of one service interval each ("start as soon as
//!   possible"). Closing selects alternate setting 0 again and drops the endpoint.
//! * Every completed transfer carries one payload: a UVC header (frame id, end of frame, error)
//!   and a piece of the JPEG. Pieces are put together until the end of the frame (or the frame id
//!   changes); a frame with a lost piece or an error bit is dropped. Only the newest finished
//!   picture is kept for the reader – a slow reader skips pictures, it never sees old ones.
//!
//! The driver has no interrupts: the host controller is pumped by the node's reads and the
//! system's tick (`poll`), so [`TDS`] transfers cover more than a tick.

use super::{config_ep, ictx, r32, w32, Hc, TRB_CONFIG_EP};
use crate::mem::alloc_pages;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

/// Isochronous TRB.
const TRB_ISOCH: u32 = 5;
/// Transfers queued while streaming (one service interval each).
pub const TDS: usize = 192;
/// Largest picture accepted (bytes); a larger one is a broken or hostile device.
pub const MAX_FRAME: usize = 1 << 20;
/// Width and height asked for.
pub const WANT: (u16, u16) = (320, 240);
/// 15 pictures a second, in 100-ns units.
const INTERVAL_15FPS: u32 = 666_666;
/// Class requests of the video class.
const SET_CUR: u8 = 0x01;
const GET_CUR: u8 = 0x81;
const VS_PROBE: u16 = 1;
const VS_COMMIT: u16 = 2;

/// An alternate setting of the streaming interface with its isochronous IN endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alt {
    /// `bAlternateSetting`.
    pub alt: u8,
    /// Endpoint address.
    pub ep: u8,
    /// `wMaxPacketSize` as in the descriptor (with the high-bandwidth bits).
    pub mps: u16,
    /// `bInterval`.
    pub interval: u8,
}

impl Alt {
    /// Bytes per service interval (packet size × transactions).
    pub fn payload(&self) -> usize {
        (self.mps & 0x7ff) as usize * (((self.mps >> 11) & 3) as usize + 1)
    }
}

/// What the descriptors say about a camera.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Video control interface (`bInterfaceNumber`).
    pub vc: u8,
    /// Video streaming interface.
    pub vs: u8,
    /// `bcdUVC` of the class header.
    pub uvc: u16,
    /// MJPEG `bFormatIndex`.
    pub format: u8,
    /// Chosen `bFrameIndex`, its width and height.
    pub frame: (u8, u16, u16),
    /// Alternate settings with an isochronous IN endpoint, smallest payload first.
    pub alts: Vec<Alt>,
}

/// Reads the camera layout from a configuration descriptor (`None`: no MJPEG camera there).
pub fn layout(cd: &[u8]) -> Option<Layout> {
    let mut l = Layout { vc: 0xff, vs: 0xff, uvc: 0x0100, format: 0, frame: (0, 0, 0), alts: Vec::new() };
    let mut frames: Vec<(u8, u16, u16)> = Vec::new();
    // (interface, alternate setting, class, subclass) of the descriptors being read
    let mut cur = (0u8, 0u8, 0u8, 0u8);
    let mut mjpeg = false;
    let mut o = 0usize;
    while o + 2 <= cd.len() {
        let n = cd[o] as usize;
        if n < 2 || o + n > cd.len() {
            break;
        }
        let d = &cd[o..o + n];
        match d[1] {
            4 if n >= 9 => {
                cur = (d[2], d[3], d[5], d[6]);
                if (cur.2, cur.3) == (0x0E, 1) && l.vc == 0xff {
                    l.vc = cur.0;
                }
                if (cur.2, cur.3) == (0x0E, 2) && l.vs == 0xff {
                    l.vs = cur.0;
                }
            }
            0x24 if cur.2 == 0x0E => class_desc(&mut l, &mut frames, &mut mjpeg, cur, d),
            5 if n >= 7 && (cur.2, cur.3) == (0x0E, 2) && cur.0 == l.vs => {
                if d[2] & 0x80 != 0 && d[3] & 3 == 1 && cur.1 != 0 {
                    l.alts.push(Alt { alt: cur.1, ep: d[2], mps: u16::from_le_bytes([d[4], d[5]]), interval: d[6] });
                }
            }
            _ => {}
        }
        o += n;
    }
    if l.vc == 0xff || l.vs == 0xff || l.format == 0 || l.alts.is_empty() {
        return None;
    }
    l.frame = choose(&frames)?;
    l.alts.sort_by_key(|a| a.payload());
    Some(l)
}

/// A class-specific interface descriptor (`d`) of interface `cur`.
fn class_desc(l: &mut Layout, frames: &mut Vec<(u8, u16, u16)>, mjpeg: &mut bool, cur: (u8, u8, u8, u8), d: &[u8]) {
    match (cur.3, d.get(2).copied()) {
        // VC_HEADER: bcdUVC
        (1, Some(1)) if d.len() >= 5 => l.uvc = u16::from_le_bytes([d[3], d[4]]),
        // VS_FORMAT_MJPEG: the first one is used; other formats end its frames
        (2, Some(6)) if d.len() >= 4 => {
            *mjpeg = l.format == 0;
            if *mjpeg {
                l.format = d[3];
            }
        }
        (2, Some(0x04 | 0x10 | 0x0C)) => *mjpeg = false,
        // VS_FRAME_MJPEG
        (2, Some(7)) if *mjpeg && d.len() >= 9 => {
            frames.push((d[3], u16::from_le_bytes([d[5], d[6]]), u16::from_le_bytes([d[7], d[8]])));
        }
        _ => {}
    }
}

/// The frame used: exactly [`WANT`], else the smallest at least as large, else the largest.
pub fn choose(frames: &[(u8, u16, u16)]) -> Option<(u8, u16, u16)> {
    let area = |f: &(u8, u16, u16)| u32::from(f.1) * u32::from(f.2);
    if let Some(f) = frames.iter().find(|f| (f.1, f.2) == WANT) {
        return Some(*f);
    }
    let big = frames.iter().filter(|f| f.1 >= WANT.0 && f.2 >= WANT.1).min_by_key(|f| area(f));
    big.or_else(|| frames.iter().max_by_key(|f| area(f))).copied()
}

/// Length of the probe/commit structure for a class version.
pub fn probe_len(uvc: u16) -> usize {
    match uvc {
        0..=0x010f => 26,
        0x0110..=0x014f => 34,
        _ => 48,
    }
}

/// The probe structure asking for `format`/`frame` at 15 pictures a second.
pub fn probe(len: usize, format: u8, frame: u8) -> Vec<u8> {
    let mut p = alloc::vec![0u8; len];
    p[0] = 1; // bmHint: keep the frame interval
    p[2] = format;
    p[3] = frame;
    p[4..8].copy_from_slice(&INTERVAL_15FPS.to_le_bytes());
    p
}

/// `dwMaxPayloadTransferSize` of a probe answer.
pub fn max_payload(p: &[u8]) -> usize {
    p.get(22..26).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

/// The alternate settings to try for a payload, best first: those that carry it from the
/// largest down (a large payload per interval finishes a picture in few intervals, so a missed
/// interval costs few pictures), else the largest there is.
pub fn alt_order(alts: &[Alt], payload: usize) -> Vec<Alt> {
    let mut v: Vec<Alt> = alts.iter().filter(|a| a.payload() >= payload).rev().copied().collect();
    if v.is_empty() {
        v.extend(alts.last().copied());
    }
    v
}

/// Configure the endpoint of `alt` and select it; false (endpoint dropped again): refused, e.g.
/// not enough bandwidth on the bus.
fn select(hc: &mut Hc, slot: u32, ctx: (u64, u64, u32), vs: u8, alt: Alt) -> bool {
    let dci = (alt.ep as u32 & 0xf) * 2 + 1;
    if config_ep(hc, slot, ctx.0, ctx.1, ctx.2, &[(dci, 5, alt.mps as u32, alt.interval)]).is_err() {
        if let Some(i) = hc.rings.iter().position(|r| r.0 == slot && r.1 == dci) {
            let (_, _, ring) = hc.rings.remove(i);
            crate::mem::free_page(ring.base);
        }
        return false;
    }
    if hc.control(slot, 0x01, 11, alt.alt as u16, vs as u16, 0, 0).is_none() {
        drop_ep(hc, slot, ctx, dci);
        return false;
    }
    true
}

/// Puts pictures together from payloads (UVC 1.1 §2.4.3.3).
#[derive(Default)]
pub struct Frames {
    asm: Vec<u8>,
    fid: Option<bool>,
    bad: bool,
    /// The newest finished picture and its number.
    pub ready: Option<(u64, Vec<u8>)>,
    seq: u64,
    /// Payload bytes, ends of frames and dropped frames seen (reported when the stream stops).
    pub counts: [u64; 3],
}

impl Frames {
    /// One payload (`ok`: the transfer completed without error).
    pub fn put(&mut self, payload: &[u8], ok: bool) {
        if !ok {
            self.bad = true;
            return;
        }
        let Some(&hlen) = payload.first() else { return };
        self.counts[0] += payload.len() as u64;
        let hlen = hlen as usize;
        if hlen < 2 || hlen > payload.len() {
            return;
        }
        let info = payload[1];
        let fid = info & 1 != 0;
        if self.fid.is_some_and(|f| f != fid) && !self.asm.is_empty() {
            self.finish();
        }
        self.fid = Some(fid);
        if info & 0x40 != 0 {
            self.bad = true;
        }
        let data = &payload[hlen..];
        if self.asm.len() + data.len() > MAX_FRAME || crate::heap::kernel(|| self.asm.try_reserve(data.len()).is_err()) {
            self.bad = true;
        } else {
            self.asm.extend_from_slice(data);
        }
        if info & 2 != 0 {
            self.finish();
        }
    }

    /// End of a picture: keep it when whole and a JPEG.
    fn finish(&mut self) {
        let pic = core::mem::take(&mut self.asm);
        let whole = !self.bad && pic.len() > 4 && pic[..2] == [0xFF, 0xD8];
        self.counts[1] += 1;
        self.counts[2] += u64::from(!whole);
        self.bad = false;
        if whole {
            self.seq += 1;
            self.ready = Some((self.seq, pic));
        }
    }

    /// Start over (stream stopped or restarted; the counts stay).
    pub fn reset(&mut self) {
        self.asm = Vec::new();
        self.fid = None;
        self.bad = false;
        self.ready = None;
    }
}

/// The streaming state of a camera.
struct Stream {
    alt: Alt,
    dci: u32,
    /// Bytes per transfer.
    td: u32,
    /// TRB address of each queued transfer.
    trb: [u64; TDS],
}

/// One camera.
struct Cam {
    /// Serial number (descriptors hold it; never reused).
    id: u64,
    hc: usize,
    slot: u32,
    /// Input and output context of the slot (for configuring the endpoint), speed.
    ctx: (u64, u64, u32),
    layout: Layout,
    /// One DMA page per transfer (taken at the first open, kept).
    pages: Vec<u64>,
    stream: Option<Stream>,
    frames: Frames,
    open: bool,
    /// The number of the last picture the reader took.
    taken: u64,
    /// Transfers while streaming: completed, with an error, without a transfer of ours; the
    /// last completion code and the last error code (reported when the stream stops).
    seen: [u32; 5],
}

static CAMS: Mutex<Vec<Cam>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Some camera streams (the tick pumps the host controllers).
static STREAMING: AtomicBool = AtomicBool::new(false);

/// Note the camera of the device in `slot` (`cd`: its configuration descriptor). Returns the
/// chosen picture size.
pub(super) fn attach(hc: &mut Hc, slot: u32, ctx: (u64, u64, u32), cd: &[u8]) -> Option<(u16, u16)> {
    let layout = layout(cd)?;
    let size = (layout.frame.1, layout.frame.2);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let cam = Cam { id, hc: hc.index, slot, ctx, layout, pages: Vec::new(), stream: None, frames: Frames::default(), open: false, taken: 0, seen: [0; 5] };
    if !crate::heap::kernel(|| CAMS.lock().try_reserve(1).is_ok()) {
        return None;
    }
    CAMS.lock().push(cam);
    Some(size)
}

/// The device in `slot` left: its camera goes away (an open descriptor reads `ENODEV`).
pub(super) fn remove(hc: &mut Hc, slot: u32) {
    let mut list = CAMS.lock();
    for c in list.iter().filter(|c| c.hc == hc.index && c.slot == slot) {
        crate::kprintln!("[uvc] camera removed");
        c.pages.iter().copied().for_each(crate::mem::free_page);
    }
    list.retain(|c| !(c.hc == hc.index && c.slot == slot));
    STREAMING.store(list.iter().any(|c| c.stream.is_some()), Ordering::Relaxed);
}

/// Queue transfer `i` of a stream.
fn arm(hc: &mut Hc, slot: u32, s: &mut Stream, page: u64, i: usize) {
    let burst = ((s.alt.mps >> 11) & 3) as u32;
    let ring = hc.ring(slot, s.dci);
    // IOC, ISP, TLBPC = transactions - 1, start as soon as possible
    s.trb[i] = ring.push(page, s.td, (TRB_ISOCH << 10) | (1 << 5) | (1 << 2) | (burst << 16) | (1 << 31));
}

/// A transfer of endpoint `(slot, dci)` completed: if it is a camera's, take the payload, queue
/// the transfer again and return true.
pub(super) fn report(hc: &mut Hc, (slot, dci): (u32, u32), trb: u64, code: u32, left: u32) -> bool {
    {
        let mut list = CAMS.lock();
        let Some(c) = list.iter_mut().find(|c| c.hc == hc.index && c.slot == slot && c.stream.as_ref().is_some_and(|s| s.dci == dci)) else { return false };
        let Some(s) = c.stream.as_mut() else { return true };
        c.seen[3] = code;
        let Some(i) = s.trb.iter().position(|&t| t == trb && t != 0) else {
            c.seen[2] += 1;
            // ring underrun (every transfer done) or an event without a transfer: queue them all again
            if code == 14 {
                c.frames.put(&[], false);
                for i in 0..TDS {
                    arm(hc, slot, s, c.pages[i], i);
                }
                w32(hc.db + slot as u64 * 4, dci);
            }
            return true;
        };
        let ok = matches!(code, 1 | 13);
        c.seen[if ok { 0 } else { 1 }] += 1;
        if !ok {
            c.seen[4] = code;
        }
        let n = s.td.saturating_sub(left).min(s.td) as usize;
        // SAFETY: the page belongs to this camera; the controller wrote at most `td` bytes into
        // it and the transfer is queued again only after the copy.
        let payload = unsafe { core::slice::from_raw_parts(c.pages[i] as *const u8, if ok { n } else { 0 }) };
        c.frames.put(payload, ok);
        arm(hc, slot, s, c.pages[i], i);
    }
    w32(hc.db + slot as u64 * 4, dci);
    crate::socket::bump();
    true
}

// ------------------------------------------------------------------ for the node (svcdev/kamera.rs)

/// Errors of the node operations.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CamError {
    /// No camera (it left).
    Gone,
    /// Already open.
    Busy,
    /// The camera refused the stream or there is no memory.
    Io,
}

/// Whether a camera is there.
pub fn present() -> bool {
    !CAMS.lock().is_empty()
}

/// Opens the first camera and starts its stream: returns its serial number.
pub fn open() -> Result<u64, CamError> {
    let mut g = super::HC.lock();
    let (id, hci) = {
        let mut list = CAMS.lock();
        let c = list.first_mut().ok_or(CamError::Gone)?;
        if c.open {
            return Err(CamError::Busy);
        }
        c.open = true;
        (c.id, c.hc)
    };
    let started = g.iter_mut().find(|h| h.index == hci).ok_or(CamError::Gone).and_then(|hc| start(hc, id));
    if started.is_err() {
        if let Some(c) = CAMS.lock().iter_mut().find(|c| c.id == id) {
            c.open = false;
        }
    }
    started.map(|_| id)
}

/// The camera's state copied out (the list lock is not held across control transfers).
fn snapshot(id: u64) -> Option<(u32, (u64, u64, u32), Layout, usize)> {
    CAMS.lock().iter().find(|c| c.id == id).map(|c| (c.slot, c.ctx, c.layout.clone(), c.pages.len()))
}

/// Negotiate the stream, configure the endpoint and queue the transfers.
fn start(hc: &mut Hc, id: u64) -> Result<(), CamError> {
    let (slot, ctx, l, have) = snapshot(id).ok_or(CamError::Gone)?;
    let payload = negotiate(hc, slot, &l).ok_or(CamError::Io)?;
    let mut pages = Vec::new();
    for _ in have..TDS {
        pages.push(alloc_pages(1).ok_or(CamError::Io)?);
    }
    // the pages stay with the camera, also when the stream does not start
    if let Some(c) = CAMS.lock().iter_mut().find(|c| c.id == id) {
        c.pages.extend(pages);
    }
    let alt = alt_order(&l.alts, payload).into_iter().find(|&a| select(hc, slot, ctx, l.vs, a)).ok_or(CamError::Io)?;
    let dci = (alt.ep as u32 & 0xf) * 2 + 1;
    let td = alt.payload().min(4096) as u32;
    let mut s = Stream { alt, dci, td, trb: [0; TDS] };
    let mut list = CAMS.lock();
    let c = list.iter_mut().find(|c| c.id == id).ok_or(CamError::Gone)?;
    if c.pages.len() < TDS {
        return Err(CamError::Io);
    }
    c.frames.reset();
    c.taken = 0;
    c.seen = [0; 5];
    for i in 0..TDS {
        arm(hc, slot, &mut s, c.pages[i], i);
    }
    c.stream = Some(s);
    STREAMING.store(true, Ordering::Relaxed);
    w32(hc.db + slot as u64 * 4, dci);
    crate::kprintln!("[uvc] streaming {}x{} MJPEG, alt {} ({} bytes/interval, {payload} asked)", l.frame.1, l.frame.2, alt.alt, alt.payload());
    Ok(())
}

/// VS_PROBE (set, get) and VS_COMMIT: returns the payload size per interval the camera needs.
fn negotiate(hc: &mut Hc, slot: u32, l: &Layout) -> Option<usize> {
    let buf = alloc_pages(1)?;
    let len = probe_len(l.uvc);
    let p = probe(len, l.format, l.frame.0);
    let wr = |hc: &mut Hc, cs: u16| {
        // SAFETY: `buf` is a page of this function; `p` is at most 48 bytes.
        unsafe { core::ptr::copy_nonoverlapping(p.as_ptr(), buf as *mut u8, p.len()) };
        hc.control(slot, 0x21, SET_CUR, cs << 8, l.vs as u16, len as u16, buf)
    };
    let mut out = None;
    if wr(hc, VS_PROBE).is_some() && hc.control(slot, 0xA1, GET_CUR, VS_PROBE << 8, l.vs as u16, len as u16, buf).is_some() {
        // SAFETY: as above; the camera wrote at most `len` bytes.
        let got = unsafe { core::slice::from_raw_parts(buf as *const u8, len) }.to_vec();
        // commit what the camera answered
        // SAFETY: as above.
        unsafe { core::ptr::copy_nonoverlapping(got.as_ptr(), buf as *mut u8, len) };
        if hc.control(slot, 0x21, SET_CUR, VS_COMMIT << 8, l.vs as u16, len as u16, buf).is_some() {
            out = Some(max_payload(&got));
        }
    }
    crate::mem::free_page(buf);
    out
}

/// Drop endpoint `dci` of `slot` (Configure Endpoint with only its drop flag).
fn drop_ep(hc: &mut Hc, slot: u32, ctx: (u64, u64, u32), dci: u32) {
    let (inp, out) = (ctx.0, ctx.1);
    // SAFETY: `inp` is the slot's input context page.
    unsafe { core::ptr::write_bytes(inp as *mut u8, 0, 4096) };
    w32(inp, 1 << dci);
    w32(inp + 4, 1);
    let sc = ictx(inp, hc.ctx, 1);
    for i in 0..4 {
        w32(sc + i * 4, r32(out + i * 4));
    }
    hc.command(inp, (TRB_CONFIG_EP << 10) | (slot << 24));
    if let Some(i) = hc.rings.iter().position(|r| r.0 == slot && r.1 == dci) {
        let (_, _, ring) = hc.rings.remove(i);
        crate::mem::free_page(ring.base);
    }
}

/// The descriptor of camera `id` was closed: stop the stream.
pub fn close(id: u64) {
    let mut g = super::HC.lock();
    let stop = {
        let mut list = CAMS.lock();
        let Some(c) = list.iter_mut().find(|c| c.id == id) else { return };
        c.open = false;
        c.frames.reset();
        let k = c.frames.counts;
        crate::kprintln!("[uvc] stopped: {} transfers, {} with errors (last {}), {} other events, last code {}; {} bytes, {} frames, {} dropped", c.seen[0], c.seen[1], c.seen[4], c.seen[2], c.seen[3], k[0], k[1], k[2]);
        c.frames.counts = [0; 3];
        c.stream.take().map(|s| (c.hc, c.slot, c.ctx, c.layout.vs, s.dci))
    };
    if let Some((hci, slot, ctx, vs, dci)) = stop {
        if let Some(hc) = g.iter_mut().find(|h| h.index == hci) {
            hc.control(slot, 0x01, 11, 0, vs as u16, 0, 0);
            drop_ep(hc, slot, ctx, dci);
        }
    }
    STREAMING.store(CAMS.lock().iter().any(|c| c.stream.is_some()), Ordering::Relaxed);
}

/// Pump the host controllers while a camera streams (quick; from reads and the tick).
pub fn poll() {
    if !STREAMING.load(Ordering::Relaxed) {
        return;
    }
    let Some(mut g) = super::HC.try_lock() else { return };
    let hcs: Vec<usize> = CAMS.lock().iter().filter(|c| c.stream.is_some()).map(|c| c.hc).collect();
    for hc in g.iter_mut().filter(|h| hcs.contains(&h.index)) {
        hc.pump();
    }
}

/// The newest picture of camera `id` not yet taken; `Ok(None)`: none yet.
pub fn take(id: u64) -> Result<Option<Vec<u8>>, CamError> {
    let mut list = CAMS.lock();
    let c = list.iter_mut().find(|c| c.id == id).ok_or(CamError::Gone)?;
    match c.frames.ready.take() {
        Some((seq, pic)) if seq > c.taken => {
            c.taken = seq;
            Ok(Some(pic))
        }
        _ => Ok(None),
    }
}

/// Whether a read would not wait (a new picture is there, or the camera left).
pub fn ready(id: u64) -> bool {
    CAMS.lock().iter().find(|c| c.id == id).is_none_or(|c| c.frames.ready.as_ref().is_some_and(|r| r.0 > c.taken))
}
