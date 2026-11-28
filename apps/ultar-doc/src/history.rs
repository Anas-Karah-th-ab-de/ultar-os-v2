//! Undo and redo. One step covers a burst of edits (a run of typed letters, one formatting
//! command) and stores only the blocks that changed, not the whole document: the memory of
//! a hundred-page document with hundreds of steps stays small.

use crate::state::Cursor;
use ultar_office::doc::{Block, Document, Page};

/// Most steps kept.
const MAX_STEPS: usize = 500;

/// The state before a burst began.
struct Open {
    before: Vec<Block>,
    page: Page,
    cursor: Cursor,
    anchor: Option<Cursor>,
}

/// One undoable change: `old` replaces `new_len` blocks from `at` when applied.
struct Step {
    at: usize,
    old: Vec<Block>,
    new_len: usize,
    page: Page,
    cursor: Cursor,
    anchor: Option<Cursor>,
}

/// The undo and redo stacks.
#[derive(Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    open: Option<Open>,
}

impl History {
    /// `true` while a burst is open (edits join it).
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Opens a burst before the first edit of it.
    pub fn begin(&mut self, doc: &Document, cursor: Cursor, anchor: Option<Cursor>) {
        if self.open.is_none() {
            self.open = Some(Open {
                before: doc.blocks.clone(),
                page: doc.page,
                cursor,
                anchor,
            });
        }
    }

    /// Closes the open burst; a burst that changed nothing leaves no step.
    pub fn end(&mut self, doc: &Document) {
        let Some(open) = self.open.take() else {
            return;
        };
        let (a, b) = (&open.before, &doc.blocks);
        let max = a.len().min(b.len());
        let head = (0..max).take_while(|&i| a[i] == b[i]).count();
        let tail = (0..max - head)
            .take_while(|&i| a[a.len() - 1 - i] == b[b.len() - 1 - i])
            .count();
        if head + tail >= a.len() && head + tail >= b.len() && open.page == doc.page {
            return;
        }
        self.undo.push(Step {
            at: head,
            old: a[head..a.len() - tail].to_vec(),
            new_len: b.len() - tail - head,
            page: open.page,
            cursor: open.cursor,
            anchor: open.anchor,
        });
        if self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.open.is_some()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn apply(
        step: Step,
        doc: &mut Document,
        cursor: &mut Cursor,
        anchor: &mut Option<Cursor>,
    ) -> Step {
        let end = (step.at + step.new_len).min(doc.blocks.len());
        let at = step.at.min(end);
        let now: Vec<Block> = doc
            .blocks
            .splice(at..end, step.old.iter().cloned())
            .collect();
        let inverse = Step {
            at,
            old: now,
            new_len: step.old.len(),
            page: doc.page,
            cursor: *cursor,
            anchor: *anchor,
        };
        doc.page = step.page;
        *cursor = step.cursor;
        *anchor = step.anchor;
        inverse
    }

    /// Takes the last step back.
    pub fn undo(
        &mut self,
        doc: &mut Document,
        cursor: &mut Cursor,
        anchor: &mut Option<Cursor>,
    ) -> bool {
        self.end(doc);
        let Some(step) = self.undo.pop() else {
            return false;
        };
        let inverse = Self::apply(step, doc, cursor, anchor);
        self.redo.push(inverse);
        true
    }

    /// Does the step that was taken back again.
    pub fn redo(
        &mut self,
        doc: &mut Document,
        cursor: &mut Cursor,
        anchor: &mut Option<Cursor>,
    ) -> bool {
        self.end(doc);
        let Some(step) = self.redo.pop() else {
            return false;
        };
        let inverse = Self::apply(step, doc, cursor, anchor);
        self.undo.push(inverse);
        true
    }

    /// Number of steps that can be taken back (for tests and the status line).
    pub fn depth(&self) -> usize {
        self.undo.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_office::doc::{Para, Style};

    fn doc(texts: &[&str]) -> Document {
        Document {
            blocks: texts
                .iter()
                .map(|t| Block::Para(Para::new(Style::Normal, t)))
                .collect(),
            ..Document::default()
        }
    }

    fn text(d: &Document, i: usize) -> String {
        match &d.blocks[i] {
            Block::Para(p) => p.text(),
            _ => String::new(),
        }
    }

    #[test]
    fn a_step_stores_only_the_changed_blocks() {
        let mut h = History::default();
        let mut d = doc(&["a", "b", "c", "d"]);
        let (mut c, mut a) = (Cursor::default(), None);
        h.begin(&d, c, a);
        d.blocks[2] = Block::Para(Para::new(Style::Normal, "C"));
        h.end(&d);
        assert_eq!(
            h.undo.last().map(|s| (s.at, s.old.len(), s.new_len)),
            Some((2, 1, 1))
        );
        assert!(h.undo(&mut d, &mut c, &mut a));
        assert_eq!(text(&d, 2), "c");
        assert!(h.redo(&mut d, &mut c, &mut a));
        assert_eq!(text(&d, 2), "C");
    }

    #[test]
    fn inserting_and_removing_blocks_undoes() {
        let mut h = History::default();
        let mut d = doc(&["a", "b"]);
        let (mut c, mut a) = (
            Cursor {
                block: 1,
                ..Cursor::default()
            },
            None,
        );
        h.begin(&d, c, a);
        d.blocks
            .insert(1, Block::Para(Para::new(Style::Normal, "x")));
        d.blocks.remove(0);
        h.end(&d);
        assert_eq!((text(&d, 0), text(&d, 1)), ("x".into(), "b".into()));
        h.undo(&mut d, &mut c, &mut a);
        assert_eq!(
            (d.blocks.len(), text(&d, 0), text(&d, 1)),
            (2, "a".into(), "b".into())
        );
        assert_eq!(c.block, 1, "the caret goes back too");
    }

    #[test]
    fn no_change_no_step_and_a_new_edit_clears_redo() {
        let mut h = History::default();
        let mut d = doc(&["a"]);
        let (mut c, mut a) = (Cursor::default(), None);
        h.begin(&d, c, a);
        h.end(&d);
        assert_eq!(h.depth(), 0);
        h.begin(&d, c, a);
        d.blocks[0] = Block::Para(Para::new(Style::Normal, "b"));
        h.end(&d);
        h.undo(&mut d, &mut c, &mut a);
        assert!(h.can_redo());
        h.begin(&d, c, a);
        d.blocks[0] = Block::Para(Para::new(Style::Normal, "z"));
        h.end(&d);
        assert!(!h.can_redo());
    }

    #[test]
    fn the_paper_is_part_of_a_step() {
        let mut h = History::default();
        let mut d = doc(&["a"]);
        let (mut c, mut a) = (Cursor::default(), None);
        h.begin(&d, c, a);
        d.page = Page::LETTER;
        h.end(&d);
        assert!(h.undo(&mut d, &mut c, &mut a));
        assert_eq!(d.page, Page::A4);
    }
}
