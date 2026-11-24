//! Step 7: hardware and drivers, tried live (board *InstallerDrivers*).

use super::{k, label_spec, muted, Cx};
use crate::action::Act;
use ultar_bus::messages::device_service::{DeviceInfo, DevicePhase};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::FontRole;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Badge, BarEdge, Button, Card, Icon, InfoBar, Label, Select, Text, Tone};

/// Class of a device (active driver's class, else capability, else `device`).
pub fn class_of(d: &DeviceInfo) -> &str {
    d.active
        .as_ref()
        .map(|a| a.class.as_str())
        .or_else(|| d.capabilities.first().map(String::as_str))
        .unwrap_or("device")
}

const CLASSES: [&str; 9] = [
    "gpu",
    "wifi",
    "net",
    "audio_out",
    "bluetooth",
    "input",
    "camera",
    "fingerprint",
    "storage",
];

/// Catalog key of a device's class name.
pub fn class_key(d: &DeviceInfo) -> String {
    let c = class_of(d);
    format!(
        "installer.class.{}",
        if CLASSES.contains(&c) { c } else { "device" }
    )
}

fn icon(class: &str) -> IconId {
    match class {
        "gpu" => IconId::Monitor,
        "wifi" => IconId::Wifi,
        "net" => IconId::Cable,
        "audio_out" => IconId::Speaker,
        "bluetooth" => IconId::Bluetooth,
        "input" => IconId::Mouse,
        "camera" => IconId::Video,
        "fingerprint" => IconId::Lock,
        _ => IconId::Chip,
    }
}

/// Tone and key of the state badge (a reason key with `.limited.` = works with limits).
pub fn badge(d: &DeviceInfo) -> (Tone, &'static str) {
    let limited = d
        .reason
        .as_ref()
        .is_some_and(|r| r.key.contains(".limited."));
    match d.phase {
        DevicePhase::Active if limited => (Tone::Warning, "installer.state.limited"),
        DevicePhase::Active => (Tone::Success, "installer.state.active"),
        DevicePhase::NoDriver => (Tone::Danger, "installer.state.none"),
        DevicePhase::Failed => (Tone::Danger, "installer.state.failed"),
        _ => (Tone::Neutral, "installer.state.probing"),
    }
}

/// Builds the page.
pub fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let head = cx
        .ui
        .add(content, Flex::row().gap(Space::S8).cross(CrossAlign::End))?;
    let texts = cx.ui.add_sized(head, Flex::column(), Sizing::grow(1))?;
    cx.ui.add(
        texts,
        Text::new(k("installer.page.7")).spec(TextSpec::title(TypeSize::S24)),
    )?;
    super::body_muted(cx.ui, texts, k("installer.page.7.sub"))?;
    let fetch = Button::new(k("installer.drivers.fetch")).icon(IconId::Download);
    let fetch = cx.act(head, fetch, Act::DriversFetch)?;
    if !cx.model.network {
        cx.ui.set_disabled(fetch, true)?;
        cx.ui
            .set_tooltip(fetch, Some(k("installer.drivers.fetch.offline")))?;
    }
    let all = cx.act(
        head,
        Button::new(k("installer.drivers.plan_all")),
        Act::DriversPlan(None),
    )?;
    cx.ui
        .set_accessible_name(all, Some(k("installer.drivers.plan_all.name")))?;
    if let Some(e) = &cx.model.devices_error {
        let bar = InfoBar::new(Tone::Danger, k("installer.drivers.unreadable"))
            .message(crate::i18n::label(e, cx.tr));
        cx.ui.add(content, bar)?;
    }
    let missing = cx.model.without_driver();
    if let Some(first) = missing.first() {
        let class = cx.tr.t(&class_key(first)).into_owned();
        let model = first
            .label
            .clone()
            .unwrap_or_else(|| first.hardware_id.clone());
        let title = Label::key_with(
            "installer.drivers.missing_one",
            &[("name", &class), ("model", &model)],
        );
        let msg = if class_of(first) == "fingerprint" {
            "installer.drivers.effect.fingerprint"
        } else {
            "installer.drivers.effect.device"
        };
        cx.ui
            .add(content, InfoBar::new(Tone::Danger, title).message(k(msg)))?;
    }
    table(cx, content)
}

/// Column shares of the device table (boards: flex 3 : 2 : 2 : 3).
const SHARES: [u8; 4] = [3, 2, 2, 3];

fn table(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let devices = cx.model.devices.clone();
    if devices.is_empty() {
        return Ok(());
    }
    let card = cx.ui.add(content, Card::new())?;
    // Boards: header 8 / 16 padded with a hairline under it; rows 4 / 16 padded, at least
    // 48 high, a hairline under every row but the last.
    let head_h = super::lines_height(cx.ui, TypeSize::S12, 1.0) + 2.0 * Space::S8.points() + 1.0;
    let head = cx.ui.add_sized(
        card,
        Card::rule(BarEdge::Bottom).flex(
            Flex::row()
                .gap(Space::S16)
                .padding(Padding::symmetric(Space::S16, Space::S8)),
        ),
        Sizing::AUTO.with_height(head_h),
    )?;
    for (i, key) in ["device", "driver", "state", "actions"].iter().enumerate() {
        let t = Text::new(k(&format!("installer.col.{key}")))
            .spec(super::small())
            .color(ColorRole::TextMuted);
        cx.ui.add_sized(head, t, Sizing::grow(SHARES[i]))?;
    }
    for (i, d) in devices.iter().enumerate() {
        row(cx, card, i, d, i + 1 == devices.len())?;
    }
    Ok(())
}

