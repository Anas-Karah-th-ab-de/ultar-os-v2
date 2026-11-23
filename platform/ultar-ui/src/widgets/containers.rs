//! Non-interactive building blocks: text, icon, settings row, card.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::layout::{Flex, Padding};
use crate::paint::{Painter, TextSpec};
use crate::text::{Overflow, TextAlign, Weight};
use crate::tokens::{size, typography, ColorRole, Radius, Space};

/// A block of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    /// What to show.
    pub label: Label,
    /// Size, weight, overflow.
    pub spec: TextSpec,
    /// Colour role.
    pub color: ColorRole,
    /// Alignment of its lines.
    pub align: TextAlign,
}

impl Text {
    /// Body text that wraps.
    pub fn new(label: Label) -> Text {
        Text {
            label,
            spec: TextSpec::new(typography::BODY).overflow(Overflow::Wrap),
            color: ColorRole::Text,
            align: TextAlign::Start,
        }
    }

    /// The same text with another spec.
    pub fn spec(mut self, spec: TextSpec) -> Text {
        self.spec = spec;
        self
    }

    /// The same text in another colour role.
    pub fn color(mut self, color: ColorRole) -> Text {
        self.color = color;
        self
    }

    /// The same text with another alignment.
    pub fn align(mut self, align: TextAlign) -> Text {
        self.align = align;
        self
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        let size = cx.text_size(&self.label, self.spec, Some(avail));
        Size::new(size.w.min(avail), size.h)
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect) {
        let color = p.color(self.color);
        style::draw_label(p, cx, &self.label, self.spec, rect, self.align, color);
    }
}

/// A stroke icon. Decorative unless given a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    /// Which icon.
    pub icon: IconId,
    /// `true` for the small size (16), `false` for the normal one (24).
    pub small: bool,
    /// Colour role.
    pub color: ColorRole,
    /// Accessible name; `None` hides the icon from the screen reader.
    pub name: Option<Label>,
}

impl Icon {
    /// A normal-size icon in the text colour.
    pub fn new(icon: IconId) -> Icon {
        Icon {
            icon,
            small: false,
            color: ColorRole::Text,
            name: None,
        }
    }

    /// The same icon at the small size.
    pub fn small(mut self) -> Icon {
        self.small = true;
        self
    }

    /// The same icon in another colour role.
    pub fn color(mut self, color: ColorRole) -> Icon {
        self.color = color;
        self
    }

    /// The same icon with an accessible name.
    pub fn name(mut self, name: Label) -> Icon {
        self.name = Some(name);
        self
    }

    fn points(&self) -> f32 {
        if self.small {
            size::ICON_SMALL
        } else {
            size::ICON
        }
    }

    pub(crate) fn measure(&self) -> Size {
        Size::new(self.points(), self.points())
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, rect: Rect) {
        let color = p.color(self.color);
        p.icon(self.icon, rect, self.points(), QuarterTurns::None, color);
    }
}

/// A settings row: label and optional sublabel at the start, one control child at the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The label.
    pub label: Label,
    /// Explanation under the label.
    pub sublabel: Option<Label>,
    /// Draw a separator line under the row (off for the last row of a card).
    pub separator: bool,
}

const ROW_PAD_INLINE: Space = Space::S16;
const ROW_PAD_BLOCK: Space = Space::S8;
const ROW_GAP: Space = Space::S16;
const ROW_MIN_HEIGHT: f32 = size::ROW_MIN;
const ROW_LABEL: TextSpec = TextSpec::new(typography::BODY)
    .weight(Weight::Medium)
    .overflow(Overflow::Ellipsis);

impl Row {
    /// A row with a separator.
    pub fn new(label: Label) -> Row {
        Row {
            label,
            sublabel: None,
            separator: true,
        }
    }

    /// The same row with a sublabel.
    pub fn sublabel(mut self, sublabel: Label) -> Row {
        self.sublabel = Some(sublabel);
        self
    }

    /// The same row without separator (last row of a group).
    pub fn last(mut self) -> Row {
        self.separator = false;
        self
    }

    /// Width left for the texts when the control takes `control` points.
    fn text_width(width: f32, control: f32) -> f32 {
        let taken = if control > 0.0 {
            control + ROW_GAP.points()
        } else {
            0.0
        };
        (width - 2.0 * ROW_PAD_INLINE.points() - taken).max(0.0)
    }

