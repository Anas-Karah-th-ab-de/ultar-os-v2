//! The Drivers and hardware page (board *SettingsDrivers*).

use super::{k, metrics, muted, spec, title, Cx};
use crate::action::Act;
use ultar_bus::messages::device_service::{DeviceInfo, DevicePhase};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::FontRole;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Badge, Button, ButtonVariant, Card, Icon, InfoBar, Label, Row, Select, Text, Tone,
};

/// Class of a device for its name and icon: the active driver's class, else the first
/// capability hint, else `device`.
pub fn class_of(d: &DeviceInfo) -> &str {
    d.active
        .as_ref()
        .map(|a| a.class.as_str())
        .or_else(|| d.capabilities.first().map(String::as_str))
        .unwrap_or("device")
}

/// Known classes (catalog keys `app.settings.drivers.class.<class>`).
pub const CLASSES: [&str; 10] = [
    "gpu",
    "wifi",
    "net",
    "audio_out",
    "bluetooth",
    "input",
    "camera",
    "fingerprint",
    "storage",
    "device",
];

/// Catalog key of a device's class name.
pub fn class_key(d: &DeviceInfo) -> String {
    let class = class_of(d);
    let class = if CLASSES.contains(&class) {
        class
    } else {
        "device"
    };
    format!("app.settings.drivers.class.{class}")
}

fn class_icon(class: &str) -> IconId {
    match class {
        "gpu" => IconId::Monitor,
        "wifi" => IconId::Wifi,
        "net" => IconId::Cable,
        "audio_out" => IconId::Speaker,
        "bluetooth" => IconId::Bluetooth,
        "input" => IconId::Mouse,
        "camera" => IconId::Video,
        "fingerprint" => IconId::Lock,
        "storage" => IconId::Disk,
        _ => IconId::Chip,
    }
}

/// Model line of a device (its label, else the hardware id).
pub fn model(d: &DeviceInfo) -> Label {
    Label::data(d.label.as_deref().unwrap_or(&d.hardware_id))
}

/// `true` when the device works with limits: its reason key names a limitation
/// (`*.limited.*`); other reasons of an active device are notes ("full acceleration").
pub fn limited(d: &DeviceInfo) -> bool {
    d.reason
        .as_ref()
        .is_some_and(|r| r.key.contains(".limited."))
}

/// The state badge: tone and key.
pub fn state_badge(d: &DeviceInfo) -> (Tone, &'static str) {
    match d.phase {
        DevicePhase::Active if limited(d) => (Tone::Warning, "app.settings.drivers.state.limited"),
        DevicePhase::Active => (Tone::Success, "app.settings.drivers.state.active"),
        DevicePhase::NoDriver => (Tone::Danger, "app.settings.drivers.state.none"),
        DevicePhase::Failed => (Tone::Danger, "app.settings.drivers.state.failed"),
        DevicePhase::Detected | DevicePhase::Probing => {
            (Tone::Neutral, "app.settings.drivers.state.probing")
        }
    }
}

/// Choices of the driver select: the active driver first ("recommended"), then the others.
pub fn driver_choices(d: &DeviceInfo) -> Vec<Label> {
    let mut list = vec![k("app.settings.drivers.recommended")];
    list.extend(
        d.alternatives
            .iter()
            .filter(|a| Some(&a.id) != d.active.as_ref().map(|x| &x.id))
            .map(|a| Label::data(&a.id)),
    );
    list
}

/// Builds the page.
pub fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    title(cx, content, k("app.settings.area.drivers"))?;
    let st = cx.state.drivers.clone();
    if let Some(error) = &st.error {
        let bar = InfoBar::new(Tone::Danger, k("app.settings.drivers.unreadable"))
            .message(crate::i18n::label(error, cx.tr));
        cx.ui.add(content, bar)?;
    }
    missing_bar(cx, content)?;
    if !st.devices.is_empty() {
        table(cx, content)?;
    }
    maintenance(cx, content)
}

fn missing_bar(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let missing = cx.state.drivers.without_driver();
    let Some(first) = missing.first() else {
        return Ok(());
    };
    let class = cx.tr.t(&class_key(first)).into_owned();
    let model = model(first).resolve(cx.tr).into_owned();
    let title = if missing.len() == 1 {
        Label::key_with(
            "app.settings.drivers.missing_one",
            &[("name", &class), ("model", &model)],
        )
    } else {
        Label::key_count(
            "app.settings.drivers.missing_many",
            "n",
            missing.len() as i64,
        )
    };
    let effect = format!("app.settings.drivers.effect.{}", class_of(first));
    let message = if missing.len() == 1 && cx.tr.has(&effect) {
        k(&effect)
    } else {
        k("app.settings.drivers.effect.device")
    };
    cx.ui
        .add(content, InfoBar::new(Tone::Danger, title).message(message))?;
    Ok(())
}

fn column_widths(cx: &Cx<'_>) -> [f32; 4] {
    let side = if super::shell::compact(cx.ui) {
        0.0
    } else {
        metrics::sidebar()
    };
    let inner = cx.ui.size().w - side - 2.0 * Space::S32.points() - 2.0 * Space::S16.points();
    metrics::driver_columns(inner, Space::S16.points())
}

