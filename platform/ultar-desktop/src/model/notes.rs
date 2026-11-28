//! Notifications of the calendar sheet (from `notify.posted`): newest first, bounded,
//! "clear all" with a 5 s undo, and what the lock screen may show (DESKTOP-SPEC §5.3:
//! content hidden on the lock screen).

use ultar_bus::messages::notify::{Action, Body, Category, Post, Posted, Urgency};
use ultar_bus::Text;

/// Most notifications kept.
pub const MAX_NOTES: usize = 64;
/// How long "clear all" can be undone.
pub const UNDO_MS: u64 = 5_000;
/// How long a banner without an action stays – the toolkit toast's rule: 4 s without an
/// action (10 s with one, once `notify.posted` carries actions), paused while the pointer
/// or the focus is on it, Esc closes the newest; "important" ones stay until closed.
pub const BANNER_MS: u64 = 4_000;
/// Highest id of the shell's own notices ([`Notes::post_local`]).
pub const LOCAL_IDS: u64 = u64::MAX;
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
    /// Its buttons (at most `notify::MAX_ACTIONS`, from the notification service).
    pub actions: Vec<Action>,
}

/// The list and the undo buffer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notes {
    list: Vec<Note>,
    undo: Option<(Vec<Note>, u64)>,
    /// Shown as banners: id and until when (`u64::MAX`: until closed), newest first.
    banners: Vec<(u64, u64)>,
    /// Counter of the shell's own notices.
    local: u64,
    /// accessibility.visual_alert: a banner also flashes the screen.
    pub visual_alert: bool,
    /// A flash is owed (a banner appeared while `visual_alert` is on).
    flash: bool,
    /// Month shown by the calendar and the selected day index.
    pub calendar: Option<(super::calendar::Month, usize)>,
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
                actions: posted.post.actions,
            },
        );
        self.list.truncate(MAX_NOTES);
    }

    /// The shell's own notice that `app` could not open what was asked (`launcher.start`
    /// refused the place or a second window), shown as a banner. Its ids count down from
    /// [`LOCAL_IDS`] so that they never meet the notification service's.
    pub fn post_local(&mut self, app: &str, now: u64) {
        self.post_notice(app, "desk.launch.refused", now);
    }

    /// The shell's own notice with the title `key` and the body `<key>.body` (a refused
    /// launch, a service that could not do what was asked, a saved screenshot), shown as a
    /// banner.
    pub fn post_notice(&mut self, app: &str, key: &str, now: u64) {
        self.post_said(app, key, Text::key(&format!("{key}.body")), now);
    }

    /// The shell's own notice with the title `key` and `body` as its text (an answer of
    /// the helper), shown as a banner and kept in the list.
    pub fn post_said(&mut self, app: &str, key: &str, body: Text, now: u64) {
        self.local = self.local.wrapping_add(1) % 1024;
        let id = LOCAL_IDS - self.local;
        self.add(Posted {
            id,
            timestamp_ms: now,
            sender: "system.shell".to_owned(),
            post: Post {
                urgency: Urgency::Normal,
                category: Category::System,
                app_id: app.to_owned(),
                title: Text::key(key),
                body: Body::Localized(body),
                actions: Vec::new(),
            },
        });
        self.show_banner(id, now);
    }

    /// The shell's own notice that a drive with media was plugged in (`apps.removable_media`
    /// = ask): a banner with an "Open" button. The action carries the drive's URI; the shell
    /// opens Files there itself (a local notice never goes to the notification service).
    pub fn post_media(&mut self, name: &str, uri: &str, now: u64) {
        self.local = self.local.wrapping_add(1) % 1024;
        let id = LOCAL_IDS - self.local;
        self.add(Posted {
            id,
            timestamp_ms: now,
            sender: "system.shell".to_owned(),
            post: Post {
                urgency: Urgency::Normal,
                category: Category::System,
                app_id: "org.ultar.files".to_owned(),
                title: Text::key("desk.media.title"),
                body: Body::Localized(
                    Text::key("desk.media.body")
                        .with(ultar_bus::TextArg::Verbatim(name.to_owned())),
                ),
                actions: vec![Action {
                    id: uri.to_owned(),
                    label: Text::key("desk.media.open"),
                }],
            },
        });
        self.show_banner(id, now);
    }

    /// The shell's own notice of a call (`id` from [`crate::model::calls`]): the title and
    /// body are texts of the catalogue, the buttons are the shell's own and never go to the
    /// notification service. It stays as a banner until it is closed; posting the same id
    /// again replaces it.
    pub fn post_call(
        &mut self,
        id: u64,
        app: &str,
        title: Text,
        body: Text,
        actions: Vec<Action>,
        now: u64,
    ) {
        self.add(Posted {
            id,
            timestamp_ms: now,
            sender: "system.shell".to_owned(),
            post: Post {
                urgency: Urgency::High,
                category: Category::Call,
                app_id: app.to_owned(),
                title,
                body: Body::Localized(body),
                actions,
            },
        });
        self.show_banner(id, now);
    }

    /// Whether notification `id` is in the list.
    pub fn has(&self, id: u64) -> bool {
        self.list.iter().any(|n| n.id == id)
    }

    /// The id of button `index` of notification `id` (note.action).
    pub fn action(&self, id: u64, index: usize) -> Option<String> {
        let note = self.list.iter().find(|n| n.id == id)?;
        note.actions.get(index).map(|a| a.id.clone())
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
        // The visual alert: a banner that appears flashes the screen too.
        self.flash |= self.visual_alert;
        true
    }

    /// Whether a visual-alert flash is owed (a banner appeared while
    /// `accessibility.visual_alert` is on); takes it, so it fires once.
    pub fn take_flash(&mut self) -> bool {
        std::mem::replace(&mut self.flash, false)
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

    /// Stops the clocks of the banners for `elapsed_ms` (the pointer or the focus is on
    /// them): every finite one stays that much longer.
    pub fn hold(&mut self, elapsed_ms: u64) {
        for (_, until) in &mut self.banners {
            if *until != u64::MAX {
                *until = until.saturating_add(elapsed_ms).min(u64::MAX - 1);
            }
        }
    }

    /// Esc: the newest banner goes (its notification stays in the sheet).
    pub fn hide_newest_banner(&mut self) -> bool {
        if self.banners.is_empty() {
            return false;
        }
        self.banners.remove(0);
        true
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
                actions: Vec::new(),
            },
            sender: format!("app.{app}"),
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
        assert!(n.hide_newest_banner() && n.banners(2).len() == MAX_BANNERS - 2);
        // Held (pointer on them): the finite ones stay as much longer.
        let before = n.next_expiry().unwrap();
        n.hold(700);
        assert_eq!(n.next_expiry(), Some(before + 700));
        assert!(n.hide_banners() && n.banners(2).is_empty() && !n.hide_newest_banner());
    }

    #[test]
    fn a_banner_flashes_the_screen_only_with_the_visual_alert_on() {
        let mut n = Notes::default();
        n.add(posted(1, "a"));
        // Off (the default): a banner appears, no flash is owed.
        assert!(n.show_banner(1, 0));
        assert!(!n.take_flash());
        // On: the next banner owes a flash, taken once.
        n.visual_alert = true;
        n.add(posted(2, "a"));
        assert!(n.show_banner(2, 0));
        assert!(n.take_flash());
        assert!(!n.take_flash(), "the flash fires once");
        // A low notification never shows a banner, so never flashes.
        let mut low = posted(3, "a");
        low.post.urgency = Urgency::Low;
        n.add(low);
        assert!(!n.show_banner(3, 0));
        assert!(!n.take_flash());
        // The shell's own notice flashes too (it shows a banner).
        n.post_notice("org.ultar.shell", "desk.service.failed", 10);
        assert!(n.take_flash());
    }
}
