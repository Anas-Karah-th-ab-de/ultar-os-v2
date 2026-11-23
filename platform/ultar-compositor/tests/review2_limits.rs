//! Re-review (w4/comprev2, against b8849e3): attacks on the new limits –
//! reserved client slots, the global memory limit, sparse-buffer refusal,
//! transfer flow control. FINDING tests fail on b8849e3.

mod common;

use common::*;
use std::os::fd::AsFd;
use std::time::Duration;
use ultar_compositor::{Compositor, Config, RightsTable};
use ultar_surface::proto::{BufferId, Event, PixelFormat, Request, Rights, SurfaceId, TransferId};

const SCREENSHOT_APP: u32 = 6000;
const LOCK: u32 = 901;

fn config(grants: &[(u32, Rights)]) -> Config {
    let mut rights = RightsTable::default();
    for &(uid, r) in grants {
        rights.grant(uid, r);
    }
    Config {
        rights,
        ..test_config()
    }
}

/// FINDING N5: the reserved slots are open to every user id with any
/// right, and such ids have no per-uid limit. A screenshot tool
/// (`ui.capture`) or a notification app (`ui.overlay`) takes all 64
/// slots; the lock screen can no longer connect.
#[test]
fn a_low_right_cannot_take_the_lock_screens_slot() {
    let grants = [
        (SCREENSHOT_APP, Rights::CAPTURE),
        (LOCK, Rights::LOCK_SCREEN),
    ];
    let mut comp = compositor(config(&grants), &[(800, 600)]);
    let mut held = Vec::new();
    for _ in 0..comp.state.config.limits.clients {
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        let peer = ultar_compositor::conn::Peer {
            pid: None,
            uid: SCREENSHOT_APP,
        };
        comp.state.accept(a, peer);
        held.push(b);
    }
    let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
    let lock = ultar_compositor::conn::Peer {
        pid: None,
        uid: LOCK,
    };
    assert!(
        comp.state.accept(a, lock).is_some(),
        "N5: uid {SCREENSHOT_APP} (ui.capture only) holds {} slots",
        comp.state.clients.len()
    );
}

/// FINDING N6: the 1 GiB is shared first come, first served. One app (four
/// connections of one uid) takes all of it; the lock screen then cannot
/// get a single buffer (and is ended with `Limit`), so the locked session
/// can never be unlocked.
#[test]
fn one_app_cannot_take_the_lock_screens_memory() {
    let mut comp = compositor(config(&[(LOCK, Rights::LOCK_SCREEN)]), &[(1920, 1080)]);
    let mut hogs = Vec::new();
    for _ in 0..4 {
        let mut p = Peer::hello_as(&mut comp, 5000);
        for b in 1..=4 {
            p.send(&Request::CreateInlineBuffer {
                buffer: BufferId(b),
                width: 8192,
                height: 2048,
                format: PixelFormat::Xrgb8888,
            });
        }
        p.pump(&mut comp);
        hogs.push(p);
    }
    let mut lock = Peer::hello_as(&mut comp, LOCK);
    lock.send(&Request::CreateInlineBuffer {
        buffer: BufferId(1),
        width: 1920,
        height: 1080,
        format: PixelFormat::Xrgb8888,
    });
    lock.pump(&mut comp);
    assert!(
        lock.error().is_none(),
        "N6: lock screen refused ({:?}); one uid holds {} MiB",
        lock.error(),
        comp.state.total_bytes() >> 20
    );
}

/// FINDING N7: until apps have uids of their own (K9) every app of the
/// session shares one uid without rights: the per-uid limit caps the
/// whole desktop at 8 app connections (the 9th app cannot open a window).
#[test]
fn nine_apps_of_one_user_can_run() {
    let mut comp = compositor(test_config(), &[(800, 600)]);
    let mut held = Vec::new();
    for _ in 0..9 {
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        let peer = ultar_compositor::conn::Peer {
            pid: None,
            uid: 1000,
        };
        held.push((comp.state.accept(a, peer).is_some(), b));
    }
    let refused = held.iter().filter(|(ok, _)| !ok).count();
    assert_eq!(refused, 0, "N7: {refused} of 9 apps of uid 1000 refused");
}

fn blocks(fd: std::os::fd::BorrowedFd<'_>) -> i64 {
    rustix::fs::fstat(fd).unwrap().st_blocks as i64
}

