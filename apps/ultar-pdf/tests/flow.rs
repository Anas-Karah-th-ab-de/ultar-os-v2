//! The reader through its event loop: keys, widgets, pointer, touch, the helper's actions and
//! what goes out to the window (clipboard, title) and to the mesh.

mod common;

use common::*;
use std::time::Duration;
use ultar_i18n::{Arg, Lang};
use ultar_pdf::action::{AgentAction, AgentReply};
use ultar_pdf::app::{App, Out};
use ultar_pdf::doc::DocState;
use ultar_pdf::state::Fit;
use ultar_ui::event::{Event, Key, PointerButton};
use ultar_ui::geom::Point;
use ultar_ui::tokens::ThemeKind;

const LIMIT: Duration = Duration::from_secs(30);

fn opened(test: &str, size: (f32, f32)) -> App {
    let a = areas(test, &["mixed.pdf"]);
    let mut app = reader(Some(DOC), &a, Lang::En, ThemeKind::Dark, size);
    open_and_show(&mut app);
    assert!(pump_until(&mut app, LIMIT, |a| a.doc.text(0).is_some()));
    app.take_out();
    app
}

fn pt(x: f32, y: f32) -> Point {
    Point { x, y }
}

#[test]
fn zoom_keys_and_fit_keys() {
    let mut app = opened("zoomkeys", WIDE);
    ctrl(&mut app, '0');
    assert_eq!(app.zoom_percent(), 100);
    ctrl(&mut app, '+');
    assert_eq!(app.zoom_percent(), 125);
    ctrl(&mut app, '-');
    ctrl(&mut app, '-');
    assert_eq!(app.zoom_percent(), 80);
    ctrl(&mut app, '2');
    assert_eq!(app.fit(), Fit::Page);
    ctrl(&mut app, '1');
    assert_eq!(app.fit(), Fit::Width);
    ctrl(&mut app, 'r');
    assert_ne!(app.rotation(), ultar_pdf::state::Rot::None);
}

#[test]
fn the_page_field_and_the_keys_move_between_pages() {
    let mut app = opened("pages", WIDE);
    assert_eq!(app.current_page(), 0);
    app.focus_page_field().expect("focus");
    press(&mut app, Key::Backspace);
    type_text(&mut app, "2");
    press(&mut app, Key::Enter);
    assert_eq!(app.current_page(), 1);
    assert_eq!(
        app.activity_uri().as_deref(),
        Some("files://documents/mixed.pdf#page=2")
    );
    press(&mut app, Key::Escape);
    key(&mut app, Key::Home, true, false);
    assert_eq!(app.current_page(), 0);
    key(&mut app, Key::End, true, false);
    assert_eq!(app.current_page(), 1);
}

#[test]
fn ctrl_f_typing_next_previous_and_n_of_m() {
    let mut app = opened("search", WIDE);
    ctrl(&mut app, 'f');
    assert!(app.search_open());
    type_text(&mut app, "fox");
    assert!(pump_until(&mut app, LIMIT, |a| a.search_done()));
    assert_eq!(app.hits().len(), 2);
    assert_eq!(app.hit_position(), Some((1, 2)));
    press(&mut app, Key::Enter);
    assert_eq!(app.hit_position(), Some((2, 2)));
    assert_eq!(app.current_page(), 1, "the view followed the hit");
    key(&mut app, Key::Enter, false, true);
    assert_eq!(app.hit_position(), Some((1, 2)));
    // the count text of the bar
    let tr = ultar_pdf::i18n::translator(Lang::En);
    let shown = tr.t_args(
        "pdf.search.count",
        &[("pos", Arg::Int(1)), ("total", Arg::Int(2))],
    );
    assert_eq!(shown, "1 of 2");
    press(&mut app, Key::Escape);
    assert!(!app.search_open());
    assert!(app.hits().is_empty());
}

