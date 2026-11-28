//! The other icons of the desktop background (CLICKS `desk.icon`): the home folder, the
//! drives and the desktop folder's items – their order, double click and menus (Files does
//! the changes) – an app dragged out of the app grid onto the desktop, and the pictures of
//! the background with all icons, its menus and the rename dialog in both themes and both
//! languages.

mod common;

use common::shots::*;
use common::*;
use ultar_bus::messages::volumes::Mounted;
use ultar_desktop::model::desk_files::DeskFile;
use ultar_desktop::model::places::DeskItem;
use ultar_desktop::model::services::Call;
use ultar_desktop::model::Open;
use ultar_desktop::shell::{icon_item, shortcut_item, Effect, Opening, Outcome, HOME_URI};
use ultar_desktop::view::Target;
use ultar_i18n::Lang;
use ultar_ui::event::{Event, PointerButton};
use ultar_ui::geom::{Point, Size};
use ultar_ui::snapshot::Image;
use ultar_ui::tokens::ThemeKind;

const FILES: &str = "org.ultar.files";
const BROWSER: &str = "org.ultar.browser";
const TERMINAL: &str = "org.ultar.terminal";

fn h() -> Harness {
    Harness::new(Lang::En, ThemeKind::Dark)
}

/// The board's desktop with shortcuts of Files, the browser, the terminal and Firefox.
fn with_shortcuts(lang: Lang, theme: ThemeKind) -> Harness {
    let mut h = Harness::new(lang, theme);
    for app in [FILES, BROWSER, TERMINAL, "org.mozilla.firefox"] {
        h.shell.d.places.add(app);
    }
    h.shell.d.places.selected = None;
    h
}

/// An item of the open menu chosen.
fn choose(h: &mut Harness, id: u32) {
    let out = h.shell.on_menu(id);
    h.apply(out);
}

fn shortcuts(h: &Harness) -> Vec<String> {
    h.shell
        .d
        .places
        .shortcuts
        .iter()
        .map(|s| s.app.clone())
        .collect()
}

fn with_files() -> Harness {
    with_files_in(Lang::En, ThemeKind::Dark)
}

/// The board's desktop with a drive and two items in the desktop folder.
fn with_files_in(lang: Lang, theme: ThemeKind) -> Harness {
    let mut h = with_shortcuts(lang, theme);
    h.shell.d.svc.set_drives(vec![Mounted {
        id: "sdb1".into(),
        name: "USB".into(),
        uri: "file:///run/ultar/media/u/USB".into(),
    }]);
    h.shell.d.places.files = vec![
        DeskFile {
            name: "Projekte".into(),
            folder: true,
        },
        DeskFile {
            name: "Bericht 2026.pdf".into(),
            folder: false,
        },
    ];
    h.apply(Outcome::surfaces());
    h
}

fn files_open(uri: &str) -> Vec<Effect> {
    vec![Effect::Launch {
        app: FILES.into(),
        open: Opening::uri(uri),
    }]
}

#[test]
fn home_drives_shortcuts_and_the_desktop_folder_stand_in_this_order() {
    let h = with_files();
    let v = h.background();
    let order: Vec<DeskItem> = v
        .targets
        .iter()
        .filter_map(|(_, t)| match t {
            Target::Desk(item) => Some(*item),
            _ => None,
        })
        .collect();
    assert_eq!(
        order,
        vec![
            DeskItem::Home,
            DeskItem::Drive(0),
            DeskItem::Shortcut(0),
            DeskItem::Shortcut(1),
            DeskItem::Shortcut(2),
            DeskItem::Shortcut(3),
            DeskItem::File(0),
            DeskItem::File(1),
        ]
    );
    let y = |item| v.ui.rect(v.node(&Target::Desk(item)).unwrap()).unwrap().y;
    assert!(
        y(DeskItem::Home) < y(DeskItem::Drive(0)),
        "down the first column"
    );
}

#[test]
fn a_double_click_opens_home_drive_folder_and_file() {
    let mut h = with_files();
    let open = |h: &mut Harness, item| {
        let mut bg = h.background();
        h.click_background(&mut bg, Target::Desk(item), 2);
        h.take()
    };
    assert_eq!(open(&mut h, DeskItem::Home), files_open(HOME_URI));
    assert_eq!(
        open(&mut h, DeskItem::Drive(0)),
        files_open("file:///run/ultar/media/u/USB")
    );
    assert_eq!(
        open(&mut h, DeskItem::File(0)),
        files_open("files://desktop/Projekte")
    );
    assert_eq!(
        open(&mut h, DeskItem::File(1)),
        vec![Effect::OpenFile {
            uri: "files://desktop/Bericht%202026.pdf".into(),
            name: "Bericht 2026.pdf".into(),
        }],
        "a file opens with its default app"
    );
}

#[test]
fn the_menus_of_items_drives_and_home_hand_the_change_to_files() {
    let mut h = with_files();
    let menu = |h: &mut Harness, item, id| {
        h.right_background(Some(Target::Desk(item)), (200, 200));
        assert_eq!(h.shell.d.open, Open::ItemMenu(item, 200, 200));
        choose(h, id);
        h.take()
    };
    let file = DeskItem::File(1);
    for (id, action) in [
        (icon_item::RENAME, "rename"),
        (icon_item::TRASH, "trash"),
        (icon_item::PROPERTIES, "properties"),
    ] {
        let uri = format!("files://desktop/Bericht%202026.pdf?action={action}");
        assert_eq!(menu(&mut h, file, id), files_open(&uri));
    }
    assert_eq!(
        menu(&mut h, file, icon_item::SHOW),
        files_open("files://desktop/")
    );
    assert_eq!(
        menu(&mut h, DeskItem::Drive(0), icon_item::EJECT),
        vec![Effect::Service(Call::Eject("sdb1".into()))]
    );
    assert_eq!(
        menu(&mut h, DeskItem::Home, icon_item::PROPERTIES),
        files_open("files://home/?action=properties")
    );
    assert_eq!(
        menu(&mut h, DeskItem::Home, icon_item::OPEN),
        files_open(HOME_URI)
    );
}

