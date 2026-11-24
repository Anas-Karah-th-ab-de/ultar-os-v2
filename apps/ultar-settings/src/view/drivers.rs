//! The Drivers and hardware page (board *SettingsDrivers*).

use super::{k, muted, spec, title, Cx};
use crate::action::Act;
use ultar_bus::messages::device_service::{DeviceInfo, DevicePhase};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::FontRole;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Badge, BarEdge, Button, ButtonVariant, Card, Icon, InfoBar, Label, Select, Text, Tone,
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

/// Column shares of the device table (boards: flex 3 : 2 : 2 : 3).
const SHARES: [u8; 4] = [3, 2, 2, 3];
/// Narrowest page on which the four columns stay readable; below it each device is a block of
/// lines (name, driver, state, actions) and the header row is left out.
const TABLE_MIN: f32 = 720.0;

/// The size of cell `i` of a device row: its column share, or its natural size when stacked.
fn cell(narrow: bool, i: usize) -> Sizing {
    if narrow {
        Sizing::AUTO
    } else {
        Sizing::grow(SHARES[i])
    }
}

fn table(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let card = cx.ui.add(content, Card::new())?;
    let narrow = super::content_width(cx.ui) < TABLE_MIN;
    let devices = cx.state.drivers.devices.clone();
    if narrow {
        for (index, d) in devices.iter().enumerate() {
            device_row(cx, card, index, d, index + 1 == devices.len(), true)?;
        }
        return Ok(());
    }
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
        let text = Text::new(k(&format!("app.settings.drivers.col.{key}")))
            .spec(spec::small())
            .color(ColorRole::TextMuted);
        cx.ui.add_sized(head, text, Sizing::grow(SHARES[i]))?;
    }
    for (index, d) in devices.iter().enumerate() {
        device_row(cx, card, index, d, index + 1 == devices.len(), false)?;
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

fn device_row(
    cx: &mut Cx<'_>,
    card: NodeId,
    index: usize,
    d: &DeviceInfo,
    last: bool,
    narrow: bool,
) -> Result<(), UiError> {
    let flex = if narrow {
        Flex::column()
            .gap(Space::S8)
            .padding(Padding::symmetric(Space::S16, Space::S12))
            .cross(CrossAlign::Start)
    } else {
        Flex::row()
            .gap(Space::S16)
            .padding(Padding::symmetric(Space::S16, Space::S4))
            .cross(CrossAlign::Center)
    };
    let sizing = if narrow {
        Sizing::AUTO
    } else {
        Sizing::AUTO.with_height(row_height(cx, d, last))
    };
    let row = if last {
        cx.ui.add_sized(card, flex, sizing)?
    } else {
        let r = Card::rule(BarEdge::Bottom).flex(flex);
        cx.ui.add_sized(card, r, sizing)?
    };
    let first = cx.ui.add_sized(
        row,
        Flex::row().gap(Space::S12).cross(CrossAlign::Center),
        cell(narrow, 0),
    )?;
    cx.ui.add(
        first,
        Icon::new(class_icon(class_of(d))).color(ColorRole::TextMuted),
    )?;
    let names = cx.ui.add(first, Flex::column())?;
    cx.ui
        .add(names, Text::new(k(&class_key(d))).spec(spec::label()))?;
    muted(cx.ui, names, model(d))?;
    let driver = d
        .active
        .as_ref()
        .map_or_else(|| Label::data("—"), |a| Label::data(&a.id));
    let mono = TextSpec::new(TypeSize::S12).role(FontRole::Mono);
    cx.ui
        .add_sized(row, Text::new(driver).spec(mono), cell(narrow, 1))?;
    let state = cx.ui.add_sized(
        row,
        Flex::column().cross(CrossAlign::Start),
        cell(narrow, 2),
    )?;
    let (tone, key) = state_badge(d);
    cx.ui
        .add(state, Badge::new(k(key)).tone(tone).with_icon())?;
    if let Some(reason) = &d.reason {
        muted(cx.ui, state, crate::i18n::label(reason, cx.tr))?;
    }
    let actions = cx.ui.add_sized(
        row,
        Flex::row()
            .gap(Space::S8)
            .cross(CrossAlign::Center)
            .main(MainAlign::End),
        cell(narrow, 3),
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
        let plan = cx.act(actions, plan, Act::DriverPlan(index))?;
        let class = cx.tr.t(&class_key(d)).into_owned();
        let name = Label::key_with("app.settings.drivers.plan.name", &[("name", &class)]);
        cx.ui.set_accessible_name(plan, Some(name))?;
        return Ok(());
    }
    let class = cx.tr.t(&class_key(d)).into_owned();
    let name = Label::key_with("app.settings.drivers.alt", &[("name", &class)]);
    let select = Select::new(name, driver_choices(d)).fit();
    let alt = cx.act(actions, select, Act::DriverAlt(index))?;
    if d.alternatives.len() <= 1 {
        cx.ui
            .set_tooltip(alt, Some(k("app.settings.drivers.no_alternative")))?;
    }
    let back = Button::new(k("app.settings.drivers.rollback")).variant(ButtonVariant::Quiet);
    let back = cx.act(actions, back, Act::DriverRollback(index))?;
    let name = Label::key_with("app.settings.drivers.rollback.name", &[("name", &class)]);
    cx.ui.set_accessible_name(back, Some(name))?;
    if d.previous.is_none() {
        cx.ui.set_disabled(back, true)?;
        let reason = k("app.settings.drivers.nothing_to_roll_back");
        cx.ui.set_tooltip(back, Some(reason))?;
    }
    Ok(())
}

/// A settings row with a leading icon (boards: `row(…, icon=…)`): icon, label and second
/// line, the controls at the end; padding 8 / 16, gap 16, at least 56 high, a hairline under
/// it unless it is the last. Returns the node the controls go into.
fn icon_row(
    cx: &mut Cx<'_>,
    card: NodeId,
    icon: IconId,
    (label, sub): (Label, Label),
    last: bool,
) -> Result<NodeId, UiError> {
    let flex = Flex::row()
        .gap(Space::S16)
        .padding(Padding::symmetric(Space::S16, Space::S8))
        .cross(CrossAlign::Center);
    let texts_h = super::lines_height(cx.ui, TypeSize::S14, 1.0)
        + super::lines_height(cx.ui, TypeSize::S12, 1.0);
    let rule = if last { 0.0 } else { 1.0 };
    let h = (texts_h + 2.0 * Space::S8.points() + rule).max(ultar_ui::tokens::size::ROW_MIN);
    let row = if last {
        cx.ui.add_sized(card, flex, Sizing::AUTO.with_height(h))?
    } else {
        let r = Card::rule(BarEdge::Bottom).flex(flex);
        cx.ui.add_sized(card, r, Sizing::AUTO.with_height(h))?
    };
    cx.ui
        .add(row, Icon::new(icon).color(ColorRole::TextMuted))?;
    let texts = cx.ui.add_sized(row, Flex::column(), Sizing::grow(1))?;
    cx.ui.add(texts, Text::new(label).spec(spec::label()))?;
    muted(cx.ui, texts, sub)?;
    cx.ui
        .add(row, Flex::row().gap(Space::S8).cross(CrossAlign::Center))
}

fn maintenance(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let card = cx.ui.add(content, Card::new())?;
    let sub = match &cx.state.drivers.firmware {
        Some((_, Some(error))) => k(error),
        Some((phase, None)) => k(phase),
        None => k("app.settings.drivers.fw.sub"),
    };
    let pair = icon_row(
        cx,
        card,
        IconId::Update,
        (k("app.settings.drivers.fw"), sub),
        false,
    )?;
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
    let end = icon_row(
        cx,
        card,
        IconId::Chip,
        (
            k("app.settings.drivers.selftest"),
            k("app.settings.drivers.selftest.sub"),
        ),
        true,
    )?;
    let test = cx.act(
        end,
        Button::new(k("app.settings.drivers.selftest.start")),
        Act::SelfTest,
    )?;
    if cx.state.drivers.testing {
        cx.ui.set_loading(test, true)?;
    }
    Ok(())
}
