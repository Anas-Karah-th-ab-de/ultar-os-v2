//! Design conformance of the hand-over surfaces of the desktop against their boards
//! (`design/conformance/boards/{MeshSend,MeshIncoming,MeshStates}.tsv`, made by
//! `tools/design-conformance/make_board_manifest.py`): every clickable element of a board is
//! looked up in the build (by what it does, in reading order) and compared – box ±2 px,
//! the board's colour at the sample point (±6 per channel), disabled and checked state, and
//! the accessible name. The result per element is OK, DIFFERENT (with what differs) or
//! MISSING; the test requires OK for all of them except the elements that belong to the
//! Settings page the board shows behind its toasts (`mesh.toggle`, `mesh.pair`, the
//! "port could not be opened" Retry), which `apps/ultar-settings` owns.
//!
//! The question on the receiving device lives in the compositor's trusted frame (centred in
//! the work area under its title bar); the toasts are the toolkit's, compared relative to
//! the toast's own close button (the board lays six toasts out as a list, the shell shows at
//! most three at the bottom edge).

mod common;

use common::*;
use std::collections::BTreeMap;
use ultar_bus::messages::mesh::{DeviceKind, MeshState, Peer, Stage, Target as Device};
use ultar_bus::messages::mesh_dialog::AskHandoff;
use ultar_desktop::dialog::{Dialogs, Question};
use ultar_desktop::model::mesh::{Act, Kind};
use ultar_desktop::model::{Desktop, Open};
use ultar_desktop::shell::Outcome;
use ultar_desktop::view::mesh_toasts;
use ultar_desktop::view::{Look, Target, View};
use ultar_i18n::Lang;
use ultar_ui::tokens::{size, ThemeKind};
use ultar_ui::ui::{A11yNode, Role};
use ultar_ui::Rect;

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

/// What the build offers for a board element, in screen coordinates.
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

fn found(view: &mut View, node: &A11yNode, at: (f32, f32)) -> Found {
    let name = node
        .name
        .as_ref()
        .map(|l| l.resolve(view.ui.translator()).into_owned())
        .unwrap_or_default();
    Found {
        name,
        rect: Rect::new(node.rect.x + at.0, node.rect.y + at.1, node.rect.w, node.rect.h),
        disabled: node.state.disabled,
        checked: node.state.checked,
    }
}

/// The element of `target` in the view.
fn of_target(view: &mut View, target: &Target, at: (f32, f32)) -> Option<Found> {
    let id = view.node(target)?;
    let node = view.ui.a11y_node(id)?;
    Some(found(view, &node, at))
}

