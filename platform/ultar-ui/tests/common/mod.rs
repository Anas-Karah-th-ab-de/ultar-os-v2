//! Shared helpers of the integration tests: demo catalogs and the composed display screen.
#![allow(dead_code)]

use std::path::PathBuf;
use ultar_ui::event::{Event, Key, Modifiers, PointerButton, Response};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{Flex, Padding, Sizing};
use ultar_ui::paint::{PaintEnv, TextSpec};
use ultar_ui::snapshot::Snapshots;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, InputMode, Space, ThemeKind, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Card, Icon, Label, Row, Select, Slider, Switch, Text, TextField, Widget,
};
use ultar_ui::{Lang, Point, Size};

pub const AR: &str = include_str!("../locales/ar.ftl");
pub const EN: &str = include_str!("../locales/en.ftl");
pub const DE: &str = include_str!("../locales/de.ftl");

pub const LANGS: [Lang; 2] = [Lang::Ar, Lang::En];

pub fn translator(lang: Lang) -> ultar_i18n::Translator {
    let mut tr = ultar_i18n::Translator::empty(lang);
    for (l, source) in [(Lang::Ar, AR), (Lang::En, EN), (Lang::De, DE)] {
        tr.add_source(l, source).expect("demo catalog parses");
    }
    tr
}

pub fn ui(lang: Lang, theme: ThemeKind, size: Size) -> Ui {
    Ui::new(
        PaintEnv::new(lang, theme),
        InputMode::Pointer,
        size,
        translator(lang),
    )
    .expect("ui")
}

pub fn snapshots() -> Snapshots {
    Snapshots::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"))
        .catalog(Lang::Ar, AR)
        .catalog(Lang::En, EN)
        .catalog(Lang::De, DE)
}

pub fn key(ui: &mut Ui, key: Key) -> Response {
    ui.handle(Event::Key {
        key,
        modifiers: Modifiers::NONE,
    })
}

pub fn shift_tab(ui: &mut Ui) -> Response {
    ui.handle(Event::Key {
        key: Key::Tab,
        modifiers: Modifiers::SHIFT,
    })
}

pub fn center(ui: &mut Ui, id: NodeId) -> Point {
    ui.layout();
    ui.rect(id).expect("node has a rectangle").center()
}

pub fn press(ui: &mut Ui, pos: Point) -> Response {
    ui.handle(Event::PointerDown {
        pos,
        button: PointerButton::Primary,
    })
}

pub fn release(ui: &mut Ui, pos: Point) -> Response {
    ui.handle(Event::PointerUp {
        pos,
        button: PointerButton::Primary,
    })
}

pub fn click(ui: &mut Ui, id: NodeId) -> Response {
    let pos = center(ui, id);
    ui.handle(Event::PointerMove { pos });
    let mut response = press(ui, pos);
    let up = release(ui, pos);
    response.consumed |= up.consumed;
    response.actions.extend(up.actions);
    response
}

/// Handles of the interactive parts of the display settings screen.
pub struct DisplayScreen {
    pub card: NodeId,
    pub primary: NodeId,
    pub resolution: NodeId,
    pub scale: NodeId,
    pub refresh: NodeId,
    pub orientation: NodeId,
    pub night: NodeId,
    pub brightness: NodeId,
    pub name: NodeId,
    pub detect: NodeId,
    pub revert: NodeId,
    pub apply: NodeId,
}

fn k(key: &str) -> Label {
    Label::key(key)
}

/// A settings row named `key` (with sublabel `key.sub` when `sub`) holding `control`.
fn row_with(
    ui: &mut Ui,
    card: NodeId,
    key: &str,
    sub: bool,
    control: impl Into<Widget>,
) -> Result<NodeId, UiError> {
    let mut row = Row::new(k(key));
    if sub {
        row = row.sublabel(k(&format!("{key}.sub")));
    }
    let row = ui.add(card, row)?;
    ui.add(row, control)
}

fn recommended(value: &str) -> Label {
    Label::key_with("demo.display.recommended", &[("value", value)])
}

fn header(ui: &mut Ui, card: NodeId) -> Result<(), UiError> {
    let flex = Flex::row()
        .padding(Padding::all(Space::S16))
        .gap(Space::S12);
    let header = ui.add(card, flex)?;
    let icon = Icon::new(IconId::Monitor).color(ColorRole::AccentText);
    ui.add(header, icon)?;
    let names = ui.add_sized(header, Flex::column(), Sizing::grow(1))?;
    let heading = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);
    let name = Label::key_with("demo.display.builtin", &[("size", "15.6″")]);
    ui.add(names, Text::new(name).spec(heading))?;
    let args = [("resolution", "3840 × 2160")];
    let connected = Label::key_with("demo.display.connected", &args);
    let small = TextSpec::new(TypeSize::S12);
    let line = Text::new(connected).spec(small).color(ColorRole::TextMuted);
    ui.add(names, line)?;
    Ok(())
}

