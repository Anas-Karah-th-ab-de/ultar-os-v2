//! The quick settings sheet (board QuickSettings): tiles in two columns, volume and
//! brightness, and a footer with the battery and the actions (screenshot, settings, lock,
//! power). A second page lists the three power modes. Unusable things are disabled with
//! their reason as text and tooltip.

use super::geometry::{self, Placement, QUICK_WIDTH};
use super::{Look, Target, View};
use crate::model::quick::{self, Avail, PowerMode, Tile};
use crate::model::services::Services;
use crate::model::{Desktop, QuickPage};
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::tokens::{size, ColorRole, Space};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Brick, BrickKind, Button, ButtonVariant, Icon, IconButton, Label, RadioGroup, Slider, Text,
};

/// Builds the quick settings for a screen; returns the view and where its surface goes.
pub fn build(
    d: &Desktop,
    page: QuickPage,
    look: Look,
    screen: Size,
    tr: &Translator,
) -> Result<(View, Placement), UiError> {
    super::card_sheet(
        look,
        screen,
        QUICK_WIDTH,
        tr,
        geometry::sheet,
        |view, card| match page {
            QuickPage::Tiles => tiles_page(view, card, d),
            QuickPage::PowerModes => modes_page(view, card, d),
            QuickPage::Bluetooth => super::bluetooth::page(view, card, d),
            QuickPage::Wifi => super::wifi::page(view, card, d),
            QuickPage::WifiJoin => super::wifi::join(view, card, d),
            QuickPage::Vpn => super::wifi::vpn_page(view, card, d),
        },
    )
}

fn tiles_page(view: &mut View, card: NodeId, d: &Desktop) -> Result<(), UiError> {
    let grid = view.ui.add(card, Flex::column().gap(Space::S8))?;
    let tiles = quick::tiles(&d.presence, &d.values, &d.svc);
    for pair in tiles.chunks(2) {
        let row = view.ui.add(grid, Flex::row().gap(Space::S8))?;
        for tile in pair {
            add_tile(view, row, tile, &d.svc)?;
        }
        if pair.len() == 1 {
            // The grid keeps two columns (board: `grid-template-columns: 1fr 1fr`).
            view.ui.add_sized(row, Flex::row(), Sizing::grow(1))?;
        }
    }
    sliders(view, card, d)?;
    footer(view, card, d)
}

pub(crate) fn add_tile(
    view: &mut View,
    row: NodeId,
    tile: &Tile,
    svc: &Services,
) -> Result<(), UiError> {
    let more = tile.id.has_list();
    let cell = view.ui.add_sized(row, Flex::row(), Sizing::grow(1))?;
    let brick = Brick::new(
        BrickKind::QuickTile {
            icon: tile.id.icon(),
            state: match (tile.avail, quick::tile_data(tile, svc)) {
                (Avail::Disabled(_), _) => Label::key("desk.reason.short"),
                (Avail::Ready, Some(data)) => Label::data(&data),
                (Avail::Ready, None) => Label::key(tile.detail),
            },
            on: tile.on,
            joined: more,
        },
        Label::key(tile.id.name_key()),
    );
    let main = view.ui.add_sized(cell, brick, Sizing::grow(1))?;
    view.mark(main, Target::Tile(tile.id));
    // A tile that cannot switch now stays operable: a click opens its page or its place
    // in Settings, where the reason is shown (its tooltip names it too).
    if let Avail::Disabled(reason) = tile.avail {
        view.ui.set_tooltip(main, Some(Label::key(reason)))?;
    }
    if more {
        let name = Label::key(format!("{}.choose", tile.id.name_key()));
        let part = Brick::new(BrickKind::QuickMore { on: tile.on }, name);
        let id = view.ui.add(cell, part)?;
        view.mark(id, Target::TileMore(tile.id));
        // The arrow always opens the page; the page says what is missing.
        let reason = match tile.avail {
            Avail::Disabled(reason) => Some(reason),
            Avail::Ready => quick::list_reason(tile.id, svc),
        };
        if let Some(reason) = reason {
            view.ui.set_tooltip(id, Some(Label::key(reason)))?;
        }
    }
    Ok(())
}

fn slider_row(
    view: &mut View,
    card: NodeId,
    icon: IconId,
    slider: Slider,
) -> Result<NodeId, UiError> {
    let row = view
        .ui
        .add(card, Flex::row().gap(Space::S12).cross(CrossAlign::Center))?;
    view.ui
        .add(row, Icon::new(icon).color(ColorRole::TextMuted))?;
    view.ui.add_sized(row, slider, Sizing::grow(1))
}

