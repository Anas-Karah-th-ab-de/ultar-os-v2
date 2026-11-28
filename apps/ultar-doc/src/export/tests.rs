//! PDF export of a real document: pages, text of both scripts, formatting, table, picture.

use super::to_pdf;
use crate::state::{Cursor, Doc};
use ultar_office::doc::{Dir, Picture};
use ultar_office::pdf::PdfDoc;

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

fn png() -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, 2, 2);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    if let Ok(mut w) = enc.write_header() {
        let _ = w.write_image_data(&[255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0]);
    }
    out
}

fn rich() -> Doc {
    let mut d = Doc::new();
    typed(&mut d, "Hello PDF world\nمرحبا بالعالم\nplain");
    d.set_pos(at(0, 0), false);
    d.set_pos(at(0, 5), true);
    d.toggle_run(|r| r.bold, |r, v| r.bold = v);
    d.toggle_run(|r| r.italic, |r, v| r.italic = v);
    d.set_pos(at(1, 0), false);
    d.set_dir(Dir::Rtl);
    d.set_pos(at(2, 5), false);
    d.insert_table(2, 2);
    typed(&mut d, "cell");
    d.insert_picture(Picture {
        bytes: png(),
        name: "p.png".into(),
        width_pt: Some(60.0),
        height_pt: Some(60.0),
    });
    d
}

#[test]
fn a_formatted_mixed_document_exports_to_a_pdf_the_reader_opens() {
    let d = rich();
    let Ok(bytes) = to_pdf(&d, "Test document") else {
        panic!("export");
    };
    assert!(bytes.starts_with(b"%PDF-1.7"));
    let Ok(pdf) = PdfDoc::open(bytes) else {
        panic!("the reader refused our PDF");
    };
    assert_eq!(pdf.pages(), 1);
    let (w, h) = pdf.page_size(0);
    assert!(
        (w - 595.0).abs() < 1.5 && (h - 842.0).abs() < 1.5,
        "A4: {w} x {h}"
    );
    let text = pdf.page_text(0);
    assert!(text.contains("Hello PDF world"), "text: {text:?}");
    assert!(
        text.contains("plain") && text.contains("cell"),
        "text: {text:?}"
    );
    assert!(
        text.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)),
        "Arabic letters survive: {text:?}"
    );
    let Some(img) = pdf.render(0, 400) else {
        panic!("render");
    };
    let red = img
        .rgba
        .chunks_exact(4)
        .filter(|p| p[0] > 200 && p[1] < 60 && p[2] < 60)
        .count();
    assert!(red > 10, "the picture is drawn");
}

#[test]
fn a_long_document_gets_many_pages_and_letter_size_is_kept() {
    let mut d = Doc::new();
    for i in 0..150 {
        typed(&mut d, &format!("paragraph number {i}\n"));
    }
    d.document.page = ultar_office::doc::Page::LETTER;
    let Ok(bytes) = to_pdf(&d, "Long") else {
        panic!("export");
    };
    let Ok(pdf) = PdfDoc::open(bytes) else {
        panic!("open");
    };
    assert!(pdf.pages() >= 3, "pages: {}", pdf.pages());
    let (w, h) = pdf.page_size(0);
    assert!((w - 612.0).abs() < 1.5 && (h - 792.0).abs() < 1.5);
    assert!(pdf.page_text(1).contains("paragraph number"));
}
