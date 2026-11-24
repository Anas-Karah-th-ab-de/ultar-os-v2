//! Design conformance – click list (docs/ui/CLICKS.md) for the actions the toolkit itself
//! defines (`menu.open`, `menu.item`, `select.option`, `tabs.tab`, `sheet.close`,
//! `toast.action`, `toast.close`) and the sizes of the boards "Menus" and "Bricks".
//! Every test drives the real widgets with synthetic pointer and keyboard input and checks the
//! effect CLICKS.md defines. Surfaces (shell, settings, store, …) are not part of the toolkit;
//! their clicks are tracked in `design/conformance/coverage.toml`.

mod common;

use common::{center, click, key};
use ultar_ui::event::{ActionKind, Event, Key, Modifiers, Response};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::tokens::{ColorRole, Space, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::{
    Button, ButtonVariant, Dialog, IconButton, Label, Menu, MenuItem, Select, Tabs, Toast, Tone,
};
use ultar_ui::{Lang, Point, Size};

const TOL: f32 = 2.0;

/// One checked expectation of the board. A known difference (listed in
/// `design/conformance/known-differences.txt` as `clicks/<id>`) must still fail – when it passes,
/// the line has to be removed; an unlisted one must pass.
fn expect(id: &str, ok: bool, detail: String) {
    let known = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../design/conformance/known-differences.txt"),
    )
    .unwrap_or_default();
    let key = format!("clicks/{id}");
    let listed = known.lines().any(|l| l.trim() == key);
    match (ok, listed) {
        (true, true) => panic!("FIXED, remove from known-differences.txt: {key}"),
        (false, false) => panic!("NEW difference {key}: {detail}"),
        (false, true) => eprintln!("known difference {key}: {detail}"),
        (true, false) => {}
    }
}

/// With `ULTAR_CONFORMANCE_OUT` set, writes the current picture for the side-by-side report.
fn dump(ui: &mut Ui, name: &str) {
    if let Ok(dir) = std::env::var("ULTAR_CONFORMANCE_OUT") {
        let canvas = ultar_ui::snapshot::render(ui).unwrap();
        let png = ultar_ui::snapshot::Image::from_canvas(&canvas)
            .encode_png()
            .unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}-toolkit.png")),
            png,
        )
        .unwrap();
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= TOL
}

fn kinds(r: &Response) -> Vec<ActionKind> {
    r.actions.iter().map(|a| a.kind).collect()
}

fn ui_ar(size: Size) -> Ui {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, size);
    ui.set_root_flex(
        Flex::column()
            .main(MainAlign::Start)
            .cross(CrossAlign::Start)
            .padding(Padding::all(Space::S32)),
    );
    ui
}

/// The "File" menu of the board "Menus": new, open, save (disabled), separator, delete (danger).
fn file_menu() -> Menu {
    let d = Label::data;
    Menu::new(vec![
        MenuItem::new(1, d("جديد"))
            .icon(IconId::Plus)
            .shortcut(d("Ctrl+N")),
        MenuItem::new(2, d("فتح…"))
            .icon(IconId::Folder)
            .shortcut(d("Ctrl+O")),
        MenuItem::new(3, d("حفظ"))
            .icon(IconId::Download)
            .shortcut(d("Ctrl+S"))
            .disabled(),
        MenuItem::new(4, d("حذف"))
            .icon(IconId::Trash)
            .shortcut(d("Del"))
            .danger()
            .separated(),
    ])
}

fn with_menu_button() -> (Ui, NodeId) {
    let mut ui = ui_ar(Size::new(1280.0, 800.0));
    let root = ui.root();
    let button = ui
        .add(
            root,
            Button::new(Label::data("ملف")).variant(ButtonVariant::Quiet),
        )
        .unwrap();
    (ui, button)
}

/// menu.open: the click opens the menu below the button; a second click or Esc closes it and
/// the focus returns to the button. Board size of the open menu: 224 × 146.
#[test]
fn menu_open_click_and_size() {
    let (mut ui, button) = with_menu_button();
    let r = click(&mut ui, button);
    assert!(
        kinds(&r).contains(&ActionKind::Activated),
        "click activates"
    );
    ui.open_menu(button, file_menu()).unwrap();
    let rect = ui.menu_rect(0).expect("menu is open");
    let b = ui.rect(button).unwrap();
    assert!(rect.y >= b.y + b.h - 1.0, "menu opens below the button");
    assert!(
        near(rect.x + rect.w, b.x + b.w),
        "Arabic: menu aligns with the start (right) edge of its button: menu {:?} button {:?}",
        rect,
        b
    );
    let size = (rect.w, rect.h);
    dump(&mut ui, "file-menu");
    // Second click on the owner closes it.
    let r = click(&mut ui, button);
    assert!(
        ui.menu_rect(0).is_none(),
        "second click closes the menu ({:?})",
        kinds(&r)
    );
    // Esc closes and returns the focus.
    ui.open_menu(button, file_menu()).unwrap();
    key(&mut ui, Key::Escape);
    assert!(ui.menu_rect(0).is_none(), "Esc closes the menu");
    assert_eq!(ui.focused(), Some(button), "focus returns to the button");
    expect(
        "menu.open/size",
        near(size.0, 224.0) && near(size.1, 146.0),
        format!("menu board 224 x 146, toolkit {} x {}", size.0, size.1),
    );
}

