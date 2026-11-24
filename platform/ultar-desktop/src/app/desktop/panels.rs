//! The shell's panels by its shape: the bar (bottom), the top bar and the dock, or the
//! phone's status bar, gesture bar and home screen – opened, rebuilt and replaced when
//! the shape changes.

use super::{screen, DesktopApp};
use crate::app::AppError;
use crate::model::Shape;
use crate::shell::{bar_panel, build_bar, build_home, build_second, second_panel, Outcome};
use ultar_surface::client::WindowConfig;
use ultar_surface::proto::{Request, Role, SurfaceId};
use ultar_ui::geom::Size;
use ultar_ui::tokens::size;

/// The window configuration of the first panel.
pub(super) fn bar_config(shape: Shape) -> WindowConfig {
    let (edge, thickness) = bar_panel(shape);
    let role = Role::Panel {
        edge,
        size: thickness as u32,
    };
    WindowConfig::with_role(role, "", false)
}

impl DesktopApp {
    /// A panel besides the bar (the dock, the gesture bar, the home screen).
    pub(super) fn is_panel(&self, id: SurfaceId) -> bool {
        self.second == Some(id) || self.home == Some(id)
    }

    /// Opens the second panel and the home screen of the shape.
    pub(super) fn open_side_panels(&mut self) -> Result<(), AppError> {
        let area = screen(&self.host);
        if let Some((edge, thickness, translucent)) = second_panel(self.shell.shape) {
            if let Some(view) = build_second(&self.shell, self.look, area.w)? {
                let role = Role::Panel {
                    edge,
                    size: thickness as u32,
                };
                let config = WindowConfig::with_role(role, "", translucent);
                self.second = Some(self.host.open(&config, view, None)?);
            }
        }
        if let Some(view) = build_home(&self.shell, self.look, area)? {
            let config = WindowConfig::with_role(Role::Background, "", false);
            self.home = Some(self.host.open(&config, view, None)?);
        }
        // Phone mode of the compositor: app windows without title bars over the work area,
        // the edge gestures for the shell (mobile.home, back).
        let on = self.shell.shape.is_compact();
        self.host.client.send(&Request::SetPhoneMode { on })?;
        Ok(())
    }

    /// Builds the second panel and the home screen again.
    pub(super) fn rebuild_side_panels(&mut self, area: Size) -> Result<(), AppError> {
        if let Some(id) = self.second {
            if let Some(view) = build_second(&self.shell, self.look, area.w)? {
                self.host.replace(id, view, None);
            }
        }
        if let Some(id) = self.home {
            if let Some(view) = build_home(&self.shell, self.look, area)? {
                self.host.replace(id, view, None);
            }
        }
        Ok(())
    }

    /// The screen changed its size: everything is built for the new one.
    pub(super) fn resized(&mut self) -> Result<Outcome, AppError> {
        Ok(Outcome::surfaces())
    }

    /// Closes every surface and opens the panels of another shape (the setting
    /// `bar.layout` changed).
    pub(super) fn reshape(&mut self, shape: Shape) -> Result<(), AppError> {
        if shape == self.shell.shape {
            return Ok(());
        }
        self.shell.d.open = crate::model::Open::None;
        let popup = self.popup.take().map(|p| p.id);
        let banners = self.banners.take().map(|b| b.0);
        for id in [popup, banners, self.second.take(), self.home.take()]
            .into_iter()
            .flatten()
        {
            self.host.close(id)?;
        }
        self.host.close(self.bar)?;
        self.shell.shape = shape;
        let view = build_bar(&self.shell, self.look, screen(&self.host).w)?;
        self.bar = self.host.open(&bar_config(shape), view, None)?;
        self.open_side_panels()?;
        self.sync_banners()
    }

    /// How far the banners move down when the first panel is at the top: popups are
    /// placed relative to that panel, and the banners stay clear of it.
    pub(super) fn banner_shift(&self) -> i32 {
        let (edge, thickness) = bar_panel(self.shell.shape);
        if edge == ultar_surface::proto::Edge::Top {
            (screen(&self.host).h - size::BAR_HEIGHT + thickness).round() as i32
        } else {
            0
        }
    }
}
