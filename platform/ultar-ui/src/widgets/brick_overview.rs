//! The overview's bricks (board Overview): a window as its thumbnail in a card with a title
//! strip and its name below, and a workspace as a miniature of its windows with its name.

use super::brick::{Brick, BrickKind};
use super::brick_art;
use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx, Picture};
use crate::geom::{Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::{Painter, TextSpec};
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, ColorRole, Radius, Space, TypeSize};
use std::rc::Rc;

/// Height of the title strip of a window card (board: 24).
pub const STRIP: f32 = 24.0;
/// Height of a workspace miniature (board: 96).
pub const MINIATURE: f32 = 96.0;
/// Width of a workspace card (board: 192).
pub const WORKSPACE_WIDTH: f32 = 192.0;
/// Most window shapes drawn in one miniature.
pub const MAX_MINIS: usize = 6;

const TITLE: TextSpec = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);
const NAME: TextSpec = TextSpec::new(TypeSize::S12);
const NAME_CHOSEN: TextSpec = TextSpec::new(TypeSize::S12).weight(Weight::Semibold);

/// A copy of a window's content as the compositor sends it (`0xXXRRGGBB` little-endian rows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub xrgb: Vec<u8>,
}

impl Thumbnail {
    /// A thumbnail whose bytes match its size; `None` otherwise.
    pub fn new(width: u32, height: u32, xrgb: Vec<u8>) -> Option<Rc<Thumbnail>> {
        let len = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        (width > 0 && height > 0 && xrgb.len() == len).then(|| {
            Rc::new(Thumbnail {
                width,
                height,
                xrgb,
            })
        })
    }
}

/// Natural size of the overview's bricks and of the fixed-size pictures and rules.
pub(super) fn measure_kind(cx: &mut MeasureCx<'_>, brick: &Brick) -> Size {
    match &brick.kind {
        BrickKind::WindowPreview { card_h, .. } => {
            let title = cx.text_size(&brick.label, TITLE, None);
            preview_size(*card_h, title.h)
        }
        BrickKind::WorkspaceCard { chosen, .. } => {
            let name = cx.text_size(&brick.label, workspace_name(*chosen), None);
            Size::new(WORKSPACE_WIDTH, MINIATURE + Space::S4.points() + name.h)
        }
        BrickKind::Mark { points } => {
            let edge = f32::from((*points).max(16));
            Size::new(edge, edge)
        }
        BrickKind::Rule => Size::new(0.0, size::BORDER),
        BrickKind::Avatar => Size::new(Space::S48.points(), Space::S48.points()),
        BrickKind::UserCard { .. } => {
            Size::new(40.0 * Space::S4.points(), 26.0 * Space::S4.points())
        }
        BrickKind::Art(_) => Size::new(size::ICON, size::ICON),
        BrickKind::Logo { width, .. } => super::brick_logo::measure(cx, *width),
        BrickKind::Handle { width } => Size::new(*width, super::brick::TARGET),
        _ => Size::new(0.0, 0.0),
    }
}

/// Paints the overview's bricks and the pictures: the sized brand mark, the company logo,
/// the avatar.
pub(super) fn paint_kind(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    rect: Rect,
    v: &Visual,
    brick: &Brick,
) {
    let label = &brick.label;
    match &brick.kind {
        BrickKind::WindowPreview {
            icon,
            picture,
            card_h,
        } => paint_preview(p, cx, rect, v, (label, *icon, picture.as_deref(), *card_h)),
        BrickKind::WorkspaceCard { minis, chosen, new } => {
            paint_workspace(p, cx, rect, v, (label, minis, *chosen, *new))
        }
        BrickKind::Mark { points } => {
            let edge = f32::from((*points).max(16));
            brick_art::paint_picture(p, Picture::Mark, rect, edge)
        }
        BrickKind::Logo { width, quiet } => super::brick_logo::paint(p, rect, *width, *quiet, v),
        BrickKind::Avatar => {
            let bg = p.color(ColorRole::Raised);
            let border = p.color(ColorRole::ControlBorder);
            p.fill(rect, Some(Radius::Round), bg);
            p.stroke(rect, Some(Radius::Round), p.hairline(), border);
            let fg = p.color(ColorRole::Text);
            p.icon(IconId::User, rect, size::ICON, QuarterTurns::None, fg);
        }
        _ => {}
    }
}

