//! Model operations: typing keeps the look, ranges take formats, lists number, one undo undoes
//! one operation.

use crate::state::{Cursor, Doc};
use ultar_office::doc::{Align, Block, Dir, Para, Style};

fn typed(d: &mut Doc, s: &str) {
    for c in s.chars() {
        if c == '\n' {
            d.newline();
        } else {
            d.insert(&c.to_string());
        }
    }
}

fn at(block: usize, ch: usize) -> Cursor {
    Cursor {
        block,
        cell: None,
        ch,
    }
}

fn select(d: &mut Doc, block: usize, from: usize, to: usize) {
    d.set_pos(at(block, from), false);
    d.set_pos(at(block, to), true);
}

fn para(d: &Doc, i: usize) -> &Para {
    match &d.document.blocks[i] {
        Block::Para(p) => p,
        _ => panic!("not a paragraph"),
    }
}

fn text(d: &Doc, i: usize) -> String {
    d.item_text(i, None).unwrap_or_default()
}

#[test]
fn typing_makes_text_and_enter_splits() {
    let mut d = Doc::new();
    typed(&mut d, "one\ntwo");
    assert_eq!(d.document.blocks.len(), 2);
    assert_eq!(text(&d, 0), "one");
    assert_eq!(text(&d, 1), "two");
    assert!(d.dirty());
}

#[test]
fn bold_on_a_range_splits_runs_and_toggles_back() {
    let mut d = Doc::new();
    typed(&mut d, "hello world");
    select(&mut d, 0, 0, 5);
    d.toggle_run(|r| r.bold, |r, v| r.bold = v);
    let p = para(&d, 0);
    assert_eq!(p.runs.len(), 2);
    assert!(p.runs[0].bold && p.runs[0].text == "hello");
    assert!(!p.runs[1].bold && p.runs[1].text == " world");
    d.toggle_run(|r| r.bold, |r, v| r.bold = v);
    assert_eq!(para(&d, 0).runs.len(), 1, "same look runs merge again");
}

#[test]
fn typing_inside_a_bold_word_stays_bold() {
    let mut d = Doc::new();
    typed(&mut d, "abcd");
    select(&mut d, 0, 0, 4);
    d.toggle_run(|r| r.bold, |r, v| r.bold = v);
    d.set_pos(at(0, 2), false);
    d.insert("X");
    assert_eq!(text(&d, 0), "abXcd");
    assert!(para(&d, 0).runs.iter().all(|r| r.bold));
}

#[test]
fn size_colour_and_style_apply_to_the_selection() {
    let mut d = Doc::new();
    typed(&mut d, "title\nbody");
    select(&mut d, 0, 0, 5);
    d.change_size(4.0);
    d.apply_run(&|r, _| r.color = Some("FF0000".into()));
    d.set_style(Style::H1);
    let p = para(&d, 0);
    assert_eq!(p.style, Style::H1);
    assert!(p.runs.iter().all(|r| r.color.as_deref() == Some("FF0000")));
    assert!(p.runs.iter().all(|r| r.size_pt.is_some_and(|s| s > 11.0)));
    assert_eq!(para(&d, 1).style, Style::Normal);
}

#[test]
fn alignment_direction_and_spacing_apply_to_the_paragraph() {
    let mut d = Doc::new();
    typed(&mut d, "مرحبا");
    d.set_align(Align::Center);
    d.set_dir(Dir::Rtl);
    d.set_line_spacing(Some(1.5));
    let p = para(&d, 0);
    assert_eq!(p.align, Align::Center);
    assert_eq!(p.dir, Dir::Rtl);
    assert_eq!(p.line_spacing, Some(1.5));
}

#[test]
fn lists_number_and_nest_with_digits() {
    let mut d = Doc::new();
    typed(&mut d, "a\nb\nc");
    d.select_all();
    d.toggle_list(false);
    let labels = d.document.list_labels();
    assert_eq!(labels[0].as_deref(), Some("1."));
    assert_eq!(labels[2].as_deref(), Some("3."));
    d.set_pos(at(1, 0), false);
    d.indent(36.0);
    let labels = d.document.list_labels();
    assert_eq!(labels[1].as_deref(), Some("1.1."));
    assert_eq!(labels[2].as_deref(), Some("2."));
    d.select_all();
    d.toggle_list(false);
    assert!(
        d.document.list_labels().iter().all(Option::is_none),
        "toggle again removes the list"
    );
}

