//! Reference images of every widget in every state, in Arabic and English, dark and light.
//! Rewrite them with `ULTAR_UPDATE_SNAPSHOTS=1 cargo test --test widget_snapshots`.

mod common;

use common::{center, key, snapshots, LANGS};
use std::collections::BTreeMap;
use ultar_ui::event::{Event, Key, PointerButton};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::snapshot::{render, Snapshots};
use ultar_ui::text::{Overflow, Weight};
use ultar_ui::tokens::{ColorRole, Space, ThemeKind, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Card, Icon, IconButton, Label, Row, Select, Slider, Switch, Text,
    TextField,
};
use ultar_ui::{Lang, Size};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Normal,
    Hovered,
    Pressed,
    Focused,
    Disabled,
    Loading,
    Error,
    Open,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            State::Normal => "normal",
            State::Hovered => "hovered",
            State::Pressed => "pressed",
            State::Focused => "focused",
            State::Disabled => "disabled",
            State::Loading => "loading",
            State::Error => "error",
            State::Open => "open",
        }
    }
}

const ACTION_STATES: [State; 6] = [
    State::Normal,
    State::Hovered,
    State::Pressed,
    State::Focused,
    State::Disabled,
    State::Loading,
];

fn apply(ui: &mut Ui, id: NodeId, state: State) -> Result<(), UiError> {
    let pos = center(ui, id);
    match state {
        State::Normal => {}
        State::Hovered => {
            ui.handle(Event::PointerMove { pos });
        }
        State::Pressed => {
            ui.handle(Event::PointerMove { pos });
            ui.handle(Event::PointerDown {
                pos,
                button: PointerButton::Primary,
            });
        }
        State::Focused => {
            while ui.focused() != Some(id) {
                assert!(key(ui, Key::Tab).consumed, "widget is reachable by Tab");
            }
        }
        State::Disabled => ui.set_disabled(id, true)?,
        State::Loading => {
            ui.set_loading(id, true)?;
            ui.advance_to(150);
        }
        State::Error => ui.set_error(id, Some(Label::key("demo.orientation.error")))?,
        State::Open => {
            while ui.focused() != Some(id) {
                key(ui, Key::Tab);
            }
            key(ui, Key::Enter);
            key(ui, Key::ArrowDown);
        }
    }
    Ok(())
}

/// Runs the cases, collects every mismatch and checks that the states of a widget differ.
struct Sheet {
    set: Snapshots,
    problems: Vec<String>,
    /// Rendered pixels per case name (Arabic, dark) to prove that states look different.
    pictures: BTreeMap<String, Vec<u8>>,
    cases: usize,
}

impl Sheet {
    fn shoot(
        &mut self,
        name: &str,
        size: Size,
        build: &dyn Fn(&mut Ui) -> Result<NodeId, UiError>,
        state: State,
    ) {
        self.cases += 1;
        for lang in LANGS {
            for theme in ThemeKind::ALL {
                let result = self.set.ui(size, lang, theme).and_then(|mut ui| {
                    ui.set_root_flex(
                        Flex::column()
                            .padding(Padding::all(Space::S16))
                            .gap(Space::S8)
                            .cross(CrossAlign::Start),
                    );
                    let id = build(&mut ui)?;
                    apply(&mut ui, id, state)?;
                    if (lang, theme) == (Lang::Ar, ThemeKind::Dark) {
                        self.pictures
                            .insert(name.to_string(), render(&mut ui)?.to_rgba());
                    }
                    self.set.assert_ui(name, &mut ui)
                });
                if let Err(e) = result {
                    self.problems
                        .push(format!("{name} {} {}: {e}", lang.code(), theme.name()));
                }
            }
        }
    }

    fn states(
        &mut self,
        widget: &str,
        size: Size,
        states: &[State],
        build: &dyn Fn(&mut Ui) -> Result<NodeId, UiError>,
    ) {
        for state in states {
            self.shoot(&format!("{widget}-{}", state.name()), size, build, *state);
        }
        let own: Vec<(&String, &Vec<u8>)> = self
            .pictures
            .iter()
            .filter(|(k, _)| k.starts_with(&format!("{widget}-")))
            .collect();
        for (i, (a, pa)) in own.iter().enumerate() {
            for (b, pb) in &own[i + 1..] {
                if pa == pb {
                    self.problems.push(format!("{a} and {b} look identical"));
                }
            }
        }
    }
}

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
                IconId::ChevronEnd,
                IconId::Volume,
                IconId::Assistant,
                IconId::Check,
                IconId::Gear,
            ] {
                ui.add(line, Icon::new(icon))?;
            }
            ui.add(
                line,
                Icon::new(IconId::ChevronEnd)
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
    assert!(
        sheet.problems.is_empty(),
        "{} problems:\n{}",
        sheet.problems.len(),
        sheet.problems.join("\n")
    );
    assert_eq!(sheet.cases, 73, "number of widget/state cases");
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
