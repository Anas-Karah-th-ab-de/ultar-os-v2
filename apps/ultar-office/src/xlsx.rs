//! Excel-Dateien (.xlsx, Office Open XML) lesen und schreiben – ohne Fremdprogramm.
//! Gelesen werden alle Blätter mit Zahlen, Texten (gemeinsame und eingebettete Zeichenketten), Wahrheitswerten,
//! Formeln und Spaltenbreiten. Geschrieben werden Formeln mitsamt berechnetem Ergebnis, damit auch Programme ohne
//! Neuberechnung (Vorschau, Handy) die Werte zeigen.

use crate::docx::{esc, read_zip_entry, zip_write};
use crate::sheet::{cell_name, col_name, fmt_num, parse_cell, Sheet, Value, Workbook};
use quick_xml::events::Event;
use quick_xml::Reader;

fn local(name: &[u8]) -> &[u8] {
    match name.iter().position(|&c| c == b':') {
        Some(p) => &name[p + 1..],
        None => name,
    }
}

fn attr(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.as_ref() == key || local(a.key.as_ref()) == key && !a.key.as_ref().starts_with(b"xmlns")).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

fn shared_strings(bytes: &[u8]) -> Vec<String> {
    let Ok(x) = read_zip_entry(bytes, "xl/sharedStrings.xml") else { return Vec::new() };
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
            Ok(Event::Text(t)) if in_t => cur.push_str(&t.unescape().map(|c| c.into_owned()).unwrap_or_default()),
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

/// Blätter in Reihenfolge: (Name, Pfad im Archiv)
fn sheet_list(bytes: &[u8]) -> Result<Vec<(String, String)>, String> {
    let wb = read_zip_entry(bytes, "xl/workbook.xml")?;
    let rels = read_zip_entry(bytes, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let mut targets = std::collections::HashMap::new();
    let mut r = Reader::from_reader(&rels[..]);
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"Relationship" => {
                if let (Some(id), Some(t)) = (attr(&e, b"Id"), attr(&e, b"Target")) {
                    let t = t.trim_start_matches('/').to_string();
                    let t = if t.starts_with("xl/") { t } else { format!("xl/{t}") };
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
                let rid = e.attributes().flatten().find(|a| local(a.key.as_ref()) == b"id").map(|a| String::from_utf8_lossy(&a.value).into_owned());
                let path = rid.and_then(|id| targets.get(&id).cloned()).unwrap_or_else(|| format!("xl/worksheets/sheet{i}.xml"));
                out.push((name, path));
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn read_sheet(x: &[u8], name: &str, ss: &[String]) -> Sheet {
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
    loop {
        let ev = r.read_event_into(&mut buf);
        match ev {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let empty = matches!(ev, Ok(Event::Empty(_)));
                match local(e.name().as_ref()) {
                    b"row" => {
                        row_no = attr(e, b"r").and_then(|v| v.parse::<u32>().ok()).map(|v| v - 1).unwrap_or(row_no);
                        next_col = 0;
                    }
                    b"c" => {
                        let pos = attr(e, b"r").and_then(|v| parse_cell(&v)).unwrap_or((row_no, next_col));
                        next_col = pos.1 + 1;
                        typ = attr(e, b"t").unwrap_or_default();
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
                        if let (Some(a), Some(b), Some(w)) = (attr(e, b"min"), attr(e, b"max"), attr(e, b"width")) {
                            if let (Ok(a), Ok(b), Ok(w)) = (a.parse::<u32>(), b.parse::<u32>(), w.parse::<f32>()) {
                                for c in a..=b.min(a + 200) {
                                    sh.widths.insert(c - 1, w);
                                }
                            }
                        }
                    }
                    _ => {}
                }
                if local(e.name().as_ref()) == b"row" {
                    row_no = attr(e, b"r").and_then(|v| v.parse::<u32>().ok()).map(|v| v - 1).unwrap_or(row_no);
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
                            (_, "s") => val.trim().parse::<usize>().ok().and_then(|i| ss.get(i).cloned()).unwrap_or_default(),
                            (_, "b") => if val.trim() == "1" { "TRUE".into() } else { "FALSE".into() },
                            (_, "str") | (_, "inlineStr") => val.clone(),
                            (_, "e") => val.clone(),
                            _ => match val.trim().parse::<f64>() {
                                Ok(n) => fmt_num(n),
                                Err(_) => val.clone(),
                            },
                        };
                        // Text, der wie eine Zahl/Formel aussieht, bleibt Text
                        let raw = if matches!(typ.as_str(), "s" | "str" | "inlineStr") && formula.is_none() && (raw.starts_with('=') || crate::sheet::literal(&raw) != Value::Text(raw.clone())) && !raw.is_empty() { format!("'{raw}") } else { raw };
                        sh.set(rr, cc, &raw);
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
    let mut wb = Workbook { sheets: Vec::new() };
    for (name, path) in sheet_list(bytes)? {
        let x = read_zip_entry(bytes, &path).unwrap_or_default();
        wb.sheets.push(read_sheet(&x, &name, &ss));
    }
    if wb.sheets.is_empty() {
        wb.sheets.push(Sheet::new("Sheet1"));
    }
    Ok(wb)
}

fn sheet_xml(wb: &Workbook, si: usize) -> String {
    let sh = &wb.sheets[si];
    let (rows, cols) = sh.extent();
    let dim = if rows == 0 { "A1".to_string() } else { format!("A1:{}", cell_name(rows - 1, cols.saturating_sub(1))) };
    let mut s = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><dimension ref="{dim}"/><sheetViews><sheetView workbookViewId="0"{}/></sheetViews><sheetFormatPr defaultRowHeight="15"/>"#,
        if sh.cells.values().any(|v| crate::doc::is_rtl(v)) { r#" rightToLeft="1""# } else { "" });
    if !sh.widths.is_empty() {
        s.push_str("<cols>");
        for (c, w) in &sh.widths {
            s.push_str(&format!(r#"<col min="{0}" max="{0}" width="{1}" customWidth="1"/>"#, c + 1, w));
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
            s.push_str(&format!(r#"<row r="{}">"#, r + 1));
            cur_row = Some(r);
        }
        let rf = format!("{}{}", col_name(c), r + 1);
        if let Some(f) = raw.strip_prefix('=') {
            let v = wb.value(si, r, c);
            let (t, vv) = match &v {
                Value::Num(n) => ("", fmt_num(*n)),
                Value::Bool(b) => (r#" t="b""#, if *b { "1".into() } else { "0".into() }),
                Value::Err(e) => (r#" t="e""#, e.clone()),
                Value::Text(x) => (r#" t="str""#, x.clone()),
                Value::Empty => (r#" t="str""#, String::new()),
            };
            s.push_str(&format!(r#"<c r="{rf}"{t}><f>{}</f><v>{}</v></c>"#, esc(f), esc(&vv)));
        } else if let Some(text) = raw.strip_prefix('\'') {
            s.push_str(&format!(r#"<c r="{rf}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#, esc(text)));
        } else {
            match crate::sheet::literal(raw) {
                Value::Num(n) => s.push_str(&format!(r#"<c r="{rf}"><v>{}</v></c>"#, fmt_num(n))),
                Value::Bool(b) => s.push_str(&format!(r#"<c r="{rf}" t="b"><v>{}</v></c>"#, b as u8)),
                _ => s.push_str(&format!(r#"<c r="{rf}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#, esc(raw))),
            }
        }
    }
    if cur_row.is_some() {
        s.push_str("</row>");
    }
    s.push_str(r#"</sheetData><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/></worksheet>"#);
    s
}

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;

/// .xlsx schreiben
pub fn write(wb: &Workbook) -> Result<Vec<u8>, String> {
    let n = wb.sheets.len().max(1);
    let mut ct = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#);
    let mut wbx = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><bookViews><workbookView/></bookViews><sheets>"#);
    let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
    let mut sheets_xml = Vec::new();
    let empty = Workbook::new();
    let w = if wb.sheets.is_empty() { &empty } else { wb };
    for i in 0..n {
        ct.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#, i + 1));
        wbx.push_str(&format!(r#"<sheet name="{}" sheetId="{}" r:id="rId{}"/>"#, esc(&w.sheets[i].name), i + 1, i + 1));
        rels.push_str(&format!(r#"<Relationship Id="rId{0}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{0}.xml"/>"#, i + 1));
        sheets_xml.push(sheet_xml(w, i));
    }
    ct.push_str("</Types>");
    wbx.push_str(r#"</sheets><calcPr calcId="191029" fullCalcOnLoad="1"/></workbook>"#);
    rels.push_str(&format!(r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#, n + 1));
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#;
    let core = crate::docx::core_xml(&w.sheets[0].name);
    let app = crate::docx::app_xml("Sheets");
    let names: Vec<String> = (1..=n).map(|i| format!("xl/worksheets/sheet{i}.xml")).collect();
    let mut files: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", ct.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("xl/workbook.xml", wbx.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
        ("xl/styles.xml", STYLES.as_bytes()),
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
        let x = String::from_utf8(read_zip_entry(&bytes, "xl/worksheets/sheet1.xml").unwrap()).unwrap();
        assert!(x.contains("<f>SUM(B2:B3)</f><v>2000.5</v>"));
        assert!(x.contains(r#"rightToLeft="1""#));
    }
}
