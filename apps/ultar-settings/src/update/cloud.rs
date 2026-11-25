//! Clicks of the cloud-model section on Settings › Assistant: choose the provider (with
//! its address when it needs one) and the model, enter the key, forget it.

use super::{ask, close, dialog_error, fields, Ctl, Outcome};
use crate::action::Act;
use crate::service::{PortResult, Ports};
use crate::state::{DialogPurpose, State};
use ultar_bus::messages::llm_cloud::Configure;
use ultar_ui::event::ActionKind;
use ultar_ui::widgets::Tone;

/// The panel the section belongs to.
pub const PANEL: &str = "assistant";

/// Reads the state (`None`: no cloud-model service on this device).
pub fn load(state: &mut State, ports: &mut Ports) {
    state.status.cloud = ports.cloud.state().ok();
    state.status.brain = ports.cloud.brain();
}

/// `true` for the actions of this section.
pub fn owns(act: Act) -> bool {
    matches!(
        act,
        Act::CloudKey | Act::CloudClear | Act::CloudProvider | Act::CloudModel | Act::CloudAddress
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

/// Another provider: one with a fixed address is chosen at once, one of the person's own
/// choice asks for its address first.
fn provider(c: &mut Ctl<'_>, index: usize) -> Outcome {
    let cloud = c.state.status.cloud.clone().unwrap_or_default();
    let Some(chosen) = cloud.providers.get(index) else {
        return Outcome::default();
    };
    if chosen.id == cloud.provider {
        return Outcome::default();
    }
    if chosen.needs_endpoint {
        return ask(c.state, DialogPurpose::CloudAddress);
    }
    let choice = Configure {
        provider: chosen.id.clone(),
        ..Configure::default()
    };
    let result = c.ports.cloud.configure(choice);
    done(c, result)
}

fn model(c: &mut Ctl<'_>, index: usize) -> Outcome {
    let cloud = c.state.status.cloud.clone().unwrap_or_default();
    let Some(chosen) = cloud.models.get(index) else {
        return Outcome::default();
    };
    let choice = Configure {
        provider: cloud.provider,
        endpoint: cloud.endpoint,
        model: chosen.id.clone(),
    };
    let result = c.ports.cloud.configure(choice);
    done(c, result)
}

/// Handles a click.
pub fn handle(c: &mut Ctl<'_>, act: Act, kind: ActionKind) -> Outcome {
    match (act, kind) {
        (Act::CloudKey, ActionKind::Activated) => ask(c.state, DialogPurpose::CloudKey),
        (Act::CloudClear, ActionKind::Activated) => ask(c.state, DialogPurpose::CloudClear),
        (Act::CloudAddress, ActionKind::Activated) => ask(c.state, DialogPurpose::CloudAddress),
        (Act::CloudProvider, ActionKind::Selected(i)) => provider(c, i),
        (Act::CloudModel, ActionKind::Selected(i)) => model(c, i),
        _ => Outcome::default(),
    }
}

/// The address typed for the provider of the person's own choice.
fn address(c: &mut Ctl<'_>) -> PortResult<()> {
    let endpoint = fields(c).first().map(|f| f.trim().to_owned());
    let cloud = c.state.status.cloud.clone().unwrap_or_default();
    let own = cloud.providers.iter().find(|p| p.needs_endpoint);
    let choice = Configure {
        provider: own.map_or_else(String::new, |p| p.id.clone()),
        endpoint: endpoint.unwrap_or_default(),
        model: String::new(),
    };
    c.ports.cloud.configure(choice)
}

/// The primary button of one of the section's dialogs.
pub fn dialog_ok(c: &mut Ctl<'_>, purpose: DialogPurpose) -> Outcome {
    let (result, said) = match purpose {
        DialogPurpose::CloudKey => {
            let key = fields(c).first().map(|f| f.trim().to_owned());
            let stored = c.ports.cloud.set_key(key.as_deref().unwrap_or(""));
            (stored, "app.settings.cloud.saved")
        }
        DialogPurpose::CloudAddress => (address(c), "app.settings.cloud.address.saved"),
        _ => (c.ports.cloud.clear_key(), "app.settings.cloud.cleared"),
    };
    if let Err(e) = result {
        return dialog_error(c.state, e.text());
    }
    close(c.state);
    load(c.state, c.ports);
    Outcome {
        rebuild: true,
        dialog: true,
        toast: Some((Tone::Success, ultar_bus::Text::key(said))),
        ..Outcome::default()
    }
}
