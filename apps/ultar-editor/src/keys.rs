//! What a key does to the document. Arrow keys are physical: in a right-to-left line the
//! left arrow moves on to the next letter (it is to the left), in a left-to-right line back.

use crate::doc::{Doc, INDENT};
use ultar_ui::event::{Key, Modifiers};

/// What the app does after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Nothing happened.
    None,
    /// The caret or the selection moved.
    Moved,
    /// The text changed.
    Changed,
    /// Save (Ctrl+S).
    Save,
    /// Put this text on the clipboard (Ctrl+C, and Ctrl+X after cutting).
    Copy(String),
    /// Ask for the clipboard (Ctrl+V).
    Paste,
    /// Scroll a page (`true`: down) and move the caret with it.
    Page(bool),
    /// A new document (Ctrl+N).
    New,
}

/// Applies `key` to `doc`; `rtl` tells whether the caret's line runs right to left.
pub fn apply(doc: &mut Doc, key: Key, m: Modifiers, rtl: bool) -> Effect {
    let effect = if m.ctrl && !m.alt {
        ctrl_key(doc, key, m.shift)
    } else if m.alt || m.meta {
        Effect::None
    } else {
        plain_key(doc, key, m.shift, rtl)
    };
    if effect == Effect::Changed {
        doc.reindex();
    }
    effect
}

/// Ctrl with a key: save, new, select all, undo/redo, clipboard, start and end of the text.
fn ctrl_key(doc: &mut Doc, key: Key, shift: bool) -> Effect {
    let b = &mut doc.buffer;
    match key {
        Key::Char(c) => match c.to_ascii_lowercase() {
            's' => Effect::Save,
            'n' => Effect::New,
            'a' => {
                b.select_all();
                Effect::Moved
            }
            'z' if shift => changed(b.redo()),
            'z' => changed(b.undo()),
            'y' => changed(b.redo()),
            'c' => copy(b.selected_text()),
            'x' => {
                let text = b.selected_text();
                if text.is_empty() {
                    return Effect::None;
                }
                b.backspace();
                doc.reindex();
                Effect::Copy(text)
            }
            'v' => Effect::Paste,
            'd' => {
                b.duplicate_line();
                Effect::Changed
            }
            _ => Effect::None,
        },
        Key::Home | Key::End => {
            b.anchor = if shift {
                b.anchor.or(Some(b.cur))
            } else {
                None
            };
            b.cur = if key == Key::Home { 0 } else { b.text.len() };
            Effect::Moved
        }
        _ => Effect::None,
    }
}

/// A key without Ctrl: typing, deleting, moving.
fn plain_key(doc: &mut Doc, key: Key, shift: bool, rtl: bool) -> Effect {
    let b = &mut doc.buffer;
    match key {
        Key::Char(c) => b.insert(c.encode_utf8(&mut [0; 4])),
        Key::Space => b.insert(" "),
        Key::Enter => b.newline(INDENT),
        Key::Tab if shift => b.indent(INDENT, true),
        Key::Tab if b.selection().is_some() => b.indent(INDENT, false),
        Key::Tab => b.insert(INDENT),
        Key::Backspace => b.backspace(),
        Key::Delete => b.delete(),
        Key::PageUp => return Effect::Page(false),
        Key::PageDown => return Effect::Page(true),
        _ => return move_caret(doc, key, shift, rtl),
    }
    Effect::Changed
}

/// Arrows, Home and End. The arrows are physical: on a right-to-left line the left arrow
/// moves on in the text.
fn move_caret(doc: &mut Doc, key: Key, shift: bool, rtl: bool) -> Effect {
    let b = &mut doc.buffer;
    let on = |b: &mut ultar_office::code::Buffer| b.right(shift); // physical: the buffer's "next"
    let back = |b: &mut ultar_office::code::Buffer| b.left(shift); // physical: the buffer's "previous"
    match (key, rtl) {
        (Key::ArrowLeft, true) | (Key::ArrowRight, false) => on(b), // physical arrow keys
        (Key::ArrowLeft, false) | (Key::ArrowRight, true) => back(b), // physical arrow keys
        (Key::ArrowUp, _) => b.up(shift),
        (Key::ArrowDown, _) => b.down(shift),
        (Key::Home, _) => b.home(shift),
        (Key::End, _) => b.end(shift),
        _ => return Effect::None,
    }
    Effect::Moved
}

fn changed(did: bool) -> Effect {
    if did {
        Effect::Changed
    } else {
        Effect::None
    }
}

fn copy(text: String) -> Effect {
    if text.is_empty() {
        Effect::None
    } else {
        Effect::Copy(text)
    }
}

/// Inserts pasted `text` at the caret (replacing the selection).
pub fn paste(doc: &mut Doc, text: &str) -> Effect {
    if text.is_empty() {
        return Effect::None;
    }
    doc.buffer.insert(text);
    doc.reindex();
    Effect::Changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_textview::{Pos, Source};

    fn none() -> Modifiers {
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

    fn typed(doc: &mut Doc, text: &str) {
        for c in text.chars() {
            let key = if c == ' ' { Key::Space } else { Key::Char(c) };
            apply(doc, key, none(), false);
        }
    }

    #[test]
    fn typing_enter_backspace_and_undo() {
        let mut d = Doc::new();
        typed(&mut d, "مرحبا");
        apply(&mut d, Key::Enter, none(), false);
        typed(&mut d, "abc");
        assert_eq!(d.lines(), 2);
        assert_eq!((d.line(0), d.line(1)), ("مرحبا", "abc"));
        apply(&mut d, Key::Backspace, none(), false);
        assert_eq!(d.line(1), "ab");
        // one typing burst (with its backspace) is one undo step
        assert_eq!(
            apply(&mut d, Key::Char('z'), ctrl(), false),
            Effect::Changed
        );
        assert_eq!(d.line(1), "");
        assert_eq!(
            apply(&mut d, Key::Char('y'), ctrl(), false),
            Effect::Changed
        );
        assert_eq!(d.line(1), "ab");
        assert!(d.dirty());
    }

    #[test]
    fn arrows_are_physical_in_arabic_lines() {
        let mut d = Doc::new();
        typed(&mut d, "سلام");
        apply(&mut d, Key::Home, none(), true);
        assert_eq!(d.caret(), Pos::new(0, 0));
        apply(&mut d, Key::ArrowLeft, none(), true);
        assert_eq!(
            d.caret(),
            Pos::new(0, "س".len()),
            "left goes on in a right-to-left line"
        );
        apply(&mut d, Key::ArrowRight, none(), true);
        assert_eq!(d.caret(), Pos::new(0, 0));
    }

    #[test]
    fn select_copy_cut_paste() {
        let mut d = Doc::new();
        typed(&mut d, "hello world");
        apply(&mut d, Key::Home, shift(), false);
        assert_eq!(
            apply(&mut d, Key::Char('c'), ctrl(), false),
            Effect::Copy("hello world".into())
        );
        assert_eq!(
            apply(&mut d, Key::Char('x'), ctrl(), false),
            Effect::Copy("hello world".into())
        );
        assert_eq!(d.line(0), "");
        assert_eq!(apply(&mut d, Key::Char('v'), ctrl(), false), Effect::Paste);
        paste(&mut d, "a\nb");
        assert_eq!(d.lines(), 2);
        assert_eq!(apply(&mut d, Key::Char('s'), ctrl(), false), Effect::Save);
        assert_eq!(
            apply(&mut d, Key::Char('c'), ctrl(), false),
            Effect::None,
            "nothing selected"
        );
    }
}