/// Size of a window card: the card of `card_h` points, 8 apart, the title line.
pub(super) fn preview_size(card_h: u16, title_h: f32) -> Size {
    Size::new(
        f32::from(card_h) * 14.0 / 9.0,
        f32::from(card_h) + Space::S8.points() + title_h,
    )
}

/// The text style of a workspace's name.
pub(super) const fn workspace_name(chosen: bool) -> TextSpec {
    if chosen {
        NAME_CHOSEN
    } else {
        NAME
    }
}

/// A window card: the thumbnail (or the app icon until it came) under a title strip, the
/// icon and the title below; the card is `card_h` points high.
pub(super) fn paint_preview(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    rect: Rect,
    v: &Visual,
    (label, icon, picture, card_h): (&Label, IconId, Option<&Thumbnail>, u16),
) {
    let card = Rect::new(rect.x, rect.y, rect.w, f32::from(card_h));
    p.shadow(card, Some(Radius::Card));
    let fill = p.color(ColorRole::Surface);
    p.fill(card, Some(Radius::Card), fill);
    let strip = Rect::new(card.x, card.y, card.w, STRIP);
    let bar = p.color(ColorRole::Bar);
    let previous = p.clip(card);
    p.fill(strip, Some(Radius::Card), bar);
    p.fill(
        Rect::new(strip.x, strip.y + STRIP / 2.0, strip.w, STRIP / 2.0),
        None,
        bar,
    );
    let line = p.color(ColorRole::Line);
    let hair = p.hairline();
    p.fill(
        Rect::new(strip.x, strip.bottom() - hair, strip.w, hair),
        None,
        line,
    );
    let content = Rect::new(card.x, strip.bottom(), card.w, card.h - STRIP);
    match picture {
        Some(t) => p.image(content, t.width, t.height, &t.xrgb),
        None => {
            let muted = p.color(ColorRole::TextMuted);
            let at = Rect::new(
                content.x + (content.w - size::ICON) / 2.0,
                content.y + (content.h - size::ICON) / 2.0,
                size::ICON,
                size::ICON,
            );
            p.icon(icon, at, size::ICON, QuarterTurns::None, muted);
        }
    }
    p.restore_clip(previous);
    let edge = if v.hovered || v.pressed {
        ColorRole::Text
    } else {
        ColorRole::ControlBorder
    };
    let edge = p.color(edge);
    p.stroke(card, Some(Radius::Card), size::BORDER, edge);
    if v.focused {
        style::focus_ring(p, card, Radius::Card);
    }
    title_line(
        p,
        cx,
        rect,
        (label, icon, card.bottom() + Space::S8.points()),
    );
}

fn title_line(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    rect: Rect,
    (label, icon, y): (&Label, IconId, f32),
) {
    let text = p.layout(
        &label.resolve(cx.tr),
        TITLE,
        Some(rect.w - size::ICON_SMALL - Space::S8.points()),
    );
    let scale = p.env().scale;
    let (tw, th) = (text.width / scale, text.height / scale);
    let gap = Space::S8.points();
    let total = size::ICON_SMALL + gap + tw;
    let mut line = Rect::new(
        rect.x + (rect.w - total).max(0.0) / 2.0,
        y,
        total.min(rect.w),
        th,
    );
    let direction = p.env().direction;
    let glyph = style::take_start(&mut line, size::ICON_SMALL, direction);
    let fg = p.color(ColorRole::Text);
    let glyph = Rect::new(
        glyph.x,
        glyph.y + (th - size::ICON_SMALL) / 2.0,
        size::ICON_SMALL,
        size::ICON_SMALL,
    );
    p.icon(icon, glyph, size::ICON_SMALL, QuarterTurns::None, fg);
    let _ = style::take_start(&mut line, gap, direction);
    p.text(&text, line, TextAlign::Start, fg);
}

