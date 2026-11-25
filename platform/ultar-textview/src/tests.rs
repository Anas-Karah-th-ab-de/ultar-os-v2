use super::*;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Lang;

fn view(lang: Lang) -> TextView {
    TextView::new(PaintEnv::new(lang, ThemeKind::Dark), false).unwrap()
}

fn lines(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn renders_mixed_lines_and_marks() {
    let mut v = view(Lang::Ar);
    v.numbers = true;
    let text = lines(&["مرحبا بالعالم", "fn main() {}", ""]);
    let marks = Marks {
        caret: Some(Pos::new(1, 3)),
        selection: Some((Pos::new(0, 0), Pos::new(1, 2))),
    };
    let c = v.render(400, 120, &text, &marks).unwrap();
    assert_eq!((c.width(), c.height()), (400, 120));
    let ground = v.ground();
    let painted = (0..120)
        .flat_map(|y| (0..400).map(move |x| (x, y)))
        .filter(|(x, y)| c.pixel(*x, *y) != Some(ground))
        .count();
    assert!(
        painted > 500,
        "text, numbers, caret and selection are drawn"
    );
}

#[test]
fn arabic_lines_start_at_the_right_latin_at_the_left() {
    let mut v = view(Lang::En);
    let text = lines(&["abc", "عربي"]);
    let y = 8.0 + v.line_height() * 1.5;
    let ar_end = v.hit(400, 395.0, y, &text);
    assert_eq!(
        ar_end,
        Pos::new(1, 0),
        "the right edge of an Arabic line is its start"
    );
    let latin_start = v.hit(400, 9.0, 10.0, &text);
    assert_eq!(latin_start, Pos::new(0, 0));
    let latin_end = v.hit(400, 300.0, 10.0, &text);
    assert_eq!(latin_end, Pos::new(0, 3));
    assert!(v.is_rtl(&text, 1) && !v.is_rtl(&text, 0));
}

#[test]
fn hit_below_the_text_is_the_last_line() {
    let mut v = view(Lang::En);
    let text = lines(&["one", "two"]);
    assert_eq!(v.hit(300, 5.0, 500.0, &text).line, 1);
}

#[test]
fn scrolling_stays_in_the_text_and_reveals() {
    let mut v = view(Lang::En);
    v.scroll(5, 100, 10);
    assert_eq!(v.first, 5);
    v.scroll(-50, 100, 10);
    assert_eq!(v.first, 0);
    v.scroll(500, 100, 10);
    assert_eq!(v.first, 90);
    v.reveal(3, 10);
    assert_eq!(v.first, 3);
    v.reveal(40, 10);
    assert_eq!(v.first, 31);
    assert!(v.rows(400) >= 10);
}

#[test]
fn control_characters_keep_their_byte_length() {
    assert_eq!(shown_text("a\tb").len(), 3);
}
