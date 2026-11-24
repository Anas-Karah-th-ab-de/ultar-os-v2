//! Contract of the screenshot service.
//!
//! The shell's screenshot button asks for one output (or the primary one); the service
//! reads its pixels from the compositor (right `ui.capture`) and saves a PNG in the
//! `Pictures` folder of the person in front, owned by that person. The compositor
//! sends nothing while the session is locked: then the answer is `Failed`.

use crate::message::{Call, Message};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "capture";

/// `capture.screenshot` – save what one output shows.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Screenshot {
    /// Compositor id of the output; `None`: the primary output.
    pub output: Option<u32>,
}

/// How a screenshot went.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum ShotOutcome {
    /// Saved at this path (in the person's `Pictures` folder).
    Saved(String),
    /// Nobody is in front (no folder to save to).
    NoUser,
    /// No such output, or no compositor.
    NoDisplay,
    /// Another screenshot is being taken.
    Busy,
    /// Reading or saving failed (locked session, full disk, …).
    Failed,
}

/// Answer to [`Screenshot`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ScreenshotReply {
    /// The result.
    pub outcome: ShotOutcome,
}

impl Message for Screenshot {
    const VERSION: u16 = 1;
}
impl Message for ScreenshotReply {
    const VERSION: u16 = 1;
}
impl Call for Screenshot {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "screenshot";
    type Reply = ScreenshotReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn messages_round_trip() {
        let r = ScreenshotReply {
            outcome: ShotOutcome::Saved("/home/amal/Pictures/a.png".into()),
        };
        assert_eq!(decode::<ScreenshotReply>(&encode(&r).unwrap()).unwrap(), r);
        let s = Screenshot { output: Some(2) };
        assert_eq!(decode::<Screenshot>(&encode(&s).unwrap()).unwrap(), s);
    }
}
