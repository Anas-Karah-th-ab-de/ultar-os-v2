//! A tiny Wayland app without any library (the wire format by hand), for
//! runs where no Ubuntu app exists (the Ultar kernel in QEMU): one window of
//! colour #3366CC that answers pings and configures, until it is closed.
//!
//!     uwl-demo [width height]

#[path = "../tests/common/wl.rs"]
mod wl;

use rustix::fs::{ftruncate, memfd_create, MemfdFlags};
use std::os::fd::{AsFd, OwnedFd};
use wl::{Arg, Args, Wl};

const COLOR: u32 = 0xFF33_66CC;

struct Ids {
    compositor: u32,
    shm: u32,
    wm: u32,
}

fn bind_globals(wl: &mut Wl) -> Option<Ids> {
    let reg = wl.new_id();
    wl.send(1, 1, &[Arg::U(reg)]);
    let cb = wl.new_id();
    wl.send(1, 0, &[Arg::U(cb)]);
    while !wl.has(cb, 0) && !wl.dead {
        std::thread::sleep(std::time::Duration::from_millis(5));
        wl.read();
    }
    let mut ids = Ids {
        compositor: 0,
        shm: 0,
        wm: 0,
    };
    for e in wl.take(reg, 0) {
        let mut a = Args::of(&e);
        let (name, iface, version) = (a.u(), a.s(), a.u());
        let (slot, want) = match iface.as_str() {
            "wl_compositor" => (&mut ids.compositor, 4),
            "wl_shm" => (&mut ids.shm, 1),
            "xdg_wm_base" => (&mut ids.wm, 1),
            _ => continue,
        };
        let id = wl.new_id();
        wl.send(
            reg,
            0,
            &[
                Arg::U(name),
                Arg::S(&iface),
                Arg::U(want.min(version)),
                Arg::U(id),
            ],
        );
        *slot = id;
    }
    (ids.compositor != 0 && ids.shm != 0 && ids.wm != 0).then_some(ids)
}

fn filled(w: i32, h: i32) -> Option<OwnedFd> {
    let fd = memfd_create("uwl-demo", MemfdFlags::CLOEXEC).ok()?;
    ftruncate(&fd, (w * h * 4) as u64).ok()?;
    let row: Vec<u8> = std::iter::repeat_n(COLOR.to_le_bytes(), w as usize)
        .flatten()
        .collect();
    for y in 0..h {
        rustix::io::pwrite(&fd, &row, (y * w * 4) as u64).ok()?;
    }
    Some(fd)
}

fn paint(wl: &mut Wl, ids: &Ids, surface: u32, (w, h): (i32, i32)) {
    let Some(fd) = filled(w, h) else { return };
    let pool = wl.new_id();
    wl.send_fd(
        ids.shm,
        0,
        &[Arg::U(pool), Arg::I(w * h * 4)],
        Some(fd.as_fd()),
    );
    let buffer = wl.new_id();
    wl.send(
        pool,
        0,
        &[
            Arg::U(buffer),
            Arg::I(0),
            Arg::I(w),
            Arg::I(h),
            Arg::I(w * 4),
            Arg::U(0),
        ],
    );
    wl.send(pool, 1, &[]);
    wl.send(surface, 1, &[Arg::U(buffer), Arg::I(0), Arg::I(0)]);
    wl.send(surface, 2, &[Arg::I(0), Arg::I(0), Arg::I(w), Arg::I(h)]);
    wl.send(surface, 6, &[]);
}

fn main() {
    let args: Vec<i32> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let size = (
        args.first().copied().unwrap_or(480).clamp(16, 4096),
        args.get(1).copied().unwrap_or(320).clamp(16, 4096),
    );
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let name = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
    let mut wl = Wl::connect(&std::path::Path::new(&dir).join(name));
    let Some(ids) = bind_globals(&mut wl) else {
        eprintln!("uwl-demo: wl_compositor, wl_shm or xdg_wm_base missing");
        std::process::exit(1);
    };
    let surface = wl.new_id();
    wl.send(ids.compositor, 0, &[Arg::U(surface)]);
    let xdg = wl.new_id();
    wl.send(ids.wm, 2, &[Arg::U(xdg), Arg::U(surface)]);
    let top = wl.new_id();
    wl.send(xdg, 1, &[Arg::U(top)]);
    wl.send(top, 2, &[Arg::S("uwl-demo")]);
    wl.send(surface, 6, &[]);
    run(&mut wl, &ids, (surface, xdg, top), size);
}

/// Answers pings and configures until the window is closed.
fn run(wl: &mut Wl, ids: &Ids, (surface, xdg, top): (u32, u32, u32), mut size: (i32, i32)) {
    println!("uwl-demo: window requested");
    while !wl.dead {
        wl.read();
        for e in wl.take(ids.wm, 0) {
            wl.send(ids.wm, 3, &[Arg::U(Args::of(&e).u())]);
        }
        for e in wl.take(top, 0) {
            let mut a = Args::of(&e);
            let (w, h) = (a.i(), a.i());
            if w > 0 && h > 0 {
                size = (w.min(4096), h.min(4096));
            }
        }
        if let Some(e) = wl.take(xdg, 0).last() {
            wl.send(xdg, 4, &[Arg::U(Args::of(e).u())]);
            paint(wl, ids, surface, size);
            println!("uwl-demo: drawn {}x{}", size.0, size.1);
        }
        if !wl.take(top, 1).is_empty() {
            println!("uwl-demo: closed");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
