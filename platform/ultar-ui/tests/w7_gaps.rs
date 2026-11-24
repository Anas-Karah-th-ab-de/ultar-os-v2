//! Toolkit gaps G1–G13 found by the settings and installer surfaces
//! (docs/integration/settings-installer.md "Toolkit gaps"), checked against the boards
//! Installer, InstallerDisk, InstallerSummary, SettingsDisplay and Account.

mod common;

use common::{center, click, key, press, release};
use ultar_ui::event::{ActionKind, Event, Key};
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::snapshot::render;
use ultar_ui::tokens::{ColorRole, Space, ThemeKind, DARK, LIGHT};
use ultar_ui::ui::{Role, Ui, UiError};
use ultar_ui::widgets::{
    ArrangedItem, Arrangement, BarEdge, Button, Card, Dialog, Label, QrCode, RadioGroup,
    RadioLayout, Select, TextField, Widget,
};
use ultar_ui::{Color, Lang, Point, Rect, Size};

fn d(text: &str) -> Label {
    Label::data(text)
}

/// With `ULTAR_CONFORMANCE_OUT` set, writes the picture for the side-by-side report.
fn dump(ui: &mut Ui, name: &str) {
    if let Ok(dir) = std::env::var("ULTAR_CONFORMANCE_OUT") {
        let canvas = render(ui).unwrap();
        let png = ultar_ui::snapshot::Image::from_canvas(&canvas)
            .encode_png()
            .unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), png).unwrap();
    }
}

fn page(lang: Lang) -> Ui {
    let mut ui = common::ui(lang, ThemeKind::Dark, Size::new(800.0, 600.0));
    ui.set_background(ColorRole::Surface);
    ui.set_root_flex(
        Flex::column()
            .main(MainAlign::Start)
            .cross(CrossAlign::Stretch)
            .padding(Padding::all(Space::S16))
            .gap(Space::S16),
    );
    ui
}

fn rgb(c: Option<Color>) -> [u8; 3] {
    c.map_or([0, 0, 0], |c| [c.r, c.g, c.b])
}

fn role(r: ColorRole) -> [u8; 3] {
    let c = DARK.get(r);
    [c.r, c.g, c.b]
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    (0..3).all(|i| a[i].abs_diff(b[i]) <= 4)
}

/// G1 + G7: the installer's scope cards (side by side) and encryption cards (stacked, one
/// not available with its reason).
#[test]
fn radio_cards_look_like_the_installer_boards() {
    let mut ui = page(Lang::Ar);
    let root = ui.root();
    let scope = RadioGroup::new(
        d("حجم التثبيت"),
        vec![d("الأساسي"), d("الموسّع"), d("اختيار مخصّص")],
    )
    .layout(RadioLayout::CardRow)
    .detail(0, d("النظام والتطبيقات الأساسية فقط"))
    .selected(2);
    let scope = ui.add(root, scope).unwrap();
    let enc = RadioGroup::new(d("التشفير"), vec![d("بلا تشفير"), d("عبارة مرور"), d("TPM")])
        .layout(RadioLayout::Cards)
        .unavailable(2, d("غير متاحة على هذا الجهاز"))
        .selected(1);
    let enc = ui.add(root, enc).unwrap();
    dump(&mut ui, "G1-G7-cards");
    let canvas = render(&mut ui).unwrap();
    let rect = ui.rect(scope).unwrap();
    let parts: Vec<_> = ui
        .a11y_tree()
        .into_iter()
        .filter(|n| n.role == Role::Radio)
        .collect();
    assert_eq!(parts.len(), 6);
    // Three equal cards 12 apart; Arabic: the first at the right.
    let w = (rect.w - 24.0) / 3.0;
    assert!(
        (parts[0].rect.w - w).abs() < 0.5 && (parts[0].rect.right() - rect.right()).abs() < 0.5
    );
    assert!((parts[1].rect.right() + 12.0 - parts[0].rect.x).abs() < 0.5);
    // Chosen card: hover fill, two-border accent edge; others: surface fill, control border.
    let chosen = parts[2].rect;
    let at = |r: Rect, dx: f32, dy: f32| rgb(canvas.pixel((r.x + dx) as i32, (r.y + dy) as i32));
    assert!(near(
        at(chosen, chosen.w / 2.0, 1.0),
        role(ColorRole::AccentText)
    ));
    assert!(near(
        at(chosen, 6.0, chosen.h / 2.0),
        role(ColorRole::Hover)
    ));
    let plain = parts[1].rect;
    assert!(near(
        at(plain, plain.w / 2.0, 0.0),
        role(ColorRole::ControlBorder)
    ));
    assert!(near(
        at(plain, 6.0, plain.h / 2.0),
        role(ColorRole::Surface)
    ));
    // Not available: `line` edge, reason announced, never chosen.
    let off = parts[5].rect;
    assert!(near(at(off, off.w / 2.0, 0.0), role(ColorRole::Line)));
    assert!(parts[5].state.disabled);
    assert_eq!(parts[5].description, Some(d("غير متاحة على هذا الجهاز")));
    // Card heights: padding 12 + dot 24 at least.
    assert!(parts[3].rect.h >= 48.0);
    unavailable_option_is_inert(&mut ui, enc, off);
}

