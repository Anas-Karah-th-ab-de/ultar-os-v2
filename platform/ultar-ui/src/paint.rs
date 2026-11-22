//! The painter: the only drawing interface widgets use. It takes logical points and colour
//! roles, applies the scale factor and the active palette, and snaps edges to device pixels.

use crate::canvas::Canvas;
use crate::color::Color;
use crate::geom::{PixelRect, Point, Rect};
use crate::icon::{IconCache, IconId, QuarterTurns};
use crate::text::{FontRole, Overflow, TextAlign, TextEngine, TextLayout, TextStyle, Weight};
use crate::tokens::{shadow, size, ColorRole, Palette, Radius, Theme, ThemeKind, TypeSize, THEME};
use std::f32::consts::TAU;
use std::rc::Rc;
use ultar_i18n::{Direction, Lang};

/// The drawing resources that live as long as a UI: text engine and icon cache.
pub struct Resources {
    /// Text shaping, layout and glyph cache.
    pub text: TextEngine,
    /// Icon outlines and masks.
    pub icons: IconCache,
}

/// What a text looks like, in token terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextSpec {
    /// Size step.
    pub size: TypeSize,
    /// Weight.
    pub weight: Weight,
    /// Font role.
    pub role: FontRole,
    /// Overflow rule.
    pub overflow: Overflow,
}

impl TextSpec {
    /// Regular text of the given size that is clipped when too long.
    pub const fn new(size: TypeSize) -> TextSpec {
        TextSpec {
            size,
            weight: Weight::Regular,
            role: FontRole::Text,
            overflow: Overflow::Clip,
        }
    }

    /// The same spec with another weight.
    pub const fn weight(mut self, weight: Weight) -> TextSpec {
        self.weight = weight;
        self
    }

    /// The same spec with another overflow rule.
    pub const fn overflow(mut self, overflow: Overflow) -> TextSpec {
        self.overflow = overflow;
        self
    }

    /// The same spec with another font role.
    pub const fn role(mut self, role: FontRole) -> TextSpec {
        self.role = role;
        self
    }
}

/// Language, direction, theme and scale a frame is painted with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintEnv {
    /// Language (decides line height and default direction).
    pub lang: Lang,
    /// Layout direction.
    pub direction: Direction,
    /// Active theme.
    pub theme: ThemeKind,
    /// Device pixels per logical point.
    pub scale: f32,
}

impl PaintEnv {
    /// The environment for a language at scale 1.
    pub fn new(lang: Lang, theme: ThemeKind) -> PaintEnv {
        PaintEnv {
            lang,
            direction: lang.direction(),
            theme,
            scale: 1.0,
        }
    }

    /// The palette of the active theme.
    pub fn palette(&self) -> &'static Palette {
        THEME.palette(self.theme)
    }

    /// The pixel text style for a spec.
    pub fn text_style(&self, spec: TextSpec) -> TextStyle {
        let mut style = TextStyle::new(spec.size.points() * self.scale, self.lang)
            .with_weight(spec.weight)
            .with_role(spec.role)
            .with_overflow(spec.overflow);
        style.direction = self.direction;
        style
    }
}

/// Shapes `text` for `spec`; `max_width` is in logical points.
pub fn layout_text(
    resources: &mut Resources,
    env: &PaintEnv,
    text: &str,
    spec: TextSpec,
    max_width: Option<f32>,
) -> Rc<TextLayout> {
    let style = env.text_style(spec);
    resources
        .text
        .layout(text, &style, max_width.map(|w| w * env.scale))
}

/// Draws one frame (or a damaged part of it) in logical points.
pub struct Painter<'a> {
    canvas: &'a mut Canvas,
    resources: &'a mut Resources,
    env: PaintEnv,
    opacity: f32,
}

