//! Helpers of the browser tests: a browser over the test engine at the board's window size.
#![allow(dead_code)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use ultar_ui::event::{Event, Key, Modifiers, PointerButton};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::{Lang, Point, Size};
use ultar_web::app::{Browser, Handoff, HandoffError, Paths};
use ultar_web::engine::test_engine::TestEngine;
use ultar_web::engine::KeyInput;

/// Client area of the board's browser window (992 × 624 minus border and title bar).
pub const CLIENT: Size = Size { w: 990.0, h: 582.0 };

/// Records files handed to the Files app; `missing` answers like an absent app.
#[derive(Clone, Default)]
pub struct Files {
    pub shown: Rc<RefCell<Vec<PathBuf>>>,
    pub missing: bool,
}

impl Handoff for Files {
    fn show_in_folder(&mut self, file: &Path) -> Result<(), HandoffError> {
        if self.missing {
            return Err(HandoffError::Missing);
        }
        self.shown.borrow_mut().push(file.to_path_buf());
        Ok(())
    }
}

pub fn ui(lang: Lang, theme: ThemeKind) -> Ui {
    let tr = ultar_web::catalog::translator(lang).unwrap();
    let mut ui = Ui::new(PaintEnv::new(lang, theme), InputMode::Pointer, CLIENT, tr).unwrap();
    ui.set_direction(if lang == Lang::Ar {
        ultar_ui::Direction::Rtl
    } else {
        ultar_ui::Direction::Ltr
    });
    ui
}

pub fn browser(lang: Lang, theme: ThemeKind) -> Browser<TestEngine> {
    Browser::new(
        TestEngine::new(),
        ui(lang, theme),
        None,
        Box::new(Files::default()),
    )
    .unwrap()
}

/// A browser with a profile in a fresh temporary folder.
pub fn browser_with_profile(name: &str, files: Files) -> (Browser<TestEngine>, Paths) {
    let base = std::env::temp_dir().join(format!("ultar-web-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let paths = Paths {
        profile: base.join("profile"),
        downloads: base.join("Downloads"),
    };
    std::fs::create_dir_all(&paths.downloads).unwrap();
    let b = Browser::new(
        TestEngine::new(),
        ui(Lang::En, ThemeKind::Light),
        Some(paths.clone()),
        Box::new(files),
    )
    .unwrap();
    (b, paths)
}

pub fn center(b: &mut Browser<TestEngine>, node: NodeId) -> Point {
    b.ui_mut().layout();
    b.ui().rect(node).expect("node is shown").center()
}

pub fn click_at(b: &mut Browser<TestEngine>, pos: Point, button: PointerButton) {
    b.pointer(Event::PointerMove { pos }).unwrap();
    b.pointer(Event::PointerDown { pos, button }).unwrap();
    b.pointer(Event::PointerUp { pos, button }).unwrap();
    b.pump(None).unwrap();
}

pub fn click(b: &mut Browser<TestEngine>, node: NodeId) {
    let pos = center(b, node);
    click_at(b, pos, PointerButton::Primary);
}

/// Types text into the focused field and presses Enter.
pub fn type_enter(b: &mut Browser<TestEngine>, text: &str) {
    for c in text.chars() {
        b.key(
            KeyInput {
                code: 0,
                character: c as u32,
                modifiers: 0,
                pressed: true,
            },
            Some(Event::Key {
                key: Key::Char(c),
                modifiers: Modifiers::NONE,
            }),
        )
        .unwrap();
    }
    b.key(
        KeyInput {
            code: 28,
            character: 13,
            modifiers: 0,
            pressed: true,
        },
        Some(Event::Key {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        }),
    )
    .unwrap();
    b.pump(None).unwrap();
}

/// A shortcut by Linux key code and modifier bits.
pub fn shortcut(b: &mut Browser<TestEngine>, code: u16, modifiers: u8) {
    b.key(
        KeyInput {
            code,
            character: 0,
            modifiers,
            pressed: true,
        },
        None,
    )
    .unwrap();
    b.pump(None).unwrap();
}

/// Opens `url` through the address field.
pub fn open(b: &mut Browser<TestEngine>, url: &str) {
    let field = b.chrome().address;
    b.ui_mut().focus(field).unwrap();
    b.ui_mut().set_text(field, "").unwrap();
    type_enter(b, url);
}