/// One board element against what was found.
fn compare(row: &Row, got: &Found, canvas: &ultar_ui::canvas::Canvas, at: (f32, f32)) -> String {
    let mut diff = Vec::new();
    let want = norm(&row.name);
    let name = norm(&got.name);
    let name_ok = !name.is_empty()
        && (want == name || want.starts_with(&name) || name.starts_with(&want) || want.contains(&name));
    if !name_ok {
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
        let px = if b.w < 16.0 { r.x + r.w / 2.0 } else { r.x + 6.0 };
        let (x, y) = ((px - at.0) as i64, (r.y + r.h / 2.0 - at.1) as i64);
        if x >= 0 && y >= 0 && x < i64::from(canvas.width()) && y < i64::from(canvas.height()) {
            let rgba = canvas.to_rgba();
            let i = (y as usize * canvas.width() as usize + x as usize) * 4;
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

fn peer(id: &str, name: &str, kind: DeviceKind, reachable: bool) -> Peer {
    Peer {
        id: id.into(),
        name: name.into(),
        kind,
        capabilities: Vec::new(),
        paired: true,
        reachable,
        rights: Vec::new(),
    }
}

// ---- board MeshSend ------------------------------------------------------------------

fn send_surface(lang: Lang, theme: ThemeKind) -> (Harness, Vec<(String, Found)>) {
    let mut d = board_desktop(lang, theme);
    d.windows = vec![window(1, "org.ultar.editor", "notes.txt", true)];
    d.svc.mesh.state = Some(MeshState {
        enabled: true,
        id: "me".into(),
        name: "ultar-pc".into(),
        peers: vec![
            peer("km-one", "KM One", DeviceKind::Phone, true),
            peer("beelink", "beelink", DeviceKind::Server, true),
            peer("office", "ultar-office", DeviceKind::Desktop, false),
        ],
    });
    let device = |id: &str, name: &str, kind, stage| Device {
        device: id.into(),
        name: name.into(),
        kind,
        stage,
    };
    d.svc.mesh.targets = vec![
        device("km-one", "KM One", DeviceKind::Phone, Stage::State),
        device("beelink", "beelink", DeviceKind::Server, Stage::File),
    ];
    d.open = Open::MeshSend;
    let mut h = Harness::with(d, lang, theme);
    h.apply(Outcome::surfaces());
    let popup = h.popup.as_mut().expect("the choice is open");
    let at = (
        popup.offset.0 as f32,
        (SCREEN.h - size::BAR_HEIGHT) + popup.offset.1 as f32,
    );
    let tree = popup.view.ui.a11y_tree();
    let mut radios: Vec<&A11yNode> = tree
        .iter()
        .filter(|n| n.part.is_some() && n.role == Role::Radio)
        .collect();
    radios.sort_by(|a, b| a.rect.y.total_cmp(&b.rect.y));
    let radios: Vec<A11yNode> = radios.into_iter().cloned().collect();
    let mut out = Vec::new();
    for n in &radios {
        out.push(("mesh.send.target".to_owned(), found(&mut popup.view, n, at)));
    }
    for (act, target) in [
        ("mesh.send", Target::MeshSend),
        ("mesh.send.cancel", Target::MeshSendCancel),
    ] {
        if let Some(f) = of_target(&mut popup.view, &target, at) {
            out.push((act.to_owned(), f));
        }
    }
    (h, out)
}

fn ordered(rows: &[Row], pool: &[(String, Found)]) -> Vec<(Row, Option<Found>)> {
    let mut next: BTreeMap<&str, usize> = BTreeMap::new();
    rows.iter()
        .map(|row| {
            let k = next.entry(row.act.as_str()).or_insert(0);
            let found = pool
                .iter()
                .filter(|(a, _)| *a == row.act)
                .nth(*k)
                .map(|(_, f)| f.clone());
            *k += 1;
            (row.clone(), found)
        })
        .collect()
}

fn report(board: &str, results: &[(Row, String)]) -> Vec<String> {
    let mut bad = Vec::new();
    for (row, status) in results {
        if status != "OK" {
            bad.push(format!("{board}#{}\t{}\t{status}", row.i, row.act));
        }
    }
    bad
}

#[test]
fn the_choice_of_devices_matches_board_mesh_send() {
    let (mut h, pool) = send_surface(Lang::Ar, ThemeKind::Dark);
    let popup = h.popup.as_mut().expect("open");
    let canvas = render(&mut popup.view, popup.size);
    let at = (
        popup.offset.0 as f32,
        (SCREEN.h - size::BAR_HEIGHT) + popup.offset.1 as f32,
    );
    let rows = manifest("MeshSend");
    let mut known = 0;
    let results: Vec<(Row, String)> = ordered(&rows, &pool)
        .into_iter()
        .map(|(row, got)| {
            let status = got.map_or("MISSING".to_owned(), |g| {
                let status = compare(&row, &g, &canvas, at);
                if status != "OK" && known_card_height(&row, &g) {
                    known += 1;
                    return "OK".to_owned();
                }
                status
            });
            (row, status)
        })
        .collect();
    let bad = report("MeshSend", &results);
    assert!(bad.is_empty(), "board MeshSend:\n{}", bad.join("\n"));
    assert_eq!(results.len(), 5, "the board's five clickable elements");
    // The deviation is exactly these five boxes: if the toolkit gets a device-card layout
    // this count drops to 0 and this allowance has to go (docs/ui/mesh.md §4).
    assert_eq!(known, 5, "the known card-height deviation covers exactly five boxes");
}

/// KNOWN DEVIATION (docs/ui/mesh.md §4): `ultar-ui` has no device-card radio layout, so the
/// toolkit's cards (padding 12, dot) are 69/67/67 px high where the board's (padding 8,
/// icon, check) are 61.6/59.6/59.6, and everything below them (the dialog is centred) sits
/// higher by the sum. Only this is allowed: same x and width, y and height off by less than
/// 45 and 10 px (the colour of an unchosen card is the toolkit's surface).
fn known_card_height(row: &Row, got: &Found) -> bool {
    let (r, b) = (got.rect, row.rect);
    let same_name = {
        let (w, n) = (norm(&row.name), norm(&got.name));
        !n.is_empty() && (w == n || w.starts_with(&n) || n.starts_with(&w) || w.contains(&n))
    };
    same_name
        && got.disabled == row.disabled
        && got.checked == row.checked
        && (r.x - b.x).abs() <= SIZE_TOL
        && (r.w - b.w).abs() <= SIZE_TOL
        && (r.y - b.y).abs() < 45.0
        && (r.h - b.h).abs() < 10.0
}

// ---- board MeshIncoming --------------------------------------------------------------

fn incoming_surface(lang: Lang, theme: ThemeKind) -> (View, (f32, f32), Vec<(String, Found)>) {
    let tr = ultar_desktop::i18n::translator(lang).expect("catalogs");
    let look = Look::new(lang, theme);
    let mut dialogs = Dialogs::default();
    dialogs.ask(
        1,
        Question::Handoff(AskHandoff {
            from_name: "KM One".into(),
            from_kind: DeviceKind::Phone,
            title: "notes.txt".into(),
            app: "org.ultar.editor".into(),
            size: Some(1_258_291),
            stage: Stage::State,
        }),
        0,
    );
    let mut view = ultar_desktop::dialog::view::build(&dialogs, 0, look, &tr)
        .expect("builds")
        .expect("a question");
    let content = view.ui.content_size();
    // The board's page is not the compositor's frame: it places the dialog by its own
    // layout. So the build is anchored on the bottom of the Accept button (the board's
    // 597.6) and the height of the content is asserted on its own (board: 342.1).
    assert!(
        lang != Lang::Ar || theme != ThemeKind::Dark || (content.h - 342.14).abs() <= 3.0,
        "content height {} (board 342.14)",
        content.h
    );
    let accept = of_target(&mut view, &Target::AllowOnce, (0.0, 0.0)).expect("accept");
    let at = (
        ((SCREEN.w - ultar_desktop::dialog::view::WIDTH) / 2.0).floor(),
        597.6 - accept.rect.bottom(),
    );
    let mut pool = Vec::new();
    for (act, target) in [
        ("mesh.decline", Target::Deny),
        ("mesh.accept", Target::AllowOnce),
    ] {
        if let Some(f) = of_target(&mut view, &target, at) {
            pool.push((act.to_owned(), f));
        }
    }
    (view, at, pool)
}

#[test]
fn the_question_matches_board_mesh_incoming() {
    let (mut view, at, pool) = incoming_surface(Lang::Ar, ThemeKind::Dark);
    let size = view.ui.size();
    let canvas = render(&mut view, size);
    let rows = manifest("MeshIncoming");
    let results: Vec<(Row, String)> = ordered(&rows, &pool)
        .into_iter()
        .map(|(row, got)| {
            let status = got.map_or("MISSING".to_owned(), |g| compare(&row, &g, &canvas, at));
            (row, status)
        })
        .collect();
    let bad = report("MeshIncoming", &results);
    assert!(bad.is_empty(), "board MeshIncoming:\n{}", bad.join("\n"));
    assert_eq!(results.len(), 2);
}

// ---- board MeshStates ----------------------------------------------------------------

/// The six toasts of the board, in order: what they say and their button.
fn board_toasts() -> [(Kind, &'static str, &'static str, Option<Act>); 6] {
    [
        (Kind::Waiting, "KM One", "notes.txt", Some(Act::Cancel(1))),
        (Kind::Opened, "KM One", "notes.txt", None),
        (Kind::Declined, "KM One", "notes.txt", None),
        (Kind::Expired, "KM One", "notes.txt", Some(Act::Retry("km-one".into()))),
        (Kind::Failed, "KM One", "notes.txt", Some(Act::Retry("km-one".into()))),
        (
            Kind::Received,
            "beelink",
            "report.pdf",
            Some(Act::Open("file:///data/inbox/report.pdf".into())),
        ),
    ]
}

/// One toast built alone: what the a11y tree offers (screen coordinates of its own surface).
fn toast_surface(lang: Lang, theme: ThemeKind, n: usize) -> (View, Vec<(String, Found)>, Rect) {
    let (kind, name, title, act) = board_toasts()[n].clone();
    let mut d = Desktop::new(lang);
    d.svc.mesh.toast(kind, name, title, act.clone());
    let tr = ultar_desktop::i18n::translator(lang).expect("catalogs");
    let look = Look::new(lang, theme);
    let (mut view, _) = mesh_toasts::build(&d, (look, SCREEN, 0), false, &tr)
        .expect("builds")
        .expect("a toast");
    let tree = view.ui.a11y_tree();
    let mut pool = Vec::new();
    let mut whole = Rect::new(0.0, 0.0, 0.0, 0.0);
    for node in &tree {
        match (node.role, node.part) {
            (Role::Status, Some(_)) => whole = node.rect,
            (Role::Button, Some(p)) => {
                let is_close = view
                    .ui
                    .translator()
                    .t("ui.toast.close")
                    .as_ref()
                    .eq(node
                        .name
                        .as_ref()
                        .map(|l| l.resolve(view.ui.translator()).into_owned())
                        .unwrap_or_default()
                        .as_str());
                let act = match (&act, is_close) {
                    (_, true) => "toast.close".to_owned(),
                    (Some(Act::Cancel(_)), _) => "mesh.offer.cancel".to_owned(),
                    (Some(Act::Retry(_)), _) => "mesh.retry".to_owned(),
                    (Some(Act::Open(_)), _) => "mesh.received.open".to_owned(),
                    (None, _) => format!("unknown{p}"),
                };
                let f = found(&mut view, node, (0.0, 0.0));
                pool.push((act, f));
            }
            _ => {}
        }
    }
    (view, pool, whole)
}

#[test]
fn every_toast_matches_board_mesh_states() {
    let rows: Vec<Row> = manifest("MeshStates");
    // The toasts of the board by the y of their close button.
    let closes: Vec<&Row> = rows.iter().filter(|r| r.act == "toast.close").collect();
    assert_eq!(closes.len(), 6, "six toasts on the board");
    let mut bad = Vec::new();
    let mut checked = 0;
    for (n, close) in closes.iter().enumerate() {
        let (mut view, pool, _) = toast_surface(Lang::Ar, ThemeKind::Dark, n);
        let ours = pool
            .iter()
            .find(|(a, _)| a == "toast.close")
            .map(|(_, f)| f.rect)
            .expect("the close button");
        // Compare relative to the close button: the board lays the toasts out as a list.
        let (dx, dy) = (close.rect.x - ours.x, close.rect.y - ours.y);
        let canvas = render(&mut view, SCREEN);
        // Its buttons: the board rows on the same line, in the toasts' column (the rows at
        // the page's edges belong to the Settings page behind the toasts).
        let group = rows
            .iter()
            .filter(|r| (r.rect.y - close.rect.y).abs() < 20.0)
            .filter(|r| r.act != "mesh.toggle" && r.act != "mesh.pair" && r.rect.x > 500.0);
        for row in group {
            checked += 1;
            let Some((_, got)) = pool.iter().find(|(a, _)| *a == row.act) else {
                bad.push(format!("MeshStates#{}\t{}\tMISSING", row.i, row.act));
                continue;
            };
            let mut moved = got.clone();
            moved.rect = Rect::new(got.rect.x + dx, got.rect.y + dy, got.rect.w, got.rect.h);
            let status = compare(row, &moved, &canvas, (dx, dy));
            if status != "OK" {
                bad.push(format!("MeshStates#{}\t{}\t{status}", row.i, row.act));
            }
        }
    }
    assert!(bad.is_empty(), "board MeshStates:\n{}", bad.join("\n"));
    assert_eq!(checked, 10, "ten toast buttons on the board");
}

// ---- other themes and languages --------------------------------------------------------

#[test]
fn every_element_exists_with_a_translated_name_in_both_languages_and_themes() {
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            let (_, pool) = send_surface(lang, theme);
            assert_eq!(pool.len(), 5, "{lang:?} {theme:?}: three devices, Send, Cancel");
            let (_, _, incoming) = incoming_surface(lang, theme);
            assert_eq!(incoming.len(), 2);
            for n in 0..6 {
                let (_, toast, whole) = toast_surface(lang, theme, n);
                assert!(whole.w > 0.0, "the toast is in the tree");
                pool.iter().chain(&incoming).chain(&toast).for_each(|(act, f)| {
                    assert!(
                        !f.name.is_empty() && !f.name.contains("desk.") && !f.name.contains("dialog."),
                        "{lang:?} {theme:?} {act}: untranslated name '{}'",
                        f.name
                    );
                });
            }
        }
    }
}
