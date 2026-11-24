//! The open notifications and the pressed buttons, without the bus.

use std::collections::{BTreeMap, VecDeque};
use ultar_bus::messages::notify::{
    Body, Invoked, NoteOutcome, Post, Posted, Urgency, MAX_ACTIONS, MAX_ACTION_ID,
};
use ultar_bus::{Text, TextArg};

/// Most open notifications.
pub const MAX_OPEN: usize = 64;
/// Most posts per sender in [`RATE_WINDOW_MS`].
pub const MAX_POSTS: u32 = 30;
/// Window of the post limit.
pub const RATE_WINDOW_MS: u64 = 60_000;
/// Most waiting pressed buttons per sender.
pub const MAX_INVOKED: usize = 16;
/// Most senders with waiting pressed buttons (and in the rate table).
pub const MAX_SENDERS: usize = 64;
/// Longest key of a text.
pub const MAX_KEY: usize = 128;
/// Longest argument or application id.
pub const MAX_ARG: usize = 512;
/// Longest body written by a person.
pub const MAX_BODY: usize = 4096;
/// Most arguments of a text.
pub const MAX_ARGS: usize = 8;

/// Why a post was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PostError {
    /// A field is too long, a key or an action id is malformed, too many buttons.
    Invalid,
    /// The sender posted too much in the last minute.
    TooMany,
}

impl std::fmt::Display for PostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid => f.write_str("invalid notification"),
            Self::TooMany => f.write_str("too many notifications"),
        }
    }
}

impl std::error::Error for PostError {}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

fn valid_text(text: &Text) -> bool {
    valid_key(&text.key)
        && text.args.len() <= MAX_ARGS
        && text.args.iter().all(|a| match a {
            TextArg::Verbatim(s) => s.len() <= MAX_ARG,
            _ => true,
        })
}

fn valid_action_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ACTION_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Checks every field of a post.
pub fn valid(post: &Post) -> bool {
    let body = match &post.body {
        Body::Empty => true,
        Body::Localized(t) => valid_text(t),
        Body::Verbatim(s) => s.len() <= MAX_BODY,
    };
    let ids_unique = post
        .actions
        .iter()
        .enumerate()
        .all(|(i, a)| !post.actions[..i].iter().any(|b| b.id == a.id));
    body && ids_unique
        && valid_text(&post.title)
        && post.app_id.len() <= MAX_ARG
        && post.actions.len() <= MAX_ACTIONS
        && post
            .actions
            .iter()
            .all(|a| valid_action_id(&a.id) && valid_text(&a.label))
}

/// The open notifications and the pressed buttons.
#[derive(Debug, Default)]
pub struct Notes {
    next_id: u64,
    open: VecDeque<Posted>,
    invoked: BTreeMap<String, VecDeque<Invoked>>,
    rate: BTreeMap<String, (u64, u32)>,
}

impl Notes {
    fn allow(&mut self, sender: &str, now_ms: u64) -> bool {
        if !self.rate.contains_key(sender) && self.rate.len() >= MAX_SENDERS {
            self.rate
                .retain(|_, (start, _)| now_ms.saturating_sub(*start) < RATE_WINDOW_MS);
            if self.rate.len() >= MAX_SENDERS {
                return false;
            }
        }
        let (start, count) = self.rate.entry(sender.to_owned()).or_insert((now_ms, 0));
        if now_ms.saturating_sub(*start) >= RATE_WINDOW_MS {
            *start = now_ms;
            *count = 0;
        }
        *count += 1;
        *count <= MAX_POSTS
    }

    /// Accepts a post from `sender`; returns it numbered and the ids that had to go.
    pub fn post(
        &mut self,
        sender: &str,
        post: Post,
        now_ms: u64,
    ) -> Result<(Posted, Vec<u64>), PostError> {
        if !valid(&post) {
            return Err(PostError::Invalid);
        }
        if !self.allow(sender, now_ms) {
            return Err(PostError::TooMany);
        }
        let mut dropped = Vec::new();
        while self.open.len() >= MAX_OPEN {
            let lowest = self
                .open
                .iter()
                .map(|p| p.post.urgency)
                .min()
                .unwrap_or(Urgency::Low);
            let index = self
                .open
                .iter()
                .position(|p| p.post.urgency == lowest)
                .unwrap_or(0);
            if let Some(gone) = self.open.remove(index) {
                dropped.push(gone.id);
            }
        }
        self.next_id += 1;
        let posted = Posted {
            id: self.next_id,
            timestamp_ms: now_ms,
            sender: sender.to_owned(),
            post,
        };
        self.open.push_back(posted.clone());
        Ok((posted, dropped))
    }