/// FINDING N8 (TOCTOU of the M3b fix): the `st_blocks` check runs once at
/// `CreateShmBuffer`. Punching the pages out afterwards is still allowed
/// (no write seal), and the compositor's next copy allocates them again
/// on its own side.
#[test]
fn a_buffer_cannot_be_hollowed_after_the_check() {
    let config = Config {
        inline_only: false,
        ..test_config()
    };
    let mut comp = compositor(config, &[(1024, 768)]);
    let mut p = Peer::hello(&mut comp);
    let s = p.window(&mut comp, 1, 0x101010);
    let c = p.last_configure(s).unwrap();
    let (w, h) = (c.width, c.height);
    let len = (w * h * 4) as usize;
    let buffer = ultar_hal_gfx::shm::SharedBuffer::create(len).unwrap();
    let frame = Request::CreateShmBuffer {
        buffer: BufferId(7),
        width: w,
        height: h,
        stride: w * 4,
        format: PixelFormat::Xrgb8888,
    }
    .encode()
    .unwrap();
    ultar_hal_gfx::fdpass::send(&p.stream, &frame, Some(buffer.fd())).unwrap();
    p.pump(&mut comp);
    let flags = rustix::fs::FallocateFlags::PUNCH_HOLE | rustix::fs::FallocateFlags::KEEP_SIZE;
    let punched = rustix::fs::fallocate(buffer.fd(), flags, 0, len as u64).is_ok();
    let hollow = blocks(buffer.fd().as_fd());
    p.send(&Request::Attach {
        surface: s,
        buffer: BufferId(7),
    });
    p.send(&Request::Commit {
        surface: s,
        configure: c.serial,
    });
    p.pump(&mut comp);
    let refilled = blocks(buffer.fd().as_fd());
    assert!(
        !punched || (p.error().is_some() && refilled == hollow),
        "N8: punched after the check (blocks {hollow}); the commit refilled {refilled} blocks on the compositor's side"
    );
}

fn serial_of(p: &Peer) -> u32 {
    p.events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Key {
                serial, state: 1, ..
            } => Some(*serial),
            _ => None,
        })
        .expect("key")
}

/// A owns a 4 MiB selection; B (focused) pastes and stops reading.
fn stalled_paste(comp: &mut Compositor) -> (Peer, Peer) {
    let mut a = Peer::hello_as(comp, 5000);
    a.window(comp, 1, 0xff0000);
    let mut b = Peer::hello_as(comp, 5001);
    b.window(comp, 1, 0x0000ff);
    let (ka, kb) = (comp.state.windows[0].key, comp.state.windows[1].key);
    comp.state.focus_window(ka);
    key(comp, 30, true);
    key(comp, 30, false);
    a.pump(comp);
    let mimes = vec!["image/png".to_string()];
    a.send(&Request::SetSelection {
        serial: serial_of(&a),
        mimes,
    });
    a.pump(comp);
    comp.state.focus_window(kb);
    key(comp, 47, true);
    key(comp, 47, false);
    b.pump(comp);
    b.send(&Request::ReadSelection {
        serial: serial_of(&b),
        mime: "image/png".into(),
    });
    b.pump(comp);
    a.pump(comp);
    let transfer = a
        .events
        .iter()
        .find_map(|e| match e {
            Event::SendRequest { transfer, .. } => Some(*transfer),
            _ => None,
        })
        .unwrap_or(TransferId(0));
    for i in 0..4 {
        a.send(&Request::SendData {
            transfer,
            data: vec![7; 1 << 20],
            last: i == 3,
        });
        a.pump(comp);
    }
    (a, b)
}

/// FINDING N9: flow control stops reading the *whole* source. A focused
/// app that pastes and then does not read freezes the clipboard owner: its
/// commits, pongs and titles are not handled until the transfer expires
/// (10 s), and the next keystroke buys the next 10 s.
#[test]
fn a_slow_reader_does_not_freeze_the_clipboard_owner() {
    let mut comp = compositor(test_config(), &[(1024, 768)]);
    let (mut a, _b) = stalled_paste(&mut comp);
    assert!(comp.state.throttled(1), "setup: source throttled");
    a.send(&Request::SetTitle {
        surface: SurfaceId(1),
        title: "still alive".into(),
    });
    for _ in 0..20 {
        a.pump(&mut comp);
    }
    let key = comp
        .state
        .windows
        .iter()
        .find(|w| w.key.client == 1)
        .unwrap()
        .key;
    let title = comp.state.surface(key).unwrap().title.clone();
    assert_eq!(title, "still alive", "N9: the clipboard owner is frozen");
}

/// FINDING N10: a throttled source is left out of the read set but
/// `poll` still reports its hang-up; `read_client` returns at once, so a
/// source that closes while throttled is neither removed nor waited for –
/// the loop spins until the transfer expires.
#[test]
fn a_dead_throttled_source_is_removed_without_spinning() {
    let mut comp = compositor(test_config(), &[(1024, 768)]);
    let (a, _b) = stalled_paste(&mut comp);
    drop(a);
    let started = std::time::Instant::now();
    for _ in 0..10 {
        comp.dispatch(Some(Duration::from_millis(50))).unwrap();
    }
    let spent = started.elapsed();
    let gone = !comp.state.clients.contains_key(&1);
    assert!(
        gone,
        "N10: dead source still connected; 10 waits of 50 ms took {spent:?}"
    );
}
