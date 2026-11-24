//! The installer app: model, services and widget tree; what each click does (CLICKS.md
//! `installer.*`). Writing to a disk is not done here: [`InstallerApp::take_install`] hands
//! the confirmed plan to the caller, which runs [`crate::engine::write::execute`].

use crate::action::Act;
use crate::components::{self, Effect, Scope};
use crate::engine::{EngineError, EraseConfirmation, Plan};
use crate::model::Model;
use crate::ports::{Devices, PlanStore};
use crate::view::{self, View};
use crate::wizard::{DiskMode, Encryption, Step, Wizard};
use ultar_bus::{Text, TextArg};
use ultar_i18n::Lang;
use ultar_ui::event::{Action, ActionKind, Event, Response};
use ultar_ui::ui::{Ui, UiError};
use ultar_ui::widgets::{Toast, Tone};

/// How the installer ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// Cancelled (or "try"): back to the live session; nothing was written.
    Cancelled,
    /// Installed; restart into the new system.
    Restart,
}

/// The installer.
pub struct InstallerApp {
    /// Widget tree.
    pub ui: Ui,
    /// Handles.
    pub view: View,
    /// State.
    pub model: Model,
    devices: Box<dyn Devices>,
    plans: Box<dyn PlanStore>,
    install: Option<(EraseConfirmation, Plan)>,
    exit: Option<Exit>,
    toast: u32,
}

impl std::fmt::Debug for InstallerApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstallerApp")
            .field("model", &self.model)
            .finish()
    }
}

const LANGS: [Lang; 3] = [Lang::Ar, Lang::En, Lang::De];

impl InstallerApp {
    /// Builds the first step.
    pub fn new(
        ui: Ui,
        wizard: Wizard,
        devices: Box<dyn Devices>,
        plans: Box<dyn PlanStore>,
    ) -> Result<InstallerApp, UiError> {
        let mut app = InstallerApp {
            ui,
            view: View::default(),
            model: Model {
                wizard,
                ..Model::default()
            },
            devices,
            plans,
            install: None,
            exit: None,
            toast: 0,
        };
        app.model.wizard.prefs.lang = LANGS
            .iter()
            .position(|l| *l == app.ui.env().lang)
            .unwrap_or(0);
        app.rebuild()?;
        Ok(app)
    }

    /// Rebuilds the window from the model.
    pub fn rebuild(&mut self) -> Result<(), UiError> {
        view::build(&mut self.ui, &self.model, &mut self.view)
    }

    /// Feeds one input event.
    pub fn handle(&mut self, event: Event) -> Result<Response, UiError> {
        let response = self.ui.handle(event);
        self.actions(response.actions.clone())?;
        Ok(response)
    }

    /// Handles toolkit actions.
    pub fn actions(&mut self, actions: Vec<Action>) -> Result<(), UiError> {
        for a in actions {
            self.action(a)?;
        }
        Ok(())
    }

    /// How the installer ended, once it did.
    pub fn exit(&self) -> Option<Exit> {
        self.exit
    }

    /// The confirmed installation, once (the caller writes it).
    pub fn take_install(&mut self) -> Option<(EraseConfirmation, Plan)> {
        self.install.take()
    }

    /// Progress of the writing.
    pub fn progress(&mut self, done: u64, total: u64) -> Result<(), UiError> {
        self.model.progress = (done, total);
        if let Some(bar) = self.view.progress {
            let v = if total == 0 {
                0.0
            } else {
                done as f32 / total as f32
            };
            self.ui.set_progress(bar, Some(v.clamp(0.0, 1.0)))?;
        }
        Ok(())
    }

    /// The writing ended.
    pub fn finished(&mut self, result: Result<(), EngineError>) -> Result<(), UiError> {
        match result {
            Ok(()) => self.model.wizard.finished(),
            Err(e) => self.model.failure = Some(Text::key(e.key())),
        }
        self.rebuild()
    }

    fn say(&mut self, tone: Tone, text: Text) -> Result<(), UiError> {
        self.toast = self.toast.wrapping_add(1);
        let label = crate::i18n::label(&text, self.ui.translator());
        self.ui.show_toast(Toast::new(self.toast, tone, label))
    }

    fn action(&mut self, a: Action) -> Result<(), UiError> {
        if Some(a.node) == self.view.steps {
            if let ActionKind::Selected(i) = a.kind {
                if let Some(&step) = self.view.step_items.get(i) {
                    if self.model.wizard.jump(step) {
                        return self.entered();
                    }
                }
                return self.rebuild();
            }
            return Ok(());
        }
        let Some(act) = self.view.registry.act(a.node) else {
            return Ok(());
        };
        self.act(act, a.kind, a.node)
    }

