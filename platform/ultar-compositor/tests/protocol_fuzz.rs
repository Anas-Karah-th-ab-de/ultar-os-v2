//! Hostile input on the socket: random bytes, random requests with random
//! fields, huge sizes, descriptor abuse. The compositor must never panic,
//! must keep memory within the limits, and must keep serving the others.

mod common;

use common::*;
use std::io::IoSlice;
use std::mem::MaybeUninit;
use std::os::fd::AsFd;
use std::time::Duration;
use ultar_compositor::{Config, Limits};
use ultar_surface::proto::*;
use ultar_surface::wire::HEADER;

fn assert_bounded(comp: &ultar_compositor::Compositor) {
    let limits = comp.state.config.limits;
    for c in comp.state.clients.values() {
        assert!(
            c.bytes <= limits.client_bytes,
            "client memory {} over limit",
            c.bytes
        );
        assert!(c.surfaces.len() <= limits.surfaces);
        assert!(c.buffers.len() <= limits.buffers);
    }
}

#[test]
fn random_bytes_never_hurt() {
    let mut comp = compositor(test_config(), &[(640, 480)]);
    let mut good = Peer::hello(&mut comp);
    good.window(&mut comp, 1, 0x112233);
    let mut rng = Rng(0xdead_beef);
    for round in 0..300 {
        let mut p = Peer::connect(&mut comp);
        if round % 2 == 0 {
            p.send(&Request::Hello {
                version: 1,
                app_id: "fuzz".into(),
                shared_memory: false,
            });
        }
        let len = rng.below(4096) as usize;
        let mut bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        if round % 3 == 0 && bytes.len() > HEADER {
            // Plausible header, random body.
            let body = (bytes.len() - HEADER) as u32;
            bytes[0..4].copy_from_slice(&body.to_le_bytes());
            bytes[4] = rng.below(30) as u8;
            bytes[5] = 0;
            bytes[6] = 0;
            bytes[7] = 0;
        }
        p.raw(&bytes);
        p.pump(&mut comp);
        assert_bounded(&comp);
    }
    // The good client is still served.
    let s = SurfaceId(1);
    good.draw(&mut comp, s, 0x445566);
    assert!(good.error().is_none());
    assert!(comp.state.windows.iter().any(|w| w.mapped));
}

#[rustfmt::skip] // one request kind per line reads like a table
fn random_request(rng: &mut Rng) -> Request {
    let s = SurfaceId(rng.below(6) as u32);
    let b = BufferId(rng.below(6) as u32);
    let big = |rng: &mut Rng| [0, 1, 7, 64, 8192, 8193, u32::MAX][rng.below(7) as usize];
    let rect = |rng: &mut Rng| Rect::new(rng.next() as i32 % 10_000, rng.next() as i32 % 10_000, big(rng), big(rng));
    let roles = [Role::Toplevel, Role::Lock, Role::Popup { parent: SurfaceId(1), x: -5, y: 5 }, Role::Background];
    match rng.below(16) {
        0 => Request::CreateSurface { surface: s, role: roles[rng.below(4) as usize] },
        1 => Request::DestroySurface { surface: s },
        2 => Request::CreateInlineBuffer { buffer: b, width: big(rng), height: big(rng), format: PixelFormat::Argb8888 },
        3 => Request::WritePixels { buffer: b, rect: rect(rng), data: vec![1; rng.below(5000) as usize] },
        4 => Request::Attach { surface: s, buffer: b },
        5 => Request::Damage { surface: s, rects: (0..rng.below(33)).map(|_| rect(rng)).collect() },
        6 => Request::Commit { surface: s, configure: rng.next() as u32 },
        7 => Request::SetState { surface: s, state: WindowState::Maximized },
        8 => Request::SetTitle { surface: s, title: "\u{202e}evil title".into() },
        9 => Request::Move { surface: s, serial: rng.next() as u32 },
        10 => Request::SetSelection { serial: rng.next() as u32, mimes: vec!["text/plain".into()] },
        11 => Request::ReadSelection { serial: 1, mime: "text/plain".into() },
        12 => Request::SendData { transfer: TransferId(rng.below(4) as u32), data: vec![0; 100], last: true },
        13 => Request::DestroyBuffer { buffer: b },
        14 => Request::SetSizeLimits { surface: s, min: (big(rng), big(rng)), max: (big(rng), big(rng)) },
        _ => Request::Frame { surface: s },
    }
}

