//! The loop glue: one window (or the desktop surface) with its state, its file system view,
//! its ports and its background work. The binary feeds it actions and keys; tests do the
//! same without a compositor.

use crate::action::{Effect, Target};
use crate::fs::copy::Cancel;
use crate::fs::{trash, AreaId, Fs, FsError, Loc};
use crate::jobs::{self, JobMsg, ThumbMsg};
use crate::model::{Dialog, Drive, Item, Running, State};
use crate::service::{LauncherPort, Mounted, PortError, VolumesPort};
use crate::update;
use crate::view::{self, Built};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use ultar_ui::event::{Action, ActionKind, Event, Key};
use ultar_ui::ui::{Ui, UiError};
use ultar_ui::widgets::{Label, Thumbnail, Toast, Tone};

/// Which surface the app shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A Files window.
    Window,
    /// The desktop icons.
    Desk,
}

/// Id of the app's toast.
const TOAST: u32 = 1;

/// The services of the app.
pub struct Ports {
    /// The app launcher.
    pub launcher: Box<dyn LauncherPort>,
    /// The drive service.
    pub volumes: Box<dyn VolumesPort>,
}

/// One Files surface.
pub struct App {
    /// The widget tree (the binary swaps it into its window).
    pub ui: Ui,
    /// The state.
    pub state: State,
    pub(crate) fs: Arc<Fs>,
    pub(crate) root: Option<std::path::PathBuf>,
    pub(crate) ports: Ports,
    pub(crate) mode: Mode,
    pub(crate) built: Built,
    pub(crate) clock: fn() -> i64,
    pub(crate) job: Option<Receiver<JobMsg>>,
    pub(crate) thumbs: Option<Receiver<ThumbMsg>>,
    /// Make thumbnails on the calling thread (tests).
    pub inline_work: bool,
}

