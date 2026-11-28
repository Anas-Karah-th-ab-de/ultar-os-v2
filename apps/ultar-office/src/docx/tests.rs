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
    d.blocks
        .push(Block::Para(Para::new(Style::Normal, "A picture")));
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
