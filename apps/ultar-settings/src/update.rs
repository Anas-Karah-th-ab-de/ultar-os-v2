//! What each click does (docs/ui/CLICKS.md): action → state change + service calls. No
//! drawing here; the [`Outcome`] tells the app what to rebuild.

mod account;
mod bluetooth;
mod cloud;
mod display;
mod drivers;
mod mesh;
mod more;
mod panel;

pub use drivers::offered as offered_drivers;

use crate::action::Act;
use crate::areas::{PageKind, AREAS};
use crate::service::{PortError, Ports};
use crate::state::{DialogPurpose, DialogState, State, SubPage};
use crate::view::View;
use ultar_bus::messages::settings::SettingValue;
use ultar_bus::Text;
use ultar_ui::event::ActionKind;
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::Tone;

/// What the view must do after an action.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Outcome {
    /// Rebuild the whole window (compact list ↔ page).
    pub rebuild_all: bool,
    /// Rebuild the content pane.
    pub rebuild: bool,
    /// Rebuild the navigation list.
    pub nav: bool,
    /// Open (or close) the dialog of `state.dialog`.
    pub dialog: bool,
    /// A short message.
    pub toast: Option<(Tone, Text)>,
}

impl Outcome {
    fn rebuild() -> Outcome {
        Outcome {
            rebuild: true,
            ..Outcome::default()
        }
    }

    fn toast(tone: Tone, key: &str) -> Outcome {
        Outcome {
            toast: Some((tone, Text::key(key))),
            ..Outcome::default()
        }
    }

    fn error(error: &PortError) -> Outcome {
        Outcome {
            toast: Some((Tone::Danger, error.text())),
            ..Outcome::default()
        }
    }

    fn dialog() -> Outcome {
        Outcome {
            dialog: true,
            ..Outcome::default()
        }
    }
}

/// Everything an action may touch.
pub struct Ctl<'a> {
    /// State.
    pub state: &'a mut State,
    /// Services.
    pub ports: &'a mut Ports,
    /// The widget tree (read values, update in place).
    pub ui: &'a mut Ui,
    /// Handles of the tree.
    pub view: &'a View,
}

/// Loads everything the open page shows.
pub fn load_page(state: &mut State, ports: &mut Ports) {
    if state.page() == crate::areas::PageKind::About {
        state.more.release = ports.os_release.as_deref().and_then(read_release);
    }
    load_panels(state, ports);
    match state.page() {
        PageKind::Display => display::load(state, ports),
        PageKind::Drivers => drivers::load(state, ports),
        PageKind::Account => account::load(state, ports),
        PageKind::Mesh => mesh::load(state, ports),
        PageKind::Status(kind) => load_status(state, ports, kind),
        PageKind::Panel(id) => {
            more::load(state, ports, id);
            if id == cloud::PANEL {
                cloud::load(state, ports);
            }
        }
        PageKind::NoService | PageKind::About => {}
    }
    report_page(state);
}

/// Reads the state of the service a status page shows (its retained topic).
pub fn load_status(state: &mut State, ports: &mut Ports, kind: crate::areas::StatusKind) {
    use crate::areas::StatusKind;
    let s = &mut state.status;
    let result = match kind {
        StatusKind::Network => ports.status.net().map(|v| s.net = Some(v)),
        StatusKind::Bluetooth => ports.status.bluetooth().map(|v| {
            s.bt_searched |= v.scanning;
            bluetooth::load_devices(&mut s.bt_devices, &v, ports);
            s.bluetooth = Some(v)
        }),
        StatusKind::Sound => ports.status.audio().map(|v| s.audio = Some(v)),
    };
    s.error = result.err().map(|e| e.text());
}

