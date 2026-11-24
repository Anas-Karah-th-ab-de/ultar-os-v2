//! Shared test helpers: the shell in the board's state (22 Nov 2026 16:52, files and
//! terminal running), a harness that clicks targets of the bar and its popup exactly like
//! the compositor would, and rendering.
#![allow(dead_code)]

use std::sync::Arc;
use ultar_desktop::model::apps::{AppInfo, AppName, Category};
use ultar_desktop::model::quick::Presence;
use ultar_desktop::model::state::Layouts;
use ultar_desktop::model::time::Clock;
use ultar_desktop::model::Desktop;
use ultar_desktop::shell::{build_bar, build_popup, Effect, Outcome, Popup, Shell};
use ultar_desktop::view::{Look, Target, View};
use ultar_i18n::Lang;
use ultar_surface::proto::{OutputId, OutputInfo, ShellWindow, WindowState};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{Action, ActionKind, Event, Key, Modifiers, PointerButton};
use ultar_ui::geom::{Point, Size};
use ultar_ui::tokens::ThemeKind;

/// 22 November 2026, 16:52 UTC – the moment of the boards.
pub const BOARD_MS: u64 = 1_795_366_320_000;
/// The boards' size.
pub const SCREEN: Size = Size::new(1440.0, 900.0);

pub struct FixedClock(pub std::sync::atomic::AtomicU64);
impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

pub fn window(id: u32, app: &str, title: &str, focused: bool) -> ShellWindow {
    ShellWindow {
        id,
        app_id: app.into(),
        title: title.into(),
        state: WindowState::Normal,
        focused,
        unresponsive: false,
        output: OutputId(0),
    }
}

pub fn output(id: u32, name: &str, primary: bool) -> OutputInfo {
    OutputInfo {
        id: OutputId(id),
        x: 0,
        y: 0,
        width: 1440,
        height: 900,
        scale_milli: 1000,
        refresh_millihertz: 60_000,
        primary,
        name: name.into(),
    }
}

/// The state of board Main: files focused, terminal running, Arabic and English layouts,
/// the settings service and the launcher answer.
pub fn board_desktop(lang: Lang, theme: ThemeKind) -> Desktop {
    let mut d = Desktop::new(lang);
    d.theme = theme;
    d.values.dark = theme == ThemeKind::Dark;
    d.windows = vec![
        window(1, "org.ultar.terminal", "الطرفية", false),
        window(2, "org.ultar.files", "الملفات – المستندات", true),
    ];
    d.layouts = Layouts {
        names: vec!["ar".into(), "en".into()],
        active: if lang == Lang::Ar { 0 } else { 1 },
    };
    d.presence = Presence {
        settings: true,
        launcher: true,
        ..Presence::default()
    };
    // The installed apps of the AppGrid board: the base system plus Firefox from the Store.
    for id in ["org.ultar.assistant", "org.ultar.phone"] {
        d.catalog.push(ultar_desktop::model::apps::installed(id));
    }
    d.catalog.push(AppInfo {
        id: "org.mozilla.firefox".into(),
        name: AppName::Data("Firefox".into()),
        icon: ultar_ui::icon::IconId::Firefox,
        category: Some(Category::Internet),
        base: false,
    });
    d.outputs = vec![output(0, "eDP-1", true), output(1, "HDMI-A-1", false)];
    d.main_display = Some("edp-1".into());
    d.values.brightness = Some(78);
    d
}

/// The notifications of board Toasts (oldest first), shown as banners at `BOARD_MS`.
pub fn board_notes(d: &mut Desktop, lang: Lang) {
    use ultar_bus::messages::notify::{Body, Category as Kind, Post, Posted, Urgency};
    use ultar_bus::{Text, TextArg};
    let i = usize::from(lang != Lang::Ar);
    let notes = [
        (
            "org.ultar.files",
            Urgency::Normal,
            ["تم توصيل ذاكرة USB", "USB drive connected"],
            ["جاهزة للاستخدام.", "Ready to use."],
        ),
        (
            "org.ultar.phone",
            Urgency::Normal,
            ["مكالمة فائتة من ليلى حداد", "Missed call from Layla Haddad"],
            ["عبر رقم Ultar", "via Ultar number"],
        ),
        (
            "org.ultar.power",
            Urgency::High,
            ["البطارية منخفضة: 9%", "Battery low: 9%"],
            [
                "صِل الشاحن، أو شغّل وضع التوفير ليدوم الجهاز أكثر.",
                "Plug in the charger or turn on the saver mode to last longer.",
            ],
        ),
    ];
    for (n, (app, urgency, title, body)) in notes.into_iter().enumerate() {
        let id = 100 + n as u64;
        let title = Text::key("desk.note.verbatim").with(TextArg::Verbatim(title[i].into()));
        let body = Body::Verbatim(body[i].into());
        let post = Post {
            urgency,
            category: Kind::App,
            app_id: app.into(),
            title,
            body,
        };
        d.notes.add(Posted {
            id,
            timestamp_ms: BOARD_MS - 10_000,
            post,
        });
        d.notes.show_banner(id, BOARD_MS);
    }
}

pub struct Harness {
    pub shell: Shell,
    pub look: Look,
    pub bar: View,
    pub popup: Option<Popup>,
    pub effects: Vec<Effect>,
    pub clock: Arc<FixedClock>,
}

