//! The unlock screen renders in every language and phase; its texts come from the catalogs.
//! `ULTAR_UNLOCK_SHOTS=<dir>` writes the renders as PNG.

use ultar_i18n::Lang;
use ultar_ui::snapshot::Image;
use ultar_unlock::{render, view, Phase, Screen};

const KEYS: [&str; 8] = [
    "unlock.title",
    "unlock.hint",
    "unlock.field",
    "unlock.checking",
    "unlock.wrong",
    "unlock.caps",
    "unlock.done",
    "unlock.failed",
];

#[test]
fn every_text_is_in_every_catalog() {
    for lang in [Lang::Ar, Lang::En, Lang::De] {
        let tr = view::translator(lang);
        for key in KEYS {
            let found = tr.lookup(key).map(|(l, _)| l);
            assert_eq!(found, Some(lang), "{key} missing in {lang:?}");
        }
    }
}

#[test]
fn renders_every_phase() {
    let out = std::env::var_os("ULTAR_UNLOCK_SHOTS").map(std::path::PathBuf::from);
    let phases = [
        Phase::Typing,
        Phase::Checking,
        Phase::Wrong,
        Phase::Failed("I/O".into()),
        Phase::Done,
    ];
    for lang in [Lang::Ar, Lang::En, Lang::De] {
        for (i, phase) in phases.iter().enumerate() {
            let screen = Screen {
                lang,
                typed: 7,
                caps: i == 0,
                phase: phase.clone(),
            };
            let canvas = render(&screen, (1280, 800), 1.0).expect("render");
            let px = canvas.data();
            let first = &px[..4];
            assert!(
                px.chunks_exact(4).any(|p| p != first),
                "{lang:?} {phase:?}: something is drawn"
            );
            if let Some(dir) = &out {
                let png = Image::from_canvas(&canvas).encode_png().unwrap();
                std::fs::write(dir.join(format!("unlock-{lang:?}-{i}.png")), png).unwrap();
            }
        }
    }
}
