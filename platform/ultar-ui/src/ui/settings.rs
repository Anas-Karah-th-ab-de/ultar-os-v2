//! Surface settings: size, language, direction, theme, scale, input mode, background,
//! reduced motion.

use super::{Ui, UiError};
use crate::geom::Size;
use crate::tokens::{ColorRole, InputMode, ThemeKind};
use ultar_i18n::{Direction, Lang};

impl Ui {
    /// Resizes the surface.
    ///
    /// Sizes must be finite and not negative (`UiError::InvalidSize`); measurements are
    /// kept (they depend on the available width, which they remember).
    pub fn set_size(&mut self, size: Size) -> Result<(), UiError> {
        let valid = |v: f32| v.is_finite() && v >= 0.0;
        if !(valid(size.w) && valid(size.h)) {
            return Err(UiError::InvalidSize);
        }
        if size != self.size {
            self.size = size;
            self.dirty.layout = true;
            self.dirty.full = true;
            self.layers.backdrop.ready = false;
        }
        Ok(())
    }

    /// Switches language; the direction follows the language.
    pub fn set_lang(&mut self, lang: Lang) {
        self.env.lang = lang;
        self.env.direction = lang.direction();
        self.tr.set_lang(lang);
        self.invalidate_layout();
    }

    /// Overrides the layout direction without changing the language. Nothing is laid out
    /// again: logical rectangles are resolved for the new direction when painting. An open
    /// menu or tooltip closes (its place depends on the direction).
    pub fn set_direction(&mut self, direction: Direction) {
        if self.env.direction != direction {
            self.layers.backdrop.ready = false;
            self.env.direction = direction;
            self.layers.menu = None;
            self.layers.tip = None;
            self.dirty.full = true;
            // Mirrored: something else lies under the resting pointer now.
            self.rehover();
        }
    }

    /// Switches the theme.
    pub fn set_theme(&mut self, theme: ThemeKind) {
        if self.env.theme != theme {
            self.env.theme = theme;
            self.dirty.full = true;
            self.layers.backdrop.ready = false;
        }
    }

    /// Sets the device pixels per logical point (0.5 to 8).
    pub fn set_scale(&mut self, scale: f32) -> Result<(), UiError> {
        if !(scale.is_finite() && (0.5..=8.0).contains(&scale)) {
            return Err(UiError::InvalidScale);
        }
        self.env.scale = scale;
        self.invalidate_layout();
        Ok(())
    }

    /// Switches between pointer and touch target sizes.
    pub fn set_input_mode(&mut self, mode: InputMode) {
        if self.mode != mode {
            self.mode = mode;
            self.invalidate_layout();
        }
    }

    /// The colour role painted behind the tree.
    pub fn set_background(&mut self, role: ColorRole) {
        self.background = Some(role);
        self.dirty.full = true;
        self.layers.backdrop.ready = false;
    }

    /// Nothing behind the tree: every region starts fully transparent (an `Rgba8` canvas
    /// then holds premultiplied pixels of what the tree paints – a floating sheet with its
    /// shadow and round corners on a translucent surface).
    pub fn set_transparent_background(&mut self) {
        self.background = None;
        self.dirty.full = true;
        self.layers.backdrop.ready = false;
    }

    pub(crate) fn background(&self) -> Option<ColorRole> {
        self.background
    }

    /// "Reduce motion": knobs jump and progress rings stand still.
    pub fn set_reduce_motion(&mut self, reduce: bool) {
        if self.reduce_motion != reduce {
            self.reduce_motion = reduce;
            self.dirty.full = true;
            self.layers.backdrop.ready = false;
        }
    }

    pub(crate) fn reduce_motion(&self) -> bool {
        self.reduce_motion
    }
}
