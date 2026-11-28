//! Mouse and touch, the three layout classes, colour dialogs, tables, pictures, the file with
//! parts the model cannot keep, the page/flow view.

mod common;

use common::*;
use std::time::Duration;
use ultar_doc::commands::Cmd;
use ultar_doc::dialogs::Ask;
use ultar_doc::state::{Cursor, Doc};
use ultar_i18n::Lang;
use ultar_office::doc::Block;
use ultar_ui::event::{Event, PointerButton};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Point;

fn at(block: usize, ch: usize) -> Cursor {
    Cursor {
        block,
        cell: None,
        ch,
    }
}

fn typed_app(tag: &str, text: &str) -> ultar_doc::App {
    let areas = areas(tag);
    let mut app = app(&areas);
    type_text(&mut app, text);
    app
}

#[test]
fn a_click_puts_the_caret_dragging_selects_double_and_triple_click_pick_word_and_paragraph() {
    let mut app = typed_app("mouse", "hello world");
    let p6 = screen_pos(&mut app, at(0, 6));
    click_at(&mut app, p6);
    assert_eq!(
        app.doc.cursor.ch, 6,
        "the click lands between the characters"
    );
    assert!(app.doc.selection().is_none());
    let (a, b) = (
        screen_pos(&mut app, at(0, 2)),
        screen_pos(&mut app, at(0, 8)),
    );
    pointer(
        &mut app,
        Event::PointerDown {
            pos: a,
            button: PointerButton::Primary,
        },
    );
    pointer(&mut app, Event::PointerMove { pos: b });
    pointer(
        &mut app,
        Event::PointerUp {
            pos: b,
            button: PointerButton::Primary,
        },
    );
    assert_eq!(app.doc.selected_text(), "llo wo", "dragging selects");
    std::thread::sleep(Duration::from_millis(500));
    let inside = screen_pos(&mut app, at(0, 8));
    click_at(&mut app, inside);
    click_at(&mut app, inside);
    assert_eq!(
        app.doc.selected_text(),
        "world",
        "double click selects the word"
    );
    click_at(&mut app, inside);
    assert_eq!(
        app.doc.selected_text(),
        "hello world",
        "triple click selects the paragraph"
    );
}

#[test]
fn the_wheel_scrolls_and_ctrl_wheel_zooms() {
    let mut app = app_text(&areas("wheel"), &"line of text\n".repeat(120));
    app.draw().unwrap();
    let r = app.ui.rect(app.nodes.text.unwrap()).unwrap();
    let pos = Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
    let top = app.scroll.1;
    pointer(&mut app, Event::Scroll { pos, lines: 3.0 });
    assert!(app.scroll.1 > top, "scrolled down");
    pointer(&mut app, Event::Scroll { pos, lines: -300.0 });
    assert_eq!(app.scroll.1, 0.0, "and clamped at the top");
    let zoom = app.doc.zoom;
    pointer(&mut app, Event::Modifiers { modifiers: ctrl() });
    pointer(&mut app, Event::Scroll { pos, lines: -1.0 });
    assert!(app.doc.zoom > zoom, "ctrl+wheel zooms in");
    pointer(
        &mut app,
        Event::Modifiers {
            modifiers: Default::default(),
        },
    );
    chord(&mut app, '-');
    assert!(app.doc.zoom < zoom * 1.2);
}