/// One line on standard error (the launcher stores it in the log as `app-settings`) for every page
/// opened: which area, and the key of the error it shows, if any (`ok` otherwise). Never a value.
fn report_page(state: &State) {
    let area = crate::areas::AREAS.get(state.area).map_or("?", |a| a.id);
    let error = match state.page() {
        PageKind::Status(_) => state.status.error.as_ref(),
        PageKind::NoService => None,
        _ => state.panels_error.as_ref(),
    };
    let verdict = match (state.page(), error) {
        (PageKind::NoService, _) => "no_service",
        (_, Some(e)) => e.key.as_str(),
        (_, None) => "ok",
    };
    eprintln!("ultar-settings: page {area} {verdict}");
}

/// Cheap refresh while the app runs: display report and update progress (no list calls).
pub fn load_page_light(state: &mut State, ports: &mut Ports) {
    match state.page() {
        PageKind::Display => display::load(state, ports),
        PageKind::Drivers | PageKind::Panel("updates") => {
            state.drivers.firmware = ports.update.progress()
        }
        PageKind::Status(kind) => load_status(state, ports, kind),
        PageKind::Mesh => mesh::load(state, ports),
        _ => {}
    }
}

/// Reads every settingsd panel (generic pages, search, Displays).
pub fn load_panels(state: &mut State, ports: &mut Ports) {
    match ports.settings.list(None) {
        Ok(panels) => {
            state.display.apply_panels(&panels);
            state.panels = panels;
            state.panels_error = None;
        }
        Err(e) => {
            state.panels_error = Some(e.text());
            state.display.settings_known = false;
        }
    }
}

/// Handles one toolkit action of `node`.
pub fn handle(c: &mut Ctl<'_>, node: NodeId, kind: ActionKind) -> Outcome {
    if Some(node) == c.view.nav {
        return match kind {
            ActionKind::Selected(i) => match c.view.nav_areas.get(i) {
                Some(&area) => open_area(c.state, c.ports, area),
                None => Outcome::default(),
            },
            _ => Outcome::default(),
        };
    }
    let Some(act) = c.view.act(node) else {
        return Outcome::default();
    };
    match act {
        Act::Search => search(c, node, kind),
        Act::Area(area) if kind == ActionKind::Activated => open_area(c.state, c.ports, area),
        Act::Back => {
            c.state.compact_list = true;
            Outcome {
                rebuild_all: true,
                ..Outcome::default()
            }
        }
        Act::PanelItem(i) => panel::item(c, i, node, kind),
        Act::DialogOk | Act::DialogCancel | Act::DialogField(_) => dialog(c, act, kind),
        a if display::owns(a) => display::handle(c, a, node, kind),
        a if drivers::owns(a) => drivers::handle(c, a, kind),
        a if more::owns(a) => more::handle(c, a, kind),
        a if account::owns(a) => account::handle(c, a, node, kind),
        a if bluetooth::owns(a) => bluetooth::handle(c, a, kind),
        a if mesh::owns(a) => mesh::handle(c, a, kind),
        a if cloud::owns(a) => cloud::handle(c, a, kind),
        _ => Outcome::default(),
    }
}

/// Opens an area (sidebar click, search result).
pub fn open_area(state: &mut State, ports: &mut Ports, area: usize) -> Outcome {
    if area >= AREAS.len() {
        return Outcome::default();
    }
    let was_list = state.compact_list;
    if state.area != area {
        state.prev_area = Some(state.area);
    }
    state.area = area;
    state.sub = SubPage::None;
    state.dialog = None;
    state.compact_list = false;
    load_page(state, ports);
    Outcome {
        rebuild_all: was_list,
        rebuild: true,
        ..Outcome::default()
    }
}

fn search(c: &mut Ctl<'_>, node: NodeId, kind: ActionKind) -> Outcome {
    match kind {
        ActionKind::TextChanged => {
            c.state.query = c.ui.text(node).map(str::to_owned).unwrap_or_default();
            Outcome {
                nav: true,
                ..Outcome::default()
            }
        }
        ActionKind::Submitted => match c.view.nav_areas.first() {
            Some(&area) => open_area(c.state, c.ports, area),
            None => Outcome::toast(Tone::Info, "app.settings.search.none"),
        },
        _ => Outcome::default(),
    }
}

