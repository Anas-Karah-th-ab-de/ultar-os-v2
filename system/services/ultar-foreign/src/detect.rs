//! What a file of the person is – from its first bytes, never from its name alone.
//!
//! Only the head is read here (the service never parses a package's insides itself:
//! the fields of a `.deb` are read by the Linux area's own `dpkg-deb` in a cell without
//! network or rights, see `cellrun.rs`).

use std::io::Read;
use std::path::Path;
use ultar_bus::messages::foreign::Source;

/// Largest file the service takes (an installer of several GiB is refused).
pub const MAX_FILE: u64 = 2 << 30;
/// Bytes read from the head.
const HEAD: usize = 4096;

/// What the head of a file says.
pub fn source_of(head: &[u8], name: &str) -> Source {
    let lower = name.to_ascii_lowercase();
    if head.starts_with(b"!<arch>\ndebian-binary   ") {
        return Source::Deb;
    }
    if head.starts_with(b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1") && lower.ends_with(".msi") {
        return Source::Msi;
    }
    if head.starts_with(b"MZ") && is_pe(head) {
        return Source::Exe;
    }
    if head.starts_with(b"PK\x03\x04") && lower.ends_with(".apk") {
        return Source::Apk;
    }
    Source::Unknown
}

/// A Windows program: the DOS header points at a `PE\0\0` signature inside the head.
fn is_pe(head: &[u8]) -> bool {
    let Some(at) = head.get(0x3c..0x40) else {
        return false;
    };
    let offset = u32::from_le_bytes([at[0], at[1], at[2], at[3]]) as usize;
    head.get(offset..offset + 4) == Some(b"PE\0\0".as_slice())
}

/// Where the service takes files from: the person's file areas and mounted drives.
/// Anything else (the service's own state, the system) is refused.
pub const SOURCES: [&str; 2] = ["/data/ultar/areas", "/run/ultar/media"];

/// A file of the person, opened.
#[derive(Debug)]
pub struct Opened {
    /// The open file (read from its start).
    pub file: std::fs::File,
    /// Its size in bytes.
    pub size: u64,
    /// What its head says.
    pub source: Source,
}

/// Opens `path` below one of `roots`, one component at a time without following any
/// link (an app with a file area cannot point the service elsewhere by a link it
/// planted), and only a plain file. Reads its head and rewinds.
pub fn open_source(roots: &[std::path::PathBuf], path: &str) -> std::io::Result<Opened> {
    use rustix::fs::{fstat, openat, FileType, Mode, OFlags, CWD};
    use std::io::{Error, ErrorKind, Seek, SeekFrom};
    if !path_ok(path) {
        return Err(Error::from(ErrorKind::InvalidInput));
    }
    let wanted = Path::new(path);
    let (root, rest) = roots
        .iter()
        .find_map(|r| {
            let rest = wanted.strip_prefix(r).ok()?;
            (rest.components().next().is_some()).then_some((r, rest))
        })
        .ok_or_else(|| Error::from(ErrorKind::PermissionDenied))?;
    let dirs = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
    let mut dir = openat(CWD, root, dirs, Mode::empty())?;
    let parts: Vec<_> = rest.components().collect();
    let (last, inner) = parts
        .split_last()
        .ok_or_else(|| Error::from(ErrorKind::InvalidInput))?;
    for part in inner {
        dir = openat(
            &dir,
            part.as_os_str(),
            dirs | OFlags::NOFOLLOW,
            Mode::empty(),
        )?;
    }
    // NONBLOCK: a planted pipe must not hold the service
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
    let fd = openat(&dir, last.as_os_str(), flags, Mode::empty())?;
    let stat = fstat(&fd)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err(Error::from(ErrorKind::InvalidInput));
    }
    let mut file = std::fs::File::from(fd);
    let mut head = Vec::with_capacity(HEAD);
    (&mut file).take(HEAD as u64).read_to_end(&mut head)?;
    file.seek(SeekFrom::Start(0))?;
    let name = wanted
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    Ok(Opened {
        size: u64::try_from(stat.st_size).unwrap_or(u64::MAX),
        source: source_of(&head, name),
        file,
    })
}

