//! "What the helper did" on Settings › Assistant (board *AssistantReach*): the last
//! requests, what was touched for each and how it went. The switches above it – whether
//! the helper acts, and in which areas – are ordinary settings of the panel; the helper
//! itself can change none of them. Requests and details are data.

use super::{k, section, Cx};
use crate::action::Act;
use std::time::{SystemTime, UNIX_EPOCH};
use ultar_bus::messages::agent::{Entry, StepOutcome};
use ultar_ui::layout::Sizing;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Badge, Button, ButtonVariant, Label, Row, Tone};

/// Most requests shown, newest first.
pub const MAX_SHOWN: usize = 10;

/// How a request went as a whole: its worst step (`None`: it only answered).
pub fn verdict(entry: &Entry) -> Option<(&'static str, Tone)> {
    let has = |outcome| entry.steps.iter().any(|s| s.outcome == outcome);
    if entry.steps.is_empty() {
        None
    } else if has(StepOutcome::NotAllowed) {
        Some(("app.settings.helper.refused", Tone::Danger))
    } else if has(StepOutcome::Failed) {
        Some(("app.settings.helper.failed", Tone::Danger))
    } else if has(StepOutcome::NotConfirmed) {
        Some(("app.settings.helper.unconfirmed", Tone::Warning))
    } else {
        Some(("app.settings.helper.done", Tone::Success))
    }
}

/// When it was, as a key and its number: minutes, hours or days ago.
pub fn ago(at: u64, now: u64) -> (&'static str, u64) {
    let minutes = now.saturating_sub(at) / 60;
    match minutes {
        0..=59 => ("app.settings.helper.ago.minutes", minutes),
        60..=1439 => ("app.settings.helper.ago.hours", minutes / 60),
        _ => ("app.settings.helper.ago.days", minutes / 1440),
    }
}

/// What was touched: the area in the person's language, then the detail as data.
fn touched(cx: &Cx<'_>, entry: &Entry) -> String {
    let parts = entry.steps.iter().map(|s| {
        let key = format!("app.settings.helper.area.{}", s.area.id());
        let area = cx.tr.t(&key).into_owned();
        if s.detail.is_empty() {
            area
        } else {
            format!("{area}: {}", s.detail)
        }
    });
    let all: Vec<String> = parts.collect();
    all.join(" · ").chars().take(120).collect()
}

fn entry(cx: &mut Cx<'_>, card: NodeId, e: &Entry, now: u64, last: bool) -> Result<(), UiError> {
    let asked: String = e.request.chars().take(80).collect();
    let (key, n) = ago(e.at, now);
    let what = touched(cx, e);
    let n = n.to_string();
    let sub = Label::key_with(key, &[("n", &n), ("what", &what)]);
    let mut row = Row::new(Label::data(&asked)).sublabel(sub);
    if last {
        row = row.last();
    }
    let row = cx.ui.add(card, row)?;
    if let Some((word, tone)) = verdict(e) {
        cx.ui.add(row, Badge::new(k(word)).tone(tone))?;
    }
    Ok(())
}

/// Adds the section; nothing on a device without the helper.
pub(crate) fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let Some(activity) = cx.state.status.activity.clone() else {
        return Ok(());
    };
    let (card, head) = section(
        cx.ui,
        content,
        Sizing::AUTO,
        k("app.settings.helper.did"),
        Some(k("app.settings.helper.did.sub")),
    )?;
    if activity.entries.is_empty() {
        let none = Row::new(k("app.settings.helper.none")).last();
        return cx.ui.add(card, none).map(|_| ());
    }
    let forget = Button::new(k("app.settings.helper.forget")).variant(ButtonVariant::Danger);
    cx.act(head, forget, Act::HelperForget)?;
    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    let now = since.map_or(0, |d| d.as_secs());
    let shown: Vec<&Entry> = activity.entries.iter().rev().take(MAX_SHOWN).collect();
    for (i, e) in shown.iter().enumerate() {
        entry(cx, card, e, now, i + 1 == shown.len())?;
    }
    Ok(())
}
