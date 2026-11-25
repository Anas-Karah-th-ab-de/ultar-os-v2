//! The recent apps: newest first, each to open again or to close, and "close all".

use super::home::small;
use super::{app_label, Builder};
use crate::click::Click;
use crate::model::Mobile;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Card, Icon, IconButton, Label, Scroll, Text};

/// Builds the body of the recent apps.
pub fn body(b: &mut Builder<'_>, root: NodeId, m: &Mobile) -> Result<(), UiError> {
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
        entry(b, list, app, m.mesh.present().then(|| m.mesh.can_send()))?;
    }
    Ok(())
}

/// One recent app: open it again (held and swiped up: continue it on another device), the
/// mesh button when the mesh service runs (`mesh`: whether it can send now), close.
fn entry(
    b: &mut Builder<'_>,
    list: NodeId,
    app: &crate::model::apps::App,
    mesh: Option<bool>,
) -> Result<(), UiError> {
    let flex = Flex::row()
        .gap(Space::S8)
        .cross(CrossAlign::Center)
        .padding(Padding::all(Space::S8));
    let card = b.add(list, Card::new().flex(flex), Sizing::AUTO)?;
    let open = Button::new(app_label(app))
        .variant(ButtonVariant::Quiet)
        .icon(app.icon);
    let open = b.click(card, open, Sizing::grow(1), Click::App(app.id.clone()))?;
    if let Some(can) = mesh {
        b.ui.set_draggable(open, can)?;
        let send = IconButton::new(IconId::Mesh, Label::key("mobile.mesh.continue"));
        let send = b.click(card, send, Sizing::AUTO, Click::MeshSend(app.id.clone()))?;
        b.ui.set_disabled(send, !can)?;
    }
    let close = IconButton::new(IconId::Close, Label::key("mobile.recents.close"));
    let id = app.id.clone();
    b.click(card, close, Sizing::AUTO, Click::CloseRecent(id))?;
    Ok(())
}
