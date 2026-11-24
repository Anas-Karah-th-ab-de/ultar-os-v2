//! The bricks of the desktop shell as the artboards draw them (`design/boards/gen.py`:
//! `tile`, `status_button`, `kbd_button`, the clock and search buttons of `bottombar`,
//! `qt` of the quick settings, `big` of the projection panel, the avatar of the lock and
//! login screens). Each is one node operated like a button; geometry and colours come from
//! the tokens only.

use super::style::{self, Visual};
use super::{brick_art, brick_quick, brick_tile};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::{Painter, TextSpec};
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, ColorRole, Radius, Space, TypeSize};

pub use super::brick_art::Picture;

/// Which shell brick a node is.
#[derive(Debug, Clone, PartialEq)]
pub enum BrickKind {
    /// App tile of the bar or dock: touch-sized square, the app icon, one dot per window
    /// (shape, not only colour); `active` = it has the focused window.
    Tile {
        /// The app icon.
        icon: IconId,
        /// Windows of the app (0 = only pinned).
        running: u8,
        /// It holds the focused window.
        active: bool,
    },
    /// A cell of the app grid: the app's tile above its name, the whole cell operated
    /// (hover, press and the open context menu fill all of it).
    AppCell {
        /// The app icon.
        icon: IconId,
        /// Windows of the app.
        running: u8,
        /// Its context menu is open.
        open: bool,
    },
    /// Touch-sized quiet square with a large icon (overview, all apps); `open` while the
    /// surface it opens is shown.
    Square {
        /// What it shows.
        picture: Picture,
        /// Its surface is open.
        open: bool,
    },
    /// The bar's search entry point: looks like a search field, opens the overview search.
    SearchPill {
        /// Width in logical points.
        width: f32,
    },
    /// Status icons in one button (opens the quick settings).
    Status {
        /// The icons, in reading order.
        icons: Vec<IconId>,
        /// The quick settings are open.
        open: bool,
    },
    /// A short quiet text button (input language).
    Chip,
    /// The clock: time above, date below, at the end (opens calendar and notifications).
    Clock {
        /// The second line.
        date: Label,
        /// Its sheet is open.
        open: bool,
    },
    /// A quick settings tile: icon, name, state; accent when on. `joined` = a
    /// [`BrickKind::QuickMore`] follows at its end (the two look like one tile).
    QuickTile {
        /// The icon.
        icon: IconId,
        /// The state line.
        state: Label,
        /// On.
        on: bool,
        /// A "choose" part follows.
        joined: bool,
    },
    /// The "choose" part at the end of a quick tile (opens its list).
    QuickMore {
        /// The tile is on.
        on: bool,
    },
    /// A large choice card with a picture (display modes of Super+P).
    ModeCard {
        /// The picture.
        picture: Picture,
        /// The chosen mode.
        chosen: bool,
    },
    /// A person to sign in as (board Login): the picture above the name, a check mark and
    /// the accent edge when chosen.
    UserCard {
        /// The chosen user.
        chosen: bool,
    },
    /// A round picture of a person (lock and login screens).
    Avatar,
    /// A hairline in the separator colour across its node (the top edge of the bar); not
    /// operable.
    Rule,
    /// A picture at icon size (the brand mark next to "Ultar OS"); not operable.
    Art(Picture),
}

/// A shell brick: its kind and its label (visible text, or accessible name of icon-only
/// bricks).
#[derive(Debug, Clone, PartialEq)]
pub struct Brick {
    /// What it is.
    pub kind: BrickKind,
    /// Visible text or accessible name.
    pub label: Label,
    /// Accessible name when it differs from the visible text (the clock: "Calendar and
    /// notifications: Sun 22 Nov 16:52").
    pub name: Option<Label>,
}

const TOUCH: f32 = size::TARGET_TOUCH_MIN;
const TARGET: f32 = size::TARGET_POINTER_MIN;
const MORE_WIDTH: f32 = size::TARGET_POINTER_MIN;
const BODY: TextSpec = TextSpec::new(TypeSize::S14);
const SMALL: TextSpec = TextSpec::new(TypeSize::S12);
const SMALL_BOLD: TextSpec = TextSpec::new(TypeSize::S12).weight(Weight::Semibold);
const STRONG: TextSpec = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);