impl<'a> Painter<'a> {
    /// A painter over `canvas` for the given environment.
    pub fn new(canvas: &'a mut Canvas, resources: &'a mut Resources, env: PaintEnv) -> Painter<'a> {
        Painter {
            canvas,
            resources,
            env,
            opacity: 1.0,
        }
    }

    /// The environment of this frame.
    pub fn env(&self) -> &PaintEnv {
        &self.env
    }

    /// The colour of a role in the active theme. The current opacity is applied once, by the
    /// drawing calls (`fill`, `stroke`, `line`, `spinner`, `text`, `icon`), not here: applying
    /// it in both places painted disabled widgets at opacity² (0.16 instead of 0.4).
    pub fn color(&self, role: ColorRole) -> Color {
        self.env.palette().get(role)
    }

    /// Sets the opacity applied to all following drawing (disabled widgets) and returns the
    /// previous value.
    pub fn set_opacity(&mut self, opacity: f32) -> f32 {
        std::mem::replace(&mut self.opacity, opacity.clamp(0.0, 1.0))
    }

    /// The width of one device pixel in logical points (borders and separators).
    pub fn hairline(&self) -> f32 {
        1.0 / self.env.scale
    }

    /// Converts a logical rectangle to pixels with all edges on the pixel grid.
    fn snap(&self, rect: Rect) -> Rect {
        let s = self.env.scale;
        let (x0, y0) = ((rect.x * s).round(), (rect.y * s).round());
        let (x1, y1) = ((rect.right() * s).round(), (rect.bottom() * s).round());
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    fn radius(&self, radius: Option<Radius>) -> f32 {
        radius.map_or(0.0, |r| r.points() * self.env.scale)
    }

    /// Restricts drawing to `rect` (intersected with the current clip); returns the previous
    /// clip for [`Painter::restore_clip`].
    pub fn clip(&mut self, rect: Rect) -> PixelRect {
        let previous = self.canvas.clip();
        let wanted = PixelRect::covering(&self.snap(rect)).intersect(&previous);
        self.canvas.set_clip(wanted);
        previous
    }

    /// Restores a clip returned by [`Painter::clip`].
    pub fn restore_clip(&mut self, previous: PixelRect) {
        self.canvas.set_clip(previous);
    }

    /// Fills a rectangle; `radius` rounds its corners.
    pub fn fill(&mut self, rect: Rect, radius: Option<Radius>, color: Color) {
        let r = self.radius(radius);
        self.canvas
            .fill_rrect(self.snap(rect), r, color.with_alpha(self.opacity));
    }

    /// Strokes the inside border of a rectangle with `width` logical points.
    pub fn stroke(&mut self, rect: Rect, radius: Option<Radius>, width: f32, color: Color) {
        let r = self.radius(radius);
        let w = (width * self.env.scale).max(1.0);
        self.canvas
            .stroke_rrect(self.snap(rect), r, w, color.with_alpha(self.opacity));
    }

    /// Paints the one shadow level under a floating rectangle.
    pub fn shadow(&mut self, rect: Rect, radius: Option<Radius>) {
        let s = self.env.scale;
        let r = self.radius(radius);
        let alpha = Theme::shadow_alpha(self.env.theme) * self.opacity;
        let moved = self.snap(rect.offset(0.0, shadow::OFFSET_Y));
        self.canvas
            .shadow(moved, r, shadow::BLUR * s, Color::BLACK.with_alpha(alpha));
    }

    /// Strokes a straight line with round caps.
    pub fn line(&mut self, from: Point, to: Point, width: f32, color: Color) {
        let s = self.env.scale;
        let (a, b) = (
            Point::new(from.x * s, from.y * s),
            Point::new(to.x * s, to.y * s),
        );
        self.canvas
            .line(a, b, width * s, color.with_alpha(self.opacity));
    }

    /// Draws the progress ring: three quarters of a circle inside `rect`, turned by `phase`
    /// (0..1 = one revolution).
    pub fn spinner(&mut self, rect: Rect, phase: f32, color: Color) {
        const STEPS: usize = 24;
        const SWEEP: f32 = 0.75;
        let s = self.env.scale;
        let width = size::STROKE * rect.w.min(rect.h) / size::ICON_SMALL * s;
        let radius = rect.w.min(rect.h) * s / 2.0 - width;
        let center = rect.center();
        let mut points = [Point::default(); STEPS + 1];
        for (i, p) in points.iter_mut().enumerate() {
            let angle = (phase + SWEEP * i as f32 / STEPS as f32) * TAU;
            *p = Point::new(
                center.x * s + radius * angle.cos(),
                center.y * s + radius * angle.sin(),
            );
        }
        self.canvas
            .stroke_polyline(&points, false, width, color.with_alpha(self.opacity));
    }

    /// Shapes text for this frame; `max_width` in logical points.
    pub fn layout(&mut self, text: &str, spec: TextSpec, max_width: Option<f32>) -> Rc<TextLayout> {
        layout_text(self.resources, &self.env, text, spec, max_width)
    }

    /// Draws a layout into `rect`, vertically centred, lines aligned by `align`.
    pub fn text(&mut self, layout: &TextLayout, rect: Rect, align: TextAlign, color: Color) {
        let s = self.env.scale;
        let top = ((rect.y + rect.h / 2.0) * s - layout.height / 2.0).round();
        let area = Rect::new(
            (rect.x * s).round(),
            top,
            (rect.w * s).round(),
            layout.height,
        );
        self.resources.text.draw(
            self.canvas,
            layout,
            area,
            align,
            color.with_alpha(self.opacity),
        );
    }

    /// Draws an icon of `points` edge length centred in `rect`. Directional icons are
    /// mirrored when the frame is right-to-left.
    pub fn icon(
        &mut self,
        icon: IconId,
        rect: Rect,
        points: f32,
        turns: QuarterTurns,
        color: Color,
    ) {
        let s = self.env.scale;
        let edge = (points * s).round();
        let stroke = size::STROKE * points / size::ICON * s;
        let flip = icon.mirrored() && self.env.direction.is_rtl();
        let mask = self.resources.icons.mask(icon, edge, stroke, flip, turns);
        let center = rect.center();
        let x = (center.x * s - edge / 2.0).round() as i32;
        let y = (center.y * s - edge / 2.0).round() as i32;
        // The mask is square with `edge * edge` entries by construction.
        let _ = self.canvas.fill_mask(
            x,
            y,
            mask.edge,
            mask.edge,
            &mask.data,
            color.with_alpha(self.opacity),
        );
    }
}
