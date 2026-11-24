//! Clicks of Settings › Mesh (docs/ui/mesh.md §2.1–2.3): the switch, pairing in both
//! directions, the page of one paired device with its rights, and unpairing.

use super::{ask, close, dialog_error, fields, Ctl, Outcome};
use crate::action::Act;
use crate::service::Ports;
use crate::state::mesh::RIGHTS;
use crate::state::{DialogPurpose, State, SubPage};
use ultar_bus::messages::mesh::Right;
use ultar_ui::event::ActionKind;
use ultar_ui::widgets::Tone;

/// Reads the mesh. A code this device shows is done as soon as one more device is paired;
/// the page of a device closes when the device is no longer paired.
pub fn load(state: &mut State, ports: &mut Ports) {
    let before = state.status.mesh.paired().len();
    let mesh = &mut state.status.mesh;
    match ports.mesh.state() {
        Ok(s) => {
            mesh.state = Some(s);
            mesh.error = None;
        }
        Err(e) => {
            mesh.state = None;
            mesh.error = Some(e.text());
        }
    }
    let shows = matches!(&state.dialog, Some(d) if d.purpose == DialogPurpose::MeshPairShow);
    if shows && mesh.paired().len() > before {
        mesh.code = None;
        state.dialog = None;
    }
    if state.sub == SubPage::MeshDevice && mesh.open().is_none() {
        mesh.device = None;
        state.sub = SubPage::None;
    }
}

/// `true` for the actions of this page.
pub fn owns(act: Act) -> bool {
    matches!(
        act,
        Act::MeshToggle
            | Act::MeshCopy
            | Act::MeshPair
            | Act::MeshDevice(_)
            | Act::MeshNearPair(_)
            | Act::MeshBack
            | Act::MeshRight(_)
            | Act::MeshUnpair
    )
}

fn reload(c: &mut Ctl<'_>, result: Result<(), crate::service::PortError>) -> Outcome {
    load(c.state, c.ports);
    Outcome {
        rebuild: true,
        toast: result.err().map(|e| (Tone::Danger, e.text())),
        ..Outcome::default()
    }
}

fn copy(c: &mut Ctl<'_>) -> Outcome {
    let id = c.state.status.mesh.state.as_ref().map(|s| s.id.clone());
    match id.map(|id| c.ports.clipboard.copy(&id)) {
        Some(Ok(())) => Outcome::toast(Tone::Success, "app.settings.mesh.copied"),
        Some(Err(e)) => Outcome::error(&e),
        None => Outcome::default(),
    }
}

fn pair_show(c: &mut Ctl<'_>) -> Outcome {
    match c.ports.mesh.pair_begin() {
        Ok(code) => {
            c.state.status.mesh.code = Some(code);
            ask(c.state, DialogPurpose::MeshPairShow)
        }
        Err(e) => Outcome::error(&e),
    }
}

fn right(c: &mut Ctl<'_>, n: usize, on: bool) -> Outcome {
    let (Some(peer), Some(right)) = (c.state.status.mesh.open(), RIGHTS.get(n)) else {
        return Outcome::default();
    };
    let mut rights: Vec<Right> = peer.rights.iter().copied().filter(|r| r != right).collect();
    if on {
        rights.push(*right);
        rights.sort();
    }
    let id = peer.id.clone();
    let result = c.ports.mesh.set_rights(&id, rights);
    reload(c, result)
}

fn open_device(c: &mut Ctl<'_>, n: usize) -> Outcome {
    let id = c.state.status.mesh.paired().get(n).map(|p| p.id.clone());
    if id.is_none() {
        return Outcome::default();
    }
    c.state.status.mesh.device = id;
    c.state.sub = SubPage::MeshDevice;
    Outcome::rebuild()
}

/// Handles a click.
pub fn handle(c: &mut Ctl<'_>, act: Act, kind: ActionKind) -> Outcome {
    match (act, kind) {
        (Act::MeshToggle, ActionKind::Toggled(on)) => {
            let result = c.ports.mesh.set_enabled(on);
            reload(c, result)
        }
        (Act::MeshRight(n), ActionKind::Toggled(on)) => right(c, n, on),
        (Act::MeshCopy, ActionKind::Activated) => copy(c),
        (Act::MeshPair, ActionKind::Activated) => pair_show(c),
        (Act::MeshDevice(n), ActionKind::Activated) => open_device(c, n),
        (Act::MeshNearPair(n), ActionKind::Activated) => {
            let id = c.state.status.mesh.near().get(n).map(|p| p.id.clone());
            if id.is_none() {
                return Outcome::default();
            }
            c.state.status.mesh.join = id;
            ask(c.state, DialogPurpose::MeshPairJoin)
        }
        (Act::MeshBack, ActionKind::Activated) => {
            c.state.status.mesh.device = None;
            c.state.sub = SubPage::None;
            Outcome::rebuild()
        }
        (Act::MeshUnpair, ActionKind::Activated) => ask(c.state, DialogPurpose::MeshUnpair),
        _ => Outcome::default(),
    }
}

fn join(c: &mut Ctl<'_>) -> Outcome {
    let code: String = fields(c)
        .first()
        .map(|f| f.trim().to_owned())
        .unwrap_or_default();
    let digits = code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit());
    let Some(device) = c.state.status.mesh.join.clone().filter(|_| digits) else {
        let wrong = ultar_bus::Text::key("app.settings.mesh.error.digits");
        return dialog_error(c.state, wrong);
    };
    match c.ports.mesh.pair_join(&device, &code) {
        Ok(()) => {
            c.state.status.mesh.join = None;
            close(c.state);
            let out = reload(c, Ok(()));
            Outcome {
                toast: Some((
                    Tone::Success,
                    ultar_bus::Text::key("app.settings.mesh.paired_now"),
                )),
                dialog: true,
                ..out
            }
        }
        Err(e) => dialog_error(c.state, e.text()),
    }
}

/// The primary button of one of this page's dialogs.
pub fn dialog_ok(c: &mut Ctl<'_>, purpose: DialogPurpose) -> Outcome {
    match purpose {
        DialogPurpose::MeshPairJoin => join(c),
        DialogPurpose::MeshUnpair => {
            let Some(id) = c.state.status.mesh.device.clone() else {
                return close(c.state);
            };
            let result = c.ports.mesh.unpair(&id);
            close(c.state);
            let out = reload(c, result);
            Outcome {
                dialog: true,
                ..out
            }
        }
        // the code is shown until the other device typed it; the one button ends it
        _ => {
            c.state.status.mesh.code = None;
            close(c.state)
        }
    }
}
