//! Design conformance of the phone (DESKTOP-SPEC §2.2c): the boards PhoneDial, PhoneIncoming
//! and PhoneInCall are rebuilt with the app in the boards' state (Arabic, dark, 1280 × 800)
//! and every clickable element of `design/conformance/phone.tsv` is compared: present with its
//! action, size ±1 point, board colours at its fill and border sample points ±16 per channel.
//! Differences that exist today are listed in `design/conformance/known-differences.txt` as
//! `phone/<Board>/<index>/<what>`; the test fails on a new one and on a listed one that is gone.
//! With `ULTAR_CONFORMANCE_OUT=<dir>` it writes the rebuilt boards as PNG.

mod common;

use common::*;
use std::path::Path;
use ultar_bus::messages::call::Phase;
use ultar_ui::canvas::Canvas;
use ultar_ui::geom::Rect;
use ultar_ui::snapshot::{render, Image};

const SIZE_TOL: f32 = 1.0;
const COLOR_TOL: i32 = 16;

struct Row {
    board: String,
    index: String,
    action: String,
    name: String,
    w: f32,
    h: f32,
    samples: Vec<(String, [u8; 3])>,
}

fn repo() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn manifest() -> Vec<Row> {
    let text = std::fs::read_to_string(repo().join("design/conformance/phone.tsv")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            assert_eq!(f.len(), 9, "phone.tsv: {l}");
            let samples = if f[8] == "-" {
                Vec::new()
            } else {
                f[8].split(';')
                    .map(|s| {
                        let p: Vec<&str> = s.split(':').collect();
                        let c: Vec<u8> = p[3].split(',').map(|v| v.parse().unwrap()).collect();
                        (p[0].to_string(), [c[0], c[1], c[2]])
                    })
                    .collect()
            };
            Row {
                board: f[0].into(),
                index: f[1].into(),
                action: f[2].into(),
                name: f[3].into(),
                w: f[6].parse().unwrap(),
                h: f[7].parse().unwrap(),
                samples,
            }
        })
        .collect()
}

/// The app in the state the board shows.
fn board_app(board: &str) -> App {
    match board {
        "PhoneDial" => {
            let mut a = app(ready());
            for k in ['4', '8', '2', '1', '7', '7'] {
                let d = digit(&a, k);
                click(&mut a, d);
            }
            a.handle(ultar_ui::event::Event::PointerLeave).unwrap();
            a
        }
        "PhoneIncoming" => app(with_calls(vec![call(1, Phase::Ringing, false)])),
        _ => app(with_calls(vec![call(1, Phase::Active, true)])),
    }
}

/// Rectangle of the element a board row names, or `None` when the app has no such element.
fn element(a: &mut App, row: &Row, nth: usize) -> Option<Rect> {
    let parts = |a: &mut App, action: &str| -> Vec<Rect> {
        let Some(owner) = nodes(a, action).first().copied() else {
            return Vec::new();
        };
        a.ui()
            .a11y_tree()
            .into_iter()
            .filter(|n| n.id == owner && n.part.is_some())
            .map(|n| n.rect)
            .collect()
    };
    match row.action.as_str() {
        "phone.way" | "phone.filter" => parts(a, &row.action).get(nth).copied(),
        "phone.backspace" => {
            let field = a.built().number?;
            a.ui().end_action_rect(field).ok().flatten()
        }
        "phone.digit" => {
            let key = row.name.chars().next()?;
            let id = digit(a, key);
            a.ui().rect(id)
        }
        action => {
            let id = *nodes(a, action).get(nth)?;
            a.ui().rect(id)
        }
    }
}

fn sample(canvas: &Canvas, rect: Rect, what: &str) -> Option<[u8; 3]> {
    let (x, y) = match what {
        "fill" => (rect.x + rect.w - 5.0, rect.y + rect.h / 2.0),
        _ => (rect.x + rect.w / 2.0, rect.y + 0.5),
    };
    canvas.pixel(x as i32, y as i32).map(|c| [c.r, c.g, c.b])
}

fn compare(row: &Row, rect: Option<Rect>, canvas: &Canvas, out: &mut Vec<(String, String)>) {
    let id = |what: &str| format!("phone/{}/{}/{what}", row.board, row.index);
    let Some(rect) = rect else {
        out.push((
            id("missing"),
            format!("{} {:?} not built", row.action, row.name),
        ));
        return;
    };
    if (rect.w - row.w).abs() > SIZE_TOL || (rect.h - row.h).abs() > SIZE_TOL {
        let detail = format!(
            "{} {}: {}x{} vs board {}x{}",
            row.action, row.name, rect.w, rect.h, row.w, row.h
        );
        out.push((id("size"), detail));
    }
    for (what, want) in &row.samples {
        let got = sample(canvas, rect, what);
        let ok = got.is_some_and(|g| {
            (0..3).all(|i| (i32::from(g[i]) - i32::from(want[i])).abs() <= COLOR_TOL)
        });
        if !ok {
            out.push((
                id(what),
                format!("{} {}: {got:?} vs board {want:?}", row.action, row.name),
            ));
        }
    }
}

#[test]
fn phone_boards_match_the_app() {
    let known = std::fs::read_to_string(repo().join("design/conformance/known-differences.txt"))
        .unwrap_or_default();
    let rows = manifest();
    assert_eq!(rows.len(), 55, "three boards, 55 clickable elements");
    let mut found = Vec::new();
    let mut ok_rows = 0;
    for board in ["PhoneDial", "PhoneIncoming", "PhoneInCall"] {
        let mut a = board_app(board);
        let canvas = render(a.ui()).unwrap();
        if let Ok(dir) = std::env::var("ULTAR_CONFORMANCE_OUT") {
            let png = Image::from_canvas(&canvas).encode_png().unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(Path::new(&dir).join(format!("{board}-app.png")), png).unwrap();
        }
        let mut seen: std::collections::HashMap<String, usize> = Default::default();
        for row in rows.iter().filter(|r| r.board == board) {
            let nth = seen.entry(row.action.clone()).or_default();
            let rect = element(&mut a, row, *nth);
            *nth += 1;
            let before = found.len();
            compare(row, rect, &canvas, &mut found);
            ok_rows += usize::from(found.len() == before);
        }
    }
    let mut new = Vec::new();
    for (id, detail) in &found {
        if known.lines().any(|l| l.trim() == id) {
            eprintln!("known difference {id}: {detail}");
        } else {
            new.push(format!("NEW difference {id}: {detail}"));
        }
    }
    let fixed: Vec<_> = known
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("phone/") && !found.iter().any(|(id, _)| id == l))
        .collect();
    eprintln!(
        "phone conformance: {ok_rows} of {} elements equal, {} differences",
        rows.len(),
        found.len()
    );
    assert!(new.is_empty(), "{}", new.join("\n"));
    assert!(
        fixed.is_empty(),
        "FIXED, remove from known-differences.txt: {fixed:?}"
    );
}
