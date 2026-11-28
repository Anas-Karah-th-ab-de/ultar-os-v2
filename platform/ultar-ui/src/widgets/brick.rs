//! The bricks of the desktop shell as the artboards draw them (`design/boards/gen.py`:
//! `tile`, `status_button`, `kbd_button`, the clock and search buttons of `bottombar`,
//! `qt` of the quick settings, `big` of the projection panel, the avatar of the lock and
//! login screens). Each is one node operated like a button; geometry and colours come from
//! the tokens only.

use super::brick_overview::{self, Thumbnail};
use super::style::{self, Visual};
use super::{brick_art, brick_phone, brick_quick, brick_tile};
use super::{Label, PaintCx};
use crate::geom::Rect;
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
    /// A shortcut on the desktop background: the app's tile of `side` points with its name
    /// below in up to two lines; quiet until hovered, filled and edged while `selected`.
    DeskIcon {
        /// The app icon.
        icon: IconId,
        /// Edge of the tile in logical points (desktop.icons.size).
        side: u16,
        /// The selected shortcut.
        selected: bool,
    },
    /// A cell of the phone's home screen and app grid: the icon in a box above the name
    /// (boards CompactHome, CompactApps); `open` while its menu is shown.
    PhoneCell {
        /// The app icon.
        icon: IconId,
        /// Its context menu is open.
        open: bool,
    },
    /// The status button of the phone's status bar: status icons, then the battery level
    /// (opens the quick settings).
    PhoneStatus {
        /// The icons, in reading order.
        icons: Vec<IconId>,
        /// The battery level text, if there is a battery.
        level: Option<Label>,
        /// The quick settings are open.
        open: bool,
    },
    /// A grab handle (the gesture bar, the handle that closes a sheet): a round bar of
    /// `width` in the middle of its node.
    Handle {
        /// Length of the bar in logical points.
        width: f32,
    },
    /// The dimmed part of the screen beside an open sheet (operable: a press closes the
    /// sheet). Painted in the scrim colour of dialogs.
    Scrim,
    /// A quiet text button of the top bar (the active app's menu, the clock): body text
    /// in `weight`, `pad` at both sides; `open` while its menu or sheet is shown.
    BarButton {
        /// Inline padding.
        pad: Space,
        /// Weight of the text.
        weight: Weight,
        /// Its surface is open.
        open: bool,
    },
    /// A hairline in the separator colour across its node (the top edge of the bar); not
    /// operable.
    Rule,
    /// A picture at icon size (the brand mark next to "Ultar OS"); not operable.
    Art(Picture),
    /// A window of the overview: its thumbnail (or its app icon until the copy came) in a
    /// card `card_h` points high with a title strip, the icon and the title below.
    WindowPreview {
        /// The app icon.
        icon: IconId,
        /// The copy of its content.
        picture: Option<std::rc::Rc<Thumbnail>>,
        /// Height of the card in points.
        card_h: u16,
    },
    /// A workspace of the overview: the shapes of its windows (points), or a plus for a
    /// new one; the accent edge and a check mark when shown.
    WorkspaceCard {
        /// Width and height of each window shape in points.
        minis: Vec<(u8, u8)>,
        /// The shown workspace.
        chosen: bool,
        /// The "new workspace" card.
        new: bool,
    },
    /// The brand mark at `points` (docs/BRAND.md §3: 48 on the lock and login screens, 96 on
    /// large empty states), in the `accent_text` colour, never mirrored; a picture.
    Mark {
        /// Edge length in logical points (at least 16).
        points: u16,
    },
    /// The company logo of KM Code Craft (docs/BRAND.md §3: installer welcome, About, the
    /// desktop background, the lock screen) at most `width` points wide, proportions kept,
    /// never above the bitmap's own size; on a dark plate in the light theme; a picture.
    Logo {
        /// Largest width in logical points.
        width: u16,
        /// Drawn quietly (the desktop background): at half strength, so it never competes
        /// with the windows and icons above it.
        quiet: bool,
    },
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

