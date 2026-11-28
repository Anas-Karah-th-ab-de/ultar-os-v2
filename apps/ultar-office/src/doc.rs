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

/// Written direction of a paragraph: by the text, or forced by the person (like Word's
/// RTL/LTR button). The letter of each run still follows the first strong character
/// ([`is_rtl`]); this controls the paragraph as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Dir {
    /// The first strong character decides.
    #[default]
    Auto,
    /// Force left-to-right.
    Ltr,
    /// Force right-to-left.
    Rtl,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Through the middle of the letters.
    pub strike: bool,
    /// Size in point; `None` = the size of the style.
    pub size_pt: Option<f32>,
    /// Text colour as six hex digits (`RRGGBB`, without `#`); `None` = the theme colour.
    pub color: Option<String>,
    /// Background of the letters (a highlighter) as six hex digits; `None` = none.
    pub highlight: Option<String>,
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
    /// Written direction ([`Dir`]).
    pub dir: Dir,
    /// Height of one line as a multiple of single (1.0, 1.5, 2.0 …); `None` = single.
    pub line_spacing: Option<f32>,
    /// First-line indentation of the start side, in point; `None` = none.
    pub indent_pt: Option<f32>,
    /// A page break stands before this paragraph (`w:br w:type="page"`).
    pub page_break: bool,
    pub runs: Vec<Run>,
}

impl Para {
    pub fn new(style: Style, text: &str) -> Self {
        Para {
            style,
            align: Align::Start,
            dir: Dir::Auto,
            line_spacing: None,
            indent_pt: None,
            page_break: false,
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
    /// Arabische/hebräische Schrift → rechts nach links; an explicit direction wins.
    pub fn rtl(&self) -> bool {
        match self.dir {
            Dir::Auto => is_rtl(&self.text()),
            Dir::Ltr => false,
            Dir::Rtl => true,
        }
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
pub struct Picture {
    /// The image bytes (PNG or JPEG, kept verbatim for the round trip).
    pub bytes: Vec<u8>,
    /// File name inside the package (kept, so saving writes the same name).
    pub name: String,
    /// Displayed width in point (`None` = natural size of the image).
    pub width_pt: Option<f32>,
    /// Displayed height in point (`None` = natural size).
    pub height_pt: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Para(Para),
    Table(Vec<Vec<String>>),
    Picture(Picture),
}

/// The paper of a document: size and margins in point (A4 and 2.5 cm by default).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Page {
    pub width_pt: f32,
    pub height_pt: f32,
    /// Top, end-of-line side (right), bottom, start side (left) as stored by Word.
    pub margin_top: f32,
    pub margin_right: f32, // physical
    pub margin_bottom: f32,
    pub margin_left: f32, // physical
}

impl Page {
    /// A4: 595.3 x 841.9 pt.
    pub const A4: Page = Page {
        width_pt: 595.3,
        height_pt: 841.9,
        margin_top: 70.9,
        margin_right: 70.9, // physical
        margin_bottom: 56.7,
        margin_left: 70.9, // physical
    };
    /// US Letter: 612 x 792 pt, one inch margins.
    pub const LETTER: Page = Page {
        width_pt: 612.0,
        height_pt: 792.0,
        margin_top: 72.0,
        margin_right: 72.0, // physical
        margin_bottom: 72.0,
        margin_left: 72.0, // physical
    };
    /// Width of the text column.
    pub fn text_width(&self) -> f32 {
        (self.width_pt - self.margin_left - self.margin_right).max(72.0) // physical
    }
    /// Height of the text area.
    pub fn text_height(&self) -> f32 {
        (self.height_pt - self.margin_top - self.margin_bottom).max(72.0)
    }
    /// Sets the margins that are given, in the order of Word's `w:pgMar`: top, end, bottom,
    /// start (physical right and left in the page's coordinates).
    pub fn set_margins(&mut self, m: [Option<f32>; 4]) {
        self.margin_top = m[0].unwrap_or(self.margin_top);
        self.margin_right = m[1].unwrap_or(self.margin_right); // physical
        self.margin_bottom = m[2].unwrap_or(self.margin_bottom);
        self.margin_left = m[3].unwrap_or(self.margin_left); // physical
    }
    /// `true` for a Letter-sized page (within a point).
    pub fn is_letter(&self) -> bool {
        (self.width_pt - 612.0).abs() < 1.0 && (self.height_pt - 792.0).abs() < 1.0
    }
}

impl Default for Page {
    fn default() -> Page {
        Page::A4
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
    /// Paper size and margins (`w:sectPr`).
    pub page: Page,
}

mod edit;
pub use edit::{same_format, Hit, MAX_LEVEL};

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
                Block::Picture(_) => {}
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
                Block::Picture(_) => {}
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
                    dir: Dir::Auto,
                    line_spacing: None,
                    indent_pt: None,
                    page_break: false,
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
                        dir: Dir::Auto,
                        line_spacing: None,
                        indent_pt: None,
                        page_break: false,
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
            page: Page::default(),
        }
    }

    /// Suchen und Ersetzen in allen Absätzen und Tabellen; liefert Anzahl
    pub fn replace(&mut self, find: &str, repl: &str) -> usize {
        self.replace_all(find, repl, true)
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
                        strike: false,
                        size_pt: None,
                        color: None,
                        highlight: None,
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
mod tests;
