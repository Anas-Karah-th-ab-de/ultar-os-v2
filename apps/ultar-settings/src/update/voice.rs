//! Clicks of the voice section on Settings > Assistant: the switch, the language, the model,
//! the training of the words and of the person's voice, and the test.

use super::{Ctl, Outcome};
use crate::action::Act;
use crate::service::{PortResult, Ports};
use crate::state::State;
use ultar_bus::messages::voice::EnrollKind;
use ultar_ui::event::ActionKind;
use ultar_ui::widgets::Tone;

/// The speech languages the section offers, in order.
pub const LANGUAGES: [&str; 3] = ["ar", "en", "de"];

/// What can be trained, in the order of the page.
pub const KINDS: [EnrollKind; 3] = [EnrollKind::Wake, EnrollKind::Secret, EnrollKind::Voice];

/// Reads the state (`None`: no voice service on this device).
pub fn load(state: &mut State, ports: &mut Ports) {
    state.status.voice = ports.voice.state().ok();
}

/// `true` for the actions of this section.
pub fn owns(act: Act) -> bool {
    matches!(
        act,
        Act::VoiceToggle
            | Act::VoiceLanguage
            | Act::VoiceModel
            | Act::VoiceTrain(_)
            | Act::VoiceForget(_)
            | Act::VoiceTest
    )
}

fn done(c: &mut Ctl<'_>, result: PortResult<()>) -> Outcome {
    load(c.state, c.ports);
    Outcome {
        rebuild: true,
        toast: result.err().map(|e| (Tone::Danger, e.text())),
        ..Outcome::default()
    }
}

/// The models the person can choose: the installed ones, in the service's order.
pub fn installed(state: &State) -> Vec<String> {
    let models = state.status.voice.as_ref().map(|v| v.models.as_slice());
    let models = models.unwrap_or_default();
    models
        .iter()
        .filter(|m| m.installed)
        .map(|m| m.name.clone())
        .collect()
}

/// Handles a click.
pub fn handle(c: &mut Ctl<'_>, act: Act, kind: ActionKind) -> Outcome {
    let result = match (act, kind) {
        (Act::VoiceToggle, ActionKind::Toggled(on)) => c.ports.voice.set_enabled(on),
        (Act::VoiceLanguage, ActionKind::Selected(i)) => match LANGUAGES.get(i) {
            Some(tag) => c.ports.voice.set_language(tag),
            None => return Outcome::default(),
        },
        (Act::VoiceModel, ActionKind::Selected(i)) => match installed(c.state).get(i) {
            Some(name) => c.ports.voice.set_model(name),
            None => return Outcome::default(),
        },
        (Act::VoiceTrain(i), ActionKind::Activated) => match KINDS.get(i) {
            Some(kind) => c.ports.voice.enroll(*kind),
            None => return Outcome::default(),
        },
        (Act::VoiceForget(i), ActionKind::Activated) => match KINDS.get(i) {
            Some(kind) => c.ports.voice.forget(*kind),
            None => return Outcome::default(),
        },
        (Act::VoiceTest, ActionKind::Activated) => c.ports.voice.test(),
        _ => return Outcome::default(),
    };
    done(c, result)
}
