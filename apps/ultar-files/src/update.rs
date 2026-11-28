//! Clicks, keys and finished jobs → state changes and effects. No input/output here: the
//! app executes the returned [`Effect`]s.

use crate::action::{Command, Effect, Target};
use crate::fs::copy::{Policy, Report};
use crate::fs::{AreaId, FsError, Kind, Loc};
use crate::model::{file_kind, Dialog, FileKind, JobKind, Notice, SortKey, State, ViewMode};
use ultar_i18n::collation::collation_key;
use ultar_ui::event::{ActionKind, Key, Modifiers};

/// Shows `loc`: remembered in the history, selection and search reset.
pub fn go(s: &mut State, loc: Loc) -> Vec<Effect> {
    if loc != s.folder {
        let from = std::mem::replace(&mut s.folder, loc);
        s.history.visit(from);
    }
    enter(s)
}

fn enter(s: &mut State) -> Vec<Effect> {
    s.selected.clear();
    s.query.clear();
    s.hits = None;
    s.error = None;
    vec![Effect::Reload]
}

/// `files.back`.
pub fn back(s: &mut State) -> Vec<Effect> {
    let Some(to) = s.history.back.pop() else {
        return Vec::new();
    };
    let from = std::mem::replace(&mut s.folder, to);
    s.history.forward.push(from);
    enter(s)
}

/// `files.forward`.
pub fn forward(s: &mut State) -> Vec<Effect> {
    let Some(to) = s.history.forward.pop() else {
        return Vec::new();
    };
    let from = std::mem::replace(&mut s.folder, to);
    s.history.back.push(from);
    enter(s)
}

/// The items shown, in order: search applied, folders first, then by the sort key.
pub fn visible(s: &State) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..s.items.len())
        .filter(|&i| {
            s.hits
                .as_ref()
                .is_none_or(|h| h.contains(&s.items[i].entry.name))
        })
        .collect();
    let (key, ascending) = s.look.sort;
    idx.sort_by(|&a, &b| {
        let (ea, eb) = (&s.items[a].entry, &s.items[b].entry);
        let folders = (eb.kind == Kind::Folder).cmp(&(ea.kind == Kind::Folder));
        let by = match key {
            SortKey::Name => std::cmp::Ordering::Equal,
            SortKey::Size => ea.size.cmp(&eb.size),
            SortKey::Modified => ea.modified.cmp(&eb.modified),
            SortKey::Kind => file_kind(ea).cmp(&file_kind(eb)),
        };
        let by = by.then_with(|| collation_key(&ea.name).cmp(&collation_key(&eb.name)));
        folders.then(if ascending { by } else { by.reverse() })
    });
    idx
}

/// A click or other action on a node with meaning `target`. `key_input`: the action came
/// from a key press (Enter on an item opens it; a click only selects).
pub fn action(s: &mut State, target: &Target, kind: &ActionKind, key_input: bool) -> Vec<Effect> {
    match (target, kind) {
        (Target::Back, ActionKind::Activated) => back(s),
        (Target::Forward, ActionKind::Activated) => forward(s),
        (Target::Place(i), ActionKind::Activated) => match s.places.get(*i).cloned() {
            Some(area) => go(s, Loc::root(area)),
            None => Vec::new(),
        },
        (Target::Drive(i), ActionKind::Activated) => match s.drives.get(*i) {
            Some(d) => {
                let root = Loc::root(d.area.clone());
                go(s, root)
            }
            None => Vec::new(),
        },
        (Target::Eject(i), ActionKind::Activated) => s
            .drives
            .get(*i)
            .map(|d| vec![Effect::Eject(d.id.clone())])
            .unwrap_or_default(),
        (Target::TrashPlace, ActionKind::Activated) if s.has_trash => {
            go(s, Loc::root(AreaId::Trash))
        }
        (Target::NewFolder, ActionKind::Activated) => command(s, Command::NewFolder),
        (Target::EmptyTrash, ActionKind::Activated) => command(s, Command::EmptyTrash),
        (Target::ViewMode, ActionKind::Activated) => {
            s.look.view = match s.look.view {
                ViewMode::Grid => ViewMode::List,
                ViewMode::List => ViewMode::Grid,
            };
            Vec::new()
        }
        (Target::Sort, ActionKind::Sorted(column, ascending)) => {
            let key = [
                SortKey::Name,
                SortKey::Size,
                SortKey::Modified,
                SortKey::Kind,
            ];
            s.look.sort = (key.get(*column).copied().unwrap_or_default(), *ascending);
            Vec::new()
        }
        _ => item_or_dialog(s, target, kind, key_input),
    }
}

