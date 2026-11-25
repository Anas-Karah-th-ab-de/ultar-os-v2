//! The periodic work of the volume service and the notifications it posts: the low-space
//! warning (once per drop, re-armed only after the free space rose again) and the daily
//! clean-up of the downloads folder. The decisions here are pure so they can be tested
//! without a bus; the service posts the notifications they ask for.

use ultar_bus::messages::notify::{Action, Body, Category, Post, Urgency};
use ultar_bus::{Text, TextArg};

/// Seconds of one day (the clean-up runs at most once a day).
pub const DAY: u64 = 86_400;

/// The sender identity of the volume service's notifications (the shell names it).
pub const APP_ID: &str = "org.ultar.volumed";

/// The button of a drive offer that mounts the drive.
pub const MOUNT_ACTION: &str = "mount";

/// The state of the periodic work between ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Jobs {
    /// A low-space warning is out (not posted again until the space rose).
    low_warned: bool,
    /// When the downloads folder was last cleaned (`None`: never).
    last_clean: Option<u64>,
}

impl Jobs {
    /// The low-space warning: `Some(percent)` when the free percentage has just dropped
    /// below `threshold` and no warning is out; the warning is re-armed only once the free
    /// percentage reached `threshold` again. `None` while nothing new is to say.
    pub fn low_space(&mut self, total: u64, free: u64, threshold: u8) -> Option<u8> {
        if total == 0 {
            return None;
        }
        let percent = (free.saturating_mul(100) / total).min(100) as u8;
        if percent < threshold {
            if self.low_warned {
                None
            } else {
                self.low_warned = true;
                Some(percent)
            }
        } else {
            self.low_warned = false;
            None
        }
    }

    /// Whether the daily clean-up should run now: it is switched on and either never ran
    /// or a day has passed since the last run.
    pub fn clean_due(&self, enabled: bool, now: u64) -> bool {
        enabled
            && self
                .last_clean
                .is_none_or(|last| now.saturating_sub(last) >= DAY)
    }

    /// Records that the clean-up ran at `now`.
    pub fn cleaned(&mut self, now: u64) {
        self.last_clean = Some(now);
    }
}

/// The notification that the data partition is almost full (`percent` free).
pub fn low_space_post(percent: u8) -> Post {
    Post {
        urgency: Urgency::Normal,
        category: Category::System,
        app_id: APP_ID.to_owned(),
        title: Text::key("volumed.low_space.title"),
        body: Body::Localized(
            Text::key("volumed.low_space.body").with(TextArg::Number(i64::from(percent))),
        ),
        actions: Vec::new(),
    }
}

/// The notification that offers a removable drive (`name`) instead of mounting it: one
/// button ([`MOUNT_ACTION`]) mounts it for the person in front.
pub fn drive_post(name: &str) -> Post {
    Post {
        urgency: Urgency::Normal,
        category: Category::System,
        app_id: APP_ID.to_owned(),
        title: Text::key("volumed.drive.title").with(TextArg::Verbatim(name.to_owned())),
        body: Body::Localized(Text::key("volumed.drive.body")),
        actions: vec![Action {
            id: MOUNT_ACTION.to_owned(),
            label: Text::key("volumed.drive.mount"),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_space_warns_once_and_rearms_only_above_the_threshold() {
        let mut j = Jobs::default();
        // 9% free of 100: below 10 → warn once.
        assert_eq!(j.low_space(100, 9, 10), Some(9));
        assert_eq!(j.low_space(100, 9, 10), None);
        assert_eq!(j.low_space(100, 5, 10), None);
        // Still below: quiet. At the threshold again: re-armed, but no warning.
        assert_eq!(j.low_space(100, 10, 10), None);
        // Drops below once more: a new warning.
        assert_eq!(j.low_space(100, 8, 10), Some(8));
        // Still below the (now higher) threshold: no second warning until it rises.
        assert_eq!(j.low_space(100, 25, 30), None);
        assert_eq!(j.low_space(100, 30, 30), None); // reached 30: re-armed
        assert_eq!(j.low_space(100, 29, 30), Some(29));
    }

    #[test]
    fn low_space_measures_the_percentage_and_guards_zero() {
        let mut j = Jobs::default();
        assert_eq!(j.low_space(0, 0, 10), None);
        // 1 of 8 free = 12%: not below 10.
        assert_eq!(j.low_space(8, 1, 10), None);
        // 1 of 20 free = 5%: below 10.
        assert_eq!(j.low_space(20, 1, 10), Some(5));
        // Full: re-armed.
        assert_eq!(j.low_space(20, 20, 10), None);
        assert_eq!(j.low_space(20, 0, 10), Some(0));
    }

    #[test]
    fn clean_up_is_daily_and_only_when_switched_on() {
        let mut j = Jobs::default();
        assert!(!j.clean_due(false, 100));
        assert!(j.clean_due(true, 100)); // never ran
        j.cleaned(100);
        assert!(!j.clean_due(true, 100 + DAY - 1));
        assert!(j.clean_due(true, 100 + DAY));
        assert!(!j.clean_due(false, 100 + 2 * DAY));
    }

    #[test]
    fn the_notifications_carry_keys_and_the_mount_button() {
        let low = low_space_post(7);
        assert_eq!(low.app_id, APP_ID);
        assert_eq!(low.category, Category::System);
        assert_eq!(low.title.key, "volumed.low_space.title");
        assert!(low.actions.is_empty());
        match low.body {
            Body::Localized(t) => {
                assert_eq!(t.key, "volumed.low_space.body");
                assert_eq!(t.args, vec![TextArg::Number(7)]);
            }
            other => panic!("body {other:?}"),
        }
        let drive = drive_post("STICK");
        assert_eq!(drive.title.key, "volumed.drive.title");
        assert_eq!(drive.title.args, vec![TextArg::Verbatim("STICK".into())]);
        assert_eq!(drive.actions.len(), 1);
        assert_eq!(drive.actions[0].id, MOUNT_ACTION);
        assert_eq!(drive.actions[0].label.key, "volumed.drive.mount");
    }
}
