//! Re-review (w4/comprev2, against b8849e3): attacks on the new trusted
//! path – secure attention keys, activation tokens, system dialogs, the
//! popup fix. FINDING tests fail on b8849e3, SOUND ones pass.

mod common;

use common::*;
use ultar_compositor::{Compositor, Config, RightsTable};
use ultar_hal::{InputEventKind, KeyState};
use ultar_surface::proto::{BufferId, Event, PixelFormat, Rect, Request, Rights, Role, SurfaceId};

const APP: u32 = 5000;
const OTHER: u32 = 5001;
const SYSTEM: u32 = 900;

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

fn press(comp: &mut Compositor, code: u16, state: KeyState) {
    comp.inject(&[ev(InputEventKind::Key { code, state })]);
}

fn chord(comp: &mut Compositor, held: u16, code: u16) {
    press(comp, held, KeyState::Pressed);
    press(comp, code, KeyState::Pressed);
    press(comp, code, KeyState::Released);
    press(comp, held, KeyState::Released);
}

/// FINDING N1: with the shipped configuration no client can hold
/// `ui.lock-screen` (the rights file is empty, the shell runs as root and
/// root is refused), yet Super+L and Ctrl+Alt+Del still lock: the screen
/// goes dark, no input goes anywhere, and nothing can ever unlock it –
/// one habitual Super+L ends the session.
#[test]
fn a_secure_attention_key_without_a_lock_screen_does_not_brick_the_session() {
    let mut comp = compositor(test_config(), &[(800, 600)]);
    let mut app = Peer::hello_as(&mut comp, APP);
    app.window(&mut comp, 1, 0xff0000);
    chord(&mut comp, 125, 38); // Super+L
    app.pump(&mut comp);
    app.take();
    key(&mut comp, 30, true);
    key(&mut comp, 30, false);
    app.pump(&mut comp);
    let keys = app
        .events
        .iter()
        .filter(|e| matches!(e, Event::Key { .. }))
        .count();
    assert!(
        !comp.state.locked() && keys > 0,
        "N1: locked={} with no possible lock screen; keys to the app={keys}",
        comp.state.locked()
    );
}

fn token_of(p: &Peer) -> u64 {
    p.events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::ActivationToken { token } => Some(*token),
            _ => None,
        })
        .unwrap_or(0)
}

/// Attacker A is clicked once by the user, then the user works in B.
fn clicked_once() -> (Compositor, Peer, Peer, u32) {
    let mut comp = compositor(test_config(), &[(1280, 800)]);
    let mut a = Peer::hello_as(&mut comp, APP);
    a.window(&mut comp, 1, 0xff0000);
    let c = comp.state.windows[0].content();
    click(&mut comp, f64::from(c.x + 5), f64::from(c.y + 5));
    a.pump(&mut comp);
    let serial = a
        .events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::PointerButton { serial, .. } => Some(*serial),
            _ => None,
        })
        .expect("press delivered");
    let mut b = Peer::hello_as(&mut comp, OTHER);
    b.window(&mut comp, 1, 0x0000ff);
    let kb = comp.state.windows[1].key;
    comp.state.focus_window(kb);
    a.pump(&mut comp);
    b.pump(&mut comp);
    (comp, a, b, serial)
}

/// FINDING N2: one user action buys an unlimited number of focus steals.
/// The serial is not consumed by `RequestActivation`: one click gives
/// 16 tokens, and every token can be parked on an unmapped window
/// (`activate_on_map` never expires) – the app takes the focus whenever it
/// wants, long after the user moved on.
#[test]
fn one_click_buys_one_activation() {
    let (mut comp, mut a, _b, serial) = clicked_once();
    let mut tokens = Vec::new();
    for _ in 0..16 {
        a.send(&Request::RequestActivation { serial });
        a.pump(&mut comp);
        tokens.push(token_of(&a));
    }
    let minted = tokens.iter().filter(|&&t| t != 0).count();
    let mut parked = 0;
    for (i, &t) in tokens.iter().enumerate().filter(|(_, &t)| t != 0) {
        let s = SurfaceId(10 + i as u32);
        a.send(&Request::CreateSurface {
            surface: s,
            role: Role::Toplevel,
        });
        a.send(&Request::Activate {
            surface: s,
            token: t,
        });
        a.pump(&mut comp);
        parked += 1;
    }
    // Much later (tokens long expired), one parked window maps.
    comp.state.now += 3_600_000;
    a.draw(&mut comp, SurfaceId(10), 0x00ff00);
    let stolen = comp.state.focus.map(|k| k.client) == Some(comp.state.windows[0].key.client);
    assert!(
        minted <= 1 && !stolen,
        "N2: {minted} tokens from one click, {parked} parked; focus taken an hour later={stolen}"
    );
}

