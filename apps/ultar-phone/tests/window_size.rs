//! The dial pad on windows lower than its content: the first window a PC gives the phone
//! (812 × 506 at 1280 × 800), the one on a 1024 × 768 screen, a small window and a phone. The
//! pad scrolls instead of being cut off: every key and the call button can be brought into the
//! window, and the top of the pad is where it was.

mod common;

use common::*;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::{Lang, Size};

fn inside(app: &mut App, id: ultar_ui::ui::NodeId, size: Size, what: &str) {
    app.ui().scroll_into_view(id).unwrap();
    app.ui().layout();
    let r = app.ui().rect(id).unwrap();
    let pad = app.built().pad.unwrap();
    let (pr, off) = (
        app.ui().rect(pad).unwrap(),
        app.ui().scroll_offset(pad).unwrap(),
    );
    assert!(
        r.y >= -0.5 && r.y + r.h <= size.h + 0.5 && r.x >= -0.5 && r.x + r.w <= size.w + 0.5,
        "{what} at {size:?}: {r:?} outside the window (pad {pr:?}, offset {off})"
    );
}

#[test]
fn the_dial_pad_scrolls_on_low_windows() {
    let sizes = [
        Size::new(812.0, 506.0),  // 1280 × 800: the window seen cut off
        Size::new(650.0, 460.0),  // 1024 × 768
        Size::new(480.0, 360.0),  // a small window
        Size::new(360.0, 640.0),  // phone (compact: pages)
        Size::new(1024.0, 768.0), // large enough
    ];
    for size in sizes {
        for lang in [Lang::Ar, Lang::En] {
            let mut app = app_with(lang, ThemeKind::Dark, size, ready());
            let call = node(&app, "phone.call");
            let pad = app.built().pad.expect("the pad is a scroll container");
            assert_eq!(
                app.ui().scroll_offset(pad).unwrap(),
                0.0,
                "starts at the top"
            );
            for c in ['1', '5', '0'] {
                let key = digit(&app, c);
                inside(&mut app, key, size, &format!("key {c}"));
            }
            inside(&mut app, call, size, "call button");
            // The keys still work after scrolling (the click lands on the moved key).
            let zero = digit(&app, '0');
            inside(&mut app, zero, size, "key 0");
            let offset = app.ui().scroll_offset(pad).unwrap();
            click(&mut app, zero);
            assert!(app.model().digits.ends_with('0'), "{size:?}: key 0 typed");
            // The pad stays where the user scrolled it (a key press rebuilds the tree).
            let pad = app.built().pad.unwrap();
            assert_eq!(app.ui().scroll_offset(pad).unwrap(), offset, "{size:?}");
        }
    }
}
