//! The app grid's own clicks (CLICKS `apps.*`): a category tab, an app's context menu.

use super::bar::{launch, STORE};
use super::Opening;
use super::{Outcome, Shell};
use crate::model::Open;
use crate::view::apps::{categories, item};

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
        item::DETAILS => launch(s, STORE, Opening::store_page(app, false)),
        item::UNINSTALL => launch(s, STORE, Opening::store_page(app, true)),
        item::PIN => {
            if s.d.pinned.iter().any(|p| p == app) {
                s.d.pinned.retain(|p| p != app);
            } else if s.d.catalog.iter().any(|a| a.id == app) {
                s.d.pinned.push(app.to_owned());
            }
            Outcome::surfaces()
        }
        _ => Outcome::none(),
    }
}
