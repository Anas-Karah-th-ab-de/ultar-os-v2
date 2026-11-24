//! Design conformance: every brick of the boards "States" (dark) and "StatesLight" (light) is
//! rebuilt with the toolkit in the same state and compared with the board – size (±1 px at 1x)
//! and the colour at sample points inside the brick, on its border and on its focus ring.
//!
//! The expectations come from `design/conformance/states.tsv`, distilled from the boards by
//! `tools/design-conformance/make_manifest.py` (no browser is needed here). Differences that are
//! already known stand in `design/conformance/known-differences.txt`; the test fails on every new
//! difference (regression) and on every listed one that no longer occurs (the list only shrinks).
//! With `ULTAR_CONFORMANCE_OUT=<dir>` it also writes the rebuilt boards as PNG for side-by-side
//! pictures (`tools/design-conformance/compare.py`).

mod common;

use common::sheet::{apply, State};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Sizing};
use ultar_ui::snapshot::{render, Image};
use ultar_ui::tokens::{ColorRole, ThemeKind};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Checkbox, IconButton, Label, List, ListItem, Select, Slider, Switch,
    TextDirection, TextField,
};
use ultar_ui::{Lang, PixelRect, Size};

/// Size tolerance at 1x (DESKTOP-SPEC §2.2c; tightened from 2 once the toolkit matched).
const SIZE_TOL: f32 = 1.0;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[derive(Clone)]
struct Sample {
    what: String,
    dx: f32,
    dy: f32,
    rgb: [u8; 3],
}

#[derive(Clone)]
struct Case {
    board: String,
    theme: ThemeKind,
    row: String,
    state: String,
    visible: bool,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fixed_width: bool,
    board_size: (u32, u32),
    samples: Vec<Sample>,
}

fn num(s: &str) -> f32 {
    s.parse()
        .unwrap_or_else(|_| panic!("number in states.tsv: {s:?}"))
}

fn parse_samples(field: &str) -> Vec<Sample> {
    if field == "-" {
        return Vec::new();
    }
    field
        .split(';')
        .map(|s| {
            let p: Vec<&str> = s.split(':').collect();
            let c: Vec<u8> = p[3].split(',').map(|v| v.parse().unwrap()).collect();
            Sample {
                what: p[0].to_string(),
                dx: num(p[1]),
                dy: num(p[2]),
                rgb: [c[0], c[1], c[2]],
            }
        })
        .collect()
}

fn cases() -> Vec<Case> {
    let text = std::fs::read_to_string(repo().join("design/conformance/states.tsv"))
        .expect("design/conformance/states.tsv (run make_manifest.py)");
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            assert_eq!(f.len(), 13, "states.tsv line: {l}");
            Case {
                board: f[0].to_string(),
                theme: if f[1] == "light" {
                    ThemeKind::Light
                } else {
                    ThemeKind::Dark
                },
                row: f[2].to_string(),
                state: f[3].to_string(),
                visible: f[4] == "1",
                x: num(f[5]),
                y: num(f[6]),
                w: num(f[7]),
                h: num(f[8]),
                fixed_width: f[9] == "1",
                board_size: (num(f[10]) as u32, num(f[11]) as u32),
                samples: parse_samples(f[12]),
            }
        })
        .collect()
}

fn state_of(name: &str) -> State {
    match name {
        "hovered" => State::Hovered,
        "pressed" => State::Pressed,
        "focused" => State::Focused,
        "disabled" => State::Disabled,
        "loading" => State::Loading,
        "error" => State::Error,
        _ => State::Normal,
    }
}

/// Builds the brick of one board row with the board's own texts.
fn build(ui: &mut Ui, row: &str, sizing: Sizing) -> Result<NodeId, UiError> {
    let root = ui.root();
    let d = Label::data;
    match row {
        "primary" | "plain" | "quiet" | "danger" => {
            let (text, variant) = match row {
                "primary" => ("احتفاظ", ButtonVariant::Primary),
                "plain" => ("تراجع", ButtonVariant::Plain),
                "quiet" => ("مسح الكل", ButtonVariant::Quiet),
                _ => ("حذف الحساب", ButtonVariant::Danger),
            };
            ui.add_sized(root, Button::new(d(text)).variant(variant), sizing)
        }
        "iconbutton" => ui.add_sized(root, IconButton::new(IconId::Gear, d("الإعدادات")), sizing),
        "switch-off" => ui.add_sized(root, Switch::new(d("بلوتوث"), false), sizing),
        "switch-on" => ui.add_sized(root, Switch::new(d("واي فاي"), true), sizing),
        "select" => ui.add_sized(
            root,
            Select::new(d("معدّل التحديث"), vec![d("60 Hz")]),
            sizing,
        ),
        "slider" => ui.add_sized(
            root,
            Slider::new(d("السطوع"), 0.0, 100.0, 1.0).value(60.0),
            sizing,
        ),
        // The board marks the value `dir="ltr"` (a host name).
        "textfield" => ui.add_sized(
            root,
            TextField::new(d("اسم الجهاز"))
                .text("ultar-pc")
                .direction(TextDirection::Ltr),
            sizing,
        ),
        "checkbox" => ui.add_sized(root, Checkbox::new(d("الوسائط"), true), sizing),
        _ => {
            let list = ui.add_sized(root, List::navigation(), sizing)?;
            ui.add(
                list,
                ListItem::new(d("KM-Office"))
                    .sublabel(d("محفوظة"))
                    .icon(IconId::Wifi)
                    .chevron(),
            )
        }
    }
}

