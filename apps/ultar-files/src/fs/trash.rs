//! The trash: `files/<name>` holds the item, `info/<name>.trashinfo` where it came from and
//! when (the layout of the freedesktop trash, with an area instead of a raw path). Records
//! are read as hostile input: size-limited, every field checked.

use super::copy::{Job, Report};
use super::{free_name, AreaId, Fs, FsError, Kind, Loc};
use std::io::{Read, Write};

/// Largest record that is read.
pub const MAX_RECORD: u64 = 8 * 1024;
const FILES: &str = "files";
const INFO: &str = "info";
const SUFFIX: &str = ".trashinfo";

/// One item in the trash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trashed {
    /// Name inside the trash (unique there).
    pub id: String,
    /// Where it was.
    pub origin: Loc,
    /// When it was deleted, seconds since 1970.
    pub deleted: i64,
    /// What it is.
    pub kind: Kind,
    /// Size in bytes (files).
    pub size: u64,
}

fn files_dir() -> Loc {
    Loc::root(AreaId::Trash)
        .child(FILES)
        .unwrap_or(Loc::root(AreaId::Trash))
}

fn info_dir() -> Loc {
    Loc::root(AreaId::Trash)
        .child(INFO)
        .unwrap_or(Loc::root(AreaId::Trash))
}

