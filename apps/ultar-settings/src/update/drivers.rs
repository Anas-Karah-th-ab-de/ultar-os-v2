//! Clicks of the Drivers and hardware page (`drivers.*`).

use super::{ask, close, dialog_error, fields, Ctl, Outcome};
use crate::action::Act;
use crate::service::Ports;
use crate::state::{DialogPurpose, State};
use ultar_bus::messages::device_service::{DeviceInfo, DriverInfo};
use ultar_bus::{Text, TextArg};
use ultar_ui::event::ActionKind;
use ultar_ui::widgets::Tone;

/// Longest path accepted for an update bundle.
pub const MAX_PATH: usize = 4096;

/// Reads the device list and the updater's progress.
pub fn load(state: &mut State, ports: &mut Ports) {
    match ports.devices.list() {
        Ok(reply) => {
            state.drivers.devices = reply.devices;
            state.drivers.notices = reply.notices;
            state.drivers.known = true;
            state.drivers.error = None;
        }
        Err(e) => {
            state.drivers.known = false;
            state.drivers.error = Some(e.text());
        }
    }
    state.drivers.firmware = ports.update.progress();
}

/// `true` for the actions of this page.
pub fn owns(act: Act) -> bool {
    matches!(
        act,
        Act::DriverAlt(_)
            | Act::DriverRollback(_)
            | Act::DriverPlan(_)
            | Act::FirmwareCheck
            | Act::FirmwareUsb
            | Act::SelfTest
    )
}

/// The alternatives offered in the select after "recommended" (the active one left out).
pub fn offered(d: &DeviceInfo) -> Vec<&DriverInfo> {
    let active = d.active.as_ref().map(|a| a.id.as_str());
    d.alternatives
        .iter()
        .filter(|a| Some(a.id.as_str()) != active)
        .collect()
}

/// Handles a click.
pub fn handle(c: &mut Ctl<'_>, act: Act, kind: ActionKind) -> Outcome {
    match (act, kind) {
        (Act::DriverAlt(row), ActionKind::Selected(i)) if i > 0 => {
            ask(c.state, DialogPurpose::DriverSwitch { row, alt: i - 1 })
        }
        (Act::DriverRollback(row), ActionKind::Activated) => rollback(c, row),
        (Act::DriverPlan(row), ActionKind::Activated) => plan(c, row),
        (Act::FirmwareCheck, ActionKind::Activated) => match c.ports.update.check() {
            Ok(()) => {
                c.state.drivers.firmware = Some(("update.state.checking".to_owned(), None));
                Outcome::rebuild()
            }
            Err(e) => Outcome::error(&e),
        },
        (Act::FirmwareUsb, ActionKind::Activated) => ask(c.state, DialogPurpose::FirmwareUsb),
        (Act::SelfTest, ActionKind::Activated) => self_test(c),
        _ => Outcome::default(),
    }
}

fn replace(state: &mut State, info: DeviceInfo) {
    if let Some(d) = state
        .drivers
        .devices
        .iter_mut()
        .find(|d| d.address == info.address)
    {
        *d = info;
    }
}

/// Dialog "switch driver" confirmed.
pub fn switch(c: &mut Ctl<'_>, row: usize, alt: usize) -> Outcome {
    let Some(d) = c.state.drivers.devices.get(row) else {
        return close(c.state);
    };
    let Some(target) = offered(d)
        .get(alt)
        .map(|a| (a.id.clone(), a.version.clone()))
    else {
        return close(c.state);
    };
    let address = d.address.clone();
    match c.ports.devices.switch(&address, &target.0, &target.1) {
        Ok(info) => {
            replace(c.state, info);
            c.state.dialog = None;
            Outcome {
                dialog: true,
                rebuild: true,
                toast: Some((Tone::Success, Text::key("app.settings.drivers.switched"))),
                ..Outcome::default()
            }
        }
        Err(e) => dialog_error(c.state, e.text()),
    }
}

