//! The system dialogs (DESKTOP-SPEC §5.3 "Systemdialoge", board SettingsDisplay's "keep?"):
//! one at a time, in the order asked, at most [`MAX_QUEUED`] waiting; a question nobody
//! answers gets the safe answer (deny, cancel, revert) – never "allow".

pub mod handoff;
pub mod view;

use ultar_bus::messages::auth::{Method, Outcome as AuthOutcome, Secret, UserInfo};
use ultar_bus::messages::bluetooth::{PairConfirm, PairQuestion};
use ultar_bus::messages::mesh::AskHandoff;
use ultar_bus::messages::settings::SettingValue;
use ultar_bus::messages::sysdialog::{
    AskPermission, AuthResult, Authenticate, Decision, ANSWER_WITHIN_MS, MAX_QUEUED,
};

/// Seconds the "keep?" dialog waits (settingsd's `CONFIRM_SECONDS`).
pub const KEEP_SECONDS: u32 = 15;

/// A question.
#[derive(Debug, Clone, PartialEq)]
pub enum Question {
    /// May an app use something?
    Permission(AskPermission),
    /// An administrator's password for an action.
    Authenticate(Authenticate),
    /// Pair with a Bluetooth device? (`sysdialog.bt_pair`, asked by the Bluetooth service.)
    BtPair(PairConfirm),
    /// Take what another device offers? (`sysdialog.handoff`, asked by the mesh.)
    Handoff(AskHandoff),
    /// Keep the new display settings?
    Keep {
        /// Setting key.
        key: String,
        /// The provisional value.
        value: SettingValue,
    },
}

/// The answer to a question.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// Permission.
    Permission(Decision),
    /// Authentication.
    Authenticate(AuthResult),
    /// Keep (true) or revert (false).
    Keep(bool),
    /// Pair (true) or not (false).
    BtPair(bool),
    /// Accept the offer (true) or decline it (false).
    Handoff(bool),
}

/// Something the dialog service asks of the world.
#[derive(Debug, PartialEq)]
pub enum DialogEffect {
    /// Answer question `id`.
    Reply(u64, Answer),
    /// Check an administrator's password.
    Verify {
        /// User.
        user: String,
        /// The typed password (wiped when dropped).
        secret: Secret,
    },
    /// Keep the provisional value (`settings.confirm`).
    Confirm(String),
    /// Take it back (`settings.revert`).
    Revert(String),
    /// Restart the settings service's timer (set the provisional value again).
    Again(String, SettingValue),
}

/// One question in the queue.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Id (the caller's request).
    pub id: u64,
    /// What is asked.
    pub question: Question,
    /// Answered for the user at this time.
    pub deadline_ms: u64,
}

/// The dialogs: the one shown first, the rest waiting.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Dialogs {
    /// Questions, the shown one first.
    pub queue: Vec<Entry>,
    /// The administrator whose password is asked.
    pub admin: Option<UserInfo>,
    /// An authentication is being checked.
    pub checking: bool,
    /// Wrong password: shown and retry not before this time.
    pub wrong_until: Option<u64>,
}

fn safe(question: &Question) -> Answer {
    match question {
        Question::Permission(_) => Answer::Permission(Decision::Deny),
        Question::Authenticate(_) => Answer::Authenticate(AuthResult::Cancelled),
        Question::Keep { .. } => Answer::Keep(false),
        Question::BtPair(_) => Answer::BtPair(false),
        Question::Handoff(_) => Answer::Handoff(false),
    }
}

impl Dialogs {
    /// The question shown now.
    pub fn current(&self) -> Option<&Entry> {
        self.queue.first()
    }

