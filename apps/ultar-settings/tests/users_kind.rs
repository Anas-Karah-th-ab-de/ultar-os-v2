//! users.new_kind: the Users page proposes the configured kind when adding a user –
//! the dialog says it and the account service gets it; an invalid value means standard.

mod common;

use common::*;
use ultar_bus::messages::account_local::{PictureInfo, UserKind};
use ultar_bus::messages::settings::SettingValue;
use ultar_i18n::Lang;
use ultar_settings::action::Act;
use ultar_settings::service::fake::Shared;
use ultar_settings::service::system::LocalUser;
use ultar_settings::App;
use ultar_ui::tokens::ThemeKind;

fn world() -> Shared {
    let world = board_world(3, true);
    world.borrow_mut().system.users = Some(vec![LocalUser {
        name: "anas".into(),
        uid: 1000,
        display_name: "anas".into(),
        kind: UserKind::Administrator,
        picture: PictureInfo::Initials,
        auto_login: false,
        current: true,
    }]);
    world
}

fn wide(world: &Shared) -> App {
    app(
        world,
        Lang::En,
        ThemeKind::Dark,
        content_size(1440.0, 900.0),
        "users",
    )
}

/// Every name, value and description of the screen-reader tree, translated.
fn texts(app: &mut App) -> Vec<String> {
    let tree = app.ui.a11y_tree();
    let tr = app.ui.translator().clone();
    tree.iter()
        .flat_map(|n| [n.name.clone(), n.value.clone(), n.description.clone()])
        .flatten()
        .map(|l| l.resolve(&tr).replace(['\u{2068}', '\u{2069}'], ""))
        .collect()
}

#[test]
fn the_new_user_kind_follows_the_setting() {
    let world = world();
    let set_kind = |value: &str| {
        let mut w = world.borrow_mut();
        let panels = w.panels.as_mut().expect("panels");
        let users = panels.iter_mut().find(|p| p.id == "users").expect("users");
        let item = users
            .items
            .iter_mut()
            .find(|i| i.key == "users.new_kind")
            .expect("new_kind");
        item.value = SettingValue::Text(value.into());
    };
    set_kind("administrator");
    let mut app = wide(&world);
    assert_eq!(app.state.new_user_kind(), UserKind::Administrator);
    let add = app.view.node(Act::UserAdd).expect("add user");
    click(&mut app, add);
    let shown = texts(&mut app);
    assert!(
        shown.iter().any(|t| t.contains("administrator rights")),
        "the dialog says the kind: {shown:?}"
    );
    for (field, text) in [(0, "omar"), (1, "sesame-1"), (2, "sesame-1")] {
        let node = app.view.node(Act::DialogField(field)).expect("field");
        click(&mut app, node);
        type_text(&mut app, text);
    }
    let ok = app.view.node(Act::DialogOk).expect("ok");
    click(&mut app, ok);
    assert_eq!(
        log(&world).last().expect("call"),
        "account.user_create omar"
    );
    let omar = app
        .state
        .more
        .users
        .iter()
        .find(|u| u.name == "omar")
        .expect("omar");
    assert_eq!(omar.kind, UserKind::Administrator);
    // Back to standard, and an invalid value falls back to standard too.
    set_kind("standard");
    let app = wide(&world);
    assert_eq!(app.state.new_user_kind(), UserKind::Standard);
    set_kind("root");
    let app = wide(&world);
    assert_eq!(app.state.new_user_kind(), UserKind::Standard);
}