fn rollback(c: &mut Ctl<'_>, row: usize) -> Outcome {
    let Some(address) = c.state.drivers.devices.get(row).map(|d| d.address.clone()) else {
        return Outcome::default();
    };
    match c.ports.devices.rollback(&address) {
        Ok(info) => {
            replace(c.state, info);
            Outcome {
                toast: Some((Tone::Success, Text::key("app.settings.drivers.rolled_back"))),
                ..Outcome::rebuild()
            }
        }
        Err(e) => Outcome::error(&e),
    }
}

/// The hardware plan of a device as TOML (ids only; no personal data).
pub fn plan_text(d: &DeviceInfo) -> String {
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut out = format!(
        "# {} hardware plan (DESKTOP-SPEC §8)\nformat = 1\n",
        ultar_brand::PRODUCT
    );
    out.push_str(&format!("address = \"{}\"\n", quote(&d.address)));
    out.push_str(&format!("hardware_id = \"{}\"\n", quote(&d.hardware_id)));
    if let Some(label) = &d.label {
        out.push_str(&format!("label = \"{}\"\n", quote(label)));
    }
    let caps: Vec<String> = d
        .capabilities
        .iter()
        .map(|c| format!("\"{}\"", quote(c)))
        .collect();
    out.push_str(&format!("capabilities = [{}]\n", caps.join(", ")));
    out
}

/// File name of a device's plan (`hardware-plan-pci-00-14-3.toml`).
pub fn plan_name(d: &DeviceInfo) -> String {
    let safe: String = d
        .address
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(64)
        .collect();
    format!("hardware-plan-{safe}.toml")
}

fn plan(c: &mut Ctl<'_>, row: usize) -> Outcome {
    let Some(d) = c.state.drivers.devices.get(row) else {
        return Outcome::default();
    };
    let (name, text) = (plan_name(d), plan_text(d));
    match c.ports.files.save(&name, text.as_bytes()) {
        Ok(path) => Outcome {
            toast: Some((
                Tone::Success,
                Text::key("app.settings.drivers.plan.saved").with(TextArg::Verbatim(path)),
            )),
            ..Outcome::default()
        },
        Err(e) => Outcome::error(&e),
    }
}

/// `true` for an absolute path without `..` parts, of sane length.
pub fn valid_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= MAX_PATH
        && !path.split('/').any(|part| part == "..")
        && !path.chars().any(char::is_control)
}

/// Dialog "update from a drive" confirmed.
pub fn usb(c: &mut Ctl<'_>) -> Outcome {
    let path = fields(c).into_iter().next().unwrap_or_default();
    let path = path.trim().to_owned();
    if !valid_path(&path) {
        return dialog_error(c.state, Text::key("app.settings.dialog.usb.invalid"));
    }
    match c.ports.update.offline(&path) {
        Ok(()) => {
            c.state.drivers.firmware = Some(("update.state.downloading".to_owned(), None));
            c.state.dialog = None;
            Outcome {
                dialog: true,
                rebuild: true,
                ..Outcome::default()
            }
        }
        Err(e) => dialog_error(c.state, e.text()),
    }
}

fn self_test(c: &mut Ctl<'_>) -> Outcome {
    c.state.drivers.testing = true;
    let result = c.ports.devices.rescan();
    c.state.drivers.testing = false;
    match result {
        Ok(devices) => {
            let n = devices.len() as i64;
            c.state.drivers.devices = devices;
            let problems = c.state.drivers.without_driver().len() as i64;
            let text = Text::key("app.settings.drivers.selftest.done")
                .with(TextArg::Number(n))
                .with(TextArg::Number(problems));
            let tone = if problems == 0 {
                Tone::Success
            } else {
                Tone::Warning
            };
            Outcome {
                toast: Some((tone, text)),
                ..Outcome::rebuild()
            }
        }
        Err(e) => Outcome::error(&e),
    }
}
