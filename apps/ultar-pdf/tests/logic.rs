//! The reader's logic on the fixture PDFs (no window events): opening, outline, text, search in
//! Latin and Arabic, passwords, damaged and missing files, zoom limits, rotation.

mod common;

use common::*;
use std::time::Duration;
use ultar_i18n::Lang;
use ultar_pdf::app::App;
use ultar_pdf::doc::DocState;
use ultar_pdf::place::ReadError;
use ultar_pdf::state::{Fit, Rot};
use ultar_ui::tokens::ThemeKind;

const LIMIT: Duration = Duration::from_secs(30);

fn open(name: &str, test: &str) -> App {
    let a = areas(test, &[name]);
    reader(
        Some(&format!("files://documents/{name}")),
        &a,
        Lang::En,
        ThemeKind::Dark,
        WIDE,
    )
}

fn search(app: &mut App, query: &str) {
    ctrl(app, 'f');
    app.start_search(query).expect("search");
    assert!(
        pump_until(app, LIMIT, |a| a.search_done()),
        "search did not finish"
    );
}

#[test]
fn the_mixed_fixture_opens_with_pages_outline_and_text() {
    let mut app = open("mixed.pdf", "mixed");
    assert!(app.busy() || app.doc.state == DocState::Opening);
    open_and_show(&mut app);
    assert_eq!(app.page_count(), 2);
    let titles: Vec<&str> = app.doc.outline.iter().map(|o| o.title.as_str()).collect();
    assert_eq!(titles, ["Start", "مرحبا", "الصفحة الثانية"]);
    assert_eq!(
        app.doc.outline.iter().map(|o| o.page).collect::<Vec<_>>(),
        [0, 0, 1]
    );
    assert!(pump_until(&mut app, LIMIT, |a| a.doc.text(0).is_some()));
    let text = app.doc.text(0).map(|t| t.all()).unwrap_or_default();
    assert!(text.contains("Ultar reader test page one"), "{text}");
    assert!(app.title().starts_with("mixed.pdf"));
    assert_eq!(
        app.activity_uri().as_deref(),
        Some("files://documents/mixed.pdf#page=1")
    );
}

#[test]
fn latin_search_finds_every_hit_ignoring_case() {
    let mut app = open("mixed.pdf", "latin");
    open_and_show(&mut app);
    search(&mut app, "fox");
    assert_eq!(app.hits().len(), 2, "{:?}", app.hits());
    let pages: Vec<usize> = app.hits().iter().map(|h| h.page).collect();
    assert_eq!(pages, [0, 1]);
    assert!(app.hits().iter().all(|h| !h.rects.is_empty()));
    assert_eq!(app.hit_position(), Some((1, 2)));
}

#[test]
fn arabic_search_matches_with_and_without_diacritics() {
    let mut app = open("mixed.pdf", "arabic");
    open_and_show(&mut app);
    search(&mut app, "مرحبا");
    assert_eq!(app.hits().len(), 1, "{:?}", app.hits());
    assert_eq!(app.hits()[0].page, 0);
    search(&mut app, "مَرْحَبًا");
    assert_eq!(app.hits().len(), 1, "marks are ignored");
    search(&mut app, "الصفحة");
    assert_eq!(app.hits().iter().map(|h| h.page).collect::<Vec<_>>(), [1]);
    // Eastern-Arabic digits and Latin words in one query do not break it.
    search(&mut app, "ملف quick");
    assert!(app.search_done());
}

#[test]
fn an_empty_search_says_so_and_a_blank_query_clears() {
    let mut app = open("mixed.pdf", "none");
    open_and_show(&mut app);
    search(&mut app, "zzzqqq");
    assert!(app.hits().is_empty());
    assert_eq!(app.hit_position(), None);
    app.start_search("   ").expect("blank");
    assert!(app.hits().is_empty());
}

