//! Single-line text input with placeholder, caret and error text.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::paint::{layout_directed, PaintEnv, Painter, Resources, TextSpec};
use crate::text::TextAlign;
use crate::tokens::{size, typography, ColorRole, InputMode, Radius, Space};
use ultar_i18n::Direction;

/// Default limit of a field's content in bytes.
pub const DEFAULT_MAX_LEN: usize = 4096;

pub(crate) const PAD: Space = Space::S12;
/// The character a hidden password is drawn with.
const MASK: char = '\u{2022}';
/// Height of the label line above a labelled field (a 12-point line and its gap).
const LABEL_BAND: f32 = Space::S16.points() + Space::S4.points();
const NATURAL_W: f32 = 4.0 * Space::S48.points();
const FIELD_TEXT: TextSpec = TextSpec::new(typography::BODY);

/// The paragraph direction of a field's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextDirection {
    /// The direction of the user interface (names, free text).
    #[default]
    Auto,
    /// Always left-to-right: phone numbers, addresses, URLs, e-mail, code. Without it a
    /// phone number typed in an Arabic interface shows its groups in reverse order.
    Ltr,
    /// Always right-to-left.
    Rtl,
}

impl TextDirection {
    /// The paragraph direction in a user interface of direction `ui`.
    pub fn resolve(self, ui: Direction) -> Direction {
        match self {
            TextDirection::Auto => ui,
            TextDirection::Ltr => Direction::Ltr,
            TextDirection::Rtl => Direction::Rtl,
        }
    }
}

/// A text field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextField {
    /// Accessible name (catalog key).
    pub name: Label,
    /// Hint shown while the field is empty.
    pub placeholder: Option<Label>,
    /// Paragraph direction of the content.
    pub direction: TextDirection,
    /// `true` for a search field: search icon at the start, clear button at the end.
    pub(crate) search: bool,
    /// `true` for a password field: the content is shown as dots.
    pub(crate) password: bool,
    /// `true` while the password is shown (its button held).
    pub(crate) revealed: bool,
    /// `true` when the name is shown above the field.
    pub(crate) show_label: bool,
    text: String,
    caret: usize,
    max_len: usize,
}

/// Characters a single-line field accepts: no control characters, no line or paragraph
/// separators (they would start a second line), no BiDi controls.
fn allowed(ch: char) -> bool {
    !ch.is_control() && !matches!(ch, '\u{2028}' | '\u{2029}') && !crate::text::is_bidi_control(ch)
}

