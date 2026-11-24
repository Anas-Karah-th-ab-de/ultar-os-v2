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