#[test]
fn dragging_over_a_line_selects_it_and_ctrl_c_copies_it() {
    let mut app = opened("select", WIDE);
    let words = app.doc.text(0).map(|t| t.words.len()).unwrap_or(0);
    assert!(words > 4);
    let first = app
        .doc
        .text(0)
        .and_then(|t| t.words.iter().position(|w| w.text == "Ultar"))
        .expect("Ultar");
    let last = app
        .doc
        .text(0)
        .and_then(|t| t.words.iter().position(|w| w.text == "one"))
        .expect("one");
    let from = app.word_point(0, first).expect("point");
    let to = app.word_point(0, last).expect("point");
    send(
        &mut app,
        Event::PointerDown {
            pos: from,
            button: PointerButton::Primary,
        },
    );
    send(
        &mut app,
        Event::PointerMove {
            pos: pt((from.x + to.x) / 2.0, from.y),
        },
    );
    send(&mut app, Event::PointerMove { pos: to });
    send(
        &mut app,
        Event::PointerUp {
            pos: to,
            button: PointerButton::Primary,
        },
    );
    assert_eq!(app.selection_range(), Some((0, first, last)));
    assert_eq!(
        app.selected_text().as_deref(),
        Some("Ultar reader test page one")
    );
    ctrl(&mut app, 'c');
    assert_eq!(
        app.take_out(),
        vec![Out::Copy("Ultar reader test page one".into())]
    );
    press(&mut app, Key::Escape);
    assert_eq!(app.selection_range(), None);
}

#[test]
fn select_all_copies_the_page_text_in_both_scripts() {
    let mut app = opened("all", WIDE);
    ctrl(&mut app, 'a');
    ctrl(&mut app, 'c');
    let copied =
        app.take_out()
            .into_iter()
            .find_map(|o| if let Out::Copy(t) = o { Some(t) } else { None });
    let copied = copied.expect("copy");
    assert!(copied.contains("The quick brown fox"), "{copied}");
    assert!(copied.contains("مرحبا"), "{}", copied.escape_unicode());
}

fn down(app: &mut App, id: u64, x: f32, y: f32) {
    send(app, Event::TouchDown { id, pos: pt(x, y) });
}

fn moved(app: &mut App, id: u64, x: f32, y: f32) {
    send(app, Event::TouchMove { id, pos: pt(x, y) });
}

fn lift(app: &mut App, id: u64, x: f32, y: f32) {
    send(app, Event::TouchUp { id, pos: pt(x, y) });
}

#[test]
fn a_pinch_zooms_the_page() {
    let mut app = opened("pinch", COMPACT);
    ctrl(&mut app, '0');
    let before = app.zoom_percent();
    down(&mut app, 1, 150.0, 400.0);
    down(&mut app, 2, 250.0, 400.0);
    moved(&mut app, 2, 350.0, 400.0);
    lift(&mut app, 2, 350.0, 400.0);
    lift(&mut app, 1, 150.0, 400.0);
    assert!(
        app.zoom_percent() > before,
        "{} -> {}",
        before,
        app.zoom_percent()
    );
}

#[test]
fn a_one_finger_drag_scrolls() {
    let mut app = opened("pan", COMPACT);
    ctrl(&mut app, '0');
    let at = app.scroll_offset();
    down(&mut app, 3, 200.0, 600.0);
    moved(&mut app, 3, 200.0, 300.0);
    lift(&mut app, 3, 200.0, 300.0);
    assert!(app.scroll_offset() > at, "dragging up scrolls down");
}

#[test]
fn the_wheel_scrolls_the_pages() {
    let mut app = opened("wheel", WIDE);
    let area = app.page_area().expect("page area");
    let at = pt(area.x + area.w / 2.0, area.y + area.h / 2.0);
    send(
        &mut app,
        Event::Scroll {
            pos: at,
            lines: 30.0,
        },
    );
    assert!(app.scroll_offset() > 0.0);
    assert_eq!(app.current_page(), 1, "a long scroll reaches page 2");
}

