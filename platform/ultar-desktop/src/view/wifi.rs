//! qs.wifi.list and qs.vpn.list: the Wi-Fi networks page, the join dialog and the VPN
//! connections page of the quick settings (desktop sheet and phone sheet; contracts of
//! docs/integration/system-services.md §3). Network and connection names are data from the
//! air or from a file: plain text, never interpreted. The password lives only in the
//! toolkit's password field (wiped buffer, shown while its button is held) until the app
//! moves it into the `net.join` call.

use super::{Target, View};
use crate::model::wifi::{self, Joining};
use crate::model::Desktop;
use ultar_bus::messages::net::WifiSecurity;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Checkbox, IconButton, Label, List, ListItem, Switch, Text, TextField,
};

/// Longest Wi-Fi password: 63 characters (or 64 hex digits).
pub const MAX_PASSWORD: usize = 64;

/// The page's head: back, the title and the radio's switch.
fn head(
    view: &mut View,
    card: NodeId,
    title: &str,
    switch: Option<(Target, Label, bool)>,
) -> Result<(), UiError> {
    let row = view.ui.add(
        card,
        Flex::row()
            .gap(Space::S8)
            .cross(CrossAlign::Center)
            .main(MainAlign::SpaceBetween),
    )?;
    let start = view
        .ui
        .add(row, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    let back = IconButton::new(IconId::Back, Label::key("desk.qs.back"));
    let back = view.ui.add(start, back)?;
    view.mark(back, Target::QuickBack);
    view.ui.add(start, Text::new(Label::key(title)))?;
    if let Some((target, name, on)) = switch {
        let id = view.ui.add(row, Switch::new(name, on))?;
        view.mark(id, target);
    }
    Ok(())
}

/// One row of a list: the item (a list of one, for its row look) and an optional button.
fn row(
    view: &mut View,
    card: NodeId,
    item: ListItem,
    target: Target,
    button: Option<(IconId, Label, Target)>,
) -> Result<NodeId, UiError> {
    let line = view
        .ui
        .add(card, Flex::row().gap(Space::S4).cross(CrossAlign::Center))?;
    let list = List::new().name(Label::key("desk.wifi.networks"));
    let list = view.ui.add_sized(line, list, Sizing::grow(1))?;
    let id = view.ui.add(list, item)?;
    view.mark(id, target);
    if let Some((icon, name, target)) = button {
        let b = view.ui.add(line, IconButton::new(icon, name))?;
        view.mark(b, target);
    }
    Ok(id)
}

/// qs.wifi.list: the radio, the networks (connected, saved, strongest first; a click joins,
/// asks for the password or leaves; the bin forgets a saved one) and "Wi-Fi settings".
pub fn page(view: &mut View, card: NodeId, d: &Desktop) -> Result<(), UiError> {
    let on = d.svc.net.as_ref().is_some_and(|n| n.wifi_on);
    let radio = Some((Target::WifiRadio, Label::key("desk.qs.wifi"), on));
    head(view, card, "desk.qs.wifi", radio)?;
    // Without the network service or a Wi-Fi device the page says so (the radio cannot
    // be switched); "Wi-Fi settings" below still leads on.
    let missing = match &d.svc.net {
        None => Some("desk.reason.no_service"),
        Some(n) if !n.wifi_present => Some("desk.reason.no_wifi_device"),
        Some(_) => None,
    };
    if let (Some(reason), Some(switch)) = (missing, view.node(&Target::WifiRadio)) {
        view.ui.set_disabled(switch, true)?;
        view.ui.set_tooltip(switch, Some(Label::key(reason)))?;
    }
    let networks = d.svc.wifi.networks.as_deref().unwrap_or_default();
    let listed = wifi::listed(networks);
    let empty = match (&d.svc.wifi.networks, missing) {
        (_, Some(reason)) => Some(reason),
        (None, None) => Some("desk.wifi.searching"),
        (Some(_), None) if listed.is_empty() => Some("desk.wifi.none"),
        (Some(_), None) => None,
    };
    if let Some(key) = empty {
        view.ui.add(card, Text::new(Label::key(key)))?;
    }
    for n in listed {
        let name: String = n.ssid.chars().take(wifi::MAX_SSID).collect();
        let signal = n.signal_percent.min(100).to_string();
        let line = Label::key_with(wifi::security_key(n), &[("signal", &signal)]);
        let icon = if n.security == WifiSecurity::Open {
            IconId::Wifi
        } else {
            IconId::Lock
        };
        let item = ListItem::new(Label::data(&name)).sublabel(line).icon(icon);
        let item = if n.connected { item.selected() } else { item };
        let forget = (n.known || d.svc.wifi.known.contains(&n.ssid)).then(|| {
            let what = Label::key_with("desk.wifi.forget", &[("name", &name)]);
            (IconId::Bin, what, Target::WifiForget(n.ssid.clone()))
        });
        let id = row(
            view,
            card,
            item,
            Target::WifiNetwork(n.ssid.clone()),
            forget,
        )?;
        if !n.joinable && !n.connected {
            view.ui.set_disabled(id, true)?;
            view.ui
                .set_tooltip(id, Some(Label::key("desk.wifi.unsupported_reason")))?;
        }
    }
    let settings = Button::new(Label::key("desk.wifi.settings")).variant(ButtonVariant::Quiet);
    let settings = view.ui.add(card, settings)?;
    view.mark(settings, Target::WifiSettings);
    Ok(())
}

/// The join dialog: the network, the password (shown while its button is held), "save
/// this network", connect and cancel; the outcome of the last try under the field.
pub fn join(view: &mut View, card: NodeId, d: &Desktop) -> Result<(), UiError> {
    let Some(j) = &d.svc.wifi.joining else {
        return page(view, card, d);
    };
    head(view, card, "desk.wifi.join.title", None)?;
    let name: String = j.ssid.chars().take(wifi::MAX_SSID).collect();
    view.ui.add(card, Text::new(Label::data(&name)))?;
    let field = TextField::password(Label::key_with(
        "desk.wifi.join.password",
        &[("name", &name)],
    ))
    .labelled()
    .max_len(MAX_PASSWORD);
    let field = view.ui.add(card, field)?;
    view.mark(field, Target::JoinPassword);
    view.ui.set_disabled(field, j.busy)?;
    view.ui
        .set_error(field, j.failed.map(|o| Label::key(wifi::join_error_key(o))))?;
    if !j.busy {
        view.ui.focus(field)?;
    }
    let remember = Checkbox::new(Label::key("desk.wifi.join.remember"), j.remember);
    let remember = view.ui.add(card, remember)?;
    view.mark(remember, Target::JoinRemember);
    buttons(view, card, j)
}

fn buttons(view: &mut View, card: NodeId, j: &Joining) -> Result<(), UiError> {
    let row = view
        .ui
        .add(card, Flex::row().gap(Space::S8).main(MainAlign::End))?;
    let cancel = view
        .ui
        .add(row, Button::new(Label::key("desk.wifi.join.cancel")))?;
    view.mark(cancel, Target::JoinCancel);
    let connect = Button::new(Label::key("desk.wifi.join.connect")).variant(ButtonVariant::Primary);
    let connect = view.ui.add(row, connect)?;
    view.mark(connect, Target::JoinConnect);
    view.ui.set_disabled(connect, j.busy)?;
    view.ui.set_loading(connect, j.busy)
}

/// qs.vpn.list: the switch, the connections (the one in use marked; a click uses it) and
/// "VPN settings" (Settings > Network > VPN, where `.conf` files are imported).
pub fn vpn_page(view: &mut View, card: NodeId, d: &Desktop) -> Result<(), UiError> {
    let on = d.svc.vpn.as_ref().is_some_and(wifi::vpn_on);
    let switch = Some((Target::VpnSwitch, Label::key("desk.qs.vpn"), on));
    head(view, card, "desk.qs.vpn", switch)?;
    if d.svc.vpn.is_none() {
        // No VPN service: the page says so; "VPN settings" below still leads on.
        if let Some(id) = view.node(&Target::VpnSwitch) {
            view.ui.set_disabled(id, true)?;
            view.ui
                .set_tooltip(id, Some(Label::key("desk.reason.no_service")))?;
        }
        view.ui
            .add(card, Text::new(Label::key("desk.reason.no_service")))?;
    }
    let profiles = d
        .svc
        .vpn
        .as_ref()
        .map(|v| v.profiles.as_slice())
        .unwrap_or_default();
    if profiles.is_empty() && d.svc.vpn.is_some() {
        view.ui.add(card, Text::new(Label::key("desk.vpn.empty")))?;
    }
    let active = d.svc.vpn.as_ref().and_then(|v| v.active.as_deref());
    for p in profiles.iter().take(ultar_bus::messages::vpn::MAX_PROFILES) {
        let name: String = p
            .name
            .chars()
            .take(ultar_bus::messages::vpn::MAX_NAME)
            .collect();
        let endpoint: String = p.endpoint.chars().take(64).collect();
        let item = ListItem::new(Label::data(&name))
            .sublabel(Label::data(&endpoint))
            .icon(IconId::Vpn);
        let item = if on && active == Some(p.name.as_str()) {
            item.selected()
        } else {
            item
        };
        row(view, card, item, Target::VpnProfile(p.name.clone()), None)?;
    }
    let settings = Button::new(Label::key("desk.vpn.settings")).variant(ButtonVariant::Quiet);
    let settings = view.ui.add(card, settings)?;
    view.mark(settings, Target::VpnSettings);
    Ok(())
}
