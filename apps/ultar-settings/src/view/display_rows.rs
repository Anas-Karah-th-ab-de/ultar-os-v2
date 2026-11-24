//! The rows of the selected display's card (resolution … brightness).

use super::display::{
    orientation_choices, profile_choices, refresh_choices, resolution_choices, scale_choices,
    Choices,
};
use super::{k, metrics, Cx};
use crate::action::Act;
use crate::state::display::DisplayEntry;
use ultar_ui::layout::{CrossAlign, Flex, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Label, Row, Select, Slider, Switch};

pub(super) fn choice_row(
    cx: &mut Cx<'_>,
    card: NodeId,
    key: &str,
    sub: Option<&str>,
    choices: Choices,
    act: Act,
) -> Result<NodeId, UiError> {
    let mut row = Row::new(k(key));
    if let Some(sub) = sub {
        row = row.sublabel(k(sub));
    }
    let row = cx.ui.add(card, row)?;
    let empty = choices.labels.is_empty();
    let select = Select::new(k(key), choices.labels)
        .selected(choices.selected)
        .fit();
    let id = cx.act(row, select, act)?;
    if empty {
        cx.ui.set_disabled(id, true)?;
        cx.ui
            .set_tooltip(id, Some(k("app.settings.display.no_modes")))?;
    }
    Ok(id)
}

/// Adds the per-display rows to the card.
pub fn rows(cx: &mut Cx<'_>, card: NodeId, d: &DisplayEntry) -> Result<(), UiError> {
    choice_row(
        cx,
        card,
        "app.settings.display.resolution",
        None,
        resolution_choices(d),
        Act::DisplayResolution,
    )?;
    choice_row(
        cx,
        card,
        "app.settings.display.scale",
        Some("app.settings.display.scale.sub"),
        scale_choices(d),
        Act::DisplayScale,
    )?;
    choice_row(
        cx,
        card,
        "app.settings.display.refresh",
        None,
        refresh_choices(d),
        Act::DisplayRefresh,
    )?;
    choice_row(
        cx,
        card,
        "app.settings.display.orientation",
        None,
        orientation_choices(d),
        Act::DisplayOrientation,
    )?;
    hdr_row(cx, card, d)?;
    choice_row(
        cx,
        card,
        "app.settings.display.profile",
        None,
        profile_choices(d),
        Act::DisplayProfile,
    )?;
    if !d.caps.brightness {
        return Ok(());
    }
    brightness_row(cx, card, d, true)
}

pub(super) fn hdr_row(cx: &mut Cx<'_>, card: NodeId, d: &DisplayEntry) -> Result<(), UiError> {
    let mut row = Row::new(k("app.settings.display.hdr"));
    if !d.caps.hdr {
        row = row.sublabel(k("app.settings.display.hdr.unsupported"));
    }
    let row = cx.ui.add(card, row)?;
    let switch = Switch::new(k("app.settings.display.hdr"), d.hdr && d.caps.hdr);
    let id = cx.act(row, switch, Act::DisplayHdr)?;
    if !d.caps.hdr {
        cx.ui.set_disabled(id, true)?;
    }
    Ok(())
}

pub(super) fn brightness_row(
    cx: &mut Cx<'_>,
    card: NodeId,
    d: &DisplayEntry,
    last: bool,
) -> Result<(), UiError> {
    let mut row = Row::new(k("app.settings.display.brightness"));
    if last {
        row = row.last();
    }
    let row = cx.ui.add(card, row)?;
    let pair = cx.ui.add_sized(
        row,
        Flex::row().gap(Space::S12).cross(CrossAlign::Center),
        Sizing::width(if super::shell::compact(cx.ui) {
            cx.ui.size().w / 2.0
        } else {
            metrics::brightness_width()
        }),
    )?;
    let name = if d.internal {
        k("app.settings.display.brightness_builtin")
    } else {
        let name = super::display::short_name(d).resolve(cx.tr).into_owned();
        Label::key_with("app.settings.display.brightness_of", &[("name", &name)])
    };
    let slider = Slider::new(name, 0.0, 100.0, 1.0).value(d.brightness as f32);
    cx.act_sized(pair, slider, Sizing::grow(1), Act::DisplayBrightness)?;
    Ok(())
}
