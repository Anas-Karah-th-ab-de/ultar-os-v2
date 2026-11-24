//! Pages without a board of their own: a `settingsd` panel as rows (switches, choices, sliders
//! and text fields), an area no service offers, and Displays > Night light.

use super::{k, muted, title, Cx};
use crate::action::Act;
use crate::areas::AREAS;
use crate::i18n::label;
use ultar_bus::messages::settings::{ItemInfo, ItemKind, SettingValue};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{Flex, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Card, EmptyState, IconButton, InfoBar, Label, Row, Select, Slider, Switch, Text, TextDirection,
    TextField, Tone,
};

/// Most steps a whole-number setting may have to be shown as a slider.
pub const MAX_SLIDER_STEPS: i64 = 200;

/// The control a generic row gets.
#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    /// Switch.
    Switch(bool),
    /// Choice: options and the selected one.
    Choice(Vec<String>, usize),
    /// Slider: min, max, step, value.
    Slider(f32, f32, f32, f32),
    /// A text field (texts, times, paths, lists as "a, b", numbers with many steps); Enter
    /// stores it.
    Edit(String),
    /// Shown, not editable here (fractional numbers).
    Value(String),
}

/// Longest text typed into a generic row (bytes; settingsd checks the real limits).
pub const MAX_EDIT: usize = 1024;

/// Separator of the entries of a list setting in its text field.
pub const LIST_SEPARATOR: char = ',';

/// Which control an item gets.
pub fn control_of(item: &ItemInfo) -> Control {
    match (&item.kind, &item.value) {
        (ItemKind::Toggle, SettingValue::Flag(on)) => Control::Switch(*on),
        (ItemKind::Choice { choices }, SettingValue::Text(v)) => {
            let ids: Vec<String> = choices.iter().map(|c| c.id.clone()).collect();
            let index = ids.iter().position(|id| id == v).unwrap_or(0);
            Control::Choice(ids, index)
        }
        (ItemKind::Integer { min, max, step }, SettingValue::Integer(v))
            if *step > 0 && (max - min) / step <= MAX_SLIDER_STEPS =>
        {
            Control::Slider(*min as f32, *max as f32, *step as f32, *v as f32)
        }
        (ItemKind::Integer { .. }, SettingValue::Integer(v)) => Control::Edit(v.to_string()),
        (ItemKind::Text { .. }, SettingValue::Text(v)) => Control::Edit(v.clone()),
        (ItemKind::List { .. }, SettingValue::List(l)) => Control::Edit(l.join(", ")),
        (_, v) => Control::Value(value_text(v)),
    }
}

/// A value as text.
pub fn value_text(v: &SettingValue) -> String {
    match v {
        SettingValue::Flag(b) => b.to_string(),
        SettingValue::Integer(i) => i.to_string(),
        SettingValue::Decimal(f) => f.to_string(),
        SettingValue::Text(t) => t.clone(),
        SettingValue::List(l) => l.join(", "),
    }
}

/// A `settingsd` panel as rows.
pub fn generic(cx: &mut Cx<'_>, content: NodeId, id: &str) -> Result<(), UiError> {
    let area = AREAS.get(cx.state.area).map_or("", |a| a.id);
    title(cx, content, k(&format!("app.settings.area.{area}")))?;
    let Some(panel) = cx.state.panel(id).cloned() else {
        let error = cx
            .state
            .panels_error
            .clone()
            .unwrap_or_else(|| ultar_bus::Text::key("app.settings.panel.empty"));
        let bar = InfoBar::new(Tone::Warning, k("app.settings.panel.unavailable"))
            .message(label(&error, cx.tr));
        cx.ui.add(content, bar)?;
        return Ok(());
    };
    let card = cx.ui.add(content, Card::new())?;
    let shown: Vec<(usize, &ItemInfo)> = panel
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| !i.template)
        .collect();
    let count = shown.len();
    for (n, (index, item)) in shown.into_iter().enumerate() {
        let mut row = Row::new(label(&item.label, cx.tr)).sublabel(label(&item.description, cx.tr));
        if n + 1 == count {
            row = row.last();
        }
        let row = cx.ui.add(card, row)?;
        let id = control(cx, row, index, item)?;
        if let (Some(id), false) = (id, item.writable) {
            cx.ui.set_disabled(id, true)?;
            cx.ui
                .set_tooltip(id, Some(k("app.settings.panel.read_only")))?;
        }
    }
    Ok(())
}

