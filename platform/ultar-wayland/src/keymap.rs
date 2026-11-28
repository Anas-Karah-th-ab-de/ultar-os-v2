//! The keyboard as Wayland clients see it: an XKB keymap generated from the
//! compositor's own layout tables (Arabic, English, German – the same order
//! as the compositor's layouts, one XKB group each), so a key gives the app
//! exactly the character the compositor would type. The compositor does not
//! tell ordinary clients which layout is active; each key event names its
//! character, and [`group_for`] picks the group that produces it.

use std::fmt::Write as _;
use ultar_compositor::keymap::{translate, Layout, Output};
use ultar_surface::proto::Modifiers;

/// Groups in XKB order (index = group number).
pub const GROUPS: [Layout; 3] = [Layout::Arabic, Layout::English, Layout::German];

/// Real XKB modifier masks (Shift, Lock, Control, Mod1 … Mod5).
pub mod mask {
    /// Shift.
    pub const SHIFT: u32 = 1;
    /// Caps Lock.
    pub const LOCK: u32 = 2;
    /// Control.
    pub const CONTROL: u32 = 4;
    /// Mod1 = Alt.
    pub const ALT: u32 = 8;
    /// Mod4 = Super.
    pub const SUPER: u32 = 64;
    /// Mod5 = AltGr (third level).
    pub const ALT_GR: u32 = 128;
}

/// Modifier state for `wl_keyboard.modifiers`: (depressed, locked).
pub fn masks(m: Modifiers) -> (u32, u32) {
    let pairs = [
        (Modifiers::SHIFT, mask::SHIFT),
        (Modifiers::CTRL, mask::CONTROL),
        (Modifiers::ALT, mask::ALT),
        (Modifiers::SUPER, mask::SUPER),
        (Modifiers::ALT_GR, mask::ALT_GR),
    ];
    let depressed = pairs
        .iter()
        .filter(|(b, _)| m.has(*b))
        .map(|(_, v)| v)
        .sum();
    let locked = if m.has(Modifiers::CAPS_LOCK) {
        mask::LOCK
    } else {
        0
    };
    (depressed, locked)
}

/// The group whose layout turns `code` with `m` into `character` (0 = the key
/// gave no single character). Keeps `current` when it fits or nothing does;
/// shortcuts (Ctrl, Alt, Super) never switch.
pub fn group_for(code: u16, m: Modifiers, character: u32, current: u32) -> u32 {
    if m.has(Modifiers::CTRL) || m.has(Modifiers::ALT) || m.has(Modifiers::SUPER) {
        return current;
    }
    let caps = m.has(Modifiers::CAPS_LOCK);
    let level = Modifiers(m.0 & (Modifiers::SHIFT | Modifiers::ALT_GR));
    let fits = |layout: Layout| match translate(layout, code, level, caps) {
        Output::Char(c) => u32::from(c) == character,
        Output::Text(_) | Output::None => character == 0,
    };
    let candidates: Vec<u32> = (0..GROUPS.len() as u32)
        .filter(|&g| fits(GROUPS[g as usize]))
        .collect();
    if candidates.contains(&current) || candidates.is_empty() {
        current
    } else {
        candidates[0]
    }
}

/// Keys without a character: evdev code and keysym name (all groups).
const SPECIAL: &[(u16, &str)] = &[
    (1, "Escape"),
    (14, "BackSpace"),
    (15, "Tab"),
    (28, "Return"),
    (29, "Control_L"),
    (42, "Shift_L"),
    (54, "Shift_R"),
    (55, "KP_Multiply"),
    (56, "Alt_L"),
    (58, "Caps_Lock"),
    (59, "F1"),
    (60, "F2"),
    (61, "F3"),
    (62, "F4"),
    (63, "F5"),
    (64, "F6"),
    (65, "F7"),
    (66, "F8"),
    (67, "F9"),
    (68, "F10"),
    (69, "Num_Lock"),
    (70, "Scroll_Lock"),
    (71, "KP_7"),
    (72, "KP_8"),
    (73, "KP_9"),
    (74, "KP_Subtract"),
    (75, "KP_4"),
    (76, "KP_5"),
    (77, "KP_6"),
    (78, "KP_Add"),
    (79, "KP_1"),
    (80, "KP_2"),
    (81, "KP_3"),
    (82, "KP_0"),
    (83, "KP_Decimal"),
    (87, "F11"),
    (88, "F12"),
    (96, "KP_Enter"),
    (97, "Control_R"),
    (98, "KP_Divide"),
    (99, "Print"),
    (100, "ISO_Level3_Shift"),
    (102, "Home"),
    (103, "Up"),
    (104, "Prior"),
    (105, "Left"),
    (106, "Right"),
    (107, "End"),
    (108, "Down"),
    (109, "Next"),
    (110, "Insert"),
    (111, "Delete"),
    (113, "XF86AudioMute"),
    (114, "XF86AudioLowerVolume"),
    (115, "XF86AudioRaiseVolume"),
    (119, "Pause"),
    (125, "Super_L"),
    (126, "Super_R"),
    (127, "Menu"),
];

