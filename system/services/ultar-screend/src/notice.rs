//! The notification that shows on this device while it is watched, with the button that
//! ends it: the person here can always stop a remote view, whatever the viewer does.

use ultar_bus::messages::notify::{Action, Body, Category, Post, Urgency};
use ultar_bus::{Text, TextArg};

/// Sender name of the notification (the shell names it).
pub const APP_ID: &str = "org.ultar.screend";
/// Id of the button that stops the session.
pub const STOP: &str = "stop";

/// The notification "`viewer` is watching and operating this screen" with a Stop button.
pub fn watched(viewer: &str) -> Post {
    Post {
        urgency: Urgency::High,
        category: Category::System,
        app_id: APP_ID.to_owned(),
        title: Text::key("desk.note.screend").with(TextArg::Verbatim(viewer.to_owned())),
        body: Body::Localized(Text::key("desk.note.screend.body")),
        actions: vec![Action {
            id: STOP.to_owned(),
            label: Text::key("desk.note.screend.stop"),
        }],
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_notification_names_the_viewer_as_data_and_offers_stop() {
        let post = watched("Amal's phone");
        assert_eq!(post.title.key, "desk.note.screend");
        assert_eq!(
            post.title.args,
            vec![TextArg::Verbatim("Amal's phone".into())]
        );
        assert_eq!(post.actions.len(), 1);
        assert_eq!(post.actions[0].id, STOP);
        assert!(post.actions[0]
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'_'));
        assert_eq!(post.urgency, Urgency::High, "it must not go unnoticed");
    }
}
