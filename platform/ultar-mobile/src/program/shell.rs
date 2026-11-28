//! `ultar-mobile` on the compositor: the status bar (panel at the top), the navigation bar
//! (panel at the bottom), the home screen (background, between the bars) and, while open,
//! the quick settings, recent apps or search as a sheet over the home screen. Apps open as
//! windows between the bars; the bars stay.

use super::bus::Link;
use super::{local_now, ms_to_minute, screen_size, zone_offset, Options, ProgramError};
use crate::app::{apply_click, apply_swipe, Effect};
use crate::click::Click;
use crate::model::{Mobile, Screen, WinAction, Window};
use crate::view::{self, metrics, Built, Part};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use ultar_hal_system::load::Sampler;
use ultar_i18n::Translator;
use ultar_shell_host::{Host, HostEvent};
use ultar_surface::client::WindowConfig;
use ultar_surface::proto::{
    Edge, Event, Request, Role, SurfaceId, SwipeEdge, WindowAction, WindowState,
};
use ultar_ui::event::{Action, ActionKind};
use ultar_ui::geom::{Point, Size};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;

/// Application id of the phone shell.
pub const APP_ID: &str = "org.ultar.mobile";
/// Longest wait of one turn (the bus is polled at least this often).
const TURN: Duration = Duration::from_millis(250);
/// How often the device card is read again.
const SAMPLE: Duration = Duration::from_secs(5);

/// The surfaces of the shell.
#[derive(Debug, Clone, Copy)]
struct Surfaces {
    status: SurfaceId,
    nav: SurfaceId,
    home: SurfaceId,
    sheet: Option<SurfaceId>,
}

/// The running phone shell.
pub struct ShellProgram {
    host: Host<Ui>,
    /// The state (tests, diagnosis).
    pub model: Mobile,
    env: PaintEnv,
    tr: Translator,
    link: Option<Link>,
    s: Surfaces,
    built: BTreeMap<SurfaceId, Built>,
    screen: Size,
    sampler: Sampler,
    sampled: Instant,
    /// Seconds east of UTC of the time zone (read again every minute).
    offset: i32,
}

impl std::fmt::Debug for ShellProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellProgram")
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

impl ShellProgram {
    /// Connects to the compositor and the bus and shows the bars and the home screen.
    pub fn start(options: &Options) -> Result<ShellProgram, ProgramError> {
        let mut host: Host<Ui> = Host::connect(&options.compositor, APP_ID)?;
        let theme = if options.light {
            ThemeKind::Light
        } else {
            ThemeKind::Dark
        };
        let env = PaintEnv::new(options.lang, theme);
        let tr = crate::i18n::translator_or_keys(options.lang);
        let screen = screen_size(host.client.outputs());
        let mut model = Mobile::default();
        model.unlocked();
        model.hardware.torch = false;
        let mut link = options.bus.as_deref().and_then(Link::connect);
        if let Some(link) = link.as_mut() {
            link.load(&mut model, &tr);
            link.report_display(&mut model, host.client.outputs());
        }
        let offset = zone_offset(link.as_ref().map_or(super::tz::DEFAULT, Link::zone));
        model.now = local_now(offset);
        let blank = || -> Result<Ui, ProgramError> {
            Ok(Ui::new(
                env,
                InputMode::Touch,
                Size::new(1.0, 1.0),
                tr.clone(),
            )?)
        };
        let panel = |edge, size| WindowConfig::with_role(Role::Panel { edge, size }, "", false);
        let status = host.open(&panel(Edge::Top, metrics::status() as u32), blank()?, None)?;
        let nav = host.open(&panel(Edge::Bottom, metrics::nav() as u32), blank()?, None)?;
        // Phone mode: apps fill the space between the bars without title bars, and the
        // compositor reports the edge swipes (the window list came with the welcome).
        host.client.send(&Request::SetPhoneMode { on: true })?;
        let bg = WindowConfig::with_role(Role::Background, "", false);
        let home = host.open(&bg, blank()?, None)?;
        let mut shell = ShellProgram {
            host,
            model,
            env,
            tr,
            link,
            s: Surfaces {
                status,
                nav,
                home,
                sheet: None,
            },
            built: BTreeMap::new(),
            screen,
            sampler: Sampler::default(),
            sampled: Instant::now(),
            offset,
        };
        shell.sample();
        shell.show()?;
        Ok(shell)
    }

