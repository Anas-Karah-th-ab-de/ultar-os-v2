//! Every state of the window painted in Arabic and English, dark and light, and the catalogs.
mod common;

use common::*;
use std::time::Instant;
use ultar_bus::messages::mesh_view::{EndReason, ViewEvent};
use ultar_i18n::Lang;
use ultar_remote::app::RemoteApp;
use ultar_remote::port::PortEvent;
use ultar_ui::tokens::ThemeKind;

/// Every state in the four variants: it paints, every operable node and the picture have a
/// name, and no text is left as a catalog key.
#[test]
fn every_state_in_arabic_and_english_dark_and_light() {
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            let now = Instant::now();
            let mut states: Vec<(&str, RemoteApp)> = Vec::new();
            let (empty, _) = app_in(lang, theme);
            states.push(("none", empty));
            let (mut choose_app, port) = app_in(lang, theme);
            port.tell(PortEvent::Peers(vec![peer("a1", "Laptop", true)]));
            choose_app.poll(now).unwrap();
            states.push(("choose", choose_app));
            let (mut waiting, port) = app_in(lang, theme);
            port.tell(PortEvent::Peers(vec![peer("a1", "Laptop", true)]));
            waiting.poll(now).unwrap();
            click(&mut waiting, "remote.connect", now);
            states.push(("waiting", waiting));
            let (mut run, port) = app_in(lang, theme);
            running(&mut run, &port, now);
            let piece: Vec<u8> = (0..64 * 64)
                .flat_map(|i| [(i % 251) as u8, 120, 200, 0])
                .collect();
            port.tell(PortEvent::View(ViewEvent::Picture {
                x: 0,
                y: 0,
                w: 64,
                h: 64,
                data: piece,
            }));
            run.poll(now).unwrap();
            states.push(("running", run));
            for (name, reason) in [
                ("refused", EndReason::Refused),
                ("stopped", EndReason::Stopped),
                ("locked", EndReason::Locked),
                ("lost", EndReason::Lost),
            ] {
                let (mut a, port) = app_in(lang, theme);
                running(&mut a, &port, now);
                port.tell(PortEvent::View(ViewEvent::Ended { reason }));
                a.poll(now).unwrap();
                states.push((name, a));
            }
            for (name, mut a) in states {
                check_state(&mut a, &format!("{name}-{lang:?}-{}", theme.name()));
            }
        }
    }
}

#[test]
fn the_three_catalogs_have_the_same_keys() {
    fn keys(lang: Lang) -> Vec<String> {
        let mut k: Vec<String> = ultar_remote::i18n::source(lang)
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once(" = ").map(|(k, _)| k.to_owned()))
            .collect();
        k.sort();
        k
    }
    let ar = keys(Lang::Ar);
    assert!(ar.len() > 30);
    assert_eq!(ar, keys(Lang::En));
    assert_eq!(ar, keys(Lang::De));
    // the manifest's name and description are in the catalog
    assert!(
        ar.contains(&"app.remote.name".to_owned()) && ar.contains(&"app.remote.desc".to_owned())
    );
}

#[test]
fn the_end_texts_name_the_device() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    running(&mut app, &port, now);
    port.tell(PortEvent::View(ViewEvent::Ended {
        reason: EndReason::Locked,
    }));
    app.poll(now).unwrap();
    let tr = ultar_remote::i18n::translator(Lang::En);
    let text = tr.t_args(
        "remote.locked.message",
        &[("name", ultar_i18n::Arg::Str("Laptop"))],
    );
    assert!(text.contains("Laptop") && text.contains("locked"), "{text}");
}
