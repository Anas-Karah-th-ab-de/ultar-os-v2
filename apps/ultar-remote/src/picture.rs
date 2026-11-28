//! The remote screen as the viewer holds it: a frame buffer that receives the pieces, the
//! scaling into the window (the whole screen always visible, bars around it) and the way back
//! for the pointer (a position in the window is a position on the remote screen).

/// Bytes of a pixel (blue, green, red, unused).
pub const BPP: usize = 4;
/// Largest side the viewer accepts (the same as the mesh).
pub const MAX_SIDE: u32 = 8192;
/// Most pixels of one remote screen (about 130 MB of memory).
pub const MAX_PIXELS: u64 = 32 * 1024 * 1024;
/// Colour of the bars around a picture that does not fill the window.
const BAR: u8 = 18;

/// How the picture is scaled into the window.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Quality {
    /// Bilinear: no staircase when the window is smaller or larger than the screen.
    #[default]
    Smooth,
    /// Nearest pixel: every remote pixel stays a sharp square.
    Sharp,
}

/// The frame buffer of the remote screen.
pub struct Remote {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Remote {
    /// A black screen of this size; `None` for a size the viewer will not hold.
    pub fn new(width: u32, height: u32) -> Option<Remote> {
        let ok = (1..=MAX_SIDE).contains(&width)
            && (1..=MAX_SIDE).contains(&height)
            && u64::from(width) * u64::from(height) <= MAX_PIXELS;
        ok.then(|| Remote {
            width,
            height,
            data: vec![0; width as usize * height as usize * BPP],
        })
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, blue-green-red-unused, row after row.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Writes a piece (`w` x `h` pixels at `x`, `y`); `false` (nothing written) when it
    /// does not lie inside the screen or its data has the wrong length.
    pub fn write(&mut self, x: u32, y: u32, w: u32, h: u32, data: &[u8]) -> bool {
        let inside = w > 0
            && h > 0
            && x.checked_add(w).is_some_and(|e| e <= self.width)
            && y.checked_add(h).is_some_and(|e| e <= self.height);
        if !inside || data.len() != w as usize * h as usize * BPP {
            return false;
        }
        let row = w as usize * BPP;
        for r in 0..h as usize {
            let at = ((y as usize + r) * self.width as usize + x as usize) * BPP;
            self.data[at..at + row].copy_from_slice(&data[r * row..(r + 1) * row]);
        }
        true
    }
}

/// Where the screen sits inside an area of the window (pixels of the window).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Fit {
    /// Screen pixel to window pixel factor.
    pub scale: f32,
    /// Left edge of the picture in the area.
    pub x: u32,
    /// Top edge of the picture in the area.
    pub y: u32,
    /// Width of the picture in the area.
    pub w: u32,
    /// Height of the picture in the area.
    pub h: u32,
    /// Size of the remote screen.
    pub screen: (u32, u32),
}

/// The largest placement of a `screen` inside `area` that keeps the proportions, centred.
pub fn fit(screen: (u32, u32), area: (u32, u32)) -> Fit {
    let (sw, sh) = (screen.0.max(1), screen.1.max(1));
    let (aw, ah) = (area.0.max(1), area.1.max(1));
    let scale = (aw as f32 / sw as f32).min(ah as f32 / sh as f32);
    let w = ((sw as f32 * scale).round() as u32).clamp(1, aw);
    let h = ((sh as f32 * scale).round() as u32).clamp(1, ah);
    Fit {
        scale,
        x: (aw - w) / 2,
        y: (ah - h) / 2,
        w,
        h,
        screen: (sw, sh),
    }
}

impl Fit {
    /// The remote position under window pixel (`px`, `py`) of the area; `None` on the bars.
    pub fn to_remote(&self, px: f32, py: f32) -> Option<(u32, u32)> {
        let (fx, fy) = (px - self.x as f32, py - self.y as f32);
        if fx < 0.0 || fy < 0.0 || fx >= self.w as f32 || fy >= self.h as f32 {
            return None;
        }
        Some(self.clamped(px, py))
    }