fn sliders(view: &mut View, card: NodeId, d: &Desktop) -> Result<(), UiError> {
    // The real level, or "—" without a device (never a 0 % that is no value).
    let volume = Slider::new(Label::key("desk.qs.volume"), 0.0, 100.0, 5.0);
    let volume = match (&d.svc.audio, volume_reason(d)) {
        (Some(a), None) => volume.value(f32::from(a.volume_percent.min(100))),
        _ => volume.unknown(),
    };
    let id = slider_row(view, card, IconId::Volume, volume)?;
    view.mark(id, Target::Volume);
    if let Some(reason) = volume_reason(d) {
        view.ui.set_disabled(id, true)?;
        view.ui.set_tooltip(id, Some(Label::key(reason)))?;
    }
    if let Some(row) = view.ui.parent(id) {
        let out = IconButton::new(IconId::Speaker, Label::key("desk.qs.output"));
        let out = view.ui.add(row, out)?;
        view.mark(out, Target::Output);
        if let Some(reason) = output_reason(d) {
            view.ui.set_disabled(out, true)?;
            view.ui.set_tooltip(out, Some(Label::key(reason)))?;
        }
    }
    let brightness = Slider::new(Label::key("desk.qs.brightness"), 0.0, 100.0, 5.0);
    let brightness = match d.values.brightness.filter(|_| d.presence.settings) {
        Some(level) => brightness.value(f32::from(level.min(100))),
        None => brightness.unknown(),
    };
    let id = slider_row(view, card, IconId::Sun, brightness)?;
    view.mark(id, Target::Brightness);
    if d.values.brightness.is_none() || !d.presence.settings {
        view.ui.set_disabled(id, true)?;
        view.ui
            .set_tooltip(id, Some(Label::key("desk.reason.no_display_setting")))?;
    }
    if let Some(row) = view.ui.parent(id) {
        let pad = Sizing::width(size::TARGET_POINTER_MIN).with_height(size::TARGET_POINTER_MIN);
        view.ui.add_sized(row, Flex::row(), pad)?;
    }
    Ok(())
}

/// Why the volume cannot be set: no audio service, no device, or a device without a volume.
pub(crate) fn volume_reason(d: &Desktop) -> Option<&'static str> {
    match &d.svc.audio {
        None => Some("desk.reason.no_service"),
        Some(a) if !a.present => Some("desk.reason.no_audio_device"),
        Some(a) if !a.has_volume => Some("desk.reason.no_volume"),
        Some(_) => None,
    }
}

/// Why no output can be chosen: no audio service or no device.
pub(crate) fn output_reason(d: &Desktop) -> Option<&'static str> {
    match &d.svc.audio {
        None => Some("desk.reason.no_service"),
        Some(a) if !a.present => Some("desk.reason.no_audio_device"),
        Some(_) => None,
    }
}

fn footer(view: &mut View, card: NodeId, d: &Desktop) -> Result<(), UiError> {
    view.ui
        .add(card, Brick::new(BrickKind::Rule, Label::data("")))?;
    let flex = Flex::row()
        .main(MainAlign::SpaceBetween)
        .cross(CrossAlign::Center)
        .padding(Padding {
            start: None,
            end: None,
            top: Some(Space::S12),
            bottom: None,
        });
    let row = view.ui.add(card, flex)?;
    match d.values.battery {
        Some((percent, _)) => {
            let label = Label::key_with("desk.qs.battery", &[("percent", &percent.to_string())]);
            let b = Button::new(label)
                .variant(ButtonVariant::Quiet)
                .icon(IconId::Battery);
            let id = view.ui.add(row, b)?;
            view.mark(id, Target::Battery);
        }
        None => {
            view.ui.add(row, Flex::row())?;
        }
    }
    let actions = view.ui.add(row, Flex::row().gap(Space::S4))?;
    if d.overview.sources.helper {
        let spark = IconButton::new(IconId::Spark, Label::key("desk.qs.assistant"));
        let id = view.ui.add(actions, spark)?;
        view.mark(id, Target::Assistant);
    }
    let items = [
        (
            IconId::Shot,
            "desk.qs.shot",
            Target::Screenshot,
            (!d.presence.bus).then_some("desk.reason.no_service"),
        ),
        (
            IconId::Gear,
            "desk.qs.settings",
            Target::Settings,
            (!d.presence.launcher).then_some("desk.reason.no_launcher"),
        ),
        (IconId::Lock, "desk.qs.lock", Target::Lock, None),
        (IconId::Power, "desk.qs.power", Target::Power, None),
    ];
    for (icon, key, target, reason) in items {
        let id = view
            .ui
            .add(actions, IconButton::new(icon, Label::key(key)))?;
        view.mark(id, target);
        if let Some(reason) = reason {
            view.ui.set_disabled(id, true)?;
            view.ui.set_tooltip(id, Some(Label::key(reason)))?;
        }
    }
    Ok(())
}

fn modes_page(view: &mut View, card: NodeId, d: &Desktop) -> Result<(), UiError> {
    let head = view
        .ui
        .add(card, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    let back = view.ui.add(
        head,
        IconButton::new(IconId::Back, Label::key("desk.qs.back")),
    )?;
    view.mark(back, Target::QuickBack);
    view.ui
        .add(head, Text::new(Label::key("desk.qs.power_mode")))?;
    let options = PowerMode::ALL
        .iter()
        .map(|m| Label::key(format!("desk.power.{}", m.word())))
        .collect();
    let current = PowerMode::ALL
        .iter()
        .position(|m| *m == d.values.power)
        .unwrap_or(1);
    let group = RadioGroup::new(Label::key("desk.qs.power_mode"), options).selected(current);
    let id = view.ui.add(card, group)?;
    view.mark(id, Target::PowerModeChoice(d.values.power));
    if !d.presence.settings {
        // The modes are shown; choosing one needs the settings service.
        view.ui
            .add(card, Text::new(Label::key("desk.reason.no_settings")))?;
        view.ui.set_disabled(id, true)?;
        view.ui
            .set_tooltip(id, Some(Label::key("desk.reason.no_settings")))?;
    }
    Ok(())
}
