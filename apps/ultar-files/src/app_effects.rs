//! Executing effects and following background work (part of [`crate::app::App`]).

use crate::action::Effect;
use crate::action::Target;
use crate::app::{drive_of, running, App};
use crate::fs::copy::Cancel;
use crate::fs::undo::Undo;
use crate::fs::{free_name, AreaId, Fs, FsError, Loc};
use crate::jobs::{self, JobMsg};
use crate::model::{Dialog, JobKind, Notice};
use crate::service::bus::STORE;
use crate::service::PortError;
use crate::update;
use std::sync::Arc;
use ultar_ui::event::ActionKind;
use ultar_ui::ui::UiError;
use ultar_ui::widgets::{Label, Menu, MenuItem};

/// Menu ids of the compact places menu start here.
pub(crate) const PLACES_MENU: u32 = 1000;

impl App {
    /// Executes effects of the update functions.
    pub fn apply(&mut self, effects: Vec<Effect>) {
        for e in effects {
            match e {
                Effect::Reload => self.reload(),
                Effect::Job(kind) => self.start_job(kind),
                Effect::Launch { app, uri } => self.launch(&app, &uri),
                Effect::Open { uri, name } => self.open_default(&uri, &name),
                Effect::Eject(id) => {
                    let done = self.ports.volumes.eject(&id).map_err(|_| FsError::Busy);
                    let more = update::ejected(&mut self.state, &id, done);
                    if !more.is_empty() {
                        self.apply(more);
                    }
                }
                Effect::NewFolder(folder) => self.new_folder(&folder),
                Effect::Rename(loc, name) => self.rename(&loc, &name),
                Effect::FocusItems => {}
                Effect::ForeignInspect(path) => self.ports.foreign.inspect(&path),
                Effect::ForeignInstall => self.foreign_install(),
                Effect::StartApp(app) => {
                    if self.ports.launcher.start(&app).is_err() {
                        self.state.notice = Some(update::notice_of("files.notice.open_failed", &app));
                    }
                }
            }
        }
    }

    /// Hands the open sheet's file and choices to the service.
    fn foreign_install(&mut self) {
        if let Some(Dialog::Foreign(sheet)) = &self.state.dialog {
            let (path, choices) = (sheet.path.clone(), sheet.choices.clone());
            self.ports.foreign.install(&path, choices);
        }
    }

    /// Takes the answers of the service for apps from other systems; `true` when the window
    /// changes.
    fn poll_foreign(&mut self) -> bool {
        let mut changed = false;
        for e in self.ports.foreign.poll() {
            changed |= update::foreign_event(&mut self.state, e);
        }
        changed
    }

    /// Opens a file with its default app: the launcher resolves the URI through
    /// `apps.default.*`; no installed app for it says so, a service that cannot is a dialog.
    fn open_default(&mut self, uri: &str, name: &str) {
        match self.ports.launcher.resolve(uri) {
            Ok(Some(app)) => self.launch(&app, uri),
            Ok(None) => {
                self.state.notice = Some(Notice {
                    key: "files.notice.no_app",
                    count: 1,
                    name: name.to_owned(),
                    undo: false,
                });
            }
            Err(e) => self.state.dialog = Some(port_failed(e)),
        }
    }

    fn launch(&mut self, app: &str, uri: &str) {
        // a failed start is about the app, never about this place: no permission dialog
        let name = crate::uri::name_of(uri);
        match self.ports.launcher.open(app, uri, None) {
            Ok(()) => {}
            Err(PortError::NotInstalled) => {
                // The Store offers to install the missing app.
                let page = format!("store://app/{app}");
                if self.ports.launcher.open(STORE, &page, None).is_err() {
                    self.state.notice = Some(update::notice_of("files.notice.no_app", &name));
                }
            }
            Err(_) => {
                self.state.notice = Some(update::notice_of("files.notice.open_failed", &name));
            }
        }
    }

