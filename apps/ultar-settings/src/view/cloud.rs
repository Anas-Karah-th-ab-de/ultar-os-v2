//! The cloud-model section on Settings › Assistant (board *AssistantCloud*): says plainly
//! that it is the last way, what leaves the device and that the provider bills for it;
//! lets the person choose provider and model and enter or forget the key. The key itself
//! is never shown. Provider and model names are data.

use super::{k, section, Cx};
use crate::action::Act;
use ultar_bus::messages::agent::Brain;
use ultar_bus::messages::llm_cloud::CloudState;
use ultar_ui::icon::IconId;
use ultar_ui::layout::Sizing;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Badge, Button, ButtonVariant, Label, Row, Select, Tone};

/// Most models offered in the choice.
pub const MAX_SHOWN: usize = 40;

fn data(text: &str) -> Label {
    let shown: String = text.chars().take(64).collect();
    Label::data(&shown)
}

/// The words for the key: none, stored and accepted, or stored and not asked about yet.
pub fn key_state(cloud: &CloudState) -> &'static str {
    match (cloud.configured, cloud.verified) {
        (false, _) => "app.settings.cloud.off",
        (true, true) => "app.settings.cloud.on",
        (true, false) => "app.settings.cloud.unchecked",
    }
}

fn provider(cx: &mut Cx<'_>, card: NodeId, cloud: &CloudState) -> Result<(), UiError> {
    let row = cx
        .ui
        .add(card, Row::new(k("app.settings.cloud.provider")))?;
    let options = cloud.providers.iter().map(|p| data(&p.name)).collect();
    let chosen = cloud.providers.iter().position(|p| p.id == cloud.provider);
    let select =
        Select::new(k("app.settings.cloud.provider"), options).selected(chosen.unwrap_or(0));
    cx.act(row, select, Act::CloudProvider)?;
    if cloud.chosen().is_some_and(|p| p.needs_endpoint) {
        let address = Row::new(k("app.settings.cloud.address")).sublabel(data(&cloud.endpoint));
        let row = cx.ui.add(card, address)?;
        let change = Button::new(k("app.settings.cloud.address.change"));
        cx.act(row, change, Act::CloudAddress)?;
    }
    Ok(())
}

fn model(cx: &mut Cx<'_>, card: NodeId, cloud: &CloudState) -> Result<(), UiError> {
    if cloud.models.is_empty() {
        return Ok(());
    }
    let row = Row::new(k("app.settings.cloud.model")).sublabel(k("app.settings.cloud.model.sub"));
    let row = cx.ui.add(card, row)?;
    let models = cloud.models.iter().take(MAX_SHOWN);
    let options = models.map(|m| data(&m.name)).collect();
    let chosen = cloud.models.iter().position(|m| m.id == cloud.model);
    let select = Select::new(k("app.settings.cloud.model"), options).selected(chosen.unwrap_or(0));
    cx.act(row, select, Act::CloudModel).map(|_| ())
}

/// The four ways the helper thinks, in their order, with a mark on the one in use.
fn order(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let (card, _) = section(
        cx.ui,
        content,
        Sizing::AUTO,
        k("app.settings.cloud.order"),
        Some(k("app.settings.cloud.order.sub")),
    )?;
    let now = cx.state.status.brain.clone();
    let ways = [
        ("own", matches!(now, Some(Brain::Local))),
        ("lent", matches!(now, Some(Brain::Mesh(_)))),
        ("cloud", matches!(now, Some(Brain::Cloud))),
        ("fixed", matches!(now, Some(Brain::Minimal))),
    ];
    for (i, (way, used)) in ways.iter().enumerate() {
        let key = format!("app.settings.cloud.order.{way}");
        let mut row = Row::new(k(&key)).sublabel(k(&format!("{key}.sub")));
        if i + 1 == ways.len() {
            row = row.last();
        }
        let row = cx.ui.add(card, row)?;
        if *used {
            let mark = Badge::new(k("app.settings.cloud.order.now")).tone(Tone::Success);
            cx.ui.add(row, mark.with(IconId::Check))?;
        }
    }
    Ok(())
}

/// Adds the section; nothing on a device without the cloud-model service.
pub(crate) fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let Some(cloud) = cx.state.status.cloud.clone() else {
        return Ok(());
    };
    order(cx, content)?;
    let (card, _) = section(
        cx.ui,
        content,
        Sizing::AUTO,
        k("app.settings.cloud.title"),
        Some(k("app.settings.cloud.sub")),
    )?;
    // one control per row
    provider(cx, card, &cloud)?;
    let key = Row::new(k("app.settings.cloud.key")).sublabel(k(key_state(&cloud)));
    let row = cx.ui.add(card, key)?;
    let enter = if cloud.configured {
        "app.settings.cloud.replace"
    } else {
        "app.settings.cloud.enter"
    };
    cx.act(row, Button::new(k(enter)), Act::CloudKey)?;
    model(cx, card, &cloud)?;
    let leaves = Row::new(k("app.settings.cloud.leaves")).sublabel(k("app.settings.cloud.cost"));
    let row = cx.ui.add(card, leaves.last())?;
    if cloud.configured {
        let clear = Button::new(k("app.settings.cloud.clear")).variant(ButtonVariant::Danger);
        cx.act(row, clear, Act::CloudClear)?;
    }
    Ok(())
}
