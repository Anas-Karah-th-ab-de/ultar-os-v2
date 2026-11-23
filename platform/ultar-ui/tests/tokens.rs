//! Tokens: generated values match `tokens.toml`, nothing else in the crate holds a colour or
//! spacing literal, text colour pairs reach WCAG contrast, targets reach their minimum size.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use ultar_ui::tokens::{
    self, layout, motion, shadow, size, state, typography, ColorRole, InputMode, Palette, Radius,
    SizeClass, Space, ThemeKind, TypeSize, DARK, LIGHT, THEME,
};
use ultar_ui::widgets::{Button, IconButton, Label, Slider, Switch, TextField};
use ultar_ui::Size;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `key = value` pairs of one `[section]` of the token file (comments stripped).
fn section(name: &str) -> Vec<(String, String)> {
    let mut inside = false;
    let mut pairs = Vec::new();
    for line in tokens::SOURCE.lines() {
        let line = line.split(" #").next().unwrap_or("").trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            inside = header.trim_end_matches(']') == name;
        } else if let (true, Some((key, value))) = (inside, line.split_once('=')) {
            pairs.push((
                key.trim().to_string(),
                value.trim().trim_matches('"').to_string(),
            ));
        }
    }
    assert!(!pairs.is_empty(), "section [{name}] not found");
    pairs
}

fn value(name: &str, key: &str) -> String {
    section(name)
        .into_iter()
        .find(|(k, _)| k == key)
        .unwrap_or_else(|| panic!("{name}.{key}"))
        .1
}

fn list(name: &str, key: &str) -> Vec<f32> {
    value(name, key)
        .trim_matches(['[', ']'])
        .split(',')
        .map(|v| v.trim().parse().unwrap())
        .collect()
}

#[test]
fn embedded_copy_equals_the_repository_tokens() {
    // The coordinator sets ULTAR_TOKENS to design/tokens.toml of the repository.
    assert_eq!(
        fs::read_to_string(manifest_dir().join("tokens.toml")).unwrap(),
        tokens::SOURCE
    );
    if let Ok(path) = std::env::var("ULTAR_TOKENS") {
        let upstream =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("ULTAR_TOKENS={path}: {e}"));
        assert_eq!(
            upstream,
            tokens::SOURCE,
            "tokens.toml differs from {path}; copy it and rebuild"
        );
    }
}

#[test]
fn generated_colours_match_the_file() {
    for (kind, palette) in [("theme.dark", &DARK), ("theme.light", &LIGHT)] {
        let pairs = section(kind);
        assert_eq!(
            pairs.len(),
            ColorRole::ALL.len(),
            "{kind}: one role per line"
        );
        for role in ColorRole::ALL {
            let c = palette.get(role);
            let hex = format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b);
            let listed = &pairs
                .iter()
                .find(|(k, _)| k == role.name())
                .unwrap_or_else(|| panic!("{}", role.name()))
                .1;
            assert_eq!(listed.to_uppercase(), hex, "{kind}.{}", role.name());
            assert!(c.is_opaque());
        }
    }
    assert_eq!(THEME.dark, DARK);
    assert_eq!(THEME.palette(ThemeKind::Light), &LIGHT);
}

