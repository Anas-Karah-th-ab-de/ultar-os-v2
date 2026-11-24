//! The banners' surface: a popup of the bar that never takes the keyboard, rebuilt when the
//! banners change; closing one removes the notification (CLICKS `note.dismiss`).

use super::{screen, DesktopApp};
use crate::app::AppError;
use crate::shell::Outcome;
use crate::view::{banners, Target};
use ultar_surface::client::WindowConfig;
use ultar_surface::proto::Role;
use ultar_ui::event::ActionKind;

impl DesktopApp {
    /// Shows, updates or removes the banners.
    pub(super) fn sync_banners(&mut self) -> Result<(), AppError> {
        let now = (
            self.shell.clock.now_ms(),
            i64::from(self.shell.clock.offset_s()),
        );
        let built = banners::build(
            &self.shell.d,
            now,
            self.look,
            screen(&self.host),
            &self.shell.tr,
        )?;
        match (built, self.banners) {
            (None, Some((id, _))) => {
                self.banners = None;
                self.host.close(id)?;
            }
            (None, None) => {}
            (Some((view, place)), Some((id, offset))) if offset == place.offset => {
                self.host.replace(id, view, Some(place.surface));
            }
            (Some((view, place)), old) => {
                if let Some((id, _)) = old {
                    self.host.close(id)?;
                }
                let role = Role::Popup {
                    parent: self.bar,
                    x: place.offset.0,
                    y: place.offset.1,
                };
                let id = self.host.open(
                    &WindowConfig::with_role(role, "", true),
                    view,
                    Some(place.surface),
                )?;
                self.banners = Some((id, place.offset));
            }
        }
        Ok(())
    }

    /// A click on a banner.
    pub(super) fn on_banner(&mut self, target: &Target, kind: &ActionKind) -> Outcome {
        match (target, kind) {
            (Target::Dismiss(id), ActionKind::Activated) => {
                self.shell.d.notes.dismiss(*id);
                Outcome::surfaces()
            }
            _ => Outcome::none(),
        }
    }
}
