//! Review round 5 (second reviewer): the frame that opens a dialog dims the pixels already
//! in the canvas (wave 5). Whatever the previous frame showed (tooltip, menu, options list,
//! toast, hover, focus ring, a held press) and whatever changed in the same frame, the
//! picture must equal the first frame of a `Ui` that never painted before.
//! Target path: platform/ultar-ui/tests/w5_frame.rs

mod common;

use common::gallery::file_menu;
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{Event, Key, Modifiers, PointerButton};
use ultar_ui::layout::Flex;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::{Button, Dialog, Label, Select, Switch, Text, Toast, Tone};
use ultar_ui::{Lang, Size};

const SIZE: Size = Size::new(420.0, 320.0);

struct Scene {
    text: NodeId,
    tip: NodeId,
    menu: NodeId,
    select: NodeId,
    switch: NodeId,
    dialog: NodeId,
}

fn scene(ui: &mut Ui) -> Scene {
    ui.set_root_flex(Flex::column());
    let root = ui.root();
    let text = ui.add(root, Text::new(Label::data("title"))).unwrap();
    let tip = ui.add(root, Button::new(Label::data("tip"))).unwrap();
    ui.set_tooltip(tip, Some(Label::key("demo.tip.settings")))
        .unwrap();
    let menu = ui
        .add(root, Button::new(Label::key("demo.menu.button")))
        .unwrap();
    let options = ["a", "b", "c"].map(Label::data).to_vec();
    let select = ui
        .add(root, Select::new(Label::data("s"), options))
        .unwrap();
    let switch = ui.add(root, Switch::new(Label::data("w"), false)).unwrap();
    let dialog = ui
        .add(root, Dialog::new(Label::key("demo.dialog.title")))
        .unwrap();
    ui.add(dialog, Button::new(Label::key("demo.cancel")))
        .unwrap();
    Scene {
        text,
        tip,
        menu,
        select,
        switch,
        dialog,
    }
}

fn at(ui: &mut Ui, id: NodeId) -> ultar_ui::Point {
    common::center(ui, id)
}

type Step = fn(&mut Ui, &Scene);

/// States the frame before the dialog shows, and changes made in the frame that opens it.
fn befores() -> Vec<(&'static str, Step)> {
    vec![
        ("tooltip", |ui, s| {
            let pos = at(ui, s.tip);
            ui.handle(Event::PointerMove { pos });
            ui.advance_to(5_000);
        }),
        ("menu", |ui, s| ui.open_menu(s.menu, file_menu()).unwrap()),
        ("options list", |ui, s| {
            let pos = at(ui, s.select);
            let button = PointerButton::Primary;
            ui.handle(Event::PointerDown { pos, button });
            ui.handle(Event::PointerUp { pos, button });
        }),
        ("toast", |ui, _| {
            ui.show_toast(Toast::new(7, Tone::Info, Label::data("saved")))
                .unwrap()
        }),
        ("hover", |ui, s| {
            let pos = at(ui, s.menu);
            ui.handle(Event::PointerMove { pos });
        }),
        ("focus ring", |ui, _| {
            ui.handle(Event::Key {
                key: Key::Tab,
                modifiers: Modifiers::NONE,
            });
        }),
        ("held press", |ui, s| {
            let pos = at(ui, s.menu);
            let button = PointerButton::Primary;
            ui.handle(Event::PointerDown { pos, button });
        }),
        ("scale 1.25", |ui, _| ui.set_scale(1.25).unwrap()),
    ]
}

fn sames() -> Vec<(&'static str, Step)> {
    vec![
        ("nothing", |_, _| {}),
        ("label", |ui, s| {
            ui.set_label(s.text, Label::data("a longer title")).unwrap()
        }),
        ("switch", |ui, s| ui.set_on(s.switch, true).unwrap()),
        ("toast", |ui, _| {
            ui.show_toast(Toast::new(8, Tone::Success, Label::data("done")))
                .unwrap()
        }),
    ]
}

fn canvas(ui: &Ui) -> Canvas {
    let s = ui.env().scale;
    let (w, h) = ((SIZE.w * s).ceil() as u32, (SIZE.h * s).ceil() as u32);
    Canvas::new(w, h, PixelFormat::Bgrx8).unwrap()
}

fn picture(before: Step, same: Step, incremental: bool) -> Canvas {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Light, SIZE);
    let s = scene(&mut ui);
    ui.advance_to(1_000);
    let mut frame = canvas(&ui);
    if incremental {
        ui.paint(&mut frame);
    }
    before(&mut ui, &s);
    if incremental {
        frame = if frame.width() == canvas(&ui).width() {
            frame
        } else {
            canvas(&ui)
        };
        ui.paint(&mut frame);
    }
    same(&mut ui, &s);
    ui.open_dialog(s.dialog).unwrap();
    if !incremental {
        frame = canvas(&ui);
    }
    ui.paint(&mut frame);
    frame
}

#[test]
fn fr01_the_frame_that_opens_a_dialog_equals_a_fresh_frame() {
    let mut failures = Vec::new();
    for (b, before) in befores() {
        for (c, same) in sames() {
            let (inc, fresh) = (picture(before, same, true), picture(before, same, false));
            let diff = inc
                .data()
                .chunks(4)
                .zip(fresh.data().chunks(4))
                .filter(|(x, y)| x != y)
                .count();
            if diff > 0 {
                failures.push(format!("{b} + {c}: {diff} px"));
            }
        }
    }
    assert!(failures.is_empty(), "stale pixels: {failures:?}");
}
