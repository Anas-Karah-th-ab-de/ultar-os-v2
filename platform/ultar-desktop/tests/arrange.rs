//! Dragging apps on the phone: on the home screen an app moves to another's place or into
//! the favourites row (CLICKS `mobile.app`); in all apps dropping one app onto another makes
//! a folder that opens, takes more apps and gives them back (CLICKS `apps.app`, CompactApps).

mod common;

use common::*;
use ultar_desktop::model::Open;
use ultar_desktop::shell::Outcome;
use ultar_desktop::view::Target;
use ultar_i18n::Lang;
use ultar_ui::tokens::ThemeKind;

fn app(id: &str) -> Target {
    Target::AppItem(id.to_owned())
}

fn drop_home(h: &mut Harness, from: &str, to: Target) {
    let onto = Harness::drag_in(h.home.as_mut().unwrap(), &app(from), &to);
    assert!(
        onto.is_some(),
        "{from} → {to:?}: the toolkit reports the drop target"
    );
    let out = h.shell.on_drop(&app(from), onto.as_ref());
    h.apply(out);
}

#[test]
fn home_screen_apps_move_and_go_into_the_favourites_row() {
    let mut h = Harness::phone(Lang::Ar, ThemeKind::Dark);
    let home = h.shell.d.home.apps.clone();
    assert!(home.len() >= 3, "{home:?}");
    let (first, third) = (home[0].clone(), home[2].clone());
    // Onto another app of the home screen: takes its place.
    drop_home(&mut h, &third, app(&first));
    assert_eq!(h.shell.d.home.apps[0], third);
    assert_eq!(h.shell.d.home.apps[1], first);
    // Onto a favourite: into the row at its place; the row keeps three, the last one comes
    // to the home screen.
    let fav = h.shell.d.pinned[0].clone();
    let last = h.shell.d.pinned.last().cloned().unwrap();
    drop_home(&mut h, &first, app(&fav));
    assert_eq!(h.shell.d.pinned[0], first);
    assert_eq!(h.shell.d.pinned.len(), 3);
    assert!(!h.shell.d.home.apps.contains(&first));
    assert!(h.shell.d.home.apps.contains(&last));
    // From the row back onto the home screen.
    drop_home(&mut h, &first, app(&third));
    assert!(!h.shell.d.pinned.contains(&first) && h.shell.d.home.apps.contains(&first));
    // The row's free place takes an app while it has room.
    let out = h.shell.on_drop(&app(&third), Some(&Target::Favourites));
    assert_eq!(out, Outcome::surfaces());
    assert!(h.shell.d.pinned.contains(&third));
    let out = h.shell.on_drop(&app(&first), Some(&Target::Favourites));
    assert_eq!(out, Outcome::none(), "full: no place");
    assert_eq!(h.shell.on_drop(&app(&first), None), Outcome::none());
}

#[test]
fn dropping_an_app_onto_another_in_all_apps_makes_a_folder() {
    let mut h = Harness::phone(Lang::Ar, ThemeKind::Dark);
    h.shell.d.open = Open::Apps(None);
    h.apply(Outcome::surfaces());
    let ids: Vec<String> = h.shell.d.catalog.iter().map(|a| a.id.clone()).collect();
    let (a, b, c) = (ids[0].clone(), ids[1].clone(), ids[2].clone());
    let onto = Harness::drag_in(&mut h.popup.as_mut().unwrap().view, &app(&a), &app(&b));
    assert_eq!(onto, Some(app(&b)));
    let out = h.shell.on_drop(&app(&a), onto.as_ref());
    h.apply(out);
    let folder = h.shell.d.home.folders[0].id;
    assert!(h.popup_target(&Target::Folder(folder)), "one folder cell");
    assert!(!h.popup_target(&app(&a)) && !h.popup_target(&app(&b)));
    // A third app dropped onto the folder joins it.
    let onto = Harness::drag_in(
        &mut h.popup.as_mut().unwrap().view,
        &app(&c),
        &Target::Folder(folder),
    );
    let out = h.shell.on_drop(&app(&c), onto.as_ref());
    h.apply(out);
    assert_eq!(h.shell.d.home.folders[0].apps.len(), 3);
    // It opens; its apps start as before; one goes out again.
    h.click_popup(Target::Folder(folder));
    assert_eq!(h.shell.d.home.open_folder, Some(folder));
    assert!(h.popup_target(&app(&a)) && h.popup_target(&Target::FolderOut));
    let onto = Harness::drag_in(
        &mut h.popup.as_mut().unwrap().view,
        &app(&a),
        &Target::FolderOut,
    );
    assert_eq!(onto, Some(Target::FolderOut));
    let out = h.shell.on_drop(&app(&a), onto.as_ref());
    h.apply(out);
    assert_eq!(h.shell.d.home.folders[0].apps.len(), 2);
    h.click_popup(Target::FolderClose);
    assert_eq!(h.shell.d.home.open_folder, None);
    assert!(h.popup_target(&app(&a)));
    // A search or a category shows the apps themselves.
    h.type_popup("x");
    assert!(!h.popup_target(&Target::Folder(folder)));
    // Closing all apps closes the folder too.
    h.shell.d.home.open_folder = Some(folder);
    let out = h.shell.close();
    h.apply(out);
    assert_eq!(h.shell.d.home.open_folder, None);
}
