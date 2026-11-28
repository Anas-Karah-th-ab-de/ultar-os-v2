//! Keys of the viewer as key codes of the viewed device (Linux evdev numbers; the viewed
//! device injects them as a keyboard). The mapping is for a US layout and covers the
//! printable ASCII characters and the usual control keys; other characters (Arabic letters,
//! umlauts) cannot be typed this way and are dropped, which the caller can see (`None`).

use ultar_bus::messages::mesh_view::RemoteInput;
use ultar_ui::event::{Key, Modifiers};

pub const CTRL: u16 = 29;
pub const SHIFT: u16 = 42;
pub const ALT: u16 = 56;
pub const META: u16 = 125;
pub const KEY_DELETE: u16 = 111;
pub const KEY_BACKSPACE: u16 = 14;
pub const KEY_TAB: u16 = 15;
pub const KEY_T: u16 = 20;
pub const KEY_F4: u16 = 62;

/// Row of letters on a US keyboard (`qwertyuiop` = codes 16..), home row, bottom row.
const LETTERS: [(char, u16); 26] = [
    ('q', 16),
    ('w', 17),
    ('e', 18),
    ('r', 19),
    ('t', 20),
    ('y', 21),
    ('u', 22),
    ('i', 23),
    ('o', 24),
    ('p', 25),
    ('a', 30),
    ('s', 31),
    ('d', 32),
    ('f', 33),
    ('g', 34),
    ('h', 35),
    ('j', 36),
    ('k', 37),
    ('l', 38),
    ('z', 44),
    ('x', 45),
    ('c', 46),
    ('v', 47),
    ('b', 48),
    ('n', 49),
    ('m', 50),
];

/// Digits `1234567890` = codes 2..=11 and their shifted symbols.
const DIGITS: &str = "1234567890";
const DIGIT_SHIFTED: &str = "!@#$%^&*()";

/// Other symbols: (plain, shifted, code).
const SYMBOLS: [(char, char, u16); 11] = [
    ('-', '_', 12),
    ('=', '+', 13),
    ('[', '{', 26),
    (']', '}', 27),
    (';', ':', 39),
    ('\'', '"', 40),
    ('`', '~', 41),
    ('\\', '|', 43),
    (',', '<', 51),
    ('.', '>', 52),
    ('/', '?', 53),
];

/// The key code of `c` and whether Shift is needed for it.
pub fn char_code(c: char) -> Option<(u16, bool)> {
    if let Some(&(_, code)) = LETTERS.iter().find(|(l, _)| *l == c.to_ascii_lowercase()) {
        return Some((code, c.is_ascii_uppercase()));
    }
    if let Some(i) = DIGITS.find(c) {
        return Some((2 + i as u16, false));
    }
    if let Some(i) = DIGIT_SHIFTED.find(c) {
        return Some((2 + i as u16, true));
    }
    for &(plain, shifted, code) in &SYMBOLS {
        if c == plain {
            return Some((code, false));
        }
        if c == shifted {
            return Some((code, true));
        }
    }
    (c == ' ').then_some((57, false))
}

/// The key code of a named key.
pub fn named_code(key: Key) -> Option<u16> {
    Some(match key {
        Key::Escape => 1,
        Key::Backspace => KEY_BACKSPACE,
        Key::Tab => KEY_TAB,
        Key::Enter => 28,
        Key::Space => 57,
        Key::Home => 102,
        Key::ArrowUp => 103,
        Key::PageUp => 104,
        Key::ArrowLeft => 105,  // physical
        Key::ArrowRight => 106, // physical
        Key::End => 107,
        Key::ArrowDown => 108,
        Key::PageDown => 109,
        Key::Delete => KEY_DELETE,
        Key::Menu => 127,
        Key::F(n @ 1..=10) => 58 + u16::from(n),
        Key::F(11) => 87,
        Key::F(12) => 88,
        _ => return None,
    })
}

/// Presses `code` with `modifiers` held (they go down first and up last).
pub fn chord(code: u16, ctrl: bool, shift: bool, alt: bool, meta: bool) -> Vec<RemoteInput> {
    let mods: Vec<u16> = [(ctrl, CTRL), (shift, SHIFT), (alt, ALT), (meta, META)]
        .into_iter()
        .filter_map(|(on, c)| on.then_some(c))
        .collect();
    let mut out: Vec<RemoteInput> = mods
        .iter()
        .map(|&code| RemoteInput::Key { code, down: true })
        .collect();
    out.push(RemoteInput::Key { code, down: true });
    out.push(RemoteInput::Key { code, down: false });
    out.extend(
        mods.iter()
            .rev()
            .map(|&code| RemoteInput::Key { code, down: false }),
    );
    out
}

