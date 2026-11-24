//! Contract of the media service: what is playing, for the lock screen's media card,
//! and its buttons.
//!
//! A player (an app) reports what it plays with [`Update`] at least every
//! [`HEARTBEAT_S`] seconds while it holds the session; the latest player that reported
//! holds it. The shell and the lock screen press the buttons ([`PlayPause`], [`Next`],
//! [`Previous`]); the player hears them on [`Command`] (it keeps only the ones naming
//! its own bus identity). Without a player [`MediaState::now`] is `None`: no card.

use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "media";
/// A player that has not reported for this long has gone.
pub const HEARTBEAT_S: u64 = 30;
/// Longest title or artist in bytes.
pub const MAX_TEXT: usize = 256;

/// What is playing.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct NowPlaying {
    /// Bus identity of the player.
    pub player: String,
    /// Title (data from the media, not interface text).
    pub title: String,
    /// Artist (data).
    pub artist: String,
    /// Playing (else paused).
    pub playing: bool,
    /// The player can go to the next item.
    pub can_next: bool,
    /// The player can go to the previous item.
    pub can_previous: bool,
}

/// `media.state` – the media session (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct MediaState {
    /// What is playing; `None`: no player, no card.
    pub now: Option<NowPlaying>,
}

/// `media.update` – a player reports what it plays (and takes the session).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Update {
    /// Title.
    pub title: String,
    /// Artist.
    pub artist: String,
    /// Playing.
    pub playing: bool,
    /// Next is possible.
    pub can_next: bool,
    /// Previous is possible.
    pub can_previous: bool,
}

/// `media.release` – the player stops holding the session.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Release {}

/// `media.play_pause` – the play/pause button.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PlayPause {}

/// `media.next` – the next button.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Next {}

/// `media.previous` – the previous button.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Previous {}

/// A button for the player.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Button {
    /// Play or pause.
    PlayPause,
    /// Next item.
    Next,
    /// Previous item.
    Previous,
}

/// `media.command` – a button pressed for `player`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Command {
    /// Bus identity of the player meant.
    pub player: String,
    /// The button.
    pub button: Button,
}

/// How a call went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaOutcome {
    /// Done (for buttons: sent to the player).
    Done,
    /// No player holds the session.
    NoPlayer,
    /// The player cannot do this (next/previous), or the caller does not hold the session.
    Unsupported,
    /// A field is too long.
    Invalid,
}

/// Answer to every media call.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct MediaReply {
    /// The result.
    pub outcome: MediaOutcome,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    NowPlaying, MediaState, Update, Release, PlayPause, Next, Previous, Command, MediaReply
);

impl Topic for MediaState {
    const TOPIC: &'static str = "media.state";
    const RETAINED: bool = true;
}

impl Topic for Command {
    const TOPIC: &'static str = "media.command";
}

macro_rules! call {
    ($t:ty, $m:literal) => {
        impl Call for $t {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $m;
            type Reply = MediaReply;
        }
    };
}
call!(Update, "update");
call!(Release, "release");
call!(PlayPause, "play_pause");
call!(Next, "next");
call!(Previous, "previous");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn messages_round_trip() {
        let s = MediaState {
            now: Some(NowPlaying {
                player: "app.music".into(),
                title: "فيروز".into(),
                artist: "x".into(),
                playing: true,
                can_next: true,
                can_previous: false,
            }),
        };
        assert_eq!(decode::<MediaState>(&encode(&s).unwrap()).unwrap(), s);
        let c = Command {
            player: "app.music".into(),
            button: Button::Next,
        };
        assert_eq!(decode::<Command>(&encode(&c).unwrap()).unwrap(), c);
    }
}
