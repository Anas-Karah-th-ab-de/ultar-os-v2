//! Settings › Wi-Fi, Network, Bluetooth, Sound: what the network, Bluetooth and sound services report
//! (Wi-Fi shows the Wi-Fi interface or that there is no adapter, Network the wired interfaces)
//! (their retained topics `net.state`, `bluetooth.state`, `audio.state`), read-only. Changes are
//! made from the desktop's quick settings; a service that cannot be read shows why.

use super::{k, title, Cx};
use crate::areas::{StatusKind, AREAS};
use crate::i18n::label;
use ultar_bus::messages::audio::AudioState;
use ultar_bus::messages::bluetooth::BluetoothState;
use ultar_bus::messages::net::{AddressMethod, Internet, LinkKind, LinkState, NetLink, NetState};
use ultar_ui::layout::Sizing;
use ultar_ui::tokens::ColorRole;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Card, InfoBar, Label, Row, Text, Tone};

fn yes_no(on: bool) -> Label {
    k(if on {
        "app.settings.status.yes"
    } else {
        "app.settings.status.no"
    })
}

fn percent(value: u8) -> Label {
    Label::data(&format!("{value} %"))
}

fn data_or_none(v: Option<&str>) -> Label {
    v.map_or_else(|| k("app.settings.status.none"), Label::data)
}

fn link_state(l: &NetLink) -> Label {
    k(match l.state {
        LinkState::Connected => "app.settings.status.connected",
        LinkState::Connecting => "app.settings.status.net.connecting",
        LinkState::Off => "app.settings.status.net.off",
        LinkState::NoCarrier if l.kind == LinkKind::Ethernet => "app.settings.status.net.no_cable",
        LinkState::NoCarrier => "app.settings.status.not_connected",
    })
}

