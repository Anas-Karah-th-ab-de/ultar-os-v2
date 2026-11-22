//! Logical layout: start/end resolve at paint time, flex rows and columns, sizing, limits.

mod common;

use common::display_settings;
use ultar_ui::layout::{CrossAlign, Flex, Length, MainAlign, Padding, Sizing};
use ultar_ui::tokens::{InputMode, SizeClass, Space, ThemeKind};
use ultar_ui::ui::{NodeId, Ui, UiError, MAX_DEPTH, MAX_NODES};
use ultar_ui::widgets::{Button, Card, Label, Row, Switch, Text};
use ultar_ui::{Direction, Lang, Rect, Size};

const SURFACE: Size = Size::new(720.0, 760.0);

fn all_rects(ui: &mut Ui) -> Vec<Rect> {
    ui.layout();
    (0..ui.node_count() as u32)
        .map(|i| ui.rect(node(ui, i)).unwrap())
        .collect()
}

/// The `index`-th node in creation order (ids are handed out sequentially).
fn node(ui: &Ui, index: u32) -> NodeId {
    let mut found = vec![ui.root()];
    let mut next = 0;
    while next < found.len() {
        found.extend_from_slice(ui.children(found[next]));
        next += 1;
    }
    found.sort();
    found[index as usize]
}

#[test]
fn the_same_tree_is_mirrored_in_rtl() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let screen = display_settings(&mut ui).unwrap();
    let logical: Vec<_> = {
        ui.layout();
        (0..ui.node_count() as u32)
            .map(|i| ui.logical_rect(node(&ui, i)).unwrap())
            .collect()
    };
    assert_eq!(ui.env().direction, Direction::Rtl);
    let rtl = all_rects(&mut ui);
    ui.set_direction(Direction::Ltr);
    let ltr = all_rects(&mut ui);
    assert_eq!(rtl.len(), ltr.len());
    assert!(rtl.len() > 30);
    let mut moved = 0;
    for (r, l) in rtl.iter().zip(&ltr) {
        assert_eq!((r.y, r.w, r.h), (l.y, l.w, l.h));
        assert!(
            (r.x - (SURFACE.w - l.right())).abs() < 0.001,
            "{r:?} is not the mirror of {l:?}"
        );
        moved += usize::from((r.x - l.x).abs() > 1.0);
    }
    assert!(moved > 10, "most nodes sit on the other side");
    // The logical rectangles did not change: direction is resolved only when asked for.
    for (i, before) in logical.iter().enumerate() {
        assert_eq!(ui.logical_rect(node(&ui, i as u32)).unwrap(), *before);
    }
    // In RTL the switch (end side) is at the left of its row, in LTR at the right.
    let row_ltr = ui.rect(screen.card).unwrap();
    let switch_ltr = ui.rect(screen.primary).unwrap();
    assert!(switch_ltr.center().x > row_ltr.center().x);
    ui.set_direction(Direction::Rtl);
    assert!(ui.rect(screen.primary).unwrap().center().x < row_ltr.center().x);
    // "Apply" is the last action: at the end edge.
    let apply = ui.rect(screen.apply).unwrap();
    let revert = ui.rect(screen.revert).unwrap();
    assert!(apply.x < revert.x, "end is left in RTL");
    assert!((apply.x - Space::S32.points()).abs() < 0.001);
}

fn row_of_three(ui: &mut Ui, flex: Flex) -> [NodeId; 3] {
    ui.set_root_flex(flex);
    let root = ui.root();
    let size = Space::S48.points();
    let add = |ui: &mut Ui| {
        ui.add_sized(root, Flex::row(), Sizing::width(size).with_height(size))
            .unwrap()
    };
    [add(ui), add(ui), add(ui)]
}

#[test]
fn gap_padding_and_main_alignment() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(400.0, 100.0));
    let (gap, pad, item) = (Space::S8.points(), Space::S16.points(), Space::S48.points());
    let flex = Flex::row().gap(Space::S8).padding(Padding::all(Space::S16));
    let ids = row_of_three(&mut ui, flex);
    ui.layout();
    let xs: Vec<f32> = ids
        .iter()
        .map(|id| ui.logical_rect(*id).unwrap().start)
        .collect();
    assert_eq!(xs, vec![pad, pad + item + gap, pad + 2.0 * (item + gap)]);
    let inner = 400.0 - 2.0 * pad;
    let free = inner - 3.0 * item - 2.0 * gap;
    ui.set_root_flex(flex.main(MainAlign::End));
    ui.layout();
    assert_eq!(ui.logical_rect(ids[0]).unwrap().start, pad + free);
    ui.set_root_flex(flex.main(MainAlign::Center));
    ui.layout();
    assert_eq!(ui.logical_rect(ids[0]).unwrap().start, pad + free / 2.0);
    ui.set_root_flex(flex.main(MainAlign::SpaceBetween));
    ui.layout();
    assert_eq!(ui.logical_rect(ids[2]).unwrap().start + item, 400.0 - pad);
    assert_eq!(
        ui.logical_rect(ids[1]).unwrap().start,
        pad + item + gap + free / 2.0
    );
}

