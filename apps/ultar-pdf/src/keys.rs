//! What a key does to the reader (the shortcuts). The zoom, fit, rotate and search state
//! live in the app; this only names the effect.

use ultar_ui::event::{Key, Modifiers};

/// What the app does after a key the widgets did not use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Nothing happened.
    None,
    ZoomIn,
    ZoomOut,
    /// Back to 100%.
    ZoomReset,
    FitWidth,
    FitPage,
    Rotate,
    /// Open the search bar (Ctrl+F).
    Search,
    /// Leave and close the search bar.
    CloseSearch,
    NextHit,
    PrevHit,
    /// Copy the selected text (Ctrl+C).
    Copy,
    /// Scroll one viewport down/up.
    PageDown,
    PageUp,
    /// Open the "open file" dialog (Ctrl+O).
    Open,
}

/// Maps a key to its effect; `searching` switches the meaning of a few keys.
pub fn apply(key: Key, m: Modifiers, searching: bool) -> Effect {
    if m.ctrl && !m.alt && !m.meta {
        match key {
            Key::Char(c) => match c.to_ascii_lowercase() {
                'f' => Effect::Search,
                'o' => Effect::Open,
                'c' => Effect::Copy,
                '+' | '=' => Effect::ZoomIn,
                '-' => Effect::ZoomOut,
                '0' => Effect::ZoomReset,
                _ => Effect::None,
            },
            Key::Enter => Effect::NextHit,
            _ => Effect::None,
        }
    } else if m.alt || m.meta {
        Effect::None
    } else {
        match key {
            Key::Escape if searching => Effect::CloseSearch,
            Key::PageDown if !searching => Effect::PageDown,
            Key::PageUp if !searching => Effect::PageUp,
            Key::Enter if searching => Effect::NextHit,
            _ => Effect::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> Modifiers {
        Modifiers::default()
    }
    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        }
    }

    #[test]
    fn shortcuts_zoom_fit_search_and_copy() {
        assert_eq!(apply(Key::Char('f'), ctrl(), false), Effect::Search);
        assert_eq!(apply(Key::Char('o'), ctrl(), false), Effect::Open);
        assert_eq!(apply(Key::Char('c'), ctrl(), true), Effect::Copy);
        assert_eq!(apply(Key::Char('+'), ctrl(), false), Effect::ZoomIn);
        assert_eq!(apply(Key::Char('='), ctrl(), false), Effect::ZoomIn);
        assert_eq!(apply(Key::Char('-'), ctrl(), false), Effect::ZoomOut);
        assert_eq!(apply(Key::Char('0'), ctrl(), false), Effect::ZoomReset);
    }

    #[test]
    fn escape_closes_the_search_and_pages_scroll() {
        assert_eq!(apply(Key::Escape, plain(), true), Effect::CloseSearch);
        assert_eq!(apply(Key::Escape, plain(), false), Effect::None);
        assert_eq!(apply(Key::PageDown, plain(), false), Effect::PageDown);
        assert_eq!(apply(Key::PageDown, plain(), true), Effect::None);
        assert_eq!(apply(Key::Char('f'), plain(), false), Effect::None);
    }
}