/// Seconds since 1970 from the system clock.
pub fn system_clock() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl App {
    /// An app over `fs` (`root`: where the areas are, to see new drives) showing `uri` (or
    /// the first place).
    pub fn new(
        ui: Ui,
        fs: Fs,
        root: Option<std::path::PathBuf>,
        ports: Ports,
        mode: Mode,
        uri: Option<&str>,
    ) -> Result<App, UiError> {
        let areas = fs.areas();
        let places: Vec<AreaId> = areas
            .iter()
            .filter(|a| !matches!(a, AreaId::Trash | AreaId::Drive(_) | AreaId::Desktop))
            .cloned()
            .collect();
        let has_trash = areas.contains(&AreaId::Trash);
        let first = match mode {
            Mode::Desk => Loc::root(AreaId::Desktop),
            Mode::Window => places
                .first()
                .cloned()
                .map_or(Loc::root(AreaId::Home), Loc::root),
        };
        let mut app = App {
            ui,
            state: State::new(places, has_trash, first),
            fs: Arc::new(fs),
            root,
            ports,
            mode,
            built: Built::default(),
            clock: system_clock,
            job: None,
            thumbs: None,
            inline_work: false,
        };
        app.poll_drives();
        if let Some(loc) = uri.and_then(|u| crate::uri::parse(u, &app.state.drives)) {
            app.state.folder = loc;
        }
        app.reload();
        app.rebuild()?;
        Ok(app)
    }

    /// Uses `clock` instead of the system clock (tests).
    pub fn with_clock(mut self, clock: fn() -> i64) -> App {
        self.clock = clock;
        self
    }

    /// The file system view.
    pub fn fs(&self) -> &Fs {
        &self.fs
    }

    /// What the nodes of the current tree mean.
    pub fn built(&self) -> &Built {
        &self.built
    }

    /// Lists the folder again (search and thumbnails included).
    pub fn reload(&mut self) {
        let s = &mut self.state;
        let listed = if s.in_trash() {
            list_trash(&self.fs)
        } else {
            self.fs
                .list(&s.folder)
                .map(|v| v.into_iter().map(item).collect())
        };
        match listed {
            Ok(items) => {
                let old = std::mem::replace(&mut s.items, items);
                for i in &mut s.items {
                    i.thumb = old
                        .iter()
                        .find(|o| o.entry == i.entry)
                        .and_then(|o| o.thumb.clone());
                }
                s.error = None;
            }
            Err(e) => {
                s.items.clear();
                s.error = Some(e);
            }
        }
        let names: Vec<String> = s.items.iter().map(|i| i.entry.name.clone()).collect();
        s.selected.retain(|n| names.contains(n));
        s.hits = (!s.query.trim().is_empty()).then(|| {
            s.items
                .iter()
                .filter(|i| crate::fs::search::matches(&self.fs, &s.folder, &i.entry, &s.query))
                .map(|i| i.entry.name.clone())
                .collect()
        });
        self.start_thumbnails();
    }

    fn start_thumbnails(&mut self) {
        let s = &self.state;
        let wanted: Vec<String> = s
            .items
            .iter()
            .filter(|i| {
                i.thumb.is_none()
                    && i.entry.kind == crate::fs::Kind::File
                    && crate::fs::thumb::is_picture_name(&i.entry.name)
            })
            .map(|i| i.entry.name.clone())
            .collect();
        if wanted.is_empty() || s.in_trash() {
            return;
        }
        let rx = jobs::thumbnails(self.fs.clone(), s.folder.clone(), wanted);
        if self.inline_work {
            while let Ok(msg) = rx.recv() {
                self.take_thumb(msg);
            }
        } else {
            self.thumbs = Some(rx);
        }
    }

    pub(crate) fn take_thumb(&mut self, (folder, name, p): ThumbMsg) -> bool {
        if folder != self.state.folder {
            return false;
        }
        let Some(item) = self.state.items.iter_mut().find(|i| i.entry.name == name) else {
            return false;
        };
        item.thumb = Thumbnail::new(p.width, p.height, p.rgba);
        true
    }

    /// Builds the tree again from the state (focus and toast restored).
    pub fn rebuild(&mut self) -> Result<(), UiError> {
        let focus = self
            .ui
            .focused()
            .and_then(|n| self.built.targets.get(&n).cloned());
        self.built = match self.mode {
            Mode::Window => {
                let compact = self.ui.size().w <= ultar_ui::tokens::layout::COMPACT_MAX;
                view::build(&mut self.ui, &self.state, compact, &self.fs)?
            }
            Mode::Desk => view::desk::build(&mut self.ui, &self.state, &self.fs)?,
        };
        if let Some(n) = focus.and_then(|t| self.built.node(&t)) {
            if self.state.dialog.is_none() {
                let _ = self.ui.focus(n);
            }
        }
        if let Some(n) = &self.state.notice {
            let label = Label::key_with(n.key, &[("name", &n.name)]);
            let label = match label {
                Label::Key { key, mut args } => {
                    args.push((
                        "count".to_owned(),
                        ultar_ui::widgets::LabelArg::Int(n.count),
                    ));
                    Label::Key { key, args }
                }
                other => other,
            };
            let mut toast = Toast::new(TOAST, Tone::Neutral, label);
            if n.undo {
                toast = toast.action(Label::key("files.cmd.undo"));
            }
            self.ui.show_toast(toast)?;
        }
        Ok(())
    }

    /// Handles the actions of one input event (`key_input`: it was a key press).
    pub fn actions(&mut self, actions: Vec<Action>, key_input: bool) -> Result<(), UiError> {
        let mut effects = Vec::new();
        let mut rebuild = false;
        let before = fingerprint(&self.state);
        for a in actions {
            let target = self.built.targets.get(&a.node).cloned();
            match (&target, &a.kind) {
                (_, ActionKind::ToastAction(_)) => effects.extend(update::command(
                    &mut self.state,
                    crate::action::Command::Undo,
                )),
                (_, ActionKind::ToastClosed(_)) => self.state.notice = None,
                (_, ActionKind::SelectionChanged | ActionKind::Selected(_))
                    if Some(a.node) == self.built.items =>
                {
                    self.read_selection()
                }
                (Some(Target::Search), ActionKind::TextChanged) => {
                    let text = self.ui.text(a.node).unwrap_or_default().to_owned();
                    effects.extend(update::search(&mut self.state, &text));
                }
                (Some(Target::RenameField), ActionKind::Submitted)
                | (Some(Target::DialogOk), ActionKind::Activated)
                    if matches!(self.state.dialog, Some(Dialog::Rename(_))) =>
                {
                    effects.extend(self.confirm_rename());
                    rebuild = true;
                }
                (Some(Target::PlacesMenu), kind) => {
                    effects.extend(self.places_action(a.node, kind)?)
                }
                (Some(Target::Items), ActionKind::RowActivated(row)) => {
                    if let Some(name) = self.built.rows.get(*row).cloned() {
                        effects.extend(update::open_item(&mut self.state, &name));
                    }
                }
                (
                    Some(Target::Item(_)),
                    ActionKind::DragEnded {
                        target: Some(to), ..
                    },
                ) => effects.extend(self.dropped(*to)),
                (Some(t), kind) => {
                    let e = match self.mode {
                        Mode::Window => update::action(&mut self.state, t, kind, key_input),
                        Mode::Desk => view::desk::action(&mut self.state, t, kind, key_input),
                    };
                    effects.extend(e);
                }
                (None, ActionKind::MenuChosen(_)) => {
                    let e = update::action(&mut self.state, &Target::Items, &a.kind, key_input);
                    effects.extend(e);
                    rebuild = true;
                }
                _ => {}
            }
        }
        rebuild |= fingerprint(&self.state) != before;
        self.finish(effects, rebuild)
    }

    fn finish(&mut self, effects: Vec<Effect>, rebuild: bool) -> Result<(), UiError> {
        let changed = !effects.is_empty() || rebuild;
        let focus_items = effects.contains(&Effect::FocusItems);
        self.apply(effects);
        if changed {
            self.rebuild()?;
        }
        if focus_items {
            if let Some(n) = self.built.items {
                let _ = self.ui.focus(n);
            }
        }
        Ok(())
    }

    /// Key presses the widgets did not use (shortcuts).
    pub fn keys(&mut self, events: Vec<Event>) -> Result<(), UiError> {
        let mut effects = Vec::new();
        for e in events {
            if let Event::Key { key, modifiers } = e {
                self.read_selection();
                let had_dialog = self.state.dialog.is_some();
                effects.extend(update::key(&mut self.state, key, modifiers));
                if key == Key::Escape
                    || had_dialog != self.state.dialog.is_some()
                    || self.state.notice.is_some()
                {
                    effects.push(Effect::FocusItems);
                }
            }
        }
        // the selection may have changed (select all, Escape): the tiles show it
        let changed = !effects.is_empty() || self.state.selected != self.selected_in_tree();
        self.finish(effects, changed)
    }

    fn selected_in_tree(&self) -> Vec<String> {
        self.built
            .item_nodes
            .iter()
            .filter(|(n, _)| self.ui.is_item_selected(*n).unwrap_or(false))
            .map(|(_, name)| name.clone())
            .collect()
    }

    pub(crate) fn read_selection(&mut self) {
        let names: Vec<String> = match self.state.look.view {
            _ if !self.built.item_nodes.is_empty() => self
                .built
                .item_nodes
                .iter()
                .filter(|(n, _)| self.ui.is_item_selected(*n).unwrap_or(false))
                .map(|(_, name)| name.clone())
                .collect(),
            _ => match self.built.items {
                Some(t) => self
                    .ui
                    .selected_rows(t)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|r| self.built.rows.get(r).cloned())
                    .collect(),
                None => Vec::new(),
            },
        };
        update::select(&mut self.state, names);
    }

    fn confirm_rename(&mut self) -> Vec<Effect> {
        let Some(field) = self.built.rename else {
            return Vec::new();
        };
        let text = self.ui.text(field).unwrap_or_default().to_owned();
        let effects = update::rename(&mut self.state, &text);
        if effects.is_empty() && self.state.dialog.is_some() {
            let _ = self
                .ui
                .set_error(field, Some(Label::key("files.error.bad_name")));
        }
        effects
    }
}