#[test]
fn cross_alignment_and_stretch() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(400.0, 100.0));
    let item = Space::S48.points();
    let ids = row_of_three(&mut ui, Flex::row().cross(CrossAlign::Start));
    ui.layout();
    assert_eq!(ui.logical_rect(ids[0]).unwrap().top, 0.0);
    ui.set_root_flex(Flex::row().cross(CrossAlign::Center));
    ui.layout();
    assert_eq!(ui.logical_rect(ids[0]).unwrap().top, (100.0 - item) / 2.0);
    ui.set_root_flex(Flex::row().cross(CrossAlign::End));
    ui.layout();
    assert_eq!(ui.logical_rect(ids[0]).unwrap().top, 100.0 - item);
    ui.set_root_flex(Flex::row().cross(CrossAlign::Stretch));
    ui.layout();
    assert_eq!(ui.logical_rect(ids[0]).unwrap().height, 100.0);
    // In a column, stretch fills the width and End puts children at the end edge.
    ui.set_root_flex(Flex::column());
    ui.layout();
    assert_eq!(ui.logical_rect(ids[1]).unwrap().width, 400.0);
    assert_eq!(ui.logical_rect(ids[1]).unwrap().top, item);
    ui.set_root_flex(Flex::column().cross(CrossAlign::End));
    ui.layout();
    let last = ui.logical_rect(ids[2]).unwrap();
    assert_eq!(
        (last.start, last.width, last.top),
        (400.0 - item, item, 2.0 * item)
    );
    assert_eq!(
        ui.rect(ids[2]).unwrap().x,
        400.0 - item,
        "end is right in LTR"
    );
    ui.set_direction(Direction::Rtl);
    assert_eq!(ui.rect(ids[2]).unwrap().x, 0.0, "and left in RTL");
}

#[test]
fn growing_children_take_the_left_over_space() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(400.0, 200.0));
    ui.set_root_flex(Flex::row().gap(Space::S8));
    let root = ui.root();
    let fixed = ui
        .add_sized(root, Flex::row(), Sizing::width(100.0))
        .unwrap();
    let one = ui.add_sized(root, Flex::row(), Sizing::grow(1)).unwrap();
    let two = ui.add_sized(root, Flex::row(), Sizing::grow(2)).unwrap();
    ui.layout();
    let free = 400.0 - 100.0 - 2.0 * Space::S8.points();
    assert_eq!(ui.logical_rect(fixed).unwrap().width, 100.0);
    assert!((ui.logical_rect(one).unwrap().width - free / 3.0).abs() < 0.001);
    assert!((ui.logical_rect(two).unwrap().width - 2.0 * free / 3.0).abs() < 0.001);
    assert!(
        (ui.logical_rect(two).unwrap().start + ui.logical_rect(two).unwrap().width - 400.0).abs()
            < 0.001
    );
    // In a column the growing child takes the left-over height.
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(400.0, 200.0));
    let root = ui.root();
    let top = ui.add(root, Text::new(Label::key("demo.save"))).unwrap();
    let filler = ui.add_sized(root, Flex::column(), Sizing::grow(1)).unwrap();
    let bottom = ui.add(root, Text::new(Label::key("demo.delete"))).unwrap();
    ui.layout();
    let (t, f, b) = (
        ui.logical_rect(top).unwrap(),
        ui.logical_rect(filler).unwrap(),
        ui.logical_rect(bottom).unwrap(),
    );
    assert_eq!(f.top, t.height);
    assert_eq!(b.top + b.height, 200.0);
    assert_eq!(f.height, 200.0 - t.height - b.height);
    assert_eq!(Sizing::AUTO.width, Length::Auto);
}

#[test]
fn text_wraps_to_the_width_and_rows_grow_with_it() {
    let mut wide = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(600.0, 400.0));
    let mut narrow = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(220.0, 400.0));
    let mut heights = Vec::new();
    for ui in [&mut wide, &mut narrow] {
        let root = ui.root();
        let text = ui
            .add(root, Text::new(Label::key("demo.card.text")))
            .unwrap();
        let row = ui
            .add(
                root,
                Row::new(Label::key("demo.night")).sublabel(Label::key("demo.card.text")),
            )
            .unwrap();
        ui.add(row, Switch::new(Label::key("demo.night"), true))
            .unwrap();
        ui.layout();
        heights.push((ui.rect(text).unwrap().h, ui.rect(row).unwrap().h));
        assert!(ui.rect(text).unwrap().w <= ui.size().w);
        let content = ui.content_size();
        assert_eq!(
            content.h,
            ui.rect(text).unwrap().h + ui.rect(row).unwrap().h
        );
    }
    assert!(
        heights[1].0 >= 2.0 * heights[0].0,
        "narrow text needs more lines: {heights:?}"
    );
    assert!(
        heights[1].1 > heights[0].1,
        "the row grows with its sublabel"
    );
    assert!(heights[0].1 >= Space::S48.points() + Space::S8.points());
}

