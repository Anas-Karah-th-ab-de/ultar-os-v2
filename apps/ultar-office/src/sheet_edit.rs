//! Editing operations of the spreadsheet app on the model: insert and delete rows and
//! columns, rename and delete sheets, fill down and right, sort, and tab-separated text for
//! the clipboard. (References inside formulas: [`crate::sheet_refs`]; find and replace:
//! [`crate::sheet_find`].)

use crate::sheet::{Sheet, Value, Workbook};
use crate::sheet_refs::{
    delete_refs, insert_refs, map_refs, relocate, rename_refs, Axis, MAX_COLS, MAX_ROWS,
};
use crate::style::CellStyle;
use std::cmp::Ordering;

/// Why a sheet could not be renamed, added or deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    /// Empty, longer than 31 characters or with a character Excel does not allow.
    Bad,
    /// Another sheet has this name.
    Taken,
    /// The last sheet cannot be deleted.
    Last,
}

/// `true` for a name a sheet may have (Excel's rules, and no apostrophe).
pub fn valid_sheet_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty()
        && n.chars().count() <= 31
        && !n.contains(['[', ']', ':', '*', '?', '/', '\\', '\''])
        && !n.chars().any(char::is_control)
}

fn remap(sh: &mut Sheet, axis: Axis, f: &dyn Fn(u32) -> Option<u32>) {
    let at = |k: (u32, u32)| match axis {
        Axis::Rows => (k.0, k.1),
        Axis::Cols => (k.1, k.0),
    };
    let put = |along: u32, across: u32| match axis {
        Axis::Rows => (along, across),
        Axis::Cols => (across, along),
    };
    let cells = std::mem::take(&mut sh.cells);
    for (k, v) in cells {
        let (along, across) = at(k);
        if let Some(n) = f(along) {
            sh.cells.insert(put(n, across), v);
        }
    }
    let styles = std::mem::take(&mut sh.styles);
    for (k, v) in styles {
        let (along, across) = at(k);
        if let Some(n) = f(along) {
            sh.styles.insert(put(n, across), v);
        }
    }
    let dims = match axis {
        Axis::Rows => &mut sh.heights,
        Axis::Cols => &mut sh.widths,
    };
    let old = std::mem::take(dims);
    for (i, v) in old {
        if let Some(n) = f(i) {
            dims.insert(n, v);
        }
    }
}

impl Workbook {
    fn rewrite_formulas(&mut self, f: &dyn Fn(&str, &str) -> String) {
        let names: Vec<String> = self.sheets.iter().map(|s| s.name.clone()).collect();
        for (sh, home) in self.sheets.iter_mut().zip(&names) {
            for raw in sh.cells.values_mut() {
                if let Some(formula) = raw.strip_prefix('=') {
                    let new = f(formula, home);
                    if new != formula {
                        *raw = format!("={new}");
                    }
                }
            }
        }
    }

    /// Inserts `n` empty rows or columns before index `at` of `sheet`; formulas everywhere
    /// follow. `false` when the sheet would grow beyond its limit.
    pub fn insert_lines(&mut self, sheet: usize, axis: Axis, at: u32, n: u32) -> bool {
        let Some(sh) = self.sheets.get(sheet) else {
            return false;
        };
        let (rows, cols) = sh.extent();
        let (used, max) = match axis {
            Axis::Rows => (rows, MAX_ROWS),
            Axis::Cols => (cols, MAX_COLS),
        };
        if n == 0 || used.max(at) + n > max {
            return false;
        }
        let target = sh.name.clone();
        remap(&mut self.sheets[sheet], axis, &|i| {
            Some(if i >= at { i + n } else { i })
        });
        self.rewrite_formulas(&|f, home| insert_refs(f, home, &target, axis, at, n));
        true
    }

