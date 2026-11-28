//! The unlock screen as a widget tree, in the look of the lock screen (brand, title, hint,
//! password field, Caps Lock warning).

use crate::{Phase, Screen};
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::{PaintEnv, TextSpec};
use ultar_ui::text::{FontRole, TextAlign, Weight};
use ultar_ui::tokens::{ColorRole, InputMode, Space, ThemeKind, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{InfoBar, Label, Text, TextField, Tone};

/// Width of the password field (as on the lock screen, `g(84)`).
const FIELD_WIDTH: f32 = 84.0 * Space::S4.points();

const CATALOGS: [(ultar_i18n::Lang, &str); 3] = [
    (ultar_i18n::Lang::Ar, include_str!("../locales/ar.ftl")),
    (ultar_i18n::Lang::En, include_str!("../locales/en.ftl")),
    (ultar_i18n::Lang::De, include_str!("../locales/de.ftl")),
];

/// A translator with this screen's catalogs.
pub fn translator(lang: ultar_i18n::Lang) -> Translator {
    let mut tr = Translator::new(lang);
    for (l, source) in CATALOGS {
        // the catalogs are part of the program and checked by the tests
        let _ = tr.add_source(l, source);
    }
    tr
}

fn error(s: &Screen) -> Option<Label> {
    match &s.phase {
        Phase::Wrong => Some(Label::key("unlock.wrong")),
        Phase::Failed(reason) => Some(Label::key_with("unlock.failed", &[("reason", reason)])),
        _ => None,
    }
}

/// Builds the screen for `size` device pixels at `scale`.
pub fn build(s: &Screen, size: (u32, u32), scale: f32) -> Result<Ui, UiError> {
    let env = PaintEnv {
        scale,
        ..PaintEnv::new(s.lang, ThemeKind::Dark)
    };
    let logical = Size::new(size.0 as f32 / scale, size.1 as f32 / scale);
    let mut ui = Ui::new(env, InputMode::Pointer, logical, translator(s.lang))?;
    ui.set_background(ColorRole::Ground);
    ui.set_root_flex(Flex::column().padding(Padding::all(Space::S24)));
    let root = ui.root();
    let middle = Flex::column()
        .gap(Space::S24)
        .main(MainAlign::Center)
        .cross(CrossAlign::Center);
    let middle = ui.add_sized(root, middle, Sizing::grow(1))?;
    let title = TextSpec {
        role: FontRole::Title,
        ..TextSpec::new(TypeSize::S24).weight(Weight::Semibold)
    };
    ui.add(
        middle,
        Text::new(Label::data(ultar_brand::PRODUCT)).spec(title),
    )?;
    let heading = TextSpec::new(TypeSize::S16).weight(Weight::Semibold);
    ui.add(
        middle,
        Text::new(Label::key("unlock.title"))
            .spec(heading)
            .align(TextAlign::Center),
    )?;
    let hint = match s.phase {
        Phase::Checking => "unlock.checking",
        Phase::Done => "unlock.done",
        _ => "unlock.hint",
    };
    ui.add(
        middle,
        Text::new(Label::key(hint))
            .color(ColorRole::TextMuted)
            .align(TextAlign::Center),
    )?;
    let width = FIELD_WIDTH.min((logical.w - 2.0 * Space::S24.points()).max(0.0));
    field(&mut ui, middle, s, width)?;
    Ok(ui)
}

/// The password field (one dot per stroke; it never holds the passphrase itself) and the
/// Caps Lock warning.
fn field(ui: &mut Ui, middle: NodeId, s: &Screen, width: f32) -> Result<(), UiError> {
    let dots: String = "x".repeat(s.typed);
    let field = TextField::password(Label::key("unlock.field"))
        .labelled()
        .text(&dots);
    let field = ui.add_sized(middle, field, Sizing::width(width))?;
    ui.set_disabled(field, matches!(s.phase, Phase::Checking | Phase::Done))?;
    ui.set_error(field, error(s))?;
    if !matches!(s.phase, Phase::Checking | Phase::Done) {
        ui.focus(field)?;
    }
    if s.caps {
        ui.add_sized(
            middle,
            InfoBar::new(Tone::Warning, Label::key("unlock.caps")),
            Sizing::width(width),
        )?;
    }
    Ok(())
}