/// Presses an app cell of the open grid and moves it to `to` (popup points), released
/// there when `release`.
fn drag_cell(h: &mut Harness, app: &str, to: Point, release: bool) {
    let p = h.popup.as_mut().expect("the grid is open");
    let node = p.view.node(&Target::AppItem(app.into())).unwrap();
    let r = p.view.ui.rect(node).unwrap();
    let from = Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
    let button = PointerButton::Primary;
    let ui = &mut p.view.ui;
    let mut actions = ui.handle(Event::PointerMove { pos: from }).actions;
    actions.extend(ui.handle(Event::PointerDown { pos: from, button }).actions);
    for k in 1..=4 {
        let f = k as f32 / 4.0;
        let pos = Point::new(from.x + (to.x - from.x) * f, from.y + (to.y - from.y) * f);
        actions.extend(ui.handle(Event::PointerMove { pos }).actions);
    }
    if release {
        actions.extend(ui.handle(Event::PointerUp { pos: to, button }).actions);
    }
    h.dispatch_popup(actions);
}

#[test]
fn an_app_dragged_out_of_the_grid_lands_on_the_desktop() {
    let mut h = h();
    h.click_bar(Target::AllApps);
    // While it is dragged the background shows, faint, where it would go.
    drag_cell(&mut h, TERMINAL, Point::new(-60.0, 300.0), false);
    assert_eq!(h.shell.d.places.dragging.as_deref(), Some(TERMINAL));
    let ghost = h.background();
    let faint = ghost.targets.len();
    assert_eq!(faint, 1, "the ghost is not operable (home only)");
    assert!(h.popup.is_some(), "the grid stays open while dragging");
    let p = h.popup.as_mut().unwrap();
    let up = p.view.ui.handle(Event::PointerUp {
        pos: Point::new(-60.0, 300.0),
        button: PointerButton::Primary,
    });
    h.dispatch_popup(up.actions);
    assert_eq!(shortcuts(&h), vec![TERMINAL.to_owned()]);
    assert_eq!(h.shell.d.places.dragging, None);
    assert_eq!(h.shell.d.open, Open::None, "dropped: the grid closes");
    assert!(h.take().contains(&Effect::Shortcuts));
    // Dropped inside the grid's card: nothing changes.
    h.click_bar(Target::AllApps);
    let p = h.popup.as_ref().unwrap();
    let inside = {
        let node = p.view.node(&Target::SearchField).unwrap();
        let r = p.view.ui.rect(node).unwrap();
        Point::new(r.x + 10.0, r.y + 10.0)
    };
    drag_cell(&mut h, BROWSER, inside, true);
    assert_eq!(shortcuts(&h), vec![TERMINAL.to_owned()]);
    assert_eq!(h.shell.d.places.dragging, None);
    assert!(
        matches!(h.shell.d.open, Open::Apps(_)),
        "the grid stays open"
    );
}

/// The background, the bar and the popup composed like the compositor does.
fn screen(h: &mut Harness) -> Image {
    let mut img = blank_of(h.look.theme, SCREEN);
    let mut bg = h.background();
    over(&mut img, &mut bg, SCREEN, (0, 0));
    let bar_top = (SCREEN.h - 56.0) as i32;
    over(
        &mut img,
        &mut h.bar,
        Size::new(SCREEN.w, 56.0),
        (0, bar_top),
    );
    if let Some(p) = &mut h.popup {
        let size = p.size;
        let off = (p.offset.0, bar_top + p.offset.1);
        over(&mut img, &mut p.view, size, off);
    }
    img
}

#[test]
fn the_background_its_menus_and_the_rename_dialog_match_their_references() {
    let mut problems = Vec::new();
    for (lang, theme) in VARIANTS {
        let mut h = with_files_in(lang, theme);
        h.shell.d.places.selected = Some(DeskItem::Shortcut(1));
        h.apply(Outcome::surfaces());
        let img = screen(&mut h);
        check("desk-shortcuts", lang, theme, &img, &mut problems);
        let at = if lang == Lang::Ar {
            (900, 260)
        } else {
            (540, 260)
        };
        h.right_background(None, at);
        let img = screen(&mut h);
        check("desk-menu", lang, theme, &img, &mut problems);
        h.shell.close();
        h.right_background(Some(Target::Desk(DeskItem::Shortcut(2))), (130, 300));
        let img = screen(&mut h);
        check("desk-shortcut-menu", lang, theme, &img, &mut problems);
        h.shell.close();
        h.right_background(Some(Target::Desk(DeskItem::File(1))), (130, 420));
        let img = screen(&mut h);
        check("desk-file-menu", lang, theme, &img, &mut problems);
        h.shell.close();
        h.right_background(Some(Target::Desk(DeskItem::Shortcut(2))), (130, 300));
        choose(&mut h, shortcut_item::RENAME);
        let img = screen(&mut h);
        check("desk-rename", lang, theme, &img, &mut problems);
    }
    assert!(problems.is_empty(), "{problems:#?}");
}
