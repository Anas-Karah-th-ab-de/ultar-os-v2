//! Shared test helpers: the shell in the board's state (22 Nov 2026 16:52, files and
//! terminal running), a harness that clicks targets of the bar and its popup exactly like
//! the compositor would, and rendering.
#![allow(dead_code)]

pub mod shots;

use std::sync::Arc;
use ultar_desktop::model::apps::{AppInfo, AppName, Category};
use ultar_desktop::model::quick::Presence;
use ultar_desktop::model::state::Layouts;
use ultar_desktop::model::time::Clock;
use ultar_desktop::model::{Desktop, Shape};
use ultar_desktop::shell::{
    build_bar, build_home, build_popup, build_second, Effect, Outcome, Popup, Shell,
};
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
        workspace: 0,
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
    d.svc.main_display = Some("edp-1".into());
    d.values.brightness = Some(78);
    // Board Overview: two workspaces, the first shown.
    d.overview.workspaces = Some((0, 2));
    d
}

/// The phone boards' size.
pub const PHONE: Size = Size::new(360.0, 800.0);

/// The state of the Compact boards: nothing running, battery 87 %, a modem, the launcher
/// and the settings service answer; the home screen and favourites of the board.
pub fn board_phone(lang: Lang, theme: ThemeKind) -> Desktop {
    let mut d = board_desktop(lang, theme);
    d.windows.clear();
    d.pinned = ultar_desktop::model::apps::phone_favourites();
    d.presence.modem = Some(true);
    d.values.battery = Some((87, false));
    d.values.night = true;
    d.outputs = vec![OutputInfo {
        width: 360,
        height: 800,
        ..output(0, "DSI-1", true)
    }];
    d
}

/// The one notification of board CompactQuick (a missed call at 16:20).
pub fn phone_note(d: &mut Desktop, lang: Lang) {
    use ultar_bus::messages::notify::{Body, Category as Kind, Post, Posted, Urgency};
    use ultar_bus::{Text, TextArg};
    let (title, body) = if lang == Lang::Ar {
        ("مكالمة فائتة من ليلى حداد", "عبر رقم Ultar")
    } else {
        ("Missed call from Layla Haddad", "via Ultar number")
    };
    d.notes.add(Posted {
        id: 7,
        timestamp_ms: BOARD_MS - 32 * 60_000,
        post: Post {
            urgency: Urgency::Normal,
            category: Kind::App,
            app_id: "org.ultar.phone".into(),
            title: Text::key("desk.note.verbatim").with(TextArg::Verbatim(title.into())),
            body: Body::Verbatim(body.into()),
            actions: Vec::new(),
        },
        sender: "app.org.ultar.phone".into(),
    });
}

