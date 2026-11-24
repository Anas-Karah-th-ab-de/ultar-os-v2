//! Design conformance of the Settings app against its boards (`design/conformance/boards/*.tsv`,
//! from `tools/design-conformance/make_board_manifest.py`): every clickable element of the
//! board is looked up in the app (by action, then accessible name) and compared – box ±2 px at
//! 1x, the board's colour at the sample point (±6 per channel), disabled/checked state and the
//! accessible name. Results per element are OK / DIFFERENT (with what differs) / MISSING;
//! elements of the window frame (`win.*`, compositor), the phone shell (`mobile.*`) and the
//! "keep?" dialog (`display.keep*`, system dialog service) are reported as owned elsewhere.
//!
//! Ratchet: `design/conformance/known-boards.txt` lists every result that is not OK. A new
//! one fails, a listed one that became OK fails too ("remove it"). `ULTAR_CONFORMANCE_UPDATE=1`
//! rewrites the list; `ULTAR_CONFORMANCE_OUT=<dir>` writes `<Board>-results.tsv`.
//! Other theme/language combinations: every element exists, with a translated name.

mod common;

use common::*;
use std::collections::BTreeMap;
use ultar_i18n::Lang;
use ultar_settings::App;
use ultar_ui::canvas::Canvas;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::ui::{A11yNode, Role};
use ultar_ui::Size;

const SIZE_TOL: f32 = 2.0;
const COLOUR_TOL: i32 = 6;
const SETTINGS_BOARDS: [&str; 6] = [
    "SettingsDisplay",
    "SettingsDisplayLight",
    "SettingsDrivers",
    "Account",
    "CompactSettings",
    "CompactDisplay",
];

#[derive(Debug, Clone)]
struct Row {
    i: usize,
    act: String,
    name: String,
    disabled: bool,
    checked: Option<bool>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rgb: Option<[u8; 3]>,
    theme: ThemeKind,
    lang: Lang,
    offset: f32,
}

fn manifest(board: &str) -> Vec<Row> {
    let path = repo().join(format!("design/conformance/boards/{board}.tsv"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
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
                x: n(f[7]),
                y: n(f[8]),
                w: n(f[9]),
                h: n(f[10]),
                rgb,
                theme: if f[14] == "light" {
                    ThemeKind::Light
                } else {
                    ThemeKind::Dark
                },
                lang: if f[15] == "en" { Lang::En } else { Lang::Ar },
                offset: n(f[16]),
            }
        })
        .collect()
}

/// Where an action is implemented when it is not in this app.
fn owner(act: &str) -> Option<&'static str> {
    if act.starts_with("win.") {
        return Some(
            "OWNED compositor title bar (platform/ultar-compositor/tests/design_conformance.rs)",
        );
    }
    if act.starts_with("mobile.") {
        return Some("OWNED phone shell (w5/shell)");
    }
    if act.starts_with("display.keep") || act == "display.revert" {
        return Some(
            "OWNED system dialog service ultar-sysdialog (w5/shell), follows settings.pending",
        );
    }
    None
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

struct Built {
    app: App,
    canvas: Canvas,
    tree: Vec<A11yNode>,
}

fn build(board: &str, theme: ThemeKind, lang: Lang, signed_in: bool) -> Built {
    let compact = board.starts_with("Compact");
    let (size, displays) = if compact {
        (Size::new(360.0, 712.0), 1)
    } else {
        (Size::new(1440.0, 860.0), 3)
    };
    let area = match board {
        "SettingsDrivers" => "drivers",
        "Account" => "accounts",
        _ => "displays",
    };
    let world = board_world(displays, signed_in);
    let mut app = app(&world, lang, theme, size, area);
    if board == "CompactSettings" {
        app.state.compact_list = true;
        ultar_settings::view::build(&mut app.ui, &app.state, &mut app.view).expect("build");
    }
    if board == "Account" && !signed_in {
        // The board shows the form being filled in.
        use ultar_settings::action::Act;
        for (act, text) in [
            (Act::AccountUser, "anas"),
            (Act::AccountPassword, "km-secret!"),
            (Act::AccountServer, "ultar.code-craft.tech"),
        ] {
            if let Some(node) = app.view.node(act) {
                app.ui.set_text(node, text).expect("text");
            }
        }
    }
    let canvas = ultar_ui::snapshot::render(&mut app.ui).expect("render");
    let tree = app.ui.a11y_tree();
    Built { app, canvas, tree }
}

