//! The style tables written to `xl/styles.xml`: the cell styles of a workbook as numbered
//! number formats, fonts, fills and `cellXfs`.

use crate::docx::esc;
use crate::sheet::Workbook;
use crate::style::{Align, CellStyle};
use std::collections::HashMap;

pub(super) struct StyleTables {
    /// (numFmtId, fontId, fillId, style) in `cellXfs` order.
    pub(super) xfs: Vec<(u32, u32, u32, CellStyle)>,
    /// Style → its `cellXfs` index.
    pub(super) index: HashMap<CellStyle, u32>,
    /// Custom number formats (index 0 → id 164).
    num_fmts: Vec<String>,
    /// Fonts (index 0 is the default).
    fonts: Vec<(bool, bool, Option<u32>)>,
    /// Fills (index 0 none, 1 gray125).
    fills: Vec<Option<u32>>,
}

pub(super) fn build_style_tables(wb: &Workbook) -> StyleTables {
    let mut index: HashMap<CellStyle, u32> = HashMap::new();
    let mut styles: Vec<CellStyle> = Vec::new();
    for sh in &wb.sheets {
        for st in sh.styles.values() {
            if !st.is_default() {
                index.entry(st.clone()).or_insert_with(|| {
                    styles.push(st.clone());
                    (styles.len() - 1) as u32
                });
            }
        }
    }
    let mut fonts: Vec<(bool, bool, Option<u32>)> = vec![(false, false, None)];
    let mut font_index: HashMap<(bool, bool, Option<u32>), u32> = HashMap::new();
    let mut fills: Vec<Option<u32>> = vec![None, None];
    let mut fill_index: HashMap<u32, u32> = HashMap::new();
    let mut num_fmts: Vec<String> = Vec::new();
    let mut fmt_index: HashMap<String, u32> = HashMap::new();
    let mut xfs = Vec::new();
    for st in &styles {
        let fid = if st.bold || st.italic || st.color.is_some() {
            let key = (st.bold, st.italic, st.color);
            *font_index.entry(key).or_insert_with(|| {
                fonts.push(key);
                (fonts.len() - 1) as u32
            })
        } else {
            0
        };
        let flid = match st.fill {
            Some(c) => *fill_index.entry(c).or_insert_with(|| {
                fills.push(Some(c));
                (fills.len() - 1) as u32
            }),
            None => 0,
        };
        let nid = if st.num_fmt.is_empty() || st.num_fmt.eq_ignore_ascii_case("general") {
            0
        } else {
            *fmt_index.entry(st.num_fmt.clone()).or_insert_with(|| {
                num_fmts.push(st.num_fmt.clone());
                (164 + num_fmts.len() - 1) as u32
            })
        };
        xfs.push((nid, fid, flid, st.clone()));
    }
    StyleTables {
        xfs,
        index,
        num_fmts,
        fonts,
        fills,
    }
}

fn argb(c: u32) -> String {
    format!("FF{c:06X}")
}

fn numfmts_xml(t: &StyleTables) -> String {
    let mut s = format!(r#"<numFmts count="{}">"#, t.num_fmts.len());
    for (i, code) in t.num_fmts.iter().enumerate() {
        s.push_str(&format!(
            r#"<numFmt numFmtId="{}" formatCode="{}"/>"#,
            164 + i,
            esc(code)
        ));
    }
    s.push_str("</numFmts>");
    s
}

fn fonts_xml(t: &StyleTables) -> String {
    let mut s = format!(r#"<fonts count="{}">"#, t.fonts.len());
    s.push_str(r#"<font><sz val="11"/><name val="Calibri"/></font>"#);
    for (b, i, c) in t.fonts.iter().skip(1) {
        s.push_str("<font>");
        if *b {
            s.push_str("<b/>");
        }
        if *i {
            s.push_str("<i/>");
        }
        if let Some(col) = c {
            s.push_str(&format!(r#"<color rgb="{}"/>"#, argb(*col)));
        }
        s.push_str(r#"<sz val="11"/><name val="Calibri"/></font>"#);
    }
    s.push_str("</fonts>");
    s
}

fn fills_xml(t: &StyleTables) -> String {
    let mut s = format!(
        r#"<fills count="{}"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill>"#,
        t.fills.len()
    );
    for col in t.fills.iter().skip(2) {
        if let Some(c) = col {
            s.push_str(&format!(
                r#"<fill><patternFill patternType="solid"><fgColor rgb="{}"/><bgColor indexed="64"/></patternFill></fill>"#,
                argb(*c)
            ));
        } else {
            s.push_str(r#"<fill><patternFill patternType="none"/></fill>"#);
        }
    }
    s.push_str("</fills>");
    s
}

/// One `<xf>` of `cellXfs`, with its alignment when it has one.
fn xf_xml(nid: u32, fid: u32, flid: u32, st: &CellStyle) -> String {
    let h = match st.align {
        Align::Center => "center",
        Align::End => "right",
        Align::Start => "left",
        Align::General => "",
    };
    if st.align == Align::General && !st.wrap {
        return format!(
            r#"<xf numFmtId="{nid}" fontId="{fid}" fillId="{flid}" borderId="0" xfId="0"/>"#
        );
    }
    let mut a = String::from("<alignment");
    if !h.is_empty() {
        a.push_str(&format!(r#" horizontal="{h}""#));
    }
    if st.wrap {
        a.push_str(r#" wrapText="1""#);
    }
    a.push_str("/>");
    format!(
        r#"<xf numFmtId="{nid}" fontId="{fid}" fillId="{flid}" borderId="0" xfId="0" applyAlignment="1">{a}</xf>"#
    )
}

pub(super) fn styles_xml(t: &StyleTables) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
    );
    s.push_str(&numfmts_xml(t));
    s.push_str(&fonts_xml(t));
    s.push_str(&fills_xml(t));
    s.push_str(r#"<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#);
    s.push_str(&format!(r#"<cellXfs count="{}">"#, t.xfs.len()));
    for (nid, fid, flid, st) in &t.xfs {
        s.push_str(&xf_xml(*nid, *fid, *flid, st));
    }
    s.push_str(
        r#"</cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#,
    );
    s
}

pub(super) fn s_attr(index: &HashMap<CellStyle, u32>, st: &CellStyle) -> String {
    if st.is_default() {
        return String::new();
    }
    match index.get(st) {
        Some(i) => format!(r#" s="{i}""#),
        None => String::new(),
    }
}
