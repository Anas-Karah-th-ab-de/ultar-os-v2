//! Step 14: the summary and the erase confirmation (board *InstallerSummary*).

use super::{k, Cx};
use crate::action::Act;
use crate::components::Component;
use crate::wizard::{Step, KEYBOARDS, TIMEZONES};
use ultar_ui::layout::{CrossAlign, Flex, Padding};
use ultar_ui::tokens::{ColorRole, Space};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Card, Checkbox, InfoBar, Label, Row, Text, Tone};

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
            .add(row, Flex::row().gap(Space::S16).cross(CrossAlign::Center))?;
        cx.ui
            .add(pair, Text::new(value).color(ColorRole::TextMuted))?;
        let edit = Button::new(k("installer.edit")).variant(ButtonVariant::Quiet);
        cx.act(pair, edit, Act::Edit(step))?;
    }
    erase(cx, content)
}

fn erase(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let w = &cx.model.wizard;
    let disk = w.disk.and_then(|i| w.disks.get(i));
    let name = disk.map_or_else(String::new, |d| {
        format!("{} · {}", d.name, super::disk::size_text(d.bytes))
    });
    let lost = disk.map_or_else(String::new, |d| {
        if d.partitions.is_empty() {
            cx.tr.t("installer.erase.nothing").into_owned()
        } else {
            d.partitions
                .join(&format!("{} ", cx.tr.t("installer.list_sep")))
        }
    });
    let title = Label::key_with("installer.erase.disk", &[("disk", &name)]);
    let message = Label::key_with("installer.erase.lost", &[("parts", &lost)]);
    let box_ = cx.ui.add(
        content,
        Flex::column().gap(Space::S12).padding(Padding::NONE),
    )?;
    cx.ui
        .add(box_, InfoBar::new(Tone::Danger, title).message(message))?;
    cx.ui.add(
        box_,
        Text::new(k("installer.erase.safe")).color(ColorRole::TextMuted),
    )?;
    let confirm = Checkbox::new(k("installer.erase.confirm"), w.erase_confirmed);
    let id = cx.act(box_, confirm, Act::Confirm)?;
    if !matches!(w.plan, Some(Ok(_))) {
        cx.ui.set_disabled(id, true)?;
    }
    Ok(())
}
