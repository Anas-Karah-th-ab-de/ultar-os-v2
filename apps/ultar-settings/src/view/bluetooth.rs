//! Settings › Bluetooth, below the service's state: the search switch, a busy bar while
//! the service searches, the devices it found (named and nearer ones first, a nameless one
//! by its address) – a found one pairs on click (the system dialog asks) – and, after a
//! search without result, "no devices found". Device names are data from the air.

use super::{k, Cx};
use crate::action::Act;
use ultar_bus::messages::bluetooth::{Device, PairPhase};
use ultar_ui::layout::Sizing;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Card, Label, List, ListItem, Progress, Switch, Text};

/// Most devices shown.
pub const MAX_SHOWN: usize = 24;
/// Longest name shown (characters).
const MAX_NAME: usize = 48;

/// The devices in the order shown: paired and connected first, then named before nameless,
/// then the stronger signal; the id keeps the order stable.
pub fn order(devices: &[Device]) -> Vec<&Device> {
    let mut v: Vec<&Device> = devices.iter().collect();
    v.sort_by_key(|d| {
        (
            !d.paired,
            !d.connected,
            d.name.trim().is_empty(),
            std::cmp::Reverse(d.rssi.unwrap_or(i8::MIN)),
            d.id.clone(),
        )
    });
    v.truncate(MAX_SHOWN);
    v
}

fn name(d: &Device) -> String {
    let n = d.name.trim();
    if n.is_empty() {
        d.id.clone()
    } else {
        n.chars().take(MAX_NAME).collect()
    }
}

/// The devices part of the Bluetooth page.
pub(crate) fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let st = &cx.state.status;
    let Some(bt) = st.bluetooth.clone().filter(|b| b.devices_supported) else {
        return Ok(());
    };
    let card = cx.ui.add_sized(content, Card::new(), Sizing::AUTO)?;
    cx.act(
        card,
        Switch::new(k("app.settings.bt.search"), bt.scanning),
        Act::BtScan,
    )?;
    if bt.scanning {
        cx.ui
            .add(card, Progress::new(k("app.settings.bt.searching")))?;
    }
    let devices: Vec<Device> = order(&cx.state.status.bt_devices.0)
        .into_iter()
        .cloned()
        .collect();
    if devices.is_empty() {
        let key = if bt.scanning {
            "app.settings.bt.searching"
        } else if cx.state.status.bt_searched {
            "app.settings.bt.not_found"
        } else {
            "app.settings.bt.hint"
        };
        cx.ui.add(card, Text::new(k(key)))?;
        return Ok(());
    }
    let list = cx
        .ui
        .add(card, List::new().name(k("app.settings.bt.devices")))?;
    for (i, d) in devices.iter().enumerate() {
        let pairing = bt
            .pairing
            .as_ref()
            .filter(|p| p.id == d.id)
            .map(|p| p.phase);
        let state = match (pairing, d.connected, d.paired) {
            (Some(PairPhase::Running), ..) => "app.settings.bt.pairing",
            (Some(PairPhase::Failed), ..) => "app.settings.bt.failed",
            (_, true, _) => "app.settings.status.connected",
            (_, false, true) => "app.settings.bt.paired",
            _ => "app.settings.bt.found",
        };
        let item = ListItem::new(Label::data(&name(d))).sublabel(k(state));
        cx.act(list, item, Act::BtPair(i))?;
    }
    Ok(())
}
