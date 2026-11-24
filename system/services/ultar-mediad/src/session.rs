//! The media session, without the bus.

use ultar_bus::messages::media::{
    Button, MediaOutcome, MediaState, NowPlaying, Update, HEARTBEAT_S, MAX_TEXT,
};

/// The player that holds the session and when it last reported (ms of a monotonic clock).
#[derive(Debug, Default)]
pub struct Session {
    now: Option<(NowPlaying, u64)>,
}

impl Session {
    /// The bus view.
    pub fn state(&self) -> MediaState {
        MediaState {
            now: self.now.as_ref().map(|(n, _)| n.clone()),
        }
    }

    /// `player` reports; it takes the session.
    pub fn update(&mut self, player: &str, u: Update, now_ms: u64) -> MediaOutcome {
        if u.title.len() > MAX_TEXT || u.artist.len() > MAX_TEXT {
            return MediaOutcome::Invalid;
        }
        self.now = Some((
            NowPlaying {
                player: player.to_owned(),
                title: u.title,
                artist: u.artist,
                playing: u.playing,
                can_next: u.can_next,
                can_previous: u.can_previous,
            },
            now_ms,
        ));
        MediaOutcome::Done
    }

    /// `player` lets go (only the holder can).
    pub fn release(&mut self, player: &str) -> MediaOutcome {
        match &self.now {
            Some((n, _)) if n.player == player => {
                self.now = None;
                MediaOutcome::Done
            }
            Some(_) => MediaOutcome::Unsupported,
            None => MediaOutcome::NoPlayer,
        }
    }

    /// A button: the player it goes to, or why not.
    pub fn press(&self, button: Button) -> Result<String, MediaOutcome> {
        let Some((n, _)) = &self.now else {
            return Err(MediaOutcome::NoPlayer);
        };
        let possible = match button {
            Button::PlayPause => true,
            Button::Next => n.can_next,
            Button::Previous => n.can_previous,
        };
        if possible {
            Ok(n.player.clone())
        } else {
            Err(MediaOutcome::Unsupported)
        }
    }

    /// Drops a player that has not reported for [`HEARTBEAT_S`]; `true` if it did.
    pub fn expire(&mut self, now_ms: u64) -> bool {
        let stale = self
            .now
            .as_ref()
            .is_some_and(|(_, at)| now_ms.saturating_sub(*at) > HEARTBEAT_S * 1000);
        if stale {
            self.now = None;
        }
        stale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn up(title: &str, next: bool) -> Update {
        Update {
            title: title.into(),
            artist: "a".into(),
            playing: true,
            can_next: next,
            can_previous: false,
        }
    }

    #[test]
    fn the_latest_player_holds_the_session_until_it_goes() {
        let mut s = Session::default();
        assert_eq!(s.press(Button::PlayPause), Err(MediaOutcome::NoPlayer));
        assert_eq!(s.release("app.a"), MediaOutcome::NoPlayer);
        assert_eq!(s.update("app.a", up("one", false), 0), MediaOutcome::Done);
        assert_eq!(s.press(Button::Next), Err(MediaOutcome::Unsupported));
        assert_eq!(s.update("app.b", up("two", true), 10), MediaOutcome::Done);
        assert_eq!(s.press(Button::Next).unwrap(), "app.b");
        assert_eq!(s.release("app.a"), MediaOutcome::Unsupported);
        assert!(!s.expire(10 + HEARTBEAT_S * 1000));
        assert!(s.expire(11 + HEARTBEAT_S * 1000));
        assert_eq!(s.state(), MediaState::default());
        assert_eq!(
            s.update("app.a", up(&"x".repeat(MAX_TEXT + 1), false), 0),
            MediaOutcome::Invalid
        );
        s.update("app.a", up("one", false), 0);
        assert_eq!(s.release("app.a"), MediaOutcome::Done);
    }
}