    /// Runs until `stop` says so or the compositor goes.
    pub fn run(&mut self, stop: &dyn Fn() -> bool) -> Result<(), ProgramError> {
        while !stop() {
            self.turn()?;
        }
        Ok(())
    }

    /// One turn: compositor events, the bus, the clock.
    pub fn turn(&mut self) -> Result<(), ProgramError> {
        let wait = Duration::from_millis(ms_to_minute()).min(TURN);
        let mut changed = false;
        for event in self.host.dispatch(Some(wait))? {
            changed |= self.event(event)?;
        }
        if let Some(link) = self.link.as_mut() {
            changed |= link.poll(&mut self.model, &self.tr);
        }
        // A notice of the mesh ends by itself.
        let shown = self.model.mesh.notice_at(self.model.now_ms);
        self.model.now_ms = super::now_ms();
        changed |= shown != self.model.mesh.notice_at(self.model.now_ms);
        let now = local_now(self.offset);
        if now != self.model.now {
            let zone = self.link.as_ref().map_or(super::tz::DEFAULT, Link::zone);
            self.offset = zone_offset(zone);
            self.model.now = local_now(self.offset);
            changed = true;
        }
        if self.sampled.elapsed() >= SAMPLE {
            changed |= self.sample();
        }
        if changed {
            self.show()?;
        }
        self.host.draw()?;
        Ok(())
    }

    fn sample(&mut self) -> bool {
        self.sampled = Instant::now();
        let (cpu, ram) = self.sampler.sample();
        let before = (self.model.status.cpu, self.model.status.ram);
        self.model.status.cpu = cpu.unwrap_or(before.0);
        self.model.status.ram = ram.unwrap_or(before.1);
        before != (self.model.status.cpu, self.model.status.ram)
    }

    /// Handles one event; `true` when the model changed.
    fn event(&mut self, event: HostEvent) -> Result<bool, ProgramError> {
        Ok(match event {
            HostEvent::Action(id, action) => self.action(id, &action),
            HostEvent::Swipe(id, delta) => {
                // A swipe that starts on the status bar comes from the top edge.
                let from = if id == self.s.status {
                    Point::new(0.0, 0.0)
                } else {
                    Point::new(0.0, self.screen.h / 2.0)
                };
                let rtl = self.env.direction == ultar_i18n::Direction::Rtl;
                let per = view::per_page(self.screen.h);
                let (changed, effects) = apply_swipe(&mut self.model, from, delta, rtl, per);
                self.effects(&effects);
                changed
            }
            HostEvent::Resized(..) => true,
            HostEvent::Compositor(Event::WindowsChanged { windows }) => {
                let list = windows
                    .into_iter()
                    .map(|w| Window {
                        id: w.id,
                        app: super::bus::shell_id(&w.app_id),
                        title: w.title,
                        minimized: w.state == WindowState::Minimized,
                        focused: w.focused,
                    })
                    .collect();
                self.model.set_windows(list);
                true
            }
            // Phone mode: up from the bottom edge = home, held = recent apps, in from a side = back.
            HostEvent::Compositor(Event::EdgeSwipe { edge, held }) => {
                let click = match (edge, held) {
                    (SwipeEdge::Bottom, false) => Click::Home,
                    (SwipeEdge::Bottom, true) => Click::Recents,
                    (SwipeEdge::Side, _) => Click::Back,
                };
                self.click(click)
            }
            HostEvent::Compositor(Event::Output(_) | Event::OutputRemoved { .. }) => {
                self.screen = screen_size(self.host.client.outputs());
                true
            }
            HostEvent::Compositor(Event::SessionLocked { locked: true }) => {
                // Back on the home screen after the next unlock.
                self.model.go(Screen::Home);
                true
            }
            _ => false,
        })
    }

