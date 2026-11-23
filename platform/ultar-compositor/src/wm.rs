//! Window placement rules: snap layouts (halves, quarters, maximise – like
//! Windows 11 and DESKTOP-SPEC §5.2), keyboard snapping, placement of new
//! windows and keeping title bars reachable. Pure functions on logical
//! rectangles; the server applies them.

use crate::geom::Rect;
use ultar_surface::proto::WindowState;

/// A snapped position (physical sides: dragging to the left screen edge
/// snaps left in every language).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Snap {
    /// Left half.
    Left, // physical
    /// Right half.
    Right, // physical
    /// Top left quarter.
    TopLeft, // physical
    /// Top right quarter.
    TopRight, // physical
    /// Bottom left quarter.
    BottomLeft, // physical
    /// Bottom right quarter.
    BottomRight, // physical
}

/// Where a dragged window would land.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SnapTarget {
    /// Fill the work area.
    Maximize,
    /// A half or quarter.
    Snap(Snap),
}

/// Arrow keys used with Super.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Arrow {
    /// Up.
    Up,
    /// Down.
    Down,
    /// Physical left.
    Left, // physical
    /// Physical right.
    Right, // physical
}

/// The rectangle of a snap position inside a work area.
pub fn snap_rect(snap: Snap, work: Rect) -> Rect {
    let half_w = work.w / 2;
    let half_h = work.h / 2;
    let (x0, x1) = (work.x, work.x + half_w);
    let (y0, y1) = (work.y, work.y + half_h);
    let (wr, hb) = (work.w - half_w, work.h - half_h);
    match snap {
        Snap::Left => Rect::new(x0, y0, half_w, work.h), // physical
        Snap::Right => Rect::new(x1, y0, wr, work.h),    // physical
        Snap::TopLeft => Rect::new(x0, y0, half_w, half_h), // physical
        Snap::TopRight => Rect::new(x1, y0, wr, half_h), // physical
        Snap::BottomLeft => Rect::new(x0, y1, half_w, hb), // physical
        Snap::BottomRight => Rect::new(x1, y1, wr, hb),  // physical
    }
}

/// The snap target for a pointer at (`x`, `y`) while a window is dragged:
/// within `edge` points of a side or corner of the work area.
pub fn snap_target(x: i32, y: i32, work: Rect, edge: i32) -> Option<SnapTarget> {
    let corner = edge * 4;
    let near_x_min = x < work.x + edge; // physical
    let near_x_max = x >= work.max_x() - edge; // physical
    let near_top = y < work.y + edge;
    let near_bottom = y >= work.max_y() - edge;
    let top_zone = y < work.y + corner;
    let bottom_zone = y >= work.max_y() - corner;
    let target = if near_x_min {
        if top_zone {
            Snap::TopLeft // physical
        } else if bottom_zone {
            Snap::BottomLeft // physical
        } else {
            Snap::Left // physical
        }
    } else if near_x_max {
        if top_zone {
            Snap::TopRight // physical
        } else if bottom_zone {
            Snap::BottomRight // physical
        } else {
            Snap::Right // physical
        }
    } else if near_top {
        if x < work.x + corner {
            Snap::TopLeft // physical
        } else if x >= work.max_x() - corner {
            Snap::TopRight // physical
        } else {
            return Some(SnapTarget::Maximize);
        }
    } else if near_bottom && (x < work.x + corner || x >= work.max_x() - corner) {
        if x < work.x + corner {
            Snap::BottomLeft // physical
        } else {
            Snap::BottomRight // physical
        }
    } else {
        return None;
    };
    Some(SnapTarget::Snap(target))
}

/// What Super+arrow does to a window in `state` / `snap` (Windows 11
/// behaviour). `None` for the state means "minimise".
pub fn keyboard_snap(
    state: WindowState,
    snap: Option<Snap>,
    arrow: Arrow,
) -> (WindowState, Option<Snap>) {
    use Snap::*;
    let normal = (WindowState::Normal, None);
    let snapped = |s| (WindowState::Normal, Some(s));
    match (state, snap, arrow) {
        (WindowState::Maximized, _, Arrow::Down) => normal,
        (WindowState::Maximized, _, Arrow::Left) => snapped(Left), // physical
        (WindowState::Maximized, _, Arrow::Right) => snapped(Right), // physical
        (WindowState::Maximized, _, Arrow::Up) => (WindowState::Maximized, None),
        (_, Some(Left), Arrow::Up) => snapped(TopLeft), // physical
        (_, Some(Right), Arrow::Up) => snapped(TopRight), // physical
        (_, Some(Left), Arrow::Down) => snapped(BottomLeft), // physical
        (_, Some(Right), Arrow::Down) => snapped(BottomRight), // physical
        (_, Some(TopLeft | BottomLeft), Arrow::Right) => snapped(Right), // physical
        (_, Some(TopRight | BottomRight), Arrow::Left) => snapped(Left), // physical
        (_, Some(TopLeft), Arrow::Down) => snapped(Left), // physical
        (_, Some(TopRight), Arrow::Down) => snapped(Right), // physical
        (_, Some(BottomLeft), Arrow::Up) => snapped(Left), // physical
        (_, Some(BottomRight), Arrow::Up) => snapped(Right), // physical
        (_, Some(Left), Arrow::Right) | (_, Some(Right), Arrow::Left) => normal, // physical
        (_, Some(s), _) => snapped(s),
        (_, None, Arrow::Up) => (WindowState::Maximized, None),
        (_, None, Arrow::Down) => (WindowState::Minimized, None),
        (_, None, Arrow::Left) => snapped(Left), // physical
        (_, None, Arrow::Right) => snapped(Right), // physical
    }
}

