//! The desktop shell program: the bar (panel at the bottom), its one popup, the bus.

mod banners;
mod effects;
pub mod link;

use super::{primary, AppError, Options};
use crate::host::{Host, HostEvent};
use crate::model::time::{Clock, SystemClock};
use crate::model::{Desktop, Open};
use crate::ports::LauncherPort;
use crate::shell::{build_bar, Outcome, Shell};
use crate::view::{Look, Target};
use link::Link;
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_surface::client::WindowConfig;
use ultar_surface::proto::{Edge, Event, OutputId, Request, Role, SurfaceId};
use ultar_ui::event::{ActionKind, Key};
use ultar_ui::geom::Size;
use ultar_ui::tokens::{size, ThemeKind};

/// Application id of the shell.
pub const APP_ID: &str = "org.ultar.desktop";
/// How often the loop looks at the bus when nothing else happens.
const BUS_POLL: Duration = Duration::from_millis(500);
/// How long a launch waits for its activation token.
const TOKEN_WAIT_MS: u64 = 300;

/// The open popup surface.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PopupSurface {
    id: SurfaceId,
    open: Open,
    offset: (i32, i32),
}

/// The running shell.
pub struct DesktopApp {
    host: Host,
    shell: Shell,
    look: Look,
    bar: SurfaceId,
    popup: Option<PopupSurface>,
    /// The banners' surface and its offset.
    banners: Option<(SurfaceId, (i32, i32))>,
    link: Option<Link>,
    displays: Vec<(OutputId, String)>,
    pending: Option<(String, u64)>,
    started: Instant,
}

impl std::fmt::Debug for DesktopApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopApp")
            .field("shell", &self.shell)
            .finish_non_exhaustive()
    }
}

fn screen(host: &Host) -> Size {
    primary(host.client.outputs()).map_or(Size::new(1280.0, 800.0), |o| {
        let s = o.scale_milli.max(1) as f32 / 1000.0;
        Size::new(o.width as f32 / s, o.height as f32 / s)
    })
}

impl DesktopApp {
    /// Connects to the compositor and the bus and shows the bar.
    pub fn start(
        options: &Options,
        user: &str,
        clock: Arc<dyn Clock>,
    ) -> Result<DesktopApp, AppError> {
        let mut host = Host::connect(&options.compositor, APP_ID)?;
        let mut d = Desktop::new(options.lang);
        if options.light {
            d.theme = ThemeKind::Light;
            d.values.dark = false;
        }
        d.outputs = host.client.outputs().to_vec();
        let link = options
            .bus
            .as_deref()
            .and_then(|p| Link::connect(p, APP_ID));
        if let Some(link) = &link {
            link.read_settings(&mut d);
            d.presence.launcher = true;
            // The launcher's apps replace the built-in list (which stays when it has none).
            if let Ok(ids) = link.ports.installed() {
                if !ids.is_empty() {
                    d.catalog = ids
                        .iter()
                        .take(crate::model::apps::MAX_APPS)
                        .map(|id| crate::model::apps::installed(id))
                        .collect();
                }
            }
        }
        let shell = Shell::new(d, clock, user);
        let look = Look::new(options.lang, shell.d.theme);
        let width = screen(&host).w;
        let view = build_bar(&shell, look, width)?;
        let role = Role::Panel {
            edge: Edge::Bottom,
            size: size::BAR_HEIGHT as u32,
        };
        let bar = host.open(&WindowConfig::with_role(role, "", false), view, None)?;
        let mut app = DesktopApp {
            host,
            shell,
            look,
            bar,
            popup: None,
            banners: None,
            link,
            displays: Vec::new(),
            pending: None,
            started: Instant::now(),
        };
        app.report_displays();
        Ok(app)
    }

    /// The shell's state (tests, diagnosis).
    pub fn shell(&self) -> &Shell {
        &self.shell
    }

    /// Runs until `stop` says so or the compositor goes.
    pub fn run(&mut self, stop: &dyn Fn() -> bool) -> Result<(), AppError> {
        while !stop() {
            self.turn()?;
        }
        Ok(())
    }

    /// One turn of the loop.
    pub fn turn(&mut self) -> Result<(), AppError> {
        let minute = time_to_minute(self.shell.clock.as_ref());
        let events = self.host.dispatch(Some(minute.min(BUS_POLL)))?;
        for event in events {
            self.event(event)?;
        }
        self.poll_bus()?;
        self.launch_due(None)?;
        if self.shell.d.notes.expire(self.shell.clock.now_ms()) {
            self.sync_banners()?;
        }
        if time_to_minute(self.shell.clock.as_ref()) > minute {
            // A new minute: the clock of the bar.
            self.rebuild(Outcome {
                bar: true,
                ..Outcome::none()
            })?;
        }
        self.host.draw()?;
        Ok(())
    }