/// Keys that may give characters (number row, letters, space, the ISO key).
fn character_keys() -> impl Iterator<Item = u16> {
    (2..=13)
        .chain(16..=27)
        .chain(30..=41)
        .chain(43..=53)
        .chain([57, 86])
}

/// XKB keysym name of a key output.
fn keysym(out: Output) -> Option<String> {
    let c = match out {
        Output::Char(' ') => return Some("space".to_owned()),
        Output::Char(c) if (c as u32) < 0x20 => return None,
        Output::Char(c) => c,
        // Lam-alef keys: their presentation-form ligatures.
        Output::Text("\u{644}\u{627}") => '\u{FEFB}',
        Output::Text("\u{644}\u{622}") => '\u{FEF5}',
        Output::Text("\u{644}\u{623}") => '\u{FEF7}',
        Output::Text("\u{644}\u{625}") => '\u{FEF9}',
        Output::Text(t) => t.chars().next()?,
        Output::None => return None,
    };
    Some(format!("U{:04X}", c as u32))
}

/// The four levels (plain, Shift, AltGr, Shift+AltGr) of `code` in `layout`.
fn levels(layout: Layout, code: u16) -> [Option<String>; 4] {
    let at = |bits: u8| keysym(translate(layout, code, Modifiers(bits), false));
    // Only the German layout has a third level (AltGr); the others ignore it.
    let third = layout == Layout::German;
    [
        at(0),
        at(Modifiers::SHIFT),
        if third { at(Modifiers::ALT_GR) } else { None },
        if third {
            at(Modifiers::SHIFT | Modifiers::ALT_GR)
        } else {
            None
        },
    ]
}

/// One group of a key: its type and symbols.
fn group_text(layout: Layout, code: u16) -> (&'static str, String) {
    let l = levels(layout, code);
    // Caps Lock changes it: a letter (Lock acts like Shift on it).
    let letter =
        translate(layout, code, Modifiers(0), true) != translate(layout, code, Modifiers(0), false);
    let four = l[2].is_some() || l[3].is_some();
    let ty = match (four, letter) {
        (true, true) => "ULTAR_FOUR_ALPHA",
        (true, false) => "ULTAR_FOUR",
        (false, true) => "ULTAR_ALPHA",
        (false, false) => "ULTAR_TWO",
    };
    let n = if four { 4 } else { 2 };
    let syms: Vec<String> = l[..n]
        .iter()
        .map(|s| s.clone().unwrap_or_else(|| "NoSymbol".to_owned()))
        .collect();
    (ty, syms.join(", "))
}

/// The whole keymap text (XKB v1, as `wl_keyboard.keymap` sends it).
pub fn keymap_text() -> String {
    let mut s = String::from("xkb_keymap {\n");
    keycodes(&mut s);
    s.push_str(TYPES);
    s.push_str(COMPAT);
    symbols(&mut s);
    s.push_str("};\n");
    s
}

fn keycodes(s: &mut String) {
    s.push_str("xkb_keycodes \"ultar\" {\n minimum = 8;\n maximum = 255;\n");
    for code in 1u16..=247 {
        let _ = writeln!(s, " <I{}> = {};", code + 8, code + 8);
    }
    s.push_str(" indicator 1 = \"Caps Lock\";\n indicator 2 = \"Num Lock\";\n};\n");
}

fn symbols(s: &mut String) {
    s.push_str("xkb_symbols \"ultar\" {\n name[Group1] = \"Arabic\";\n name[Group2] = \"English (US)\";\n name[Group3] = \"German\";\n");
    for &(code, name) in SPECIAL {
        let _ = writeln!(s, " key <I{}> {{ [ {name} ] }};", code + 8);
    }
    for code in character_keys() {
        let mut parts = Vec::new();
        for (g, &layout) in GROUPS.iter().enumerate() {
            let (ty, syms) = group_text(layout, code);
            parts.push(format!(
                "type[Group{n}] = \"{ty}\", symbols[Group{n}] = [ {syms} ]",
                n = g + 1
            ));
        }
        let _ = writeln!(s, " key <I{}> {{ {} }};", code + 8, parts.join(", "));
    }
    s.push_str(MODMAP);
    s.push_str("};\n");
}

