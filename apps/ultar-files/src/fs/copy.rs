//! Copy, move and permanent delete of several items: progress, cancel at any time, a
//! conflict policy, and a report that lists what worked and what failed (partial failures
//! never stop the other items).

use super::{free_name, Fs, FsError, Kind, Loc, MAX_DEPTH};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Bytes copied between two progress reports and cancel checks.
pub const CHUNK: usize = 256 * 1024;

/// What happens when the target name is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Leave the item and report the conflict (the app asks, then runs again).
    Ask,
    /// Leave the item.
    Skip,
    /// Replace the target (folders: merge into it).
    Replace,
    /// Keep both: the new item gets a free name ("name 2.txt").
    KeepBoth,
}

/// A cancel switch shared between the app and a running job.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    /// Asks the job to stop at the next chunk.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// `true` once cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// How far a job is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Bytes done.
    pub done_bytes: u64,
    /// Bytes in all.
    pub total_bytes: u64,
    /// Items (files, folders, links) done.
    pub done_items: u64,
    /// Items in all.
    pub total_items: u64,
}

impl Progress {
    /// Share done, 0..=1 (by bytes, by items when there are no bytes).
    pub fn fraction(&self) -> f32 {
        let (d, t) = if self.total_bytes > 0 {
            (self.done_bytes, self.total_bytes)
        } else {
            (self.done_items, self.total_items.max(1))
        };
        (d as f64 / t as f64).clamp(0.0, 1.0) as f32
    }
}

/// What a job did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Source and where it went (for undo).
    pub done: Vec<(Loc, Loc)>,
    /// Items that failed, with the reason.
    pub failed: Vec<(Loc, FsError)>,
    /// Source and the taken target (policy [`Policy::Ask`]).
    pub conflicts: Vec<(Loc, Loc)>,
    /// Items left as asked.
    pub skipped: Vec<Loc>,
    /// The job was cancelled (items after that are not in any list).
    pub cancelled: bool,
}

/// A running job: the file system, the cancel switch and the progress sink.
pub struct Job<'a> {
    fs: &'a Fs,
    cancel: Cancel,
    sink: &'a mut dyn FnMut(Progress),
    progress: Progress,
}

