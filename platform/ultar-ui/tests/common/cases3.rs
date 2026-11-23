//! Snapshot cases of the wave-3 widgets: every widget in every state it has.

use super::gallery::{delete_dialog, file_menu, networks, share_sheet};
use super::sheet::{Sheet, State};
use super::{center, key};
use ultar_ui::event::{Event, Key, PointerButton};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{Flex, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    Badge, Button, Card, Checkbox, EmptyState, IconButton, InfoBar, Label, ListItem, Progress,
    RadioGroup, Scroll, SearchField, Tabs, Text, TextDirection, TextField, Tone,
};
use ultar_ui::Size;

fn k(key: &str) -> Label {
    Label::key(key)
}

const CHOICE_STATES: [State; 5] = [
    State::Normal,
    State::Hovered,
    State::Pressed,
    State::Focused,
    State::Disabled,
];

fn choices(sheet: &mut Sheet) {
    let small = Size::new(300.0, 64.0);
    for (name, checked) in [("checkbox-off", false), ("checkbox-on", true)] {
        sheet.states(name, small, &CHOICE_STATES, &|ui| {
            let root = ui.root();
            ui.add(root, Checkbox::new(k("demo.check.updates"), checked))
        });
    }
    let radio = |selected: Option<usize>| {
        move |ui: &mut Ui| {
            let root = ui.root();
            let options = vec![
                k("demo.radio.dark"),
                k("demo.radio.light"),
                k("demo.radio.auto"),
            ];
            let group = RadioGroup::new(k("demo.radio"), options);
            ui.add(root, selected.map_or(group.clone(), |i| group.selected(i)))
        }
    };
    let size = Size::new(240.0, 130.0);
    sheet.states("radio", size, &CHOICE_STATES, &radio(Some(1)));
    sheet.shoot("radio-unset-normal", size, &radio(None), State::Normal);
    sheet.states("tabs", Size::new(400.0, 64.0), &CHOICE_STATES, &|ui| {
        let root = ui.root();
        let labels = vec![
            k("demo.tabs.general"),
            k("demo.tabs.network"),
            k("demo.tabs.about"),
        ];
        ui.add(root, Tabs::new(k("demo.tabs"), labels).selected(1))
    });
}

fn lists(sheet: &mut Sheet) {
    let size = Size::new(392.0, 210.0);
    let build = |pick: usize| {
        move |ui: &mut Ui| {
            let root = ui.root();
            let card = ui.add_sized(root, Card::new(), Sizing::width(360.0))?;
            let (_, items, _) = networks(ui, card)?;
            Ok(items[pick])
        }
    };
    // The third item (plain) carries the states; the first shows "selected".
    sheet.states("listitem", size, &CHOICE_STATES, &build(2));
    sheet.shoot(
        "listitem-with-control-focused",
        size,
        &build(1),
        State::Focused,
    );
}

fn badges(ui: &mut Ui) -> Result<NodeId, UiError> {
    let root = ui.root();
    let plain = ui.add(root, Flex::row().gap(Space::S8))?;
    let icons = ui.add(root, Flex::row().gap(Space::S8))?;
    let texts = [
        (Tone::Neutral, "demo.badge.beta"),
        (Tone::Accent, "demo.badge.new"),
        (Tone::Info, "demo.badge.online"),
        (Tone::Success, "demo.badge.synced"),
        (Tone::Warning, "demo.badge.low"),
        (Tone::Danger, "demo.badge.failed"),
    ];
    let mut last = root;
    for (tone, text) in texts.iter().take(3) {
        last = ui.add(plain, Badge::new(k(text)).tone(*tone))?;
    }
    for (tone, text) in texts.iter().skip(2) {
        last = ui.add(icons, Badge::new(k(text)).tone(*tone).with_icon())?;
    }
    Ok(last)
}

