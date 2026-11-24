//! Raw access to the partitions of the A/B layout (boot control and both slots), for the
//! system updater only.
//!
//! Nodes `/dev/ultar/part/<GPT name>` for `ULTAR-BOOTCTL`, `ultar-kernel-a/b`,
//! `ultar-system-a/b` – on the disk the loader named (`ultar.disk=`), nothing else. Rules:
//! * open: root (effective uid 0) with the raw-device right only;
//! * writing the running slot's partitions is refused (`EBUSY`), and so is writing any slot
//!   partition when the running slot is unknown;
//! * every read goes to the device (there is no cache), every write goes through to it;
//! * `fsync` is a device flush (virtio: write-through, NVMe FLUSH, ATA FLUSH CACHE EXT).
//!
//! Installer mode (started from an installation medium, `efi::VOM_INSTALLER`) adds whole-disk
//! nodes for the graphical installer, see [`scan_installer`]:
//! * `/dev/ultar/disk/<device>` for every block device except the medium (read and write),
//!   `/dev/ultar/medium` for the medium itself (read only, the source of the installation);
//! * they exist only in installer mode and only when exactly one disk carries the medium's
//!   partition `ULTAR-INSTALL` (else nothing is offered: the boot disk is never a target);
//! * open: the raw-device right **and** the installer's identity (user or group
//!   [`INSTALLER_ID`], `rootfs/etc/ultar/ids.toml`) – root without that group is refused.

use crate::driver::BLOCK;
use crate::proc::{self, FdKind};
use crate::syscall::with;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
use ultar_slots::gpt::Header;
use ultar_slots::layout::{self, RAW_PARTS};

const EIO: i64 = 5;
const EBADF: i64 = 9;
const EACCES: i64 = 13;
const EFAULT: i64 = 14;
const EBUSY: i64 = 16;
const ENOENT: i64 = 2;
const EINVAL: i64 = 22;
const ENOSPC: i64 = 28;
const EROFS: i64 = 30;
/// Largest transfer of one call; a longer request is a short read/write.
const MAX_IO: usize = 1 << 20;

/// Node names below `/dev/` (same order as `RAW_PARTS`).
pub const NODES: [&str; 5] = [
    "ultar/part/ULTAR-BOOTCTL",
    "ultar/part/ultar-kernel-a",
    "ultar/part/ultar-kernel-b",
    "ultar/part/ultar-system-a",
    "ultar/part/ultar-system-b",
];

/// One partition of the boot disk.
#[derive(Clone, Copy)]
pub struct Part {
    disk: usize,
    first: u64,
    sectors: u64,
    sector_size: usize,
    name: &'static str,
}

impl Part {
    fn bytes(&self) -> u64 {
        self.sectors * self.sector_size as u64
    }
}

/// An open node: the partition, whether it may be written, and the file position.
pub struct RawFd {
    part: Part,
    write: bool,
    pos: Mutex<u64>,
}

impl RawFd {
    /// Size in bytes (for `fstat`).
    pub fn size(&self) -> u64 {
        self.part.bytes()
    }
}

static PARTS: Mutex<Vec<Part>> = Mutex::new(Vec::new());
static BOOT_DISK: Mutex<Option<usize>> = Mutex::new(None);

/// User and group id of the installer (`installer` in `rootfs/etc/ultar/ids.toml`).
pub const INSTALLER_ID: u32 = 840;
/// GPT name of the partition that marks the installation medium.
const MEDIUM_PART: &str = "ULTAR-INSTALL";
/// Node of the medium below `/dev/`.
pub const MEDIUM_NODE: &str = "ultar/medium";
/// Directory of the target disks below `/dev/`.
pub const DISK_DIR: &str = "ultar/disk";
/// Installer mode: the medium's disk and the target disks (node name, whole disk).
static INSTALLER: Mutex<Option<(usize, Vec<(String, Part)>)>> = Mutex::new(None);

