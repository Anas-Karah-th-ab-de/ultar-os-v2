//! Cards and tiles of a radio group (boards: padding 12, radius 12, 12 apart; chosen: `hover`
//! fill and a two-border `accent_text` edge; not available: a `line` edge, the title in
//! `control_border`, the reason as the second line).

use super::{line, RadioGroup, RadioLayout, DOT, GAP, TITLE, UNIT};
use crate::geom::{Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::{Painter, TextSpec};
use crate::text::TextAlign;
use crate::tokens::{size, typography, ColorRole, Radius, Space, TypeSize};
use crate::widgets::style::{self, Visual};
use crate::widgets::{Label, MeasureCx, PaintCx};

/// Inner spacing of a card.
const PAD: Space = Space::S12;
/// Space above the split bar of a card (boards: 8).
const SPLIT_GAP: Space = Space::S8;
/// Space between the lines of a tile.
const TILE_GAP: Space = Space::S4;
/// Inner spacing of a compact choice (inline, block).
const CHOICE_PAD: (Space, Space) = (Space::S12, Space::S4);
/// Smallest height of a compact choice (board: `space.48`).
const CHOICE_MIN: Space = Space::S48;

impl RadioGroup {
    /// The second line of option `index`: the reason it is not available, else its detail.
    fn second_line(&self, index: usize) -> Option<&Label> {
        self.reason(index)
            .or_else(|| self.details.get(index).and_then(Option::as_ref))
    }

    /// Inner spacing (inline, block) of a card of this layout.
    fn pads(&self) -> (f32, f32) {
        match self.layout {
            RadioLayout::Choices => (CHOICE_PAD.0.points(), CHOICE_PAD.1.points()),
            _ => (PAD.points(), PAD.points()),
        }
    }

    fn picture_of(&self, index: usize) -> Option<crate::widgets::Pictogram> {
        self.pictograms.get(index).copied().flatten()
    }

    /// The pictogram of tile `index`, 48 × 24 centred at the top of `area`; returns the
    /// area below it.
    fn paint_picture(
        &self,
        p: &mut Painter<'_>,
        area: Rect,
        index: usize,
        color: crate::color::Color,
    ) -> Rect {
        use crate::widgets::Pictogram;
        let Some(kind) = self.picture_of(index) else {
            return area;
        };
        let h = size::ICON;
        let box_ = style::centered(Rect::new(area.x, area.y, area.w, h), Size::new(2.0 * h, h));
        let stroke = size::STROKE;
        let screen = |x: f32| Rect::new(box_.x + x, box_.y + 4.0, 20.0, 16.0);
        let (left, right) = (screen(2.0), screen(26.0));
        let draw = |p: &mut Painter<'_>, r: Rect, mode: u8| match mode {
            1 => p.fill(r, None, color),
            2 => {
                // dashed outline: the four sides in 2-point dashes
                let mut x = r.x;
                while x < r.right() {
                    let w = 2.0f32.min(r.right() - x);
                    p.fill(Rect::new(x, r.y, w, stroke), None, color);
                    p.fill(Rect::new(x, r.bottom() - stroke, w, stroke), None, color);
                    x += 4.0;
                }
                let mut y = r.y;
                while y < r.bottom() {
                    let hh = 2.0f32.min(r.bottom() - y);
                    p.fill(Rect::new(r.x, y, stroke, hh), None, color);
                    p.fill(Rect::new(r.right() - stroke, y, stroke, hh), None, color);
                    y += 4.0;
                }
            }
            _ => p.stroke(r, None, stroke, color),
        };
        let (a, b) = match kind {
            Pictogram::Extend => (0, 0),
            Pictogram::Mirror => (1, 1),
            Pictogram::First => (1, 2),
            Pictogram::Second => (2, 1),
        };
        draw(p, left, a);
        draw(p, right, b);
        if kind == Pictogram::Extend {
            let y = box_.y + h / 2.0;
            p.line(
                crate::geom::Point::new(box_.x + 16.0, y),
                crate::geom::Point::new(box_.x + 32.0, y),
                stroke,
                color,
            );
        }
        let gap = h + TILE_GAP.points();
        Rect::new(area.x, area.y + gap, area.w, (area.h - gap).max(0.0))
    }

    fn split_of(&self, index: usize) -> Option<&Vec<(Label, u32)>> {
        self.splits.get(index).and_then(Option::as_ref)
    }

    /// The split bar of card `index` in `area` (24 high, control radius, `control_border`
    /// edge; parts in `raised`, the last in `hover` with a two-point `accent_text` start edge).
    fn paint_split(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, area: Rect, index: usize) {
        let Some(parts) = self.split_of(index) else {
            return;
        };
        let total: u64 = parts.iter().map(|(_, w)| u64::from(*w)).sum::<u64>().max(1);
        let rtl = p.env().direction.is_rtl();
        let (border, ring) = (p.hairline(), 2.0 * size::BORDER);
        let mut start = 0.0f32;
        let scale = p.env().scale;
        for (i, (label, weight)) in parts.iter().enumerate() {
            let w = area.w * (*weight as f32) / total as f32;
            let x = if rtl {
                area.right() - start - w
            } else {
                area.x + start
            };
            let seg = Rect::new(x, area.y, w, area.h);
            let last = i + 1 == parts.len();
            let fill = p.color(if last {
                ColorRole::Hover
            } else {
                ColorRole::Raised
            });
            p.fill(seg, None, fill);
            if last && i > 0 {
                let edge_x = if rtl { seg.right() - ring } else { seg.x };
                let accent = p.color(ColorRole::AccentText);
                p.fill(Rect::new(edge_x, seg.y, ring, seg.h), None, accent);
            }
            let text = label.resolve(cx.tr);
            let layout = p.layout(&text, TextSpec::new(TypeSize::S12), Some(w));
            let h = layout.height / scale;
            let color = p.color(ColorRole::Text);
            let at = Rect::new(seg.x, seg.y + (seg.h - h) / 2.0, seg.w, h);
            p.text(&layout, at, TextAlign::Center, color);
            start += w;
        }
        let edge = p.color(ColorRole::ControlBorder);
        p.stroke(area, Some(Radius::Control), border, edge);
    }

    /// Width the text of a card or tile has inside a card of `width`.
    fn text_width(&self, width: f32) -> f32 {
        let lead = match self.layout {
            RadioLayout::Tiles => 0.0,
            RadioLayout::Choices => size::ICON_SMALL + Space::S8.points(),
            _ => DOT + GAP.points(),
        };
        if self.layout == RadioLayout::Choices {
            return (width - 2.0 * self.pads().0 - lead).max(0.0);
        }
        (width - 2.0 * (PAD.points() + size::BORDER) - lead).max(0.0)
    }

    /// Width of the edge of card `index` (boards: the chosen card has a two-border edge, the
    /// others one; CSS border-box, so the edge is inside the card and around the padding).
    fn edge(&self, index: usize) -> f32 {
        if self.selected == Some(index) {
            2.0 * size::BORDER
        } else {
            size::BORDER
        }
    }

    /// Height of the card of option `index` at `width`.
    fn card_height(&self, cx: &mut MeasureCx<'_>, index: usize, width: f32) -> f32 {
        let mut text = line(cx, typography::BODY);
        if let Some(second) = self.second_line(index) {
            let spec = style::SMALL_TEXT;
            let lines = cx.text_size(second, spec, Some(self.text_width(width))).h;
            text += lines.max(line(cx, TypeSize::S12));
            if self.layout == RadioLayout::Tiles {
                text += TILE_GAP.points();
            }
        }
        if self.split_of(index).is_some() {
            text += SPLIT_GAP.points() + size::ICON + 2.0 * size::BORDER;
        }
        if self.picture_of(index).is_some() {
            text += size::ICON + TILE_GAP.points();
        }
        match self.layout {
            // Boards: border-box with the two-border edge of the chosen choice.
            RadioLayout::Choices => {
                let edge = 4.0 * size::BORDER;
                (text + 2.0 * self.pads().1 + edge).max(CHOICE_MIN.points())
            }
            RadioLayout::Tiles => text + 2.0 * (PAD.points() + self.edge(index)),
            _ => text.max(DOT) + 2.0 * (PAD.points() + self.edge(index)),
        }
    }

    pub(super) fn measure_cards(&mut self, cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        let count = self.options.len();
        let gap = GAP.points();
        let row = matches!(
            self.layout,
            RadioLayout::CardRow | RadioLayout::Tiles | RadioLayout::Choices
        );
        let width = if row && count > 0 {
            ((avail - gap * (count as f32 - 1.0)) / count as f32).max(0.0)
        } else {
            avail
        };
        let heights: Vec<f32> = (0..count).map(|i| self.card_height(cx, i, width)).collect();
        let tallest = heights.iter().copied().fold(0.0f32, f32::max);
        let fixed = |h: f32| (h * UNIT).round().max(0.0) as u32;
        self.heights = if row {
            vec![fixed(tallest); count]
        } else {
            heights.iter().map(|h| fixed(*h)).collect()
        };
        let total = if row {
            tallest
        } else {
            heights.iter().sum::<f32>() + gap * count.saturating_sub(1) as f32
        };
        Size::new(avail, total)
    }

    /// One card or tile.
    pub(super) fn paint_card(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        card: Rect,
        index: usize,
        v: &Visual,
    ) {
        let (chosen, off) = (self.selected == Some(index), self.is_unavailable(index));
        if self.layout == RadioLayout::Choices {
            self.paint_choice(p, cx, card, index, v);
            return;
        }
        let (fill, edge, width) = match (chosen, off) {
            (_, true) => (None, ColorRole::Line, p.hairline()),
            (true, false) => (
                Some(ColorRole::Hover),
                ColorRole::AccentText,
                2.0 * p.hairline(),
            ),
            (false, false) => {
                let fill = if v.hovered || v.pressed {
                    style::neutral_background(v)
                } else {
                    ColorRole::Surface
                };
                (Some(fill), ColorRole::ControlBorder, p.hairline())
            }
        };
        if let Some(fill) = fill {
            let color = p.color(fill);
            p.fill(card, Some(Radius::Card), color);
        }
        let edge = p.color(edge);
        p.stroke(card, Some(Radius::Card), width, edge);
        self.paint_inside(p, cx, card, index, (v, chosen, off));
        if v.focused {
            style::focus_ring(p, card, Radius::Card);
        }
    }

    /// Everything inside the edge of card `index`: dot, pictogram, texts, split bar.
    fn paint_inside(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        card: Rect,
        index: usize,
        (v, chosen, off): (&Visual, bool, bool),
    ) {
        let pad = PAD.points() + self.edge(index);
        let inner = Rect::new(
            card.x + pad,
            card.y + pad,
            (card.w - 2.0 * pad).max(0.0),
            (card.h - 2.0 * pad).max(0.0),
        );
        if self.layout == RadioLayout::Tiles {
            let ink = p.color(if off {
                ColorRole::ControlBorder
            } else {
                ColorRole::Text
            });
            let inner = self.paint_picture(p, inner, index, ink);
            self.paint_tile_text(p, cx, inner, index, (chosen, off));
        } else {
            let direction = p.env().direction;
            let mut text = inner;
            let slot = style::take_start(&mut text, DOT, direction);
            style::take_start(&mut text, GAP.points(), direction);
            let dot = Rect::new(slot.x, slot.y, DOT, DOT);
            self.paint_dot(p, dot, v, (chosen, off));
            self.paint_lines(p, cx, text, index, (TextAlign::Start, off));
            if self.split_of(index).is_some() {
                let h = size::ICON + 2.0 * size::BORDER;
                let bar = Rect::new(text.x, text.bottom() - h, text.w, h);
                self.paint_split(p, cx, bar, index);
            }
        }
    }

    /// Title and second line from the top of `area`.
    fn paint_lines(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        area: Rect,
        index: usize,
        (align, off): (TextAlign, bool),
    ) {
        let scale = p.env().scale;
        let title_role = if off {
            ColorRole::ControlBorder
        } else {
            ColorRole::Text
        };
        let title_text = self.options[index].resolve(cx.tr);
        let title = p.layout(&title_text, TITLE, Some(area.w));
        let title_h = title.height / scale;
        let color = p.color(title_role);
        p.text(
            &title,
            Rect::new(area.x, area.y, area.w, title_h),
            align,
            color,
        );
        let bottom = area.y + title_h;
        if let Some(second) = self.second_line(index) {
            let gap = if self.layout == RadioLayout::Tiles {
                TILE_GAP.points()
            } else {
                0.0
            };
            let text = second.resolve(cx.tr);
            let layout = p.layout(&text, style::SMALL_TEXT, Some(area.w));
            let h = layout.height / scale;
            let muted = p.color(ColorRole::TextMuted);
            p.text(
                &layout,
                Rect::new(area.x, bottom + gap, area.w, h),
                align,
                muted,
            );
        }
    }

    /// A tile: the title centred, with a check mark before it when chosen.
    fn paint_tile_text(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        area: Rect,
        index: usize,
        (chosen, off): (bool, bool),
    ) {
        if !chosen {
            self.paint_lines(p, cx, area, index, (TextAlign::Center, off));
            return;
        }
        // The check (16) and 4 points sit before the title; both are centred together.
        let text = self.options[index].resolve(cx.tr);
        let title = p.layout(&text, TITLE, Some(area.w));
        let width = title.width / p.env().scale;
        let mark = size::ICON_SMALL + TILE_GAP.points();
        let line_h = title.height / p.env().scale;
        let mut row = style::centered(
            Rect::new(area.x, area.y, area.w, line_h),
            Size::new((width + mark).min(area.w), line_h),
        );
        let direction = p.env().direction;
        let slot = style::take_start(&mut row, size::ICON_SMALL, direction);
        let color = p.color(ColorRole::Text);
        p.icon(
            IconId::Check,
            slot,
            size::ICON_SMALL,
            QuarterTurns::None,
            color,
        );
        style::take_start(&mut row, TILE_GAP.points(), direction);
        p.text(&title, row, TextAlign::Start, color);
        let below = Rect::new(area.x, area.y + line_h, area.w, (area.h - line_h).max(0.0));
        if let Some(second) = self.second_line(index) {
            let text = second.resolve(cx.tr);
            let layout = p.layout(&text, style::SMALL_TEXT, Some(area.w));
            let h = layout.height / p.env().scale;
            let muted = p.color(ColorRole::TextMuted);
            let at = Rect::new(below.x, below.y + TILE_GAP.points(), below.w, h);
            p.text(&layout, at, TextAlign::Center, muted);
        }
    }

    /// A compact choice: check mark (when chosen) and the two lines from the start edge.
    fn paint_choice(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        card: Rect,
        index: usize,
        v: &Visual,
    ) {
        let (chosen, off) = (self.selected == Some(index), self.is_unavailable(index));
        let hair = p.hairline();
        let (fill, edge, width) = match (chosen, off) {
            (_, true) => (None, ColorRole::Line, hair),
            (true, false) => (Some(ColorRole::Hover), ColorRole::AccentText, 2.0 * hair),
            (false, false) => {
                let fill = (v.hovered || v.pressed).then(|| style::neutral_background(v));
                (fill, ColorRole::ControlBorder, hair)
            }
        };
        if let Some(fill) = fill {
            let color = p.color(fill);
            p.fill(card, Some(Radius::Control), color);
        }
        let color = p.color(edge);
        p.stroke(card, Some(Radius::Control), width, color);
        let (pi, pb) = self.pads();
        let mut inner = Rect::new(
            card.x + pi,
            card.y + pb,
            (card.w - 2.0 * pi).max(0.0),
            (card.h - 2.0 * pb).max(0.0),
        );
        let direction = p.env().direction;
        let slot = style::take_start(&mut inner, size::ICON_SMALL, direction);
        style::take_start(&mut inner, Space::S8.points(), direction);
        if chosen {
            let mark = style::centered(slot, Size::new(size::ICON_SMALL, size::ICON_SMALL));
            let text = p.color(ColorRole::Text);
            p.icon(
                IconId::Check,
                mark,
                size::ICON_SMALL,
                QuarterTurns::None,
                text,
            );
        }
        let lines = self.lines_height(p, cx, index, inner.w);
        let top = inner.y + ((inner.h - lines) / 2.0).max(0.0);
        let area = Rect::new(inner.x, top, inner.w, lines);
        self.paint_lines(p, cx, area, index, (TextAlign::Start, off));
        if v.focused {
            style::focus_ring(p, card, Radius::Control);
        }
    }

    /// Height of the two lines of option `index` at `width` (painting scale).
    fn lines_height(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, index: usize, width: f32) -> f32 {
        let scale = p.env().scale;
        let title = self.options[index].resolve(cx.tr);
        let mut h = p.layout(&title, TITLE, Some(width)).height / scale;
        if let Some(second) = self.second_line(index) {
            let text = second.resolve(cx.tr);
            h += p.layout(&text, style::SMALL_TEXT, Some(width)).height / scale;
        }
        h
    }
}