/// menu.item: a click runs the command and closes every level; a disabled item does nothing.
#[test]
fn menu_item_click_runs_and_closes() {
    let (mut ui, button) = with_menu_button();
    ui.open_menu(button, file_menu()).unwrap();
    let rect = ui.menu_rect(0).unwrap();
    // Third row (disabled "save"): rows are 32 high after the 4-point inset (board: 8 + 32 per row).
    let row = |i: f32| Point::new(rect.x + rect.w / 2.0, rect.y + 4.0 + 32.0 * i + 16.0);
    ui.handle(Event::PointerMove { pos: row(2.0) });
    common::press(&mut ui, row(2.0));
    let r = common::release(&mut ui, row(2.0));
    assert!(
        !kinds(&r)
            .iter()
            .any(|k| matches!(k, ActionKind::MenuChosen(_))),
        "a disabled item runs nothing"
    );
    assert!(
        ui.menu_rect(0).is_some(),
        "menu stays open after a disabled item"
    );
    ui.handle(Event::PointerMove { pos: row(0.0) });
    common::press(&mut ui, row(0.0));
    let r = common::release(&mut ui, row(0.0));
    assert!(
        kinds(&r).contains(&ActionKind::MenuChosen(1)),
        "first item chosen: {:?}",
        kinds(&r)
    );
    assert!(ui.menu_rect(0).is_none(), "menu closed after choosing");
}

/// menu.item with a submenu opens it (Enter / arrow towards the end side) instead of running.
#[test]
fn menu_item_with_submenu_opens_it() {
    let (mut ui, button) = with_menu_button();
    let d = Label::data;
    let sort = Menu::new(vec![
        MenuItem::new(11, d("الاسم")).checked(true),
        MenuItem::new(12, d("التاريخ")),
        MenuItem::new(13, d("الحجم")),
    ]);
    let menu = Menu::new(vec![
        MenuItem::new(1, d("نسخ"))
            .icon(IconId::Copy)
            .shortcut(d("Ctrl+C")),
        MenuItem::new(2, d("ترتيب حسب")).submenu(sort),
        MenuItem::new(3, d("عرض المخفيّة")).checked(true).separated(),
    ]);
    let pos = center(&mut ui, button);
    ui.open_context_menu(button, pos, menu).unwrap();
    key(&mut ui, Key::ArrowDown);
    key(&mut ui, Key::ArrowDown);
    let r = key(&mut ui, Key::Enter);
    assert!(
        !kinds(&r)
            .iter()
            .any(|k| matches!(k, ActionKind::MenuChosen(_))),
        "Enter on a submenu item opens it, runs nothing"
    );
    let sub = ui.menu_rect(1).expect("submenu open");
    let top = ui.menu_rect(0).unwrap();
    dump(&mut ui, "context-menu");
    eprintln!("context menu {top:?} submenu {sub:?} at {pos:?}");
    // RTL: the submenu opens towards the end side = to the left of the menu.
    assert!(
        sub.x + sub.w / 2.0 < top.x,
        "submenu opens towards the end side (left in Arabic)"
    );
    expect(
        "menu.item/submenu-gap",
        sub.x + sub.w <= top.x - 2.0,
        format!(
            "board: 4 pt gap between menu and submenu; toolkit overlaps by {}",
            sub.x + sub.w - top.x
        ),
    );
    expect(
        "menu.item/width",
        near(sub.w, 160.0) && near(top.w, 224.0),
        format!("board widths 224 / 160, toolkit {} / {}", top.w, sub.w),
    );
}

