//! The shell's panels by its shape: the bar (bottom), or the top bar and the dock –
//! opened, rebuilt and replaced when the shape changes.

use super::{screen, DesktopApp};
use crate::app::AppError;
use crate::model::metrics::{Sampler, SAMPLE_MS};
use crate::model::places::DeskItem;
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
    /// The icon the background shows selected.
    pub(super) selected: Option<DeskItem>,
    /// The desktop folder (`--desk-folder`): its items are shown after the shortcuts.
    pub(super) folder: Option<std::path::PathBuf>,
    /// When the desktop folder was last read (ms since the start).
    pub(super) scanned: Option<u64>,
    /// Reads the system display's values; when it last did (ms since the start).
    pub(super) sampler: Option<(Sampler, u64)>,
}

impl SidePanels {
    /// No surfaces yet; `folder`: the desktop folder whose items the background shows.
    pub(super) fn new(folder: Option<std::path::PathBuf>) -> SidePanels {
        SidePanels {
            folder,
            ..SidePanels::default()
        }
    }
}

/// How often the desktop folder is read again (a new, renamed or removed item shows).
const SCAN_MS: u64 = 2000;

/// What the desktop background was built for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct BackgroundKey {
    look: crate::view::Look,
    size: (u32, u32),
    shape: Shape,
    /// What its shortcuts show ([`desk_mark`]).
    desk: u64,
}

/// A mark of what the icons on the background show: shortcuts, folder items, drives, the
/// app dragged out of the grid, their settings and the installed apps (the selection
/// changes in place, see `select_icon`).
fn desk_mark(d: &crate::model::Desktop) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    d.places.shortcuts.hash(&mut h);
    d.places.files.hash(&mut h);
    d.places.dragging.hash(&mut h);
    for drive in d.svc.drives.iter().flatten() {
        (&drive.id, &drive.name).hash(&mut h);
    }
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
        let toasts = self.banners.toasts.surface.take().map(|b| b.0);
        self.banners.toasts = Default::default();
        for id in [popup, banners, toasts, self.side.dock.take()]
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
        self.sync_banners()?;
        self.sync_mesh_toasts()
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
                self.select_icon(id);
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

    /// Another icon was selected: its brick (and the one before) change in place, so the
    /// second click of a double click still lands on the same tree.
    fn select_icon(&mut self, id: SurfaceId) {
        let selected = self.shell.d.places.selected;
        if self.side.selected == selected {
            return;
        }
        self.side.selected = selected;
        let Some(view) = self.host.view_mut(id) else {
            return;
        };
        let nodes: Vec<(ultar_ui::ui::NodeId, DeskItem)> = view
            .targets
            .iter()
            .filter_map(|(n, t)| match t {
                crate::view::Target::Desk(item) => Some((*n, *item)),
                _ => None,
            })
            .collect();
        for (node, item) in nodes {
            let Some(ultar_ui::widgets::Widget::Brick(brick)) = view.ui.widget(node) else {
                continue;
            };
            if let ultar_ui::widgets::BrickKind::DeskIcon { icon, side, .. } = brick.kind {
                let kind = ultar_ui::widgets::BrickKind::DeskIcon {
                    icon,
                    side,
                    selected: selected == Some(item),
                };
                let _ = view.ui.set_brick(node, kind);
            }
        }
    }

    /// Reads the system display's values every [`SAMPLE_MS`] while it is shown and changes
    /// its texts and charts in place.
    pub(super) fn sample_metrics(&mut self) {
        if !self.shell.d.prefs.desk.metrics {
            return;
        }
        let now = self.now_ms();
        let (sampler, last) = self
            .side
            .sampler
            .get_or_insert_with(|| (Sampler::new("/"), 0));
        if *last != 0 && now.saturating_sub(*last) < SAMPLE_MS {
            return;
        }
        *last = now.max(1);
        sampler.sample(now, &mut self.shell.d.places.metrics);
        let Some((id, _)) = self.side.background else {
            return;
        };
        let Some(view) = self.host.view_mut(id) else {
            return;
        };
        let (m, tr) = (&self.shell.d.places.metrics, &self.shell.tr);
        let nodes: Vec<(ultar_ui::ui::NodeId, crate::view::Target)> = view
            .targets
            .iter()
            .filter(|(_, t)| {
                matches!(
                    t,
                    crate::view::Target::MetricValue(_) | crate::view::Target::MetricGraph(_)
                )
            })
            .cloned()
            .collect();
        for (node, target) in nodes {
            let _ = match target {
                crate::view::Target::MetricValue(g) => view
                    .ui
                    .set_label(node, crate::view::metrics::value(m, g, tr)),
                crate::view::Target::MetricGraph(g) => {
                    view.ui.set_brick(node, crate::view::metrics::meter(m, g))
                }
                _ => Ok(()),
            };
        }
    }

    /// Reads the desktop folder again every [`SCAN_MS`]; a change shows on the background.
    pub(super) fn scan_desk_folder(&mut self) -> Result<(), AppError> {
        let Some(dir) = self.side.folder.clone() else {
            return Ok(());
        };
        let now = self.now_ms();
        if self
            .side
            .scanned
            .is_some_and(|t| now.saturating_sub(t) < SCAN_MS)
        {
            return Ok(());
        }
        self.side.scanned = Some(now);
        let files = crate::model::desk_files::list(&dir);
        let places = &mut self.shell.d.places;
        if files == places.files {
            return Ok(());
        }
        if matches!(places.selected, Some(DeskItem::File(_))) {
            places.selected = None;
        }
        places.files = files;
        self.sync_background()
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
