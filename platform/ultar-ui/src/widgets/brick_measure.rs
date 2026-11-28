//! Natural sizes of the shell bricks (split from `brick.rs`).

use super::brick::{Brick, BrickKind, BODY, MORE_WIDTH, SMALL, SMALL_BOLD, STRONG, TARGET, TOUCH};
use super::brick_overview;
use super::{brick_phone, MeasureCx};
use crate::geom::Size;
use crate::tokens::{size, Space};

impl Brick {
    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>) -> Size {
        let pad = Space::S8.points();
        match &self.kind {
            BrickKind::Tile { .. } | BrickKind::Square { .. } => Size::new(TOUCH, TOUCH),
            BrickKind::AppCell { .. } => Size::new(TOUCH + 2.0 * Space::S16.points(), 2.0 * TOUCH),
            BrickKind::PhoneCell { .. } => Size::new(
                brick_phone::CELL_ICON_BOX + Space::S8.points(),
                brick_phone::CELL_HEIGHT,
            ),
            BrickKind::BarButton { pad, weight, .. } => {
                let text = cx.text_size(&self.label, BODY.weight(*weight), None);
                Size::new(text.w + 2.0 * pad.points(), TARGET)
            }
            BrickKind::PhoneStatus { icons, level, .. } => {
                let gap = brick_phone::STATUS_GAP;
                let text = level
                    .as_ref()
                    .map_or(0.0, |l| cx.text_size(l, brick_phone::LEVEL, None).w);
                let n = icons.len() as f32;
                Size::new(2.0 * pad + n * (size::ICON_SMALL + gap) + text, TARGET)
            }
            BrickKind::SearchPill { width } => Size::new(*width, TARGET),
            BrickKind::Status { icons, .. } => {
                let n = icons.len().max(1) as f32;
                let gap = Space::S12.points();
                Size::new(2.0 * pad + n * size::ICON_SMALL + (n - 1.0) * gap, TARGET)
            }
            BrickKind::Chip => {
                let text = cx.text_size(&self.label, SMALL_BOLD, None);
                Size::new((text.w + 2.0 * pad).max(TARGET), TARGET)
            }
            BrickKind::Clock { date, .. } => {
                let time = cx.text_size(&self.label, SMALL_BOLD, None);
                let day = cx.text_size(date, SMALL, None);
                Size::new(time.w.max(day.w) + 2.0 * pad, TOUCH)
            }
            BrickKind::QuickTile { .. } => {
                let text = cx.text_size(&self.label, STRONG, None);
                let w = 2.0 * Space::S12.points() + size::ICON + Space::S12.points() + text.w;
                Size::new(w, size::BAR_HEIGHT)
            }
            BrickKind::QuickMore { .. } => Size::new(MORE_WIDTH, size::BAR_HEIGHT),
            BrickKind::ModeCard { .. } => {
                let text = cx.text_size(&self.label, STRONG, None);
                Size::new(
                    text.w + 2.0 * Space::S24.points(),
                    36.0 * Space::S4.points(),
                )
            }
            // the overview's cards, the pictures, the rule, the scrim and the handle
            _ => brick_overview::measure_kind(cx, self),
        }
    }
}
