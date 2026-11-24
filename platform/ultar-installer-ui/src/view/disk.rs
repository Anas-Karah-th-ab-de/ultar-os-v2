//! Steps 10 and 11: disk and encryption (board *InstallerDisk*).

use super::{card_title, k, muted, Cx};
use crate::action::Act;
use crate::wizard::{DiskMode, Encryption};
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Card, InfoBar, Label, RadioGroup, Select, Slider, Text, TextField, Tone};

/// `1.0 TB` / `512 GB` – decimal units as disks are sold (Latin units, §13 no. 65).
pub fn size_text(bytes: u64) -> String {
    const GB: u64 = 1_000_000_000;
    if bytes >= 1000 * GB {
        format!("{:.1} TB", bytes as f64 / (1000 * GB) as f64)
    } else if bytes >= GB {
        format!("{} GB", bytes / GB)
    } else {
        format!("{} MB", bytes / 1_000_000)
    }
}

fn card(
    cx: &mut Cx<'_>,
    parent: NodeId,
    title: &str,
    sub: &str,
) -> Result<(NodeId, NodeId), UiError> {
    let card = cx.ui.add_sized(parent, Card::new(), Sizing::grow(1))?;
    let head = cx.ui.add(
        card,
        Flex::row()
            .padding(Padding::all(Space::S16))
            .gap(Space::S16)
            .cross(CrossAlign::Center),
    )?;
    let texts = cx.ui.add_sized(head, Flex::column(), Sizing::grow(1))?;
    cx.ui.add(texts, Text::new(k(title)).spec(card_title()))?;
    muted(cx.ui, texts, k(sub))?;
    Ok((card, head))
}

/// Builds the page.
pub fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let row = cx.ui.add(
        content,
        Flex::row().gap(Space::S16).cross(CrossAlign::Start),
    )?;
    disk(cx, row)?;
    encryption(cx, row)
}

fn disk(cx: &mut Cx<'_>, row: NodeId) -> Result<(), UiError> {
    let w = cx.model.wizard.clone();
    let (card, head) = card(cx, row, "installer.disk", "installer.disk.sub")?;
    let mut options: Vec<Label> = w
        .disks
        .iter()
        .map(|d| Label::data(&format!("{} · {}", d.name, size_text(d.bytes))))
        .collect();
    if options.is_empty() {
        options.push(k("installer.disk.none"));
    }
    let select = Select::new(k("installer.disk"), options).selected(w.disk.unwrap_or(0));
    let select = cx.act(head, select, Act::DiskSelect)?;
    if w.disks.is_empty() {
        cx.ui.set_disabled(select, true)?;
    }
    let body = cx.ui.add(
        card,
        Flex::column()
            .padding(Padding::all(Space::S16))
            .gap(Space::S12),
    )?;
    let modes = vec![
        k("installer.disk.whole"),
        k("installer.disk.beside"),
        k("installer.disk.manual"),
    ];
    let index = match w.disk_mode {
        DiskMode::Whole => 0,
        DiskMode::Beside => 1,
        DiskMode::Manual => 2,
    };
    cx.act(
        body,
        RadioGroup::new(k("installer.disk.mode"), modes).selected(index),
        Act::DiskMode,
    )?;
    if w.disk_mode != DiskMode::Whole {
        let bar = InfoBar::new(Tone::Warning, k("installer.reason.mode"))
            .message(k("installer.disk.mode.later"));
        cx.ui.add(body, bar)?;
    }
    let share = Slider::new(k("installer.disk.share"), 0.0, 100.0, 5.0).value(f32::from(w.share));
    let share = cx.act(body, share, Act::DiskShare)?;
    if w.disk_mode != DiskMode::Beside {
        cx.ui.set_disabled(share, true)?;
    }
    if let Some(Err(e)) = &w.plan {
        let bar = InfoBar::new(Tone::Danger, k(e.key()));
        cx.ui.add(body, bar)?;
    }
    muted(cx.ui, body, k("installer.disk.note"))?;
    Ok(())
}

fn encryption(cx: &mut Cx<'_>, row: NodeId) -> Result<(), UiError> {
    let w = cx.model.wizard.clone();
    let (card, _) = card(cx, row, "installer.enc", "installer.enc.sub")?;
    let body = cx.ui.add(
        card,
        Flex::column()
            .padding(Padding::all(Space::S16))
            .gap(Space::S12),
    )?;
    let modes = vec![
        k("installer.enc.none"),
        k("installer.enc.pass"),
        k("installer.enc.tpm"),
    ];
    let index = match w.encryption {
        Encryption::None => 0,
        Encryption::Passphrase => 1,
        Encryption::Tpm => 2,
    };
    cx.act(
        body,
        RadioGroup::new(k("installer.enc.mode"), modes).selected(index),
        Act::EncMode,
    )?;
    if w.encryption == Encryption::Passphrase {
        let first = TextField::password(k("installer.enc.pass.field"))
            .labelled()
            .max_len(256);
        let a = cx.act(body, first, Act::EncPass(0))?;
        let again = TextField::password(k("installer.enc.pass.again"))
            .labelled()
            .max_len(256);
        let b = cx.act(body, again, Act::EncPass(1))?;
        cx.view.passes = vec![a, b];
    }
    if w.encryption != Encryption::None {
        let bar = InfoBar::new(Tone::Warning, k("installer.reason.encryption"))
            .message(k("installer.enc.later"));
        cx.ui.add(body, bar)?;
    }
    Ok(())
}
