//! Round trips, texts and rules of the foreign-apps contract.

use super::*;
use crate::message::{decode, encode};

fn round_trip<T: Message + PartialEq + std::fmt::Debug>(value: &T) {
    let bytes = encode(value).unwrap();
    assert_eq!(&decode::<T>(&bytes).unwrap(), value);
}

#[test]
fn messages_round_trip() {
    let choices = Choices {
        network: NetworkChoice::Internet,
        lan: false,
        folders: vec!["documents".into()],
    };
    round_trip(&Install {
        from: InstallFrom::File("/data/ultar/areas/downloads/a.deb".into()),
        choices: choices.clone(),
    });
    round_trip(&InspectReply {
        outcome: InspectOutcome::Found(Inspected {
            source: Source::Deb,
            name: "mousepad".into(),
            version: "0.6.2-1".into(),
            publisher: "Xubuntu Developers".into(),
            size_bytes: 9000,
            verified: false,
            installable: true,
            suggested: Choices::default(),
            signer: String::new(),
            refusal: None,
            not_granted: Vec::new(),
        }),
    });
    round_trip(&Progress {
        job: 7,
        step: Step::Failed(Failure::Refused),
        percent: 40,
        apps: Vec::new(),
    });
    round_trip(&ListReply {
        apps: vec![ForeignApp {
            id: "foreign.deb-mousepad".into(),
            name: "Mousepad".into(),
            names: vec![("ar".into(), "ماوس باد".into())],
            source: Source::Deb,
            package: "mousepad".into(),
            version: "0.6.2-1".into(),
            icon: "/data/ultar/foreign/icons/foreign.deb-mousepad.png".into(),
            choices,
        }],
    });
    round_trip(&InstallReply {
        outcome: InstallOutcome::Started(3),
    });
}

#[test]
fn an_android_inspection_round_trips() {
    round_trip(&InspectReply {
        outcome: InspectOutcome::Found(Inspected {
            source: Source::Apk,
            name: "Calculator".into(),
            version: "1.4.2".into(),
            publisher: String::new(),
            size_bytes: 3_000_000,
            verified: false,
            installable: false,
            suggested: Choices::default(),
            signer: format!("{} Jane Doe", "ab".repeat(32)),
            refusal: Some(Refusal::NeedsArm),
            not_granted: vec!["camera".into()],
        }),
    });
}

#[test]
fn texts_exist_in_every_language_with_the_same_keys() {
    let keys = |src: &str| -> Vec<String> {
        let mut k: Vec<String> = src
            .lines()
            .filter(|l| !l.starts_with('#') && l.contains(" = "))
            .map(|l| l.split(" = ").next().unwrap_or_default().to_owned())
            .collect();
        k.sort();
        k
    };
    let ar = keys(LOCALES[0].1);
    assert!(ar.contains(&"foreign.confirm.install".to_owned()));
    for s in Source::ALL {
        assert!(ar.contains(&format!("foreign.source.{}", s.word())));
    }
    for r in Refusal::ALL {
        assert!(
            ar.contains(&format!("foreign.refusal.{}", r.word())),
            "{r:?}"
        );
    }
    for p in ANDROID_NOT_GRANTED {
        assert!(ar.contains(&format!("foreign.android.{}", p.1)), "{p:?}");
    }
    for k in ["foreign.fail.signer_changed", "foreign.fail.older"] {
        assert!(ar.contains(&k.to_owned()));
    }
    for (_, src) in &LOCALES[1..] {
        assert_eq!(keys(src), ar);
    }
}

#[test]
fn sources_and_choices() {
    for s in Source::ALL {
        assert_eq!(Source::from_word(s.word()), Some(s));
    }
    for now in [Source::Deb, Source::Exe, Source::Msi, Source::Apk] {
        assert!(now.installable());
    }
    assert!(!Source::Unknown.installable());
    assert!(Choices::default().is_valid());
    let lan_without_net = Choices {
        lan: true,
        ..Choices::default()
    };
    assert!(!lan_without_net.is_valid());
    let twice = Choices {
        folders: vec!["documents".into(), "documents".into()],
        ..Choices::default()
    };
    assert!(!twice.is_valid());
    let unknown = Choices {
        folders: vec!["/etc".into()],
        ..Choices::default()
    };
    assert!(!unknown.is_valid());
}

#[test]
fn granting_more_is_noticed_and_taking_back_is_not() {
    let none = Choices::default();
    let net = Choices {
        network: NetworkChoice::Internet,
        ..Choices::default()
    };
    let lan = Choices {
        lan: true,
        ..net.clone()
    };
    let docs = Choices {
        folders: vec!["documents".into()],
        ..net.clone()
    };
    assert!(net.widens(&none) && lan.widens(&net) && docs.widens(&net));
    assert!(
        docs.widens(&lan),
        "a folder is more, even with less network"
    );
    for (after, before) in [(&none, &net), (&net, &lan), (&net, &docs), (&none, &none)] {
        assert!(!after.widens(before), "{after:?} after {before:?}");
    }
}