/// G7 on a radio group: never chosen by API, click or keyboard.
fn unavailable_option_is_inert(ui: &mut Ui, enc: ultar_ui::ui::NodeId, off: Rect) {
    assert_eq!(
        ui.set_selected(enc, 2),
        Err(UiError::OptionUnavailable {
            node: enc,
            index: 2
        })
    );
    let pos = off.center();
    press(ui, pos);
    assert!(release(ui, pos).actions.is_empty(), "a click does nothing");
    // The keyboard skips it: from option 1 down wraps to option 0.
    ui.focus(enc).unwrap();
    let r = key(ui, Key::ArrowDown);
    assert!(r.actions.iter().any(|a| a.kind == ActionKind::Selected(0)));
}

/// G1: tiles (boards "SettingsDisplay": display mode).
#[test]
fn radio_tiles_look_like_the_display_board() {
    let mut ui = page(Lang::Ar);
    let root = ui.root();
    // Tiles (display mode): centred title, check mark on the chosen one, no dot.
    let tiles = RadioGroup::new(
        d("وضع العرض"),
        vec![d("توسيع"), d("تكرار"), d("شاشة واحدة فقط")],
    )
    .layout(RadioLayout::Tiles)
    .detail(0, d("سطح مكتب واحد على كل الشاشات"))
    .selected(0);
    let tiles = ui.add(root, tiles).unwrap();
    let canvas = render(&mut ui).unwrap();
    let t = ui.rect(tiles).unwrap();
    let first = Rect::new(t.right() - (t.w - 24.0) / 3.0, t.y, (t.w - 24.0) / 3.0, t.h);
    let top = rgb(canvas.pixel((first.x + first.w / 2.0) as i32, (first.y + 1.0) as i32));
    assert!(near(top, role(ColorRole::AccentText)));
    // Padding 12, title line, 4, one detail line: about 12 + 22 + 4 + 19 + 12.
    assert!(t.h > 60.0 && t.h < 80.0, "tile height {}", t.h);
    let ink = (first.x as i32..first.right() as i32)
        .filter(|x| rgb(canvas.pixel(*x, (first.y + 22.0) as i32))[0] > 200)
        .count();
    assert!(ink > 4, "title (and check) drawn");
}

/// G7 for a select: the option stays in the list, cannot be chosen.
#[test]
fn a_select_option_can_be_unavailable() {
    let mut ui = page(Lang::En);
    let root = ui.root();
    let select = Select::new(d("Disk"), vec![d("Whole disk"), d("Beside Windows")])
        .unavailable(1, d("Not enough free space"));
    let select = ui.add_sized(root, select, Sizing::width(240.0)).unwrap();
    assert_eq!(
        ui.set_selected(select, 1),
        Err(UiError::OptionUnavailable {
            node: select,
            index: 1
        })
    );
    click(&mut ui, select);
    key(&mut ui, Key::ArrowDown);
    let r = key(&mut ui, Key::Enter);
    assert!(r.actions.iter().all(|a| a.kind != ActionKind::Selected(1)));
    assert_eq!(ui.selected(select).unwrap(), 0);
    let options: Vec<_> = ui
        .a11y_tree()
        .into_iter()
        .filter(|n| n.role == Role::Option)
        .collect();
    assert!(options[1].state.disabled && options[1].description.is_some());
}

