//! Clicks of the cloud-model section on Settings › Assistant: enter the key, forget it.

use super::{ask, close, dialog_error, fields, Ctl, Outcome};
use crate::action::Act;
use crate::service::Ports;
use crate::state::{DialogPurpose, State};
use ultar_ui::event::ActionKind;
use ultar_ui::widgets::Tone;

/// The panel the section belongs to.
pub const PANEL: &str = "assistant";

/// Reads whether a key is stored (`None`: no cloud-model service on this device).
pub fn load(state: &mut State, ports: &mut Ports) {
    state.status.cloud = ports.cloud.state().ok();
}

/// `true` for the actions of this section.
pub fn owns(act: Act) -> bool {
    matches!(act, Act::CloudKey | Act::CloudClear)
}

/// Handles a click.
pub fn handle(c: &mut Ctl<'_>, act: Act, kind: ActionKind) -> Outcome {
    match (act, kind) {
        (Act::CloudKey, ActionKind::Activated) => ask(c.state, DialogPurpose::CloudKey),
        (Act::CloudClear, ActionKind::Activated) => ask(c.state, DialogPurpose::CloudClear),
        _ => Outcome::default(),
    }
}

/// The primary button of one of the two dialogs.
pub fn dialog_ok(c: &mut Ctl<'_>, purpose: DialogPurpose) -> Outcome {
    let result = if purpose == DialogPurpose::CloudKey {
        let key = fields(c).first().map(|f| f.trim().to_owned());
        c.ports.cloud.set_key(key.as_deref().unwrap_or(""))
    } else {
        c.ports.cloud.clear_key()
    };
    if let Err(e) = result {
        return dialog_error(c.state, e.text());
    }
    close(c.state);
    load(c.state, c.ports);
    let said = if purpose == DialogPurpose::CloudKey {
        "app.settings.cloud.saved"
    } else {
        "app.settings.cloud.cleared"
    };
    Outcome {
        rebuild: true,
        dialog: true,
        toast: Some((Tone::Success, ultar_bus::Text::key(said))),
        ..Outcome::default()
    }
}