    /// Asks a question; a full queue answers it safely at once.
    pub fn ask(&mut self, id: u64, question: Question, now_ms: u64) -> Vec<DialogEffect> {
        if self.queue.len() >= MAX_QUEUED {
            return self.finish_with(id, safe(&question), &question);
        }
        let seconds = match question {
            Question::Keep { .. } => u64::from(KEEP_SECONDS) * 1000,
            _ => ANSWER_WITHIN_MS,
        };
        // A newer "keep?" for the same key replaces the waiting one (one change pending).
        if let Question::Keep { key, .. } = &question {
            self.queue
                .retain(|e| !matches!(&e.question, Question::Keep { key: k, .. } if k == key));
        }
        self.queue.push(Entry {
            id,
            question,
            deadline_ms: now_ms.saturating_add(seconds),
        });
        Vec::new()
    }

    fn finish_with(&mut self, id: u64, answer: Answer, question: &Question) -> Vec<DialogEffect> {
        let mut out = Vec::new();
        if let Question::Keep { key, .. } = question {
            let keep = answer == Answer::Keep(true);
            out.push(if keep {
                DialogEffect::Confirm(key.clone())
            } else {
                DialogEffect::Revert(key.clone())
            });
        } else {
            out.push(DialogEffect::Reply(id, answer));
        }
        out
    }

    /// The user answered the current question.
    pub fn answer(&mut self, answer: Answer) -> Vec<DialogEffect> {
        if self.queue.is_empty() || self.checking {
            return Vec::new();
        }
        let entry = self.queue.remove(0);
        self.wrong_until = None;
        self.finish_with(entry.id, answer, &entry.question)
    }

    /// Esc / close: the safe answer.
    pub fn dismiss(&mut self) -> Vec<DialogEffect> {
        match self.current().map(|e| safe(&e.question)) {
            Some(a) => self.answer(a),
            None => Vec::new(),
        }
    }

    /// "I need more time": the keep question waits 15 s more (once per press).
    pub fn more_time(&mut self, now_ms: u64) -> Vec<DialogEffect> {
        match self.queue.first_mut() {
            Some(e) => match &e.question {
                Question::Keep { key, value } => {
                    e.deadline_ms = now_ms.saturating_add(u64::from(KEEP_SECONDS) * 1000);
                    vec![DialogEffect::Again(key.clone(), value.clone())]
                }
                _ => Vec::new(),
            },
            None => Vec::new(),
        }
    }

    /// Answers every question whose time is over (the shown one and waiting ones).
    pub fn tick(&mut self, now_ms: u64) -> Vec<DialogEffect> {
        let (late, keep): (Vec<Entry>, Vec<Entry>) =
            self.queue.drain(..).partition(|e| e.deadline_ms <= now_ms);
        self.queue = keep;
        if late
            .iter()
            .any(|e| matches!(e.question, Question::Authenticate(_)))
        {
            self.checking = false;
        }
        late.iter()
            .flat_map(|e| self.finish_with(e.id, safe(&e.question), &e.question))
            .collect()
    }

    /// The next deadline.
    pub fn next_deadline(&self) -> Option<u64> {
        self.queue.iter().map(|e| e.deadline_ms).min()
    }

    /// The Bluetooth service ended the pairing it asked about (`bluetooth.state.pairing`
    /// is no longer running): a passkey shown for typing closes – the pairing went on
    /// without a click, so the answer is "go on".
    pub fn pairing_over(&mut self) -> Vec<DialogEffect> {
        let showing = matches!(
            self.current().map(|e| &e.question),
            Some(Question::BtPair(PairConfirm {
                question: PairQuestion::Show(_),
                ..
            }))
        );
        if showing {
            self.answer(Answer::BtPair(true))
        } else {
            Vec::new()
        }
    }

    /// The settings service resolved a change itself (timeout, a newer change): its
    /// question goes without an answer.
    pub fn resolved(&mut self, key: &str) {
        self.queue
            .retain(|e| !matches!(&e.question, Question::Keep { key: k, .. } if k == key));
    }

    /// Submit of the authentication dialog.
    pub fn submit_password(&mut self, secret: Secret, now_ms: u64) -> Vec<DialogEffect> {
        let is_auth = matches!(
            self.current().map(|e| &e.question),
            Some(Question::Authenticate(_))
        );
        let waiting = self.wrong_until.is_some_and(|t| now_ms < t);
        let Some(admin) = self
            .admin
            .as_ref()
            .filter(|_| is_auth && !self.checking && !waiting)
        else {
            return Vec::new();
        };
        if secret.is_empty() {
            return Vec::new();
        }
        self.checking = true;
        vec![DialogEffect::Verify {
            user: admin.name.clone(),
            secret,
        }]
    }

