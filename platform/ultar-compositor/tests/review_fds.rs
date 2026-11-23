//! Descriptor leak check of the independent review (w4/comprev). Alone in
//! its own test binary: `/proc/self/fd` is process-wide, parallel tests
//! would disturb the count.

mod common;

use common::*;
use ultar_compositor::Compositor;
use ultar_surface::proto::Request;

fn open_fds() -> usize {
    std::fs::read_dir("/proc/self/fd").unwrap().count()
}

/// One shared-memory client: a real buffer plus `extra` unclaimed
/// descriptors, then it disappears.
fn shm_client(comp: &mut Compositor, extra: usize) {
    use std::os::fd::AsFd;
    let mut p = Peer::hello(comp);
    let buffer = ultar_hal_gfx::shm::SharedBuffer::create(64 * 64 * 4).unwrap();
    let frame = Request::CreateShmBuffer {
        buffer: ultar_surface::proto::BufferId(1),
        width: 64,
        height: 64,
        stride: 256,
        format: ultar_surface::proto::PixelFormat::Xrgb8888,
    }
    .encode()
    .unwrap();
    ultar_hal_gfx::fdpass::send(&p.stream, &frame, Some(buffer.fd())).unwrap();
    let (pipe, _w) = std::io::pipe().unwrap();
    for _ in 0..extra {
        ultar_hal_gfx::fdpass::send(&p.stream, &[], Some(pipe.as_fd())).ok();
    }
    p.pump(comp);
    drop(p);
    comp.dispatch(Some(std::time::Duration::ZERO)).unwrap();
}

/// SOUND: descriptors of buffers, unclaimed descriptors and descriptors of
/// clients that were dropped for abuse are all closed.
#[test]
fn no_descriptor_leaks() {
    let config = ultar_compositor::Config {
        inline_only: false,
        ..test_config()
    };
    let mut comp = compositor(config, &[(320, 200)]);
    shm_client(&mut comp, 0);
    let before = open_fds();
    for i in 0..40 {
        shm_client(&mut comp, i % 7);
    }
    comp.dispatch(Some(std::time::Duration::ZERO)).unwrap();
    assert_eq!(comp.state.clients.len(), 0);
    let list = || {
        let mut v: Vec<String> = std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(|e| std::fs::read_link(e.ok()?.path()).ok())
            .map(|p| p.display().to_string())
            .collect();
        v.sort();
        v
    };
    assert_eq!(open_fds(), before, "descriptors leaked: {:?}", list());
}