/// The rows of one interface: name, state, MAC, address (with how it was set), router, DNS,
/// speed.
fn link_rows(l: &NetLink, rows: &mut Vec<(&'static str, Label)>) {
    let name = match &l.driver {
        Some(d) if !l.name.is_empty() => Label::data(&format!("{} ({d})", l.name)),
        _ => data_or_none(Some(l.name.as_str()).filter(|n| !n.is_empty())),
    };
    rows.push(("app.settings.status.net.interface", name));
    rows.push(("app.settings.status.net.state", link_state(l)));
    rows.push((
        "app.settings.status.net.mac",
        data_or_none(Some(l.mac.as_str()).filter(|m| !m.is_empty())),
    ));
    rows.push((
        "app.settings.status.net.address",
        data_or_none(l.address.as_deref()),
    ));
    if l.address.is_some() {
        let method = match l.method {
            AddressMethod::Dhcp => "app.settings.status.net.method.dhcp",
            AddressMethod::Static => "app.settings.status.net.method.static",
            AddressMethod::Unknown => "app.settings.status.none",
        };
        rows.push(("app.settings.status.net.method", k(method)));
    }
    rows.push((
        "app.settings.status.net.gateway",
        data_or_none(l.gateway.as_deref()),
    ));
    let dns = (!l.dns.is_empty()).then(|| l.dns.join(", "));
    rows.push(("app.settings.status.net.dns", data_or_none(dns.as_deref())));
    rows.push((
        "app.settings.status.net.speed",
        l.speed_mbit.map_or_else(
            || k("app.settings.status.none"),
            |m| Label::data(&format!("{m} Mbit/s")),
        ),
    ));
}

fn internet(s: &NetState) -> Label {
    k(match s.internet {
        Internet::Connected => "app.settings.status.net.internet.connected",
        Internet::LocalOnly => "app.settings.status.net.internet.local",
        Internet::Checking => "app.settings.status.net.internet.checking",
        Internet::Offline => "app.settings.status.net.internet.offline",
    })
}

/// Rows of the Network page: the wired connection, the internet, then every wired
/// interface (or that there is none).
pub fn net_rows(s: &NetState) -> Vec<(&'static str, Label)> {
    let wired = match &s.wired {
        Some(w) => match w.speed_mbit {
            Some(mbit) => Label::data(&format!("{mbit} Mbit/s")),
            None => k("app.settings.status.connected"),
        },
        None => k("app.settings.status.not_connected"),
    };
    let mut rows = vec![
        ("app.settings.status.net.wired", wired),
        ("app.settings.status.net.internet", internet(s)),
    ];
    if s.ethernet.is_empty() {
        rows.push((
            "app.settings.status.net.interface",
            k("app.settings.status.net.no_wired_adapter"),
        ));
    }
    for l in &s.ethernet {
        link_rows(l, &mut rows);
    }
    rows.push(("app.settings.status.net.metered", yes_no(s.metered)));
    rows
}

/// Rows of the Wi-Fi page: the radio, the network and the Wi-Fi interface (`None`: no
/// Wi-Fi adapter – the page says that instead).
pub fn wifi_rows(s: &NetState) -> Option<Vec<(&'static str, Label)>> {
    let link = s.wifi_link.as_ref().filter(|_| s.wifi_present)?;
    let none = || k("app.settings.status.none");
    let mut rows = vec![
        ("app.settings.status.net.wifi_on", yes_no(s.wifi_on)),
        (
            "app.settings.status.net.ssid",
            s.ssid.as_deref().map_or_else(none, Label::data),
        ),
        (
            "app.settings.status.net.signal",
            s.signal_percent.map_or_else(none, percent),
        ),
    ];
    link_rows(link, &mut rows);
    rows.push(("app.settings.status.net.airplane", yes_no(s.airplane)));
    Some(rows)
}

/// Rows of the Bluetooth page.
pub fn bluetooth_rows(s: &BluetoothState) -> Vec<(&'static str, Label)> {
    vec![
        ("app.settings.status.bt.present", yes_no(s.present)),
        ("app.settings.status.bt.on", yes_no(s.on)),
        (
            "app.settings.status.bt.connected",
            Label::data(&s.connected.to_string()),
        ),
        ("app.settings.status.bt.scanning", yes_no(s.scanning)),
    ]
}

/// Rows of the Sound page.
pub fn sound_rows(s: &AudioState) -> Vec<(&'static str, Label)> {
    let output = if s.output_name.is_empty() {
        k("app.settings.status.none")
    } else {
        Label::data(&s.output_name)
    };
    vec![
        ("app.settings.status.sound.present", yes_no(s.present)),
        ("app.settings.status.sound.output", output),
        (
            "app.settings.status.sound.volume",
            percent(s.volume_percent),
        ),
        ("app.settings.status.sound.muted", yes_no(s.muted)),
    ]
}

/// Builds the page of `kind`.
pub(crate) fn build(cx: &mut Cx<'_>, content: NodeId, kind: StatusKind) -> Result<(), UiError> {
    let area = AREAS.get(cx.state.area).copied().unwrap_or(AREAS[0]);
    title(cx, content, k(&area.label_key()))?;
    let st = &cx.state.status;
    // Wi-Fi and Network read the same service, each page its own interface
    if let (StatusKind::Network, "wifi", Some(net), None) = (kind, area.id, &st.net, &st.error) {
        if wifi_rows(net).is_none() {
            let bar = InfoBar::new(Tone::Info, k("app.settings.status.net.no_wifi_adapter"))
                .message(k("app.settings.status.net.no_wifi_adapter.detail"));
            cx.ui.add(content, bar)?;
            return Ok(());
        }
    }
    let rows = match kind {
        StatusKind::Network if area.id == "wifi" => st.net.as_ref().and_then(wifi_rows),
        StatusKind::Network => st.net.as_ref().map(net_rows),
        StatusKind::Bluetooth => st.bluetooth.as_ref().map(bluetooth_rows),
        StatusKind::Sound => st.audio.as_ref().map(sound_rows),
    };
    let (Some(rows), None) = (rows, &st.error) else {
        let error = st
            .error
            .clone()
            .unwrap_or_else(|| ultar_bus::Text::key("app.settings.error.unavailable"));
        let bar = InfoBar::new(Tone::Warning, k("app.settings.status.unavailable"))
            .message(label(&error, cx.tr));
        cx.ui.add(content, bar)?;
        return Ok(());
    };
    let card = cx.ui.add_sized(content, Card::new(), Sizing::AUTO)?;
    for (key, value) in rows {
        let row = cx.ui.add(card, Row::new(k(key)))?;
        cx.ui
            .add(row, Text::new(value).color(ColorRole::TextMuted))?;
    }
    if kind == StatusKind::Bluetooth {
        super::bluetooth::build(cx, content)?;
    }
    Ok(())
}
