//! Window decorations drawn by the compositor (DESKTOP-SPEC §5.2): title
//! in the middle, buttons at the **end** of the title bar – minimise,
//! maximise/restore, close – mirrored in Arabic (buttons on the left).
//! Every colour, size and icon comes from the toolkit's tokens.

use crate::geom::Rect;
use crate::pixels::Image;
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::icon::{IconCache, IconId, QuarterTurns};
use ultar_ui::paint::{PaintEnv, Painter, Resources, TextSpec};
use ultar_ui::text::{Overflow, TextAlign, TextEngine};
use ultar_ui::tokens::{size, ColorRole, Space, ThemeKind, TypeSize};
use ultar_ui::{Direction, Lang};

/// Height of the title bar in logical points (token `size.titlebar_height`).
pub const TITLEBAR: i32 = size::TITLEBAR_HEIGHT as i32;
/// Width of a title bar button (square).
pub const BUTTON: i32 = TITLEBAR;
/// Width of the grab zone along window borders (DESKTOP-SPEC: 8).
pub fn border_grab() -> i32 {
    Space::S8.points() as i32
}

/// A title bar button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    /// Minimise.
    Minimize,
    /// Maximise or restore.
    Maximize,
    /// Close.
    Close,
}

/// What a point of a decorated window is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    /// The app's pixels.
    Content,
    /// The title bar (drag to move, double click to maximise).
    Title,
    /// A button.
    Button(Button),
    /// A border; bits as `ResizeEdges`.
    Border(u8),
}

/// Button positions relative to the frame (logical points), in reading
/// order from the end: close is outermost.
pub fn buttons(frame_w: i32, rtl: bool) -> [(Button, Rect); 3] {
    let order = [Button::Close, Button::Maximize, Button::Minimize];
    let place = |i: i32| {
        let from_end = BUTTON * (i + 1);
        let x = if rtl { BUTTON * i } else { frame_w - from_end };
        Rect::new(x, 0, BUTTON, TITLEBAR)
    };
    [
        (order[0], place(0)),
        (order[1], place(1)),
        (order[2], place(2)),
    ]
}

/// Finds the part under a point (logical, relative to the frame). `title`
/// is false for undecorated windows (fullscreen); `resizable` adds the
/// border zones.
pub fn hit(
    frame_w: i32,
    frame_h: i32,
    x: i32,
    y: i32,
    title: bool,
    resizable: bool,
    rtl: bool,
) -> Option<Part> {
    let grab = border_grab();
    let inside = x >= 0 && y >= 0 && x < frame_w && y < frame_h;
    if !inside {
        // The resize zone is the band of `grab` points around the frame
        // (where the shadow is drawn).
        let near = x >= -grab && y >= -grab && x < frame_w + grab && y < frame_h + grab;
        if !(resizable && near) {
            return None;
        }
        let mut edges = 0u8;
        if y < 0 {
            edges |= 1;
        }
        if y >= frame_h {
            edges |= 2;
        }
        if x < 0 {
            edges |= 4; // physical: left edge
        }
        if x >= frame_w {
            edges |= 8; // physical: right edge
        }
        return Some(Part::Border(edges));
    }
    if title && y < TITLEBAR {
        let hit_button = buttons(frame_w, rtl)
            .into_iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(b, _)| Part::Button(b));
        return Some(hit_button.unwrap_or(Part::Title));
    }
    Some(Part::Content)
}

/// Everything the look of a title bar depends on.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TitleKey {
    /// Width in device pixels.
    pub width: u32,
    /// Scale × 1000.
    pub scale_milli: u32,
    /// Title text (already cut to the protocol limit).
    pub title: String,
    /// The window has the keyboard focus.
    pub focused: bool,
    /// Button under the pointer.
    pub hover: Option<Button>,
    /// Button held down.
    pub pressed: Option<Button>,
    /// Maximised (restore icon).
    pub maximized: bool,
    /// The app does not answer pings.
    pub unresponsive: bool,
}

/// Draws title bars with the toolkit.
pub struct DecorRenderer {
    resources: Resources,
    lang: Lang,
    theme: ThemeKind,
}