/// G3 + G9: a sidebar region and a danger card.
#[test]
fn bar_regions_and_danger_cards() {
    let mut ui = page(Lang::Ar);
    let root = ui.root();
    let side = ui
        .add_sized(
            root,
            Card::bar(BarEdge::End),
            Sizing::AUTO.with_height(120.0),
        )
        .unwrap();
    let danger = ui.add(root, Card::danger()).unwrap();
    ui.add(danger, ultar_ui::widgets::Text::new(d("سيُمحى القرص")))
        .unwrap();
    let canvas = render(&mut ui).unwrap();
    let r = ui.rect(side).unwrap();
    let px = |x: f32, y: f32| rgb(canvas.pixel(x as i32, y as i32));
    // Arabic: the end edge is the left one.
    assert!(near(px(r.x, r.y + 60.0), role(ColorRole::Line)));
    assert!(near(px(r.right() - 1.0, r.y + 60.0), role(ColorRole::Bar)));
    assert!(near(px(r.x + 20.0, r.y + 60.0), role(ColorRole::Bar)));
    let c = ui.rect(danger).unwrap();
    assert!(near(
        px(c.x + c.w / 2.0, c.y + 1.0),
        role(ColorRole::Danger)
    ));
    assert!(near(
        px(c.x + 8.0, c.y + c.h / 2.0),
        role(ColorRole::Raised)
    ));
    assert_eq!(ui.a11y_node(danger).unwrap().role, Role::Alert);
}

/// G4 + G5: an accessible name of its own; focus set by the application after a rebuild.
#[test]
fn names_of_their_own_and_programmatic_focus() {
    let mut ui = page(Lang::Ar);
    let root = ui.root();
    let undo = ui.add(root, Button::new(d("استرجاع السابق"))).unwrap();
    ui.set_accessible_name(undo, Some(d("بطاقة الرسوميات: استرجاع التعريف السابق")))
        .unwrap();
    assert_eq!(
        ui.a11y_node(undo).unwrap().name,
        Some(d("بطاقة الرسوميات: استرجاع التعريف السابق"))
    );
    ui.set_accessible_name(undo, None).unwrap();
    assert_eq!(ui.a11y_node(undo).unwrap().name, Some(d("استرجاع السابق")));
    // A page rebuilt: the focused node is gone, the new one gets the focus.
    key(&mut ui, Key::Tab);
    assert_eq!(ui.focused(), Some(undo));
    ui.remove(undo).unwrap();
    let next = ui.add(root, Button::new(d("التالي"))).unwrap();
    ui.focus(next).unwrap();
    assert_eq!(ui.focused(), Some(next));
    assert!(ui.a11y_node(next).unwrap().state.focused);
    let card = ui.add(root, Card::new()).unwrap();
    assert!(ui.focus(card).is_err(), "only focusable nodes");
    ui.set_disabled(next, true).unwrap();
    assert!(ui.focus(next).is_err(), "not a disabled one");
}

/// G6: a row that wraps (boards "Account": the account buttons) and equal columns.
#[test]
fn rows_wrap_and_columns_share_the_width() {
    let mut ui = page(Lang::En);
    ui.set_root_flex(Flex::column().cross(CrossAlign::Start).gap(Space::S16));
    let root = ui.root();
    let row = ui
        .add_sized(
            root,
            Flex::row().gap(Space::S8).wrap(),
            Sizing::width(330.0),
        )
        .unwrap();
    let buttons: Vec<_> = ["Change password", "Two-step sign-in", "Sign out"]
        .iter()
        .map(|t| ui.add(row, Button::new(d(t))).unwrap())
        .collect();
    let cols = ui
        .add_sized(root, Flex::row().gap(Space::S12), Sizing::width(300.0))
        .unwrap();
    let a = ui
        .add_sized(cols, Button::new(d("A")), Sizing::grow(1))
        .unwrap();
    let b = ui
        .add_sized(cols, Button::new(d("A much longer label")), Sizing::grow(1))
        .unwrap();
    ui.layout();
    let r: Vec<Rect> = buttons.iter().map(|b| ui.rect(*b).unwrap()).collect();
    assert_eq!(r[0].y, r[1].y, "first two on one line");
    assert!(
        (r[2].y - (r[0].bottom() + 8.0)).abs() < 0.5,
        "third on the next line, 8 below: {r:?}"
    );
    assert_eq!(r[2].x, r[0].x, "the new line starts at the start edge");
    assert!(ui.rect(row).unwrap().h >= r[2].bottom() - r[0].y);
    let (ra, rb) = (ui.rect(a).unwrap(), ui.rect(b).unwrap());
    assert!(
        (ra.w - rb.w).abs() < 0.5 && (ra.w - 144.0).abs() < 0.5,
        "basis 0: equal columns"
    );
}

