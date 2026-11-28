//! Design conformance of the phone's hand-over surfaces against their boards
//! (`design/conformance/boards/{CompactMeshSend,CompactMeshIncoming}.tsv`, made by
//! `tools/design-conformance/make_board_manifest.py`): every clickable element of a board is
//! looked up in the build (by what it does, in reading order) and compared – box ±2 px, the
//! board's colour at the sample point (±6 per channel), disabled and checked state, and the
//! accessible name. The result per element is OK, DIFFERENT (with what differs) or MISSING;
//! the tests require OK for all of them except two named deviations:
//!
//! * the device cards are the toolkit's radio cards (padding 12, a dot), 69/67 px high where
//!   the board's (padding 8, icon, check) are 61.6/59.6 – `ultar-ui` has no device-card
//!   layout yet (docs/ui/mesh.md §4);
//! * the phone's buttons are touch targets of at least 44 px (the board's are 32 px inside a
//!   44 px row): same top edge and width, 12 px taller.

use ultar_bus::messages::mesh::{DeviceKind, SendOutcome, Stage};
use ultar_bus::messages::mesh_dialog::AskHandoff;
use ultar_dialog_core::{Dialogs, Question};
use ultar_mobile::click::Click;
use ultar_mobile::dialog::{build, DialogClick};
use ultar_mobile::model::Screen;
use ultar_mobile::{Look, MobileApp};
use ultar_ui::geom::{Rect, Size};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{A11yNode, Role, Ui};
use ultar_ui::Lang;

mod common;

const BOARD: Size = Size::new(360.0, 800.0);
const SIZE_TOL: f32 = 2.0;
const COLOUR_TOL: i32 = 6;

#[derive(Debug, Clone)]
struct Row {
    i: usize,
    act: String,
    name: String,
    disabled: bool,
    checked: Option<bool>,
    rect: Rect,
    rgb: Option<[u8; 3]>,
}