/// The control at the end of a generic row (`None`: a value shown as text).
fn control(
    cx: &mut Cx<'_>,
    row: NodeId,
    index: usize,
    item: &ItemInfo,
) -> Result<Option<NodeId>, UiError> {
    let name = label(&item.label, cx.tr);
    let act = Act::PanelItem(index);
    let wide = Sizing::width(super::metrics::brightness_width());
    let id = match control_of(item) {
        Control::Switch(on) => cx.act(row, Switch::new(name, on), act)?,
        Control::Choice(_, selected) => {
            let ItemKind::Choice { choices } = &item.kind else {
                return Ok(None);
            };
            let options = choices.iter().map(|c| label(&c.label, cx.tr)).collect();
            cx.act(row, Select::new(name, options).selected(selected), act)?
        }
        Control::Slider(min, max, step, value) => {
            let slider = Slider::new(name, min, max, step).value(value);
            cx.act_sized(row, slider, wide, act)?
        }
        Control::Edit(text) => {
            let field = TextField::new(name).text(&text).max_len(MAX_EDIT);
            // Free text may be Arabic; times, paths, hosts, names and numbers read left to
            // right.
            let free = matches!(&item.kind, ItemKind::Text { format } if format == "text");
            let field = if free {
                field
            } else {
                field.direction(TextDirection::Ltr)
            };
            cx.act_sized(row, field, wide, act)?
        }
        Control::Value(text) => {
            cx.ui.add(row, Text::new(Label::data(&text)))?;
            return Ok(None);
        }
    };
    Ok(Some(id))
}

/// An area no service offers settings for in this version.
pub fn no_service(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let area = AREAS.get(cx.state.area).copied().unwrap_or(AREAS[0]);
    title(cx, content, k(&area.label_key()))?;
    let card = cx.ui.add(content, Card::padded())?;
    let empty = EmptyState::new(area.icon, k("app.settings.no_service"))
        .message(k("app.settings.no_service.sub"));
    cx.ui.add(card, empty)?;
    Ok(())
}

/// Displays > Night light: schedule and warmth.
pub fn night_light(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let row = cx.ui.add(content, Flex::row().gap(Space::S8))?;
    let back = IconButton::new(IconId::Back, k("app.settings.night.back"));
    cx.act(row, back, Act::NightBack)?;
    cx.ui.add(
        row,
        Text::new(k("app.settings.display.night")).spec(super::spec::page_title()),
    )?;
    let st = &cx.state.display;
    let card = cx.ui.add(content, Card::new())?;
    let schedules = ["sunset_to_sunrise", "custom", "always"];
    let options = schedules
        .iter()
        .map(|s| k(&format!("settings.display.night_light.schedule.choice.{s}")))
        .collect();
    let index = schedules
        .iter()
        .position(|s| *s == st.night_schedule)
        .unwrap_or(0);
    let r = cx
        .ui
        .add(card, Row::new(k("app.settings.night.schedule")))?;
    let select = Select::new(k("app.settings.night.schedule"), options).selected(index);
    cx.act(r, select, Act::NightSchedule)?;
    let r = cx.ui.add(
        card,
        Row::new(k("app.settings.night.warmth"))
            .sublabel(k("app.settings.night.warmth.sub"))
            .last(),
    )?;
    let warm = Slider::new(k("app.settings.night.warmth"), 1700.0, 6500.0, 100.0)
        .value(st.night_temperature as f32);
    let sizing = Sizing::width(super::metrics::brightness_width());
    cx.act_sized(r, warm, sizing, Act::NightWarmth)?;
    muted(cx.ui, content, k("app.settings.night.note"))?;
    Ok(())
}
