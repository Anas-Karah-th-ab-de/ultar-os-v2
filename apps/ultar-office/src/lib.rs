//! ultar-office – die Büro-Apps von Ultar OS als reine Rust-Bibliothek.
//!
//! | App | Formate | Modul |
//! |---|---|---|
//! | PDF-Leser | .pdf (rendern, Text, Suche) | [`pdf`] |
//! | Schreiben (Word) | .docx lesen/schreiben, .md, .txt | [`doc`], [`docx`] |
//! | Tabellen (Excel) | .xlsx lesen/schreiben mit Formeln, .csv/.tsv | [`sheet`], [`xlsx`] |
//! | Code (Notepad++) | jede Textdatei, Kodierungen, Hervorhebung | [`code`] |
//!
//! Keine C-Bibliotheken, kein Ubuntu-Bereich: statisch gebaut läuft alles auf dem Linux-Kern des Handys und
//! auf dem eigenen Ultar-Kern (Linux-ABI). Der KI-Agent greift über dieselben Funktionen zu – in der Oberfläche
//! als Werkzeuge, von außen über das Befehlszeilenprogramm `udoc` (JSON-Ausgabe).

pub mod calc;
pub mod code;
pub mod doc;
pub mod docx;
pub mod numfmt;
pub mod pdf;
pub mod pdfout;
mod pdf_text;
pub mod sheet;
pub mod sheet_edit;
pub mod sheet_find;
pub mod sheet_refs;
pub mod style;
pub mod xlsx;

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Pdf,
    /// Textdokument (.docx, .md als Dokument)
    Doc,
    /// Tabelle (.xlsx, .csv, .tsv)
    Sheet,
    /// Text/Quellcode
    Text,
    /// nicht unterstützt (Bild, Archiv …)
    Other,
}

impl Kind {
    pub fn of(path: &str) -> Kind {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "pdf" => Kind::Pdf,
            "docx" | "docm" | "dotx" => Kind::Doc,
            "xlsx" | "xlsm" | "xltx" | "csv" | "tsv" => Kind::Sheet,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "avi" | "mp4" | "mkv" | "mp3"
            | "ogg" | "wav" | "zip" | "gz" | "xz" | "zst" | "tar" | "uap" | "deb" | "apk"
            | "exe" | "msi" | "so" | "bin" | "img" | "iso" | "gguf" | "ttf" | "otf" | "doc"
            | "xls" | "ppt" | "pptx" | "odt" | "ods" => Kind::Other,
            _ => Kind::Text,
        }
    }
}

/// Ist die Datei eine Textdatei? (für unbekannte Endungen: keine NUL-Bytes am Anfang)
pub fn looks_text(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return true;
    }
    !bytes.iter().take(8192).any(|&b| b == 0)
}

pub fn read_doc(path: &str) -> Result<doc::Document, String> {
    let b = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "docx" | "docm" | "dotx" => docx::read(&b),
        "md" | "markdown" => Ok(doc::Document::from_markdown(&code::decode(&b).0)),
        _ => Ok(doc::Document::from_text(&code::decode(&b).0)),
    }
}

pub fn write_doc(path: &str, d: &doc::Document) -> Result<(), String> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = match ext.as_str() {
        "docx" => docx::write(d)?,
        "md" | "markdown" => d.to_markdown().into_bytes(),
        _ => d.to_text().into_bytes(),
    };
    write_atomic(path, &bytes)
}

pub fn read_book(path: &str) -> Result<sheet::Workbook, String> {
    let b = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "csv" | "tsv" | "txt" => {
            let name = Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Sheet1");
            let mut sh = sheet::read_csv(&code::decode(&b).0);
            sh.name = name.chars().take(31).collect();
            Ok(sheet::Workbook { sheets: vec![sh] })
        }
        _ => xlsx::read(&b),
    }
}

pub fn write_book(path: &str, wb: &sheet::Workbook, active: usize) -> Result<(), String> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = match ext.as_str() {
        "csv" => sheet::write_csv(wb, active, ',').into_bytes(),
        "tsv" => sheet::write_csv(wb, active, '\t').into_bytes(),
        _ => xlsx::write(wb)?,
    };
    write_atomic(path, &bytes)
}

/// Erst in eine Nachbardatei schreiben, dann umbenennen – bei Stromausfall bleibt die alte Fassung heil.
pub fn write_atomic(path: &str, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }
    let tmp = format!("{path}.ultar-tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("{path}: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{path}: {e}")
    })
}

