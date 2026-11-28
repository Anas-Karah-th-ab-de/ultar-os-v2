//! Who sits in front of the screen, from `session.state`: a session ends the moment the
//! person in front changes or nobody is in front (sign-in screen, user switch, sign-out).
//! This is the first way to know the screen is not the viewed person's any more; the capture
//! that stops delivering (a plain lock) stays as the fallback in [`crate::screen`].

/// The person in front, as far as `session.state` has told.
#[derive(Default, Debug)]
pub struct Seat {
    /// The last state seen: `None` = not heard yet, `Some(None)` = nobody in front.
    now: Option<Option<String>>,
    /// A session runs.
    running: bool,
    /// The person in front when it began (`None` while that is not known yet).
    at_begin: Option<Option<String>>,
}

impl Seat {
    /// `session.state` arrived with `user` in front; `true` when a running session must end.
    pub fn observe(&mut self, user: Option<String>) -> bool {
        self.now = Some(user.clone());
        if !self.running {
            return false;
        }
        match &self.at_begin {
            // the first news during a session that began before any was heard
            None if user.is_some() => {
                self.at_begin = Some(user);
                false
            }
            None => true,
            Some(begin) => user.is_none() || user != *begin,
        }
    }

    /// A session begins; `false` when nobody is in front: it must not start. Without news
    /// yet the capture decides.
    pub fn begin(&mut self) -> bool {
        self.running = true;
        self.at_begin = self.now.clone();
        self.now.as_ref().is_none_or(|u| u.is_some())
    }

    /// The session is over.
    pub fn end(&mut self) {
        self.running = false;
        self.at_begin = None;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn user(n: &str) -> Option<String> {
        Some(n.to_owned())
    }

    #[test]
    fn the_same_person_in_front_keeps_the_session() {
        let mut s = Seat::default();
        assert!(!s.observe(user("anas")));
        assert!(s.begin());
        assert!(!s.observe(user("anas")), "the same state again");
    }

    #[test]
    fn nobody_in_front_or_another_person_ends_it() {
        let mut s = Seat::default();
        s.observe(user("anas"));
        assert!(s.begin());
        assert!(s.observe(None), "sign-in screen");
        let mut s = Seat::default();
        s.observe(user("anas"));
        assert!(s.begin());
        assert!(s.observe(user("sara")), "another person");
    }

    #[test]
    fn nothing_ends_when_no_session_runs_and_no_session_starts_at_the_sign_in_screen() {
        let mut s = Seat::default();
        assert!(!s.observe(None));
        assert!(!s.begin(), "nobody in front");
        s.end();
        assert!(!s.observe(user("anas")), "not running");
    }

    #[test]
    fn a_state_not_heard_yet_leaves_it_to_the_capture() {
        let mut s = Seat::default();
        assert!(s.begin());
        assert!(
            s.observe(None),
            "then the first news says nobody is in front"
        );
        let mut s = Seat::default();
        assert!(s.begin());
        assert!(
            !s.observe(user("anas")),
            "the first news names the person: that is who began"
        );
    }
}
