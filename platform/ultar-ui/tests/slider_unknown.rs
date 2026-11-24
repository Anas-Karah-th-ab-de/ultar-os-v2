//! Regression (owner, quick settings in Arabic): a slider with nothing behind it (no audio
//! device, no display setting) said "0%" and drew its knob at the far end. `Slider::unknown`
//! writes "—" and draws no knob and no filled part: its picture does not depend on the value
//! it happens to hold.
mod common;

use ultar_ui::snapshot::{render, Image};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::widgets::{Label, Slider};
use ultar_ui::{Lang, Size};

const S: Size = Size::new(400.0, 120.0);

fn picture(lang: Lang, value: f32, unknown: bool) -> Vec<u8> {
    let mut ui = common::ui(lang, ThemeKind::Dark, S);
    let root = ui.root();
    let slider = Slider::new(Label::key("demo.brightness"), 0.0, 100.0, 5.0).value(value);
    let slider = if unknown { slider.unknown() } else { slider };
    let id = ui.add(root, slider).unwrap();
    ui.set_disabled(id, true).unwrap();
    let canvas = render(&mut ui).unwrap();
    Image::from_canvas(&canvas).rgba
}

#[test]
fn an_unknown_slider_draws_no_knob_and_no_level() {
    for lang in [Lang::Ar, Lang::En] {
        // a known value moves the knob
        assert_ne!(
            picture(lang, 0.0, false),
            picture(lang, 100.0, false),
            "{lang:?}: the knob follows the value"
        );
        // an unknown one shows the same picture whatever it holds …
        let unknown = picture(lang, 0.0, true);
        assert_eq!(
            unknown,
            picture(lang, 100.0, true),
            "{lang:?}: nothing of the value is drawn"
        );
        // … and that picture is neither 0 % nor 100 %
        assert_ne!(unknown, picture(lang, 0.0, false), "{lang:?}: not 0 %");
        assert_ne!(unknown, picture(lang, 100.0, false), "{lang:?}: not 100 %");
    }
}
