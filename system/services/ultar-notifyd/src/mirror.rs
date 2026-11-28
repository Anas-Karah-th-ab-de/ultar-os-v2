//! Notifications between the person's devices (mesh item C2), without the bus.
//!
//! Outgoing: [`outgoing`] decides what of a posted notification may leave this device and in
//! which form. Only what a person wrote travels (verbatim title argument and verbatim body):
//! interface texts are keys of this device's catalogue and mean nothing elsewhere. A private
//! notification (password, one-time code), a system notice and a notification without any
//! human text never leave.
//!
//! Incoming: [`incoming`] turns a mirrored notification into a local post that names the
//! device it came from. A target that is locked (nobody signed in) shows nothing when the
//! source asked for that, and otherwise only that something arrived.

use ultar_bus::messages::mesh_calls::{NoteMirror, NoteRemote};
use ultar_bus::messages::notify::{Body, Category, Post, Urgency};
use ultar_bus::messages::notify_mirror::Privacy;
use ultar_bus::{Text, TextArg};

/// Longest title that is mirrored (as the mesh allows).
pub const MAX_TITLE: usize = 128;
/// Longest text that is mirrored (as the mesh allows).
pub const MAX_TEXT: usize = 1024;
/// Longest application id taken over from another device.
pub const MAX_APP: usize = 64;
/// Sender name of a mirrored notification (it is no bus identity, so no app can close or
/// answer it; only the shell can).
pub const SENDER: &str = "mesh";

fn text_of(key: &str, args: Vec<TextArg>) -> Text {
    Text {
        key: key.to_owned(),
        args,
    }
}

fn cut(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// What of `post` may be mirrored, or `None` when nothing may.
pub fn outgoing(post: &Post, privacy: Privacy) -> Option<NoteMirror> {
    if privacy == Privacy::Private || post.category == Category::System {
        return None;
    }
    let title = post
        .title
        .args
        .iter()
        .find_map(|a| match a {
            TextArg::Verbatim(s) if !s.is_empty() => Some(cut(s, MAX_TITLE)),
            _ => None,
        })
        .unwrap_or_default();
    let text = match &post.body {
        Body::Verbatim(s) => cut(s, MAX_TEXT),
        Body::Empty | Body::Localized(_) => String::new(),
    };
    if title.is_empty() && text.is_empty() {
        return None;
    }
    Some(NoteMirror {
        title,
        text,
        app: cut(&post.app_id, MAX_APP),
        urgency: post.urgency,
        confidential: false,
        hide_on_locked: privacy == Privacy::HideOnLocked,
    })
}

/// The local post for a notification that came from another device, or `None` when this
/// device is locked and the source wants it shown only on an unlocked device.
pub fn incoming(note: &NoteRemote, locked: bool) -> Option<Post> {
    if locked && note.hide_on_locked {
        return None;
    }
    let from = TextArg::Verbatim(cut(&note.from_name, MAX_TITLE));
    let (title, body) = if locked {
        (text_of("notify.mirror.locked", vec![from]), Body::Empty)
    } else {
        let what = if note.title.is_empty() {
            note.app.clone()
        } else {
            note.title.clone()
        };
        let body = if note.text.is_empty() {
            Body::Empty
        } else {
            Body::Verbatim(note.text.clone())
        };
        (
            text_of(
                "notify.mirror.title",
                vec![from, TextArg::Verbatim(cut(&what, MAX_TITLE))],
            ),
            body,
        )
    };
    Some(Post {
        // Another device must not wake this screen: at most the usual sound.
        urgency: note.urgency.min(Urgency::Normal),
        category: Category::App,
        app_id: format!("mesh.{}", cut(&note.app, MAX_APP)),
        title,
        body,
        actions: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::valid;

    fn post(category: Category, who: &str, body: Body) -> Post {
        Post {
            urgency: Urgency::High,
            category,
            app_id: "tech.code-craft.phone".into(),
            title: text_of("notify.sms.title", vec![TextArg::Verbatim(who.into())]),
            body,
            actions: Vec::new(),
        }
    }

    fn remote(hide: bool) -> NoteRemote {
        NoteRemote {
            from: "phone".into(),
            from_name: "Anas phone".into(),
            title: "Samer".into(),
            text: "hello".into(),
            app: "tech.code-craft.phone".into(),
            urgency: Urgency::High,
            hide_on_locked: hide,
        }
    }

    #[test]
    fn private_and_system_notices_never_leave() {
        let sms = post(Category::Sms, "Samer", Body::Verbatim("hi".into()));
        assert!(outgoing(&sms, Privacy::Private).is_none());
        let system = post(Category::System, "x", Body::Verbatim("battery".into()));
        assert!(outgoing(&system, Privacy::Normal).is_none());
        assert!(outgoing(&sms, Privacy::Normal).is_some());
    }

    #[test]
    fn only_what_a_person_wrote_travels() {
        let sms = post(Category::Sms, "Samer", Body::Verbatim("hi".into()));
        let m = outgoing(&sms, Privacy::Normal).unwrap();
        assert_eq!((m.title.as_str(), m.text.as_str()), ("Samer", "hi"));
        assert!(!m.hide_on_locked && !m.confidential);
        assert!(outgoing(&sms, Privacy::HideOnLocked).unwrap().hide_on_locked);
        // Interface texts only: nothing to carry.
        let keys = Post {
            title: Text::key("notify.update.ready"),
            body: Body::Localized(Text::key("notify.update.body")),
            ..post(Category::App, "", Body::Empty)
        };
        assert!(outgoing(&keys, Privacy::Normal).is_none());
    }

    #[test]
    fn long_texts_are_cut_on_a_character_boundary() {
        let long = "\u{0628}".repeat(MAX_TEXT);
        let m = outgoing(
            &post(Category::Message, "A", Body::Verbatim(long)),
            Privacy::Normal,
        )
        .unwrap();
        assert!(m.text.len() <= MAX_TEXT && m.text.len() > MAX_TEXT - 2);
        assert!(m.text.chars().all(|c| c == '\u{0628}'));
    }

    #[test]
    fn a_mirrored_note_names_its_device_and_is_a_valid_post() {
        let p = incoming(&remote(false), false).unwrap();
        assert!(valid(&p));
        assert_eq!(p.title.key, "notify.mirror.title");
        assert_eq!(
            p.title.args,
            vec![
                TextArg::Verbatim("Anas phone".into()),
                TextArg::Verbatim("Samer".into())
            ]
        );
        assert_eq!(p.body, Body::Verbatim("hello".into()));
        assert_eq!(p.urgency, Urgency::Normal, "another device cannot alert");
        assert!(p.actions.is_empty());
    }

    #[test]
    fn a_locked_device_shows_no_content_or_nothing() {
        assert!(incoming(&remote(true), true).is_none());
        let p = incoming(&remote(false), true).unwrap();
        assert!(valid(&p));
        assert_eq!(p.title.key, "notify.mirror.locked");
        assert_eq!(p.body, Body::Empty);
        assert!(!format!("{p:?}").contains("hello") && !format!("{p:?}").contains("Samer"));
        assert!(incoming(&remote(true), false).is_some());
    }
}