/// select.option: the click chooses the option and closes the list; Esc closes without change.
#[test]
fn select_option_click() {
    let mut ui = ui_ar(Size::new(600.0, 400.0));
    let root = ui.root();
    let d = Label::data;
    let select = ui
        .add_sized(
            root,
            Select::new(
                d("معدّل التحديث"),
                vec![d("60 Hz"), d("120 Hz"), d("144 Hz")],
            )
            .selected(1),
            Sizing::width(160.0),
        )
        .unwrap();
    let r = click(&mut ui, select);
    assert!(ui.is_open(select), "click opens the list: {:?}", kinds(&r));
    key(&mut ui, Key::Escape);
    assert!(!ui.is_open(select), "Esc closes");
    assert_eq!(ui.selected(select).unwrap(), 1, "Esc keeps the choice");
    click(&mut ui, select);
    key(&mut ui, Key::ArrowDown);
    let r = key(&mut ui, Key::Enter);
    assert!(
        kinds(&r).contains(&ActionKind::Selected(2)),
        "arrow + Enter chooses: {:?}",
        kinds(&r)
    );
    assert!(!ui.is_open(select), "the list closes after choosing");
    let b = ui.rect(select).unwrap();
    assert!(near(b.h, 32.0), "select height board 32, toolkit {}", b.h);
}

/// tabs.tab: click shows the page; arrows move in reading direction; Home/End.
#[test]
fn tabs_tab_click_and_keys() {
    let mut ui = common::ui(Lang::En, ThemeKind::Light, Size::new(600.0, 200.0));
    ui.set_root_flex(Flex::column().cross(CrossAlign::Start));
    let root = ui.root();
    let d = Label::data;
    let tabs = ui
        .add(
            root,
            Tabs::new(
                d("Settings pages"),
                vec![d("General"), d("Display"), d("Sound"), d("Network")],
            ),
        )
        .unwrap();
    ui.layout();
    dump(&mut ui, "tabs");
    let rect = ui.rect(tabs).unwrap();
    // Click on the third tab (LTR: x grows).
    let pos = Point::new(rect.x + 197.2 + 37.0, rect.y + rect.h / 2.0);
    ui.handle(Event::PointerMove { pos });
    common::press(&mut ui, pos);
    let r = common::release(&mut ui, pos);
    assert!(
        kinds(&r).contains(&ActionKind::Selected(2)),
        "click on Sound shows it (board geometry): {:?}",
        kinds(&r)
    );
    let r = key(&mut ui, Key::ArrowRight);
    assert!(
        kinds(&r).contains(&ActionKind::Selected(3)),
        "arrow right in English moves on"
    );
    let r = key(&mut ui, Key::Home);
    assert!(
        kinds(&r).contains(&ActionKind::Selected(0)),
        "Home = first tab"
    );
    let r = key(&mut ui, Key::End);
    assert!(
        kinds(&r).contains(&ActionKind::Selected(3)),
        "End = last tab"
    );
    assert!(
        near(rect.h, 33.0) || near(rect.h, 32.0),
        "tab strip height board 33, toolkit {}",
        rect.h
    );
    expect(
        "tabs.tab/width",
        near(rect.w, 327.72),
        format!("tab strip width board 327.7, toolkit {}", rect.w),
    );
}

/// tabs.tab in Arabic: the arrow pointing to the start side (right) goes back.
#[test]
fn tabs_arrows_follow_reading_direction_in_arabic() {
    let mut ui = ui_ar(Size::new(600.0, 200.0));
    let root = ui.root();
    let d = Label::data;
    let tabs = ui
        .add(
            root,
            Tabs::new(d("صفحات"), vec![d("عام"), d("الشاشة"), d("الصوت")]),
        )
        .unwrap();
    while ui.focused() != Some(tabs) {
        key(&mut ui, Key::Tab);
    }
    let r = key(&mut ui, Key::ArrowLeft);
    assert!(
        kinds(&r).contains(&ActionKind::Selected(1)),
        "ArrowLeft in Arabic = next tab: {:?}",
        kinds(&r)
    );
}

