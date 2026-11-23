//! The painter: the only drawing interface widgets use. It takes logical points and colour
//! roles, applies the scale factor and the active palette, and snaps edges to device pixels.

use crate::canvas::Canvas;
use crate::color::Color;
use crate::geom::{PixelRect, Point, Rect};
use crate::icon::{IconCache, IconId, QuarterTurns};
use crate::text::{FontRole, Overflow, TextAlign, TextEngine, TextLayout, TextStyle, Weight};
use crate::tokens::{
    shadow, size, typography, ColorRole, Palette, Radius, Theme, ThemeKind, TypeSize, THEME,
};
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

    /// A title: bold, in the title family (`family_arabic_title` for Arabic).
    pub const fn title(size: TypeSize) -> TextSpec {
        TextSpec::new(size)
            .weight(Weight::Bold)
            .role(FontRole::Title)
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
    /// Arabic smaller than the body size is drawn one weight step heavier (token
    /// `type.small_weight_arabic`): Naskh at 12 points has 30 % less ink than Latin.
    pub fn text_style(&self, spec: TextSpec) -> TextStyle {
        let small_arabic = self.lang == Lang::Ar && spec.size < typography::BODY;
        let weight = if small_arabic && spec.weight == Weight::Regular {
            Weight::from_value(typography::SMALL_WEIGHT_ARABIC)
        } else {
            spec.weight
        };
        let mut style = TextStyle::new(spec.size.points() * self.scale, self.lang)
            .with_weight(weight)
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

/// Shapes `text` for `spec` as a paragraph of direction `base` (instead of the direction of
/// the user interface); `max_width` is in logical points.
pub fn layout_directed(
    resources: &mut Resources,
    env: &PaintEnv,
    text: &str,
    spec: TextSpec,
    max_width: Option<f32>,
    base: Direction,
) -> Rc<TextLayout> {
    let mut style = env.text_style(spec);
    style.direction = base;
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

    /// The width of borders and separators in logical points (token `size.border`).
    pub fn hairline(&self) -> f32 {
        size::BORDER
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

    /// The part of the surface that is drawn now (the current clip), in logical points.
    /// Widgets with many rows draw only the rows inside it.
    pub fn visible(&self) -> Rect {
        let clip = self.canvas.clip().to_rect();
        let s = self.env.scale;
        Rect::new(clip.x / s, clip.y / s, clip.w / s, clip.h / s)
    }

    /// Copies `rect` (logical points) of `source`, a canvas of the same size, into the
    /// canvas – pixels are replaced, not blended (the backdrop under a dialog).
    pub fn copy_from(&mut self, source: &Canvas, rect: Rect) {
        let area = PixelRect::covering(&self.snap(rect)).intersect(&self.canvas.clip());
        self.canvas.blit(source, area, area.x0, area.y0);
    }

    /// Restores a clip returned by [`Painter::clip`].
    pub fn restore_clip(&mut self, previous: PixelRect) {
        self.canvas.set_clip(previous);
    }

    /// Fills a rectangle; `radius` rounds its corners.
    pub fn fill(&mut self, rect: Rect, radius: Option<Radius>, color: Color) {
        let r = self.radius(radius);
        let device = self.snap(rect);
        let color = color.with_alpha(self.opacity);
        // A square rectangle on whole pixels (the background of a region) is covered fully:
        // the same pixels by the row blend, which a large surface shares among the cores.
        let whole = |v: f32| (v - v.round()).abs() < 1e-3;
        let aligned = [device.x, device.y, device.right(), device.bottom()]
            .into_iter()
            .all(whole);
        if r <= 0.0 && aligned && !device.is_empty() {
            let area = PixelRect::new(
                device.x.round() as i32,
                device.y.round() as i32,
                device.right().round() as i32,
                device.bottom().round() as i32,
            );
            self.canvas.blend_rect(area, color);
        } else {
            self.canvas.fill_rrect(device, r, color);
        }
    }

    /// Blends `color` over a rectangle whose edges lie on device pixels (a damage region):
    /// the same pixels as [`Painter::fill`] without corners, for large areas faster.
    pub fn dim(&mut self, rect: Rect, color: Color) {
        let r = self.snap(rect);
        let area = PixelRect::new(
            r.x.round() as i32,
            r.y.round() as i32,
            r.right().round() as i32,
            r.bottom().round() as i32,
        );
        self.canvas.blend_rect(area, color.with_alpha(self.opacity));
    }

    /// Strokes the inside border of a rectangle with `width` logical points.
    pub fn stroke(&mut self, rect: Rect, radius: Option<Radius>, width: f32, color: Color) {
        let r = self.radius(radius);
        let w = (width * self.env.scale).max(1.0);
        self.canvas
            .stroke_rrect(self.snap(rect), r, w, color.with_alpha(self.opacity));
    }

    /// Paints the one shadow level under a floating rectangle: the equivalent of the CSS
    /// `box-shadow: 0 offset_y blur` of the artboards. A CSS blur of `blur` fades from full
    /// to nothing across `2 × blur`, centred on the (offset) outline, so the shadow reaches
    /// `blur` points beside and `offset_y + blur` below the rectangle.
    ///
    /// Every caller fills `rect` with an opaque panel right after; at full opacity the part
    /// that panel covers (inside the rounded corners) is not shaded at all.
    pub fn shadow(&mut self, rect: Rect, radius: Option<Radius>) {
        let s = self.env.scale;
        let r = self.radius(radius);
        let alpha = Theme::shadow_alpha(self.env.theme) * self.opacity;
        let moved = self.snap(rect.offset(0.0, shadow::OFFSET_Y));
        let fade = 2.0 * shadow::BLUR * s;
        let color = Color::BLACK.with_alpha(alpha);
        if self.opacity < 1.0 {
            self.canvas.shadow(moved, r, fade, color);
            return;
        }
        let inner = self.snap(rect);
        let hole = PixelRect::new(
            (inner.x + r).ceil() as i32 + 1,
            (inner.y + r).ceil() as i32 + 1,
            (inner.right() - r).floor() as i32 - 1,
            (inner.bottom() - r).floor() as i32 - 1,
        );
        if hole.is_empty() {
            self.canvas.shadow(moved, r, fade, color);
        } else {
            self.canvas.shadow_around(moved, r, fade, color, hole);
        }
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
        const SWEEP: f32 = crate::tokens::state::SPINNER_SWEEP;
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

    /// Shapes text as a paragraph of direction `base`; `max_width` in logical points.
    pub fn layout_directed(
        &mut self,
        text: &str,
        spec: TextSpec,
        max_width: Option<f32>,
        base: Direction,
    ) -> Rc<TextLayout> {
        layout_directed(self.resources, &self.env, text, spec, max_width, base)
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
