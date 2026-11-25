//! The phone's system dialogs (`ultar-mobile-dialog`): the questions of the shared dialog
//! state (`ultar-dialog-core`, the same as the desktop's `ultar-sysdialog`) as a sheet from
//! the bottom edge (board CompactMeshIncoming, docs/ui/mesh.md §2.4). The surface covers the
//! screen: the upper part is dimmed with the scrim colour, the sheet below holds the question
//! and touch-size buttons over the whole width – the primary action above the safe one.
//!
//! [`build`] makes the tree, [`choose`] says what a tap means for the dialog state (no
//! drawing, no bus).

pub mod handoff;
pub mod questions;
pub mod sheet;

use ultar_bus::messages::sysdialog::Decision;
use ultar_dialog_core::{Answer, DialogEffect, Dialogs, Question};
use ultar_i18n::Translator;
use ultar_ui::ui::{NodeId, Ui, UiError};

/// What a button of a system dialog means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogClick {
    /// The safe answer: deny, cancel, decline, revert, "no".
    Safe,
    /// Accept, pair, "yes" (pairing, offers of another device).
    Accept,
    /// Permission: allow once.
    AllowOnce,
    /// Permission: always allow.
    AllowAlways,
    /// Authentication: check the typed password.
    Authenticate,
    /// "Keep these display settings".
    Keep,
    /// "I need more time" (the keep question).
    MoreTime,
}

/// Handles of one built dialog.
#[derive(Debug, Default)]
pub struct DialogBuilt {
    /// Operable nodes and what they mean.
    pub clicks: Vec<(NodeId, DialogClick)>,
    /// The password field (authentication).
    pub password: Option<NodeId>,
    /// The node with the focus when the dialog opens (the safe answer, or the field).
    pub focus: Option<NodeId>,
}

impl DialogBuilt {
    /// What `node` means.
    pub fn click(&self, node: NodeId) -> Option<DialogClick> {
        self.clicks
            .iter()
            .find(|(n, _)| *n == node)
            .map(|(_, c)| *c)
    }

    /// The node of `click`.
    pub fn node(&self, click: DialogClick) -> Option<NodeId> {
        self.clicks
            .iter()
            .find(|(_, c)| *c == click)
            .map(|(n, _)| *n)
    }
}

/// What a tap asks of the dialog state.
#[derive(Debug, PartialEq)]
pub enum Choice {
    /// Run these effects.
    Effects(Vec<DialogEffect>),
    /// Take the password out of the field and check it.
    Submit,
}

/// What `click` means for the current question (`now_ms` for "more time").
pub fn choose(d: &mut Dialogs, click: DialogClick, now_ms: u64) -> Choice {
    let question = d.current().map(|e| &e.question);
    let answer = match (click, question) {
        (DialogClick::Safe, Some(_)) => return Choice::Effects(d.dismiss()),
        (DialogClick::Accept, Some(Question::BtPair(_))) => Answer::BtPair(true),
        (DialogClick::Accept, Some(Question::Handoff(_))) => Answer::Handoff(true),
        (DialogClick::AllowOnce, Some(Question::Permission(_))) => {
            Answer::Permission(Decision::AllowOnce)
        }
        (DialogClick::AllowAlways, Some(Question::Permission(_))) => {
            Answer::Permission(Decision::AllowAlways)
        }
        (DialogClick::Keep, Some(Question::Keep { .. })) => Answer::Keep(true),
        (DialogClick::MoreTime, Some(Question::Keep { .. })) => {
            return Choice::Effects(d.more_time(now_ms))
        }
        (DialogClick::Authenticate, Some(Question::Authenticate(_))) => return Choice::Submit,
        _ => return Choice::Effects(Vec::new()),
    };
    Choice::Effects(d.answer(answer))
}

/// The title of the compositor's trusted frame: the app that asks (data), or the system.
pub fn frame_title(d: &Dialogs, tr: &Translator) -> String {
    match d.current().map(|e| &e.question) {
        Some(Question::Permission(p)) => p.app_name.clone(),
        Some(Question::Authenticate(a)) => a.app_name.clone(),
        _ => tr.t("mobile.dialog.system").into_owned(),
    }
}

/// Builds the current question into `ui` (cleared first); `None` when nothing is asked.
/// `now_ms` for the countdown and the wait after a wrong password.
pub fn build(ui: &mut Ui, d: &Dialogs, now_ms: u64) -> Result<Option<DialogBuilt>, UiError> {
    let Some(entry) = d.current() else {
        ui.clear();
        return Ok(None);
    };
    let mut b = sheet::Sheet::start(ui)?;
    let body = b.body;
    match &entry.question {
        Question::Permission(p) => questions::permission(&mut b, body, p)?,
        Question::BtPair(p) => questions::bt_pair(&mut b, body, p)?,
        Question::Handoff(p) => handoff::handoff(&mut b, body, p)?,
        Question::Authenticate(a) => questions::auth(&mut b, body, d, a, now_ms)?,
        Question::Keep { value, .. } => {
            let remaining = entry.deadline_ms.saturating_sub(now_ms);
            questions::keep(&mut b, body, value, remaining)?;
        }
    }
    let built = b.built;
    if let Some(focus) = built.focus {
        ui.focus(focus)?;
    }
    Ok(Some(built))
}
