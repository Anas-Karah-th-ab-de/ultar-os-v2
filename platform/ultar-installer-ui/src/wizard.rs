//! The 16 steps of the installation (DESKTOP-SPEC §6) as a pure state machine: what the user
//! entered, which steps are finished, whether "Next" may go on and why not. Nothing here
//! touches a disk; [`Wizard::confirm_install`] is the only way to an [`EraseConfirmation`].

use crate::components::{Component, Scope};
use crate::engine::{answers::Answers, DiskInfo, EngineError, EraseConfirmation, Plan};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// The steps, numbered as on the boards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Step {
    /// 1 Language.
    #[default]
    Language = 1,
    /// 2 Accessibility.
    Access,
    /// 3 Keyboard.
    Keyboard,
    /// 4 Network.
    Network,
    /// 5 Try or install.
    TryOrInstall,
    /// 6 Kind of installation (desktop or server).
    Kind,
    /// 7 Hardware and drivers.
    Drivers,
    /// 8 Components.
    Components,
    /// 9 Extras.
    Extras,
    /// 10 Disk (shown together with 11).
    Disk,
    /// 11 Encryption.
    Encryption,
    /// 12 Account.
    Account,
    /// 13 Time zone.
    TimeZone,
    /// 14 Summary.
    Summary,
    /// 15 Installation.
    Install,
    /// 16 Done.
    Done,
}

impl Step {
    /// Every step in order.
    pub const ALL: [Step; 16] = [
        Step::Language,
        Step::Access,
        Step::Keyboard,
        Step::Network,
        Step::TryOrInstall,
        Step::Kind,
        Step::Drivers,
        Step::Components,
        Step::Extras,
        Step::Disk,
        Step::Encryption,
        Step::Account,
        Step::TimeZone,
        Step::Summary,
        Step::Install,
        Step::Done,
    ];

    /// 1-based number.
    pub fn number(self) -> usize {
        self as usize
    }

    /// Catalog key of the name.
    pub fn key(self) -> String {
        format!("installer.step.{}", self.number())
    }

    /// The page this step is shown on (10 and 11 share one).
    pub fn page(self) -> Step {
        if self == Step::Encryption {
            Step::Disk
        } else {
            self
        }
    }

    fn next(self) -> Step {
        match self {
            Step::Disk | Step::Encryption => Step::Account,
            s => Step::ALL.get(s.number()).copied().unwrap_or(Step::Done),
        }
    }

    fn prev(self) -> Step {
        match self {
            Step::Account => Step::Disk,
            Step::Disk | Step::Encryption => Step::Extras,
            s => Step::ALL
                .get(s.number().saturating_sub(2))
                .copied()
                .unwrap_or(Step::Language),
        }
    }
}

/// How the disk is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiskMode {
    /// Erase everything (the only mode this version writes).
    #[default]
    Whole,
    /// Beside the existing system (needs shrinking foreign partitions: not available).
    Beside,
    /// Manual partitioning (not available).
    Manual,
}

/// Encryption of the data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encryption {
    /// None.
    #[default]
    None,
    /// Passphrase at every start (needs the encryption service: not available).
    Passphrase,
    /// Device chip (TPM).
    Tpm,
}

/// The simple choices of steps 1–6, 9 and 13.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Prefs {
    /// Language index (ar, en, de).
    pub lang: usize,
    /// Accessibility: large text, screen reader.
    pub access: [bool; 2],
    /// Keyboard layout index.
    pub keyboard: usize,
    /// Install (false: try only).
    pub install: bool,
    /// Server edition.
    pub server: bool,
    /// Extras: updates during install, media codecs.
    pub extras: [bool; 2],
    /// Time zone index.
    pub timezone: usize,
}

/// Everything entered.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Wizard {
    /// Shown step.
    pub step: Step,
    /// Highest step reached (steps before it are finished).
    pub reached: Step,
    /// Came from the summary through "Edit": Next returns there.
    pub editing: bool,
    /// Language, accessibility, keyboard, try/install, edition, extras, time zone.
    pub prefs: Prefs,
    /// Selection size.
    pub scope: Scope,
    /// Chosen components.
    pub components: BTreeSet<Component>,
    /// Installation medium.
    pub source: PathBuf,
    /// Disks offered.
    pub disks: Vec<DiskInfo>,
    /// Chosen disk.
    pub disk: Option<usize>,
    /// Disk mode.
    pub disk_mode: DiskMode,
    /// Share of the free space beside an existing system (percent).
    pub share: u8,
    /// Encryption.
    pub encryption: Encryption,
    /// Full name, user name, device name.
    pub account: [String; 3],
    /// The erase box on the summary.
    pub erase_confirmed: bool,
    /// The plan for the chosen disk, or why there is none.
    pub plan: Option<Result<Plan, EngineError>>,
}

/// Time zones offered (IANA ids), Damascus first.
pub const TIMEZONES: [&str; 6] = [
    "Asia/Damascus",
    "Asia/Beirut",
    "Asia/Amman",
    "Europe/Istanbul",
    "Europe/Berlin",
    "UTC",
];
/// Keyboard layouts offered.
pub const KEYBOARDS: [&str; 3] = ["ar+us", "us", "de"];
/// Longest user and device name.
pub const MAX_NAME: usize = 32;

/// `true` for a valid user or device name: a lowercase letter, then letters, digits, `-`.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && name.len() <= MAX_NAME
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

