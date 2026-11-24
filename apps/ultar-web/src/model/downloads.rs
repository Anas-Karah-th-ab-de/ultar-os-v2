//! The download list of the browser (progress, Open, Show in folder, Clear). Files go only
//! into the downloads folder (the one folder of the user the page cell can write); names
//! coming from pages are made safe before they are used as paths.

use crate::engine::DownloadId;
use std::path::{Path, PathBuf};

/// Most downloads listed; finished ones are dropped first.
pub const MAX_DOWNLOADS: usize = 100;
/// Longest file name kept (bytes).
pub const MAX_NAME: usize = 200;

/// The state of a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Receiving: bytes so far and size (0 = unknown).
    Running {
        /// Bytes received.
        received: u64,
        /// Size, 0 when unknown.
        total: u64,
    },
    /// Complete.
    Done,
    /// Failed.
    Failed,
    /// Cancelled by the user.
    Cancelled,
}

/// One download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    /// Engine identifier.
    pub id: DownloadId,
    /// Safe file name inside the downloads folder.
    pub name: String,
    /// Source address.
    pub url: String,
    /// State.
    pub state: State,
}

impl Download {
    /// Share received, 0..1 (`None` while the size is unknown or when not running).
    pub fn share(&self) -> Option<f32> {
        match self.state {
            State::Running { received, total } if total > 0 => {
                Some((received as f64 / total as f64).clamp(0.0, 1.0) as f32)
            }
            _ => None,
        }
    }
}

/// Makes a page-supplied file name safe: no folders, no control characters, no leading dot,
/// at most [`MAX_NAME`] bytes; an empty result becomes `download`.
pub fn safe_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut out: String = last
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect();
    out = out.trim().trim_start_matches('.').to_string();
    while out.len() > MAX_NAME {
        out.pop();
    }
    if out.is_empty() {
        "download".to_string()
    } else {
        out
    }
}

/// The downloads of this session.
#[derive(Debug, Clone, Default)]
pub struct Downloads {
    list: Vec<Download>,
}

impl Downloads {
    /// Adds a started download (its name made safe).
    pub fn start(&mut self, id: DownloadId, url: &str, name: &str) {
        if self.list.len() >= MAX_DOWNLOADS {
            match self
                .list
                .iter()
                .position(|d| !matches!(d.state, State::Running { .. }))
            {
                Some(i) => {
                    self.list.remove(i);
                }
                None => return,
            }
        }
        self.list.push(Download {
            id,
            name: safe_name(name),
            url: url.chars().take(crate::url::MAX_URL).collect(),
            state: State::Running {
                received: 0,
                total: 0,
            },
        });
    }

    /// Updates the progress of a running download.
    pub fn progress(&mut self, id: DownloadId, received: u64, total: u64) {
        if let Some(d) = self.get_mut(id) {
            if matches!(d.state, State::Running { .. }) {
                d.state = State::Running { received, total };
            }
        }
    }

    /// Marks a download finished.
    pub fn finish(&mut self, id: DownloadId, ok: bool) {
        if let Some(d) = self.get_mut(id) {
            if matches!(d.state, State::Running { .. }) {
                d.state = if ok { State::Done } else { State::Failed };
            }
        }
    }

    /// Marks a download cancelled (returns `false` if it was not running).
    pub fn cancel(&mut self, id: DownloadId) -> bool {
        match self.get_mut(id) {
            Some(d) if matches!(d.state, State::Running { .. }) => {
                d.state = State::Cancelled;
                true
            }
            _ => false,
        }
    }

    /// Removes every download that is not running ("Clear").
    pub fn clear_finished(&mut self) {
        self.list
            .retain(|d| matches!(d.state, State::Running { .. }));
    }

    /// The list, oldest first.
    pub fn list(&self) -> &[Download] {
        &self.list
    }

    /// Number of running downloads.
    pub fn running(&self) -> usize {
        self.list
            .iter()
            .filter(|d| matches!(d.state, State::Running { .. }))
            .count()
    }

    /// One download.
    pub fn get(&self, id: DownloadId) -> Option<&Download> {
        self.list.iter().find(|d| d.id == id)
    }

    fn get_mut(&mut self, id: DownloadId) -> Option<&mut Download> {
        self.list.iter_mut().find(|d| d.id == id)
    }

    /// The file of a download inside `folder`.
    pub fn path(&self, folder: &Path, id: DownloadId) -> Option<PathBuf> {
        self.get(id).map(|d| folder.join(&d.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("a\\b\\c.txt"), "c.txt");
        assert_eq!(safe_name(".bashrc"), "bashrc");
        assert_eq!(safe_name("x\u{202e}gpj.exe"), "xgpj.exe");
        assert_eq!(safe_name("\u{0}\n"), "download");
        assert_eq!(safe_name(&"ع".repeat(300)).len(), MAX_NAME);
        assert_eq!(safe_name("تقرير.pdf"), "تقرير.pdf");
    }

    #[test]
    fn lifecycle_and_limits() {
        let mut d = Downloads::default();
        d.start(DownloadId(1), "https://a/x", "x.pdf");
        d.progress(DownloadId(1), 50, 100);
        assert_eq!(d.get(DownloadId(1)).unwrap().share(), Some(0.5));
        assert_eq!(d.running(), 1);
        d.finish(DownloadId(1), true);
        d.progress(DownloadId(1), 1, 1);
        assert_eq!(d.get(DownloadId(1)).unwrap().state, State::Done);
        assert!(!d.cancel(DownloadId(1)));
        d.start(DownloadId(2), "https://a/y", "y");
        assert!(d.cancel(DownloadId(2)));
        d.finish(DownloadId(2), true);
        assert_eq!(d.get(DownloadId(2)).unwrap().state, State::Cancelled);
        assert_eq!(
            d.path(Path::new("/dl"), DownloadId(1)),
            Some(PathBuf::from("/dl/x.pdf"))
        );
        d.clear_finished();
        assert!(d.list().is_empty());
        for i in 0..MAX_DOWNLOADS as u32 {
            d.start(DownloadId(i + 10), "u", "n");
        }
        d.start(DownloadId(999), "u", "n");
        assert_eq!(d.list().len(), MAX_DOWNLOADS);
        assert!(d.get(DownloadId(999)).is_none());
        d.finish(DownloadId(10), false);
        d.start(DownloadId(999), "u", "n");
        assert!(d.get(DownloadId(999)).is_some());
        assert!(d.get(DownloadId(10)).is_none());
    }
}
