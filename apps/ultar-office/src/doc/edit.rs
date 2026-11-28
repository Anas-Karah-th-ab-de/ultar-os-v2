//! Editing of the document model that keeps the formatting of the runs: inserting, deleting,
//! splitting and formatting character ranges of a paragraph, searching and replacing over the
//! whole document, and the labels of bullet and numbered lists (digits 0-9 always).
//!
//! All offsets count characters (not bytes) of the paragraph text.

use super::{Block, Document, Para, Run, Style};

/// Deepest list level that is told apart.
pub const MAX_LEVEL: usize = 5;
/// Points of indentation that make one list level.
const LEVEL_PT: f32 = 36.0;

/// One place where a search text was found: characters `start..start + len` of the paragraph
/// (or of one table cell, `cell = (row, column)`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub block: usize,
    pub cell: Option<(usize, usize)>,
    pub start: usize,
    pub len: usize,
}

/// `true` when two runs look the same (the text is not compared).
pub fn same_format(a: &Run, b: &Run) -> bool {
    a.bold == b.bold
        && a.italic == b.italic
        && a.underline == b.underline
        && a.strike == b.strike
        && a.size_pt == b.size_pt
        && a.color == b.color
        && a.highlight == b.highlight
}

fn chars(s: &str, from: usize, to: usize) -> String {
    s.chars().skip(from).take(to.saturating_sub(from)).collect()
}

impl Para {
    /// Number of characters.
    pub fn char_len(&self) -> usize {
        self.runs.iter().map(|r| r.text.chars().count()).sum()
    }

    /// Makes a run boundary at character `at` and returns the index of the first run that
    /// starts at or after it (`runs.len()` when `at` is the end).
    fn boundary(&mut self, at: usize) -> usize {
        let mut acc = 0;
        let mut i = 0;
        while i < self.runs.len() {
            let n = self.runs[i].text.chars().count();
            if at <= acc {
                return i;
            }
            if at < acc + n {
                let tail = chars(&self.runs[i].text, at - acc, n);
                self.runs[i].text = chars(&self.runs[i].text, 0, at - acc);
                let run = Run {
                    text: tail,
                    ..self.runs[i].clone()
                };
                self.runs.insert(i + 1, run);
                return i + 1;
            }
            acc += n;
            i += 1;
        }
        self.runs.len()
    }

    /// Merges neighbours of the same look, drops empty runs and keeps at least one run (an
    /// empty paragraph keeps the look of its caret).
    pub fn normalize(&mut self) {
        let keep = self.runs.first().cloned().unwrap_or_default();
        let mut out: Vec<Run> = Vec::with_capacity(self.runs.len());
        for r in self.runs.drain(..) {
            if r.text.is_empty() {
                continue;
            }
            match out.last_mut() {
                Some(l) if same_format(l, &r) => l.text.push_str(&r.text),
                _ => out.push(r),
            }
        }
        if out.is_empty() {
            out.push(Run {
                text: String::new(),
                ..keep
            });
        }
        self.runs = out;
    }

    /// The look of the character before `at` (of the first one at the start): an empty run.
    pub fn format_at(&self, at: usize) -> Run {
        let mut acc = 0;
        let mut found: Option<&Run> = None;
        for r in &self.runs {
            let n = r.text.chars().count();
            if n > 0 {
                if found.is_none() || at > acc {
                    found = Some(r);
                }
                if at <= acc + n {
                    break;
                }
            }
            acc += n;
        }
        let r = found.or(self.runs.first());
        Run {
            text: String::new(),
            ..r.cloned().unwrap_or_default()
        }
    }

    /// Inserts `s` at character `at`; the new text gets `look` or, without it, the look of
    /// the character before.
    pub fn insert_at(&mut self, at: usize, s: &str, look: Option<&Run>) {
        if s.is_empty() {
            return;
        }
        let at = at.min(self.char_len());
        let look = look.cloned().unwrap_or_else(|| self.format_at(at));
        if self.char_len() == 0 {
            self.runs = vec![Run {
                text: s.to_owned(),
                ..look
            }];
            return;
        }
        let i = self.boundary(at);
        self.runs.insert(
            i,
            Run {
                text: s.to_owned(),
                ..look
            },
        );
        self.normalize();
    }

