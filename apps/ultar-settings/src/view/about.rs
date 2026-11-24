//! Settings › About (docs/BRAND.md §4): the mark 96, then name, version, build, company,
//! developer, website, contact and licences. Names come from `ultar-brand` (`brand.*` keys of
//! the translator); version and build from the installed release, never a constant.

use super::{k, muted, spec, title, Cx};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::tokens::{ColorRole, Space};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Card, Icon, Label, Row, Text};

/// Size of the mark on the About page (BRAND.md §4).
pub const MARK: u16 = 96;

/// The rows: label key and value.
pub fn rows(cx: &Cx<'_>) -> Vec<(&'static str, Label)> {
    let unknown = || k("app.settings.about.unknown");
    let (version, build) = match &cx.state.release {
        Some((v, b)) => (Label::data(v), Label::data(b)),
        None => (unknown(), unknown()),
    };
    vec![
        ("brand.about.name", k("brand.product")),
        ("brand.about.version", version),
        ("brand.about.build", build),
        ("brand.about.vendor", k("brand.vendor")),
        ("brand.about.developer", k("brand.developer")),
        ("brand.about.website", Label::data(ultar_brand::WEBSITE)),
        (
            "brand.about.contact",
            Label::data(ultar_brand::CONTACT_EMAIL),
        ),
    ]
}

/// Builds the page.
pub fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    title(cx, content, k("app.settings.area.about"))?;
    let head = cx.ui.add(
        content,
        Flex::column()
            .gap(Space::S8)
            .cross(CrossAlign::Center)
            .padding(Padding::symmetric(Space::S16, Space::S16)),
    )?;
    let mark = Icon::new(IconId::Mark)
        .sized(MARK)
        .color(ColorRole::AccentText)
        .name(k("brand.label.mark"));
    cx.ui.add(head, mark)?;
    cx.ui
        .add(head, Text::new(k("brand.product")).spec(spec::page_title()))?;
    muted(cx.ui, head, k("brand.by"))?;
    let card = cx.ui.add_sized(content, Card::new(), Sizing::AUTO)?;
    let rows = rows(cx);
    for (key, value) in rows {
        let row = cx.ui.add(card, Row::new(k(key)))?;
        cx.ui
            .add(row, Text::new(value).color(ColorRole::TextMuted))?;
    }
    let row = Row::new(k("brand.about.licences"))
        .sublabel(k("app.settings.about.third_party"))
        .last();
    let row = cx.ui.add(card, row)?;
    cx.ui.add(
        row,
        Text::new(k("brand.about.licence.system")).color(ColorRole::TextMuted),
    )?;
    Ok(())
}
