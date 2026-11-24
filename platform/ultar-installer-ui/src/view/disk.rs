//! Steps 10 and 11: disk and encryption (board *InstallerDisk*).

use super::{k, muted, Cx};
use crate::action::Act;
use crate::engine::{EngineError, PartRole, Plan};
use crate::wizard::{DiskMode, Encryption};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    BarEdge, Button, Card, Icon, IconButton, InfoBar, Label, RadioGroup, RadioLayout, Row, Select,
    Slider, Text, TextDirection, TextField, Tone,
};

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

/// A card with a head (title, description, optional end control) set off by a hairline.
fn card(
    cx: &mut Cx<'_>,
    parent: NodeId,
    title: &str,
    sub: &str,
) -> Result<(NodeId, NodeId), UiError> {
    let card = cx.ui.add_sized(parent, Card::new(), Sizing::grow(1))?;
    let head = cx.ui.add(
        card,
        Card::rule(BarEdge::Bottom).flex(
            Flex::row()
                .padding(Padding::all(Space::S16))
                .gap(Space::S16)
                .cross(CrossAlign::Center),
        ),
    )?;
    let texts = cx.ui.add_sized(head, Flex::column(), Sizing::grow(1))?;
    let h2 = TextSpec::title(TypeSize::S16).weight(Weight::Semibold);
    cx.ui.add(texts, Text::new(k(title)).spec(h2))?;
    muted(cx.ui, texts, k(sub))?;
    Ok((card, head))
}

fn body(cx: &mut Cx<'_>, card: NodeId) -> Result<NodeId, UiError> {
    cx.ui.add(
        card,
        Flex::column()
            .padding(Padding::all(Space::S16))
            .gap(Space::S12),
    )
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
    let select = Select::new(k("installer.disk"), options)
        .selected(w.disk.unwrap_or(0))
        .fit();
    let select = cx.act(head, select, Act::DiskSelect)?;
    if w.disks.is_empty() {
        cx.ui.set_disabled(select, true)?;
    }
    let body = body(cx, card)?;
    let group = mode_group(&w);
    cx.act(body, group, Act::DiskMode)?;
    match w.layout.mode {
        DiskMode::Manual => manual(cx, body)?,
        _ => {
            let share = Slider::new(k("installer.disk.share"), 0.0, 100.0, 5.0)
                .value(f32::from(w.layout.share))
                .unit(Label::data("%"));
            let share = cx.act(body, share, Act::DiskShare)?;
            if w.layout.mode != DiskMode::Beside {
                cx.ui.set_disabled(share, true)?;
            }
        }
    }
    if let Some(Err(e)) = &w.plan {
        let bar = InfoBar::new(Tone::Danger, k(e.key())).message(error_detail(e));
        cx.ui.add(body, bar)?;
    }
    let note = cx
        .ui
        .add(body, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    cx.ui.add(
        note,
        Icon::new(IconId::Info).small().color(ColorRole::TextMuted),
    )?;
    muted(cx.ui, note, k("installer.disk.note"))?;
    Ok(())
}

/// The three ways of using the disk; "beside" shows the split, both need a GPT.
fn mode_group(w: &crate::wizard::Wizard) -> RadioGroup {
    let index = match w.layout.mode {
        DiskMode::Whole => 0,
        DiskMode::Beside => 1,
        DiskMode::Manual => 2,
    };
    let modes = ["whole", "beside", "manual"];
    let mut group = RadioGroup::new(
        k("installer.disk.mode"),
        modes
            .iter()
            .map(|m| k(&format!("installer.disk.{m}")))
            .collect(),
    )
    .selected(index)
    .layout(RadioLayout::Cards);
    for (i, m) in modes.iter().enumerate() {
        group = group.detail(i, k(&format!("installer.disk.{m}.sub")));
    }
    let chosen = w.disk.and_then(|i| w.disks.get(i));
    if let Some(d) = chosen {
        if d.table.is_none() {
            let reason = k("installer.reason.no_table");
            group = group.unavailable(1, reason.clone()).unavailable(2, reason);
        } else if let Some(split) = beside_split(w, d) {
            group = group.split(1, split);
        }
    }
    group
}

/// The parts of the disk after installing beside: existing partitions together, then
/// Ultar OS (from the plan), as weights in MiB.
fn beside_split(
    w: &crate::wizard::Wizard,
    d: &crate::engine::DiskInfo,
) -> Option<Vec<(Label, u32)>> {
    let used: u64 = d.table.iter().flatten().map(|e| e.bytes()).sum();
    let ours: u64 = match &w.plan {
        Some(Ok(p)) if p.keep.is_some() => ultar_slots::install::parts(&p.layout)
            .map(|t| t.sectors * 512)
            .sum(),
        _ => 0,
    };
    let mib = |b: u64| u32::try_from((b >> 20).max(1)).unwrap_or(u32::MAX);
    let others = d
        .table
        .iter()
        .flatten()
        .map(|e| e.name.as_str())
        .find(|n| !n.is_empty())
        .unwrap_or("—")
        .to_owned();
    let product = cx_product();
    Some(vec![
        (
            Label::data(&format!("{others} · {}", size_text(used))),
            mib(used),
        ),
        (
            Label::data(&format!("{product} · {}", size_text(ours))),
            mib(ours.max(1 << 20)),
        ),
    ])
}

fn cx_product() -> &'static str {
    ultar_brand::PRODUCT
}

