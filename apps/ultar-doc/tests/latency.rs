//! Typing in a document of 100 pages: every key (edit, layout of the changed paragraph,
//! redraw of the visible part) must answer within the budget (`docs/APP-RULES.md` section 8:
//! reaction to typing 50 ms on the phone, 30 ms on the PC). The numbers are printed; a debug
//! build is much slower than the release build the budget is meant for, so there the check is
//! only a loose bound (run `cargo test --release -p ultar-doc --test latency` for the real one).

mod common;

use common::*;
use std::time::{Duration, Instant};
use ultar_doc::state::Cursor;
use ultar_doc::App;
use ultar_office::doc::{Block, Document, Para, Style};
use ultar_ui::event::Key;

const PHONE_BUDGET: Duration = Duration::from_millis(50);
const PC_BUDGET: Duration = Duration::from_millis(30);

fn hundred_pages() -> Document {
    let latin = "The quick brown fox jumps over the lazy dog while the editor keeps every line in its place. ";
    let arabic = "\u{627}\u{644}\u{646}\u{635} \u{627}\u{644}\u{639}\u{631}\u{628}\u{64a} \u{64a}\u{628}\u{642}\u{649} \u{645}\u{646}\u{638}\u{645}\u{627}\u{64b} \u{62d}\u{62a}\u{649} \u{641}\u{64a} \u{627}\u{644}\u{645}\u{633}\u{62a}\u{646}\u{62f}\u{627}\u{62a} \u{627}\u{644}\u{637}\u{648}\u{64a}\u{644}\u{629} ";
    let mut d = Document::from_text("");
    d.blocks.clear();
    let mut i = 0;
    loop {
        let text = if i % 3 == 2 {
            arabic.repeat(5)
        } else {
            latin.repeat(4)
        };
        let mut p = Para::new(
            if i % 40 == 0 {
                Style::H1
            } else {
                Style::Normal
            },
            &text,
        );
        if i % 3 == 2 {
            p.dir = ultar_office::doc::Dir::Rtl;
        }
        d.blocks.push(Block::Para(p));
        i += 1;
        if i > 900 {
            return d;
        }
    }
}

fn stats(mut v: Vec<Duration>) -> (Duration, Duration, Duration) {
    v.sort();
    (v[v.len() / 2], v[v.len() * 99 / 100], v[v.len() - 1])
}

/// Types a burst of keys at the start of `block` and returns the slowest one.
fn type_burst(app: &mut App, (name, block): (&str, usize), pages: usize) -> Duration {
    app.doc.set_pos(
        Cursor {
            block,
            cell: None,
            ch: 3,
        },
        false,
    );
    app.show_caret().unwrap();
    let mut times = Vec::new();
    for c in "typing latency test 123".chars() {
        let t = Instant::now();
        let key = if c == ' ' { Key::Space } else { Key::Char(c) };
        press(app, key);
        times.push(t.elapsed());
    }
    let (median, p99, max) = stats(times);
    eprintln!("typing at the {name} of {pages} pages: median {median:?}, p99 {p99:?}, max {max:?}");
    max
}

#[test]
fn typing_in_a_hundred_pages_stays_within_the_budget() {
    let areas = areas("latency");
    let doc = ultar_doc::state::Doc::from_document(hundred_pages());
    let mut app = app_with(
        &areas,
        doc,
        ultar_i18n::Lang::En,
        ultar_ui::tokens::ThemeKind::Dark,
        (1280.0, 800.0),
    );
    let started = Instant::now();
    app.draw().unwrap();
    let cold = started.elapsed();
    let pages = app.lay.as_ref().unwrap().pages.len();
    assert!(pages >= 100, "the document has {pages} pages");
    let release = !cfg!(debug_assertions);
    let mut worst = Duration::ZERO;
    for (name, block) in [("start", 1usize), ("middle", 450), ("end", 880)] {
        worst = worst.max(type_burst(&mut app, (name, block), pages));
    }
    // Backspace and Enter change the structure, which re-lays out more.
    let t = Instant::now();
    press(&mut app, Key::Enter);
    press(&mut app, Key::Backspace);
    let structure = t.elapsed() / 2;
    eprintln!("cold layout and first draw of {pages} pages: {cold:?}; Enter/Backspace: {structure:?} each; worst key {worst:?}");
    if release {
        assert!(
            worst <= PHONE_BUDGET,
            "a key took {worst:?}, the phone budget is {PHONE_BUDGET:?}"
        );
        assert!(
            worst <= PC_BUDGET,
            "a key took {worst:?}, the PC budget is {PC_BUDGET:?}"
        );
        assert!(cold < Duration::from_secs(2), "opening takes {cold:?}");
    } else {
        assert!(
            worst < Duration::from_secs(3),
            "even a debug build answers a key in {worst:?}"
        );
    }
}

#[test]
fn scrolling_and_page_down_in_a_hundred_pages_are_quick() {
    let areas = areas("latency2");
    let doc = ultar_doc::state::Doc::from_document(hundred_pages());
    let mut app = app_with(
        &areas,
        doc,
        ultar_i18n::Lang::Ar,
        ultar_ui::tokens::ThemeKind::Light,
        (1280.0, 800.0),
    );
    app.draw().unwrap();
    let t = Instant::now();
    for _ in 0..20 {
        press(&mut app, Key::PageDown);
    }
    let per = t.elapsed() / 20;
    eprintln!("PageDown in 100 pages: {per:?} each");
    assert!(
        app.doc.cursor.block > 10,
        "the caret moved down: {:?}",
        app.doc.cursor
    );
    if !cfg!(debug_assertions) {
        assert!(per <= PHONE_BUDGET, "PageDown took {per:?}");
    }
}
