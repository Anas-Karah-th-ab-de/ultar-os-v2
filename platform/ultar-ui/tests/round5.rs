//! Round 5 of the UI review (`docs/review/ui-runde-5.md`): node handles with generations
//! (D1), rows that follow their identity (D2), the large-area blend (D3), accessibility
//! nodes of long tables only near the view (D9), the check mark of menus and lists (note).

mod common;

use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::geom::PixelRect;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::ui::{Role, UiError};
use ultar_ui::widgets::{Button, Label, Table};
use ultar_ui::{Color, Lang, Rect, Size};

#[test]
fn a_removed_handle_never_reaches_the_node_that_reuses_its_slot() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(400.0, 300.0));
    let root = ui.root();
    let old = ui.add(root, Button::new(Label::data("old"))).expect("old");
    ui.remove(old).expect("remove");
    let new = ui.add(root, Button::new(Label::data("new"))).expect("new");
    assert_ne!(old, new, "same slot, new generation");
    assert!(matches!(
        ui.set_label(old, Label::data("x")),
        Err(UiError::UnknownNode(_))
    ));
    assert!(ui.set_label(new, Label::data("y")).is_ok());
    assert_eq!(ui.node_count(), 2);
    assert!(
        ui.remove(old).is_err(),
        "a stale handle cannot remove the new node"
    );
    assert!(ui.widget(old).is_none());
}

#[test]
fn a_long_table_has_accessibility_nodes_only_near_the_view() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(600.0, 400.0));
    let root = ui.root();
    let rows = (0..5000)
        .map(|i| vec![Label::data(&format!("file {i}")), Label::data("1 KB")])
        .collect();
    let columns = vec![Label::data("Name"), Label::data("Size")];
    let mut table = Table::new(Label::data("Files"), columns).rows(rows);
    table.selected = Some(4000);
    let id = ui.add(root, table).expect("table");
    let tree = ui.a11y_tree();
    let rows: Vec<_> = tree
        .iter()
        .filter(|n| n.id == id && n.role == Role::TableRow)
        .collect();
    assert!(rows.len() < 100, "{} row nodes", rows.len());
    assert!(rows.iter().all(|r| r.position.is_some_and(|p| p.1 == 5000)));
    assert!(
        rows.iter().any(|r| r.position == Some((4001, 5000))),
        "the selected row is always there"
    );
    assert!(rows.iter().any(|r| r.position == Some((1, 5000))));
    let cells = tree
        .iter()
        .filter(|n| n.id == id && n.role == Role::Cell)
        .count();
    assert_eq!(cells, 2 * rows.len());
}

#[test]
fn a_large_blend_equals_the_fill_of_the_same_rectangle() {
    // Above the size from which the rows are shared among threads.
    let (w, h) = (2400, 1200);
    let mut a = Canvas::new(w, h, PixelFormat::Bgrx8).expect("a");
    let mut b = Canvas::new(w, h, PixelFormat::Bgrx8).expect("b");
    for c in [&mut a, &mut b] {
        c.fill_rrect(
            Rect::new(10.0, 10.0, 900.0, 700.0),
            12.0,
            Color::BLACK.with_alpha(0.3),
        );
    }
    let dim = Color::BLACK.with_alpha(0.6);
    a.blend_rect(PixelRect::new(0, 0, w as i32, h as i32), dim);
    b.fill_rrect(Rect::new(0.0, 0.0, w as f32, h as f32), 0.0, dim);
    assert!(a.data() == b.data());
    // An area outside the buffer draws nothing and does not panic.
    a.blend_rect(PixelRect::new(-50, h as i32 + 10, -1, h as i32 + 20), dim);
}