struct Rendered {
    canvas: Canvas,
    rect: ultar_ui::Rect,
}

fn render_case(c: &Case) -> Result<Rendered, String> {
    let size = Size::new(c.w.ceil() + 96.0, c.h.ceil() + 64.0);
    let mut ui = common::ui(Lang::Ar, c.theme, size);
    ui.set_background(ColorRole::Surface);
    ui.set_root_flex(
        Flex::column()
            .main(MainAlign::Center)
            .cross(CrossAlign::Center),
    );
    // The board's slider box is the slide area; its value (48 wide, 12 apart) is a sibling
    // at the end. The toolkit's slider holds both: build it that much wider.
    let value_slot = if c.row == "slider" { 60.0 } else { 0.0 };
    let sizing = if c.fixed_width {
        Sizing::width(c.w + value_slot)
    } else {
        Sizing::AUTO
    };
    let id = build(&mut ui, &c.row, sizing).map_err(|e| format!("build: {e}"))?;
    apply(&mut ui, id, state_of(&c.state)).map_err(|e| format!("state: {e}"))?;
    let canvas = render(&mut ui).map_err(|e| format!("render: {e}"))?;
    // A list item is measured, not its list.
    let mut rect = ui.rect(id).ok_or("no rect")?;
    if value_slot > 0.0 {
        // Arabic: the slide area is the start (right) part.
        rect.x += value_slot;
        rect.w -= value_slot;
    }
    if c.state == "error" {
        // The board's box is the control; the error message is a separate element below it
        // (role=alert). The toolkit draws it inside the node: compare without it.
        let calm = render_case(&Case {
            state: "normal".into(),
            samples: Vec::new(),
            ..c.clone()
        })?;
        rect.h -= (rect.h - calm.rect.h).max(0.0);
    }
    Ok(Rendered { canvas, rect })
}

fn dist(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0)
}

fn tolerance(what: &str) -> u8 {
    match what {
        "fill" | "ground" => 4,
        _ => 40,
    }
}

/// Compares one case; returns (stable key, detail) per difference.
fn compare(c: &Case, r: &Rendered) -> Vec<(String, String)> {
    let id = format!("{}/{}/{}", c.board, c.row, c.state);
    let mut out = Vec::new();
    if (r.rect.h - c.h).abs() > SIZE_TOL {
        out.push((
            format!("{id}: height"),
            format!("board {} toolkit {}", c.h, r.rect.h),
        ));
    }
    if !c.fixed_width && (r.rect.w - c.w).abs() > SIZE_TOL {
        out.push((
            format!("{id}: width"),
            format!("board {} toolkit {}", c.w, r.rect.w),
        ));
    }
    for s in &c.samples {
        let x = (r.rect.x + s.dx).floor() as i32;
        let y = (r.rect.y + s.dy).floor() as i32;
        let got = r
            .canvas
            .pixel(x, y)
            .map(|p| [p.r, p.g, p.b])
            .unwrap_or([0, 0, 0]);
        if dist(got, s.rgb) > tolerance(&s.what) {
            out.push((
                format!("{id}: {}", s.what),
                format!("board {:?} toolkit {:?} at ({x},{y})", s.rgb, got),
            ));
        }
    }
    out
}

