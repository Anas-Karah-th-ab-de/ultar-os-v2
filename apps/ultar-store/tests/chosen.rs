//! Components chosen at the installation (`apps.install_selection`, written by
//! `ultar-firstboot`): the Discover page names how many are still missing and their cards
//! carry the "Chosen at installation" badge; installed ones and unknown ids are not shown.

mod common;

use common::{Scene, LANGS};
use ultar_store::settings::{AppUpdateSettings, CHOSEN, MAX_CHOSEN};

/// Whether a read-out text contains `want` (the bidi isolates around arguments removed).
fn has(scene: &mut Scene, want: &str) -> bool {
    scene
        .a11y_texts()
        .iter()
        .any(|t| t.replace(['\u{2068}', '\u{2069}'], "").contains(want))
}

#[test]
fn missing_chosen_components_are_named_and_marked() {
    for wide in [true, false] {
        let mut s = Scene::ready(LANGS[1], common::dark(), wide);
        let installed: Vec<String> = s.store.installed.iter().map(|a| a.id.clone()).collect();
        let missing: Vec<String> = s
            .store
            .apps()
            .iter()
            .map(|a| a.id.clone())
            .filter(|id| !installed.contains(id))
            .take(2)
            .collect();
        assert_eq!(missing.len(), 2, "the fixture has apps not installed");
        s.rebuild();
        assert!(
            !has(&mut s, "chosen at installation"),
            "nothing chosen: no hint"
        );
        let mut chosen = missing.clone();
        chosen.extend(installed.first().cloned());
        chosen.push("not-in-the-catalogue".into());
        s.store.chosen = chosen;
        assert_eq!(s.store.chosen_missing().len(), 2);
        s.rebuild();
        assert!(has(&mut s, "2 components chosen at installation"));
        if wide {
            // The compact row is read out by its name (the badge is inside it).
            assert!(has(&mut s, "Chosen at installation"));
        }
    }
}

#[test]
fn the_setting_is_taken_only_when_valid() {
    let mut settings = AppUpdateSettings::default();
    let list = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(settings.set_list(CHOSEN, &list(&["km-office", "coder"])));
    assert_eq!(settings.chosen, ["km-office", "coder"]);
    assert!(!settings.set_list(CHOSEN, &list(&["Bad Id"])));
    assert!(!settings.set_list("apps.autostart", &list(&["coder"])));
    let many: Vec<String> = (0..=MAX_CHOSEN).map(|i| format!("a{i}")).collect();
    assert!(!settings.set_list(CHOSEN, &many));
    assert_eq!(
        settings.chosen,
        ["km-office", "coder"],
        "the last good value stays"
    );
}
