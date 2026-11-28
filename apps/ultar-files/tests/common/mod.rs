//! Helpers of the integration tests: a temporary area root, an app with fake ports.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use ultar_files::fs::{AreaId, Fs, Loc};
use ultar_files::service::Fake;
use ultar_files::{App, Mode, Ports};
use ultar_i18n::Lang;
use ultar_ui::event::{Action, ActionKind, Event, Key, Modifiers, PointerButton};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::Size;

static N: AtomicU32 = AtomicU32::new(0);

/// A fresh directory with the areas home, documents, downloads, media-pictures, media-music,
/// media-videos, desktop, trash and removable/USB.
pub struct Root(pub PathBuf);

impl Root {
    pub fn new() -> Root {
        let dir = std::env::temp_dir().join(format!(
            "ultar-files-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        for a in [
            "home",
            "documents",
            "downloads",
            "media-pictures",
            "media-music",
            "media-videos",
            "desktop",
            "trash",
            "removable/USB",
        ] {
            std::fs::create_dir_all(dir.join(a)).unwrap();
        }
        Root(dir)
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    pub fn write(&self, rel: &str, data: &[u8]) {
        let p = self.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }

    pub fn fs(&self) -> Fs {
        Fs::open(&self.0)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn loc(area: AreaId, parts: &[&str]) -> Loc {
    parts
        .iter()
        .fold(Loc::root(area), |l, p| l.child(p).unwrap())
}

pub fn exists(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

pub fn clock() -> i64 {
    1_790_000_000
}

pub fn ui(lang: Lang, theme: ThemeKind, size: Size) -> Ui {
    let mut ui = Ui::new(
        PaintEnv::new(lang, theme),
        InputMode::Pointer,
        size,
        ultar_files::i18n::translator(lang),
    )
    .unwrap();
    ui.set_direction(if lang == Lang::Ar {
        ultar_ui::Direction::Rtl
    } else {
        ultar_ui::Direction::Ltr
    });
    ui
}

/// The ports of the tests: shared fakes the test reads afterwards.
pub struct Shared(pub std::rc::Rc<std::cell::RefCell<Fake>>);

impl ultar_files::service::LauncherPort for Shared {
    fn resolve(&mut self, uri: &str) -> Result<Option<String>, ultar_files::service::PortError> {
        self.0.borrow_mut().resolve(uri)
    }
    fn open(
        &mut self,
        app: &str,
        uri: &str,
        t: Option<u64>,
    ) -> Result<(), ultar_files::service::PortError> {
        self.0.borrow_mut().open(app, uri, t)
    }
    fn start(&mut self, app: &str) -> Result<(), ultar_files::service::PortError> {
        self.0.borrow_mut().start(app)
    }
}

impl ultar_files::service::ForeignPort for Shared {
    fn inspect(&mut self, path: &str) {
        self.0.borrow_mut().inspect(path)
    }
    fn install(&mut self, path: &str, choices: ultar_bus::messages::foreign::Choices) {
        self.0.borrow_mut().install(path, choices)
    }
    fn poll(&mut self) -> Vec<ultar_files::service::ForeignEvent> {
        self.0.borrow_mut().poll()
    }
}

impl ultar_files::service::VolumesPort for Shared {
    fn eject(&mut self, id: &str) -> Result<bool, ultar_files::service::PortError> {
        self.0.borrow_mut().eject(id)
    }
    fn changed(&mut self) -> Option<Vec<ultar_files::service::Mounted>> {
        self.0.borrow_mut().changed()
    }
}

pub fn usb() -> ultar_files::service::Mounted {
    ultar_files::service::Mounted {
        id: "sdb1".into(),
        name: "USB".into(),
        uri: "file:///run/ultar/media/u/USB".into(),
    }
}

/// An app on `root` (window size of the board: 832 × 456 below the title bar).
pub fn app(
    root: &Root,
    mode: Mode,
    lang: Lang,
    uri: Option<&str>,
) -> (App, std::rc::Rc<std::cell::RefCell<Fake>>) {
    let fake = std::rc::Rc::new(std::cell::RefCell::new(Fake::default()));
    fake.borrow_mut().drive_states.push_back(vec![usb()]);
    let ports = Ports {
        launcher: Box::new(Shared(fake.clone())),
        volumes: Box::new(Shared(fake.clone())),
        foreign: Box::new(Shared(fake.clone())),
    };
    let size = if mode == Mode::Desk {
        Size::new(1440.0, 844.0)
    } else {
        Size::new(832.0, 456.0)
    };
    let ui = ui(lang, ThemeKind::Dark, size);
    let mut app = App::new(ui, root.fs(), Some(root.0.clone()), ports, mode, uri)
        .unwrap()
        .with_clock(clock);
    app.inline_work = true;
    app.reload();
    app.rebuild().unwrap();
    app.ui.layout();
    (app, fake)
}

pub fn node(app: &App, target: &ultar_files::action::Target) -> NodeId {
    app.built()
        .node(target)
        .unwrap_or_else(|| panic!("no node for {target:?}"))
}

/// Clicks the centre of `id` like a pointer and hands the actions to the app.
pub fn click(app: &mut App, id: NodeId) {
    click_with(app, id, Modifiers::default(), PointerButton::Primary)
}

pub fn click_with(app: &mut App, id: NodeId, m: Modifiers, button: PointerButton) {
    press(app, id, m, button, true)
}

static NOW: AtomicU32 = AtomicU32::new(0);

fn press(app: &mut App, id: NodeId, m: Modifiers, button: PointerButton, advance: bool) {
    if advance {
        let t = NOW.fetch_add(1000, Ordering::Relaxed) + 1000;
        app.ui.advance_to(u64::from(t) * 10);
    }
    app.ui.layout();
    let r = app.ui.rect(id).unwrap();
    let pos = ultar_ui::Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
    let mut actions: Vec<Action> = Vec::new();
    actions.extend(app.ui.handle(Event::Modifiers { modifiers: m }).actions);
    actions.extend(app.ui.handle(Event::PointerMove { pos }).actions);
    actions.extend(app.ui.handle(Event::PointerDown { pos, button }).actions);
    actions.extend(app.ui.handle(Event::PointerUp { pos, button }).actions);
    actions.extend(app.ui.take_actions());
    if std::env::var_os("FILES_DEBUG").is_some() {
        eprintln!(
            "click {id:?} at {pos:?} -> {actions:?} target {:?}",
            app.built().targets.get(&id)
        );
    }
    app.actions(actions, false).unwrap();
    app.ui.layout();
}

pub fn double_click(app: &mut App, id: NodeId) {
    click(app, id);
    press(app, id, Modifiers::default(), PointerButton::Primary, false);
}

/// A key press: the tree's actions, then the shortcut if the tree did not use it.
pub fn key(app: &mut App, key: Key, modifiers: Modifiers) {
    let ev = Event::Key { key, modifiers };
    if ultar_files::update::is_shortcut(&ev) {
        app.keys(vec![ev]).unwrap();
        app.ui.layout();
        return;
    }
    let r = app.ui.handle(ev);
    if std::env::var_os("FILES_DEBUG").is_some() {
        eprintln!("key {ev:?} -> {r:?}");
    }
    let mut actions = r.actions;
    actions.extend(app.ui.take_actions());
    app.actions(actions, true).unwrap();
    if !r.consumed {
        app.keys(vec![ev]).unwrap();
    }
    app.ui.layout();
}

pub fn act(app: &mut App, id: NodeId, kind: ActionKind) {
    app.actions(vec![Action { node: id, kind }], false).unwrap();
    app.ui.layout();
}

pub fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Modifiers::default()
    }
}