impl Brick {
    /// A brick.
    pub fn new(kind: BrickKind, label: Label) -> Brick {
        Brick {
            kind,
            label,
            name: None,
        }
    }

    /// The same brick with its own accessible name.
    pub fn named(mut self, name: Label) -> Brick {
        self.name = Some(name);
        self
    }

    /// The accessible name.
    pub fn accessible_name(&self) -> &Label {
        self.name.as_ref().unwrap_or(&self.label)
    }

    /// `true` for bricks that are not operated (a rule).
    pub fn is_static(&self) -> bool {
        matches!(
            self.kind,
            BrickKind::Rule | BrickKind::Art(_) | BrickKind::Avatar
        )
    }

    /// `true` for bricks that act as an on/off switch (quick tiles).
    pub fn is_toggle(&self) -> bool {
        matches!(self.kind, BrickKind::QuickTile { .. })
    }

    /// On / chosen / open state, if the brick has one.
    pub fn checked(&self) -> Option<bool> {
        match self.kind {
            BrickKind::QuickTile { on, .. } => Some(on),
            BrickKind::ModeCard { chosen, .. } | BrickKind::UserCard { chosen } => Some(chosen),
            _ => None,
        }
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>) -> Size {
        let pad = Space::S8.points();
        match &self.kind {
            BrickKind::Tile { .. } | BrickKind::Square { .. } => Size::new(TOUCH, TOUCH),
            BrickKind::AppCell { .. } => Size::new(TOUCH + 2.0 * Space::S16.points(), 2.0 * TOUCH),
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
            BrickKind::Avatar => Size::new(Space::S48.points(), Space::S48.points()),
            BrickKind::UserCard { .. } => {
                Size::new(40.0 * Space::S4.points(), 26.0 * Space::S4.points())
            }
            BrickKind::Rule => Size::new(0.0, size::BORDER),
            BrickKind::Art(_) => Size::new(size::ICON, size::ICON),
        }
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, v: &Visual) {
        let previous = p.set_opacity(v.opacity());
        match &self.kind {
            BrickKind::Tile { .. } | BrickKind::AppCell { .. } | BrickKind::UserCard { .. } => {
                self.paint_tiles(p, cx, rect, v)
            }
            BrickKind::Square { picture, open } => {
                quiet_fill(p, rect, v, *open);
                brick_art::paint_picture(p, *picture, rect, size::ICON);
            }
            BrickKind::SearchPill { .. } => self.paint_pill(p, cx, rect, v),
            BrickKind::Status { icons, open } => {
                quiet_fill(p, rect, v, *open);
                paint_icon_row(p, icons, rect);
            }
            BrickKind::Chip => {
                quiet_fill(p, rect, v, false);
                let fg = p.color(ColorRole::TextMuted);
                style::draw_label(p, cx, &self.label, SMALL_BOLD, rect, TextAlign::Center, fg);
            }
            BrickKind::Clock { date, open } => self.paint_clock(p, cx, rect, v, (date, *open)),
            BrickKind::QuickTile {
                icon,
                state,
                on,
                joined,
            } => self.paint_quick(p, cx, rect, v, (*icon, state, *on, *joined)),
            BrickKind::QuickMore { on } => brick_quick::paint_more(p, rect, v, *on),
            BrickKind::ModeCard { picture, chosen } => {
                self.paint_mode(p, cx, rect, v, (*picture, *chosen))
            }
            BrickKind::Rule => {
                let line = p.color(ColorRole::Line);
                p.fill(rect, None, line);
            }
            BrickKind::Art(picture) => brick_art::paint_picture(p, *picture, rect, size::ICON),
            BrickKind::Avatar => {
                let bg = p.color(ColorRole::Raised);
                let border = p.color(ColorRole::ControlBorder);
                p.fill(rect, Some(Radius::Round), bg);
                p.stroke(rect, Some(Radius::Round), p.hairline(), border);
                let fg = p.color(ColorRole::Text);
                p.icon(IconId::User, rect, size::ICON, QuarterTurns::None, fg);
            }
        }
        if v.focused {
            let ring = match self.kind {
                BrickKind::QuickTile { .. }
                | BrickKind::ModeCard { .. }
                | BrickKind::UserCard { .. } => Radius::Card,
                BrickKind::Avatar => Radius::Round,
                _ => Radius::Control,
            };
            style::focus_ring(p, rect, ring);
        }
        p.set_opacity(previous);
    }

