//! The helper's sheet (boards HelperSheet, HelperConfirm): where it thinks, the last
//! requests with every step – area, what it was used on, how it went –, the answer, and
//! the field to ask. A step that waits for the person says so; the question itself is the
//! system's dialog. Requests, details and the model's words are data.

use super::calendar::label_of;
use super::geometry::{self, Placement};
use super::{Look, Target, View};
use crate::model::helper::{Helper, Turn};
use crate::model::Desktop;
use ultar_bus::messages::agent::{Area, Brain, Step, StepOutcome};
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Badge, Button, ButtonVariant, Icon, IconButton, InfoBar, Label, SearchField, Text, Tone,
};

/// Sheet width (`g(128)`).
pub const WIDTH: f32 = 128.0 * Space::S4.points();
/// Requests shown, newest last (older ones are in Settings › Assistant).
pub const SHOWN: usize = 2;

/// Icon of an area.
pub fn area_icon(area: Area) -> IconId {
    match area {
        Area::Help => IconId::Info,
        Area::Apps => IconId::Apps,
        Area::Sound => IconId::Volume,
        Area::Network => IconId::Wifi,
        Area::Screen => IconId::Shot,
        Area::Settings => IconId::Gear,
        Area::Notifications => IconId::Bell,
        Area::Storage => IconId::Disk,
        Area::Devices => IconId::Mesh,
        Area::Power => IconId::Power,
    }
}

/// Word and tone of how a step went.
pub fn outcome(outcome: StepOutcome) -> (&'static str, Tone) {
    match outcome {
        StepOutcome::Done => ("desk.helper.step.done", Tone::Success),
        StepOutcome::Failed => ("desk.helper.step.failed", Tone::Danger),
        StepOutcome::NotConfirmed => ("desk.helper.step.unconfirmed", Tone::Warning),
        StepOutcome::NotAllowed => ("desk.helper.step.refused", Tone::Danger),
    }
}

/// Where the helper thinks, as the badge of the head.
pub fn brain(brain: &Brain) -> (&'static str, Tone) {
    match brain {
        Brain::Local => ("desk.helper.brain.local", Tone::Success),
        Brain::Mesh(_) => ("desk.helper.brain.mesh", Tone::Success),
        Brain::Cloud => ("desk.helper.brain.cloud", Tone::Warning),
        Brain::Minimal => ("desk.helper.brain.minimal", Tone::Neutral),
    }
}

fn muted(label: Label) -> Text {
    Text::new(label)
        .spec(TextSpec::new(TypeSize::S12))
        .color(ColorRole::TextMuted)
}

fn head(view: &mut View, card: NodeId, helper: &Helper) -> Result<(), UiError> {
    let row = Flex::row().cross(CrossAlign::Center).gap(Space::S8);
    let row = view.ui.add(card, row)?;
    view.ui.add(row, Icon::new(IconId::Spark))?;
    let title = Text::new(Label::key("desk.helper.title"))
        .spec(TextSpec::new(TypeSize::S16).weight(Weight::Semibold));
    view.ui.add_sized(row, title, Sizing::grow(1))?;
    if let Some(now) = &helper.brain {
        let (key, tone) = brain(now);
        view.ui.add(row, Badge::new(Label::key(key)).tone(tone))?;
    }
    let gear = IconButton::new(IconId::Gear, Label::key("desk.helper.reach"));
    let gear = view.ui.add(row, gear)?;
    view.mark(gear, Target::HelperSettings);
    Ok(())
}

fn step(view: &mut View, list: NodeId, step: &Step) -> Result<(), UiError> {
    let row = Flex::row().cross(CrossAlign::Center).gap(Space::S8);
    let row = view.ui.add(list, row)?;
    let icon = Icon::new(area_icon(step.area)).small();
    view.ui.add(row, icon.color(ColorRole::TextMuted))?;
    let area = format!("desk.helper.area.{}", step.area.id());
    let area =
        Text::new(Label::key(area)).spec(TextSpec::new(TypeSize::S14).weight(Weight::Semibold));
    view.ui.add(row, area)?;
    let detail: String = step.detail.chars().take(60).collect();
    view.ui
        .add_sized(row, muted(Label::data(&detail)), Sizing::grow(1))?;
    let (key, tone) = outcome(step.outcome);
    view.ui.add(row, Badge::new(Label::key(key)).tone(tone))?;
    Ok(())
}

fn turn(view: &mut View, card: NodeId, turn: &Turn, tr: &Translator) -> Result<(), UiError> {
    let asked: String = turn.asked.chars().take(200).collect();
    let asked =
        Text::new(Label::data(&asked)).spec(TextSpec::new(TypeSize::S14).weight(Weight::Semibold));
    view.ui.add(card, asked)?;
    if !turn.steps.is_empty() {
        let list = view.ui.add(card, Flex::column().gap(Space::S4))?;
        for s in &turn.steps {
            step(view, list, s)?;
        }
    }
    match (&turn.said, &turn.waiting) {
        (Some(said), _) => {
            view.ui.add(card, Text::new(label_of(said, tr)))?;
        }
        (None, Some(tool)) => {
            // what waits is named in the person's language where the helper has a text
            let key = format!("agent.action.{tool}");
            let what = if tr.has(&key) {
                Label::key(key)
            } else {
                Label::key("desk.helper.waiting.text")
            };
            let bar = InfoBar::new(Tone::Warning, Label::key("desk.helper.waiting")).message(what);
            view.ui.add(card, bar)?;
        }
        (None, None) => {
            view.ui
                .add(card, muted(Label::key("desk.helper.thinking")))?;
        }
    }
    Ok(())
}

/// Builds the sheet; returns the view and where its surface goes.
pub fn build(
    d: &Desktop,
    look: Look,
    screen: Size,
    tr: &Translator,
) -> Result<(View, Placement), UiError> {
    let helper = &d.overview.helper;
    super::card_sheet(look, screen, WIDTH, tr, geometry::sheet, |view, card| {
        head(view, card, helper)?;
        if helper.turns.is_empty() {
            view.ui.add(card, muted(Label::key("desk.helper.empty")))?;
        }
        let from = helper.turns.len().saturating_sub(SHOWN);
        for t in &helper.turns[from..] {
            turn(view, card, t, tr)?;
        }
        let row = Flex::row().cross(CrossAlign::Center).gap(Space::S8);
        let row = view.ui.add(card, row)?;
        let field = SearchField::new(Label::key("desk.helper.field"))
            .placeholder(Label::key("desk.helper.field.hint"))
            .text(&helper.query);
        let field = view.ui.add_sized(row, field, Sizing::grow(1))?;
        view.mark(field, Target::SearchField);
        if helper.busy() {
            let stop = view
                .ui
                .add(row, Button::new(Label::key("desk.helper.stop")))?;
            view.mark(stop, Target::HelperStop);
        } else {
            let send = Button::new(Label::key("desk.helper.send")).variant(ButtonVariant::Primary);
            let send = view.ui.add(row, send)?;
            view.mark(send, Target::HelperSend);
        }
        Ok(())
    })
}
