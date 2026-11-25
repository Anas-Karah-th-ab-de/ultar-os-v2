//! "Continue from ‹device›?" (`sysdialog.handoff`, board CompactMeshIncoming,
//! docs/ui/mesh.md §2.4): who offers, what it is, that it comes from a paired device over an
//! encrypted channel, that this is a question of the system – Accept above Decline. Names and
//! titles are data from another device and shown as plain text.

use super::sheet::{Sheet, SMALL};
use super::DialogClick;
use crate::model::apps::system_apps;
use ultar_bus::messages::mesh::Stage;
use ultar_bus::messages::mesh_dialog::AskHandoff;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{Overflow, Weight};
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Badge, Card, Icon, Label, Text, Tone};

/// Longest title shown (characters).
pub const MAX_TITLE: usize = 96;
/// Longest device or app name shown (characters).
pub const MAX_NAME: usize = 64;

/// A size as people read it: `1.2 MB`, digits 0-9, the unit as data.
pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let (mut unit, mut divisor) = (0, 1u64);
    while bytes >= divisor.saturating_mul(1024) && unit < UNITS.len() - 1 {
        divisor = divisor.saturating_mul(1024);
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    let (bytes, divisor) = (u128::from(bytes), u128::from(divisor));
    let tenths = (bytes * 10 + divisor / 2) / divisor;
    format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
}

/// The app's name as the phone shows it: a system app by its catalog key (`editor` and
/// `org.ultar.editor` alike), any other by its id (data).
fn app_name(app: &str, tr: &ultar_i18n::Translator) -> String {
    let known = system_apps()
        .into_iter()
        .find(|a| a.id == app || a.id.rsplit('.').next() == Some(app));
    let name = match known {
        Some(a) => match &a.key {
            Some(key) => tr.t(key).into_owned(),
            None => a.name,
        },
        None => app.to_owned(),
    };
    name.chars().take(MAX_NAME).collect()
}

/// "App · size · at the same place", as far as each part is known.
pub fn detail(p: &AskHandoff, tr: &ultar_i18n::Translator) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !p.app.is_empty() {
        parts.push(app_name(&p.app, tr));
    }
    parts.extend(p.size.map(size_text));
    if p.stage == Stage::State {
        parts.push(tr.t("mobile.dialog.handoff.same_place").into_owned());
    }
    parts.join(" · ")
}

/// The card that names what is offered.
fn offered(b: &mut Sheet<'_>, body: NodeId, p: &AskHandoff) -> Result<(), UiError> {
    let what = p.title.chars().take(MAX_TITLE).collect::<String>();
    let row = Flex::row()
        .padding(Padding::all(Space::S12))
        .gap(Space::S12);
    let card = b.add(body, Card::new().flex(row), Sizing::AUTO)?;
    let icon = Icon::new(IconId::KindText).color(ColorRole::TextMuted);
    b.add(card, icon, Sizing::AUTO)?;
    let column = b.add(card, Flex::column(), Sizing::grow(1))?;
    let strong = TextSpec::new(TypeSize::S14)
        .weight(Weight::Semibold)
        .overflow(Overflow::Wrap);
    b.add(
        column,
        Text::new(Label::data(&what)).spec(strong),
        Sizing::AUTO,
    )?;
    let line = detail(p, b.ui.translator());
    let line = Text::new(Label::data(&line)).spec(SMALL);
    b.add(column, line.color(ColorRole::TextMuted), Sizing::AUTO)?;
    Ok(())
}

/// Paired, encrypted, and the line that this is a question of the system.
fn assurances(b: &mut Sheet<'_>, body: NodeId) -> Result<(), UiError> {
    let badges = b.add(body, Flex::row().wrap().gap(Space::S8), Sizing::AUTO)?;
    let paired = Badge::new(Label::key("mobile.dialog.handoff.paired")).tone(Tone::Success);
    b.add(badges, paired.with_icon(), Sizing::AUTO)?;
    let encrypted = Badge::new(Label::key("mobile.dialog.handoff.encrypted")).tone(Tone::Success);
    b.add(badges, encrypted.with(IconId::Secure), Sizing::AUTO)?;
    let line = b.add(body, Flex::row().gap(Space::S8), Sizing::AUTO)?;
    let lock = Icon::new(IconId::Lock).small().color(ColorRole::TextMuted);
    b.add(line, lock, Sizing::AUTO)?;
    let system = Text::new(Label::key("mobile.dialog.handoff.system")).spec(SMALL);
    b.add(line, system.color(ColorRole::TextMuted), Sizing::grow(1))?;
    Ok(())
}

/// Builds the question into the sheet `body`.
pub fn handoff(b: &mut Sheet<'_>, body: NodeId, p: &AskHandoff) -> Result<(), UiError> {
    let name = p.from_name.chars().take(MAX_NAME).collect::<String>();
    let (title, text) = if p.app.is_empty() {
        (
            "mobile.dialog.handoff.file",
            "mobile.dialog.handoff.file.text",
        )
    } else {
        ("mobile.dialog.handoff.title", "mobile.dialog.handoff.text")
    };
    let who = [("name", name.as_str())];
    b.texts(Label::key_with(title, &who), Label::key_with(text, &who))?;
    offered(b, body, p)?;
    assurances(b, body)?;
    let accept = ("mobile.dialog.handoff.accept", DialogClick::Accept);
    b.yes_no(Some(accept), "mobile.dialog.handoff.decline")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::size_text;

    #[test]
    fn sizes_read_as_people_expect() {
        assert_eq!(size_text(0), "0 B");
        assert_eq!(size_text(1023), "1023 B");
        assert_eq!(size_text(1024), "1.0 KB");
        assert_eq!(size_text(1_258_291), "1.2 MB");
        assert_eq!(size_text(5 * 1024 * 1024 * 1024), "5.0 GB");
    }
}
