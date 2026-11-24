//! Unit tests of the text field.

use super::*;

#[test]
fn editing_keeps_the_caret_on_boundaries() {
    let mut f = TextField::new(Label::key("f")).text("سلام");
    assert_eq!((f.content(), f.caret()), ("سلام", 8));
    assert!(f.backspace());
    assert_eq!((f.content(), f.caret()), ("سلا", 6));
    assert!(f.move_caret(false) && f.move_caret(false));
    assert_eq!(f.caret(), 2);
    assert!(f.insert('ـ'));
    assert_eq!(f.content(), "سـلا");
    assert!(f.delete());
    assert_eq!(f.content(), "سـا");
    f.set_caret(1);
    assert_eq!(f.caret(), 0, "moved back to a boundary");
    assert!(!f.backspace() && !f.move_caret(false));
    f.set_caret(999);
    assert_eq!(f.caret(), f.content().len());
    assert!(!f.delete() && !f.move_caret(true));
}

#[test]
fn control_characters_and_the_limit_are_refused() {
    let mut f = TextField::new(Label::key("f")).max_len(5);
    assert!(!f.insert('\n') && !f.insert('\u{202E}') && !f.insert('\u{7}'));
    f.set_text("ab\tc\u{2066}d");
    assert_eq!(f.content(), "abcd");
    assert!(f.insert('e'));
    assert!(!f.insert('f'), "limit of 5 bytes reached");
    assert!(!f.insert('ش'));
    f.set_text("ششششش");
    assert_eq!(
        f.content(),
        "شش",
        "cut at a character boundary below the limit"
    );
    let shrunk = TextField::new(Label::key("f")).text("abcdef").max_len(3);
    assert_eq!(shrunk.content(), "abc");
    assert!(TextField::new(Label::key("f"))
        .placeholder(Label::key("p"))
        .placeholder
        .is_some());
}

#[test]
fn long_text_scrolls_to_keep_the_caret_visible() {
    let inner = Rect::new(10.0, 0.0, 100.0, 20.0);
    assert_eq!(
        TextField::line_left(50.0, 50.0, inner, Direction::Ltr),
        10.0
    );
    assert_eq!(
        TextField::line_left(300.0, 300.0, inner, Direction::Ltr),
        10.0 - 200.0
    );
    assert_eq!(
        TextField::line_left(300.0, 20.0, inner, Direction::Ltr),
        10.0
    );
    // Right-to-left: a short line sits at the right; a long one shifts so that the caret
    // (at x = 0 of the line, the end of the text) is at the left inner edge.
    assert_eq!(TextField::line_left(50.0, 0.0, inner, Direction::Rtl), 60.0);
    assert_eq!(
        TextField::line_left(300.0, 0.0, inner, Direction::Rtl),
        10.0
    );
    assert_eq!(
        TextField::line_left(300.0, 300.0, inner, Direction::Rtl),
        -190.0
    );
}

#[test]
fn password_bytes_stay_in_one_buffer_that_is_wiped() {
    let mut f = TextField::password(Label::key("pw"));
    let capacity = f.text.capacity();
    assert!(capacity >= DEFAULT_MAX_LEN);
    for ch in "geheim-١٢٣".chars() {
        assert!(f.insert(ch));
    }
    assert!(f.backspace() && f.backspace());
    f.set_caret(0);
    assert!(f.delete());
    assert_eq!(f.content(), "eheim-١");
    // Deleted bytes behind the end are zero, the buffer did not move.
    let len = f.text.len();
    let spare = f.text.capacity() - len;
    assert!(spare > 0 && f.text.capacity() == capacity);
    f.set_text("");
    assert_eq!(f.content(), "");
    assert_eq!(f.text.capacity(), capacity);
    let normal = TextField::new(Label::key("n")).text("x");
    assert_eq!(normal.content(), "x");
    let short = TextField::password(Label::key("pw")).max_len(8);
    assert!(short.text.capacity() >= 8);
}