/// What the tree shows besides the selection (a change means: build it again).
fn fingerprint(s: &State) -> String {
    format!(
        "{:?}{:?}{:?}{:?}{:?}{:?}{}{}{}",
        s.folder,
        s.dialog,
        s.look.view,
        s.look.sort,
        s.notice,
        s.error,
        s.job.is_some(),
        s.edit.clipboard.is_some(),
        s.query
    )
}

fn item(entry: crate::fs::Entry) -> Item {
    Item {
        entry,
        trashed: None,
        thumb: None,
    }
}

fn list_trash(fs: &Fs) -> Result<Vec<Item>, FsError> {
    Ok(trash::list(fs)?
        .into_iter()
        .map(|t| Item {
            entry: crate::fs::Entry {
                name: t.id.clone(),
                kind: t.kind,
                size: t.size,
                modified: t.deleted,
                mode: 0,
            },
            trashed: Some(t),
            thumb: None,
        })
        .collect())
}

pub(crate) fn drive_of(m: &Mounted) -> Option<Drive> {
    let last = m.uri.trim_end_matches('/').rsplit('/').next()?;
    crate::fs::check_name(last).ok()?;
    Some(Drive {
        id: m.id.clone(),
        name: m.name.clone(),
        uri: m.uri.trim_end_matches('/').to_owned(),
        area: AreaId::Drive(last.to_owned()),
    })
}

pub(crate) fn port_failed(e: PortError) -> Dialog {
    let fs_error = match e {
        PortError::Unavailable => FsError::Io,
        PortError::NotInstalled | PortError::Refused => FsError::Permission,
    };
    Dialog::Failed(fs_error, 1)
}

/// Marks the running job (used by [`App::apply`]).
pub(crate) fn running(kind: crate::model::JobKind, cancel: Cancel) -> Running {
    Running {
        kind,
        progress: Default::default(),
        cancel,
    }
}