fn known() -> BTreeSet<String> {
    std::fs::read_to_string(repo().join("design/conformance/known-differences.txt"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// One rebuilt brick: board, board size, theme, position on the board, picture.
type Placed<'a> = (String, (u32, u32), ThemeKind, f32, f32, &'a Rendered);

fn write_replicas(all: &[Placed<'_>], dir: &Path) {
    let mut boards: Vec<(String, (u32, u32), ThemeKind)> = Vec::new();
    for (b, size, theme, ..) in all {
        if !boards.iter().any(|(n, ..)| n == b) {
            boards.push((b.clone(), *size, *theme));
        }
    }
    std::fs::create_dir_all(dir).unwrap();
    for (board, (w, h), theme) in boards {
        let mut big = Canvas::new(w, h, PixelFormat::Rgba8).unwrap();
        let surface = ultar_ui::tokens::THEME
            .palette(theme)
            .get(ColorRole::Surface);
        big.clear(surface);
        for (b, _, _, x, y, r) in all.iter().filter(|e| e.0 == board) {
            let _ = b;
            let ox = (x - r.rect.x).round() as i32;
            let oy = (y - r.rect.y).round() as i32;
            // Only the brick and its focus ring, so neighbouring cells stay untouched.
            let m = 6.0;
            let area = PixelRect::new(
                (r.rect.x - m).floor().max(0.0) as i32,
                (r.rect.y - m).floor().max(0.0) as i32,
                ((r.rect.x + r.rect.w + m).ceil() as i32).min(r.canvas.width() as i32),
                ((r.rect.y + r.rect.h + m).ceil() as i32).min(r.canvas.height() as i32),
            );
            big.blit(&r.canvas, area, ox + area.x0, oy + area.y0);
        }
        let png = Image::from_canvas(&big).encode_png().unwrap();
        std::fs::write(dir.join(format!("{board}-toolkit.png")), png).unwrap();
    }
}

#[test]
fn states_boards_match_the_toolkit() {
    let cases = cases();
    assert_eq!(cases.len(), 168, "12 bricks x 7 states x 2 themes");
    let mut found: Vec<(String, String)> = Vec::new();
    let mut rendered = Vec::new();
    for c in cases.iter().filter(|c| c.visible) {
        match render_case(c) {
            Ok(r) => {
                found.extend(compare(c, &r));
                rendered.push((c, r));
            }
            Err(e) => found.push((format!("{}/{}/{}: build", c.board, c.row, c.state), e)),
        }
    }
    if let Ok(dir) = std::env::var("ULTAR_CONFORMANCE_OUT") {
        let list: Vec<_> = rendered
            .iter()
            .map(|(c, r)| (c.board.clone(), c.board_size, c.theme, c.x, c.y, r))
            .collect();
        write_replicas(&list, Path::new(&dir));
        let report: String = found.iter().map(|(k, d)| format!("{k}\t{d}\n")).collect();
        std::fs::write(Path::new(&dir).join("states-differences.tsv"), report).unwrap();
    }
    let known = known();
    let keys: BTreeSet<String> = found.iter().map(|(k, _)| k.clone()).collect();
    let new: Vec<_> = found.iter().filter(|(k, _)| !known.contains(k)).collect();
    let fixed: Vec<_> = known
        .iter()
        .filter(|k| k.starts_with("States") && !keys.contains(*k))
        .collect();
    let mut msg = String::new();
    for (k, d) in &new {
        msg += &format!("NEW difference {k}: {d}\n");
    }
    for k in &fixed {
        msg += &format!("FIXED, remove from known-differences.txt: {k}\n");
    }
    assert!(
        new.is_empty() && fixed.is_empty(),
        "{msg}{} differences, {} known",
        found.len(),
        known.len()
    );
}

/// DESKTOP-SPEC §13 #65 / boards: numbers and units inside Arabic text are a left-to-right
/// island (`<bdi dir=ltr>` on every board). A value passed as a message argument must keep
/// its order: "3840 × 2160" must not be shown as "2160 × 3840".
#[test]
fn message_arguments_stay_left_to_right_in_arabic() {
    use ultar_ui::text::{TextEngine, TextLayout, TextStyle};
    fn visual(l: &TextLayout) -> String {
        l.visual_clusters()
            .iter()
            .filter_map(|c| l.text[*c as usize..].chars().next())
            .collect()
    }
    let tr = common::translator(Lang::Ar);
    let mut engine = TextEngine::new().unwrap();
    let mut broken = Vec::new();
    for value in ["3840 × 2160", "60 Hz", "15.6″"] {
        let label = Label::key_with("demo.display.connected", &[("resolution", value)]);
        let text = label.resolve(&tr).into_owned();
        let shown = visual(&engine.layout(&text, &TextStyle::new(14.0, Lang::Ar), None));
        if !shown.contains(value) {
            broken.push(format!("{value:?} shown as {shown:?}"));
        }
    }
    let known = known();
    let key = "text/bidi-message-arguments";
    match (broken.is_empty(), known.contains(key)) {
        (true, true) => panic!("FIXED, remove from known-differences.txt: {key}"),
        (false, false) => panic!("NEW difference {key}: {}", broken.join("; ")),
        _ => {}
    }
}