/// What a key press of the viewer becomes on the viewed device; `None` for a key that has no
/// code there (the caller drops it).
pub fn sequence(key: Key, m: Modifiers) -> Option<Vec<RemoteInput>> {
    match key {
        Key::Char(c) => {
            let (code, shifted) = char_code(c)?;
            Some(chord(code, m.ctrl, m.shift || shifted, m.alt, m.meta))
        }
        other => Some(chord(named_code(other)?, m.ctrl, m.shift, m.alt, m.meta)),
    }
}

/// A combination the toolbar sends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Combo {
    CtrlAltDelete,
    CtrlAltBackspace,
    CtrlAltT,
    AltTab,
    AltF4,
    Super,
}

impl Combo {
    /// All combinations in menu order.
    pub const ALL: [Combo; 6] = [
        Combo::CtrlAltDelete,
        Combo::CtrlAltBackspace,
        Combo::CtrlAltT,
        Combo::AltTab,
        Combo::AltF4,
        Combo::Super,
    ];

    /// The catalog key of its name.
    pub fn key(self) -> &'static str {
        match self {
            Combo::CtrlAltDelete => "remote.keys.cad",
            Combo::CtrlAltBackspace => "remote.keys.cab",
            Combo::CtrlAltT => "remote.keys.cat",
            Combo::AltTab => "remote.keys.alttab",
            Combo::AltF4 => "remote.keys.altf4",
            Combo::Super => "remote.keys.super",
        }
    }

    /// The key events of the combination.
    pub fn inputs(self) -> Vec<RemoteInput> {
        match self {
            Combo::CtrlAltDelete => chord(KEY_DELETE, true, false, true, false),
            Combo::CtrlAltBackspace => chord(KEY_BACKSPACE, true, false, true, false),
            Combo::CtrlAltT => chord(KEY_T, true, false, true, false),
            Combo::AltTab => chord(KEY_TAB, false, false, true, false),
            Combo::AltF4 => chord(KEY_F4, false, false, true, false),
            Combo::Super => vec![
                RemoteInput::Key {
                    code: META,
                    down: true,
                },
                RemoteInput::Key {
                    code: META,
                    down: false,
                },
            ],
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn k(code: u16, down: bool) -> RemoteInput {
        RemoteInput::Key { code, down }
    }

    #[test]
    fn letters_digits_and_symbols_have_us_codes() {
        assert_eq!(char_code('a'), Some((30, false)));
        assert_eq!(char_code('A'), Some((30, true)));
        assert_eq!(char_code('1'), Some((2, false)));
        assert_eq!(char_code('0'), Some((11, false)));
        assert_eq!(char_code('!'), Some((2, true)));
        assert_eq!(char_code('?'), Some((53, true)));
        assert_eq!(char_code(' '), Some((57, false)));
        assert_eq!(char_code('ب'), None);
        assert_eq!(char_code('ä'), None);
    }

    #[test]
    fn modifiers_go_down_first_and_up_last() {
        let m = Modifiers {
            ctrl: true,
            shift: false,
            alt: false,
            meta: false,
        };
        let seq = sequence(Key::Char('c'), m).unwrap();
        assert_eq!(
            seq,
            vec![k(29, true), k(46, true), k(46, false), k(29, false)]
        );
        let upper = sequence(Key::Char('Q'), Modifiers::default()).unwrap();
        assert_eq!(
            upper,
            vec![k(42, true), k(16, true), k(16, false), k(42, false)]
        );
    }

    #[test]
    fn named_keys_and_unknown_keys() {
        let enter = sequence(Key::Enter, Modifiers::default()).unwrap();
        assert_eq!(enter, vec![k(28, true), k(28, false)]);
        assert_eq!(named_code(Key::F(5)), Some(63));
        assert_eq!(named_code(Key::F(11)), Some(87));
        assert!(sequence(Key::Char('ش'), Modifiers::default()).is_none());
    }

    #[test]
    fn every_input_is_a_valid_word_the_viewed_device_accepts() {
        for combo in Combo::ALL {
            for i in combo.inputs() {
                assert_eq!(RemoteInput::parse(&i.word()), Some(i));
            }
        }
        assert_eq!(
            Combo::CtrlAltDelete.inputs(),
            vec![
                k(29, true),
                k(56, true),
                k(111, true),
                k(111, false),
                k(56, false),
                k(29, false)
            ]
        );
        assert_eq!(Combo::Super.inputs(), vec![k(125, true), k(125, false)]);
    }
}
