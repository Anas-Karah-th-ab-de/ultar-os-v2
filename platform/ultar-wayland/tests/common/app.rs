//! A Wayland app built from the raw client: globals, a window with a solid
//! colour, input objects, the clipboard.
#![allow(dead_code)]

use super::wl::{Arg, Args, Wl};
use super::Harness;
use rustix::fs::{ftruncate, memfd_create, MemfdFlags};
use std::os::fd::{AsFd, OwnedFd};
use std::time::Duration;

/// Opcodes used by the tests.
pub mod op {
    pub const SYNC: u16 = 0;
    pub const GET_REGISTRY: u16 = 1;
    pub const BIND: u16 = 0;
    pub const CREATE_SURFACE: u16 = 0;
    pub const ATTACH: u16 = 1;
    pub const DAMAGE: u16 = 2;
    pub const FRAME: u16 = 3;
    pub const COMMIT: u16 = 6;
    pub const SET_BUFFER_SCALE: u16 = 8;
    pub const CREATE_POOL: u16 = 0;
    pub const CREATE_BUFFER: u16 = 0;
    pub const CREATE_POSITIONER: u16 = 1;
    pub const GET_XDG_SURFACE: u16 = 2;
    pub const PONG: u16 = 3;
    pub const GET_TOPLEVEL: u16 = 1;
    pub const GET_POPUP: u16 = 2;
    pub const SET_GEOMETRY: u16 = 3;
    pub const ACK: u16 = 4;
    pub const SET_TITLE: u16 = 2;
    pub const SET_MAXIMIZED: u16 = 9;
    pub const DESTROY: u16 = 0;
    pub const POOL_DESTROY: u16 = 1;
    pub const POS_SET_SIZE: u16 = 1;
    pub const POS_ANCHOR_RECT: u16 = 2;
    pub const POS_ANCHOR: u16 = 3;
    pub const POS_GRAVITY: u16 = 4;
    pub const POPUP_GRAB: u16 = 1;
    pub const GET_POINTER: u16 = 0;
    pub const GET_KEYBOARD: u16 = 1;
    pub const CREATE_SOURCE: u16 = 0;
    pub const GET_DEVICE: u16 = 1;
    pub const SOURCE_OFFER: u16 = 0;
    pub const SET_SELECTION: u16 = 1;
    pub const OFFER_RECEIVE: u16 = 1;
}

/// The app.
pub struct App {
    pub wl: Wl,
    pub globals: Vec<(u32, String, u32)>,
    pub compositor: u32,
    pub shm: u32,
    pub wm: u32,
    pub seat: u32,
    pub ddm: u32,
    pub pointer: u32,
    pub keyboard: u32,
    pub device: u32,
}

/// A window.
pub struct Win {
    pub surface: u32,
    pub xdg: u32,
    pub top: u32,
    pub size: (i32, i32),
}

impl App {
    /// Connects and binds the globals.
    pub fn connect(h: &mut Harness) -> App {
        let mut wl = Wl::connect(&h.socket);
        let reg = wl.new_id();
        wl.send(1, op::GET_REGISTRY, &[Arg::U(reg)]);
        let mut app = App {
            wl,
            globals: Vec::new(),
            compositor: 0,
            shm: 0,
            wm: 0,
            seat: 0,
            ddm: 0,
            pointer: 0,
            keyboard: 0,
            device: 0,
        };
        assert!(app.roundtrip(h), "the bridge answers");
        for e in app.wl.take(reg, 0) {
            let mut a = Args::of(&e);
            let (name, iface, ver) = (a.u(), a.s(), a.u());
            app.globals.push((name, iface, ver));
        }
        app.compositor = app.bind(reg, "wl_compositor", 6);
        app.shm = app.bind(reg, "wl_shm", 1);
        app.wm = app.bind(reg, "xdg_wm_base", 5);
        app.seat = app.bind(reg, "wl_seat", 7);
        app.ddm = app.bind(reg, "wl_data_device_manager", 3);
        app
    }

    pub fn bind(&mut self, reg: u32, iface: &str, version: u32) -> u32 {
        let Some(&(name, _, max)) = self.globals.iter().find(|g| g.1 == iface) else {
            panic!("global {iface} missing");
        };
        let id = self.wl.new_id();
        self.wl.send(
            reg,
            op::BIND,
            &[
                Arg::U(name),
                Arg::S(iface),
                Arg::U(version.min(max)),
                Arg::U(id),
            ],
        );
        id
    }

    /// wl_display.sync and wait for it.
    pub fn roundtrip(&mut self, h: &mut Harness) -> bool {
        let cb = self.wl.new_id();
        self.wl.send(1, op::SYNC, &[Arg::U(cb)]);
        self.until(h, Duration::from_secs(5), |w| w.has(cb, 0))
    }

