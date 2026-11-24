//! Bricks added for the phone boards (PhoneDial, PhoneInCall): compact choices
//! (`RadioLayout::Choices`), the action button inside a text field (`TextField::end_action`),
//! pill tabs (`Tabs::pills`) and tile / toggle buttons (`Button::tile`, `Button::toggled`).
//! Behaviour, sizes and colours against the boards, plus snapshots (ar/en × dark/light) in
//! `tests/snapshots/phone-bricks/`.

mod common;

use common::{click, translator};
use std::path::PathBuf;
use ultar_ui::event::{ActionKind, Event, PointerButton};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Sizing};
use ultar_ui::paint::PaintEnv;
use ultar_ui::snapshot::{render, Snapshots};
use ultar_ui::tokens::{ColorRole, InputMode, Space, ThemeKind, DARK};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Label, RadioGroup, RadioLayout, Tabs, TextDirection, TextField,
};
use ultar_ui::{Lang, Size};

const SIZE: Size = Size { w: 400.0, h: 360.0 };

fn ui(lang: Lang) -> Ui {
    Ui::new(
        PaintEnv::new(lang, ThemeKind::Dark),
        InputMode::Pointer,
        SIZE,
        translator(lang),
    )
    .unwrap()
}

struct Nodes {
    way: NodeId,
    field: NodeId,
    tabs: NodeId,
    mute: NodeId,
    video: NodeId,
    hangup: NodeId,
}

fn sheet(ui: &mut Ui) -> Result<Nodes, UiError> {
    let root = ui.root();
    ui.set_root_flex(Flex::column().gap(Space::S12).cross(CrossAlign::Start));
    let way = RadioGroup::new(
        Label::data("way"),
        vec![Label::data("Ultar"), Label::data("SIM")],
    )
    .layout(RadioLayout::Choices)
    .detail(0, Label::data("encrypted"))
    .detail(1, Label::data("SIM 1"))
    .selected(0);
    let way = ui.add_sized(root, way, Sizing::width(351.0))?;
    let field = TextField::new(Label::data("number"))
        .direction(TextDirection::Ltr)
        .text("UOS-4821-77")
        .end_action(IconId::Backspace, Label::data("delete"))
        .fill_height();
    let field = ui.add_sized(
        root,
        field,
        Sizing::width(351.0).with_height(Space::S48.points()),
    )?;
    let tabs = Tabs::new(
        Label::data("filter"),
        vec![Label::data("All"), Label::data("Missed")],
    )
    .pills();
    let row = ui.add(root, Flex::row())?;
    let tabs = ui.add(row, tabs)?;
    let bar = ui.add(root, Flex::row().gap(Space::S8))?;
    let tile = |label: &str, icon: IconId| Button::new(Label::data(label)).icon(icon).tile();
    let mute = ui.add_sized(
        bar,
        tile("Mute", IconId::Mic).toggled(false),
        Sizing::width(104.0),
    )?;
    let video = ui.add_sized(
        bar,
        tile("Video", IconId::Video).toggled(true),
        Sizing::width(104.0),
    )?;
    let end = tile("End", IconId::Hangup).variant(ButtonVariant::Danger);
    let hangup = ui.add_sized(bar, end, Sizing::width(104.0))?;
    Ok(Nodes {
        way,
        field,
        tabs,
        mute,
        video,
        hangup,
    })
}

fn rgb(c: ultar_ui::Color) -> [u8; 3] {
    [c.r, c.g, c.b]
}

#[test]
fn choices_are_compact_and_border_box_like_the_board() {
    let mut ui = ui(Lang::Ar);
    let n = sheet(&mut ui).unwrap();
    ui.layout();
    let parts: Vec<_> = ui
        .a11y_tree()
        .into_iter()
        .filter(|a| a.id == n.way && a.part.is_some())
        .map(|a| a.rect)
        .collect();
    assert_eq!(parts.len(), 2);
    // Board PhoneDial: 170.5 (chosen, two borders) and 168.5, 53.58 high, 12 apart.
    assert!(
        (parts[0].w - 170.5).abs() <= 0.5 && (parts[1].w - 168.5).abs() <= 0.5,
        "{parts:?}"
    );
    assert!((parts[0].h - 53.58).abs() <= 1.0, "{parts:?}");
    assert!(
        parts[0].x > parts[1].x,
        "Arabic: first choice at the start (right)"
    );
    let canvas = render(&mut ui).unwrap();
    let fill = canvas
        .pixel((parts[0].right() - 5.0) as i32, parts[0].center().y as i32)
        .unwrap();
    assert_eq!(rgb(fill), rgb(DARK.get(ColorRole::Hover)));
    let other = canvas
        .pixel((parts[1].right() - 5.0) as i32, parts[1].center().y as i32)
        .unwrap();
    assert_eq!(
        rgb(other),
        rgb(DARK.get(ColorRole::Ground)),
        "not chosen: transparent"
    );
    click(&mut ui, n.way);
}