pub(super) const TOUCH: f32 = size::TARGET_TOUCH_MIN;
pub(super) const TARGET: f32 = size::TARGET_POINTER_MIN;
pub(super) const MORE_WIDTH: f32 = size::TARGET_POINTER_MIN;
pub(super) const BODY: TextSpec = TextSpec::new(TypeSize::S14);
pub(super) const SMALL: TextSpec = TextSpec::new(TypeSize::S12);
pub(super) const SMALL_BOLD: TextSpec = TextSpec::new(TypeSize::S12).weight(Weight::Semibold);
pub(super) const STRONG: TextSpec = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);

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
            BrickKind::Rule
                | BrickKind::Art(_)
                | BrickKind::Mark { .. }
                | BrickKind::Logo { .. }
                | BrickKind::Avatar
        )
    }

    /// `true` for bricks that act as an on/off switch (quick tiles).
    pub fn is_toggle(&self) -> bool {
        matches!(self.kind, BrickKind::QuickTile { .. })
    }

    /// The status icon under `pos` (its index in reading order, `icons` of
    /// [`BrickKind::Status`]); `None` beside the icons and for other bricks. A click on an
    /// icon reports `Selected(index)` (the network icon opens its own page), a click
    /// beside them and the keyboard report `Activated`.
    pub(crate) fn part_at(&self, rect: Rect, pos: crate::geom::Point, rtl: bool) -> Option<usize> {
        let BrickKind::Status { icons, .. } = &self.kind else {
            return None;
        };
        if !rect.contains(pos) {
            return None;
        }
        // the same places as `paint_icon_row`: exactly the glyphs (the padding and the gaps
        // around them are the button's own place, which opens the tiles)
        let gap = Space::S12.points();
        let n = icons.len();
        let width = n as f32 * size::ICON_SMALL + (n as f32 - 1.0).max(0.0) * gap;
        let start = rect.x + (rect.w - width) / 2.0;
        (0..n).find_map(|slot| {
            let x = start + slot as f32 * (size::ICON_SMALL + gap);
            let hit = pos.x >= x && pos.x < x + size::ICON_SMALL;
            hit.then(|| if rtl { n - 1 - slot } else { slot })
        })
    }

    /// On / chosen / open state, if the brick has one.
    pub fn checked(&self) -> Option<bool> {
        match self.kind {
            BrickKind::QuickTile { on, .. } => Some(on),
            BrickKind::ModeCard { chosen, .. }
            | BrickKind::UserCard { chosen }
            | BrickKind::WorkspaceCard { chosen, .. }
            | BrickKind::DeskIcon {
                selected: chosen, ..
            } => Some(chosen),
            _ => None,
        }
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, v: &Visual) {
        let previous = p.set_opacity(v.opacity());
        match &self.kind {
            BrickKind::Tile { .. } | BrickKind::AppCell { .. } | BrickKind::UserCard { .. } => {
                self.paint_tiles(p, cx, rect, v)
            }
            BrickKind::Square { .. }
            | BrickKind::Chip
            | BrickKind::PhoneCell { .. }
            | BrickKind::Handle { .. }
            | BrickKind::PhoneStatus { .. }
            | BrickKind::BarButton { .. }
            | BrickKind::Scrim => self.paint_shapes(p, cx, rect, v),
            BrickKind::SearchPill { .. } => self.paint_pill(p, cx, rect, v),
            BrickKind::Status { icons, open } => {
                quiet_fill(p, rect, v, *open);
                paint_icon_row(p, icons, rect);
            }
            BrickKind::Clock { date, open } => self.paint_clock(p, cx, rect, v, (date, *open)),
            BrickKind::QuickTile {
                icon,
                state,
                on,
                joined,
            } => {
                brick_quick::paint_quick(p, cx, rect, v, (&self.label, *icon, state, *on, *joined))
            }
            BrickKind::QuickMore { on } => brick_quick::paint_more(p, rect, v, *on),
            BrickKind::ModeCard { picture, chosen } => {
                brick_art::paint_mode(p, cx, rect, v, (&self.label, *picture, *chosen))
            }
            BrickKind::Rule => {
                let line = p.color(ColorRole::Line);
                p.fill(rect, None, line);
            }
            BrickKind::Art(picture) => brick_art::paint_picture(p, *picture, rect, size::ICON),
            // the overview's cards and the pictures (avatar, brand mark, company logo)
            _ => brick_overview::paint_kind(p, cx, rect, v, self),
        }
        if v.focused && self.kind != BrickKind::Scrim {
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

    /// The quiet squares and chips, the bricks of the phone shell and of the top bar.
    fn paint_shapes(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, v: &Visual) {
        match &self.kind {
            BrickKind::Square { picture, open } => {
                quiet_fill(p, rect, v, *open);
                brick_art::paint_picture(p, *picture, rect, size::ICON);
            }
            BrickKind::Chip => {
                quiet_fill(p, rect, v, false);
                let fg = p.color(ColorRole::TextMuted);
                style::draw_label(p, cx, &self.label, SMALL_BOLD, rect, TextAlign::Center, fg);
            }
            BrickKind::PhoneCell { icon, open } => {
                quiet_fill(p, rect, v, *open);
                brick_phone::paint_cell(p, cx, rect, (&self.label, *icon));
            }
            BrickKind::Handle { width } => brick_phone::paint_handle(p, rect, v, *width),
            BrickKind::BarButton { weight, open, .. } => {
                quiet_fill(p, rect, v, *open);
                let fg = p.color(ColorRole::Text);
                let spec = BODY.weight(*weight);
                style::draw_label(p, cx, &self.label, spec, rect, TextAlign::Center, fg);
            }
            BrickKind::Scrim => {
                let dim = super::Dialog::scrim(p);
                p.fill(rect, None, dim);
            }
            BrickKind::PhoneStatus { icons, level, open } => {
                quiet_fill(p, rect, v, *open);
                brick_phone::paint_status(p, cx, rect, (icons, level.as_ref()));
            }
            _ => {}
        }
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