    fn action(&mut self, id: SurfaceId, action: &Action) -> bool {
        let Some(built) = self.built.get(&id) else {
            return false;
        };
        let node = action.node;
        match action.kind {
            ActionKind::ValueChanged(v) if Some(node) == built.brightness => {
                let v = v.clamp(0.0, 100.0) as u8;
                self.model.quick.brightness = v;
                self.effects(&[Effect::Brightness(v)]);
                false
            }
            ActionKind::ValueChanged(v) if Some(node) == built.volume => {
                let v = v.clamp(0.0, 100.0) as u8;
                self.model.quick.volume = v;
                self.effects(&[Effect::Volume(v)]);
                false
            }
            ActionKind::TextChanged if Some(node) == built.search => {
                let text = self
                    .host
                    .view(id)
                    .and_then(|ui| ui.text(node).ok())
                    .unwrap_or_default();
                self.model.query = text.to_owned();
                true
            }
            ActionKind::DragEnded { delta, .. } => {
                let Some(click) = crate::app::drag_click(built.click(node), delta) else {
                    return false;
                };
                self.click(click)
            }
            ActionKind::Closed if self.model.power_menu => {
                self.model.power_menu = false;
                true
            }
            // A device of the choice (the radio group of the sheet).
            ActionKind::Selected(i) if Some(node) == built.targets => {
                self.click(Click::MeshChoose(i))
            }
            ActionKind::Activated | ActionKind::Toggled(_) => {
                let Some(click) = built.click(node).cloned() else {
                    return false;
                };
                self.click(click)
            }
            _ => false,
        }
    }

    /// Where the node of `click` is on the screen (points): the sheet first, then the bars,
    /// then the home screen (tests drive the touch screen with it).
    pub fn locate(&mut self, click: &Click) -> Option<ultar_ui::geom::Rect> {
        // A device of the choice is a card of the radio group, not a node of its own.
        if let Click::MeshChoose(i) = click {
            let sheet = self.s.sheet?;
            self.built.get(&sheet)?.targets?;
            let mut cards: Vec<_> = self
                .host
                .view_mut(sheet)?
                .a11y_tree()
                .into_iter()
                .filter(|n| n.role == ultar_ui::ui::Role::Radio && n.part.is_some())
                .collect();
            cards.sort_by(|a, b| a.rect.y.total_cmp(&b.rect.y));
            let r = cards.get(*i)?.rect;
            return Some(ultar_ui::geom::Rect::new(
                r.x,
                r.y + metrics::status(),
                r.w,
                r.h,
            ));
        }
        let order = [
            self.s.sheet,
            Some(self.s.status),
            Some(self.s.nav),
            Some(self.s.home),
        ];
        for id in order.into_iter().flatten() {
            let Some(node) = self.built.get(&id).and_then(|b| b.node(click)) else {
                continue;
            };
            let r = self.host.view(id)?.rect(node)?;
            let dy = if Some(id) == self.s.sheet {
                metrics::status()
            } else if id == self.s.nav {
                self.screen.h - metrics::nav()
            } else {
                0.0
            };
            return Some(ultar_ui::geom::Rect::new(r.x, r.y + dy, r.w, r.h));
        }
        None
    }

    /// Carries out a click; `true` when the model changed.
    pub fn click(&mut self, click: Click) -> bool {
        let mut effects = Vec::new();
        let per = view::per_page(self.screen.h);
        let changed = apply_click(&mut self.model, click, per, &mut effects);
        self.effects(&effects);
        changed
    }

