//! One worksheet part written: view, columns, rows and cells (formulas with their result).

use super::styles_out::{s_attr, StyleTables};
use crate::docx::esc;
use crate::sheet::{cell_name, col_name, fmt_num, Sheet, Value, Workbook};

/// The `<pane>` of a sheet with frozen rows or columns (empty without).
fn pane_xml(sh: &Sheet) -> String {
    if sh.frozen_rows == 0 && sh.frozen_cols == 0 {
        return String::new();
    }
    let corner = cell_name(sh.frozen_rows, sh.frozen_cols);
    let xs = format!(r#" xSplit="{}""#, sh.frozen_cols);
    let ys = format!(r#" ySplit="{}""#, sh.frozen_rows);
    let (active, x, y) = match (sh.frozen_cols > 0, sh.frozen_rows > 0) {
        (true, true) => ("bottomRight", xs, ys),
        (true, false) => ("topRight", xs, String::new()),
        _ => ("bottomLeft", String::new(), ys),
    };
    format!(r#"<pane{x}{y} topLeftCell="{corner}" activePane="{active}" state="frozen"/>"#)
}

/// The `<c>` of a formula: the formula and its calculated result.
fn formula_cell(rf: &str, sa: &str, f: &str, v: &Value) -> String {
    let (ty, vv) = match v {
        Value::Num(n) => ("", fmt_num(*n)),
        Value::Bool(b) => (r#" t="b""#, if *b { "1".into() } else { "0".into() }),
        Value::Err(e) => (r#" t="e""#, e.clone()),
        Value::Text(x) => (r#" t="str""#, x.clone()),
        Value::Empty => (r#" t="str""#, String::new()),
    };
    format!(
        r#"<c r="{rf}"{sa}{ty}><f>{}</f><v>{}</v></c>"#,
        esc(f),
        esc(&vv)
    )
}

/// The `<c>` of a cell that holds text (an inline string).
fn text_cell(rf: &str, sa: &str, text: &str) -> String {
    format!(
        r#"<c r="{rf}"{sa} t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
        esc(text)
    )
}

/// The `<c>` of a constant: a number, a truth value or text.
fn constant_cell(rf: &str, sa: &str, raw: &str) -> String {
    match crate::sheet::literal(raw) {
        Value::Num(n) => format!(r#"<c r="{rf}"{sa}><v>{}</v></c>"#, fmt_num(n)),
        Value::Bool(b) => format!(r#"<c r="{rf}"{sa} t="b"><v>{}</v></c>"#, b as u8),
        _ => text_cell(rf, sa, raw),
    }
}

/// The `<sheetData>` rows of a sheet.
fn rows_xml(wb: &Workbook, si: usize, t: &StyleTables, calc: &mut crate::calc::Calc) -> String {
    let sh = &wb.sheets[si];
    let mut s = String::from("<sheetData>");
    let mut cur_row: Option<u32> = None;
    for (&(r, c), raw) in &sh.cells {
        if cur_row != Some(r) {
            if cur_row.is_some() {
                s.push_str("</row>");
            }
            let ht = sh
                .heights
                .get(&r)
                .map(|h| format!(r#" ht="{h}" customHeight="1""#))
                .unwrap_or_default();
            s.push_str(&format!(r#"<row r="{rr}"{ht}>"#, rr = r + 1));
            cur_row = Some(r);
        }
        let rf = format!("{}{}", col_name(c), r + 1);
        let sa = s_attr(&t.index, &sh.style(r, c));
        if let Some(f) = raw.strip_prefix('=') {
            s.push_str(&formula_cell(&rf, &sa, f, &calc.value(wb, si, r, c)));
        } else if let Some(text) = raw.strip_prefix('\'') {
            s.push_str(&text_cell(&rf, &sa, text));
        } else {
            s.push_str(&constant_cell(&rf, &sa, raw));
        }
    }
    if cur_row.is_some() {
        s.push_str("</row>");
    }
    s.push_str("</sheetData>");
    s
}

pub(super) fn sheet_xml(
    wb: &Workbook,
    si: usize,
    t: &StyleTables,
    calc: &mut crate::calc::Calc,
) -> String {
    let sh = &wb.sheets[si];
    let (rows, cols) = sh.extent();
    let dim = if rows == 0 {
        "A1".to_string()
    } else {
        format!("A1:{}", cell_name(rows - 1, cols.saturating_sub(1)))
    };
    let view = format!(
        r#"<sheetView workbookViewId="0"{}>{}</sheetView>"#,
        if sh.rtl { r#" rightToLeft="1""# } else { "" },
        pane_xml(sh)
    );
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><dimension ref="{dim}"/><sheetViews>{view}</sheetViews><sheetFormatPr defaultRowHeight="15"/>"#
    );
    if !sh.widths.is_empty() {
        s.push_str("<cols>");
        for (c, w) in &sh.widths {
            s.push_str(&format!(
                r#"<col min="{0}" max="{0}" width="{1}" customWidth="1"/>"#,
                c + 1,
                w
            ));
        }
        s.push_str("</cols>");
    }
    s.push_str(&rows_xml(wb, si, t, calc));
    s.push_str(r#"<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/></worksheet>"#);
    s
}
