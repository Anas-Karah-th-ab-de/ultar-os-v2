//! What the desktop asks of Files for its icons (`files://desktop/<name>?action=…`, the
//! shell's right-click menus): the item's folder opens with the item selected and the
//! command runs – the rename dialog, the properties, the trash; a new folder in the desktop
//! folder.
mod common;

use common::{app, exists, loc, Root};
use ultar_files::fs::AreaId;
use ultar_files::model::Dialog;
use ultar_files::Mode;
use ultar_i18n::Lang;

#[test]
fn rename_and_properties_open_on_the_item_in_its_folder() {
    let root = Root::new();
    root.write("desktop/Notiz.txt", b"x");
    let (a, _) = app(
        &root,
        Mode::Window,
        Lang::En,
        Some("files://desktop/Notiz.txt?action=rename"),
    );
    let item = loc(AreaId::Desktop, &["Notiz.txt"]);
    assert_eq!(a.state.folder, loc(AreaId::Desktop, &[]));
    assert_eq!(a.state.selected, vec!["Notiz.txt".to_owned()]);
    assert_eq!(a.state.dialog, Some(Dialog::Rename(item.clone())));
    let (a, _) = app(
        &root,
        Mode::Window,
        Lang::En,
        Some("files://desktop/Notiz.txt?action=properties"),
    );
    assert_eq!(a.state.dialog, Some(Dialog::Properties(item)));
    // The home folder itself: its properties.
    let (a, _) = app(
        &root,
        Mode::Window,
        Lang::En,
        Some("files://home/?action=properties"),
    );
    assert_eq!(
        a.state.dialog,
        Some(Dialog::Properties(loc(AreaId::Home, &[])))
    );
}

#[test]
fn a_missing_item_or_an_unknown_action_changes_nothing() {
    let root = Root::new();
    let (a, _) = app(
        &root,
        Mode::Window,
        Lang::En,
        Some("files://desktop/weg.txt?action=trash"),
    );
    assert_eq!(a.state.dialog, None);
    assert!(a.state.job.is_none());
    root.write("desktop/da.txt", b"x");
    let (a, _) = app(
        &root,
        Mode::Window,
        Lang::En,
        Some("files://desktop/?action=format"),
    );
    assert_eq!(a.state.folder, loc(AreaId::Desktop, &[]));
    assert_eq!(a.state.dialog, None);
    assert!(exists(&root.path("desktop/da.txt")));
}

#[test]
fn trash_starts_the_move_and_new_folder_makes_one_ready_to_name() {
    let root = Root::new();
    root.write("desktop/alt.txt", b"x");
    let (a, _) = app(
        &root,
        Mode::Window,
        Lang::En,
        Some("files://desktop/alt.txt?action=trash"),
    );
    assert!(
        a.state.job.is_some(),
        "the move to the trash runs (with undo)"
    );
    let (a, _) = app(
        &root,
        Mode::Window,
        Lang::En,
        Some("files://desktop/?action=new_folder"),
    );
    let made: Vec<_> = std::fs::read_dir(root.path("desktop"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();
    assert_eq!(made.len(), 1, "one new folder");
    assert!(
        matches!(a.state.dialog, Some(Dialog::Rename(_))),
        "its name ready to type"
    );
}
