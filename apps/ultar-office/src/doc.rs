//! Textdokument (Schreiben-App): Absätze mit Format, Tabellen. Austausch mit .docx, Markdown und Text.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    Normal,
    Title,
    H1,
    H2,
    H3,
    Bullet,
    Number,
    Quote,
    Code,
}

impl Style {
    pub const ALL: [Style; 9] = [
        Style::Normal,
        Style::Title,
        Style::H1,
        Style::H2,
        Style::H3,
        Style::Bullet,
        Style::Number,
        Style::Quote,
        Style::Code,
    ];
    /// Schriftgröße in Punkt (Bildschirm und .docx)
    pub fn size(self) -> f32 {
        match self {
            Style::Title => 26.0,
            Style::H1 => 20.0,
            Style::H2 => 16.0,
            Style::H3 => 14.0,
            Style::Code => 10.5,
            _ => 12.0,
        }
    }
    pub fn bold(self) -> bool {
        matches!(self, Style::Title | Style::H1 | Style::H2 | Style::H3)
    }
    pub fn key(self) -> &'static str {
        match self {
            Style::Normal => "normal",
            Style::Title => "title",
            Style::H1 => "h1",
            Style::H2 => "h2",
            Style::H3 => "h3",
            Style::Bullet => "bullet",
            Style::Number => "number",
            Style::Quote => "quote",
            Style::Code => "code",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Run {
    pub fn plain(t: &str) -> Self {
        Run {
            text: t.into(),
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Para {
    pub style: Style,
    pub align: Align,
    pub runs: Vec<Run>,
}

impl Para {
    pub fn new(style: Style, text: &str) -> Self {
        Para {
            style,
            align: Align::Start,
            runs: vec![Run::plain(text)],
        }
    }
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
    /// Text ersetzen; Format des ersten Laufs bleibt erhalten
    pub fn set_text(&mut self, t: &str) {
        let mut r = self.runs.first().cloned().unwrap_or_default();
        r.text = t.into();
        self.runs = vec![r];
    }
    /// Arabische/hebräische Schrift → rechts nach links
    pub fn rtl(&self) -> bool {
        is_rtl(&self.text())
    }
    pub fn all_bold(&self) -> bool {
        self.runs
            .iter()
            .any(|r| r.bold && !r.text.trim().is_empty())
            && self.runs.iter().all(|r| r.bold || r.text.trim().is_empty())
    }
}

pub fn is_rtl(s: &str) -> bool {
    for c in s.chars() {
        let u = c as u32;
        if (0x0590..=0x08FF).contains(&u)
            || (0xFB1D..=0xFDFF).contains(&u)
            || (0xFE70..=0xFEFF).contains(&u)
        {
            return true;
        }
        if c.is_alphabetic() {
            return false;
        }
    }
    false
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Para(Para),
    Table(Vec<Vec<String>>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
}

impl Document {
    pub fn paras(&self) -> impl Iterator<Item = &Para> {
        self.blocks.iter().filter_map(|b| {
            if let Block::Para(p) = b {
                Some(p)
            } else {
                None
            }
        })
    }

    pub fn word_count(&self) -> usize {
        self.to_text().split_whitespace().count()
    }

    /// Reiner Text (für Agent, Suche, Zählung)
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let mut n = 0;
        for b in &self.blocks {
            match b {
                Block::Para(p) => {
                    match p.style {
                        Style::Bullet => out.push_str("• "),
                        Style::Number => {
                            n += 1;
                            out.push_str(&format!("{n}. "));
                        }
                        _ => {}
                    }
                    if p.style != Style::Number {
                        n = 0;
                    }
                    out.push_str(&p.text());
                    out.push('\n');
                }
                Block::Table(t) => {
                    for row in t {
                        out.push_str(&row.join("\t"));
                        out.push('\n');
                    }
                }
            }
        }
        out
    }

    /// Markdown (verlustarm: Überschriften, Listen, Zitat, Code, fett/kursiv, Tabellen)
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        let mut n = 0;
        for b in &self.blocks {
            match b {
                Block::Para(p) => {
                    let mut body = String::new();
                    for r in &p.runs {
                        let t = &r.text;
                        if t.trim().is_empty() || p.style == Style::Code {
                            body.push_str(t);
                            continue;
                        }
                        let mark = match (r.bold, r.italic) {
                            (true, true) => "***",
                            (true, false) => "**",
                            (false, true) => "*",
                            _ => "",
                        };
                        body.push_str(&format!("{mark}{t}{mark}"));
                    }
                    if p.style == Style::Number {
                        n += 1;
                    } else {
                        n = 0;
                    }
                    let line = match p.style {
                        Style::Title | Style::H1 => format!("# {body}"),
                        Style::H2 => format!("## {body}"),
                        Style::H3 => format!("### {body}"),
                        Style::Bullet => format!("- {body}"),
                        Style::Number => format!("{n}. {body}"),
                        Style::Quote => format!("> {body}"),
                        Style::Code => format!("    {body}"),
                        Style::Normal => body,
                    };
                    out.push_str(&line);
                    out.push('\n');
                    if !matches!(p.style, Style::Bullet | Style::Number | Style::Code) {
                        out.push('\n');
                    }
                }
                Block::Table(t) => {
                    for (i, row) in t.iter().enumerate() {
                        out.push_str(&format!("| {} |\n", row.join(" | ")));
                        if i == 0 {
                            out.push_str(&format!("|{}\n", "---|".repeat(row.len())));
                        }
                    }
                    out.push('\n');
                }
            }
        }
        out.trim_end().to_string() + "\n"
    }

    /// Aus Markdown oder einfachem Text (Leerzeile = neuer Absatz)
    pub fn from_markdown(md: &str) -> Document {
        let mut d = Document::default();
        let mut table: Vec<Vec<String>> = Vec::new();
        let mut para = String::new();
        let flush = |d: &mut Document, para: &mut String| {
            if !para.trim().is_empty() {
                d.blocks.push(Block::Para(Para {
                    style: Style::Normal,
                    align: Align::Start,
                    runs: inline(para.trim()),
                }));
            }
            para.clear();
        };
        for line in md.lines() {
            let t = line.trim_end();
            let tt = t.trim_start();
            if tt.starts_with('|') && tt.ends_with('|') && tt.len() > 1 {
                flush(&mut d, &mut para);
                let cells: Vec<String> = tt[1..tt.len() - 1]
                    .split('|')
                    .map(|c| c.trim().to_string())
                    .collect();
                if !cells
                    .iter()
                    .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
                {
                    table.push(cells);
                }
                continue;
            } else if !table.is_empty() {
                d.blocks.push(Block::Table(std::mem::take(&mut table)));
            }
            if tt.is_empty() {
                flush(&mut d, &mut para);
                continue;
            }
            let (style, body) = if let Some(b) = tt.strip_prefix("### ") {
                (Some(Style::H3), b)
            } else if let Some(b) = tt.strip_prefix("## ") {
                (Some(Style::H2), b)
            } else if let Some(b) = tt.strip_prefix("# ") {
                (
                    Some(if d.blocks.is_empty() {
                        Style::Title
                    } else {
                        Style::H1
                    }),
                    b,
                )
            } else if let Some(b) = tt
                .strip_prefix("- ")
                .or_else(|| tt.strip_prefix("* "))
                .or_else(|| tt.strip_prefix("• "))
            {
                (Some(Style::Bullet), b)
            } else if let Some(b) = tt.strip_prefix("> ") {
                (Some(Style::Quote), b)
            } else if t.starts_with("    ") || t.starts_with('\t') {
                (
                    Some(Style::Code),
                    t.trim_start_matches('\t').strip_prefix("    ").unwrap_or(t),
                )
            } else if let Some(p) = tt
                .find(". ")
                .filter(|&p| p > 0 && p < 4 && tt[..p].chars().all(|c| c.is_ascii_digit()))
            {
                (Some(Style::Number), &tt[p + 2..])
            } else {
                (None, tt)
            };
            match style {
                Some(s) => {
                    flush(&mut d, &mut para);
                    let runs = if s == Style::Code {
                        vec![Run::plain(body)]
                    } else {
                        inline(body)
                    };
                    d.blocks.push(Block::Para(Para {
                        style: s,
                        align: Align::Start,
                        runs,
                    }));
                }
                None => {
                    if !para.is_empty() {
                        para.push(' ');
                    }
                    para.push_str(body);
                }
            }
        }
        flush(&mut d, &mut para);
        if !table.is_empty() {
            d.blocks.push(Block::Table(table));
        }
        // "# Titel" am Anfang bleibt Titel, nur wenn weitere Überschriften folgen – sonst H1
        d
    }

    /// Einfacher Text: jede Zeile ein Absatz
    pub fn from_text(t: &str) -> Document {
        Document {
            blocks: t
                .lines()
                .map(|l| Block::Para(Para::new(Style::Normal, l)))
                .collect(),
        }
    }

    /// Suchen und Ersetzen in allen Absätzen und Tabellen; liefert Anzahl
    pub fn replace(&mut self, find: &str, repl: &str) -> usize {
        if find.is_empty() {
            return 0;
        }
        let mut n = 0;
        for b in &mut self.blocks {
            match b {
                Block::Para(p) => {
                    let mut hit = false;
                    for r in &mut p.runs {
                        let k = r.text.matches(find).count();
                        if k > 0 {
                            n += k;
                            r.text = r.text.replace(find, repl);
                            hit = true;
                        }
                    }
                    // Fundstelle über Laufgrenzen hinweg
                    if !hit && p.text().contains(find) {
                        let t = p.text();
                        n += t.matches(find).count();
                        p.set_text(&t.replace(find, repl));
                    }
                }
                Block::Table(t) => {
                    for c in t.iter_mut().flatten() {
                        let k = c.matches(find).count();
                        if k > 0 {
                            n += k;
                            *c = c.replace(find, repl);
                        }
                    }
                }
            }
        }
        n
    }
}

/// **fett**, *kursiv*, ***beides***
fn inline(s: &str) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        let (mark, b, i) = if rest.starts_with("***") {
            ("***", true, true)
        } else if rest.starts_with("**") {
            ("**", true, false)
        } else if rest.starts_with('*') || rest.starts_with('_') {
            (&rest[..1], false, true)
        } else {
            ("", false, false)
        };
        if !mark.is_empty() {
            if let Some(end) = rest[mark.len()..].find(mark) {
                let inner = &rest[mark.len()..mark.len() + end];
                if !inner.is_empty() && !inner.starts_with(' ') {
                    out.push(Run {
                        text: inner.into(),
                        bold: b,
                        italic: i,
                        underline: false,
                    });
                    rest = &rest[mark.len() * 2 + end..];
                    continue;
                }
            }
        }
        // bis zum nächsten möglichen Zeichen kopieren
        let next = rest
            .char_indices()
            .skip(1)
            .find(|(_, c)| *c == '*' || *c == '_')
            .map(|(k, _)| k)
            .unwrap_or(rest.len());
        match out.last_mut() {
            Some(r) if !r.bold && !r.italic => r.text.push_str(&rest[..next]),
            _ => out.push(Run::plain(&rest[..next])),
        }
        rest = &rest[next..];
    }
    if out.is_empty() {
        out.push(Run::default());
    }
    out
}

#[cfg(test)]
mod tests {
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
}