    fn act(
        &mut self,
        act: Act,
        kind: ActionKind,
        node: ultar_ui::ui::NodeId,
    ) -> Result<(), UiError> {
        use ActionKind::{Activated, Toggled};
        let w = &mut self.model.wizard;
        match (act, kind) {
            (Act::Cancel, Activated) => {
                self.model.cancel_dialog = true;
                self.rebuild()
            }
            (Act::CancelNo, Activated) => {
                self.model.cancel_dialog = false;
                self.rebuild()
            }
            (Act::CancelYes, Activated) => {
                self.exit = Some(Exit::Cancelled);
                Ok(())
            }
            (Act::Prev, Activated) => {
                w.prev();
                self.entered()
            }
            (Act::Next, Activated) => self.next(),
            (Act::Edit(step), Activated) => {
                w.edit(step);
                self.entered()
            }
            (Act::Confirm, Toggled(on)) => {
                w.erase_confirmed = on;
                view::footer_state(&mut self.ui, &self.model, &self.view)
            }
            (Act::Restart, Activated) => {
                self.exit = Some(Exit::Restart);
                Ok(())
            }
            (Act::DriversPlan(row), Activated) => self.plan(row),
            (Act::DriversAlt(row), ActionKind::Selected(i)) if i > 0 => self.try_driver(row, i - 1),
            (Act::DriversTest(row), Activated) => self.test(row),
            (Act::Component(c), Toggled(on)) => self.component(c, on),
            (Act::EncPass(_), ActionKind::TextChanged) => self.passphrases(),
            _ => self.entry(act, kind, node),
        }
    }

    /// Entries of the pages (choices, switches, fields).
    fn entry(
        &mut self,
        act: Act,
        kind: ActionKind,
        node: ultar_ui::ui::NodeId,
    ) -> Result<(), UiError> {
        use ActionKind::{Selected, ValueChanged};
        let w = &mut self.model.wizard;
        match (act, kind) {
            (Act::Scope, Selected(i)) => {
                w.scope = Scope::ALL.get(i).copied().unwrap_or_default();
                w.components = components::of_scope(w.scope, &w.components);
                self.rebuild()
            }
            (Act::DiskSelect, Selected(i)) => {
                w.choose_disk(i);
                self.rebuild()
            }
            (Act::DiskMode, Selected(i)) => {
                w.disk_mode = [DiskMode::Whole, DiskMode::Beside, DiskMode::Manual]
                    .get(i)
                    .copied()
                    .unwrap_or_default();
                self.rebuild()
            }
            (Act::DiskShare, ValueChanged(v)) => {
                w.share = v.round().clamp(0.0, 100.0) as u8;
                Ok(())
            }
            (Act::EncMode, Selected(i)) => {
                w.encryption = [Encryption::None, Encryption::Passphrase, Encryption::Tpm]
                    .get(i)
                    .copied()
                    .unwrap_or_default();
                self.rebuild()
            }
            _ => self.prefs_entry(act, kind, node),
        }
    }

    /// Entries of the steps without a board (language … time zone, account).
    fn prefs_entry(
        &mut self,
        act: Act,
        kind: ActionKind,
        node: ultar_ui::ui::NodeId,
    ) -> Result<(), UiError> {
        use ActionKind::{Selected, TextChanged, Toggled};
        let w = &mut self.model.wizard;
        match (act, kind) {
            (Act::Lang, Selected(i)) => self.language(i),
            (Act::Access(i) | Act::Extra(i), Toggled(on)) => {
                let slots = if matches!(act, Act::Access(_)) {
                    &mut w.prefs.access
                } else {
                    &mut w.prefs.extras
                };
                if let Some(slot) = slots.get_mut(i) {
                    *slot = on;
                }
                Ok(())
            }
            (Act::Keyboard, Selected(i)) => {
                w.prefs.keyboard = i;
                Ok(())
            }
            (Act::TimeZone, Selected(i)) => {
                w.prefs.timezone = i;
                Ok(())
            }
            (Act::TryOrInstall, Selected(i)) => {
                w.prefs.install = i == 1;
                view::footer_state(&mut self.ui, &self.model, &self.view)
            }
            (Act::Kind, Selected(i)) => {
                w.prefs.server = i == 1;
                Ok(())
            }
            (Act::AccountField(i), TextChanged) => {
                let text = self.ui.text(node).map(str::to_owned).unwrap_or_default();
                if let Some(slot) = self.model.wizard.account.get_mut(i) {
                    *slot = text;
                }
                view::footer_state(&mut self.ui, &self.model, &self.view)
            }
            _ => Ok(()),
        }
    }

