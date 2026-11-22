//! Pointer, touch and scroll input; damage tracking and partial repaint; animation clock.

mod common;

use common::{center, click, display_settings, press, release};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{Action, ActionKind, Event, Key, PointerButton};
use ultar_ui::tokens::{motion, InputMode, Space, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::{Button, Label, Select, Slider, TextField};
use ultar_ui::{Lang, Point, Size};

const SURFACE: Size = Size::new(720.0, 760.0);

fn kinds(actions: &[Action]) -> Vec<ActionKind> {
    actions.iter().map(|a| a.kind).collect()
}

fn canvas_for(ui: &Ui, format: PixelFormat) -> Canvas {
    Canvas::new(ui.size().w as u32, ui.size().h as u32, format).unwrap()
}

#[test]
fn hover_press_and_click_drive_the_button_states() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let s = display_settings(&mut ui).unwrap();
    let pos = center(&mut ui, s.apply);
    assert_eq!(ui.visual(s.apply), Default::default());
    ui.handle(Event::PointerMove { pos });
    assert!(ui.visual(s.apply).hovered);
    assert_eq!(ui.hovered(), Some(s.apply));
    assert!(press(&mut ui, pos).consumed);
    let v = ui.visual(s.apply);
    assert!(
        v.pressed && v.hovered && !v.focused,
        "pointer focus does not show the ring"
    );
    assert_eq!(ui.focused(), Some(s.apply));
    let up = release(&mut ui, pos);
    assert_eq!(
        up.actions,
        vec![Action {
            node: s.apply,
            kind: ActionKind::Activated
        }]
    );
    assert!(!ui.visual(s.apply).pressed);
    ui.handle(Event::PointerLeave);
    assert!(!ui.visual(s.apply).hovered);
    // Using the keyboard afterwards shows the ring on the widget that has focus.
    common::key(&mut ui, Key::Enter);
    assert!(ui.visual(s.apply).focused);
}

#[test]
fn releasing_outside_cancels_and_other_buttons_are_ignored() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, SURFACE);
    let s = display_settings(&mut ui).unwrap();
    let pos = center(&mut ui, s.apply);
    press(&mut ui, pos);
    let away = Point::new(5.0, 5.0);
    assert!(
        ui.handle(Event::PointerMove { pos: away }).consumed,
        "the press captures the pointer"
    );
    let up = release(&mut ui, away);
    assert!(up.consumed && up.actions.is_empty());
    let secondary = ui.handle(Event::PointerDown {
        pos,
        button: PointerButton::Secondary,
    });
    assert!(!secondary.consumed && !ui.visual(s.apply).pressed);
    assert!(
        !ui.handle(Event::PointerUp {
            pos,
            button: PointerButton::Middle
        })
        .consumed
    );
    assert!(
        !press(&mut ui, away).consumed,
        "empty space is not consumed"
    );
    assert!(!release(&mut ui, away).consumed);
    // Disabled widgets take neither hover nor press.
    ui.set_disabled(s.apply, true).unwrap();
    ui.handle(Event::PointerMove { pos });
    assert!(!press(&mut ui, pos).consumed);
    let v = ui.visual(s.apply);
    assert!(v.disabled && !v.hovered && !v.pressed);
    // Disabling a card disables everything inside it.
    ui.set_disabled(s.card, true).unwrap();
    assert!(ui.visual(s.primary).disabled && ui.is_disabled(s.name));
    assert!(click(&mut ui, s.primary).actions.is_empty());
}

