//! qs.bluetooth.list: the Bluetooth devices page of the quick settings (bluetooth v2:
//! `devices`, `scan`, `pair`, `connect`, `disconnect`, `forget`). Paired devices first, then the
//! ones found while searching; a click pairs, connects or disconnects, the bin forgets a paired
//! one. The device names are data from the air: plain text, never interpreted.
//!
//! While a scan runs the list follows the service (`bluetooth.state` changes its
//! `devices_revision` with every device found) under a busy bar; nearer devices (stronger
//! signal) and named ones come first, a nameless one shows its address. A scan that ended
//! without a device says so.

use super::{Target, View};
use crate::model::Desktop;
use ultar_bus::messages::bluetooth::{Device, DeviceKind, PairPhase};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{IconButton, Label, List, ListItem, Progress, Switch, Text};

/// Most devices listed.
pub const MAX_DEVICES: usize = 12;
/// Longest device name shown (characters).
pub const MAX_NAME: usize = 48;

fn icon(kind: DeviceKind) -> IconId {
    match kind {
        DeviceKind::Keyboard | DeviceKind::Combo => IconId::Keyboard,
        DeviceKind::Audio => IconId::Speaker,
        DeviceKind::Phone => IconId::Phone,
        _ => IconId::Bluetooth,
    }
}

/// The state line of a device: pairing, connected (with its battery), paired, or found.
fn state(d: &Desktop, dev: &Device) -> Label {
    let pairing = d
        .svc
        .bluetooth
        .as_ref()
        .and_then(|b| b.pairing.as_ref())
        .filter(|p| p.id == dev.id);
    match (pairing.map(|p| p.phase), dev.connected, dev.paired) {
        (Some(PairPhase::Running), ..) => Label::key("desk.bt.pairing"),
        (Some(PairPhase::Failed), ..) => Label::key("desk.bt.failed"),
        (_, true, _) => match dev.battery_percent {
            Some(b) => Label::key_with("desk.bt.connected_battery", &[("percent", &b.to_string())]),
            None => Label::key("desk.bt.connected"),
        },
        (_, false, true) => Label::key("desk.bt.paired"),
        _ => Label::key("desk.bt.found"),
    }
}

/// The devices in the order shown: paired and connected first, then named before nameless,
/// then the stronger signal; the address keeps the order stable.
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
    v
}

/// The name shown: the device's name (at most [`MAX_NAME`] characters) or, without one, its
/// address.
pub fn shown_name(dev: &Device) -> String {
    let name = dev.name.trim();
    if name.is_empty() {
        dev.id.clone()
    } else {
        name.chars().take(MAX_NAME).collect()
    }
}

/// The page: back, title and the search switch, then the devices.
pub fn page(view: &mut View, card: NodeId, d: &Desktop) -> Result<(), UiError> {
    let head = view.ui.add(
        card,
        Flex::row()
            .gap(Space::S8)
            .cross(CrossAlign::Center)
            .main(MainAlign::SpaceBetween),
    )?;
    let start = view
        .ui
        .add(head, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    let back = view.ui.add(
        start,
        IconButton::new(IconId::Back, Label::key("desk.qs.back")),
    )?;
    view.mark(back, Target::QuickBack);
    view.ui
        .add(start, Text::new(Label::key("desk.qs.bluetooth")))?;
    let scanning = d.svc.bluetooth.as_ref().is_some_and(|b| b.scanning);
    let scan = view
        .ui
        .add(head, Switch::new(Label::key("desk.bt.search"), scanning))?;
    view.mark(scan, Target::BtScan);
    let missing = match &d.svc.bluetooth {
        None => Some("desk.reason.no_service"),
        Some(b) if !b.devices_supported => Some("desk.reason.no_bt_device"),
        Some(_) => None,
    };
    if let Some(reason) = missing {
        // Nothing to search with: the page says so.
        view.ui.set_disabled(scan, true)?;
        view.ui.set_tooltip(scan, Some(Label::key(reason)))?;
        view.ui.add(card, Text::new(Label::key(reason)))?;
        return Ok(());
    }
    if scanning {
        view.ui
            .add(card, Progress::new(Label::key("desk.bt.searching")))?;
    }
    let devices = order(&d.svc.bt_devices);
    if devices.is_empty() {
        let key = match (scanning, d.svc.bt_searched) {
            (true, _) => "desk.bt.searching",
            (false, true) => "desk.bt.not_found",
            (false, false) => "desk.bt.none",
        };
        view.ui.add(card, Text::new(Label::key(key)))?;
    }
    for dev in devices.into_iter().take(MAX_DEVICES) {
        // One row per device: the device (a list of one, for its row look) and, when
        // paired, the bin that forgets it.
        let row = view
            .ui
            .add(card, Flex::row().gap(Space::S4).cross(CrossAlign::Center))?;
        let name = shown_name(dev);
        let list = List::new().name(Label::key("desk.bt.devices"));
        let list = view.ui.add_sized(row, list, Sizing::grow(1))?;
        let item = ListItem::new(Label::data(&name))
            .sublabel(state(d, dev))
            .icon(icon(dev.kind));
        let id = view.ui.add(list, item)?;
        view.mark(id, Target::BtDevice(dev.id.clone()));
        if dev.paired {
            let what = Label::key_with("desk.bt.forget", &[("name", &name)]);
            let forget = view.ui.add(row, IconButton::new(IconId::Bin, what))?;
            view.mark(forget, Target::BtForget(dev.id.clone()));
        }
    }
    Ok(())
}
