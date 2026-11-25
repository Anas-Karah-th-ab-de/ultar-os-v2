//! The dot-matrix clock of the home and lock screen ("Ultar Noir"): every digit is 5 × 7
//! round dots, the colon two dots, drawn in the text colour on the screen ground.

use ultar_ui::canvas::Canvas;
use ultar_ui::geom::Rect;
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::ColorRole;
use ultar_ui::widgets::{Label, PixelArea};

/// Dot columns of a digit.
const DIGIT_W: usize = 5;
/// Dot rows of every glyph.
pub const ROWS: usize = 7;

/// The rows of a digit, most significant bit = first column.
fn digit(c: char) -> Option<[u8; ROWS]> {
    Some(match c {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        _ => return None,
    })
}

/// Dot columns of `time` (digits 5, colon 1, one empty column between glyphs).
pub fn columns(time: &str) -> usize {
    let glyphs: Vec<usize> = time
        .chars()
        .map(|c| if c == ':' { 1 } else { DIGIT_W })
        .collect();
    glyphs.iter().sum::<usize>() + glyphs.len().saturating_sub(1)
}

/// The picture node of a clock `height` points high (its width follows the time).
pub fn area(time: &str, height: f32) -> (PixelArea, f32) {
    let pitch = height / ROWS as f32;
    let width = pitch * columns(time) as f32;
    let name = Label::data(time);
    (PixelArea::new(name), width)
}

/// Paints `time` over the whole canvas: dots in the text colour, the rest the ground.
pub fn paint(canvas: &mut Canvas, time: &str, env: &PaintEnv) {
    canvas.clear(env.palette().get(ColorRole::Ground));
    let cols = columns(time).max(1) as f32;
    let pitch = (canvas.width() as f32 / cols).min(canvas.height() as f32 / ROWS as f32);
    let dot = pitch * 0.78;
    let color = env.palette().get(ColorRole::Text);
    let mut x0 = 0usize;
    for c in time.chars() {
        let rows: [u8; ROWS] = if c == ':' {
            [0, 0, 1, 0, 1, 0, 0]
        } else {
            digit(c).unwrap_or([0; ROWS])
        };
        let width = if c == ':' { 1 } else { DIGIT_W };
        for (row, bits) in rows.iter().enumerate() {
            for col in 0..width {
                let bit = width - 1 - col;
                if bits >> bit & 1 == 1 {
                    let x = (x0 + col) as f32 * pitch + (pitch - dot) / 2.0;
                    let y = row as f32 * pitch + (pitch - dot) / 2.0;
                    canvas.fill_rrect(Rect::new(x, y, dot, dot), dot / 2.0, color);
                }
            }
        }
        x0 += width + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_ui::canvas::PixelFormat;
    use ultar_ui::tokens::ThemeKind;
    use ultar_ui::Lang;

    #[test]
    fn a_time_has_its_columns() {
        assert_eq!(columns("19:56"), 4 * 5 + 1 + 4);
        assert_eq!(columns(""), 0);
    }

    #[test]
    fn digits_are_drawn_in_the_text_colour() {
        let env = PaintEnv::new(Lang::En, ThemeKind::Dark);
        let mut c = Canvas::new(250, 70, PixelFormat::Bgrx8).unwrap();
        paint(&mut c, "18:88", &env);
        let text = env.palette().get(ColorRole::Text);
        let ground = env.palette().get(ColorRole::Ground);
        // The first column of "1" is empty in its top row; "8" fills its second row's edges.
        let lit = (0..250)
            .flat_map(|x| (0..70).map(move |y| (x, y)))
            .filter(|(x, y)| c.pixel(*x, *y).is_some_and(|p| p == text))
            .count();
        assert!(lit > 500, "lit {lit}");
        assert_eq!(c.pixel(0, 0), Some(ground));
    }
}