impl std::fmt::Debug for DecorRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecorRenderer")
            .field("lang", &self.lang)
            .finish()
    }
}

impl DecorRenderer {
    /// A renderer for a system language and theme; `None` if the toolkit's
    /// fonts or icons cannot be loaded.
    pub fn new(lang: Lang, theme: ThemeKind) -> Option<DecorRenderer> {
        Some(DecorRenderer {
            resources: Resources {
                text: TextEngine::new().ok()?,
                icons: IconCache::new().ok()?,
            },
            lang,
            theme,
        })
    }

    /// Right-to-left layout (Arabic and other RTL system languages).
    pub fn rtl(&self) -> bool {
        self.lang.direction() == Direction::Rtl
    }

    /// The ground colour of the theme (background without wallpaper).
    pub fn ground(&self) -> u32 {
        let env = PaintEnv::new(self.lang, self.theme);
        to_pixel(env.palette().get(ColorRole::Ground))
    }

    /// Shadow opacity of the theme.
    pub fn shadow_alpha(&self) -> f32 {
        match self.theme {
            ThemeKind::Dark => ultar_ui::tokens::shadow::ALPHA_DARK,
            ThemeKind::Light => ultar_ui::tokens::shadow::ALPHA_LIGHT,
        }
    }

    /// Renders a title bar (opaque, device pixels).
    pub fn render(&mut self, key: &TitleKey) -> Option<Image> {
        let scale = key.scale_milli as f32 / 1000.0;
        let height = (TITLEBAR as f32 * scale).round() as u32;
        let mut canvas = Canvas::new(key.width.max(1), height.max(1), PixelFormat::Bgrx8).ok()?;
        let mut env = PaintEnv::new(self.lang, self.theme);
        env.scale = scale;
        let rtl = self.rtl();
        env.direction = if rtl { Direction::Rtl } else { Direction::Ltr };
        let width = key.width as f32 / scale;
        let mut painter = Painter::new(&mut canvas, &mut self.resources, env);
        let bar = ultar_ui::Rect::new(0.0, 0.0, width, TITLEBAR as f32);
        let background = if key.focused {
            ColorRole::Raised
        } else {
            ColorRole::Surface
        };
        painter.fill(bar, None, painter.color(background));
        let line = ultar_ui::Rect::new(0.0, TITLEBAR as f32 - size::BORDER, width, size::BORDER);
        painter.fill(line, None, painter.color(ColorRole::Line));
        let text_role = if key.focused && !key.unresponsive {
            ColorRole::Text
        } else {
            ColorRole::TextMuted
        };
        let room = (width - 2.0 * 3.0 * BUTTON as f32).max(0.0);
        let spec = TextSpec::new(TypeSize::S14).overflow(Overflow::Ellipsis);
        let layout = painter.layout(&key.title, spec, Some(room));
        let text_rect = ultar_ui::Rect::new(3.0 * BUTTON as f32, 0.0, room, TITLEBAR as f32);
        painter.text(
            &layout,
            text_rect,
            TextAlign::Center,
            painter.color(text_role),
        );
        for (button, r) in buttons(width as i32, rtl) {
            draw_button(&mut painter, key, button, r, text_role, rtl);
        }
        Image::from_bytes(canvas.width(), canvas.height(), canvas.data(), true)
    }
}

/// Draws one title bar button (hover/press fill, icon; restore = two
/// squares when maximised).
fn draw_button(
    painter: &mut Painter<'_>,
    key: &TitleKey,
    button: Button,
    r: Rect,
    text_role: ColorRole,
    rtl: bool,
) {
    let rect = ultar_ui::Rect::new(
        r.x as f32,
        0.0,
        r.w as f32,
        (TITLEBAR as f32) - size::BORDER,
    );
    let active = key.hover == Some(button) || key.pressed == Some(button);
    let mut icon_role = text_role;
    if active {
        let fill = if button == Button::Close {
            ColorRole::Danger
        } else {
            ColorRole::Hover
        };
        painter.fill(rect, None, painter.color(fill));
        if button == Button::Close {
            icon_role = ColorRole::Ground;
        }
    }
    let icon = match button {
        Button::Minimize => IconId::Min,
        Button::Maximize => IconId::Max,
        Button::Close => IconId::Close,
    };
    let color = painter.color(icon_role);
    painter.icon(icon, rect, size::ICON_SMALL, QuarterTurns::None, color);
    if button == Button::Maximize && key.maximized {
        // Restore = two squares (DESKTOP-SPEC §4): the second square
        // one border step up and towards the end.
        let shift = 2.0 * size::BORDER * 2.0;
        let dx = if rtl { -shift } else { shift };
        let second = ultar_ui::Rect::new(rect.x + dx, rect.y - shift, rect.w, rect.h);
        painter.icon(
            icon,
            second,
            size::ICON_SMALL - 2.0 * shift,
            QuarterTurns::None,
            color,
        );
    }
}

