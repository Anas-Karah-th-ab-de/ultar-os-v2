//! Helpers of the reader's tests: a places directory with fixture PDFs, a reader on a headless
//! `Ui`, pumping the event loop until the worker has answered, and a generator for long PDFs.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use ultar_i18n::Lang;
use ultar_pdf::app::App;
use ultar_ui::event::{Event, Key, Modifiers};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Size;

/// The screens of the three layout classes (logical pixels).
pub const COMPACT: (f32, f32) = (400.0, 760.0);
pub const MEDIUM: (f32, f32) = (800.0, 600.0);
pub const WIDE: (f32, f32) = (1280.0, 800.0);

pub const DOC: &str = "files://documents/mixed.pdf";

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A fresh places directory with `documents/` (the files are copied from the fixtures).
pub fn areas(test: &str, fixtures: &[&str]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ultar-pdf-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("documents")).expect("create areas");
    for name in fixtures {
        std::fs::copy(fixture(name), dir.join("documents").join(name)).expect("copy fixture");
    }
    dir
}

pub fn ui(lang: Lang, theme: ThemeKind, size: (f32, f32)) -> Ui {
    Ui::new(
        PaintEnv::new(lang, theme),
        InputMode::Pointer,
        Size::new(size.0, size.1),
        ultar_pdf::i18n::translator(lang),
    )
    .expect("ui")
}

/// A reader for `uri` (a `files://documents/…` address), not yet pumped.
pub fn reader(
    uri: Option<&str>,
    areas: &Path,
    lang: Lang,
    theme: ThemeKind,
    size: (f32, f32),
) -> App {
    let doc = App::open(uri, areas);
    let data = areas.join("data");
    std::fs::create_dir_all(&data).expect("data dir");
    App::new(ui(lang, theme, size), doc, areas.to_path_buf(), data).expect("app")
}

/// Pumps the loop (`poll`) until `done` holds; `false` after `limit`.
pub fn pump_until(app: &mut App, limit: Duration, done: impl Fn(&App) -> bool) -> bool {
    let start = Instant::now();
    loop {
        app.poll().expect("poll");
        if done(app) {
            return true;
        }
        if start.elapsed() > limit {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Pumps until the document is open and the first page picture is there.
pub fn open_and_show(app: &mut App) {
    let ok = pump_until(app, Duration::from_secs(30), |a| {
        a.doc.state == ultar_pdf::doc::DocState::Open && a.doc.stale_image(0).is_some()
    });
    assert!(ok, "the document did not open and show page 1");
}

/// Sends one event the way the window does: the widgets first, their actions to the reader,
/// and what the widgets did not use to the reader's keys, pointer and touch.
pub fn send(app: &mut App, event: Event) {
    if let Event::Key { key, modifiers } = event {
        // The window's shortcut filter hands these to the reader before the widgets see them.
        if ultar_pdf::keys::is_shortcut(key, modifiers, app.key_context()) {
            app.key(key, modifiers).expect("shortcut");
            return;
        }
    }
    let response = app.ui.handle(event);
    app.actions(response.actions).expect("actions");
    match event {
        Event::Key { key, modifiers } if !response.consumed => {
            app.key(key, modifiers).expect("key")
        }
        // The window queues every touch and every press, release and wheel turn for the app.
        Event::TouchDown { .. }
        | Event::TouchMove { .. }
        | Event::TouchUp { .. }
        | Event::TouchCancel { .. } => app.touch(event).expect("touch"),
        Event::PointerDown { .. }
        | Event::PointerMove { .. }
        | Event::PointerUp { .. }
        | Event::Scroll { .. } => app.pointer(event).expect("pointer"),
        _ => {}
    }
}

pub fn key(app: &mut App, key: Key, ctrl: bool, shift: bool) {
    let modifiers = Modifiers {
        ctrl,
        shift,
        ..Modifiers::default()
    };
    send(app, Event::Key { key, modifiers });
}

pub fn ctrl(app: &mut App, c: char) {
    key(app, Key::Char(c), true, false);
}

pub fn press(app: &mut App, k: Key) {
    key(app, k, false, false);
}

pub fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, if c == ' ' { Key::Space } else { Key::Char(c) });
    }
}

/// A PDF of `pages` pages (Helvetica text, a title and 40 lines each), no compression.
pub fn long_pdf(pages: usize) -> Vec<u8> {
    let mut objs: Vec<String> = Vec::new();
    objs.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 5 + 2 * i)).collect();
    objs.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {pages} >>",
        kids.join(" ")
    ));
    objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    for i in 0..pages {
        let mut text = format!("BT /F1 28 Tf 60 780 Td (Page {} of {pages}) Tj ET\n", i + 1);
        for l in 0..40 {
            let y = 740 - l * 17;
            text += &format!(
                "BT /F1 11 Tf 60 {y} Td (Line {l} of page {} lorem ipsum dolor sit amet) Tj ET\n",
                i + 1
            );
        }
        objs.push(format!(
            "<< /Length {} >>\nstream\n{text}endstream",
            text.len()
        ));
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> >> >>",
            4 + 2 * i
        ));
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
}