    /// Deletes the characters `from..to`.
    pub fn delete_range(&mut self, from: usize, to: usize) {
        if from >= to {
            return;
        }
        let keep = self.format_at(from);
        let a = self.boundary(from);
        let b = self.boundary(to);
        self.runs.drain(a..b);
        if self.runs.iter().all(|r| r.text.is_empty()) {
            self.runs = vec![keep];
        }
        self.normalize();
    }

    /// Splits at character `at`: `self` keeps the head, the tail is returned. Lists go on in
    /// the tail, headings continue as normal text, a page break stays before the head.
    pub fn split_off(&mut self, at: usize) -> Para {
        let at = at.min(self.char_len());
        let look = self.format_at(at);
        let i = self.boundary(at);
        let tail_runs = self.runs.split_off(i);
        self.normalize_with(&look);
        let mut tail = Para {
            style: match self.style {
                Style::Title | Style::H1 | Style::H2 | Style::H3 | Style::Quote => Style::Normal,
                s => s,
            },
            page_break: false,
            runs: tail_runs,
            ..self.clone()
        };
        tail.normalize_with(&look);
        tail
    }

    fn normalize_with(&mut self, look: &Run) {
        if self.runs.is_empty() {
            self.runs.push(look.clone());
        }
        self.normalize();
    }

    /// Appends the runs of `other` (merging two paragraphs).
    pub fn append(&mut self, other: &Para) {
        if self.char_len() == 0 {
            self.runs.clear();
        }
        self.runs.extend(other.runs.iter().cloned());
        self.normalize();
    }

    /// Changes the look of the characters `from..to` with `f`.
    pub fn format_range(&mut self, from: usize, to: usize, f: &dyn Fn(&mut Run)) {
        if from >= to {
            return;
        }
        let a = self.boundary(from);
        let b = self.boundary(to);
        for r in &mut self.runs[a..b] {
            f(r);
        }
        self.normalize();
    }

    /// `true` when `pred` holds for every run of the characters `from..to` (for an empty
    /// range: for the look at the caret). Used to toggle bold and friends like Word does.
    pub fn all_in_range(&self, from: usize, to: usize, pred: &dyn Fn(&Run) -> bool) -> bool {
        if from >= to {
            return pred(&self.format_at(from));
        }
        let mut acc = 0;
        for r in &self.runs {
            let n = r.text.chars().count();
            if n > 0 && acc < to && acc + n > from && !pred(r) {
                return false;
            }
            acc += n;
        }
        true
    }

    /// The text of the characters `from..to`.
    pub fn slice(&self, from: usize, to: usize) -> String {
        chars(&self.text(), from, to)
    }

    /// The paragraph reduced to the characters `from..to` (formatting kept).
    pub fn sub_para(&self, from: usize, to: usize) -> Para {
        let mut p = self.clone();
        let len = p.char_len();
        p.delete_range(to.min(len), len);
        p.delete_range(0, from.min(p.char_len()));
        p
    }
}

/// The folded characters of `text` with the index of each in the original: characters `fold`
/// maps to `None` do not take part in a search (accents, joiners).
fn folded(text: &str, fold: &dyn Fn(char) -> Option<char>) -> Vec<(char, usize)> {
    text.chars()
        .enumerate()
        .filter_map(|(i, c)| fold(c).map(|f| (f, i)))
        .collect()
}

fn hits_in(
    text: &str,
    needle: &[char],
    fold: &dyn Fn(char) -> Option<char>,
    mut put: impl FnMut(usize, usize),
) {
    let hay = folded(text, fold);
    if needle.is_empty() || hay.len() < needle.len() {
        return;
    }
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()]
            .iter()
            .map(|h| h.0)
            .eq(needle.iter().copied())
        {
            let start = hay[i].1;
            let end = hay[i + needle.len() - 1].1 + 1;
            put(start, end - start);
            i += needle.len();
        } else {
            i += 1;
        }
    }
}

impl Document {
    /// Every place `needle` occurs (not overlapping), in document order. `fold` maps each
    /// character of both text and needle to its search form (`None`: ignored).
    pub fn find_all_with(&self, needle: &str, fold: &dyn Fn(char) -> Option<char>) -> Vec<Hit> {
        let needle: Vec<char> = needle.chars().filter_map(fold).collect();
        let mut out = Vec::new();
        for (bi, b) in self.blocks.iter().enumerate() {
            match b {
                Block::Para(p) => hits_in(&p.text(), &needle, fold, |start, len| {
                    out.push(Hit {
                        block: bi,
                        cell: None,
                        start,
                        len,
                    });
                }),
                Block::Table(t) => {
                    for (ri, row) in t.iter().enumerate() {
                        for (ci, cell) in row.iter().enumerate() {
                            hits_in(cell, &needle, fold, |start, len| {
                                out.push(Hit {
                                    block: bi,
                                    cell: Some((ri, ci)),
                                    start,
                                    len,
                                });
                            });
                        }
                    }
                }
                Block::Picture(_) => {}
            }
        }
        out
    }