    /// Deletes `n` rows or columns from index `at` of `sheet`; formulas that read them show
    /// `#REF!`.
    pub fn delete_lines(&mut self, sheet: usize, axis: Axis, at: u32, n: u32) -> bool {
        let Some(sh) = self.sheets.get(sheet) else {
            return false;
        };
        if n == 0 {
            return false;
        }
        let target = sh.name.clone();
        remap(&mut self.sheets[sheet], axis, &|i| {
            if i < at {
                Some(i)
            } else if i < at + n {
                None
            } else {
                Some(i - n)
            }
        });
        self.rewrite_formulas(&|f, home| delete_refs(f, home, &target, axis, at, n));
        true
    }

    /// Renames a sheet; formulas that name it follow.
    pub fn rename_sheet(&mut self, sheet: usize, new: &str) -> Result<(), NameError> {
        let new = new.trim();
        if !valid_sheet_name(new) {
            return Err(NameError::Bad);
        }
        let taken = self
            .sheets
            .iter()
            .enumerate()
            .any(|(i, s)| i != sheet && s.name.eq_ignore_ascii_case(new));
        if taken {
            return Err(NameError::Taken);
        }
        let Some(old) = self.sheets.get(sheet).map(|s| s.name.clone()) else {
            return Err(NameError::Bad);
        };
        self.rewrite_formulas(&|f, _| rename_refs(f, &old, new));
        self.sheets[sheet].name = new.to_owned();
        Ok(())
    }

    /// Deletes a sheet; formulas that read it show `#REF!`.
    pub fn delete_sheet(&mut self, sheet: usize) -> Result<(), NameError> {
        if self.sheets.len() <= 1 || sheet >= self.sheets.len() {
            return Err(NameError::Last);
        }
        let old = self.sheets[sheet].name.clone();
        self.sheets.remove(sheet);
        self.rewrite_formulas(&|f, _| {
            map_refs(f, &mut |t| {
                let hit = t
                    .sheet
                    .as_deref()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&old));
                hit.then(|| "#REF!".to_owned())
            })
        });
        Ok(())
    }

    /// A name for a new sheet: `Sheet2`, `Sheet3` …
    pub fn new_sheet_name(&self, base: &str) -> String {
        (self.sheets.len() + 1..)
            .map(|n| format!("{base}{n}"))
            .find(|n| self.sheet_index(n).is_none())
            .unwrap_or_else(|| base.to_owned())
    }

    /// Copies the first row of the rectangle down over the others (columns keep their own
    /// content; relative references move, styles are copied).
    pub fn fill_down(&mut self, sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) {
        self.fill(sheet, (r0, c0, r1, c1), true);
    }

    /// Copies the first column of the rectangle to the right over the others.
    pub fn fill_across(&mut self, sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) {
        self.fill(sheet, (r0, c0, r1, c1), false);
    }

    fn fill(&mut self, sheet: usize, (r0, c0, r1, c1): (u32, u32, u32, u32), down: bool) {
        let Some(sh) = self.sheets.get_mut(sheet) else {
            return;
        };
        let lines = if down { c0..=c1 } else { r0..=r1 };
        for line in lines {
            let (sr, sc) = if down { (r0, line) } else { (line, c0) };
            let raw = sh.get(sr, sc).to_owned();
            let style = sh.style(sr, sc);
            let steps = if down { r1 - r0 } else { c1 - c0 };
            for k in 1..=steps {
                let (r, c) = if down { (r0 + k, line) } else { (line, c0 + k) };
                let (dr, dc) = if down {
                    (i64::from(k), 0)
                } else {
                    (0, i64::from(k))
                };
                sh.set(r, c, &relocate(&raw, dr, dc));
                sh.set_style(r, c, style.clone());
            }
        }
    }

    /// Sorts the rows `r0..=r1` of the columns `c0..=c1` by column `key`. Numbers come before
    /// text, then truth values, errors; empty cells stay last in either direction. Formulas
    /// move their relative references with their row. `text` compares two texts (the app
    /// passes the locale's collation).
    pub fn sort_rows(
        &mut self,
        sheet: usize,
        rect: (u32, u32, u32, u32),
        key: u32,
        ascending: bool,
        text: &dyn Fn(&str, &str) -> Ordering,
    ) -> Result<(), String> {
        let (r0, c0, r1, c1) = rect;
        if !(c0..=c1).contains(&key) {
            return Err("sort key outside of the range".into());
        }
        if r1 <= r0 || sheet >= self.sheets.len() {
            return Ok(());
        }
        type Line = (u32, Value, Vec<(u32, String, CellStyle)>);
        let mut rows: Vec<Line> = Vec::new();
        for r in r0..=r1 {
            let k = self.value(sheet, r, key);
            let sh = &self.sheets[sheet];
            let cells = sh
                .cells
                .range((r, c0)..=(r, c1))
                .map(|(&(_, c), raw)| (c, raw.clone(), sh.style(r, c)))
                .collect();
            rows.push((r, k, cells));
        }
        rows.sort_by(|a, b| compare_values(&a.1, &b.1, ascending, text));
        let sh = &mut self.sheets[sheet];
        for r in r0..=r1 {
            for c in c0..=c1 {
                sh.cells.remove(&(r, c));
                sh.styles.remove(&(r, c));
            }
        }
        for (i, (from, _, cells)) in rows.into_iter().enumerate() {
            let to = r0 + i as u32;
            for (c, raw, style) in cells {
                sh.set(to, c, &relocate(&raw, i64::from(to) - i64::from(from), 0));
                sh.set_style(to, c, style);
            }
        }
        Ok(())
    }

    /// The rectangle as tab-separated text (rows end with a line feed); `formulas` gives the
    /// formulas, otherwise the shown values.
    pub fn to_tsv(&self, sheet: usize, rect: (u32, u32, u32, u32), formulas: bool) -> String {
        let (r0, c0, r1, c1) = rect;
        let Some(sh) = self.sheets.get(sheet) else {
            return String::new();
        };
        let mut out = String::new();
        for r in r0..=r1 {
            for c in c0..=c1 {
                if c > c0 {
                    out.push('\t');
                }
                let text = if formulas {
                    sh.get(r, c).to_owned()
                } else {
                    self.value(sheet, r, c).text()
                };
                if text.contains(['\t', '\n', '\r', '"']) {
                    out.push('"');
                    out.push_str(&text.replace('"', "\"\""));
                    out.push('"');
                } else {
                    out.push_str(&text);
                }
            }
            out.push('\n');
        }
        out
    }
}

