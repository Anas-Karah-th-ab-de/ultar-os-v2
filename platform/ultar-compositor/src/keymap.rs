//! Physical keyboard layouts: Linux key code + modifiers → text.
//!
//! Arabic (PC 101, the layout Windows and Ubuntu call "Arabic") is the
//! default; US English is the second layout. Digits are 0–9 in both
//! (plan §3.4b). Super+Space switches. The on-screen keyboard
//! (`platform/ultar-ime`) does not use this table: it commits text
//! through the input-method role.

use ultar_surface::proto::Modifiers;

/// A keyboard layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layout {
    /// Arabic (PC 101).
    Arabic,
    /// English (US).
    English,
}

/// What a key produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// One character.
    Char(char),
    /// Several characters (the lam-alef keys).
    Text(&'static str),
    /// Nothing (modifier, function key, unknown).
    None,
}

const US: [&str; 4] = ["1234567890-=", "qwertyuiop[]", "asdfghjkl;'`", "zxcvbnm,./"];
const US_SHIFT: [&str; 4] = [
    "!@#$%^&*()_+",
    "QWERTYUIOP{}",
    "ASDFGHJKL:\"~",
    "ZXCVBNM<>?",
];
/// Arabic rows; `#` stands for a multi-character key handled separately.
const AR: [&str; 4] = [
    "1234567890-=",
    "\u{636}\u{635}\u{62B}\u{642}\u{641}\u{63A}\u{639}\u{647}\u{62E}\u{62D}\u{62C}\u{62F}",
    "\u{634}\u{633}\u{64A}\u{628}\u{644}\u{627}\u{62A}\u{646}\u{645}\u{643}\u{637}\u{630}",
    "\u{626}\u{621}\u{624}\u{631}#\u{649}\u{629}\u{648}\u{632}\u{638}",
];
const AR_SHIFT: [&str; 4] = [
    "!@#$%^&*)(_+",
    "\u{64E}\u{64B}\u{64F}\u{64C}#\u{625}\u{2018}\u{F7}\u{D7}\u{61B}<>",
    "\u{650}\u{64D}][#\u{623}\u{640}\u{60C}/:\"~",
    "~\u{652}}{#\u{622}\u{2019},.\u{61F}",
];
/// First key code of each row.
const ROW_START: [u16; 4] = [2, 16, 30, 44];

/// Modifier bit of a key code, if it is a modifier.
pub fn modifier_bit(code: u16) -> Option<u8> {
    Some(match code {
        42 | 54 => Modifiers::SHIFT,
        29 | 97 => Modifiers::CTRL,
        56 => Modifiers::ALT,
        100 => Modifiers::ALT_GR,
        125 | 126 => Modifiers::SUPER,
        _ => return None,
    })
}

fn row_char(rows: &[&str; 4], code: u16) -> Option<char> {
    let (row, start) = ROW_START
        .iter()
        .enumerate()
        .rev()
        .find(|(_, &s)| code >= s)
        .map(|(i, &s)| (i, s))?;
    rows[row].chars().nth(usize::from(code - start))
}

