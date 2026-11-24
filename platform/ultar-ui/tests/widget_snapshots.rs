//! Reference images of every widget in every state, in Arabic and English, dark and light.
//! Rewrite them with `ULTAR_UPDATE_SNAPSHOTS=1 cargo test --test widget_snapshots`.

mod common;

use common::sheet::{Sheet, State, ACTION_STATES};
use common::snapshots;
use std::collections::BTreeMap;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::snapshot::render;
use ultar_ui::text::{Overflow, Weight};
use ultar_ui::tokens::{ColorRole, Space, ThemeKind, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Card, Icon, IconButton, Label, Row, Select, Slider, Switch, Text,
    TextField,
};
use ultar_ui::{Lang, Size};

const SMALL: Size = Size::new(220.0, 64.0);
const WIDE: Size = Size::new(320.0, 88.0);

fn buttons(sheet: &mut Sheet) {
    for (name, variant) in [
        ("button-primary", ButtonVariant::Primary),
        ("button-plain", ButtonVariant::Plain),
        ("button-quiet", ButtonVariant::Quiet),
        ("button-danger", ButtonVariant::Danger),
    ] {
        let label = if variant == ButtonVariant::Danger {
            "demo.delete"
        } else {
            "demo.save"
        };
        sheet.states(name, SMALL, &ACTION_STATES, &|ui| {
            let root = ui.root();
            ui.add(root, Button::new(Label::key(label)).variant(variant))
        });
    }
    sheet.shoot(
        "button-with-icon-normal",
        SMALL,
        &|ui| {
            let root = ui.root();
            ui.add(
                root,
                Button::new(Label::key("demo.detect")).icon(IconId::Search),
            )
        },
        State::Normal,
    );
    sheet.states("iconbutton", SMALL, &ACTION_STATES, &|ui| {
        let root = ui.root();
        ui.add(
            root,
            IconButton::new(IconId::Gear, Label::key("demo.settings")),
        )
    });
}

fn switches_and_sliders(sheet: &mut Sheet) {
    for (name, on) in [("switch-off", false), ("switch-on", true)] {
        sheet.states(name, SMALL, &ACTION_STATES, &|ui| {
            let root = ui.root();
            ui.add(root, Switch::new(Label::key("demo.night"), on))
        });
    }
    let states = [
        State::Normal,
        State::Hovered,
        State::Pressed,
        State::Focused,
        State::Disabled,
    ];
    sheet.states("slider", Size::new(260.0, 64.0), &states, &|ui| {
        let root = ui.root();
        ui.add(
            root,
            Slider::new(Label::key("demo.brightness"), 0.0, 100.0, 5.0).value(30.0),
        )
    });
}

fn selects(sheet: &mut Sheet) {
    let states = [
        State::Normal,
        State::Hovered,
        State::Pressed,
        State::Focused,
        State::Disabled,
        State::Loading,
        State::Error,
        State::Open,
    ];
    sheet.states("select", Size::new(260.0, 190.0), &states, &|ui| {
        let root = ui.root();
        ui.add(root, common::orientation_select())
    });
    sheet.shoot(
        "select-data-normal",
        SMALL,
        &|ui| {
            let root = ui.root();
            let options = vec![
                Label::key_with("demo.display.recommended", &[("value", "3840 × 2160")]),
                Label::data("60 Hz"),
            ];
            ui.add(
                root,
                Select::new(Label::key("demo.display.resolution"), options),
            )
        },
        State::Normal,
    );
}

fn text_fields(sheet: &mut Sheet) {
    let states = [
        State::Normal,
        State::Hovered,
        State::Pressed,
        State::Focused,
        State::Disabled,
        State::Error,
    ];
    sheet.states("textfield", WIDE, &states, &|ui| {
        let root = ui.root();
        ui.add(
            root,
            TextField::new(Label::key("demo.device")).placeholder(Label::key("demo.device.hint")),
        )
    });
    sheet.states(
        "textfield-filled",
        WIDE,
        &[State::Normal, State::Focused, State::Error],
        &|ui| {
            let root = ui.root();
            let value = Label::key("demo.device.value")
                .resolve(ui.translator())
                .into_owned();
            ui.add(root, TextField::new(Label::key("demo.device")).text(&value))
        },
    );
}

fn settings_rows(ui: &mut Ui) -> Result<NodeId, UiError> {
    let root = ui.root();
    let card = ui.add_sized(root, Card::new(), Sizing::width(360.0))?;
    let label = Row::new(Label::key("demo.night")).sublabel(Label::key("demo.night.sub"));
    let row = ui.add(card, label)?;
    ui.add(row, Switch::new(Label::key("demo.night"), true))?;
    let plain = ui.add(card, Row::new(Label::key("demo.display.hdr")).last())?;
    let unavailable = Text::new(Label::key("demo.display.unavailable"))
        .spec(TextSpec::new(TypeSize::S12))
        .color(ColorRole::TextMuted);
    ui.add(plain, unavailable)?;
    Ok(row)
}

fn text_card(ui: &mut Ui, floating: bool) -> Result<NodeId, UiError> {
    let root = ui.root();
    let card = if floating {
        Card::padded().floating()
    } else {
        Card::padded()
    };
    let card = ui.add_sized(root, card, Sizing::width(300.0))?;
    let title = TextSpec::new(TypeSize::S16).weight(Weight::Semibold);
    ui.add(card, Text::new(Label::key("demo.card.title")).spec(title))?;
    ui.add(
        card,
        Text::new(Label::key("demo.card.text")).color(ColorRole::TextMuted),
    )?;
    Ok(card)
}

