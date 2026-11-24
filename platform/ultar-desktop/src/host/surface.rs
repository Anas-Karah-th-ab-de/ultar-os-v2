//! One compositor surface running one widget tree: configure → size, scale and direction;
//! compositor input → toolkit events; damaged pixels → commits. Translucent surfaces paint
//! into an RGBA canvas whose pixels are premultiplied (everything is drawn over a cleared,
//! transparent background) and go out as `Argb8888`.

use crate::view::View;
use ultar_i18n::Direction;
use ultar_surface::client::{Client, ClientError, Window};
use ultar_surface::proto::{Configure, Event, Rect, FIXED_ONE};
use ultar_surface::ui::{device_size, translate};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{Event as UiEvent, Response};
use ultar_ui::geom::{Point, Size};

/// A surface and its tree.
pub struct HostSurface {
    /// The compositor surface.
    pub window: Window,
    /// The tree and what its nodes mean.
    pub view: View,
    /// The size the client chooses (popups get no size from the compositor).
    pub fixed: Option<Size>,
    translucent: bool,
    canvas: Option<Canvas>,
    staging: Vec<u8>,
    pointer: Point,
    full: bool,
}

impl std::fmt::Debug for HostSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostSurface")
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}

fn point(x: i32, y: i32) -> Point {
    Point::new(x as f32 / FIXED_ONE as f32, y as f32 / FIXED_ONE as f32)
}

impl HostSurface {
    /// A surface for `window` showing `view`.
    pub fn new(window: Window, view: View, fixed: Option<Size>, translucent: bool) -> HostSurface {
        HostSurface {
            window,
            view,
            fixed,
            translucent,
            canvas: None,
            staging: Vec::new(),
            pointer: Point::new(0.0, 0.0),
            full: true,
        }
    }

    /// Replaces the tree (same window); everything is painted again.
    pub fn replace(&mut self, view: View, fixed: Option<Size>) {
        self.view = view;
        self.fixed = fixed;
        if let Some(c) = self.window.configure() {
            let _ = self.configure(&c);
        }
        self.full = true;
    }

    /// The size of a configure (or the client's own for popups).
    pub fn logical_size(&self, c: &Configure) -> Size {
        match self.fixed {
            Some(size) if c.width == 0 || c.height == 0 => size,
            _ => Size::new(c.width as f32, c.height as f32),
        }
    }

    /// Applies a configure to the tree and the canvas.
    pub fn configure(&mut self, c: &Configure) -> Result<(), ClientError> {
        let size = self.logical_size(c);
        let scale = c.scale_milli as f32 / 1000.0;
        let ui = &mut self.view.ui;
        ui.set_scale(scale).map_err(|_| ClientError::InvalidSize)?;
        ui.set_size(size).map_err(|_| ClientError::InvalidSize)?;
        ui.set_direction(if c.rtl {
            Direction::Rtl
        } else {
            Direction::Ltr
        });
        let sized = Configure {
            width: size.w.round() as u32,
            height: size.h.round() as u32,
            ..*c
        };
        let (w, h) = device_size(&sized);
        let format = if self.translucent {
            PixelFormat::Rgba8
        } else {
            PixelFormat::Bgrx8
        };
        let keep = self
            .canvas
            .as_ref()
            .is_some_and(|k| k.width() == w && k.height() == h);
        if !keep {
            self.canvas = Some(Canvas::new(w, h, format).map_err(|_| ClientError::InvalidSize)?);
            self.staging.clear();
            self.full = true;
        }
        Ok(())
    }

    /// Turns a compositor event of this surface into a toolkit event and handles it.
    pub fn input(&mut self, event: &Event) -> Option<(UiEvent, Response)> {
        if let Event::PointerEnter { x, y, .. } | Event::PointerMotion { x, y, .. } = event {
            self.pointer = point(*x, *y);
        }
        let ui_event = translate(event, self.pointer)?;
        let response = self.view.ui.handle(ui_event);
        Some((ui_event, response))
    }

    /// Where the pointer is (logical points of the surface).
    pub fn pointer(&self) -> Point {
        self.pointer
    }

    /// Paints and presents what changed, if the compositor is ready for a frame.
    pub fn draw(&mut self, client: &mut Client) -> Result<(), ClientError> {
        let Some(canvas) = self.canvas.as_mut() else {
            return Ok(());
        };
        if !self.window.can_draw() || !(self.full || self.view.ui.needs_paint()) {
            return Ok(());
        }
        if std::mem::take(&mut self.full) {
            self.view.ui.paint_all(canvas);
        } else {
            self.view.ui.paint(canvas);
        }
        let damage = canvas.take_damage();
        let rects: Vec<Rect> = damage
            .rects()
            .iter()
            .map(|r| {
                Rect::new(
                    r.x0,
                    r.y0,
                    (r.x1 - r.x0).max(0) as u32,
                    (r.y1 - r.y0).max(0) as u32,
                )
            })
            .filter(|r| r.width > 0 && r.height > 0)
            .collect();
        let size = (canvas.width(), canvas.height());
        if !self.translucent {
            return self.window.present(client, canvas.data(), size, &rects);
        }
        // RGBA (premultiplied) → BGRA = little-endian 0xAARRGGBB, only the damaged rows.
        let data = canvas.data();
        if self.staging.len() != data.len() {
            self.staging = data.to_vec();
        }
        let row = size.0 as usize * 4;
        for r in &rects {
            for y in r.y as usize..(r.y as usize + r.height as usize) {
                let start = y * row + r.x as usize * 4;
                let end = start + r.width as usize * 4;
                let (Some(src), Some(dst)) =
                    (data.get(start..end), self.staging.get_mut(start..end))
                else {
                    continue;
                };
                for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                    d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
                }
            }
        }
        self.window.present(client, &self.staging, size, &rects)
    }
}
