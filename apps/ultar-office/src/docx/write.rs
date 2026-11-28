//! The writer of the docx: paragraphs, runs, tables, pictures and the package parts.

use super::*;

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
        rpr.push_str(&format!(
            r#"<w:sz w:val="{half}"/><w:szCs w:val="{half}"/>"#
        ));
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

/// The id of the style of a paragraph in `styles.xml` (none for normal text).
fn style_id(style: Style) -> Option<&'static str> {
    match style {
        Style::Normal => None,
        Style::Title => Some("Title"),
        Style::H1 => Some("Heading1"),
        Style::H2 => Some("Heading2"),
        Style::H3 => Some("Heading3"),
        Style::Bullet | Style::Number => Some("ListParagraph"),
        Style::Quote => Some("Quote"),
        Style::Code => Some("Code"),
    }
}

fn para_xml(p: &Para) -> String {
    let rtl = p.rtl();
    let mut ppr = String::new();
    let sid = style_id(p.style);
    if let Some(s) = sid {
        ppr.push_str(&format!(r#"<w:pStyle w:val="{s}"/>"#));
    }
    // the order of the elements is the one of the schema (Word is strict about it)
    if p.page_break {
        ppr.push_str("<w:pageBreakBefore/>");
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
    if let Some(ls) = p.line_spacing {
        let line = (ls * 240.0).round().max(1.0) as u32;
        ppr.push_str(&format!(
            r#"<w:spacing w:line="{line}" w:lineRule="auto"/>"#
        ));
    }
    if let Some(ind) = p.indent_pt {
        let dxa = (ind * 20.0).round().max(0.0) as u32;
        ppr.push_str(&format!(r#"<w:ind w:start="{dxa}"/>"#));
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
    if name.to_ascii_lowercase().ends_with(".jpeg") | name.to_ascii_lowercase().ends_with(".jpg") {
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

/// The section properties: paper size, margins and the direction of the document.
fn sect_xml(d: &Document) -> String {
    let rtl_doc = d.paras().next().is_some_and(|p| is_rtl(&p.text()));
    let tw = |pt: f32| (pt * 20.0).round().max(0.0) as u32;
    let pg = &d.page;
    format!(
        r#"<w:sectPr><w:pgSz w:w="{}" w:h="{}"/><w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="708" w:footer="708" w:gutter="0"/>{}</w:sectPr>"#,
        tw(pg.width_pt),
        tw(pg.height_pt),
        tw(pg.margin_top),
        tw(pg.margin_right), // physical
        tw(pg.margin_bottom),
        tw(pg.margin_left), // physical
        if rtl_doc { "<w:bidi/>" } else { "" }
    )
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
    let sect = sect_xml(d);
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