/// Inhalt einer beliebigen unterstützten Datei als Text (für Agent, Suche, Vorschau).
/// `max` begrenzt die Länge in Zeichen.
pub fn read_text(path: &str, max: usize) -> Result<String, String> {
    let t = match Kind::of(path) {
        Kind::Pdf => {
            let b = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
            pdf::PdfDoc::open(b)?.text(60)
        }
        Kind::Doc => read_doc(path)?.to_markdown(),
        Kind::Sheet => {
            let wb = read_book(path)?;
            let mut out = String::new();
            for (i, sh) in wb.sheets.iter().enumerate() {
                if wb.sheets.len() > 1 {
                    out.push_str(&format!("## {}\n", sh.name));
                }
                out.push_str(&sheet_table(&wb, i, None));
            }
            out
        }
        Kind::Text => {
            let b = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
            if !looks_text(&b) {
                return Err("not a text file".into());
            }
            code::decode(&b).0
        }
        Kind::Other => return Err("unsupported file type".into()),
    };
    Ok(if t.chars().count() > max {
        t.chars().take(max).collect::<String>() + "\n…"
    } else {
        t
    })
}

/// Blatt (oder Bereich) als Text-Tabelle mit Zeilen-/Spaltenköpfen: Werte, Formeln in Klammern
pub fn sheet_table(
    wb: &sheet::Workbook,
    si: usize,
    range: Option<((u32, u32), (u32, u32))>,
) -> String {
    let Some(sh) = wb.sheets.get(si) else {
        return String::new();
    };
    let (rows, cols) = sh.extent();
    if rows == 0 {
        return "(empty)\n".into();
    }
    let ((r0, c0), (r1, c1)) = range.unwrap_or(((0, 0), (rows.min(200) - 1, cols.min(26) - 1)));
    let mut out = String::new();
    out.push_str("   \t");
    out.push_str(
        &(c0..=c1)
            .map(sheet::col_name)
            .collect::<Vec<_>>()
            .join("\t"),
    );
    out.push('\n');
    for r in r0..=r1 {
        out.push_str(&format!("{}\t", r + 1));
        let cells: Vec<String> = (c0..=c1)
            .map(|c| {
                let raw = sh.get(r, c);
                let v = wb.value(si, r, c).text();
                if raw.starts_with('=') {
                    format!("{v} [{raw}]")
                } else {
                    v
                }
            })
            .collect();
        out.push_str(&cells.join("\t"));
        out.push('\n');
    }
    out
}

/// Kurzinfo zu einer Datei (Agent, `udoc info`)
pub fn info(path: &str) -> Result<serde_json::Value, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
    let kind = Kind::of(path);
    let mut v = serde_json::json!({"path": path, "kind": kind, "bytes": meta.len()});
    match kind {
        Kind::Pdf => {
            let d = pdf::PdfDoc::open(std::fs::read(path).map_err(|e| e.to_string())?)?;
            let i = d.info();
            v["pages"] = i.pages.into();
            v["title"] = i.title.into();
            v["author"] = i.author.into();
            v["page_size_pt"] = serde_json::json!([i.size_pt.0, i.size_pt.1]);
        }
        Kind::Doc => {
            let d = read_doc(path)?;
            v["paragraphs"] = d.paras().count().into();
            v["tables"] = d
                .blocks
                .iter()
                .filter(|b| matches!(b, doc::Block::Table(_)))
                .count()
                .into();
            v["words"] = d.word_count().into();
        }
        Kind::Sheet => {
            let wb = read_book(path)?;
            v["sheets"] = wb
                .sheets
                .iter()
                .map(|s| {
                    let (r, c) = s.extent();
                    serde_json::json!({"name": s.name, "rows": r, "cols": c})
                })
                .collect::<Vec<_>>()
                .into();
        }
        Kind::Text => {
            let b = std::fs::read(path).map_err(|e| e.to_string())?;
            let buf = code::Buffer::from_bytes(&b);
            let (l, w, c) = buf.stats();
            v["lines"] = l.into();
            v["words"] = w.into();
            v["chars"] = c.into();
            v["encoding"] = buf.enc.label().into();
            v["eol"] = buf.eol.label().into();
            v["language"] = code::Lang::from_path(path).name().into();
        }
        Kind::Other => {}
    }
    Ok(v)
}