    /// The nearest remote position (a drag that leaves the picture stays on its edge).
    pub fn clamped(&self, px: f32, py: f32) -> (u32, u32) {
        let map = |p: f32, from: u32, size: u32, screen: u32| {
            let local = ((p - from as f32) / size.max(1) as f32).clamp(0.0, 1.0);
            ((local * screen as f32) as u32).min(screen - 1)
        };
        (
            map(px, self.x, self.w, self.screen.0),
            map(py, self.y, self.h, self.screen.1),
        )
    }
}

/// The remote screen scaled into an area of `area` pixels: blue-green-red-unused rows.
pub fn render(remote: &Remote, area: (u32, u32), quality: Quality) -> Vec<u8> {
    let (aw, ah) = (area.0.max(1), area.1.max(1));
    let mut out = vec![BAR; aw as usize * ah as usize * BPP];
    let f = fit((remote.width, remote.height), (aw, ah));
    let (sw, sh) = (remote.width as usize, remote.height as usize);
    let src = remote.data();
    for dy in 0..f.h as usize {
        let row = ((f.y as usize + dy) * aw as usize + f.x as usize) * BPP;
        match quality {
            Quality::Sharp => {
                let sy = (dy * sh / f.h as usize).min(sh - 1);
                for dx in 0..f.w as usize {
                    let sx = (dx * sw / f.w as usize).min(sw - 1);
                    let s = (sy * sw + sx) * BPP;
                    out[row + dx * BPP..row + dx * BPP + BPP].copy_from_slice(&src[s..s + BPP]);
                }
            }
            Quality::Smooth => {
                let fy =
                    ((dy as f32 + 0.5) * sh as f32 / f.h as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
                let (y0, ty) = (fy as usize, fy.fract());
                let y1 = (y0 + 1).min(sh - 1);
                for dx in 0..f.w as usize {
                    let fx = ((dx as f32 + 0.5) * sw as f32 / f.w as f32 - 0.5)
                        .clamp(0.0, (sw - 1) as f32);
                    let (x0, tx) = (fx as usize, fx.fract());
                    let x1 = (x0 + 1).min(sw - 1);
                    for c in 0..3 {
                        let at = |x: usize, y: usize| f32::from(src[(y * sw + x) * BPP + c]);
                        let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
                        let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
                        out[row + dx * BPP + c] = (top * (1.0 - ty) + bottom * ty).round() as u8;
                    }
                    out[row + dx * BPP + 3] = 255;
                }
            }
        }
    }
    for px in out.chunks_exact_mut(BPP) {
        px[3] = 255;
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, v: u8) -> Remote {
        let mut r = Remote::new(w, h).unwrap();
        assert!(r.write(0, 0, w, h, &vec![v; (w * h) as usize * BPP]));
        r
    }

    #[test]
    fn pieces_land_where_they_belong_and_bad_pieces_are_refused() {
        let mut r = Remote::new(4, 3).unwrap();
        assert!(r.write(1, 1, 2, 1, &[7; 8]));
        assert_eq!(&r.data()[(4 + 1) * BPP..(4 + 3) * BPP], &[7; 8]);
        assert_eq!(r.data()[0], 0);
        assert!(!r.write(3, 0, 2, 1, &[1; 8]), "leaves the screen");
        assert!(!r.write(0, 0, 2, 1, &[1; 7]), "wrong length");
        assert!(!r.write(0, 0, 0, 1, &[]), "empty");
        assert!(!r.write(u32::MAX, 0, 2, 1, &[1; 8]), "overflow");
        assert!(Remote::new(0, 5).is_none());
        assert!(Remote::new(MAX_SIDE + 1, 5).is_none());
        assert!(Remote::new(MAX_SIDE, MAX_SIDE).is_none(), "too many pixels");
    }

    #[test]
    fn the_whole_screen_fits_with_bars_and_keeps_its_proportions() {
        let f = fit((1920, 1080), (800, 800));
        assert_eq!((f.w, f.h), (800, 450));
        assert_eq!((f.x, f.y), (0, 175));
        let f = fit((100, 100), (400, 200));
        assert_eq!((f.x, f.y, f.w, f.h), (100, 0, 200, 200));
        assert!(f.scale > 1.9 && f.scale < 2.1);
    }

    #[test]
    fn the_pointer_goes_back_to_the_remote_screen() {
        let f = fit((1000, 500), (500, 400));
        assert_eq!((f.w, f.h, f.y), (500, 250, 75));
        assert_eq!(f.to_remote(0.0, 75.0), Some((0, 0)));
        assert_eq!(f.to_remote(250.0, 200.0), Some((500, 250)));
        assert_eq!(f.to_remote(499.9, 324.9), Some((999, 499)));
        assert_eq!(f.to_remote(10.0, 10.0), None, "on the bar");
        assert_eq!(f.clamped(900.0, -5.0), (999, 0));
    }

    #[test]
    fn scaling_keeps_colours_and_paints_bars() {
        let r = solid(2, 2, 200);
        for q in [Quality::Smooth, Quality::Sharp] {
            let out = render(&r, (8, 4), q);
            assert_eq!(out.len(), 8 * 4 * BPP);
            // the picture is 4x4 in the middle: columns 2..6
            assert_eq!(out[(2 * 8 + 3) * BPP], 200);
            assert_eq!(out[(2 * 8 + 3) * BPP + 3], 255);
            assert_eq!(out[0], BAR, "bar left");
            assert_eq!(out[(8 - 1) * BPP], BAR, "bar right");
        }
    }

    #[test]
    fn sharp_keeps_hard_edges_and_smooth_blends_them() {
        let mut r = Remote::new(2, 1).unwrap();
        assert!(r.write(0, 0, 1, 1, &[0, 0, 0, 0]));
        assert!(r.write(1, 0, 1, 1, &[255, 255, 255, 0]));
        let sharp = render(&r, (4, 2), Quality::Sharp);
        assert_eq!([sharp[0], sharp[4], sharp[8], sharp[12]], [0, 0, 255, 255]);
        let smooth = render(&r, (4, 2), Quality::Smooth);
        assert!(smooth[4] > 0 && smooth[4] < 255 && smooth[8] > smooth[4]);
    }
}
