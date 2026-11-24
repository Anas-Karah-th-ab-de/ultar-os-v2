//! Step 14: the summary and the erase confirmation (board *InstallerSummary*).

use super::{k, Cx};
use crate::action::Act;
use crate::components::Component;
use crate::wizard::{Step, KEYBOARDS, TIMEZONES};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Card, Checkbox, Icon, Label, Row, Text};

/// Summary lines: label key, the step "Edit" opens, the value.
fn lang_line(cx: &Cx<'_>) -> String {
    let p = &cx.model.wizard.prefs;
    let lang = ["ar", "en", "de"].get(p.lang).copied().unwrap_or("ar");
    let kb = KEYBOARDS
        .get(p.keyboard)
        .copied()
        .unwrap_or("us")
        .replace('+', "_");
    let lang = cx.tr.t(&format!("installer.lang.{lang}")).into_owned();
    let kb = cx.tr.t(&format!("installer.keyboard.{kb}")).into_owned();
    format!("{lang} · {kb}")
}

fn components_line(cx: &Cx<'_>) -> String {
    let sep = format!("{} ", cx.tr.t("installer.list_sep"));
    let names: Vec<String> = cx
        .model
        .wizard
        .components
        .iter()
        .map(|c: &Component| {
            cx.tr
                .t(&format!("installer.component.{}", c.id()))
                .into_owned()
        })
        .collect();
    names.join(&sep)
}

fn encryption_key(w: &crate::wizard::Wizard) -> &'static str {
    match w.encryption {
        crate::wizard::Encryption::None => "installer.enc.none",
        crate::wizard::Encryption::Passphrase => "installer.enc.pass",
        crate::wizard::Encryption::Tpm => "installer.enc.tpm",
    }
}

/// Summary lines: label key, the step "Edit" opens, the value.
pub fn lines(cx: &Cx<'_>) -> Vec<(&'static str, Step, Label)> {
    let w = &cx.model.wizard;
    let t = |key: &str| cx.tr.t(key).into_owned();
    let account: Vec<&str> = w
        .account
        .iter()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect();
    let network = if cx.model.network {
        "installer.network.connected"
    } else {
        "installer.network.none"
    };
    let zone = TIMEZONES.get(w.prefs.timezone).copied().unwrap_or("UTC");
    vec![
        (
            "installer.sum.lang",
            Step::Language,
            Label::data(&lang_line(cx)),
        ),
        (
            "installer.sum.network",
            Step::Network,
            Label::data(&t(network)),
        ),
        (
            "installer.sum.components",
            Step::Components,
            Label::data(&components_line(cx)),
        ),
        (
            "installer.sum.encryption",
            Step::Encryption,
            Label::data(&t(encryption_key(w))),
        ),
        (
            "installer.sum.account",
            Step::Account,
            Label::data(&account.join(" · ")),
        ),
        ("installer.sum.timezone", Step::TimeZone, Label::data(zone)),
    ]
}

/// Builds the page.
pub fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let card = cx.ui.add(content, Card::new())?;
    let rows = lines(cx);
    let n = rows.len();
    for (i, (key, step, value)) in rows.into_iter().enumerate() {
        let mut row = Row::new(k(key));
        if i + 1 == n {
            row = row.last();
        }
        let row = cx.ui.add(card, row)?;
        let pair = cx
            .ui
            .add(row, Flex::row().gap(Space::S12).cross(CrossAlign::Center))?;
        cx.ui
            .add(pair, Text::new(value).color(ColorRole::TextMuted))?;
        let edit = Button::new(k("installer.edit")).variant(ButtonVariant::Quiet);
        let edit = cx.act(pair, edit, Act::Edit(step))?;
        let label = cx.tr.t(key).into_owned();
        let name = Label::key_with("installer.edit.name", &[("what", &label)]);
        cx.ui.set_accessible_name(edit, Some(name))?;
    }
    erase(cx, content)?;
    product(cx, content)
}

/// What will be installed (BRAND.md §4): product, version of the medium, company.
fn product(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let version = cx.model.version.clone().unwrap_or_else(|| "—".to_owned());
    let line = Label::key_with(
        "installer.summary.product",
        &[
            ("product", ultar_brand::PRODUCT),
            ("version", &version),
            ("vendor", ultar_brand::VENDOR),
        ],
    );
    super::muted(cx.ui, content, line)?;
    Ok(())
}

fn names(cx: &Cx<'_>, list: &[crate::engine::Existing]) -> String {
    let sep = format!("{} ", cx.tr.t("installer.list_sep"));
    let names: Vec<&str> = list.iter().map(|e| e.name.as_str()).collect();
    if names.is_empty() {
        cx.tr.t("installer.erase.nothing").into_owned()
    } else {
        names.join(&sep)
    }
}

/// The lines of the "what will be erased" card for the chosen way of using the disk.
fn erase_lines(cx: &Cx<'_>) -> Vec<Label> {
    let w = &cx.model.wizard;
    let disk = w.disk.and_then(|i| w.disks.get(i));
    let name = disk.map_or_else(String::new, |d| {
        format!("{} · {}", d.name, super::disk::size_text(d.bytes))
    });
    let plan = match &w.plan {
        Some(Ok(p)) => Some(p),
        _ => None,
    };
    match plan.and_then(|p| p.keep.as_ref().map(|k| (p, k))) {
        None => {
            let lost = disk.map_or_else(String::new, |d| {
                names(cx, d.table.as_deref().unwrap_or_default())
            });
            vec![
                Label::key_with("installer.erase.disk", &[("disk", &name)]),
                Label::key_with("installer.erase.lost", &[("parts", &lost)]),
                k("installer.erase.safe"),
            ]
        }
        Some((p, kept)) => {
            let mut out = Vec::new();
            if p.removed.is_empty() {
                out.push(k("installer.summary.free"));
            } else {
                let gone = names(cx, &p.removed);
                out.push(Label::key_with(
                    "installer.summary.removed",
                    &[("names", &gone)],
                ));
            }
            let stay = names(cx, kept);
            out.push(Label::key_with(
                "installer.summary.keep",
                &[("names", &stay)],
            ));
            out.push(k("installer.erase.safe"));
            out
        }
    }
}

fn erase(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let lines = erase_lines(cx);
    let card = cx.ui.add(content, Card::danger())?;
    let head = cx
        .ui
        .add(card, Flex::row().gap(Space::S12).cross(CrossAlign::Center))?;
    cx.ui
        .add(head, Icon::new(IconId::Error).color(ColorRole::Danger))?;
    let h2 = TextSpec::title(TypeSize::S16).weight(Weight::Semibold);
    cx.ui.add(
        head,
        Text::new(k("installer.erase.title"))
            .spec(h2)
            .color(ColorRole::Danger),
    )?;
    let list = cx.ui.add(card, Flex::column().gap(Space::S4))?;
    for (i, line) in lines.into_iter().enumerate() {
        let spec = if i == 0 {
            TextSpec::new(TypeSize::S14).weight(Weight::Bold)
        } else {
            TextSpec::new(TypeSize::S14)
        };
        cx.ui.add(list, Text::new(line).spec(spec))?;
    }
    let w = &cx.model.wizard;
    let confirm = Checkbox::new(k("installer.erase.confirm"), w.erase_confirmed).wide();
    let id = cx.act(card, confirm, Act::Confirm)?;
    if !matches!(w.plan, Some(Ok(_))) {
        cx.ui.set_disabled(id, true)?;
    }
    Ok(())
}