fn text(b: &Built, label: &Option<ultar_ui::widgets::Label>) -> String {
    label
        .as_ref()
        .map(|l| l.resolve(b.app.ui.translator()).into_owned())
        .unwrap_or_default()
}

/// Candidates of a board action: a11y nodes (and parts) of registered nodes.
fn candidates<'a>(b: &'a Built, act: &str) -> Vec<&'a A11yNode> {
    let owner_act = act;
    let ids: Vec<_> = b
        .app
        .view
        .entries()
        .into_iter()
        .filter(|(_, a)| a.id() == Some(owner_act))
        .map(|(n, _)| n)
        .collect();
    let part_role = match act {
        "display.select" | "display.mode" => Some(Role::Radio),
        _ => None,
    };
    b.tree
        .iter()
        .filter(|n| ids.contains(&n.id))
        .filter(|n| match part_role {
            Some(role) => n.part.is_some() && n.role == role,
            None => n.part.is_none(),
        })
        .collect()
}

fn name_score(b: &Built, row: &Row, n: &A11yNode) -> u8 {
    let name = norm(&text(b, &n.name));
    let value = norm(&text(b, &n.value));
    let want = norm(&row.name);
    if name.is_empty() {
        return 0;
    }
    if want == name {
        3
    } else if want == format!("{name}:{value}") || want == format!("{name}{value}") {
        2
    } else if want.starts_with(&name)
        || name.starts_with(&want)
        || (name.chars().count() >= 3 && want.contains(&name))
    {
        1
    } else {
        0
    }
}

/// The box the board marks for an element. Text fields: the board's action sits on the
/// `<input>` inside the field brick, i.e. the field less its 1-point border, its 12-point
/// inline padding and (search) the 16-point icon with its 8-point gap at the start edge; a
/// visible label (12-point line + 4 gap = 20) sits above the field.
fn input_box(n: &A11yNode, lang: Lang) -> ultar_ui::Rect {
    let r = n.rect;
    if n.role == Role::Slider && n.part.is_none() {
        // The board's slider is the track; the toolkit node also holds the value text slot
        // (48 + gap 12) at its end edge.
        let x = if lang == Lang::Ar { r.x + 60.0 } else { r.x };
        return ultar_ui::Rect::new(x, r.y, r.w - 60.0, r.h);
    }
    if !matches!(
        n.role,
        Role::TextField | Role::SearchField | Role::PasswordField
    ) || n.part.is_some()
    {
        return r;
    }
    let top = if r.h >= 56.0 { 24.0 } else { 0.0 };
    // An error draws a two-point edge: the input is one point smaller on every side.
    let edge = if n.error.is_some() { 2.0 } else { 1.0 };
    let (inset, icon) = (
        12.0 + edge,
        if n.role == Role::SearchField {
            24.0
        } else {
            0.0
        },
    );
    let x = if lang == Lang::Ar {
        r.x + inset
    } else {
        r.x + inset + icon
    };
    ultar_ui::Rect::new(
        x,
        r.y + top + edge,
        r.w - 2.0 * inset - icon,
        32.0 - 2.0 * edge,
    )
}

fn pixel(c: &Canvas, x: f32, y: f32) -> Option<[u8; 3]> {
    let (xi, yi) = (x as i64, y as i64);
    if xi < 0 || yi < 0 || xi >= i64::from(c.width()) || yi >= i64::from(c.height()) {
        return None;
    }
    let rgba = c.to_rgba();
    let i = (yi as usize * c.width() as usize + xi as usize) * 4;
    Some([rgba[i], rgba[i + 1], rgba[i + 2]])
}

