//! Excel-Dateien (.xlsx, Office Open XML) lesen und schreiben – ohne Fremdprogramm.
//! Gelesen werden alle Blätter mit Zahlen, Texten (gemeinsame und eingebettete Zeichenketten), Wahrheitswerten,
//! Formeln und Spaltenbreiten. Geschrieben werden Formeln mitsamt berechnetem Ergebnis, damit auch Programme ohne
//! Neuberechnung (Vorschau, Handy) die Werte zeigen.

use crate::docx::{esc, zip_write};
use std::io::Read;

mod sheet_in;
mod sheet_out;
mod styles_in;
mod styles_out;

use self::sheet_in::read_sheet;
use self::sheet_out::sheet_xml;
use self::styles_in::read_styles;
use self::styles_out::{build_style_tables, styles_xml};

/// Largest decompressed part of an .xlsx that is read (a zip bomb stops here).
pub const MAX_PART_BYTES: u64 = 256 << 20;
/// Most cells of one sheet that are read; more is TOO_LARGE.
pub const MAX_CELLS: usize = 4_000_000;
/// The error of a workbook that is too large to open.
pub const TOO_LARGE: &str = "too large";

/// One part of the zip file, at most [MAX_PART_BYTES] big.
fn read_zip_entry(bytes: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("not a zip/office file: {e}"))?;
    let f = z.by_name(name).map_err(|_| format!("{name} missing"))?;
    if f.size() > MAX_PART_BYTES {
        return Err(TOO_LARGE.into());
    }
    let mut v = Vec::new();
    f.take(MAX_PART_BYTES + 1)
        .read_to_end(&mut v)
        .map_err(|e| e.to_string())?;
    if v.len() as u64 > MAX_PART_BYTES {
        return Err(TOO_LARGE.into());
    }
    Ok(v)
}
use crate::sheet::{Sheet, Workbook};
use quick_xml::events::Event;
use quick_xml::Reader;

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
        .map(|a| {
            a.unescape_value()
                .map(|v| v.into_owned())
                .unwrap_or_else(|_| String::from_utf8_lossy(&a.value).into_owned())
        })
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

/// .xlsx lesen
pub fn read(bytes: &[u8]) -> Result<Workbook, String> {
    let ss = shared_strings(bytes);
    let styles = read_styles(bytes);
    let mut wb = Workbook { sheets: Vec::new() };
    for (name, path) in sheet_list(bytes)? {
        let x = match read_zip_entry(bytes, &path) {
            Ok(x) => x,
            Err(e) if e == TOO_LARGE => return Err(e),
            Err(_) => Vec::new(),
        };
        wb.sheets.push(read_sheet(&x, &name, &ss, &styles)?);
    }
    if wb.sheets.is_empty() {
        wb.sheets.push(Sheet::new("Sheet1"));
    }
    Ok(wb)
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
    let mut calc = crate::calc::Calc::new(crate::sheet::excel_now());
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
        sheets_xml.push(sheet_xml(w, i, &tables, &mut calc));
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
    use crate::sheet::Value;
    use crate::style::{Align, CellStyle};

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
        wb.sheets[0].rtl = true;
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
