//! The scroll container: wheel, touch drag, scrollbar, keyboard, focus follows scroll,
//! clipping of what is scrolled out, damage.

mod common;

use common::gallery::{gallery, Gallery};
use common::{click, key, press, release};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{ActionKind, Event, Key, PointerButton};
use ultar_ui::layout::Sizing;
use ultar_ui::tokens::{ColorRole, ThemeKind, DARK};
use ultar_ui::ui::{NodeId, Role, Ui};
use ultar_ui::widgets::{Button, Label, Scroll, Select, Slider};
use ultar_ui::{Lang, Point, Size};

const SURFACE: Size = Size::new(720.0, 420.0);

fn setup(lang: Lang) -> (Ui, Gallery) {
    let mut ui = common::ui(lang, ThemeKind::Dark, SURFACE);
    let g = gallery(&mut ui).unwrap();
    ui.layout();
    (ui, g)
}

fn offset(ui: &Ui, id: NodeId) -> f32 {
    ui.scroll_offset(id).unwrap()
}

fn wheel(ui: &mut Ui, pos: Point, lines: f32) -> ultar_ui::event::Response {
    ui.handle(Event::Scroll { pos, lines })
}

#[test]
fn the_wheel_scrolls_and_stops_at_both_ends() {
    let (mut ui, g) = setup(Lang::Ar);
    let view = ui.rect(g.scroll).unwrap();
    let inside = view.center();
    let down = wheel(&mut ui, inside, 2.0);
    assert!(down.consumed);
    assert_eq!(down.actions.len(), 1);
    assert_eq!(down.actions[0].node, g.scroll);
    assert_eq!(down.actions[0].kind, ActionKind::Scrolled(96.0));
    assert_eq!(offset(&ui, g.scroll), 96.0);
    let up = wheel(&mut ui, inside, -50.0);
    assert!(up.consumed);
    assert_eq!(offset(&ui, g.scroll), 0.0);
    assert!(
        !wheel(&mut ui, inside, -1.0).consumed,
        "at the top: the wheel is not used"
    );
    wheel(&mut ui, inside, 1.0e9);
    let bottom = offset(&ui, g.scroll);
    assert!(bottom > 300.0);
    assert!(!wheel(&mut ui, inside, 1.0).consumed, "at the bottom");
    assert!(!wheel(&mut ui, inside, f32::NAN).consumed);
    assert!(!wheel(&mut ui, inside, f32::INFINITY).consumed);
    assert_eq!(offset(&ui, g.scroll), bottom);
    assert!(
        !wheel(&mut ui, Point::new(view.center().x, 4.0), 1.0).consumed,
        "outside the viewport"
    );
    // Setter: clamped, no action, never NaN.
    ui.set_scroll_offset(g.scroll, -5.0).unwrap();
    assert_eq!(offset(&ui, g.scroll), 0.0);
    ui.set_scroll_offset(g.scroll, f32::NAN).unwrap();
    assert_eq!(offset(&ui, g.scroll), 0.0);
    ui.set_scroll_offset(g.scroll, f32::INFINITY).unwrap();
    assert_eq!(offset(&ui, g.scroll), bottom);
    assert!(ui.set_scroll_offset(g.tabs, 1.0).is_err() && ui.scroll_offset(g.tabs).is_err());
}