fn read_sectors(disk: usize, lba: u64, count: usize, ss: usize) -> Option<Vec<u8>> {
    let mut buf = crate::trybuf::try_vec(count * ss)?;
    BLOCK.lock().get_mut(disk)?.read(lba, &mut buf).ok()?;
    Some(buf)
}

/// The checked primary partition table of `disk`: header and entry array.
fn table(disk: usize) -> Option<(Header, Vec<u8>, usize)> {
    let ss = BLOCK.lock().get(disk)?.sector_size();
    let head = read_sectors(disk, 1, 1, ss)?;
    let h = Header::parse(&head, 1).ok()?;
    let mut entries = read_sectors(disk, h.entries_lba, h.entries_len().div_ceil(ss), ss)?;
    entries.truncate(h.entries_len());
    h.check_entries(&entries).ok()?;
    Some((h, entries, ss))
}

/// Finds the boot disk (GPT disk GUID from the loader) and its A/B partitions. Called once
/// after the drivers are bound; without A/B start nothing is registered.
pub fn scan() {
    scan_installer();
    let Some(guid) = crate::bootslot::disk() else { return };
    let disks = BLOCK.lock().len();
    let found: Vec<usize> = (0..disks).filter(|&d| table(d).is_some_and(|(h, _, _)| h.disk_guid == guid)).collect();
    let [disk] = found[..] else {
        crate::kprintln!("[boot] boot disk not found or not unique ({} matches)", found.len());
        return;
    };
    let Some((h, entries, ss)) = table(disk) else { return };
    let mut parts = PARTS.lock();
    for name in RAW_PARTS {
        match h.find(&entries, name) {
            Ok(Some(e)) => parts.push(Part { disk, first: e.first_lba, sectors: e.blocks(), sector_size: ss, name }),
            Ok(None) => {}
            Err(_) => crate::kprintln!("[boot] partition {name}: table refused"),
        }
    }
    *BOOT_DISK.lock() = Some(disk);
}

/// Installer mode only: finds the medium (the one disk with a partition `ULTAR-INSTALL`) and
/// offers every other disk as a whole. Without exactly one medium nothing is offered.
pub fn scan_installer() {
    if !crate::efi::VOM_INSTALLER.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let disks = BLOCK.lock().len();
    let has_medium = |d: usize| table(d).is_some_and(|(h, e, _)| matches!(h.find(&e, MEDIUM_PART), Ok(Some(_))));
    let media: Vec<usize> = (0..disks).filter(|&d| has_medium(d)).collect();
    let [medium] = media[..] else {
        crate::kprintln!("[installer] medium not found or not unique ({} disks with {MEDIUM_PART}): no disks offered", media.len());
        return;
    };
    let whole = |d: usize| {
        let bl = BLOCK.lock();
        let dev = bl.get(d)?;
        Some((dev.name(), Part { disk: d, first: 0, sectors: dev.sectors(), sector_size: dev.sector_size(), name: "" }))
    };
    let targets: Vec<(String, Part)> = (0..disks).filter(|&d| d != medium).filter_map(whole).filter(|(n, _)| valid_name(n)).collect();
    for (n, p) in &targets {
        crate::kprintln!("[installer] /dev/{DISK_DIR}/{n}: {} MiB", p.bytes() >> 20);
    }
    *INSTALLER.lock() = Some((medium, targets));
}

/// A device name usable as a node name (letters, digits, `-`, `_`; never `.` or `/`).
fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 32 && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Installer mode with a found medium: the medium's index in the block device list.
pub fn installer_medium() -> Option<usize> {
    INSTALLER.lock().as_ref().map(|(m, _)| *m)
}

/// Names of the target disks (listing of `/dev/ultar/disk`); empty outside installer mode.
pub fn disk_names() -> Vec<String> {
    INSTALLER.lock().as_ref().map(|(_, t)| t.iter().map(|(n, _)| n.clone()).collect()).unwrap_or_default()
}

