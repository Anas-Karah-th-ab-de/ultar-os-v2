//! Contracts of the notification service. They replace the append-only log
//! file and the 400 ms polling loop of the old shell.
//!
//! Version 2: a notification carries up to [`MAX_ACTIONS`] buttons
//! ([`Post::actions`]); [`Posted`] names the bus identity of the sender (set by the
//! service, it cannot be forged). The shell invokes a button with [`Invoke`]; the
//! sender learns it from the hint topic [`InvokedHint`] (carries only the
//! sender's identity) and fetches its own invoked actions with [`TakeInvoked`] –
//! no app sees another app's actions.

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

/// Most buttons of one notification.
pub const MAX_ACTIONS: usize = 3;
/// Longest action id (`[a-z0-9_]`).
pub const MAX_ACTION_ID: usize = 32;
/// Name of the service on the bus.
pub const SERVICE: &str = "notify";

/// One button of a notification.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Action {
    /// Id the sender gets back when the button is pressed (`[a-z0-9_]`, ≤ 32).
    pub id: String,
    /// Label as i18n key plus arguments.
    pub label: Text,
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
    /// Buttons (at most [`MAX_ACTIONS`]).
    pub actions: Vec<Action>,
}

/// Answer to [`Post`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PostReply {
    /// Identifier the service assigned to the notification.
    pub id: u64,
}

impl Message for Post {
    const VERSION: u16 = 2;
}

impl Message for PostReply {
    const VERSION: u16 = 1;
}

impl Call for Post {
    const SERVICE: &'static str = SERVICE;
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
    /// Bus identity of the sender (set by the service).
    pub sender: String,
    /// The notification as posted.
    pub post: Post,
}

impl Message for Posted {
    const VERSION: u16 = 2;
}

impl Topic for Posted {
    const TOPIC: &'static str = "notify.posted";
}

/// `notify.invoke` – the user pressed a button (the shell only).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Invoke {
    /// The notification.
    pub id: u64,
    /// The button's id.
    pub action: String,
}

/// `notify.dismiss` – the user closed a notification (the shell), or its sender
/// withdraws it.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dismiss {
    /// The notification.
    pub id: u64,
}

/// How [`Invoke`] or [`Dismiss`] went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteOutcome {
    /// Done.
    Done,
    /// No such notification or button (any more).
    Unknown,
    /// The caller may not do this.
    Refused,
}

/// Answer to [`Invoke`] and [`Dismiss`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct NoteReply {
    /// The result.
    pub outcome: NoteOutcome,
}

/// `notify.list` – the notifications that are still open (shell, lock screen).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct List {}

/// Answer to [`List`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ListReply {
    /// Open notifications, oldest first.
    pub notes: Vec<Posted>,
}

/// `notify.take_invoked` – the caller's invoked buttons (and forget them).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TakeInvoked {}

/// One invoked button.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Invoked {
    /// The notification.
    pub id: u64,
    /// The button's id.
    pub action: String,
}

/// Answer to [`TakeInvoked`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct TakeInvokedReply {
    /// Oldest first.
    pub invoked: Vec<Invoked>,
}

/// `notify.dismissed` – a notification is gone (closed, invoked, withdrawn, dropped).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dismissed {
    /// The notification.
    pub id: u64,
}

/// `notify.invoked` – a hint: `sender` has invoked buttons waiting ([`TakeInvoked`]).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct InvokedHint {
    /// Bus identity of the sender.
    pub sender: String,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    Invoke,
    Dismiss,
    NoteReply,
    List,
    ListReply,
    TakeInvoked,
    Invoked,
    TakeInvokedReply,
    Dismissed,
    InvokedHint
);

macro_rules! call {
    ($t:ty, $m:literal, $r:ty) => {
        impl Call for $t {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $m;
            type Reply = $r;
        }
    };
}
call!(Invoke, "invoke", NoteReply);
call!(Dismiss, "dismiss", NoteReply);
call!(List, "list", ListReply);
call!(TakeInvoked, "take_invoked", TakeInvokedReply);

impl Topic for Dismissed {
    const TOPIC: &'static str = "notify.dismissed";
}

impl Topic for InvokedHint {
    const TOPIC: &'static str = "notify.invoked";
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
            actions: vec![Action {
                id: "reply".into(),
                label: Text::key("notify.action.reply"),
            }],
        }
    }

    #[test]
    fn post_and_posted_round_trip() {
        let post = sms();
        assert_eq!(decode::<Post>(&encode(&post).unwrap()).unwrap(), post);
        let posted = Posted {
            id: 12,
            timestamp_ms: 1_700_000_000_000,
            sender: "app.messages".into(),
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
    fn action_messages_round_trip() {
        let i = Invoke {
            id: 3,
            action: "reply".into(),
        };
        assert_eq!(decode::<Invoke>(&encode(&i).unwrap()).unwrap(), i);
        let t = TakeInvokedReply {
            invoked: vec![Invoked {
                id: 3,
                action: "reply".into(),
            }],
        };
        assert_eq!(decode::<TakeInvokedReply>(&encode(&t).unwrap()).unwrap(), t);
        let h = InvokedHint {
            sender: "app.messages".into(),
        };
        assert_eq!(decode::<InvokedHint>(&encode(&h).unwrap()).unwrap(), h);
        let r = NoteReply {
            outcome: NoteOutcome::Refused,
        };
        assert_eq!(decode::<NoteReply>(&encode(&r).unwrap()).unwrap(), r);
        assert_eq!(<Post as Message>::VERSION, 2);
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