/// G8: a QR code – dark modules on a light ground in both themes, named as an image.
#[test]
fn a_qr_code_is_dark_on_light() {
    for theme in [ThemeKind::Dark, ThemeKind::Light] {
        let mut ui = common::ui(Lang::Ar, theme, Size::new(200.0, 200.0));
        ui.set_root_flex(Flex::column().cross(CrossAlign::Start));
        let root = ui.root();
        let code = QrCode::new(d("رمز QR لرقم Ultar"), b"ultar:+963-11-000").unwrap();
        let modules = code.modules();
        let id = ui.add(root, code).unwrap();
        let canvas = render(&mut ui).unwrap();
        let r = ui.rect(id).unwrap();
        assert_eq!((r.w, r.h), (96.0, 96.0));
        let module = (96.0 / (modules as f32 + 8.0)).floor();
        let left = r.x + (96.0 - module * modules as f32) / 2.0;
        let top = r.y + (96.0 - module * modules as f32) / 2.0;
        let ink = LIGHT.get(ColorRole::Text);
        let ground = LIGHT.get(ColorRole::Raised);
        let at = |x: f32, y: f32| canvas.pixel(x.round() as i32, y.round() as i32);
        // Top-left finder pattern: dark corner module, light quiet zone before it.
        assert_eq!(at(left + module / 2.0, top + module / 2.0), Some(ink));
        assert_eq!(at(left - module, top + module / 2.0), Some(ground));
        assert_eq!(ui.a11y_node(id).unwrap().role, Role::Image);
    }
}

/// G10: the slider writes its value with a unit.
#[test]
fn a_slider_writes_its_value() {
    let mut ui = page(Lang::En);
    let root = ui.root();
    let scale = ultar_ui::widgets::Slider::new(d("Scale"), 100.0, 200.0, 25.0)
        .value(125.0)
        .unit(d("%"));
    let id = ui.add_sized(root, scale, Sizing::width(300.0)).unwrap();
    let plain = ultar_ui::widgets::Slider::new(d("Scale"), 100.0, 200.0, 25.0).value(125.0);
    let other = ui.add_sized(root, plain, Sizing::width(300.0)).unwrap();
    let canvas = render(&mut ui).unwrap();
    // "125%" is wider than "25%": the value slot (48 at the end) holds more ink.
    let ink = |id| {
        let r = ui.rect(id).unwrap();
        (r.right() as i32 - 48..r.right() as i32)
            .flat_map(|x| (r.y as i32..r.bottom() as i32).map(move |y| (x, y)))
            .filter(|(x, y)| rgb(canvas.pixel(*x, *y))[0] > 120)
            .count()
    };
    assert!(
        ink(id) > ink(other),
        "value 125 written, not the 25 % of the range"
    );
}

