//! The product mark of Ultar OS: two open arcs.
//!
//! The mark is a stroke drawing in the 24-unit box of the system icon set (design spec §4,
//! §5.1). It is drawn by the icon pipeline of `ultar-ui` (`IconId::Mark`, same path data –
//! `ultar-check brand` compares them) with the stroke width of the tokens (`size.stroke`) and
//! the colour `accent_text` of the current theme, so it is sharp at every size and has the
//! right contrast on dark and light surfaces. It is never mirrored, filled, stretched,
//! rotated or drawn from a bitmap.

/// Name of the mark in the design sources and in the icon set.
pub const NAME: &str = "mark";

/// SVG path data of the two arcs in the [`VIEW_BOX`].
pub const PATH: &str = "M12 3a9 9 0 1 0 9 9M12 8a4 4 0 1 0 4 4";

/// Edge length of the box the path is drawn in.
pub const VIEW_BOX: u32 = 24;

/// The mark is not mirrored in right-to-left layouts (it has no reading direction).
pub const MIRRORED: bool = false;

/// Token of the colour the mark is drawn in (`[theme.*]` of `design/tokens.toml`).
pub const COLOR_TOKEN: &str = "accent_text";

/// Smallest edge length in logical points (`size.icon_small` of the tokens). Below it the two
/// arcs run together.
pub const MIN_SIZE: u32 = 16;

/// Edge length on the bar (overview button) in logical points (`size.icon`).
pub const BAR_SIZE: u32 = 24;

/// Edge length on the lock and login screen in logical points.
pub const SCREEN_SIZE: u32 = 48;

/// Edge length on the boot picture, the installer welcome page and in About, in logical points.
pub const HERO_SIZE: u32 = 96;

/// Free space on every side for a mark of edge length `size`: a quarter of it. Nothing but
/// the background may be inside.
pub const fn clear_space(size: u32) -> u32 {
    size / 4
}

/// Whether a mark of edge length `size` logical points may be drawn.
pub const fn size_allowed(size: u32) -> bool {
    size >= MIN_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the numbers of the path (the mark only uses `M` and relative `a`).
    fn numbers(path: &str) -> impl Iterator<Item = u32> + '_ {
        path.split(|c: char| !c.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .map(|part| part.parse().unwrap())
    }

    #[test]
    fn path_is_two_arcs_inside_the_box() {
        assert_eq!(PATH.matches('M').count(), 2);
        assert_eq!(PATH.matches('a').count(), 2);
        assert!(PATH
            .chars()
            .all(|c| c.is_ascii_digit() || c == ' ' || c == 'M' || c == 'a'));
        assert!(numbers(PATH).all(|n| n <= VIEW_BOX));
        // outer arc: radius 9 around the centre (12, 12); inner arc: radius 4
        let all: [u32; 18] = {
            let mut out = [0; 18];
            for (slot, n) in out.iter_mut().zip(numbers(PATH)) {
                *slot = n;
            }
            out
        };
        assert_eq!(
            all,
            [12, 3, 9, 9, 0, 1, 0, 9, 9, 12, 8, 4, 4, 0, 1, 0, 4, 4]
        );
    }

    #[test]
    fn sizes_and_clear_space() {
        assert!(!size_allowed(MIN_SIZE - 1));
        for size in [MIN_SIZE, BAR_SIZE, SCREEN_SIZE, HERO_SIZE] {
            assert!(size_allowed(size));
            assert_eq!(clear_space(size) * 4, size);
        }
        let sizes = [MIN_SIZE, BAR_SIZE, SCREEN_SIZE, HERO_SIZE];
        assert!(sizes.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(BAR_SIZE, VIEW_BOX);
        assert_eq!(MIRRORED, PATH.is_empty());
        assert_eq!(NAME, "mark");
        assert_eq!(COLOR_TOKEN, "accent_text");
    }
}
