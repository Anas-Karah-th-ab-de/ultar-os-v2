//! Design conformance of the browser window against the board "DesktopBottom"
//! (design/boards/project/DesktopBottom.dc.html, English, light). The board side is
//! `tests/fixtures/browser-board.tsv`, written by tools/design-conformance/browser_boxes.py
//! from the headless board render (run.sh checks it is current). Every `browser.*` element:
//! its box at ±1 point, disabled state, and the colours of the regions it sits on.
//! A known difference is listed in design/conformance/known-differences.txt as
//! `browser/<id>`: it must still fail; an unlisted one must pass (the list only shrinks).

mod common;

use common::*;
use ultar_ui::canvas::Canvas;
use ultar_ui::color::Color;
use ultar_ui::geom::Rect;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Lang;
use ultar_web::app::Browser;
use ultar_web::engine::test_engine::TestEngine;

const TOL: f32 = 1.0;

struct Row {
    act: String,
    name: String,
    b: Rect,
    visual: Rect,
    fg: Color,
    disabled: bool,
}

fn rgb(text: &str) -> Color {
    let inner = text
        .trim_start_matches("rgba(")
        .trim_start_matches("rgb(")
        .trim_end_matches(')');
    let v: Vec<u8> = inner
        .split(',')
        .map(|p| p.trim().parse::<f32>().unwrap_or(0.0) as u8)
        .collect();
    Color::rgb(v[0], v[1], v[2])
}

fn board() -> Vec<Row> {
    let text = include_str!("fixtures/browser-board.tsv");
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let n = |i: usize| f[i].parse::<f32>().unwrap();
            Row {
                act: f[0].into(),
                name: f[1].into(),
                b: Rect::new(n(2), n(3), n(4), n(5)),
                visual: Rect::new(n(6), n(7), n(8), n(9)),
                fg: rgb(f[12]),
                disabled: f[13] == "1",
            }
        })
        .collect()
}

fn expect(id: &str, ok: bool, detail: String) {
    let known = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../design/conformance/known-differences.txt"),
    )
    .unwrap_or_default();
    let key = format!("browser/{id}");
    let listed = known.lines().any(|l| l.trim() == key);
    match (ok, listed) {
        (true, true) => panic!("FIXED, remove from known-differences.txt: {key}"),
        (false, false) => panic!("NEW difference {key}: {detail}"),
        (false, true) => eprintln!("known difference {key}: {detail}"),
        (true, false) => {}
    }
}

fn near(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() <= TOL
        && (a.y - b.y).abs() <= TOL
        && (a.w - b.w).abs() <= TOL
        && (a.h - b.h).abs() <= TOL
}

/// The board's state: tab 1 "New tab" (shown, start page), tab 2 "code-craft.tech".
fn board_state() -> Browser<TestEngine> {
    let mut b = browser(Lang::En, ThemeKind::Light);
    let n = b.chrome().new_tab;
    click(&mut b, n);
    open(&mut b, "code-craft.tech/title/code-craft.tech");
    b.select_tab(0).unwrap();
    b.ui_mut().layout();
    b
}

fn pixel(c: &Canvas, x: f32, y: f32) -> Color {
    c.pixel(x as i32, y as i32).unwrap()
}

fn same(a: Color, b: Color) -> bool {
    a.r.abs_diff(b.r) <= 3 && a.g.abs_diff(b.g) <= 3 && a.b.abs_diff(b.b) <= 3
}

fn dump(c: &Canvas, name: &str) {
    if let Ok(dir) = std::env::var("ULTAR_CONFORMANCE_OUT") {
        let png = ultar_ui::snapshot::Image::from_canvas(c)
            .encode_png()
            .unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}-toolkit.png")),
            png,
        )
        .unwrap();
    }
}

/// The toolkit's box of a board element: (id, toolkit box, board box).
fn toolkit_box(
    chrome: &ultar_web::chrome::Chrome,
    ui: &ultar_ui::ui::Ui,
    row: &Row,
    counters: &mut (usize, usize, usize),
) -> (String, Rect, Rect) {
    let rect = |n| ui.rect(n).unwrap();
    let (tab, close, field) = counters;
    match row.act.as_str() {
        "browser.tab" => {
            // The board's tab button sits inside the chip's 1-point border; the toolkit's
            // tab button carries a transparent 1-point edge instead: same text box.
            let r = rect(chrome.tabs[*tab].0);
            *tab += 1;
            (
                format!("tab{tab}"),
                Rect::new(r.x + 1.0, r.y + 1.0, r.w - 1.0, r.h - 2.0),
                row.b,
            )
        }
        "browser.tabclose" => {
            *close += 1;
            (
                format!("tabclose{close}"),
                rect(chrome.tabs[*close - 1].1),
                row.b,
            )
        }
        "browser.address" => {
            *field += 1;
            let node = if *field == 1 {
                chrome.address
            } else {
                chrome.start_search.unwrap()
            };
            (format!("address{field}"), rect(node), row.visual)
        }
        act => {
            let node = match act {
                "browser.newtab" => chrome.new_tab,
                "browser.back" => chrome.back,
                "browser.forward" => chrome.forward,
                "browser.reload" => chrome.reload,
                "browser.shield" => chrome.shield,
                "browser.downloads" => chrome.downloads,
                other => panic!("unexpected board element {other}"),
            };
            (
                act.trim_start_matches("browser.").to_string(),
                rect(node),
                row.b,
            )
        }
    }
}

