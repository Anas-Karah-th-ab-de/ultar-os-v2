//! Running the shell's effects and keeping the surfaces in step with its state.

use super::{screen, DesktopApp, PopupSurface, TOKEN_WAIT_MS};
use crate::app::AppError;
use crate::app::{internal, mode_outputs};
use crate::model::services::Call;
use crate::model::Open;
use crate::ports::services::{Answer, ServicesPort};
use crate::ports::{LauncherPort, LockPort, PortError, SettingsPort};

/// Sender of the shell's own notices.
const SHELL: &str = "org.ultar.shell";
use crate::shell::{build_bar, build_popup, Effect, Outcome};
use ultar_bus::messages::settings::{DisplayDescriptor, SettingValue};
use ultar_surface::client::WindowConfig;
use ultar_surface::proto::{DisplayModeRequest, Request, Role, SurfaceId};

/// A rebuilt join dialog keeps what was typed: the text moves from the old field (emptied,
/// its buffer wiped) into the new one.
fn carry_password(old: &mut crate::view::View, new: &mut crate::view::View) {
    let target = crate::view::Target::JoinPassword;
    let (Some(from), Some(to)) = (old.node(&target), new.node(&target)) else {
        return;
    };
    let text = old.ui.text(from).unwrap_or_default().to_owned();
    let secret = ultar_bus::messages::auth::Secret::new(text);
    let _ = old.ui.set_text(from, "");
    let _ = new.ui.set_text(to, secret.expose());
}

/// The same kind of popup (a display mode panel stays one surface while its mode steps).
fn same_surface(a: Open, b: Open) -> bool {
    a == b || matches!((a, b), (Open::Projection(_), Open::Projection(_)))
}

impl DesktopApp {
    /// Runs the effects, then rebuilds what changed.
    pub(super) fn apply(&mut self, outcome: Outcome) -> Result<(), AppError> {
        for effect in &outcome.effects {
            self.run_effect(effect)?;
        }
        self.rebuild(outcome)
    }

    fn run_effect(&mut self, effect: &Effect) -> Result<(), AppError> {
        match effect {
            Effect::Launch { app, open } => {
                // The click's activation token first; the launch follows its answer.
                let asked = self.host.request_activation()?;
                let deadline = self.now_ms() + if asked { TOKEN_WAIT_MS } else { 0 };
                self.pending = Some((app.clone(), open.clone(), deadline));
                self.launch_due(None)?;
            }
            Effect::Window(id, action) => {
                self.host.client.send(&Request::WindowAction {
                    window: *id,
                    action: *action,
                })?;
            }
            Effect::Setting(key, value) => {
                if let Some(link) = &self.link {
                    let _ = link.ports.set(key, value.clone());
                }
            }
            Effect::DisplayMode(mode) => self.choose_display_mode(*mode),
            Effect::Lock => {
                // A lock that did not happen is said, never silent (the lock screen
                // service may be missing or refuse).
                let locked = self.link.as_ref().is_some_and(|l| l.ports.lock().is_ok());
                if !locked {
                    let now = self.now_ms();
                    self.shell
                        .d
                        .notes
                        .post_notice(SHELL, "desk.service.failed", now);
                }
            }
            Effect::Workspace(index) => {
                self.host
                    .client
                    .send(&Request::SwitchWorkspace { index: *index })?;
            }
            Effect::Thumbnails(windows, (max_width, max_height)) => {
                for window in windows {
                    self.host.client.send(&Request::WindowThumbnail {
                        window: *window,
                        max_width: *max_width,
                        max_height: *max_height,
                    })?;
                }
            }
            Effect::Service(call) => self.service(call),
            Effect::Layout(index) => {
                self.host
                    .client
                    .send(&Request::SetKeyboardLayout { index: *index })?;
            }
            Effect::History => self.keep_history(),
        }
        Ok(())
    }

    /// Keeps the search history beside the shell: save it when `search.history` is on,
    /// delete it when off (nothing is stored then).
    fn keep_history(&mut self) {
        let dir = self.history_dir.as_deref();
        let user = &self.shell.user;
        if self.shell.d.overview.sources.history {
            crate::model::history::save(&self.shell.d.overview.history, dir, user);
        } else {
            crate::model::history::delete(dir, user);
        }
    }

    /// A call to a system service; its answer goes into the state (outputs, a day's events)
    /// or becomes the shell's notice (saved screenshot, a service that could not).
    fn service(&mut self, call: &Call) {
        let Some(link) = &self.link else {
            return;
        };
        let now = self.now_ms();
        if call.is_slow() {
            if !self.slow.start(&link.ports, call.clone()) && !self.slow.runs(call) {
                self.shell
                    .d
                    .notes
                    .post_notice(SHELL, "desk.service.failed", now);
            }
            return;
        }
        let answer = link.ports.call(call);
        self.take_answer(call, answer, now);
    }