/// The text of `code` in `layout` with `modifiers` (and Caps Lock).
pub fn translate(layout: Layout, code: u16, modifiers: Modifiers, caps: bool) -> Output {
    if modifiers.has(Modifiers::CTRL)
        || modifiers.has(Modifiers::ALT)
        || modifiers.has(Modifiers::SUPER)
    {
        // Shortcuts: report the Latin letter so Ctrl+C is Ctrl+C in Arabic too.
        return match row_char(&US, code) {
            Some(c) if code < 58 => Output::Char(c),
            _ => Output::None,
        };
    }
    match code {
        57 => return Output::Char(' '),
        15 => return Output::Char('\t'),
        28 | 96 => return Output::Char('\n'),
        43 => {
            return Output::Char(if modifiers.has(Modifiers::SHIFT) {
                '|'
            } else {
                '\\'
            })
        }
        _ => {}
    }
    if !(2..=53).contains(&code)
        || code == 14
        || code == 15
        || code == 28
        || code == 29
        || code == 42
    {
        return Output::None;
    }
    let shift = modifiers.has(Modifiers::SHIFT);
    match layout {
        Layout::English => {
            let letter = row_char(&US, code).is_some_and(|c| c.is_ascii_alphabetic());
            let upper = shift ^ (caps && letter);
            let rows = if upper { &US_SHIFT } else { &US };
            row_char(rows, code).map_or(Output::None, Output::Char)
        }
        Layout::Arabic => {
            let rows = if shift { &AR_SHIFT } else { &AR };
            match row_char(rows, code) {
                Some('#') if code == 48 => Output::Text(match shift {
                    false => "\u{644}\u{627}",
                    true => "\u{644}\u{622}",
                }),
                Some('#') if code == 20 => Output::Text("\u{644}\u{625}"),
                Some('#') if code == 34 => Output::Text("\u{644}\u{623}"),
                Some('#') => Output::Char('#'),
                Some(c) => Output::Char(c),
                None => Output::None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers(0);
    const SHIFT: Modifiers = Modifiers(Modifiers::SHIFT);

    #[test]
    fn english_layout() {
        assert_eq!(
            translate(Layout::English, 30, NONE, false),
            Output::Char('a')
        );
        assert_eq!(
            translate(Layout::English, 30, SHIFT, false),
            Output::Char('A')
        );
        assert_eq!(
            translate(Layout::English, 30, NONE, true),
            Output::Char('A')
        );
        assert_eq!(
            translate(Layout::English, 30, SHIFT, true),
            Output::Char('a')
        );
        assert_eq!(translate(Layout::English, 2, NONE, true), Output::Char('1'));
        assert_eq!(
            translate(Layout::English, 2, SHIFT, false),
            Output::Char('!')
        );
        assert_eq!(
            translate(Layout::English, 53, NONE, false),
            Output::Char('/')
        );
        assert_eq!(
            translate(Layout::English, 57, NONE, false),
            Output::Char(' ')
        );
        assert_eq!(
            translate(Layout::English, 43, SHIFT, false),
            Output::Char('|')
        );
        assert_eq!(translate(Layout::English, 14, NONE, false), Output::None);
        assert_eq!(translate(Layout::English, 59, NONE, false), Output::None);
        assert_eq!(translate(Layout::English, 1, NONE, false), Output::None);
    }

    #[test]
    fn arabic_layout_with_western_digits() {
        assert_eq!(
            translate(Layout::Arabic, 30, NONE, false),
            Output::Char('ش')
        );
        assert_eq!(
            translate(Layout::Arabic, 16, NONE, false),
            Output::Char('ض')
        );
        assert_eq!(
            translate(Layout::Arabic, 41, NONE, false),
            Output::Char('ذ')
        );
        assert_eq!(
            translate(Layout::Arabic, 53, NONE, false),
            Output::Char('ظ')
        );
        assert_eq!(
            translate(Layout::Arabic, 53, SHIFT, false),
            Output::Char('؟')
        );
        assert_eq!(translate(Layout::Arabic, 2, NONE, false), Output::Char('1'));
        assert_eq!(
            translate(Layout::Arabic, 48, NONE, false),
            Output::Text("لا")
        );
        assert_eq!(
            translate(Layout::Arabic, 48, SHIFT, false),
            Output::Text("لآ")
        );
        assert_eq!(
            translate(Layout::Arabic, 20, SHIFT, false),
            Output::Text("لإ")
        );
        assert_eq!(
            translate(Layout::Arabic, 34, SHIFT, false),
            Output::Text("لأ")
        );
        assert_eq!(
            translate(Layout::Arabic, 4, SHIFT, false),
            Output::Char('#')
        );
        assert_eq!(
            translate(Layout::Arabic, 16, SHIFT, false),
            Output::Char('\u{64E}')
        );
    }

    #[test]
    fn shortcuts_and_table_shape() {
        // Shortcuts use the Latin letter.
        let ctrl = Modifiers(Modifiers::CTRL);
        assert_eq!(
            translate(Layout::Arabic, 46, ctrl, false),
            Output::Char('c')
        );
        assert_eq!(translate(Layout::Arabic, 59, ctrl, false), Output::None);
        for row in AR.iter().chain(AR_SHIFT.iter()) {
            assert_eq!(
                row.chars().count(),
                if row.starts_with('\u{626}') || row.starts_with('~') {
                    10
                } else {
                    12
                },
                "{row}"
            );
        }
    }

    #[test]
    fn modifier_bits() {
        assert_eq!(modifier_bit(42), Some(Modifiers::SHIFT));
        assert_eq!(modifier_bit(97), Some(Modifiers::CTRL));
        assert_eq!(modifier_bit(125), Some(Modifiers::SUPER));
        assert_eq!(modifier_bit(100), Some(Modifiers::ALT_GR));
        assert_eq!(modifier_bit(56), Some(Modifiers::ALT));
        assert_eq!(modifier_bit(30), None);
    }
}
