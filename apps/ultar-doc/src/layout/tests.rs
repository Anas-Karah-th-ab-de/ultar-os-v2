//! Layout: mixed Arabic/Latin lines are reordered per paragraph, the flowing view wraps to the
//! view, the page view breaks pages, caret and hit test agree, lists have their markers.

use super::{build, Geo, Layout, ParaCache};
use crate::state::Cursor;
use ultar_office::doc::{Block, Dir, Document, Para, Style};
use ultar_ui::text::TextEngine;

fn geo(page_view: bool, view_w: f32) -> Geo {
    Geo {
        page_view,
        view_w,
        scale: 1.0,
        zoom: 1.0,
    }
}

fn lay(doc: &Document, g: Geo) -> Layout {
    let Ok(mut eng) = TextEngine::new() else {
        panic!("fonts");
    };
    build(doc, &g, &mut eng, &mut ParaCache::default())
}

fn doc_of(paras: &[(&str, Dir)]) -> Document {
    let mut d = Document::from_text("");
    d.blocks.clear();
    for (t, dir) in paras {
        let mut p = Para::new(Style::Normal, t);
        p.dir = *dir;
        d.blocks.push(Block::Para(p));
    }
    d
}

/// The words of the first line, from the left edge to the right edge.
fn visual_words(l: &Layout) -> Vec<String> {
    let line = l.placed[0].line();
    let mut v: Vec<(f32, String)> = line
        .pieces
        .iter()
        .filter(|p| !p.space && !p.text.trim().is_empty())
        .map(|p| (p.x, p.text.trim().to_owned()))
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.into_iter().map(|(_, t)| t).collect()
}

#[test]
fn an_rtl_paragraph_runs_from_the_right_and_keeps_latin_words_whole() {
    let d = doc_of(&[("مرحبا hello عالم", Dir::Rtl)]);
    let l = lay(&d, geo(false, 800.0));
    assert_eq!(l.placed.len(), 1, "one line");
    assert_eq!(visual_words(&l), ["عالم", "hello", "مرحبا"]);
    assert!(l.placed[0].line().rtl);
}

#[test]
fn an_ltr_paragraph_with_arabic_reverses_only_the_arabic_run() {
    let d = doc_of(&[("abc مرحبا بكم def", Dir::Ltr)]);
    let l = lay(&d, geo(false, 800.0));
    assert_eq!(visual_words(&l), ["abc", "بكم", "مرحبا", "def"]);
    assert!(!l.placed[0].line().rtl);
}

#[test]
fn every_paragraph_has_its_own_direction_in_one_document() {
    let d = doc_of(&[("hello", Dir::Ltr), ("مرحبا", Dir::Rtl)]);
    let l = lay(&d, geo(false, 600.0));
    assert!(!l.placed[0].line().rtl && l.placed[1].line().rtl);
    let a = l.caret(&Cursor {
        block: 0,
        cell: None,
        ch: 0,
    });
    let b = l.caret(&Cursor {
        block: 1,
        cell: None,
        ch: 0,
    });
    let (Some(a), Some(b)) = (a, b) else {
        panic!("carets");
    };
    assert!(
        a.x < b.x,
        "the caret of an Arabic paragraph starts on the right"
    );
}

#[test]
fn the_flowing_view_wraps_to_the_width_and_has_one_page() {
    let text = "word ".repeat(80);
    let d = doc_of(&[(&text, Dir::Ltr)]);
    let wide = lay(&d, geo(false, 900.0));
    let narrow = lay(&d, geo(false, 300.0));
    assert!(narrow.placed.len() > wide.placed.len() * 2);
    assert_eq!(narrow.pages.len(), 1);
    assert!(narrow
        .placed
        .iter()
        .all(|p| p.line().pieces.iter().all(|x| x.x + x.w <= 300.0 + 1.0)));
}