#[test]
fn touch_tap_double_tap_drag_scroll_and_hold_to_select() {
    let mut app = app_text(
        &areas("touch"),
        &format!("hello world\n{}", "more text\n".repeat(150)),
    );
    let p = screen_pos(&mut app, at(0, 8));
    pointer(&mut app, Event::TouchDown { id: 1, pos: p });
    pointer(&mut app, Event::TouchUp { id: 1, pos: p });
    assert_eq!(app.doc.cursor, at(0, 8), "a tap puts the caret");
    pointer(&mut app, Event::TouchDown { id: 2, pos: p });
    pointer(&mut app, Event::TouchUp { id: 2, pos: p });
    assert_eq!(
        app.doc.selected_text(),
        "world",
        "a double tap selects the word"
    );
    // Dragging a finger scrolls.
    let from = Point::new(p.x, p.y + 200.0);
    pointer(&mut app, Event::TouchDown { id: 3, pos: from });
    pointer(
        &mut app,
        Event::TouchMove {
            id: 3,
            pos: Point::new(p.x, p.y + 40.0),
        },
    );
    pointer(
        &mut app,
        Event::TouchUp {
            id: 3,
            pos: Point::new(p.x, p.y + 40.0),
        },
    );
    assert!(app.scroll.1 > 100.0, "a drag scrolls: {}", app.scroll.1);
    assert_eq!(
        app.doc.selected_text(),
        "world",
        "and does not touch the selection"
    );
    // Holding first, then dragging, selects.
    app.scroll.1 = 0.0;
    let (a, b) = (
        screen_pos(&mut app, at(0, 0)),
        screen_pos(&mut app, at(0, 5)),
    );
    pointer(&mut app, Event::TouchDown { id: 4, pos: a });
    std::thread::sleep(Duration::from_millis(520));
    pointer(&mut app, Event::TouchMove { id: 4, pos: b });
    pointer(&mut app, Event::TouchUp { id: 4, pos: b });
    assert_eq!(app.doc.selected_text(), "hello", "hold and drag selects");
}

#[test]
fn the_three_classes_show_more_and_more_and_more_holds_the_rest() {
    let dir = areas("classes");
    let size = |w: f32, h: f32| app_with(&dir, Doc::new(), Lang::En, ThemeKind::Dark, (w, h));
    let (compact, medium, wide) = (size(360.0, 720.0), size(640.0, 800.0), size(1280.0, 800.0));
    let count = |a: &ultar_doc::App| {
        ultar_doc::commands::ALL
            .iter()
            .filter(|&&c| a.command_node(c).is_some())
            .count()
    };
    assert!(count(&compact) < count(&medium) && count(&medium) < count(&wide));
    assert!(
        compact.command_node(Cmd::Bold).is_some() && compact.command_node(Cmd::Strike).is_none()
    );
    assert!(wide.command_node(Cmd::Table).is_some() && wide.command_node(Cmd::More).is_none());
    let mut compact = compact;
    type_text(&mut compact, "abc");
    chord(&mut compact, 'a');
    {
        let n = compact.command_node(Cmd::More).unwrap();
        click(&mut compact, n);
    }
    assert_eq!(compact.ask, Some(Ask::More));
    let strike = compact
        .nodes
        .picks
        .iter()
        .find(|(_, p)| *p == ultar_doc::dialogs::Pick::Command(Cmd::Strike))
        .map(|(n, _)| *n)
        .unwrap();
    click(&mut compact, strike);
    assert!(
        compact.ask.is_none(),
        "a command of More runs and closes the dialog"
    );
    assert!(compact.doc.is_on(Cmd::Strike), "and has done its work");
}

#[test]
fn text_colour_and_highlight_come_from_the_dialogs() {
    let mut app = typed_app("colour", "red text");
    chord(&mut app, 'a');
    {
        let n = app.command_node(Cmd::Color).unwrap();
        click(&mut app, n);
    }
    assert_eq!(app.ask, Some(Ask::Color));
    let red = app.nodes.picks[2].0;
    click(&mut app, red);
    {
        let n = app.command_node(Cmd::Highlight).unwrap();
        click(&mut app, n);
    }
    let yellow = app.nodes.picks[1].0;
    click(&mut app, yellow);
    let Block::Para(p) = &app.doc.document.blocks[0] else {
        panic!("para");
    };
    assert!(p
        .runs
        .iter()
        .all(|r| r.color.as_deref() == Some("C00000") && r.highlight.as_deref() == Some("FFFF00")));
}