fn progress_bars(sheet: &mut Sheet) {
    let size = Size::new(300.0, 48.0);
    let progress = |value: Option<f32>, clock: u64| {
        move |ui: &mut Ui| {
            let root = ui.root();
            let bar = Progress::new(k("demo.progress"));
            let bar = value.map_or(bar.clone(), |v| bar.value(v));
            let id = ui.add_sized(root, bar, Sizing::width(260.0))?;
            ui.advance_to(clock);
            Ok(id)
        }
    };
    let normal = State::Normal;
    let known = progress(Some(0.4), 0);
    sheet.shoot("progress-determinate-normal", size, &known, normal);
    let full = progress(Some(1.0), 0);
    sheet.shoot("progress-full-normal", size, &full, normal);
    let busy = progress(None, 300);
    sheet.shoot("progress-indeterminate-normal", size, &busy, normal);
}

fn info_bar(ui: &mut Ui, tone: Tone) -> Result<NodeId, UiError> {
    let root = ui.root();
    let title = k(&format!("demo.bar.{}", tone.name()));
    let mut bar = InfoBar::new(tone, title);
    if tone != Tone::Success {
        bar = bar.message(k(&format!("demo.bar.{}.msg", tone.name())));
    }
    let bar = ui.add(root, bar)?;
    match tone {
        Tone::Info => ui.add(bar, Button::new(k("demo.bar.action")))?,
        Tone::Danger => ui.add(bar, IconButton::new(IconId::Close, k("demo.close")))?,
        _ => bar,
    };
    Ok(bar)
}

fn status(sheet: &mut Sheet) {
    let normal = State::Normal;
    sheet.shoot(
        "badge-tones-normal",
        Size::new(420.0, 104.0),
        &badges,
        normal,
    );
    progress_bars(sheet);
    for tone in [Tone::Info, Tone::Success, Tone::Warning, Tone::Danger] {
        let name = format!("infobar-{}-normal", tone.name());
        let size = Size::new(480.0, 112.0);
        sheet.shoot(&name, size, &|ui| info_bar(ui, tone), normal);
    }
    let empty = |action: bool| {
        move |ui: &mut Ui| {
            let root = ui.root();
            let state = EmptyState::new(IconId::Folder, k("demo.empty.title"));
            let id = ui.add(root, state.message(k("demo.empty.msg")))?;
            if action {
                ui.add(id, Button::new(k("demo.empty.action")))?;
            }
            Ok(id)
        }
    };
    let with_action = empty(true);
    let size = Size::new(400.0, 280.0);
    sheet.shoot("emptystate-action-normal", size, &with_action, normal);
    let plain = empty(false);
    let size = Size::new(400.0, 230.0);
    sheet.shoot("emptystate-plain-normal", size, &plain, normal);
}

fn fields(sheet: &mut Sheet) {
    let wide = Size::new(320.0, 64.0);
    let states = [
        State::Normal,
        State::Hovered,
        State::Pressed,
        State::Focused,
        State::Disabled,
    ];
    sheet.states("searchfield", wide, &states, &|ui| {
        let root = ui.root();
        let field = SearchField::new(k("demo.search")).placeholder(k("demo.search.hint"));
        ui.add(root, field)
    });
    sheet.states(
        "searchfield-filled",
        wide,
        &[State::Normal, State::Focused],
        &|ui| {
            let root = ui.root();
            let value = k("demo.search.value").resolve(ui.translator()).into_owned();
            ui.add(root, SearchField::new(k("demo.search")).text(&value))
        },
    );
    // A phone number: left-to-right content in both languages (review finding 19).
    sheet.shoot(
        "textfield-ltr-focused",
        wide,
        &|ui| {
            let root = ui.root();
            let field = TextField::new(k("demo.phone")).direction(TextDirection::Ltr);
            ui.add(root, field.text("+963 11 123 4567"))
        },
        State::Focused,
    );
}

fn scroller(ui: &mut Ui) -> Result<NodeId, UiError> {
    let root = ui.root();
    let card = ui.add_sized(root, Card::new(), Sizing::width(320.0))?;
    let sizing = Sizing::width(320.0).with_height(168.0);
    let scroll = ui.add_sized(card, Scroll::new().name(k("demo.scroll")), sizing)?;
    for n in 1..=8 {
        let label = Label::key_count("demo.scroll.item", "n", n);
        ui.add(scroll, ListItem::new(label).separator())?;
    }
    Ok(scroll)
}

