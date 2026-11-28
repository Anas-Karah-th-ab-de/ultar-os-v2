//! `/dev/ultar/hciN`: the raw HCI transport of a USB Bluetooth controller for the Bluetooth
//! service (the transport itself is `drivers/gen_xhci/bt.rs`).
//!
//! * open: right `BLUETOOTH` only; one descriptor per controller at a time (`EBUSY`), shared by
//!   `dup`/`fork` like any descriptor; the last close frees the controller for the next opener;
//! * `write`: exactly one H4 record per call – `0x01` + an HCI command (3 + length octets) or
//!   `0x02` + ACL data (4 + length octets, at most 1021 data octets); anything else `EINVAL`, a
//!   controller that does not take it `EIO`; vendor commands (OGF `0x3F`: firmware patch RAM,
//!   reset to the boot loader, …) `EPERM` – the Bluetooth service needs none, and a compromised
//!   service must not reprogram or brick the controller (review S-11);
//! * `read`: exactly one record per call – `0x04` + an event or `0x02` + ACL data; a buffer
//!   smaller than the record: `EINVAL` (the record stays); nothing there: waits, or `EAGAIN` with
//!   `O_NONBLOCK`; `poll` reports `POLLIN`;
//! * the controller left: `read`/`write` answer `ENODEV`.

use super::{EAGAIN, EBUSY, EFAULT, EINVAL, EIO, ENODEV};
use crate::drivers::gen_xhci::bt::{self, BtError};
use crate::proc::{self, FdKind};
use crate::syscall::with;
use alloc::sync::Arc;

/// Longest record accepted by `write`: type, ACL header, data.
const MAX_WRITE: usize = 1 + 4 + bt::MAX_ACL_DATA;
/// H4 type of an HCI command.
const H4_COMMAND: u8 = 0x01;
/// Opcode group of the vendor commands.
const OGF_VENDOR: u16 = 0x3f;
const EPERM: i64 = 1;

/// Whether the H4 record is a vendor command.
fn is_vendor_command(rec: &[u8]) -> bool {
    rec.len() >= 3 && rec[0] == H4_COMMAND && u16::from_le_bytes([rec[1], rec[2]]) >> 10 == OGF_VENDOR
}

/// An open controller node.
pub struct HciFd {
    id: u64,
}

impl Drop for HciFd {
    fn drop(&mut self) {
        bt::close(self.id);
    }
}

/// What a read or write on the node needs from the system call.
pub enum Io {
    /// Done: the result.
    Val(i64),
    /// Wait a little (the driver polls) and repeat the call.
    Block,
}

fn errno(e: BtError) -> i64 {
    match e {
        BtError::Gone => -ENODEV,
        BtError::Busy => -EBUSY,
        BtError::Invalid => -EINVAL,
        BtError::Io => -EIO,
    }
}

/// Open controller node `index` (the caller holds the right).
pub fn open(index: u8) -> Result<FdKind, i64> {
    let id = bt::open(index).map_err(errno)?;
    Ok(FdKind::Hci(Arc::new(HciFd { id })))
}

/// The node behind descriptor `fd` and whether it is non-blocking.
fn handle(fd: u64) -> Option<(Arc<HciFd>, bool)> {
    with(|p| match p.fd(fd) {
        Some(d) => match &d.kind {
            FdKind::Hci(h) => Some((h.clone(), d.flags & 0x800 != 0)),
            _ => None,
        },
        None => None,
    })
    .flatten()
}

/// `read` on a node; `None` if `fd` is not one.
pub fn sys_read(fd: u64, buf: u64, n: u64) -> Option<Io> {
    let (h, nonblock) = handle(fd)?;
    bt::recover();
    bt::poll();
    let rec = match bt::take(h.id) {
        Ok(Some(r)) => r,
        Ok(None) if nonblock => return Some(Io::Val(-EAGAIN)),
        Ok(None) => return Some(Io::Block),
        Err(e) => return Some(Io::Val(errno(e))),
    };
    if rec.len() as u64 > n {
        bt::untake(h.id, rec);
        return Some(Io::Val(-EINVAL));
    }
    if !with(|p| proc::write_user(p.space(), buf, &rec)).unwrap_or(false) {
        bt::untake(h.id, rec);
        return Some(Io::Val(-EFAULT));
    }
    Some(Io::Val(rec.len() as i64))
}

/// `write` on a node; `None` if `fd` is not one.
pub fn sys_write(fd: u64, buf: u64, n: u64) -> Option<i64> {
    let (h, _) = handle(fd)?;
    if n == 0 || n as usize > MAX_WRITE {
        return Some(-EINVAL);
    }
    let Some(rec) = with(|p| proc::read_user(p.space(), buf, n as usize)).flatten() else { return Some(-EFAULT) };
    if is_vendor_command(&rec) {
        return Some(-EPERM);
    }
    Some(match bt::send(h.id, &rec) {
        Ok(()) => n as i64,
        Err(e) => errno(e),
    })
}

/// Whether a read on the node would not wait (`poll`).
pub fn ready(h: &HciFd) -> bool {
    bt::poll();
    bt::ready(h.id)
}