    /// The answer of `auth.verify`.
    pub fn verified(&mut self, outcome: Option<AuthOutcome>, now_ms: u64) -> Vec<DialogEffect> {
        if !self.checking {
            return Vec::new();
        }
        self.checking = false;
        match outcome {
            Some(AuthOutcome::Granted) => self.answer(Answer::Authenticate(AuthResult::Authorized)),
            Some(
                AuthOutcome::Denied { retry_after_ms } | AuthOutcome::Throttled { retry_after_ms },
            ) => {
                self.wrong_until = Some(now_ms.saturating_add(retry_after_ms));
                Vec::new()
            }
            Some(AuthOutcome::Unavailable) => {
                self.answer(Answer::Authenticate(AuthResult::Unavailable))
            }
            None => Vec::new(),
        }
    }

    /// The method used for the administrator (passwords only in the dialog).
    pub fn method(&self) -> Method {
        Method::Password
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::messages::sysdialog::Permission;

    fn perm() -> Question {
        Question::Permission(AskPermission {
            app_id: "org.x".into(),
            app_name: "X".into(),
            permission: Permission::Camera,
            reason: None,
        })
    }

    #[test]
    fn questions_queue_and_unanswered_ones_are_denied() {
        let mut d = Dialogs::default();
        assert!(d.ask(1, perm(), 0).is_empty());
        assert!(d.ask(2, perm(), 10).is_empty());
        assert_eq!(d.current().unwrap().id, 1);
        assert_eq!(
            d.answer(Answer::Permission(Decision::AllowOnce)),
            vec![DialogEffect::Reply(
                1,
                Answer::Permission(Decision::AllowOnce)
            )]
        );
        assert_eq!(d.next_deadline(), Some(10 + ANSWER_WITHIN_MS));
        assert_eq!(
            d.tick(10 + ANSWER_WITHIN_MS),
            vec![DialogEffect::Reply(2, Answer::Permission(Decision::Deny))]
        );
        for i in 0..MAX_QUEUED as u64 {
            d.ask(i, perm(), 0);
        }
        assert_eq!(
            d.ask(99, perm(), 0),
            vec![DialogEffect::Reply(99, Answer::Permission(Decision::Deny))],
            "full queue: denied at once"
        );
        assert_eq!(
            d.dismiss(),
            vec![DialogEffect::Reply(0, Answer::Permission(Decision::Deny))]
        );
    }

    #[test]
    fn keep_reverts_unless_kept_and_more_time_restarts() {
        let mut d = Dialogs::default();
        let keep = Question::Keep {
            key: "display.mode".into(),
            value: SettingValue::Text("mirror".into()),
        };
        d.ask(0, keep.clone(), 0);
        assert!(d.tick(14_999).is_empty());
        assert_eq!(
            d.more_time(10_000),
            vec![DialogEffect::Again(
                "display.mode".into(),
                SettingValue::Text("mirror".into())
            )]
        );
        assert!(d.tick(20_000).is_empty());
        assert_eq!(
            d.tick(25_000),
            vec![DialogEffect::Revert("display.mode".into())]
        );
        d.ask(0, keep.clone(), 0);
        d.ask(0, keep.clone(), 0);
        assert_eq!(d.queue.len(), 1, "one question per key");
        assert_eq!(
            d.answer(Answer::Keep(true)),
            vec![DialogEffect::Confirm("display.mode".into())]
        );
        d.ask(0, keep, 0);
        d.resolved("display.mode");
        assert!(d.current().is_none());
    }

    #[test]
    fn authentication_checks_the_administrator() {
        let mut d = Dialogs::default();
        let ask = Question::Authenticate(Authenticate {
            app_id: "org.ultar.settings".into(),
            app_name: "S".into(),
            action: ultar_bus::Text::key("a"),
        });
        d.ask(7, ask, 0);
        assert!(
            d.submit_password(Secret::new("pw".into()), 0).is_empty(),
            "no admin known"
        );
        d.admin = Some(UserInfo {
            name: "anas".into(),
            display_name: "A".into(),
            has_pin: false,
            admin: true,
        });
        assert!(d.submit_password(Secret::default(), 0).is_empty());
        let effects = d.submit_password(Secret::new("pw".into()), 0);
        assert!(matches!(&effects[0], DialogEffect::Verify { user, .. } if user == "anas"));
        assert!(
            d.answer(Answer::Authenticate(AuthResult::Authorized))
                .is_empty(),
            "busy"
        );
        assert!(d
            .verified(
                Some(AuthOutcome::Denied {
                    retry_after_ms: 1000
                }),
                0
            )
            .is_empty());
        assert!(
            d.submit_password(Secret::new("pw".into()), 500).is_empty(),
            "waiting"
        );
        d.submit_password(Secret::new("pw".into()), 1000);
        assert_eq!(
            d.verified(Some(AuthOutcome::Granted), 1000),
            vec![DialogEffect::Reply(
                7,
                Answer::Authenticate(AuthResult::Authorized)
            )]
        );
        assert_eq!(d.method(), Method::Password);
    }

    fn pairing(question: PairQuestion) -> Question {
        Question::BtPair(PairConfirm {
            device_name: "Keyboard K380".into(),
            address: "AA:BB:CC:DD:EE:FF".into(),
            kind: ultar_bus::messages::bluetooth::DeviceKind::Keyboard,
            question,
        })
    }

    #[test]
    fn pairing_is_asked_answered_and_safe_when_unanswered() {
        let mut d = Dialogs::default();
        d.ask(1, pairing(PairQuestion::Compare(42)), 0);
        assert_eq!(
            d.pairing_over(),
            Vec::new(),
            "a comparison waits for the person"
        );
        assert_eq!(
            d.answer(Answer::BtPair(true)),
            vec![DialogEffect::Reply(1, Answer::BtPair(true))]
        );
        d.ask(2, pairing(PairQuestion::Consent), 0);
        assert_eq!(
            d.tick(ANSWER_WITHIN_MS),
            vec![DialogEffect::Reply(2, Answer::BtPair(false))],
            "unanswered: no pairing"
        );
        d.ask(3, pairing(PairQuestion::Show(7)), 0);
        assert_eq!(
            d.pairing_over(),
            vec![DialogEffect::Reply(3, Answer::BtPair(true))],
            "the typed number ended it: the dialog closes"
        );
        d.ask(4, pairing(PairQuestion::Show(7)), 0);
        assert_eq!(
            d.dismiss(),
            vec![DialogEffect::Reply(4, Answer::BtPair(false))]
        );
    }

    fn offer() -> Question {
        Question::Handoff(AskHandoff {
            from_name: "KM One".into(),
            from_kind: ultar_bus::messages::mesh::DeviceKind::Phone,
            title: "notes.txt".into(),
            app: "editor".into(),
            size: Some(1_200_000),
            stage: ultar_bus::messages::mesh::Stage::State,
        })
    }

    #[test]
    fn an_offer_of_another_device_is_declined_unless_the_person_accepts() {
        let mut d = Dialogs::default();
        d.ask(1, offer(), 0);
        assert_eq!(
            d.answer(Answer::Handoff(true)),
            vec![DialogEffect::Reply(1, Answer::Handoff(true))]
        );
        d.ask(2, offer(), 0);
        assert_eq!(
            d.dismiss(),
            vec![DialogEffect::Reply(2, Answer::Handoff(false))]
        );
        d.ask(3, offer(), 0);
        assert_eq!(
            d.tick(ANSWER_WITHIN_MS),
            vec![DialogEffect::Reply(3, Answer::Handoff(false))],
            "nobody answered: nothing is taken"
        );
    }
}