#[test]
fn agent_actions_open_go_to_page_search_and_copy_text() {
    let a = areas("agent", &["mixed.pdf"]);
    let mut app = reader(None, &a, Lang::En, ThemeKind::Dark, WIDE);
    assert_eq!(
        app.agent(AgentAction::GoToPage { page: 1 }).expect("a"),
        AgentReply::NotOpen
    );
    assert_eq!(
        app.agent(AgentAction::Open {
            uri: "files://../etc/passwd".into()
        })
        .expect("a"),
        AgentReply::Invalid
    );
    assert_eq!(
        app.agent(AgentAction::Open { uri: DOC.into() }).expect("a"),
        AgentReply::Done
    );
    open_and_show(&mut app);
    assert_eq!(
        app.agent(AgentAction::GoToPage { page: 2 }).expect("a"),
        AgentReply::Done
    );
    assert_eq!(app.current_page(), 1);
    assert_eq!(
        app.agent(AgentAction::GoToPage { page: 3 }).expect("a"),
        AgentReply::OutOfRange { pages: 2 }
    );
    assert_eq!(
        app.agent(AgentAction::GoToPage { page: 0 }).expect("a"),
        AgentReply::OutOfRange { pages: 2 }
    );
    assert_eq!(
        app.agent(AgentAction::Search {
            query: "fox".into()
        })
        .expect("a"),
        AgentReply::Done
    );
    assert!(pump_until(&mut app, LIMIT, |a| a.search_done()));
    assert_eq!(app.hits().len(), 2);
    assert_eq!(
        app.agent(AgentAction::Search { query: "  ".into() })
            .expect("a"),
        AgentReply::Invalid
    );
    app.take_out();
    assert_eq!(
        app.agent(AgentAction::CopyText { page: Some(1) })
            .expect("a"),
        AgentReply::Done
    );
    let copied = pump_out(&mut app).expect("the text of page 1 is copied");
    assert!(copied.contains("Ultar reader test page one"), "{copied}");
    assert_eq!(
        app.agent(AgentAction::CopyText { page: Some(9) })
            .expect("a"),
        AgentReply::OutOfRange { pages: 2 }
    );
}

fn pump_out(app: &mut App) -> Option<String> {
    let start = std::time::Instant::now();
    loop {
        app.poll().expect("poll");
        for o in app.take_out() {
            if let Out::Copy(text) = o {
                return Some(text);
            }
        }
        if start.elapsed() > LIMIT {
            return None;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_locked_file_goes_through_the_password_dialog() {
    let a = areas("pwflow", &["locked.pdf"]);
    let mut app = reader(
        Some("files://documents/locked.pdf"),
        &a,
        Lang::En,
        ThemeKind::Light,
        COMPACT,
    );
    assert!(pump_until(&mut app, LIMIT, |a| a.doc.state == DocState::NeedsPassword));
    assert!(app.asking());
    assert_eq!(app.activity_uri(), None);
    app.retry_password("ultar").expect("retry");
    open_and_show(&mut app);
    assert_eq!(
        app.activity_uri().as_deref(),
        Some("files://documents/locked.pdf#page=1")
    );
}

#[test]
fn opening_another_file_by_ctrl_o_and_the_recents_list() {
    let a = areas("open", &["mixed.pdf", "plain3.pdf"]);
    let mut app = reader(Some(DOC), &a, Lang::En, ThemeKind::Dark, WIDE);
    open_and_show(&mut app);
    ctrl(&mut app, 'o');
    assert!(app.asking());
    app.open_entry("documents/plain3.pdf").expect("open");
    assert!(!app.asking());
    open_and_show(&mut app);
    assert_eq!(app.page_count(), 3);
    assert!(app.title().starts_with("plain3.pdf"));
    let saved = std::fs::read_to_string(a.join("data").join("recents")).or_else(|_| {
        std::fs::read_dir(a.join("data"))
            .ok()
            .and_then(|mut d| d.next())
            .and_then(|e| e.ok())
            .map(|e| std::fs::read_to_string(e.path()))
            .unwrap_or_else(|| Ok(String::new()))
    });
    let saved = saved.unwrap_or_default();
    assert!(
        saved.contains("plain3.pdf") && saved.contains("mixed.pdf"),
        "{saved}"
    );
}

#[test]
fn closing_the_window_asks_nothing() {
    let mut app = opened("close", WIDE);
    app.close_requested();
    assert!(app.take_out().contains(&Out::Close));
}