    /// Plain search: letters are compared as they are, or lower-cased without `match_case`.
    pub fn find_all(&self, needle: &str, match_case: bool) -> Vec<Hit> {
        let fold = move |c: char| {
            Some(if match_case {
                c
            } else {
                c.to_lowercase().next().unwrap_or(c)
            })
        };
        self.find_all_with(needle, &fold)
    }

    /// Replaces one hit; the new text takes the look of the first replaced character.
    pub fn replace_hit(&mut self, hit: &Hit, repl: &str) {
        match (self.blocks.get_mut(hit.block), hit.cell) {
            (Some(Block::Para(p)), None) => {
                let look = p.format_at(hit.start + 1);
                p.delete_range(hit.start, hit.start + hit.len);
                p.insert_at(hit.start, repl, Some(&look));
            }
            (Some(Block::Table(t)), Some((r, c))) => {
                if let Some(cell) = t.get_mut(r).and_then(|row| row.get_mut(c)) {
                    let head = chars(cell, 0, hit.start);
                    let tail = chars(cell, hit.start + hit.len, cell.chars().count());
                    *cell = format!("{head}{repl}{tail}");
                }
            }
            _ => {}
        }
    }

    /// Replaces every occurrence and returns how many there were.
    pub fn replace_all(&mut self, find: &str, repl: &str, match_case: bool) -> usize {
        let hits = self.find_all(find, match_case);
        for hit in hits.iter().rev() {
            self.replace_hit(hit, repl);
        }
        hits.len()
    }

