//! Excel-Dateien (.xlsx, Office Open XML) lesen und schreiben – ohne Fremdprogramm.
//! Gelesen werden alle Blätter mit Zahlen, Texten (gemeinsame und eingebettete Zeichenketten), Wahrheitswerten,
//! Formeln und Spaltenbreiten. Geschrieben werden Formeln mitsamt berechnetem Ergebnis, damit auch Programme ohne
//! Neuberechnung (Vorschau, Handy) die Werte zeigen.

use crate::docx::{esc, read_zip_entry, zip_write};
use crate::sheet::{cell_name, col_name, fmt_num, parse_cell, Sheet, Value, Workbook};
use crate::style::{Align, CellStyle};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;

fn local(name: &[u8]) -> &[u8] {
    match name.iter().position(|&c| c == b':') {
        Some(p) => &name[p + 1..],
        None => name,
    }
}

fn attr(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| {
            a.key.as_ref() == key
                || local(a.key.as_ref()) == key && !a.key.as_ref().starts_with(b"xmlns")
        })
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

fn shared_strings(bytes: &[u8]) -> Vec<String> {
    let Ok(x) = read_zip_entry(bytes, "xl/sharedStrings.xml") else {
        return Vec::new();
    };
    let mut r = Reader::from_reader(&x[..]);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut in_si, mut in_t, mut in_rph) = (false, false, false);
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"si" => {
                    in_si = true;
                    cur.clear();
                }
                b"t" if in_si && !in_rph => in_t = true,
                b"rPh" => in_rph = true,
                _ => {}
            },
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"si" => out.push(String::new()),
            Ok(Event::Text(t)) if in_t => {
                cur.push_str(&t.unescape().map(|c| c.into_owned()).unwrap_or_default())
            }
            Ok(Event::End(e)) => match local(e.name().as_ref()) {
                b"t" => in_t = false,
                b"rPh" => in_rph = false,
                b"si" => {
                    in_si = false;
                    out.push(std::mem::take(&mut cur));
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// `"FFRRGGBB"`/`"RRGGBB"` → 0xRRGGBB (the alpha byte is ignored).
fn parse_rgb(s: &str) -> Option<u32> {
    let t = s.trim().trim_start_matches('#');
    let hex = if t.len() >= 6 { &t[t.len() - 6..] } else { t };
    u32::from_str_radix(hex, 16).ok()
}

/// The custom number formats of `styles.xml` (id → code).
fn styles_num_fmts(x: &[u8]) -> HashMap<u32, String> {
    let mut r = Reader::from_reader(x);
    let mut buf = Vec::new();
    let mut map = HashMap::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e))
                if local(e.name().as_ref()) == b"numFmt" =>
            {
                if let (Some(id), Some(code)) = (
                    attr(e, b"numFmtId").and_then(|v| v.parse().ok()),
                    attr(e, b"formatCode"),
                ) {
                    map.insert(id, code);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    map
}

/// The fonts of `styles.xml`: (bold, italic, text colour 0xRRGGBB).
fn styles_fonts(x: &[u8]) -> Vec<(bool, bool, Option<u32>)> {
    let mut r = Reader::from_reader(x);
    let mut buf = Vec::new();
    let mut fonts: Vec<(bool, bool, Option<u32>)> = vec![(false, false, None)];
    let mut in_fonts = false;
    let mut cur = fonts[0];
    let mut fresh = true;
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match local(e.name().as_ref()) {
                b"fonts" => {
                    in_fonts = true;
                    fonts.clear();
                }
                b"font" if in_fonts => {
                    cur = (false, false, None);
                    fresh = true;
                }
                b"b" if in_fonts => cur.0 = true,
                b"i" if in_fonts => cur.1 = true,
                b"color" if in_fonts => cur.2 = attr(e, b"rgb").and_then(|v| parse_rgb(&v)),
                _ => {}
            },
            Ok(Event::Empty(ref e)) => match local(e.name().as_ref()) {
                b"font" if in_fonts => {
                    fonts.push(cur);
                    cur = (false, false, None);
                    fresh = false;
                }
                b"b" if in_fonts => cur.0 = true,
                b"i" if in_fonts => cur.1 = true,
                b"color" if in_fonts => cur.2 = attr(e, b"rgb").and_then(|v| parse_rgb(&v)),
                _ => {}
            },
            Ok(Event::End(ref e)) => match local(e.name().as_ref()) {
                b"font" if in_fonts && fresh => fonts.push(cur),
                b"fonts" => break,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    if fonts.is_empty() {
        fonts.push((false, false, None));
    }
    fonts
}

/// The solid fills of `styles.xml` (index → 0xRRGGBB; 0 = none, 1 = gray125 placeholder).
fn styles_fills(x: &[u8]) -> Vec<Option<u32>> {
    let mut r = Reader::from_reader(x);
    let mut buf = Vec::new();
    let mut fills: Vec<Option<u32>> = vec![];
    let mut in_fills = false;
    let mut cur: Option<u32> = None;
    let mut fresh = true;
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match local(e.name().as_ref()) {
                b"fills" => {
                    in_fills = true;
                    fills.clear();
                }
                b"fill" if in_fills => {
                    cur = None;
                    fresh = true;
                }
                b"fgColor" | b"bgColor" if in_fills => {
                    cur = attr(e, b"rgb").and_then(|v| parse_rgb(&v)).or(cur);
                }
                _ => {}
            },
            Ok(Event::Empty(ref e)) => match local(e.name().as_ref()) {
                b"fill" if in_fills => {
                    fills.push(cur);
                    cur = None;
                    fresh = false;
                }
                b"fgColor" | b"bgColor" if in_fills => {
                    cur = attr(e, b"rgb").and_then(|v| parse_rgb(&v)).or(cur);
                }
                _ => {}
            },
            Ok(Event::End(ref e)) => match local(e.name().as_ref()) {
                b"fill" if in_fills && fresh => fills.push(cur),
                b"fills" => break,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    fills.resize(fills.len().max(2), None);
    fills
}

/// The format code of a (possibly built-in) number format id.
fn num_fmt_code(id: u32, custom: &HashMap<u32, String>) -> String {
    if let Some(c) = custom.get(&id) {
        return c.clone();
    }
    match id {
        1 => "0".into(),
        2 => "0.00".into(),
        3 => "#,##0".into(),
        4 => "#,##0.00".into(),
        9 => "0%".into(),
        10 => "0.00%".into(),
        14 => "dd.mm.yyyy".into(),
        22 => "yyyy-mm-dd".into(),
        _ => String::new(),
    }
}

/// The cell styles at their `cellXf` index, resolved from the tables above.
fn styles_cell_xfs(
    x: &[u8],
    num_fmts: &HashMap<u32, String>,
    fonts: &[(bool, bool, Option<u32>)],
    fills: &[Option<u32>],
) -> Vec<CellStyle> {
    let mut r = Reader::from_reader(x);
    let mut buf = Vec::new();
    let mut out = vec![CellStyle::default()];
    let mut in_cellxfs = false;
    let (mut cur, mut fresh) = (CellStyle::default(), true);
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match local(e.name().as_ref()) {
                b"cellXfs" => {
                    in_cellxfs = true;
                    out.clear();
                }
                b"xf" if in_cellxfs => {
                    cur = CellStyle::default();
                    fresh = true;
                    let fid = attr(e, b"numFmtId").and_then(|v| v.parse().ok()).unwrap_or(0);
                    cur.num_fmt = num_fmt_code(fid, num_fmts);
                    if let Some(i) = attr(e, b"fontId").and_then(|v| v.parse::<usize>().ok()) {
                        if let Some(&(b, it, c)) = fonts.get(i) {
                            cur.bold = b;
                            cur.italic = it;
                            cur.color = c;
                        }
                    }
                    if let Some(i) = attr(e, b"fillId").and_then(|v| v.parse::<usize>().ok()) {
                        cur.fill = fills.get(i).copied().flatten();
                    }
                }
                b"alignment" if in_cellxfs => {
                    cur.align = match attr(e, b"horizontal").as_deref() {
                        Some("center") => Align::Center,
                        Some("right") => Align::End,
                        Some("left") => Align::Start,
                        _ => Align::General,
                    };
                    cur.wrap = attr(e, b"wrapText").as_deref() == Some("1");
                }
                _ => {}
            },
            Ok(Event::Empty(ref e)) => {
                if local(e.name().as_ref()) == b"xf" && in_cellxfs {
                    out.push(std::mem::take(&mut cur));
                    fresh = false;
                }
            }
            Ok(Event::End(ref e)) => match local(e.name().as_ref()) {
                b"xf" if in_cellxfs && fresh => out.push(std::mem::take(&mut cur)),
                b"cellXfs" => break,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    if out.is_empty() {
        out.push(CellStyle::default());
    }
    out
}

/// Alle Zellformatstile einer Datei (Index = `s`-Attribut der Zellen).
fn read_styles(bytes: &[u8]) -> Vec<CellStyle> {
    let Ok(x) = read_zip_entry(bytes, "xl/styles.xml") else {
        return vec![CellStyle::default()];
    };
    let num_fmts = styles_num_fmts(&x);
    let fonts = styles_fonts(&x);
    let fills = styles_fills(&x);
    styles_cell_xfs(&x, &num_fmts, &fonts, &fills)
}

/// Blätter in Reihenfolge: (Name, Pfad im Archiv)
fn sheet_list(bytes: &[u8]) -> Result<Vec<(String, String)>, String> {
    let wb = read_zip_entry(bytes, "xl/workbook.xml")?;
    let rels = read_zip_entry(bytes, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let mut targets = std::collections::HashMap::new();
    let mut r = Reader::from_reader(&rels[..]);
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"Relationship" =>
            {
                if let (Some(id), Some(t)) = (attr(&e, b"Id"), attr(&e, b"Target")) {
                    let t = t.trim_start_matches('/').to_string();
                    let t = if t.starts_with("xl/") {
                        t
                    } else {
                        format!("xl/{t}")
                    };
                    targets.insert(id, t);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    let mut out = Vec::new();
    let mut r = Reader::from_reader(&wb[..]);
    let mut i = 0;
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"sheet" => {
                i += 1;
                let name = attr(&e, b"name").unwrap_or_else(|| format!("Sheet{i}"));
                let rid = e
                    .attributes()
                    .flatten()
                    .find(|a| local(a.key.as_ref()) == b"id")
                    .map(|a| String::from_utf8_lossy(&a.value).into_owned());
                let path = rid
                    .and_then(|id| targets.get(&id).cloned())
                    .unwrap_or_else(|| format!("xl/worksheets/sheet{i}.xml"));
                out.push((name, path));
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn read_sheet(x: &[u8], name: &str, ss: &[String], styles: &[CellStyle]) -> Sheet {
    let mut sh = Sheet::new(name);
    let mut r = Reader::from_reader(x);
    let mut buf = Vec::new();
    let mut cur: Option<(u32, u32)> = None;
    let mut typ = String::new();
    let mut val = String::new();
    let mut formula: Option<String> = None;
    let (mut in_v, mut in_f, mut in_is_t) = (false, false, false);
    let mut next_col = 0u32;
    let mut row_no = 0u32;
    let mut style_idx: Option<u32> = None;
    loop {
        let ev = r.read_event_into(&mut buf);
        match ev {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let empty = matches!(ev, Ok(Event::Empty(_)));
                match local(e.name().as_ref()) {
                    b"row" => {
                        row_no = attr(e, b"r")
                            .and_then(|v| v.parse::<u32>().ok())
                            .map(|v| v - 1)
                            .unwrap_or(row_no);
                        next_col = 0;
                        if let Some(h) = attr(e, b"ht").and_then(|v| v.parse::<f32>().ok()) {
                            sh.heights.insert(row_no, h);
                        }
                    }
                    b"sheetView" => {
                        sh.rtl = attr(e, b"rightToLeft").as_deref() == Some("1");
                    }
                    b"c" => {
                        let pos = attr(e, b"r")
                            .and_then(|v| parse_cell(&v))
                            .unwrap_or((row_no, next_col));
                        next_col = pos.1 + 1;
                        typ = attr(e, b"t").unwrap_or_default();
                        style_idx = attr(e, b"s").and_then(|v| v.parse().ok());
                        val.clear();
                        formula = None;
                        cur = if empty { None } else { Some(pos) };
                    }
                    b"v" if !empty => in_v = true,
                    b"f" => {
                        if !empty {
                            in_f = true;
                            formula = Some(String::new());
                        }
                    }
                    b"t" if !empty => in_is_t = true,
                    b"col" => {
                        if let (Some(a), Some(b), Some(w)) =
                            (attr(e, b"min"), attr(e, b"max"), attr(e, b"width"))
                        {
                            if let (Ok(a), Ok(b), Ok(w)) =
                                (a.parse::<u32>(), b.parse::<u32>(), w.parse::<f32>())
                            {
                                for c in a..=b.min(a + 200) {
                                    sh.widths.insert(c - 1, w);
                                }
                            }
                        }
                    }
                    _ => {}
                }
                if local(e.name().as_ref()) == b"row" {
                    row_no = attr(e, b"r")
                        .and_then(|v| v.parse::<u32>().ok())
                        .map(|v| v - 1)
                        .unwrap_or(row_no);
                }
            }
            Ok(Event::Text(t)) => {
                let s = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                if in_v || in_is_t {
                    val.push_str(&s);
                } else if in_f {
                    if let Some(f) = formula.as_mut() {
                        f.push_str(&s);
                    }
                }
            }
            Ok(Event::End(ref e)) => match local(e.name().as_ref()) {
                b"v" => in_v = false,
                b"f" => in_f = false,
                b"t" => in_is_t = false,
                b"row" => row_no += 1,
                b"c" => {
                    if let Some((rr, cc)) = cur.take() {
                        let raw = match (&formula, typ.as_str()) {
                            (Some(f), _) if !f.is_empty() => format!("={f}"),
                            (_, "s") => val
                                .trim()
                                .parse::<usize>()
                                .ok()
                                .and_then(|i| ss.get(i).cloned())
                                .unwrap_or_default(),
                            (_, "b") => {
                                if val.trim() == "1" {
                                    "TRUE".into()
                                } else {
                                    "FALSE".into()
                                }
                            }
                            (_, "str") | (_, "inlineStr") => val.clone(),
                            (_, "e") => val.clone(),
                            _ => match val.trim().parse::<f64>() {
                                Ok(n) => fmt_num(n),
                                Err(_) => val.clone(),
                            },
                        };
                        // Text, der wie eine Zahl/Formel aussieht, bleibt Text
                        let raw = if matches!(typ.as_str(), "s" | "str" | "inlineStr")
                            && formula.is_none()
                            && (raw.starts_with('=')
                                || crate::sheet::literal(&raw) != Value::Text(raw.clone()))
                            && !raw.is_empty()
                        {
                            format!("'{raw}")
                        } else {
                            raw
                        };
                        sh.set(rr, cc, &raw);
                        if let Some(i) = style_idx {
                            if let Some(st) = styles.get(i as usize) {
                                if !st.is_default() {
                                    sh.styles.insert((rr, cc), st.clone());
                                }
                            }
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    sh
}

/// .xlsx lesen
pub fn read(bytes: &[u8]) -> Result<Workbook, String> {
    let ss = shared_strings(bytes);
    let styles = read_styles(bytes);
    let mut wb = Workbook { sheets: Vec::new() };
    for (name, path) in sheet_list(bytes)? {
        let x = read_zip_entry(bytes, &path).unwrap_or_default();
        wb.sheets.push(read_sheet(&x, &name, &ss, &styles));
    }
    if wb.sheets.is_empty() {
        wb.sheets.push(Sheet::new("Sheet1"));
    }
    Ok(wb)
}

struct StyleTables {
    /// (numFmtId, fontId, fillId, style) in `cellXfs` order.
    xfs: Vec<(u32, u32, u32, CellStyle)>,
    /// Style → its `cellXfs` index.
    index: HashMap<CellStyle, u32>,
    /// Custom number formats (index 0 → id 164).
    num_fmts: Vec<String>,
    /// Fonts (index 0 is the default).
    fonts: Vec<(bool, bool, Option<u32>)>,
    /// Fills (index 0 none, 1 gray125).
    fills: Vec<Option<u32>>,
}

fn build_style_tables(wb: &Workbook) -> StyleTables {
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

fn styles_xml(t: &StyleTables) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="{}">"#,
        t.num_fmts.len()
    );
    for (i, code) in t.num_fmts.iter().enumerate() {
        s.push_str(&format!(
            r#"<numFmt numFmtId="{}" formatCode="{}"/>"#,
            164 + i,
            esc(code)
        ));
    }
    s.push_str(&format!(r#"</numFmts><fonts count="{}">"#, t.fonts.len()));
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
    s.push_str(&format!(
        r#"</fonts><fills count="{}"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill>"#,
        t.fills.len()
    ));
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
    s.push_str(r#"</fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#);
    s.push_str(&format!(r#"<cellXfs count="{}">"#, t.xfs.len()));
    for (nid, fid, flid, st) in &t.xfs {
        let h = match st.align {
            Align::Center => "center",
            Align::End => "right",
            Align::Start => "left",
            Align::General => "",
        };
        let plain = st.align == Align::General && !st.wrap;
        if plain {
            s.push_str(&format!(
                r#"<xf numFmtId="{nid}" fontId="{fid}" fillId="{flid}" borderId="0" xfId="0"/>"#
            ));
        } else {
            let mut a = String::from("<alignment");
            if !h.is_empty() {
                a.push_str(&format!(r#" horizontal="{h}""#));
            }
            if st.wrap {
                a.push_str(r#" wrapText="1""#);
            }
            a.push_str("/>");
            s.push_str(&format!(
                r#"<xf numFmtId="{nid}" fontId="{fid}" fillId="{flid}" borderId="0" xfId="0" applyAlignment="1">{a}</xf>"#
            ));
        }
    }
    s.push_str(
        r#"</cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#,
    );
    s
}

fn s_attr(index: &HashMap<CellStyle, u32>, st: &CellStyle) -> String {
    if st.is_default() {
        return String::new();
    }
    match index.get(st) {
        Some(i) => format!(r#" s="{i}""#),
        None => String::new(),
    }
}

fn sheet_xml(wb: &Workbook, si: usize, t: &StyleTables) -> String {
    let sh = &wb.sheets[si];
    let (rows, cols) = sh.extent();
    let dim = if rows == 0 {
        "A1".to_string()
    } else {
        format!("A1:{}", cell_name(rows - 1, cols.saturating_sub(1)))
    };
    let rtl = sh.rtl || sh.cells.values().any(|v| crate::doc::is_rtl(v));
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><dimension ref="{dim}"/><sheetViews><sheetView workbookViewId="0"{}/></sheetViews><sheetFormatPr defaultRowHeight="15"/>"#,
        if rtl { r#" rightToLeft="1""# } else { "" }
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
    s.push_str("<sheetData>");
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
            let v = wb.value(si, r, c);
            let (ty, vv) = match &v {
                Value::Num(n) => ("", fmt_num(*n)),
                Value::Bool(b) => (r#" t="b""#, if *b { "1".into() } else { "0".into() }),
                Value::Err(e) => (r#" t="e""#, e.clone()),
                Value::Text(x) => (r#" t="str""#, x.clone()),
                Value::Empty => (r#" t="str""#, String::new()),
            };
            s.push_str(&format!(
                r#"<c r="{rf}"{sa}{ty}><f>{}</f><v>{}</v></c>"#,
                esc(f),
                esc(&vv)
            ));
        } else if let Some(text) = raw.strip_prefix('\'') {
            s.push_str(&format!(
                r#"<c r="{rf}"{sa} t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
                esc(text)
            ));
        } else {
            match crate::sheet::literal(raw) {
                Value::Num(n) => s.push_str(&format!(
                    r#"<c r="{rf}"{sa}><v>{}</v></c>"#,
                    fmt_num(n)
                )),
                Value::Bool(b) => s.push_str(&format!(
                    r#"<c r="{rf}"{sa} t="b"><v>{}</v></c>"#,
                    b as u8
                )),
                _ => s.push_str(&format!(
                    r#"<c r="{rf}"{sa} t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
                    esc(raw)
                )),
            }
        }
    }
    if cur_row.is_some() {
        s.push_str("</row>");
    }
    s.push_str(r#"</sheetData><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/></worksheet>"#);
    s
}

/// .xlsx schreiben
pub fn write(wb: &Workbook) -> Result<Vec<u8>, String> {
    let tables = build_style_tables(wb);
    let n = wb.sheets.len().max(1);
    let mut ct = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#,
    );
    let mut wbx = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><bookViews><workbookView/></bookViews><sheets>"#,
    );
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    let mut sheets_xml = Vec::new();
    let empty = Workbook::new();
    let w = if wb.sheets.is_empty() { &empty } else { wb };
    for i in 0..n {
        ct.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#, i + 1));
        wbx.push_str(&format!(
            r#"<sheet name="{}" sheetId="{}" r:id="rId{}"/>"#,
            esc(&w.sheets[i].name),
            i + 1,
            i + 1
        ));
        rels.push_str(&format!(r#"<Relationship Id="rId{0}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{0}.xml"/>"#, i + 1));
        sheets_xml.push(sheet_xml(w, i, &tables));
    }
    ct.push_str("</Types>");
    wbx.push_str(r#"</sheets><calcPr calcId="191029" fullCalcOnLoad="1"/></workbook>"#);
    rels.push_str(&format!(r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#, n + 1));
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#;
    let core = crate::docx::core_xml(&w.sheets[0].name);
    let app = crate::docx::app_xml("Sheets");
    let names: Vec<String> = (1..=n)
        .map(|i| format!("xl/worksheets/sheet{i}.xml"))
        .collect();
    let styles = styles_xml(&tables);
    let mut files: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", ct.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("xl/workbook.xml", wbx.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
        ("xl/styles.xml", styles.as_bytes()),
        ("docProps/core.xml", core.as_bytes()),
        ("docProps/app.xml", app.as_bytes()),
    ];
    for i in 0..n {
        files.push((names[i].as_str(), sheets_xml[i].as_bytes()));
    }
    zip_write(&files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xlsx_hin_und_zurueck() {
        let mut wb = Workbook::new();
        wb.sheets[0].name = "Umsatz".into();
        wb.set_ref(0, "A1", "Monat").unwrap();
        wb.set_ref(0, "B1", "Betrag").unwrap();
        wb.set_ref(0, "A2", "يناير").unwrap();
        wb.set_ref(0, "B2", "1200.5").unwrap();
        wb.set_ref(0, "A3", "Feb & <März>").unwrap();
        wb.set_ref(0, "B3", "800").unwrap();
        wb.set_ref(0, "B4", "=SUM(B2:B3)").unwrap();
        wb.set_ref(0, "C1", "TRUE").unwrap();
        wb.set_ref(0, "D1", "'00123").unwrap();
        wb.set_ref(0, "Kosten!A1", "=Umsatz!B4/2").unwrap();
        let bytes = write(&wb).unwrap();
        let back = read(&bytes).unwrap();
        assert_eq!(back.sheets.len(), 2);
        assert_eq!(back.sheets[0].name, "Umsatz");
        assert_eq!(back.sheets[0].get(1, 0), "يناير");
        assert_eq!(back.sheets[0].get(2, 0), "Feb & <März>");
        assert_eq!(back.sheets[0].get(3, 1), "=SUM(B2:B3)");
        assert_eq!(back.sheets[0].get(0, 3), "'00123");
        assert_eq!(back.value(0, 3, 1), Value::Num(2000.5));
        assert_eq!(back.value(1, 0, 0), Value::Num(1000.25));
        assert_eq!(back.value(0, 0, 2), Value::Bool(true));
        // berechnetes Ergebnis steht in der Datei (für Vorschauen ohne Neuberechnung)
        let x =
            String::from_utf8(read_zip_entry(&bytes, "xl/worksheets/sheet1.xml").unwrap()).unwrap();
        assert!(x.contains("<f>SUM(B2:B3)</f><v>2000.5</v>"));
        assert!(x.contains(r#"rightToLeft="1""#));
    }

    #[test]
    fn format_hin_und_zurueck() {
        let mut wb = Workbook::new();
        wb.sheets[0].name = "Format".into();
        wb.set_ref(0, "A1", "1234.5").unwrap();
        wb.sheets[0].set_style(
            0,
            0,
            CellStyle {
                bold: true,
                italic: true,
                color: Some(0xFF0000),
                fill: Some(0xFFFF00),
                align: Align::Center,
                wrap: false,
                num_fmt: "0.00".into(),
            },
        );
        wb.set_ref(0, "B2", "0.125").unwrap();
        wb.sheets[0].set_style(
            1,
            1,
            CellStyle {
                num_fmt: "0%".into(),
                ..CellStyle::default()
            },
        );
        wb.sheets[0].heights.insert(0, 30.0);
        wb.sheets[0].rtl = true;
        let bytes = write(&wb).unwrap();
        let back = read(&bytes).unwrap();
        let st = back.sheets[0].style(0, 0);
        assert!(st.bold && st.italic, "bold+italic round trip");
        assert_eq!(st.color, Some(0xFF0000));
        assert_eq!(st.fill, Some(0xFFFF00));
        assert_eq!(st.align, Align::Center);
        assert_eq!(st.num_fmt, "0.00");
        assert_eq!(back.sheets[0].style(1, 1).num_fmt, "0%");
        assert!((back.sheets[0].height(0) - 30.0).abs() < 0.5, "Zeile 30 pt");
        assert!(back.sheets[0].rtl, "rightToLeft");
        // die Standardzellen bleiben unformatiert
        assert_eq!(back.sheets[0].style(2, 2), CellStyle::default());
    }
}