const TYPES: &str = "xkb_types \"ultar\" {
 type \"ONE_LEVEL\" { modifiers = none; level_name[Level1] = \"Any\"; };
 type \"ULTAR_TWO\" { modifiers = Shift; map[Shift] = Level2; level_name[Level1] = \"Base\"; level_name[Level2] = \"Shift\"; };
 type \"ULTAR_ALPHA\" { modifiers = Shift+Lock; map[Shift] = Level2; map[Lock] = Level2; level_name[Level1] = \"Base\"; level_name[Level2] = \"Caps\"; };
 type \"ULTAR_FOUR\" { modifiers = Shift+Mod5; map[Shift] = Level2; map[Mod5] = Level3; map[Shift+Mod5] = Level4; level_name[Level1] = \"Base\"; level_name[Level2] = \"Shift\"; level_name[Level3] = \"AltGr\"; level_name[Level4] = \"Shift AltGr\"; };
 type \"ULTAR_FOUR_ALPHA\" { modifiers = Shift+Lock+Mod5; map[Shift] = Level2; map[Lock] = Level2; map[Mod5] = Level3; map[Shift+Mod5] = Level4; map[Lock+Mod5] = Level4; level_name[Level1] = \"Base\"; level_name[Level2] = \"Shift\"; level_name[Level3] = \"AltGr\"; level_name[Level4] = \"Shift AltGr\"; };
};
";

const COMPAT: &str = "xkb_compat \"ultar\" {
 interpret.useModMapMods = AnyLevel;
 interpret.repeat = False;
 interpret Caps_Lock+AnyOfOrNone(all) { action = LockMods(modifiers = Lock); };
 interpret Num_Lock+AnyOfOrNone(all) { action = LockMods(modifiers = Mod2); };
 interpret Any+AnyOf(all) { action = SetMods(modifiers = modMapMods, clearLocks); };
 indicator \"Caps Lock\" { whichModState = Locked; modifiers = Lock; };
 indicator \"Num Lock\" { whichModState = Locked; modifiers = Mod2; };
};
";

const MODMAP: &str = " modifier_map Shift { <I50>, <I62> };
 modifier_map Lock { <I66> };
 modifier_map Control { <I37>, <I105> };
 modifier_map Mod1 { <I64> };
 modifier_map Mod2 { <I77> };
 modifier_map Mod4 { <I133>, <I134> };
 modifier_map Mod5 { <I108> };
";

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers(0);

    #[test]
    fn group_follows_the_character() {
        let a = u32::from('a');
        let sheen = u32::from('ش');
        assert_eq!(group_for(30, NONE, sheen, 1), 0);
        assert_eq!(group_for(30, NONE, a, 0), 1);
        assert_eq!(
            group_for(16, NONE, u32::from('q'), 0),
            1,
            "q: en and de, first fits"
        );
        assert_eq!(
            group_for(16, NONE, u32::from('q'), 2),
            2,
            "current kept when it fits"
        );
        assert_eq!(group_for(21, NONE, u32::from('z'), 1), 2, "QWERTZ");
        assert_eq!(
            group_for(2, NONE, u32::from('1'), 2),
            2,
            "digits fit everywhere"
        );
        assert_eq!(
            group_for(48, NONE, 0, 1),
            0,
            "lam-alef sends text, no character"
        );
        let ctrl = Modifiers(Modifiers::CTRL);
        assert_eq!(group_for(30, ctrl, a, 0), 0, "shortcuts never switch");
        let altgr = Modifiers(Modifiers::ALT_GR);
        assert_eq!(group_for(16, altgr, u32::from('@'), 0), 2);
    }

    #[test]
    fn masks_of_modifiers() {
        let m = Modifiers(Modifiers::SHIFT | Modifiers::CTRL | Modifiers::CAPS_LOCK);
        assert_eq!(masks(m), (mask::SHIFT | mask::CONTROL, mask::LOCK));
        assert_eq!(
            masks(Modifiers(Modifiers::ALT_GR | Modifiers::SUPER)),
            (mask::ALT_GR | mask::SUPER, 0)
        );
    }

    #[test]
    fn keymap_has_three_groups_and_every_key() {
        let k = keymap_text();
        assert!(k.starts_with("xkb_keymap {") && k.ends_with("};\n"));
        assert!(
            k.contains("<I38> { type[Group1] = \"ULTAR_TWO\", symbols[Group1] = [ U0634"),
            "KEY_A arabic"
        );
        assert!(k.contains("type[Group2] = \"ULTAR_ALPHA\", symbols[Group2] = [ U0061, U0041 ]"));
        assert!(k.contains("<I24> {"), "KEY_Q");
        assert!(k.contains("U0040"), "German AltGr+Q = @");
        assert!(k.contains("UFEFB"), "lam-alef");
        assert!(k.contains("<I36> { [ Return ] }"));
        assert_eq!(k.matches('{').count(), k.matches('}').count());
        assert!(!k.contains("U000A") && !k.contains("U0009"));
    }
}
