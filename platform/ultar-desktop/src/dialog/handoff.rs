//! The question "take what another device offers?" (`sysdialog.handoff`, board MeshIncoming,
//! docs/ui/mesh.md §2.4): who offers, what it is, that it comes from a paired device over an
//! encrypted channel, and that nothing is stored or opened before the person accepts.
//! Names and titles are data from another device.

use super::view::{button, texts};
use crate::model::apps::{base_apps, installed};
use crate::view::{Target, View};
use ultar_bus::messages::mesh::{AskHandoff, Stage};
use ultar_i18n::Translator;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{Flex, MainAlign, Padding};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Badge, Button, ButtonVariant, Card, Icon, Label, Text, Tone};

const SMALL: TextSpec = TextSpec::new(TypeSize::S12);

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
    // Rounded to one place: 1 258 291 bytes are 1.2 MB.
    let (bytes, divisor) = (u128::from(bytes), u128::from(divisor));
    let tenths = (bytes * 10 + divisor / 2) / divisor;
    format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
}

/// "App · size · opens at the same place", as far as each part is known.
fn detail(p: &AskHandoff, tr: &Translator) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !p.app.is_empty() {
        // The app's name as the shell shows it everywhere else.
        let known = base_apps().into_iter().find(|a| a.id == p.app);
        let app = known.unwrap_or_else(|| installed(&p.app));
        parts.push(app.display_name(tr).chars().take(64).collect());
    }
    parts.extend(p.size.map(size_text));
    if p.stage == Stage::State {
        parts.push(tr.t("dialog.handoff.same_place").into_owned());
    }
    parts.join(" · ")
}

/// The box that names what is offered.
fn offered(view: &mut View, root: NodeId, p: &AskHandoff, tr: &Translator) -> Result<(), UiError> {
    let what = p.title.chars().take(96).collect::<String>();
    let row = Flex::row()
        .padding(Padding::all(Space::S12))
        .gap(Space::S12);
    let card = view.ui.add(root, Card::new().flex(row))?;
    let icon = Icon::new(IconId::KindText).color(ColorRole::TextMuted);
    view.ui.add(card, icon)?;
    let column = view.ui.add(card, Flex::column())?;
    let strong = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);
    view.ui
        .add(column, Text::new(Label::data(&what)).spec(strong))?;
    let line = Text::new(Label::data(&detail(p, tr))).spec(SMALL);
    view.ui.add(column, line.color(ColorRole::TextMuted))?;
    Ok(())
}

/// Paired, encrypted, and the line that this is a question of the system.
fn assurances(view: &mut View, root: NodeId) -> Result<(), UiError> {
    let badges = view.ui.add(root, Flex::row().gap(Space::S8))?;
    let paired = Badge::new(Label::key("dialog.handoff.paired")).tone(Tone::Success);
    view.ui.add(badges, paired.with_icon())?;
    let encrypted = Badge::new(Label::key("dialog.handoff.encrypted")).tone(Tone::Success);
    view.ui.add(badges, encrypted.with(IconId::Secure))?;
    let line = view.ui.add(root, Flex::row().gap(Space::S8))?;
    let lock = Icon::new(IconId::Lock).small().color(ColorRole::TextMuted);
    view.ui.add(line, lock)?;
    let system = Text::new(Label::key("dialog.handoff.system")).spec(SMALL);
    view.ui.add(line, system.color(ColorRole::TextMuted))?;
    Ok(())
}

/// Builds the question below `root`; the node that gets the focus.
pub(super) fn handoff(
    view: &mut View,
    root: NodeId,
    p: &AskHandoff,
    tr: &Translator,
) -> Result<NodeId, UiError> {
    let name = p.from_name.chars().take(64).collect::<String>();
    let (title, text) = if p.app.is_empty() {
        ("dialog.handoff.file", "dialog.handoff.file.text")
    } else {
        ("dialog.handoff.title", "dialog.handoff.text")
    };
    let who = [("name", name.as_str())];
    texts(
        view,
        Label::key_with(title, &who),
        Label::key_with(text, &who),
    )?;
    offered(view, root, p, tr)?;
    assurances(view, root)?;
    let row = Flex::row().gap(Space::S8).main(MainAlign::End);
    let row = view.ui.add(root, row)?;
    let decline = "dialog.handoff.decline";
    let decline = button(view, row, decline, ButtonVariant::Plain, Target::Deny)?;
    let accept = Button::new(Label::key("dialog.handoff.accept"))
        .variant(ButtonVariant::Primary)
        .icon(IconId::Check);
    let accept = view.ui.add(row, accept)?;
    view.mark(accept, Target::AllowOnce);
    // The safe answer has the focus: Enter by habit declines.
    Ok(decline)
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
        assert_eq!(size_text(300_000), "293.0 KB");
        assert_eq!(size_text(5 * 1024 * 1024 * 1024), "5.0 GB");
        assert_eq!(size_text(u64::MAX), "17179869184.0 GB");
    }
}