/// What differs in the box (±2 points; an open value compares one anchored edge).
fn box_diff(row: &Row, r: ultar_ui::Rect) -> Option<String> {
    let (y, d) = (r.y + row.offset, |a: f32, b: f32| (a - b).abs() > SIZE_TOL);
    // A board value the specification leaves open ("—", sizes measured at run time) makes
    // the element as wide as that dash; with real data only the start edge, the top and the
    // height can be compared.
    let open = row.name.contains('\u{2014}');
    let (x_off, w_off) = if !open {
        (d(r.x, row.x), d(r.w, row.w))
    } else if row.lang == Lang::Ar {
        (d(r.x + r.w, row.x + row.w), false)
    } else {
        (d(r.x, row.x), false)
    };
    if x_off || d(y, row.y) || w_off || d(r.h, row.h) {
        return Some(format!(
            "box {:.0},{:.0} {:.0}x{:.0} (board {:.0},{:.0} {:.0}x{:.0})",
            r.x, y, r.w, r.h, row.x, row.y, row.w, row.h
        ));
    }
    None
}

fn compare(
    b: &Built,
    row: &Row,
    used: &mut Vec<(ultar_ui::ui::NodeId, Option<usize>)>,
) -> Option<String> {
    let cands: Vec<&A11yNode> = candidates(b, &row.act)
        .into_iter()
        .filter(|n| !used.contains(&(n.id, n.part)))
        .collect();
    let best = cands
        .iter()
        .enumerate()
        .min_by_key(|(i, n)| (std::cmp::Reverse(name_score(b, row, n)), *i))
        .map(|(_, n)| *n)?;
    used.push((best.id, best.part));
    let mut diff = Vec::new();
    if name_score(b, row, best) == 0 {
        diff.push(format!("name '{}'", text(b, &best.name)));
    }
    if let Some(d) = box_diff(row, input_box(best, row.lang)) {
        diff.push(d);
    }
    if let Some(want) = row.rgb {
        let px = if row.w < 16.0 {
            row.x + row.w / 2.0
        } else {
            row.x + 6.0
        };
        if let Some(got) = pixel(&b.canvas, px, row.y + row.h / 2.0 - row.offset) {
            if (0..3).any(|i| (i32::from(got[i]) - i32::from(want[i])).abs() > COLOUR_TOL) {
                diff.push(format!("colour {got:?} (board {want:?})"));
            }
        }
    }
    if best.state.disabled != row.disabled {
        diff.push(format!("disabled {}", best.state.disabled));
    }
    if row.checked.is_some() && best.state.checked != row.checked {
        diff.push(format!("checked {:?}", best.state.checked));
    }
    Some(if diff.is_empty() {
        "OK".into()
    } else {
        format!("DIFFERENT {}", diff.join("; "))
    })
}

fn results(board: &str) -> Vec<(usize, String, String)> {
    let rows = manifest(board);
    let Some(first) = rows.first() else {
        return Vec::new();
    };
    // Account: the board shows the signed-out card beside the signed-in one; the signed-out
    // build is asked first (its status card), the signed-in one for the rest.
    let mut builds = Vec::new();
    if board == "Account" {
        builds.push(build(board, first.theme, first.lang, false));
    }
    builds.push(build(board, first.theme, first.lang, true));
    let mut used: Vec<Vec<_>> = vec![Vec::new(); builds.len()];
    let mut out = Vec::new();
    for row in &rows {
        if let Some(o) = owner(&row.act) {
            out.push((row.i, row.act.clone(), o.to_owned()));
            continue;
        }
        let status = builds
            .iter()
            .enumerate()
            .find_map(|(k, b)| compare(b, row, &mut used[k]))
            .unwrap_or_else(|| "MISSING".into());
        out.push((row.i, row.act.clone(), status));
    }
    if let Some(dir) = std::env::var_os("ULTAR_CONFORMANCE_OUT") {
        let dir = std::path::PathBuf::from(dir);
        let lines: Vec<String> = out
            .iter()
            .map(|(i, a, s)| format!("{board}\t{i}\t{a}\t{s}"))
            .collect();
        std::fs::create_dir_all(&dir).expect("out");
        std::fs::write(
            dir.join(format!("{board}-results.tsv")),
            lines.join("\n") + "\n",
        )
        .expect("write");
        write_png(&dir.join(format!("{board}-app.png")), &mut builds[0].app.ui);
    }
    out
}