/// G12: the show-password control outside the field is a toggle.
#[test]
fn a_password_shown_by_a_button_outside() {
    let mut ui = page(Lang::Ar);
    let root = ui.root();
    let field = TextField::password(d("عبارة المرور"))
        .reveal_outside()
        .text("secret");
    let field = ui.add_sized(root, field, Sizing::width(240.0)).unwrap();
    let parts = |ui: &mut Ui| {
        ui.a11y_tree()
            .into_iter()
            .filter(|n| n.role == Role::Button)
            .count()
    };
    assert_eq!(parts(&mut ui), 0, "no button inside the field");
    let revealed =
        |ui: &Ui| matches!(ui.widget(field), Some(Widget::TextField(f)) if f.is_revealed());
    ui.set_password_shown(field, true).unwrap();
    assert!(revealed(&ui));
    // It stays shown when the focus moves on (a toggle, not a hold).
    ui.focus(field).unwrap();
    key(&mut ui, Key::Tab);
    assert!(revealed(&ui));
    ui.set_password_shown(field, false).unwrap();
    assert!(!revealed(&ui));
    let plain = ui.add(root, TextField::new(d("الاسم"))).unwrap();
    assert_eq!(
        ui.set_password_shown(plain, true),
        Err(UiError::NotApplicable(plain))
    );
}

/// G13: removing the node a dialog was added to closes the dialog and its modal layer.
#[test]
fn removing_the_host_of_an_open_dialog_closes_it() {
    let mut ui = page(Lang::En);
    let root = ui.root();
    let view = ui.add(root, Card::new()).unwrap();
    let outside = ui.add(root, Button::new(d("Outside"))).unwrap();
    let dialog = ui.add(view, Dialog::new(d("Keep?"))).unwrap();
    ui.add(dialog, Button::new(d("Keep"))).unwrap();
    ui.open_dialog(dialog).unwrap();
    ui.remove(view).unwrap();
    assert_eq!(ui.open_dialog_node(), None);
    assert!(ui.widget(dialog).is_none(), "the dialog went with its host");
    let r = click(&mut ui, outside);
    assert!(
        r.actions.iter().any(|a| a.kind == ActionKind::Activated),
        "no modal layer left"
    );
}

/// G2: the display arrangement – click selects, dragging moves and snaps, arrows choose.
#[test]
fn an_arrangement_item_is_dragged_and_snaps() {
    let mut ui = page(Lang::Ar);
    let root = ui.root();
    let item = |n: &str, x: f32, w: f32, h: f32| {
        ArrangedItem::new(
            d(&format!("الشاشة {n}")),
            d(n),
            Rect::new(x, (144.0 - h) / 2.0, w, h),
        )
    };
    let canvas = Arrangement::new(
        d("الشاشة المحدَّدة"),
        vec![
            item("1", 232.0, 176.0, 112.0).tag(d("الرئيسية")),
            item("2", 80.0, 144.0, 96.0).caption(d("Dell 24″")),
        ],
    )
    .selected(0);
    let id = ui.add(root, canvas).unwrap();
    dump(&mut ui, "G2-arrangement");
    render(&mut ui).unwrap();
    let rect = ui.rect(id).unwrap();
    assert_eq!(rect.h, 144.0);
    // Click on item 2 selects it.
    let on_two = Point::new(rect.x + 150.0, rect.y + 72.0);
    press(&mut ui, on_two);
    let r = release(&mut ui, on_two);
    assert!(r.actions.iter().any(|a| a.kind == ActionKind::Selected(1)));
    // Drag item 2 to 5 points left of item 1: it snaps to touch it (x = 232 - 144 = 88).
    press(&mut ui, on_two);
    let to = Point::new(on_two.x + 3.0, on_two.y + 3.0);
    ui.handle(Event::PointerMove { pos: to });
    let r = release(&mut ui, to);
    assert!(
        r.actions.iter().any(|a| a.kind == ActionKind::Arranged(1)),
        "{:?}",
        r.actions
    );
    let placed = ui.arranged_rect(id, 1).unwrap();
    assert_eq!(
        (placed.x, placed.y),
        (88.0, 24.0),
        "snapped to the neighbour's edge and top"
    );
    // Arrow keys change the selection.
    ui.focus(id).unwrap();
    let r = key(&mut ui, Key::ArrowDown);
    assert!(r.actions.iter().any(|a| a.kind == ActionKind::Selected(0)));
    let radios = ui
        .a11y_tree()
        .into_iter()
        .filter(|n| n.role == Role::Radio)
        .count();
    assert_eq!(radios, 2);
    let _ = center(&mut ui, id);
}
