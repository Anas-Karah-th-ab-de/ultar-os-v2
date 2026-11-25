//! What the helper did lately: kept so the person can read it in Settings, for as many
//! days as the person chose (`assistant.history_days`; 0 keeps nothing).

use std::path::{Path, PathBuf};
use ultar_bus::messages::agent::{Activity, Answer, Entry, MAX_ACTIVITY};

const DAY: u64 = 24 * 60 * 60;
/// Days kept until the person chose otherwise.
pub const DEFAULT_DAYS: u64 = 30;

/// The list, and where it is kept (nowhere without a folder: then it lasts until restart).
#[derive(Debug, Default)]
pub struct Log {
    file: Option<PathBuf>,
    days: u64,
    list: Activity,
}

impl Log {
    /// The list kept in `dir`.
    pub fn open(dir: Option<&Path>) -> Self {
        let file = dir.map(|d| d.join("activity.json"));
        let text = file.as_ref().and_then(|f| std::fs::read_to_string(f).ok());
        let list = text.and_then(|t| serde_json::from_str(&t).ok());
        Self {
            file,
            days: DEFAULT_DAYS,
            list: list.unwrap_or_default(),
        }
    }

    /// The entries now.
    pub fn list(&self) -> &Activity {
        &self.list
    }

    fn store(&self) -> bool {
        let Some(file) = &self.file else {
            return true;
        };
        let part = file.with_extension("part");
        let text = serde_json::to_string(&self.list).unwrap_or_default();
        std::fs::write(&part, text).is_ok() && std::fs::rename(&part, file).is_ok()
    }

    fn trim(&mut self, now: u64) {
        let oldest = now.saturating_sub(self.days * DAY);
        let days = self.days;
        self.list.entries.retain(|e| days > 0 && e.at >= oldest);
        let over = self.list.entries.len().saturating_sub(MAX_ACTIVITY);
        self.list.entries.drain(..over);
    }

    /// The person chose how many days are kept.
    pub fn keep_days(&mut self, days: u64, now: u64) {
        self.days = days;
        self.trim(now);
        self.store();
    }

    /// Writes one answered request down.
    pub fn note(&mut self, request: &str, answer: &Answer, now: u64) {
        self.list.entries.push(Entry {
            at: now,
            request: request.to_owned(),
            brain: answer.brain.clone(),
            steps: answer.steps.clone(),
            said: answer.text.clone(),
        });
        self.trim(now);
        self.store();
    }

    /// Forgets everything; `false` when the file could not be rewritten.
    pub fn clear(&mut self) -> bool {
        self.list.entries.clear();
        self.store()
    }

    /// The last entries in words, for the model.
    pub fn lately(&self) -> String {
        let last = self.list.entries.iter().rev().take(5);
        let lines: Vec<String> = last
            .map(|e| {
                let steps: Vec<String> = e
                    .steps
                    .iter()
                    .map(|s| format!("{} {} {:?}", s.tool, s.detail, s.outcome))
                    .collect();
                format!("asked \"{}\": {}", e.request, steps.join(", "))
            })
            .collect();
        if lines.is_empty() {
            "nothing yet".to_owned()
        } else {
            lines.join("; ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::messages::agent::{Area, Step, StepOutcome};

    fn answered() -> Answer {
        Answer {
            steps: vec![Step::new("mute", StepOutcome::Done, Area::Sound)],
            ..Answer::default()
        }
    }

    #[test]
    fn the_list_is_kept_trimmed_and_forgotten_as_the_person_chose() {
        let dir = std::env::temp_dir().join(format!("agent-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut log = Log::open(Some(&dir));
        for at in 0..60 {
            log.note("mute", &answered(), 1_000_000 + at);
        }
        assert_eq!(log.list().entries.len(), MAX_ACTIVITY);
        assert!(log.lately().contains("mute"));
        // it survives a restart
        let mut again = Log::open(Some(&dir));
        assert_eq!(again.list().entries.len(), MAX_ACTIVITY);
        // one day kept: what is older goes
        again.keep_days(1, 1_000_000 + 2 * DAY);
        assert!(again.list().entries.is_empty());
        again.note("mute", &answered(), 5_000_000);
        again.keep_days(0, 5_000_000);
        assert!(again.list().entries.is_empty(), "0 days keeps nothing");
        again.keep_days(30, 5_000_000);
        again.note("mute", &answered(), 5_000_000);
        assert!(again.clear());
        assert!(Log::open(Some(&dir)).list().entries.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
