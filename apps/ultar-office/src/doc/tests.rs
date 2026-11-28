use super::*;

#[test]
fn markdown_hin_und_zurueck() {
    let md = "# Bericht\n\nDas ist **wichtig** und *schön*.\n\n## Punkte\n- eins\n- zwei\n1. erst\n2. dann\n\n| A | B |\n|---|---|\n| 1 | 2 |\n";
    let d = Document::from_markdown(md);
    let ps: Vec<&Para> = d.paras().collect();
    assert_eq!(ps[0].style, Style::Title);
    assert_eq!(ps[1].runs[1].text, "wichtig");
    assert!(ps[1].runs[1].bold);
    assert!(ps[1].runs[3].italic);
    assert_eq!(ps[2].style, Style::H2);
    assert_eq!(ps[3].style, Style::Bullet);
    assert_eq!(ps[5].style, Style::Number);
    assert!(matches!(d.blocks.last(), Some(Block::Table(t)) if t.len() == 2));
    let md2 = d.to_markdown();
    assert!(md2.contains("**wichtig**"));
    assert!(md2.contains("2. dann"));
    assert_eq!(Document::from_markdown(&md2), d);
}

#[test]
fn rtl_und_ersetzen() {
    let mut d = Document::from_text("مرحبا بالعالم\nHallo Welt");
    assert!(d.paras().next().unwrap().rtl());
    assert!(!d.paras().nth(1).unwrap().rtl());
    assert_eq!(d.replace("Welt", "Ultar"), 1);
    assert!(d.to_text().contains("Hallo Ultar"));
}

#[test]
fn direction_button_wins_over_the_text() {
    let ltr = Para {
        dir: Dir::Ltr,
        ..Para::new(Style::Normal, "مرحبا")
    };
    assert!(!ltr.rtl(), "forced LTR overrides Arabic text");
    let rtl = Para {
        dir: Dir::Rtl,
        ..Para::new(Style::Normal, "hello")
    };
    assert!(rtl.rtl(), "forced RTL overrides Latin text");
    assert!(!Para::new(Style::Normal, "hello").rtl());
}

#[test]
fn runs_carry_the_formatting() {
    let mut r = Run::plain("x");
    r.strike = true;
    r.size_pt = Some(14.0);
    r.color = Some("FF0000".into());
    r.highlight = Some("FFFF00".into());
    let p = Para {
        style: Style::Normal,
        align: Align::Center,
        dir: Dir::Auto,
        line_spacing: Some(2.0),
        indent_pt: Some(24.0),
        page_break: false,
        runs: vec![r],
    };
    assert_eq!(p.runs[0].size_pt, Some(14.0));
    assert_eq!(p.runs[0].color.as_deref(), Some("FF0000"));
    assert_eq!(p.line_spacing, Some(2.0));
}
