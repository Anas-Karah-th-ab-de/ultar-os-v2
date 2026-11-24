//! Dragging apps on the phone (CLICKS `mobile.app`: "drag: move on the home screen or into
//! the favourites row"; `apps.app` on CompactApps: "drag onto another app: folder"): what a
//! drop does to the arrangement. The app resolves the node under the drop to a [`Target`].

use super::{Outcome, Shell};
use crate::model::Open;
use crate::view::compact::home::MAX_FAVOURITES;
use crate::view::Target;

impl Shell {
    /// A drag of `dragged` ended over `onto` (`None`: over nothing that takes drops).
    pub fn on_drop(&mut self, dragged: &Target, onto: Option<&Target>) -> Outcome {
        let (Target::AppItem(app), Some(onto)) = (dragged, onto) else {
            return Outcome::none();
        };
        let changed = if matches!(self.d.open, Open::Apps(_)) {
            self.drop_in_grid(app, onto)
        } else {
            self.drop_on_home(app, onto)
        };
        if changed {
            Outcome::surfaces()
        } else {
            Outcome::none()
        }
    }

    /// All apps: onto another app makes or joins a folder, onto a folder joins it, onto the
    /// "out of the folder" place takes it out.
    fn drop_in_grid(&mut self, app: &str, onto: &Target) -> bool {
        let home = &mut self.d.home;
        match onto {
            Target::AppItem(other) => home.drop_on(app, other),
            Target::Folder(id) => home.drop_into(app, *id),
            Target::FolderOut => home.take_out(app),
            _ => false,
        }
    }

    /// The home screen: onto a favourite takes its place in the favourites row (the row's
    /// last one goes to the home screen when it is full), onto a home app moves there, onto
    /// the row's free place joins the row.
    fn drop_on_home(&mut self, app: &str, onto: &Target) -> bool {
        let d = &mut self.d;
        match onto {
            Target::AppItem(other) if other == app => false,
            Target::AppItem(other) if d.pinned.contains(other) => {
                d.pinned.retain(|p| p != app);
                d.home.apps.retain(|a| a != app);
                let at = d.pinned.iter().position(|p| p == other).unwrap_or(0);
                d.pinned.insert(at, app.to_owned());
                while d.pinned.len() > MAX_FAVOURITES {
                    if let Some(out) = d.pinned.pop() {
                        d.home.move_to(&out, None);
                    }
                }
                true
            }
            Target::AppItem(other) => {
                d.pinned.retain(|p| p != app);
                d.home.move_to(app, Some(other))
            }
            Target::Favourites if !d.pinned.iter().any(|p| p == app) => {
                if d.pinned.len() >= MAX_FAVOURITES {
                    return false;
                }
                d.home.apps.retain(|a| a != app);
                d.pinned.push(app.to_owned());
                true
            }
            _ => false,
        }
    }

    /// A folder cell of all apps: opens it; its close button: back to all apps.
    pub(super) fn folder(&mut self, open: Option<u32>) -> Outcome {
        self.d.home.open_folder = open.filter(|id| self.d.home.folder(*id).is_some());
        Outcome::surfaces()
    }
}