#[test]
fn random_requests_keep_limits() {
    let limits = Limits {
        client_bytes: 64 << 20,
        surfaces: 8,
        buffers: 8,
        ..Limits::default()
    };
    let mut comp = compositor(
        Config {
            limits,
            ..test_config()
        },
        &[(800, 600)],
    );
    let mut rng = Rng(42);
    let mut errors = 0;
    for _ in 0..40 {
        let mut p = Peer::hello(&mut comp);
        for _ in 0..60 {
            let r = random_request(&mut rng);
            p.send(&r);
            if rng.below(4) == 0 {
                p.pump(&mut comp);
                assert_bounded(&comp);
            }
            if p.closed {
                break;
            }
        }
        p.pump(&mut comp);
        errors += usize::from(p.error().is_some());
        assert_bounded(&comp);
    }
    assert!(
        errors > 10,
        "most random sessions end in an error ({errors})"
    );
    comp.render_now();
}

#[test]
fn huge_sizes_and_memory_limits() {
    let limits = Limits {
        client_bytes: 32 << 20,
        ..Limits::default()
    };
    let mut comp = compositor(
        Config {
            limits,
            ..test_config()
        },
        &[(800, 600)],
    );
    let mut p = Peer::hello(&mut comp);
    // 8192 x 8192 x 4 = 256 MiB > 32 MiB.
    p.send(&Request::CreateInlineBuffer {
        buffer: BufferId(1),
        width: 8192,
        height: 8192,
        format: PixelFormat::Xrgb8888,
    });
    p.pump(&mut comp);
    assert_eq!(p.error(), Some(ErrorCode::Limit));
    let mut q = Peer::hello(&mut comp);
    q.send(&Request::CreateInlineBuffer {
        buffer: BufferId(1),
        width: 8193,
        height: 1,
        format: PixelFormat::Xrgb8888,
    });
    q.pump(&mut comp);
    assert_eq!(q.error(), Some(ErrorCode::BadBuffer));
    let mut r = Peer::hello(&mut comp);
    // A frame header announcing more than the maximum body.
    r.raw(&[0xff, 0xff, 0xff, 0x7f, 8, 0, 0, 0]);
    r.pump(&mut comp);
    assert_eq!(r.error(), Some(ErrorCode::Malformed));
    assert_eq!(comp.state.clients.len(), 0);
}

#[test]
fn protocol_order_is_enforced() {
    let mut comp = compositor(test_config(), &[(800, 600)]);
    let mut p = Peer::connect(&mut comp);
    p.send(&Request::CreateSurface {
        surface: SurfaceId(1),
        role: Role::Toplevel,
    });
    p.pump(&mut comp);
    assert_eq!(
        p.error(),
        Some(ErrorCode::Malformed),
        "request before Hello"
    );
    let mut q = Peer::hello(&mut comp);
    q.send(&Request::Hello {
        version: 1,
        app_id: "again".into(),
        shared_memory: false,
    });
    q.pump(&mut comp);
    assert_eq!(q.error(), Some(ErrorCode::Malformed), "second Hello");
    let mut r = Peer::hello(&mut comp);
    r.send(&Request::CreateSurface {
        surface: SurfaceId(0),
        role: Role::Toplevel,
    });
    r.pump(&mut comp);
    assert_eq!(r.error(), Some(ErrorCode::UnknownObject), "id 0");
    let mut s = Peer::hello(&mut comp);
    s.send(&Request::CreateSurface {
        surface: SurfaceId(1),
        role: Role::Lock,
    });
    s.pump(&mut comp);
    assert_eq!(
        s.error(),
        Some(ErrorCode::Permission),
        "lock screen without right"
    );
    let mut t = Peer::hello(&mut comp);
    t.send(&Request::ImeCommit { text: "x".into() });
    t.pump(&mut comp);
    assert_eq!(
        t.error(),
        Some(ErrorCode::Permission),
        "input method without right"
    );
}

fn shm_request(width: u32, height: u32, stride: u32) -> Vec<u8> {
    let format = PixelFormat::Xrgb8888;
    let request = Request::CreateShmBuffer {
        buffer: BufferId(1),
        width,
        height,
        stride,
        format,
    };
    request.encode().unwrap()
}