    fn next(&mut self) -> Result<(), UiError> {
        let w = &mut self.model.wizard;
        if w.step == Step::TryOrInstall && !w.prefs.install {
            self.exit = Some(Exit::Cancelled);
            return Ok(());
        }
        if w.step == Step::Summary {
            return match w.confirm_install() {
                Ok(pending) => {
                    self.install = Some(pending);
                    self.rebuild()
                }
                Err(reason) => self.say(Tone::Warning, Text::key(reason)),
            };
        }
        match w.advance() {
            Ok(()) => self.entered(),
            Err(reason) => self.say(Tone::Warning, Text::key(reason)),
        }
    }

    /// A step was entered: load what it shows.
    fn entered(&mut self) -> Result<(), UiError> {
        let w = &mut self.model.wizard;
        match w.step {
            Step::Drivers => self.load_devices(),
            Step::Disk if w.disk.is_none() && !w.disks.is_empty() => w.choose_disk(0),
            _ => {}
        }
        self.rebuild()
    }

    /// Reads the device list (step 7).
    pub fn load_devices(&mut self) {
        match self.devices.list() {
            Ok(list) => {
                self.model.devices = list;
                self.model.devices_error = None;
            }
            Err(e) => self.model.devices_error = Some(e.text()),
        }
    }

    fn component(&mut self, c: components::Component, on: bool) -> Result<(), UiError> {
        let w = &mut self.model.wizard;
        w.scope = Scope::Custom;
        let effect = components::toggle(&mut w.components, c, on);
        self.rebuild()?;
        match effect {
            Effect::RemovedFirefox => {
                self.say(Tone::Info, Text::key("installer.hint.firefox_removed"))
            }
            Effect::AddedLinux => self.say(Tone::Info, Text::key("installer.hint.linux_added")),
            Effect::None => Ok(()),
        }
    }

    fn passphrases(&mut self) -> Result<(), UiError> {
        if let [a, b] = self.view.passes[..] {
            let differ = self.ui.text(a).ok() != self.ui.text(b).ok()
                && !self.ui.text(b).unwrap_or("").is_empty();
            let error = differ.then(|| ultar_ui::widgets::Label::key("installer.enc.differ"));
            self.ui.set_error(b, error)?;
        }
        Ok(())
    }

    fn language(&mut self, i: usize) -> Result<(), UiError> {
        let lang = LANGS.get(i).copied().unwrap_or(Lang::Ar);
        self.model.wizard.prefs.lang = i;
        self.ui.set_lang(lang);
        self.ui.set_direction(lang.direction());
        self.rebuild()
    }

    fn plan(&mut self, row: Option<usize>) -> Result<(), UiError> {
        let devices: Vec<_> = match row {
            Some(i) => self.model.devices.get(i).cloned().into_iter().collect(),
            None => self.model.devices.clone(),
        };
        let mut text = String::from("# Ultar OS hardware plan\nformat = 1\n");
        for d in &devices {
            let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
            text.push_str(&format!(
                "\n[[device]]\naddress = \"{}\"\nhardware_id = \"{}\"\n",
                q(&d.address),
                q(&d.hardware_id)
            ));
            if let Some(l) = &d.label {
                text.push_str(&format!("label = \"{}\"\n", q(l)));
            }
        }
        let name = match row {
            Some(i) => format!("hardware-plan-{i}.toml"),
            None => "hardware-plan.toml".to_owned(),
        };
        match self.plans.save(&name, text.as_bytes()) {
            Ok(path) => self.say(
                Tone::Success,
                Text::key("installer.drivers.plan.saved").with(TextArg::Verbatim(path)),
            ),
            Err(e) => self.say(Tone::Danger, e.text()),
        }
    }

    fn try_driver(&mut self, row: usize, alt: usize) -> Result<(), UiError> {
        let Some(d) = self.model.devices.get(row).cloned() else {
            return Ok(());
        };
        let active = d.active.as_ref().map(|a| a.id.clone());
        let Some(target) = d
            .alternatives
            .iter()
            .filter(|a| Some(&a.id) != active.as_ref())
            .nth(alt)
            .cloned()
        else {
            return Ok(());
        };
        match self.devices.switch(&d.address, &target.id, &target.version) {
            Ok(info) => {
                self.model.devices[row] = info;
                self.rebuild()?;
                self.say(Tone::Success, Text::key("installer.drivers.tried"))
            }
            Err(e) => self.say(Tone::Danger, e.text()),
        }
    }

    fn test(&mut self, row: usize) -> Result<(), UiError> {
        let Some(address) = self.model.devices.get(row).map(|d| d.address.clone()) else {
            return Ok(());
        };
        match self.devices.test(&address) {
            Ok(info) => {
                let (tone, key) = crate::view::drivers_badge(&info);
                self.model.devices[row] = info;
                self.rebuild()?;
                self.say(tone, Text::key(key))
            }
            Err(e) => self.say(Tone::Danger, e.text()),
        }
    }
}
