//! The helper's sheet (Super+H, the quick settings' button, a request from the search):
//! ask, see each step, stop waiting, open what the helper may do.

use super::bar::{launch, SETTINGS};
use super::{Effect, Opening, Outcome, Shell};
use crate::model::helper::MAX_ASK;
use crate::model::services::Call;
use crate::model::Open;

/// The Settings page of the helper.
pub const SETTINGS_PAGE: &str = "assistant";

/// Opens or closes the sheet; nothing without the helper service.
pub(super) fn toggle(s: &mut Shell) -> Outcome {
    if !s.d.overview.sources.helper {
        return Outcome::none();
    }
    s.toggle(Open::Helper)
}

/// Text typed into the field.
pub(super) fn typed(s: &mut Shell, text: &str) -> Outcome {
    s.d.overview.helper.query = text.chars().take(MAX_ASK).collect();
    Outcome {
        effects: Vec::new(),
        bar: false,
        popup: true,
    }
}

fn ask(s: &mut Shell, text: &str) -> Outcome {
    let Some(asked) = s.d.overview.helper.ask(text) else {
        return Outcome::none();
    };
    let mut out = Outcome::surfaces();
    out.effects.push(Effect::Service(Call::AskHelper(asked)));
    out
}

/// Enter or "Send": the typed request goes to the helper.
pub(super) fn send(s: &mut Shell) -> Outcome {
    let text = s.d.overview.helper.query.clone();
    ask(s, &text)
}

/// "Ask the helper" in the search: the sheet opens and shows the steps; while the helper
/// is still busy the answer comes as a notification.
pub(super) fn ask_from_search(s: &mut Shell, text: &str) -> Outcome {
    s.d.overview.query.clear();
    if s.d.overview.helper.busy() {
        s.d.open = Open::None;
        return Outcome::effect_and_close(Effect::Service(Call::AskHelper(text.to_owned())));
    }
    s.d.open = Open::Helper;
    ask(s, text)
}

/// "Stop": the person no longer waits; what was done stays done.
pub(super) fn stop(s: &mut Shell) -> Outcome {
    if s.d.overview.helper.stop() {
        Outcome::surfaces()
    } else {
        Outcome::none()
    }
}

/// The gear: what the helper reaches and did, in Settings.
pub(super) fn settings(s: &mut Shell) -> Outcome {
    launch(s, SETTINGS, Opening::settings_page(SETTINGS_PAGE))
}
