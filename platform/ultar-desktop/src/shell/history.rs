//! Keeping the search history (`search.history`): writing an opened result down and running
//! a remembered entry again. Split out of `sheets.rs` so each stays a small module.

use super::bar::{launch, SETTINGS};
use super::sheets::open_app;
use super::{Effect, Opening, Outcome, Shell};
use crate::model::history::Record;
use crate::model::search::panel_area;

/// Writes one opened result into the search history (when `search.history` is on) and asks
/// for it to be kept beside the shell.
pub(super) fn remember(s: &mut Shell, record: Record) -> Option<Effect> {
    if s.d.overview.sources.history {
        s.d.overview.history.record(record);
        Some(Effect::History)
    } else {
        None
    }
}

/// A remembered entry runs again: a search returns to the field, an app or a panel opens
/// (and is remembered again, newest first).
pub(super) fn open_record(s: &mut Shell, record: &Record) -> Outcome {
    match record {
        Record::Search(t) => {
            s.d.overview.query = t.clone();
            Outcome::surfaces()
        }
        Record::App(id) => {
            let mut out = open_app(s, id);
            out.effects.extend(remember(s, Record::App(id.clone())));
            out
        }
        Record::Setting(panel) => {
            let area = panel_area(panel);
            let mut out = launch(s, SETTINGS, Opening::settings_page(area));
            out.effects
                .extend(remember(s, Record::Setting(panel.clone())));
            out
        }
    }
}