#[test]
fn switch_toggles_on_click_and_animates_with_the_clock() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let s = display_settings(&mut ui).unwrap();
    ui.advance_to(1_000);
    assert!(!ui.animating());
    assert_eq!(
        kinds(&click(&mut ui, s.night).actions),
        vec![ActionKind::Toggled(true)]
    );
    assert!(ui.is_on(s.night).unwrap());
    assert!(ui.animating(), "the knob is in transit");
    let mut frames = Vec::new();
    let mut canvas = canvas_for(&ui, PixelFormat::Rgba8);
    for step in 0..=4u64 {
        ui.advance_to(1_000 + step * u64::from(motion::FEEDBACK) / 4);
        ui.paint_all(&mut canvas);
        frames.push(canvas.to_rgba());
    }
    assert!(
        frames.windows(2).take(3).all(|w| w[0] != w[1]),
        "each animation frame differs"
    );
    assert!(
        !ui.animating(),
        "the animation ends after the feedback duration"
    );
    ui.advance_to(5_000);
    assert!(!ui.needs_paint(), "nothing to repaint once it has ended");
    // Reduce motion: the knob jumps.
    ui.set_reduce_motion(true);
    click(&mut ui, s.night);
    assert!(!ui.animating());
    // A loading widget keeps the frame clock running (progress ring).
    ui.set_reduce_motion(false);
    ui.set_loading(s.apply, true).unwrap();
    assert!(ui.animating());
    ui.paint(&mut canvas);
    ui.advance_to(5_100);
    assert!(ui.needs_paint());
    assert!(
        click(&mut ui, s.apply).actions.is_empty(),
        "loading buttons do not activate"
    );
    ui.set_loading(s.apply, false).unwrap();
    assert!(!ui.animating());
}

#[test]
fn slider_follows_the_pointer_in_both_directions() {
    for lang in common::LANGS {
        let mut ui = common::ui(lang, ThemeKind::Dark, Size::new(400.0, 100.0));
        let root = ui.root();
        let slider = ui
            .add(
                root,
                Slider::new(Label::key("demo.brightness"), 0.0, 100.0, 1.0),
            )
            .unwrap();
        ui.layout();
        let rect = ui.rect(slider).unwrap();
        let y = rect.center().y;
        let start_x = if lang == Lang::Ar {
            rect.right() - 1.0
        } else {
            rect.x + 1.0
        };
        let end_x = if lang == Lang::Ar {
            rect.x + 1.0
        } else {
            rect.right() - 1.0
        };
        let down = press(&mut ui, Point::new(rect.center().x, y));
        assert_eq!(
            kinds(&down.actions),
            vec![ActionKind::ValueChanged(50.0)],
            "{lang:?}"
        );
        assert!(ui.visual(slider).pressed);
        let drag = ui.handle(Event::PointerMove {
            pos: Point::new(end_x, y),
        });
        assert_eq!(kinds(&drag.actions), vec![ActionKind::ValueChanged(100.0)]);
        // Dragging far outside still tracks; the same position twice emits nothing.
        let far = ui.handle(Event::PointerMove {
            pos: Point::new(start_x - 500.0 * (end_x - start_x).signum(), y + 300.0),
        });
        assert_eq!(kinds(&far.actions), vec![ActionKind::ValueChanged(0.0)]);
        assert!(ui
            .handle(Event::PointerMove {
                pos: Point::new(start_x, y)
            })
            .actions
            .is_empty());
        release(&mut ui, Point::new(start_x, y));
        assert!(
            ui.handle(Event::PointerMove {
                pos: Point::new(end_x, y)
            })
            .actions
            .is_empty(),
            "no drag after release"
        );
        assert_eq!(ui.value(slider).unwrap(), 0.0);
    }
}

/// A select with ten options above a button, and a function giving the centre of the
/// n-th visible popup row.
fn long_select(ui: &mut Ui) -> (NodeId, NodeId, impl Fn(f32) -> Point) {
    let root = ui.root();
    let options: Vec<Label> = (1..=10).map(|i| Label::data(&format!("{i}0 Hz"))).collect();
    let select = Select::new(Label::key("demo.display.refresh"), options);
    let select = ui.add(root, select).unwrap();
    let below = ui.add(root, Button::new(Label::key("demo.save"))).unwrap();
    ui.layout();
    let field = ui.rect(select).unwrap();
    let row = InputMode::Pointer.min_target();
    let first = field.bottom() + Space::S8.points();
    let option = move |n: f32| Point::new(field.center().x, first + row * (n + 0.5));
    (select, below, option)
}

fn scroll(ui: &mut Ui, pos: Point, lines: f32) -> bool {
    ui.handle(Event::Scroll { pos, lines }).consumed
}

