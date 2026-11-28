//! Laying out one paragraph: the text is cut into tokens (words, spaces, tabs) of one run
//! and one bidi level, the tokens are shaped by the text engine, filled into lines, ordered
//! visually per line (Arabic words run right to left inside a left-to-right line and the
//! other way round) and aligned. The result does not depend on where the paragraph stands on
//! the page, so it is cached.

use super::{LineBox, ParaLayout, Piece, PieceStyle};
use std::rc::Rc;
use ultar_i18n::Direction;
use ultar_office::doc::{Align, Para, Run, Style};
use ultar_ui::text::{FontRole, Overflow, TextEngine, TextLayout, TextStyle, Weight};
use unicode_bidi::{BidiInfo, Level};

/// Points of a tab stop.
const TAB_PT: f32 = 36.0;
/// Indent of list paragraphs without an own indent, points.
const LIST_INDENT_PT: f32 = 28.0;
/// Space between a list marker and the text, points.
const MARKER_GAP_PT: f32 = 6.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Space,
    Tab,
    Newline,
}

struct Tok {
    s: usize,
    e: usize,
    kind: Kind,
    run: usize,
    level: Level,
    text: String,
    w: f32,
    layout: Option<Rc<TextLayout>>,
}

/// Points before and after a paragraph of `style`.
fn spacing_pt(style: Style) -> (f32, f32) {
    match style {
        Style::Title => (0.0, 12.0),
        Style::H1 => (14.0, 6.0),
        Style::H2 => (10.0, 4.0),
        Style::H3 => (8.0, 3.0),
        Style::Bullet | Style::Number => (0.0, 3.0),
        Style::Quote => (4.0, 8.0),
        Style::Code => (0.0, 0.0),
        Style::Normal => (0.0, 8.0),
    }
}

/// The look of a run in a paragraph of `style`, sizes in pixels (`k` pixels per point).
pub fn piece_style(run: &Run, style: Style, k: f32) -> PieceStyle {
    let quote = style == Style::Quote;
    PieceStyle {
        size: run.size_pt.unwrap_or_else(|| style.size()) * k,
        bold: run.bold || style.bold(),
        italic: run.italic || quote,
        underline: run.underline,
        strike: run.strike,
        color: run
            .color
            .as_deref()
            .and_then(hex)
            .or(quote.then_some([0x55; 3])),
        highlight: run.highlight.as_deref().and_then(hex),
        mono: style == Style::Code,
        title: matches!(style, Style::Title | Style::H1 | Style::H2 | Style::H3),
    }
}

