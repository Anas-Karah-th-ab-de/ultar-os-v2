use super::*;

fn engine() -> ReaderEngine {
    ReaderEngine::new(Lang::Ar, "UltarWeb-test").unwrap()
}

fn wait(e: &mut ReaderEngine) -> Vec<EngineEvent> {
    let mut all = Vec::new();
    for _ in 0..200 {
        let got = e.poll(Some(Duration::from_millis(100))).unwrap();
        let done = got
            .iter()
            .any(|ev| matches!(ev, EngineEvent::Progress { loading: false, .. }));
        all.extend(got);
        if done {
            break;
        }
    }
    all
}

#[test]
fn a_blank_page_navigates_and_draws() {
    let mut e = engine();
    let tab = e.create_tab(false).unwrap();
    e.resize(tab, 640, 400, 1.0).unwrap();
    e.load(tab, "about:blank").unwrap();
    let events = wait(&mut e);
    assert!(events
        .iter()
        .any(|ev| matches!(ev, EngineEvent::Navigated { url, .. } if url == "about:blank")));
    assert!(events
        .iter()
        .any(|ev| matches!(ev, EngineEvent::Frame { .. })));
    let f = e.frame(tab).unwrap();
    assert_eq!((f.width, f.height), (640, 400));
    assert_eq!(f.pixels.len(), 640 * 400 * 4);
}

#[test]
fn an_unreachable_page_shows_why_in_arabic() {
    let mut e = engine();
    let tab = e.create_tab(false).unwrap();
    e.resize(tab, 640, 400, 1.0).unwrap();
    e.load(tab, "http://127.0.0.1:9/").unwrap();
    let events = wait(&mut e);
    assert!(events
        .iter()
        .any(|ev| matches!(ev, EngineEvent::Title { title, .. } if title.contains("تعذّر"))));
    assert!(e.frame(tab).is_some());
}

#[test]
fn history_and_search_address() {
    assert_eq!(
        plain_search("https://duckduckgo.com/?q=%D8%B3"),
        "https://html.duckduckgo.com/html/?q=%D8%B3"
    );
    let mut e = engine();
    let tab = e.create_tab(false).unwrap();
    e.load(tab, "about:blank").unwrap();
    wait(&mut e);
    e.navigate(tab, "about:blank".into()).unwrap();
    let events = wait(&mut e);
    assert!(events
        .iter()
        .any(|ev| matches!(ev, EngineEvent::Navigated { can_back: true, .. })));
    e.back(tab).unwrap();
    let events = wait(&mut e);
    assert!(events.iter().any(|ev| matches!(
        ev,
        EngineEvent::Navigated {
            can_back: false,
            can_forward: true,
            ..
        }
    )));
}