#[test]
fn rectangles_and_hits_follow_the_offset_and_the_viewport_clips() {
    let (mut ui, g) = setup(Lang::En);
    let view = ui.rect(g.scroll).unwrap();
    let before = ui.rect(g.checkbox).unwrap();
    ui.set_scroll_offset(g.scroll, 60.0).unwrap();
    let after = ui.rect(g.checkbox).unwrap();
    assert_eq!((after.x, after.y), (before.x, before.y - 60.0));
    assert_eq!(
        ui.logical_rect(g.checkbox).unwrap().top,
        before.y,
        "the layout is untouched"
    );
    // The checkbox is scrolled above the viewport now: a click where it would be (over the
    // tab bar) must not reach it.
    let ghost = ui.rect(g.checkbox).unwrap().center();
    assert!(ghost.y < view.y);
    press(&mut ui, ghost);
    release(&mut ui, ghost);
    assert!(!ui.is_checked(g.checkbox).unwrap());
    // What is below the viewport is not hit either.
    let below = Point::new(view.center().x, view.bottom() + 6.0);
    press(&mut ui, below);
    assert!(release(&mut ui, below).actions.is_empty());
    // Painting: nothing of the scrolled content appears outside the viewport.
    let mut canvas = Canvas::new(720, 420, PixelFormat::Rgba8).unwrap();
    ui.paint_all(&mut canvas);
    let ground = DARK.get(ColorRole::Ground);
    let y_below = (view.bottom() + 3.0) as i32;
    for x in 0..720 {
        assert_eq!(canvas.pixel(x, y_below), Some(ground), "x={x}");
    }
}

#[test]
fn focus_follows_scroll_in_both_directions() {
    let (mut ui, g) = setup(Lang::Ar);
    let view = ui.rect(g.scroll).unwrap();
    let inside = |ui: &Ui, id: NodeId| {
        let r = ui.rect(id).unwrap();
        r.y >= view.y && r.bottom() <= view.bottom()
    };
    assert!(
        !inside(&ui, g.status.empty_action),
        "starts below the viewport"
    );
    let mut seen = Vec::new();
    for _ in 0..40 {
        key(&mut ui, Key::Tab);
        let Some(id) = ui.focused() else { break };
        if seen.contains(&id) {
            break;
        }
        if ui.rect(id).unwrap().h <= view.h && id != g.scroll {
            let scrolled_parent = [
                g.status.bar_action,
                g.checkbox,
                g.radio,
                g.items[0],
                g.items[1],
                g.items[2],
                g.item_switch,
                g.status.empty_action,
            ]
            .contains(&id);
            assert!(
                !scrolled_parent || inside(&ui, id),
                "{:?} focused but not visible",
                ui.widget(id)
            );
        }
        seen.push(id);
    }
    assert!(seen.contains(&g.status.empty_action));
    let deep = offset(&ui, g.scroll);
    assert!(deep > 0.0);
    // Back up with Shift+Tab: the view follows upwards again.
    while ui.focused() != Some(g.checkbox) {
        common::shift_tab(&mut ui);
    }
    assert!(inside(&ui, g.checkbox));
    assert!(offset(&ui, g.scroll) < deep);
    // Explicit request.
    ui.scroll_into_view(g.status.empty_action).unwrap();
    assert!(inside(&ui, g.status.empty_action));
    // A node of another (larger) tree is unknown here.
    let mut small = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    assert!(small.scroll_into_view(g.status.empty_action).is_err());
}

#[test]
fn keyboard_scrolls_the_container_itself_and_around_the_focus() {
    let (mut ui, g) = setup(Lang::En);
    assert!(
        ui.focus_order().contains(&g.scroll),
        "a scrollable container is a focus stop"
    );
    while ui.focused() != Some(g.scroll) {
        key(&mut ui, Key::Tab);
    }
    assert!(key(&mut ui, Key::ArrowDown).consumed);
    assert_eq!(offset(&ui, g.scroll), 48.0);
    assert!(key(&mut ui, Key::PageDown).consumed);
    let paged = offset(&ui, g.scroll);
    assert!(paged > 48.0 + 100.0);
    assert!(key(&mut ui, Key::End).consumed);
    let bottom = offset(&ui, g.scroll);
    assert!(!key(&mut ui, Key::PageDown).consumed, "already at the end");
    assert!(key(&mut ui, Key::PageUp).consumed && offset(&ui, g.scroll) < bottom);
    assert!(key(&mut ui, Key::Home).consumed);
    assert_eq!(offset(&ui, g.scroll), 0.0);
    assert!(
        key(&mut ui, Key::ArrowUp).consumed,
        "at the top the arrow moves focus instead"
    );
    assert_ne!(ui.focused(), Some(g.scroll));
    // With focus on a widget inside, Page Down scrolls its container.
    while ui.focused() != Some(g.checkbox) {
        key(&mut ui, Key::Tab);
    }
    let before = offset(&ui, g.scroll);
    assert!(key(&mut ui, Key::PageDown).consumed);
    assert!(offset(&ui, g.scroll) > before);
    let node = ui.a11y_node(g.scroll).unwrap();
    assert_eq!(
        (node.role, node.name, node.focusable),
        (Role::ScrollArea, Some(Label::key("demo.scroll")), true)
    );
}

