//! What a sender says about the privacy of a notification, for mirroring it to the person's
//! other devices (mesh item C2): `notify.post_ex` is `notify.post` plus a [`Privacy`].
//! A notification posted with plain `notify.post` is mirrored like [`Privacy::Normal`].
//! The Posted topic does not change: the mirror is the notification service's own business.

use crate::message::{Call, Message};
use crate::messages::notify::{Post, PostReply, SERVICE};
use serde::{Deserialize, Serialize};

/// How far a notification may travel.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Privacy {
    /// May be mirrored; a target that is locked (nobody signed in) shows only that
    /// something arrived, without its content (the default).
    #[default]
    Normal,
    /// May be mirrored, but a target that is locked shows nothing at all.
    HideOnLocked,
    /// Never leaves this device (a password, a one-time code, anything sensitive).
    Private,
}

/// `notify.post_ex` - a notification with its privacy.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct PostEx {
    /// The notification as for `notify.post`.
    pub post: Post,
    /// How far it may travel.
    pub privacy: Privacy,
}

impl Message for Privacy {
    const VERSION: u16 = 1;
}

impl Message for PostEx {
    const VERSION: u16 = 1;
}

impl Call for PostEx {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "post_ex";
    type Reply = PostReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn privacy_round_trips_and_defaults_to_normal() {
        for p in [Privacy::Normal, Privacy::HideOnLocked, Privacy::Private] {
            assert_eq!(decode::<Privacy>(&encode(&p).unwrap()).unwrap(), p);
        }
        assert_eq!(Privacy::default(), Privacy::Normal);
    }
}