    fn effects(&mut self, effects: &[Effect]) {
        for e in effects {
            if let Effect::Window(window, action) = e {
                let action = match action {
                    WinAction::Activate => WindowAction::Activate,
                    WinAction::Minimize => WindowAction::Minimize,
                    WinAction::Close => WindowAction::Close,
                    WinAction::Back => WindowAction::Back,
                };
                let request = Request::WindowAction {
                    window: *window,
                    action,
                };
                let _ = self.host.client.send(&request);
            }
        }
        let Some(link) = &self.link else { return };
        for e in effects {
            match e {
                Effect::MeshSend { app, device } => {
                    // The window's title when the app has one (a document, a page), else its name.
                    let window = self.model.windows.iter().rev().find(|w| &w.app == app);
                    let title = match window.filter(|w| !w.title.is_empty()) {
                        Some(w) => w.title.clone(),
                        None => match self.model.app(app) {
                            Some(a) => a
                                .key
                                .as_ref()
                                .map_or_else(|| a.name.clone(), |k| self.tr.t(k).into_owned()),
                            None => app.clone(),
                        },
                    };
                    if let Some(outcome) = link.send(app, &title, device.clone()) {
                        self.model.mesh.sent(app, outcome, self.model.now_ms);
                        if self.model.mesh.choice.is_some() {
                            // The choice survives `go` (it is cleared only on other screens).
                            self.model.go(Screen::Mesh);
                        }
                    }
                }
                other => link.run(other),
            }
        }
    }

    fn new_tree(&self, size: Size, part: Part) -> Result<(Ui, Built), ProgramError> {
        let mut ui = Ui::new(self.env, InputMode::Touch, size, self.tr.clone())?;
        let built = view::build(&mut ui, &self.model, part)?;
        if let Some(field) = built.search {
            ui.focus(field)?;
        }
        Ok((ui, built))
    }

    /// Builds every surface again; opens or closes the sheet.
    fn show(&mut self) -> Result<(), ProgramError> {
        let w = self.screen.w;
        let mut home = self.model.clone();
        home.screen = Screen::Home;
        home.power_menu = false;
        let (status, b) = self.new_tree(Size::new(w, metrics::status()), Part::Status)?;
        self.put(self.s.status, status, b);
        let (nav, b) = self.new_tree(Size::new(w, metrics::nav()), Part::Nav)?;
        self.put(self.s.nav, nav, b);
        let (bg, b) = {
            let mut ui = Ui::new(self.env, InputMode::Touch, self.screen, self.tr.clone())?;
            let built = view::build(&mut ui, &home, Part::Background)?;
            (ui, built)
        };
        self.put(self.s.home, bg, b);
        self.show_sheet()
    }

    fn show_sheet(&mut self) -> Result<(), ProgramError> {
        let open = matches!(
            self.model.screen,
            Screen::Quick | Screen::Recents | Screen::Search | Screen::Mesh
        );
        let size = Size::new(
            self.screen.w,
            (self.screen.h - metrics::status() - metrics::nav()).max(1.0),
        );
        match (open, self.s.sheet) {
            (true, sheet) => {
                let (ui, b) = self.new_tree(size, Part::Body)?;
                let id = match sheet {
                    Some(id) => {
                        self.host.replace(id, ui, Some(size));
                        id
                    }
                    None => {
                        let role = Role::Popup {
                            parent: self.s.status,
                            x: 0,
                            y: metrics::status() as i32,
                        };
                        let config = WindowConfig::with_role(role, "", false);
                        self.host.open(&config, ui, Some(size))?
                    }
                };
                self.s.sheet = Some(id);
                self.built.insert(id, b);
            }
            (false, Some(id)) => {
                self.host.close(id)?;
                self.built.remove(&id);
                self.s.sheet = None;
            }
            (false, None) => {}
        }
        Ok(())
    }

    fn put(&mut self, id: SurfaceId, ui: Ui, built: Built) {
        self.host.replace(id, ui, None);
        self.built.insert(id, built);
    }
}

/// Runs the phone shell.
pub fn run(options: &Options, stop: &dyn Fn() -> bool) -> Result<(), ProgramError> {
    ShellProgram::start(options)?.run(stop)
}
