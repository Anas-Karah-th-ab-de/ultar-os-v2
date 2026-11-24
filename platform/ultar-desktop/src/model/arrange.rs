//! How the phone's apps are arranged (CLICKS `mobile.app`, `apps.app` on CompactApps): the
//! order of the home screen, apps moved there or into the favourites row by dragging, and
//! folders made by dropping one app onto another in all apps. Bounded; an app is in at
//! most one folder; a folder with fewer than two apps dissolves.

/// Most folders.
pub const MAX_FOLDERS: usize = 16;
/// Most apps in one folder.
pub const MAX_IN_FOLDER: usize = 16;
/// Most apps on the home screen.
pub const MAX_HOME: usize = 64;

/// A folder of all apps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// Stable number (its name: "Folder {n}").
    pub id: u32,
    /// Its apps in order.
    pub apps: Vec<String>,
}

/// The phone's arrangement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Home {
    /// Apps on the home screen, in order (the favourites row shows `Desktop::pinned`).
    pub apps: Vec<String>,
    /// The folders of all apps.
    pub folders: Vec<Folder>,
    /// The folder shown open in all apps.
    pub open_folder: Option<u32>,
}

impl Home {
    /// The home screen with `apps`.
    pub fn with(apps: Vec<String>) -> Home {
        Home {
            apps,
            ..Home::default()
        }
    }

    /// The folder holding `app`.
    pub fn folder_of(&self, app: &str) -> Option<&Folder> {
        self.folders
            .iter()
            .find(|f| f.apps.iter().any(|a| a == app))
    }

    /// A folder by number.
    pub fn folder(&self, id: u32) -> Option<&Folder> {
        self.folders.iter().find(|f| f.id == id)
    }

    /// mobile.app drag: `app` takes the place of `before` on the home screen (or goes to
    /// the end); `false` when nothing changed.
    pub fn move_to(&mut self, app: &str, before: Option<&str>) -> bool {
        if before == Some(app) {
            return false;
        }
        let was = self.apps.len();
        self.apps.retain(|a| a != app);
        if self.apps.len() == was && was >= MAX_HOME {
            return false;
        }
        let at = before
            .and_then(|b| self.apps.iter().position(|a| a == b))
            .unwrap_or(self.apps.len());
        self.apps.insert(at, app.to_owned());
        true
    }

    /// apps.app drag onto another app: a new folder of both, or `app` joins the folder of
    /// `onto`. `false` when it cannot (same app, limits).
    pub fn drop_on(&mut self, app: &str, onto: &str) -> bool {
        if app == onto {
            return false;
        }
        if let Some(id) = self.folder_of(onto).map(|f| f.id) {
            return self.drop_into(app, id);
        }
        if self.folders.len() >= MAX_FOLDERS {
            return false;
        }
        self.take_out(app);
        self.take_out(onto);
        let id = self.folders.iter().map(|f| f.id).max().unwrap_or(0) + 1;
        self.folders.push(Folder {
            id,
            apps: vec![onto.to_owned(), app.to_owned()],
        });
        true
    }

    /// `app` into folder `id`; `false` when the folder is full or missing.
    pub fn drop_into(&mut self, app: &str, id: u32) -> bool {
        let full = self
            .folder(id)
            .is_none_or(|f| f.apps.len() >= MAX_IN_FOLDER || f.apps.iter().any(|a| a == app));
        if full {
            return false;
        }
        self.take_out(app);
        if let Some(f) = self.folders.iter_mut().find(|f| f.id == id) {
            f.apps.push(app.to_owned());
        }
        true
    }

    /// `app` out of its folder (back among all apps); a folder left with one app goes.
    pub fn take_out(&mut self, app: &str) -> bool {
        let mut changed = false;
        for f in &mut self.folders {
            let before = f.apps.len();
            f.apps.retain(|a| a != app);
            changed |= f.apps.len() != before;
        }
        self.folders.retain(|f| f.apps.len() >= 2);
        if self.open_folder.is_some_and(|id| self.folder(id).is_none()) {
            self.open_folder = None;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[String]) -> Vec<&str> {
        v.iter().map(String::as_str).collect()
    }

    #[test]
    fn moving_on_the_home_screen() {
        let mut h = Home::with(vec!["a".into(), "b".into(), "c".into()]);
        assert!(h.move_to("c", Some("a")));
        assert_eq!(ids(&h.apps), ["c", "a", "b"]);
        assert!(h.move_to("c", None));
        assert_eq!(ids(&h.apps), ["a", "b", "c"]);
        assert!(!h.move_to("a", Some("a")));
        assert!(h.move_to("new", Some("b")), "from the favourites row");
        assert_eq!(ids(&h.apps), ["a", "new", "b", "c"]);
        let mut full = Home::with((0..MAX_HOME).map(|i| i.to_string()).collect());
        assert!(!full.move_to("x", None), "bounded");
    }

    #[test]
    fn folders_are_made_joined_and_dissolved() {
        let mut h = Home::default();
        assert!(!h.drop_on("a", "a"));
        assert!(h.drop_on("b", "a"));
        assert_eq!(
            h.folders,
            vec![Folder {
                id: 1,
                apps: vec!["a".into(), "b".into()]
            }]
        );
        assert!(h.drop_on("c", "b"), "onto an app in a folder: joins it");
        assert_eq!(h.folder(1).map(|f| f.apps.len()), Some(3));
        assert!(!h.drop_into("c", 1), "already in it");
        assert!(h.drop_on("x", "y"));
        assert!(h.drop_into("x", 1), "moves from its folder");
        assert!(h.folder(2).is_none(), "y alone: dissolved");
        h.open_folder = Some(1);
        for app in ["a", "b", "c"] {
            h.take_out(app);
        }
        assert!(h.folders.is_empty() && h.open_folder.is_none());
        for i in 0..40 {
            h.drop_on(&format!("p{i}"), &format!("q{i}"));
        }
        assert_eq!(h.folders.len(), MAX_FOLDERS);
        for i in 0..40 {
            h.drop_into(&format!("r{i}"), h.folders[0].id);
        }
        assert_eq!(h.folders[0].apps.len(), MAX_IN_FOLDER);
    }
}
