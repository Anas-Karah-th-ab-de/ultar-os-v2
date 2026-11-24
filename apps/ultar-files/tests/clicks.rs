//! Every `files.*` and `desk.*` entry of docs/ui/CLICKS.md, clicked like a user would,
//! with the effect checked (real file system, fake launcher and drive service).
mod common;

use common::*;
use ultar_files::action::{Command, Target};
use ultar_files::fs::{AreaId, Loc};
use ultar_files::model::Dialog;
use ultar_files::Mode;
use ultar_i18n::Lang;
use ultar_ui::event::{ActionKind, Key, Modifiers, PointerButton};

fn board_root() -> Root {
    let r = Root::new();
    for d in ["مشاريع", "عقود", "فواتير"] {
        std::fs::create_dir_all(r.path(&format!("documents/{d}"))).unwrap();
    }
    r.write("documents/تقرير.docx", b"docx");
    r.write("documents/ميزانية.xlsx", b"xlsx");
    r.write("documents/مخطط.png", b"not really a png");
    r
}

fn docs(app: &mut ultar_files::App) {
    let n = node(app, &Target::Place(1));
    click(app, n);
    assert_eq!(app.state.folder, Loc::root(AreaId::Documents));
}

#[test]
fn files_place_opens_each_place_in_this_window() {
    let r = board_root();
    let (mut app, _) = app(&r, Mode::Window, Lang::Ar, None);
    let places = app.state.places.clone();
    assert_eq!(
        places[..5],
        [
            AreaId::Home,
            AreaId::Documents,
            AreaId::Downloads,
            AreaId::Pictures,
            AreaId::Music
        ]
    );
    for (i, area) in places.iter().enumerate() {
        let n = node(&app, &Target::Place(i));
        click(&mut app, n);
        assert_eq!(app.state.folder, Loc::root(area.clone()));
    }
}

#[test]
fn files_drive_and_files_eject() {
    let r = board_root();
    let (mut app, fake) = app(&r, Mode::Window, Lang::Ar, None);
    assert_eq!(app.state.drives.len(), 1);
    let n = node(&app, &Target::Drive(0));
    click(&mut app, n);
    assert_eq!(app.state.folder, Loc::root(AreaId::Drive("USB".into())));
    fake.borrow_mut().busy.push("sdb1".into());
    let n = node(&app, &Target::Eject(0));
    click(&mut app, n);
    assert_eq!(app.state.dialog, Some(Dialog::DriveBusy("sdb1".into())));
    fake.borrow_mut().busy.clear();
    let ok = node(&app, &Target::DialogOk);
    click(&mut app, ok);
    assert_eq!(fake.borrow().ejected, ["sdb1"]);
    assert!(app.state.drives.is_empty());
    assert_eq!(
        app.state.notice.as_ref().map(|n| n.key),
        Some("files.notice.ejected")
    );
    assert_eq!(
        app.state.folder,
        Loc::root(AreaId::Home),
        "the window leaves the ejected drive"
    );
}

#[test]
fn files_trashplace_shows_restore_and_delete_permanently() {
    let r = board_root();
    let (mut app, _) = app(&r, Mode::Window, Lang::Ar, None);
    docs(&mut app);
    let item = node(&app, &Target::Item("عقود".into()));
    click(&mut app, item);
    key(&mut app, Key::Delete, Modifiers::default());
    assert!(!r.path("documents/عقود").exists());
    let n = node(&app, &Target::TrashPlace);
    click(&mut app, n);
    assert!(app.state.in_trash());
    assert_eq!(app.state.items.len(), 1);
    let item = node(&app, &Target::Item("عقود".into()));
    click(&mut app, item);
    act(
        &mut app,
        item,
        ActionKind::MenuChosen(Command::Restore.id()),
    );
    assert!(r.path("documents/عقود").is_dir());
    assert!(app.state.items.is_empty());
    // delete permanently asks first
    docs(&mut app);
    let item = node(&app, &Target::Item("فواتير".into()));
    click(&mut app, item);
    act(&mut app, item, ActionKind::MenuChosen(Command::Trash.id()));
    let n = node(&app, &Target::TrashPlace);
    click(&mut app, n);
    let item = node(&app, &Target::Item("فواتير".into()));
    click(&mut app, item);
    act(&mut app, item, ActionKind::MenuChosen(Command::Purge.id()));
    assert!(matches!(app.state.dialog, Some(Dialog::ConfirmPurge(_))));
    let ok = node(&app, &Target::DialogOk);
    click(&mut app, ok);
    assert!(app.state.items.is_empty() && !r.path("documents/فواتير").exists());
}