fn scrolling_and_cards(sheet: &mut Sheet) {
    let states = [
        State::Normal,
        State::Scrolled,
        State::Hovered,
        State::Focused,
        State::Disabled,
    ];
    sheet.states("scroll", Size::new(352.0, 200.0), &states, &scroller);
    sheet.shoot(
        "card-raised-normal",
        Size::new(380.0, 230.0),
        &|ui| {
            let root = ui.root();
            let card = ui.add_sized(root, Card::padded().raised(), Sizing::width(300.0))?;
            let title = ultar_ui::paint::TextSpec::new(ultar_ui::tokens::TypeSize::S16)
                .weight(ultar_ui::text::Weight::Semibold);
            ui.add(card, Text::new(k("demo.confirm.title")).spec(title))?;
            ui.add(card, Text::new(k("demo.confirm.text")))?;
            let actions = ui.add(card, Flex::row().gap(Space::S8))?;
            ui.add(actions, Button::new(k("demo.revert")))?;
            let keep =
                Button::new(k("demo.apply")).variant(ultar_ui::widgets::ButtonVariant::Primary);
            ui.add(actions, keep)?;
            Ok(card)
        },
        State::Normal,
    );
}

fn floating(sheet: &mut Sheet) {
    sheet.shoot(
        "tooltip-shown-normal",
        Size::new(220.0, 110.0),
        &|ui| {
            let root = ui.root();
            let id = ui.add(root, IconButton::new(IconId::Gear, k("demo.settings")))?;
            ui.set_tooltip(id, Some(k("demo.tip.settings")))?;
            key(ui, Key::Tab);
            Ok(id)
        },
        State::Normal,
    );
    let menu = |submenu: bool| {
        move |ui: &mut Ui| {
            let root = ui.root();
            let button = ui.add(root, Button::new(k("demo.menu.button")))?;
            key(ui, Key::Tab);
            ui.open_menu(button, file_menu())?;
            if submenu {
                for step in [Key::ArrowDown, Key::ArrowDown, Key::Enter] {
                    key(ui, step);
                }
            }
            Ok(button)
        }
    };
    let size = Size::new(520.0, 290.0);
    sheet.shoot("menu-open-normal", size, &menu(false), State::Normal);
    sheet.shoot("menu-submenu-normal", size, &menu(true), State::Normal);
    sheet.shoot(
        "contextmenu-open-normal",
        Size::new(392.0, 300.0),
        &|ui| {
            let root = ui.root();
            let card = ui.add_sized(root, Card::new(), Sizing::width(360.0))?;
            let (list, items, _) = networks(ui, card)?;
            ui.set_context_menu(list, Some(file_menu()))?;
            let pos = center(ui, items[0]);
            ui.handle(Event::PointerDown {
                pos,
                button: PointerButton::Secondary,
            });
            Ok(list)
        },
        State::Normal,
    );
}

fn modals(sheet: &mut Sheet) {
    let behind = |ui: &mut Ui| {
        let root = ui.root();
        ui.add(root, Button::new(k("demo.menu.button")))?;
        ui.add(root, Text::new(k("demo.card.text")))
    };
    sheet.shoot(
        "dialog-open-normal",
        Size::new(560.0, 320.0),
        &|ui| {
            behind(ui)?;
            let (dialog, _, _) = delete_dialog(ui)?;
            key(ui, Key::Tab);
            ui.open_dialog(dialog)?;
            Ok(dialog)
        },
        State::Normal,
    );
    for (name, size) in [
        ("sheet-compact-normal", Size::new(360.0, 400.0)),
        ("sheet-side-normal", Size::new(720.0, 300.0)),
    ] {
        sheet.shoot(
            name,
            size,
            &|ui| {
                behind(ui)?;
                let (id, _) = share_sheet(ui)?;
                ui.open_dialog(id)?;
                Ok(id)
            },
            State::Normal,
        );
    }
}

/// Runs all wave-3 cases.
pub fn all(sheet: &mut Sheet) {
    choices(sheet);
    lists(sheet);
    status(sheet);
    fields(sheet);
    scrolling_and_cards(sheet);
    floating(sheet);
    modals(sheet);
}