impl<'a> Job<'a> {
    /// A job over `fs`.
    pub fn new(fs: &'a Fs, cancel: Cancel, sink: &'a mut dyn FnMut(Progress)) -> Job<'a> {
        Job {
            fs,
            cancel,
            sink,
            progress: Progress::default(),
        }
    }

    fn check(&self) -> Result<(), FsError> {
        if self.cancel.is_cancelled() {
            Err(FsError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn tick(&mut self, bytes: u64, items: u64) {
        self.progress.done_bytes = self.progress.done_bytes.saturating_add(bytes);
        self.progress.done_items = self.progress.done_items.saturating_add(items);
        (self.sink)(self.progress);
    }

    /// Counts items and bytes below `sources` (links not followed).
    pub fn measure(&mut self, sources: &[Loc]) {
        let mut p = Progress::default();
        for s in sources {
            self.count(s, 0, &mut p);
        }
        self.progress = Progress {
            done_bytes: 0,
            done_items: 0,
            ..p
        };
        (self.sink)(self.progress);
    }

    fn count(&self, loc: &Loc, depth: usize, p: &mut Progress) {
        let Ok(e) = self.fs.stat(loc) else { return };
        p.total_items += 1;
        p.total_bytes += e.size;
        if e.kind == Kind::Folder && depth < MAX_DEPTH {
            for child in self.fs.list(loc).unwrap_or_default() {
                if let Ok(c) = loc.child(&child.name) {
                    self.count(&c, depth + 1, p);
                }
            }
        }
    }

    /// Copies each source into the folder `dest`.
    pub fn copy(&mut self, sources: &[Loc], dest: &Loc, policy: Policy) -> Report {
        self.transfer(sources, dest, policy, false)
    }

    /// Moves each source into the folder `dest` (a rename where possible, else copy and
    /// delete; a source is deleted only after its whole copy succeeded).
    pub fn move_to(&mut self, sources: &[Loc], dest: &Loc, policy: Policy) -> Report {
        self.transfer(sources, dest, policy, true)
    }

    fn transfer(&mut self, sources: &[Loc], dest: &Loc, policy: Policy, moving: bool) -> Report {
        self.measure(sources);
        let mut report = Report::default();
        for src in sources {
            if self.cancel.is_cancelled() {
                report.cancelled = true;
                break;
            }
            match self.one(src, dest, policy, moving) {
                Ok(Some(to)) => report.done.push((src.clone(), to)),
                Ok(None) => report.skipped.push(src.clone()),
                Err(Step::Conflict(to)) => report.conflicts.push((src.clone(), to)),
                Err(Step::Failed(FsError::Cancelled)) => {
                    report.cancelled = true;
                    break;
                }
                Err(Step::Failed(e)) => report.failed.push((src.clone(), e)),
            }
        }
        report
    }

    fn one(
        &mut self,
        src: &Loc,
        dest: &Loc,
        policy: Policy,
        moving: bool,
    ) -> Result<Option<Loc>, Step> {
        if src.contains(dest) {
            return Err(Step::Failed(FsError::IntoItself));
        }
        let name = src.name().ok_or(Step::Failed(FsError::BadName))?;
        let mut to = dest.child(name)?;
        if moving && src.parent().as_ref() == Some(dest) {
            return Ok(None); // already there
        }
        let mut replace = false;
        if self.fs.exists(&to) {
            match policy {
                Policy::Ask => return Err(Step::Conflict(to)),
                Policy::Skip => return Ok(None),
                Policy::KeepBoth => {
                    let free = free_name(name, |n| dest.child(n).is_ok_and(|c| self.fs.exists(&c)))
                        .ok_or(Step::Failed(FsError::Exists))?;
                    to = dest.child(&free)?;
                }
                Policy::Replace => replace = true,
            }
        }
        if moving && !replace {
            match self.fs.rename(src, &to, false) {
                Ok(()) => {
                    let size = self.fs.stat(&to).map_or(0, |e| e.size);
                    self.tick(size, 1);
                    return Ok(Some(to));
                }
                Err(FsError::Busy) => {} // another file system: copy, then delete
                Err(e) => return Err(Step::Failed(e)),
            }
        }
        self.copy_item(src, &to, replace, 0)?;
        if moving {
            self.remove_tree(src, 0)?;
        }
        Ok(Some(to))
    }

    /// Moves one item to exactly `to` (which must not exist): a rename where possible,
    /// else copy and delete.
    pub fn move_as(&mut self, src: &Loc, to: &Loc) -> Result<(), FsError> {
        if src.contains(to) {
            return Err(FsError::IntoItself);
        }
        if self.fs.exists(to) {
            return Err(FsError::Exists);
        }
        self.measure(std::slice::from_ref(src));
        match self.fs.rename(src, to, false) {
            Ok(()) => {
                let p = self.progress;
                self.tick(p.total_bytes, p.total_items);
                Ok(())
            }
            Err(FsError::Busy) => {
                self.copy_item(src, to, false, 0)?;
                self.remove_tree(src, 0)
            }
            Err(e) => Err(e),
        }
    }

    fn copy_item(
        &mut self,
        src: &Loc,
        to: &Loc,
        replace: bool,
        depth: usize,
    ) -> Result<(), FsError> {
        self.check()?;
        if depth > MAX_DEPTH {
            return Err(FsError::TooDeep);
        }
        let e = self.fs.stat(src)?;
        match e.kind {
            Kind::Folder => {
                let target = if replace { self.fs.stat(to).ok() } else { None };
                match target.map(|t| t.kind) {
                    Some(Kind::Folder) => {}
                    Some(_) => {
                        self.remove_tree(to, 0)?;
                        self.fs.mkdir(to)?;
                    }
                    None => self.fs.mkdir(to)?,
                }
                self.tick(0, 1);
                for child in self.fs.list(src)? {
                    let (s, t) = (src.child(&child.name)?, to.child(&child.name)?);
                    self.copy_item(&s, &t, replace, depth + 1)?;
                }
                Ok(())
            }
            Kind::File => self.copy_file(src, to, replace, e.mode),
            Kind::Link => {
                let target = self.fs.read_link(src)?;
                if replace && self.fs.exists(to) {
                    self.remove_tree(to, 0)?;
                }
                self.fs.make_link(&target, to)?;
                self.tick(0, 1);
                Ok(())
            }
            Kind::Other => Err(FsError::Permission),
        }
    }

    fn copy_file(&mut self, src: &Loc, to: &Loc, replace: bool, mode: u32) -> Result<(), FsError> {
        let (dir, name) = self.fs.parent(to)?;
        let part = free_name(&format!(".{name}.part"), |n| {
            rustix::fs::statat(&dir, n, rustix::fs::AtFlags::SYMLINK_NOFOLLOW).is_ok()
        })
        .ok_or(FsError::Exists)?;
        let part_loc = to.parent().ok_or(FsError::BadName)?.child(&part)?;
        let mut input = self.fs.open_read(src)?;
        let mut output = Fs::create_in(&dir, &part, mode | 0o600)?;
        let result = self.pump(&mut input, &mut output);
        let result = result
            .and_then(|()| output.sync_all().map_err(|e| FsError::from_io(&e)))
            .and_then(|()| {
                if replace && self.fs.stat(to).is_ok_and(|t| t.kind == Kind::Folder) {
                    self.remove_tree(to, 0)?;
                }
                self.fs.rename(&part_loc, to, replace)
            });
        if result.is_err() {
            let _ = self.fs.remove_one(&part_loc, false);
        } else {
            let _ = self.fs.set_mode(to, mode);
            self.tick(0, 1);
        }
        result
    }

    fn pump(
        &mut self,
        input: &mut std::fs::File,
        output: &mut std::fs::File,
    ) -> Result<(), FsError> {
        let mut buf = vec![0u8; CHUNK];
        loop {
            self.check()?;
            let n = input.read(&mut buf).map_err(|e| FsError::from_io(&e))?;
            if n == 0 {
                return Ok(());
            }
            output
                .write_all(&buf[..n])
                .map_err(|e| FsError::from_io(&e))?;
            self.tick(n as u64, 0);
        }
    }

    /// Deletes each item for good (folders with everything in them).
    pub fn delete(&mut self, items: &[Loc]) -> Report {
        self.measure(items);
        let mut report = Report::default();
        for item in items {
            match self.remove_tree(item, 0) {
                Ok(()) => report.done.push((item.clone(), item.clone())),
                Err(FsError::Cancelled) => {
                    report.cancelled = true;
                    break;
                }
                Err(e) => report.failed.push((item.clone(), e)),
            }
        }
        report
    }

    /// Removes `loc` and, for a folder, everything below it; links are removed, not followed.
    pub fn remove_tree(&mut self, loc: &Loc, depth: usize) -> Result<(), FsError> {
        self.check()?;
        if depth > MAX_DEPTH {
            return Err(FsError::TooDeep);
        }
        let e = self.fs.stat(loc)?;
        if e.kind == Kind::Folder {
            for child in self.fs.list(loc)? {
                self.remove_tree(&loc.child(&child.name)?, depth + 1)?;
            }
        }
        self.fs.remove_one(loc, e.kind == Kind::Folder)?;
        self.tick(e.size, 1);
        Ok(())
    }
}

/// Why one item was not done.
enum Step {
    Conflict(Loc),
    Failed(FsError),
}

impl From<FsError> for Step {
    fn from(e: FsError) -> Step {
        Step::Failed(e)
    }
}
