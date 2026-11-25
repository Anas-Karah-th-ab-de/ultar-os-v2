//! The recent apps: newest first, each to open again or to close, and "close all".

use super::home::small;
use super::{app_label, bars, Builder};
use crate::click::Click;
use crate::model::Mobile;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Card, Icon, IconButton, Label, Scroll, Text};

/// Builds the recent apps.
pub fn build(b: &mut Builder<'_>, root: NodeId, m: &Mobile) -> Result<(), UiError> {
    bars::status(b, root, m, true)?;
    let head = b.flex(
        root,
        Flex::row()
            .main(MainAlign::SpaceBetween)
            .cross(CrossAlign::Center)
            .padding(Padding::symmetric(Space::S16, Space::S8)),
        Sizing::AUTO,
    )?;
    let spec = TextSpec::title(TypeSize::S20).weight(Weight::Semibold);
    b.add(
        head,
        Text::new(Label::key("mobile.recents.title")).spec(spec),
        Sizing::AUTO,
    )?;
    let apps: Vec<_> = m.recents.iter().filter_map(|id| m.app(id)).collect();
    if !apps.is_empty() {
        let all = Button::new(Label::key("mobile.recents.clear")).variant(ButtonVariant::Quiet);
        b.click(head, all, Sizing::AUTO, Click::ClearRecents)?;
    }
    let list = Flex::column()
        .gap(Space::S8)
        .padding(Padding::symmetric(Space::S16, Space::S8));
    let list = b.add(root, Scroll::new().flex(list), Sizing::grow(1))?;
    if apps.is_empty() {
        let empty = b.flex(
            list,
            Flex::column()
                .cross(CrossAlign::Center)
                .gap(Space::S8)
                .padding(Padding::all(Space::S32)),
            Sizing::AUTO,
        )?;
        b.add(empty, Icon::new(IconId::Workspaces), Sizing::AUTO)?;
        b.add(
            empty,
            small(Label::key("mobile.recents.none")),
            Sizing::AUTO,
        )?;
    }
    for app in apps {
        entry(b, list, app)?;
    }
    bars::nav(b, root)
}

/// One recent app: open it again, or close it.
fn entry(b: &mut Builder<'_>, list: NodeId, app: &crate::model::apps::App) -> Result<(), UiError> {
    let flex = Flex::row()
        .gap(Space::S8)
        .cross(CrossAlign::Center)
        .padding(Padding::all(Space::S8));
    let card = b.add(list, Card::new().flex(flex), Sizing::AUTO)?;
    let open = Button::new(app_label(app))
        .variant(ButtonVariant::Quiet)
        .icon(app.icon);
    b.click(card, open, Sizing::grow(1), Click::App(app.id.clone()))?;
    let close = IconButton::new(IconId::Close, Label::key("mobile.recents.close"));
    let id = app.id.clone();
    b.click(card, close, Sizing::AUTO, Click::CloseRecent(id))?;
    Ok(())
}