#[test]
fn a_container_whose_content_fits_is_no_focus_stop_and_ignores_the_wheel() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, SURFACE);
    let root = ui.root();
    let scroll = ui.add_sized(root, Scroll::new(), Sizing::grow(1)).unwrap();
    let button = ui
        .add(scroll, Button::new(Label::key("demo.save")))
        .unwrap();
    assert_eq!(ui.focus_order(), vec![button]);
    let pos = ui.rect(scroll).unwrap().center();
    assert!(!wheel(&mut ui, pos, 3.0).consumed);
    assert!(!ui.a11y_node(scroll).unwrap().focusable);
    // Without a height of its own the container is as tall as its content.
    let natural = ui.add(root, Scroll::new()).unwrap();
    let inner = ui
        .add(natural, Button::new(Label::key("demo.apply")))
        .unwrap();
    ui.layout();
    assert_eq!(ui.rect(natural).unwrap().h, ui.rect(inner).unwrap().h);
}

#[test]
fn a_finger_drags_the_content_and_a_short_touch_still_taps() {
    let (mut ui, g) = setup(Lang::Ar);
    let start = ui.rect(g.checkbox).unwrap().center();
    // A tap: down and up at the same place toggles the checkbox.
    ui.handle(Event::TouchDown { id: 1, pos: start });
    let tap = ui.handle(Event::TouchUp { id: 1, pos: start });
    assert_eq!(
        tap.actions.iter().map(|a| a.kind).collect::<Vec<_>>(),
        vec![ActionKind::Toggled(true)]
    );
    // A drag: the finger moves up by 70 points, the content follows, nothing is tapped.
    ui.handle(Event::TouchDown { id: 2, pos: start });
    let small = ui.handle(Event::TouchMove {
        id: 2,
        pos: Point::new(start.x, start.y - 5.0),
    });
    assert!(small.actions.is_empty(), "inside the slop nothing moves");
    let moved = ui.handle(Event::TouchMove {
        id: 2,
        pos: Point::new(start.x, start.y - 70.0),
    });
    assert_eq!(
        moved.actions.iter().map(|a| a.kind).collect::<Vec<_>>(),
        vec![ActionKind::Scrolled(70.0)]
    );
    let back = ui.handle(Event::TouchMove {
        id: 2,
        pos: Point::new(start.x, start.y - 30.0),
    });
    assert_eq!(
        back.actions[0].kind,
        ActionKind::Scrolled(30.0),
        "the content sticks to the finger"
    );
    let up = ui.handle(Event::TouchUp {
        id: 2,
        pos: Point::new(start.x, start.y - 30.0),
    });
    assert!(up.consumed && up.actions.is_empty(), "a drag is not a tap");
    assert!(ui.is_checked(g.checkbox).unwrap(), "unchanged by the drag");
}

#[test]
fn a_cancelled_drag_stops_and_the_mouse_does_not_drag_content() {
    let (mut ui, g) = setup(Lang::Ar);
    let start = ui.rect(g.checkbox).unwrap().center();
    // A cancelled drag leaves the offset and clears the state.
    ui.handle(Event::TouchDown { id: 3, pos: start });
    ui.handle(Event::TouchMove {
        id: 3,
        pos: Point::new(start.x, start.y - 40.0),
    });
    assert!(ui.handle(Event::TouchCancel { id: 3 }).consumed);
    let kept = offset(&ui, g.scroll);
    assert_eq!(kept, 40.0);
    ui.handle(Event::TouchMove {
        id: 3,
        pos: Point::new(start.x, start.y - 200.0),
    });
    assert_eq!(offset(&ui, g.scroll), kept);
    // A mouse press does not drag the content.
    press(&mut ui, start);
    ui.handle(Event::PointerMove {
        pos: Point::new(start.x, start.y - 80.0),
    });
    assert_eq!(offset(&ui, g.scroll), kept);
    release(&mut ui, start);
}