/// Sender, urgency, title and body (ar, en), buttons (id, ar, en).
type BoardNote<'a> = (
    &'a str,
    ultar_bus::messages::notify::Urgency,
    [&'a str; 2],
    [&'a str; 2],
    &'a [(&'a str, [&'a str; 2])],
);

/// The notifications of board Toasts with their buttons (note.action).
fn board_note_data() -> [BoardNote<'static>; 3] {
    use ultar_bus::messages::notify::Urgency;
    [
        (
            "org.ultar.files",
            Urgency::Normal,
            ["تم توصيل ذاكرة USB", "USB drive connected"],
            ["جاهزة للاستخدام.", "Ready to use."],
            &[("open", ["فتح", "Open"]), ("eject", ["إخراج", "Eject"])],
        ),
        (
            "org.ultar.phone",
            Urgency::Normal,
            ["مكالمة فائتة من ليلى حداد", "Missed call from Layla Haddad"],
            ["عبر رقم Ultar", "via Ultar number"],
            &[
                ("call_back", ["معاودة الاتصال", "Call back"]),
                ("message", ["رسالة", "Message"]),
            ],
        ),
        (
            "org.ultar.power",
            Urgency::High,
            ["البطارية منخفضة: 9%", "Battery low: 9%"],
            [
                "صِل الشاحن، أو شغّل وضع التوفير ليدوم الجهاز أكثر.",
                "Plug in the charger or turn on the saver mode to last longer.",
            ],
            &[("saver", ["تشغيل وضع التوفير", "Turn on saver mode"])],
        ),
    ]
}

/// The notifications of board Toasts (oldest first), shown as banners at `BOARD_MS`.
pub fn board_notes(d: &mut Desktop, lang: Lang) {
    use ultar_bus::messages::notify::{Body, Category as Kind, Post, Posted};
    use ultar_bus::{Text, TextArg};
    let i = usize::from(lang != Lang::Ar);
    let notes = board_note_data();
    for (n, (app, urgency, title, body, buttons)) in notes.into_iter().enumerate() {
        let id = 100 + n as u64;
        let title = Text::key("desk.note.verbatim").with(TextArg::Verbatim(title[i].into()));
        let body = Body::Verbatim(body[i].into());
        let post = Post {
            urgency,
            category: Kind::App,
            app_id: app.into(),
            title,
            body,
            actions: buttons
                .iter()
                .map(|(id, label)| ultar_bus::messages::notify::Action {
                    id: (*id).into(),
                    label: Text::key("desk.note.verbatim").with(TextArg::Verbatim(label[i].into())),
                })
                .collect(),
        };
        d.notes.add(Posted {
            id,
            timestamp_ms: BOARD_MS - 10_000,
            sender: format!("app.{app}"),
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
    /// The dock or the phone's gesture bar.
    pub second: Option<View>,
    /// The phone's home screen.
    pub home: Option<View>,
    pub screen: Size,
}

impl Harness {
    pub fn new(lang: Lang, theme: ThemeKind) -> Harness {
        Harness::with(board_desktop(lang, theme), lang, theme)
    }

    pub fn with(d: Desktop, lang: Lang, theme: ThemeKind) -> Harness {
        Harness::shaped(d, lang, theme, Shape::default(), SCREEN)
    }

    /// The phone shell in the state of the Compact boards.
    pub fn phone(lang: Lang, theme: ThemeKind) -> Harness {
        let d = board_phone(lang, theme);
        Harness::shaped(d, lang, theme, Shape::COMPACT, PHONE)
    }

    /// The desktop with the top bar and the dock (board DesktopTop).
    pub fn top(lang: Lang, theme: ThemeKind) -> Harness {
        let d = board_desktop(lang, theme);
        Harness::shaped(d, lang, theme, Shape::TOP, SCREEN)
    }

    pub fn shaped(d: Desktop, lang: Lang, theme: ThemeKind, shape: Shape, screen: Size) -> Harness {
        let clock = Arc::new(FixedClock(std::sync::atomic::AtomicU64::new(BOARD_MS)));
        let mut shell = Shell::new(d, clock.clone(), "أنس");
        shell.shape = shape;
        let look = Look::new(lang, theme);
        let bar = build_bar(&shell, look, screen.w).unwrap();
        let mut h = Harness {
            shell,
            look,
            bar,
            popup: None,
            effects: Vec::new(),
            clock,
            second: None,
            home: None,
            screen,
        };
        h.apply(Outcome::surfaces());
        h
    }

    fn layout(&mut self) {
        let _ = self.bar.ui.content_size();
        for v in [
            self.popup.as_mut().map(|p| &mut p.view),
            self.second.as_mut(),
            self.home.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            let _ = v.ui.content_size();
        }
    }

    pub fn apply(&mut self, out: Outcome) {
        self.effects.extend(out.effects.clone());
        if out.bar {
            self.bar = build_bar(&self.shell, self.look, self.screen.w).unwrap();
            self.second = build_second(&self.shell, self.look, self.screen.w).unwrap();
            self.home = build_home(&self.shell, self.look, self.screen).unwrap();
        }
        if out.popup {
            self.popup = build_popup(&self.shell, self.look, self.screen).unwrap();
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
        // The status button: beside its icons (the network icon has its own page, see
        // `click_status_icon`).
        let x = if *target == Target::Status {
            r.x + 2.0
        } else {
            r.x + r.w / 2.0
        };
        let pos = Point::new(x, r.y + r.h / 2.0);
        Harness::press_at(view, pos, button)
    }

    /// A press and release at `pos` of `view`.
    fn press_at(view: &mut View, pos: Point, button: PointerButton) -> Vec<Action> {
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

    /// A click on the middle of the bar's status button (the network icon when it is the
    /// only one) – or, with `icon`, on that status icon (reading order, `icons` of the
    /// status brick).
    pub fn click_status_icon(&mut self, icon: usize, count: usize) {
        let node = self.bar.node(&Target::Status).expect("the status button");
        let r = self.bar.ui.rect(node).expect("laid out");
        // the icon row of the brick: 16-point icons, 12 apart, centred; mirrored in Arabic
        let icon_w = ultar_ui::tokens::size::ICON_SMALL;
        let gap = ultar_ui::tokens::Space::S12.points();
        let width = count as f32 * icon_w + (count as f32 - 1.0) * gap;
        let slot = if self.bar.ui.env().direction.is_rtl() {
            count - 1 - icon
        } else {
            icon
        };
        let x = r.x + (r.w - width) / 2.0 + slot as f32 * (icon_w + gap) + icon_w / 2.0;
        let a = Harness::press_at(
            &mut self.bar,
            Point::new(x, r.y + r.h / 2.0),
            PointerButton::Primary,
        );
        self.dispatch_bar(a);
    }

    /// A click on the second panel (dock, gesture bar) or the home screen.
    pub fn click_side(&mut self, target: Target) {
        let view = [self.second.as_mut(), self.home.as_mut()]
            .into_iter()
            .flatten()
            .find(|v| v.node(&target).is_some())
            .unwrap_or_else(|| panic!("no side panel shows {target:?}"));
        let actions = Harness::press(view, &target, PointerButton::Primary);
        for a in actions {
            let out = self.shell.on_surface(&target, &a.kind);
            self.apply(out);
        }
    }

    /// A secondary click (a long press of touch) on the home screen: the toolkit opens
    /// the node's context menu; true when a menu is open afterwards.
    pub fn right_home(&mut self, target: Target) -> bool {
        let home = self.home.as_mut().expect("a home screen");
        let _ = Harness::press(home, &target, PointerButton::Secondary);
        home.ui.menu_rect(0).is_some()
    }

    /// A drag with the primary button from `from` to the middle of `to` in `view` (home
    /// screen or popup, like the toolkit sees it): the dragged node's target and the target
    /// of the node under the drop, as the app resolves them.
    pub fn drag_in(view: &mut View, from: &Target, to: &Target) -> Option<Target> {
        let centre = |v: &View, t: &Target| {
            let r = v.ui.rect(v.node(t).unwrap()).unwrap();
            Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0)
        };
        let (a, b) = (centre(view, from), centre(view, to));
        let button = PointerButton::Primary;
        view.ui.handle(Event::PointerMove { pos: a });
        view.ui.handle(Event::PointerDown { pos: a, button });
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            let pos = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            view.ui.handle(Event::PointerMove { pos });
        }
        let actions = view.ui.handle(Event::PointerUp { pos: b, button }).actions;
        actions.iter().find_map(|act| match act.kind {
            ActionKind::DragEnded {
                target: Some(n), ..
            } => view.target(n).cloned(),
            _ => None,
        })
    }

    /// An action of a node given directly (drags, menu items).
    pub fn act(&mut self, target: Target, kind: ActionKind) {
        let out = self.shell.on_surface(&target, &kind);
        self.apply(out);
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