/// The rows about the display itself: primary, resolution, scale, refresh, orientation.
fn display_rows(ui: &mut Ui, card: NodeId) -> Result<[NodeId; 5], UiError> {
    let on = Switch::new(k("demo.display.primary"), true);
    let primary = row_with(ui, card, "demo.display.primary", true, on)?;
    let options = vec![
        recommended("3840 × 2160"),
        Label::data("2560 × 1440"),
        Label::data("1920 × 1080"),
    ];
    let select = Select::new(k("demo.display.resolution"), options);
    let resolution = row_with(ui, card, "demo.display.resolution", false, select)?;
    let options = vec![
        Label::data("100%"),
        Label::data("150%"),
        recommended("200%"),
    ];
    let select = Select::new(k("demo.display.scale"), options).selected(2);
    let scale = row_with(ui, card, "demo.display.scale", true, select)?;
    let options = vec![Label::data("60 Hz"), Label::data("120 Hz")];
    let select = Select::new(k("demo.display.refresh"), options);
    let refresh = row_with(ui, card, "demo.display.refresh", false, select)?;
    let orientation = row_with(ui, card, "demo.orientation", false, orientation_select())?;
    let unavailable = Text::new(k("demo.display.unavailable"))
        .spec(TextSpec::new(TypeSize::S12))
        .color(ColorRole::TextMuted);
    row_with(ui, card, "demo.display.hdr", true, unavailable)?;
    Ok([primary, resolution, scale, refresh, orientation])
}

/// Night light, brightness and device name.
fn device_rows(ui: &mut Ui, card: NodeId) -> Result<[NodeId; 3], UiError> {
    let off = Switch::new(k("demo.night"), false);
    let night = row_with(ui, card, "demo.night", true, off)?;
    let slider = Slider::new(k("demo.brightness"), 0.0, 100.0, 5.0).value(70.0);
    let brightness = row_with(ui, card, "demo.brightness", false, slider)?;
    let field = TextField::new(k("demo.device")).placeholder(k("demo.device.hint"));
    let row = ui.add(card, Row::new(k("demo.device")).last())?;
    let name = ui.add(row, field)?;
    Ok([night, brightness, name])
}

fn actions(ui: &mut Ui) -> Result<[NodeId; 3], UiError> {
    let root = ui.root();
    let actions = ui.add(root, Flex::row().gap(Space::S8))?;
    let quiet = Button::new(k("demo.detect"))
        .variant(ButtonVariant::Quiet)
        .icon(IconId::Search);
    let detect = ui.add(actions, quiet)?;
    ui.add_sized(actions, Flex::row(), Sizing::grow(1))?;
    let revert = ui.add(actions, Button::new(k("demo.revert")))?;
    let primary = Button::new(k("demo.apply")).variant(ButtonVariant::Primary);
    let apply = ui.add(actions, primary)?;
    Ok([detect, revert, apply])
}

/// The "display settings card" screen modelled on the design artboard SettingsDisplay:
/// a title, the card of the selected display with its rows, and the action buttons.
pub fn display_settings(ui: &mut Ui) -> Result<DisplayScreen, UiError> {
    let padding = Padding::symmetric(Space::S32, Space::S24);
    ui.set_root_flex(Flex::column().padding(padding).gap(Space::S16));
    let root = ui.root();
    let title = TextSpec::new(TypeSize::S24).weight(Weight::Bold);
    ui.add(root, Text::new(k("demo.display.title")).spec(title))?;
    let card = ui.add(root, Card::new())?;
    header(ui, card)?;
    let [primary, resolution, scale, refresh, orientation] = display_rows(ui, card)?;
    let [night, brightness, name] = device_rows(ui, card)?;
    let [detect, revert, apply] = actions(ui)?;
    Ok(DisplayScreen {
        card,
        primary,
        resolution,
        scale,
        refresh,
        orientation,
        night,
        brightness,
        name,
        detect,
        revert,
        apply,
    })
}

pub fn orientation_select() -> Select {
    Select::new(
        Label::key("demo.orientation"),
        vec![
            Label::key("demo.orientation.landscape"),
            Label::key("demo.orientation.portrait"),
            Label::key("demo.orientation.flipped"),
        ],
    )
}