#[test]
fn a_slider_under_the_finger_keeps_its_drag() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(400.0, 200.0));
    let root = ui.root();
    let scroll = ui.add_sized(root, Scroll::new(), Sizing::grow(1)).unwrap();
    let slider = ui
        .add(
            scroll,
            Slider::new(Label::key("demo.brightness"), 0.0, 100.0, 1.0),
        )
        .unwrap();
    for _ in 0..12 {
        ui.add(scroll, Button::new(Label::key("demo.save")))
            .unwrap();
    }
    ui.layout();
    let rect = ui.rect(slider).unwrap();
    let start = Point::new(rect.x + 20.0, rect.center().y);
    ui.handle(Event::TouchDown { id: 1, pos: start });
    let moved = ui.handle(Event::TouchMove {
        id: 1,
        pos: Point::new(rect.x + 120.0, start.y + 30.0),
    });
    assert!(matches!(moved.actions[0].kind, ActionKind::ValueChanged(v) if v > 20.0));
    assert_eq!(
        offset(&ui, scroll),
        0.0,
        "the slider has the finger, the list stays"
    );
    ui.handle(Event::TouchUp { id: 1, pos: start });
}

#[test]
fn the_scrollbar_thumb_can_be_dragged_and_the_track_pressed() {
    for lang in [Lang::En, Lang::Ar] {
        let (mut ui, g) = setup(lang);
        let view = ui.rect(g.scroll).unwrap();
        // The bar is at the end edge: right in English, left in Arabic.
        let x = if lang == Lang::Ar {
            view.x + 5.0
        } else {
            view.right() - 5.0
        };
        let top = Point::new(x, view.y + 10.0);
        let down = press(&mut ui, top);
        assert!(down.consumed, "{lang:?}");
        let drag = ui.handle(Event::PointerMove {
            pos: Point::new(x, view.bottom() + 500.0),
        });
        assert!(drag.consumed);
        let bottom = offset(&ui, g.scroll);
        assert!(bottom > 300.0, "{lang:?}: dragged to the end, got {bottom}");
        ui.handle(Event::PointerMove {
            pos: Point::new(x, view.y + view.h / 2.0),
        });
        let middle = offset(&ui, g.scroll);
        assert!(middle > 0.0 && middle < bottom);
        let up = release(&mut ui, Point::new(x, view.y));
        assert!(up.consumed && up.actions.is_empty());
        ui.handle(Event::PointerMove {
            pos: Point::new(x, view.y),
        });
        assert_eq!(
            offset(&ui, g.scroll),
            middle,
            "released: the thumb no longer follows"
        );
        // A press on the track below the thumb brings the thumb there.
        ui.set_scroll_offset(g.scroll, 0.0).unwrap();
        let low = Point::new(x, view.bottom() - 4.0);
        press(&mut ui, low);
        assert!(offset(&ui, g.scroll) > middle);
        release(&mut ui, low);
        // The widget under the bar strip was not activated by any of this.
        assert!(!ui.is_checked(g.checkbox).unwrap());
    }
}

