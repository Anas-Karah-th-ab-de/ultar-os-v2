//! The desktop shell program: the bar (panel at the bottom), its one popup, the bus.

mod banners;
mod effects;
pub mod link;
mod panels;
mod slow;
mod topics;

use super::keyboard;
use super::{primary, AppError, Options};
use crate::host::{Host, HostEvent};
use crate::model::time::{Clock, SystemClock};
use crate::model::{Desktop, Open, Shape};
use crate::ports::LauncherPort;
use crate::shell::{build_bar, Opening, Outcome, Shell};
use crate::view::{Look, Target};
use link::Link;
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_surface::proto::{Event, OutputId, Request, SurfaceId};
use ultar_ui::event::{ActionKind, Key};
use ultar_ui::geom::Size;
use ultar_ui::tokens::ThemeKind;

/// Application id of the shell.
pub const APP_ID: &str = "org.ultar.desktop";
/// How often the loop looks at the bus when nothing else happens.
const BUS_POLL: Duration = Duration::from_millis(500);
/// How long a launch waits for its activation token.
const TOKEN_WAIT_MS: u64 = 300;
/// How long the visual-alert flash stays (the compositor's limit is 1000 ms).
const FLASH_MS: u32 = 200;

/// The open popup surface.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PopupSurface {
    id: SurfaceId,
    open: Open,
    offset: (i32, i32),
}

/// The banners' surface.
#[derive(Debug, Default)]
struct BannerSurface {
    /// The surface and its offset.
    surface: Option<(SurfaceId, (i32, i32))>,
    /// When the banners' clocks were last looked at (ms).
    tick: u64,
}

