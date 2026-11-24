//! Cards and tiles of a radio group (boards: padding 12, radius 12, 12 apart; chosen: `hover`
//! fill and a two-border `accent_text` edge; not available: a `line` edge, the title in
//! `control_border`, the reason as the second line).

use super::{line, RadioGroup, RadioLayout, DOT, GAP, TITLE, UNIT};
use crate::geom::{Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::Painter;
use crate::text::TextAlign;
use crate::tokens::{size, typography, ColorRole, Radius, Space, TypeSize};
use crate::widgets::style::{self, Visual};
use crate::widgets::{Label, MeasureCx, PaintCx};

/// Inner spacing of a card.
const PAD: Space = Space::S12;
/// Space between the lines of a tile.
const TILE_GAP: Space = Space::S4;

impl RadioGroup {
    /// The second line of option `index`: the reason it is not available, else its detail.
    fn second_line(&self, index: usize) -> Option<&Label> {
        self.reason(index)
            .or_else(|| self.details.get(index).and_then(Option::as_ref))
    }

    /// Width the text of a card or tile has inside a card of `width`.
    fn text_width(&self, width: f32) -> f32 {
        let dot = if self.layout == RadioLayout::Tiles {
            0.0
        } else {
            DOT + GAP.points()
        };
        (width - 2.0 * PAD.points() - dot).max(0.0)
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
        let content = if self.layout == RadioLayout::Tiles {
            text
        } else {
            text.max(DOT)
        };
        content + 2.0 * PAD.points()
    }

    pub(super) fn measure_cards(&mut self, cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        let count = self.options.len();
        let gap = GAP.points();
        let row = matches!(self.layout, RadioLayout::CardRow | RadioLayout::Tiles);
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
        let inner = Rect::new(
            card.x + PAD.points(),
            card.y + PAD.points(),
            (card.w - 2.0 * PAD.points()).max(0.0),
            (card.h - 2.0 * PAD.points()).max(0.0),
        );
        if self.layout == RadioLayout::Tiles {
            self.paint_tile_text(p, cx, inner, index, (chosen, off));
        } else {
            let direction = p.env().direction;
            let mut text = inner;
            let slot = style::take_start(&mut text, DOT, direction);
            style::take_start(&mut text, GAP.points(), direction);
            let dot = Rect::new(slot.x, slot.y, DOT, DOT);
            self.paint_dot(p, dot, v, (chosen, off));
            self.paint_lines(p, cx, text, index, (TextAlign::Start, off));
        }
        if v.focused {
            style::focus_ring(p, card, Radius::Card);
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
}