    fn poll_bus(&mut self) -> Result<(), AppError> {
        let Some(link) = &self.link else {
            return Ok(());
        };
        let now = self.shell.clock.now_ms();
        let (keys, any) = link.poll(&mut self.shell.d, now);
        if keys
            .iter()
            .any(|k| k.starts_with("display.mode") || k == "display.single_display")
        {
            self.apply_display_mode();
        }
        if any {
            if self.look.theme != self.shell.d.theme {
                self.look.theme = self.shell.d.theme;
            }
            self.rebuild(Outcome::surfaces())?;
        }
        Ok(())
    }

    fn event(&mut self, event: HostEvent) -> Result<(), AppError> {
        let outcome = match event {
            HostEvent::Action(id, action) => self.action(id, &action.kind, action.node),
            HostEvent::Key(id, key, m) if Some(id) == self.popup.map(|p| p.id) => {
                if m.meta && matches!(key, Key::Char('p' | 'P')) {
                    self.shell.on_shell_key(25)
                } else {
                    self.shell.on_popup_key(&key)
                }
            }
            HostEvent::Key(id, Key::Escape, _) if id == self.bar => self.release(),
            HostEvent::Menu(id, node) if id == self.bar => match self.target(id, node) {
                Some(t) => self.shell.on_bar_menu(&t),
                None => Outcome::none(),
            },
            HostEvent::Resized(id, _) if id == self.bar => Outcome::surfaces(),
            HostEvent::Compositor(e) => self.compositor(e)?,
            _ => Outcome::none(),
        };
        self.apply(outcome)
    }

    fn target(&self, id: SurfaceId, node: ultar_ui::ui::NodeId) -> Option<Target> {
        self.host.view(id).and_then(|v| v.target(node)).cloned()
    }

    fn action(&mut self, id: SurfaceId, kind: &ActionKind, node: ultar_ui::ui::NodeId) -> Outcome {
        let Some(target) = self.target(id, node) else {
            return Outcome::none();
        };
        if id == self.bar {
            return self.shell.on_bar(&target, kind);
        }
        if self.banners.is_some_and(|(b, _)| b == id) {
            return self.on_banner(&target, kind);
        }
        match (kind, &target) {
            (ActionKind::MenuChosen(item), t) if !matches!(t, Target::AppItem(_)) => {
                self.shell.on_menu(*item)
            }
            (ActionKind::Closed, Target::Menu) => self.shell.close(),
            (ActionKind::TextChanged, Target::SearchField) => {
                let text = self
                    .host
                    .view(id)
                    .and_then(|v| v.ui.text(node).ok())
                    .unwrap_or_default()
                    .to_owned();
                self.shell.on_query(&text)
            }
            _ => self.shell.on_popup(&target, kind),
        }
    }

    /// Esc on the bar: the keyboard goes back to the window.
    fn release(&mut self) -> Outcome {
        let _ = self.host.client.send(&Request::ShellFocus {
            surface: SurfaceId(0),
        });
        Outcome::none()
    }

    fn compositor(&mut self, event: Event) -> Result<Outcome, AppError> {
        let d = &mut self.shell.d;
        Ok(match event {
            Event::ShellKey { code, .. } => self.shell.on_shell_key(code),
            Event::WindowsChanged { windows } => {
                d.windows = windows;
                Outcome::surfaces()
            }
            Event::KeyboardLayouts { names, active } => {
                d.layouts = crate::model::state::Layouts { names, active };
                Outcome::surfaces()
            }
            Event::ShellFocusLost { .. } | Event::SessionLocked { locked: true } => {
                self.shell.close()
            }
            Event::Output(_) | Event::OutputRemoved { .. } => {
                d.outputs = self.host.client.outputs().to_vec();
                self.report_displays();
                Outcome::surfaces()
            }
            Event::ActivationToken { token } => {
                self.launch_due(Some(token))?;
                Outcome::none()
            }
            _ => Outcome::none(),
        })
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// Time until the next minute of the wall clock.
fn time_to_minute(clock: &dyn Clock) -> Duration {
    Duration::from_millis(crate::model::time::LocalTime::ms_to_next_minute(
        clock.now_ms(),
    ))
}

/// Starts the shell on the system clock.
pub fn run(options: &Options, user: &str, stop: &dyn Fn() -> bool) -> Result<(), AppError> {
    let mut app = DesktopApp::start(options, user, Arc::new(SystemClock::default()))?;
    app.run(stop)
}