/// The installer node for `/dev/<rest>` (`ultar/medium`, `ultar/disk/<device>`), if it exists.
pub fn installer_node(rest: &str) -> Option<&'static str> {
    let g = INSTALLER.lock();
    let (_, targets) = g.as_ref()?;
    if rest == MEDIUM_NODE {
        return Some(MEDIUM_NODE);
    }
    let name = rest.strip_prefix(DISK_DIR)?.strip_prefix('/')?;
    targets.iter().any(|(n, _)| n == name).then_some("ultar/disk")
}

/// Opens an installer node (`dev` = the path below `/dev/`); `write` for O_WRONLY/O_RDWR.
/// Refused (`EACCES`) without the raw-device right or without the installer's identity;
/// the medium never opens for writing (`EROFS`).
pub fn open_installer(dev: &str, write: bool, raw_right: bool) -> Result<FdKind, i64> {
    let cred = crate::cred::current();
    if !raw_right || !(cred.euid() == INSTALLER_ID || cred.in_group(INSTALLER_ID)) {
        return Err(-EACCES);
    }
    let g = INSTALLER.lock();
    let (medium, targets) = g.as_ref().ok_or(-ENOENT)?;
    let part = if dev == MEDIUM_NODE {
        if write {
            return Err(-EROFS);
        }
        let bl = BLOCK.lock();
        let d = bl.get(*medium).ok_or(-EIO)?;
        Part { disk: *medium, first: 0, sectors: d.sectors(), sector_size: d.sector_size(), name: "" }
    } else {
        let name = dev.strip_prefix(DISK_DIR).and_then(|r| r.strip_prefix('/')).ok_or(-EINVAL)?;
        targets.iter().find(|(n, _)| n == name).map(|(_, p)| *p).ok_or(-ENOENT)?
    };
    Ok(FdKind::RawPart(Arc::new(RawFd { part, write, pos: Mutex::new(0) })))
}

/// Index of the boot disk in the block device list (A/B start only).
pub fn boot_disk() -> Option<usize> {
    *BOOT_DISK.lock()
}

/// Disk and first sector of the A/B partition `name` (for mounting the slot's system).
pub fn locate(name: &str) -> Option<(usize, u64)> {
    PARTS.lock().iter().find(|p| p.name == name).map(|p| (p.disk, p.first))
}

/// Whether `name` is one of the partitions that are only ever reached through this module.
pub fn is_ab_name(name: &str) -> bool {
    RAW_PARTS.contains(&name)
}

/// The node for `/dev/<rest>`, if it exists.
pub fn node(rest: &str) -> Option<&'static str> {
    let parts = PARTS.lock();
    NODES.iter().copied().find(|n| *n == rest && parts.iter().any(|p| Some(p.name) == n.strip_prefix("ultar/part/")))
}

/// Names of the existing nodes (directory listing of `/dev/ultar/part`).
pub fn names() -> Vec<String> {
    PARTS.lock().iter().map(|p| String::from(p.name)).collect()
}

/// Opens node `dev` (`ultar/part/<name>`); `write` for O_WRONLY/O_RDWR.
pub fn open(dev: &str, write: bool, raw_right: bool) -> Result<FdKind, i64> {
    if !raw_right || crate::cred::current().euid() != 0 {
        return Err(-EACCES);
    }
    let name = dev.strip_prefix("ultar/part/").ok_or(-EINVAL)?;
    let part = *PARTS.lock().iter().find(|p| p.name == name).ok_or(-EINVAL)?;
    if write && name != layout::BOOTCTL_PART {
        match crate::bootslot::slot() {
            Some(running) if !layout::part_of_slot(name, running) => {}
            _ => return Err(-EBUSY),
        }
    }
    Ok(FdKind::RawPart(Arc::new(RawFd { part, write, pos: Mutex::new(0) })))
}

fn handle(fd: u64) -> Option<Arc<RawFd>> {
    with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::RawPart(r)) => Some(r.clone()),
        _ => None,
    })
    .flatten()
}

/// Sector range that covers `len` bytes at `off`: (first sector, count, offset in it).
fn span(part: &Part, off: u64, len: usize) -> (u64, usize, usize) {
    let ss = part.sector_size as u64;
    let first = off / ss;
    let last = (off + len as u64).div_ceil(ss);
    (first, (last - first) as usize, (off % ss) as usize)
}

