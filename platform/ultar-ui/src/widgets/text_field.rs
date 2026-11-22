//! Single-line text input with placeholder, caret and error text.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::paint::{layout_text, PaintEnv, Painter, Resources, TextSpec};
use crate::text::TextAlign;
use crate::tokens::{size, typography, ColorRole, InputMode, Radius, Space};
use ultar_i18n::Direction;

/// Default limit of a field's content in bytes.
pub const DEFAULT_MAX_LEN: usize = 4096;

const PAD: Space = Space::S12;
const NATURAL_W: f32 = 4.0 * Space::S48.points();
const FIELD_TEXT: TextSpec = TextSpec::new(typography::BODY);

/// A text field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextField {
    /// Accessible name (catalog key).
    pub name: Label,
    /// Hint shown while the field is empty.
    pub placeholder: Option<Label>,
    text: String,
    caret: usize,
    max_len: usize,
}

fn allowed(ch: char) -> bool {
    !ch.is_control() && !crate::text::is_bidi_control(ch)
}

impl TextField {
    /// An empty field.
    pub fn new(name: Label) -> TextField {
        TextField {
            name,
            placeholder: None,
            text: String::new(),
            caret: 0,
            max_len: DEFAULT_MAX_LEN,
        }
    }

    /// The same field with a placeholder.
    pub fn placeholder(mut self, placeholder: Label) -> TextField {
        self.placeholder = Some(placeholder);
        self
    }

    /// The same field with a content limit in bytes.
    pub fn max_len(mut self, max_len: usize) -> TextField {
        self.max_len = max_len;
        let text = std::mem::take(&mut self.text);
        self.set_text(&text);
        self
    }

    /// The same field with initial content.
    pub fn text(mut self, text: &str) -> TextField {
        self.set_text(text);
        self
    }

    /// The content.
    pub fn content(&self) -> &str {
        &self.text
    }

    /// The caret position as a byte offset (always on a character boundary).
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Replaces the content (control characters dropped, cut at the limit); caret at the end.
    pub fn set_text(&mut self, text: &str) {
        self.text.clear();
        for ch in text.chars().filter(|c| allowed(*c)) {
            if self.text.len() + ch.len_utf8() > self.max_len {
                break;
            }
            self.text.push(ch);
        }
        self.caret = self.text.len();
    }

    /// Inserts a character at the caret. Returns `false` when it was refused (control
    /// character or limit reached).
    pub fn insert(&mut self, ch: char) -> bool {
        if !allowed(ch) || self.text.len() + ch.len_utf8() > self.max_len {
            return false;
        }
        self.text.insert(self.caret, ch);
        self.caret += ch.len_utf8();
        true
    }

    fn previous_boundary(&self) -> Option<usize> {
        self.text[..self.caret]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
    }

    fn next_boundary(&self) -> Option<usize> {
        self.text[self.caret..]
            .chars()
            .next()
            .map(|c| self.caret + c.len_utf8())
    }

    /// Deletes the character before the caret; `false` at the beginning.
    pub fn backspace(&mut self) -> bool {
        match self.previous_boundary() {
            Some(start) => {
                self.text.replace_range(start..self.caret, "");
                self.caret = start;
                true
            }
            None => false,
        }
    }

    /// Deletes the character after the caret; `false` at the end.
    pub fn delete(&mut self) -> bool {
        match self.next_boundary() {
            Some(end) => {
                self.text.replace_range(self.caret..end, "");
                true
            }
            None => false,
        }
    }

    /// Moves the caret one character forward (towards the end of the text) or backward;
    /// `false` when it cannot move.
    pub fn move_caret(&mut self, forward: bool) -> bool {
        let target = if forward {
            self.next_boundary()
        } else {
            self.previous_boundary()
        };
        match target {
            Some(position) => {
                self.caret = position;
                true
            }
            None => false,
        }
    }

    /// Puts the caret at `byte` (moved back to a character boundary, clamped to the content).
    pub fn set_caret(&mut self, byte: usize) {
        let mut at = byte.min(self.text.len());
        while !self.text.is_char_boundary(at) {
            at -= 1;
        }
        self.caret = at;
    }

    pub(crate) fn field(rect: Rect, mode: InputMode) -> Rect {
        Rect::new(rect.x, rect.y, rect.w, mode.min_target().min(rect.h))
    }

    /// Left end of the text line so that the caret stays inside `inner`.
    fn line_left(width: f32, caret_x: f32, inner: Rect, direction: Direction) -> f32 {
        if direction.is_rtl() {
            let left = inner.right() - width;
            left + (inner.x - (left + caret_x)).max(0.0)
        } else {
            inner.x - (caret_x - inner.w).max(0.0)
        }
    }

    /// The caret position (byte offset) for a pointer at `pos`.
    pub(crate) fn caret_at(
        &self,
        res: &mut Resources,
        env: &PaintEnv,
        rect: Rect,
        mode: InputMode,
        pos: Point,
    ) -> usize {
        let inner = style::inset_inline(TextField::field(rect, mode), PAD);
        let layout = layout_text(res, env, &self.text, FIELD_TEXT, None);
        let (width, caret_x) = (
            layout.width / env.scale,
            layout.caret_x(self.caret) / env.scale,
        );
        let left = TextField::line_left(width, caret_x, inner, env.direction);
        layout.index_at_x((pos.x - left) * env.scale)
    }

