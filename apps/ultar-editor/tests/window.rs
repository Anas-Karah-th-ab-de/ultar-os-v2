//! The editor window headless: open a file of an area, type, save, close with changes.

use std::path::PathBuf;
use ultar_editor::app::{Editor, Out};
use ultar_i18n::Lang;
use ultar_textview::Source;
use ultar_ui::event::{Event, Key, Modifiers};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Size;

fn areas(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ultar-editor-win-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("documents")).unwrap();
    d
}

fn editor(uri: Option<&str>, areas: &std::path::Path) -> Editor {
    let ui = Ui::new(
        PaintEnv::new(Lang::Ar, ThemeKind::Dark),
        InputMode::Pointer,
        Size::new(800.0, 500.0),
        ultar_editor::i18n::translator(Lang::Ar),
    )
    .unwrap();
    let (doc, failed) = Editor::open(uri, areas);
    assert_eq!(failed, None);
    Editor::new(ui, doc, areas.to_path_buf()).unwrap()
}

fn key(e: &mut Editor, key: Key, ctrl: bool) {
    let modifiers = Modifiers {
        ctrl,
        ..Modifiers::default()
    };
    e.key(Event::Key { key, modifiers }).unwrap();
}

fn type_text(e: &mut Editor, text: &str) {
    for c in text.chars() {
        key(e, if c == ' ' { Key::Space } else { Key::Char(c) }, false);
    }
}

#[test]
fn opens_types_and_saves_a_file_from_files() {
    let a = areas("save");
    std::fs::write(a.join("documents/ملاحظات.txt"), "سطر\n").unwrap();
    let uri = "files://documents/%D9%85%D9%84%D8%A7%D8%AD%D8%B8%D8%A7%D8%AA.txt";
    let mut e = editor(Some(uri), &a);
    assert_eq!(e.doc.lines(), 2);
    assert!(e
        .take_out()
        .iter()
        .any(|o| matches!(o, Out::Title(t) if t.contains("ملاحظات.txt"))));
    key(&mut e, Key::End, true);
    type_text(&mut e, "مرحبا hello");
    assert!(e.doc.dirty());
    key(&mut e, Key::Char('s'), true);
    assert!(!e.doc.dirty());
    assert_eq!(
        std::fs::read_to_string(a.join("documents/ملاحظات.txt")).unwrap(),
        "سطر\nمرحبا hello"
    );
    e.close_requested().unwrap();
    assert!(e.take_out().contains(&Out::Close));
}

#[test]
fn closing_with_changes_asks_and_a_new_document_asks_for_a_name() {
    let a = areas("ask");
    let mut e = editor(None, &a);
    e.take_out();
    type_text(&mut e, "ملاحظة");
    e.close_requested().unwrap();
    assert!(!e.take_out().contains(&Out::Close), "asks first");
    assert!(e.ui.open_dialog_node().is_some());
    assert!(!e.text_focused());
}

#[test]
fn copy_and_paste_go_through_the_window() {
    let a = areas("clip");
    let mut e = editor(None, &a);
    type_text(&mut e, "abc");
    key(&mut e, Key::Char('a'), true);
    key(&mut e, Key::Char('c'), true);
    assert!(e.take_out().contains(&Out::Copy("abc".into())));
    key(&mut e, Key::Char('v'), true);
    assert!(e.take_out().contains(&Out::Paste));
    e.pasted("\nسطر").unwrap();
    assert_eq!(e.doc.lines(), 2);
}
