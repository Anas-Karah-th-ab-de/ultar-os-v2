//! The mesh toasts' surface (board MeshStates): a popup of the bar that never takes the
//! keyboard, rebuilt when the toasts change; the toolkit reports a button (Cancel, Retry,
//! Open) and the closing of a toast through its root node.

use super::{screen, DesktopApp};
use crate::app::AppError;
use crate::model::mesh::Toast;
use crate::shell::Outcome;
use crate::view::mesh_toasts;
use ultar_surface::client::WindowConfig;
use ultar_surface::proto::{Edge, Role, SurfaceId};
use ultar_ui::event::ActionKind;

/// The toasts' surface and what it shows.
#[derive(Debug, Default)]
pub(super) struct ToastSurface {
    /// The surface and its offset.
    pub(super) surface: Option<(SurfaceId, (i32, i32))>,
    /// The toasts it was built for: it is built again only when they change (a rebuild
    /// starts the toolkit's clocks again).
    shown: Vec<Toast>,
}

impl DesktopApp {
    /// Shows, updates or removes the mesh toasts.
    pub(super) fn sync_mesh_toasts(&mut self) -> Result<(), AppError> {
        let want = &self.shell.d.svc.mesh.toasts;
        if *want == self.mesh_toasts.shown {
            return Ok(());
        }
        let now = self.host.now_ms();
        let top_bar = crate::shell::bar_panel(self.shell.shape).0 == Edge::Top;
        let built = mesh_toasts::build(
            &self.shell.d,
            (self.look, screen(&self.host), now),
            top_bar,
            &self.shell.tr,
        )?;
        self.mesh_toasts.shown = want.clone();
        match (built, self.mesh_toasts.surface) {
            (None, Some((id, _))) => {
                self.mesh_toasts.surface = None;
                self.host.close(id)?;
            }
            (None, None) => {}
            (Some((view, place)), old) => {
                self.shell.d.svc.mesh.built_at = self.shell.clock.now_ms();
                if let Some((id, offset)) = old {
                    if offset == place.offset {
                        self.host.replace(id, view, Some(place.surface));
                        return Ok(());
                    }
                    self.host.close(id)?;
                }
                let role = Role::Popup {
                    parent: self.bar,
                    x: place.offset.0,
                    y: place.offset.1,
                };
                let config = WindowConfig::with_role(role, "", true);
                let id = self.host.open(&config, view, Some(place.surface))?;
                self.mesh_toasts.surface = Some((id, place.offset));
            }
        }
        Ok(())
    }

    /// The toolkit reported a toast of the mesh: its button, or its closing.
    pub(super) fn on_mesh_toast(&mut self, kind: &ActionKind) -> Outcome {
        let now = self.shell.clock.now_ms();
        let mesh = &mut self.shell.d.svc.mesh;
        match kind {
            ActionKind::ToastAction(id) => match mesh.take_act(*id) {
                Some(act) => self.shell.on_mesh_toast(act),
                None => Outcome::none(),
            },
            ActionKind::ToastClosed(id) => {
                // A waiting toast that timed out stays: build it again.
                self.mesh_toasts.shown.clear();
                if mesh.closed(*id, now) {
                    Outcome::surfaces()
                } else {
                    Outcome::none()
                }
            }
            _ => Outcome::none(),
        }
    }
}