    /// Puts an answer into the state (or the shell's notice).
    fn take_answer(&mut self, call: &Call, answer: Result<Answer, PortError>, now: u64) {
        let d = &mut self.shell.d;
        match answer {
            Ok(Answer::Done) => {}
            Ok(Answer::Outputs(outputs)) => d.svc.set_outputs(outputs),
            Ok(Answer::BtDevices(devices)) => d.svc.set_bt_devices(devices),
            Ok(Answer::Networks(networks)) => d.svc.wifi.set_networks(networks),
            Ok(Answer::Known(ssids)) => d.svc.wifi.set_known(ssids),
            Ok(Answer::Joined(_)) => {}
            Ok(Answer::Sent(outcome)) => crate::model::mesh::sent(d, outcome, now),
            Ok(Answer::Helper(answer)) => {
                // asked in the helper's sheet: the answer goes there, and into a
                // notification only when the sheet was closed meanwhile
                let asked = match call {
                    Call::AskHelper(text) => text.as_str(),
                    _ => "",
                };
                let taken = d.overview.helper.answer(asked, answer.clone());
                if taken && d.open == Open::Helper {
                    return;
                }
                // an answer thought in the internet says so in its title
                let cloud = answer.brain == ultar_bus::messages::agent::Brain::Cloud;
                let title = if cloud {
                    "desk.helper.title.cloud"
                } else {
                    "desk.helper.title"
                };
                d.notes.post_said(SHELL, title, answer.text, now);
            }
            Ok(Answer::NotDone) | Err(_) if matches!(call, Call::AskHelper(_)) => {
                let asked = match call {
                    Call::AskHelper(text) => text.as_str(),
                    _ => "",
                };
                if d.overview.helper.failed(asked) && d.open == Open::Helper {
                    return;
                }
                d.notes.post_notice(SHELL, "desk.helper.failed", now);
            }
            Ok(Answer::NotDone) | Err(_) if *call == Call::Networks => {
                d.svc.wifi.set_networks(Vec::new());
            }
            Ok(Answer::Day(events)) => {
                if let Call::Day(y, m, day) = call {
                    d.svc.set_day((*y, *m, *day), events);
                }
            }
            Ok(Answer::Saved(_)) => d.notes.post_notice(SHELL, "desk.service.shot_saved", now),
            Ok(Answer::NotDone) | Err(_) => {
                d.notes.post_notice(SHELL, "desk.service.failed", now);
            }
        }
    }

    /// The slow calls that finished: their answers into the state, then a rebuild.
    pub(super) fn finish_slow(&mut self) -> Result<(), AppError> {
        for (call, answer) in self.slow.finished() {
            let now = self.now_ms();
            let outcome = match (&call, answer) {
                (Call::Join(ssid, ..), answer) => {
                    let outcome = match answer {
                        Ok(Answer::Joined(o)) => Some(o),
                        _ => None,
                    };
                    let (out, say) = self.shell.joined(ssid, outcome);
                    if say {
                        self.shell
                            .d
                            .notes
                            .post_notice(SHELL, "desk.wifi.join.failed_notice", now);
                    }
                    out
                }
                (_, answer) => {
                    self.take_answer(&call, answer, now);
                    Outcome::surfaces()
                }
            };
            self.apply(outcome)?;
        }
        Ok(())
    }

    /// Takes the typed password out of the join dialog's field (the field is emptied; its
    /// buffer is wiped by the toolkit).
    pub(super) fn take_password(&mut self, id: SurfaceId) -> ultar_bus::messages::auth::Secret {
        let Some(view) = self.host.view_mut(id) else {
            return ultar_bus::messages::auth::Secret::default();
        };
        let Some(field) = view.node(&crate::view::Target::JoinPassword) else {
            return ultar_bus::messages::auth::Secret::default();
        };
        let secret = view.ui.text(field).unwrap_or_default().to_owned();
        let _ = view.ui.set_text(field, "");
        ultar_bus::messages::auth::Secret::new(secret)
    }

    /// Starts the waiting app when its token came or its wait is over.
    pub(super) fn launch_due(&mut self, token: Option<u64>) -> Result<(), AppError> {
        let due = self
            .pending
            .as_ref()
            .is_some_and(|(_, _, deadline)| token.is_some() || self.now_ms() >= *deadline);
        if !due {
            return Ok(());
        }
        let now = self.now_ms();
        if let (Some((app, open, _)), Some(link)) = (self.pending.take(), &self.link) {
            match link.ports.start(&app, &open, token.filter(|t| *t != 0)) {
                Ok(()) => {}
                // The app cannot take that place: say so (the launcher stays usable).
                Err(PortError::Refused) => {
                    self.shell.d.notes.post_local(&app, now);
                    self.rebuild(Outcome::surfaces())?;
                }
                Err(PortError::Unavailable) => {
                    self.shell.d.presence.launcher = false;
                    self.rebuild(Outcome::surfaces())?;
                }
            }
        }
        Ok(())
    }