#[test]
fn empty_trash_asks_first() {
    let r = board_root();
    let (mut app, _) = app(&r, Mode::Window, Lang::En, None);
    docs(&mut app);
    key(&mut app, Key::Char('a'), ctrl());
    assert_eq!(app.state.selected.len(), 6);
    key(&mut app, Key::Delete, Modifiers::default());
    let n = node(&app, &Target::TrashPlace);
    click(&mut app, n);
    assert_eq!(app.state.items.len(), 6);
    let empty = node(&app, &Target::EmptyTrash);
    click(&mut app, empty);
    assert_eq!(app.state.dialog, Some(Dialog::ConfirmEmpty));
    let cancel = node(&app, &Target::DialogCancel);
    click(&mut app, cancel);
    assert_eq!(app.state.items.len(), 6, "cancel keeps everything");
    let empty = node(&app, &Target::EmptyTrash);
    click(&mut app, empty);
    let ok = node(&app, &Target::DialogOk);
    click(&mut app, ok);
    assert!(app.state.items.is_empty());
}

#[test]
fn files_search_filters_by_name_and_content_and_esc_clears() {
    let r = board_root();
    r.write("documents/ملاحظة.txt", "الميزانية السنوية".as_bytes());
    let (mut app, _) = app(&r, Mode::Window, Lang::Ar, None);
    docs(&mut app);
    let field = node(&app, &Target::Search);
    click(&mut app, field);
    for c in "ميزانية".chars() {
        key(&mut app, Key::Char(c), Modifiers::default());
    }
    let mut hits = app.state.hits.clone().unwrap();
    hits.sort();
    assert_eq!(
        hits,
        ["ملاحظة.txt", "ميزانية.xlsx"],
        "name and content at once"
    );
    assert_eq!(app.built().item_nodes.len(), 2);
    key(&mut app, Key::Escape, Modifiers::default());
    assert!(app.state.query.is_empty() && app.state.hits.is_none());
    assert_eq!(app.built().item_nodes.len(), 7);
}

#[test]
fn files_newfolder_creates_and_selects_the_name_for_typing() {
    let r = board_root();
    let (mut app, _) = app(&r, Mode::Window, Lang::Ar, None);
    docs(&mut app);
    let n = node(&app, &Target::NewFolder);
    click(&mut app, n);
    assert!(r.path("documents/مجلد جديد").is_dir());
    assert!(matches!(app.state.dialog, Some(Dialog::Rename(_))));
    let field = app.built().rename.unwrap();
    assert_eq!(app.ui.focused(), Some(field));
    app.ui.set_text(field, "أرشيف 2026").unwrap();
    let ok = node(&app, &Target::DialogOk);
    click(&mut app, ok);
    assert!(r.path("documents/أرشيف 2026").is_dir());
    assert_eq!(app.state.selected, ["أرشيف 2026"]);
    // a bad name keeps the dialog with an error
    key(
        &mut app,
        Key::Char('n'),
        Modifiers {
            ctrl: true,
            shift: true,
            ..Modifiers::default()
        },
    );
    assert!(r.path("documents/مجلد جديد").is_dir(), "Ctrl+Shift+N");
    let field = app.built().rename.unwrap();
    app.ui.set_text(field, "a/b").unwrap();
    let ok = node(&app, &Target::DialogOk);
    click(&mut app, ok);
    assert!(matches!(app.state.dialog, Some(Dialog::Rename(_))));
}