    /// The open notifications, oldest first.
    pub fn list(&self) -> Vec<Posted> {
        self.open.iter().cloned().collect()
    }

    /// Closes `id`. `owner_or_system`: the caller is the shell or `id`'s sender.
    pub fn dismiss(&mut self, id: u64, caller: &str, system: bool) -> NoteOutcome {
        let Some(index) = self.open.iter().position(|p| p.id == id) else {
            return NoteOutcome::Unknown;
        };
        if !system && self.open[index].sender != caller {
            return NoteOutcome::Refused;
        }
        self.open.remove(index);
        NoteOutcome::Done
    }

    /// Presses button `action` of `id`: closes it and queues the press for its sender.
    /// Returns the sender.
    pub fn invoke(&mut self, id: u64, action: &str) -> Result<String, NoteOutcome> {
        let index = self
            .open
            .iter()
            .position(|p| p.id == id && p.post.actions.iter().any(|a| a.id == action))
            .ok_or(NoteOutcome::Unknown)?;
        let Some(note) = self.open.remove(index) else {
            return Err(NoteOutcome::Unknown);
        };
        if !self.invoked.contains_key(&note.sender) && self.invoked.len() >= MAX_SENDERS {
            if let Some(first) = self.invoked.keys().next().cloned() {
                self.invoked.remove(&first);
            }
        }
        let queue = self.invoked.entry(note.sender.clone()).or_default();
        if queue.len() >= MAX_INVOKED {
            queue.pop_front();
        }
        queue.push_back(Invoked {
            id,
            action: action.to_owned(),
        });
        Ok(note.sender)
    }

