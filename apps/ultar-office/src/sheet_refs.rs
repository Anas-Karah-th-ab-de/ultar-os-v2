//! References inside formulas: how they move when a formula is copied, when rows or columns
//! are inserted or deleted, and when a sheet is renamed or deleted.

use crate::sheet::col_name;
use regex::Regex;
use std::sync::OnceLock;

/// Most rows of a sheet (like Excel).
pub const MAX_ROWS: u32 = 1_048_576;
/// Most columns of a sheet (like Excel: XFD).
pub const MAX_COLS: u32 = 16_384;

/// Rows or columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Rows.
    Rows,
    /// Columns.
    Cols,
}

/// One corner of a reference: `$B$7`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Corner {
    col_abs: bool,
    col: u32,
    row_abs: bool,
    row: u32,
}

impl Corner {
    fn index(&self, axis: Axis) -> u32 {
        match axis {
            Axis::Rows => self.row,
            Axis::Cols => self.col,
        }
    }

    fn with(mut self, axis: Axis, i: u32) -> Corner {
        match axis {
            Axis::Rows => self.row = i,
            Axis::Cols => self.col = i,
        }
        self
    }

    fn text(&self) -> String {
        format!(
            "{}{}{}{}",
            if self.col_abs { "$" } else { "" },
            col_name(self.col),
            if self.row_abs { "$" } else { "" },
            self.row + 1
        )
    }
}

/// A reference found in a formula.
pub(crate) struct RefTok {
    /// The sheet prefix as written (with `!`), empty when there is none.
    pre: String,
    /// The sheet's name without quotes.
    pub(crate) sheet: Option<String>,
    a: Corner,
    b: Option<Corner>,
}

fn ref_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?P<pre>(?:'[^']+'|[A-Za-z_][A-Za-z0-9_.]*)!)?(?P<c1d>\$?)(?P<c1>[A-Za-z]{1,3})(?P<r1d>\$?)(?P<r1>[0-9]{1,7})(?::(?P<c2d>\$?)(?P<c2>[A-Za-z]{1,3})(?P<r2d>\$?)(?P<r2>[0-9]{1,7}))?",
        )
        .ok()
    })
    .as_ref()
}

fn col_index(letters: &str) -> Option<u32> {
    let mut n: u32 = 0;
    for b in letters.bytes() {
        n = n * 26 + u32::from(b.to_ascii_uppercase() - b'A') + 1;
    }
    (1..=MAX_COLS).contains(&n).then(|| n - 1)
}

fn corner(d: &str, c: &str, rd: &str, r: &str) -> Option<Corner> {
    let row: u32 = r.parse().ok()?;
    if row == 0 || row > MAX_ROWS {
        return None;
    }
    Some(Corner {
        col_abs: d == "$",
        col: col_index(c)?,
        row_abs: rd == "$",
        row: row - 1,
    })
}

fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.' || c == '$'
}

/// Calls `f` for every reference outside of string literals and replaces it by what `f`
/// returns (`None` keeps it).
pub(crate) fn map_refs(formula: &str, f: &mut dyn FnMut(&RefTok) -> Option<String>) -> String {
    let Some(re) = ref_re() else {
        return formula.to_owned();
    };
    let mut out = String::with_capacity(formula.len() + 8);
    for (i, part) in formula.split('"').enumerate() {
        if i > 0 {
            out.push('"');
        }
        if i % 2 == 1 {
            out.push_str(part);
            continue;
        }
        let mut last = 0;
        for caps in re.captures_iter(part) {
            let Some(all) = caps.get(0) else { continue };
            let before = part[..all.start()].chars().next_back();
            let after = part[all.end()..].chars().next();
            if before.is_some_and(word) || after.is_some_and(|c| word(c) || c == '(' || c == '!') {
                continue;
            }
            let g = |n: &str| caps.name(n).map_or("", |m| m.as_str());
            let Some(a) = corner(g("c1d"), g("c1"), g("r1d"), g("r1")) else {
                continue;
            };
            let b = if caps.name("c2").is_some() {
                let Some(b) = corner(g("c2d"), g("c2"), g("r2d"), g("r2")) else {
                    continue;
                };
                Some(b)
            } else {
                None
            };
            let pre = g("pre").to_owned();
            let sheet =
                (!pre.is_empty()).then(|| pre.trim_end_matches('!').trim_matches('\'').to_owned());
            let tok = RefTok { pre, sheet, a, b };
            if let Some(text) = f(&tok) {
                out.push_str(&part[last..all.start()]);
                out.push_str(&text);
                last = all.end();
            }
        }
        out.push_str(&part[last..]);
    }
    out
}

