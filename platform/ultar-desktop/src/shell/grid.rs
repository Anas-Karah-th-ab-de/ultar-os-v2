//! The app grid's own clicks (CLICKS `apps.*`): a category tab, an app's context menu.

use super::bar::{launch, SETTINGS, STORE};
use super::Opening;
use super::{Outcome, Shell};
use crate::model::Open;
use crate::view::apps::{categories, foreign_source, item};

/// `apps.category`: tab 0 is "All", then the categories that have apps.
pub(super) fn category(s: &mut Shell, index: usize) -> Outcome {
    let chosen = index
        .checked_sub(1)
        .and_then(|i| categories(&s.d).get(i).copied());
    s.d.open = Open::Apps(chosen);
    Outcome::surfaces()
}

/// An item of an app's context menu (`menu.item`, `apps.pin`).
pub(super) fn app_menu(s: &mut Shell, app: &str, id: u32) -> Outcome {
    match id {
        item::OPEN => super::sheets::open_app(s, app),
        item::NEW_WINDOW => launch(s, app, Opening::new_window()),
        // apps from other systems are kept in Settings, not in the Store
        item::DETAILS | item::UNINSTALL if foreign_source(app).is_some() => {
            launch(s, SETTINGS, Opening::settings_page("foreign"))
        }
        item::DETAILS => launch(s, STORE, Opening::store_page(app, false)),
        item::UNINSTALL => launch(s, STORE, Opening::store_page(app, true)),
        item::DESKTOP => super::desk::toggle_shortcut(s, app),
        item::PIN => {
            if s.d.places.pinned.iter().any(|p| p == app) {
                s.d.places.pinned.retain(|p| p != app);
            } else if s.d.catalog.iter().any(|a| a.id == app) {
                s.d.places.pinned.push(app.to_owned());
            }
            Outcome::surfaces()
        }
        _ => Outcome::none(),
    }
}
