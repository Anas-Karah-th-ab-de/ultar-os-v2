//! Helpers of the flow tests: a window over a temporary "areas" folder, input through the
//! same path the compositor client uses (the widget tree first, then what it did not use).
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use ultar_doc::state::Doc;
use ultar_doc::App;
use ultar_i18n::Lang;
use ultar_ui::event::{Action, Event, Key, Modifiers, PointerButton};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::{Point, Size};

/// A fresh areas folder with `documents/`.
pub fn areas(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ultar-doc-t-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("documents")).unwrap();
    d
}

pub fn ui(lang: Lang, theme: ThemeKind, size: (f32, f32)) -> Ui {
    let env = PaintEnv::new(lang, theme);
    Ui::new(
        env,
        InputMode::Pointer,
        Size::new(size.0, size.1),
        ultar_doc::i18n::translator(lang),
    )
    .unwrap()
}

/// A window of `size` points around `doc`.
pub fn app_with(areas: &Path, doc: Doc, lang: Lang, theme: ThemeKind, size: (f32, f32)) -> App {
    let mut app = App::new(ui(lang, theme, size), doc, areas.to_path_buf(), None).unwrap();
    app.rebuild().unwrap();
    app.take_out();
    app
}

/// A new empty document in a wide English window.
pub fn app(areas: &Path) -> App {
    app_with(
        areas,
        Doc::new(),
        Lang::En,
        ThemeKind::Dark,
        (1280.0, 800.0),
    )
}

/// A wide English window around a document made from `text` (no typing: big texts are slow
/// to type key by key in a test build).
pub fn app_text(areas: &Path, text: &str) -> App {
    let doc = Doc::from_document(ultar_office::doc::Document::from_text(text));
    app_with(areas, doc, Lang::En, ThemeKind::Dark, (1280.0, 800.0))
}

pub fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Modifiers::default()
    }
}

pub fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::default()
    }
}

/// A key press the way the window delivers it.
pub fn key(app: &mut App, key: Key, modifiers: Modifiers) {
    let ev = Event::Key { key, modifiers };
    if app.text_focused() && (key != Key::Tab || modifiers.ctrl) {
        app.key(ev).unwrap();
        return;
    }
    let r = app.ui.handle(ev);
    let mut actions: Vec<Action> = r.actions;
    actions.extend(app.ui.take_actions());
    app.actions(actions).unwrap();
    if !r.consumed {
        app.key(ev).unwrap();
    }
}

pub fn press(app: &mut App, key_: Key) {
    key(app, key_, Modifiers::default());
}

pub fn chord(app: &mut App, c: char) {
    key(app, Key::Char(c), ctrl());
}

/// Types text (a newline is Enter, a space the space key).
pub fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        match c {
            '\n' => press(app, Key::Enter),
            ' ' => press(app, Key::Space),
            c => press(app, Key::Char(c)),
        }
    }
}

/// The text of a block of the document.
pub fn text(app: &App, block: usize) -> String {
    app.doc.item_text(block, None).unwrap_or_default()
}

fn send(app: &mut App, events: &[Event]) {
    let mut actions: Vec<Action> = Vec::new();
    for &e in events {
        actions.extend(app.ui.handle(e).actions);
    }
    actions.extend(app.ui.take_actions());
    // The window hands the pointer to the document first: a click that closes a dialog must
    // not also land on the text below it.
    for &e in events {
        app.pointer(e).unwrap();
    }
    app.actions(actions).unwrap();
}

/// One pointer or touch event through the tree and then to the app, as the window does.
pub fn pointer(app: &mut App, event: Event) {
    send(app, &[event]);
}

pub fn click_at(app: &mut App, pos: Point) {
    send(
        app,
        &[
            Event::PointerMove { pos },
            Event::PointerDown {
                pos,
                button: PointerButton::Primary,
            },
            Event::PointerUp {
                pos,
                button: PointerButton::Primary,
            },
        ],
    );
}

/// Clicks the centre of a widget.
pub fn click(app: &mut App, id: NodeId) {
    app.ui.layout();
    let r = app.ui.rect(id).unwrap();
    click_at(app, Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0));
}

/// Where a document position is on the screen (points), from the layout.
pub fn screen_pos(app: &mut App, c: ultar_doc::state::Cursor) -> Point {
    app.draw().unwrap();
    let left = app.paper_x();
    let pos = app.lay.as_ref().unwrap().caret(&c).unwrap();
    let r = app.ui.rect(app.nodes.text.unwrap()).unwrap();
    let s = app.ui.env().scale;
    Point::new(
        r.x + (left + pos.x + 1.0) / s,
        r.y + (pos.y - app.scroll.1 + pos.h / 2.0) / s,
    )
}

/// Types into the dialog's name field and confirms with its default button.
pub fn answer_name(app: &mut App, name: &str) {
    let field = app.nodes.name_field.unwrap();
    app.ui.set_text(field, name).unwrap();
    let ok = app.nodes.ok.unwrap();
    click(app, ok);
}

pub fn sample_png(path: &Path) {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, 4, 2);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().unwrap();
    w.write_image_data(&[200, 30, 30].repeat(8)).unwrap();
    drop(w);
    std::fs::write(path, out).unwrap();
}

/// Clicks the button of `cmd` in the tool bar.
pub fn press_cmd(app: &mut App, cmd: ultar_doc::commands::Cmd) {
    let n = app.command_node(cmd).unwrap();
    click(app, n);
}

/// The first table of the document.
pub fn first_table(app: &App) -> Vec<Vec<String>> {
    app.doc
        .document
        .blocks
        .iter()
        .find_map(|b| match b {
            ultar_office::doc::Block::Table(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap()
}

/// The first picture of the document.
pub fn first_picture(app: &App) -> ultar_office::doc::Picture {
    app.doc
        .document
        .blocks
        .iter()
        .find_map(|b| match b {
            ultar_office::doc::Block::Picture(p) => Some(p.clone()),
            _ => None,
        })
        .unwrap()
}

/// Clicks the confirming button of the open dialog.
pub fn click_ok(app: &mut App) {
    let n = app.nodes.ok.unwrap();
    click(app, n);
}

/// Clicks the cancelling button of the open dialog.
pub fn click_cancel(app: &mut App) {
    let n = app.nodes.cancel.unwrap();
    click(app, n);
}
