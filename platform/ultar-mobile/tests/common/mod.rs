//! Shared helpers of the phone shell's tests: a sample phone, clicks by meaning, renders.
#![allow(dead_code)]

use ultar_mobile::click::Click;
use ultar_mobile::model::quick::Toggle;
use ultar_mobile::model::time::LocalTime;
use ultar_mobile::model::{Mobile, Note, Status};
use ultar_mobile::{Effect, Look, MobileApp};
use ultar_ui::event::{Event, PointerButton};
use ultar_ui::geom::{Point, Size};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Lang;

/// The old phone's screen in points (720 × 1520 pixels at scale 2).
pub const PHONE: Size = Size::new(360.0, 760.0);

/// 25 November 2026, 19:56 UTC.
pub const SAMPLE_MS: u64 = 1_795_636_560_000;

/// A phone with battery, radios, a few notifications and recent apps.
pub fn sample(locked: bool) -> Mobile {
    let mut m = Mobile {
        now: LocalTime::at(SAMPLE_MS, 0),
        status: Status {
            battery: Some(87),
            charging: false,
            wifi: Some(3),
            cell: Some(2),
            cpu: 12,
            ram: 41,
        },
        ..Mobile::default()
    };
    m.quick.network = Some("KM-Office".into());
    m.quick.set(Toggle::Bluetooth, true);
    m.notes = vec![
        Note {
            id: 1,
            app: "org.ultar.phone".into(),
            title: "Layla Haddad".into(),
            body: "Missed call".into(),
            time: "16:20".into(),
        },
        Note {
            id: 2,
            app: "org.ultar.store".into(),
            title: "Ultar Store".into(),
            body: "2 updates ready".into(),
            time: "15:02".into(),
        },
    ];
    m.recents = vec!["org.ultar.files".into(), "org.ultar.settings".into()];
    if !locked {
        m.unlocked();
    }
    m
}

pub fn app(lang: Lang, theme: ThemeKind, model: Mobile) -> MobileApp {
    MobileApp::new(Look::phone(lang, theme), PHONE, model).unwrap()
}

/// Taps the node of `click`; panics when the screen has none.
pub fn tap(a: &mut MobileApp, click: &Click) -> Vec<Effect> {
    let id = a
        .built()
        .node(click)
        .unwrap_or_else(|| panic!("no node for {click:?}"));
    a.ui().layout();
    let pos = a.ui().rect(id).expect("shown").center();
    tap_at(a, pos)
}

pub fn tap_at(a: &mut MobileApp, pos: Point) -> Vec<Effect> {
    let mut effects = a.handle(Event::PointerMove { pos }).unwrap();
    effects.extend(
        a.handle(Event::PointerDown {
            pos,
            button: PointerButton::Primary,
        })
        .unwrap(),
    );
    effects.extend(
        a.handle(Event::PointerUp {
            pos,
            button: PointerButton::Primary,
        })
        .unwrap(),
    );
    effects
}

/// Writes the current screen to `$ULTAR_MOBILE_OUT/<name>.png`.
pub fn dump(a: &mut MobileApp, name: &str) {
    if let Ok(dir) = std::env::var("ULTAR_MOBILE_OUT") {
        let canvas = ultar_ui::snapshot::render(a.ui()).unwrap();
        let png = ultar_ui::snapshot::Image::from_canvas(&canvas)
            .encode_png()
            .unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), png).unwrap();
    }
}
