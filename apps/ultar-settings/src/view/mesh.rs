//! Settings › Mesh (boards *SettingsMesh*, *MeshDevice*; docs/ui/mesh.md §2.1, §2.3): the
//! switch, this device with its id, the paired devices, the devices in reach, and the page
//! of one paired device with its eight rights. Device names are data from the network.

use super::{k, muted, section, title, Cx};
use crate::action::Act;
use crate::i18n::label;
use crate::state::mesh::RIGHTS;
use ultar_bus::messages::mesh::{DeviceKind, MeshState, Peer, Right};
use ultar_ui::icon::IconId;
use ultar_ui::layout::Sizing;
use ultar_ui::tokens::ColorRole;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Card, InfoBar, Label, List, ListItem, Row, Switch, Text, Tone,
};

/// Most devices shown in one list.
pub const MAX_SHOWN: usize = 24;
/// Longest device name shown (characters).
const MAX_NAME: usize = 48;

/// The word of a kind of device (key suffix).
pub fn kind_word(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Phone => "phone",
        DeviceKind::Tablet => "tablet",
        DeviceKind::Desktop => "desktop",
        DeviceKind::Server => "server",
        DeviceKind::Board => "board",
        DeviceKind::Tv => "tv",
        DeviceKind::Speaker => "speaker",
        DeviceKind::Other => "other",
    }
}

fn kind_icon(kind: DeviceKind) -> IconId {
    match kind {
        DeviceKind::Phone | DeviceKind::Tablet => IconId::Phone,
        DeviceKind::Server | DeviceKind::Board => IconId::Server,
        DeviceKind::Tv => IconId::Monitor,
        DeviceKind::Speaker => IconId::Speaker,
        DeviceKind::Desktop | DeviceKind::Other => IconId::Device,
    }
}

/// The word of a right (key suffix).
pub fn right_word(right: Right) -> &'static str {
    match right {
        Right::Handoff => "handoff",
        Right::Files => "files",
        Right::Clipboard => "clipboard",
        Right::View => "view",
        Right::Control => "control",
        Right::Audio => "audio",
        Right::Compute => "compute",
        Right::Model => "model",
    }
}

fn name(text: &str) -> Label {
    let shown: String = text.trim().chars().take(MAX_NAME).collect();
    Label::data(&shown)
}

fn kind_label(cx: &Cx<'_>, kind: DeviceKind) -> String {
    let key = format!("app.settings.mesh.kind.{}", kind_word(kind));
    k(&key).resolve(cx.tr).into_owned()
}

fn unavailable(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let error = cx.state.status.mesh.error.clone();
    let error = error.unwrap_or_else(|| ultar_bus::Text::key("app.settings.error.unavailable"));
    let bar = InfoBar::new(Tone::Warning, k("app.settings.mesh.unavailable"))
        .message(label(&error, cx.tr));
    cx.ui.add(content, bar).map(|_| ())
}

fn this_device(cx: &mut Cx<'_>, content: NodeId, mesh: &MeshState) -> Result<(), UiError> {
    let card = cx.ui.add_sized(content, Card::new(), Sizing::AUTO)?;
    let row = cx.ui.add(card, Row::new(k("app.settings.mesh.this")))?;
    cx.ui
        .add(row, Text::new(name(&mesh.name)).color(ColorRole::TextMuted))?;
    let id = Row::new(k("app.settings.mesh.id"))
        .sublabel(Label::data(&mesh.id))
        .last();
    let row = cx.ui.add(card, id)?;
    let copy = Button::new(k("app.settings.mesh.copy")).icon(IconId::Copy);
    cx.act(row, copy, Act::MeshCopy).map(|_| ())
}