    fn new_folder(&mut self, folder: &Loc) {
        let base = self.ui.translator().t("files.new_folder_name").into_owned();
        let fs = self.fs.clone();
        let free = free_name(&base, |n| folder.child(n).is_ok_and(|c| fs.exists(&c)));
        let made = free.ok_or(FsError::Exists).and_then(|n| {
            let loc = folder.child(&n)?;
            fs.mkdir(&loc).map(|()| loc)
        });
        match made {
            Ok(loc) => {
                self.state.edit.undo.push(Undo::Created(loc.clone()));
                self.reload();
                self.state.selected = loc.name().map(|n| vec![n.to_owned()]).unwrap_or_default();
                self.state.dialog = Some(Dialog::Rename(loc));
            }
            Err(e) => self.state.dialog = Some(Dialog::Failed(e, 1)),
        }
    }

    fn rename(&mut self, loc: &Loc, name: &str) {
        let to = loc
            .parent()
            .ok_or(FsError::BadName)
            .and_then(|p| p.child(name));
        // a rename where the kernel can, else copy and delete (folders on the Ultar kernel)
        let fs = self.fs.clone();
        let mut sink = |_| {};
        let mut job = crate::fs::copy::Job::new(&fs, Cancel::default(), &mut sink);
        match to.and_then(|to| job.move_as(loc, &to).map(|()| to)) {
            Ok(to) => {
                self.state
                    .edit
                    .undo
                    .push(Undo::Renamed(loc.clone(), to.clone()));
                self.reload();
                self.state.selected = vec![name.to_owned()];
            }
            Err(e) => self.state.dialog = Some(Dialog::Failed(e, 1)),
        }
    }

    fn start_job(&mut self, kind: JobKind) {
        if self.state.job.is_some() {
            return;
        }
        let step = if kind == JobKind::Undo {
            match self.state.edit.undo.pop() {
                Some(s) => Some(s),
                None => return,
            }
        } else {
            None
        };
        let cancel = Cancel::default();
        let now = (self.clock)();
        self.state.job = Some(running(kind.clone(), cancel.clone()));
        if self.inline_work {
            let (tx, rx) = std::sync::mpsc::channel();
            let report = jobs::run(&self.fs, &kind, step.as_ref(), cancel, now, &tx);
            let undo = jobs::undo_of(&kind, &report);
            drop(rx);
            let more = update::job_done(&mut self.state, kind, &report, undo);
            self.apply(more);
        } else {
            self.job = Some(jobs::spawn(self.fs.clone(), kind, step, cancel, now));
        }
    }

    /// Follows background work and the drive service; rebuilds when something changed.
    pub fn poll(&mut self) -> Result<(), UiError> {
        let mut changed = self.poll_drives();
        changed |= self.poll_job();
        changed |= self.poll_foreign();
        if let Some(rx) = self.thumbs.take() {
            let mut open = true;
            loop {
                match rx.try_recv() {
                    Ok(msg) => changed |= self.take_thumb(msg),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        open = false;
                        break;
                    }
                }
            }
            if open {
                self.thumbs = Some(rx);
            }
        }
        if changed {
            self.rebuild()?;
        }
        Ok(())
    }

    fn poll_job(&mut self) -> bool {
        let Some(rx) = self.job.take() else {
            return false;
        };
        loop {
            match rx.try_recv() {
                Ok(JobMsg::Progress(p)) => {
                    if let Some(job) = &mut self.state.job {
                        job.progress = p;
                    }
                    if let Some(bar) = self.built.progress {
                        let _ = self.ui.set_progress(bar, Some(p.fraction()));
                    }
                }
                Ok(JobMsg::Done(done)) => {
                    let (kind, report, undo) = *done;
                    let more = update::job_done(&mut self.state, kind, &report, undo);
                    self.apply(more);
                    return true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.job = Some(rx);
                    return false;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.state.job = None;
                    self.state.dialog = Some(Dialog::Failed(FsError::Io, 1));
                    return true;
                }
            }
        }
    }

    /// Takes a new drive list from the drive service; `true` when it changed.
    pub(crate) fn poll_drives(&mut self) -> bool {
        let Some(mounted) = self.ports.volumes.changed() else {
            return false;
        };
        if let Some(root) = &self.root {
            self.fs = Arc::new(Fs::open(root));
        }
        let drives: Vec<_> = mounted
            .iter()
            .filter_map(drive_of)
            .filter(|d| self.fs.has(&d.area))
            .collect();
        let gone = matches!(&self.state.folder.area, AreaId::Drive(_))
            && !drives.iter().any(|d| d.area == self.state.folder.area);
        self.state.drives = drives;
        if gone {
            let home = self
                .state
                .places
                .first()
                .cloned()
                .map_or(Loc::root(AreaId::Home), Loc::root);
            let more = update::go(&mut self.state, home);
            self.apply(more);
        }
        true
    }
}