fn read_at(part: &Part, off: u64, len: usize) -> Result<Vec<u8>, i64> {
    let len = len.min(MAX_IO).min(part.bytes().saturating_sub(off) as usize);
    if len == 0 {
        return Ok(Vec::new());
    }
    let (first, count, skip) = span(part, off, len);
    let mut buf = read_sectors(part.disk, part.first + first, count, part.sector_size).ok_or(-EIO)?;
    buf.drain(..skip);
    buf.truncate(len);
    Ok(buf)
}

fn write_at(part: &Part, off: u64, data: &[u8]) -> Result<usize, i64> {
    let len = data.len().min(MAX_IO);
    if off >= part.bytes() {
        return Err(-ENOSPC);
    }
    let len = len.min((part.bytes() - off) as usize);
    let (first, count, skip) = span(part, off, len);
    let ss = part.sector_size;
    let mut buf = crate::trybuf::try_vec(count * ss).ok_or(-ENOSPC)?;
    let mut bl = BLOCK.lock();
    let dev = bl.get_mut(part.disk).ok_or(-EIO)?;
    // partial first/last sector: read-modify-write
    if skip != 0 || len % ss != 0 {
        dev.read(part.first + first, &mut buf[..ss]).map_err(|_| -EIO)?;
        let tail = (count - 1) * ss;
        dev.read(part.first + first + count as u64 - 1, &mut buf[tail..]).map_err(|_| -EIO)?;
    }
    buf[skip..skip + len].copy_from_slice(&data[..len]);
    dev.write(part.first + first, &buf).map_err(|_| -EIO)?;
    Ok(len)
}

/// `read`/`pread64` on a node; `None` if `fd` is not one.
pub fn sys_read(fd: u64, buf: u64, n: u64, at: Option<u64>) -> Option<i64> {
    let r = handle(fd)?;
    let mut pos = r.pos.lock();
    let off = at.unwrap_or(*pos);
    let data = match read_at(&r.part, off, n as usize) {
        Ok(d) => d,
        Err(e) => return Some(e),
    };
    if !data.is_empty() && !with(|p| proc::write_user(p.space(), buf, &data)).unwrap_or(false) {
        return Some(-EFAULT);
    }
    if at.is_none() {
        *pos = off + data.len() as u64;
    }
    Some(data.len() as i64)
}

/// `write`/`pwrite64` on a node; `None` if `fd` is not one.
pub fn sys_write(fd: u64, buf: u64, n: u64, at: Option<u64>) -> Option<i64> {
    let r = handle(fd)?;
    if !r.write {
        return Some(-EBADF);
    }
    let mut pos = r.pos.lock();
    let off = at.unwrap_or(*pos);
    let len = (n as usize).min(MAX_IO);
    let Some(data) = with(|p| proc::read_user(p.space(), buf, len)).flatten() else { return Some(-EFAULT) };
    let done = match write_at(&r.part, off, &data) {
        Ok(d) => d,
        Err(e) => return Some(e),
    };
    if at.is_none() {
        *pos = off + done as u64;
    }
    Some(done as i64)
}

/// `lseek` on a node.
pub fn sys_lseek(fd: u64, off: i64, whence: u64) -> Option<i64> {
    let r = handle(fd)?;
    let mut pos = r.pos.lock();
    let base = match whence {
        0 => 0,
        1 => *pos as i64,
        2 => r.part.bytes() as i64,
        _ => return Some(-EINVAL),
    };
    let Some(new) = base.checked_add(off).filter(|n| *n >= 0) else { return Some(-EINVAL) };
    *pos = new as u64;
    Some(new)
}

/// `fsync`/`fdatasync` on a node: the device's write cache is flushed.
pub fn sys_fsync(fd: u64) -> Option<i64> {
    let r = handle(fd)?;
    let mut bl = BLOCK.lock();
    let ok = bl.get_mut(r.part.disk).is_some_and(|d| d.flush().is_ok());
    Some(if ok { 0 } else { -EIO })
}