#[test]
fn generated_numbers_match_the_file() {
    let spaces: Vec<f32> = Space::ALL.iter().map(|s| s.points()).collect();
    assert_eq!(spaces, list("space", "values"));
    let sizes: Vec<f32> = TypeSize::ALL.iter().map(|s| s.points()).collect();
    assert_eq!(sizes, list("type", "sizes"));
    let number = |section: &str, key: &str| value(section, key).parse::<f32>().unwrap();
    assert_eq!(Radius::Control.points(), number("radius", "control"));
    assert_eq!(Radius::Card.points(), number("radius", "card"));
    assert_eq!(Radius::Round.points(), number("radius", "round"));
    assert_eq!(size::BAR_HEIGHT, number("size", "bar_height"));
    assert_eq!(size::TITLEBAR_HEIGHT, number("size", "titlebar_height"));
    assert_eq!(
        size::TARGET_POINTER_MIN,
        number("size", "target_pointer_min")
    );
    assert_eq!(size::TARGET_TOUCH_MIN, number("size", "target_touch_min"));
    assert_eq!(size::ICON_SMALL, number("size", "icon_small"));
    assert_eq!(size::ICON, number("size", "icon"));
    assert_eq!(size::STROKE, number("size", "stroke"));
    assert_eq!(size::FOCUS_RING, number("size", "focus_ring"));
    assert_eq!(motion::FEEDBACK as f32, number("motion", "feedback"));
    assert_eq!(motion::FADE as f32, number("motion", "fade"));
    assert_eq!(motion::OVERVIEW as f32, number("motion", "overview"));
    assert_eq!(layout::COMPACT_MAX, number("layout", "compact_max"));
    assert_eq!(layout::MEDIUM_MAX, number("layout", "medium_max"));
    assert_eq!(shadow::OFFSET_Y, number("shadow", "offset_y"));
    assert_eq!(shadow::BLUR, number("shadow", "blur"));
    assert_eq!(shadow::ALPHA_DARK, number("shadow", "alpha_dark"));
    assert_eq!(shadow::ALPHA_LIGHT, number("shadow", "alpha_light"));
    assert_eq!(typography::BODY.points(), number("type", "body"));
    assert_eq!(
        typography::LINE_HEIGHT_LATIN,
        number("type", "line_height_latin")
    );
    assert_eq!(
        typography::LINE_HEIGHT_ARABIC,
        number("type", "line_height_arabic")
    );
    assert_eq!(typography::FAMILY_LATIN, value("type", "family_latin"));
    assert_eq!(
        typography::FAMILY_ARABIC_TEXT,
        value("type", "family_arabic_text")
    );
    assert_eq!(
        typography::FAMILY_ARABIC_TITLE,
        value("type", "family_arabic_title")
    );
    assert_eq!(typography::FAMILY_MONO, value("type", "family_mono"));
    let weights: Vec<f32> = typography::WEIGHTS.iter().map(|w| f32::from(*w)).collect();
    assert_eq!(weights, list("type", "weights"));
}

#[test]
fn state_and_wave_3_tokens_match_the_file() {
    let number = |section: &str, key: &str| value(section, key).parse::<f32>().unwrap();
    assert_eq!(
        typography::FAMILY_MONO_ARABIC,
        value("type", "family_mono_arabic")
    );
    assert_eq!(state::DISABLED_OPACITY, number("state", "disabled_opacity"));
    assert_eq!(state::HOVER_OVERLAY, number("state", "hover_overlay"));
    assert_eq!(state::PRESSED_OVERLAY, number("state", "pressed_overlay"));
    assert_eq!(state::SCRIM_DARK, number("state", "scrim_dark"));
    assert_eq!(state::SCRIM_LIGHT, number("state", "scrim_light"));
    assert_eq!(state::FOCUS_RING.name(), value("state", "focus_ring"));
    assert_eq!(
        section("state").len(),
        6,
        "every state token is checked here"
    );
}

#[test]
fn size_classes_use_the_token_limits() {
    assert_eq!(SizeClass::for_width(360.0), SizeClass::Compact);
    assert_eq!(
        SizeClass::for_width(layout::COMPACT_MAX + 0.5),
        SizeClass::Medium
    );
    assert_eq!(SizeClass::for_width(800.0), SizeClass::Medium);
    assert_eq!(
        SizeClass::for_width(layout::MEDIUM_MAX + 1.0),
        SizeClass::Wide
    );
    assert_eq!(SizeClass::for_width(1920.0), SizeClass::Wide);
}

fn rust_sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut out = vec![manifest_dir().join("build.rs")];
    walk(&manifest_dir().join("src"), &mut out);
    walk(&manifest_dir().join("tests"), &mut out);
    assert!(out.len() > 30, "sources found: {}", out.len());
    out
}

/// `true` when `line` holds a `#` followed by exactly six or eight hex digits.
fn has_hex_colour(line: &str) -> bool {
    let bytes = line.as_bytes();
    (0..bytes.len()).filter(|i| bytes[*i] == b'#').any(|i| {
        let digits = bytes[i + 1..]
            .iter()
            .take_while(|b| b.is_ascii_hexdigit())
            .count();
        digits == 6 || digits == 8
    })
}