/// A workspace card: its miniature (the shapes of its windows, or a plus for a new one)
/// with the accent edge when shown, its name below (check mark and semibold when shown).
pub(super) fn paint_workspace(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    rect: Rect,
    v: &Visual,
    (label, minis, chosen, new): (&Label, &[(u8, u8)], bool, bool),
) {
    let mini = Rect::new(rect.x, rect.y, rect.w, MINIATURE);
    let fill = p.color(if v.hovered || v.pressed {
        ColorRole::Hover
    } else {
        ColorRole::Surface
    });
    p.fill(mini, Some(Radius::Control), fill);
    let (width, edge) = if chosen {
        (2.0, ColorRole::AccentText)
    } else {
        (size::BORDER, ColorRole::ControlBorder)
    };
    let edge = p.color(edge);
    p.stroke(mini, Some(Radius::Control), width, edge);
    if v.focused {
        style::focus_ring(p, mini, Radius::Control);
    }
    let fg = p.color(ColorRole::Text);
    if new {
        let at = Rect::new(
            mini.x + (mini.w - size::ICON) / 2.0,
            mini.y + (mini.h - size::ICON) / 2.0,
            size::ICON,
            size::ICON,
        );
        p.icon(IconId::Plus, at, size::ICON, QuarterTurns::None, fg);
    } else {
        paint_minis(p, mini, minis);
    }
    workspace_label(p, cx, rect, (label, chosen, mini.bottom()));
}

/// A workspace's name below its miniature: check mark and semibold when shown.
fn workspace_label(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    rect: Rect,
    (label, chosen, top): (&Label, bool, f32),
) {
    let fg = p.color(ColorRole::Text);
    let spec = workspace_name(chosen);
    let text = p.layout(&label.resolve(cx.tr), spec, Some(rect.w));
    let scale = p.env().scale;
    let (tw, th) = (text.width / scale, text.height / scale);
    let mark = if chosen {
        size::ICON_SMALL + Space::S4.points()
    } else {
        0.0
    };
    let total = tw + mark;
    let mut line = Rect::new(
        rect.x + (rect.w - total).max(0.0) / 2.0,
        top + Space::S4.points(),
        total.min(rect.w),
        th,
    );
    let direction = p.env().direction;
    if chosen {
        let glyph = style::take_start(&mut line, size::ICON_SMALL, direction);
        let glyph = Rect::new(
            glyph.x,
            glyph.y + (th - size::ICON_SMALL) / 2.0,
            size::ICON_SMALL,
            size::ICON_SMALL,
        );
        p.icon(
            IconId::Check,
            glyph,
            size::ICON_SMALL,
            QuarterTurns::None,
            fg,
        );
        let _ = style::take_start(&mut line, Space::S4.points(), direction);
    }
    p.text(&text, line, TextAlign::Start, fg);
}

/// The windows of a workspace as small cards in a centred row, 4 apart (sizes in points).
fn paint_minis(p: &mut Painter<'_>, mini: Rect, minis: &[(u8, u8)]) {
    let gap = Space::S4.points();
    let shown = &minis[..minis.len().min(MAX_MINIS)];
    let total: f32 = shown.iter().map(|(w, _)| f32::from(*w)).sum::<f32>()
        + gap * shown.len().saturating_sub(1) as f32;
    let mut x = mini.x + (mini.w - total).max(0.0) / 2.0;
    let (fill, edge) = (
        p.color(ColorRole::Raised),
        p.color(ColorRole::ControlBorder),
    );
    for (w, h) in shown {
        let (w, h) = (f32::from(*w), f32::from(*h));
        let r = Rect::new(x, mini.y + (mini.h - h) / 2.0, w, h);
        p.fill(r, Some(Radius::Control), fill);
        p.stroke(r, Some(Radius::Control), size::BORDER, edge);
        x += w + gap;
    }
}
