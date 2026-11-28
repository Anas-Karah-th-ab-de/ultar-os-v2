//! The typed passphrase as key strokes, not as text: the installer took the passphrase as
//! text in whatever layout was active then, so at start the strokes are turned into text in
//! every layout and each candidate is tried (the installation's layout first).

use ultar_compositor::keymap::{self, Layout, Output};
use ultar_surface::proto::Modifiers;
use zeroize::Zeroizing;

/// Most strokes kept (the installer's limit is 256 bytes).
pub const MAX_STROKES: usize = 256;
const CAPS_LOCK: u16 = 58;
const BACKSPACE: u16 = 14;
const ENTER: u16 = 28;
const KP_ENTER: u16 = 96;
const ESCAPE: u16 = 1;
const ALL: [Layout; 3] = [Layout::Arabic, Layout::English, Layout::German];

/// What a key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Nothing to show.
    None,
    /// The field or the Caps Lock hint changed.
    Changed,
    /// Enter: try the passphrase.
    Submit,
}

/// The strokes: key code, held modifiers and Caps Lock, packed (wiped when dropped).
#[derive(Default)]
pub struct Typed {
    strokes: Zeroizing<Vec<u32>>,
    held: u8,
    caps: bool,
}

fn pack(code: u16, mods: u8, caps: bool) -> u32 {
    u32::from(code) | u32::from(mods) << 16 | u32::from(caps) << 24
}

fn unpack(s: u32) -> (u16, Modifiers, bool) {
    (
        (s & 0xffff) as u16,
        Modifiers((s >> 16) as u8),
        s >> 24 != 0,
    )
}

impl Typed {
    /// Feeds one key change (`down`: pressed or repeated).
    pub fn key(&mut self, code: u16, down: bool) -> Action {
        if let Some(bit) = keymap::modifier_bit(code) {
            self.held = if down {
                self.held | bit
            } else {
                self.held & !bit
            };
            return Action::None;
        }
        if !down {
            return Action::None;
        }
        match code {
            CAPS_LOCK => self.caps = !self.caps,
            BACKSPACE => {
                self.strokes.pop();
            }
            ESCAPE => self.strokes.clear(),
            ENTER | KP_ENTER => return Action::Submit,
            _ => {
                let mods = Modifiers(self.held);
                let text = ALL
                    .iter()
                    .any(|&l| keymap::translate(l, code, mods, self.caps) != Output::None);
                if !text || self.strokes.len() >= MAX_STROKES {
                    return Action::None;
                }
                self.strokes.push(pack(code, self.held, self.caps));
            }
        }
        Action::Changed
    }

    /// Strokes typed so far.
    pub fn len(&self) -> usize {
        self.strokes.len()
    }

    /// No strokes.
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty()
    }

    /// Caps Lock is on.
    pub fn caps(&self) -> bool {
        self.caps
    }

    /// Forgets the strokes (after a wrong passphrase).
    pub fn clear(&mut self) {
        self.strokes.clear();
    }

    /// The strokes as text in `layout`.
    pub fn text(&self, layout: Layout) -> Zeroizing<String> {
        let mut out = Zeroizing::new(String::with_capacity(self.strokes.len() * 4));
        for &s in self.strokes.iter() {
            let (code, mods, caps) = unpack(s);
            match keymap::translate(layout, code, mods, caps) {
                Output::Char(c) => out.push(c),
                Output::Text(t) => out.push_str(t),
                Output::None => {}
            }
        }
        out
    }

    /// The texts to try: `first` layout first, then the others; each different text once.
    pub fn candidates(&self, first: Layout) -> Vec<Zeroizing<String>> {
        let mut out: Vec<Zeroizing<String>> = Vec::new();
        for layout in std::iter::once(first).chain(ALL.into_iter().filter(|&l| l != first)) {
            let t = self.text(layout);
            if !t.is_empty() && !out.iter().any(|o| **o == *t) {
                out.push(t);
            }
        }
        out
    }
}

/// The layout the installer's keyboard id was active in (`ar+us`, `us`, `de`).
pub fn layout_of(keyboard: &str) -> Layout {
    match keyboard {
        "de" => Layout::German,
        "us" => Layout::English,
        _ => Layout::Arabic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(codes: &[(u16, bool)]) -> Typed {
        let mut t = Typed::default();
        for &(c, shift) in codes {
            if shift {
                t.key(42, true);
            }
            t.key(c, true);
            t.key(c, false);
            if shift {
                t.key(42, false);
            }
        }
        t
    }

    #[test]
    fn strokes_become_text_in_each_layout() {
        // keys y (21) and z (44): QWERTY "yz", QWERTZ "zy"
        let t = typed(&[(21, false), (44, false), (2, false), (2, true)]);
        assert_eq!(*t.text(Layout::English), "yz1!");
        assert_eq!(*t.text(Layout::German), "zy1!");
        let c = t.candidates(Layout::German);
        assert_eq!(*c[0], "zy1!");
        assert_eq!(c.len(), 3, "Arabic, English and German differ");
    }

    #[test]
    fn backspace_escape_caps_and_enter() {
        let mut t = typed(&[(30, false), (48, false)]);
        assert_eq!(t.key(BACKSPACE, true), Action::Changed);
        assert_eq!(*t.text(Layout::English), "a");
        t.key(CAPS_LOCK, true);
        assert!(t.caps());
        t.key(30, true);
        assert_eq!(*t.text(Layout::English), "aA");
        assert_eq!(t.key(ENTER, true), Action::Submit);
        t.key(ESCAPE, true);
        assert!(t.is_empty());
    }

    #[test]
    fn keys_without_text_are_not_kept() {
        let mut t = Typed::default();
        assert_eq!(t.key(59, true), Action::None, "F1");
        assert_eq!(t.key(103, true), Action::None, "arrow up");
        assert!(t.is_empty());
    }

    #[test]
    fn layout_of_the_installer_keyboard() {
        assert_eq!(layout_of("de"), Layout::German);
        assert_eq!(layout_of("us"), Layout::English);
        assert_eq!(layout_of("ar+us"), Layout::Arabic);
    }
}