/// `true` when a spacing word is directly given a number: `padding: 12`, `gap = 8.0`,
/// `radius(4)`, `margin: 2`.
fn has_numeric_spacing(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    ["padding", "gap", "radius", "margin", "inset", "spacing"]
        .iter()
        .any(|word| {
            lower.match_indices(word).any(|(at, _)| {
                let rest = lower[at + word.len()..].trim_start();
                let Some(after) = rest.strip_prefix([':', '=', '(']) else {
                    return false;
                };
                // Skip a type annotation such as `: f32 =`.
                let after = after.trim_start();
                let typed =
                    after.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
                let after = match typed.trim_start().strip_prefix('=') {
                    Some(value) if typed.len() < after.len() => value,
                    _ => after,
                };
                after
                    .trim_start_matches(['=', ' '])
                    .starts_with(|c: char| c.is_ascii_digit())
            })
        })
}

/// `true` when a colour is constructed from number literals.
fn has_colour_literal(line: &str) -> bool {
    ["Color::rgb(", "Color::rgba("].iter().any(|ctor| {
        line.match_indices(ctor).any(|(at, _)| {
            line[at + ctor.len()..]
                .trim_start()
                .starts_with(|c: char| c.is_ascii_digit())
        })
    })
}

#[test]
fn the_scanners_recognise_what_they_should() {
    let hash = '#';
    assert!(has_hex_colour(&format!("let c = \"{hash}0B0D11\";")));
    assert!(has_hex_colour(&format!("{hash}ffffff80")));
    assert!(!has_hex_colour(&format!(
        "{hash}[derive(Debug)] {hash}12 {hash}abcde"
    )));
    assert!(has_numeric_spacing("padding: 12,") && has_numeric_spacing("let gap = 8.0;"));
    assert!(has_numeric_spacing("corner_radius(4)") && has_numeric_spacing("MARGIN: f32 = 2.0"));
    assert!(!has_numeric_spacing("padding: Padding::all(Space::S16),"));
    assert!(!has_numeric_spacing("let gap = flex.gap_points();"));
    assert!(
        has_colour_literal("Color::rgb(12, 13, 14)")
            && has_colour_literal("Color::rgba( 0, 0, 0, 9)")
    );
    assert!(!has_colour_literal("Color::rgb(r, g, b)"));
}