fn pick(ui: &mut Ui, pos: Point) -> Vec<ActionKind> {
    press(ui, pos);
    kinds(&release(ui, pos).actions)
}

#[test]
fn select_popup_is_modal_and_chooses_on_click() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(400.0, 500.0));
    let (select, below, option) = long_select(&mut ui);
    assert_eq!(
        kinds(&click(&mut ui, select).actions),
        vec![ActionKind::Opened]
    );
    assert!(ui.visual(select).pressed, "an open select looks pressed");
    // The popup covers the button below: clicks go to the option, not the button.
    assert!(ui.rect(below).unwrap().contains(option(0.0)));
    ui.handle(Event::PointerMove { pos: option(2.0) });
    assert_eq!(ui.hovered(), None, "nothing under the popup is hovered");
    let chosen = vec![ActionKind::Selected(2), ActionKind::Closed];
    assert_eq!(pick(&mut ui, option(2.0)), chosen);
    assert_eq!(ui.selected(select).unwrap(), 2);
    // A click outside closes the list without choosing and without activating anything.
    click(&mut ui, select);
    let corner = Point::new(5.0, 495.0);
    let outside = press(&mut ui, corner);
    assert!(outside.consumed);
    assert_eq!(kinds(&outside.actions), vec![ActionKind::Closed]);
    release(&mut ui, corner);
    assert_eq!(ui.selected(select).unwrap(), 2);
    // Clicking the field again while open closes it.
    click(&mut ui, select);
    let pos = center(&mut ui, select);
    press(&mut ui, pos);
    assert!(!ui.is_open(select));
    release(&mut ui, pos);
    // Programmatic changes validate the index.
    assert!(ui.set_selected(select, 10).is_err());
    ui.set_selected(select, 0).unwrap();
    assert_eq!(ui.selected(select).unwrap(), 0);
}

#[test]
fn select_popup_scrolls_long_lists() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(400.0, 500.0));
    let (select, _, option) = long_select(&mut ui);
    // Scrolling moves the visible window; the first visible row is then a later option.
    click(&mut ui, select);
    assert!(scroll(&mut ui, option(1.0), 3.0));
    let chosen = vec![ActionKind::Selected(3), ActionKind::Closed];
    assert_eq!(pick(&mut ui, option(0.0)), chosen);
    click(&mut ui, select);
    scroll(&mut ui, option(1.0), 100.0);
    let chosen = vec![ActionKind::Selected(9), ActionKind::Closed];
    assert_eq!(
        pick(&mut ui, option(5.0)),
        chosen,
        "scrolling stops at the last option"
    );
    click(&mut ui, select);
    scroll(&mut ui, option(1.0), -100.0);
    assert!(
        !scroll(&mut ui, Point::new(1.0, 499.0), 1.0),
        "outside the popup"
    );
    assert!(!scroll(&mut ui, option(1.0), f32::NAN));
    let chosen = vec![ActionKind::Selected(0), ActionKind::Closed];
    assert_eq!(
        pick(&mut ui, option(0.0)),
        chosen,
        "scrolled back to the top"
    );
    assert!(!scroll(&mut ui, option(1.0), 1.0), "no popup, no scrolling");
    // The keyboard highlight scrolls the list too.
    click(&mut ui, select);
    for _ in 0..8 {
        common::key(&mut ui, Key::ArrowDown);
    }
    let chosen = vec![ActionKind::Selected(8), ActionKind::Closed];
    assert_eq!(
        pick(&mut ui, option(5.0)),
        chosen,
        "the highlighted option is the last visible"
    );
}

#[test]
fn popup_opens_upwards_at_the_bottom_edge() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(300.0, 200.0));
    ui.set_root_flex(ultar_ui::layout::Flex::column().main(ultar_ui::layout::MainAlign::End));
    let root = ui.root();
    let select = ui.add(root, common::orientation_select()).unwrap();
    click(&mut ui, select);
    let field = ui.rect(select).unwrap();
    let row = InputMode::Pointer.min_target();
    let last = Point::new(field.center().x, field.y - Space::S8.points() - row * 0.5);
    press(&mut ui, last);
    assert_eq!(
        kinds(&release(&mut ui, last).actions),
        vec![ActionKind::Selected(2), ActionKind::Closed]
    );
}