/// Makes sure `files/` and `info/` exist.
pub fn prepare(fs: &Fs) -> Result<(), FsError> {
    for d in [files_dir(), info_dir()] {
        match fs.mkdir(&d) {
            Ok(()) | Err(FsError::Exists) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// The record text for `origin` deleted at `now`.
pub fn record(origin: &Loc, now: i64) -> String {
    format!(
        "[Trash Info]\nPath={}\nDeletionDate={now}\n",
        escape(&origin.encode())
    )
}

/// Reads a record (hostile input: unknown lines are ignored, the path is checked).
pub fn parse_record(text: &str) -> Result<(Loc, i64), FsError> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("[Trash Info]") {
        return Err(FsError::BadRecord);
    }
    let (mut path, mut date) = (None, None);
    for line in lines {
        if let Some(v) = line.strip_prefix("Path=") {
            path = Some(Loc::decode(&unescape(v)?)?);
        } else if let Some(v) = line.strip_prefix("DeletionDate=") {
            date = v.trim().parse::<i64>().ok();
        }
    }
    Ok((
        path.ok_or(FsError::BadRecord)?,
        date.ok_or(FsError::BadRecord)?,
    ))
}

fn escape(text: &str) -> String {
    let mut out = Vec::with_capacity(text.len());
    for b in text.bytes() {
        if matches!(b, b'%' | b'\n' | b'\r') {
            out.extend_from_slice(format!("%{b:02X}").as_bytes());
        } else {
            out.push(b);
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

fn unescape(text: &str) -> Result<String, FsError> {
    let raw = text.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' {
            let hex = raw.get(i + 1..i + 3).ok_or(FsError::BadRecord)?;
            let s = std::str::from_utf8(hex).map_err(|_| FsError::BadRecord)?;
            out.push(u8::from_str_radix(s, 16).map_err(|_| FsError::BadRecord)?);
            i += 3;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| FsError::BadRecord)
}

/// Moves `items` to the trash; `done` pairs each source with its trash id (as a location).
pub fn put(job: &mut Job<'_>, fs: &Fs, items: &[Loc], now: i64) -> Report {
    let mut report = Report::default();
    if let Err(e) = prepare(fs) {
        report.failed = items.iter().map(|i| (i.clone(), e)).collect();
        return report;
    }
    for item in items {
        match put_one(job, fs, item, now) {
            Ok(id) => report.done.push((item.clone(), id)),
            Err(FsError::Cancelled) => {
                report.cancelled = true;
                break;
            }
            Err(e) => report.failed.push((item.clone(), e)),
        }
    }
    report
}

fn put_one(job: &mut Job<'_>, fs: &Fs, item: &Loc, now: i64) -> Result<Loc, FsError> {
    if item.area == AreaId::Trash || item.parts.is_empty() {
        return Err(FsError::Permission);
    }
    let name = item.name().ok_or(FsError::BadName)?;
    let (info, files) = (info_dir(), files_dir());
    let id = free_name(name, |n| {
        files.child(n).is_ok_and(|c| fs.exists(&c))
            || info
                .child(&format!("{n}{SUFFIX}"))
                .is_ok_and(|c| fs.exists(&c))
    })
    .ok_or(FsError::Exists)?;
    let record_loc = info.child(&format!("{id}{SUFFIX}"))?;
    write_record(fs, &record_loc, &record(item, now))?;
    let to = files.child(&id)?;
    let result = job.move_as(item, &to).map(|()| to);
    if result.is_err() {
        let _ = fs.remove_one(&record_loc, false);
    }
    result
}

fn write_record(fs: &Fs, loc: &Loc, text: &str) -> Result<(), FsError> {
    let (dir, name) = fs.parent(loc)?;
    let mut f = Fs::create_in(&dir, &name, 0o600)?;
    let done = f
        .write_all(text.as_bytes())
        .and_then(|()| f.sync_all())
        .map_err(|e| FsError::from_io(&e));
    if done.is_err() {
        let _ = fs.remove_one(loc, false);
    }
    done
}

fn read_record(fs: &Fs, id: &str) -> Result<(Loc, i64), FsError> {
    let loc = info_dir().child(&format!("{id}{SUFFIX}"))?;
    let mut text = String::new();
    fs.open_read(&loc)?
        .take(MAX_RECORD)
        .read_to_string(&mut text)
        .map_err(|_| FsError::BadRecord)?;
    parse_record(&text)
}

/// The items in the trash, newest first. Items without a readable record are listed with
/// their own name as origin in the trash itself (they can be deleted, not restored).
pub fn list(fs: &Fs) -> Result<Vec<Trashed>, FsError> {
    let files = files_dir();
    let entries = match fs.list(&files) {
        Err(FsError::NotFound) => return Ok(Vec::new()),
        other => other?,
    };
    let mut out: Vec<Trashed> = entries
        .into_iter()
        .map(|e| {
            let (origin, deleted) = read_record(fs, &e.name)
                .unwrap_or_else(|_| (files.child(&e.name).unwrap_or(files.clone()), 0));
            Trashed {
                id: e.name,
                origin,
                deleted,
                kind: e.kind,
                size: e.size,
            }
        })
        .collect();
    out.sort_by(|a, b| b.deleted.cmp(&a.deleted).then(a.id.cmp(&b.id)));
    Ok(out)
}

/// Puts trashed items back where they were (a taken name gets a free one; a missing folder
/// on the way is created). `done` pairs the trash location with the restored one.
pub fn restore(job: &mut Job<'_>, fs: &Fs, ids: &[String]) -> Report {
    let mut report = Report::default();
    for id in ids {
        let from = match files_dir().child(id) {
            Ok(f) => f,
            Err(e) => {
                report.failed.push((files_dir(), e));
                continue;
            }
        };
        match restore_one(job, fs, id, &from) {
            Ok(to) => report.done.push((from, to)),
            Err(FsError::Cancelled) => {
                report.cancelled = true;
                break;
            }
            Err(e) => report.failed.push((from, e)),
        }
    }
    report
}

fn restore_one(job: &mut Job<'_>, fs: &Fs, id: &str, from: &Loc) -> Result<Loc, FsError> {
    let (origin, _) = read_record(fs, id)?;
    if origin.area == AreaId::Trash {
        return Err(FsError::BadRecord);
    }
    let folder = origin.parent().ok_or(FsError::BadRecord)?;
    make_dirs(fs, &folder)?;
    let name = origin.name().ok_or(FsError::BadRecord)?;
    let free =
        free_name(name, |n| folder.child(n).is_ok_and(|c| fs.exists(&c))).ok_or(FsError::Exists)?;
    let to = folder.child(&free)?;
    job.move_as(from, &to)?;
    let _ = fs.remove_one(&info_dir().child(&format!("{id}{SUFFIX}"))?, false);
    Ok(to)
}

fn make_dirs(fs: &Fs, folder: &Loc) -> Result<(), FsError> {
    let mut at = Loc::root(folder.area.clone());
    for part in &folder.parts {
        at = at.child(part)?;
        match fs.mkdir(&at) {
            Ok(()) | Err(FsError::Exists) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Deletes trashed items for good (`ids`), with their records.
pub fn delete(job: &mut Job<'_>, ids: &[String]) -> Report {
    let mut report = Report::default();
    for id in ids {
        let (Ok(item), Ok(rec)) = (
            files_dir().child(id),
            info_dir().child(&format!("{id}{SUFFIX}")),
        ) else {
            report.failed.push((files_dir(), FsError::BadName));
            continue;
        };
        match job.remove_tree(&item, 0) {
            Ok(()) => {
                let _ = job.remove_tree(&rec, 0);
                report.done.push((item.clone(), item));
            }
            Err(FsError::Cancelled) => {
                report.cancelled = true;
                break;
            }
            Err(e) => report.failed.push((item, e)),
        }
    }
    report
}

/// Empties the trash: every item and every record (also records without an item).
pub fn empty(job: &mut Job<'_>, fs: &Fs) -> Report {
    let ids: Vec<String> = list(fs)
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.id)
        .collect();
    let report = delete(job, &ids);
    if !report.cancelled {
        for e in fs.list(&info_dir()).unwrap_or_default() {
            if let Ok(rec) = info_dir().child(&e.name) {
                let _ = job.remove_tree(&rec, 0);
            }
        }
    }
    report
}