/// A card with a close button over its top end corner (`Flex::stack`, inset 4).
fn stacked_card(ui: &mut Ui) -> Result<NodeId, UiError> {
    let root = ui.root();
    let stack = Flex::stack().main(ultar_ui::layout::MainAlign::End);
    let stack = ui.add_sized(root, stack, Sizing::width(300.0))?;
    let card = ui.add_sized(stack, Card::padded(), Sizing::grow(1))?;
    let title = TextSpec::new(TypeSize::S16).weight(Weight::Semibold);
    ui.add(card, Text::new(Label::key("demo.card.title")).spec(title))?;
    ui.add(
        card,
        Text::new(Label::key("demo.card.text")).color(ColorRole::TextMuted),
    )?;
    let corner = Flex::row().padding(ultar_ui::layout::Padding::all(Space::S4));
    let corner = ui.add(stack, corner)?;
    ui.add(
        corner,
        IconButton::new(IconId::Close, Label::key("demo.settings")),
    )?;
    Ok(stack)
}

fn rows_cards_and_texts(sheet: &mut Sheet) {
    let states = [State::Normal, State::Disabled];
    sheet.states("row", Size::new(392.0, 150.0), &states, &settings_rows);
    let card = Size::new(360.0, 190.0);
    sheet.shoot(
        "card-normal",
        card,
        &|ui| text_card(ui, false),
        State::Normal,
    );
    sheet.shoot(
        "card-floating",
        card,
        &|ui| text_card(ui, true),
        State::Normal,
    );
    let ellipsis = |ui: &mut Ui| {
        let root = ui.root();
        let spec = TextSpec::new(TypeSize::S14).overflow(Overflow::Ellipsis);
        let text = Text::new(Label::key("demo.card.text")).spec(spec);
        ui.add_sized(root, text, Sizing::width(200.0))
    };
    sheet.shoot(
        "text-ellipsis-normal",
        Size::new(240.0, 60.0),
        &ellipsis,
        State::Normal,
    );
}

fn icons(sheet: &mut Sheet) {
    sheet.shoot(
        "icon-directional-normal",
        SMALL,
        &|ui| {
            let root = ui.root();
            let line = ui.add(root, Flex::row().gap(Space::S16))?;
            for icon in [
                IconId::Forward,
                IconId::Volume,
                IconId::Spark,
                IconId::Check,
                IconId::Gear,
            ] {
                ui.add(line, Icon::new(icon))?;
            }
            ui.add(
                line,
                Icon::new(IconId::Forward)
                    .small()
                    .color(ColorRole::AccentText),
            )
        },
        State::Normal,
    );
    sheet.shoot(
        "icon-set-normal",
        Size::new(368.0, 256.0),
        &|ui| {
            let root = ui.root();
            let mut last = root;
            for chunk in IconId::ALL.chunks(8) {
                let line = ui.add(root, Flex::row().gap(Space::S16))?;
                for icon in chunk {
                    last = ui.add(line, Icon::new(*icon))?;
                }
            }
            Ok(last)
        },
        State::Normal,
    );
}

#[test]
fn every_widget_in_every_state_matches_its_reference() {
    let mut sheet = Sheet {
        set: snapshots(),
        problems: Vec::new(),
        pictures: BTreeMap::new(),
        cases: 0,
    };
    buttons(&mut sheet);
    switches_and_sliders(&mut sheet);
    selects(&mut sheet);
    text_fields(&mut sheet);
    rows_cards_and_texts(&mut sheet);
    icons(&mut sheet);
    let earlier = sheet.cases;
    common::cases3::all(&mut sheet);
    let wave3 = sheet.cases;
    common::cases4::all(&mut sheet);
    let wave4 = sheet.cases;
    // Overlay layout (Flex::stack, shell round 3): a close button inside the top end corner.
    let card = Size::new(360.0, 190.0);
    sheet.shoot("stack-corner-normal", card, &stacked_card, State::Normal);
    assert!(
        sheet.problems.is_empty(),
        "{} problems:\n{}",
        sheet.problems.len(),
        sheet.problems.join("\n")
    );
    assert_eq!(
        earlier, 73,
        "number of widget/state cases of the first delivery"
    );
    assert_eq!(wave3 - earlier, 58, "number of wave-3 cases");
    assert_eq!(wave4 - wave3, 28, "number of wave-4 cases");
    assert_eq!(sheet.cases - wave4, 1, "number of shell round-3 cases");
    // No stale or missing files: every case has four references, plus the composed screen
    // (four) and its touch variant at double scale (one).
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let files = std::fs::read_dir(dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .is_ok_and(|e| e.path().extension().is_some_and(|x| x == "png"))
        })
        .count();
    assert_eq!(files, sheet.cases * 4 + 4 + 1);
}

#[test]
fn directional_icons_differ_between_languages_and_others_do_not() {
    let set = snapshots();
    let picture = |lang: Lang, icon: IconId| {
        let mut ui = set
            .ui(Size::new(40.0, 40.0), lang, ThemeKind::Dark)
            .unwrap();
        let root = ui.root();
        ui.set_root_flex(Flex::column().cross(CrossAlign::Center));
        ui.add(root, Icon::new(icon)).unwrap();
        render(&mut ui).unwrap().to_rgba()
    };
    for icon in IconId::ALL {
        let same = picture(Lang::Ar, *icon) == picture(Lang::En, *icon);
        assert_eq!(same, !icon.mirrored(), "{}", icon.name());
    }
}
