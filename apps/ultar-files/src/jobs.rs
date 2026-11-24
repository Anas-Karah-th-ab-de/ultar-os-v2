//! Background work: file jobs and thumbnails run on their own threads so the window always
//! stays usable (APP-RULES §8); results come back over channels.

use crate::fs::copy::{Cancel, Job, Progress, Report};
use crate::fs::thumb::{self, Picture};
use crate::fs::undo::{self, Undo};
use crate::fs::{trash, Fs, Loc};
use crate::model::JobKind;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// What a job thread reports.
#[derive(Debug)]
pub enum JobMsg {
    /// How far it is.
    Progress(Progress),
    /// It ended: what it was, what it did, how to undo it.
    Done(Box<(JobKind, Report, Option<Undo>)>),
}

/// The undo step of a finished job.
pub fn undo_of(kind: &JobKind, report: &Report) -> Option<Undo> {
    let to = || report.done.iter().map(|(_, t)| t.clone());
    match kind {
        JobKind::Copy(..) => Some(Undo::Copied(to().collect())),
        JobKind::Move(..) => Some(Undo::Moved(report.done.clone())),
        JobKind::Trash(_) => Some(Undo::Trashed(
            to().filter_map(|t| t.name().map(str::to_owned)).collect(),
        )),
        _ => None,
    }
}

/// Runs `kind` (with `step` for an undo) on the calling thread.
pub fn run(
    fs: &Fs,
    kind: &JobKind,
    step: Option<&Undo>,
    cancel: Cancel,
    now: i64,
    tx: &Sender<JobMsg>,
) -> Report {
    let mut sink = |p: Progress| {
        let _ = tx.send(JobMsg::Progress(p));
    };
    let mut job = Job::new(fs, cancel, &mut sink);
    match kind {
        JobKind::Copy(items, to, policy) => job.copy(items, to, *policy),
        JobKind::Move(items, to, policy) => job.move_to(items, to, *policy),
        JobKind::Trash(items) => trash::put(&mut job, fs, items, now),
        JobKind::Restore(ids) => trash::restore(&mut job, fs, ids),
        JobKind::Purge(ids) => trash::delete(&mut job, ids),
        JobKind::Empty => trash::empty(&mut job, fs),
        JobKind::Undo => match step {
            Some(step) => undo::apply(&mut job, fs, step, now),
            None => Report::default(),
        },
    }
}

/// Starts `kind` on a new thread; its messages arrive on the returned receiver.
pub fn spawn(
    fs: Arc<Fs>,
    kind: JobKind,
    step: Option<Undo>,
    cancel: Cancel,
    now: i64,
) -> Receiver<JobMsg> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let report = run(&fs, &kind, step.as_ref(), cancel, now, &tx);
        let undo = undo_of(&kind, &report);
        let _ = tx.send(JobMsg::Done(Box::new((kind, report, undo))));
    });
    rx
}

/// A thumbnail made in the background: the folder it belongs to, the item, the picture.
pub type ThumbMsg = (Loc, String, Picture);

/// Makes thumbnails of `names` in `folder` on a new thread (failures are left out).
pub fn thumbnails(fs: Arc<Fs>, folder: Loc, names: Vec<String>) -> Receiver<ThumbMsg> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for name in names {
            let Ok(loc) = folder.child(&name) else {
                continue;
            };
            if let Ok(p) = thumb::thumbnail(&fs, &loc) {
                if tx.send((folder.clone(), name, p)).is_err() {
                    return;
                }
            }
        }
    });
    rx
}