fn error_detail(e: &EngineError) -> Label {
    match e {
        EngineError::NoFreeSpace(need, free) | EngineError::TooSmall(need, free) => {
            Label::key_with(
                "installer.error.sizes",
                &[("need", &size_text(*need)), ("have", &size_text(*free))],
            )
        }
        _ => Label::data(""),
    }
}

/// The manual partition editor: every partition with its role and a delete button, then a
/// new partition (size in MiB, created in the largest free region).
fn manual(cx: &mut Cx<'_>, body: NodeId) -> Result<(), UiError> {
    let w = cx.model.wizard.clone();
    let d = w.disk.and_then(|i| w.disks.get(i)).cloned();
    let list = cx.ui.add(body, Card::new())?;
    let roles: Vec<Label> = PartRole::ALL.iter().map(|r| k(r.key())).collect();
    for (i, p) in w.layout.manual.iter().enumerate() {
        let last = i + 1 == w.layout.manual.len();
        manual_row(cx, list, (i, p, last), d.as_ref(), &roles)?;
    }
    let new = cx
        .ui
        .add(body, Flex::row().gap(Space::S8).cross(CrossAlign::End))?;
    let size = TextField::new(k("installer.manual.size"))
        .labelled()
        .direction(TextDirection::Ltr)
        .max_len(9);
    let size = cx.act_sized(new, size, Sizing::grow(1), Act::ManualSize)?;
    cx.view.manual_size = Some(size);
    cx.act(
        new,
        Button::new(k("installer.manual.create")),
        Act::ManualCreate,
    )?;
    if let Ok(src) = Plan::minimum(&w.source) {
        let mib = |s: u64| (s / ultar_slots::install::MIB).to_string();
        muted(
            cx.ui,
            body,
            Label::key_with(
                "installer.manual.minimum",
                &[
                    ("esp", &mib(src[0])),
                    ("a", &mib(src[1])),
                    ("b", &mib(src[2])),
                    ("data", &mib(src[3])),
                ],
            ),
        )?;
    }
    Ok(())
}

