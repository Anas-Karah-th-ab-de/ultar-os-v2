//! A long document stays smooth: a 300-page PDF opens, scrolls through all its pages and keeps
//! the picture cache inside its budget; a 500-page one too. Times are printed for the record.
//! (Debug build: the APP-RULES §8 budgets are for release; the limits here are the debug-build
//! bounds of the same behaviour, and the per-event reaction is the one that matters.)

mod common;

use common::*;
use std::time::{Duration, Instant};
use ultar_i18n::Lang;
use ultar_pdf::app::App;
use ultar_pdf::doc::{Doc, DocState, PAGE_BUDGET, THUMB_BUDGET};
use ultar_ui::event::{Event, Key};
use ultar_ui::geom::Point;
use ultar_ui::tokens::ThemeKind;

const LIMIT: Duration = Duration::from_secs(60);

/// APP-RULES section 8: a click or key is answered in 50 ms, the warm start shows content in
/// 300 ms. The budgets are for release builds; an unoptimised build (every per-pixel loop 20 to
/// 40 times slower) gets a proportional allowance so that `cargo test` stays meaningful.
fn budget() -> Duration {
    Duration::from_millis(if cfg!(debug_assertions) { 1500 } else { 50 })
}

fn open_budget() -> Duration {
    Duration::from_millis(if cfg!(debug_assertions) { 10_000 } else { 300 })
}

fn long(pages: usize, size: (f32, f32)) -> (App, Duration) {
    let a = areas(&format!("long{pages}"), &[]);
    let started = Instant::now();
    let doc = Doc::from_bytes(
        long_pdf(pages),
        Some(format!("files://documents/long{pages}.pdf")),
        None,
    );
    let data = a.join("data");
    std::fs::create_dir_all(&data).expect("data");
    let mut app = App::new(ui(Lang::En, ThemeKind::Dark, size), doc, a, data).expect("app");
    open_and_show(&mut app);
    (app, started.elapsed())
}

#[test]
fn a_300_page_document_opens_and_scrolls_smoothly() {
    let (mut app, opened) = long(300, WIDE);
    println!("300 pages: open to first picture {opened:?}");
    assert_eq!(app.page_count(), 300);
    assert!(opened < open_budget(), "open took {opened:?}");
    // Every key press is handled without waiting for the worker.
    let mut worst = Duration::ZERO;
    let mut total = Duration::ZERO;
    let mut presses = 0u32;
    let steps = if cfg!(debug_assertions) { 60 } else { 400 };
    for _ in 0..steps {
        let t = Instant::now();
        key(&mut app, Key::PageDown, false, false);
        let d = t.elapsed();
        worst = worst.max(d);
        total += d;
        presses += 1;
        app.poll().expect("poll");
        if app.current_page() >= 299 {
            break;
        }
    }
    println!(
        "scroll: {presses} presses, mean {:?}, worst {:?}, page {}",
        total / presses,
        worst,
        app.current_page()
    );
    assert!(
        app.current_page() > steps / 4,
        "page down reached page {}",
        app.current_page()
    );
    assert!(worst < budget(), "a key press took {worst:?}");
    // jump to the end and back: the pictures follow the view
    key(&mut app, Key::End, true, false);
    assert_eq!(app.current_page(), 299);
    assert!(pump_until(&mut app, LIMIT, |a| a
        .doc
        .stale_image(299)
        .is_some()));
    key(&mut app, Key::Home, true, false);
    assert!(pump_until(&mut app, LIMIT, |a| a
        .doc
        .stale_image(0)
        .is_some()));
    assert!(
        app.doc.cache_bytes() <= PAGE_BUDGET + THUMB_BUDGET,
        "cache {} bytes",
        app.doc.cache_bytes()
    );
}

#[test]
fn the_wheel_and_zoom_on_a_long_document_react_quickly() {
    let (mut app, _) = long(300, WIDE);
    let area = app.page_area().expect("area");
    let at = Point {
        x: area.x + area.w / 2.0,
        y: area.y + area.h / 2.0,
    };
    let mut worst = Duration::ZERO;
    for i in 0..60 {
        let t = Instant::now();
        send(
            &mut app,
            Event::Scroll {
                pos: at,
                lines: if i % 2 == 0 { 5.0 } else { -2.0 },
            },
        );
        worst = worst.max(t.elapsed());
        app.poll().expect("poll");
    }
    ctrl(&mut app, '+');
    ctrl(&mut app, '+');
    let t = Instant::now();
    ctrl(&mut app, '+');
    let zoom = t.elapsed();
    println!("wheel worst {worst:?}, zoom step {zoom:?}");
    assert!(worst < budget(), "wheel took {worst:?}");
    assert!(zoom < budget(), "zoom took {zoom:?}");
    let settled = pump_until(&mut app, Duration::from_secs(20), |a| !a.busy());
    assert!(
        settled,
        "still busy: missing {:?}, cache {} bytes",
        app.doc.missing(),
        app.doc.cache_bytes()
    );
}

#[test]
fn a_500_page_document_keeps_its_memory_inside_the_budget() {
    let (mut app, opened) = long(500, WIDE);
    println!("500 pages: open to first picture {opened:?}");
    assert_eq!(app.page_count(), 500);
    for target in [0usize, 120, 260, 499, 30, 400, 250] {
        app.goto(target).expect("goto");
        assert!(
            pump_until(&mut app, LIMIT, |a| a.doc.stale_image(target).is_some()),
            "page {target}"
        );
        assert!(
            app.doc.cache_bytes() <= PAGE_BUDGET + THUMB_BUDGET,
            "after page {target}: {} bytes",
            app.doc.cache_bytes()
        );
    }
    // a big zoom makes pictures bigger, never the cache
    app.set_fit(ultar_pdf::state::Fit::Zoom(400)).expect("zoom");
    assert!(pump_until(&mut app, LIMIT, |a| a
        .doc
        .stale_image(250)
        .is_some()));
    assert!(
        app.doc.cache_bytes() <= PAGE_BUDGET + THUMB_BUDGET,
        "{} bytes",
        app.doc.cache_bytes()
    );
    assert_eq!(app.doc.state, DocState::Open);
}

#[test]
fn searching_a_300_page_document_is_incremental_and_the_view_stays_responsive() {
    let (mut app, _) = long(300, WIDE);
    ctrl(&mut app, 'f');
    type_text(&mut app, "page 150 ");
    let t = Instant::now();
    app.start_search("page 150").expect("search");
    assert!(t.elapsed() < budget(), "starting the search blocked");
    assert!(pump_until(&mut app, LIMIT, |a| a.search_done()));
    assert!(!app.hits().is_empty());
    assert!(
        app.hits().iter().any(|h| h.page == 149),
        "hits {:?}",
        app.hits().iter().map(|h| h.page).collect::<Vec<_>>()
    );
}