    /// The bricks painted in `brick_tile`: the bar's tile, the grid's cell, the user card.
    fn paint_tiles(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, v: &Visual) {
        match self.kind {
            BrickKind::Tile {
                icon,
                running,
                active,
            } => brick_tile::paint_tile(p, rect, v, (icon, running, active)),
            BrickKind::AppCell {
                icon,
                running,
                open,
            } => {
                quiet_fill(p, rect, v, open);
                brick_tile::paint_cell(p, cx, rect, &self.label, (icon, running));
            }
            BrickKind::UserCard { chosen } => {
                brick_tile::paint_user(p, cx, rect, v, (&self.label, chosen))
            }
            _ => {}
        }
    }

    fn paint_pill(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, v: &Visual) {
        let bg = if v.hovered || v.pressed {
            p.color(style::neutral_background(v))
        } else {
            p.color(ColorRole::Surface)
        };
        p.fill(rect, Some(Radius::Control), bg);
        let border = p.color(ColorRole::ControlBorder);
        p.stroke(rect, Some(Radius::Control), p.hairline(), border);
        let fg = p.color(ColorRole::TextMuted);
        let mut inner = style::inset_inline(rect, Space::S12);
        let direction = p.env().direction;
        let glyph = style::take_start(&mut inner, size::ICON_SMALL, direction);
        p.icon(
            IconId::Search,
            glyph,
            size::ICON_SMALL,
            QuarterTurns::None,
            fg,
        );
        let _ = style::take_start(&mut inner, Space::S8.points(), direction);
        style::draw_label(p, cx, &self.label, BODY, inner, TextAlign::Start, fg);
    }

    fn paint_clock(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        (date, open): (&Label, bool),
    ) {
        quiet_fill(p, rect, v, open);
        let inner = style::inset_inline(rect, Space::S8);
        let line = p
            .layout(&self.label.resolve(cx.tr), SMALL_BOLD, None)
            .height
            / p.env().scale;
        let second = p.layout(&date.resolve(cx.tr), SMALL, None).height / p.env().scale;
        let top = inner.y + (inner.h - line - second) / 2.0;
        let (text, muted) = (p.color(ColorRole::Text), p.color(ColorRole::TextMuted));
        let first = Rect::new(inner.x, top, inner.w, line);
        style::draw_label(p, cx, &self.label, SMALL_BOLD, first, TextAlign::End, text);
        let below = Rect::new(inner.x, top + line, inner.w, second);
        style::draw_label(p, cx, date, SMALL, below, TextAlign::End, muted);
    }

    fn paint_quick(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        (icon, state, on, joined): (IconId, &Label, bool, bool),
    ) {
        let direction = p.env().direction;
        let body = rect;
        let reach = if joined { Space::S12.points() } else { 0.0 };
        // The shape reaches under the "choose" part at the end, which paints its own end.
        let shape = if direction.is_rtl() {
            Rect::new(rect.x - reach, rect.y, rect.w + reach, rect.h)
        } else {
            Rect::new(rect.x, rect.y, rect.w + reach, rect.h)
        };
        let saved = p.clip(rect);
        brick_quick::quick_shape(p, shape, v, on);
        p.restore_clip(saved);
        let fg = p.color(if on {
            ColorRole::OnAccent
        } else {
            ColorRole::Text
        });
        let mut inner = style::inset_inline(body, Space::S12);
        let glyph = style::take_start(&mut inner, size::ICON, direction);
        p.icon(icon, glyph, size::ICON, QuarterTurns::None, fg);
        let _ = style::take_start(&mut inner, Space::S12.points(), direction);
        let name = p
            .layout(&self.label.resolve(cx.tr), STRONG, Some(inner.w))
            .height;
        let sub = p.layout(&state.resolve(cx.tr), SMALL, Some(inner.w)).height;
        let s = p.env().scale;
        let top = inner.y + (inner.h - (name + sub) / s) / 2.0;
        let first = Rect::new(inner.x, top, inner.w, name / s);
        style::draw_label(p, cx, &self.label, STRONG, first, TextAlign::Start, fg);
        let second = Rect::new(inner.x, top + name / s, inner.w, sub / s);
        style::draw_label(p, cx, state, SMALL, second, TextAlign::Start, fg);
    }

