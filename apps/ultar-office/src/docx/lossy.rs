//! What reading a `.docx` cannot keep. The document model has paragraphs with runs, plain
//! tables and pictures; everything else of a Word file is dropped on the next save. This
//! module names those things so the word processor can warn before it overwrites the file
//! (and keep the original next to it).

use super::{attr, local, read_zip_entry};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::Cursor;

/// Whether the number in the attribute `key` of `e` is greater than `n`.
fn attr_over(e: &quick_xml::events::BytesStart, key: &[u8], n: u32) -> bool {
    attr(e, key)
        .and_then(|v| v.parse::<u32>().ok())
        .is_some_and(|v| v > n)
}

/// The features that show in the names of the parts of the package.
fn part_features(bytes: &[u8]) -> Vec<&'static str> {
    let mut found: Vec<&'static str> = Vec::new();
    let Ok(z) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return found;
    };
    for name in z.file_names() {
        let k = if name.starts_with("word/header") || name.starts_with("word/footer") {
            "headers_footers"
        } else if name.starts_with("word/comments") {
            "comments"
        } else if name.starts_with("word/charts/") || name.starts_with("word/diagrams/") {
            "charts"
        } else if name.starts_with("word/embeddings/") {
            "objects"
        } else {
            continue;
        };
        if !found.contains(&k) {
            found.push(k);
        }
    }
    found
}

/// The features of a `.docx` that saving through the document model would lose, as stable
/// keys (`headers_footers`, `footnotes`, `comments`, `tracked_changes`, `text_boxes`,
/// `fields`, `links`, `merged_cells`, `nested_tables`, `table_format`, `floating_objects`,
/// `charts`, `objects`, `sections`, `columns`, `list_levels`). Empty for a file the model
/// carries completely.
pub fn lossy_features(bytes: &[u8]) -> Vec<&'static str> {
    let mut found = part_features(bytes);
    let Ok(xml) = read_zip_entry(bytes, "word/document.xml") else {
        return found;
    };
    let mut add = |k: &'static str| {
        if !found.contains(&k) {
            found.push(k);
        }
    };
    let mut r = Reader::from_reader(&xml[..]);
    let mut buf = Vec::new();
    let (mut tbl, mut sections) = (0usize, 0usize);
    loop {
        let ev = r.read_event_into(&mut buf);
        match ev {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let is_start = matches!(ev, Ok(Event::Start(_)));
                match local(e.name().as_ref()) {
                    b"tbl" if is_start => {
                        tbl += 1;
                        if tbl > 1 {
                            add("nested_tables");
                        }
                    }
                    b"ins" | b"del" | b"moveFrom" | b"moveTo" => add("tracked_changes"),
                    b"footnoteReference" | b"endnoteReference" => add("footnotes"),
                    b"txbxContent" => add("text_boxes"),
                    b"fldSimple" | b"instrText" => add("fields"),
                    b"hyperlink" => add("links"),
                    b"gridSpan" | b"hMerge" | b"vMerge" => {
                        if local(e.name().as_ref()) != b"gridSpan" || attr_over(e, b"val", 1) {
                            add("merged_cells");
                        }
                    }
                    b"shd" | b"tcBorders" | b"rPr" if tbl > 0 => add("table_format"),
                    b"anchor" | b"pict" => add("floating_objects"),
                    b"object" => add("objects"),
                    b"sectPr" if is_start => sections += 1,
                    b"cols" if attr_over(e, b"num", 1) => add("columns"),
                    b"ilvl" if attr_over(e, b"val", 0) => add("list_levels"),
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) if local(e.name().as_ref()) == b"tbl" => {
                tbl = tbl.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    if sections > 1 {
        add("sections");
    }
    found
}
