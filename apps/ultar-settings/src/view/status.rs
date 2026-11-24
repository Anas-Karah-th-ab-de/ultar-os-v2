//! Settings › Network, Bluetooth, Sound: what the network, Bluetooth and sound services report
//! (their retained topics `net.state`, `bluetooth.state`, `audio.state`), read-only. Changes are
//! made from the desktop's quick settings; a service that cannot be read shows why.

use super::{k, title, Cx};
use crate::areas::{StatusKind, AREAS};
use crate::i18n::label;
use ultar_bus::messages::audio::AudioState;
use ultar_bus::messages::bluetooth::BluetoothState;
use ultar_bus::messages::net::NetState;
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

/// Rows of the Network page.
pub fn net_rows(s: &NetState) -> Vec<(&'static str, Label)> {
    let none = || k("app.settings.status.none");
    let mut rows = vec![
        (
            "app.settings.status.net.wifi_present",
            yes_no(s.wifi_present),
        ),
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
    let wired = match &s.wired {
        Some(w) => match w.speed_mbit {
            Some(mbit) => Label::data(&format!("{mbit} Mbit/s")),
            None => k("app.settings.status.connected"),
        },
        None => k("app.settings.status.not_connected"),
    };
    rows.push(("app.settings.status.net.wired", wired));
    rows.push(("app.settings.status.net.airplane", yes_no(s.airplane)));
    rows.push(("app.settings.status.net.metered", yes_no(s.metered)));
    rows
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
    let rows = match kind {
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