fn render(tok: &RefTok, a: Corner, b: Option<Corner>) -> String {
    match b {
        Some(b) => format!("{}{}:{}", tok.pre, a.text(), b.text()),
        None => format!("{}{}", tok.pre, a.text()),
    }
}

fn error_ref(tok: &RefTok) -> String {
    format!("{}#REF!", tok.pre)
}

/// `formula` (without `=`) as if copied `dr` rows down and `dc` columns to the end: relative
/// parts move, `$` parts stay; a reference that leaves the sheet becomes `#REF!`.
pub fn move_refs(formula: &str, dr: i64, dc: i64) -> String {
    let shift = |c: Corner| -> Option<Corner> {
        let row = if c.row_abs {
            i64::from(c.row)
        } else {
            i64::from(c.row) + dr
        };
        let col = if c.col_abs {
            i64::from(c.col)
        } else {
            i64::from(c.col) + dc
        };
        let ok = (0..i64::from(MAX_ROWS)).contains(&row) && (0..i64::from(MAX_COLS)).contains(&col);
        ok.then_some(Corner {
            row: row as u32,
            col: col as u32,
            ..c
        })
    };
    map_refs(formula, &mut |t| {
        let a = shift(t.a);
        let b = t.b.map(shift);
        Some(match (a, b) {
            (Some(a), None) => render(t, a, None),
            (Some(a), Some(Some(b))) => render(t, a, Some(b)),
            _ => error_ref(t),
        })
    })
}

/// The raw content of a cell as if copied `dr` rows and `dc` columns away (formulas move
/// their relative references, anything else stays).
pub fn relocate(raw: &str, dr: i64, dc: i64) -> String {
    match raw.strip_prefix('=') {
        Some(f) if dr != 0 || dc != 0 => format!("={}", move_refs(f, dr, dc)),
        _ => raw.to_owned(),
    }
}

fn same_sheet(tok: &RefTok, home: &str, target: &str) -> bool {
    tok.sheet
        .as_deref()
        .unwrap_or(home)
        .eq_ignore_ascii_case(target)
}

/// `formula` of the sheet `home` after `n` rows or columns were inserted at `at` in `target`.
pub fn insert_refs(formula: &str, home: &str, target: &str, axis: Axis, at: u32, n: u32) -> String {
    let max = match axis {
        Axis::Rows => MAX_ROWS,
        Axis::Cols => MAX_COLS,
    };
    let up = |c: Corner| {
        let i = c.index(axis);
        c.with(axis, if i >= at { (i + n).min(max - 1) } else { i })
    };
    map_refs(formula, &mut |t| {
        if !same_sheet(t, home, target) {
            return None;
        }
        Some(render(t, up(t.a), t.b.map(up)))
    })
}

/// `formula` of the sheet `home` after `n` rows or columns were deleted at `at` in `target`.
pub fn delete_refs(formula: &str, home: &str, target: &str, axis: Axis, at: u32, n: u32) -> String {
    let (at, n) = (i64::from(at), i64::from(n));
    map_refs(formula, &mut |t| {
        if !same_sheet(t, home, target) {
            return None;
        }
        let Some(b) = t.b else {
            let i = i64::from(t.a.index(axis));
            return Some(if i >= at && i < at + n {
                error_ref(t)
            } else {
                let j = if i >= at + n { i - n } else { i };
                render(t, t.a.with(axis, j as u32), None)
            });
        };
        let (mut s, mut e) = (i64::from(t.a.index(axis)), i64::from(b.index(axis)));
        let flip = s > e;
        if flip {
            std::mem::swap(&mut s, &mut e);
        }
        let ns = if s < at {
            s
        } else if s < at + n {
            at
        } else {
            s - n
        };
        let ne = if e < at {
            e
        } else if e < at + n {
            at - 1
        } else {
            e - n
        };
        if ne < ns {
            return Some(error_ref(t));
        }
        let (x, y) = if flip { (ne, ns) } else { (ns, ne) };
        Some(render(
            t,
            t.a.with(axis, x as u32),
            Some(b.with(axis, y as u32)),
        ))
    })
}

fn quote_name(name: &str) -> String {
    let plain = !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    if plain {
        format!("{name}!")
    } else {
        format!("'{name}'!")
    }
}

/// `formula` after the sheet `old` was renamed to `new`.
pub fn rename_refs(formula: &str, old: &str, new: &str) -> String {
    let pre = quote_name(new);
    map_refs(formula, &mut |t| {
        let hit = t
            .sheet
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case(old));
        hit.then(|| {
            let (a, b) = (t.a, t.b);
            let tok = RefTok {
                pre: pre.clone(),
                sheet: None,
                a,
                b,
            };
            render(&tok, a, b)
        })
    })
}