/// The frame of a new window of content size `w × h` (plus `title` points
/// of title bar): centred in the work area, cascaded by `index`, never
/// larger than the work area.
pub fn place_new(work: Rect, w: i32, h: i32, title: i32, index: usize) -> Rect {
    let fw = w.clamp(1, work.w.max(1));
    let fh = (h + title).clamp(1, work.h.max(1));
    let step = title.max(1) * (index % 8) as i32;
    let x = work.x + (work.w - fw) / 2 + step;
    let y = work.y + (work.h - fh) / 2 + step;
    constrain(Rect::new(x, y, fw, fh), work, title)
}

/// Moves a frame so that at least `title` points of its title bar stay
/// inside the work area (it can be grabbed again).
pub fn constrain(frame: Rect, work: Rect, title: i32) -> Rect {
    let grab = title.max(1);
    let min_x = work.x - frame.w + grab * 2;
    let max_x = work.max_x() - grab * 2;
    let x = frame.x.clamp(min_x.min(max_x), max_x.max(min_x));
    let y = frame.y.clamp(work.y, (work.max_y() - grab).max(work.y));
    Rect::new(x, y, frame.w, frame.h)
}

/// Clamps a content size to the window's limits (0 = none) and the work
/// area.
pub fn clamp_size(w: i32, h: i32, min: (u32, u32), max: (u32, u32), work: Rect) -> (i32, i32) {
    let lim = |v: i32, lo: u32, hi: u32, area: i32| {
        let lo = (lo as i32).min(area);
        let hi = if hi == 0 { area } else { (hi as i32).min(area) };
        v.clamp(lo.max(1), hi.max(lo.max(1)))
    };
    (lim(w, min.0, max.0, work.w), lim(h, min.1, max.1, work.h))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect::new(0, 0, 1920, 1024);

    #[test]
    fn snap_rects_tile_the_work_area() {
        assert_eq!(snap_rect(Snap::Left, WORK), Rect::new(0, 0, 960, 1024));
        assert_eq!(snap_rect(Snap::Right, WORK), Rect::new(960, 0, 960, 1024));
        assert_eq!(
            snap_rect(Snap::BottomRight, WORK),
            Rect::new(960, 512, 960, 512)
        );
        let odd = Rect::new(10, 10, 101, 51);
        let l = snap_rect(Snap::Left, odd);
        let r = snap_rect(Snap::Right, odd);
        assert_eq!(l.max_x(), r.x);
        assert_eq!(r.max_x(), odd.max_x());
        let tl = snap_rect(Snap::TopLeft, odd);
        let bl = snap_rect(Snap::BottomLeft, odd);
        assert_eq!(tl.max_y(), bl.y);
        assert_eq!(bl.max_y(), odd.max_y());
    }

    #[test]
    fn drag_targets_at_edges_and_corners() {
        assert_eq!(snap_target(960, 500, WORK, 8), None);
        assert_eq!(
            snap_target(2, 500, WORK, 8),
            Some(SnapTarget::Snap(Snap::Left))
        );
        assert_eq!(
            snap_target(1919, 500, WORK, 8),
            Some(SnapTarget::Snap(Snap::Right))
        );
        assert_eq!(snap_target(960, 0, WORK, 8), Some(SnapTarget::Maximize));
        assert_eq!(
            snap_target(0, 0, WORK, 8),
            Some(SnapTarget::Snap(Snap::TopLeft))
        );
        assert_eq!(
            snap_target(20, 2, WORK, 8),
            Some(SnapTarget::Snap(Snap::TopLeft))
        );
        assert_eq!(
            snap_target(1900, 3, WORK, 8),
            Some(SnapTarget::Snap(Snap::TopRight))
        );
        assert_eq!(
            snap_target(0, 1020, WORK, 8),
            Some(SnapTarget::Snap(Snap::BottomLeft))
        );
        assert_eq!(
            snap_target(1919, 1020, WORK, 8),
            Some(SnapTarget::Snap(Snap::BottomRight))
        );
        assert_eq!(
            snap_target(1900, 1023, WORK, 8),
            Some(SnapTarget::Snap(Snap::BottomRight))
        );
        assert_eq!(
            snap_target(20, 1023, WORK, 8),
            Some(SnapTarget::Snap(Snap::BottomLeft))
        );
        assert_eq!(snap_target(960, 1023, WORK, 8), None);
    }

    #[test]
    fn super_arrows_walk_the_layouts() {
        use WindowState::*;
        let n = |s| (Normal, Some(s));
        assert_eq!(keyboard_snap(Normal, None, Arrow::Left), n(Snap::Left));
        assert_eq!(keyboard_snap(Normal, None, Arrow::Up), (Maximized, None));
        assert_eq!(keyboard_snap(Normal, None, Arrow::Down), (Minimized, None));
        assert_eq!(keyboard_snap(Normal, None, Arrow::Right), n(Snap::Right));
        assert_eq!(keyboard_snap(Maximized, None, Arrow::Down), (Normal, None));
        assert_eq!(keyboard_snap(Maximized, None, Arrow::Up), (Maximized, None));
        assert_eq!(keyboard_snap(Maximized, None, Arrow::Left), n(Snap::Left));
        assert_eq!(keyboard_snap(Maximized, None, Arrow::Right), n(Snap::Right));
    }

    #[test]
    fn super_arrows_from_snapped_positions() {
        use WindowState::*;
        let n = |s| (Normal, Some(s));
        assert_eq!(
            keyboard_snap(Normal, Some(Snap::Left), Arrow::Up),
            n(Snap::TopLeft)
        );
        assert_eq!(
            keyboard_snap(Normal, Some(Snap::Right), Arrow::Up),
            n(Snap::TopRight)
        );
        assert_eq!(
            keyboard_snap(Normal, Some(Snap::Left), Arrow::Down),
            n(Snap::BottomLeft)
        );
        assert_eq!(
            keyboard_snap(Normal, Some(Snap::Right), Arrow::Down),
            n(Snap::BottomRight)
        );
        assert_eq!(
            keyboard_snap(Normal, Some(Snap::Left), Arrow::Right),
            (Normal, None)
        );
        assert_eq!(
            keyboard_snap(Normal, Some(Snap::Right), Arrow::Left),
            (Normal, None)
        );
        assert_eq!(
            keyboard_snap(Normal, Some(Snap::TopLeft), Arrow::Right),
            n(Snap::Right)
        );
        assert_eq!(
            keyboard_snap(Normal, Some(Snap::BottomRight), Arrow::Left),
            n(Snap::Left)
        );
        assert_eq!(
            keyboard_snap(Normal, Some(Snap::TopLeft), Arrow::Down),
            n(Snap::Left)
        );
        assert_eq!(
            keyboard_snap(Normal, Some(Snap::TopRight), Arrow::Down),
            n(Snap::Right)
        );
        assert_eq!(
            keyboard_snap(Normal, Some(Snap::BottomLeft), Arrow::Up),
            n(Snap::Left)
        );
        assert_eq!(
            keyboard_snap(Normal, Some(Snap::BottomRight), Arrow::Up),
            n(Snap::Right)
        );
        assert_eq!(
            keyboard_snap(Normal, Some(Snap::TopLeft), Arrow::Left),
            n(Snap::TopLeft)
        );
    }

    #[test]
    fn placement_and_constraints() {
        let first = place_new(WORK, 800, 600, 40, 0);
        assert_eq!(first, Rect::new(560, 192, 800, 640));
        let second = place_new(WORK, 800, 600, 40, 1);
        assert_eq!(second.x, first.x + 40);
        let huge = place_new(WORK, 5000, 5000, 40, 0);
        assert!(WORK.contains_rect(&huge));
        let off = constrain(Rect::new(-5000, -100, 400, 300), WORK, 40);
        assert!(off.max_x() >= 80 && off.y == 0);
        let below = constrain(Rect::new(100, 5000, 400, 300), WORK, 40);
        assert_eq!(below.y, 1024 - 40);
        assert_eq!(clamp_size(10, 10, (200, 100), (0, 0), WORK), (200, 100));
        assert_eq!(clamp_size(5000, 5000, (0, 0), (0, 0), WORK), (1920, 1024));
        assert_eq!(clamp_size(900, 900, (0, 0), (800, 0), WORK), (800, 900));
        assert_eq!(clamp_size(10, 10, (5000, 5000), (0, 0), WORK), (1920, 1024));
    }
}