#[test]
fn files_item_selects_opens_and_has_a_context_menu() {
    let r = board_root();
    let (mut app, fake) = app(&r, Mode::Window, Lang::Ar, None);
    docs(&mut app);
    let names: Vec<String> = app
        .built()
        .item_nodes
        .iter()
        .map(|(_, n)| n.clone())
        .collect();
    for n in [
        "مشاريع",
        "عقود",
        "فواتير",
        "تقرير.docx",
        "ميزانية.xlsx",
        "مخطط.png",
    ] {
        assert!(names.contains(&n.to_owned()), "{n}");
    }
    // single click selects; Ctrl adds
    let a = node(&app, &Target::Item("تقرير.docx".into()));
    let b = node(&app, &Target::Item("ميزانية.xlsx".into()));
    click(&mut app, a);
    assert_eq!(app.state.selected, ["تقرير.docx"]);
    click_with(&mut app, b, ctrl(), PointerButton::Primary);
    let mut sel = app.state.selected.clone();
    sel.sort();
    assert_eq!(sel, ["تقرير.docx", "ميزانية.xlsx"]);
    // double click on a file: its default app with a files:// URI
    let a = node(&app, &Target::Item("تقرير.docx".into()));
    double_click(&mut app, a);
    assert_eq!(
        fake.borrow().opened.last().unwrap(),
        &(
            "org.ultar.office".to_owned(),
            "files://documents/تقرير.docx".to_owned()
        )
    );
    // the app is missing: the Store offers it
    fake.borrow_mut().installed = vec!["org.ultar.store".into()];
    let p = node(&app, &Target::Item("مخطط.png".into()));
    double_click(&mut app, p);
    assert_eq!(
        fake.borrow().opened.last().unwrap(),
        &(
            "org.ultar.store".to_owned(),
            "store://app/org.ultar.images".to_owned()
        )
    );
    // Enter on a folder opens it in this window
    let f = node(&app, &Target::Item("مشاريع".into()));
    click(&mut app, f);
    app.ui.focus(f).unwrap();
    key(&mut app, Key::Enter, Modifiers::default());
    assert_eq!(
        app.state.folder,
        Loc::root(AreaId::Documents).child("مشاريع").unwrap()
    );
}

#[test]
fn files_item_right_click_opens_its_context_menu() {
    let r = board_root();
    let (mut app, _) = app(&r, Mode::Window, Lang::Ar, None);
    docs(&mut app);
    // right click opens the context menu of the item
    let f = node(&app, &Target::Item("عقود".into()));
    click_with(&mut app, f, Modifiers::default(), PointerButton::Secondary);
    assert_eq!(app.ui.menu_owner(), Some(f));
    act(
        &mut app,
        f,
        ActionKind::MenuChosen(Command::Properties.id()),
    );
    assert!(matches!(app.state.dialog, Some(Dialog::Properties(_))));
}