    /// Super+P chose a mode: through the settings service ("keep?" follows from there);
    /// the compositor gets it when the service announces the change.
    fn choose_display_mode(&mut self, mode: usize) {
        let Some(link) = &self.link else {
            return;
        };
        let (first, second) = mode_outputs(&self.shell.d.outputs);
        let id_of = |o: Option<ultar_surface::proto::OutputId>| {
            o.and_then(|o| {
                self.displays
                    .iter()
                    .find(|(id, _)| *id == o)
                    .map(|(_, s)| s.clone())
            })
        };
        let (word, single) = match mode {
            0 => ("single", id_of(first)),
            1 => ("mirror", None),
            3 => ("single", id_of(second)),
            _ => ("extend", None),
        };
        if word == "single" {
            let Some(single) = single else {
                return;
            };
            let _ = link
                .ports
                .set("display.single_display", SettingValue::Text(single));
        }
        let _ = link
            .ports
            .set("display.mode", SettingValue::Text(word.into()));
    }

    /// The settings service announced the display mode: the compositor follows.
    pub(super) fn apply_display_mode(&mut self) {
        let Some(link) = &self.link else {
            return;
        };
        let word = match link.ports.get("display.mode") {
            Ok(SettingValue::Text(t)) => t,
            _ => return,
        };
        let mode = match word.as_str() {
            "mirror" => DisplayModeRequest::Mirror,
            "single" => {
                let single = match link.ports.get("display.single_display") {
                    Ok(SettingValue::Text(t)) => t,
                    _ => String::new(),
                };
                match self.displays.iter().find(|(_, s)| *s == single) {
                    Some((o, _)) => DisplayModeRequest::Single(*o),
                    None => return,
                }
            }
            _ => DisplayModeRequest::Extend,
        };
        let _ = self.host.client.send(&Request::SetDisplayMode { mode });
    }

    /// Tells the settings service which displays are connected (the shell is the bridge
    /// between the compositor's outputs and `display.<id>.*`).
    pub(super) fn report_displays(&mut self) {
        let Some(link) = &self.link else {
            return;
        };
        let outputs = self.shell.d.outputs.clone();
        let descriptors: Vec<DisplayDescriptor> = outputs
            .iter()
            .map(|o| DisplayDescriptor {
                connector: o.name.to_ascii_lowercase(),
                internal: internal(o),
                width_px: o.width,
                height_px: o.height,
                refresh_mhz: o.refresh_millihertz,
                ..DisplayDescriptor::default()
            })
            .collect();
        if let Ok(ids) = link.ports.set_displays(&descriptors) {
            self.displays = outputs.iter().map(|o| o.id).zip(ids).collect();
            let main = super::primary(&outputs).map(|o| o.id);
            self.shell.d.svc.main_display = self
                .displays
                .iter()
                .find(|(id, _)| Some(*id) == main)
                .map(|(_, s)| s.clone());
            if let Some(key) = super::link::brightness_key(&self.shell.d) {
                if let Ok(value) = link.ports.get(&key) {
                    super::link::apply_setting(&mut self.shell.d, &key, &value);
                }
            }
        }
    }

    /// Builds the bar and the popup again as the outcome says.
    pub(super) fn rebuild(&mut self, outcome: Outcome) -> Result<(), AppError> {
        self.look.theme = self.shell.d.theme;
        let v = self.shell.d.values;
        self.look = self.look.with_access(v.high_contrast, v.large_text);
        self.look.reduce_motion = v.reduce_motion;
        let screen = screen(&self.host);
        if outcome.bar {
            let view = build_bar(&self.shell, self.look, screen.w)?;
            self.host.replace(self.bar, view, None);
            self.rebuild_side_panels(screen)?;
        }
        if outcome.popup {
            self.sync_popup(screen)?;
        }
        self.sync_banners()
    }

    fn sync_popup(&mut self, screen: ultar_ui::geom::Size) -> Result<(), AppError> {
        let open = self.shell.d.open;
        let built = if open == Open::None {
            None
        } else {
            build_popup(&self.shell, self.look, screen)?
        };
        let Some(popup) = built else {
            if let Some(old) = self.popup.take() {
                self.host.close(old.id)?;
                self.host.client.send(&Request::ShellFocus {
                    surface: SurfaceId(0),
                })?;
            }
            self.shell.d.open = Open::None;
            return Ok(());
        };
        match self.popup {
            Some(p) if same_surface(p.open, open) && p.offset == popup.offset => {
                let mut view = popup.view;
                if let Some(old) = self.host.view_mut(p.id) {
                    carry_password(old, &mut view);
                }
                self.host.replace(p.id, view, Some(popup.size));
            }
            _ => {
                if let Some(old) = self.popup.take() {
                    self.host.close(old.id)?;
                }
                let role = Role::Popup {
                    parent: self.bar,
                    x: popup.offset.0,
                    y: popup.offset.1,
                };
                let config = WindowConfig::with_role(role, "", popup.translucent);
                let id = self.host.open(&config, popup.view, Some(popup.size))?;
                self.popup = Some(PopupSurface {
                    id,
                    open,
                    offset: popup.offset,
                });
            }
        }
        if let Some(p) = self.popup {
            self.host
                .client
                .send(&Request::ShellFocus { surface: p.id })?;
        }
        Ok(())
    }
}
