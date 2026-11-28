//! The shell's panels by its shape: the bar (bottom), or the top bar and the dock –
//! opened, rebuilt and replaced when the shape changes.

use super::{screen, DesktopApp};
use crate::app::AppError;
use crate::model::Shape;
use crate::shell::{bar_panel, build_bar, build_second, second_panel, Outcome};
use ultar_surface::client::WindowConfig;
use ultar_surface::proto::{Request, Role, SurfaceId};
use ultar_ui::geom::Size;
use ultar_ui::tokens::size;

/// The surfaces besides the bar: the dock (top-bar shape) and the desktop background.
#[derive(Debug, Default)]
pub(super) struct SidePanels {
    /// The dock.
    pub(super) dock: Option<SurfaceId>,
    /// The desktop background and what it was built for.
    pub(super) background: Option<(SurfaceId, BackgroundKey)>,
    /// The shortcut the background shows selected.
    pub(super) selected: Option<usize>,
}

/// What the desktop background was built for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct BackgroundKey {
    look: crate::view::Look,
    size: (u32, u32),
    shape: Shape,
    /// What its shortcuts show ([`desk_mark`]).
    desk: u64,
}

/// A mark of what the shortcuts on the background show: the list, their settings and the
/// installed apps (the selection changes in place, see `select_shortcut`).
fn desk_mark(d: &crate::model::Desktop) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    d.places.shortcuts.hash(&mut h);
    d.prefs.desk.hash(&mut h);
    for app in &d.catalog {
        (&app.id, app.icon as u32).hash(&mut h);
    }
    h.finish()
}

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
    /// A panel besides the bar (the dock).
    pub(super) fn is_panel(&self, id: SurfaceId) -> bool {
        self.side.dock == Some(id)
    }

    /// The desktop background.
    pub(super) fn is_background(&self, id: SurfaceId) -> bool {
        self.side.background.is_some_and(|(b, _)| b == id)
    }

    /// Opens the second panel of the shape.
    pub(super) fn open_side_panels(&mut self) -> Result<(), AppError> {
        let area = screen(&self.host);
        if let Some((edge, thickness, translucent)) = second_panel(self.shell.shape) {
            if let Some(view) = build_second(&self.shell, self.look, area.w)? {
                let role = Role::Panel {
                    edge,
                    size: thickness as u32,
                };
                let config = WindowConfig::with_role(role, "", translucent);
                self.side.dock = Some(self.host.open(&config, view, None)?);
            }
        }
        // The desktop never uses the compositor's phone mode (app windows without title
        // bars, edge gestures): that is the phone shell's (`ultar-mobile`).
        self.host
            .client
            .send(&Request::SetPhoneMode { on: false })?;
        Ok(())
    }

    /// Builds the second panel again.
    pub(super) fn rebuild_side_panels(&mut self, area: Size) -> Result<(), AppError> {
        if let Some(id) = self.side.dock {
            if let Some(view) = build_second(&self.shell, self.look, area.w)? {
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
        let banners = self.banners.surface.take().map(|b| b.0);
        for id in [popup, banners, self.side.dock.take()]
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
        self.sync_background()?;
        self.sync_banners()
    }

    /// Opens the desktop background, or builds it again when what it shows changed (look,
    /// screen, shape, shortcuts); a minute of the clock leaves it alone.
    pub(super) fn sync_background(&mut self) -> Result<(), AppError> {
        let area = screen(&self.host);
        let key = BackgroundKey {
            look: self.look,
            size: (area.w.round() as u32, area.h.round() as u32),
            shape: self.shell.shape,
            desk: desk_mark(&self.shell.d),
        };
        let build = |app: &Self| {
            crate::view::wallpaper::build(&app.shell.d, app.look, area, key.shape, &app.shell.tr)
        };
        if let Some((id, shown)) = self.side.background {
            if shown == key {
                self.select_shortcut(id);
                return Ok(());
            }
            let view = build(self)?;
            self.host.replace(id, view, Some(area));
            self.side.background = Some((id, key));
        } else {
            let view = build(self)?;
            let config = WindowConfig::with_role(Role::Background, "", false);
            let id = self.host.open(&config, view, Some(area))?;
            self.side.background = Some((id, key));
        }
        self.side.selected = self.shell.d.places.selected;
        Ok(())
    }

    /// Another shortcut was selected: its brick (and the one before) change in place, so the
    /// second click of a double click still lands on the same tree.
    fn select_shortcut(&mut self, id: SurfaceId) {
        let selected = self.shell.d.places.selected;
        if self.side.selected == selected {
            return;
        }
        self.side.selected = selected;
        let Some(view) = self.host.view_mut(id) else {
            return;
        };
        let nodes: Vec<(ultar_ui::ui::NodeId, usize)> = view
            .targets
            .iter()
            .filter_map(|(n, t)| match t {
                crate::view::Target::Shortcut(i) => Some((*n, *i)),
                _ => None,
            })
            .collect();
        for (node, index) in nodes {
            let Some(ultar_ui::widgets::Widget::Brick(brick)) = view.ui.widget(node) else {
                continue;
            };
            if let ultar_ui::widgets::BrickKind::DeskIcon { icon, side, .. } = brick.kind {
                let kind = ultar_ui::widgets::BrickKind::DeskIcon {
                    icon,
                    side,
                    selected: selected == Some(index),
                };
                let _ = view.ui.set_brick(node, kind);
            }
        }
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
