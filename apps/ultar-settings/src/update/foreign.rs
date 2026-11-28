//! Clicks of Settings › Apps from other systems (`docs/FREMD-APPS.md`): the Linux area and
//! its update, the page of one app with its permissions (more is confirmed by an
//! administrator in the system dialog, less applies at once) and removing an app.

use super::{ask, close, Ctl, Outcome};
use crate::action::Act;
use crate::service::foreign::outcome_error;
use crate::service::Ports;
use crate::state::foreign::FOLDERS;
use crate::state::{DialogPurpose, State, SubPage};
use ultar_bus::messages::foreign::{Choices, InstallOutcome, NetworkChoice, Outcome as Done, Step};
use ultar_bus::Text;
use ultar_ui::event::ActionKind;
use ultar_ui::widgets::Tone;

/// Reads the area and the apps. The page of an app closes when the app is gone.
pub fn load(state: &mut State, ports: &mut Ports) {
    let view = &mut state.status.foreign;
    match ports.foreign.runtime().and_then(|r| Ok((r, ports.foreign.list()?))) {
        Ok((runtime, apps)) => {
            view.runtime = Some(runtime);
            view.apps = apps;
            view.error = None;
        }
        Err(e) => {
            view.runtime = None;
            view.apps.clear();
            view.error = Some(e.text());
        }
    }
    if state.sub == SubPage::ForeignApp && state.status.foreign.app().is_none() {
        state.status.foreign.open = None;
        state.sub = SubPage::None;
    }
}

/// Job steps and list changes while the page is open; `Some` when it must be redrawn.
pub fn poll(state: &mut State, ports: &mut Ports) -> Option<Outcome> {
    let (steps, changed) = ports.foreign.news();
    let mut toast = None;
    for p in &steps {
        if !state.status.foreign.jobs.contains(&p.job) {
            continue;
        }
        let key = match p.step {
            Step::Done => "app.settings.foreign.job.done",
            Step::Cancelled => "app.settings.foreign.job.cancelled",
            Step::Failed(_) => "app.settings.foreign.job.failed",
            _ => continue,
        };
        state.status.foreign.jobs.retain(|j| *j != p.job);
        let tone = if p.step == Step::Done {
            Tone::Success
        } else {
            Tone::Warning
        };
        toast = Some((tone, Text::key(key)));
    }
    if !changed && toast.is_none() {
        return None;
    }
    load(state, ports);
    Some(Outcome {
        rebuild: true,
        toast,
        ..Outcome::default()
    })
}

/// `true` for the actions of this page.
pub fn owns(act: Act) -> bool {
    matches!(
        act,
        Act::ForeignApp(_)
            | Act::ForeignBack
            | Act::ForeignInternet
            | Act::ForeignLan
            | Act::ForeignFolder(_)
            | Act::ForeignKeep
            | Act::ForeignRemove
            | Act::ForeignUpdate
    )
}

/// Handles a click.
pub fn handle(c: &mut Ctl<'_>, act: Act, kind: ActionKind) -> Outcome {
    match (act, kind) {
        (Act::ForeignApp(n), ActionKind::Activated) => open_app(c, n),
        (Act::ForeignBack, ActionKind::Activated) => {
            c.state.status.foreign.open = None;
            c.state.sub = SubPage::None;
            Outcome::rebuild()
        }
        (Act::ForeignInternet, ActionKind::Toggled(on)) => change(c, |ch| {
            ch.network = if on {
                NetworkChoice::Internet
            } else {
                NetworkChoice::None
            };
            ch.lan &= on;
        }),
        (Act::ForeignLan, ActionKind::Toggled(on)) => change(c, |ch| {
            ch.lan = on && ch.network == NetworkChoice::Internet;
        }),
        (Act::ForeignFolder(n), ActionKind::Toggled(on)) => match FOLDERS.get(n) {
            Some(word) => change(c, |ch| {
                ch.folders.retain(|f| f != word);
                if on {
                    ch.folders.push((*word).to_owned());
                }
            }),
            None => Outcome::default(),
        },
        (Act::ForeignKeep, ActionKind::Toggled(on)) => {
            c.state.status.foreign.keep_data = on;
            Outcome::default()
        }
        (Act::ForeignRemove, ActionKind::Activated) => ask(c.state, DialogPurpose::ForeignRemove),
        (Act::ForeignUpdate, ActionKind::Activated) => update_runtime(c),
        _ => Outcome::default(),
    }
}

fn open_app(c: &mut Ctl<'_>, n: usize) -> Outcome {
    let id = c.state.status.foreign.apps.get(n).map(|a| a.id.clone());
    if id.is_none() {
        return Outcome::default();
    }
    c.state.status.foreign.open = id;
    c.state.status.foreign.keep_data = false;
    c.state.sub = SubPage::ForeignApp;
    Outcome::rebuild()
}

/// Sends the open app's changed choices: less applies at once, more waits for the
/// administrator (the switch goes back until the service says it is done).
fn change(c: &mut Ctl<'_>, edit: impl FnOnce(&mut Choices)) -> Outcome {
    let Some(app) = c.state.status.foreign.app() else {
        return Outcome::default();
    };
    let (id, mut choices) = (app.id.clone(), app.choices.clone());
    edit(&mut choices);
    let answer = c.ports.foreign.set_choices(&id, choices);
    load(c.state, c.ports);
    let toast = match answer {
        Ok(Done::Asking(job)) => {
            c.state.status.foreign.jobs.push(job);
            Some((Tone::Info, Text::key("app.settings.foreign.asking")))
        }
        Ok(outcome) => outcome_error(outcome).map(|t| (Tone::Danger, t)),
        Err(e) => Some((Tone::Danger, e.text())),
    };
    Outcome {
        rebuild: true,
        toast,
        ..Outcome::default()
    }
}

fn update_runtime(c: &mut Ctl<'_>) -> Outcome {
    let key = match c.ports.foreign.update_runtime() {
        Ok(InstallOutcome::Started(job)) => {
            c.state.status.foreign.jobs.push(job);
            return Outcome::toast(Tone::Info, "app.settings.foreign.updating");
        }
        Ok(InstallOutcome::RuntimeMissing) => "app.settings.foreign.runtime.missing",
        Ok(InstallOutcome::Busy) => "app.settings.foreign.error.busy",
        Ok(_) => "app.settings.error.failed",
        Err(e) => return Outcome::error(&e),
    };
    Outcome::toast(Tone::Warning, key)
}

/// The primary button of this page's dialog (remove).
pub fn dialog_ok(c: &mut Ctl<'_>) -> Outcome {
    let view = &c.state.status.foreign;
    let Some(id) = view.app().map(|a| a.id.clone()) else {
        return close(c.state);
    };
    let keep = view.keep_data;
    let answer = c.ports.foreign.remove(&id, keep);
    close(c.state);
    load(c.state, c.ports);
    let toast = match answer {
        Ok(Done::Done) => (Tone::Success, Text::key("app.settings.foreign.removed")),
        Ok(outcome) => (
            Tone::Danger,
            outcome_error(outcome).unwrap_or_else(|| Text::key("app.settings.error.failed")),
        ),
        Err(e) => (Tone::Danger, e.text()),
    };
    Outcome {
        rebuild: true,
        dialog: true,
        toast: Some(toast),
        ..Outcome::default()
    }
}