#[test]
fn no_colour_or_spacing_literal_outside_the_tokens() {
    let mut offences = Vec::new();
    for path in rust_sources() {
        let relative = path
            .strip_prefix(manifest_dir())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        // color.rs defines the type and its two value-free constants; build.rs is the token
        // generator (it writes the constructor calls); this file holds the scanner's inputs.
        let colour_module = relative == "src/color.rs";
        let this_file = relative == "tests/tokens.rs";
        let generator = relative == "build.rs";
        for (number, line) in fs::read_to_string(&path).unwrap().lines().enumerate() {
            let bad = has_hex_colour(line) && !this_file
                || has_colour_literal(line) && !colour_module && !this_file && !generator
                || has_numeric_spacing(line) && !this_file;
            if bad {
                offences.push(format!("{relative}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert!(
        offences.is_empty(),
        "literals outside the tokens module:\n{}",
        offences.join("\n")
    );
}

/// Text role on background role pairs the toolkit paints.
const TEXT_PAIRS: [(ColorRole, ColorRole); 26] = [
    // Wave 3: badge and info-bar tones on the raised background; muted text (tabs, menu
    // shortcuts, sublabels of list items) on the hover and pressed backgrounds.
    (ColorRole::Info, ColorRole::Raised),
    (ColorRole::Success, ColorRole::Raised),
    (ColorRole::Warning, ColorRole::Raised),
    (ColorRole::TextMuted, ColorRole::Hover),
    (ColorRole::TextMuted, ColorRole::Line),
    (ColorRole::Text, ColorRole::Ground),
    (ColorRole::Text, ColorRole::Bar),
    (ColorRole::Text, ColorRole::Surface),
    (ColorRole::Text, ColorRole::Raised),
    (ColorRole::Text, ColorRole::Hover),
    (ColorRole::Text, ColorRole::Line),
    (ColorRole::TextMuted, ColorRole::Ground),
    (ColorRole::TextMuted, ColorRole::Bar),
    (ColorRole::TextMuted, ColorRole::Surface),
    (ColorRole::TextMuted, ColorRole::Raised),
    (ColorRole::OnAccent, ColorRole::Accent),
    (ColorRole::AccentText, ColorRole::Ground),
    (ColorRole::AccentText, ColorRole::Surface),
    (ColorRole::AccentText, ColorRole::Raised),
    (ColorRole::Danger, ColorRole::Surface),
    (ColorRole::Danger, ColorRole::Raised),
    (ColorRole::Danger, ColorRole::Hover),
    (ColorRole::Danger, ColorRole::Line),
    (ColorRole::Info, ColorRole::Surface),
    (ColorRole::Success, ColorRole::Surface),
    (ColorRole::Warning, ColorRole::Surface),
];

fn failing_pairs(palette: &Palette, pairs: &[(ColorRole, ColorRole)], minimum: f32) -> Vec<String> {
    pairs
        .iter()
        .filter_map(|(fg, bg)| {
            let ratio = palette.get(*fg).contrast_ratio(palette.get(*bg));
            (ratio < minimum).then(|| format!("{} on {}: {ratio:.2}", fg.name(), bg.name()))
        })
        .collect()
}

#[test]
fn text_colour_pairs_reach_wcag_aa() {
    for (kind, palette) in [(ThemeKind::Dark, &DARK), (ThemeKind::Light, &LIGHT)] {
        let failing = failing_pairs(palette, &TEXT_PAIRS, 4.5);
        assert!(
            failing.is_empty(),
            "{}: below 4.5:1: {failing:?}",
            kind.name()
        );
    }
}

#[test]
fn control_borders_and_focus_ring_reach_three_to_one() {
    let pairs = [
        (ColorRole::ControlBorder, ColorRole::Surface),
        (ColorRole::ControlBorder, ColorRole::Raised),
        (ColorRole::AccentText, ColorRole::Surface),
        (ColorRole::AccentText, ColorRole::Ground),
        (ColorRole::Danger, ColorRole::Surface),
        // Wave 3: icon of a selected list item while hovered or pressed, the fill of a
        // progress bar on its track, unchecked boxes and radio rings, the scrollbar thumb,
        // the mark inside a checked box or chosen radio option.
        (ColorRole::AccentText, ColorRole::Hover),
        (ColorRole::AccentText, ColorRole::Line),
        (ColorRole::ControlBorder, ColorRole::Ground),
        (ColorRole::ControlBorder, ColorRole::Hover),
        (ColorRole::TextMuted, ColorRole::Surface),
        (ColorRole::OnAccent, ColorRole::Accent),
    ];
    for (kind, palette) in [(ThemeKind::Dark, &DARK), (ThemeKind::Light, &LIGHT)] {
        let failing = failing_pairs(palette, &pairs, 3.0);
        assert!(
            failing.is_empty(),
            "{}: below 3:1: {failing:?}",
            kind.name()
        );
    }
}

#[test]
fn interactive_widgets_reach_the_minimum_target_size() {
    for mode in [InputMode::Pointer, InputMode::Touch] {
        for lang in common::LANGS {
            let mut ui = common::ui(lang, ThemeKind::Dark, Size::new(400.0, 600.0));
            ui.set_input_mode(mode);
            ui.set_root_flex(
                ultar_ui::layout::Flex::column().cross(ultar_ui::layout::CrossAlign::Start),
            );
            let root = ui.root();
            let ids = [
                ui.add(root, Button::new(Label::data("x"))).unwrap(),
                ui.add(
                    root,
                    IconButton::new(ultar_ui::icon::IconId::Close, Label::key("demo.delete")),
                )
                .unwrap(),
                ui.add(root, Switch::new(Label::key("demo.night"), false))
                    .unwrap(),
                ui.add(root, common::orientation_select()).unwrap(),
                ui.add(
                    root,
                    Slider::new(Label::key("demo.brightness"), 0.0, 1.0, 0.0),
                )
                .unwrap(),
                ui.add(root, TextField::new(Label::key("demo.device")))
                    .unwrap(),
            ];
            ui.layout();
            for id in ids {
                let rect = ui.rect(id).unwrap();
                let minimum = mode.min_target();
                assert!(
                    rect.h >= minimum && rect.w >= minimum,
                    "{mode:?} {lang:?} {:?}: {rect:?}",
                    ui.widget(id)
                );
            }
        }
    }
    assert_eq!(InputMode::Pointer.min_target(), size::TARGET_POINTER_MIN);
    assert_eq!(InputMode::Touch.min_target(), size::TARGET_TOUCH_MIN);
}
