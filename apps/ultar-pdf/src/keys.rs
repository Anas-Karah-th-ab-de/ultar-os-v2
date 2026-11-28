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
    /// Back to 100 %.
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
    SelectAll,
    ClearSelection,
    /// Scroll one line.
    LineDown,
    LineUp,
    LineLeft,
    LineRight,
    /// Scroll one viewport down/up.
    PageDown,
    PageUp,
    /// The first / last page.
    First,
    Last,
    /// Open the "open file" dialog (Ctrl+O).
    Open,
}

/// What the app tells the key map about the moment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Context {
    /// The search bar is open.
    pub searching: bool,
    /// A text field has the keyboard (the page number, the search): its own keys stay its own.
    pub typing: bool,
}

/// Maps a key to its effect.
pub fn apply(key: Key, m: Modifiers, cx: Context) -> Effect {
    if m.ctrl && !m.alt && !m.meta {
        return with_ctrl(key, m.shift);
    }
    if m.alt || m.meta {
        return Effect::None;
    }
    match key {
        Key::Escape if cx.searching => Effect::CloseSearch,
        Key::Escape => Effect::ClearSelection,
        Key::Enter if cx.searching => hit_step(m.shift),
        Key::F(3) => hit_step(m.shift),
        _ if cx.typing => Effect::None,
        Key::PageDown => Effect::PageDown,
        Key::PageUp => Effect::PageUp,
        Key::Space if m.shift => Effect::PageUp,
        Key::Space => Effect::PageDown,
        Key::ArrowDown => Effect::LineDown,
        Key::ArrowUp => Effect::LineUp,
        Key::ArrowLeft => Effect::LineLeft,
        Key::ArrowRight => Effect::LineRight,
        Key::Home => Effect::First,
        Key::End => Effect::Last,
        _ => Effect::None,
    }
}

fn hit_step(back: bool) -> Effect {
    if back {
        Effect::PrevHit
    } else {
        Effect::NextHit
    }
}

fn with_ctrl(key: Key, shift: bool) -> Effect {
    match key {
        Key::Char(c) => match c.to_ascii_lowercase() {
            'f' => Effect::Search,
            'o' => Effect::Open,
            'c' => Effect::Copy,
            'a' => Effect::SelectAll,
            'r' => Effect::Rotate,
            'g' => hit_step(shift),
            '1' => Effect::FitWidth,
            '2' => Effect::FitPage,
            '+' | '=' => Effect::ZoomIn,
            '-' => Effect::ZoomOut,
            '0' => Effect::ZoomReset,
            _ => Effect::None,
        },
        Key::Enter => hit_step(shift),
        Key::Home => Effect::First,
        Key::End => Effect::Last,
        _ => Effect::None,
    }
}

/// `true` for the keys the window hands to the app before the widgets see them (so Ctrl+F
/// works while a field has the keyboard, and the arrows scroll instead of moving the focus).
pub fn is_shortcut(key: Key, m: Modifiers, cx: Context) -> bool {
    if m.ctrl && !m.alt && !m.meta {
        return !(cx.typing && matches!(key, Key::Char('a' | 'c' | 'x' | 'v' | 'z' | 'A' | 'C' | 'X' | 'V' | 'Z')))
            && with_ctrl(key, m.shift) != Effect::None;
    }
    match key {
        Key::Escape => cx.searching,
        Key::Enter => cx.searching && m.shift,
        Key::F(3) => true,
        Key::PageDown | Key::PageUp | Key::ArrowDown | Key::ArrowUp | Key::ArrowLeft
        | Key::ArrowRight | Key::Home | Key::End | Key::Space => {
            !cx.typing && !m.alt && !m.meta && apply(key, m, cx) != Effect::None
        }
        _ => false,
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
    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Modifiers::default()
        }
    }
    const IDLE: Context = Context {
        searching: false,
        typing: false,
    };
    const FIND: Context = Context {
        searching: true,
        typing: true,
    };

    #[test]
    fn shortcuts_zoom_fit_search_and_copy() {
        for (c, e) in [
            ('f', Effect::Search),
            ('o', Effect::Open),
            ('c', Effect::Copy),
            ('a', Effect::SelectAll),
            ('r', Effect::Rotate),
            ('1', Effect::FitWidth),
            ('2', Effect::FitPage),
            ('+', Effect::ZoomIn),
            ('=', Effect::ZoomIn),
            ('-', Effect::ZoomOut),
            ('0', Effect::ZoomReset),
            ('q', Effect::None),
        ] {
            assert_eq!(apply(Key::Char(c), ctrl(), IDLE), e, "ctrl+{c}");
        }
        assert_eq!(apply(Key::Char('f'), plain(), IDLE), Effect::None);
    }

    #[test]
    fn the_search_keys_step_forward_and_back() {
        assert_eq!(apply(Key::Enter, plain(), FIND), Effect::NextHit);
        assert_eq!(apply(Key::Enter, shift(), FIND), Effect::PrevHit);
        assert_eq!(apply(Key::F(3), plain(), IDLE), Effect::NextHit);
        assert_eq!(apply(Key::F(3), shift(), IDLE), Effect::PrevHit);
        assert_eq!(apply(Key::Char('g'), ctrl(), IDLE), Effect::NextHit);
        assert_eq!(apply(Key::Escape, plain(), FIND), Effect::CloseSearch);
        assert_eq!(apply(Key::Escape, plain(), IDLE), Effect::ClearSelection);
        assert_eq!(apply(Key::Enter, plain(), IDLE), Effect::None);
    }

    #[test]
    fn scroll_keys_work_only_while_no_field_has_the_keyboard() {
        assert_eq!(apply(Key::PageDown, plain(), IDLE), Effect::PageDown);
        assert_eq!(apply(Key::Space, shift(), IDLE), Effect::PageUp);
        assert_eq!(apply(Key::ArrowDown, plain(), IDLE), Effect::LineDown);
        assert_eq!(apply(Key::Home, plain(), IDLE), Effect::First);
        assert_eq!(apply(Key::End, ctrl(), FIND), Effect::Last);
        for key in [Key::PageDown, Key::ArrowLeft, Key::Home, Key::Space] {
            assert_eq!(apply(key, plain(), FIND), Effect::None, "{key:?} in a field");
        }
    }

    #[test]
    fn the_window_hands_over_only_what_the_widgets_would_misuse() {
        assert!(is_shortcut(Key::Char('f'), ctrl(), FIND), "Ctrl+F inside the search field");
        assert!(!is_shortcut(Key::Char('c'), ctrl(), FIND), "Ctrl+C copies the field's text");
        assert!(is_shortcut(Key::Char('c'), ctrl(), IDLE));
        assert!(is_shortcut(Key::ArrowDown, plain(), IDLE));
        assert!(!is_shortcut(Key::ArrowDown, plain(), FIND));
        assert!(is_shortcut(Key::Escape, plain(), FIND));
        assert!(!is_shortcut(Key::Escape, plain(), IDLE));
        assert!(is_shortcut(Key::Enter, shift(), FIND));
        assert!(!is_shortcut(Key::Enter, plain(), FIND), "Enter submits the field");
        assert!(!is_shortcut(Key::Tab, plain(), IDLE));
    }
}