/// Splits tab-separated text (as Excel and other apps put it on the clipboard) into rows
/// and fields; quoted fields may hold tabs, line ends and doubled quotes.
pub fn parse_tsv(text: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cur.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cur.push(c);
            }
            continue;
        }
        match c {
            '"' if cur.is_empty() => {
                quoted = true;
                started = true;
            }
            '\t' => {
                row.push(std::mem::take(&mut cur));
                started = true;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut cur));
                rows.push(std::mem::take(&mut row));
                started = false;
            }
            _ => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started || !cur.is_empty() || !row.is_empty() {
        row.push(cur);
        rows.push(row);
    }
    rows
}

/// The place of a value in a sorted column: numbers, text, truth values, errors, empty.
fn value_rank(v: &Value) -> u8 {
    match v {
        Value::Num(_) => 0,
        Value::Text(_) => 1,
        Value::Bool(_) => 2,
        Value::Err(_) => 3,
        Value::Empty => 4,
    }
}

/// Orders two values for a sort; empty cells stay last in either direction.
fn compare_values(
    a: &Value,
    b: &Value,
    ascending: bool,
    text: &dyn Fn(&str, &str) -> Ordering,
) -> Ordering {
    let (ra, rb) = (value_rank(a), value_rank(b));
    if ra == 4 || rb == 4 {
        return ra.cmp(&rb);
    }
    let ord = match (a, b) {
        (Value::Num(x), Value::Num(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Text(x), Value::Text(y)) => text(x, y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => ra.cmp(&rb),
    };
    if ascending {
        ord
    } else {
        ord.reverse()
    }
}
