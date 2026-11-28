//! Fuzzing with a fixed seed: connections send well-formed but random
//! requests (random objects, values, strings, arrays, descriptors) and
//! random bytes. Wrong requests may end their own connection, never the
//! bridge: afterwards a fresh connection still gets its window shown.

mod common;

use common::app::App;
use common::wl::Arg;
use common::{Harness, Rng};
use std::os::fd::AsFd;
use std::time::Duration;
use wayland_server::backend::protocol::{ArgumentType, Interface, MessageDesc};
use wayland_server::Resource;

const SEED: u64 = 0x0057_4C46_5A5A_2026;
const ROUNDS: usize = 200;
const STEPS: usize = 400;
/// Ids up to this are the globals bound by `App::connect`.
const GLOBALS_UP_TO: u32 = 8;
const WORDS: [u32; 10] = [
    0,
    1,
    2,
    3,
    7,
    0xFFFF_FFFF,
    0x7FFF_FFFF,
    0x8000_0000,
    4096,
    65_536,
];
const TEXTS: [&str; 6] = [
    "",
    "a",
    "مرحبا بالعالم",
    "text/plain;charset=utf-8",
    "\u{202E}evil\u{202C}",
    "Ünïcødé ✓",
];

fn interfaces(app: &App) -> Vec<(u32, &'static Interface)> {
    use wayland_protocols::xdg::shell::server::xdg_wm_base::XdgWmBase;
    use wayland_server::protocol::{
        wl_compositor::WlCompositor, wl_data_device_manager::WlDataDeviceManager, wl_seat::WlSeat,
        wl_shm::WlShm,
    };
    vec![
        (app.compositor, WlCompositor::interface()),
        (app.shm, WlShm::interface()),
        (app.wm, XdgWmBase::interface()),
        (app.seat, WlSeat::interface()),
        (app.ddm, WlDataDeviceManager::interface()),
    ]
}

fn word(rng: &mut Rng) -> u32 {
    match rng.below(10) {
        0 => WORDS[rng.below(WORDS.len() as u64) as usize],
        1..=4 => rng.below(4) as u32,
        _ => rng.below(600) as u32,
    }
}