/// Suchen/Ersetzen in Dokument oder Textdatei (Agent). Liefert Anzahl.
pub fn replace_in_file(path: &str, find: &str, repl: &str, regex: bool) -> Result<usize, String> {
    match Kind::of(path) {
        Kind::Doc => {
            let mut d = read_doc(path)?;
            let n = d.replace(find, repl);
            if n > 0 {
                write_doc(path, &d)?;
            }
            Ok(n)
        }
        Kind::Text => {
            let b = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
            let mut buf = code::Buffer::from_bytes(&b);
            let n = buf.replace_all(
                find,
                repl,
                &code::FindOpt {
                    case: true,
                    word: false,
                    regex,
                },
            )?;
            if n > 0 {
                write_atomic(path, &buf.to_bytes())?;
            }
            Ok(n)
        }
        Kind::Sheet => {
            let mut wb = read_book(path)?;
            let mut n = 0;
            for sh in &mut wb.sheets {
                for v in sh.cells.values_mut() {
                    let k = v.matches(find).count();
                    if k > 0 {
                        n += k;
                        *v = v.replace(find, repl);
                    }
                }
            }
            if n > 0 {
                write_book(path, &wb, 0)?;
            }
            Ok(n)
        }
        _ => Err("cannot edit this file type".into()),
    }
}

/// Neue Datei mit Inhalt: .docx aus Markdown, .xlsx/.csv aus CSV-Text, sonst Text
pub fn create(path: &str, content: &str) -> Result<(), String> {
    match Kind::of(path) {
        Kind::Doc => write_doc(path, &doc::Document::from_markdown(content)),
        Kind::Sheet => {
            let mut sh = sheet::read_csv(content);
            sh.name = "Sheet1".into();
            write_book(path, &sheet::Workbook { sheets: vec![sh] }, 0)
        }
        Kind::Text => write_atomic(path, content.as_bytes()),
        Kind::Pdf => Err("creating PDF files is not supported yet – create a .docx instead".into()),
        Kind::Other => Err("unsupported file type".into()),
    }
}

/// Zelle(n) setzen (Agent, `udoc sheet set`): liefert den neuen Wert
pub fn sheet_set(path: &str, cell: &str, raw: &str) -> Result<String, String> {
    let mut wb = if Path::new(path).exists() {
        read_book(path)?
    } else {
        sheet::Workbook::new()
    };
    if raw.starts_with('=') {
        sheet::check_formula(raw).map_err(|e| format!("formula: {e}"))?;
    }
    let (si, r, c) = wb.set_ref(0, cell, raw)?;
    write_book(path, &wb, si)?;
    Ok(wb.value(si, r, c).text())
}

/// Bereich lesen (Agent, `udoc sheet get`)
pub fn sheet_get(path: &str, range: &str) -> Result<String, String> {
    let wb = read_book(path)?;
    if range.trim().is_empty() {
        return Ok((0..wb.sheets.len())
            .map(|i| format!("## {}\n{}", wb.sheets[i].name, sheet_table(&wb, i, None)))
            .collect::<Vec<_>>()
            .join("\n"));
    }
    let (sh, a, b) = sheet::parse_range(range).ok_or_else(|| format!("bad range: {range}"))?;
    let si = match sh {
        Some(n) => wb.sheet_index(&n).ok_or_else(|| format!("no sheet {n}"))?,
        None => 0,
    };
    if a == b {
        let raw = wb.sheets[si].get(a.0, a.1).to_string();
        let v = wb.value(si, a.0, a.1).text();
        return Ok(if raw.starts_with('=') {
            format!("{v} [{raw}]")
        } else {
            v
        });
    }
    Ok(sheet_table(&wb, si, Some((a, b))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datei_api() {
        let d = std::env::temp_dir().join(format!("ultar-office-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let p = |n: &str| d.join(n).to_string_lossy().into_owned();
        create(&p("brief.docx"), "# Brief\n\nHallo **Welt**").unwrap();
        assert!(read_text(&p("brief.docx"), 1000)
            .unwrap()
            .contains("**Welt**"));
        assert_eq!(
            replace_in_file(&p("brief.docx"), "Welt", "Ultar", false).unwrap(),
            1
        );
        assert!(read_text(&p("brief.docx"), 1000).unwrap().contains("Ultar"));
        assert_eq!(sheet_set(&p("k.xlsx"), "A1", "5").unwrap(), "5");
        assert_eq!(sheet_set(&p("k.xlsx"), "A2", "7").unwrap(), "7");
        assert_eq!(
            sheet_set(&p("k.xlsx"), "A3", "=SUM(A1:A2)*2").unwrap(),
            "24"
        );
        assert!(sheet_get(&p("k.xlsx"), "A3")
            .unwrap()
            .starts_with("24 [=SUM"));
        assert!(sheet_set(&p("k.xlsx"), "A4", "=SUM(A1").is_err());
        create(&p("x.rs"), "fn main() {}\n").unwrap();
        assert_eq!(info(&p("x.rs")).unwrap()["language"], "Rust");
        create(&p("t.csv"), "a,b\n1,2\n").unwrap();
        assert_eq!(sheet_get(&p("t.csv"), "B2").unwrap(), "2");
        let _ = std::fs::remove_dir_all(&d);
    }
}
