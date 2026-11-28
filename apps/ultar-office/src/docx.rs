//! Word-Dateien (.docx, Office Open XML) lesen und schreiben – ohne Fremdprogramm.
//! Gelesen werden Absätze mit Formatvorlage (Titel, Überschrift 1–3, Liste, Zitat), Ausrichtung,
//! fett/kursiv/unterstrichen und Tabellen. Geschrieben wird eine schlanke, Word-/LibreOffice-taugliche Datei
//! mit Formatvorlagen, Aufzählung/Nummerierung und Rechts-nach-links-Absätzen für Arabisch.

use crate::doc::{is_rtl, Align, Block, Dir, Document, Para, Picture, Run, Style};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::{Cursor, Read, Write};

fn local(name: &[u8]) -> &[u8] {
    match name.iter().position(|&c| c == b':') {
        Some(p) => &name[p + 1..],
        None => name,
    }
}

fn attr(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| local(a.key.as_ref()) == key)
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

fn on(e: &quick_xml::events::BytesStart) -> bool {
    !matches!(
        attr(e, b"val").as_deref(),
        Some("0") | Some("false") | Some("none")
    )
}

mod lossy;
mod write;
pub(crate) use write::zip_write;
pub use write::{esc, write};
#[cfg(test)]
mod tests;
pub use lossy::lossy_features;

/// The colour of a Word highlighter name (`w:highlight`) as six hex digits.
fn highlight_hex(name: &str) -> Option<String> {
    let hex = match name {
        "yellow" => "FFFF00",
        "green" => "00FF00",
        "cyan" => "00FFFF",
        "magenta" => "FF00FF",
        "blue" => "0000FF",
        "red" => "FF0000",
        "darkBlue" => "000080",
        "darkCyan" => "008080",
        "darkGreen" => "008000",
        "darkMagenta" => "800080",
        "darkRed" => "800000",
        "darkYellow" => "808000",
        "darkGray" => "808080",
        "lightGray" => "C0C0C0",
        "black" => "000000",
        "white" => "FFFFFF",
        _ => return None,
    };
    Some(hex.to_owned())
}

/// A colour value as six upper-case hex digits; `auto`/`none` and empty mean "theme colour".
fn color_val(v: &str) -> Option<String> {
    let v = v.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("auto") || v.eq_ignore_ascii_case("none") {
        return None;
    }
    Some(v.to_ascii_uppercase())
}

pub fn read_zip_entry(bytes: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("not a zip/office file: {e}"))?;
    let mut f = z.by_name(name).map_err(|_| format!("{name} missing"))?;
    let mut v = Vec::new();
    f.read_to_end(&mut v).map_err(|e| e.to_string())?;
    Ok(v)
}