fn send_fds(
    stream: &std::os::unix::net::UnixStream,
    bytes: &[u8],
    fds: &[std::os::fd::BorrowedFd<'_>],
) {
    use rustix::net::{sendmsg, SendAncillaryBuffer, SendAncillaryMessage, SendFlags};
    let mut space = [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(16))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    assert!(control.push(SendAncillaryMessage::ScmRights(fds)));
    stream.set_nonblocking(false).unwrap();
    sendmsg(
        stream,
        &[IoSlice::new(bytes)],
        &mut control,
        SendFlags::NOSIGNAL,
    )
    .unwrap();
    stream.set_nonblocking(true).unwrap();
}

#[test]
fn descriptor_abuse_is_refused() {
    let shm_config = Config {
        inline_only: false,
        ..test_config()
    };
    let mut comp = compositor(shm_config, &[(800, 600)]);
    assert!(ultar_hal_gfx::shm::probe(), "Linux test machine has memfd");
    let request = shm_request;
    // Unsealed memory file.
    let mut p = Peer::hello(&mut comp);
    let fd = rustix::fs::memfd_create("t", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
    rustix::fs::ftruncate(&fd, 4096).unwrap();
    send_fds(&p.stream, &request(16, 16, 64), &[fd.as_fd()]);
    p.pump(&mut comp);
    assert_eq!(p.error(), Some(ErrorCode::BadBuffer), "unsealed");
    // Sealed but too small for the announced size.
    let mut q = Peer::hello(&mut comp);
    let small = ultar_hal_gfx::shm::SharedBuffer::create(64).unwrap();
    send_fds(&q.stream, &request(16, 16, 64), &[small.fd()]);
    q.pump(&mut comp);
    assert_eq!(q.error(), Some(ErrorCode::BadBuffer), "too small");
    // A pipe instead of memory.
    let mut r = Peer::hello(&mut comp);
    let (reader, _w) = std::io::pipe().unwrap();
    send_fds(&r.stream, &request(4, 4, 16), &[reader.as_fd()]);
    r.pump(&mut comp);
    assert_eq!(r.error(), Some(ErrorCode::BadBuffer), "pipe");
}

#[test]
fn descriptor_floods_and_lying_strides_are_refused() {
    let mut comp = compositor(
        Config {
            inline_only: false,
            ..test_config()
        },
        &[(800, 600)],
    );
    let request = shm_request;
    let small = ultar_hal_gfx::shm::SharedBuffer::create(64).unwrap();
    // Many descriptors with one message.
    let mut s = Peer::hello(&mut comp);
    let many: Vec<_> = (0..12).map(|_| small.fd()).collect();
    send_fds(
        &s.stream,
        &Request::Pong { serial: 1 }.encode().unwrap(),
        &many,
    );
    s.pump(&mut comp);
    assert_eq!(s.error(), Some(ErrorCode::Malformed), "descriptor flood");
    // Stride lies.
    let mut t = Peer::hello(&mut comp);
    let ok = ultar_hal_gfx::shm::SharedBuffer::create(16 * 16 * 4).unwrap();
    send_fds(&t.stream, &request(16, 16, 8), &[ok.fd()]);
    t.pump(&mut comp);
    assert_eq!(t.error(), Some(ErrorCode::BadBuffer), "stride below a row");
}

#[test]
fn a_proper_shared_buffer_shows_its_pixels() {
    let mut comp = compositor(
        Config {
            inline_only: false,
            ..test_config()
        },
        &[(800, 600)],
    );
    let request = shm_request;
    // The proper way works and shows pixels.
    let mut u = Peer::hello(&mut comp);
    u.send(&Request::CreateSurface {
        surface: SurfaceId(1),
        role: Role::Toplevel,
    });
    u.pump(&mut comp);
    let c = u.last_configure(SurfaceId(1)).unwrap();
    let mut good =
        ultar_hal_gfx::shm::SharedBuffer::create((c.width * c.height * 4) as usize).unwrap();
    good.bytes_mut()
        .chunks_exact_mut(4)
        .for_each(|px| px.copy_from_slice(&0xff12_3456u32.to_le_bytes()));
    send_fds(
        &u.stream,
        &request(c.width, c.height, c.width * 4),
        &[good.fd()],
    );
    u.send(&Request::Attach {
        surface: SurfaceId(1),
        buffer: BufferId(1),
    });
    u.send(&Request::Commit {
        surface: SurfaceId(1),
        configure: c.serial,
    });
    u.pump(&mut comp);
    assert!(u.error().is_none());
    assert!(u
        .events
        .iter()
        .any(|e| matches!(e, Event::BufferReleased { .. })));
    comp.render_now();
    let w = comp
        .state
        .windows
        .iter()
        .find(|w| w.mapped)
        .unwrap()
        .content();
    let frame = comp.frame(OutputId(0)).unwrap();
    assert_eq!(frame.pixel(w.x + 20, w.y + 20), Some(0xff12_3456));
    drop(u);
    comp.dispatch(Some(Duration::ZERO)).unwrap();
    assert!(comp.state.clients.is_empty());
}
