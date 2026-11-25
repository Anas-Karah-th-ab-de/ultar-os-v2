//! Undo of the last operations ("Undo" in the toast, Ctrl+Z): each step knows how to go
//! back. Undo never deletes for good: undoing a copy moves the copies to the trash.

use super::copy::{Job, Report};
use super::{trash, Fs, FsError, Loc};
use std::collections::VecDeque;

/// Most steps kept.
pub const MAX_UNDO: usize = 32;

/// One operation that can be undone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Undo {
    /// Items moved to the trash: their ids there.
    Trashed(Vec<String>),
    /// Items moved: (from, to).
    Moved(Vec<(Loc, Loc)>),
    /// Copies made.
    Copied(Vec<Loc>),
    /// A rename: (from, to).
    Renamed(Loc, Loc),
    /// A new folder.
    Created(Loc),
}

/// The undo steps, newest last, at most [`MAX_UNDO`].
#[derive(Clone, Debug, Default)]
pub struct UndoStack {
    steps: VecDeque<Undo>,
}

impl UndoStack {
    /// Remembers a step (the oldest is forgotten beyond the limit). Empty steps are ignored.
    pub fn push(&mut self, step: Undo) {
        let empty = match &step {
            Undo::Trashed(v) => v.is_empty(),
            Undo::Moved(v) => v.is_empty(),
            Undo::Copied(v) => v.is_empty(),
            Undo::Renamed(..) | Undo::Created(_) => false,
        };
        if empty {
            return;
        }
        if self.steps.len() == MAX_UNDO {
            self.steps.pop_front();
        }
        self.steps.push_back(step);
    }

    /// Takes the newest step.
    pub fn pop(&mut self) -> Option<Undo> {
        self.steps.pop_back()
    }

    /// `true` when there is something to undo.
    pub fn can_undo(&self) -> bool {
        !self.steps.is_empty()
    }

    /// Number of steps.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Goes back one step; `now` dates the trash records of undone copies.
pub fn apply(job: &mut Job<'_>, fs: &Fs, step: &Undo, now: i64) -> Report {
    match step {
        Undo::Trashed(ids) => trash::restore(job, fs, ids),
        Undo::Moved(pairs) => {
            let mut report = Report::default();
            for (from, to) in pairs.iter().rev() {
                match job.move_as(to, from) {
                    Ok(()) => report.done.push((to.clone(), from.clone())),
                    Err(FsError::Cancelled) => {
                        report.cancelled = true;
                        break;
                    }
                    Err(e) => report.failed.push((to.clone(), e)),
                }
            }
            report
        }
        Undo::Copied(copies) => trash::put(job, fs, copies, now),
        // a folder may only move by copy and delete (the kernel answers EXDEV): move_as does both
        Undo::Renamed(from, to) => single(to, job.move_as(to, from), from),
        Undo::Created(loc) => single(loc, fs.remove_one(loc, true), loc),
    }
}

fn single(item: &Loc, result: Result<(), FsError>, to: &Loc) -> Report {
    let mut report = Report::default();
    match result {
        Ok(()) => report.done.push((item.clone(), to.clone())),
        Err(e) => report.failed.push((item.clone(), e)),
    }
    report
}
