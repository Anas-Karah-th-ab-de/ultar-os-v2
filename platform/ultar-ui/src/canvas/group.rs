//! Group opacity: a part of the surface is drawn at full strength and then faded towards
//! what lay there before, so overlapping shapes of one widget (a fill and its border) are
//! dimmed together instead of each on top of the other (DESKTOP-SPEC §3: a disabled widget
//! is the *whole* brick at 0.40, as CSS `opacity` on the artboards).

use super::blend::BPP;
use super::{Canvas, PixelFormat};
use crate::geom::PixelRect;

/// The pixels under a group, saved before it is drawn.
#[derive(Debug, Clone)]
pub struct Backdrop {
    pixels: Vec<u8>,
    area: PixelRect,
}

impl Backdrop {
    /// The saved area (inside the surface and the clip it was taken with).
    pub fn area(&self) -> PixelRect {
        self.area
    }
}

impl Canvas {
    /// Saves the pixels of `area` (cut to the surface and the current clip); `None` when
    /// nothing of it can be drawn.
    pub fn save_backdrop(&self, area: PixelRect) -> Option<Backdrop> {
        let area = area.intersect(&self.clip);
        if area.is_empty() {
            return None;
        }
        let row = area.width() as usize * BPP;
        let mut pixels = Vec::with_capacity(row * area.height() as usize);
        for y in area.y0..area.y1 {
            let start = (y as usize * self.width as usize + area.x0 as usize) * BPP;
            pixels.extend_from_slice(self.data.get(start..start + row)?);
        }
        Some(Backdrop { pixels, area })
    }

    /// Fades the saved area towards its backdrop: every pixel becomes
    /// `backdrop + (pixel − backdrop) × opacity` – the group drawn at `opacity`.
    pub fn fade_to_backdrop(&mut self, backdrop: &Backdrop, opacity: f32) {
        let area = backdrop.area;
        let keep = (opacity.clamp(0.0, 1.0) * 256.0).round() as i32;
        if keep >= 256 {
            return;
        }
        let row = area.width() as usize * BPP;
        let opaque = self.format == PixelFormat::Bgrx8;
        for (i, y) in (area.y0..area.y1).enumerate() {
            let Some(saved) = backdrop.pixels.get(i * row..(i + 1) * row) else {
                break;
            };
            let line = self.row_mut(y, area.x0, area.x1);
            for (k, (dst, below)) in line.iter_mut().zip(saved).enumerate() {
                if opaque && k % BPP == BPP - 1 {
                    continue;
                }
                let (top, base) = (i32::from(*dst), i32::from(*below));
                *dst = (base + (((top - base) * keep) >> 8)).clamp(0, 255) as u8;
            }
        }
        self.damage.add(area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::geom::Rect;
    use crate::tokens::{ColorRole, DARK};

    fn mixed(a: Color, b: Color, t: f32) -> u8 {
        (f32::from(a.r) + (f32::from(b.r) - f32::from(a.r)) * t) as u8
    }

    #[test]
    fn a_group_is_faded_as_one() {
        let (ground, line, text) = (
            DARK.get(ColorRole::Ground),
            DARK.get(ColorRole::Line),
            DARK.get(ColorRole::Text),
        );
        let mut canvas = Canvas::new(8, 8, PixelFormat::Rgba8).unwrap();
        canvas.clear(ground);
        let saved = canvas.save_backdrop(PixelRect::new(0, 0, 8, 8)).unwrap();
        // Two shapes on top of each other: the second must not show the first through it.
        canvas.fill_rrect(Rect::new(0.0, 0.0, 8.0, 8.0), 0.0, line);
        canvas.fill_rrect(Rect::new(0.0, 0.0, 4.0, 8.0), 0.0, text);
        canvas.fade_to_backdrop(&saved, 0.5);
        let red = |x| i32::from(canvas.pixel(x, 1).map_or(0, |c| c.r));
        assert!((red(1) - i32::from(mixed(ground, text, 0.5))).abs() <= 1);
        assert!((red(6) - i32::from(mixed(ground, line, 0.5))).abs() <= 1);
        assert_eq!(saved.area(), PixelRect::new(0, 0, 8, 8));
    }

    #[test]
    fn nothing_is_saved_outside_the_clip() {
        let (ground, text) = (DARK.get(ColorRole::Ground), DARK.get(ColorRole::Text));
        let mut canvas = Canvas::new(4, 4, PixelFormat::Bgrx8).unwrap();
        canvas.clear(ground);
        canvas.set_clip(PixelRect::new(0, 0, 2, 2));
        assert!(canvas.save_backdrop(PixelRect::new(2, 2, 4, 4)).is_none());
        let saved = canvas.save_backdrop(PixelRect::new(-5, -5, 9, 9)).unwrap();
        assert_eq!(saved.area(), PixelRect::new(0, 0, 2, 2));
        canvas.fill_rrect(Rect::new(0.0, 0.0, 4.0, 4.0), 0.0, text);
        canvas.fade_to_backdrop(&saved, 0.0);
        assert_eq!(canvas.pixel(0, 0), Some(ground));
        canvas.fade_to_backdrop(&saved, 1.0);
        assert_eq!(canvas.pixel(0, 0), Some(ground));
    }
}
