//! .xlsx files from other programs (tests/fixtures) and the round trip of a file this library
//! made itself.

use ultar_office::numfmt::{format as format_number_with, Names};
use ultar_office::sheet::{Value, Workbook};
use ultar_office::style::Align;

fn format_number(n: f64, code: &str) -> String {
    format_number_with(n, code, &Names::default())
}
use ultar_office::xlsx;

const NOW: f64 = 45_366.75;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn val(wb: &Workbook, s: usize, cell: &str) -> Value {
    let (_, (r, c), _) = ultar_office::sheet::parse_range(cell).unwrap();
    wb.value_at(s, r, c, NOW)
}

#[test]
fn a_file_written_by_openpyxl_is_read_completely() {
    let wb = xlsx::read(&fixture("openpyxl-sample.xlsx")).unwrap();
    assert_eq!(wb.sheets.len(), 2);
    let u = &wb.sheets[0];
    assert_eq!(u.name, "Umsatz");
    assert_eq!(val(&wb, 0, "A2"), Value::Text("يناير".into()));
    assert_eq!(val(&wb, 0, "A3"), Value::Text("Feb & <März>".into()));
    assert_eq!(val(&wb, 0, "B2"), Value::Num(1200.5));
    // formulas arrive without a cached result: the engine calculates them
    assert_eq!(u.get(3, 1), "=SUM(B2:B3)");
    assert_eq!(val(&wb, 0, "B4"), Value::Num(2000.5));
    assert_eq!(val(&wb, 0, "F1"), Value::Text("hoch".into()));
    assert_eq!(val(&wb, 0, "E1"), Value::Bool(true));
    assert_eq!(val(&wb, 1, "A1"), Value::Num(1000.25));
    assert_eq!(val(&wb, 1, "A2"), Value::Num(4001.0));
    // formats
    let pct = u.style(1, 2).num_fmt;
    assert_eq!(format_number(0.6, &pct), "60.0%");
    assert_eq!(format_number(45292.0, &u.style(0, 3).num_fmt), "2024-01-01");
    assert_eq!(
        format_number(1_234_567.891, &u.style(0, 6).num_fmt),
        "1,234,567.89"
    );
    let head = u.style(0, 0);
    assert!(head.bold);
    assert_eq!(head.color, Some(0xFF0000));
    assert_eq!(u.style(4, 0).fill, Some(0xFFFF00));
    let mid = u.style(4, 1);
    assert_eq!(mid.align, Align::Center);
    assert!(mid.wrap);
    assert_eq!(val(&wb, 0, "B5"), Value::Text("mehr\nZeilen".into()));
    // layout
    assert_eq!(u.width(0), 18.0);
    assert_eq!(u.height(0), 30.0);
    assert_eq!((u.frozen_rows, u.frozen_cols), (1, 1));
    assert!(!u.rtl);
    assert!(wb.sheets[1].rtl);
}

#[test]
fn a_file_in_the_layout_of_excel_is_read_completely() {
    let wb = xlsx::read(&fixture("excel-like.xlsx")).unwrap();
    assert_eq!(
        wb.sheets
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["Daten", "Zweites"]
    );
    let d = &wb.sheets[0];
    // shared strings, rich text runs joined, white space kept
    assert_eq!(val(&wb, 0, "A1"), Value::Text("Name".into()));
    assert_eq!(val(&wb, 0, "C1"), Value::Text("مرحبا".into()));
    assert_eq!(val(&wb, 0, "D1"), Value::Text("Fett und normal".into()));
    assert_eq!(val(&wb, 0, "E1"), Value::Text("  Leerzeichen  ".into()));
    assert_eq!(val(&wb, 0, "A6"), Value::Text("inline".into()));
    // self-closing <xf/> entries and the ones with an <alignment> child
    let head = d.style(0, 0);
    assert!(head.bold && head.italic);
    assert_eq!(head.color, Some(0x0000FF));
    assert_eq!(format_number(0.255, &d.style(1, 1).num_fmt), "26%");
    assert_eq!(d.style(1, 3).fill, Some(0xFFFF00));
    assert_eq!(
        format_number(1234.5, &d.style(1, 3).num_fmt),
        "1,234.50 EUR"
    );
    assert_eq!(d.style(1, 4).align, Align::Center);
    assert!(d.style(1, 4).wrap);
    // dates (built-in format 14)
    assert!(
        format_number(45292.0, &d.style(1, 2).num_fmt).contains("2024")
            || format_number(45292.0, &d.style(1, 2).num_fmt).ends_with("24")
    );
    // every kind of cached result: recalculated, not copied
    assert_eq!(val(&wb, 0, "B3"), Value::Err("#DIV/0!".into()));
    assert_eq!(val(&wb, 0, "C3"), Value::Bool(true));
    assert_eq!(val(&wb, 0, "D3"), Value::Err("#N/A".into()));
    assert_eq!(val(&wb, 0, "E3"), Value::Text("x10".into()));
    assert_eq!(val(&wb, 0, "B4"), Value::Num(14.0));
    assert_eq!(val(&wb, 0, "C4"), Value::Text("big".into()));
    assert_eq!(val(&wb, 0, "D4"), Value::Num(15.0));
    assert_eq!(val(&wb, 1, "A1"), Value::Num(14.0));
    // layout
    assert_eq!(d.width(0), 14.7109375);
    assert_eq!(d.width(1), 14.7109375);
    assert_eq!(d.height(1), 30.0);
    assert_eq!((d.frozen_rows, d.frozen_cols), (1, 1));
    assert!(wb.sheets[1].rtl && !d.rtl);
}

