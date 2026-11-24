//! Every Settings page at the owner's VMware screen (1024x768) and in a small window (640x480), in
//! Arabic and English: nothing sticks out at the sides of the window (no horizontal clipping: the content
//! reflows to the width, cards shrink or stack) and no text is squeezed narrower than its longest word
//! (text wraps between words, never one character per line). `ULTAR_CONFORMANCE_OUT=<dir>` also writes
//! the renders (`layout-<w>x<h>-<lang>-<area>.png`).

mod common;

use common::*;
use ultar_i18n::Lang;
use ultar_settings::areas::AREAS;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::Widget;

/// How far a child may reach over its parent's edge (a card's 16-point padding: full-width rules).
const BLEED: f32 = 16.5;
/// Points a character of the UI's body text is at least wide (13-15 point fonts, Arabic and Latin).
const MIN_CHAR: f32 = 4.0;

fn walk(ui: &Ui, id: NodeId, width: f32, problems: &mut Vec<String>) {
    let Some(r) = ui.logical_rect(id) else {
        return;
    };
    if r.width > 0.0 && r.height > 0.0 {
        if r.start < -0.5 || r.start + r.width > width + 0.5 {
            let what = ui.widget(id).map(widget_name).unwrap_or("?");
            problems.push(format!(
                "{what} {:?} outside the window ({:.0}..{:.0} of {width:.0})",
                id,
                r.start,
                r.start + r.width
            ));
        }
        if let Some(Widget::Text(t)) = ui.widget(id) {
            let text = t.label.resolve(ui.translator());
            let longest = text
                .split_whitespace()
                .map(|w| {
                    w.chars()
                        .filter(|c| !ultar_ui::text::is_bidi_control(*c))
                        .count()
                })
                .max()
                .unwrap_or(0);
            let need = (longest as f32 * MIN_CHAR).min(40.0);
            if longest >= 3 && r.width + 0.5 < need {
                problems.push(format!(
                    "text {text:?} only {:.0} points wide (its longest word needs ~{need:.0})",
                    r.width
                ));
            }
        }
    }
    // a child sticks out of its parent at the sides (cut off by the parent's card or column), except
    // where the parent scrolls
    let scrolls = matches!(ui.widget(id), Some(Widget::Scroll(_)));
    for &child in ui.children(id) {
        if let (false, Some(c)) = (scrolls, ui.logical_rect(child)) {
            let visible = c.width > 0.0 && c.height > 0.0 && r.width > 0.0;
            // up to BLEED: rules and highlights drawn over a card's padding
            if visible
                && (c.start < r.start - BLEED || c.start + c.width > r.start + r.width + BLEED)
            {
                let what = ui.widget(child).map(widget_name).unwrap_or("?");
                problems.push(format!(
                    "{what} {child:?} sticks out of its parent ({:.0}..{:.0} in {:.0}..{:.0})",
                    c.start,
                    c.start + c.width,
                    r.start,
                    r.start + r.width
                ));
            }
        }
        walk(ui, child, width, problems);
    }
}

fn widget_name(w: &Widget) -> &'static str {
    match w {
        Widget::Text(_) => "text",
        Widget::Card(_) => "card",
        Widget::Flex(_) => "flex",
        Widget::Row(_) => "row",
        Widget::Button(_) => "button",
        _ => "widget",
    }
}

#[test]
fn every_page_reflows_without_clipping_or_squeezed_text() {
    let out = std::env::var_os("ULTAR_CONFORMANCE_OUT").map(std::path::PathBuf::from);
    let mut problems = Vec::new();
    for (w, h) in [(1024.0, 768.0), (640.0, 480.0)] {
        for lang in [Lang::Ar, Lang::En] {
            for area in AREAS.iter() {
                for signed_in in [true, false] {
                    if !signed_in && area.id != "accounts" {
                        continue;
                    }
                    let world = board_world(2, signed_in);
                    let size = content_size(w, h);
                    let mut app = app(&world, lang, ThemeKind::Dark, size, area.id);
                    app.ui.layout();
                    let mut found = Vec::new();
                    walk(&app.ui, app.ui.root(), size.w, &mut found);
                    for p in found {
                        problems.push(format!("{w}x{h} {lang:?} {} ({signed_in}): {p}", area.id));
                    }
                    if let Some(dir) = &out {
                        let name = format!("layout-{w}x{h}-{lang:?}-{}-{signed_in}.png", area.id);
                        write_png(&dir.join(name), &mut app.ui);
                    }
                }
            }
        }
    }
    problems.dedup();
    assert!(
        problems.is_empty(),
        "{} layout problems:\n{}",
        problems.len(),
        problems
            .iter()
            .take(80)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