#[test]
fn nested_containers_hand_the_wheel_outwards_at_their_end() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(400.0, 300.0));
    let root = ui.root();
    let outer = ui.add_sized(root, Scroll::new(), Sizing::grow(1)).unwrap();
    let inner = ui
        .add_sized(outer, Scroll::new(), Sizing::AUTO.with_height(120.0))
        .unwrap();
    for _ in 0..8 {
        ui.add(inner, Button::new(Label::key("demo.save"))).unwrap();
    }
    for _ in 0..12 {
        ui.add(outer, Button::new(Label::key("demo.apply")))
            .unwrap();
    }
    ui.layout();
    let pos = ui.rect(inner).unwrap().center();
    wheel(&mut ui, pos, 1.0);
    assert_eq!(
        (offset(&ui, inner), offset(&ui, outer)),
        (48.0, 0.0),
        "the inner one first"
    );
    wheel(&mut ui, pos, 100.0);
    let inner_end = offset(&ui, inner);
    assert_eq!(offset(&ui, outer), 0.0);
    wheel(&mut ui, pos, 1.0);
    assert_eq!(
        (offset(&ui, inner), offset(&ui, outer)),
        (inner_end, 48.0),
        "then the outer one"
    );
    // Focus inside the inner container reveals it in both.
    ui.set_scroll_offset(outer, 1.0e6).unwrap();
    ui.set_scroll_offset(inner, 0.0).unwrap();
    let first = ui.children(inner)[0];
    while ui.focused() != Some(first) {
        key(&mut ui, Key::Tab);
    }
    let (view_outer, view_inner, rect) = (
        ui.rect(outer).unwrap(),
        ui.rect(inner).unwrap(),
        ui.rect(first).unwrap(),
    );
    assert!(rect.y >= view_inner.y && rect.bottom() <= view_inner.bottom());
    assert!(rect.y >= view_outer.y && rect.bottom() <= view_outer.bottom());
}

#[test]
fn scrolling_closes_what_is_anchored_to_the_content_and_repaints_the_viewport() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(400.0, 300.0));
    let root = ui.root();
    let scroll = ui.add_sized(root, Scroll::new(), Sizing::grow(1)).unwrap();
    let select = ui.add(scroll, common::orientation_select()).unwrap();
    let _: Select = common::orientation_select();
    for _ in 0..12 {
        ui.add(scroll, Button::new(Label::key("demo.save")))
            .unwrap();
    }
    let mut canvas = Canvas::new(400, 300, PixelFormat::Rgba8).unwrap();
    ui.paint(&mut canvas);
    click(&mut ui, select);
    assert!(ui.is_open(select));
    ui.set_scroll_offset(scroll, 40.0).unwrap();
    assert!(!ui.is_open(select), "the list would hang in the air");
    ui.paint(&mut canvas);
    let mut full = Canvas::new(400, 300, PixelFormat::Rgba8).unwrap();
    ui.paint_all(&mut full);
    assert_eq!(
        canvas.data(),
        full.data(),
        "incremental frame after scrolling equals a full repaint"
    );
    canvas.take_damage();
    ui.handle(Event::PointerDown {
        pos: Point::new(-5.0, -5.0),
        button: PointerButton::Middle,
    });
    ui.paint(&mut canvas);
    assert!(
        canvas.damage().is_empty(),
        "nothing changed, nothing painted"
    );
}

#[test]
fn a_press_on_a_half_visible_widget_does_not_move_the_content() {
    let (mut ui, g) = setup(Lang::En);
    // Scroll so that the checkbox is cut by the top edge of the viewport.
    ui.set_scroll_offset(g.scroll, 20.0).unwrap();
    let view = ui.rect(g.scroll).unwrap();
    let rect = ui.rect(g.checkbox).unwrap();
    assert!(rect.y < view.y && rect.bottom() > view.y);
    let pos = Point::new(rect.x + 8.0, view.y + 4.0);
    press(&mut ui, pos);
    assert_eq!(offset(&ui, g.scroll), 20.0, "pointer focus does not scroll");
    let toggled = release(&mut ui, pos);
    assert_eq!(toggled.actions[0].kind, ActionKind::Toggled(true));
    // Keyboard focus on the same widget brings it fully into view.
    key(&mut ui, Key::Tab);
    common::shift_tab(&mut ui);
    assert_eq!(ui.focused(), Some(g.checkbox));
    assert!(ui.rect(g.checkbox).unwrap().y >= view.y);
}
