//! The composed display settings screen: structure, snapshots in both languages and themes.

mod common;

use common::{display_settings, snapshots, LANGS};
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::{Lang, Size};

const SCREEN: Size = Size::new(720.0, 760.0);

#[test]
fn display_settings_screen_snapshots() {
    let set = snapshots();
    for lang in LANGS {
        for theme in ThemeKind::ALL {
            set.assert_snapshot(
                "screen-display",
                |ui| display_settings(ui).map(|_| ()),
                SCREEN,
                lang,
                theme,
            )
            .unwrap_or_else(|e| panic!("{e}"));
        }
    }
}

#[test]
fn touch_mode_at_double_scale_snapshot() {
    // 44-point targets, two device pixels per point, Arabic.
    let set = snapshots().input_mode(InputMode::Touch);
    let mut ui = set
        .ui(Size::new(400.0, 880.0), Lang::Ar, ThemeKind::Dark)
        .unwrap();
    ui.set_scale(2.0).unwrap();
    display_settings(&mut ui).unwrap();
    set.assert_ui("screen-display-touch-2x", &mut ui)
        .unwrap_or_else(|e| panic!("{e}"));
    let canvas = ultar_ui::snapshot::render(&mut ui).unwrap();
    assert_eq!((canvas.width(), canvas.height()), (800, 1760));
}