fn table(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let card = cx.ui.add(content, Card::new())?;
    let widths = column_widths(cx);
    let head = cx.ui.add(
        card,
        Flex::row()
            .gap(Space::S16)
            .padding(Padding::symmetric(Space::S16, Space::S8)),
    )?;
    for (i, key) in ["device", "driver", "state", "actions"].iter().enumerate() {
        let text = Text::new(k(&format!("app.settings.drivers.col.{key}")))
            .spec(spec::small())
            .color(ColorRole::TextMuted);
        cx.ui.add_sized(head, text, Sizing::width(widths[i]))?;
    }
    let devices = cx.state.drivers.devices.clone();
    for (index, d) in devices.iter().enumerate() {
        device_row(cx, card, index, d, &widths)?;
    }
    Ok(())
}

fn device_row(
    cx: &mut Cx<'_>,
    card: NodeId,
    index: usize,
    d: &DeviceInfo,
    w: &[f32; 4],
) -> Result<(), UiError> {
    let row = cx.ui.add(
        card,
        Flex::row()
            .gap(Space::S16)
            .padding(Padding::symmetric(Space::S16, Space::S4))
            .cross(CrossAlign::Center),
    )?;
    let first = cx.ui.add_sized(
        row,
        Flex::row().gap(Space::S12).cross(CrossAlign::Center),
        Sizing::width(w[0]),
    )?;
    cx.ui.add(first, Icon::new(class_icon(class_of(d))))?;
    let names = cx.ui.add(first, Flex::column())?;
    cx.ui
        .add(names, Text::new(k(&class_key(d))).spec(spec::label()))?;
    muted(cx.ui, names, model(d))?;
    let driver = d
        .active
        .as_ref()
        .map_or_else(|| Label::data("—"), |a| Label::data(&a.id));
    let mono = TextSpec::new(TypeSize::S14).role(FontRole::Mono);
    cx.ui
        .add_sized(row, Text::new(driver).spec(mono), Sizing::width(w[1]))?;
    let state = cx.ui.add_sized(
        row,
        Flex::column().cross(CrossAlign::Start),
        Sizing::width(w[2]),
    )?;
    let (tone, key) = state_badge(d);
    cx.ui
        .add(state, Badge::new(k(key)).tone(tone).with_icon())?;
    if let Some(reason) = &d.reason {
        muted(cx.ui, state, crate::i18n::label(reason, cx.tr))?;
    }
    let actions = cx.ui.add_sized(
        row,
        Flex::row().gap(Space::S8).cross(CrossAlign::Center),
        Sizing::width(w[3]),
    )?;
    actions_cell(cx, actions, index, d)
}

/// Driver choice and rollback, or the hardware plan for a device without driver.
fn actions_cell(
    cx: &mut Cx<'_>,
    actions: NodeId,
    index: usize,
    d: &DeviceInfo,
) -> Result<(), UiError> {
    if d.active.is_none() {
        let plan = Button::new(k("app.settings.drivers.plan")).icon(IconId::Download);
        cx.act(actions, plan, Act::DriverPlan(index))?;
        return Ok(());
    }
    let class = cx.tr.t(&class_key(d)).into_owned();
    let name = Label::key_with("app.settings.drivers.alt", &[("name", &class)]);
    let select = Select::new(name, driver_choices(d));
    let alt = cx.act(actions, select, Act::DriverAlt(index))?;
    if d.alternatives.len() <= 1 {
        cx.ui
            .set_tooltip(alt, Some(k("app.settings.drivers.no_alternative")))?;
    }
    let back = Button::new(k("app.settings.drivers.rollback")).variant(ButtonVariant::Quiet);
    let back = cx.act(actions, back, Act::DriverRollback(index))?;
    if d.previous.is_none() {
        cx.ui.set_disabled(back, true)?;
        let reason = k("app.settings.drivers.nothing_to_roll_back");
        cx.ui.set_tooltip(back, Some(reason))?;
    }
    Ok(())
}

fn maintenance(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let card = cx.ui.add(content, Card::new())?;
    let sub = match &cx.state.drivers.firmware {
        Some((_, Some(error))) => k(error),
        Some((phase, None)) => k(phase),
        None => k("app.settings.drivers.fw.sub"),
    };
    let row = Row::new(k("app.settings.drivers.fw")).sublabel(sub);
    let row = cx.ui.add(card, row)?;
    let pair = cx.ui.add(row, Flex::row().gap(Space::S8))?;
    let check = cx.act(
        pair,
        Button::new(k("app.settings.drivers.fw.check")),
        Act::FirmwareCheck,
    )?;
    cx.view.live.push((Act::FirmwareCheck, check));
    cx.act(
        pair,
        Button::new(k("app.settings.drivers.fw.usb")),
        Act::FirmwareUsb,
    )?;
    let row = Row::new(k("app.settings.drivers.selftest"))
        .sublabel(k("app.settings.drivers.selftest.sub"))
        .last();
    let row = cx.ui.add(card, row)?;
    let test = cx.act(
        row,
        Button::new(k("app.settings.drivers.selftest.start")),
        Act::SelfTest,
    )?;
    if cx.state.drivers.testing {
        cx.ui.set_loading(test, true)?;
    }
    Ok(())
}