fn paired(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let peers: Vec<Peer> = cx.state.status.mesh.paired().into_iter().cloned().collect();
    let (card, header) = section(
        cx.ui,
        content,
        Sizing::AUTO,
        k("app.settings.mesh.paired"),
        Some(k("app.settings.mesh.paired.sub")),
    )?;
    let pair = Button::new(k("app.settings.mesh.pair")).variant(ButtonVariant::Primary);
    cx.act(header, pair, Act::MeshPair)?;
    if peers.is_empty() {
        let row = cx
            .ui
            .add(card, Row::new(k("app.settings.mesh.none")).last())?;
        return muted(cx.ui, row, k("app.settings.mesh.none.sub")).map(|_| ());
    }
    let list = cx
        .ui
        .add(card, List::new().name(k("app.settings.mesh.paired")))?;
    for (i, p) in peers.iter().take(MAX_SHOWN).enumerate() {
        let state = if p.reachable {
            "app.settings.mesh.peer.connected"
        } else {
            "app.settings.mesh.peer.away"
        };
        let kind = kind_label(cx, p.kind);
        let item = ListItem::new(name(&p.name))
            .icon(kind_icon(p.kind))
            .sublabel(Label::key_with(state, &[("kind", &kind)]))
            .chevron();
        cx.act(list, item, Act::MeshDevice(i))?;
    }
    Ok(())
}

fn near(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let peers: Vec<Peer> = cx.state.status.mesh.near().into_iter().cloned().collect();
    if peers.is_empty() {
        return Ok(());
    }
    let (card, _) = section(
        cx.ui,
        content,
        Sizing::AUTO,
        k("app.settings.mesh.near"),
        Some(k("app.settings.mesh.near.sub")),
    )?;
    let list = cx
        .ui
        .add(card, List::new().name(k("app.settings.mesh.near")))?;
    for (i, p) in peers.iter().take(MAX_SHOWN).enumerate() {
        let kind = kind_label(cx, p.kind);
        let item = ListItem::new(name(&p.name))
            .icon(kind_icon(p.kind))
            .sublabel(Label::key_with(
                "app.settings.mesh.near.pair",
                &[("kind", &kind)],
            ));
        cx.act(list, item, Act::MeshNearPair(i))?;
    }
    Ok(())
}

/// The page.
pub(crate) fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    title(cx, content, k("app.settings.area.mesh"))?;
    let Some(mesh) = cx.state.status.mesh.state.clone() else {
        return unavailable(cx, content);
    };
    let card = cx.ui.add_sized(content, Card::new(), Sizing::AUTO)?;
    let row = Row::new(k("app.settings.area.mesh"))
        .sublabel(k("app.settings.mesh.toggle.sub"))
        .last();
    let row = cx.ui.add(card, row)?;
    let switch = Switch::new(k("app.settings.area.mesh"), mesh.enabled);
    cx.act(row, switch, Act::MeshToggle)?;
    if !mesh.enabled {
        return muted(cx.ui, content, k("app.settings.mesh.off")).map(|_| ());
    }
    this_device(cx, content, &mesh)?;
    paired(cx, content)?;
    near(cx, content)
}

/// The page of the open paired device.
pub(crate) fn device(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let Some(peer) = cx.state.status.mesh.open().cloned() else {
        return build(cx, content);
    };
    let head = cx.ui.add(
        content,
        ultar_ui::layout::Flex::row().gap(ultar_ui::tokens::Space::S8),
    )?;
    let back = Button::new(k("app.settings.mesh.back")).icon(IconId::Back);
    cx.act(head, back, Act::MeshBack)?;
    cx.ui.add(
        content,
        Text::new(name(&peer.name)).spec(super::spec::page_title()),
    )?;
    let kind = kind_label(cx, peer.kind);
    let state = if peer.reachable {
        "app.settings.mesh.peer.connected"
    } else {
        "app.settings.mesh.peer.away"
    };
    muted(cx.ui, content, Label::key_with(state, &[("kind", &kind)]))?;
    let (card, _) = section(
        cx.ui,
        content,
        Sizing::AUTO,
        k("app.settings.mesh.rights"),
        Some(k("app.settings.mesh.rights.sub")),
    )?;
    for (i, right) in RIGHTS.iter().enumerate() {
        let key = format!("app.settings.mesh.right.{}", right_word(*right));
        let mut row = Row::new(k(&key)).sublabel(k(&format!("{key}.sub")));
        if i + 1 == RIGHTS.len() {
            row = row.last();
        }
        let row = cx.ui.add(card, row)?;
        let on = peer.rights.contains(right);
        cx.act(row, Switch::new(k(&key), on), Act::MeshRight(i))?;
    }
    let unpair = Button::new(k("app.settings.mesh.unpair")).variant(ButtonVariant::Danger);
    cx.act_sized(content, unpair, Sizing::AUTO, Act::MeshUnpair)
        .map(|_| ())
}
