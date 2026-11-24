//! Running the shell's effects and keeping the surfaces in step with its state.

use super::{screen, DesktopApp, PopupSurface, TOKEN_WAIT_MS};
use crate::app::AppError;
use crate::app::{internal, mode_outputs};
use crate::model::Open;
use crate::ports::{LauncherPort, LockPort, SettingsPort};
use crate::shell::{build_bar, build_popup, Effect, Outcome};
use ultar_bus::messages::settings::{DisplayDescriptor, SettingValue};
use ultar_surface::client::WindowConfig;
use ultar_surface::proto::{DisplayModeRequest, Request, Role, SurfaceId};

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
            Effect::Launch { app, .. } => {
                // The click's activation token first; the launch follows its answer.
                let asked = self.host.request_activation()?;
                let deadline = self.now_ms() + if asked { TOKEN_WAIT_MS } else { 0 };
                self.pending = Some((app.clone(), deadline));
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
                if let Some(link) = &self.link {
                    let _ = link.ports.lock();
                }
            }
            Effect::Layout(index) => {
                self.host
                    .client
                    .send(&Request::SetKeyboardLayout { index: *index })?;
            }
        }
        Ok(())
    }

    /// Starts the waiting app when its token came or its wait is over.
    pub(super) fn launch_due(&mut self, token: Option<u64>) -> Result<(), AppError> {
        let due = self
            .pending
            .as_ref()
            .is_some_and(|(_, deadline)| token.is_some() || self.now_ms() >= *deadline);
        if !due {
            return Ok(());
        }
        if let (Some((app, _)), Some(link)) = (self.pending.take(), &self.link) {
            if link.ports.start(&app, token.filter(|t| *t != 0)).is_err() {
                self.shell.d.presence.launcher = false;
                self.rebuild(Outcome::surfaces())?;
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
            self.shell.d.main_display = self
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
        let screen = screen(&self.host);
        if outcome.bar {
            let view = build_bar(&self.shell, self.look, screen.w)?;
            self.host.replace(self.bar, view, None);
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
            Some(p) if p.open == open && p.offset == popup.offset => {
                self.host.replace(p.id, popup.view, Some(popup.size));
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
