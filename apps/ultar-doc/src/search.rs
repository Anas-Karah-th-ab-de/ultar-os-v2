//! Find and replace. Search is forgiving for Arabic (vowel marks, tatweel and the alef/yeh/
//! teh-marbuta variants are ignored, as in the rest of Ultar) and optionally case sensitive.
//! Hits keep their original offsets, so selecting and replacing work on the real text.

use crate::state::{Cell, Cursor, Doc};
use ultar_i18n::arabic::{fold_letter, is_ignorable};
use ultar_office::doc::Hit;

/// The state of the find bar/dialog.
#[derive(Debug, Clone, Default)]
pub struct Find {
    pub needle: String,
    pub repl: String,
    pub match_case: bool,
    pub hits: Vec<Hit>,
    /// Index into `hits` of the hit the caret sits on.
    pub current: Option<usize>,
}

fn folder(match_case: bool) -> impl Fn(char) -> Option<char> {
    move |c| {
        if is_ignorable(c) {
            return None;
        }
        let c = fold_letter(c);
        Some(if match_case {
            c
        } else {
            c.to_lowercase().next().unwrap_or(c)
        })
    }
}

fn hit_cursor(h: &Hit, end: bool) -> Cursor {
    let cell = h.cell.map(|(row, col)| Cell { row, col });
    Cursor {
        block: h.block,
        cell,
        ch: if end { h.start + h.len } else { h.start },
    }
}

impl Doc {
    /// All hits of `needle` in the whole document (paragraphs and table cells).
    pub fn find_hits(&self, needle: &str, match_case: bool) -> Vec<Hit> {
        if needle.is_empty() {
            return Vec::new();
        }
        self.document.find_all_with(needle, &folder(match_case))
    }

    /// Replaces `hits` (from the end, so offsets stay valid) as one undo step.
    pub fn replace_hits(&mut self, hits: &[Hit], repl: &str) -> usize {
        if hits.is_empty() {
            return 0;
        }
        self.begin();
        for h in hits.iter().rev() {
            self.document.replace_hit(h, repl);
        }
        self.hist.end(&self.document);
        self.clamp();
        hits.len()
    }

    /// Replaces every hit of `find` (the agent's `replace` and "replace all").
    pub fn replace_everywhere(&mut self, find: &str, repl: &str, match_case: bool) -> usize {
        let hits = self.find_hits(find, match_case);
        self.replace_hits(&hits, repl)
    }
}

impl Find {
    /// Searches again (after a change of the text, the needle or the options) and keeps the
    /// current hit near the caret.
    pub fn run(&mut self, doc: &Doc) {
        self.hits = doc.find_hits(&self.needle, self.match_case);
        self.current = None;
        let at = (
            doc.cursor.block,
            doc.cursor.cell.map(|c| (c.row, c.col)),
            doc.cursor.ch,
        );
        let key = |h: &Hit| (h.block, h.cell, h.start);
        self.current = self
            .hits
            .iter()
            .position(|h| key(h) >= at)
            .or(if self.hits.is_empty() { None } else { Some(0) });
    }

    /// Selects the next (`dir > 0`) or previous hit, wrapping round. Returns whether there is one.
    pub fn step(&mut self, doc: &mut Doc, dir: isize) -> bool {
        if self.hits.is_empty() {
            return false;
        }
        let n = self.hits.len() as isize;
        let i = match self.current {
            Some(i) if self.selected(doc, i) => (i as isize + dir.signum()).rem_euclid(n),
            Some(i) => i as isize,
            None => 0,
        } as usize;
        self.current = Some(i);
        self.select(doc, i);
        true
    }

    fn selected(&self, doc: &Doc, i: usize) -> bool {
        self.hits.get(i).is_some_and(|h| {
            doc.anchor == Some(hit_cursor(h, false)) && doc.cursor == hit_cursor(h, true)
        })
    }