fn manifest(board: &str) -> Vec<Row> {
    let path = format!(
        "{}/../../design/conformance/boards/{board}.tsv",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let n = |s: &str| s.parse::<f32>().expect("number");
            let rgb = (f[11] != "-").then(|| {
                let c: Vec<u8> = f[11].split(',').map(|v| v.parse().expect("rgb")).collect();
                [c[0], c[1], c[2]]
            });
            Row {
                i: f[1].parse().expect("i"),
                act: f[2].into(),
                name: f[4].into(),
                disabled: f[5] == "1",
                checked: match f[6] {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => None,
                },
                rect: Rect::new(n(f[7]), n(f[8]), n(f[9]), n(f[10])),
                rgb,
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
struct Found {
    name: String,
    rect: Rect,
    disabled: bool,
    checked: Option<bool>,
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| {
            !c.is_whitespace()
                && !('\u{2066}'..='\u{2069}').contains(c)
                && !matches!(c, '\u{200e}' | '\u{200f}' | '\u{200b}')
        })
        .collect()
}

fn found(ui: &Ui, node: &A11yNode) -> Found {
    Found {
        name: node
            .name
            .as_ref()
            .map(|l| l.resolve(ui.translator()).into_owned())
            .unwrap_or_default(),
        rect: node.rect,
        disabled: node.state.disabled,
        checked: node.state.checked,
    }
}

fn name_ok(want: &str, got: &str) -> bool {
    let (w, n) = (norm(want), norm(got));
    !n.is_empty() && (w == n || w.starts_with(&n) || n.starts_with(&w) || w.contains(&n))
}

fn compare(row: &Row, got: &Found, canvas: &ultar_ui::canvas::Canvas) -> String {
    let mut diff = Vec::new();
    if !name_ok(&row.name, &got.name) {
        diff.push(format!("name '{}' (board '{}')", got.name, row.name));
    }
    let (r, b) = (got.rect, row.rect);
    let off = |x: f32, y: f32| (x - y).abs() > SIZE_TOL;
    if off(r.x, b.x) || off(r.y, b.y) || off(r.w, b.w) || off(r.h, b.h) {
        diff.push(format!(
            "box {:.1},{:.1} {:.1}x{:.1} (board {:.1},{:.1} {:.1}x{:.1})",
            r.x, r.y, r.w, r.h, b.x, b.y, b.w, b.h
        ));
    }
    if let Some(want) = row.rgb {
        let (x, y) = ((r.x + 6.0) as usize, (r.y + r.h / 2.0) as usize);
        if x < canvas.width() as usize && y < canvas.height() as usize {
            let rgba = canvas.to_rgba();
            let i = (y * canvas.width() as usize + x) * 4;
            let got = [rgba[i], rgba[i + 1], rgba[i + 2]];
            if (0..3).any(|k| (i32::from(got[k]) - i32::from(want[k])).abs() > COLOUR_TOL) {
                diff.push(format!("colour {got:?} (board {want:?})"));
            }
        }
    }
    if got.disabled != row.disabled {
        diff.push(format!("disabled {}", got.disabled));
    }
    if row.checked.is_some() && got.checked != row.checked {
        diff.push(format!("checked {:?}", got.checked));
    }
    if diff.is_empty() {
        "OK".into()
    } else {
        format!("DIFFERENT {}", diff.join("; "))
    }
}

/// The two named deviations (see the header): a device card of the toolkit, a touch-size
/// button. Everything but the box (and the unchosen card's surface colour) must still match.
fn known(row: &Row, got: &Found) -> bool {
    let (r, b) = (got.rect, row.rect);
    let same = name_ok(&row.name, &got.name)
        && got.disabled == row.disabled
        && (row.checked.is_none() || got.checked == row.checked)
        && (r.x - b.x).abs() <= SIZE_TOL
        && (r.w - b.w).abs() <= SIZE_TOL;
    match row.act.as_str() {
        "mesh.send.target" => same && (r.y - b.y).abs() < 45.0 && (r.h - b.h).abs() < 10.0,
        "mesh.send" | "mesh.send.cancel" | "mesh.accept" | "mesh.decline" => {
            same && (r.y - b.y).abs() <= SIZE_TOL && (r.h - b.h).abs() <= 12.5
        }
        _ => false,
    }
}

/// Compares every row of `board` with the pool; returns (problems, number of known ones).
fn run(
    board: &str,
    pool: &[(String, Found)],
    canvas: &ultar_ui::canvas::Canvas,
) -> (Vec<String>, usize) {
    let rows = manifest(board);
    let (mut bad, mut accepted) = (Vec::new(), 0);
    let mut next: std::collections::BTreeMap<&str, usize> = Default::default();
    for row in &rows {
        let k = next.entry(row.act.as_str()).or_insert(0);
        let got = pool.iter().filter(|(a, _)| *a == row.act).nth(*k);
        *k += 1;
        let status = match got {
            None => "MISSING".to_owned(),
            Some((_, f)) => {
                let s = compare(row, f, canvas);
                if s != "OK" && known(row, f) {
                    accepted += 1;
                    "OK".into()
                } else {
                    s
                }
            }
        };
        if status != "OK" {
            bad.push(format!("{board}#{}\t{}\t{status}", row.i, row.act));
        }
    }
    (bad, accepted)
}

// ---- board CompactMeshSend -----------------------------------------------------------

fn send_app(lang: Lang, theme: ThemeKind) -> MobileApp {
    let mut m = common::sample(false);
    m.mesh
        .sent("org.ultar.files", SendOutcome::Choose(common::targets()), 0);
    m.go(Screen::Mesh);
    MobileApp::new(Look::phone(lang, theme), BOARD, m).expect("builds")
}

fn send_pool(a: &mut MobileApp) -> Vec<(String, Found)> {
    a.ui().layout();
    let mut pool = Vec::new();
    for (act, click) in [
        ("mesh.back", Click::MeshBack),
        ("mesh.send", Click::MeshConfirm),
        ("mesh.send.cancel", Click::MeshCancel),
    ] {
        if let Some(id) = a.built().node(&click) {
            if let Some(n) = a.ui().a11y_node(id) {
                pool.push((act.to_owned(), found(a.ui(), &n)));
            }
        }
    }
    let mut cards: Vec<A11yNode> = a
        .ui()
        .a11y_tree()
        .into_iter()
        .filter(|n| n.role == Role::Radio && n.part.is_some())
        .collect();
    cards.sort_by(|x, y| x.rect.y.total_cmp(&y.rect.y));
    for n in &cards {
        pool.push(("mesh.send.target".to_owned(), found(a.ui(), n)));
    }
    pool
}

#[test]
fn the_choice_matches_board_compact_mesh_send() {
    let mut a = send_app(Lang::Ar, ThemeKind::Dark);
    let pool = send_pool(&mut a);
    let canvas = ultar_ui::snapshot::render(a.ui()).expect("renders");
    let (bad, accepted) = run("CompactMeshSend", &pool, &canvas);
    assert!(bad.is_empty(), "board CompactMeshSend:\n{}", bad.join("\n"));
    assert_eq!(
        manifest("CompactMeshSend").len(),
        5,
        "five clickable elements"
    );
    // Exactly the cards (2) and the two buttons: the named deviations, nothing else.
    assert_eq!(accepted, 4, "the known deviations cover exactly four boxes");
}

// ---- board CompactMeshIncoming -------------------------------------------------------

fn incoming(lang: Lang, theme: ThemeKind) -> (Ui, Vec<(String, Found)>) {
    let tr = ultar_mobile::i18n::translator(lang).expect("catalogs");
    let mut ui = Ui::new(PaintEnv::new(lang, theme), InputMode::Touch, BOARD, tr).expect("ui");
    let mut d = Dialogs::default();
    d.ask(
        1,
        Question::Handoff(AskHandoff {
            from_name: "ultar-pc".into(),
            from_kind: DeviceKind::Desktop,
            title: "notes.txt".into(),
            app: "editor".into(),
            size: Some(1_258_291),
            stage: Stage::State,
        }),
        0,
    );
    let built = build(&mut ui, &d, 0).expect("builds").expect("a question");
    ui.layout();
    let mut pool = Vec::new();
    for (act, click) in [
        ("mesh.accept", DialogClick::Accept),
        ("mesh.decline", DialogClick::Safe),
    ] {
        let id = built.node(click).expect("button");
        let n = ui.a11y_node(id).expect("node");
        pool.push((act.to_owned(), found(&ui, &n)));
    }
    (ui, pool)
}

#[test]
fn the_question_matches_board_compact_mesh_incoming() {
    let (mut ui, pool) = incoming(Lang::Ar, ThemeKind::Dark);
    let canvas = ultar_ui::snapshot::render(&mut ui).expect("renders");
    let (bad, accepted) = run("CompactMeshIncoming", &pool, &canvas);
    assert!(
        bad.is_empty(),
        "board CompactMeshIncoming:\n{}",
        bad.join("\n")
    );
    assert_eq!(manifest("CompactMeshIncoming").len(), 2);
    assert!(accepted <= 2, "only the touch-size buttons may differ");
}

#[test]
fn every_element_has_a_translated_name_in_both_languages_and_themes() {
    for lang in [Lang::Ar, Lang::En, Lang::De] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            let mut a = send_app(lang, theme);
            let pool = send_pool(&mut a);
            let (_, offer) = incoming(lang, theme);
            assert_eq!(pool.len(), 5, "{lang:?} {theme:?}");
            for (act, f) in pool.iter().chain(&offer) {
                assert!(
                    !f.name.is_empty() && !f.name.contains("mobile."),
                    "{lang:?} {theme:?} {act}: untranslated name '{}'",
                    f.name
                );
            }
        }
    }
}