#[test]
fn settings_row_puts_its_control_at_the_end_centred() {
    for lang in common::LANGS {
        let mut ui = common::ui(lang, ThemeKind::Dark, Size::new(500.0, 300.0));
        let root = ui.root();
        let card = ui.add(root, Card::new()).unwrap();
        let row = ui.add(card, Row::new(Label::key("demo.night"))).unwrap();
        let control = ui
            .add(row, Switch::new(Label::key("demo.night"), false))
            .unwrap();
        ui.layout();
        let (r, c) = (
            ui.logical_rect(row).unwrap(),
            ui.logical_rect(control).unwrap(),
        );
        assert_eq!(
            c.start + c.width,
            r.start + r.width - Space::S16.points(),
            "{lang:?}"
        );
        assert!((c.top + c.height / 2.0 - (r.top + r.height / 2.0)).abs() < 0.001);
        let physical = ui.rect(control).unwrap();
        if lang == Lang::Ar {
            assert_eq!(physical.x, Space::S16.points());
        } else {
            assert_eq!(physical.right(), 500.0 - Space::S16.points());
        }
    }
}

#[test]
fn tree_limits_and_errors() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(100.0, 100.0));
    let root = ui.root();
    let button = ui.add(root, Button::new(Label::key("demo.save"))).unwrap();
    assert_eq!(
        ui.add(button, Flex::row()).unwrap_err(),
        UiError::NotAContainer(button)
    );
    let row = ui.add(root, Row::new(Label::key("demo.night"))).unwrap();
    ui.add(row, Switch::new(Label::key("demo.night"), false))
        .unwrap();
    assert_eq!(
        ui.add(row, Switch::new(Label::key("demo.night"), false))
            .unwrap_err(),
        UiError::NotAContainer(row)
    );
    let mut deepest = root;
    let mut depth_error = None;
    for _ in 0..MAX_DEPTH + 2 {
        match ui.add(deepest, Flex::column()) {
            Ok(id) => deepest = id,
            Err(e) => {
                depth_error = Some(e);
                break;
            }
        }
    }
    assert_eq!(depth_error, Some(UiError::TooDeep));
    ui.layout();
    let mut many = None;
    for _ in 0..MAX_NODES + 1 {
        if let Err(e) = ui.add(root, Flex::row()) {
            many = Some(e);
            break;
        }
    }
    assert_eq!(many, Some(UiError::TooManyNodes));
    assert_eq!(ui.node_count(), MAX_NODES);
    ui.layout();
    ui.clear();
    assert_eq!(ui.node_count(), 1);
    assert!(
        ui.rect(button).is_none() && ui.children(button).is_empty() && ui.widget(button).is_none()
    );
    assert!(matches!(
        ui.add(button, Flex::row()),
        Err(UiError::UnknownNode(_))
    ));
    assert_eq!(ui.set_scale(0.0), Err(UiError::InvalidScale));
    assert_eq!(ui.set_scale(f32::NAN), Err(UiError::InvalidScale));
    for error in [
        UiError::TooDeep,
        UiError::TooManyNodes,
        UiError::InvalidScale,
        UiError::NotAContainer(root),
    ] {
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn scale_and_touch_mode_change_pixels_not_logic() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let screen = display_settings(&mut ui).unwrap();
    ui.layout();
    let before = ui.rect(screen.apply).unwrap();
    ui.set_scale(2.0).unwrap();
    ui.layout();
    let scaled = ui.rect(screen.apply).unwrap();
    assert!(
        (scaled.w - before.w).abs() <= 1.0 && scaled.h == before.h,
        "logical size is stable: {before:?} {scaled:?}"
    );
    let canvas = ultar_ui::snapshot::render(&mut ui).unwrap();
    assert_eq!((canvas.width(), canvas.height()), (1440, 1520));
    ui.set_input_mode(InputMode::Touch);
    ui.layout();
    assert_eq!(
        ui.rect(screen.apply).unwrap().h,
        InputMode::Touch.min_target()
    );
    assert_eq!(ui.input_mode(), InputMode::Touch);
    assert_eq!(ui.size_class(), SizeClass::Medium);
    ui.set_size(Size::new(390.0, 800.0));
    assert_eq!(ui.size_class(), SizeClass::Compact);
    ui.set_size(Size::new(1440.0, 900.0));
    assert_eq!(ui.size_class(), SizeClass::Wide);
}