impl Wizard {
    /// A wizard for a medium and the disks the launcher offers.
    pub fn new(source: PathBuf, disks: Vec<DiskInfo>) -> Wizard {
        Wizard {
            source,
            disks,
            prefs: Prefs {
                install: true,
                ..Prefs::default()
            },
            components: crate::components::defaults(),
            scope: Scope::Custom,
            share: 40,
            account: [String::new(), String::new(), "ultar-pc".to_owned()],
            ..Wizard::default()
        }
    }

    /// Why "Next" cannot go on from the shown step (`None`: it can).
    pub fn blocked(&self) -> Option<&'static str> {
        match self.step {
            Step::TryOrInstall if !self.prefs.install => Some("installer.reason.try"),
            Step::Disk | Step::Encryption => self.disk_blocked(),
            Step::Account if !valid_name(&self.account[1]) => Some("installer.reason.user"),
            Step::Account if !valid_name(&self.account[2]) => Some("installer.reason.device"),
            Step::Summary if !self.erase_confirmed => Some("installer.reason.confirm"),
            Step::Summary if !matches!(self.plan, Some(Ok(_))) => Some("installer.reason.disk"),
            Step::Install | Step::Done => Some("installer.reason.running"),
            _ => None,
        }
    }

    fn disk_blocked(&self) -> Option<&'static str> {
        match (&self.plan, self.disk_mode, self.encryption) {
            (None, ..) => Some("installer.reason.disk"),
            (Some(Err(EngineError::TooSmall(..))), ..) => Some("installer.reason.space"),
            (Some(Err(EngineError::TargetIsSource)), ..) => Some("installer.reason.source"),
            (Some(Err(_)), ..) => Some("installer.reason.medium"),
            (_, DiskMode::Beside | DiskMode::Manual, _) => Some("installer.reason.mode"),
            (_, _, Encryption::Passphrase | Encryption::Tpm) => Some("installer.reason.encryption"),
            _ => None,
        }
    }

    /// "Next": goes on (or back to the summary after an edit). `Err(reason)` when blocked.
    pub fn advance(&mut self) -> Result<(), &'static str> {
        if let Some(reason) = self.blocked() {
            return Err(reason);
        }
        if self.step == Step::Summary {
            return Err("installer.reason.install_button");
        }
        let next = if self.editing {
            Step::Summary
        } else {
            self.step.next()
        };
        self.editing = false;
        self.step = next;
        if next > self.reached {
            self.reached = next;
        }
        Ok(())
    }

    /// "Previous": entries of the shown step stay.
    pub fn prev(&mut self) {
        if self.step > Step::Language && self.step < Step::Install {
            self.step = self.step.prev();
            self.editing = false;
        }
    }

    /// Jumps back to a finished step (later steps cannot be jumped to).
    pub fn jump(&mut self, step: Step) -> bool {
        if step < self.reached && self.step < Step::Install && step < Step::Install {
            self.step = step.page();
            self.editing = false;
            return true;
        }
        false
    }

    /// "Edit" on the summary: the step of that line; Next returns to the summary.
    pub fn edit(&mut self, step: Step) -> bool {
        if self.step == Step::Summary && self.jump(step) {
            self.editing = true;
            return true;
        }
        false
    }

    /// Chooses a disk and plans (reads medium and disk only).
    pub fn choose_disk(&mut self, index: usize) {
        self.disk = (index < self.disks.len()).then_some(index);
        self.erase_confirmed = false;
        self.plan = self
            .disk
            .map(|i| Plan::new(&self.source, &self.disks[i], self.answers()));
    }

    /// The choices handed to the first start.
    pub fn answers(&self) -> Answers {
        let mut a = Answers::default();
        a.set(
            "lang",
            ["ar", "en", "de"]
                .get(self.prefs.lang)
                .copied()
                .unwrap_or("ar"),
        );
        a.set(
            "keyboard",
            KEYBOARDS.get(self.prefs.keyboard).copied().unwrap_or("us"),
        );
        a.set("large_text", &self.prefs.access[0].to_string());
        a.set("screen_reader", &self.prefs.access[1].to_string());
        a.set(
            "edition",
            if self.prefs.server {
                "server"
            } else {
                "desktop"
            },
        );
        let ids: Vec<&str> = self.components.iter().map(|c| c.id()).collect();
        a.set("components", &ids.join(","));
        a.set("updates_during_install", &self.prefs.extras[0].to_string());
        a.set("codecs", &self.prefs.extras[1].to_string());
        a.set("full_name", &self.account[0]);
        a.set("user", &self.account[1]);
        a.set("device", &self.account[2]);
        a.set(
            "timezone",
            TIMEZONES.get(self.prefs.timezone).copied().unwrap_or("UTC"),
        );
        a
    }

    /// The Install button on the summary: the confirmation for exactly the chosen target,
    /// only with the erase box ticked. Moves to step 15.
    pub fn confirm_install(&mut self) -> Result<(EraseConfirmation, Plan), &'static str> {
        if self.step != Step::Summary {
            return Err("installer.reason.not_summary");
        }
        if let Some(reason) = self.blocked() {
            return Err(reason);
        }
        let Some(Ok(mut plan)) = self.plan.clone() else {
            return Err("installer.reason.disk");
        };
        plan.answers = self.answers();
        let confirmation = EraseConfirmation {
            target: plan.target.clone(),
            bytes: plan.target_bytes,
        };
        self.step = Step::Install;
        self.reached = Step::Install;
        Ok((confirmation, plan))
    }

    /// The installation finished.
    pub fn finished(&mut self) {
        if self.step == Step::Install {
            self.step = Step::Done;
            self.reached = Step::Done;
        }
    }
}

#[cfg(test)]
#[path = "wizard_tests.rs"]
mod tests;
