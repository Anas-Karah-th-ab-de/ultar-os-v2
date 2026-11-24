//! The frame: steps at the start side, the page, the footer with Cancel / Previous / Next.

use super::{body_muted, k, muted, Cx, View};
use crate::action::Act;
use crate::model::Model;
use crate::wizard::Step;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Dialog, Icon, Label, List, ListItem, Scroll, Text};

/// Width of the steps column (board: 256 = 8 × 32).
pub fn steps_width() -> f32 {
    8.0 * Space::S32.points()
}

/// Height of the footer (board: 64 = 2 × 32).
pub fn footer_height() -> f32 {
    2.0 * Space::S32.points()
}

/// Builds the whole window for the model.
pub fn build(ui: &mut Ui, model: &Model, view: &mut View) -> Result<(), UiError> {
    ui.clear();
    *view = View::default();
    ui.set_root_flex(Flex::row().cross(CrossAlign::Stretch));
    let tr = ui.translator().clone();
    let cx = &mut Cx {
        ui,
        view,
        tr: &tr,
        model,
    };
    let root = cx.ui.root();
    steps(cx, root)?;
    let main = cx.ui.add_sized(root, Flex::column(), Sizing::grow(1))?;
    let scroll = cx.ui.add_sized(main, Scroll::new(), Sizing::grow(1))?;
    let content = cx.ui.add(
        scroll,
        Flex::column()
            .padding(Padding::symmetric(Space::S32, Space::S24))
            .gap(Space::S16),
    )?;
    super::pages::build(cx, content)?;
    footer(cx, main)?;
    if model.cancel_dialog {
        cancel_dialog(cx, root)?;
    }
    footer_state(cx.ui, model, cx.view)
}

fn steps(cx: &mut Cx<'_>, root: NodeId) -> Result<(), UiError> {
    let scroll = Scroll::new()
        .flex(
            Flex::column()
                .padding(Padding::symmetric(Space::S16, Space::S24))
                .gap(Space::S16),
        )
        .name(k("installer.steps"));
    let side = cx
        .ui
        .add_sized(root, scroll, Sizing::width(steps_width()))?;
    let brand = cx
        .ui
        .add(side, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    cx.ui
        .add(brand, Icon::new(IconId::Ring).color(ColorRole::AccentText))?;
    let title = TextSpec::new(TypeSize::S14).weight(Weight::Bold);
    cx.ui
        .add(brand, Text::new(k("installer.title")).spec(title))?;
    let list = cx
        .ui
        .add(side, List::navigation().name(k("installer.steps")))?;
    cx.view.steps = Some(list);
    let w = &cx.model.wizard;
    let page = w.step.page();
    for step in Step::ALL {
        let done = step < w.reached && step.page() != page;
        let mut item = ListItem::new(k(&step.key()));
        if done {
            item = item.icon(IconId::Check);
        }
        if step == page {
            item = item.selected();
        }
        let id = cx.act(list, item, Act::Step(step))?;
        cx.view.step_items.push(step);
        if done {
            let note = Text::new(k("installer.step.done"))
                .spec(super::small())
                .color(ColorRole::TextMuted);
            cx.ui.add(id, note)?;
        }
        if !done && step != page {
            cx.ui.set_disabled(id, true)?;
        }
    }
    Ok(())
}

fn footer(cx: &mut Cx<'_>, main: NodeId) -> Result<(), UiError> {
    let flex = Flex::row()
        .padding(Padding::symmetric(Space::S32, Space::S12))
        .gap(Space::S8)
        .cross(CrossAlign::Center);
    let bar = cx
        .ui
        .add_sized(main, flex, Sizing::AUTO.with_height(footer_height()))?;
    let step = cx.model.wizard.step;
    if step >= Step::Install {
        return Ok(());
    }
    let cancel = Button::new(k("installer.cancel")).variant(ButtonVariant::Quiet);
    cx.act(bar, cancel, Act::Cancel)?;
    let info = cx.ui.add_sized(
        bar,
        Flex::column().padding(Padding::symmetric(Space::S16, Space::S4)),
        Sizing::grow(1),
    )?;
    let note = format!("installer.footer.{}", step.page().number());
    if cx.tr.has(&note) {
        body_muted(cx.ui, info, footer_note(cx, &note))?;
    }
    let reason = muted(cx.ui, info, Label::data(""))?;
    cx.view.reason = Some(reason);
    let prev = cx.act(bar, Button::new(k("installer.prev")), Act::Prev)?;
    if step == Step::Language {
        cx.ui.set_disabled(prev, true)?;
    }
    let label = if step == Step::Summary {
        "installer.install"
    } else {
        "installer.next"
    };
    let next = cx.act(
        bar,
        Button::new(k(label)).variant(ButtonVariant::Primary),
        Act::Next,
    )?;
    cx.view.next = Some(next);
    Ok(())
}

fn footer_note(cx: &Cx<'_>, key: &str) -> Label {
    let m = cx.model;
    let working = m
        .devices
        .len()
        .saturating_sub(m.without_driver().len())
        .to_string();
    let missing = m.without_driver().len().to_string();
    Label::key_with(
        key.to_owned(),
        &[("working", &working), ("missing", &missing)],
    )
}

/// Updates Next (enabled or disabled with the reason beside it) without a rebuild.
pub fn footer_state(ui: &mut Ui, model: &Model, view: &View) -> Result<(), UiError> {
    let (Some(next), Some(reason)) = (view.next, view.reason) else {
        return Ok(());
    };
    let blocked = model.wizard.blocked();
    ui.set_disabled(next, blocked.is_some())?;
    ui.set_label(reason, blocked.map_or_else(|| Label::data(""), k))?;
    Ok(())
}

fn cancel_dialog(cx: &mut Cx<'_>, root: NodeId) -> Result<(), UiError> {
    let dialog = cx.ui.add(
        root,
        Dialog::new(k("installer.cancel.title")).flex(Flex::column().gap(Space::S16)),
    )?;
    cx.view.dialog = Some(dialog);
    muted(cx.ui, dialog, k("installer.cancel.text"))?;
    let row = cx.ui.add(
        dialog,
        Flex::row()
            .gap(Space::S8)
            .main(ultar_ui::layout::MainAlign::End),
    )?;
    let no = cx.act(row, Button::new(k("installer.cancel.no")), Act::CancelNo)?;
    let yes = Button::new(k("installer.cancel.yes")).variant(ButtonVariant::Danger);
    let yes = cx.act(row, yes, Act::CancelYes)?;
    cx.ui.set_default_action(dialog, Some(no))?;
    cx.ui.set_cancel_action(dialog, Some(no))?;
    let _ = yes;
    cx.ui.open_dialog(dialog)
}
