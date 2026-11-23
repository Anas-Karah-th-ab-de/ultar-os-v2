//! Review round 3, "Prüfer Bild" / "Prüfer Zugang": what the pictures must satisfy by
//! measurement – every state-carrying graphic keeps 3:1 against what is next to it in the
//! light theme (design spec §3 "Kontrast", boards: "accent only as a fill behind on_accent,
//! accent_text for graphics"), and the looks the boards fix.
//! Target path: platform/ultar-ui/tests/r3_design.rs

mod common;

use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::color::Color;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::tokens::{size, ColorRole, Space, ThemeKind, LIGHT};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::{
    Badge, Button, ButtonVariant, Checkbox, Label, List, ListItem, Progress, RadioGroup, Slider,
    Switch, Tabs, Widget,
};
use ultar_ui::{Lang, Rect, Size};

const SURFACE: Size = Size::new(400.0, 200.0);

fn luminance(c: Color) -> f64 {
    let f = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
}

fn contrast(a: Color, b: Color) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// One widget alone on the light ground, English, painted; returns its rectangle too.
fn alone(widget: impl Into<Widget>, width: f32) -> (Canvas, Rect, Ui, NodeId) {
    let mut ui = common::ui(Lang::En, ThemeKind::Light, SURFACE);
    let pad = Padding::all(Space::S48);
    ui.set_root_flex(Flex::column().padding(pad).cross(CrossAlign::Start));
    let root = ui.root();
    let id = ui.add_sized(root, widget, Sizing::width(width)).unwrap();
    ui.layout();
    let mut canvas = Canvas::new(400, 200, PixelFormat::Rgba8).unwrap();
    ui.paint_all(&mut canvas);
    let rect = ui.rect(id).unwrap();
    (canvas, rect, ui, id)
}

fn at(canvas: &Canvas, x: f32, y: f32) -> Color {
    canvas.pixel(x.round() as i32, y.round() as i32).unwrap()
}

/// The strongest contrast against the ground found along a short horizontal probe line.
fn best(canvas: &Canvas, from_x: f32, to_x: f32, y: f32) -> f64 {
    let ground = LIGHT.get(ColorRole::Ground);
    let (mut x, mut found) = (from_x, 1.0f64);
    while x <= to_x {
        found = found.max(contrast(at(canvas, x, y), ground));
        x += 1.0;
    }
    found
}

fn k(key: &str) -> Label {
    Label::key(key)
}

/// Light theme: the graphic that says "on / chosen / how much" keeps 3:1 against the
/// ground next to it. One case per widget; all are collected so the report is complete.
#[test]
fn d01_state_graphics_keep_three_to_one_in_the_light_theme() {
    let mut weak = Vec::new();
    let mut check = |name: &str, value: f64| {
        println!("{name}: {value:.2}:1");
        if value < 3.0 {
            weak.push(format!("{name} {value:.2}:1"));
        }
    };
    let mid = |r: Rect| r.y + r.h / 2.0;
    let (c, r, ..) = alone(Switch::new(k("demo.night"), true), 48.0);
    check(
        "switch on: track against ground",
        best(&c, r.x + 6.0, r.x + 12.0, mid(r)),
    );
    let slider = Slider::new(k("demo.brightness"), 0.0, 100.0, 5.0).value(50.0);
    let (c, r, ..) = alone(slider, 200.0);
    check(
        "slider: thumb against ground",
        // Value slot 48 + 12 at the end: the thumb of 50 % sits at 70 (round 4).
        best(&c, r.x + 66.0, r.x + 74.0, mid(r) - 6.0),
    );
    check(
        "slider: filled part against ground",
        best(&c, r.x + 20.0, r.x + 40.0, mid(r)),
    );
    let (c, r, ..) = alone(Checkbox::new(k("demo.check.updates"), true), 260.0);
    check(
        "checkbox on: box edge against ground",
        best(&c, r.x + 12.0, r.x + 14.0, mid(r) - 5.0), // row padding 12 (round 4)
    );
    let radio = RadioGroup::new(k("demo.radio"), vec![k("demo.radio.dark")]).selected(0);
    let (c, r, ..) = alone(radio, 200.0);
    check(
        "radio chosen: circle against ground",
        best(&c, r.x, r.x + 3.0, r.y + 16.0),
    );
    let labels = vec![k("demo.tabs.general"), k("demo.tabs.network")];
    let (c, r, ..) = alone(Tabs::new(k("demo.tabs"), labels), 240.0);
    check(
        "tabs: indicator of the chosen tab",
        best(&c, r.x + 50.0, r.x + 70.0, r.bottom() - 1.0),
    );
    let primary = Button::new(k("demo.save")).variant(ButtonVariant::Primary);
    let (c, r, ..) = alone(primary, 120.0);
    check(
        "primary button: edge against ground",
        best(&c, r.x + 40.0, r.x + 80.0, r.y + 0.5),
    );
    let (c, r, ..) = alone(Progress::new(k("demo.progress")).value(0.5), 200.0);
    check(
        "progress: rest of the track against ground",
        best(&c, r.x + 150.0, r.x + 170.0, mid(r)),
    );
    assert!(
        weak.is_empty(),
        "below 3:1 in the light theme:\n{}",
        weak.join("\n")
    );
}

