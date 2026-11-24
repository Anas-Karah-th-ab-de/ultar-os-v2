//! [`Fs`]: one open directory per granted area; every access walks from it one name at a
//! time with `O_NOFOLLOW`, so a symbolic link anywhere on the way stops the walk instead of
//! leading out of the area.

use super::{AreaId, Entry, FsError, Kind, Loc, AREAS, MAX_ENTRIES};
use rustix::fs::{AtFlags, FileType, Mode, OFlags};
use std::fs::File;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::Path;

type R<T> = Result<T, FsError>;

const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// The granted areas, each as an open directory.
#[derive(Debug, Default)]
pub struct Fs {
    roots: Vec<(AreaId, OwnedFd)>,
}

fn errno(e: rustix::io::Errno) -> FsError {
    FsError::from_errno(e)
}

impl Fs {
    /// The areas found below `root` (the cell's `/areas`): each fixed area at its
    /// [`AreaId::dir_name`], each drive below `removable/`. Missing areas are left out; a
    /// symbolic link in place of an area is refused.
    pub fn open(root: &Path) -> Fs {
        let mut fs = Fs::default();
        let Ok(base) = rustix::fs::open(root, DIR_FLAGS, Mode::empty()) else {
            return fs;
        };
        for area in AREAS {
            if let Ok(fd) =
                rustix::fs::openat(&base, area.dir_name().as_str(), DIR_FLAGS, Mode::empty())
            {
                fs.roots.push((area, fd));
            }
        }
        fs.refresh_drives(&base);
        fs
    }

    fn refresh_drives(&mut self, base: &OwnedFd) {
        self.roots.retain(|(a, _)| !matches!(a, AreaId::Drive(_)));
        let Ok(removable) = rustix::fs::openat(base, "removable", DIR_FLAGS, Mode::empty()) else {
            return;
        };
        let Ok(names) = list_names(&removable) else {
            return;
        };
        for name in names {
            if let Ok(fd) = rustix::fs::openat(&removable, name.as_str(), DIR_FLAGS, Mode::empty())
            {
                self.roots.push((AreaId::Drive(name), fd));
            }
        }
    }

    /// An area from an already open directory (tests, the desktop's own area).
    pub fn with_area(mut self, area: AreaId, dir: &Path) -> R<Fs> {
        let fd = rustix::fs::open(dir, DIR_FLAGS, Mode::empty()).map_err(errno)?;
        self.roots.retain(|(a, _)| *a != area);
        self.roots.push((area, fd));
        Ok(self)
    }

    /// Forgets a drive that was ejected or pulled.
    pub fn drop_area(&mut self, area: &AreaId) {
        self.roots.retain(|(a, _)| a != area);
    }

    /// The granted areas, in sidebar order.
    pub fn areas(&self) -> Vec<AreaId> {
        let mut v: Vec<AreaId> = self.roots.iter().map(|(a, _)| a.clone()).collect();
        v.sort();
        v
    }

    /// `true` when `area` is granted.
    pub fn has(&self, area: &AreaId) -> bool {
        self.roots.iter().any(|(a, _)| a == area)
    }

