//! Notifications of the calendar sheet (from `notify.posted`): newest first, bounded,
//! "clear all" with a 5 s undo, and what the lock screen may show (DESKTOP-SPEC §5.3:
//! content hidden on the lock screen).

use ultar_bus::messages::notify::{Body, Posted, Urgency};
use ultar_bus::Text;

/// Most notifications kept.
pub const MAX_NOTES: usize = 64;
/// How long "clear all" can be undone.
pub const UNDO_MS: u64 = 5_000;
/// How long a banner stays (board Toasts: 5 s; "important" ones stay until closed).
pub const BANNER_MS: u64 = 5_000;
/// Most banners at once.
pub const MAX_BANNERS: usize = 3;

/// One notification.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// Id of the notification service.
    pub id: u64,
    /// Sender.
    pub app_id: String,
    /// Title.
    pub title: Text,
    /// Body.
    pub body: Body,
    /// When (ms since the epoch).
    pub at_ms: u64,
    /// Its urgency.
    pub urgency: Urgency,
}

/// The list and the undo buffer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notes {
    list: Vec<Note>,
    undo: Option<(Vec<Note>, u64)>,
    /// Shown as banners: id and until when (`u64::MAX`: until closed), newest first.
    banners: Vec<(u64, u64)>,
}

impl Notes {
    /// Takes a posted notification (newest first; the oldest goes beyond the limit; an id
    /// seen before replaces the old one).
    pub fn add(&mut self, posted: Posted) {
        self.list.retain(|n| n.id != posted.id);
        self.list.insert(
            0,
            Note {
                id: posted.id,
                app_id: posted.post.app_id,
                title: posted.post.title,
                body: posted.post.body,
                at_ms: posted.timestamp_ms,
                urgency: posted.post.urgency,
            },
        );
        self.list.truncate(MAX_NOTES);
    }

    /// The notifications, newest first.
    pub fn list(&self) -> &[Note] {
        &self.list
    }

    /// Shows a notification as a banner from `now` (not low ones: those wait in the
    /// panel). Returns whether a banner appeared.
    pub fn show_banner(&mut self, id: u64, now: u64) -> bool {
        let Some(note) = self.list.iter().find(|n| n.id == id) else {
            return false;
        };
        let until = match note.urgency {
            Urgency::Low => return false,
            Urgency::Normal => now.saturating_add(BANNER_MS),
            Urgency::High => u64::MAX,
        };
        self.banners.retain(|(b, _)| *b != id);
        self.banners.insert(0, (id, until));
        self.banners.truncate(MAX_BANNERS);
        true
    }

    /// The banners shown at `now`, newest first.
    pub fn banners(&self, now: u64) -> Vec<&Note> {
        self.banners
            .iter()
            .filter(|(_, until)| now < *until)
            .filter_map(|(id, _)| self.list.iter().find(|n| n.id == *id))
            .collect()
    }

    /// Forgets banners whose time is over; `true` if one went.
    pub fn expire(&mut self, now: u64) -> bool {
        let before = self.banners.len();
        self.banners.retain(|(_, until)| now < *until);
        before != self.banners.len()
    }

    /// The next moment a banner goes, if any.
    pub fn next_expiry(&self) -> Option<u64> {
        self.banners
            .iter()
            .map(|(_, u)| *u)
            .filter(|u| *u != u64::MAX)
            .min()
    }

    /// Hides every banner (the sheet with all notifications opened, do not disturb).
    pub fn hide_banners(&mut self) -> bool {
        let had = !self.banners.is_empty();
        self.banners.clear();
        had
    }

    /// Removes one (and its banner).
    pub fn dismiss(&mut self, id: u64) -> bool {
        self.banners.retain(|(b, _)| *b != id);
        let before = self.list.len();
        self.list.retain(|n| n.id != id);
        before != self.list.len()
    }

    /// Removes all; undo possible until `now + UNDO_MS`.
    pub fn clear(&mut self, now: u64) {
        if self.list.is_empty() {
            return;
        }
        self.banners.clear();
        self.undo = Some((std::mem::take(&mut self.list), now + UNDO_MS));
    }

    /// Brings the cleared ones back if still in time.
    pub fn undo(&mut self, now: u64) -> bool {
        match self.undo.take() {
            Some((list, until)) if now < until => {
                self.list = list;
                true
            }
            _ => false,
        }
    }

    /// The undo is still offered.
    pub fn can_undo(&self, now: u64) -> bool {
        self.undo.as_ref().is_some_and(|(_, until)| now < *until)
    }

    /// For the lock screen: per app the number of notifications, no content.
    pub fn counts_by_app(&self) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> = Vec::new();
        for n in &self.list {
            match out.iter_mut().find(|(a, _)| *a == n.app_id) {
                Some((_, c)) => *c += 1,
                None => out.push((n.app_id.clone(), 1)),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::messages::notify::{Category, Post, Urgency};

    fn posted(id: u64, app: &str) -> Posted {
        Posted {
            id,
            timestamp_ms: id * 1000,
            post: Post {
                urgency: Urgency::Normal,
                category: Category::App,
                app_id: app.into(),
                title: Text::key("t"),
                body: Body::Verbatim("secret content".into()),
            },
        }
    }

    #[test]
    fn newest_first_bounded_dismiss_clear_and_undo() {
        let mut n = Notes::default();
        for i in 0..70 {
            n.add(posted(i, if i % 2 == 0 { "a" } else { "b" }));
        }
        assert_eq!(n.list().len(), MAX_NOTES);
        assert_eq!(n.list()[0].id, 69);
        n.add(posted(69, "a"));
        assert_eq!(n.list().len(), MAX_NOTES);
        assert!(n.dismiss(69) && !n.dismiss(69));
        let counts = n.counts_by_app();
        assert_eq!(counts.iter().map(|c| c.1).sum::<usize>(), MAX_NOTES - 1);
        n.clear(100);
        assert!(n.list().is_empty() && n.can_undo(100));
        assert!(!n.undo(100 + UNDO_MS));
        n.add(posted(1, "a"));
        n.clear(0);
        assert!(n.undo(10) && n.list().len() == 1);
        let mut empty = Notes::default();
        empty.clear(0);
        assert!(!empty.can_undo(0));
    }

    #[test]
    fn banners_go_after_five_seconds_important_ones_stay_low_ones_never_show() {
        let mut n = Notes::default();
        for (id, urgency) in [(1, Urgency::Normal), (2, Urgency::High), (3, Urgency::Low)] {
            let mut p = posted(id, "a");
            p.post.urgency = urgency;
            n.add(p);
        }
        assert!(
            n.show_banner(1, 0)
                && n.show_banner(2, 0)
                && !n.show_banner(3, 0)
                && !n.show_banner(9, 0)
        );
        assert_eq!(
            n.banners(0).iter().map(|b| b.id).collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert_eq!(n.next_expiry(), Some(BANNER_MS));
        assert!(n.expire(BANNER_MS));
        assert_eq!(n.banners(BANNER_MS).len(), 1);
        for id in 10..15 {
            n.add(posted(id, "b"));
            n.show_banner(id, 1);
        }
        assert_eq!(n.banners(2).len(), MAX_BANNERS);
        assert!(n.dismiss(14) && n.banners(2).iter().all(|b| b.id != 14));
        assert!(n.hide_banners() && n.banners(2).is_empty());
    }
}
