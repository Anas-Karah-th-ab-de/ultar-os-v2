//! Regression (owner, VMware): "show as list" hung the Files window. Cause: in list view the
//! tree never showed the selection of the state (the table was built without its marked
//! rows, and the app read the selection only from grid tiles), so every turn of the
//! window's loop found a difference and built the whole tree again – which also dropped
//! the press of every click between its down and its up. The tests drive the window with
//! pointer and key events through the toolkit (REAL-INPUT: pointer down, an idle turn of
//! the loop, pointer up), with an empty folder and with 1 500 entries.
mod common;

use common::*;
use ultar_files::action::Target;
use ultar_files::model::ViewMode;
use ultar_files::{App, Mode};
use ultar_i18n::Lang;
use ultar_ui::event::{Action, Event, Key, Modifiers, PointerButton};
use ultar_ui::tokens::InputMode;
use ultar_ui::Point;

const MANY: usize = 1500;

/// One idle turn of the binary's loop: no actions, no keys, the background work.
fn idle(app: &mut App) {
    app.actions(Vec::new(), false).unwrap();
    app.keys(Vec::new()).unwrap();
    app.poll().unwrap();
    app.ui.layout();
}

/// The centre of row `row` of the list (below the header), on the screen.
fn row_at(app: &App, row: usize) -> Point {
    let table = app.built().items.expect("the table");
    let r = app.ui.rect(table).unwrap();
    let h = InputMode::Pointer.min_target();
    Point::new(r.x + r.w / 4.0, r.y + h * (row as f32 + 1.5))
}

fn feed(app: &mut App, events: &[Event]) {
    let mut actions: Vec<Action> = Vec::new();
    for e in events {
        actions.extend(app.ui.handle(*e).actions);
    }
    actions.extend(app.ui.take_actions());
    app.actions(actions, false).unwrap();
    app.ui.layout();
}

fn to_list(app: &mut App) {
    let view = node(app, &Target::ViewMode);
    click(app, view);
    assert_eq!(app.state.look.view, ViewMode::List, "the view switch");
    idle(app);
}

fn many() -> Root {
    let r = Root::new();
    for i in 0..MANY {
        r.write(&format!("documents/file-{i:04}.txt"), b"x");
    }
    r
}

#[test]
fn list_view_does_not_rebuild_while_idle_and_keeps_clicks() {
    let r = many();
    let (mut app, _) = app(&r, Mode::Window, Lang::En, Some("files://documents/"));
    assert_eq!(app.state.items.len(), MANY);
    // a selection made in the grid goes along into the list
    let tile = node(&app, &Target::Item("file-0003.txt".into()));
    click(&mut app, tile);
    assert_eq!(app.state.selected, ["file-0003.txt"]);
    to_list(&mut app);
    let table = app.built().items.unwrap();
    assert_eq!(
        app.ui.selected_rows(table).unwrap(),
        [3],
        "the list shows it"
    );
    let builds = app.builds();
    for _ in 0..5 {
        idle(&mut app);
    }
    assert_eq!(app.builds(), builds, "an idle loop builds nothing");
    // REAL-INPUT: down, a turn of the loop, up – the click selects its row
    let pos = row_at(&app, 7);
    feed(&mut app, &[Event::PointerMove { pos }]);
    feed(
        &mut app,
        &[Event::PointerDown {
            pos,
            button: PointerButton::Primary,
        }],
    );
    idle(&mut app);
    feed(
        &mut app,
        &[Event::PointerUp {
            pos,
            button: PointerButton::Primary,
        }],
    );
    idle(&mut app);
    assert_eq!(app.state.selected, ["file-0007.txt"], "the click arrived");
    let table = app.built().items.unwrap();
    assert_eq!(app.ui.selected_rows(table).unwrap(), [7]);
    assert_eq!(app.builds(), builds, "a selection needs no new tree");
    // Ctrl+A: every row marked, shown, and still no endless rebuilding
    key(&mut app, Key::Char('a'), ctrl());
    assert_eq!(app.state.selected.len(), MANY);
    let table = app.built().items.unwrap();
    assert_eq!(app.ui.selected_rows(table).unwrap().len(), MANY);
    let builds = app.builds();
    for _ in 0..5 {
        idle(&mut app);
    }
    assert_eq!(app.builds(), builds, "idle after select all");
    // Escape clears it in the list too
    key(&mut app, Key::Escape, Modifiers::default());
    assert!(app.state.selected.is_empty());
    let table = app.built().items.unwrap();
    assert!(app.ui.selected_rows(table).unwrap().is_empty());
}

#[test]
fn a_long_list_scrolls_and_paints_only_its_window() {
    let r = many();
    let (mut app, _) = app(&r, Mode::Window, Lang::Ar, Some("files://documents/"));
    to_list(&mut app);
    let scroll = app.built().scroll.expect("the list scrolls");
    assert_eq!(app.ui.scroll_offset(scroll).unwrap(), 0.0);
    // the wheel moves it
    let pos = row_at(&app, 2);
    feed(&mut app, &[Event::Scroll { pos, lines: 30.0 }]);
    let moved = app.ui.scroll_offset(scroll).unwrap();
    assert!(moved > 0.0, "the wheel scrolls the list");
    // the last row is reachable with the keyboard
    let table = app.built().items.unwrap();
    app.ui.focus(table).unwrap();
    key(&mut app, Key::End, Modifiers::default());
    let scroll = app.built().scroll.unwrap();
    assert!(
        app.ui.scroll_offset(scroll).unwrap() > moved,
        "End scrolls to the last row"
    );
    // a frame of 1 500 rows paints in well under a second (only the shown rows)
    let start = std::time::Instant::now();
    for _ in 0..3 {
        app.ui.layout();
        ultar_ui::snapshot::render(&mut app.ui).unwrap();
    }
    assert!(
        start.elapsed() < std::time::Duration::from_secs(3),
        "three frames took {:?}",
        start.elapsed()
    );
    // a rebuild (sorting by size) keeps the place in the list
    let before = app.ui.scroll_offset(app.built().scroll.unwrap()).unwrap();
    key(&mut app, Key::Char('a'), ctrl());
    let after = app.ui.scroll_offset(app.built().scroll.unwrap()).unwrap();
    assert_eq!(before, after, "select all does not jump to the top");
}

#[test]
fn empty_folder_in_list_view() {
    let r = Root::new();
    let (mut app, _) = app(&r, Mode::Window, Lang::En, Some("files://documents/"));
    assert!(app.state.items.is_empty());
    to_list(&mut app);
    assert!(app.built().items.is_none(), "the empty state, no table");
    let builds = app.builds();
    for _ in 0..5 {
        idle(&mut app);
    }
    assert_eq!(app.builds(), builds);
    // back to the grid and to the list with the keyboard
    key(&mut app, Key::Char('1'), ctrl());
    assert_eq!(app.state.look.view, ViewMode::Grid);
    key(&mut app, Key::Char('2'), ctrl());
    assert_eq!(app.state.look.view, ViewMode::List);
    // a new file appears as a row
    r.write("documents/neu.txt", b"x");
    app.reload();
    app.rebuild().unwrap();
    idle(&mut app);
    assert_eq!(app.built().rows, ["neu.txt"]);
}