#[test]
fn end_action_sits_inside_the_field_and_activates_it() {
    let mut ui = ui(Lang::Ar);
    let n = sheet(&mut ui).unwrap();
    ui.layout();
    let rect = ui.rect(n.field).unwrap();
    assert_eq!((rect.w, rect.h), (351.0, 48.0));
    let button = ui.end_action_rect(n.field).unwrap().unwrap();
    assert_eq!((button.w, button.h), (32.0, 32.0));
    assert!(
        button.x < rect.center().x,
        "Arabic: the end edge is on the left"
    );
    assert!(rect.contains(button.center()));
    let pos = button.center();
    ui.handle(Event::PointerDown {
        pos,
        button: PointerButton::Primary,
    });
    let r = ui.handle(Event::PointerUp {
        pos,
        button: PointerButton::Primary,
    });
    assert!(r
        .actions
        .iter()
        .any(|a| a.node == n.field && a.kind == ActionKind::Activated));
    assert_eq!(
        ui.text(n.field).unwrap(),
        "UOS-4821-77",
        "the field itself does not edit"
    );
    let tr = ui.translator().clone();
    let named = ui
        .a11y_tree()
        .into_iter()
        .any(|a| a.id == n.field && a.part == Some(2) && a.describe(&tr).contains("delete"));
    assert!(named, "the button is announced with its name");
    let plain = TextField::new(Label::data("x"));
    let id = ui.add(ui.root(), plain).unwrap();
    assert_eq!(ui.end_action_rect(id).unwrap(), None);
}

#[test]
fn pill_tabs_are_label_plus_24_filled_and_without_rule() {
    let mut ui = ui(Lang::En);
    let n = sheet(&mut ui).unwrap();
    ui.layout();
    let tabs: Vec<_> = ui
        .a11y_tree()
        .into_iter()
        .filter(|a| a.id == n.tabs && a.part.is_some())
        .map(|a| a.rect)
        .collect();
    assert_eq!(tabs.len(), 2);
    assert!(
        (tabs[1].x - tabs[0].right() - Space::S4.points()).abs() < 0.01,
        "{tabs:?}"
    );
    let canvas = render(&mut ui).unwrap();
    let chosen = canvas
        .pixel(tabs[0].x as i32 + 5, tabs[0].center().y as i32)
        .unwrap();
    assert_eq!(rgb(chosen), rgb(DARK.get(ColorRole::Hover)));
    let bottom = canvas
        .pixel(tabs[0].center().x as i32, tabs[0].bottom() as i32 - 1)
        .unwrap();
    assert_eq!(
        rgb(bottom),
        rgb(DARK.get(ColorRole::Hover)),
        "chosen pill: no underline"
    );
    let bottom = canvas
        .pixel(tabs[1].center().x as i32, tabs[1].bottom() as i32 - 1)
        .unwrap();
    assert_eq!(
        rgb(bottom),
        rgb(DARK.get(ColorRole::Ground)),
        "no rule, no underline"
    );
    click(&mut ui, n.tabs);
}

#[test]
fn tiles_are_even_and_toggles_announce_pressed() {
    let mut ui = ui(Lang::Ar);
    let n = sheet(&mut ui).unwrap();
    ui.layout();
    let (m, v, h) = (
        ui.rect(n.mute).unwrap(),
        ui.rect(n.video).unwrap(),
        ui.rect(n.hangup).unwrap(),
    );
    assert_eq!(m.w, 104.0);
    assert_eq!(m.h, v.h);
    assert_eq!(m.h, h.h);
    assert!((m.h - 67.19).abs() <= 1.0, "board PhoneInCall: {}", m.h);
    let tree = ui.a11y_tree();
    let pressed = |id: NodeId| {
        tree.iter()
            .any(|a| a.id == id && a.part.is_none() && a.state.pressed)
    };
    assert!(pressed(n.video) && !pressed(n.mute));
    let canvas = render(&mut ui).unwrap();
    let edge = canvas.pixel(v.center().x as i32, v.y as i32).unwrap();
    assert_eq!(rgb(edge), rgb(DARK.get(ColorRole::AccentText)));
    let off = canvas.pixel(m.center().x as i32, m.y as i32).unwrap();
    assert_eq!(rgb(off), rgb(DARK.get(ColorRole::ControlBorder)));
    let danger = canvas.pixel(h.center().x as i32, h.y as i32).unwrap();
    assert_eq!(rgb(danger), rgb(DARK.get(ColorRole::Danger)));
    let r = click(&mut ui, n.mute);
    assert!(r
        .actions
        .iter()
        .any(|a| a.node == n.mute && a.kind == ActionKind::Activated));
}

#[test]
fn snapshots_of_the_phone_bricks() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/phone-bricks");
    let set = Snapshots::new(dir)
        .catalog(Lang::Ar, common::AR)
        .catalog(Lang::En, common::EN);
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            set.assert_snapshot(
                "phone-bricks",
                |ui| sheet(ui).map(|_| ()),
                SIZE,
                lang,
                theme,
            )
            .unwrap();
        }
    }
}