/// `RRGGBB` to bytes.
pub fn hex(s: &str) -> Option<[u8; 3]> {
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// The engine style of a piece of text.
pub fn text_style(ps: &PieceStyle, rtl: bool, arabic: bool) -> TextStyle {
    TextStyle {
        size: ps.size.max(1.0),
        weight: if ps.bold {
            Weight::Bold
        } else {
            Weight::Regular
        },
        role: if ps.mono {
            FontRole::Mono
        } else if ps.title {
            FontRole::Title
        } else {
            FontRole::Text
        },
        direction: if rtl { Direction::Rtl } else { Direction::Ltr },
        overflow: Overflow::Clip,
        line_height: if arabic { 1.55 } else { 1.3 },
        arabic_metrics: arabic,
    }
}

/// `true` for letters of the Arabic blocks.
pub fn is_arabic(c: char) -> bool {
    matches!(u32::from(c), 0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
}

fn kind_of(c: char) -> Kind {
    match c {
        '\n' => Kind::Newline,
        '\t' => Kind::Tab,
        c if c.is_whitespace() => Kind::Space,
        _ => Kind::Word,
    }
}

/// The marker (bullet or number) of a list paragraph shaped like its first run.
fn marker_piece(eng: &mut TextEngine, p: &Para, label: &str, rtl: bool, k: f32) -> Piece {
    let run = p.runs.first().cloned().unwrap_or_default();
    let ps = piece_style(&Run { bold: false, ..run }, p.style, k);
    let layout = eng.layout(label, &text_style(&ps, rtl, false), None);
    Piece {
        start: 0,
        end: 0,
        x: 0.0,
        w: layout.width,
        rtl,
        space: false,
        style: ps,
        text: label.to_owned(),
        layout: Some(layout),
    }
}

/// Splits a word that is wider than a whole line into pieces that fit: `(start, end, text)`.
fn split_wide(
    eng: &mut TextEngine,
    t: &Tok,
    avail: f32,
    ps: &PieceStyle,
) -> Vec<(usize, usize, String)> {
    let chars: Vec<char> = t.text.chars().collect();
    let rtl = t.level.is_rtl();
    let mut out = Vec::new();
    let mut from = 0;
    while from < chars.len() {
        let mut to = from + 1;
        while to < chars.len() {
            let probe: String = chars[from..=to].iter().collect();
            let arabic = probe.chars().any(is_arabic);
            if eng.layout(&probe, &text_style(ps, rtl, arabic), None).width > avail {
                break;
            }
            to += 1;
        }
        out.push((t.s + from, t.s + to, chars[from..to].iter().collect()));
        from = to;
    }
    out
}

/// Cuts the text of `p` into words, spaces, tabs and line feeds of one run and one bidi level.
fn split_tokens(p: &Para) -> Vec<Tok> {
    let text = p.text();
    let bidi = BidiInfo::new(
        &text,
        Some(if p.rtl() { Level::rtl() } else { Level::ltr() }),
    );
    let mut run_of = Vec::new();
    for (ri, r) in p.runs.iter().enumerate() {
        run_of.extend(std::iter::repeat_n(ri, r.text.chars().count()));
    }
    let mut toks: Vec<Tok> = Vec::new();
    for (i, (byte, c)) in text.char_indices().enumerate() {
        let (kind, run) = (kind_of(c), run_of.get(i).copied().unwrap_or(0));
        let level = bidi.levels.get(byte).copied().unwrap_or_else(Level::ltr);
        match toks.last_mut() {
            Some(t)
                if t.kind == kind
                    && t.run == run
                    && t.level == level
                    && matches!(kind, Kind::Word | Kind::Space) =>
            {
                t.e = i + 1;
                t.text.push(c);
            }
            _ => toks.push(Tok {
                s: i,
                e: i + 1,
                kind,
                run,
                level,
                text: c.to_string(),
                w: 0.0,
                layout: None,
            }),
        }
    }
    toks
}

fn tokens(eng: &mut TextEngine, p: &Para, k: f32, avail: f32) -> Vec<Tok> {
    let toks = split_tokens(p);
    let mut out = Vec::with_capacity(toks.len());
    for mut t in toks {
        let run = p.runs.get(t.run).cloned().unwrap_or_default();
        let ps = piece_style(&run, p.style, k);
        match t.kind {
            Kind::Newline => {}
            Kind::Tab => t.w = TAB_PT * k,
            _ => {
                let arabic = t.text.chars().any(is_arabic);
                let l = eng.layout(&t.text, &text_style(&ps, t.level.is_rtl(), arabic), None);
                if t.kind == Kind::Word && l.width > avail && avail > 8.0 {
                    for (s, e, text) in split_wide(eng, &t, avail, &ps) {
                        let arabic = text.chars().any(is_arabic);
                        let l = eng.layout(&text, &text_style(&ps, t.level.is_rtl(), arabic), None);
                        let w = l.width;
                        out.push(Tok {
                            s,
                            e,
                            kind: Kind::Word,
                            run: t.run,
                            level: t.level,
                            text,
                            w,
                            layout: Some(l),
                        });
                    }
                    continue;
                }
                t.w = l.width;
                t.layout = Some(l);
            }
        }
        out.push(t);
    }
    out
}

/// Cuts the tokens into lines: indexes of the tokens of each line and whether the line ends
/// with a line feed.
fn fill(toks: &[Tok], avail: f32) -> Vec<(Vec<usize>, bool)> {
    let mut lines: Vec<(Vec<usize>, bool)> = Vec::new();
    let (mut cur, mut w) = (Vec::new(), 0.0_f32);
    for (i, t) in toks.iter().enumerate() {
        if t.kind == Kind::Newline {
            lines.push((std::mem::take(&mut cur), true));
            w = 0.0;
            continue;
        }
        if t.kind == Kind::Word && !cur.is_empty() && w + t.w > avail + 0.5 {
            lines.push((std::mem::take(&mut cur), false));
            w = 0.0;
        }
        cur.push(i);
        w += t.w;
    }
    lines.push((cur, false));
    lines
}

/// Where one paragraph is laid out: column geometry shared by all its lines.
struct Frame {
    rtl: bool,
    /// Left and right edge of the text box (the indent is on the start side).
    bx0: f32,
    bx1: f32,
    k: f32,
}

/// The visual order of the tokens `idx` of a line; the trailing spaces take the paragraph's
/// direction so that they do not jump into the text.
fn visual_order(toks: &[Tok], idx: &[usize], trailing: usize, rtl: bool) -> Vec<usize> {
    let base = if rtl { Level::rtl() } else { Level::ltr() };
    let mut levels: Vec<Level> = idx.iter().map(|&i| toks[i].level).collect();
    let n = levels.len();
    for l in &mut levels[n - trailing..] {
        *l = base;
    }
    if levels.is_empty() {
        Vec::new()
    } else {
        BidiInfo::reorder_visual(&levels)
    }
}

/// The piece of a token placed at `x` with the width `w`.
fn piece_of(t: &Tok, p: &Para, k: f32, (x, w): (f32, f32)) -> Piece {
    let run = p.runs.get(t.run).cloned().unwrap_or_default();
    Piece {
        start: t.s,
        end: t.e,
        x,
        w,
        rtl: t.level.is_rtl(),
        space: t.kind != Kind::Word,
        style: piece_style(&run, p.style, k),
        text: t.text.clone(),
        layout: t.layout.clone(),
    }
}

/// Places the tokens `idx` of one line; returns the pieces, the height and the baseline.
fn place_line(
    toks: &[Tok],
    idx: &[usize],
    p: &Para,
    f: &Frame,
    last: bool,
    hard: bool,
) -> (Vec<Piece>, f32, f32) {
    let avail = f.bx1 - f.bx0;
    let n = idx.len();
    let trailing = idx
        .iter()
        .rev()
        .take_while(|&&i| toks[i].kind != Kind::Word)
        .count();
    let order = visual_order(toks, idx, trailing, f.rtl);
    let content_w: f32 = idx[..n - trailing].iter().map(|&i| toks[i].w).sum();
    let gaps = idx[..n - trailing]
        .iter()
        .filter(|&&i| toks[i].kind != Kind::Word)
        .count();
    let free = (avail - content_w).max(0.0);
    let justify = p.align == Align::Justify && !last && !hard && gaps > 0;
    let extra = if justify { free / gaps as f32 } else { 0.0 };
    let shown = content_w + extra * gaps as f32;
    let start_x = match (p.align, f.rtl) {
        (Align::Center, _) => f.bx0 + (avail - shown) / 2.0,
        (Align::End, false) | (Align::Start | Align::Justify, true) => f.bx1 - shown,
        _ => f.bx0,
    };
    let tail_w: f32 = idx[n - trailing..].iter().map(|&i| toks[i].w).sum();
    let mut x = if f.rtl {
        start_x - tail_w // physical
    } else {
        start_x // physical
    };
    let (mut h, mut asc) = (0.0_f32, 0.0_f32);
    let mut pieces = Vec::with_capacity(n);
    for &vi in &order {
        let t = &toks[idx[vi]];
        let stretch = justify && vi < n - trailing && t.kind != Kind::Word;
        let w = t.w + if stretch { extra } else { 0.0 };
        if let Some(l) = &t.layout {
            h = h.max(l.height);
            asc = asc.max(l.lines.first().map_or(0.0, |ln| ln.baseline));
        }
        pieces.push(piece_of(t, p, f.k, (x, w)));
        x += w;
    }
    (pieces, h, asc)
}

/// The list marker, outside the text box on the start side.
fn placed_marker(eng: &mut TextEngine, p: &Para, label: &str, f: &Frame) -> Piece {
    let mut m = marker_piece(eng, p, label, f.rtl, f.k);
    let gap = MARKER_GAP_PT * f.k;
    m.x = if f.rtl {
        f.bx1 + gap
    } else {
        f.bx0 - gap - m.w
    };
    m
}

/// Height and baseline of an empty line (the height of an "A" in the style of the paragraph).
fn blank_line(eng: &mut TextEngine, blank: &PieceStyle, rtl: bool) -> (f32, f32) {
    let l = eng.layout("A", &text_style(blank, rtl, false), None);
    (l.height, l.lines.first().map_or(0.0, |ln| ln.baseline))
}

/// The text box of a paragraph in a column of `width` (the indent is on the start side) and
/// the width left for the text.
fn frame_of(p: &Para, list: bool, width: f32, k: f32) -> (Frame, f32) {
    let rtl = p.rtl();
    let indent_pt = p
        .indent_pt
        .unwrap_or(0.0)
        .max(if list { LIST_INDENT_PT } else { 0.0 });
    let indent = (indent_pt * k).min(width * 0.6);
    let (bx0, bx1) = if rtl {
        (0.0, width - indent)
    } else {
        (indent, width)
    };
    (Frame { rtl, bx0, bx1, k }, (width - indent).max(16.0))
}

/// Lays out `p`. `width` is the width of the column in pixels, `k` the pixels per point,
/// `label` the list marker.
pub fn layout_para(
    eng: &mut TextEngine,
    p: &Para,
    label: Option<&str>,
    width: f32,
    k: f32,
) -> ParaLayout {
    let (frame, avail) = frame_of(p, label.is_some(), width, k);
    let (rtl, bx0, bx1) = (frame.rtl, frame.bx0, frame.bx1);
    let toks = tokens(eng, p, k, avail);
    let lines = fill(&toks, avail);
    let first_run = p.runs.first().cloned().unwrap_or_default();
    let blank = piece_style(&first_run, p.style, k);
    let spacing = p.line_spacing.unwrap_or(1.0).clamp(0.5, 4.0);
    let count = lines.len();
    let mut out: Vec<LineBox> = Vec::with_capacity(count);
    let (mut y, mut next_start) = (0.0, 0);
    for (li, (idx, hard)) in lines.into_iter().enumerate() {
        let (pieces, mut h, mut asc) = place_line(&toks, &idx, p, &frame, li + 1 == count, hard);
        if h <= 0.0 {
            (h, asc) = blank_line(eng, &blank, rtl);
        }
        let start = idx.first().map_or(next_start, |&i| toks[i].s);
        let end = idx.last().map_or(next_start, |&i| toks[i].e);
        next_start = end + usize::from(hard);
        let line_h = h * spacing;
        let start_x = if rtl { bx1 } else { bx0 };
        out.push(LineBox {
            start,
            end,
            y,
            h: line_h,
            base: asc,
            pieces,
            marker: None,
            rtl,
            hard_end: hard,
            start_x,
        });
        y += line_h;
    }
    if let (Some(label), Some(first)) = (label, out.first_mut()) {
        first.marker = Some(placed_marker(eng, p, label, &frame));
    }
    let (before, after) = spacing_pt(p.style);
    ParaLayout {
        lines: out,
        h: y,
        before: before * k,
        after: after * k,
        style: p.style,
    }
}