fn translucent_popup(p: &mut Peer, comp: &mut Compositor, pixel: u32) {
    let s = SurfaceId(2);
    let role = Role::Popup {
        parent: SurfaceId(1),
        x: -5000,
        y: -5000,
    };
    p.send(&Request::CreateSurface { surface: s, role });
    p.send(&Request::CreateInlineBuffer {
        buffer: BufferId(77),
        width: 1280,
        height: 800,
        format: PixelFormat::Argb8888,
    });
    let data: Vec<u8> = pixel
        .to_le_bytes()
        .iter()
        .copied()
        .cycle()
        .take(1280 * 800 * 4)
        .collect();
    for (i, rows) in data.chunks(1280 * 4 * 200).enumerate() {
        p.send(&Request::WritePixels {
            buffer: BufferId(77),
            rect: Rect::new(0, i as i32 * 200, 1280, 200),
            data: rows.to_vec(),
        });
        p.pump(comp);
    }
    p.send(&Request::Attach {
        surface: s,
        buffer: BufferId(77),
    });
    p.send(&Request::Commit {
        surface: s,
        configure: 0,
    });
    p.pump(comp);
}

/// FINDING N3: the C3 fix skips only alpha 0. A popup of alpha 1/255
/// (invisible to the eye) over the work area still takes every click on
/// the other windows below its (top) parent.
#[test]
fn a_nearly_invisible_popup_cannot_take_other_apps_clicks() {
    let mut comp = compositor(test_config(), &[(1280, 800)]);
    let mut victim = Peer::hello_as(&mut comp, OTHER);
    victim.window(&mut comp, 1, 0x00ff00);
    let content = comp.state.windows[0].content();
    let mut spy = Peer::hello_as(&mut comp, APP);
    spy.window(&mut comp, 1, 0xff0000);
    let ks = comp.state.windows[1].key;
    comp.state.focus_window(ks);
    translucent_popup(&mut spy, &mut comp, 0x0100_0000);
    victim.take();
    spy.take();
    click(
        &mut comp,
        f64::from(content.x + 20),
        f64::from(content.y + 20),
    );
    victim.pump(&mut comp);
    spy.pump(&mut comp);
    let buttons = |p: &Peer| {
        p.events
            .iter()
            .filter(|e| matches!(e, Event::PointerButton { .. }))
            .count()
    };
    assert!(
        buttons(&victim) > 0 && buttons(&spy) == 0,
        "N3: click on the victim went to victim={} spy={}",
        buttons(&victim),
        buttons(&spy)
    );
}

/// FINDING N4: overlays (`ui.overlay`, a lower right than
/// `ui.system-dialog`, e.g. a notification service) are stacked above the
/// system dialog, are not dimmed and take its clicks: they can cover the
/// trusted dialog's text or buttons.
#[test]
fn an_overlay_cannot_cover_a_system_dialog() {
    let grants = [(SYSTEM, Rights::SYSTEM_DIALOG), (APP, Rights::OVERLAY)];
    let mut comp = compositor(config(&grants), &[(800, 600)]);
    let mut dialog = Peer::hello_as(&mut comp, SYSTEM);
    dialog.window_role(&mut comp, 1, Role::SystemDialog, 0xffffff);
    let d = comp.state.windows[0].content();
    let mut notifier = Peer::hello_as(&mut comp, APP);
    notifier.send(&Request::CreateSurface {
        surface: SurfaceId(1),
        role: Role::Overlay,
    });
    notifier.pump(&mut comp);
    let serial = notifier.last_configure(SurfaceId(1)).unwrap().serial;
    // Overlays are sized by their client (no clamp): cover the screen.
    notifier.fill(&mut comp, SurfaceId(1), serial, (800, 600), 0xff00ff);
    let o = comp.state.layer_rect(comp.state.layers[0]).unwrap();
    let both = d.intersect(&o);
    comp.render_now();
    let covered = both.is_some_and(|r| {
        let hit = comp.state.hit_test(f64::from(r.x + 1), f64::from(r.y + 1));
        hit.is_some_and(|(k, _, _)| k.client != comp.state.windows[0].key.client)
    });
    assert!(
        both.is_none() || !covered,
        "N4: overlay {o:?} covers system dialog {d:?} and takes its clicks"
    );
}

/// SOUND: the first rows a capture client receives come from a frame that
/// already shows the recording indicator (it was presented before any
/// pixel left the compositor).
#[test]
fn capture_data_never_precedes_the_indicator() {
    let mut comp = compositor(config(&[(APP, Rights::CAPTURE)]), &[(640, 480)]);
    let mut app = Peer::hello_as(&mut comp, OTHER);
    app.window(&mut comp, 1, 0x202020);
    comp.render_now();
    let o = comp.state.layout.primary().unwrap().clone();
    let r = ultar_compositor::cursor::indicator_rect(&o, comp.state.rtl());
    let (cx, cy) = ((r.x + r.w / 2) as usize, (r.y + r.h / 2) as usize);
    let before = comp.frame(o.id).unwrap().data()[cy * 640 + cx];
    let mut cap = Peer::hello_as(&mut comp, APP);
    cap.send(&Request::Capture { output: o.id });
    for _ in 0..20 {
        cap.pump(&mut comp);
    }
    let first = cap
        .events
        .iter()
        .find_map(|e| match e {
            Event::CaptureData { row: 0, data, .. } => Some(data.clone()),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "no capture data: {:?}",
                cap.events
                    .iter()
                    .map(|e| format!("{e:?}").chars().take(60).collect::<String>())
                    .collect::<Vec<_>>()
            )
        });
    let i = (cy * 640 + cx) * 4;
    let px = u32::from_le_bytes([first[i], first[i + 1], first[i + 2], first[i + 3]]);
    assert_ne!(px & 0xff_ffff, before & 0xff_ffff, "indicator missing");
}
