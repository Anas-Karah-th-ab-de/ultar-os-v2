//! Contracts of the notification service. They replace the append-only log
//! file and the 400 ms polling loop of the old shell.

use crate::message::{Call, Message, Text, Topic};
use serde::{Deserialize, Serialize};

/// How strongly a notification asks for attention.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, PartialOrd, Ord)]
pub enum Urgency {
    /// Shown in the panel only.
    Low,
    /// Shown with the usual sound or vibration.
    #[default]
    Normal,
    /// Alerts immediately and wakes the screen (incoming call).
    High,
}

/// What a notification is about.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Category {
    /// Incoming or missed call.
    Call,
    /// Text message from the mobile network.
    Sms,
    /// Message from a chat application.
    Message,
    /// Notice from the system itself.
    System,
    /// Anything else an application reports.
    #[default]
    App,
}

/// Body of a notification.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum Body {
    /// No body.
    Empty,
    /// Interface text: i18n key plus arguments.
    Localized(Text),
    /// Content written by a person (SMS text, chat message). This is data,
    /// not interface text, and is shown exactly as received.
    Verbatim(String),
}

/// `notify.post` – ask the notification service to show a notification.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Post {
    /// Attention level.
    pub urgency: Urgency,
    /// Kind of notification.
    pub category: Category,
    /// Application identifier of the sender, e.g. `tech.code-craft.phone`.
    pub app_id: String,
    /// Title as i18n key plus arguments.
    pub title: Text,
    /// Body.
    pub body: Body,
}

/// Answer to [`Post`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PostReply {
    /// Identifier the service assigned to the notification.
    pub id: u64,
}

impl Message for Post {
    const VERSION: u16 = 1;
}

impl Message for PostReply {
    const VERSION: u16 = 1;
}

impl Call for Post {
    const SERVICE: &'static str = "notify";
    const METHOD: &'static str = "post";
    type Reply = PostReply;
}

/// `notify.posted` – a notification was accepted and should be shown.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Posted {
    /// Identifier assigned by the notification service.
    pub id: u64,
    /// Milliseconds since the Unix epoch when it was accepted.
    pub timestamp_ms: u64,
    /// The notification as posted.
    pub post: Post,
}

impl Message for Posted {
    const VERSION: u16 = 1;
}

impl Topic for Posted {
    const TOPIC: &'static str = "notify.posted";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode, TextArg};

    fn sms() -> Post {
        Post {
            urgency: Urgency::Normal,
            category: Category::Sms,
            app_id: "tech.code-craft.messages".into(),
            title: Text::key("notify.sms.from").with(TextArg::Verbatim("+963 000".into())),
            body: Body::Verbatim("مرحبا".into()),
        }
    }

    #[test]
    fn post_and_posted_round_trip() {
        let post = sms();
        assert_eq!(decode::<Post>(&encode(&post).unwrap()).unwrap(), post);
        let posted = Posted {
            id: 12,
            timestamp_ms: 1_700_000_000_000,
            post,
        };
        assert_eq!(decode::<Posted>(&encode(&posted).unwrap()).unwrap(), posted);
        let reply = PostReply { id: 12 };
        assert_eq!(
            decode::<PostReply>(&encode(&reply).unwrap()).unwrap(),
            reply
        );
    }

    #[test]
    fn urgency_orders_and_defaults() {
        assert!(Urgency::Low < Urgency::Normal && Urgency::Normal < Urgency::High);
        assert_eq!(Urgency::default(), Urgency::Normal);
        assert_eq!(Category::default(), Category::App);
    }

    #[test]
    fn all_body_and_category_variants_round_trip() {
        let bodies = [
            Body::Empty,
            Body::Localized(Text::key("notify.update.ready")),
            Body::Verbatim("x".into()),
        ];
        for body in bodies {
            assert_eq!(decode::<Body>(&encode(&body).unwrap()).unwrap(), body);
        }
        let categories = [
            Category::Call,
            Category::Sms,
            Category::Message,
            Category::System,
            Category::App,
        ];
        for category in categories {
            assert_eq!(
                decode::<Category>(&encode(&category).unwrap()).unwrap(),
                category
            );
        }
    }
}