/// One partition of the manual layout: name and size, its use, delete.
fn manual_row(
    cx: &mut Cx<'_>,
    list: NodeId,
    (i, p, last): (usize, &crate::engine::ManualPart, bool),
    d: Option<&crate::engine::DiskInfo>,
    roles: &[Label],
) -> Result<(), UiError> {
    let name = p
        .existing
        .and_then(|e| d?.table.as_ref()?.iter().find(|x| x.index == e))
        .map_or_else(
            || cx.tr.t("installer.manual.new").into_owned(),
            |e| e.name.clone(),
        );
    let label = Label::data(&format!("{name} · {}", size_text(p.sectors * 512)));
    let mut row = Row::new(label).sublabel(Label::key_with(
        "installer.manual.at",
        &[("mib", &(p.first / ultar_slots::install::MIB).to_string())],
    ));
    if last {
        row = row.last();
    }
    let row = cx.ui.add(list, row)?;
    let ctl = cx
        .ui
        .add(row, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    let index = PartRole::ALL.iter().position(|r| *r == p.role).unwrap_or(0);
    let mut select = Select::new(
        Label::key_with("installer.manual.role", &[("name", &name)]),
        roles.to_vec(),
    )
    .selected(index)
    .fit();
    if p.existing.is_none() {
        select = select.unavailable(1, k("installer.manual.delete_new"));
    }
    cx.act(ctl, select, Act::ManualRole(i))?;
    let del = IconButton::new(
        IconId::Trash,
        Label::key_with("installer.manual.delete", &[("name", &name)]),
    );
    let del = cx.act(ctl, del, Act::ManualDelete(i))?;
    if p.role == PartRole::Delete {
        cx.ui.set_disabled(del, true)?;
    }
    Ok(())
}

fn encryption(cx: &mut Cx<'_>, row: NodeId) -> Result<(), UiError> {
    let w = cx.model.wizard.clone();
    let (card, _) = card(cx, row, "installer.enc", "installer.enc.sub")?;
    let body = body(cx, card)?;
    let index = match w.encryption {
        Encryption::None | Encryption::Tpm => 0,
        Encryption::Passphrase => 1,
    };
    let group = RadioGroup::new(
        k("installer.enc.mode"),
        vec![k("installer.enc.none"), k("installer.enc.pass")],
    )
    .selected(index)
    .layout(RadioLayout::Cards)
    .detail(0, k("installer.enc.none.sub"))
    .detail(1, k("installer.enc.pass.sub"));
    cx.act(body, group, Act::EncMode)?;
    if w.encryption == Encryption::Passphrase {
        passphrase_fields(cx, body, &w)?;
    }
    // TPM: shown, not available while the kernel has no TPM driver.
    let tpm = cx.ui.add(
        body,
        Card::padded().flex(Flex::column().padding(Padding::all(Space::S12))),
    )?;
    let title = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);
    cx.ui.add(
        tpm,
        Text::new(k("installer.enc.tpm"))
            .spec(title)
            .color(ColorRole::ControlBorder),
    )?;
    muted(cx.ui, tpm, k("installer.enc.tpm.reason"))?;
    Ok(())
}

/// The passphrase, its repetition and the show button beside the first field.
fn passphrase_fields(
    cx: &mut Cx<'_>,
    body: NodeId,
    w: &crate::wizard::Wizard,
) -> Result<(), UiError> {
    let fields = cx.ui.add(
        body,
        Flex::column().gap(Space::S8).padding(Padding {
            top: Some(Space::S8),
            ..Padding::default()
        }),
    )?;
    let line = cx
        .ui
        .add(fields, Flex::row().gap(Space::S8).cross(CrossAlign::End))?;
    let first = TextField::password(k("installer.enc.pass.field"))
        .direction(TextDirection::Auto)
        .labelled()
        .reveal_outside()
        .text(w.secrets.passphrase[0].expose_for_field())
        .max_len(crate::secret::MAX_SECRET);
    let a = cx.act_sized(line, first, Sizing::grow(1), Act::EncPass(0))?;
    let show = IconButton::new(IconId::Eye, k("installer.enc.show"));
    cx.act(line, show, Act::EncShow)?;
    let again = TextField::password(k("installer.enc.pass.again"))
        .direction(TextDirection::Auto)
        .labelled()
        .reveal_outside()
        .text(w.secrets.passphrase[1].expose_for_field())
        .max_len(crate::secret::MAX_SECRET);
    let b = cx.act(fields, again, Act::EncPass(1))?;
    cx.view.passes = vec![a, b];
    let [p1, p2] = &w.secrets.passphrase;
    if !p2.is_empty() && p1 != p2 {
        cx.ui.set_error(b, Some(k("installer.enc.differ")))?;
    }
    Ok(())
}
