//! Bricks added for the browser window (apps/ultar-web): the application-supplied picture,
//! region and chip cards, the tab button and the status icon button.

mod common;

use ultar_ui::canvas::Canvas;
use ultar_ui::color::Color;
use ultar_ui::geom::PixelRect;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::tokens::{ColorRole, Space, ThemeKind, LIGHT};
use ultar_ui::ui::{Role, UiError};
use ultar_ui::widgets::{BarEdge, Button, ButtonVariant, Card, IconButton, Label, PixelArea};
use ultar_ui::{Lang, Size};

/// A colour from its channels (red, green, blue).
fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::rgb(r, g, b)
}

#[test]
fn a_picture_shows_supplied_pixels_and_only_repaints_what_changed() {
    let mut ui = common::ui(Lang::En, ThemeKind::Light, Size::new(40.0, 30.0));
    let root = ui.root();
    let pic = ui
        .add_sized(root, PixelArea::new(Label::data("page")), Sizing::grow(1))
        .unwrap();
    let canvas = ultar_ui::snapshot::render(&mut ui).unwrap();
    assert_eq!(canvas.pixel(5, 5), Some(LIGHT.get(ColorRole::Surface)));
    assert_eq!(ui.picture_size(pic), None);
    assert!(matches!(
        ui.write_picture(pic, PixelRect::new(0, 0, 1, 1), &[0; 4], 4),
        Err(UiError::WrongWidget { .. })
    ));
    // A 10 × 10 blue frame (BGRX).
    let frame = Canvas::from_bgrx(10, 10, [255, 0, 0, 0].repeat(100)).unwrap();
    ui.set_picture(pic, frame).unwrap();
    assert_eq!(ui.picture_size(pic), Some((10, 10)));
    let canvas = ultar_ui::snapshot::render(&mut ui).unwrap();
    assert_eq!(canvas.pixel(5, 5), Some(rgb(0, 0, 255)));
    assert_eq!(canvas.pixel(20, 5), Some(LIGHT.get(ColorRole::Surface)));
    // Two red pixels written into the frame.
    ui.write_picture(
        pic,
        PixelRect::new(2, 2, 4, 3),
        &[0, 0, 255, 0, 0, 0, 255, 0],
        8,
    )
    .unwrap();
    assert!(ui.needs_paint());
    let canvas = ultar_ui::snapshot::render(&mut ui).unwrap();
    assert_eq!(canvas.pixel(3, 2), Some(rgb(255, 0, 0)));
    assert_eq!(canvas.pixel(4, 2), Some(rgb(0, 0, 255)));
    assert!(ui
        .write_picture(pic, PixelRect::new(0, 0, 4, 4), &[0; 8], 16)
        .is_err());
    ui.clear_picture(pic).unwrap();
    assert_eq!(ui.picture_size(pic), None);
    let node = ui.a11y_node(pic).unwrap();
    assert_eq!(node.role, Role::Image);
    assert!(node.focusable);
    let button = ui.add(root, Button::new(Label::data("x"))).unwrap();
    assert!(ui
        .set_picture(button, Canvas::from_bgrx(1, 1, vec![0; 4]).unwrap())
        .is_err());
}

#[test]
fn region_chip_tab_and_status_bricks() {
    let mut ui = common::ui(Lang::En, ThemeKind::Light, Size::new(400.0, 120.0));
    let root = ui.root();
    let strip = ui
        .add_sized(
            root,
            Card::region(ColorRole::Raised, BarEdge::Bottom).flex(
                Flex::row().cross(CrossAlign::Start).padding(Padding {
                    start: Some(Space::S8),
                    end: None,
                    top: Some(Space::S8),
                    bottom: None,
                }),
            ),
            Sizing::width(400.0).with_height(49.0),
        )
        .unwrap();
    let chip = ui.add(strip, Card::chip(true)).unwrap();
    let mut tab = Button::new(Label::data("Tab"));
    tab.variant = ButtonVariant::Tab(true);
    let tab = ui.add(chip, tab).unwrap();
    let close = ui
        .add(chip, IconButton::new(IconId::Close, Label::data("close")))
        .unwrap();
    let other = ui.add(strip, Card::chip(false)).unwrap();
    let mut plain = Button::new(Label::data("Other"));
    plain.variant = ButtonVariant::Tab(false);
    let plain = ui.add(other, plain).unwrap();
    let status = ui
        .add(
            strip,
            IconButton::new(IconId::Block, Label::data("p"))
                .status(IconId::Check, ColorRole::Success),
        )
        .unwrap();
    ui.layout();
    assert_eq!(ui.rect(status).unwrap().w, 52.0);
    assert_eq!(ui.rect(close).unwrap().x, ui.rect(tab).unwrap().right()); // physical
    let canvas = ultar_ui::snapshot::render(&mut ui).unwrap();
    assert_eq!(canvas.pixel(399, 10), Some(LIGHT.get(ColorRole::Raised)));
    assert_eq!(canvas.pixel(399, 48), Some(LIGHT.get(ColorRole::Line)));
    let c = ui.rect(chip).unwrap();
    assert_eq!(
        canvas.pixel(c.x as i32 + 4, c.y as i32 + 4),
        Some(LIGHT.get(ColorRole::Surface))
    );
    assert_eq!(
        canvas.pixel(c.x as i32 + 16, c.y as i32),
        Some(LIGHT.get(ColorRole::ControlBorder))
    );
    let o = ui.rect(other).unwrap();
    assert_eq!(
        canvas.pixel(o.x as i32 + 4, o.y as i32 + 4),
        Some(LIGHT.get(ColorRole::Raised))
    );
    let shown = ui.a11y_node(tab).unwrap();
    assert_eq!((shown.role, shown.state.selected), (Role::Tab, Some(true)));
    assert_eq!(ui.a11y_node(plain).unwrap().state.selected, Some(false));
}

#[test]
fn a_shown_tab_is_semibold() {
    let mut ui = common::ui(Lang::En, ThemeKind::Light, Size::new(400.0, 120.0));
    let root = ui.root();
    let row = ui.add(root, Flex::row()).unwrap();
    let mut regular = Button::new(Label::data("Other"));
    regular.variant = ButtonVariant::Tab(false);
    let plain = ui.add(row, regular).unwrap();
    // The chosen tab's label is semibold, so it is wider than the same label regular.
    let mut same = Button::new(Label::data("Other"));
    same.variant = ButtonVariant::Tab(true);
    let bold = ui.add(row, same).unwrap();
    ui.layout();
    assert!(ui.rect(bold).unwrap().w > ui.rect(plain).unwrap().w);
}
