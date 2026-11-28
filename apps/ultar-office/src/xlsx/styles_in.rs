//! The cell styles of `xl/styles.xml`: number formats, fonts, fills and the `cellXfs` table.

use super::{attr, local, read_zip_entry};
use crate::style::{Align, CellStyle};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;

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

/// One `<xf>` of `cellXfs`: number format, font and fill resolved from the tables.
fn xf_style(
    e: &BytesStart,
    num_fmts: &HashMap<u32, String>,
    fonts: &[(bool, bool, Option<u32>)],
    fills: &[Option<u32>],
) -> CellStyle {
    let mut cur = CellStyle::default();
    let fid = attr(e, b"numFmtId")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
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
    cur
}

/// The `<alignment>` of an `<xf>`.
fn set_alignment(e: &BytesStart, cur: &mut CellStyle) {
    cur.align = match attr(e, b"horizontal").as_deref() {
        Some("center") => Align::Center,
        Some("right") => Align::End,
        Some("left") => Align::Start,
        _ => Align::General,
    };
    cur.wrap = matches!(attr(e, b"wrapText").as_deref(), Some("1") | Some("true"));
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
        let ev = r.read_event_into(&mut buf);
        match ev {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => match local(e.name().as_ref()) {
                b"cellXfs" => {
                    in_cellxfs = true;
                    out.clear();
                }
                b"xf" if in_cellxfs => {
                    cur = xf_style(e, num_fmts, fonts, fills);
                    fresh = true;
                    if matches!(ev, Ok(Event::Empty(_))) {
                        out.push(std::mem::take(&mut cur));
                        fresh = false;
                    }
                }
                b"alignment" if in_cellxfs => set_alignment(e, &mut cur),
                _ => {}
            },
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
pub(super) fn read_styles(bytes: &[u8]) -> Vec<CellStyle> {
    let Ok(x) = read_zip_entry(bytes, "xl/styles.xml") else {
        return vec![CellStyle::default()];
    };
    let num_fmts = styles_num_fmts(&x);
    let fonts = styles_fonts(&x);
    let fills = styles_fills(&x);
    styles_cell_xfs(&x, &num_fmts, &fonts, &fills)
}