/// The running shell.
pub struct DesktopApp {
    host: Host,
    shell: Shell,
    look: Look,
    bar: SurfaceId,
    /// The dock and the desktop background.
    side: panels::SidePanels,
    popup: Option<PopupSurface>,
    /// The banners' surface and when their clocks were last looked at.
    banners: BannerSurface,
    link: Option<Link>,
    displays: Vec<(OutputId, String)>,
    pending: Option<(String, Opening, u64)>,
    started: Instant,
    /// The bar's place was given on the command line (the setting does not move it).
    bar_fixed: bool,
    /// Service calls running on workers (scan, join, VPN).
    slow: slow::SlowCalls,
    /// Which layout announcements are the user's switches (stored in the settings).
    layouts: keyboard::LayoutSync,
    /// The shell's own folder for the per-person search history and desktop shortcuts
    /// (`None`: nothing kept across restarts).
    history_dir: Option<std::path::PathBuf>,
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

/// Reads the followed settings and the launcher's apps, which replace the built-in list
/// (it stays when the launcher has none).
fn read_services(link: &Link, d: &mut Desktop) {
    link.read_settings(d);
    d.presence.launcher = true;
    d.presence.bus = true;
    if let Ok(ids) = link.ports.installed() {
        if !ids.is_empty() {
            d.catalog = ids
                .iter()
                .take(crate::model::apps::MAX_APPS)
                .map(|id| crate::model::apps::installed(id))
                .collect();
        }
    }
    // The Store's cached, signature-checked catalogue for the overview search
    // (search.source.store); no network. Absent or unverified: nothing offered.
    d.overview.store = crate::model::store::load_from_env();
}

/// What is kept beside the shell and in the desktop folder: the person's remembered
/// searches (only shown when `search.history` is on; off keeps nothing), their desktop
/// shortcuts and the desktop folder's items.
fn read_kept(d: &mut Desktop, options: &Options, user: &str) {
    let state = options.state.as_deref();
    d.overview.history = crate::model::history::load(state, user);
    if !d.overview.sources.history {
        d.overview.history.clear();
    }
    d.places.shortcuts = crate::model::places::load(state, user);
    if let Some(dir) = &options.desk_folder {
        d.places.files = crate::model::desk_files::list(dir);
    }
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
            read_services(link, &mut d);
        }
        read_kept(&mut d, options, user);
        let size = screen(&host);
        let place = options
            .bar
            .or_else(|| link.as_ref().and_then(Link::bar_place))
            .unwrap_or_default();
        let mut shell = Shell::new(d, clock, user);
        shell.shape = Shape { bar: place };
        // The language of the settings (region.language) when the service answered, else
        // the session's; the accent and text size of the settings.
        let v = shell.d.values;
        let look = Look::new(shell.d.lang, shell.d.theme)
            .with_access(v.high_contrast, v.large_text)
            .with_prefs(&shell.d.prefs);
        let view = build_bar(&shell, look, size.w)?;
        let bar = host.open(&panels::bar_config(shell.shape), view, None)?;
        let mut app = DesktopApp {
            host,
            shell,
            look,
            bar,
            side: panels::SidePanels::new(options.desk_folder.clone()),
            bar_fixed: options.bar.is_some(),
            popup: None,
            banners: BannerSurface::default(),
            link,
            displays: Vec::new(),
            pending: None,
            started: Instant::now(),
            slow: slow::SlowCalls::default(),
            layouts: keyboard::LayoutSync::default(),
            history_dir: options.state.clone(),
        };
        app.apply_keyboard()?;
        app.open_side_panels()?;
        app.sync_background()?;
        app.report_displays();
        Ok(app)
    }

    /// The shell's state (tests, diagnosis).
    pub fn shell(&self) -> &Shell {
        &self.shell
    }

    /// Where the node of `target` is drawn: in the open popup (`true`) or on the bar
    /// (`false`), in logical points of that surface (tests drive the pointer with it).
    pub fn locate(&self, target: &Target) -> Option<(bool, ultar_ui::geom::Rect)> {
        let popup = self.popup.map(|p| (true, p.id));
        popup
            .into_iter()
            .chain(std::iter::once((false, self.bar)))
            .find_map(|(is_popup, id)| {
                let view = self.host.view(id)?;
                let node = view.node(target)?;
                view.ui.rect(node).map(|r| (is_popup, r))
            })
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
        let seconds = self.shell.d.prefs.clock.seconds;
        let tick = clock_tick(self.shell.clock.as_ref(), seconds);
        let wait = time_to_tick(self.shell.clock.as_ref(), seconds);
        let events = self.host.dispatch(Some(wait.min(BUS_POLL)))?;
        for event in events {
            self.event(event)?;
        }
        self.poll_bus()?;
        self.scan_desk_folder()?;
        self.finish_slow()?;
        self.launch_due(None)?;
        self.tick_banners()?;
        if clock_tick(self.shell.clock.as_ref(), seconds) != tick {
            // A new minute (or second, with bar.clock.show_seconds): the clock of the bar.
            self.rebuild(Outcome {
                bar: true,
                ..Outcome::none()
            })?;
        }
        self.send_visual_alert();
        self.host.draw()?;
        Ok(())
    }

    /// A notification alerted while `accessibility.visual_alert` is on: flash the
    /// screen through the compositor (the shell cannot draw over other windows).
    fn send_visual_alert(&mut self) {
        if self.shell.d.notes.take_flash() {
            let _ = self
                .host
                .client
                .send(&Request::VisualAlert { ms: FLASH_MS });
        }
    }

    fn poll_bus(&mut self) -> Result<(), AppError> {
        let Some(link) = &self.link else {
            return Ok(());
        };
        let now = self.shell.clock.now_ms();
        let before = wifi_mark(&self.shell.d);
        let (keys, any, calendar) = link.poll(&mut self.shell.d, now);
        // The networks page follows the connection: joined, left or lost – a fresh list.
        let wifi_page = self.shell.d.open == Open::Quick(crate::model::QuickPage::Wifi);
        if wifi_page && wifi_mark(&self.shell.d) != before {
            self.apply(Outcome::effect(crate::shell::Effect::Service(
                crate::model::services::Call::Networks,
            )))?;
        }
        // The devices page follows the service: a new state, a fresh list.
        let bt_page = self.shell.d.open == Open::Quick(crate::model::QuickPage::Bluetooth);
        if any && bt_page {
            self.apply(Outcome::effect(crate::shell::Effect::Service(
                crate::model::services::Call::BtDevices,
            )))?;
        }
        if calendar && self.shell.d.open == Open::Calendar {
            if let Some(effect) = self.shell.day_request() {
                self.apply(Outcome {
                    effects: vec![effect],
                    ..Outcome::surfaces()
                })?;
            }
        }
        self.react_media()?;
        if keys
            .iter()
            .any(|k| k.starts_with("display.mode") || k == "display.single_display")
        {
            self.apply_display_mode();
        }
        if keys.iter().any(|k| {
            k == keyboard::LAYOUTS || k == keyboard::ACTIVE || k == keyboard::INPUT_LANGUAGES
        }) {
            self.apply_keyboard()?;
        }
        if keys.iter().any(|k| k == link::BAR_POSITION) && !self.bar_fixed {
            let place = self.link.as_ref().and_then(Link::bar_place);
            if let Some(place) = place {
                return self.reshape(Shape { bar: place });
            }
        }
        self.sync_local(&keys)?;
        if any {
            if self.look.theme != self.shell.d.theme {
                self.look.theme = self.shell.d.theme;
            }
            self.rebuild(Outcome::surfaces())?;
        }
        Ok(())
    }

    /// What the shell keeps or reads beside the settings: the Store's catalogue, the history.
    fn sync_local(&mut self, keys: &[String]) -> Result<(), AppError> {
        self.sync_store_catalog(keys);
        self.sync_history(keys)
    }

    /// search.source.store changed: read the Store's cached catalogue again (a fresh verified
    /// cache may have appeared); the switch itself gates what is offered.
    fn sync_store_catalog(&mut self, keys: &[String]) {
        if keys.iter().any(|k| k == "search.source.store") {
            self.shell.d.overview.store = crate::model::store::load_from_env();
        }
    }

    /// search.history changed: keep the file beside the shell in step (save when on, delete
    /// when off). The entries are already in the state (`apply_setting` clears them on off).
    fn sync_history(&mut self, keys: &[String]) -> Result<(), AppError> {
        if keys.iter().any(|k| k == "search.history") {
            self.apply(Outcome::effect(crate::shell::Effect::History))?;
        }
        Ok(())
    }

    /// A drive with media was plugged in: `apps.removable_media` decides what happens.
    fn react_media(&mut self) -> Result<(), AppError> {
        let media = self.shell.media_inserted();
        if media.bar || media.popup || !media.effects.is_empty() {
            self.apply(media)?;
        }
        Ok(())
    }

    fn event(&mut self, event: HostEvent) -> Result<(), AppError> {
        let outcome = match event {
            HostEvent::Action(id, action) => self.action(id, &action.kind, action.node),
            HostEvent::Key(id, key, m) if Some(id) == self.popup.map(|p| p.id) => {
                if m.meta && matches!(key, Key::Char('p' | 'P')) {
                    self.shell.on_shell_key(25)
                } else if key == Key::Escape && self.shell.d.notes.hide_newest_banner() {
                    // Esc closes the newest banner first, then what is open (the shell
                    // has the keyboard only while one of its popups is open).
                    Outcome::none()
                } else {
                    self.shell.on_popup_key(&key)
                }
            }
            HostEvent::Key(id, Key::Escape, _) if id == self.bar => {
                // Esc closes the newest banner first, then gives the keyboard back.
                if self.shell.d.notes.hide_newest_banner() {
                    Outcome::surfaces()
                } else {
                    self.release()
                }
            }
            HostEvent::Menu(id, node, _) if id == self.bar || self.is_panel(id) => {
                match self.target(id, node) {
                    Some(t) => self.shell.on_bar_menu(&t),
                    None => Outcome::none(),
                }
            }
            HostEvent::Menu(id, node, at) if self.is_background(id) => {
                // The background lies at the screen's corner: its points are the screen's.
                let target = self.target(id, node);
                let at = (at.x.round() as i32, at.y.round() as i32);
                self.shell.on_desk_menu(target.as_ref(), at)
            }
            HostEvent::Resized(id, _) if id == self.bar => self.resized()?,
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
        if self.is_panel(id) {
            return self.shell.on_surface(&target, kind);
        }
        if self.is_background(id) {
            return self.shell.on_desk(&target, kind);
        }
        if self.banners.surface.is_some_and(|(b, _)| b == id) {
            return self.on_banner(&target, kind);
        }
        match (kind, &target) {
            (ActionKind::MenuChosen(item), t) if !matches!(t, Target::AppItem(_)) => {
                self.shell.on_menu(*item)
            }
            (ActionKind::Closed, Target::Menu) => self.shell.close(),
            // The join dialog: the password goes from the field (wiped) into the call.
            (ActionKind::Activated, Target::JoinConnect)
            | (ActionKind::Submitted, Target::JoinPassword) => {
                let secret = self.take_password(id);
                self.shell.join_with(secret)
            }
            (ActionKind::TextChanged, Target::SearchField | Target::RenameField) => {
                let text = self
                    .host
                    .view(id)
                    .and_then(|v| v.ui.text(node).ok())
                    .unwrap_or_default()
                    .to_owned();
                if target == Target::RenameField {
                    self.shell.on_rename_text(&text)
                } else {
                    self.shell.on_query(&text)
                }
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
            Event::WindowsChanged { windows } => self.shell.on_windows(windows),
            Event::Workspaces { current, count } => {
                d.overview.workspaces = Some((current, count));
                Outcome::surfaces()
            }
            Event::WindowThumbnail {
                window,
                width,
                height,
                data,
            } => match ultar_ui::widgets::Thumbnail::new(width.into(), height.into(), data) {
                Some(thumb) => {
                    d.overview.keep_thumb(window, thumb);
                    Outcome::surfaces()
                }
                None => Outcome::none(),
            },
            Event::KeyboardLayouts { names, active } => {
                let save = self.layouts.announced(&names, active);
                d.layouts = crate::model::state::Layouts { names, active };
                self.save_layout(save);
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

    /// Gives the compositor the input languages of the settings service (list, then the
    /// active one: the compositor takes requests in order).
    fn apply_keyboard(&mut self) -> Result<(), AppError> {
        let Some((names, active)) = self.link.as_ref().and_then(|l| keyboard::wanted(&l.ports))
        else {
            return Ok(());
        };
        let known = &self.shell.d.layouts;
        self.layouts
            .applied(&names, active, (&known.names, known.active));
        self.host
            .client
            .send(&Request::SetKeyboardLayouts { names })?;
        self.host
            .client
            .send(&Request::SetKeyboardLayout { index: active })?;
        Ok(())
    }

    /// Stores the active layout when the user switched (it is the one at the next start).
    fn save_layout(&mut self, name: Option<String>) {
        let (Some(link), Some(name)) = (&self.link, name) else {
            return;
        };
        use crate::ports::SettingsPort;
        let value = ultar_bus::messages::settings::SettingValue::Text(name.clone());
        if link.ports.set(keyboard::ACTIVE, value).is_ok() {
            self.layouts.stored(name);
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// What of the Wi-Fi connection makes the networks page ask again: its phase and network.
fn wifi_mark(
    d: &crate::model::Desktop,
) -> Option<(ultar_bus::messages::net::WifiPhase, Option<String>)> {
    d.svc.wifi.link.as_ref().map(|l| (l.phase, l.ssid.clone()))
}

/// Time until the next minute of the wall clock.
/// Which minute (or second) of the clock it is: the bar is built again when it changes.
fn clock_tick(clock: &dyn Clock, seconds: bool) -> u64 {
    clock.now_ms() / if seconds { 1000 } else { 60_000 }
}

/// How long until the bar's clock changes.
fn time_to_tick(clock: &dyn Clock, seconds: bool) -> Duration {
    let now = clock.now_ms();
    if seconds {
        Duration::from_millis(1000 - now % 1000)
    } else {
        Duration::from_millis(crate::model::time::LocalTime::ms_to_next_minute(now))
    }
}

/// Starts the shell on the system clock.
pub fn run(options: &Options, user: &str, stop: &dyn Fn() -> bool) -> Result<(), AppError> {
    let mut app = DesktopApp::start(options, user, Arc::new(SystemClock::default()))?;
    app.run(stop)
}
