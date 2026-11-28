//! Document-wide settings that are undoable like text: the paper size.

use crate::state::Doc;
use ultar_office::doc::Page;

impl Doc {
    /// Sets the paper to A4 or US Letter (margins as of that format).
    pub fn set_paper(&mut self, letter: bool) {
        let want = if letter { Page::LETTER } else { Page::A4 };
        if self.document.page == want {
            return;
        }
        self.begin();
        self.document.page = want;
        self.hist.end(&self.document);
    }

    /// `true` when the paper is US Letter.
    pub fn is_letter(&self) -> bool {
        self.document.page.is_letter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_paper_changes_and_undoes() {
        let mut d = Doc::new();
        assert!(!d.is_letter());
        d.set_paper(true);
        assert!(d.is_letter() && d.dirty());
        assert!(d.undo());
        assert!(!d.is_letter());
        assert!(d.redo() && d.is_letter());
    }
}
