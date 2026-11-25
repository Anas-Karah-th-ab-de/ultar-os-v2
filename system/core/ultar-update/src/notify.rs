//! Notifications about updates, posted through `notify.post` and shown by the shell.
//!
//! [`NotifyingPublisher`] wraps the progress publisher: it forwards every state change and,
//! when `updates.notify` is on, posts a notification the first time a release becomes available
//! or a restart is needed (once per phase and version, not on every progress event). The texts
//! are keys of the shell's catalogue (`desk.note.update.*`), which renders them; the sender is
//! [`APP_ID`]. [`usb_offer`] is the offer of an update found on a plugged-in drive.

use crate::contract::{Phase, Progress};
use crate::service::{lock, Publish};
use crate::settings::UpdateSettings;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::notify::{Body, Category, Post, Urgency};
use ultar_bus::{Client, Text, TextArg};

/// Sender identity of the update service's notifications (the shell names it).
pub const APP_ID: &str = "org.ultar.update";
/// A system update is available.
pub const AVAILABLE_KEY: &str = "desk.note.update.available";
/// A restart finishes the update.
pub const RESTART_KEY: &str = "desk.note.update.restart";
/// An update was found on a drive.
pub const USB_KEY: &str = "desk.note.update.usb";
/// Body of the drive offer: the drive's name.
pub const USB_BODY_KEY: &str = "desk.note.update.usb.body";

/// How long a `notify.post` call may take.
const POST_TIMEOUT: Duration = Duration::from_secs(3);

/// Posts a notification (the bus call in production, a collector in tests).
pub trait PostNote: Send + Sync {
    /// Shows `post`; a lost notification is not worth failing an update for.
    fn post(&self, post: &Post);
}

/// [`PostNote`] over the bus (`notify.post`).
pub struct BusPost(pub Client);

impl PostNote for BusPost {
    fn post(&self, post: &Post) {
        let _ = self.0.call(post, POST_TIMEOUT);
    }
}

fn note(title_key: &str) -> Post {
    Post {
        urgency: Urgency::Normal,
        category: Category::System,
        app_id: APP_ID.to_string(),
        title: Text::key(title_key),
        body: Body::Empty,
        actions: Vec::new(),
    }
}

/// The offer of an update found on the drive `name` (shown when `updates.offline_usb` and
/// `updates.notify` are on).
pub fn usb_offer(name: &str) -> Post {
    Post {
        body: Body::Localized(Text::key(USB_BODY_KEY).with(TextArg::Verbatim(name.to_string()))),
        ..note(USB_KEY)
    }
}

/// A progress publisher that also notifies about news, gated by `updates.notify`.
pub struct NotifyingPublisher<P: Publish> {
    inner: P,
    poster: Arc<dyn PostNote>,
    settings: Arc<Mutex<UpdateSettings>>,
    /// The last phase and version notified about (so a held state is not posted again).
    last: Mutex<Option<(Phase, String)>>,
}

impl<P: Publish> NotifyingPublisher<P> {
    /// Wraps `inner`, posting through `poster` while the shared settings allow notifying.
    pub fn new(inner: P, poster: Arc<dyn PostNote>, settings: Arc<Mutex<UpdateSettings>>) -> Self {
        Self {
            inner,
            poster,
            settings,
            last: Mutex::new(None),
        }
    }

    fn news(&self, progress: &Progress) {
        if !lock(&self.settings).notify {
            return;
        }
        let Some(version) = &progress.version else {
            return;
        };
        let title_key = match progress.phase {
            Phase::Available => AVAILABLE_KEY,
            Phase::RebootRequired => RESTART_KEY,
            _ => return,
        };
        let fresh = {
            let mut last = lock(&self.last);
            let now = (progress.phase, version.clone());
            if last.as_ref() == Some(&now) {
                false
            } else {
                *last = Some(now);
                true
            }
        };
        if fresh {
            self.poster.post(&note(title_key));
        }
    }
}