#[test]
fn the_page_view_breaks_pages_and_honours_a_page_break() {
    let paras: Vec<(&str, Dir)> = (0..120).map(|_| ("line of text here", Dir::Ltr)).collect();
    let d = doc_of(&paras);
    let l = lay(&d, geo(true, 1000.0));
    assert!(l.pages.len() >= 2, "120 paragraphs need more than one page");
    let mut d2 = doc_of(&[("a", Dir::Ltr), ("b", Dir::Ltr)]);
    if let Block::Para(p) = &mut d2.blocks[1] {
        p.page_break = true;
    }
    let l2 = lay(&d2, geo(true, 1000.0));
    assert_eq!(l2.pages.len(), 2);
    assert_eq!(l2.placed[1].page, 1);
}

#[test]
fn the_paper_is_a4_and_follows_the_page_setting() {
    let mut d = doc_of(&[("x", Dir::Ltr)]);
    let a4 = lay(&d, geo(true, 1000.0));
    d.page = ultar_office::doc::Page::LETTER;
    let letter = lay(&d, geo(true, 1000.0));
    assert!(
        (a4.paper_w - 595.0).abs() < 3.0,
        "A4 is about 595 pt wide: {}",
        a4.paper_w
    );
    assert!((letter.paper_w - 612.0).abs() < 3.0);
}

#[test]
fn hit_test_and_caret_agree() {
    let d = doc_of(&[("hello world", Dir::Ltr), ("مرحبا بالعالم", Dir::Rtl)]);
    let l = lay(&d, geo(false, 600.0));
    for (block, n) in [(0usize, 11usize), (1, 13)] {
        for ch in 0..=n {
            let c = Cursor {
                block,
                cell: None,
                ch,
            };
            let Some(pos) = l.caret(&c) else {
                panic!("no caret at {block}/{ch}");
            };
            let Some(back) = l.hit(pos.x + 0.5, pos.y + pos.h / 2.0) else {
                panic!("no hit at {block}/{ch}");
            };
            assert_eq!(back.block, block);
            assert!(
                back.ch.abs_diff(ch) <= 1,
                "block {block}: caret at {ch} hit as {}",
                back.ch
            );
        }
    }
}

#[test]
fn list_markers_are_placed_and_digits_are_latin() {
    let mut d = doc_of(&[("one", Dir::Ltr), ("two", Dir::Ltr)]);
    for b in &mut d.blocks {
        if let Block::Para(p) = b {
            p.style = Style::Number;
        }
    }
    let l = lay(&d, geo(true, 1000.0));
    let m1 = l.placed[0].line().marker.as_ref().map(|m| m.text.clone());
    let m2 = l.placed[1].line().marker.as_ref().map(|m| m.text.clone());
    assert_eq!(m1.as_deref(), Some("1."));
    assert_eq!(m2.as_deref(), Some("2."));
}

#[test]
fn selection_rectangles_cover_the_selected_lines() {
    let d = doc_of(&[("first line", Dir::Ltr), ("second line", Dir::Ltr)]);
    let l = lay(&d, geo(false, 600.0));
    let a = Cursor {
        block: 0,
        cell: None,
        ch: 2,
    };
    let b = Cursor {
        block: 1,
        cell: None,
        ch: 4,
    };
    let rects = l.selection_rects(&a, &b);
    assert!(rects.len() >= 2);
    assert!(rects.iter().all(|r| r.w > 0.0 && r.h > 0.0));
}

#[test]
fn a_table_has_boxes_and_its_cells_have_lines() {
    let mut d = doc_of(&[("before", Dir::Ltr)]);
    d.blocks.push(Block::Table(vec![
        vec!["a".into(), "b".into()],
        vec!["c".into(), "dd".into()],
    ]));
    let l = lay(&d, geo(true, 1000.0));
    assert_eq!(l.tables.len(), 1);
    assert_eq!(l.tables[0].rows.len(), 2);
    assert_eq!(l.tables[0].col_x.len(), 2);
    assert!(l.placed.iter().filter(|p| p.cell.is_some()).count() >= 4);
}