/// Converts a toolkit colour to a premultiplied pixel.
pub fn to_pixel(c: ultar_ui::Color) -> u32 {
    let a = u32::from(c.a);
    let pm = |v: u8| (u32::from(v) * a + 127) / 255;
    a << 24 | pm(c.r) << 16 | pm(c.g) << 8 | pm(c.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_sit_at_the_end_and_mirror_in_rtl() {
        let ltr = buttons(400, false);
        assert_eq!(ltr[0], (Button::Close, Rect::new(360, 0, 40, 40)));
        assert_eq!(ltr[2], (Button::Minimize, Rect::new(280, 0, 40, 40)));
        let rtl = buttons(400, true);
        assert_eq!(rtl[0], (Button::Close, Rect::new(0, 0, 40, 40)));
        assert_eq!(rtl[2], (Button::Minimize, Rect::new(80, 0, 40, 40)));
    }

    #[test]
    fn hit_testing() {
        let h = |x, y| hit(400, 300, x, y, true, true, false);
        assert_eq!(h(200, 20), Some(Part::Title));
        assert_eq!(h(380, 20), Some(Part::Button(Button::Close)));
        assert_eq!(
            hit(400, 300, 20, 20, true, true, true),
            Some(Part::Button(Button::Close))
        );
        assert_eq!(h(200, 200), Some(Part::Content));
        assert_eq!(h(-3, 150), Some(Part::Border(4)));
        assert_eq!(h(402, 150), Some(Part::Border(8)));
        assert_eq!(h(200, 303), Some(Part::Border(2)));
        assert_eq!(h(-3, 303), Some(Part::Border(6)));
        assert_eq!(h(200, -3), Some(Part::Border(1)));
        assert_eq!(h(-30, 150), None);
        assert_eq!(hit(400, 300, -3, 150, false, false, false), None);
        assert_eq!(
            hit(400, 300, 200, 10, false, false, false),
            Some(Part::Content)
        );
    }

    #[test]
    fn title_bars_render_with_tokens() {
        let mut r = DecorRenderer::new(Lang::Ar, ThemeKind::Dark).unwrap();
        assert!(r.rtl());
        let key = TitleKey {
            width: 400,
            scale_milli: 1000,
            title: "الملفات".into(),
            focused: true,
            hover: Some(Button::Close),
            pressed: None,
            maximized: true,
            unresponsive: false,
        };
        let img = r.render(&key).unwrap();
        assert_eq!((img.width(), img.height()), (400, 40));
        // Close (hovered, danger) sits on the left in Arabic.
        let danger = to_pixel(
            PaintEnv::new(Lang::Ar, ThemeKind::Dark)
                .palette()
                .get(ColorRole::Danger),
        );
        assert_eq!(img.pixel(2, 2), Some(danger));
        assert_ne!(img.pixel(398, 2), Some(danger));
        let doubled = r
            .render(&TitleKey {
                scale_milli: 2000,
                width: 800,
                ..key.clone()
            })
            .unwrap();
        assert_eq!(doubled.height(), 80);
        let en = DecorRenderer::new(Lang::En, ThemeKind::Light).unwrap();
        assert!(!en.rtl());
        assert!(en.shadow_alpha() < r.shadow_alpha());
        assert_eq!(to_pixel(ultar_ui::Color::rgba(255, 0, 0, 128)), 0x8080_0000);
        assert_eq!(r.ground() >> 24, 0xff);
    }
}