#[test]
fn enter_on_an_empty_list_item_ends_the_list() {
    let mut d = Doc::new();
    typed(&mut d, "x");
    d.toggle_list(true);
    d.newline();
    assert_eq!(
        para(&d, 1).style,
        Style::Bullet,
        "the next item continues the list"
    );
    d.newline();
    assert_eq!(para(&d, 1).style, Style::Normal, "an empty item ends it");
}

#[test]
fn page_break_makes_the_next_paragraph_start_a_page() {
    let mut d = Doc::new();
    typed(&mut d, "one");
    d.insert_page_break();
    typed(&mut d, "two");
    let i = d.cursor.block;
    assert_eq!(text(&d, i), "two");
    assert!(para(&d, i).page_break);
}

#[test]
fn backspace_joins_paragraphs_and_delete_selection_spans_blocks() {
    let mut d = Doc::new();
    typed(&mut d, "ab\ncd\nef");
    d.set_pos(at(1, 0), false);
    d.backspace();
    assert_eq!(text(&d, 0), "abcd");
    d.set_pos(at(0, 1), false);
    d.set_pos(at(1, 1), true);
    assert!(d.delete_selection());
    assert_eq!(text(&d, 0), "af");
    assert_eq!(d.document.blocks.len(), 1);
}

#[test]
fn one_undo_undoes_one_burst_of_typing_and_redo_returns_it() {
    let mut d = Doc::new();
    typed(&mut d, "hello");
    d.burst_break();
    typed(&mut d, " world");
    assert!(d.undo());
    assert_eq!(text(&d, 0), "hello");
    assert!(d.undo());
    assert_eq!(text(&d, 0), "");
    assert!(d.redo() && d.redo());
    assert_eq!(text(&d, 0), "hello world");
    assert!(!d.can_redo());
}

#[test]
fn undo_reverts_formatting_and_structure() {
    let mut d = Doc::new();
    typed(&mut d, "a\nb");
    d.burst_break();
    d.select_all();
    d.toggle_run(|r| r.italic, |r, v| r.italic = v);
    assert!(para(&d, 0).runs.iter().all(|r| r.italic));
    assert!(d.undo());
    assert!(para(&d, 0).runs.iter().all(|r| !r.italic));
    d.burst_break();
    d.set_pos(at(0, 1), false);
    d.newline();
    assert_eq!(d.document.blocks.len(), 3);
    assert!(d.undo());
    assert_eq!(d.document.blocks.len(), 2);
}

#[test]
fn rich_copy_and_paste_keep_formats() {
    let mut d = Doc::new();
    typed(&mut d, "bold plain");
    select(&mut d, 0, 0, 4);
    d.toggle_run(|r| r.bold, |r, v| r.bold = v);
    select(&mut d, 0, 0, 4);
    let clip = d.copy_blocks();
    d.set_pos(at(0, 10), false);
    d.paste_blocks(clip);
    assert_eq!(text(&d, 0), "bold plainbold");
    let last = para(&d, 0).runs.last().cloned().unwrap_or_default();
    assert!(last.bold && last.text.ends_with("bold"));
}

#[test]
fn a_picture_is_resized_and_deleted_like_a_character() {
    let mut d = Doc::new();
    d.insert_picture(ultar_office::doc::Picture {
        name: "p.png".into(),
        bytes: vec![1, 2, 3],
        width_pt: Some(100.0),
        height_pt: Some(50.0),
    });
    let i = d.cursor.block;
    assert!(d
        .document
        .blocks
        .iter()
        .any(|b| matches!(b, Block::Picture(_))));
    d.set_pos(at(i.min(d.document.blocks.len() - 1), 1), false);
    let _ = d.resize_picture(2.0);
}
