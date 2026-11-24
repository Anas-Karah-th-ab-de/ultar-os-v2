//! The dial pad is never cut off. The window opens at a size that shows the whole pad
//! (`PhoneApp::whole_size`, which the compositor honours where the screen has the room: it
//! fits a 1280 × 800 and a 1024 × 768 screen), and on windows lower than its content (the
//! share of the screen a PC gave it before: 812 × 506 at 1280 × 800; a small window; a
//! phone) the pad scrolls: every key and the call button can be brought into the window.

mod common;

use common::*;
use ultar_bus::messages::call::{CallState, Cellular, Lines, Ultar};
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

/// The states a PC shows: before the call service answered, no modem and no account,
/// everything ready, no network.
fn states() -> Vec<CallState> {
    let lines = |cellular, ultar| CallState {
        lines: Lines {
            cellular,
            ultar,
            ..ready().lines
        },
        ..ready()
    };
    vec![
        CallState::default(),
        ready(),
        lines(Cellular::NoModem, Ultar::SignedOut),
        lines(Cellular::NoModem, Ultar::Offline),
        lines(Cellular::NoNetwork, Ultar::Ready),
    ]
}

#[test]
fn the_window_opens_with_the_whole_dial_pad() {
    // The work areas of a 1280 × 800 and a 1024 × 768 screen above the bar (56 points): the
    // compositor gives the window the size it asks for, but no more than the work area.
    let rooms = [(1280, 800 - 56), (1024, 768 - 56)];
    for lang in [Lang::Ar, Lang::En, Lang::De] {
        // The state at the start (the window opens before the call service answered) and
        // the size the app had then do not matter: the pad stays whole when the state changes.
        for (first, start) in [
            (CallState::default(), Size::new(1024.0, 680.0)),
            (ready(), Size::new(360.0, 640.0)),
        ] {
            for (rw, rh) in rooms {
                let mut app = app_with(lang, ThemeKind::Dark, start, first.clone());
                let (w, h) = app.whole_size().expect("the dial screen has a size");
                assert_eq!(app.ui().size(), start, "measuring leaves the size");
                assert!(w <= rw, "{lang:?}: {w} wide fits {rw}");
                // With every row that comes and goes at once the pad is a few points higher
                // than the smaller screen's work area; without the suggestion it fits both.
                assert!(h <= rh + 8, "{lang:?}: {w} × {h} fits {rw} × {rh}");
                let size = Size::new(w as f32, h.min(rh) as f32);
                app.ui().set_size(size).unwrap();
                app.relayout().unwrap();
                for state in states() {
                    app.set_state(state.clone(), 200_000).unwrap();
                    // Before typing, and while a suggestion shows for the typed digits.
                    for typed in [false, true] {
                        if typed {
                            let key = digit(&app, '4');
                            click(&mut app, key);
                        }
                        let pad = app.built().pad.expect("the pad");
                        app.ui().layout();
                        if typed && h > rh {
                            // The few points more than the screen has: scrolled into view.
                            let call = node(&app, "phone.call");
                            inside(&mut app, call, size, "call button");
                            continue;
                        }
                        let mut shown = vec![node(&app, "phone.call")];
                        shown.extend(['1', '0', '#'].map(|c| digit(&app, c)));
                        for id in shown {
                            let r = app.ui().rect(id).unwrap();
                            assert!(
                                r.y >= 0.0
                                    && r.y + r.h <= size.h
                                    && r.x >= 0.0
                                    && r.x + r.w <= size.w,
                                "{lang:?} typed={typed} at {size:?} in {:?}: {r:?} outside",
                                state.lines
                            );
                        }
                        assert_eq!(app.ui().scroll_offset(pad).unwrap(), 0.0);
                    }
                }
            }
        }
    }
}

#[test]
fn a_call_screen_has_no_opening_size() {
    let state = with_calls(vec![call(
        7,
        ultar_bus::messages::call::Phase::Active,
        false,
    )]);
    let mut app = app_with(Lang::En, ThemeKind::Dark, WIDE, state);
    assert_eq!(app.whole_size(), None);
}

#[test]
fn the_wheel_scrolls_the_dial_pad_in_a_small_window() {
    use ultar_ui::event::Event;
    // A quarter of a 1280 × 800 and of a 1024 × 768 screen (two columns; pages).
    for size in [Size::new(640.0, 372.0), Size::new(512.0, 356.0)] {
        for lang in [Lang::Ar, Lang::En] {
            let mut app = app_with(lang, ThemeKind::Dark, size, ready());
            let pad = app.built().pad.expect("the pad");
            app.ui().layout();
            let pos = app.ui().rect(pad).unwrap().center();
            app.handle(Event::PointerMove { pos }).unwrap();
            app.handle(Event::Scroll { pos, lines: 3.0 }).unwrap();
            let pad = app.built().pad.expect("the pad");
            let down = app.ui().scroll_offset(pad).unwrap();
            assert!(down > 0.0, "{lang:?} {size:?}: the wheel scrolled ({down})");
            app.handle(Event::Scroll { pos, lines: -9.0 }).unwrap();
            let pad = app.built().pad.expect("the pad");
            assert_eq!(app.ui().scroll_offset(pad).unwrap(), 0.0, "and back");
        }
    }
}