type Objects = Vec<(u32, &'static Interface)>;

/// A value of one argument.
enum Val {
    W(u32),
    S(String),
    A(Vec<u8>),
}

/// Mostly an object of the wanted kind (that goes deeper), now and then any.
fn object(rng: &mut Rng, objects: &Objects, wanted: Option<&&'static Interface>) -> u32 {
    let name = wanted.map_or("", |i| i.name);
    let fitting: Vec<u32> = objects
        .iter()
        .filter(|(_, i)| i.name == name)
        .map(|(id, _)| *id)
        .collect();
    match (fitting.len(), objects.len(), rng.below(16)) {
        (n, _, 1..) if n > 0 => fitting[rng.below(n as u64) as usize],
        (_, n, 0) if n > 0 => objects[rng.below(n as u64) as usize].0,
        _ => 0,
    }
}

fn text(rng: &mut Rng) -> String {
    match rng.below(10) {
        0 => "x".repeat(rng.below(5000) as usize),
        _ => TEXTS[rng.below(TEXTS.len() as u64) as usize].to_owned(),
    }
}

/// Random values for a request (`None` in place of the descriptor).
fn values(
    app: &mut App,
    rng: &mut Rng,
    objects: &mut Objects,
    desc: &MessageDesc,
) -> Vec<Option<Val>> {
    let mut wanted = desc.arg_interfaces.iter();
    let mut out = Vec::new();
    for t in desc.signature {
        out.push(match t {
            ArgumentType::Int | ArgumentType::Uint | ArgumentType::Fixed => Some(Val::W(word(rng))),
            ArgumentType::Object(_) => Some(Val::W(object(rng, objects, wanted.next()))),
            ArgumentType::NewId => {
                let id = app.wl.new_id();
                if let Some(child) = desc.child_interface {
                    objects.push((id, child));
                }
                Some(Val::W(id))
            }
            ArgumentType::Str(_) => Some(Val::S(text(rng))),
            ArgumentType::Array => Some(Val::A(
                (0..rng.below(40)).map(|_| rng.next() as u8).collect(),
            )),
            ArgumentType::Fd => None,
        });
    }
    out
}

/// One random request on a random known object.
fn request(app: &mut App, rng: &mut Rng, objects: &mut Objects, pool: &std::os::fd::OwnedFd) {
    let (obj, iface) = objects[rng.below(objects.len() as u64) as usize];
    if iface.requests.is_empty() {
        return;
    }
    let op = rng.below(iface.requests.len() as u64) as usize;
    let desc = &iface.requests[op];
    // The globals stay (else most connections end at once); other objects
    // are destroyed now and then.
    if desc.is_destructor && (obj <= GLOBALS_UP_TO || rng.below(3) != 0) {
        return;
    }
    let vals = values(app, rng, objects, desc);
    let fd = vals.iter().any(Option::is_none).then(|| pool.as_fd());
    let args: Vec<Arg<'_>> = vals
        .iter()
        .flatten()
        .map(|v| match v {
            Val::W(w) => Arg::U(*w),
            Val::S(s) => Arg::S(s.as_str()),
            Val::A(a) => Arg::A(a.as_slice()),
        })
        .collect();
    app.wl.send_fd(obj, op as u16, &args, fd);
    if desc.is_destructor && rng.below(16) != 0 {
        objects.retain(|(id, _)| *id != obj);
    }
}

/// Bytes that are no message at all (or a message about a lie).
fn garbage(app: &mut App, rng: &mut Rng) {
    let n = 8 + rng.below(64) as usize;
    let mut bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
    if rng.below(2) == 0 {
        // A plausible header with a wrong size.
        let obj = 1 + rng.below(12) as u32;
        let size_op = ((rng.below(0x1_0000) as u32) << 16) | rng.below(12) as u32;
        bytes[..4].copy_from_slice(&obj.to_ne_bytes());
        bytes[4..8].copy_from_slice(&size_op.to_ne_bytes());
    }
    app.wl.raw(&bytes, None);
}

/// A shown window and the input objects join the targets (commit,
/// configure, input and the compositor side get their share).
fn with_window(h: &mut Harness, app: &mut App, objects: &mut Objects) {
    use wayland_protocols::xdg::shell::server::{
        xdg_surface::XdgSurface, xdg_toplevel::XdgToplevel,
    };
    use wayland_server::protocol::{
        wl_data_device::WlDataDevice, wl_keyboard::WlKeyboard, wl_pointer::WlPointer,
        wl_surface::WlSurface,
    };
    let win = app.window(h, "fuzz", (64, 64), 0xFF10_2030);
    objects.extend([
        (win.surface, WlSurface::interface()),
        (win.xdg, XdgSurface::interface()),
        (win.top, XdgToplevel::interface()),
        (app.pointer, WlPointer::interface()),
        (app.keyboard, WlKeyboard::interface()),
        (app.device, WlDataDevice::interface()),
    ]);
}

/// One connection's random requests; whether the bridge ended it.
fn round(
    h: &mut Harness,
    rng: &mut Rng,
    n: usize,
    pool: &std::os::fd::OwnedFd,
    sent: &mut usize,
) -> bool {
    let mut app = App::connect(h);
    app.input();
    let mut objects = interfaces(&app);
    if n % 2 == 1 {
        with_window(h, &mut app, &mut objects);
    }
    for step in 0..STEPS {
        if n % 6 == 5 && step == STEPS / 2 {
            garbage(&mut app, rng);
        } else {
            request(&mut app, rng, &mut objects, pool);
        }
        *sent += 1;
        if step % 16 == 0 {
            app.wl.read();
            h.pump(1);
        }
        if app.wl.dead || app.wl.error().is_some() || objects.is_empty() {
            return true;
        }
    }
    false
}

#[test]
fn random_requests_never_end_the_bridge() {
    let mut h = Harness::start("fuzz", &["sleep", "120"]);
    let mut rng = Rng(SEED);
    let pool = common::app::filled(64, 64, 0xFF10_2030);
    let (mut sent, mut ended) = (0usize, 0usize);
    for n in 0..ROUNDS {
        ended += usize::from(round(&mut h, &mut rng, n, &pool, &mut sent));
        h.pump(5);
        assert!(h.alive(), "the bridge survived round {n}");
    }
    let mut app = App::connect(&mut h);
    let _win = app.window(&mut h, "after fuzz", (160, 100), 0xFF44_8899);
    assert!(app.roundtrip(&mut h));
    let shown = h.pump_until(Duration::from_secs(5), |h| {
        h.find(0xFF44_8899).0 >= 160 * 100 * 9 / 10
    });
    assert!(shown, "a fresh window after the fuzzing");
    assert!(h.alive());
    println!("fuzz: {ROUNDS} connections, {sent} requests, {ended} ended by the bridge with a protocol error");
}