    fn text_heights(&self, cx: &mut MeasureCx<'_>, width: f32) -> (f32, f32) {
        let label = cx.text_size(&self.label, ROW_LABEL, Some(width)).h;
        let sub = match &self.sublabel {
            Some(sub) => cx.text_size(sub, style::SMALL_TEXT, Some(width)).h,
            None => 0.0,
        };
        (label, sub)
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>, avail: f32, control: Size) -> Size {
        let (label, sub) = self.text_heights(cx, Row::text_width(avail, control.w));
        let content = (label + sub).max(control.h) + 2.0 * ROW_PAD_BLOCK.points();
        Size::new(avail, content.max(ROW_MIN_HEIGHT))
    }

    /// The logical placement of the control inside a row of `size`: (offset from the row's
    /// start, offset from its top).
    pub(crate) fn control_offset(size: Size, control: Size) -> (f32, f32) {
        (
            size.w - ROW_PAD_INLINE.points() - control.w,
            (size.h - control.h) / 2.0,
        )
    }

    pub(crate) fn paint(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        control: f32,
    ) {
        let previous = p.set_opacity(v.opacity());
        let direction = p.env().direction;
        let mut inner = style::inset_inline(rect, ROW_PAD_INLINE);
        if control > 0.0 {
            style::take_end(&mut inner, control + ROW_GAP.points(), direction);
        }
        let scale = p.env().scale;
        let label_text = self.label.resolve(cx.tr);
        let label = p.layout(&label_text, ROW_LABEL, Some(inner.w));
        let label_h = label.height / scale;
        let sub = self.sublabel.as_ref().map(|s| {
            let text = s.resolve(cx.tr);
            p.layout(&text, style::SMALL_TEXT, Some(inner.w))
        });
        let sub_h = sub.as_ref().map_or(0.0, |l| l.height / scale);
        let top = rect.y + (rect.h - label_h - sub_h) / 2.0;
        let color = p.color(ColorRole::Text);
        p.text(
            &label,
            Rect::new(inner.x, top, inner.w, label_h),
            TextAlign::Start,
            color,
        );
        if let Some(sub) = sub {
            let muted = p.color(ColorRole::TextMuted);
            p.text(
                &sub,
                Rect::new(inner.x, top + label_h, inner.w, sub_h),
                TextAlign::Start,
                muted,
            );
        }
        p.set_opacity(previous);
        if self.separator {
            let (line, hairline) = (p.color(ColorRole::Line), p.hairline());
            let y = rect.bottom() - hairline;
            p.fill(Rect::new(rect.x, y, rect.w, hairline), None, line);
        }
    }
}

/// A card: a surface with border and corner radius that lays out its children as a flex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Card {
    /// Layout of the children.
    pub flex: Flex,
    /// `true` for a floating card (drawn with the shadow).
    pub floating: bool,
    /// `true` for the raised look: raised background (a floating card
    /// that asks for a decision, such as "keep these display settings?").
    pub raised: bool,
}

impl Card {
    /// A card whose children fill its width edge to edge (for settings rows).
    pub fn new() -> Card {
        Card {
            flex: Flex::column(),
            floating: false,
            raised: false,
        }
    }

    /// A card with inner spacing on all sides and a gap between children.
    pub fn padded() -> Card {
        Card {
            flex: Flex::column()
                .padding(Padding::all(Space::S16))
                .gap(Space::S12),
            floating: false,
            raised: false,
        }
    }

    /// The same card with another child layout.
    pub fn flex(mut self, flex: Flex) -> Card {
        self.flex = flex;
        self
    }

    /// The same card floating above the surface.
    pub fn floating(mut self) -> Card {
        self.floating = true;
        self
    }

    /// The same card floating, with the raised background.
    pub fn raised(mut self) -> Card {
        self.floating = true;
        self.raised = true;
        self
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, rect: Rect) {
        if self.floating {
            p.shadow(rect, Some(Radius::Card));
        }
        // Boards (REVIEW-SELF B5): a raised card has the raised fill, no gold border.
        let (background, border) = if self.raised {
            (ColorRole::Raised, ColorRole::Line)
        } else {
            (ColorRole::Surface, ColorRole::Line)
        };
        let (surface, line, hairline) = (p.color(background), p.color(border), p.hairline());
        p.fill(rect, Some(Radius::Card), surface);
        p.stroke(rect, Some(Radius::Card), hairline, line);
    }
}

impl Default for Card {
    fn default() -> Card {
        Card::new()
    }
}

/// Extra reach of the shadow beside (x) and below (y) a floating rectangle, for damage
/// rectangles: the shadow fades out `blur` points outside the offset outline, as the CSS
/// `box-shadow: 0 offset_y blur` of the artboards does.
pub(crate) fn shadow_reach() -> Point {
    use crate::tokens::shadow;
    Point::new(shadow::BLUR, shadow::OFFSET_Y + shadow::BLUR)
}