/// Stores one setting; on refusal the page is rebuilt from the stored values.
pub(crate) fn set(c: &mut Ctl<'_>, key: &str, value: SettingValue) -> Outcome {
    match c.ports.settings.set(key, value) {
        Ok(()) => Outcome::default(),
        Err(e) => {
            load_panels(c.state, c.ports);
            Outcome {
                rebuild: true,
                ..Outcome::error(&e)
            }
        }
    }
}

/// Opens a dialog.
pub(crate) fn ask(state: &mut State, purpose: DialogPurpose) -> Outcome {
    state.dialog = Some(DialogState {
        purpose,
        error: None,
    });
    Outcome::dialog()
}

/// Closes the dialog.
pub(crate) fn close(state: &mut State) -> Outcome {
    state.dialog = None;
    Outcome::dialog()
}

/// The texts typed into the open dialog's fields.
pub(crate) fn fields(c: &Ctl<'_>) -> Vec<String> {
    c.view
        .dialog_fields
        .iter()
        .map(|f| c.ui.text(*f).map(str::to_owned).unwrap_or_default())
        .collect()
}

fn dialog(c: &mut Ctl<'_>, act: Act, kind: ActionKind) -> Outcome {
    if kind != ActionKind::Activated && kind != ActionKind::Submitted {
        return Outcome::default();
    }
    if c.state.sub == SubPage::CreateAccount {
        return account::create_page(c, act);
    }
    let Some(purpose) = c.state.dialog.as_ref().map(|d| d.purpose) else {
        return Outcome::default();
    };
    if act == Act::DialogCancel || kind == ActionKind::Closed {
        if let DialogPurpose::DriverSwitch { .. } = purpose {
            let out = close(c.state);
            return Outcome {
                rebuild: true,
                ..out
            };
        }
        return close(c.state);
    }
    if matches!(act, Act::DialogField(_)) && kind != ActionKind::Submitted {
        return Outcome::default();
    }
    match purpose {
        DialogPurpose::DriverSwitch { row, alt } => drivers::switch(c, row, alt),
        DialogPurpose::FirmwareUsb => drivers::usb(c),
        DialogPurpose::Qr => close(c.state),
        DialogPurpose::UserAdd | DialogPurpose::UserRemove(_) | DialogPurpose::UserPassword(_) => {
            more::dialog_ok(c, purpose)
        }
        DialogPurpose::MeshPairShow | DialogPurpose::MeshPairJoin | DialogPurpose::MeshUnpair => {
            mesh::dialog_ok(c, purpose)
        }
        DialogPurpose::CloudKey
        | DialogPurpose::CloudClear
        | DialogPurpose::CloudAddress
        | DialogPurpose::HelperForget => cloud::dialog_ok(c, purpose),
        DialogPurpose::ChangePassword
        | DialogPurpose::DeleteAccount
        | DialogPurpose::RemoveDevice(_) => account::dialog_ok(c, purpose),
    }
}

/// A dialog stays open and shows `key` as its error.
pub(crate) fn dialog_error(state: &mut State, text: Text) -> Outcome {
    if let Some(d) = state.dialog.as_mut() {
        d.error = Some(text);
    }
    Outcome::dialog()
}

/// `VERSION_ID` and `BUILD_ID` (or "—") of an os-release file (at most 4 KiB read).
pub fn read_release(path: &std::path::Path) -> Option<(String, String)> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(4096)
        .read_to_string(&mut text)
        .ok()?;
    let version = ultar_brand::release::field(&text, "VERSION_ID")?.to_owned();
    let build = ultar_brand::release::field(&text, "BUILD_ID")
        .unwrap_or("—")
        .to_owned();
    Some((version, build))
}