    fn root(&self, area: &AreaId) -> R<BorrowedFd<'_>> {
        self.roots
            .iter()
            .find(|(a, _)| a == area)
            .map(|(_, fd)| fd.as_fd())
            .ok_or(FsError::NoArea)
    }

    /// Opens the folder `loc` (every step without following links).
    pub fn dir(&self, loc: &Loc) -> R<OwnedFd> {
        let root = self.root(&loc.area)?;
        let mut fd = rustix::fs::openat(root, ".", DIR_FLAGS, Mode::empty()).map_err(errno)?;
        for part in &loc.parts {
            fd = match rustix::fs::openat(&fd, part.as_str(), DIR_FLAGS, Mode::empty()) {
                Ok(next) => next,
                // O_NOFOLLOW on a link: ELOOP, or ENOTDIR together with O_DIRECTORY.
                Err(_) if is_link_in(&fd, part) => return Err(FsError::Link),
                Err(e) => return Err(errno(e)),
            };
        }
        Ok(fd)
    }

    /// The folder holding `loc` and `loc`'s name (an area root has no parent).
    pub fn parent(&self, loc: &Loc) -> R<(OwnedFd, String)> {
        let parent = loc.parent().ok_or(FsError::BadName)?;
        let name = loc.name().ok_or(FsError::BadName)?.to_owned();
        Ok((self.dir(&parent)?, name))
    }

    /// What `loc` is (the item itself; a link is reported as a link).
    pub fn stat(&self, loc: &Loc) -> R<Entry> {
        if loc.parts.is_empty() {
            let fd = self.dir(loc)?;
            let st = rustix::fs::fstat(&fd).map_err(errno)?;
            return Ok(entry(loc.area.name(), &st));
        }
        let (dir, name) = self.parent(loc)?;
        stat_in(&dir, &name)
    }

    /// The items of the folder `loc` (at most [`MAX_ENTRIES`]; more is [`FsError::TooDeep`]).
    pub fn list(&self, loc: &Loc) -> R<Vec<Entry>> {
        let dir = self.dir(loc)?;
        let names = list_names(&dir)?;
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            match stat_in(&dir, &name) {
                Ok(e) => out.push(e),
                Err(FsError::NotFound) => {} // gone meanwhile
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    /// Opens the file `loc` for reading (a link or a non-file is refused).
    pub fn open_read(&self, loc: &Loc) -> R<File> {
        let (dir, name) = self.parent(loc)?;
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
        let fd = rustix::fs::openat(&dir, name.as_str(), flags, Mode::empty()).map_err(errno)?;
        let st = rustix::fs::fstat(&fd).map_err(errno)?;
        if FileType::from_raw_mode(st.st_mode) != FileType::RegularFile {
            return Err(FsError::NotFound);
        }
        Ok(File::from(fd))
    }

    /// Creates the file `name` in the open folder `dir`; fails when anything has that name.
    pub fn create_in(dir: &OwnedFd, name: &str, mode: u32) -> R<File> {
        super::check_name(name)?;
        let flags =
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let fd = rustix::fs::openat(dir, name, flags, Mode::from_raw_mode(mode & 0o777))
            .map_err(errno)?;
        Ok(File::from(fd))
    }

    /// Creates the folder `loc`.
    pub fn mkdir(&self, loc: &Loc) -> R<()> {
        let (dir, name) = self.parent(loc)?;
        rustix::fs::mkdirat(&dir, name.as_str(), Mode::from_raw_mode(0o755)).map_err(errno)
    }

    /// Renames `from` to `to` inside one file system; an existing `to` is never replaced
    /// unless `replace` (then a file replaces a file atomically).
    pub fn rename(&self, from: &Loc, to: &Loc, replace: bool) -> R<()> {
        let (fd, fname) = self.parent(from)?;
        let (td, tname) = self.parent(to)?;
        if !replace && exists_in(&td, &tname) {
            return Err(FsError::Exists);
        }
        rustix::fs::renameat(&fd, fname.as_str(), &td, tname.as_str()).map_err(|e| match e {
            rustix::io::Errno::XDEV => FsError::Busy,
            other => errno(other),
        })
    }

    /// `true` when `loc` exists (links count, they are items too).
    pub fn exists(&self, loc: &Loc) -> bool {
        self.parent(loc).is_ok_and(|(d, n)| exists_in(&d, &n))
    }

    /// Removes the file, link or empty folder `loc`.
    pub fn remove_one(&self, loc: &Loc, folder: bool) -> R<()> {
        let (dir, name) = self.parent(loc)?;
        let flags = if folder {
            AtFlags::REMOVEDIR
        } else {
            AtFlags::empty()
        };
        rustix::fs::unlinkat(&dir, name.as_str(), flags).map_err(errno)
    }

    /// Where the link `loc` points (raw bytes; never followed).
    pub fn read_link(&self, loc: &Loc) -> R<Vec<u8>> {
        let (dir, name) = self.parent(loc)?;
        rustix::fs::readlinkat(&dir, name.as_str(), Vec::new())
            .map(|c| c.into_bytes())
            .map_err(errno)
    }

    /// Makes `loc` a link to `target` (a copied link stays a link).
    pub fn make_link(&self, target: &[u8], loc: &Loc) -> R<()> {
        let (dir, name) = self.parent(loc)?;
        let target: &std::ffi::OsStr = std::os::unix::ffi::OsStrExt::from_bytes(target);
        rustix::fs::symlinkat(target, &dir, name.as_str()).map_err(errno)
    }

    /// Changes the permission bits of `loc` (not through links).
    pub fn set_mode(&self, loc: &Loc, mode: u32) -> R<()> {
        let (dir, name) = self.parent(loc)?;
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
        let fd = rustix::fs::openat(&dir, name.as_str(), flags, Mode::empty()).map_err(errno)?;
        rustix::fs::fchmod(&fd, Mode::from_raw_mode(mode & 0o777)).map_err(errno)
    }
}

fn is_link_in(dir: &OwnedFd, name: &str) -> bool {
    rustix::fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW)
        .is_ok_and(|st| FileType::from_raw_mode(st.st_mode) == FileType::Symlink)
}

fn exists_in(dir: &OwnedFd, name: &str) -> bool {
    rustix::fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW).is_ok()
}

fn stat_in(dir: &OwnedFd, name: &str) -> R<Entry> {
    let st = rustix::fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW).map_err(errno)?;
    Ok(entry(name.to_owned(), &st))
}

fn entry(name: String, st: &rustix::fs::Stat) -> Entry {
    let kind = match FileType::from_raw_mode(st.st_mode) {
        FileType::Directory => Kind::Folder,
        FileType::RegularFile => Kind::File,
        FileType::Symlink => Kind::Link,
        _ => Kind::Other,
    };
    #[allow(clippy::unnecessary_cast)] // c_long: 32 bits on some targets
    let modified = st.st_mtime as i64;
    Entry {
        name,
        kind,
        size: if kind == Kind::File {
            u64::try_from(st.st_size).unwrap_or(0)
        } else {
            0
        },
        modified,
        mode: st.st_mode & 0o7777,
    }
}

/// Names in the open folder `dir` (not `.`/`..`; names that are not UTF-8 or not valid
/// single names are left out – they cannot be shown or typed).
pub(crate) fn list_names(dir: &OwnedFd) -> R<Vec<String>> {
    let fd = rustix::fs::openat(dir, ".", DIR_FLAGS, Mode::empty()).map_err(errno)?;
    let mut reader = rustix::fs::Dir::read_from(&fd).map_err(errno)?;
    let mut names = Vec::new();
    for item in reader.by_ref() {
        let item = item.map_err(errno)?;
        let Ok(name) = item.file_name().to_str() else {
            continue;
        };
        if name == "." || name == ".." || super::check_name(name).is_err() {
            continue;
        }
        if names.len() >= MAX_ENTRIES {
            return Err(FsError::TooDeep);
        }
        names.push(name.to_owned());
    }
    Ok(names)
}
