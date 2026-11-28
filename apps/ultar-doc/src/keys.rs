//! What a key does to the document. Arrow keys are physical: in a right-to-left paragraph the
//! left arrow moves on to the next letter, in a left-to-right one back. `Ctrl+B/I/U` toggle
//! bold/italic/underline; `Ctrl+S` saves, `Ctrl+N` starts a new document, `Ctrl+C/X/V` copy,
//! cut and paste.

use crate::state::Doc;
use ultar_ui::event::{Key, Modifiers};

/// What the app does after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    Moved,
    Changed,
    Save,
    New,
    Copy(String),
    Paste,
}

/// Applies `key` to `doc`; `rtl` tells whether the caret's paragraph runs right to left.
pub fn apply(doc: &mut Doc, key: Key, m: Modifiers, rtl: bool) -> Effect {
    if m.ctrl && !m.alt {
        ctrl_key(doc, key, m.shift)
    } else if m.alt || m.meta {
        Effect::None
    } else {
        plain_key(doc, key, m.shift, rtl)
    }
}

fn ctrl_key(doc: &mut Doc, key: Key, shift: bool) -> Effect {
    match key {
        Key::Char(c) => match c.to_ascii_lowercase() {
            's' => Effect::Save,
            'n' => Effect::New,
            'a' => {
                doc.select_all();
                Effect::Moved
            }
            'z' if shift => changed(doc.redo()),
            'z' => changed(doc.undo()),
            'y' => changed(doc.redo()),
            'c' => copy(doc.selected_text()),
            'x' => {
                let text = doc.selected_text();
                if text.is_empty() {
                    return Effect::None;
                }
                doc.delete_selection();
                Effect::Copy(text)
            }
            'v' => Effect::Paste,
            'b' => {
                doc.for_each_run(|r| r.bold = !(r.bold && !r.text.is_empty()));
                Effect::Changed
            }
            'i' => {
                doc.for_each_run(|r| r.italic = !(r.italic && !r.text.is_empty()));
                Effect::Changed
            }
            'u' => {
                doc.for_each_run(|r| r.underline = !(r.underline && !r.text.is_empty()));
                Effect::Changed
            }
            _ => Effect::None,
        },
        _ => Effect::None,
    }
}

fn plain_key(doc: &mut Doc, key: Key, shift: bool, rtl: bool) -> Effect {
    match key {
        Key::Char(c) => {
            doc.insert(&c.to_string());
            Effect::Changed
        }
        Key::Space => {
            doc.insert(" ");
            Effect::Changed
        }
        Key::Enter => {
            doc.newline();
            Effect::Changed
        }
        Key::Tab => {
            doc.insert("    ");
            Effect::Changed
        }
        Key::Backspace => {
            doc.backspace();
            Effect::Changed
        }
        Key::Delete => {
            doc.delete();
            Effect::Changed
        }
        Key::ArrowLeft | Key::ArrowRight => {
            let on = matches!(key, Key::ArrowLeft) == rtl;
            doc.move_chars(if on { 1 } else { -1 }, shift);
            Effect::Moved
        }
        Key::Home => {
            doc.home(shift);
            Effect::Moved
        }
        Key::End => {
            doc.end(shift);
            Effect::Moved
        }
        Key::ArrowUp => {
            doc.move_block(-1, shift);
            Effect::Moved
        }
        Key::ArrowDown => {
            doc.move_block(1, shift);
            Effect::Moved
        }
        _ => Effect::None,
    }
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
    doc.insert(text);
    Effect::Changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Cursor;

    fn no() -> Modifiers {
        Modifiers::default()
    }
    fn ctrl() -> Modifiers {
        Modifiers { ctrl: true, ..Modifiers::default() }
    }

    fn typed(doc: &mut Doc, s: &str) {
        for c in s.chars() {
            let key = if c == ' ' { Key::Space } else { Key::Char(c) };
            apply(doc, key, no(), false);
        }
    }

    #[test]
    fn typing_and_ctrl_bold_work() {
        let mut d = Doc::new();
        typed(&mut d, "hello");
        assert_eq!(d.item_text(0, None), Some("hello".into()));
        apply(&mut d, Key::Char('a'), ctrl(), false);
        apply(&mut d, Key::Char('b'), ctrl(), false);
        let ultar_office::doc::Block::Para(p) = &d.document.blocks[0] else {
            panic!("expected a paragraph");
        };
        assert!(p.runs.iter().all(|r| r.bold));
        apply(&mut d, Key::Char('s'), ctrl(), false);
        assert_eq!(
            apply(&mut d, Key::Home, no(), false),
            Effect::Moved
        );
    }

    #[test]
    fn copy_cut_paste_flow() {
        let mut d = Doc::new();
        typed(&mut d, "abc");
        apply(&mut d, Key::Char('a'), ctrl(), false);
        assert_eq!(apply(&mut d, Key::Char('c'), ctrl(), false), Effect::Copy("abc".into()));
        assert_eq!(apply(&mut d, Key::Char('x'), ctrl(), false), Effect::Copy("abc".into()));
        assert_eq!(d.item_text(0, None), Some(String::new()));
        assert_eq!(apply(&mut d, Key::Char('v'), ctrl(), false), Effect::Paste);
        paste(&mut d, "\nxyz");
        assert_eq!(d.document.blocks.len(), 2);
    }

    #[test]
    fn arrows_are_physical_in_arabic() {
        let mut d = Doc::new();
        typed(&mut d, "سلام");
        apply(&mut d, Key::Home, no(), true);
        d.cursor = Cursor { block: 0, cell: None, ch: 0 };
        apply(&mut d, Key::ArrowLeft, no(), true);
        assert_eq!(d.cursor.ch, 1, "left goes on in a right-to-left paragraph");
    }
}