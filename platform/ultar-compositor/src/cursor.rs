//! Images the compositor draws itself: the pointer and the recording
//! indicator (a dot at the top end corner of every output while the
//! screen is captured; apps cannot draw over it or hide it).

use crate::geom::Rect;
use crate::output::Output;
use crate::pixels::{self, Image};
use ultar_ui::tokens::{self, Space};

/// Edge of the pointer image in logical points (token `size.icon`).
pub const CURSOR: i32 = tokens::size::ICON as i32;

/// The logical area the pointer image covers at a position.
pub fn cursor_rect(x: f64, y: f64) -> Rect {
    Rect::new(
        x.floor() as i32 - 1,
        y.floor() as i32 - 1,
        CURSOR + 2,
        CURSOR + 2,
    )
}

/// Arrow outline in units of 1/24 of the cursor edge (the classic pointer).
const ARROW: [(f32, f32); 7] = [
    (1.0, 1.0),
    (1.0, 18.0),
    (5.0, 14.0),
    (8.0, 21.0),
    (11.0, 20.0),
    (8.0, 13.0),
    (13.0, 13.0),
];

fn inside(poly: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut odd = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            odd = !odd;
        }
        j = i;
    }
    odd
}

/// Draws the pointer at a scale: fill in the theme's text colour, outline
/// in its ground colour.
pub fn cursor_image(scale: f64, fill: u32, outline: u32) -> Option<Image> {
    let edge = ((f64::from(CURSOR) * scale).ceil() as u32).max(1);
    let mut img = Image::new(edge, edge, 0)?;
    let unit = edge as f32 / 24.0;
    let width = edge as usize;
    for y in 0..edge {
        for x in 0..edge {
            let (mut core, mut any) = (0u32, 0u32);
            for s in 0..4 {
                let px = (x as f32 + (s % 2) as f32 * 0.5 + 0.25) / unit;
                let py = (y as f32 + (s / 2) as f32 * 0.5 + 0.25) / unit;
                let near = [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)]
                    .iter()
                    .any(|(dx, dy)| inside(&ARROW, px + dx, py + dy));
                core += u32::from(
                    inside(&ARROW, px, py)
                        && [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)]
                            .iter()
                            .all(|(dx, dy)| inside(&ARROW, px + dx * 0.9, py + dy * 0.9)),
                );
                any += u32::from(inside(&ARROW, px, py) || near);
            }
            let px = if core > 0 {
                pixels::over(scaled(outline, any * 64), scaled(fill, core * 64))
            } else {
                scaled(outline, any * 64)
            };
            img.data_mut()[y as usize * width + x as usize] = px;
        }
    }
    Some(img)
}

fn scaled(color: u32, amount: u32) -> u32 {
    let a = amount.min(255);
    let ch = |shift: u32| (((color >> shift) & 0xff) * a / 255) << shift;
    ch(24) | ch(16) | ch(8) | ch(0)
}

/// Edge of the recording indicator in logical points.
const INDICATOR: i32 = Space::S12.points() as i32;

/// Where the recording indicator is drawn on an output (logical): its top
/// end corner (top left in right-to-left languages).
pub fn indicator_rect(o: &Output, rtl: bool) -> Rect {
    let r = o.logical_rect();
    let margin = Space::S8.points() as i32;
    let x = if rtl {
        r.x + margin
    } else {
        r.max_x() - margin - INDICATOR
    };
    Rect::new(x, r.y + margin, INDICATOR, INDICATOR)
}

/// A filled dot of `color` (premultiplied) with a soft edge.
pub fn dot_image(edge: u32, color: u32) -> Option<Image> {
    let mut img = Image::new(edge, edge, 0)?;
    let c = edge as f32 / 2.0;
    for y in 0..edge {
        for x in 0..edge {
            let d = ((x as f32 + 0.5 - c).powi(2) + (y as f32 + 0.5 - c).powi(2)).sqrt();
            let cover = (c - d + 0.5).clamp(0.0, 1.0);
            img.data_mut()[(y * edge + x) as usize] = scaled(color, (cover * 255.0) as u32);
        }
    }
    Some(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_is_drawn_with_outline() {
        let img = cursor_image(1.0, 0xffff_ffff, 0xff00_0000).unwrap();
        assert_eq!(img.width(), 24);
        assert_eq!(
            img.pixel(23, 23),
            Some(0),
            "outside the arrow is transparent"
        );
        let body = img.pixel(3, 10).unwrap();
        assert_eq!(body >> 24, 0xff);
        assert!(body & 0xff > 0x80, "inside is the fill colour");
        let big = cursor_image(2.0, 0xffff_ffff, 0xff00_0000).unwrap();
        assert_eq!(big.width(), 48);
        assert_eq!(cursor_rect(10.5, 20.2), Rect::new(9, 19, 26, 26));
        assert!(inside(&ARROW, 2.0, 10.0));
        assert!(!inside(&ARROW, 20.0, 2.0));
        assert_eq!(scaled(0xff80_4020, 128), 0x8040_2010);
    }
}