/// A path the service accepts from a caller: absolute, normalised, not too long.
pub fn path_ok(path: &str) -> bool {
    path.len() <= ultar_bus::messages::foreign::MAX_PATH
        && path.starts_with('/')
        && !path.contains('\0')
        && path[1..]
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

/// The file's name without the kind (`mousepad_0.6.2_amd64.deb` → `mousepad`) – a
/// first name until the package says its own.
pub fn stem(path: &Path) -> String {
    let name = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    name.split('_')
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pe() -> Vec<u8> {
        let mut h = vec![0u8; 256];
        h[..2].copy_from_slice(b"MZ");
        h[0x3c..0x40].copy_from_slice(&128u32.to_le_bytes());
        h[128..132].copy_from_slice(b"PE\0\0");
        h
    }

    #[test]
    fn kinds_come_from_the_head() {
        let deb = b"!<arch>\ndebian-binary   1700000000  0     0     100644  4         `\n2.0\n";
        assert_eq!(source_of(deb, "x.zip"), Source::Deb);
        assert_eq!(source_of(&pe(), "setup.exe"), Source::Exe);
        assert_eq!(source_of(b"MZ just text", "setup.exe"), Source::Unknown);
        let ole = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1rest";
        assert_eq!(source_of(ole, "Setup.MSI"), Source::Msi);
        assert_eq!(source_of(ole, "letter.doc"), Source::Unknown);
        assert_eq!(source_of(b"PK\x03\x04....", "app.apk"), Source::Apk);
        assert_eq!(source_of(b"PK\x03\x04....", "photos.zip"), Source::Unknown);
        assert_eq!(source_of(b"", "a.deb"), Source::Unknown);
    }

    #[test]
    fn paths_and_names() {
        assert!(path_ok("/data/ultar/areas/downloads/a.deb"));
        for bad in [
            "a.deb",
            "/data/../etc/shadow",
            "/data//a",
            "/x/./y",
            "",
            "/a\0b",
        ] {
            assert!(!path_ok(bad), "{bad:?}");
        }
        assert_eq!(stem(Path::new("/d/mousepad_0.6.2-1_amd64.deb")), "mousepad");
        assert_eq!(stem(Path::new("/d/setup.exe")), "setup");
    }

    #[test]
    fn only_plain_files_below_the_sources_open_and_links_are_not_followed() {
        let base = std::env::temp_dir().join(format!("ufd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let area = base.join("areas");
        std::fs::create_dir_all(area.join("downloads")).unwrap();
        std::fs::write(base.join("secret"), b"!<arch>\ndebian-binary   x").unwrap();
        std::fs::write(area.join("downloads/a.deb"), b"!<arch>\ndebian-binary   x").unwrap();
        std::os::unix::fs::symlink(base.join("secret"), area.join("downloads/l.deb")).unwrap();
        std::os::unix::fs::symlink(&base, area.join("up")).unwrap();
        let roots = [area.clone()];
        let at = |p: &str| area.join(p).to_str().unwrap().to_owned();
        let opened = open_source(&roots, &at("downloads/a.deb")).unwrap();
        assert_eq!(opened.source, Source::Deb);
        assert_eq!(opened.size, 25);
        // the head was rewound
        let mut all = Vec::new();
        (&opened.file).read_to_end(&mut all).unwrap();
        assert_eq!(all.len(), 25);
        for refused in [at("downloads/l.deb"), at("up/secret"), at("downloads")] {
            assert!(open_source(&roots, &refused).is_err(), "{refused}");
        }
        let outside = base.join("secret");
        let e = open_source(&roots, outside.to_str().unwrap()).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(open_source(&roots, area.to_str().unwrap()).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