fn summary_key(status: &str) -> &str {
    status.split(' ').next().unwrap_or(status)
}

#[test]
fn boards_match_or_are_listed() {
    let known_path = repo().join("design/conformance/known-boards.txt");
    let known_text = std::fs::read_to_string(&known_path).unwrap_or_default();
    let mine: Vec<String> = known_text
        .lines()
        .filter(|l| {
            !l.starts_with('#')
                && SETTINGS_BOARDS
                    .iter()
                    .any(|b| l.starts_with(&format!("{b}#")))
        })
        .map(str::to_owned)
        .collect();
    let mut now = Vec::new();
    let mut totals: BTreeMap<String, usize> = BTreeMap::new();
    for board in SETTINGS_BOARDS {
        for (i, act, status) in results(board) {
            *totals
                .entry(format!("{board} {}", summary_key(&status)))
                .or_default() += 1;
            if status != "OK" {
                now.push(format!("{board}#{i}\t{act}\t{status}"));
            }
        }
    }
    eprintln!("{totals:#?}");
    if std::env::var_os("ULTAR_CONFORMANCE_UPDATE").is_some() {
        let others: Vec<&str> = known_text
            .lines()
            .filter(|l| !mine.iter().any(|m| m == l))
            .collect();
        let mut all: Vec<String> = others.iter().map(|s| (*s).to_owned()).collect();
        all.extend(now.iter().cloned());
        std::fs::write(&known_path, all.join("\n") + "\n").expect("write known");
        return;
    }
    let new: Vec<&String> = now.iter().filter(|l| !mine.contains(l)).collect();
    let gone: Vec<&String> = mine.iter().filter(|l| !now.contains(l)).collect();
    assert!(
        new.is_empty() && gone.is_empty(),
        "new differences: {new:#?}\nresolved (remove from known-boards.txt): {gone:#?}"
    );
}

#[test]
fn every_element_exists_in_all_themes_and_languages() {
    for board in SETTINGS_BOARDS {
        let rows = manifest(board);
        for (theme, lang) in [
            (ThemeKind::Dark, Lang::Ar),
            (ThemeKind::Light, Lang::Ar),
            (ThemeKind::Dark, Lang::En),
            (ThemeKind::Light, Lang::En),
        ] {
            let mut builds = vec![build(board, theme, lang, true)];
            if board == "Account" {
                builds.push(build(board, theme, lang, false));
            }
            let mut need: BTreeMap<&str, usize> = BTreeMap::new();
            for r in rows.iter().filter(|r| owner(&r.act).is_none()) {
                *need.entry(r.act.as_str()).or_default() += 1;
            }
            for (act, count) in need {
                let have = builds
                    .iter()
                    .map(|b| candidates(b, act).len())
                    .max()
                    .unwrap_or(0);
                assert!(
                    have >= count,
                    "{board} {theme:?} {lang:?}: {act} {have} of {count}"
                );
            }
            for b in &builds {
                for n in &b.tree {
                    let name = text(b, &n.name);
                    assert!(
                        !name.starts_with("app.settings.") && !name.starts_with("settings."),
                        "{board} {lang:?}: untranslated {name}"
                    );
                }
            }
        }
    }
}