    /// The marker of every block: `•` or a number like `1.` / `2.1.` (digits 0-9) for list
    /// paragraphs, `None` for everything else. Numbers continue over consecutive list
    /// paragraphs; deeper indentation (36 pt a level) numbers a sub-list.
    pub fn list_labels(&self) -> Vec<Option<String>> {
        let mut out = Vec::with_capacity(self.blocks.len());
        let mut counters: Vec<u32> = Vec::new();
        let mut base: Option<f32> = None;
        for b in &self.blocks {
            let list = match b {
                Block::Para(p) if matches!(p.style, Style::Bullet | Style::Number) => p,
                _ => {
                    counters.clear();
                    base = None;
                    out.push(None);
                    continue;
                }
            };
            let indent = list.indent_pt.unwrap_or(0.0);
            let first = *base.get_or_insert(indent);
            let level = (((indent - first) / LEVEL_PT).round().max(0.0) as usize).min(MAX_LEVEL);
            if list.style == Style::Bullet {
                out.push(Some(["•", "–", "•", "–", "•", "–"][level].to_owned()));
                continue;
            }
            counters.truncate(level + 1);
            while counters.len() <= level {
                counters.push(0);
            }
            counters[level] += 1;
            let label: Vec<String> = counters.iter().map(u32::to_string).collect();
            out.push(Some(format!("{}.", label.join("."))));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold(t: &str) -> Run {
        Run {
            bold: true,
            ..Run::plain(t)
        }
    }

    fn para(runs: Vec<Run>) -> Para {
        Para {
            runs,
            ..Para::default()
        }
    }

    #[test]
    fn typing_inside_a_formatted_paragraph_keeps_every_run() {
        let mut p = para(vec![Run::plain("ab "), bold("cd"), Run::plain(" ef")]);
        p.insert_at(4, "X", None);
        assert_eq!(p.text(), "ab cXd ef");
        assert_eq!(p.runs.len(), 3, "the X joins the bold run");
        assert!(p.runs[1].bold && p.runs[1].text == "cXd");
        p.insert_at(0, "Z", None);
        assert!(!p.runs[0].bold);
        assert_eq!(p.text(), "Zab cXd ef");
    }

    #[test]
    fn delete_across_runs_and_format_range() {
        let mut p = para(vec![Run::plain("ab "), bold("cd"), Run::plain(" ef")]);
        p.delete_range(2, 6);
        assert_eq!(p.text(), "abef");
        assert!(p.runs.iter().all(|r| !r.bold));
        let mut q = para(vec![Run::plain("abcdef")]);
        q.format_range(1, 4, &|r| r.italic = true);
        assert_eq!(q.runs.len(), 3);
        assert!(q.runs[1].italic && q.runs[1].text == "bcd");
        assert!(q.all_in_range(1, 4, &|r| r.italic));
        assert!(!q.all_in_range(0, 4, &|r| r.italic));
        q.format_range(1, 4, &|r| r.italic = false);
        assert_eq!(q.runs.len(), 1, "equal neighbours merge again");
    }

    #[test]
    fn split_and_append_round_trip() {
        let mut p = Para {
            style: Style::H1,
            ..para(vec![Run::plain("ab"), bold("cd")])
        };
        let tail = p.split_off(3);
        assert_eq!(p.text(), "abc");
        assert_eq!(tail.text(), "d");
        assert!(tail.runs[0].bold);
        assert_eq!(
            tail.style,
            Style::Normal,
            "a heading is followed by normal text"
        );
        let mut l = Para::new(Style::Bullet, "x");
        assert_eq!(l.split_off(1).style, Style::Bullet);
        p.append(&tail);
        assert_eq!(p.text(), "abcd");
    }

    #[test]
    fn an_empty_paragraph_keeps_the_look_of_the_caret() {
        let mut p = para(vec![bold("x")]);
        p.delete_range(0, 1);
        assert_eq!(p.char_len(), 0);
        assert!(p.runs[0].bold, "typing goes on in bold");
        p.insert_at(0, "y", None);
        assert!(p.runs[0].bold);
    }

    #[test]
    fn replace_keeps_the_look_and_works_in_tables() {
        let mut d = Document {
            blocks: vec![
                Block::Para(para(vec![
                    Run::plain("say "),
                    bold("Hello"),
                    Run::plain(" you"),
                ])),
                Block::Table(vec![vec!["Hello x".into(), "y".into()]]),
            ],
            ..Document::default()
        };
        assert_eq!(d.find_all("hello", false).len(), 2);
        assert_eq!(d.find_all("hello", true).len(), 0);
        assert_eq!(d.replace_all("Hello", "مرحبا", true), 2);
        let Block::Para(p) = &d.blocks[0] else {
            panic!("paragraph expected");
        };
        assert_eq!(p.text(), "say مرحبا you");
        assert!(p.runs.iter().any(|r| r.bold && r.text == "مرحبا"));
        let Block::Table(t) = &d.blocks[1] else {
            panic!("table expected");
        };
        assert_eq!(t[0][0], "مرحبا x");
    }

    #[test]
    fn search_can_ignore_marks_and_reports_original_offsets() {
        let d = Document::from_text("كَتَبَ الولد");
        let fold = |c: char| {
            if ('\u{64B}'..='\u{652}').contains(&c) {
                None
            } else {
                Some(c)
            }
        };
        let hits = d.find_all_with("كتب", &fold);
        assert_eq!(
            hits,
            vec![Hit {
                block: 0,
                cell: None,
                start: 0,
                len: 5
            }]
        );
    }

    #[test]
    fn list_labels_use_western_digits_and_continue() {
        let n = |t: &str, indent: Option<f32>| {
            Block::Para(Para {
                indent_pt: indent,
                ..Para::new(Style::Number, t)
            })
        };
        let d = Document {
            blocks: vec![
                n("a", Some(36.0)),
                n("b", Some(36.0)),
                n("c", Some(72.0)),
                n("d", Some(36.0)),
                Block::Para(Para::new(Style::Normal, "x")),
                n("e", None),
                Block::Para(Para::new(Style::Bullet, "f")),
            ],
            ..Document::default()
        };
        let labels = d.list_labels();
        assert_eq!(labels[0].as_deref(), Some("1."));
        assert_eq!(labels[1].as_deref(), Some("2."));
        assert_eq!(labels[2].as_deref(), Some("2.1."));
        assert_eq!(labels[3].as_deref(), Some("3."));
        assert_eq!(labels[4], None);
        assert_eq!(labels[5].as_deref(), Some("1."), "a new list starts at one");
        assert_eq!(labels[6].as_deref(), Some("•"));
    }
}
