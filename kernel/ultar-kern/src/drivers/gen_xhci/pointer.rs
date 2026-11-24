//! USB pointing devices of the xHCI driver: every HID interface whose report descriptor has X and Y
//! (mice, absolute tablets such as QEMU's `usb-tablet`) is read in report protocol and its reports go
//! to the device's own input node (`input.rs`).

use super::hid::{self, Layout};
use super::{config_ep, w32, Desc, Hc, TRB_NORMAL};
use crate::mem::alloc_pages;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// Largest report read at once.
const REPORT: u32 = 64;

/// An attached pointing device.
struct Pointer {
    hc: usize,
    slot: u32,
    dci: u32,
    buf: u64,
    layout: Layout,
}

/// The pointing devices of every controller (locked after the controller list).
static PTRS: Mutex<Vec<Pointer>> = Mutex::new(Vec::new());

/// Whether controller `hc` has a pointing device (its events must be pumped).
pub(super) fn any(hc: usize) -> bool {
    PTRS.lock().iter().any(|p| p.hc == hc)
}

/// Set up interface `d` of the device in `slot` if it is a pointing device; `Ok(None)`: it is not.
pub(super) fn attach(
    hc: &mut Hc,
    slot: u32,
    ctx: (u64, u64, u32),
    d: &Desc,
) -> Result<Option<String>, String> {
    let Some(&(addr, _, mp, interval)) = d.eps.iter().find(|e| e.0 & 0x80 != 0 && e.1 & 3 == 3)
    else {
        return Ok(None);
    };
    let buf = alloc_pages(1).ok_or("no memory")?;
    // GET_DESCRIPTOR (HID report descriptor) of the interface
    let n = hc
        .control(slot, 0x81, 6, 0x2200, d.iface as u16, 4096, buf)
        .unwrap_or(0);
    // SAFETY: `buf` is a fresh page of this driver; the device wrote `n` <= 4096 bytes into it.
    let desc = unsafe { core::slice::from_raw_parts(buf as *const u8, n.min(4096)) };
    let Some(layout) = hid::parse(desc) else {
        crate::mem::free_page(buf);
        return Ok(None);
    };
    let dci = (addr as u32 & 0xf) * 2 + 1;
    config_ep(
        hc,
        slot,
        ctx.0,
        ctx.1,
        ctx.2,
        &[(dci, 7, mp as u32, interval)],
    )?;
    if d.sub == 1 {
        hc.control(slot, 0x21, 0x0B, 1, d.iface as u16, 0, 0); // SET_PROTOCOL: report
    }
    hc.control(slot, 0x21, 0x0A, 0, d.iface as u16, 0, 0); // SET_IDLE
    let abs = layout.x.abs;
    PTRS.lock().push(Pointer {
        hc: hc.index,
        slot,
        dci,
        buf,
        layout,
    });
    request(hc, slot, dci, buf);
    Ok(Some(String::from(if abs { "tablet" } else { "mouse" })))
}

/// Ask for the next report.
fn request(hc: &mut Hc, slot: u32, dci: u32, buf: u64) {
    let ring = hc.ring(slot, dci);
    ring.push(buf, REPORT, (TRB_NORMAL << 10) | (1 << 5) | (1 << 2));
    w32(hc.db + slot as u64 * 4, dci);
}

/// Absolute ranges of a tablet (min/max of X and Y), `Some(None)` for a relative mouse, `None` if the
/// device in `slot` is no pointing device.
pub(super) fn ranges(hc: &Hc, slot: u32) -> Option<Option<[i32; 4]>> {
    let list = PTRS.lock();
    let l = &list
        .iter()
        .find(|p| p.hc == hc.index && p.slot == slot)?
        .layout;
    Some(l.x.abs.then_some([l.x.min, l.x.max, l.y.min, l.y.max]))
}

/// A transfer of endpoint `(slot, dci)` completed (`code`, `left` bytes not transferred): if it is a
/// pointer's report, hand it to its input node, ask for the next one and return true.
pub(super) fn report(hc: &mut Hc, (slot, dci): (u32, u32), code: u32, left: u32) -> bool {
    let buf = {
        let list = PTRS.lock();
        let Some(p) = list
            .iter()
            .find(|p| p.hc == hc.index && p.slot == slot && p.dci == dci)
        else {
            return false;
        };
        if matches!(code, 1 | 13) {
            let n = REPORT.saturating_sub(left) as usize;
            // SAFETY: `buf` is this pointer's page; the controller wrote `n` <= REPORT bytes into it.
            let data = unsafe { core::slice::from_raw_parts(p.buf as *const u8, n) };
            let l = &p.layout;
            let body = if l.id != 0 {
                data.split_first()
                    .filter(|(id, _)| **id == l.id)
                    .map(|x| x.1)
            } else {
                Some(data)
            };
            if let Some(body) = body {
                let buttons = l
                    .buttons
                    .iter()
                    .enumerate()
                    .fold(0u8, |b, (i, f)| b | ((f.read(body) != 0) as u8) << i);
                let wheel = l.wheel.map_or(0, |f| f.read(body));
                crate::input::usb_pointer(
                    hc.index,
                    slot,
                    buttons,
                    l.x.read(body),
                    l.y.read(body),
                    wheel,
                );
            }
        }
        p.buf
    };
    request(hc, slot, dci, buf);
    true
}

/// The device in `slot` left: forget its pointers and give their pages back.
pub(super) fn remove(hc: &mut Hc, slot: u32) {
    let mut list = PTRS.lock();
    for p in list.iter().filter(|p| p.hc == hc.index && p.slot == slot) {
        crate::mem::free_page(p.buf);
    }
    list.retain(|p| !(p.hc == hc.index && p.slot == slot));
}