impl App {
    /// The compact places button: opens the menu; a choice goes there.
    pub(crate) fn places_action(
        &mut self,
        node: ultar_ui::ui::NodeId,
        kind: &ActionKind,
    ) -> Result<Vec<Effect>, UiError> {
        match kind {
            ActionKind::Activated => {
                let menu = self.places_menu();
                self.ui.open_menu(node, menu)?;
                Ok(Vec::new())
            }
            ActionKind::MenuChosen(id) => Ok(self.place_chosen(*id)),
            _ => Ok(Vec::new()),
        }
    }

    pub(crate) fn places_menu(&self) -> Menu {
        let s = &self.state;
        let mut items: Vec<MenuItem> = s
            .places
            .iter()
            .enumerate()
            .map(|(i, a)| MenuItem::new(PLACES_MENU + i as u32, Label::key(a.label_key())))
            .collect();
        let base = PLACES_MENU + s.places.len() as u32;
        items.extend(s.drives.iter().enumerate().map(|(i, d)| {
            MenuItem::new(
                base + i as u32,
                Label::key_with("files.place.drive", &[("name", &d.name)]),
            )
        }));
        if s.has_trash {
            items.push(
                MenuItem::new(
                    base + s.drives.len() as u32,
                    Label::key("files.place.trash"),
                )
                .separated(),
            );
        }
        Menu::new(items)
    }

    pub(crate) fn place_chosen(&mut self, id: u32) -> Vec<Effect> {
        let s = &mut self.state;
        let i = id.saturating_sub(PLACES_MENU) as usize;
        let target = if i < s.places.len() {
            Target::Place(i)
        } else if i < s.places.len() + s.drives.len() {
            Target::Drive(i - s.places.len())
        } else {
            Target::TrashPlace
        };
        update::action(s, &target, &ActionKind::Activated, false)
    }

    pub(crate) fn dropped(&mut self, to: ultar_ui::ui::NodeId) -> Vec<Effect> {
        self.read_selection();
        let dest = match self.built.targets.get(&to) {
            Some(Target::Item(name)) if !self.state.selected.contains(name) => {
                self.state.folder.child(name).ok()
            }
            Some(Target::Place(i)) => self.state.places.get(*i).cloned().map(Loc::root),
            Some(Target::Drive(i)) => self.state.drives.get(*i).map(|d| Loc::root(d.area.clone())),
            Some(Target::TrashPlace) => {
                return update::command(&mut self.state, crate::action::Command::Trash)
            }
            _ => None,
        };
        match dest {
            Some(dest) if self.state.job.is_none() => {
                vec![Effect::Job(crate::model::JobKind::Move(
                    self.state.selection(),
                    dest,
                    crate::fs::copy::Policy::Ask,
                ))]
            }
            _ => Vec::new(),
        }
    }
}

/// A launcher service that cannot answer is shown as a failed dialog (not about this place).
fn port_failed(e: PortError) -> Dialog {
    let fs_error = match e {
        PortError::Unavailable => FsError::Io,
        PortError::NotInstalled | PortError::Refused => FsError::Permission,
    };
    Dialog::Failed(fs_error, 1)
}
