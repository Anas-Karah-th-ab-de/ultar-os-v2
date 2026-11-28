//! The 16 steps of the installation (DESKTOP-SPEC §6) as a pure state machine: what the user
//! entered, which steps are finished, whether "Next" may go on and why not. Nothing here
//! touches a disk; [`Wizard::confirm_install`] is the only way to an [`EraseConfirmation`].

use crate::components::{Component, Scope};
use crate::engine::{
    answers::Answers, DiskInfo, EngineError, EraseConfirmation, ManualPart, PartRole, Plan,
};
use crate::secret::Secret;
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
    /// Erase everything.
    #[default]
    Whole,
    /// Beside the existing system: only into the largest free region, nothing is shrunk or
    /// moved.
    Beside,
    /// Manual partitioning: create, delete and assign partitions (ESP, slot A/B, data).
    Manual,
}

/// Encryption of the data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encryption {
    /// None.
    #[default]
    None,
    /// Passphrase at every start.
    Passphrase,
    /// Device chip (TPM) – shown as not available while the kernel has no TPM driver.
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

/// How the chosen disk is used.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DiskUse {
    /// Whole disk, beside, manual.
    pub mode: DiskMode,
    /// Share of the free space beside an existing system (percent).
    pub share: u8,
    /// The partitions of a manual layout.
    pub manual: Vec<ManualPart>,
}

/// What the user typed that must never be written or printed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Secrets {
    /// The user's password and its repetition (the first start gets an Argon2id hash).
    pub password: [Secret; 2],
    /// The passphrase of the encryption and its repetition.
    pub passphrase: [Secret; 2],
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
    /// How the chosen disk is used.
    pub layout: DiskUse,
    /// Encryption.
    pub encryption: Encryption,
    /// Full name, user name, device name and – optional – the PC's public SSH key for the
    /// developer access (`ultar-sshd`, [`crate::ssh_key`]).
    pub account: [String; 4],
    /// Password and passphrase as typed (memory only).
    pub secrets: Secrets,
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
/// Shortest user password (characters counted as bytes of UTF-8).
pub const MIN_PASSWORD: usize = 8;
/// Longest user password (the account service's limit).
pub const MAX_PASSWORD: usize = ultar_bus::messages::account_service::MAX_PASSWORD;
/// Shortest encryption passphrase.
pub const MIN_PASSPHRASE: usize = 12;
/// Most partitions of a manual layout.
pub const MAX_MANUAL: usize = 64;