#[test]
fn every_browser_element_sits_where_the_board_draws_it() {
    let b = board_state();
    let rows = board();
    assert_eq!(rows.len(), 12, "board elements");
    let chrome = b.chrome().clone();
    let ui = b.ui();
    let mut counters = (0, 0, 0);
    for row in &rows {
        let (id, mine, theirs) = toolkit_box(&chrome, ui, row, &mut counters);
        expect(
            &format!("{id}/box"),
            near(mine, theirs),
            format!("{} toolkit {mine:?} board {theirs:?}", row.name),
        );
    }
}

#[test]
fn disabled_states_are_the_boards() {
    let b = board_state();
    let rows = board();
    let chrome = b.chrome().clone();
    let ui = b.ui();
    let disabled = |act: &str| rows.iter().find(|r| r.act == act).unwrap().disabled;
    expect(
        "back/disabled",
        ui.is_disabled(chrome.back) == disabled("browser.back"),
        String::new(),
    );
    expect(
        "forward/disabled",
        ui.is_disabled(chrome.forward) == disabled("browser.forward"),
        String::new(),
    );
    expect(
        "reload/disabled",
        ui.is_disabled(chrome.reload) == disabled("browser.reload"),
        String::new(),
    );
}

#[test]
fn the_window_has_the_boards_colours() {
    let mut b = board_state();
    let chrome = b.chrome().clone();
    let canvas = ultar_ui::snapshot::render(b.ui_mut()).unwrap();
    dump(&canvas, "Browser");
    let ui = b.ui();
    let raised = Color::rgb(243, 244, 247);
    let line = Color::rgb(213, 217, 224);
    let surface = Color::rgb(255, 255, 255);
    let border = Color::rgb(118, 126, 144);
    let checks = [
        ("strip/fill", pixel(&canvas, 400.0, 3.0), raised),
        ("strip/line", pixel(&canvas, 400.0, 48.0), line),
        ("toolbar/fill", pixel(&canvas, 400.0, 52.0), surface),
        ("toolbar/line", pixel(&canvas, 400.0, 97.0), line),
        ("start/fill", pixel(&canvas, 20.0, 300.0), surface),
    ];
    for (id, got, want) in checks {
        expect(id, same(got, want), format!("{got:?} want {want:?}"));
    }
    let chip = ui.rect(chrome.chips[0]).unwrap();
    expect(
        "tab1/fill",
        same(pixel(&canvas, chip.x + 4.0, chip.y + 4.0), surface),
        format!("{:?}", pixel(&canvas, chip.x + 4.0, chip.y + 4.0)),
    );
    expect(
        "tab1/border",
        same(pixel(&canvas, chip.x + 16.0, chip.y), border),
        format!("{:?}", pixel(&canvas, chip.x + 16.0, chip.y)),
    );
    let chip2 = ui.rect(chrome.chips[1]).unwrap();
    expect(
        "tab2/fill",
        same(pixel(&canvas, chip2.x + 4.0, chip2.y + 4.0), raised),
        String::new(),
    );
    let field = ui.rect(chrome.address).unwrap();
    expect(
        "address/fill",
        same(pixel(&canvas, field.x + 40.0, field.y + 4.0), raised),
        String::new(),
    );
    expect(
        "address/border",
        same(pixel(&canvas, field.x + 40.0, field.y), border),
        format!("{:?}", pixel(&canvas, field.x + 40.0, field.y)),
    );
}

#[test]
fn the_protection_button_is_green() {
    let mut b = board_state();
    let chrome = b.chrome().clone();
    let canvas = ultar_ui::snapshot::render(b.ui_mut()).unwrap();
    let ui = b.ui();
    // The protection button draws its icons in the board's green (success).
    let shield = ui.rect(chrome.shield).unwrap();
    let green = board()
        .iter()
        .find(|r| r.act == "browser.shield")
        .unwrap()
        .fg;
    let mut found = false;
    for y in shield.y as i32..(shield.y + shield.h) as i32 {
        for x in shield.x as i32..(shield.x + shield.w) as i32 {
            found |= same(canvas.pixel(x, y).unwrap(), green);
        }
    }
    expect(
        "shield/colour",
        found,
        format!("no {green:?} pixel in the protection button"),
    );
}
