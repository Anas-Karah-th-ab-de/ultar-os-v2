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

    /// The button at the end of the field: 0 = clear (search field holding text),
    /// 1 = show the password while held (password field).
    pub(crate) fn end_button(&self) -> Option<usize> {
        if self.password {
            Some(1)
        } else if self.search && !self.content().is_empty() {
            Some(0)
        } else {
            None
        }
    }

    /// The parts of a field of `field` extent: search icon slot, end button (a full target
    /// square at the end edge), text area.
    fn parts(&self, field: Rect, direction: Direction) -> (Rect, Rect, Rect) {
        let mut inner = style::inset_inline(field, PAD);
        let mut button = Rect::default();
        if self.search || self.password {
            let mut whole = field;
            button = style::take_end(&mut whole, field.h.min(field.w), direction);
            style::take_end(&mut inner, (button.w - PAD.points()).max(0.0), direction);
        }
        if !self.search {
            return (Rect::default(), button, inner);
        }
        let icon = style::take_start(&mut inner, ICON, direction);
        style::take_start(&mut inner, ICON_GAP.points(), direction);
        (icon, button, inner)
    }

    /// The area the text is drawn in.
    pub(crate) fn text_area(&self, field: Rect, direction: Direction) -> Rect {
        self.parts(field, direction).2
    }

    /// The rectangle of the end button, if the field shows one.
    pub(crate) fn end_button_rect(&self, field: Rect, direction: Direction) -> Option<Rect> {
        self.end_button().map(|_| self.parts(field, direction).1)
    }

    /// The part under `pos`: the end button (see [`TextField::end_button`]).
    pub(crate) fn part_at(&self, field: Rect, pos: Point, direction: Direction) -> Option<usize> {
        let button = self.end_button_rect(field, direction)?;
        button.contains(pos).then(|| self.end_button()).flatten()
    }

    /// Paints the search icon and the end button (nothing for a plain field).
    pub(crate) fn paint_search_parts(&self, p: &mut Painter<'_>, field: Rect) {
        let direction = p.env().direction;
        let (icon, button, _) = self.parts(field, direction);
        let muted = p.color(ColorRole::TextMuted);
        if self.search {
            p.icon(IconId::Search, icon, ICON, QuarterTurns::None, muted);
        }
        let glyph = match self.end_button() {
            Some(0) => IconId::Close,
            Some(_) if self.revealed => IconId::Eye,
            Some(_) => IconId::Eye,
            None => return,
        };
        p.icon(glyph, button, ICON, QuarterTurns::None, muted);
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
        assert!(icon.x < text.x && text.right() <= clear.x + PAD.points());
        assert_eq!((clear.w, clear.h), (field.h, field.h), "a full target");
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
