//! Search field: a text field with a search icon at its start and, while it holds text, a
//! clear button at its end. Escape clears it; Enter submits.

use super::style;
use super::text_field::PAD;
use super::{Label, TextField};
use crate::geom::{Point, Rect};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::Painter;
use crate::tokens::{size, ColorRole, Space};
use ultar_i18n::Direction;

/// A search field. In the tree it is a text field (`Ui::text`, `Ui::set_text` and the text
/// actions apply); its accessibility role is "search field".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchField {
    field: TextField,
}

const ICON: f32 = size::ICON_SMALL;
const ICON_GAP: Space = Space::S8;

impl SearchField {
    /// An empty search field.
    pub fn new(name: Label) -> SearchField {
        let mut field = TextField::new(name);
        field.search = true;
        SearchField { field }
    }

    /// The same field with a placeholder.
    pub fn placeholder(mut self, placeholder: Label) -> SearchField {
        self.field = self.field.placeholder(placeholder);
        self
    }

    /// The same field with initial content.
    pub fn text(mut self, text: &str) -> SearchField {
        self.field = self.field.text(text);
        self
    }

    /// The same field with a content limit in bytes.
    pub fn max_len(mut self, max_len: usize) -> SearchField {
        self.field = self.field.max_len(max_len);
        self
    }

    /// The text field this search field is in the tree.
    pub fn into_field(self) -> TextField {
        self.field
    }
}

impl TextField {
    /// `true` for a search field.
    pub fn is_search(&self) -> bool {
        self.search
    }

    /// Empties the field; `false` when it was empty already.
    pub fn clear(&mut self) -> bool {
        let had_text = !self.content().is_empty();
        self.set_text("");
        had_text
    }

    /// `true` while the clear button shows.
    fn has_clear(&self) -> bool {
        self.search && !self.content().is_empty()
    }

    /// The parts of a field of `field` extent: search icon slot, clear slot, text area.
    fn parts(&self, field: Rect, direction: Direction) -> (Rect, Rect, Rect) {
        let mut inner = style::inset_inline(field, PAD);
        if !self.search {
            return (Rect::default(), Rect::default(), inner);
        }
        let icon = style::take_start(&mut inner, ICON, direction);
        style::take_start(&mut inner, ICON_GAP.points(), direction);
        let clear = style::take_end(&mut inner, ICON, direction);
        style::take_end(&mut inner, ICON_GAP.points(), direction);
        (icon, clear, inner)
    }

    /// The area the text is drawn in.
    pub(crate) fn text_area(&self, field: Rect, direction: Direction) -> Rect {
        self.parts(field, direction).2
    }

    /// The part under `pos`: `Some(0)` for the clear button.
    pub(crate) fn part_at(&self, field: Rect, pos: Point, direction: Direction) -> Option<usize> {
        let clear = self.parts(field, direction).1;
        // The whole end of the field is the target, not only the 16-point cross.
        let target = clear.inflate(PAD.points());
        (self.has_clear() && target.contains(pos)).then_some(0)
    }

    /// Paints the search icon and the clear button (nothing for a plain field).
    pub(crate) fn paint_search_parts(&self, p: &mut Painter<'_>, field: Rect) {
        if !self.search {
            return;
        }
        let (icon, clear, _) = self.parts(field, p.env().direction);
        let muted = p.color(ColorRole::TextMuted);
        p.icon(IconId::Search, icon, ICON, QuarterTurns::None, muted);
        if self.has_clear() {
            p.icon(IconId::Close, clear, ICON, QuarterTurns::None, muted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clear_button_exists_only_with_text() {
        let field = Rect::new(0.0, 0.0, 4.0 * Space::S48.points(), Space::S32.points());
        let empty = SearchField::new(Label::key("s"))
            .placeholder(Label::key("p"))
            .max_len(64)
            .into_field();
        assert!(empty.is_search() && empty.placeholder.is_some());
        let (icon, clear, text) = empty.parts(field, Direction::Ltr);
        assert!(icon.x < text.x && text.right() < clear.x);
        assert_eq!(empty.part_at(field, clear.center(), Direction::Ltr), None);
        let mut filled = SearchField::new(Label::key("s")).text("نص").into_field();
        assert_eq!(
            filled.part_at(field, clear.center(), Direction::Ltr),
            Some(0)
        );
        assert_eq!(filled.part_at(field, text.center(), Direction::Ltr), None);
        // Right-to-left: the clear button is at the left end.
        let (icon_rtl, clear_rtl, _) = filled.parts(field, Direction::Rtl);
        assert!(clear_rtl.x < icon_rtl.x);
        assert_eq!(
            filled.part_at(field, clear_rtl.center(), Direction::Rtl),
            Some(0)
        );
        assert!(filled.clear() && !filled.clear());
        assert_eq!((filled.content(), filled.caret()), ("", 0));
        // A plain field has neither part and uses its whole inner width for text.
        let plain = TextField::new(Label::key("f")).text("x");
        assert!(!plain.is_search());
        assert_eq!(plain.part_at(field, clear.center(), Direction::Ltr), None);
        assert!(plain.text_area(field, Direction::Ltr).w > text.w);
    }
}