#[test]
fn search_steps_wrap_around_the_hits() {
    let mut app = open("mixed.pdf", "step");
    open_and_show(&mut app);
    search(&mut app, "fox");
    app.step_hit(1).expect("next");
    assert_eq!(app.hit_position(), Some((2, 2)));
    assert_eq!(app.current_page(), 1);
    app.step_hit(1).expect("wrap");
    assert_eq!(app.hit_position(), Some((1, 2)));
    app.step_hit(-1).expect("back");
    assert_eq!(app.hit_position(), Some((2, 2)));
}

#[test]
fn a_locked_file_asks_for_the_password_and_opens_with_it() {
    let mut app = open("locked.pdf", "locked");
    assert!(pump_until(&mut app, LIMIT, |a| a.doc.state != DocState::Opening));
    assert_eq!(app.doc.state, DocState::NeedsPassword);
    assert!(app.asking());
    app.retry_password("wrong").expect("retry");
    assert!(pump_until(&mut app, LIMIT, |a| a.doc.state != DocState::Opening));
    assert_eq!(
        app.doc.state,
        DocState::NeedsPassword,
        "a wrong password asks again"
    );
    app.retry_password("ultar").expect("retry");
    open_and_show(&mut app);
    assert_eq!(app.page_count(), 3);
    assert!(!app.asking());
}

#[test]
fn damaged_and_missing_files_are_states_not_crashes() {
    let a = areas("damaged", &["broken.pdf"]);
    let mut app = reader(
        Some("files://documents/broken.pdf"),
        &a,
        Lang::En,
        ThemeKind::Dark,
        WIDE,
    );
    assert!(pump_until(&mut app, LIMIT, |a| a.doc.state != DocState::Opening));
    assert_eq!(app.doc.state, DocState::Damaged);
    assert_eq!(app.activity_uri(), None, "nothing is reported to the mesh");
    let mut app = reader(
        Some("files://documents/gone.pdf"),
        &a,
        Lang::En,
        ThemeKind::Dark,
        WIDE,
    );
    assert!(pump_until(&mut app, LIMIT, |a| a.doc.state != DocState::Opening));
    assert_eq!(app.doc.state, DocState::NoFile);
    assert_eq!(app.doc.read_error, Some(ReadError::Missing));
    let none = reader(None, &a, Lang::En, ThemeKind::Dark, WIDE);
    assert_eq!(none.doc.state, DocState::NoFile);
    assert_eq!(none.doc.read_error, None);
}

#[test]
fn zoom_stays_between_fifty_and_four_hundred_percent() {
    let mut app = open("mixed.pdf", "zoom");
    open_and_show(&mut app);
    app.set_fit(Fit::Zoom(100)).expect("100");
    assert_eq!(app.zoom_percent(), 100);
    for _ in 0..30 {
        app.zoom_by(1).expect("in");
    }
    assert_eq!(app.zoom_percent(), 400);
    for _ in 0..40 {
        app.zoom_by(-1).expect("out");
    }
    assert_eq!(app.zoom_percent(), 50);
    ctrl(&mut app, '0');
    assert_eq!(app.zoom_percent(), 100);
    ctrl(&mut app, '2');
    assert_eq!(app.fit(), Fit::Page);
    ctrl(&mut app, '1');
    assert_eq!(app.fit(), Fit::Width);
}

#[test]
fn rotating_turns_by_quarters_and_back() {
    let mut app = open("mixed.pdf", "rotate");
    open_and_show(&mut app);
    for _ in 0..4 {
        app.rotate().expect("rotate");
    }
    assert_eq!(app.rotation(), Rot::None);
    app.rotate().expect("rotate");
    assert_ne!(app.rotation(), Rot::None);
}

#[test]
fn opening_with_a_page_fragment_starts_on_that_page() {
    let a = areas("fragment", &["mixed.pdf"]);
    let mut app = reader(
        Some("files://documents/mixed.pdf#page=2"),
        &a,
        Lang::En,
        ThemeKind::Dark,
        WIDE,
    );
    open_and_show(&mut app);
    assert_eq!(app.current_page(), 1);
    assert_eq!(
        app.activity_uri().as_deref(),
        Some("files://documents/mixed.pdf#page=2")
    );
}
