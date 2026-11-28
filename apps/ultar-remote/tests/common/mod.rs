//! Helpers of the window tests: a scripted mesh, clicks, the state a session needs.
#![allow(dead_code)]

use std::time::Instant;
use ultar_bus::messages::mesh::{DeviceKind, Peer};
use ultar_bus::messages::mesh_view::ViewEvent;
use ultar_i18n::Lang;
use ultar_remote::app::{Phase, RemoteApp};
use ultar_remote::port::{FakePort, PortEvent};
use ultar_ui::event::{Action, ActionKind, Event, PointerButton};
use ultar_ui::paint::PaintEnv;
use ultar_ui::snapshot::{render, Image};
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::Label;
use ultar_ui::{Point, Size};

pub const W: u32 = 320;
pub const H: u32 = 200;

pub fn peer(id: &str, name: &str, reachable: bool) -> Peer {
    Peer {
        id: id.to_owned(),
        name: name.to_owned(),
        kind: DeviceKind::Desktop,
        capabilities: Vec::new(),
        paired: true,
        reachable,
        rights: Vec::new(),
    }
}

pub fn app_in(lang: Lang, theme: ThemeKind) -> (RemoteApp, FakePort) {
    let ui = Ui::new(
        PaintEnv::new(lang, theme),
        InputMode::Pointer,
        Size::new(900.0, 600.0),
        ultar_remote::i18n::translator(lang),
    )
    .unwrap();
    let port = FakePort::default();
    (RemoteApp::new(ui, Box::new(port.clone())).unwrap(), port)
}

pub fn fresh() -> (RemoteApp, FakePort) {
    app_in(Lang::En, ThemeKind::Dark)
}

pub fn find(app: &mut RemoteApp, key: &str) -> NodeId {
    app.ui
        .a11y_tree()
        .into_iter()
        .find(|n| n.name == Some(Label::key(key)))
        .unwrap_or_else(|| panic!("no node named {key}"))
        .id
}

pub fn has(app: &mut RemoteApp, key: &str) -> bool {
    app.ui
        .a11y_tree()
        .iter()
        .any(|n| n.name == Some(Label::key(key)))
}

pub fn click(app: &mut RemoteApp, key: &str, now: Instant) {
    let id = find(app, key);
    app.ui.layout();
    let r = app.ui.rect(id).unwrap();
    let pos = Point {
        x: r.x + r.w / 2.0,
        y: r.y + r.h / 2.0,
    };
    let mut actions = Vec::new();
    for event in [
        Event::PointerMove { pos },
        Event::PointerDown {
            pos,
            button: PointerButton::Primary,
        },
        Event::PointerUp {
            pos,
            button: PointerButton::Primary,
        },
    ] {
        actions.extend(app.ui.handle(event).actions);
    }
    app.actions(actions, now).unwrap();
}

pub fn choose(app: &mut RemoteApp, key: &str, index: usize, now: Instant) {
    let node = find(app, key);
    app.actions(
        vec![Action {
            node,
            kind: ActionKind::Selected(index),
        }],
        now,
    )
    .unwrap();
}

/// Connects to the first device and plays "yes" with a screen of `W` x `H` (blue-green-red).
pub fn running(app: &mut RemoteApp, port: &FakePort, now: Instant) {
    port.tell(PortEvent::Peers(vec![peer("a1", "Laptop", true)]));
    app.poll(now).unwrap();
    click(app, "remote.connect", now);
    assert_eq!(app.phase(), Phase::Waiting);
    port.tell(PortEvent::View(ViewEvent::Waiting {
        name: "Laptop".into(),
    }));
    port.tell(PortEvent::View(ViewEvent::Running {
        width: W,
        height: H,
    }));
    app.poll(now).unwrap();
    assert_eq!(app.phase(), Phase::Running);
}

/// The window position of remote pixel (`x`, `y`) (the picture area starts at its node).
pub fn at(app: &mut RemoteApp, x: u32, y: u32) -> Point {
    let id = find(app, "remote.picture");
    app.ui.layout();
    let r = app.ui.rect(id).unwrap();
    let remote = app.remote().unwrap();
    let f = ultar_remote::picture::fit((remote.width(), remote.height()), (r.w as u32, r.h as u32));
    Point {
        x: r.x + f.x as f32 + (x as f32 + 0.5) * f.scale,
        y: r.y + f.y as f32 + (y as f32 + 0.5) * f.scale,
    }
}

/// One state painted: something is there, every operable node has a name, no key shows.
pub fn check_state(a: &mut RemoteApp, tag: &str) {
    a.ui.layout();
    let canvas = render(&mut a.ui).unwrap();
    let img = Image::from_canvas(&canvas);
    let first = &img.rgba[..4];
    assert!(
        img.rgba.chunks(4).any(|p| p != first),
        "{tag}: something is painted"
    );
    for n in a.ui.a11y_tree() {
        if n.focusable {
            assert!(n.name.is_some(), "{tag}: {:?} has no name", n.role);
        }
        if let Some(label) = n.name {
            let text = label.resolve(a.ui.translator());
            assert!(!text.is_empty(), "{tag}: empty name");
            assert!(!text.contains("remote."), "{tag}: key shown: {text}");
        }
    }
    if let Ok(dir) = std::env::var("ULTAR_REMOTE_SHOTS") {
        let png = img.encode_png().unwrap();
        let path = std::path::Path::new(&dir).join(format!("remote-{tag}.png"));
        ultar_ui::snapshot::write_atomic(&path, &png).unwrap();
    }
}
