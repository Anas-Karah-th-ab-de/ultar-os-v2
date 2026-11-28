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
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"Relationship" => {
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
                        para = Some(Para::default());
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
                        if let Some(rr) = run.as_mut() {
                            rr.text.push('\n');
                        }
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
                                Some(l)
                                    if l.bold == rr.bold
                                        && l.italic == rr.italic
                                        && l.underline == rr.underline =>
                                {
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
                        if tbl_depth > 0 {
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
                            let name = target
                                .rsplit('/')
                                .next()
                                .unwrap_or("image.png")
                                .to_owned();
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

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            // in XML 1.0 verbotene Steuerzeichen weglassen
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => o.push(c),
        }
    }
    o
}

fn run_xml(r: &Run, rtl: bool, mono: bool) -> String {
    let mut rpr = String::new();
    if mono {
        rpr.push_str(
            r#"<w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New"/>"#,
        );
    }
    if r.bold {
        rpr.push_str("<w:b/><w:bCs/>");
    }
    if r.italic {
        rpr.push_str("<w:i/><w:iCs/>");
    }
    if r.underline {
        rpr.push_str(r#"<w:u w:val="single"/>"#);
    }
    if r.strike {
        rpr.push_str("<w:strike/>");
    }
    if let Some(pt) = r.size_pt {
        let half = (pt * 2.0).round().max(2.0) as u32;
        rpr.push_str(&format!(r#"<w:sz w:val="{half}"/><w:szCs w:val="{half}"/>"#));
    }
    if let Some(c) = &r.color {
        if !c.is_empty() {
            rpr.push_str(&format!(r#"<w:color w:val="{c}"/>"#));
        }
    }
    if let Some(h) = &r.highlight {
        if !h.is_empty() {
            rpr.push_str(&format!(
                r#"<w:shd w:val="clear" w:color="auto" w:fill="{h}"/>"#
            ));
        }
    }
    if rtl {
        rpr.push_str("<w:rtl/>");
    }
    let mut body = String::new();
    for (i, part) in r.text.split('\n').enumerate() {
        if i > 0 {
            body.push_str("<w:br/>");
        }
        for (k, seg) in part.split('\t').enumerate() {
            if k > 0 {
                body.push_str("<w:tab/>");
            }
            if !seg.is_empty() {
                body.push_str(&format!(r#"<w:t xml:space="preserve">{}</w:t>"#, esc(seg)));
            }
        }
    }
    if rpr.is_empty() {
        format!("<w:r>{body}</w:r>")
    } else {
        format!("<w:r><w:rPr>{rpr}</w:rPr>{body}</w:r>")
    }
}

fn para_xml(p: &Para) -> String {
    let rtl = p.rtl();
    let mut ppr = String::new();
    let sid = match p.style {
        Style::Normal => None,
        Style::Title => Some("Title"),
        Style::H1 => Some("Heading1"),
        Style::H2 => Some("Heading2"),
        Style::H3 => Some("Heading3"),
        Style::Bullet | Style::Number => Some("ListParagraph"),
        Style::Quote => Some("Quote"),
        Style::Code => Some("Code"),
    };
    if let Some(s) = sid {
        ppr.push_str(&format!(r#"<w:pStyle w:val="{s}"/>"#));
    }
    match p.style {
        Style::Bullet => {
            ppr.push_str(r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>"#)
        }
        Style::Number => {
            ppr.push_str(r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr>"#)
        }
        _ => {}
    }
    if rtl {
        ppr.push_str("<w:bidi/>");
    }
    if let Some(ind) = p.indent_pt {
        let dxa = (ind * 20.0).round().max(0.0) as u32;
        ppr.push_str(&format!(r#"<w:ind w:start="{dxa}"/>"#));
    }
    if let Some(ls) = p.line_spacing {
        let line = (ls * 240.0).round().max(1.0) as u32;
        ppr.push_str(&format!(r#"<w:spacing w:line="{line}" w:lineRule="auto"/>"#));
    }
    let jc = match (p.align, rtl) {
        (Align::Center, _) => Some("center"),
        (Align::Justify, _) => Some("both"),
        // in RTL-Absätzen bedeutet "left" den Anfang (rechts) – Word dreht selbst
        (Align::End, false) => Some("right"),
        (Align::End, true) => Some("left"),
        _ => None,
    };
    if let Some(j) = jc {
        ppr.push_str(&format!(r#"<w:jc w:val="{j}"/>"#));
    }
    let runs: String = p
        .runs
        .iter()
        .map(|r| run_xml(r, rtl, p.style == Style::Code))
        .collect();
    if ppr.is_empty() {
        format!("<w:p>{runs}</w:p>")
    } else {
        format!("<w:p><w:pPr>{ppr}</w:pPr>{runs}</w:p>")
    }
}

fn table_xml(t: &[Vec<String>]) -> String {
    let cols = t.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
    let w = 9000 / cols;
    let mut s = String::from(
        r#"<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="5000" w:type="pct"/><w:tblBorders><w:top w:val="single" w:sz="4" w:color="999999"/><w:left w:val="single" w:sz="4" w:color="999999"/><w:bottom w:val="single" w:sz="4" w:color="999999"/><w:right w:val="single" w:sz="4" w:color="999999"/><w:insideH w:val="single" w:sz="4" w:color="999999"/><w:insideV w:val="single" w:sz="4" w:color="999999"/></w:tblBorders></w:tblPr><w:tblGrid>"#,
    );
    for _ in 0..cols {
        s.push_str(&format!(r#"<w:gridCol w:w="{w}"/>"#));
    }
    s.push_str("</w:tblGrid>");
    for (ri, row) in t.iter().enumerate() {
        s.push_str("<w:tr>");
        for c in 0..cols {
            let txt = row.get(c).map(|x| x.as_str()).unwrap_or("");
            let mut p = Para::new(Style::Normal, txt);
            if ri == 0 {
                p.runs[0].bold = true;
            }
            s.push_str(&format!(
                r#"<w:tc><w:tcPr><w:tcW w:w="{w}" w:type="dxa"/></w:tcPr>{}</w:tc>"#,
                para_xml(&p)
            ));
        }
        s.push_str("</w:tr>");
    }
    s.push_str("</w:tbl>");
    s
}

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:cs="Arial" w:eastAsia="Calibri"/><w:sz w:val="24"/><w:szCs w:val="24"/><w:lang w:val="de-DE" w:bidi="ar-SA"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>
<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="240"/></w:pPr><w:rPr><w:b/><w:bCs/><w:color w:val="1F2937"/><w:sz w:val="52"/><w:szCs w:val="52"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="360" w:after="120"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:bCs/><w:color w:val="8A6D0B"/><w:sz w:val="40"/><w:szCs w:val="40"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="80"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:bCs/><w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="200" w:after="60"/><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:b/><w:bCs/><w:sz w:val="28"/><w:szCs w:val="28"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="60"/><w:ind w:left="720"/><w:contextualSpacing/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:ind w:left="720" w:right="720"/></w:pPr><w:rPr><w:i/><w:iCs/><w:color w:val="555555"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Code"><w:name w:val="Code"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="0"/><w:shd w:val="clear" w:color="auto" w:fill="F3F4F6"/></w:pPr><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New"/><w:sz w:val="21"/></w:rPr></w:style>
<w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:tblPr><w:tblCellMar><w:left w:w="108" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>
</w:styles>"#;

const NUMBERING: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="singleLevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum>
<w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="singleLevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
<w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>"#;

const CONTENT_TYPES_BASE: &str = r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Default Extension="jpg" ContentType="image/jpeg"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#;

const DOC_RELS_BASE: &str = r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>"#;

/// The content type of an image part by its file name.
fn image_mime(name: &str) -> &'static str {
    if name
        .to_ascii_lowercase()
        .ends_with(".jpeg")
        | name.to_ascii_lowercase().ends_with(".jpg")
    {
        "image/jpeg"
    } else {
        "image/png"
    }
}

fn content_types_xml(images: &[(String, Vec<u8>)]) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    );
    s.push_str(CONTENT_TYPES_BASE);
    for (name, _) in images {
        s.push_str(&format!(
            r#"<Override PartName="/word/media/{name}" ContentType="{}"/>"#,
            image_mime(name)
        ));
    }
    s.push_str("</Types>");
    s
}

fn doc_rels_xml(images: &[(String, Vec<u8>)]) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    s.push_str(DOC_RELS_BASE);
    for (i, (name, _)) in images.iter().enumerate() {
        s.push_str(&format!(
            r#"<Relationship Id="rIdImg{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/{name}"/>"#,
            i + 1
        ));
    }
    s.push_str("</Relationships>");
    s
}

/// Displayed size of a picture in EMU (914 400 per inch, 12 700 per point).
fn picture_extent_emu(pic: &Picture) -> (i64, i64) {
    let cx = pic.width_pt.unwrap_or(200.0).max(1.0) * 12700.0;
    let cy = pic.height_pt.unwrap_or(150.0).max(1.0) * 12700.0;
    (cx.round() as i64, cy.round() as i64)
}

/// A paragraph that shows one picture (`w:drawing`).
fn picture_xml(idx: u32, cx: i64, cy: i64) -> String {
    let rid = format!("rIdImg{idx}");
    format!(
        r#"<w:p><w:r><w:drawing><wp:inline><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{idx}" name="Picture {idx}"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{idx}" name="Picture {idx}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#
    )
}

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#;

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

pub(crate) fn zip_write(files: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opt = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (n, d) in files {
        w.start_file(*n, opt).map_err(|e| e.to_string())?;
        w.write_all(d).map_err(|e| e.to_string())?;
    }
    Ok(w.finish().map_err(|e| e.to_string())?.into_inner())
}

/// .docx schreiben
pub fn write(d: &Document) -> Result<Vec<u8>, String> {
    let mut body = String::new();
    let mut images: Vec<(String, Vec<u8>)> = Vec::new();
    for b in &d.blocks {
        match b {
            Block::Para(p) => body.push_str(&para_xml(p)),
            Block::Table(t) => body.push_str(&table_xml(t)),
            Block::Picture(pic) => {
                let idx = images.len() + 1;
                let name = if pic.name.trim().is_empty() {
                    format!("image{idx}.png")
                } else {
                    pic.name.trim().to_owned()
                };
                let (cx, cy) = picture_extent_emu(pic);
                body.push_str(&picture_xml(idx as u32, cx, cy));
                images.push((name.to_owned(), pic.bytes.clone()));
            }
        }
    }
    let rtl_doc = d.paras().next().is_some_and(|p| is_rtl(&p.text()));
    let sect = format!(
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1417" w:bottom="1134" w:left="1417" w:header="708" w:footer="708" w:gutter="0"/>{}</w:sectPr>"#,
        if rtl_doc { "<w:bidi/>" } else { "" }
    );
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body>{body}<w:p/>{sect}</w:body></w:document>"#
    );
    let title = d
        .paras()
        .find(|p| !p.text().trim().is_empty())
        .map(|p| p.text())
        .unwrap_or_default();
    let core = core_xml(&title.chars().take(80).collect::<String>());
    let app = app_xml("Writer");
    let rels = doc_rels_xml(&images);
    let types = content_types_xml(&images);
    let mut parts: Vec<String> = Vec::new();
    for (name, _) in &images {
        parts.push(if name.starts_with("word/") {
            name.clone()
        } else {
            format!("word/media/{name}")
        });
    }
    let mut files: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", RELS.as_bytes()),
        ("word/document.xml", xml.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
        ("word/styles.xml", STYLES.as_bytes()),
        ("word/numbering.xml", NUMBERING.as_bytes()),
        ("docProps/core.xml", core.as_bytes()),
        ("docProps/app.xml", app.as_bytes()),
    ];
    for (i, (_, data)) in images.iter().enumerate() {
        files.push((parts[i].as_str(), data.as_slice()));
    }
    zip_write(&files)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The writer ends the body with an empty paragraph (Word wants one after a table).
    fn strip_trailing_empty(mut d: Document) -> Document {
        while matches!(d.blocks.last(), Some(Block::Para(p)) if p.text().is_empty()) {
            d.blocks.pop();
        }
        d
    }

    /// `Auto` direction resolves at write time (a right-to-left paragraph is stored as
    /// `<w:bidi/>`), so the round trip cannot tell `Auto` from the resolved direction.
    fn resolved(mut d: Document) -> Document {
        for b in &mut d.blocks {
            if let Block::Para(p) = b {
                p.dir = if p.rtl() { Dir::Rtl } else { Dir::Ltr };
            }
        }
        d
    }

    #[test]
    fn docx_hin_und_zurueck() {
        let md = "# Angebot\n\nSehr geehrte **Damen** und *Herren*,\n\n## Positionen\n- Beratung\n- Umsetzung\n1. Start\n2. Ende\n\n> Zitat & <Test>\n\nمرحبا بكم في KM Code Craft\n\n| Pos | Preis |\n|---|---|\n| A | 10 |\n";
        let d = Document::from_markdown(md);
        let bytes = write(&d).unwrap();
        let back = read(&bytes).unwrap();
        assert_eq!(resolved(strip_trailing_empty(back)), resolved(d));
        assert!(
            String::from_utf8_lossy(&read_zip_entry(&bytes, "word/document.xml").unwrap())
                .contains("<w:bidi/>")
        );
    }

    #[test]
    fn run_and_para_formatting_round_trips() {
        let mut d = Document::default();
        let mut run = Run::plain("سلام");
        run.bold = true;
        run.strike = true;
        run.size_pt = Some(18.0);
        run.color = Some("1F2937".into());
        run.highlight = Some("FFF000".into());
        let mut p = Para::new(Style::Normal, "");
        p.runs = vec![run];
        p.dir = Dir::Rtl;
        p.line_spacing = Some(1.5);
        p.indent_pt = Some(36.0);
        d.blocks.push(Block::Para(p));

        let bytes = write(&d).unwrap();
        let back = strip_trailing_empty(read(&bytes).unwrap());
        assert_eq!(back.blocks.len(), 1);
        let Some(Block::Para(q)) = back.blocks.first() else {
            panic!("expected a paragraph");
        };
        assert_eq!(q.dir, Dir::Rtl);
        assert!(q.rtl());
        assert_eq!(q.line_spacing, Some(1.5));
        assert_eq!(q.indent_pt, Some(36.0));
        let r = &q.runs[0];
        assert!(r.strike);
        assert_eq!(r.size_pt, Some(18.0));
        assert_eq!(r.color.as_deref(), Some("1F2937"));
        assert_eq!(r.highlight.as_deref(), Some("FFF000"));
    }

    #[test]
    fn pictures_round_trip() {
        let png: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0, 0, 0, 0];
        let mut d = Document::default();
        d.blocks.push(Block::Para(Para::new(Style::Normal, "A picture")));
        d.blocks.push(Block::Picture(Picture {
            bytes: png.clone(),
            name: "image1.png".into(),
            width_pt: Some(100.0),
            height_pt: Some(50.0),
        }));
        let bytes = write(&d).unwrap();
        let back = strip_trailing_empty(read(&bytes).unwrap());
        assert_eq!(back.blocks.len(), 2);
        let pic = back.blocks.iter().find_map(|b| match b {
            Block::Picture(p) => Some(p),
            _ => None,
        });
        let pic = pic.expect("picture round-trips");
        assert_eq!(pic.bytes, png);
        assert_eq!(pic.name, "image1.png");
        assert!((pic.width_pt.unwrap() - 100.0).abs() < 0.1);
        assert!((pic.height_pt.unwrap() - 50.0).abs() < 0.1);
    }
}