    fn paint_mode(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        (picture, chosen): (Picture, bool),
    ) {
        let bg = if chosen || v.hovered {
            ColorRole::Hover
        } else if v.pressed {
            ColorRole::Line
        } else {
            ColorRole::Raised
        };
        let bg = p.color(bg);
        p.fill(rect, Some(Radius::Card), bg);
        let (width, border) = if chosen {
            (size::FOCUS_RING, ColorRole::AccentText)
        } else {
            (size::BORDER, ColorRole::ControlBorder)
        };
        let border = p.color(border);
        p.stroke(rect, Some(Radius::Card), width, border);
        let gap = Space::S12.points();
        let art = Size::new(Space::S48.points(), size::ICON);
        let text = p.layout(&self.label.resolve(cx.tr), STRONG, None);
        let text_h = text.height / p.env().scale;
        let top = rect.y + (rect.h - art.h - gap - text_h) / 2.0;
        let art_rect = Rect::new(rect.x + (rect.w - art.w) / 2.0, top, art.w, art.h);
        brick_art::paint_picture(p, picture, art_rect, size::ICON);
        let fg = p.color(ColorRole::Text);
        let text_w = text.width / p.env().scale;
        let mark = if chosen {
            size::ICON_SMALL + Space::S4.points()
        } else {
            0.0
        };
        let line = Rect::new(
            rect.x + (rect.w - text_w - mark) / 2.0,
            top + art.h + gap,
            text_w + mark,
            text_h,
        );
        let mut rest = line;
        let direction = p.env().direction;
        if chosen {
            let glyph = style::take_start(&mut rest, size::ICON_SMALL, direction);
            let check = p.color(ColorRole::AccentText);
            p.icon(
                IconId::Check,
                glyph,
                size::ICON_SMALL,
                QuarterTurns::None,
                check,
            );
            let _ = style::take_start(&mut rest, Space::S4.points(), direction);
        }
        p.text(&text, rest, TextAlign::Start, fg);
    }
}

/// Background of quiet bricks: hover while open or hovered, line while pressed.
fn quiet_fill(p: &mut Painter<'_>, rect: Rect, v: &Visual, open: bool) {
    let role = if v.pressed {
        Some(ColorRole::Line)
    } else if open || v.hovered {
        Some(ColorRole::Hover)
    } else {
        None
    };
    if let Some(role) = role {
        let bg = p.color(role);
        p.fill(rect, Some(Radius::Control), bg);
    }
}

fn paint_icon_row(p: &mut Painter<'_>, icons: &[IconId], rect: Rect) {
    let gap = Space::S12.points();
    let n = icons.len() as f32;
    let width = n * size::ICON_SMALL + (n - 1.0).max(0.0) * gap;
    let mut x = rect.x + (rect.w - width) / 2.0;
    let fg = p.color(ColorRole::Text);
    let rtl = p.env().direction.is_rtl();
    let ordered: Vec<IconId> = if rtl {
        icons.iter().rev().copied().collect()
    } else {
        icons.to_vec()
    };
    for icon in ordered {
        let glyph = Rect::new(x, rect.y, size::ICON_SMALL, rect.h);
        p.icon(icon, glyph, size::ICON_SMALL, QuarterTurns::None, fg);
        x += size::ICON_SMALL + gap;
    }
}