    /// Pumps compositor and socket until `cond`.
    pub fn until(
        &mut self,
        h: &mut Harness,
        timeout: Duration,
        mut cond: impl FnMut(&Wl) -> bool,
    ) -> bool {
        let wl = &mut self.wl;
        h.pump_until(timeout, |_| {
            wl.read();
            cond(wl) || wl.dead
        }) && !wl.dead
    }

    /// Pointer, keyboard and data device.
    pub fn input(&mut self) {
        self.pointer = self.wl.new_id();
        self.wl
            .send(self.seat, op::GET_POINTER, &[Arg::U(self.pointer)]);
        self.keyboard = self.wl.new_id();
        self.wl.fd_events.push((self.keyboard, 0));
        self.wl
            .send(self.seat, op::GET_KEYBOARD, &[Arg::U(self.keyboard)]);
        self.device = self.wl.new_id();
        self.wl.send(
            self.ddm,
            op::GET_DEVICE,
            &[Arg::U(self.device), Arg::U(self.seat)],
        );
    }

    /// A toplevel: initial commit, configure, a buffer of `color`.
    pub fn window(&mut self, h: &mut Harness, title: &str, size: (i32, i32), color: u32) -> Win {
        let surface = self.wl.new_id();
        self.wl
            .send(self.compositor, op::CREATE_SURFACE, &[Arg::U(surface)]);
        let xdg = self.wl.new_id();
        self.wl.send(
            self.wm,
            op::GET_XDG_SURFACE,
            &[Arg::U(xdg), Arg::U(surface)],
        );
        let top = self.wl.new_id();
        self.wl.send(xdg, op::GET_TOPLEVEL, &[Arg::U(top)]);
        self.wl.send(top, op::SET_TITLE, &[Arg::S(title)]);
        self.wl.send(surface, op::COMMIT, &[]);
        assert!(
            self.until(h, Duration::from_secs(5), |w| w.has(xdg, 0)),
            "first configure"
        );
        let mut win = Win {
            surface,
            xdg,
            top,
            size,
        };
        self.ack_latest(&mut win);
        self.paint(&win, size, color);
        win
    }

    /// Acknowledges the newest configure; takes its size if it gives one.
    pub fn ack_latest(&mut self, win: &mut Win) -> Vec<u32> {
        let mut states = Vec::new();
        if let Some(e) = self.wl.take(win.top, 0).last() {
            let mut a = Args::of(e);
            let (w, h) = (a.i(), a.i());
            states = a
                .a()
                .chunks(4)
                .map(|c| u32::from_ne_bytes(c.try_into().unwrap()))
                .collect();
            if w > 0 && h > 0 {
                win.size = (w, h);
            }
        }
        if let Some(e) = self.wl.take(win.xdg, 0).last() {
            let serial = Args::of(e).u();
            self.wl.send(win.xdg, op::ACK, &[Arg::U(serial)]);
        }
        states
    }

    /// A new buffer of `size` filled with `color`, attached and committed.
    pub fn paint(&mut self, win: &Win, size: (i32, i32), color: u32) {
        self.paint_surface(win.surface, size, color);
    }

    pub fn paint_surface(&mut self, surface: u32, size: (i32, i32), color: u32) {
        let (w, h) = size;
        let fd = filled(w, h, color);
        let pool = self.wl.new_id();
        self.wl.send_fd(
            self.shm,
            op::CREATE_POOL,
            &[Arg::U(pool), Arg::I(w * h * 4)],
            Some(fd.as_fd()),
        );
        let buffer = self.wl.new_id();
        self.wl.send(
            pool,
            op::CREATE_BUFFER,
            &[
                Arg::U(buffer),
                Arg::I(0),
                Arg::I(w),
                Arg::I(h),
                Arg::I(w * 4),
                Arg::U(0),
            ],
        );
        self.wl.send(pool, op::POOL_DESTROY, &[]);
        self.wl
            .send(surface, op::ATTACH, &[Arg::U(buffer), Arg::I(0), Arg::I(0)]);
        self.wl.send(
            surface,
            op::DAMAGE,
            &[Arg::I(0), Arg::I(0), Arg::I(w), Arg::I(h)],
        );
        self.wl.send(surface, op::COMMIT, &[]);
    }
}

/// A memory file of `w × h` pixels of `color` (ARGB8888).
pub fn filled(w: i32, h: i32, color: u32) -> OwnedFd {
    let fd = memfd_create("t", MemfdFlags::CLOEXEC).unwrap();
    ftruncate(&fd, (w * h * 4) as u64).unwrap();
    let bytes: Vec<u8> = std::iter::repeat_n(color.to_le_bytes(), (w * h) as usize)
        .flatten()
        .collect();
    rustix::io::pwrite(&fd, &bytes, 0).unwrap();
    fd
}