impl TextField {
    /// An empty field.
    pub fn new(name: Label) -> TextField {
        TextField {
            name,
            placeholder: None,
            direction: TextDirection::Auto,
            search: false,
            password: false,
            revealed: false,
            show_label: false,
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

    /// A password field: dots instead of the characters, a button at the end that shows the
    /// password while it is held, content always left-to-right, no value for screen readers.
    pub fn password(name: Label) -> TextField {
        TextField {
            password: true,
            direction: TextDirection::Ltr,
            ..TextField::new(name)
        }
    }

    /// `true` for a password field.
    pub fn is_password(&self) -> bool {
        self.password
    }

    /// `true` while the password is shown.
    pub fn is_revealed(&self) -> bool {
        self.revealed
    }

    /// The text as drawn: the content, or one dot per character of a hidden password.
    pub(crate) fn display(&self) -> std::borrow::Cow<'_, str> {
        if self.password && !self.revealed {
            std::borrow::Cow::Owned(MASK.to_string().repeat(self.text.chars().count()))
        } else {
            std::borrow::Cow::Borrowed(&self.text)
        }
    }

    /// A byte offset of the content as an offset of the drawn text.
    fn to_display(&self, byte: usize) -> usize {
        if self.password && !self.revealed {
            self.text[..byte.min(self.text.len())].chars().count() * MASK.len_utf8()
        } else {
            byte
        }
    }

    /// A byte offset of the drawn text as an offset of the content.
    fn byte_from_display(&self, byte: usize) -> usize {
        if self.password && !self.revealed {
            let chars = byte / MASK.len_utf8();
            self.text
                .char_indices()
                .nth(chars)
                .map_or(self.text.len(), |(i, _)| i)
        } else {
            byte
        }
    }

    /// The same field with a fixed paragraph direction for its content.
    pub fn direction(mut self, direction: TextDirection) -> TextField {
        self.direction = direction;
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

    /// Moves the caret one character towards the physical right (`right`) or left, as the
    /// text is displayed in a paragraph of base direction `base_rtl`: over a left-to-right
    /// character "right" is forward, over a right-to-left character it is backward. At a
    /// place where neither neighbour lies on the wanted side (the end of a run against the
    /// base direction) the caret moves logically, as seen from the base direction.
    pub fn move_caret_visual(&mut self, right: bool, base_rtl: bool) -> bool {
        let base = if base_rtl {
            unicode_bidi::Level::rtl()
        } else {
            unicode_bidi::Level::ltr()
        };
        let bidi = unicode_bidi::BidiInfo::new(&self.text, Some(base));
        let rtl_at = |byte: usize| bidi.levels.get(byte).map(|level| level.is_rtl());
        let next_rtl = if self.caret < self.text.len() {
            rtl_at(self.caret)
        } else {
            None
        };
        let previous_rtl = self.previous_boundary().and_then(rtl_at);
        // Crossing the next character moves right when it is left-to-right; crossing the
        // previous one moves right when it is right-to-left.
        let forward = match (next_rtl, previous_rtl) {
            (Some(next), _) if next != right => true,
            (_, Some(previous)) if previous == right => false,
            _ => right != base_rtl,
        };
        self.move_caret(forward)
    }

    /// Puts the caret at `byte` (moved back to a character boundary, clamped to the content).
    pub fn set_caret(&mut self, byte: usize) {
        let mut at = byte.min(self.text.len());
        while !self.text.is_char_boundary(at) {
            at -= 1;
        }
        self.caret = at;
    }

    /// The field part of the node: below its visible label (if shown), above the error.
    pub(crate) fn field(&self, rect: Rect, mode: InputMode) -> Rect {
        let top = if self.show_label { LABEL_BAND } else { 0.0 };
        let h = mode.min_target().min((rect.h - top).max(0.0));
        Rect::new(rect.x, rect.y + top, rect.w, h)
    }

    /// The same field with its name shown above it (boards: label 12 / 500).
    pub fn labelled(mut self) -> TextField {
        self.show_label = true;
        self
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
        let inner = self.text_area(self.field(rect, mode), env.direction);
        let base = self.direction.resolve(env.direction);
        let shown = self.display();
        let layout = layout_directed(res, env, &shown, FIELD_TEXT, None, base);
        let (width, caret_x) = (
            layout.width / env.scale,
            layout.caret_x(self.to_display(self.caret)) / env.scale,
        );
        let left = TextField::line_left(width, caret_x, inner, base);
        self.byte_from_display(layout.index_at_x((pos.x - left) * env.scale))
    }

    pub(crate) fn measure(
        &self,
        cx: &mut MeasureCx<'_>,
        avail: f32,
        error: Option<&Label>,
    ) -> Size {
        let width = NATURAL_W.min(avail);
        let label = if self.show_label { LABEL_BAND } else { 0.0 };
        Size::new(
            width,
            label + cx.mode.min_target() + style::error_height(cx, error, width),
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
        let field = self.field(rect, cx.mode);
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
        self.paint_search_parts(p, field);
        if self.show_label {
            let band = Rect::new(rect.x, rect.y, rect.w, LABEL_BAND - Space::S4.points());
            let muted = p.color(ColorRole::TextMuted);
            let spec = style::SMALL_TEXT.weight(crate::text::Weight::Medium);
            style::draw_label(p, cx, &self.name, spec, band, TextAlign::Start, muted);
        }
        let inner = self.text_area(field, p.env().direction);
        self.paint_content(p, cx, inner, has_focus);
        style::paint_error(p, cx, error, rect, field.bottom() - rect.y);
        if v.focused {
            style::focus_ring(p, field, Radius::Card);
        }
        p.set_opacity(previous);
    }

    fn paint_content(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, inner: Rect, has_focus: bool) {
        let base = self.direction.resolve(p.env().direction);
        let scale = p.env().scale;
        let shown = self.display();
        let layout = p.layout_directed(&shown, FIELD_TEXT, None, base);
        let caret = self.to_display(self.caret);
        let (width, caret_x) = (layout.width / scale, layout.caret_x(caret) / scale);
        let left = TextField::line_left(width, caret_x, inner, base);
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
mod tests;