    /// The pressed buttons waiting for `sender` (and forgets them).
    pub fn take(&mut self, sender: &str) -> Vec<Invoked> {
        self.invoked
            .remove(sender)
            .map(Vec::from)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::messages::notify::{Action, Category};

    fn post(urgency: Urgency, actions: &[&str]) -> Post {
        Post {
            urgency,
            category: Category::App,
            app_id: "tech.code-craft.notes".into(),
            title: Text::key("notes.reminder").with(TextArg::Verbatim("x".into())),
            body: Body::Verbatim("body".into()),
            actions: actions
                .iter()
                .map(|id| Action {
                    id: (*id).into(),
                    label: Text::key("notes.action.open"),
                })
                .collect(),
        }
    }

    #[test]
    fn invalid_posts_are_refused() {
        let mut n = Notes::default();
        let mut bad = vec![
            post(Urgency::Normal, &["a", "b", "c", "d"]),
            post(Urgency::Normal, &["A"]),
            post(Urgency::Normal, &["a", "a"]),
        ];
        let mut p = post(Urgency::Normal, &[]);
        p.title = Text::key("Bad Key");
        bad.push(p);
        let mut p = post(Urgency::Normal, &[]);
        p.body = Body::Verbatim("x".repeat(MAX_BODY + 1));
        bad.push(p);
        let mut p = post(Urgency::Normal, &[]);
        p.app_id = "x".repeat(MAX_ARG + 1);
        bad.push(p);
        let mut p = post(Urgency::Normal, &[]);
        p.body = Body::Localized(Text::key("k").with(TextArg::Verbatim("x".repeat(MAX_ARG + 1))));
        bad.push(p);
        for p in bad {
            assert_eq!(n.post("app.a", p, 0).err(), Some(PostError::Invalid));
        }
        assert!(n.list().is_empty());
        assert!(
            !PostError::TooMany.to_string().is_empty()
                && !PostError::Invalid.to_string().is_empty()
        );
    }

    #[test]
    fn buttons_go_back_to_their_sender_only() {
        let mut n = Notes::default();
        let (a, _) = n
            .post("app.a", post(Urgency::Normal, &["open", "snooze"]), 1)
            .unwrap();
        let (b, _) = n
            .post("app.b", post(Urgency::Normal, &["open"]), 2)
            .unwrap();
        assert_eq!((a.id, b.id, a.sender.as_str()), (1, 2, "app.a"));
        assert_eq!(n.invoke(a.id, "nope"), Err(NoteOutcome::Unknown));
        assert_eq!(n.invoke(a.id, "snooze").unwrap(), "app.a");
        assert_eq!(
            n.invoke(a.id, "snooze"),
            Err(NoteOutcome::Unknown),
            "closed by the press"
        );
        assert!(n.take("app.b").is_empty());
        let got = n.take("app.a");
        assert_eq!((got.len(), got[0].action.as_str()), (1, "snooze"));
        assert!(n.take("app.a").is_empty());
        assert_eq!(n.dismiss(b.id, "app.a", false), NoteOutcome::Refused);
        assert_eq!(n.dismiss(b.id, "app.b", false), NoteOutcome::Done);
        assert_eq!(n.dismiss(b.id, "system.shell", true), NoteOutcome::Unknown);
    }

    #[test]
    fn limits_hold() {
        let mut n = Notes::default();
        for i in 0..MAX_POSTS {
            n.post("app.a", post(Urgency::Normal, &[]), u64::from(i))
                .unwrap();
        }
        assert_eq!(
            n.post("app.a", post(Urgency::Normal, &[]), 100).err(),
            Some(PostError::TooMany)
        );
        n.post("app.a", post(Urgency::Normal, &[]), RATE_WINDOW_MS + 1)
            .unwrap();
        // Fill up with high ones from many senders: the low/normal ones go first.
        let mut dropped_total = 0;
        for i in 0..MAX_OPEN {
            let sender = format!("app.s{}", i % 8);
            let (_, dropped) = n
                .post(&sender, post(Urgency::High, &["x"]), 10 + i as u64)
                .unwrap();
            dropped_total += dropped.len();
        }
        assert_eq!(n.list().len(), MAX_OPEN);
        assert!(n.list().iter().all(|p| p.post.urgency == Urgency::High));
        assert_eq!(dropped_total, MAX_POSTS as usize + 1);
        // Invoked queues are bounded.
        let ids: Vec<u64> = n
            .list()
            .iter()
            .filter(|p| p.sender == "app.s0")
            .map(|p| p.id)
            .collect();
        for id in &ids {
            n.invoke(*id, "x").unwrap();
        }
        assert!(n.take("app.s0").len() <= MAX_INVOKED);
        // The rate table is bounded too.
        let mut m = Notes::default();
        for i in 0..MAX_SENDERS {
            m.post(&format!("app.r{i}"), post(Urgency::Low, &[]), 0)
                .unwrap();
        }
        assert_eq!(
            m.post("app.new", post(Urgency::Low, &[]), 1).err(),
            Some(PostError::TooMany)
        );
        assert!(m
            .post("app.new", post(Urgency::Low, &[]), RATE_WINDOW_MS + 5)
            .is_ok());
    }

    #[test]
    fn fuzzed_posts_never_panic_and_stay_bounded() {
        let mut seed = 0x1234_5678_u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut n = Notes::default();
        for i in 0..5000u64 {
            let r = next();
            let len = (r % 40) as usize;
            let id: String = (0..len)
                .map(|k| (b'a' + ((r >> k) % 30) as u8) as char)
                .collect();
            let mut p = post(Urgency::Low, &[]);
            p.actions = vec![Action {
                id: id.clone(),
                label: Text::key(&id),
            }];
            let _ = n.post(&format!("app.f{}", r % 70), p, i * 1000);
            let _ = n.invoke(r % 200, &id);
            let _ = n.dismiss(r % 200, "app.f1", r % 2 == 0);
        }
        assert!(n.list().len() <= MAX_OPEN);
        assert!(n.invoked.len() <= MAX_SENDERS && n.rate.len() <= MAX_SENDERS);
    }
}
