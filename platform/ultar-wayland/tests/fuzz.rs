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
use wayland_server::backend::protocol::{ArgumentType, Interface};
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

/// One random request on a random known object.
fn request(
    app: &mut App,
    rng: &mut Rng,
    objects: &mut Vec<(u32, &'static Interface)>,
    pool: &std::os::fd::OwnedFd,
) {
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
    let mut words: Vec<u32> = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    let mut arrays: Vec<Vec<u8>> = Vec::new();
    let mut kinds = Vec::new();
    let mut fd = false;
    let mut wanted = desc.arg_interfaces.iter();
    for t in desc.signature {
        match t {
            ArgumentType::Int | ArgumentType::Uint | ArgumentType::Fixed => {
                words.push(word(rng));
                kinds.push(0);
            }
            ArgumentType::Object(_) => {
                // Mostly an object of the right kind (deeper), now and then any.
                let name = wanted.next().map_or("", |i| i.name);
                let fitting: Vec<u32> = objects
                    .iter()
                    .filter(|(_, i)| i.name == name)
                    .map(|(id, _)| *id)
                    .collect();
                let pick = match (fitting.len(), objects.len(), rng.below(16)) {
                    (n, _, 1..) if n > 0 => fitting[rng.below(n as u64) as usize],
                    (_, n, 0) if n > 0 => objects[rng.below(n as u64) as usize].0,
                    _ => 0,
                };
                words.push(pick);
                kinds.push(0);
            }
            ArgumentType::NewId => {
                let id = app.wl.new_id();
                if let Some(child) = desc.child_interface {
                    objects.push((id, child));
                }
                words.push(id);
                kinds.push(0);
            }
            ArgumentType::Str(_) => {
                let t = if rng.below(10) == 0 {
                    "x".repeat(rng.below(5000) as usize)
                } else {
                    TEXTS[rng.below(TEXTS.len() as u64) as usize].to_owned()
                };
                texts.push(t);
                kinds.push(1);
            }
            ArgumentType::Array => {
                arrays.push((0..rng.below(40)).map(|_| rng.next() as u8).collect());
                kinds.push(2);
            }
            ArgumentType::Fd => fd = true,
        }
    }
    let (mut w, mut t, mut a) = (words.iter(), texts.iter(), arrays.iter());
    let args: Vec<Arg<'_>> = kinds
        .iter()
        .filter_map(|k| match k {
            0 => w.next().map(|v| Arg::U(*v)),
            1 => t.next().map(|s| Arg::S(s.as_str())),
            _ => a.next().map(|v| Arg::A(v.as_slice())),
        })
        .collect();
    let fd = fd.then(|| pool.as_fd());
    if std::env::var_os("FUZZ_WHY").is_some() {
        println!(
            "  {}@{obj}.{} {:?} {:?}",
            iface.name,
            desc.name,
            words,
            texts.iter().map(|t| t.len()).collect::<Vec<_>>()
        );
    }
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

#[test]
fn random_requests_never_end_the_bridge() {
    let mut h = Harness::start("fuzz", &["sleep", "120"]);
    let mut rng = Rng(SEED);
    let pool = common::app::filled(64, 64, 0xFF10_2030);
    let (mut sent, mut ended) = (0usize, 0usize);
    for round in 0..ROUNDS {
        let mut app = App::connect(&mut h);
        app.input();
        let mut objects = interfaces(&app);
        if round % 2 == 1 {
            // Half the rounds start from a shown window (commit, configure,
            // input and the compositor side get their share).
            use wayland_protocols::xdg::shell::server::{
                xdg_surface::XdgSurface, xdg_toplevel::XdgToplevel,
            };
            use wayland_server::protocol::{
                wl_data_device::WlDataDevice, wl_keyboard::WlKeyboard, wl_pointer::WlPointer,
                wl_surface::WlSurface,
            };
            let win = app.window(&mut h, "fuzz", (64, 64), 0xFF10_2030);
            objects.extend([
                (win.surface, WlSurface::interface()),
                (win.xdg, XdgSurface::interface()),
                (win.top, XdgToplevel::interface()),
                (app.pointer, WlPointer::interface()),
                (app.keyboard, WlKeyboard::interface()),
                (app.device, WlDataDevice::interface()),
            ]);
        }
        for step in 0..STEPS {
            if round % 6 == 5 && step == STEPS / 2 {
                garbage(&mut app, &mut rng);
            } else {
                request(&mut app, &mut rng, &mut objects, &pool);
            }
            sent += 1;
            if step % 16 == 0 {
                app.wl.read();
                h.pump(1);
            }
            if app.wl.dead || app.wl.error().is_some() || objects.is_empty() {
                if std::env::var_os("FUZZ_WHY").is_some() {
                    println!("round {round} step {step}: {:?}", app.wl.error());
                }
                ended += 1;
                break;
            }
        }
        h.pump(5);
        assert!(h.alive(), "the bridge survived round {round}");
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
