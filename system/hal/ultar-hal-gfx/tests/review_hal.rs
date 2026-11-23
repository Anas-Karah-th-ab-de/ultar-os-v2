//! Regression tests of the independent review (w4/comprev, against
//! 0f4846e) for the graphics HAL. FINDING tests fail on 0f4846e, SOUND
//! ones pass.

use rustix::fs::{fallocate, fcntl_add_seals, ftruncate, memfd_create};
use rustix::fs::{FallocateFlags, MemfdFlags, SealFlags};
use std::io::Write;
use std::os::fd::{AsFd, OwnedFd};
use std::path::PathBuf;
use ultar_hal::{InputDeviceId, InputEventKind, KeyState};
use ultar_hal_gfx::evdev::{AxisRange, Decoder, Evdev, RECORD};
use ultar_hal_gfx::shm::{SealedMapping, SharedBuffer};

fn record(kind: u16, code: u16, value: i32) -> [u8; RECORD] {
    let mut r = [0u8; RECORD];
    r[16..18].copy_from_slice(&kind.to_ne_bytes());
    r[18..20].copy_from_slice(&code.to_ne_bytes());
    r[20..24].copy_from_slice(&value.to_ne_bytes());
    r
}

/// FINDING m3: `SYN_DROPPED` cancels touches but forgets held keys and
/// buttons. A Super/Alt/Ctrl or mouse button pressed before the drop and
/// released during it stays "held" in the compositor for ever (every click
/// becomes a window move with Super stuck).
#[test]
fn syn_dropped_releases_held_keys() {
    let unit = AxisRange { min: 0, max: 65535 };
    let mut d = Decoder::new(InputDeviceId(0), unit, unit, false);
    let mut out = Vec::new();
    d.feed(&record(1, 125, 1), &mut out); // KEY_LEFTMETA down
    d.feed(&record(0, 0, 0), &mut out); // SYN_REPORT
    out.clear();
    d.feed(&record(0, 3, 0), &mut out); // SYN_DROPPED (release lost)
    d.feed(&record(0, 0, 0), &mut out);
    let released = out.iter().any(|e| {
        matches!(
            e.kind,
            InputEventKind::Key {
                code: 125,
                state: KeyState::Released
            }
        )
    });
    assert!(released, "m3: Super stays held after SYN_DROPPED: {out:?}");
}

fn fifo(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let ok = std::process::Command::new("mkfifo")
        .arg(&path)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "mkfifo");
    path
}

/// FINDING M6: a device that hangs up (USB keyboard or mouse unplugged:
/// `read` fails with ENODEV, `poll` reports POLLERR|POLLHUP for ever) is
/// never removed. The compositor polls its descriptor (`server.rs` wait,
/// `backend/hal.rs` input_fds) and wakes at once on every turn: 100 % CPU
/// until restart. A FIFO whose writer closed behaves the same way.
#[test]
fn a_hung_up_input_device_is_dropped() {
    let path = fifo("ultar-review-evdev");
    let evdev = Evdev::open_paths(std::slice::from_ref(&path)).unwrap();
    let mut writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    writer.write_all(&record(0, 0, 0)).unwrap();
    drop(writer);
    use ultar_hal::Input;
    let _ = evdev.poll(0);
    let _ = evdev.poll(0);
    let handles = evdev.poll_handles();
    let mut fds: Vec<_> = handles
        .iter()
        .map(|h| rustix::event::PollFd::new(h, rustix::event::PollFlags::IN))
        .collect();
    let zero = rustix::event::Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let ready = rustix::event::poll(&mut fds, Some(&zero)).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        ready,
        0,
        "M6: a hung-up device stays in the poll set ({} devices kept)",
        evdev.devices().unwrap().len()
    );
}

fn sealed_sparse(len: u64) -> OwnedFd {
    let fd = memfd_create("review", MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING).unwrap();
    ftruncate(&fd, len).unwrap();
    fcntl_add_seals(&fd, SealFlags::SHRINK | SealFlags::GROW | SealFlags::SEAL).unwrap();
    fd
}

fn rss_shmem_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    status
        .lines()
        .find_map(|l| l.strip_prefix("RssShmem:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap_or(0)
}

/// FINDING M3b: a client can send a sealed but *sparse* memory file (no
/// page allocated, costs the client nothing). The compositor's read faults
/// the pages in, so they are allocated – and charged – on the compositor's
/// side (first-touch accounting), up to 256 MiB per buffer. The design says
/// buffers are charged to the client.
#[test]
fn sparse_buffers_are_refused() {
    let len = 64u64 << 20;
    let fd = sealed_sparse(len);
    let Ok(mapping) = SealedMapping::map(fd, len) else {
        return; // refused: fixed
    };
    let before = rss_shmem_kib();
    let mut row = vec![0u32; 4096];
    for offset in (0..len as usize).step_by(row.len() * 4) {
        mapping.copy_pixels(offset, &mut row).unwrap();
    }
    let grown = rss_shmem_kib().saturating_sub(before);
    panic!("M3b: sparse 64 MiB buffer accepted; reading it put {grown} KiB on the reader");
}

/// SOUND: a hole punched into a sealed buffer after it was mapped reads as
/// zeros – no SIGBUS (SHRINK seal holds, the size never changes). Since
/// fix round 2 (N8) buffers also carry `FUTURE_WRITE`, so the punch itself
/// is refused; either way the compositor never faults.
#[test]
fn punched_holes_read_as_zero_without_sigbus() {
    let mut buffer = SharedBuffer::create(1 << 20).unwrap();
    buffer.bytes_mut().fill(0xff);
    let mapping = SealedMapping::map(buffer.fd().try_clone_to_owned().unwrap(), 1 << 20).unwrap();
    let flags = FallocateFlags::PUNCH_HOLE | FallocateFlags::KEEP_SIZE;
    let punched = fallocate(buffer.fd(), flags, 0, 1 << 20).is_ok();
    let shrink = ftruncate(buffer.fd().as_fd(), 4096);
    assert!(shrink.is_err(), "sealed buffer must not shrink");
    let mut row = [1u32; 256];
    mapping.copy_pixels((1 << 20) - 1024, &mut row).unwrap();
    let expected = if punched { 0 } else { 0xffff_ffff };
    assert!(row.iter().all(|&p| p == expected));
}

/// SOUND: huge-page memory files either map (pages reserved at map time)
/// or are refused – never a later SIGBUS.
#[test]
fn huge_page_memory_files_never_fault_later() {
    let flags = MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING | MemfdFlags::HUGETLB;
    let Ok(fd) = memfd_create("review-huge", flags) else {
        return; // no hugetlbfs here
    };
    if ftruncate(&fd, 2 << 20).is_err() {
        return;
    }
    let _ = fcntl_add_seals(&fd, SealFlags::SHRINK);
    if let Ok(mapping) = SealedMapping::map(fd, 2 << 20) {
        let mut row = [0u32; 16];
        mapping.copy_pixels(0, &mut row).unwrap();
    }
}