    fn select(&self, doc: &mut Doc, i: usize) {
        if let Some(h) = self.hits.get(i) {
            doc.set_pos(hit_cursor(h, false), false);
            doc.set_pos(hit_cursor(h, true), true);
        }
    }

    /// Replaces the current hit and moves on. Returns whether one was replaced.
    pub fn replace_current(&mut self, doc: &mut Doc) -> bool {
        let Some(i) = self.current.filter(|&i| i < self.hits.len()) else {
            return false;
        };
        if !self.selected(doc, i) {
            self.select(doc, i);
            return false;
        }
        let hit = self.hits[i].clone();
        doc.replace_hits(&[hit], &self.repl);
        self.run(doc);
        if let Some(c) = self.current {
            self.select(doc, c);
        }
        true
    }

    /// Replaces all hits; returns how many.
    pub fn replace_all(&mut self, doc: &mut Doc) -> usize {
        let hits = std::mem::take(&mut self.hits);
        let n = doc.replace_hits(&hits, &self.repl);
        self.run(doc);
        n
    }

    /// The hits as (block, cell, start, len) for drawing.
    pub fn marks(&self) -> impl Iterator<Item = (Cursor, Cursor, bool)> + '_ {
        self.hits.iter().enumerate().map(|(i, h)| {
            (
                hit_cursor(h, false),
                hit_cursor(h, true),
                Some(i) == self.current,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Doc {
        let mut d = Doc::new();
        for c in text.chars() {
            if c == '\n' {
                d.newline();
            } else {
                d.insert(&c.to_string());
            }
        }
        d
    }

    #[test]
    fn find_ignores_case_unless_asked_and_arabic_marks_and_variants() {
        let d = doc("Hello hello\nمَدْرَسة وأحمد");
        assert_eq!(d.find_hits("HELLO", false).len(), 2);
        assert_eq!(d.find_hits("HELLO", true).len(), 0);
        assert_eq!(
            d.find_hits("مدرسه", false).len(),
            1,
            "marks and teh marbuta/heh are ignored"
        );
        assert_eq!(
            d.find_hits("احمد", false).len(),
            1,
            "alef variants are folded"
        );
    }

    #[test]
    fn step_walks_the_hits_and_wraps() {
        let mut d = doc("ab ab ab");
        let mut f = Find {
            needle: "ab".into(),
            ..Find::default()
        };
        f.run(&d);
        assert_eq!(f.hits.len(), 3);
        assert!(f.step(&mut d, 1));
        assert_eq!((d.anchor.map(|a| a.ch), d.cursor.ch), (Some(0), 2));
        f.step(&mut d, 1);
        assert_eq!(d.cursor.ch, 5);
        f.step(&mut d, 1);
        f.step(&mut d, 1);
        assert_eq!(d.cursor.ch, 2, "wraps to the first hit");
    }

    #[test]
    fn replace_keeps_the_look_and_one_undo_undoes_all() {
        let mut d = doc("one two one");
        d.select_all();
        d.toggle_run(|r| r.bold, |r, v| r.bold = v);
        d.burst_break();
        let mut f = Find {
            needle: "one".into(),
            repl: "three".into(),
            ..Find::default()
        };
        f.run(&d);
        assert_eq!(f.replace_all(&mut d), 2);
        assert_eq!(d.item_text(0, None).as_deref(), Some("three two three"));
        let ultar_office::doc::Block::Para(p) = &d.document.blocks[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().all(|r| r.bold));
        assert!(d.undo());
        assert_eq!(d.item_text(0, None).as_deref(), Some("one two one"));
    }

    #[test]
    fn replace_current_replaces_the_selected_hit_only() {
        let mut d = doc("a x a");
        let mut f = Find {
            needle: "a".into(),
            repl: "b".into(),
            ..Find::default()
        };
        f.run(&d);
        assert!(!f.replace_current(&mut d), "the first call selects");
        assert!(f.replace_current(&mut d));
        assert_eq!(d.item_text(0, None).as_deref(), Some("b x a"));
    }
}
