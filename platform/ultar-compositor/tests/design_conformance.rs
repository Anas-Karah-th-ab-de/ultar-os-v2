//! Design conformance of the window title bar (boards Main, SettingsDisplay, SettingsDisplayLight,
//! Store; CLICKS.md `win.min`, `win.max`, `win.close`). The board draws three 32 × 32 buttons
//! (radius 8) with a 4-point gap, 4 points from the end edge, vertically centred in the 40-point
//! title bar; Arabic mirrors them to the left. Known differences are listed in
//! `design/conformance/known-differences.txt` as `decor/<id>` (the list only shrinks).

use ultar_compositor::decor::{buttons, Button, TITLEBAR};
use ultar_compositor::geom::Rect;

/// Board geometry for a 1440-point wide window (SettingsDisplay / SettingsDisplayLight).
fn board(rtl: bool) -> [(Button, Rect); 3] {
    if rtl {
        [
            (Button::Close, Rect::new(4, 4, 32, 32)),
            (Button::Maximize, Rect::new(40, 4, 32, 32)),
            (Button::Minimize, Rect::new(76, 4, 32, 32)),
        ]
    } else {
        [
            (Button::Close, Rect::new(1404, 4, 32, 32)),
            (Button::Maximize, Rect::new(1368, 4, 32, 32)),
            (Button::Minimize, Rect::new(1332, 4, 32, 32)),
        ]
    }
}

fn expect(id: &str, ok: bool, detail: String) {
    let known = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../design/conformance/known-differences.txt"),
    )
    .unwrap_or_default();
    let key = format!("decor/{id}");
    let listed = known.lines().any(|l| l.trim() == key);
    match (ok, listed) {
        (true, true) => panic!("FIXED, remove from known-differences.txt: {key}"),
        (false, false) => panic!("NEW difference {key}: {detail}"),
        (false, true) => eprintln!("known difference {key}: {detail}"),
        (true, false) => {}
    }
}

#[test]
fn title_bar_height_matches_the_boards() {
    assert_eq!(
        TITLEBAR, 40,
        "board title bar: 40 points (size.titlebar_height)"
    );
}

#[test]
fn title_bar_buttons_order_and_mirroring() {
    for rtl in [false, true] {
        let got = buttons(1440, rtl);
        let want = board(rtl);
        for ((gb, _), (wb, _)) in got.iter().zip(want.iter()) {
            assert_eq!(gb, wb, "order from the end edge: close, maximise, minimise");
        }
        // Mirroring: in Arabic close is the leftmost button.
        let close = got[0].1;
        let min = got[2].1;
        assert_eq!(close.x < min.x, rtl, "rtl={rtl}: close at the end edge");
    }
}

#[test]
fn title_bar_button_geometry_matches_the_boards() {
    for rtl in [false, true] {
        let got = buttons(1440, rtl);
        let want = board(rtl);
        let same = got.iter().zip(want.iter()).all(|((_, g), (_, w))| {
            (g.x - w.x).abs() <= 2
                && (g.y - w.y).abs() <= 2
                && (g.w - w.w).abs() <= 2
                && (g.h - w.h).abs() <= 2
        });
        let detail: Vec<String> = got
            .iter()
            .zip(want.iter())
            .map(|((b, g), (_, w))| {
                format!(
                    "{b:?}: board {},{} {}x{} compositor {},{} {}x{}",
                    w.x, w.y, w.w, w.h, g.x, g.y, g.w, g.h
                )
            })
            .collect();
        expect(
            if rtl {
                "win.buttons/geometry-rtl"
            } else {
                "win.buttons/geometry-ltr"
            },
            same,
            detail.join("; "),
        );
    }
}