/// Formatvorlagen-Kennung (styles.xml) → Name; bei Word steht in document.xml nur die Kennung
fn style_names(bytes: &[u8]) -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    let Ok(x) = read_zip_entry(bytes, "word/styles.xml") else {
        return m;
    };
    let mut r = Reader::from_reader(&x[..]);
    let mut buf = Vec::new();
    let mut cur: Option<String> = None;
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"style" => cur = attr(&e, b"styleId"),
                b"name" => {
                    if let (Some(id), Some(n)) = (cur.clone(), attr(&e, b"val")) {
                        m.insert(id, n);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    m
}

/// `word/_rels/document.xml.rels`: relationship id → target part (relative, no leading slash).
fn document_rels(bytes: &[u8]) -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    let Ok(x) = read_zip_entry(bytes, "word/_rels/document.xml.rels") else {
        return m;
    };
    let mut r = Reader::from_reader(&x[..]);
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"Relationship" =>
            {
                if let (Some(id), Some(t)) = (attr(&e, b"Id"), attr(&e, b"Target")) {
                    m.insert(id, t.trim_start_matches('/').to_owned());
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    m
}

fn style_from(id: &str, names: &std::collections::HashMap<String, String>) -> Style {
    let n = names
        .get(id)
        .map(|s| s.as_str())
        .unwrap_or(id)
        .to_ascii_lowercase()
        .replace(' ', "");
    let id = id.to_ascii_lowercase();
    let is = |k: &str| n == k || id == k;
    if is("title") || n.starts_with("titel") {
        Style::Title
    } else if is("heading1") || is("berschrift1") || n.ends_with("berschrift1") {
        Style::H1
    } else if is("heading2") || n.ends_with("berschrift2") {
        Style::H2
    } else if is("heading3") || is("heading4") || n.ends_with("berschrift3") {
        Style::H3
    } else if n.contains("quote") || n.contains("zitat") {
        Style::Quote
    } else if n.contains("code") || n.contains("htmlpreformatted") {
        Style::Code
    } else if n.contains("listbullet") || n.contains("aufzählung") {
        Style::Bullet
    } else if n.contains("listnumber") {
        Style::Number
    } else {
        Style::Normal
    }
}

/// Nummerierungen, die Zahlen (statt Punkte) erzeugen: numId → true
fn numbering_decimal(bytes: &[u8]) -> std::collections::HashMap<String, bool> {
    let mut out = std::collections::HashMap::new();
    let Ok(x) = read_zip_entry(bytes, "word/numbering.xml") else {
        return out;
    };
    let mut abs: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    let mut r = Reader::from_reader(&x[..]);
    let mut buf = Vec::new();
    let (mut cur_abs, mut lvl0, mut cur_num) = (None::<String>, false, None::<String>);
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"abstractNum" => cur_abs = attr(&e, b"abstractNumId"),
                b"lvl" => lvl0 = attr(&e, b"ilvl").as_deref() == Some("0"),
                b"numFmt" if lvl0 => {
                    if let Some(a) = &cur_abs {
                        abs.insert(
                            a.clone(),
                            attr(&e, b"val").is_some_and(|v| v != "bullet" && v != "none"),
                        );
                    }
                }
                b"num" => cur_num = attr(&e, b"numId"),
                b"abstractNumId" => {
                    if let (Some(n), Some(a)) = (&cur_num, attr(&e, b"val")) {
                        out.insert(n.clone(), abs.get(&a).copied().unwrap_or(false));
                    }
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

/// .docx lesen
pub fn read(bytes: &[u8]) -> Result<Document, String> {
    let x = read_zip_entry(bytes, "word/document.xml")?;
    let names = style_names(bytes);
    let nums = numbering_decimal(bytes);
    let rels = document_rels(bytes);
    let mut r = Reader::from_reader(&x[..]);
    let mut buf = Vec::new();
    let mut doc = Document::default();
    // Zustand
    let mut para: Option<Para> = None;
    let mut run: Option<Run> = None;
    let mut in_rpr = false;
    let mut in_ppr = false;
    let mut in_t = false;
    // a page break inside a paragraph: the next paragraph starts a new page
    let mut carry_break = false;
    // the paragraph holds nothing but a page break (Word's "insert page break")
    let mut br_only = false;
    let mut num_id: Option<String> = None;
    // Bilder (w:drawing): rId + Größe eines Bildes im laufenden Absatz
    let mut in_drawing = false;
    let mut draw_extent: Option<(i64, i64)> = None;
    let mut draw_embed: Option<String> = None;
    let mut para_pics: Vec<(String, (i64, i64))> = Vec::new();
    // Tabellen (verschachtelt: nur äußerste zählt, innere als Text)
    let mut tbl_depth = 0usize;
    let mut table: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    loop {
        let ev = r.read_event_into(&mut buf);
        match ev {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let empty = matches!(ev, Ok(Event::Empty(_)));
                match local(e.name().as_ref()) {
                    b"tbl" if !empty => {
                        tbl_depth += 1;
                        if tbl_depth == 1 {
                            table.clear();
                        }
                    }
                    b"tr" if tbl_depth == 1 && !empty => row.clear(),
                    b"tc" if tbl_depth == 1 && !empty => cell.clear(),
                    b"p" => {
                        br_only = false;
                        para = Some(Para {
                            page_break: std::mem::take(&mut carry_break),
                            ..Para::default()
                        });
                        num_id = None;
                        para_pics.clear();
                        draw_extent = None;
                        draw_embed = None;
                        in_drawing = false;
                        if empty {
                            if tbl_depth == 0 {
                                doc.blocks.push(Block::Para(Para::default()));
                            }
                            para = None;
                        }
                    }
                    b"pPr" if !empty => in_ppr = true,
                    b"pStyle" if in_ppr => {
                        if let (Some(p), Some(v)) = (para.as_mut(), attr(e, b"val")) {
                            p.style = style_from(&v, &names);
                        }
                    }
                    b"numId" if in_ppr => num_id = attr(e, b"val"),
                    b"jc" if in_ppr && !in_rpr => {
                        if let Some(p) = para.as_mut() {
                            p.align = match attr(e, b"val").as_deref() {
                                Some("center") => Align::Center,
                                Some("right") | Some("end") => Align::End,
                                Some("both") | Some("distribute") => Align::Justify,
                                _ => Align::Start,
                            };
                        }
                    }
                    b"r" if !empty => run = Some(Run::default()),
                    b"rPr" if !empty => in_rpr = true,
                    b"b" if in_rpr => {
                        if let Some(rr) = run.as_mut() {
                            rr.bold = on(e);
                        }
                    }
                    b"i" if in_rpr => {
                        if let Some(rr) = run.as_mut() {
                            rr.italic = on(e);
                        }
                    }
                    b"u" if in_rpr => {
                        if let Some(rr) = run.as_mut() {
                            rr.underline = on(e);
                        }
                    }
                    b"strike" | b"dstrike" if in_rpr => {
                        if let Some(rr) = run.as_mut() {
                            rr.strike = on(e);
                        }
                    }
                    b"sz" | b"szCs" if in_rpr => {
                        if let Some(v) = attr(e, b"val").and_then(|s| s.parse::<f32>().ok()) {
                            if let Some(rr) = run.as_mut() {
                                rr.size_pt = Some(v / 2.0);
                            }
                        }
                    }
                    b"color" if in_rpr => {
                        if let Some(rr) = run.as_mut() {
                            rr.color = color_val(&attr(e, b"val").unwrap_or_default());
                        }
                    }
                    b"shd" if in_rpr => {
                        if let Some(fill) = attr(e, b"fill") {
                            if fill != "auto" && !fill.is_empty() {
                                if let Some(rr) = run.as_mut() {
                                    rr.highlight = Some(fill.to_ascii_uppercase());
                                }
                            }
                        }
                    }
                    b"bidi" if in_ppr => {
                        if let Some(p) = para.as_mut() {
                            p.dir = Dir::Rtl;
                        }
                    }
                    b"spacing" if in_ppr => {
                        if let Some(v) = attr(e, b"line").and_then(|s| s.parse::<f32>().ok()) {
                            if let Some(p) = para.as_mut() {
                                p.line_spacing = Some(v / 240.0);
                            }
                        }
                    }
                    b"ind" if in_ppr => {
                        let dxa = attr(e, b"start")
                            .or_else(|| attr(e, b"left"))
                            .and_then(|s| s.parse::<f32>().ok());
                        if let Some(v) = dxa {
                            if let Some(p) = para.as_mut() {
                                p.indent_pt = Some(v / 20.0);
                            }
                        }
                    }
                    b"drawing" if !empty => in_drawing = true,
                    b"extent" if in_drawing => {
                        let v = attr(e, b"cx")
                            .and_then(|s| s.parse::<i64>().ok())
                            .zip(attr(e, b"cy").and_then(|s| s.parse::<i64>().ok()));
                        draw_extent = draw_extent.or(v);
                    }
                    b"blip" if in_drawing => {
                        draw_embed = attr(e, b"embed");
                    }
                    b"t" if !empty => in_t = true,
                    b"tab" if !in_ppr => {
                        if let Some(rr) = run.as_mut() {
                            rr.text.push('\t');
                        }
                    }
                    b"br" | b"cr" => {
                        if attr(e, b"type").as_deref() == Some("page") {
                            // text before the break stays on this page, the rest goes on
                            let nothing_yet = para.as_ref().is_some_and(|p| p.text().is_empty())
                                && run.as_ref().is_none_or(|r| r.text.is_empty());
                            match para.as_mut() {
                                Some(p) if nothing_yet => {
                                    p.page_break = true;
                                    br_only = true;
                                }
                                Some(p) if tbl_depth == 0 => {
                                    // text before the break ends this paragraph; the rest is
                                    // a paragraph of its own that starts the next page
                                    if let Some(rr) = run.as_mut().filter(|r| !r.text.is_empty()) {
                                        let text = std::mem::take(&mut rr.text);
                                        p.runs.push(Run { text, ..rr.clone() });
                                    }
                                    let next = Para {
                                        page_break: true,
                                        runs: Vec::new(),
                                        ..p.clone()
                                    };
                                    let done = std::mem::replace(p, next);
                                    doc.blocks.push(Block::Para(done));
                                }
                                _ => {}
                            }
                        } else if let Some(rr) = run.as_mut() {
                            rr.text.push('\n');
                        }
                    }
                    b"pageBreakBefore" if in_ppr => {
                        if let Some(p) = para.as_mut() {
                            p.page_break = on(e);
                        }
                    }
                    b"highlight" if in_rpr => {
                        if let Some(rr) = run.as_mut() {
                            rr.highlight = highlight_hex(&attr(e, b"val").unwrap_or_default());
                        }
                    }
                    b"pgSz" => {
                        let twips = |k: &[u8]| attr(e, k).and_then(|s| s.parse::<f32>().ok());
                        if let (Some(w), Some(h)) = (twips(b"w"), twips(b"h")) {
                            if w > 1000.0 && h > 1000.0 {
                                doc.page.width_pt = w / 20.0;
                                doc.page.height_pt = h / 20.0;
                            }
                        }
                    }
                    b"pgMar" => {
                        let pt = |k: &[u8]| {
                            attr(e, k)
                                .and_then(|s| s.parse::<f32>().ok())
                                .map(|v| (v / 20.0).max(0.0))
                        };
                        let page = &mut doc.page;
                        let end = pt(b"right").or_else(|| pt(b"end"));
                        let start = pt(b"left").or_else(|| pt(b"start"));
                        page.set_margins([pt(b"top"), end, pt(b"bottom"), start]);
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) if in_t => {
                let s = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                if let Some(rr) = run.as_mut() {
                    rr.text.push_str(&s);
                }
            }
            Ok(Event::CData(t)) if in_t => {
                if let Some(rr) = run.as_mut() {
                    rr.text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Ok(Event::End(ref e)) => match local(e.name().as_ref()) {
                b"t" => in_t = false,
                b"rPr" => in_rpr = false,
                b"pPr" => {
                    in_ppr = false;
                    if let (Some(p), Some(n)) = (para.as_mut(), num_id.as_ref()) {
                        if n != "0" && matches!(p.style, Style::Normal) {
                            p.style = if nums.get(n).copied().unwrap_or(false) {
                                Style::Number
                            } else {
                                Style::Bullet
                            };
                        }
                    }
                }
                b"r" => {
                    if let (Some(rr), Some(p)) = (run.take(), para.as_mut()) {
                        if !rr.text.is_empty() {
                            match p.runs.last_mut() {
                                Some(l) if crate::doc::same_format(l, &rr) => {
                                    l.text.push_str(&rr.text)
                                }
                                _ => p.runs.push(rr),
                            }
                        }
                    }
                }
                b"p" => {
                    in_drawing = false;
                    let has_pics = !para_pics.is_empty();
                    if let Some(p) = para.take() {
                        if tbl_depth == 0 && br_only && p.text().is_empty() && !has_pics {
                            carry_break = true;
                        } else if tbl_depth > 0 {
                            if !cell.is_empty() {
                                cell.push('\n');
                            }
                            cell.push_str(&p.text());
                        } else if !(has_pics && p.text().is_empty()) {
                            doc.blocks.push(Block::Para(p));
                        }
                    }
                    if tbl_depth == 0 {
                        for (rid, (cx, cy)) in para_pics.drain(..) {
                            let Some(target) = rels.get(&rid) else {
                                continue;
                            };
                            let part = if target.starts_with("word/") {
                                target.clone()
                            } else {
                                format!("word/{target}")
                            };
                            let Ok(media) = read_zip_entry(bytes, &part) else {
                                continue;
                            };
                            let name = target.rsplit('/').next().unwrap_or("image.png").to_owned();
                            doc.blocks.push(Block::Picture(Picture {
                                bytes: media,
                                name,
                                width_pt: Some(cx as f32 / 12700.0),
                                height_pt: Some(cy as f32 / 12700.0),
                            }));
                        }
                    }
                }
                b"drawing" => {
                    in_drawing = false;
                    if let (Some(rid), Some(extent)) = (draw_embed.take(), draw_extent.take()) {
                        para_pics.push((rid, extent));
                    } else {
                        draw_extent = None;
                        draw_embed = None;
                    }
                }
                b"tc" if tbl_depth == 1 => row.push(std::mem::take(&mut cell)),
                b"tr" if tbl_depth == 1 => table.push(std::mem::take(&mut row)),
                b"tbl" => {
                    if tbl_depth == 1 {
                        doc.blocks.push(Block::Table(std::mem::take(&mut table)));
                    }
                    tbl_depth = tbl_depth.saturating_sub(1);
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("document.xml: {e}")),
            _ => {}
        }
        buf.clear();
    }
    Ok(doc)
}

pub(crate) fn app_xml(app_kind: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Ultar {app_kind} – KM Code Craft</Application></Properties>"#
    )
}

pub(crate) fn core_xml(title: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title><dc:creator>Ultar OS</dc:creator></cp:coreProperties>"#,
        esc(title)
    )
}