/// sheet.close: Esc or a press on the dimmed part closes the sheet; the focus returns to the
/// control that opened it.
#[test]
fn sheet_close_returns_focus() {
    let mut ui = common::ui(Lang::En, ThemeKind::Light, Size::new(360.0, 640.0));
    let root = ui.root();
    let opener = ui
        .add(root, IconButton::new(IconId::Sharing, Label::data("Share")))
        .unwrap();
    let sheet = ui.add(root, Dialog::sheet(Label::data("Share"))).unwrap();
    ui.add(
        sheet,
        Button::new(Label::data("Copy link"))
            .icon(IconId::Copy)
            .variant(ButtonVariant::Primary),
    )
    .unwrap();
    let close = ui.add(sheet, Button::new(Label::data("Close"))).unwrap();
    click(&mut ui, opener);
    ui.open_dialog(sheet).unwrap();
    assert!(ui.is_dialog_open(sheet));
    dump(&mut ui, "sheet");
    let r = key(&mut ui, Key::Escape);
    assert!(
        kinds(&r).contains(&ActionKind::Closed),
        "Esc closes the sheet"
    );
    assert!(!ui.is_dialog_open(sheet));
    assert_eq!(ui.focused(), Some(opener), "focus returns to the opener");
    // A press on the dimmed part (top of the surface) closes it too.
    ui.open_dialog(sheet).unwrap();
    let r = common::press(&mut ui, Point::new(180.0, 10.0));
    common::release(&mut ui, Point::new(180.0, 10.0));
    assert!(
        kinds(&r).contains(&ActionKind::Closed) || !ui.is_dialog_open(sheet),
        "press on the scrim closes the sheet"
    );
    // The close button inside emits Activated for the application to close it.
    ui.open_dialog(sheet).unwrap();
    let r = click(&mut ui, close);
    assert!(kinds(&r).contains(&ActionKind::Activated));
    let b = ui.rect(close).unwrap();
    assert!(
        near(b.h, 32.0),
        "sheet button height board 32, toolkit {}",
        b.h
    );
}

fn toast_ui() -> Ui {
    let mut ui = common::ui(Lang::En, ThemeKind::Light, Size::new(1280.0, 800.0));
    ui.set_background(ColorRole::Ground);
    ui.show_toast(
        Toast::new(7, Tone::Success, Label::data("3 files moved to the Trash."))
            .action(Label::data("Undo")),
    )
    .unwrap();
    // A toast is hit-tested where it was last painted.
    ultar_ui::snapshot::render(&mut ui).unwrap();
    ui
}

/// Rect of the toast's status node and of its buttons from the accessibility tree.
fn toast_parts(ui: &mut Ui) -> Vec<ultar_ui::Rect> {
    let tree = ui.a11y_tree();
    let status = tree
        .iter()
        .position(|n| {
            n.name
                .as_ref()
                .is_some_and(|l| l.resolve(ui.translator()).contains("Trash"))
        })
        .expect("toast in the accessibility tree");
    tree[status..].iter().take(3).map(|n| n.rect).collect()
}

/// toast.action: the action runs and the toast closes; Ctrl+Enter does the same.
#[test]
fn toast_action_click_and_keyboard() {
    let mut ui = toast_ui();
    dump(&mut ui, "toast");
    let parts = toast_parts(&mut ui);
    assert_eq!(parts.len(), 3, "status, action, close");
    assert!(
        near(parts[0].w, 400.0) && near(parts[0].h, 48.0),
        "toast board 400 x 48, toolkit {} x {}",
        parts[0].w,
        parts[0].h
    );
    let pos = parts[1].center();
    let down = kinds(&common::press(&mut ui, pos));
    let up = kinds(&common::release(&mut ui, pos));
    let all: Vec<_> = down.iter().chain(up.iter()).copied().collect();
    assert!(
        all.contains(&ActionKind::ToastAction(7)),
        "action runs: {all:?}"
    );
    expect(
        "toast.action/on-release",
        !down.contains(&ActionKind::ToastAction(7)),
        "the toast action runs on press, every other button on release (a press cannot be taken back)".into(),
    );
    assert!(ui.toasts().is_empty(), "toast closed after its action");
    let mut ui = toast_ui();
    let r = ui.handle(Event::Key {
        key: Key::Enter,
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
    });
    assert!(
        kinds(&r).contains(&ActionKind::ToastAction(7)),
        "Ctrl+Enter runs it"
    );
}

/// toast.close: the close button and Esc close it; otherwise it hides after motion.toast.
#[test]
fn toast_close_click_escape_and_timeout() {
    let mut ui = toast_ui();
    let parts = toast_parts(&mut ui);
    let pos = parts[2].center();
    common::press(&mut ui, pos);
    common::release(&mut ui, pos);
    assert!(ui.toasts().is_empty(), "close button closes");
    let mut ui = toast_ui();
    let r = key(&mut ui, Key::Escape);
    assert!(
        kinds(&r).contains(&ActionKind::ToastClosed(7)),
        "Esc closes the newest"
    );
    let mut ui = toast_ui();
    ui.advance_to(4_001);
    let closed = ui.take_actions();
    expect(
        "toast.close/timeout",
        closed.iter().any(|a| a.kind == ActionKind::ToastClosed(7)) || ui.toasts().is_empty(),
        "CLICKS.md: a toast closes by itself after motion.toast; the toolkit keeps toasts with an action open".into(),
    );
}
