//! The reader against a file in Word's dialect (`fixtures/word-style.docx`, assembled by
//! `fixtures/make_word_style.py`; not exported by Word itself) and the round trip of what
//! the model carries.

use ultar_office::doc::{Block, Dir, Document, Style};
use ultar_office::docx;

const FIXTURE: &[u8] = include_bytes!("fixtures/word-style.docx");

fn read() -> Document {
    docx::read(FIXTURE).expect("fixture reads")
}

fn paras(d: &Document) -> Vec<&ultar_office::doc::Para> {
    d.paras().collect()
}

#[test]
fn word_dialect_is_read_with_its_formatting() {
    let d = read();
    let p = paras(&d);
    assert_eq!(p[0].style, Style::Title);
    assert!(p[0].rtl(), "Arabic title with <w:bidi/>");
    assert_eq!(p[0].text(), "تقرير المشروع");
    assert_eq!(p[1].style, Style::H1);
    // plain, bold, italic+underline, highlight, colour+size
    let runs = &p[2].runs;
    assert!(runs.iter().any(|r| r.bold && r.text == "world"));
    assert!(runs
        .iter()
        .any(|r| r.italic && r.underline && r.text == "styled"));
    assert!(runs
        .iter()
        .any(|r| r.highlight.as_deref() == Some("FFFF00") && r.text == "marked"));
    let red = runs.iter().find(|r| r.text == "red big").expect("red run");
    assert_eq!(red.color.as_deref(), Some("FF0000"));
    assert_eq!(red.size_pt, Some(14.0));
    // the hyperlink text is kept
    assert!(p[3].text().contains("the site"));
}

#[test]
fn lists_page_break_table_and_section_of_word() {
    let d = read();
    let styles: Vec<Style> = paras(&d).iter().map(|p| p.style).collect();
    assert_eq!(styles.iter().filter(|s| **s == Style::Bullet).count(), 2);
    assert_eq!(styles.iter().filter(|s| **s == Style::Number).count(), 2);
    let labels: Vec<String> = d.list_labels().into_iter().flatten().collect();
    assert_eq!(labels, ["•", "•", "1.", "2."]);
    // the page break paragraph is gone, the next paragraph starts the page
    let after = paras(&d)
        .into_iter()
        .find(|p| p.text() == "After the break")
        .expect("paragraph");
    assert!(after.page_break);
    assert!(paras(&d)
        .iter()
        .all(|p| !(p.text().is_empty() && !p.page_break)));
    let table = d.blocks.iter().find_map(|b| match b {
        Block::Table(t) => Some(t),
        _ => None,
    });
    let table = table.expect("table");
    assert_eq!(table[0], ["Header"]);
    assert_eq!(table[1], ["a", "b"]);
    assert!(
        d.to_text().contains("inserted text"),
        "tracked insertion text is kept"
    );
    // Letter, one inch margins
    assert!(d.page.is_letter());
    assert!((d.page.margin_left - 72.0).abs() < 0.01 && (d.page.margin_top - 72.0).abs() < 0.01);
}

#[test]
fn what_the_model_cannot_keep_is_named() {
    let lost = docx::lossy_features(FIXTURE);
    for key in [
        "headers_footers",
        "links",
        "tracked_changes",
        "merged_cells",
    ] {
        assert!(lost.contains(&key), "{key} missing in {lost:?}");
    }
    assert!(!lost.contains(&"footnotes"));
    // a file of our own writer loses nothing
    let own = docx::write(&Document::from_markdown("# Titel\n\ntext **fett**\n")).expect("write");
    assert_eq!(docx::lossy_features(&own), Vec::<&str>::new());
}

#[test]
fn what_the_model_carries_survives_a_save() {
    let d = read();
    let back = docx::read(&docx::write(&d).expect("write")).expect("read back");
    let (a, b) = (paras(&d), paras(&back));
    let texts = |v: &[&ultar_office::doc::Para]| -> Vec<String> {
        v.iter()
            .filter(|p| !p.text().is_empty())
            .map(|p| p.text())
            .collect()
    };
    assert_eq!(texts(&a), texts(&b));
    let after = b
        .iter()
        .find(|p| p.text() == "After the break")
        .expect("paragraph");
    assert!(after.page_break, "the page break survives");
    assert!(b[0].rtl() && b[0].dir == Dir::Rtl);
    assert!(b[2].runs.iter().any(|r| r.bold && r.text == "world"));
    assert!(b[2]
        .runs
        .iter()
        .any(|r| r.highlight.as_deref() == Some("FFFF00")));
    assert!(back.page.is_letter(), "paper size survives");
    assert!((back.page.margin_right - 72.0).abs() < 0.01);
    assert_eq!(
        d.list_labels().into_iter().flatten().count(),
        back.list_labels().into_iter().flatten().count()
    );
}

#[test]
fn a_page_break_inside_text_moves_the_rest_to_the_next_page() {
    use std::io::Write;
    // "one<page break>two" in one paragraph, `pageBreakBefore` on the next one
    let xml = r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:r><w:t>one</w:t></w:r><w:r><w:br w:type="page"/></w:r><w:r><w:t>two</w:t></w:r></w:p>
<w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:r><w:t>three</w:t></w:r></w:p>
<w:p><w:r><w:t>four</w:t></w:r></w:p></w:body></w:document>"#;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        z.start_file("word/document.xml", opts).expect("start");
        z.write_all(xml.as_bytes()).expect("write");
        z.finish().expect("finish");
    }
    let d = docx::read(buf.get_ref()).expect("reads");
    let p = paras(&d);
    let shown: Vec<(String, bool)> = p.iter().map(|p| (p.text(), p.page_break)).collect();
    assert_eq!(
        shown,
        [
            ("one".to_owned(), false),
            ("two".to_owned(), true),
            ("three".to_owned(), true),
            ("four".to_owned(), false)
        ]
    );
}
