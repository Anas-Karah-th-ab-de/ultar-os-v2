//! Info bar: a message of one tone inside the content, with an optional action at its end.
//! Empty state: what a view shows when it has nothing to show.

use super::style;
use super::{Label, MeasureCx, PaintCx, Tone};
use crate::geom::{Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::{Painter, TextSpec};
use crate::text::{FontRole, Overflow, TextAlign, Weight};
use crate::tokens::{size, typography, ColorRole, Radius, Space, TypeSize};

/// An info bar. Its tone is shown by icon and spoken name; it may hold one child, the action
/// (a button, or an icon button that dismisses the bar).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoBar {
    /// The meaning: info, success, warning or danger.
    pub tone: Tone,
    /// The message in one line.
    pub title: Label,
    /// More detail under the title.
    pub message: Option<Label>,
}

const PAD_INLINE: Space = Space::S16;
const PAD_BLOCK: Space = Space::S12;
const GAP: Space = Space::S12;
const MIN_HEIGHT: f32 = Space::S48.points();
const TITLE: TextSpec = TextSpec::new(typography::BODY)
    .weight(Weight::Semibold)
    .overflow(Overflow::Wrap);

impl InfoBar {
    /// An info bar of `tone` with a title.
    pub fn new(tone: Tone, title: Label) -> InfoBar {
        InfoBar {
            tone,
            title,
            message: None,
        }
    }

    /// The same bar with a detail text.
    pub fn message(mut self, message: Label) -> InfoBar {
        self.message = Some(message);
        self
    }

    fn text_width(width: f32, control: f32) -> f32 {
        let taken = if control > 0.0 {
            control + GAP.points()
        } else {
            0.0
        };
        (width - 2.0 * PAD_INLINE.points() - size::ICON - GAP.points() - taken).max(0.0)
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>, avail: f32, control: Size) -> Size {
        let width = InfoBar::text_width(avail, control.w);
        let mut text = cx.text_size(&self.title, TITLE, Some(width)).h;
        if let Some(message) = &self.message {
            text += cx.text_size(message, style::SMALL_TEXT, Some(width)).h;
        }
        let content = text.max(control.h).max(size::ICON) + 2.0 * PAD_BLOCK.points();
        Size::new(avail, content.max(MIN_HEIGHT))
    }

    /// Logical placement of the action inside a bar of `size`.
    pub(crate) fn control_offset(size: Size, control: Size) -> (f32, f32) {
        (
            size.w - PAD_INLINE.points() - control.w,
            (size.h - control.h) / 2.0,
        )
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, control: f32) {
        let (bg, tone, hairline) = (
            p.color(ColorRole::Raised),
            p.color(self.tone.role()),
            p.hairline(),
        );
        p.fill(rect, Some(Radius::Control), bg);
        p.stroke(rect, Some(Radius::Control), hairline, tone);
        let direction = p.env().direction;
        let mut inner = style::inset_inline(rect, PAD_INLINE);
        if control > 0.0 {
            style::take_end(&mut inner, control + GAP.points(), direction);
        }
        let slot = style::take_start(&mut inner, size::ICON, direction);
        style::take_start(&mut inner, GAP.points(), direction);
        p.icon(self.tone.icon(), slot, size::ICON, QuarterTurns::None, tone);
        let texts = [
            Some((&self.title, TITLE, ColorRole::Text)),
            self.message
                .as_ref()
                .map(|m| (m, style::SMALL_TEXT, ColorRole::TextMuted)),
        ];
        stack_texts(p, cx, inner, TextAlign::Start, &texts);
    }
}

/// Draws texts one under the other, the block centred vertically in `area`.
fn stack_texts(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    area: Rect,
    align: TextAlign,
    texts: &[Option<(&Label, TextSpec, ColorRole)>],
) {
    let scale = p.env().scale;
    let mut laid = Vec::with_capacity(texts.len());
    for (label, spec, role) in texts.iter().flatten() {
        let text = label.resolve(cx.tr);
        let layout = p.layout(&text, *spec, Some(area.w));
        laid.push((layout, *role));
    }
    let total: f32 = laid.iter().map(|(l, _)| l.height / scale).sum();
    let mut top = area.y + (area.h - total) / 2.0;
    for (layout, role) in laid {
        let height = layout.height / scale;
        let color = p.color(role);
        p.text(
            &layout,
            Rect::new(area.x, top, area.w, height),
            align,
            color,
        );
        top += height;
    }
}

/// An empty state: icon, title, explanation, and optionally one child, the action that
/// fills the view (a button).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyState {
    /// The icon above the title.
    pub icon: IconId,
    /// What is empty, in a few words.
    pub title: Label,
    /// What the user can do about it.
    pub message: Option<Label>,
}

const EMPTY_PAD: Space = Space::S32;
const EMPTY_ICON: f32 = Space::S48.points();
const EMPTY_GAP: Space = Space::S16;
const EMPTY_TITLE: TextSpec = TextSpec::new(TypeSize::S16)
    .weight(Weight::Semibold)
    .role(FontRole::Title)
    .overflow(Overflow::Wrap);
const EMPTY_TEXT: TextSpec = TextSpec::new(typography::BODY).overflow(Overflow::Wrap);

impl EmptyState {
    /// An empty state with icon and title.
    pub fn new(icon: IconId, title: Label) -> EmptyState {
        EmptyState {
            icon,
            title,
            message: None,
        }
    }

    /// The same empty state with an explanation.
    pub fn message(mut self, message: Label) -> EmptyState {
        self.message = Some(message);
        self
    }

    fn text_height(&self, cx: &mut MeasureCx<'_>, width: f32) -> f32 {
        let mut height = cx.text_size(&self.title, EMPTY_TITLE, Some(width)).h;
        if let Some(message) = &self.message {
            height += cx.text_size(message, EMPTY_TEXT, Some(width)).h;
        }
        height
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>, avail: f32, control: Size) -> Size {
        let width = (avail - 2.0 * EMPTY_PAD.points()).max(0.0);
        let mut height = EMPTY_ICON + EMPTY_GAP.points() + self.text_height(cx, width);
        if control.h > 0.0 {
            height += EMPTY_GAP.points() + control.h;
        }
        Size::new(avail, height + 2.0 * EMPTY_PAD.points())
    }

    /// Logical placement of the action: centred, at the bottom of the content.
    pub(crate) fn control_offset(size: Size, control: Size) -> (f32, f32) {
        (
            (size.w - control.w) / 2.0,
            size.h - EMPTY_PAD.points() - control.h,
        )
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, control: Size) {
        let pad = EMPTY_PAD.points();
        let inner = Rect::new(
            rect.x + pad,
            rect.y + pad,
            rect.w - 2.0 * pad,
            rect.h - 2.0 * pad,
        );
        let icon = Rect::new(inner.x, inner.y, inner.w, EMPTY_ICON);
        let muted = p.color(ColorRole::TextMuted);
        p.icon(self.icon, icon, EMPTY_ICON, QuarterTurns::None, muted);
        let top = icon.bottom() + EMPTY_GAP.points();
        let below = if control.h > 0.0 {
            EMPTY_GAP.points() + control.h
        } else {
            0.0
        };
        let area = Rect::new(
            inner.x,
            top,
            inner.w,
            (inner.bottom() - below - top).max(0.0),
        );
        let texts = [
            Some((&self.title, EMPTY_TITLE, ColorRole::Text)),
            self.message
                .as_ref()
                .map(|m| (m, EMPTY_TEXT, ColorRole::TextMuted)),
        ];
        stack_texts(p, cx, area, TextAlign::Center, &texts);
    }
}
