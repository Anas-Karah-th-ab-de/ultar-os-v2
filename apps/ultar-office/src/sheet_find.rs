//! Find and replace in the cells of a workbook.

use crate::sheet::Workbook;


/// How find and replace compare.
#[derive(Clone, Copy, Debug, Default)]
pub struct FindOpts {
    /// Upper and lower case differ.
    pub match_case: bool,
    /// The whole content of the cell must be the text.
    pub whole_cell: bool,
    /// Also look at the shown values of formulas (otherwise only at what was typed).
    pub in_values: bool,
    /// Brings both texts to one form before comparing (the app passes the Arabic search
    /// normalisation: letter forms, marks).
    pub normalize: Option<fn(&str) -> String>,
}

fn fold(s: &str, o: &FindOpts) -> String {
    let s = o.normalize.map_or_else(|| s.to_owned(), |n| n(s));
    if o.match_case {
        s
    } else {
        s.to_lowercase()
    }
}

/// `true` when `needle` is in `hay` (or is all of it) as `o` says.
pub fn text_matches(hay: &str, needle: &str, o: &FindOpts) -> bool {
    if needle.is_empty() {
        return false;
    }
    let (h, n) = (fold(hay, o), fold(needle, o));
    if o.whole_cell {
        h == n
    } else {
        h.contains(&n)
    }
}

/// Replaces `needle` by `with` in `raw` (every place, or the whole content with `whole_cell`);
/// `None` when nothing matched. This compares letter by letter (case as `match_case`
/// says), without the search normalisation.
pub fn replace_text(raw: &str, needle: &str, with: &str, o: &FindOpts) -> Option<String> {
    if needle.is_empty() {
        return None;
    }
    let low = |c: char| c.to_lowercase().next().unwrap_or(c);
    let eq = |a: char, b: char| if o.match_case { a == b } else { low(a) == low(b) };
    let hay: Vec<char> = raw.chars().collect();
    let nd: Vec<char> = needle.chars().collect();
    if o.whole_cell {
        let same = hay.len() == nd.len() && hay.iter().zip(&nd).all(|(a, b)| eq(*a, *b));
        return same.then(|| with.to_owned());
    }
    let mut out = String::new();
    let (mut i, mut hit) = (0, false);
    while i < hay.len() {
        if i + nd.len() <= hay.len() && hay[i..i + nd.len()].iter().zip(&nd).all(|(a, b)| eq(*a, *b)) {
            out.push_str(with);
            i += nd.len();
            hit = true;
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    hit.then_some(out)
}

impl Workbook {
    /// Every cell (sheet, row, column) that matches, sheet by sheet, row by row. `only`
    /// limits the search to one sheet.
    pub fn find_all(&self, only: Option<usize>, needle: &str, o: &FindOpts) -> Vec<(usize, u32, u32)> {
        let mut out = Vec::new();
        for (si, sh) in self.sheets.iter().enumerate() {
            if only.is_some_and(|s| s != si) {
                continue;
            }
            for (&(r, c), raw) in &sh.cells {
                let hit = text_matches(raw, needle, o)
                    || (o.in_values
                        && raw.starts_with('=')
                        && text_matches(&self.value(si, r, c).text(), needle, o));
                if hit {
                    out.push((si, r, c));
                }
            }
        }
        out
    }
}

