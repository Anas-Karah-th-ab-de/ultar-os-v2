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

    /// Switches the accent colour (`appearance.accent_color`).
    pub fn set_accent(&mut self, accent: crate::tokens::Accent) {
        if self.env.accent != accent {
            self.env.accent = accent;
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

    /// accessibility.high_contrast: the high-contrast palette over the theme.
    pub fn set_high_contrast(&mut self, on: bool) {
        if self.env.high_contrast != on {
            self.env.high_contrast = on;
            self.dirty.full = true;
            self.layers.backdrop.ready = false;
        }
    }

    /// accessibility.large_text: text this many times larger (1 to 2; 1 = off). Laid out again.
    pub fn set_text_scale(&mut self, factor: f32) -> Result<(), UiError> {
        if !(factor.is_finite() && (1.0..=2.0).contains(&factor)) {
            return Err(UiError::InvalidScale);
        }
        if self.env.text_scale != factor {
            self.env.text_scale = factor;
            self.invalidate_layout();
        }
        Ok(())
    }

    /// Text a field put on the clipboard (Ctrl+C, Ctrl+X) since the last call; the window offers
    /// it to the compositor. Never the content of a password field.
    pub fn take_copy(&mut self) -> Option<String> {
        self.clipboard.0.take()
    }

    /// `true` once after a field asked for the clipboard (Ctrl+V); the window then reads the
    /// clipboard and hands the text to [`Ui::paste`].
    pub fn take_paste(&mut self) -> bool {
        self.clipboard
            .1
            .is_some_and(|id| self.focused() == Some(id))
            && std::mem::take(&mut self.clipboard.1).is_some()
    }

    /// The clipboard's text for the focused field that asked for it (Ctrl+V); its actions.
    pub fn paste(&mut self, text: &str) -> Vec<crate::event::Action> {
        let mut out = Vec::new();
        let Some(id) = self.focused() else {
            return out;
        };
        if let Ok(field) = self.text_field_mut(id) {
            if field.paste(text) {
                out.push(crate::event::Action {
                    node: id,
                    kind: crate::event::ActionKind::TextChanged,
                });
            }
            self.touch(id);
        }
        out
    }

    /// "Reduce motion": knobs jump and progress rings stand still.
    pub fn set_reduce_motion(&mut self, reduce: bool) {
        if self.access.reduce_motion != reduce {
            self.access.reduce_motion = reduce;
            self.dirty.full = true;
            self.layers.backdrop.ready = false;
        }
    }

    pub(crate) fn reduce_motion(&self) -> bool {
        self.access.reduce_motion
    }

    /// `accessibility.double_click_ms`: two clicks of a double click may be this many
    /// milliseconds apart ([`MIN_DOUBLE_CLICK_MS`] to [`MAX_DOUBLE_CLICK_MS`]; anything
    /// else falls back to the token `motion.double_click`, 400 ms).
    pub fn set_double_click_ms(&mut self, ms: u16) {
        self.access.double_click_ms = if (MIN_DOUBLE_CLICK_MS..=MAX_DOUBLE_CLICK_MS).contains(&ms) {
            ms
        } else {
            crate::tokens::motion::DOUBLE_CLICK as u16
        };
    }

    /// The double-click interval in effect, in milliseconds.
    pub fn double_click_ms(&self) -> u16 {
        self.access.double_click_ms
    }
}

/// Shortest double-click interval a window accepts (`accessibility.double_click_ms`), ms.
pub const MIN_DOUBLE_CLICK_MS: u16 = 200;
/// Longest double-click interval a window accepts, ms.
pub const MAX_DOUBLE_CLICK_MS: u16 = 1000;
