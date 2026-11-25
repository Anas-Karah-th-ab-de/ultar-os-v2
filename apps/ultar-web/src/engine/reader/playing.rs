//! Sound and video in the reader's pages: a click on a media box starts its player (or
//! pauses, plays on, plays again); the box shows the state, the newest picture and a bar.
//! A page that is itself a sound or video file plays at once.

use super::page::{self, MediaBox, MediaView};
use super::{fetch, ReaderEngine, Tab};
use crate::engine::{EngineEvent, FrameDamage, TabId};
use crate::media::player::{Fetch, Player, Status};
use std::collections::BTreeMap;
use ultar_i18n::{Arg, Translator};

/// The players of one tab, by file.
#[derive(Default)]
pub(super) struct TabMedia {
    players: BTreeMap<String, Player>,
    /// What each box showed last: its state and the number of its picture.
    shown: BTreeMap<String, (Status, u64)>,
}

impl TabMedia {
    /// The state of every player (tests).
    #[cfg(test)]
    pub(super) fn players_status(&self) -> Vec<Status> {
        self.players.values().map(Player::status).collect()
    }

    /// Stops everything (another page).
    pub(super) fn clear(&mut self) {
        self.players.clear();
        self.shown.clear();
    }

    /// What the box of `src` shows now.
    pub(super) fn view(&self, src: &str, tr: &Translator) -> MediaView {
        let Some(p) = self.players.get(src) else {
            return MediaView {
                label: tr.t("web.media.play").into_owned(),
                progress: None,
                picture: None,
            };
        };
        let (pos, length) = p.position();
        let played = length
            .filter(|l| !l.is_zero())
            .map(|l| pos.as_secs_f32() / l.as_secs_f32());
        let picture = p.picture().1;
        let (label, progress) = match p.status() {
            Status::Loading(done) => {
                let percent = done.map_or(0, |d| d / 10);
                let label = tr.t_args(
                    "web.media.loading",
                    &[("percent", Arg::Int(percent.into()))],
                );
                (label, done.map(|d| d as f32 / 1000.0))
            }
            Status::Playing if picture.is_some() => (String::new(), played),
            Status::Playing => (tr.t("web.media.playing").into_owned(), played),
            Status::Paused => (tr.t("web.media.paused").into_owned(), played),
            Status::Ended => (tr.t("web.media.ended").into_owned(), Some(1.0)),
            Status::FetchFailed(why) => (
                tr.t_args("web.media.failed", &[("reason", Arg::Str(&why))]),
                None,
            ),
            Status::Unsupported(what) => (
                tr.t_args("web.media.unsupported", &[("what", Arg::Str(&what))]),
                None,
            ),
        };
        MediaView {
            label,
            progress,
            picture,
        }
    }
}

impl ReaderEngine {
    /// A click on the media box `b`: start, pause, play on or again.
    pub(super) fn click_media(&mut self, tab: TabId, b: &MediaBox) {
        let wake = self.wake_write.try_clone().ok();
        let client = self.client.clone();
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let media = &mut t.media;
        let again = match media.players.get(&b.src) {
            Some(p) => match p.status() {
                Status::Ended => p.replay(),
                Status::FetchFailed(_) | Status::Unsupported(_) => None,
                _ => {
                    p.toggle();
                    return;
                }
            },
            None => {
                let url = b.src.clone();
                let fetch: Fetch = Box::new(move |progress, stop| {
                    client
                        .get_media(&url, progress, stop)
                        .map_err(|e| fetch_reason(&e))
                });
                Some(Player::start(fetch, wake))
            }
        };
        if let Some(p) = again {
            media.players.insert(b.src.clone(), p);
        }
        self.refresh_media(tab, true);
    }

    /// Draws what changed in the media boxes of `tab` (all of them when `all`).
    pub(super) fn refresh_media(&mut self, tab: TabId, all: bool) {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let Tab {
            media,
            laid,
            frame,
            scroll,
            ..
        } = t;
        let (Some(laid), Some(canvas)) = (laid.as_ref(), frame.as_mut()) else {
            return;
        };
        let mut damage: Option<(u32, u32, u32, u32)> = None;
        for b in page::media_boxes(laid) {
            let Some(p) = media.players.get(&b.src) else {
                continue;
            };
            let now = (p.status(), p.picture().0);
            let changed = media.shown.get(&b.src) != Some(&now);
            // the bar moves while playing: redraw at least with every picture or state
            if !(all || changed || now.0 == Status::Playing || matches!(now.0, Status::Loading(_)))
            {
                continue;
            }
            media.shown.insert(b.src.clone(), now);
            let view = media.view(&b.src, &self.tr);
            if let Some(d) =
                self.painting
                    .draw_media_box(canvas, &b, *scroll, &view, &name_of(&b.src))
            {
                damage = Some(damage.map_or(d, |a| {
                    (a.0.min(d.0), a.1.min(d.1), a.2.max(d.2), a.3.max(d.3))
                }));
            }
        }
        if let Some((x0, y0, x1, y1)) = damage.filter(|d| d.2 > d.0 && d.3 > d.1) {
            self.events.push(EngineEvent::Frame {
                tab,
                damage: FrameDamage { x0, y0, x1, y1 },
            });
        }
    }

    /// After a wake: every tab with players draws what changed.
    pub(super) fn refresh_all_media(&mut self) {
        let tabs: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|(_, t)| !t.media.players.is_empty())
            .map(|(id, _)| *id)
            .collect();
        for tab in tabs {
            self.refresh_media(tab, false);
        }
    }

    /// A page that is a sound or video file: one box, playing at once.
    pub(super) fn autoplay(&mut self, tab: TabId) {
        let first = self
            .tabs
            .get(&tab)
            .and_then(|t| t.laid.as_ref())
            .and_then(|l| page::media_boxes(l).into_iter().next());
        if let Some(b) = first {
            self.click_media(tab, &b);
        }
    }
}

/// The name shown under a box: the last part of the address.
fn name_of(src: &str) -> String {
    super::html::Block::media(super::html::MediaKind::Audio, src, "").text
}

fn fetch_reason(e: &fetch::FetchError) -> String {
    match e {
        fetch::FetchError::Unreachable(why) => why.clone(),
        fetch::FetchError::Status(code) => format!("HTTP {code}"),
        fetch::FetchError::NotText(mime) => mime.clone(),
        fetch::FetchError::TooLarge => format!("> {} MB", fetch::MAX_MEDIA >> 20),
        fetch::FetchError::Blocked(_) => "blocked".into(),
    }
}