    pub(crate) fn measure(
        &self,
        cx: &mut MeasureCx<'_>,
        avail: f32,
        error: Option<&Label>,
    ) -> Size {
        let width = NATURAL_W.min(avail);
        Size::new(
            width,
            cx.mode.min_target() + style::error_height(cx, error, width),
        )
    }

    /// Paints the field; `has_focus` shows the caret (also for pointer focus).
    pub(crate) fn paint(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        has_focus: bool,
        error: Option<&Label>,
    ) {
        let previous = p.set_opacity(v.opacity());
        let field = TextField::field(rect, cx.mode);
        let bg = if v.hovered && !has_focus {
            ColorRole::Hover
        } else {
            ColorRole::Raised
        };
        let bg = p.color(bg);
        p.fill(field, Some(Radius::Control), bg);
        let (border, width) = match (v.error, has_focus) {
            (true, _) => (ColorRole::Danger, size::STROKE),
            (false, true) => (ColorRole::AccentText, size::STROKE),
            (false, false) => (ColorRole::ControlBorder, p.hairline()),
        };
        let border = p.color(border);
        p.stroke(field, Some(Radius::Control), width, border);
        let inner = style::inset_inline(field, PAD);
        self.paint_content(p, cx, inner, has_focus);
        style::paint_error(p, cx, error, rect, field.h);
        if v.focused {
            style::focus_ring(p, field, Radius::Card);
        }
        p.set_opacity(previous);
    }

    fn paint_content(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, inner: Rect, has_focus: bool) {
        let direction = p.env().direction;
        let scale = p.env().scale;
        let layout = p.layout(&self.text, FIELD_TEXT, None);
        let (width, caret_x) = (layout.width / scale, layout.caret_x(self.caret) / scale);
        let left = TextField::line_left(width, caret_x, inner, direction);
        let saved = p.clip(inner);
        if self.text.is_empty() {
            if let Some(placeholder) = &self.placeholder {
                let muted = p.color(ColorRole::TextMuted);
                style::draw_label(
                    p,
                    cx,
                    placeholder,
                    style::CONTROL_TEXT,
                    inner,
                    TextAlign::Start,
                    muted,
                );
            }
        } else {
            let color = p.color(ColorRole::Text);
            p.text(
                &layout,
                Rect::new(left, inner.y, width, inner.h),
                TextAlign::Start,
                color,
            );
        }
        p.restore_clip(saved);
        if has_focus {
            let x = left + caret_x;
            let reach = layout.height / scale / 2.0;
            let middle = inner.y + inner.h / 2.0;
            let color = p.color(ColorRole::Text);
            p.line(
                Point::new(x, middle - reach),
                Point::new(x, middle + reach),
                size::STROKE,
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_keeps_the_caret_on_boundaries() {
        let mut f = TextField::new(Label::key("f")).text("سلام");
        assert_eq!((f.content(), f.caret()), ("سلام", 8));
        assert!(f.backspace());
        assert_eq!((f.content(), f.caret()), ("سلا", 6));
        assert!(f.move_caret(false) && f.move_caret(false));
        assert_eq!(f.caret(), 2);
        assert!(f.insert('ـ'));
        assert_eq!(f.content(), "سـلا");
        assert!(f.delete());
        assert_eq!(f.content(), "سـا");
        f.set_caret(1);
        assert_eq!(f.caret(), 0, "moved back to a boundary");
        assert!(!f.backspace() && !f.move_caret(false));
        f.set_caret(999);
        assert_eq!(f.caret(), f.content().len());
        assert!(!f.delete() && !f.move_caret(true));
    }

    #[test]
    fn control_characters_and_the_limit_are_refused() {
        let mut f = TextField::new(Label::key("f")).max_len(5);
        assert!(!f.insert('\n') && !f.insert('\u{202E}') && !f.insert('\u{7}'));
        f.set_text("ab\tc\u{2066}d");
        assert_eq!(f.content(), "abcd");
        assert!(f.insert('e'));
        assert!(!f.insert('f'), "limit of 5 bytes reached");
        assert!(!f.insert('ش'));
        f.set_text("ششششش");
        assert_eq!(
            f.content(),
            "شش",
            "cut at a character boundary below the limit"
        );
        let shrunk = TextField::new(Label::key("f")).text("abcdef").max_len(3);
        assert_eq!(shrunk.content(), "abc");
        assert!(TextField::new(Label::key("f"))
            .placeholder(Label::key("p"))
            .placeholder
            .is_some());
    }

    #[test]
    fn long_text_scrolls_to_keep_the_caret_visible() {
        let inner = Rect::new(10.0, 0.0, 100.0, 20.0);
        assert_eq!(
            TextField::line_left(50.0, 50.0, inner, Direction::Ltr),
            10.0
        );
        assert_eq!(
            TextField::line_left(300.0, 300.0, inner, Direction::Ltr),
            10.0 - 200.0
        );
        assert_eq!(
            TextField::line_left(300.0, 20.0, inner, Direction::Ltr),
            10.0
        );
        // Right-to-left: a short line sits at the right; a long one shifts so that the caret
        // (at x = 0 of the line, the end of the text) is at the left inner edge.
        assert_eq!(TextField::line_left(50.0, 0.0, inner, Direction::Rtl), 60.0);
        assert_eq!(
            TextField::line_left(300.0, 0.0, inner, Direction::Rtl),
            10.0
        );
        assert_eq!(
            TextField::line_left(300.0, 300.0, inner, Direction::Rtl),
            -190.0
        );
    }
}
