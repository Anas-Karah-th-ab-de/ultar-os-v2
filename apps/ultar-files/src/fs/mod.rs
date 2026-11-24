//! File access of the app: only inside the named areas the cell grants, one path component
//! at a time from an open directory, never following a symbolic link.
//!
//! [`Fs`] holds one open directory per area; a [`Loc`] is an area plus checked name
//! components. Nothing here formats text for people: errors are [`FsError`] values whose
//! [`FsError::key`] is an i18n key.

mod area;
pub mod copy;
mod name;
mod safe;
pub mod search;
pub mod thumb;
pub mod trash;
pub mod undo;

pub use area::{AreaId, AREAS};
pub use name::{check_name, free_name, MAX_NAME};
pub use safe::Fs;

use std::fmt;

/// Deepest folder nesting the app walks (copy, delete, search).
pub const MAX_DEPTH: usize = 64;
/// Most entries one folder listing returns.
pub const MAX_ENTRIES: usize = 50_000;

/// A place inside an area: the area and the names of the folders and the item below it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Loc {
    /// The area.
    pub area: AreaId,
    /// Checked names (no `/`, no NUL, not `.` or `..`, at most [`MAX_NAME`] bytes).
    pub parts: Vec<String>,
}

impl Loc {
    /// The root folder of `area`.
    pub fn root(area: AreaId) -> Loc {
        Loc {
            area,
            parts: Vec::new(),
        }
    }

    /// The item `name` in this folder (the name is checked).
    pub fn child(&self, name: &str) -> Result<Loc, FsError> {
        check_name(name)?;
        if self.parts.len() >= MAX_DEPTH {
            return Err(FsError::TooDeep);
        }
        let mut parts = self.parts.clone();
        parts.push(name.to_owned());
        Ok(Loc {
            area: self.area.clone(),
            parts,
        })
    }

    /// The folder holding this item (`None` for an area root).
    pub fn parent(&self) -> Option<Loc> {
        let mut parts = self.parts.clone();
        parts.pop()?;
        Some(Loc {
            area: self.area.clone(),
            parts,
        })
    }

    /// The last name (`None` for an area root).
    pub fn name(&self) -> Option<&str> {
        self.parts.last().map(String::as_str)
    }

    /// `true` when `other` is this place or lies below it.
    pub fn contains(&self, other: &Loc) -> bool {
        self.area == other.area && other.parts.starts_with(&self.parts)
    }

    /// `area:a/b/c` – the form kept in the trash's records and in `files://` URIs.
    pub fn encode(&self) -> String {
        format!("{}:{}", self.area.name(), self.parts.join("/"))
    }

    /// Reads [`Loc::encode`]'s form; every part is checked like a typed name.
    pub fn decode(text: &str) -> Result<Loc, FsError> {
        let (area, path) = text.split_once(':').ok_or(FsError::BadRecord)?;
        let area = AreaId::parse(area).ok_or(FsError::BadRecord)?;
        let mut loc = Loc::root(area);
        for part in path.split('/').filter(|p| !p.is_empty()) {
            loc = loc.child(part).map_err(|_| FsError::BadRecord)?;
        }
        Ok(loc)
    }
}

/// What an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// A folder.
    Folder,
    /// A regular file.
    File,
    /// A symbolic link (shown, never followed).
    Link,
    /// Anything else (device, socket, pipe): shown, not opened.
    Other,
}

/// One item of a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its name.
    pub name: String,
    /// What it is (of the item itself, links not followed).
    pub kind: Kind,
    /// Size in bytes (files; 0 otherwise).
    pub size: u64,
    /// Last change, seconds since 1970.
    pub modified: i64,
    /// Permission bits (`0o777` part of the mode).
    pub mode: u32,
}

