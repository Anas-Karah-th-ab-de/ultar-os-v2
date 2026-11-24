//! Unit tests of the menu geometry and stepping.

use super::*;

fn item(id: u32) -> MenuItem {
    MenuItem::new(id, Label::data(&id.to_string()))
}

#[test]
fn limits_levels_and_stepping() {
    let sub = Menu::new(vec![item(10), item(11)]);
    let menu = Menu::new(vec![
        item(1).disabled(),
        item(2).submenu(sub.clone()),
        item(3).separated().danger().checked(true),
    ]);
    assert!(menu.within_limits());
    assert_eq!(menu.level(&[]), Some(&menu));
    assert_eq!(menu.level(&[1]), Some(&sub));
    assert_eq!(menu.level(&[0]), None);
    assert_eq!(menu.level(&[9]), None);
    assert_eq!(menu.step(None, true), Some(1), "disabled items are skipped");
    assert_eq!(menu.step(Some(1), true), Some(2));
    assert_eq!(
        menu.step(Some(2), true),
        Some(1),
        "wraps past the disabled one"
    );
    assert_eq!(menu.step(Some(1), false), Some(2));
    assert_eq!(menu.step(None, false), Some(2));
    let dead = Menu::new(vec![item(1).disabled()]);
    assert_eq!(dead.step(None, true), None);
    assert_eq!(Menu::default().step(None, true), None);
    assert!(
        !Menu::default().within_limits(),
        "an empty menu cannot open"
    );
    let many = Menu::new((0..=MAX_MENU_ITEMS as u32).map(item).collect());
    assert!(!many.within_limits());
    let mut deep = Menu::new(vec![item(0)]);
    for _ in 0..MAX_MENU_DEPTH {
        deep = Menu::new(vec![item(0).submenu(deep)]);
    }
    assert!(!deep.within_limits());
    assert_eq!(fit(5.0, 0.0, 3.0), 3.0);
    assert_eq!(fit(5.0, 8.0, 3.0), 8.0);
}
