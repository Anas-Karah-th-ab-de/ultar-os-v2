//! Round trips and limits of the launcher contract.

use super::*;
use crate::message::{decode_versioned, encode};

fn round<T: Message + Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(
    value: &T,
) {
    let back: T = decode_versioned(T::VERSION, &encode(value).unwrap()).unwrap();
    assert_eq!(&back, value);
}

#[test]
fn messages_round_trip_and_words_are_distinct() {
    round(&ListReply {
        apps: vec![AppStatus {
            app: "camera".into(),
            uid: 20001,
            profile: "app.system".into(),
            version: "1.0.0".into(),
            instances: vec![InstanceStatus {
                role: Role::Service,
                state: InstanceState::Backoff,
                pid: 0,
                restarts: 2,
                last_exit: Some(Exit::Setup(121)),
            }],
        }],
    });
    round(&Start {
        app: "camera".into(),
        role: Role::Ui,
        activation_token: Some(u64::MAX),
        uri: Some("trash:///".into()),
        new_window: true,
    });
    round(&StartV1 {
        app: "camera".into(),
        role: Role::Ui,
        activation_token: None,
    });
    assert_eq!(<Start as Message>::VERSION, 2);
    round(&PermissionsReply {
        declared: vec![PermissionState {
            name: "network".into(),
            granted: false,
        }],
        bus: vec!["notify.post".into()],
        files: vec!["media:pictures:rw".into()],
    });
    round(&Uninstall {
        app: "x".into(),
        keep_data: true,
    });
    let states = [
        InstanceState::Starting,
        InstanceState::Running,
        InstanceState::Stopping,
        InstanceState::Backoff,
        InstanceState::Stopped,
        InstanceState::Crashed,
        InstanceState::Failed,
    ];
    let words: std::collections::BTreeSet<&str> = states.iter().map(|s| s.word()).collect();
    assert_eq!(words.len(), states.len());
    assert_eq!(states.iter().filter(|s| s.is_active()).count(), 3);
    assert_ne!(Role::Ui.word(), Role::Service.word());
    assert_eq!(<Install as Call>::METHOD, "install");
    assert_eq!(<Revoke as Call>::SERVICE, SERVICE);
}

#[test]
fn default_app_calls_round_trip() {
    round(&Resolve {
        uri: "https://example.org/a".into(),
    });
    round(&ResolveReply {
        app: Some("browser".into()),
    });
    round(&ResolveReply { app: None });
    round(&Candidates { kind: Kind::Photos });
    round(&CandidatesReply {
        apps: vec!["images".into(), "acme.viewer".into()],
    });
    assert_eq!(<Resolve as Call>::METHOD, "resolve");
    assert_eq!(<Candidates as Call>::METHOD, "candidates");
    assert_eq!(<Resolve as Call>::SERVICE, SERVICE);
}

#[test]
fn kinds_have_distinct_words_keys_and_defaults() {
    let words: std::collections::BTreeSet<&str> = Kind::ALL.iter().map(|k| k.word()).collect();
    assert_eq!(words.len(), Kind::ALL.len());
    for kind in Kind::ALL {
        assert_eq!(Kind::from_word(kind.word()), Some(kind));
        assert_eq!(kind.key(), format!("apps.default.{}", kind.word()));
        assert!(!kind.default_app().is_empty());
        assert!(!kind.schemes().is_empty());
    }
    assert_eq!(Kind::from_word("nope"), None);
    assert_eq!(Kind::Web.key(), "apps.default.web");
    assert_eq!(Kind::Music.default_app(), Kind::Video.default_app());
    assert!(Kind::Web.schemes().contains(&"https"));
    assert!(Kind::Documents.schemes().contains(&"file"));
}

#[test]
fn uris_name_their_kind() {
    for (uri, kind) in [
        ("https://example.org/a", Kind::Web),
        ("mailto:a@example.org", Kind::Mail),
        ("calendar://event/1", Kind::Calendar),
        ("file:///m/u/song.mp3", Kind::Music),
        ("file:///m/u/film.mkv", Kind::Video),
        ("file:///m/u/pic.JPG", Kind::Photos),
        ("file:///m/u/doc.pdf", Kind::Documents),
    ] {
        assert_eq!(kind_of_uri(uri), Some(kind), "{uri}");
    }
    for uri in ["trash:///", "file:///m/u/folder", "file:///m/u/a.zip", ""] {
        assert_eq!(kind_of_uri(uri), None, "{uri}");
    }
}
