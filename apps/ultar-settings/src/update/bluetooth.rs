//! Clicks of the Bluetooth page: the search switch and pairing a found device. The list
//! follows the service while a scan runs: it is read again whenever `bluetooth.state`
//! changes its `devices_revision`.

use super::{Ctl, Outcome};
use crate::action::Act;
use crate::service::{BtCall, Ports};
use ultar_bus::messages::bluetooth::{BluetoothState, Device};
use ultar_ui::event::ActionKind;
use ultar_ui::widgets::Tone;

/// Reads the devices when the service's list changed since the last read.
pub fn load_devices(
    devices: &mut (Vec<Device>, Option<u32>),
    state: &BluetoothState,
    ports: &mut Ports,
) {
    if !state.devices_supported {
        *devices = (Vec::new(), None);
        return;
    }
    if devices.1 == Some(state.devices_revision) {
        return;
    }
    if let Ok(list) = ports.status.bt_devices() {
        *devices = (list, Some(state.devices_revision));
    }
}

/// `true` for the actions of this page.
pub fn owns(act: Act) -> bool {
    matches!(act, Act::BtScan | Act::BtPair(_))
}

/// Handles a click.
pub fn handle(c: &mut Ctl<'_>, act: Act, kind: ActionKind) -> Outcome {
    let call = match (act, kind) {
        (Act::BtScan, ActionKind::Toggled(on)) => BtCall::Scan(on),
        (Act::BtPair(n), ActionKind::Activated) => {
            let shown = crate::view::bluetooth::order(&c.state.status.bt_devices.0);
            match shown.get(n).filter(|d| !d.paired) {
                Some(d) => BtCall::Pair(d.id.clone()),
                None => return Outcome::default(),
            }
        }
        _ => return Outcome::default(),
    };
    let result = c.ports.status.bt_call(call);
    // the state follows at once (scanning, pairing), the list with the next revision
    super::load_status(c.state, c.ports, crate::areas::StatusKind::Bluetooth);
    Outcome {
        rebuild: true,
        toast: result.err().map(|e| (Tone::Danger, e.text())),
        ..Outcome::default()
    }
}