#[test]
fn a_table_and_a_picture_are_inserted_typed_into_saved_and_read_back() {
    let areas = areas("table");
    sample_png(&areas.join("documents/red.png"));
    let mut app = app(&areas);
    type_text(&mut app, "intro");
    press_cmd(&mut app, Cmd::Table);
    assert!(app.doc.in_table());
    type_text(&mut app, "cell text");
    press_cmd(&mut app, Cmd::RowAdd);
    press_cmd(&mut app, Cmd::ColAdd);
    let rows = first_table(&app);
    assert_eq!(
        (rows.len(), rows[0].len()),
        (4, 4),
        "3x3 plus a row and a column"
    );
    assert!(
        rows.iter().flatten().any(|c| c.contains("cell text")),
        "typing goes into the cell"
    );
    // A picture from a file in Documents.
    press_cmd(&mut app, Cmd::Picture);
    answer_name(&mut app, "red.png");
    let pic = first_picture(&app);
    assert_eq!(pic.width_pt, Some(3.0), "4 pixels at 96 dpi");
    app.doc.set_pos(
        at(
            app.doc
                .document
                .blocks
                .iter()
                .position(|b| matches!(b, Block::Picture(_)))
                .unwrap(),
            0,
        ),
        false,
    );
    press_cmd(&mut app, Cmd::PicBigger);
    let bigger = first_picture(&app).width_pt.unwrap();
    assert!(bigger > 3.0);
    chord(&mut app, 's');
    answer_name(&mut app, "mixed");
    let back = Doc::open(&areas.join("documents/mixed.docx")).unwrap();
    assert!(back
        .document
        .blocks
        .iter()
        .any(|b| matches!(b, Block::Table(t) if t.len() == 4)));
    assert!(back
        .document
        .blocks
        .iter()
        .any(|b| matches!(b, Block::Picture(p) if p.bytes.starts_with(&[0x89, b'P']))));
    // A bad picture name says why instead of failing silently.
    press_cmd(&mut app, Cmd::Picture);
    answer_name(&mut app, "missing.png");
    assert!(app.message.is_some());
}

#[test]
fn a_file_with_parts_the_model_cannot_keep_warns_and_keeps_the_original() {
    let areas = areas("lossy");
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ultar-office/tests/fixtures/word-style.docx");
    let file = areas.join("documents/w.docx");
    std::fs::copy(&src, &file).unwrap();
    let (doc, err) = ultar_doc::App::open(Some("files://documents/w.docx"), &areas);
    assert!(
        err.is_none() && !doc.lossy.is_empty(),
        "the fixture has parts that are lost: {:?}",
        doc.lossy
    );
    let mut app = app_with(&areas, doc, Lang::En, ThemeKind::Light, (1280.0, 800.0));
    assert_eq!(app.ask, Some(Ask::Lossy), "the warning comes first");
    {
        let n = app.nodes.ok.unwrap();
        click(&mut app, n);
    }
    assert!(app.ask.is_none());
    type_text(&mut app, "!");
    chord(&mut app, 's');
    assert!(app.ask.is_none());
    let kept = areas.join("documents/w.original.docx");
    assert_eq!(
        std::fs::read(&kept).unwrap(),
        std::fs::read(&src).unwrap(),
        "the original is kept byte for byte"
    );
    assert!(Doc::open(&file).is_ok());
}

#[test]
fn page_and_flow_view_zoom_and_paper_size() {
    let mut app = app_text(&areas("view"), &"paragraph\n".repeat(150));
    assert!(app.doc.page_view);
    app.draw().unwrap();
    assert!(app.lay.as_ref().unwrap().pages.len() >= 3);
    {
        let n = app.command_node(Cmd::PageToggle).unwrap();
        click(&mut app, n);
    }
    app.draw().unwrap();
    assert!(!app.doc.page_view && app.lay.as_ref().unwrap().pages.len() == 1);
    {
        let n = app.command_node(Cmd::PageToggle).unwrap();
        click(&mut app, n);
    }
    {
        let n = app.command_node(Cmd::Paper).unwrap();
        click(&mut app, n);
    }
    assert!(app.doc.is_letter());
    {
        let n = app.command_node(Cmd::ZoomIn).unwrap();
        click(&mut app, n);
    }
    assert!((app.doc.zoom - 1.25).abs() < 0.01);
    chord(&mut app, 'z');
    assert!(!app.doc.is_letter(), "the paper size is one undo step");
    // A phone starts in the flowing view.
    let phone = app_with(
        &areas("phone"),
        Doc::new(),
        Lang::Ar,
        ThemeKind::Dark,
        (360.0, 720.0),
    );
    assert!(!phone.doc.page_view, "compact windows flow");
}