impl<P: Publish> Publish for NotifyingPublisher<P> {
    fn publish(&self, progress: &Progress) {
        self.inner.publish(progress);
        self.news(progress);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Phase;

    #[derive(Default)]
    struct CollectNotes(Mutex<Vec<Post>>);
    impl PostNote for CollectNotes {
        fn post(&self, post: &Post) {
            self.0.lock().unwrap().push(post.clone());
        }
    }

    #[derive(Default)]
    struct CollectProgress(Mutex<Vec<Progress>>);
    impl Publish for CollectProgress {
        fn publish(&self, progress: &Progress) {
            self.0.lock().unwrap().push(progress.clone());
        }
    }

    fn progress(phase: Phase, version: Option<&str>) -> Progress {
        Progress {
            phase,
            done: 0,
            total: 0,
            version: version.map(str::to_string),
            critical: false,
            notes_key: None,
            error_key: None,
        }
    }

    fn rig(
        notify: bool,
    ) -> (
        NotifyingPublisher<CollectProgress>,
        Arc<CollectNotes>,
        Arc<Mutex<UpdateSettings>>,
    ) {
        let notes = Arc::new(CollectNotes::default());
        let settings = Arc::new(Mutex::new(UpdateSettings {
            notify,
            ..UpdateSettings::default()
        }));
        let publisher =
            NotifyingPublisher::new(CollectProgress::default(), notes.clone(), settings.clone());
        (publisher, notes, settings)
    }

    #[test]
    fn notifies_once_per_phase_and_version_and_forwards_progress() {
        let (publisher, notes, _settings) = rig(true);
        publisher.publish(&progress(Phase::Checking, None));
        publisher.publish(&progress(Phase::Available, Some("0.7.1")));
        publisher.publish(&progress(Phase::Available, Some("0.7.1")));
        publisher.publish(&progress(Phase::Downloading, Some("0.7.1")));
        publisher.publish(&progress(Phase::RebootRequired, Some("0.7.1")));
        publisher.publish(&progress(Phase::RebootRequired, Some("0.7.1")));
        let posted = notes.0.lock().unwrap();
        let titles: Vec<&str> = posted.iter().map(|p| p.title.key.as_str()).collect();
        assert_eq!(titles, [AVAILABLE_KEY, RESTART_KEY]);
        assert!(posted.iter().all(|p| p.app_id == APP_ID));
        // Every progress event reached the inner publisher.
        assert_eq!(publisher.inner.0.lock().unwrap().len(), 6);
    }

    #[test]
    fn switched_off_notifies_nothing_but_still_forwards() {
        let (publisher, notes, settings) = rig(false);
        publisher.publish(&progress(Phase::Available, Some("0.7.1")));
        publisher.publish(&progress(Phase::RebootRequired, Some("0.7.1")));
        assert!(notes.0.lock().unwrap().is_empty());
        assert_eq!(publisher.inner.0.lock().unwrap().len(), 2);
        // Switched on at runtime: the next news is posted.
        lock(&settings).notify = true;
        publisher.publish(&progress(Phase::Available, Some("0.7.2")));
        assert_eq!(notes.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_new_version_is_news_again() {
        let (publisher, notes, _settings) = rig(true);
        publisher.publish(&progress(Phase::Available, Some("0.7.1")));
        publisher.publish(&progress(Phase::Available, Some("0.7.2")));
        assert_eq!(notes.0.lock().unwrap().len(), 2);
    }

    #[test]
    fn the_drive_offer_names_the_drive() {
        let post = usb_offer("STICK");
        assert_eq!(post.title.key, USB_KEY);
        assert_eq!(post.category, Category::System);
        match &post.body {
            Body::Localized(t) => {
                assert_eq!(t.key, USB_BODY_KEY);
                assert_eq!(t.args, [TextArg::Verbatim("STICK".into())]);
            }
            other => panic!("{other:?}"),
        }
    }
}
