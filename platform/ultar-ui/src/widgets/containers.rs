//! Non-interactive building blocks: text, icon, settings row, card.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::layout::{Flex, Padding};
use crate::paint::{Painter, TextSpec};
use crate::text::{Overflow, TextAlign, Weight};
use crate::tokens::{size, typography, ColorRole, Radius, Space, TypeSize};

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
    /// A size of its own in points (brand mark 48 / 96), else the small or normal size.
    pub points: Option<u16>,
}

impl Icon {
    /// A normal-size icon in the text colour.
    pub fn new(icon: IconId) -> Icon {
        Icon {
            icon,
            small: false,
            color: ColorRole::Text,
            name: None,
            points: None,
        }
    }

    /// The same icon drawn `points` large (the brand mark: 48 on login, 96 on About).
    pub fn sized(mut self, points: u16) -> Icon {
        self.points = Some(points);
        self
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
        if let Some(p) = self.points {
            f32::from(p)
        } else if self.small {
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
        // Boards: CSS line boxes are not rounded (a card of rows would drift otherwise), and
        // the separator is inside the row's minimum height (border-box).
        let exact = |h: f32, size: TypeSize| {
            let line = super::list::line_box(cx.env, size);
            let factor = if cx.env.lang == ultar_i18n::Lang::Ar {
                typography::LINE_HEIGHT_ARABIC
            } else {
                typography::LINE_HEIGHT_LATIN
            };
            (h / line).round() * size.points() * factor
        };
        let (label, sub) = (exact(label, TypeSize::S14), exact(sub, TypeSize::S12));
        let separator = if self.separator { size::BORDER } else { 0.0 };
        let content = (label + sub).max(control.h) + 2.0 * ROW_PAD_BLOCK.points() + separator;
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

/// How a card looks (boards).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CardTone {
    /// Surface (or raised) fill, `line` border, radius 12.
    #[default]
    Plain,
    /// Something serious that is about to happen (boards: "what will be erased"): raised
    /// fill, a two-border `danger` edge, radius 12; announced as an alert.
    Danger,
    /// A bar or sidebar region: `bar` fill, no radius, one `line` border on one edge.
    Bar(BarEdge),
    /// The dock of the desktop shell (board DesktopTop): a floating card in the `bar` fill
    /// with the `line` border, radius 12.
    Dock,
    /// A sheet attached to the top edge of its surface (the phone's quick settings, board
    /// CompactQuick): square top corners, no border at the top and the sides.
    TopSheet,
    /// A part of a card set off by a hairline (boards: the card head's `border-bottom`): no
    /// fill of its own, one `line` border on one edge.
    Rule(BarEdge),
}

/// The edge of a bar region that carries its border.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BarEdge {
    /// The start edge (right in Arabic).
    Start,
    /// The end edge (a sidebar: left in Arabic).
    End,
    /// The top edge (a footer bar).
    Top,
    /// The bottom edge (a header bar).
    Bottom,
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
    /// Plain, danger or bar look.
    pub tone: CardTone,
}

impl Card {
    /// A card whose children fill its width edge to edge (for settings rows).
    pub fn new() -> Card {
        Card {
            flex: Flex::column(),
            floating: false,
            raised: false,
            tone: CardTone::Plain,
        }
    }

    /// A danger-toned card (boards "InstallerSummary": padding 16, gap 12, a two-border
    /// `danger` edge on the raised fill), announced as an alert.
    pub fn danger() -> Card {
        Card {
            tone: CardTone::Danger,
            ..Card::padded()
        }
    }

    /// A bar or sidebar region (boards: `bar` fill and a `line` border on `edge`; settings
    /// sidebar 288 wide with padding 12 and gap 4, installer steps, footer bars). Size and
    /// spacing come from its sizing and flex.
    pub fn bar(edge: BarEdge) -> Card {
        Card {
            tone: CardTone::Bar(edge),
            ..Card::new()
        }
    }

    /// A part of a card separated from the next by a hairline on `edge` (boards: card head
    /// with `border-bottom`), without a fill of its own.
    pub fn rule(edge: BarEdge) -> Card {
        Card {
            tone: CardTone::Rule(edge),
            ..Card::new()
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
            tone: CardTone::Plain,
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

    /// The same card as the dock: floating, in the bar's colour.
    pub fn dock(mut self) -> Card {
        self.floating = true;
        self.tone = CardTone::Dock;
        self
    }

    /// The same card as a sheet attached to the top edge of its surface.
    pub fn top_sheet(mut self) -> Card {
        self.floating = true;
        self.tone = CardTone::TopSheet;
        self
    }

    /// The same card floating, with the raised background.
    pub fn raised(mut self) -> Card {
        self.floating = true;
        self.raised = true;
        self
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, rect: Rect) {
        // A sheet's outline starts above and beside its node: the surface cuts the top
        // corners and the side borders away.
        let rect = if self.tone == CardTone::TopSheet {
            let (up, side) = (Radius::Card.points(), p.hairline());
            Rect::new(rect.x - side, rect.y - up, rect.w + 2.0 * side, rect.h + up)
        } else {
            rect
        };
        if self.floating {
            p.shadow(rect, Some(Radius::Card));
        }
        match self.tone {
            CardTone::Bar(edge) => return Card::paint_bar(p, rect, edge, true),
            CardTone::Rule(edge) => return Card::paint_bar(p, rect, edge, false),
            CardTone::Danger => {
                let (fill, danger) = (p.color(ColorRole::Raised), p.color(ColorRole::Danger));
                p.fill(rect, Some(Radius::Card), fill);
                p.stroke(rect, Some(Radius::Card), 2.0 * p.hairline(), danger);
                return;
            }
            CardTone::Plain | CardTone::Dock | CardTone::TopSheet => {}
        }
        // Boards (REVIEW-SELF B5): a raised card has the raised fill, no gold border.
        let (background, border) = if self.tone == CardTone::Dock {
            (ColorRole::Bar, ColorRole::Line)
        } else if self.raised {
            (ColorRole::Raised, ColorRole::Line)
        } else {
            (ColorRole::Surface, ColorRole::Line)
        };
        let (surface, line, hairline) = (p.color(background), p.color(border), p.hairline());
        p.fill(rect, Some(Radius::Card), surface);
        p.stroke(rect, Some(Radius::Card), hairline, line);
    }
}

impl Card {
    /// A bar region: `bar` fill and one `line` border on `edge` (mirrored in RTL).
    fn paint_bar(p: &mut Painter<'_>, rect: Rect, edge: BarEdge, filled: bool) {
        let (bar, line, hairline) = (
            p.color(ColorRole::Bar),
            p.color(ColorRole::Line),
            p.hairline(),
        );
        if filled {
            p.fill(rect, None, bar);
        }
        let rtl = p.env().direction.is_rtl();
        let border = match edge {
            BarEdge::Top => Rect::new(rect.x, rect.y, rect.w, hairline),
            BarEdge::Bottom => Rect::new(rect.x, rect.bottom() - hairline, rect.w, hairline),
            BarEdge::Start | BarEdge::End => {
                let left = (edge == BarEdge::Start) != rtl;
                let x = if left {
                    rect.x
                } else {
                    rect.right() - hairline
                };
                Rect::new(x, rect.y, hairline, rect.h)
            }
        };
        p.fill(border, None, line);
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
