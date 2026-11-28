//! The PDF writer: the file is well formed (cross-reference table points at the objects) and
//! the reader of this crate opens it, sees the pages, draws them and finds the text.

use super::*;
use crate::pdf::PdfDoc;

static FONT: &[u8] = include_bytes!("../../../../assets/fonts/NotoSans-Regular.ttf");

fn sample() -> Vec<u8> {
    let mut pdf = Pdf::new();
    pdf.set_title("Test");
    let font = pdf.add_font("Noto Sans", FONT).unwrap_or_default();
    let img = pdf
        .add_rgb(2, 2, vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0])
        .unwrap_or_default();
    let p0 = pdf.add_page(595.0, 842.0);
    pdf.add_page(612.0, 792.0);
    if let Some(p) = pdf.page(p0) {
        p.fill_rect(50.0, 50.0, 200.0, 40.0, [255, 230, 100]);
        p.stroke_rect(50.0, 50.0, 200.0, 40.0, 1.0, [0, 0, 0]);
        p.image(img, 300.0, 100.0, 80.0, 80.0);
    }
    // Glyph numbers of the font; the text map says what they stand for.
    for (i, c) in "Hello".chars().enumerate() {
        pdf.glyph(
            p0,
            font,
            40 + i as u16 * 3,
            24.0,
            (60.0 + i as f32 * 14.0, 80.0),
            0.0,
            [0, 0, 0],
            &c.to_string(),
        );
    }
    pdf.glyph(1, font, 50, 12.0, (72.0, 100.0), 0.2, [10, 20, 30], "x");
    pdf.finish()
}

#[test]
fn the_file_is_well_formed_with_a_correct_cross_reference_table() {
    let b = sample();
    assert!(b.starts_with(b"%PDF-1.7\n"));
    assert!(b.ends_with(b"%%EOF\n"));
    let tail_at = b
        .windows(10)
        .rposition(|w| w == b"startxref\n")
        .unwrap_or(0);
    let tail = String::from_utf8_lossy(&b[tail_at + 10..]).into_owned();
    let start: usize = tail
        .lines()
        .next()
        .and_then(|l| l.parse().ok())
        .unwrap_or(usize::MAX);
    assert!(
        b[start..].starts_with(b"xref\n"),
        "startxref points at the table"
    );
    let table = String::from_utf8_lossy(&b[start..tail_at]).into_owned();
    let mut lines = table.lines().skip(1);
    let size: usize = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert!(size > 10);
    lines.next();
    for id in 1..size {
        let off: usize = lines.next().and_then(|l| l[..10].parse().ok()).unwrap_or(0);
        let want = format!("{id} 0 obj");
        assert!(
            b[off..].starts_with(want.as_bytes()),
            "object {id} is at {off}"
        );
    }
}

#[test]
fn the_reader_opens_it_sees_both_pages_and_draws_them() {
    let Ok(doc) = PdfDoc::open(sample()) else {
        panic!("the reader refused the file");
    };
    assert_eq!(doc.pages(), 2);
    let (w, h) = doc.page_size(0);
    assert!((w - 595.0).abs() < 1.0 && (h - 842.0).abs() < 1.0);
    let (w1, _) = doc.page_size(1);
    assert!((w1 - 612.0).abs() < 1.0);
    let Some(img) = doc.render(0, 300) else {
        panic!("render");
    };
    let drawn = img
        .rgba
        .chunks_exact(4)
        .filter(|p| p[0] < 250 || p[1] < 250 || p[2] < 250)
        .count();
    assert!(drawn > 200, "the page has drawn content: {drawn}");
}

#[test]
fn text_can_be_copied_through_the_unicode_map() {
    let Ok(doc) = PdfDoc::open(sample()) else {
        panic!("open");
    };
    assert!(
        doc.page_text(0).contains("Hello"),
        "got {:?}",
        doc.page_text(0)
    );
}

#[test]
fn the_font_tables_are_read() {
    let Some(t) = ttf_info(FONT) else {
        panic!("font");
    };
    assert!(t.units == 1000 || t.units == 2048);
    assert!(t.advance(FONT, 3) > 0);
    assert!(ttf_info(b"not a font at all").is_none());
}

#[test]
fn a_jpeg_header_gives_size_and_components() {
    let jpg = [
        0xFF, 0xD8, 0xFF, 0xC0, 0, 17, 8, 0, 16, 0, 32, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1,
        0xFF, 0xD9,
    ];
    assert_eq!(jpeg_info(&jpg), Some((32, 16, 3)));
    assert_eq!(jpeg_info(b"nope"), None);
    let mut pdf = Pdf::new();
    assert!(pdf.add_jpeg(&jpg).is_ok());
    assert!(pdf.add_jpeg(b"nope").is_err());
    assert!(pdf.add_rgb(2, 2, vec![0; 5]).is_err());
}

#[test]
fn an_empty_document_still_has_one_page() {
    let Ok(doc) = PdfDoc::open(Pdf::new().finish()) else {
        panic!("open");
    };
    assert_eq!(doc.pages(), 1);
}