/// Why a file operation failed – each with an i18n key for the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FsError {
    /// The item does not exist (any more).
    NotFound,
    /// An item of that name exists already.
    Exists,
    /// The system refused: no permission or a read-only place.
    Permission,
    /// A symbolic link was in the way and is never followed.
    Link,
    /// A folder was expected.
    NotFolder,
    /// The disk is full.
    NoSpace,
    /// The folder is not empty.
    NotEmpty,
    /// The name is empty, `.`/`..`, too long or holds `/` or NUL.
    BadName,
    /// The area is not granted to this app (or not mounted).
    NoArea,
    /// A folder cannot be put inside itself.
    IntoItself,
    /// Nesting deeper than [`MAX_DEPTH`] or a folder with more than [`MAX_ENTRIES`] items.
    TooDeep,
    /// A record of the trash is damaged.
    BadRecord,
    /// The item is in use (a busy mount point).
    Busy,
    /// The user cancelled.
    Cancelled,
    /// Another input/output error.
    Io,
}

impl FsError {
    /// The i18n key of the message for the user.
    pub fn key(self) -> &'static str {
        match self {
            FsError::NotFound => "files.error.not_found",
            FsError::Exists => "files.error.exists",
            FsError::Permission => "files.error.permission",
            FsError::Link => "files.error.link",
            FsError::NotFolder => "files.error.not_folder",
            FsError::NoSpace => "files.error.no_space",
            FsError::NotEmpty => "files.error.not_empty",
            FsError::BadName => "files.error.bad_name",
            FsError::NoArea => "files.error.no_area",
            FsError::IntoItself => "files.error.into_itself",
            FsError::TooDeep => "files.error.too_deep",
            FsError::BadRecord => "files.error.bad_record",
            FsError::Busy => "files.error.busy",
            FsError::Cancelled => "files.error.cancelled",
            FsError::Io => "files.error.io",
        }
    }

    pub(crate) fn from_errno(e: rustix::io::Errno) -> FsError {
        use rustix::io::Errno;
        match e {
            Errno::NOENT => FsError::NotFound,
            Errno::EXIST => FsError::Exists,
            Errno::ACCESS | Errno::PERM | Errno::ROFS => FsError::Permission,
            Errno::LOOP => FsError::Link,
            Errno::NOTDIR => FsError::NotFolder,
            Errno::NOSPC | Errno::DQUOT => FsError::NoSpace,
            Errno::NOTEMPTY => FsError::NotEmpty,
            Errno::NAMETOOLONG | Errno::INVAL => FsError::BadName,
            Errno::BUSY => FsError::Busy,
            _ => FsError::Io,
        }
    }

    pub(crate) fn from_io(e: &std::io::Error) -> FsError {
        e.raw_os_error().map_or(FsError::Io, |n| {
            FsError::from_errno(rustix::io::Errno::from_raw_os_error(n))
        })
    }
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

impl std::error::Error for FsError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_encode_and_decode_with_checked_parts() {
        let loc = Loc::root(AreaId::Documents)
            .child("عقود")
            .unwrap()
            .child("a b.txt")
            .unwrap();
        assert_eq!(loc.encode(), "documents:عقود/a b.txt");
        assert_eq!(Loc::decode(&loc.encode()).unwrap(), loc);
        assert_eq!(loc.parent().unwrap().name(), Some("عقود"));
        assert!(Loc::root(AreaId::Documents).contains(&loc));
        assert!(!loc.contains(&Loc::root(AreaId::Documents)));
        for bad in [
            "documents:a/../b",
            "nowhere:a",
            "documents",
            "documents:a/\0",
        ] {
            assert_eq!(Loc::decode(bad), Err(FsError::BadRecord), "{bad}");
        }
        assert_eq!(Loc::root(AreaId::Home).child(".."), Err(FsError::BadName));
    }

    #[test]
    fn errors_have_keys_and_errno_mapping() {
        use rustix::io::Errno;
        assert_eq!(FsError::from_errno(Errno::LOOP), FsError::Link);
        assert_eq!(FsError::from_errno(Errno::ROFS), FsError::Permission);
        assert_eq!(FsError::from_errno(Errno::XDEV), FsError::Io);
        assert_eq!(FsError::NoSpace.to_string(), "files.error.no_space");
    }
}