/// The items, the dialogs and the job bar.
fn item_or_dialog(
    s: &mut State,
    target: &Target,
    kind: &ActionKind,
    key_input: bool,
) -> Vec<Effect> {
    if let Some(effects) = foreign_action(s, target, kind) {
        return effects;
    }
    match (target, kind) {
        (Target::Item(name), ActionKind::DoubleActivated) => open_item(s, name),
        (Target::Item(name), ActionKind::Activated) if key_input => {
            s.selected = vec![name.clone()];
            open_item(s, name)
        }
        (Target::Item(name), ActionKind::Activated) => {
            s.selected = vec![name.clone()];
            Vec::new()
        }
        (_, ActionKind::MenuChosen(id)) => Command::from_id(*id)
            .map(|c| command(s, c))
            .unwrap_or_default(),
        (Target::DialogOk, ActionKind::Activated) => dialog_ok(s, Policy::Replace),
        (Target::DialogAlt, ActionKind::Activated) => dialog_ok(s, Policy::KeepBoth),
        (Target::DialogCancel, ActionKind::Activated | ActionKind::Closed) => {
            s.dialog = None;
            Vec::new()
        }
        (Target::JobCancel, ActionKind::Activated) => {
            if let Some(job) = &s.job {
                job.cancel.cancel();
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// The text of the search field changed (`files.search`).
pub fn search(s: &mut State, text: &str) -> Vec<Effect> {
    let text: String = text.chars().take(crate::fs::search::MAX_QUERY).collect();
    if text == s.query {
        return Vec::new();
    }
    s.query = text;
    vec![Effect::Reload]
}

/// The selection of the item list changed (toolkit multi-selection: Ctrl, Shift).
pub fn select(s: &mut State, names: Vec<String>) {
    s.selected = names;
}

/// Opens an item: a folder here, a file with its default app (`launcher.start`).
pub fn open_item(s: &mut State, name: &str) -> Vec<Effect> {
    let Some(item) = s.item(name).cloned() else {
        return Vec::new();
    };
    let Ok(loc) = s.folder.child(name) else {
        return Vec::new();
    };
    if s.in_trash() {
        s.dialog = Some(Dialog::Properties(loc));
        return Vec::new();
    }
    match file_kind(&item.entry) {
        FileKind::Folder => go(s, loc),
        FileKind::Link => {
            s.dialog = Some(Dialog::Failed(FsError::Link, 1));
            Vec::new()
        }
        // A package of another system goes to the install sheet (the service reads it).
        _ if crate::foreign::is_installable_name(name) => install_sheet(s, name, &loc),
        // A file opens with its default app: the launcher resolves the URI through
        // apps.default.* (Effect::Open); "no app" is said when none is installed for it.
        _ => vec![Effect::Open {
            uri: crate::uri::of_loc(&loc),
            name: name.to_owned(),
        }],
    }
}

/// Opens the install sheet for the package `name` at `loc` and asks the service about it.
fn install_sheet(s: &mut State, name: &str, loc: &Loc) -> Vec<Effect> {
    let Some(path) = crate::foreign::host_path(loc, &s.drives) else {
        s.notice = Some(notice_of("files.notice.open_failed", name));
        return Vec::new();
    };
    let sheet = crate::foreign::Sheet::new(name, path.clone());
    s.dialog = Some(Dialog::Foreign(Box::new(sheet)));
    vec![Effect::ForeignInspect(path)]
}

/// Clicks inside the install sheet; `None` when the action is not about it.
fn foreign_action(s: &mut State, target: &Target, kind: &ActionKind) -> Option<Vec<Effect>> {
    let Some(Dialog::Foreign(sheet)) = &mut s.dialog else {
        return None;
    };
    match (target, kind) {
        (Target::ForeignInternet, ActionKind::Toggled(on)) => sheet.set_internet(*on),
        (Target::ForeignLan, ActionKind::Toggled(on)) => sheet.set_lan(*on),
        (Target::ForeignFolder(word), ActionKind::Toggled(on)) => sheet.set_folder(word, *on),
        (Target::DialogOk, ActionKind::Activated) => return Some(foreign_ok(s)),
        (Target::DialogCancel, ActionKind::Activated | ActionKind::Closed) => {
            // closing a running installation hides the sheet; the job goes on and its end
            // is said in a notice
            if let Some(job) = sheet.job() {
                s.foreign_job = Some((job, sheet.file.clone()));
            }
            s.dialog = None;
        }
        _ => return None,
    }
    Some(Vec::new())
}

fn foreign_ok(s: &mut State) -> Vec<Effect> {
    let Some(Dialog::Foreign(sheet)) = &mut s.dialog else {
        return Vec::new();
    };
    if let crate::foreign::Phase::Done(apps) = &sheet.phase {
        let app = apps.first().cloned();
        s.dialog = None;
        return app.map(Effect::StartApp).into_iter().collect();
    }
    if sheet.start() {
        return vec![Effect::ForeignInstall];
    }
    Vec::new()
}

/// What the service for apps from other systems answered; `true` when the window changes.
pub fn foreign_event(s: &mut State, event: crate::service::ForeignEvent) -> bool {
    use crate::foreign::Phase;
    use crate::service::ForeignEvent as E;
    let sheet = match &mut s.dialog {
        Some(Dialog::Foreign(sheet)) => Some(sheet),
        _ => None,
    };
    match (event, sheet) {
        (E::Inspected(a), Some(sh)) if sh.phase == Phase::Reading => sh.inspected(a),
        (E::Started(a), Some(sh)) if sh.phase == Phase::Starting => sh.started(a),
        (E::Progress(p), Some(sh)) if sh.progress(&p) => {}
        (E::Progress(p), _) => return background_job(s, &p),
        _ => return false,
    }
    true
}

/// A step of an installation whose sheet was closed: its end is said in a notice.
fn background_job(s: &mut State, p: &ultar_bus::messages::foreign::Progress) -> bool {
    use ultar_bus::messages::foreign::Step;
    let Some((job, name)) = s.foreign_job.clone() else {
        return false;
    };
    let key = match p.step {
        _ if job != p.job => return false,
        Step::Done => "files.notice.installed",
        Step::Cancelled => "files.notice.install_cancelled",
        Step::Failed(_) => "files.notice.install_failed",
        _ => return false,
    };
    s.foreign_job = None;
    s.notice = Some(notice_of(key, &name));
    true
}

/// A notice about one item (no undo).
pub fn notice_of(key: &'static str, name: &str) -> Notice {
    notice(key, 1, name, false)
}

fn notice(key: &'static str, count: usize, name: &str, undo: bool) -> Notice {
    Notice {
        key,
        count: count as i64,
        name: name.to_owned(),
        undo,
    }
}

/// A command of a menu or shortcut.
pub fn command(s: &mut State, cmd: Command) -> Vec<Effect> {
    let sel = s.selection();
    let busy = s.job.is_some();
    match cmd {
        Command::Open => sel
            .first()
            .and_then(|l| l.name().map(str::to_owned))
            .map(|n| open_item(s, &n))
            .unwrap_or_default(),
        Command::Install if !s.in_trash() => match sel.first() {
            Some(l) if l.name().is_some_and(crate::foreign::is_installable_name) => {
                let name = l.name().unwrap_or_default().to_owned();
                install_sheet(s, &name, l)
            }
            _ => Vec::new(),
        },
        Command::Copy | Command::Cut if !sel.is_empty() && !s.in_trash() => {
            s.edit.clipboard = Some((sel, cmd == Command::Cut));
            Vec::new()
        }
        Command::Paste if !busy && !s.in_trash() => match s.edit.clipboard.clone() {
            Some((items, true)) => {
                s.edit.clipboard = None;
                vec![Effect::Job(JobKind::Move(
                    items,
                    s.folder.clone(),
                    Policy::Ask,
                ))]
            }
            Some((items, false)) => vec![Effect::Job(JobKind::Copy(
                items,
                s.folder.clone(),
                Policy::Ask,
            ))],
            None => Vec::new(),
        },
        Command::Rename if sel.len() == 1 && !s.in_trash() => {
            s.dialog = sel.into_iter().next().map(Dialog::Rename);
            Vec::new()
        }
        Command::Trash if !busy && !sel.is_empty() && !s.in_trash() => {
            vec![Effect::Job(JobKind::Trash(sel))]
        }
        Command::Trash | Command::Purge if s.in_trash() && !s.selected.is_empty() => {
            s.dialog = Some(Dialog::ConfirmPurge(s.selected_ids()));
            Vec::new()
        }
        _ => command_rest(s, cmd, busy, sel),
    }
}

/// Properties, trash, new folder, selection, undo, eject.
fn command_rest(s: &mut State, cmd: Command, busy: bool, sel: Vec<Loc>) -> Vec<Effect> {
    match cmd {
        Command::Properties => {
            let target = sel.into_iter().next().unwrap_or_else(|| s.folder.clone());
            s.dialog = Some(Dialog::Properties(target));
            Vec::new()
        }
        Command::Restore if !busy && s.in_trash() && !s.selected.is_empty() => {
            vec![Effect::Job(JobKind::Restore(s.selected_ids()))]
        }
        Command::EmptyTrash if s.has_trash && !busy => {
            s.dialog = Some(Dialog::ConfirmEmpty);
            Vec::new()
        }
        Command::NewFolder if !s.in_trash() => vec![Effect::NewFolder(s.folder.clone())],
        Command::SelectAll => {
            s.selected = visible(s)
                .into_iter()
                .map(|i| s.items[i].entry.name.clone())
                .collect();
            Vec::new()
        }
        Command::Undo if !busy && s.edit.undo.can_undo() => vec![Effect::Job(JobKind::Undo)],
        Command::Eject => match &s.folder.area {
            AreaId::Drive(_) => s
                .drives
                .iter()
                .find(|d| d.area == s.folder.area)
                .map(|d| vec![Effect::Eject(d.id.clone())])
                .unwrap_or_default(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn dialog_ok(s: &mut State, policy: Policy) -> Vec<Effect> {
    match s.dialog.take() {
        Some(Dialog::ConfirmEmpty) => vec![Effect::Job(JobKind::Empty)],
        Some(Dialog::ConfirmPurge(ids)) => vec![Effect::Job(JobKind::Purge(ids))],
        Some(Dialog::Conflict(job, _)) => match *job {
            JobKind::Copy(items, to, _) => vec![Effect::Job(JobKind::Copy(items, to, policy))],
            JobKind::Move(items, to, _) => vec![Effect::Job(JobKind::Move(items, to, policy))],
            _ => Vec::new(),
        },
        Some(Dialog::DriveBusy(id)) => vec![Effect::Eject(id)],
        _ => Vec::new(),
    }
}

/// The rename dialog was confirmed with `text`.
pub fn rename(s: &mut State, text: &str) -> Vec<Effect> {
    let Some(Dialog::Rename(loc)) = s.dialog.clone() else {
        return Vec::new();
    };
    let text = text.trim();
    if loc.name() == Some(text) {
        s.dialog = None;
        return Vec::new();
    }
    if crate::fs::check_name(text).is_err() {
        return Vec::new(); // the view shows the error under the field
    }
    s.dialog = None;
    vec![Effect::Rename(loc, text.to_owned())]
}

/// Keys that are shortcuts before the widgets see them (the widgets would move focus with
/// them): Alt with an arrow.
pub fn is_shortcut(event: &ultar_ui::event::Event) -> bool {
    use Key::{ArrowLeft as Back, ArrowRight as Ahead, ArrowUp as Up}; // physical
    matches!(event, ultar_ui::event::Event::Key { key: Back | Ahead | Up, modifiers } if modifiers.alt && !modifiers.ctrl)
}

/// Keys the widgets did not use: the shortcuts of Files.
pub fn key(s: &mut State, key: Key, m: Modifiers) -> Vec<Effect> {
    let ctrl = m.ctrl && !m.alt;
    match key {
        Key::Char(c) if ctrl => match (c.to_ascii_lowercase(), m.shift) {
            ('c', false) => command(s, Command::Copy),
            ('x', false) => command(s, Command::Cut),
            ('v', false) => command(s, Command::Paste),
            ('z', false) => command(s, Command::Undo),
            ('a', false) => command(s, Command::SelectAll),
            ('n', true) => command(s, Command::NewFolder),
            ('1', false) => set_view(s, ViewMode::Grid),
            ('2', false) => set_view(s, ViewMode::List),
            ('r', false) => reload(s),
            _ => Vec::new(),
        },
        Key::F(5) if !m.command() => reload(s),
        Key::F(2) if !m.command() => command(s, Command::Rename),
        Key::Delete if !m.command() => command(s, Command::Trash),
        Key::ArrowLeft if m.alt => back(s), // physical: Alt+Left is back in every language
        Key::ArrowRight if m.alt => forward(s), // physical
        Key::ArrowUp if m.alt => up(s),
        Key::Backspace if !m.command() => up(s),
        Key::Escape if !s.query.is_empty() => {
            s.query.clear();
            s.hits = None;
            vec![Effect::Reload, Effect::FocusItems]
        }
        Key::Escape => {
            s.selected.clear();
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// F5 / Ctrl+R: read the folder again (a search is run again on the new list).
fn reload(s: &mut State) -> Vec<Effect> {
    if s.job.is_some() {
        return Vec::new();
    }
    vec![Effect::Reload, Effect::FocusItems]
}

fn set_view(s: &mut State, view: ViewMode) -> Vec<Effect> {
    s.look.view = view;
    Vec::new()
}

fn up(s: &mut State) -> Vec<Effect> {
    match s.folder.parent() {
        Some(parent) => go(s, parent),
        None => Vec::new(),
    }
}

/// A job ended: notice with Undo, conflicts asked, failures shown, undo step kept.
pub fn job_done(
    s: &mut State,
    kind: JobKind,
    report: &Report,
    undo: Option<crate::fs::undo::Undo>,
) -> Vec<Effect> {
    s.job = None;
    if let Some(step) = undo {
        s.edit.undo.push(step);
    }
    let n = report.done.len();
    let first = report
        .done
        .first()
        .and_then(|(f, _)| f.name())
        .unwrap_or_default()
        .to_owned();
    let (key, offers_undo) = match &kind {
        JobKind::Copy(..) => ("files.notice.copied", true),
        JobKind::Move(..) => ("files.notice.moved", true),
        JobKind::Trash(_) => ("files.notice.trashed", true),
        JobKind::Restore(_) => ("files.notice.restored", false),
        JobKind::Purge(_) => ("files.notice.purged", false),
        JobKind::Empty => ("files.notice.emptied", false),
        JobKind::Undo => ("files.notice.undone", false),
    };
    if n > 0 || matches!(kind, JobKind::Empty) {
        s.notice = Some(notice(key, n, &first, offers_undo));
    }
    if report.cancelled {
        s.notice = Some(notice(
            "files.notice.cancelled",
            n,
            &first,
            offers_undo && n > 0,
        ));
    }
    s.dialog = if let Some((_, to)) = report.conflicts.first() {
        let sources = report.conflicts.iter().map(|(f, _)| f.clone()).collect();
        let job = match kind {
            JobKind::Copy(_, dest, _) => JobKind::Copy(sources, dest, Policy::Ask),
            JobKind::Move(_, dest, _) => JobKind::Move(sources, dest, Policy::Ask),
            other => other,
        };
        Some(Dialog::Conflict(Box::new(job), to.clone()))
    } else {
        report
            .failed
            .first()
            .map(|(_, e)| Dialog::Failed(*e, report.failed.len()))
    };
    s.selected = report
        .done
        .iter()
        .filter(|(_, to)| to.parent().as_ref() == Some(&s.folder))
        .filter_map(|(_, to)| to.name().map(str::to_owned))
        .collect();
    vec![Effect::Reload]
}

/// The answer of `volumes.eject` for the drive `id`.
pub fn ejected(s: &mut State, id: &str, done: Result<bool, FsError>) -> Vec<Effect> {
    match done {
        Ok(true) => {
            let name = s
                .drives
                .iter()
                .find(|d| d.id == id)
                .map(|d| d.name.clone())
                .unwrap_or_default();
            s.notice = Some(notice("files.notice.ejected", 1, &name, false));
            let leave = s
                .drives
                .iter()
                .any(|d| d.id == id && d.area == s.folder.area);
            s.drives.retain(|d| d.id != id);
            match (leave, s.places.first().cloned()) {
                (true, Some(home)) => go(s, Loc::root(home)),
                _ => Vec::new(),
            }
        }
        Ok(false) => {
            s.dialog = Some(Dialog::DriveBusy(id.to_owned()));
            Vec::new()
        }
        Err(e) => {
            s.dialog = Some(Dialog::Failed(e, 1));
            Vec::new()
        }
    }
}