/// The manual layout of a disk as it is now: its partitions, all kept.
pub fn manual_from(d: &DiskInfo) -> Vec<ManualPart> {
    d.table
        .iter()
        .flatten()
        .map(|e| ManualPart {
            existing: Some(e.index),
            first: e.first,
            sectors: e.last - e.first + 1,
            role: PartRole::Keep,
        })
        .collect()
}

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
            layout: DiskUse {
                share: 40,
                ..DiskUse::default()
            },
            account: [
                String::new(),
                String::new(),
                "ultar-pc".to_owned(),
                String::new(),
            ],
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
            Step::Account if !crate::ssh_key::valid_or_empty(&self.account[3]) => {
                Some("installer.reason.ssh_key")
            }
            Step::Account => self.password_blocked(),
            Step::Summary if !self.erase_confirmed => Some("installer.reason.confirm"),
            Step::Summary if !matches!(self.plan, Some(Ok(_))) => Some("installer.reason.disk"),
            Step::Install | Step::Done => Some("installer.reason.running"),
            _ => None,
        }
    }

    fn disk_blocked(&self) -> Option<&'static str> {
        match &self.plan {
            None => return Some("installer.reason.disk"),
            Some(Err(EngineError::TooSmall(..))) => return Some("installer.reason.space"),
            Some(Err(EngineError::TargetIsSource)) => return Some("installer.reason.source"),
            Some(Err(EngineError::NoTable)) => return Some("installer.reason.no_table"),
            Some(Err(EngineError::NoFreeSpace(..))) => {
                return Some("installer.reason.no_free_space")
            }
            Some(Err(EngineError::Manual(key))) => return Some(key),
            Some(Err(_)) => return Some("installer.reason.medium"),
            Some(Ok(_)) => {}
        }
        match self.encryption {
            Encryption::None => None,
            Encryption::Tpm => Some("installer.reason.tpm"),
            Encryption::Passphrase => {
                let [a, b] = &self.secrets.passphrase;
                if a.len() < MIN_PASSPHRASE {
                    Some("installer.reason.pass_short")
                } else if a != b {
                    Some("installer.reason.pass_mismatch")
                } else {
                    None
                }
            }
        }
    }

    fn password_blocked(&self) -> Option<&'static str> {
        let [a, b] = &self.secrets.password;
        if a.len() < MIN_PASSWORD {
            Some("installer.reason.password_short")
        } else if a.len() > MAX_PASSWORD {
            Some("installer.reason.password_long")
        } else if a != b {
            Some("installer.reason.password_mismatch")
        } else {
            None
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
        self.layout.manual = self
            .disk
            .map(|i| manual_from(&self.disks[i]))
            .unwrap_or_default();
        self.replan();
    }

    /// Plans again after the disk, the mode, the share or the manual layout changed.
    pub fn replan(&mut self) {
        self.erase_confirmed = false;
        let answers = self.answers();
        let encrypt = self.encryption == Encryption::Passphrase;
        self.plan = self.disk.map(|i| {
            let d = &self.disks[i];
            let plan = match self.layout.mode {
                DiskMode::Whole => Plan::new(&self.source, d, answers),
                DiskMode::Beside => Plan::beside(&self.source, d, self.layout.share, answers),
                DiskMode::Manual => Plan::manual(&self.source, d, &self.layout.manual, answers),
            };
            // room for the LUKS2 metadata; the passphrase itself is taken at confirmation
            match plan {
                Ok(p) if encrypt => p.encrypted(crate::secret::Secret::default()),
                other => other,
            }
        });
    }

    /// Manual layout: a new partition of `mib` MiB in the largest free region; `false` when
    /// it does not fit.
    pub fn manual_create(&mut self, mib: u64) -> bool {
        let Some(d) = self.disk.map(|i| &self.disks[i]) else {
            return false;
        };
        let used: Vec<(u64, u64)> = self
            .layout
            .manual
            .iter()
            .filter(|p| p.role != PartRole::Delete)
            .map(|p| (p.first, p.first + p.sectors - 1))
            .collect();
        let sectors = mib.saturating_mul(ultar_slots::install::MIB);
        let Some((first, _)) = d
            .free_regions(&used)
            .into_iter()
            .find(|(_, len)| *len >= sectors && sectors > 0)
        else {
            return false;
        };
        if self.layout.manual.len() >= MAX_MANUAL {
            return false;
        }
        self.layout.manual.push(ManualPart {
            existing: None,
            first,
            sectors,
            role: PartRole::Keep,
        });
        self.layout.manual.sort_by_key(|p| p.first);
        self.replan();
        true
    }

    /// Manual layout: removes a new partition, or marks an existing one for deletion.
    pub fn manual_delete(&mut self, index: usize) {
        match self.layout.manual.get(index).map(|p| p.existing) {
            Some(None) => {
                self.layout.manual.remove(index);
            }
            Some(Some(_)) => self.layout.manual[index].role = PartRole::Delete,
            None => return,
        }
        self.replan();
    }

    /// Manual layout: assigns a role. A new partition cannot be "kept" as foreign data:
    /// "keep" on it means "not used yet"; "delete" is only for existing partitions.
    pub fn manual_assign(&mut self, index: usize, role: PartRole) {
        let Some(p) = self.layout.manual.get_mut(index) else {
            return;
        };
        if role == PartRole::Delete && p.existing.is_none() {
            return;
        }
        p.role = role;
        self.replan();
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
        if !self.account[3].trim().is_empty() {
            a.set("ssh_key", self.account[3].trim());
        }
        a.set(
            "disk_mode",
            ["whole", "beside", "manual"][self.layout.mode as usize],
        );
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
        let hash = crate::secret::hash_password(&self.secrets.password[0])
            .map_err(|_| "installer.reason.password_hash")?;
        plan.answers.set("password_hash", &hash);
        plan.encrypt = match self.encryption {
            Encryption::Passphrase => Some(self.secrets.passphrase[0].clone()),
            _ => None,
        };
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
