//! The accessibility view of the output: the magnifier and colour filter the
//! display service configures (`Request::ConfigureVisual`, right
//! `ui.output-config`), and the visual-alert flash the shell asks for
//! (`Request::VisualAlert`, right `ui.shell`). The render path applies them to a
//! copy of the composed frame (`crate::effects`); this module keeps the values
//! and the flash deadline and damages the outputs when they change.

use super::{ClientId, State};
use ultar_i18n::Lang;
use ultar_surface::proto::{
    ColourFilter, ErrorCode, LookLang, Request, Rights, DEFAULT_ZOOM_PERCENT,
};
use ultar_ui::tokens::ThemeKind;

/// The accessibility view configuration and the flash deadline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Visual {
    /// The magnifier is on (`accessibility.zoom`).
    pub zoom: bool,
    /// Magnification in percent (`accessibility.zoom_percent`).
    pub zoom_percent: u16,
    /// The colour filter (`accessibility.colour_filter`).
    pub filter: ColourFilter,
    /// The visual-alert flash shows until this time (ms; 0 = none).
    pub flash_until: u64,
}

impl Default for Visual {
    /// The settings schema's defaults: no magnifier, no filter, no flash.
    fn default() -> Self {
        Visual {
            zoom: false,
            zoom_percent: DEFAULT_ZOOM_PERCENT,
            filter: ColourFilter::None,
            flash_until: 0,
        }
    }
}

impl Visual {
    /// The magnifier scales the output (on and above 100 %).
    pub fn zoom_active(&self) -> bool {
        self.zoom && self.zoom_percent > 100
    }

    /// Anything that changes the frame presented at `now` (magnifier, filter or
    /// flash): the render path then transforms the whole frame.
    pub fn active(&self, now: u64) -> bool {
        self.zoom_active() || self.filter != ColourFilter::None || now < self.flash_until
    }
}

impl State {
    /// One of the accessibility view requests, checked against the client's right.
    pub(super) fn visual_request(&mut self, client: ClientId, r: Request) -> Result<(), ErrorCode> {
        match r {
            Request::ConfigureVisual {
                zoom,
                zoom_percent,
                filter,
            } => {
                self.require_visual_permission(client, Rights::OUTPUT_CONFIG)?;
                let v = &mut self.session.visual;
                (v.zoom, v.zoom_percent, v.filter) = (zoom, zoom_percent, filter);
                self.damage_all();
                Ok(())
            }
            Request::VisualAlert { ms } => {
                self.require_visual_permission(client, Rights::SHELL)?;
                self.session.visual.flash_until = self.now.saturating_add(u64::from(ms));
                self.damage_all();
                Ok(())
            }
            Request::ConfigureLook { lang, light } => {
                self.require_visual_permission(client, Rights::OUTPUT_CONFIG)?;
                self.set_look(lang, light);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// The language and theme of the title bars, dialog frames and the ground
    /// (`region.language`, `appearance.theme`): every title bar is drawn again, on the side
    /// the language reads from.
    pub(super) fn set_look(&mut self, lang: LookLang, light: bool) {
        let lang = match lang {
            LookLang::Ar => Lang::Ar,
            LookLang::En => Lang::En,
            LookLang::De => Lang::De,
        };
        let theme = if light {
            ThemeKind::Light
        } else {
            ThemeKind::Dark
        };
        if self.config.lang == lang && self.config.theme == theme && self.decor.is_some() {
            return;
        }
        self.config.lang = lang;
        self.config.theme = theme;
        self.decor = crate::decor::DecorRenderer::new(lang, theme);
        for w in &mut self.windows {
            w.title.cache = None;
        }
        self.damage_all();
    }

    fn require_visual_permission(&self, client: ClientId, needed: Rights) -> Result<(), ErrorCode> {
        match self.clients.get(&client) {
            Some(c) if c.rights.contains(needed) => Ok(()),
            Some(_) => Err(ErrorCode::Permission),
            None => Err(ErrorCode::UnknownObject),
        }
    }

    /// Ends the visual-alert flash when its time is over (from `tick_outputs`).
    pub(super) fn tick_visual(&mut self) {
        let until = self.session.visual.flash_until;
        if until > 0 && self.now >= until {
            self.session.visual.flash_until = 0;
            self.damage_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_magnifier_needs_more_than_full_size() {
        let mut v = Visual::default();
        assert!(!v.active(0), "defaults change nothing");
        v.zoom = true;
        v.zoom_percent = 100;
        assert!(!v.zoom_active(), "100 % is the output as it is");
        v.zoom_percent = 200;
        assert!(v.zoom_active() && v.active(0));
        v.zoom = false;
        assert!(!v.zoom_active());
        v.filter = ColourFilter::Greyscale;
        assert!(v.active(0), "a filter alone changes the frame");
        v.filter = ColourFilter::None;
        v.flash_until = 100;
        assert!(
            v.active(99) && !v.active(100),
            "the flash shows until its deadline"
        );
    }
}
