//! Regression tests of the independent robustness review (w4/comprev,
//! against 0f4846e): pacing, flow control, outputs, damage. Tests marked
//! FINDING fail on 0f4846e, SOUND ones pass.

mod common;

use common::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use ultar_compositor::backend::Headless;
use ultar_compositor::output::DisplayMode;
use ultar_compositor::{Clock, Compositor};
use ultar_surface::proto::{Event, OutputId, Request, SurfaceId, TransferId};

/// A clock the test moves by hand.
struct Manual(Arc<AtomicU64>);

impl Clock for Manual {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn manual_compositor(sizes: &[(u32, u32)]) -> (Compositor, Arc<AtomicU64>) {
    let time = Arc::new(AtomicU64::new(1_000));
    let comp = Compositor::new(
        test_config(),
        Box::new(Headless::new(sizes)),
        Box::new(Manual(time.clone())),
    );
    (comp, time)
}

fn count(p: &mut Peer, f: impl Fn(&Event) -> bool) -> usize {
    p.take().iter().filter(|e| f(e)).count()
}

/// FINDING M4: a frame callback on an output without damage is answered
/// at once and `last_present` is not moved, so a client that only sends
/// `Frame` gets `FrameDone` as fast as it can ask – no pacing, the
/// compositor spins (and a side channel on other clients' damage).
#[test]
fn frame_callbacks_without_damage_are_paced() {
    let (mut comp, time) = manual_compositor(&[(800, 600)]);
    let mut a = Peer::hello(&mut comp);
    let s = a.window(&mut comp, 1, 0xff0000);
    comp.render_now();
    time.fetch_add(20, Ordering::SeqCst);
    a.take();
    let mut done = 0;
    for _ in 0..50 {
        a.send(&Request::Frame { surface: s });
        a.pump(&mut comp);
        done += count(&mut a, |e| matches!(e, Event::FrameDone { .. }));
    }
    assert!(
        done <= 1,
        "M4: {done} FrameDone within one refresh interval (clock frozen)"
    );
}

fn key_serial(p: &Peer) -> u32 {
    p.events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Key {
                serial, state: 1, ..
            } => Some(*serial),
            _ => None,
        })
        .expect("key delivered")
}

fn transfer_of(p: &Peer) -> TransferId {
    p.events
        .iter()
        .find_map(|e| match e {
            Event::SendRequest { transfer, .. } => Some(*transfer),
            _ => None,
        })
        .expect("send request")
}

/// Source A owns a selection, reader B pastes. Returns the transfer.
fn paste_setup(comp: &mut Compositor) -> (Peer, Peer, TransferId) {
    let mut a = Peer::hello(comp);
    a.window(comp, 1, 0xff0000);
    let mut b = Peer::hello(comp);
    b.window(comp, 1, 0x0000ff);
    let (ka, kb) = (comp.state.windows[0].key, comp.state.windows[1].key);
    comp.state.focus_window(ka);
    key(comp, 30, true);
    a.pump(comp);
    let mimes = vec!["image/png".to_string()];
    a.send(&Request::SetSelection {
        serial: key_serial(&a),
        mimes,
    });
    a.pump(comp);
    key(comp, 30, false);
    comp.state.focus_window(kb);
    key(comp, 47, true);
    b.pump(comp);
    b.send(&Request::ReadSelection {
        serial: key_serial(&b),
        mime: "image/png".into(),
    });
    b.pump(comp);
    a.pump(comp);
    let t = transfer_of(&a);
    (a, b, t)
}

/// FINDING M5: transfers are forwarded without flow control: the source is
/// read at 2 MiB per turn while the reader's queue is capped at 8 MiB, but
/// a transfer may be 16 MiB. Pasting a 12 MiB image into an app that is
/// busy for a moment disconnects that app ("too slow").
#[test]
fn a_large_paste_does_not_disconnect_the_reader() {
    let mut comp = compositor(test_config(), &[(800, 600)]);
    let (mut a, b, transfer) = paste_setup(&mut comp);
    for i in 0..12 {
        a.send(&Request::SendData {
            transfer,
            data: vec![0x5a; 1 << 20],
            last: i == 11,
        });
        a.pump(&mut comp);
    }
    let reader = comp.state.windows.iter().any(|w| w.key.client == 2);
    assert!(
        reader && comp.state.clients.len() == 2,
        "M5: reader connected={reader}, clients={} after a 12 MiB paste",
        comp.state.clients.len()
    );
    drop(b);
}

