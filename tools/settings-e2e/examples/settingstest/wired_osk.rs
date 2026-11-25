//! The on-screen keyboard's settings: the keyboard's own identity is told every change over
//! the real bus, and its own code turns them into what it does – when it appears, whether it
//! offers suggestions and corrects typos.

use super::{change, choose, page, Follower, Run};
use ultar_bus::Client;
use ultar_desktop::osk::{Keyboard, Out, ACCESSIBILITY, AUTOCORRECT, MODE, SUGGESTIONS};
use ultar_ime::Act;

/// Keyboard page and accessibility page -> `ultar-osk`.
pub(crate) fn osk(bus: &Client, admin: &Client, run: &Run) -> Result<(), String> {
    let osk = Follower::connect(run, "the on-screen keyboard", "osk")?;
    let mut k = Keyboard::default();
    k.ime_state(true, 0);
    k.hardware = true;
    if k.visible() {
        return Err("automatic shows the keyboard beside a hardware keyboard".into());
    }
    let mut app = page(bus, run, "keyboard")?;
    // automatic -> always (the second choice)
    let mode = choose((&mut app, admin), ("keyboard", MODE), 1)?;
    osk.told(MODE, &mode)?;
    k.setting(MODE, &mode);
    if !k.visible() {
        return Err(format!(
            "keyboard.on_screen {mode:?} does not show the keyboard"
        ));
    }
    // Suggestions off: none for a typed prefix.
    let off = change((&mut app, admin), ("keyboard", SUGGESTIONS), None)?;
    osk.told(SUGGESTIONS, &off)?;
    k.setting(SUGGESTIONS, &off);
    k.press(&Act::Lang);
    k.press(&Act::Text("t".into()));
    if !k.suggestions().is_empty() {
        return Err("suggestions still offered after switching them off".into());
    }
    // Autocorrect on: a typo is replaced on space.
    let on = change((&mut app, admin), ("keyboard", AUTOCORRECT), None)?;
    osk.told(AUTOCORRECT, &on)?;
    k.setting(AUTOCORRECT, &on);
    k.word.clear();
    for c in ["t", "e", "h"] {
        k.press(&Act::Text(c.into()));
    }
    if !k.press(&Act::Space).contains(&Out::Commit("the".into())) {
        return Err("autocorrect does not fix 'teh'".into());
    }
    // Accessibility: always shown, whatever the mode.
    let mut access = page(bus, run, "accessibility")?;
    let forced = change((&mut access, admin), ("accessibility", ACCESSIBILITY), None)?;
    osk.told(ACCESSIBILITY, &forced)?;
    k.setting(ACCESSIBILITY, &forced);
    k.setting(
        MODE,
        &ultar_bus::messages::settings::SettingValue::Text("never".into()),
    );
    if !k.visible() {
        return Err("the accessibility switch does not show the keyboard".into());
    }
    println!(
        "SETTINGS: keyboard and accessibility pages -> on-screen keyboard: shown always, \
         suggestions off, autocorrect on, shown by the accessibility switch"
    );
    Ok(())
}