#[test]
fn touch_presses_without_hover_and_can_be_cancelled() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    ui.set_input_mode(InputMode::Touch);
    let s = display_settings(&mut ui).unwrap();
    let pos = center(&mut ui, s.primary);
    assert!(ui.handle(Event::TouchDown { id: 7, pos }).consumed);
    assert!(ui.visual(s.primary).pressed && !ui.visual(s.primary).hovered);
    assert!(
        !ui.handle(Event::TouchDown { id: 8, pos }).consumed,
        "a second finger is ignored"
    );
    assert!(!ui.handle(Event::TouchUp { id: 8, pos }).consumed);
    assert!(!ui.handle(Event::TouchMove { id: 8, pos }).consumed);
    let up = ui.handle(Event::TouchUp { id: 7, pos });
    assert_eq!(kinds(&up.actions), vec![ActionKind::Toggled(false)]);
    // Cancelled touches act on nothing.
    ui.handle(Event::TouchDown { id: 9, pos });
    assert!(ui.handle(Event::TouchCancel { id: 9 }).consumed);
    assert!(!ui.visual(s.primary).pressed);
    assert!(!ui.is_on(s.primary).unwrap());
    assert!(!ui.handle(Event::TouchCancel { id: 9 }).consumed);
    assert!(!ui.handle(Event::TouchUp { id: 9, pos }).consumed);
    // Touch drags a slider.
    let rect = ui.rect(s.brightness).unwrap();
    ui.handle(Event::TouchDown {
        id: 1,
        pos: rect.center(),
    });
    let moved = ui.handle(Event::TouchMove {
        id: 1,
        pos: Point::new(rect.x - 50.0, rect.center().y),
    });
    assert_eq!(
        kinds(&moved.actions),
        vec![ActionKind::ValueChanged(100.0)],
        "the left end is the maximum in RTL"
    );
    ui.handle(Event::TouchUp {
        id: 1,
        pos: rect.center(),
    });
}

#[test]
fn clicking_a_text_field_places_the_caret() {
    // (language, content, where its first letter is: at the left or the right end)
    for (lang, content, first_is_left) in [(Lang::En, "abcdef", true), (Lang::Ar, "سلام", false)]
    {
        let mut ui = common::ui(lang, ThemeKind::Dark, Size::new(400.0, 100.0));
        let root = ui.root();
        let field = ui
            .add(
                root,
                TextField::new(Label::key("demo.device")).text(content),
            )
            .unwrap();
        ui.layout();
        let rect = ui.rect(field).unwrap();
        let y = rect.center().y;
        let (left, right) = (
            Point::new(rect.x + 2.0, y),
            Point::new(rect.right() - 2.0, y),
        );
        let (at_first, at_last) = if first_is_left {
            (left, right)
        } else {
            (right, left)
        };
        press(&mut ui, at_first);
        release(&mut ui, at_first);
        common::key(&mut ui, Key::Char('X'));
        assert_eq!(
            ui.text(field).unwrap(),
            format!("X{content}"),
            "{lang:?}: caret before the first letter"
        );
        press(&mut ui, at_last);
        release(&mut ui, at_last);
        common::key(&mut ui, Key::Char('Y'));
        assert_eq!(
            ui.text(field).unwrap(),
            format!("X{content}Y"),
            "{lang:?}: caret after the last letter"
        );
        assert!(ui.visual(field).focused, "typing shows where the focus is");
        press(&mut ui, at_last);
        assert!(
            !ui.visual(field).focused && ui.focused() == Some(field),
            "a click alone shows no ring"
        );
        release(&mut ui, at_last);
        ui.set_text(field, "x").unwrap();
        assert_eq!(ui.text(field).unwrap(), "x");
        assert!(ui.text(root).is_err() && ui.value(field).is_err() && ui.is_on(field).is_err());
    }
}
