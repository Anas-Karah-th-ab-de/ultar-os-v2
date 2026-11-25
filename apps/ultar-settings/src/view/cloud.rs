//! The cloud-model section on Settings › Assistant: says plainly that it is the last way
//! and that requests leave the device, shows whether a key is stored, and lets the person
//! enter or forget it. The key itself is never shown.

use super::{k, section, Cx};
use crate::action::Act;
use ultar_ui::layout::Sizing;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Label, Row};

/// Adds the section; nothing on a device without the cloud-model service.
pub(crate) fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let Some(cloud) = cx.state.status.cloud.clone() else {
        return Ok(());
    };
    let (card, _) = section(
        cx.ui,
        content,
        Sizing::AUTO,
        k("app.settings.cloud.title"),
        Some(k("app.settings.cloud.sub")),
    )?;
    let provider: String = cloud.provider.chars().take(32).collect();
    let state = if cloud.configured {
        "app.settings.cloud.on"
    } else {
        "app.settings.cloud.off"
    };
    // one control per row: the provider with its state and the key button, then what
    // leaves the device with the button that forgets the key
    let row = cx
        .ui
        .add(card, Row::new(Label::data(&provider)).sublabel(k(state)))?;
    let enter = if cloud.configured {
        "app.settings.cloud.replace"
    } else {
        "app.settings.cloud.enter"
    };
    cx.act(row, Button::new(k(enter)), Act::CloudKey)?;
    let row = cx
        .ui
        .add(card, Row::new(k("app.settings.cloud.leaves")).last())?;
    if cloud.configured {
        let clear = Button::new(k("app.settings.cloud.clear")).variant(ButtonVariant::Danger);
        cx.act(row, clear, Act::CloudClear)?;
    }
    Ok(())
}