/// FINDING m2: mirror mode overwrites the scale of the other outputs
/// (fit factor, any value 250–8000) and extend mode does not restore it:
/// after mirror -> extend the second display keeps e.g. 133 %.
#[test]
fn mirror_then_extend_restores_the_scale() {
    let mut comp = compositor(test_config(), &[(1920, 1080), (1024, 768)]);
    let second = comp.state.layout.outputs()[1].id;
    let before = comp.state.layout.get(second).unwrap().scale_milli;
    comp.state.layout.set_mode(DisplayMode::Mirror);
    comp.state.outputs_changed();
    comp.state.layout.set_mode(DisplayMode::Extend);
    comp.state.outputs_changed();
    let after = comp.state.layout.get(second).unwrap().scale_milli;
    assert_eq!(after, before, "m2: scale {before} became {after}");
}

fn assert_incremental(comp: &mut Compositor, step: &str) {
    comp.render_now();
    let ids: Vec<OutputId> = comp.state.layout.enabled().map(|o| o.id).collect();
    for id in ids {
        let reference = comp.render_reference(id).unwrap();
        let frame = comp.frame(id).unwrap();
        let differ = frame
            .data()
            .iter()
            .zip(reference.data())
            .filter(|(x, y)| x != y)
            .count();
        assert_eq!(differ, 0, "{step}: output {id:?} differs in {differ} px");
    }
}

/// SOUND: incremental == full with fractional scales (125–275 %), mirror
/// mode and a scale change while windows are shown.
#[test]
fn incremental_equals_full_with_fractional_scales_and_mirror() {
    let mut comp = compositor(test_config(), &[(1280, 800), (1000, 700)]);
    let mut peers = Vec::new();
    for i in 0..3u32 {
        let mut p = Peer::hello(&mut comp);
        p.window(&mut comp, 1, 0x304050 + i * 0x101010);
        peers.push(p);
    }
    let second = comp.state.layout.outputs()[1].id;
    let mut rng = Rng(0x5eed);
    for step in 0..12 {
        let scale = 1000 + 250 * (rng.below(8) as u32);
        comp.state.layout.set_scale(second, scale);
        if step == 6 {
            comp.state.layout.set_mode(DisplayMode::Mirror);
        }
        comp.state.outputs_changed();
        for p in &mut peers {
            p.pump(&mut comp);
            p.draw(&mut comp, SurfaceId(1), rng.next() as u32 & 0xff_ffff);
        }
        let x = rng.below(1200) as f64;
        click(&mut comp, x, 300.0);
        assert_incremental(&mut comp, &format!("step {step} scale {scale}"));
    }
}

/// SOUND: a client that stops in the middle of a 1 MiB message does not
/// stall anyone else (non-blocking reads, the frame waits in its reader).
#[test]
fn a_half_sent_message_blocks_nobody() {
    let mut comp = compositor(test_config(), &[(640, 480)]);
    let mut slow = Peer::hello(&mut comp);
    let frame = Request::WritePixels {
        buffer: ultar_surface::proto::BufferId(1),
        rect: ultar_surface::proto::Rect::new(0, 0, 512, 512),
        data: vec![0; 512 * 512 * 4],
    }
    .encode()
    .unwrap();
    slow.raw(&frame[..frame.len() / 2]);
    slow.pump(&mut comp);
    let mut fast = Peer::hello(&mut comp);
    fast.window(&mut comp, 1, 0x00ff00);
    assert!(comp.state.windows.iter().any(|w| w.mapped));
    assert_eq!(comp.state.clients.len(), 2);
}