/// Height of a device row (boards: 4 / 16 padded, at least 48, a hairline under it unless
/// it is the last; texts at their exact line heights).
fn row_height(cx: &Cx<'_>, d: &DeviceInfo, last: bool) -> f32 {
    let names_h = super::lines_height(cx.ui, TypeSize::S14, 1.0)
        + super::lines_height(cx.ui, TypeSize::S12, 1.0);
    // the badge: 24 high plus its two borders (CSS content box)
    let state_h = 26.0
        + if d.reason.is_some() {
            super::lines_height(cx.ui, TypeSize::S12, 1.0)
        } else {
            0.0
        };
    let rule = if last { 0.0 } else { 1.0 };
    (names_h.max(state_h).max(32.0) + 2.0 * Space::S4.points()).max(48.0) + rule
}

/// The row container: 4 / 16 padded, the hairline under it unless it is the last.
fn row_frame(cx: &mut Cx<'_>, card: NodeId, h: f32, last: bool) -> Result<NodeId, UiError> {
    let flex = Flex::row()
        .gap(Space::S16)
        .padding(Padding::symmetric(Space::S16, Space::S4))
        .cross(CrossAlign::Center);
    if last {
        cx.ui.add_sized(card, flex, Sizing::AUTO.with_height(h))
    } else {
        let row = Card::rule(BarEdge::Bottom).flex(flex);
        cx.ui.add_sized(card, row, Sizing::AUTO.with_height(h))
    }
}

fn row(cx: &mut Cx<'_>, card: NodeId, i: usize, d: &DeviceInfo, last: bool) -> Result<(), UiError> {
    let r = row_frame(cx, card, row_height(cx, d, last), last)?;
    let c0 = cx.ui.add_sized(
        r,
        Flex::row().gap(Space::S12).cross(CrossAlign::Center),
        Sizing::grow(SHARES[0]),
    )?;
    cx.ui.add(c0, Icon::new(icon(class_of(d))))?;
    let names = cx.ui.add(c0, Flex::column())?;
    cx.ui
        .add(names, Text::new(k(&class_key(d))).spec(label_spec()))?;
    muted(
        cx.ui,
        names,
        Label::data(d.label.as_deref().unwrap_or(&d.hardware_id)),
    )?;
    let driver = d
        .active
        .as_ref()
        .map_or_else(|| Label::data("—"), |a| Label::data(&a.id));
    cx.ui.add_sized(
        r,
        Text::new(driver).spec(TextSpec::new(TypeSize::S14).role(FontRole::Mono)),
        Sizing::grow(SHARES[1]),
    )?;
    let c2 = cx.ui.add_sized(
        r,
        Flex::column().cross(CrossAlign::Start),
        Sizing::grow(SHARES[2]),
    )?;
    let (tone, key) = badge(d);
    cx.ui.add(c2, Badge::new(k(key)).tone(tone).with_icon())?;
    if let Some(reason) = &d.reason {
        muted(cx.ui, c2, crate::i18n::label(reason, cx.tr))?;
    }
    let c3 = cx.ui.add_sized(
        r,
        Flex::row()
            .gap(Space::S8)
            .cross(CrossAlign::Center)
            .main(MainAlign::End),
        Sizing::grow(SHARES[3]),
    )?;
    actions(cx, c3, i, d)
}

/// Driver choice and live test, or the hardware plan of a device without driver.
fn actions(cx: &mut Cx<'_>, c3: NodeId, i: usize, d: &DeviceInfo) -> Result<(), UiError> {
    if d.active.is_none() {
        let plan = Button::new(k("installer.drivers.plan")).icon(IconId::Download);
        let plan = cx.act(c3, plan, Act::DriversPlan(Some(i)))?;
        let class = cx.tr.t(&class_key(d)).into_owned();
        let name = Label::key_with("installer.drivers.plan.name", &[("name", &class)]);
        cx.ui.set_accessible_name(plan, Some(name))?;
        return Ok(());
    }
    let class = cx.tr.t(&class_key(d)).into_owned();
    let mut options = vec![k("installer.drivers.recommended")];
    let active = d.active.as_ref().map(|a| a.id.clone());
    options.extend(
        d.alternatives
            .iter()
            .filter(|a| Some(&a.id) != active.as_ref())
            .map(|a| Label::data(&a.id)),
    );
    let name = Label::key_with("installer.drivers.alt", &[("name", &class)]);
    cx.act(c3, Select::new(name, options).fit(), Act::DriversAlt(i))?;
    let test =
        Button::new(k("installer.drivers.test")).variant(ultar_ui::widgets::ButtonVariant::Quiet);
    let test = cx.act(c3, test, Act::DriversTest(i))?;
    let name = Label::key_with("installer.drivers.test.name", &[("name", &class)]);
    cx.ui.set_accessible_name(test, Some(name))?;
    Ok(())
}