#[test]
fn what_the_app_writes_reads_back_the_same() {
    let mut wb = xlsx::read(&fixture("excel-like.xlsx")).unwrap();
    wb.sheets[0].set(10, 0, "=VLOOKUP(10,A2:E4,2,FALSE)");
    wb.sheets[0].set(11, 0, "'007");
    wb.sheets[0].set(12, 0, "اختبار");
    wb.sheets[1].frozen_rows = 2;
    let bytes = xlsx::write(&wb).unwrap();
    let back = xlsx::read(&bytes).unwrap();
    assert_eq!(back.sheets.len(), 2);
    for (a, b) in wb.sheets.iter().zip(&back.sheets) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.cells, b.cells, "cells of {}", a.name);
        assert_eq!(a.styles, b.styles, "styles of {}", a.name);
        assert_eq!(a.heights, b.heights);
        assert_eq!(a.widths, b.widths);
        assert_eq!(
            (a.rtl, a.frozen_rows, a.frozen_cols),
            (b.rtl, b.frozen_rows, b.frozen_cols)
        );
    }
    for c in ["B3", "C3", "D3", "E3", "B4", "C4", "D4", "A11"] {
        assert_eq!(val(&wb, 0, c), val(&back, 0, c), "{c}");
    }
    // an Arabic cell on a left-to-right sheet stays left to right
    assert!(!back.sheets[0].rtl);
    // writing it again changes nothing
    assert_eq!(
        xlsx::read(&xlsx::write(&back).unwrap()).unwrap().sheets[0].cells,
        back.sheets[0].cells
    );
}

#[test]
fn a_chain_of_formulas_is_written_with_the_right_results() {
    let mut wb = Workbook::new();
    wb.sheets[0].set(0, 0, "1");
    for r in 1..2000 {
        wb.sheets[0].set(r, 0, &format!("=A{r}+1"));
    }
    let bytes = xlsx::write(&wb).unwrap();
    let x = ultar_office::docx::read_zip_entry(&bytes, "xl/worksheets/sheet1.xml").unwrap();
    let x = String::from_utf8(x).unwrap();
    assert!(x.contains("<f>A1999+1</f><v>2000</v>"));
}

#[test]
fn damaged_and_hostile_files_give_errors_not_crashes() {
    assert!(xlsx::read(b"").is_err());
    assert!(xlsx::read(b"not a zip at all").is_err());
    let good = fixture("excel-like.xlsx");
    // a zip cut in the middle
    assert!(xlsx::read(&good[..good.len() / 2]).is_err());
    // a sheet whose XML is broken
    let mut broken = good.clone();
    let at = broken
        .windows(5)
        .position(|w| w == b"<dime")
        .map_or(0, |p| p);
    // (deflated: flipping bytes breaks the stream or the XML, either is an error or a sheet)
    for i in 0..40 {
        if let Some(b) = broken.get_mut(at + i) {
            *b ^= 0xFF;
        }
    }
    let _ = xlsx::read(&broken);
    // no panic on any prefix
    for n in (0..good.len()).step_by(97) {
        let _ = xlsx::read(&good[..n]);
    }
}