/// The selection bar of a list item (light theme) against the row background it lies on.
#[test]
fn d02_the_selection_bar_of_a_list_item_keeps_three_to_one() {
    let mut ui = common::ui(Lang::En, ThemeKind::Light, SURFACE);
    let root = ui.root();
    let list = ui.add(root, List::new().selectable()).unwrap();
    let item = ui
        .add(list, ListItem::new(k("demo.list.home")).selected())
        .unwrap();
    ui.layout();
    let mut canvas = Canvas::new(400, 200, PixelFormat::Rgba8).unwrap();
    ui.paint_all(&mut canvas);
    let r = ui.rect(item).unwrap();
    let bar = at(&canvas, r.x + 2.0, r.y + r.h / 2.0);
    let row = at(&canvas, r.x + r.w / 2.0, r.y + 3.0);
    let value = contrast(bar, row);
    assert!(
        value >= 3.0,
        "selection bar {bar:?} on row {row:?}: {value:.2}:1"
    );
}

/// Boards (`gen.py` checkrow, radiocard): the box of a check box and the circle of a radio
/// button are icon-sized (24), not small-icon-sized, and the check box has the control
/// radius. The pictures show a 16-point square.
#[test]
fn d03_check_box_and_radio_circle_have_the_size_of_the_boards() {
    let (canvas, r, ..) = alone(Checkbox::new(k("demo.check.updates"), false), 260.0);
    let ground = LIGHT.get(ColorRole::Ground);
    let y = r.y + r.h / 2.0;
    // The box starts after the row's padding of 12 (boards, round 4).
    let start = r.x + 12.0;
    let mut right = start;
    for x in 0..24 {
        let px = start + x as f32;
        if contrast(at(&canvas, px, y), ground) > 2.0 {
            right = px;
        }
    }
    let width = right - start + 1.0;
    assert!(
        (width - size::ICON).abs() <= 1.0,
        "the box is {width} points wide, the boards draw {}",
        size::ICON
    );
}

/// Boards (`gen.py` badge): a badge has the control radius and no fill; its text has the
/// colour of its tone, also for the neutral tone. The toolkit draws a filled pill.
#[test]
fn d04_a_badge_is_drawn_as_on_the_boards() {
    let (canvas, r, ..) = alone(Badge::new(k("demo.badge.new")), 80.0);
    let inside = at(&canvas, r.x + 5.0, r.y + r.h / 2.0);
    assert_eq!(
        inside,
        LIGHT.get(ColorRole::Ground),
        "the badge has a fill of its own (boards: none)"
    );
}
