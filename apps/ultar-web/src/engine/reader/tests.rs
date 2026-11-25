use super::*;

fn engine() -> ReaderEngine {
    let mut e = ReaderEngine::new(Lang::Ar, &fetch::Route::Direct).unwrap();
    // the tests' own server is on this machine; the browser never reaches it
    e.client.allow_local = true;
    e
}

#[test]
fn local_addresses_are_refused_without_a_request() {
    let mut e = ReaderEngine::new(Lang::Ar, &fetch::Route::Direct).unwrap();
    let tab = e.create_tab(false).unwrap();
    e.resize(tab, 640, 400, 1.0).unwrap();
    e.load(tab, "http://127.0.0.1:9/").unwrap();
    let events = wait(&mut e);
    assert!(events
        .iter()
        .any(|ev| matches!(ev, EngineEvent::Title { title, .. } if title.contains("تعذّر"))));
    let doc = e.tabs[&tab].doc.as_ref().unwrap();
    assert!(
        doc.blocks.iter().any(|b| b.text.contains("حماية")),
        "{:?}",
        doc.blocks
    );
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

/// A tiny HTTP server on 127.0.0.1 serving `files` (path, type, body); returns its address.
fn serve(files: Vec<(&'static str, &'static str, Vec<u8>)>) -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let path = first.split_whitespace().nth(1).unwrap_or("/").to_owned();
            let mut stream = stream;
            match files.iter().find(|f| f.0 == path) {
                Some((_, mime, body)) => {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(body);
                }
                None => {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
            }
        }
    });
    addr
}

fn frames_until(e: &mut ReaderEngine, done: impl Fn(&ReaderEngine) -> bool) -> usize {
    let mut frames = 0;
    for _ in 0..400 {
        let got = e.poll(Some(Duration::from_millis(25))).unwrap();
        frames += got
            .iter()
            .filter(|ev| matches!(ev, EngineEvent::Frame { .. }))
            .count();
        if done(e) {
            break;
        }
    }
    frames
}

fn playing(e: &ReaderEngine, tab: TabId, want: crate::media::player::Status) -> bool {
    e.tabs[&tab].media.players_status().contains(&want)
}

#[test]
fn a_video_in_a_page_plays_on_click() {
    use crate::media::player::Status;
    let mp4 = crate::media::video::tests::mp4_file(12);
    let html = b"<h1>\xd9\x81\xd9\x8a\xd8\xaf\xd9\x8a\xd9\x88</h1><video src='/clip.mp4'></video>"
        .to_vec();
    let base = serve(vec![
        ("/", "text/html; charset=utf-8", html),
        ("/clip.mp4", "video/mp4", mp4),
    ]);
    let mut e = engine();
    let tab = e.create_tab(false).unwrap();
    e.resize(tab, 800, 600, 1.0).unwrap();
    e.load(tab, &format!("{base}/")).unwrap();
    wait(&mut e);
    let boxes = page::media_boxes(e.tabs[&tab].laid.as_ref().unwrap());
    assert_eq!(boxes.len(), 1);
    assert_eq!(boxes[0].src, format!("{base}/clip.mp4"));
    let (x, y, w, h) = boxes[0].rect;
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    e.pointer(
        tab,
        PointerInput::Down {
            x: cx,
            y: cy,
            button: 1,
        },
    )
    .unwrap();
    e.pointer(
        tab,
        PointerInput::Up {
            x: cx,
            y: cy,
            button: 1,
        },
    )
    .unwrap();
    let frames = frames_until(&mut e, |e| playing(e, tab, Status::Ended));
    assert!(
        playing(&e, tab, Status::Ended),
        "the video plays to its end"
    );
    assert!(frames >= 5, "pictures reach the page: {frames}");
    // the picture is in the frame: the middle of the box is not the dark ground
    let f = e.frame(tab).unwrap();
    let at = (cy as usize * f.stride) + cx as usize * 4;
    assert_ne!(&f.pixels[at..at + 3], &[20, 16, 16]);
}

#[test]
fn a_sound_file_address_plays_at_once() {
    use crate::media::player::Status;
    let wav = crate::media::audio::tests::wav(16_000, 0.2);
    let base = serve(vec![("/tone.wav", "audio/wav", wav)]);
    let mut e = engine();
    let tab = e.create_tab(false).unwrap();
    e.resize(tab, 640, 400, 1.0).unwrap();
    e.load(tab, &format!("{base}/tone.wav")).unwrap();
    wait(&mut e);
    frames_until(&mut e, |e| playing(e, tab, Status::Ended));
    assert!(playing(&e, tab, Status::Ended));
}