#[test]
fn files_back_and_forward() {
    let r = board_root();
    let (mut app, _) = app(&r, Mode::Window, Lang::Ar, None);
    let back = node(&app, &Target::Back);
    assert!(app.ui.is_disabled(back), "no history yet");
    assert!(app.ui.is_disabled(node(&app, &Target::Forward)));
    docs(&mut app);
    let back = node(&app, &Target::Back);
    click(&mut app, back);
    assert_eq!(app.state.folder, Loc::root(AreaId::Home));
    let fwd = node(&app, &Target::Forward);
    assert!(!app.ui.is_disabled(fwd));
    click(&mut app, fwd);
    assert_eq!(app.state.folder, Loc::root(AreaId::Documents));
    key(
        &mut app,
        Key::ArrowLeft,
        Modifiers {
            alt: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(app.state.folder, Loc::root(AreaId::Home));
    key(
        &mut app,
        Key::ArrowRight,
        Modifiers {
            alt: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(app.state.folder, Loc::root(AreaId::Documents));
}

#[test]
fn copy_paste_conflict_and_undo_from_the_toast() {
    let r = board_root();
    let (mut app, _) = app(&r, Mode::Window, Lang::Ar, None);
    docs(&mut app);
    let a = node(&app, &Target::Item("تقرير.docx".into()));
    click(&mut app, a);
    key(&mut app, Key::Char('c'), ctrl());
    key(&mut app, Key::Char('v'), ctrl());
    // pasting into the same folder: the name is taken
    assert!(matches!(app.state.dialog, Some(Dialog::Conflict(..))));
    let keep = node(&app, &Target::DialogAlt);
    click(&mut app, keep);
    assert!(r.path("documents/تقرير 2.docx").exists());
    assert!(app.state.notice.as_ref().is_some_and(|n| n.undo));
    let toast_node = app.ui.root();
    act(&mut app, toast_node, ActionKind::ToastAction(1));
    assert!(!r.path("documents/تقرير 2.docx").exists(), "undo of a copy");
    // cut + paste into another place moves
    let a = node(&app, &Target::Item("ميزانية.xlsx".into()));
    click(&mut app, a);
    key(&mut app, Key::Char('x'), ctrl());
    let n = node(&app, &Target::Place(2));
    click(&mut app, n);
    key(&mut app, Key::Char('v'), ctrl());
    assert!(
        r.path("downloads/ميزانية.xlsx").exists() && !r.path("documents/ميزانية.xlsx").exists()
    );
    key(&mut app, Key::Char('z'), ctrl());
    assert!(
        r.path("documents/ميزانية.xlsx").exists(),
        "Ctrl+Z moves it back"
    );
}

#[test]
fn drag_and_drop_moves_into_a_folder() {
    let r = board_root();
    let (mut app, _) = app(&r, Mode::Window, Lang::Ar, None);
    docs(&mut app);
    let a = node(&app, &Target::Item("تقرير.docx".into()));
    click(&mut app, a);
    let to = node(&app, &Target::Item("مشاريع".into()));
    act(
        &mut app,
        a,
        ActionKind::DragEnded {
            delta: ultar_ui::Point::new(10.0, 0.0),
            target: Some(to),
        },
    );
    assert!(r.path("documents/مشاريع/تقرير.docx").exists());
}

#[test]
fn permission_errors_are_shown_clearly() {
    if rustix::process::geteuid().is_root() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let r = board_root();
    std::fs::set_permissions(
        r.path("documents/عقود"),
        std::fs::Permissions::from_mode(0o000),
    )
    .unwrap();
    let (mut app, _) = app(&r, Mode::Window, Lang::Ar, None);
    docs(&mut app);
    let f = node(&app, &Target::Item("عقود".into()));
    double_click(&mut app, f);
    assert_eq!(app.state.error, Some(ultar_files::fs::FsError::Permission));
    std::fs::set_permissions(
        r.path("documents/عقود"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
}

#[test]
fn desk_icon_selects_opens_and_has_its_menu() {
    let r = board_root();
    let (mut app, fake) = app(&r, Mode::Desk, Lang::Ar, None);
    let home = node(&app, &Target::Place(0));
    click(&mut app, home);
    assert!(app.ui.is_item_selected(home).unwrap());
    double_click(&mut app, home);
    assert_eq!(
        fake.borrow().opened.last().unwrap(),
        &("org.ultar.files".to_owned(), "files://home/".to_owned())
    );
    let usb = node(&app, &Target::Drive(0));
    double_click(&mut app, usb);
    let usb = node(&app, &Target::Drive(0));
    assert_eq!(
        fake.borrow().opened.last().unwrap().1,
        "file:///run/ultar/media/u/USB"
    );
    click_with(
        &mut app,
        usb,
        Modifiers::default(),
        PointerButton::Secondary,
    );
    assert_eq!(app.ui.menu_owner(), Some(usb));
    act(&mut app, usb, ActionKind::MenuChosen(Command::Eject.id()));
    assert_eq!(fake.borrow().ejected, ["sdb1"]);
    // an item of the desktop folder: rename and move to trash
    r.write("desktop/ملاحظة.txt", b"x");
    app.reload();
    app.rebuild().unwrap();
    let item = node(&app, &Target::Item("ملاحظة.txt".into()));
    act(&mut app, item, ActionKind::MenuChosen(Command::Trash.id()));
    assert!(!r.path("desktop/ملاحظة.txt").exists());
}
