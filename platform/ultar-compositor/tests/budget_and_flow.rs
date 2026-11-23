//! Fix round 2: per-transfer flow control (pull), popups inside their
//! parent, and the frame budget's window cap.

mod common;

use common::*;
use ultar_compositor::Compositor;
use ultar_surface::proto::{Event, Request, Role, SurfaceId, TransferId};

fn key_serial(p: &mut Peer, comp: &mut Compositor, code: u16) -> u32 {
    key(comp, code, true);
    key(comp, code, false);
    p.pump(comp);
    p.events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Key {
                serial, state: 1, ..
            } => Some(*serial),
            _ => None,
        })
        .unwrap()
}

fn asks(p: &mut Peer) -> usize {
    p.take()
        .iter()
        .filter(|e| matches!(e, Event::SendRequest { .. }))
        .count()
}

/// A source that sends one chunk per `SendRequest` moves 12 MiB to a
/// reader that reads; the compositor never holds more than the queue.
#[test]
fn a_paced_source_moves_a_large_paste() {
    let mut comp = compositor(test_config(), &[(1024, 768)]);
    let mut a = Peer::hello_as(&mut comp, 5000);
    a.window(&mut comp, 1, 0xff0000);
    let mut b = Peer::hello_as(&mut comp, 5001);
    b.window(&mut comp, 1, 0x0000ff);
    let serial = key_serial(&mut a, &mut comp, 30);
    let mimes = vec!["image/png".to_string()];
    a.send(&Request::SetSelection { serial, mimes });
    a.pump(&mut comp);
    let kb = comp.state.windows[1].key;
    comp.state.focus_window(kb);
    let serial = key_serial(&mut b, &mut comp, 47);
    b.send(&Request::ReadSelection {
        serial,
        mime: "image/png".into(),
    });
    b.pump(&mut comp);
    a.pump(&mut comp);
    let (mut sent, mut got, mut last) = (0, 0, false);
    let mut pending = asks(&mut a);
    for _ in 0..2000 {
        if pending > 0 && sent < 12 {
            pending -= 1;
            sent += 1;
            a.send(&Request::SendData {
                transfer: TransferId(1),
                data: vec![1; 1 << 20],
                last: sent == 12,
            });
        }
        a.pump(&mut comp);
        b.pump(&mut comp);
        for e in b.take() {
            if let Event::Data { data, last: l, .. } = e {
                got += data.len();
                last |= l;
            }
        }
        pending += asks(&mut a);
        if last {
            break;
        }
    }
    assert!(last && got == 12 << 20, "got {got} bytes, last={last}");
    assert_eq!(comp.state.clients.len(), 2);
}

/// A popup never leaves its parent's content (it cannot cover other
/// windows or the shell, whatever position and size it asks for).
#[test]
fn popups_stay_inside_their_parent() {
    let mut comp = compositor(test_config(), &[(1280, 800)]);
    let mut a = Peer::hello(&mut comp);
    a.window(&mut comp, 1, 0xff0000);
    let parent = comp.state.windows[0].content();
    for (i, (x, y)) in [(-5000, -5000), (5000, 5000), (10, 10)]
        .into_iter()
        .enumerate()
    {
        let id = 2 + i as u32;
        let role = Role::Popup {
            parent: SurfaceId(1),
            x,
            y,
        };
        a.send(&Request::CreateSurface {
            surface: SurfaceId(id),
            role,
        });
        a.pump(&mut comp);
        a.fill(&mut comp, SurfaceId(id), 0, (2000, 1500), 0x00ff00);
        let key = *comp.state.layers.last().unwrap();
        let r = comp.state.layer_rect(key).unwrap();
        assert_eq!(
            r.intersect(&parent),
            Some(r),
            "popup {r:?} outside {parent:?}"
        );
    }
}

/// One uid shows at most `windows_per_uid` windows (frame cost); the
/// focused one is always among them.
#[test]
fn one_uid_shows_a_bounded_number_of_windows() {
    let mut comp = compositor(test_config(), &[(1280, 800)]);
    let cap = comp.state.config.limits.share.windows_per_uid;
    let mut peers = Vec::new();
    for c in 0..2u32 {
        let mut p = Peer::hello_as(&mut comp, 5000);
        for id in 1..=(cap as u32 / 2 + 4) {
            let s = SurfaceId(id);
            p.send(&Request::CreateSurface {
                surface: s,
                role: Role::Toplevel,
            });
            p.pump(&mut comp);
            let serial = p.last_configure(s).unwrap().serial;
            p.fill(&mut comp, s, serial, (200, 100), 0x101010 * (c + 1));
        }
        peers.push(p);
    }
    let lowest = comp.state.windows[0].key;
    comp.state.set_focus(Some(lowest));
    let (scene, culled) = comp.state.scene_parts();
    let shown = scene.iter().filter(|p| p.window.is_some()).count();
    assert!(
        shown <= cap + 1 && !culled.is_empty(),
        "{shown} windows shown"
    );
    assert!(
        scene.iter().any(|p| p.key == lowest),
        "focused window culled"
    );
}