impl Harness {
    pub fn new(lang: Lang, theme: ThemeKind) -> Harness {
        Harness::with(board_desktop(lang, theme), lang, theme)
    }

    pub fn with(d: Desktop, lang: Lang, theme: ThemeKind) -> Harness {
        let clock = Arc::new(FixedClock(std::sync::atomic::AtomicU64::new(BOARD_MS)));
        let shell = Shell::new(d, clock.clone(), "أنس");
        let look = Look::new(lang, theme);
        let bar = build_bar(&shell, look, SCREEN.w).unwrap();
        let mut h = Harness {
            shell,
            look,
            bar,
            popup: None,
            effects: Vec::new(),
            clock,
        };
        h.layout();
        h
    }

    fn layout(&mut self) {
        let _ = self.bar.ui.content_size();
        if let Some(p) = &mut self.popup {
            let _ = p.view.ui.content_size();
        }
    }

    pub fn apply(&mut self, out: Outcome) {
        self.effects.extend(out.effects.clone());
        if out.bar {
            self.bar = build_bar(&self.shell, self.look, SCREEN.w).unwrap();
        }
        if out.popup {
            self.popup = build_popup(&self.shell, self.look, SCREEN).unwrap();
        }
        self.layout();
    }

    pub fn take(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    fn press(view: &mut View, target: &Target, button: PointerButton) -> Vec<Action> {
        let node = view
            .node(target)
            .unwrap_or_else(|| panic!("no node for {target:?}"));
        let r = view
            .ui
            .rect(node)
            .unwrap_or_else(|| panic!("{target:?} not laid out"));
        let pos = Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
        view.ui.handle(Event::PointerMove { pos });
        let mut a = view.ui.handle(Event::PointerDown { pos, button }).actions;
        a.extend(view.ui.handle(Event::PointerUp { pos, button }).actions);
        a
    }

    fn dispatch_bar(&mut self, actions: Vec<Action>) {
        for a in actions {
            let t = self.bar.target(a.node).cloned();
            if let Some(t) = t {
                let out = self.shell.on_bar(&t, &a.kind);
                self.apply(out);
            }
        }
    }

    pub fn dispatch_popup(&mut self, actions: Vec<Action>) {
        for a in actions {
            let Some(p) = &self.popup else { return };
            let t = p.view.target(a.node).cloned();
            let out = match (t, &a.kind) {
                (Some(t @ Target::AppItem(_)), k @ ActionKind::MenuChosen(_)) => {
                    self.shell.on_popup(&t, k)
                }
                (_, ActionKind::MenuChosen(id)) => self.shell.on_menu(*id),
                (Some(Target::Menu), ActionKind::Closed) => self.shell.close(),
                (Some(Target::SearchField), ActionKind::TextChanged) => {
                    let text = p.view.ui.text(a.node).unwrap().to_owned();
                    self.shell.on_query(&text)
                }
                (Some(t), k) => self.shell.on_popup(&t, k),
                (None, _) => continue,
            };
            self.apply(out);
        }
    }

    pub fn click_bar(&mut self, target: Target) {
        let a = Harness::press(&mut self.bar, &target, PointerButton::Primary);
        self.dispatch_bar(a);
    }

    pub fn middle_bar(&mut self, target: Target) {
        let a = Harness::press(&mut self.bar, &target, PointerButton::Middle);
        self.dispatch_bar(a);
    }

    pub fn right_bar(&mut self, target: Target) {
        let out = self.shell.on_bar_menu(&target);
        self.apply(out);
    }

    pub fn click_popup(&mut self, target: Target) {
        let p = self.popup.as_mut().expect("a popup is open");
        let a = Harness::press(&mut p.view, &target, PointerButton::Primary);
        self.dispatch_popup(a);
    }

    /// A secondary click on a popup node (the toolkit opens its context menu).
    pub fn right_popup(&mut self, target: Target) {
        let p = self.popup.as_mut().expect("a popup is open");
        let a = Harness::press(&mut p.view, &target, PointerButton::Secondary);
        self.dispatch_popup(a);
    }

    pub fn key_popup(&mut self, key: Key) {
        let p = self.popup.as_mut().expect("a popup is open");
        let r = p.view.ui.handle(Event::Key {
            key,
            modifiers: Modifiers::default(),
        });
        let consumed = r.consumed;
        self.dispatch_popup(r.actions);
        if !consumed {
            let out = self.shell.on_popup_key(&key);
            self.apply(out);
        }
    }

    pub fn type_popup(&mut self, text: &str) {
        for ch in text.chars() {
            self.key_popup(Key::Char(ch));
        }
    }

    pub fn popup_target(&self, target: &Target) -> bool {
        self.popup
            .as_ref()
            .is_some_and(|p| p.view.node(target).is_some())
    }
}

/// Paints a view into a canvas (translucent views on RGBA over black like the compositor
/// composes them over the ground colour – the snapshot shows the card and its shadow).
pub fn render(view: &mut View, size: Size) -> Canvas {
    let mut c = Canvas::new(
        size.w.round() as u32,
        size.h.round() as u32,
        PixelFormat::Rgba8,
    )
    .unwrap();
    view.ui.paint_all(&mut c);
    c
}
