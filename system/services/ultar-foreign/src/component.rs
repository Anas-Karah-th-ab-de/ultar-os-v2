//! The component „Linux-Programme“ (with Wine: „Windows-Programme“): the Linux area as a
//! signed component, installed from a drive or the person's areas (where the Ultar Store
//! puts its downloads) and unpacked by the service into `<root>/linux`
//! (`docs/integration/ultar-foreign.md`, „Der Linux-Bereich als Komponente“).
//!
//! A component is two files side by side:
//!
//! - `foreign.linux-runtime-<version>-<arch>.uap` – small and signed (ultar-pkg, kind `data`,
//!   checked against the device's built-in keys, `/etc/ultar/trusted-keys`). Its manifest
//!   names the archive, its size, its SHA-256 and the size of the unpacked tree
//!   (`[component]`).
//! - the archive (`foreign.linux-runtime-<version>-<arch>.tar.gz`) of any size: the package
//!   limits of the launcher and the Store and ultar-pkg's reading in memory only ever see
//!   the small file; the archive is streamed.
//!
//! Order of [`update`] (each step published on `foreign.progress`):
//!
//! 1. **Prepare** – the newest component of this device's architecture whose signature a
//!    built-in key confirms (each `.uap` is copied into the inbox first, opened without
//!    following links, so no link planted in a file area points the service elsewhere).
//! 2. **Confirm** – an administrator confirms in the system dialog (name, version, signer).
//! 3. **Prepare** – the archive is copied into the inbox while its SHA-256 is counted; only
//!    a copy that matches the signed digest goes on (what is unpacked is exactly what was
//!    checked, whatever happens to the drive meanwhile).
//! 4. **Install** – the unpacking cell ([`Mode::Unpack`]: root of the cell, the new area its
//!    only writable mount, no network, no `/proc`, no device nodes) unpacks into
//!    `<root>/linux.new`; device nodes, fifos and the insides of `proc`/`sys`/`dev`/`run`/`tmp`
//!    are left out, set-id bits are taken away. Its `etc/ultar-runtime` must name the signed
//!    version and components.
//! 5. The old area moves aside (`linux.old`, emptied in a cell at the service's next start,
//!    when no app of it runs any more), the new one takes its place.
//!
//! An area that holds the person's programs is not replaced (they would be gone): its
//! packages are updated from its sources, which needs the network barrier (open).

use crate::cellrun::{self, CellJob, Mode, INBOX_IN_CELL};
use crate::detect;
use crate::jobs::{self, Ctx};
use crate::layout::{area_ok, field_in};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use ultar_bus::messages::foreign::{Failure, Step};
use ultar_bus::{Client, Text, TextArg};

/// The id of the component in its signed manifest.
pub const ID: &str = "foreign.linux-runtime";
/// The beginning of a component's file name.
pub const PREFIX: &str = "foreign.linux-runtime-";
/// The longest a component's small signed file may be.
pub const MAX_UAP: u64 = 4 << 20;
/// How deep below a source the service looks (a drive's top, `Downloads` of an area).
const DEPTH: usize = 3;
/// How many entries it looks at in all.
const MAX_ENTRIES: usize = 8192;
/// Room left over after the copy and the unpacked tree.
const SPARE: u64 = 256 << 20;
/// Longest wait for the unpacking cell.
const UNPACK_LIMIT: Duration = Duration::from_secs(90 * 60);
/// Longest wait for the cell that empties an area.
const CLEAR_LIMIT: Duration = Duration::from_secs(30 * 60);
/// Top directories of an area whose insides a cell mounts itself.
const MOUNTED: [&str; 5] = ["proc", "sys", "dev", "run", "tmp"];

/// The signed manifest of a component.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    /// [`ID`].
    pub id: String,
    /// Shown in the system dialog.
    #[serde(default)]
    pub name: String,
    /// The area's version (its `etc/ultar-runtime` names the same).
    pub version: String,
    /// `x86_64` or `aarch64`.
    pub arch: String,
    /// `data`.
    pub kind: String,
    /// The archive.
    pub component: Part,
}

/// The archive a manifest pins.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Part {
    /// Its file name, next to the `.uap`.
    pub archive: String,
    /// Its size in bytes.
    pub size: u64,
    /// Its SHA-256 (hex).
    pub sha256: String,
    /// Bytes of the unpacked tree.
    pub unpacked: u64,
    /// `windows` when the area has Wine (as its `etc/ultar-runtime` says).
    #[serde(default)]
    pub components: String,
}

impl Manifest {
    /// Reads and checks the signed bytes: the right id, kind and architecture, a plain
    /// archive name next to the package and a digest of the right form.
    pub fn parse(bytes: &[u8], arch: &str) -> Option<Self> {
        let m: Self = toml::from_str(std::str::from_utf8(bytes).ok()?).ok()?;
        let name_ok = |n: &str| {
            n.starts_with(PREFIX)
                && n.ends_with(".tar.gz")
                && n.len() <= 128
                && n
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        };
        let text_ok = |t: &str| t.len() <= 64 && !t.chars().any(char::is_control);
        (m.id == ID
            && m.kind == "data"
            && m.arch == arch
            && name_ok(&m.component.archive)
            && m.component.sha256.len() == 64
            && m.component.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            && !m.version.is_empty()
            && text_ok(&m.version)
            && text_ok(&m.name)
            && text_ok(&m.component.components))
        .then_some(m)
    }
}

/// Versions compared by their numbers (`26.04-20261120` < `26.04-20261201`).
pub fn version_key(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().unwrap_or(u64::MAX))
        .collect()
}

/// The `.uap` files of components below the sources (plain files only, links never
/// followed, at most [`DEPTH`] levels and [`MAX_ENTRIES`] entries).
pub fn candidates(sources: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut seen = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = sources.iter().map(|s| (s.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                return found;
            }
            let Ok(kind) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().into_owned();
            if kind.is_dir() && depth + 1 < DEPTH && !name.starts_with('.') {
                stack.push((e.path(), depth + 1));
            } else if kind.is_file() && name.starts_with(PREFIX) && name.ends_with(".uap") {
                found.push(e.path());
            }
        }
    }
    found.sort();
    found
}

/// A component whose signature a built-in key confirmed.
#[derive(Clone, Debug)]
pub struct Found {
    /// Its `.uap` (where the person has it).
    pub uap: PathBuf,
    /// Its manifest.
    pub manifest: Manifest,
    /// The trusted key that signed it (its file name in the key directory).
    pub signer: String,
}

/// Copies at most `limit` bytes of an opened file to `to` (`0644`).
fn copy_limited(from: &mut std::fs::File, to: &Path, limit: u64) -> std::io::Result<u64> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(to)?;
    let n = std::io::copy(&mut from.take(limit + 1), &mut out)?;
    if n > limit {
        return Err(std::io::Error::from(std::io::ErrorKind::FileTooLarge));
    }
    out.sync_all()?;
    Ok(n)
}

/// A fresh inbox directory for `job` (`0755`, only the service writes there).
fn job_dir(ctx: &Ctx, job: u64) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = ctx.layout.inbox().join(format!("runtime-{job}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))?;
    Ok(dir)
}

/// The newest valid component for this device below the sources.
pub fn newest(ctx: &Ctx, dir: &Path) -> Option<Found> {
    let mut best: Option<Found> = None;
    for (i, uap) in candidates(&ctx.sources).into_iter().enumerate() {
        let Some(path) = uap.to_str() else { continue };
        let Ok(mut opened) = detect::open_source(&ctx.sources, path) else {
            continue;
        };
        let copy = dir.join(format!("{i}.uap"));
        if copy_limited(&mut opened.file, &copy, MAX_UAP).is_err() {
            let _ = std::fs::remove_file(&copy);
            continue;
        }
        let signed = ultar_pkg::verify_signed(&copy, &ctx.trusted);
        let _ = std::fs::remove_file(&copy);
        let Ok(signed) = signed else { continue };
        let Some(manifest) = Manifest::parse(&signed.bytes, std::env::consts::ARCH) else {
            continue;
        };
        let newer = best.as_ref().is_none_or(|b| {
            version_key(&manifest.version) > version_key(&b.manifest.version)
        });
        if newer {
            best = Some(Found {
                uap,
                manifest,
                signer: signed.key_name,
            });
        }
    }
    best
}

/// Copies the archive of `found` to `to` while counting its SHA-256; fine when the copy
/// has the signed size and digest.
fn fetch(ctx: &Ctx, found: &Found, to: &Path, progress: &dyn Fn(Step, u8)) -> Result<(), Failure> {
    use std::os::unix::fs::OpenOptionsExt;
    let part = &found.manifest.component;
    let archive = found
        .uap
        .parent()
        .map(|p| p.join(&part.archive))
        .ok_or(Failure::Damaged)?;
    let path = archive.to_str().ok_or(Failure::Damaged)?;
    let mut opened = detect::open_source(&ctx.sources, path).map_err(|_| Failure::Damaged)?;
    if opened.size != part.size {
        return Err(Failure::Damaged);
    }
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(to)
        .map_err(|_| Failure::Internal)?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let (mut done, mut shown) = (0u64, 0u8);
    loop {
        let n = opened.file.read(&mut buf).map_err(|_| Failure::Damaged)?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if done > part.size {
            return Err(Failure::Damaged);
        }
        hash.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|_| Failure::Space)?;
        let percent = 10 + (done * 20 / part.size.max(1)) as u8;
        if percent != shown {
            shown = percent;
            progress(Step::Prepare, percent);
        }
    }
    out.sync_all().map_err(|_| Failure::Space)?;
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    (done == part.size && digest.eq_ignore_ascii_case(&part.sha256))
        .then_some(())
        .ok_or(Failure::Damaged)
}

/// Free bytes where the service keeps the area.
fn free(root: &Path) -> u64 {
    rustix::fs::statvfs(root).map_or(0, |s| s.f_bavail.saturating_mul(s.f_frsize))
}

/// Empties an area in a cell (its files belong to root of a cell) and removes it.
pub fn clear(ctx: &Ctx, area: &Path) -> Result<(), Failure> {
    if std::fs::symlink_metadata(area).is_err() {
        return Ok(());
    }
    let cell = CellJob {
        mode: Mode::Clear,
        runtime: area.to_owned(),
        inbox: None,
        program: vec!["/".into()],
    };
    match cellrun::run(&ctx.helper, &cell, CLEAR_LIMIT) {
        Ok(r) if r.code == Some(0) => {}
        _ => return Err(Failure::Internal),
    }
    std::fs::remove_dir(area).map_err(|_| Failure::Internal)
}

/// What [`update`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Updated {
    /// The new area is in place (its version).
    Installed(String),
    /// The area is already this version or newer.
    Unchanged,
    /// The administrator did not confirm.
    Cancelled,
}

/// The confirmation text.
fn confirm_text(found: &Found) -> Text {
    let windows = found.manifest.component.components.split(',').any(|c| c.trim() == "windows");
    Text {
        key: if windows {
            "foreign.confirm.runtime.windows"
        } else {
            "foreign.confirm.runtime"
        }
        .into(),
        args: vec![
            TextArg::Verbatim(found.manifest.version.clone()),
            TextArg::Verbatim(found.signer.chars().take(64).collect()),
        ],
    }
}

/// Installs or replaces the Linux area from the newest component below the sources
/// (on a worker thread). `holds_apps`: programs of the person live in the area.
pub fn update(
    ctx: &Ctx,
    client: &Client,
    job: u64,
    caller: &str,
    holds_apps: bool,
) -> Result<Updated, Failure> {
    jobs::step(client, job, Step::Prepare, 2, Vec::new());
    let dir = job_dir(ctx, job).map_err(|_| Failure::Internal)?;
    let result = update_in(ctx, client, job, caller, holds_apps, &dir);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn update_in(
    ctx: &Ctx,
    client: &Client,
    job: u64,
    caller: &str,
    holds_apps: bool,
    dir: &Path,
) -> Result<Updated, Failure> {
    let layout = &ctx.layout;
    // no component on a drive or in the areas: it comes from the Store (network)
    let found = newest(ctx, dir).ok_or(Failure::Network)?;
    let installed = layout.runtime_installed();
    if installed
        && version_key(&found.manifest.version) <= version_key(&layout.runtime_version())
    {
        return Ok(Updated::Unchanged);
    }
    if installed && holds_apps {
        return Err(Failure::Refused);
    }
    let part = &found.manifest.component;
    let needed = part.size.saturating_add(part.unpacked).saturating_add(SPARE);
    if free(&layout.root) < needed {
        return Err(Failure::Space);
    }
    jobs::step(client, job, Step::Confirm, 5, Vec::new());
    if !jobs::confirm(client, caller, confirm_text(&found))? {
        return Ok(Updated::Cancelled);
    }
    let progress = |step, percent| jobs::step(client, job, step, percent, Vec::new());
    put_in_place(ctx, &found, dir, &progress).map(Updated::Installed)
}

/// Copies, checks and unpacks the archive of `found` (the administrator confirmed) and puts
/// the new area in place of the old one; `dir`: an inbox directory of the job.
pub fn put_in_place(
    ctx: &Ctx,
    found: &Found,
    dir: &Path,
    progress: &dyn Fn(Step, u8),
) -> Result<String, Failure> {
    let layout = &ctx.layout;
    let part = &found.manifest.component;
    let archive = dir.join("runtime.tar.gz");
    let _ = std::fs::remove_file(&archive);
    if let Err(failure) = fetch(ctx, found, &archive, progress) {
        let _ = std::fs::remove_file(&archive);
        return Err(failure);
    }
    progress(Step::Install, 35);
    let new = layout.root.join("linux.new");
    clear(ctx, &new)?;
    std::fs::create_dir(&new).map_err(|_| Failure::Internal)?;
    let cell = CellJob {
        mode: Mode::Unpack,
        runtime: new.clone(),
        inbox: Some(dir.to_owned()),
        program: vec![format!("{INBOX_IN_CELL}/runtime.tar.gz")],
    };
    let unpacked = cellrun::run(&ctx.helper, &cell, UNPACK_LIMIT);
    let _ = std::fs::remove_file(&archive);
    let fresh = area_ok(&new);
    let ok = matches!(unpacked, Ok(ref r) if r.code == Some(0))
        && fresh
        && field_in(&new, "version") == found.manifest.version
        && field_in(&new, "components") == part.components;
    if !ok {
        let _ = clear(ctx, &new);
        return Err(Failure::Damaged);
    }
    progress(Step::Install, 95);
    let current = layout.runtime();
    let old = layout.root.join("linux.old");
    if std::fs::symlink_metadata(&current).is_ok() {
        clear(ctx, &old)?;
        std::fs::rename(&current, &old).map_err(|_| Failure::Internal)?;
    }
    std::fs::rename(&new, &current).map_err(|_| Failure::Internal)?;
    Ok(found.manifest.version.clone())
}

/// Whether `path` (an entry of an archive) is unpacked: relative, normal, not the
/// root itself, not inside a directory a cell mounts.
pub fn entry_ok(path: &Path) -> bool {
    use std::path::Component;
    let mut parts = Vec::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
            _ => return false,
        }
    }
    match parts.as_slice() {
        [] => false,
        [first, _, ..] if MOUNTED.contains(&first.as_str()) => false,
        _ => true,
    }
}

/// Unpacks the gzip'ed tar `archive` into `into` (in the unpacking cell: `/`). Owners are
/// kept when `owners` (root of the cell with `cell_chown`); device nodes, fifos and
/// sockets are left out; set-id bits are taken away.
pub fn unpack(archive: &Path, into: &Path, owners: bool) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let file = std::fs::File::open(archive)?;
    let gz = flate2::read::GzDecoder::new(std::io::BufReader::with_capacity(1 << 20, file));
    let mut ar = tar::Archive::new(gz);
    ar.set_preserve_permissions(true);
    ar.set_preserve_ownerships(owners);
    ar.set_preserve_mtime(true);
    ar.set_unpack_xattrs(false);
    if owners {
        // the area's root belongs to root, as every directory in it
        rustix::fs::chown(into, Some(rustix::fs::Uid::ROOT), Some(rustix::fs::Gid::ROOT))?;
    }
    for entry in ar.entries()? {
        let mut e = entry?;
        let kind = e.header().entry_type();
        if !(kind.is_file() || kind.is_dir() || kind.is_symlink() || kind.is_hard_link()) {
            continue;
        }
        let path = e.path()?.into_owned();
        if !entry_ok(&path) {
            continue;
        }
        if !e.unpack_in(into)? {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
        }
        if kind.is_file() {
            let at = into.join(&path);
            let mode = std::fs::symlink_metadata(&at)?.permissions().mode();
            if mode & 0o6000 != 0 {
                std::fs::set_permissions(&at, std::fs::Permissions::from_mode(mode & 0o1777))?;
            }
        }
    }
    Ok(())
}

/// Empties `dir` (in the cell that clears an area: `/`).
pub fn empty(dir: &Path) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        if e.file_type()?.is_dir() {
            std::fs::remove_dir_all(e.path())?;
        } else {
            std::fs::remove_file(e.path())?;
        }
    }
    Ok(())
}

/// Removes what an earlier replacement left aside (at the service's start, before an app
/// of the old area can run).
pub fn tidy(ctx: &Ctx) {
    for name in ["linux.old", "linux.new"] {
        let _ = clear(ctx, &ctx.layout.root.join(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(arch: &str, archive: &str) -> String {
        format!(
            "id = \"{ID}\"\nname = \"Linux area\"\nversion = \"26.04-20261128\"\narch = \"{arch}\"\n\
             kind = \"data\"\npublisher = \"KM Code Craft\"\nsize = 1\n[component]\n\
             archive = \"{archive}\"\nsize = 10\nsha256 = \"{}\"\nunpacked = 20\ncomponents = \"windows\"\n",
            "ab".repeat(32)
        )
    }

    #[test]
    fn only_a_component_for_this_device_with_a_plain_archive_name_is_taken() {
        let good = "foreign.linux-runtime-26.04-20261128-x86_64.tar.gz";
        let m = Manifest::parse(manifest("x86_64", good).as_bytes(), "x86_64").unwrap();
        assert_eq!(m.component.components, "windows");
        assert!(Manifest::parse(manifest("aarch64", good).as_bytes(), "x86_64").is_none());
        for bad in ["../foreign.linux-runtime-x.tar.gz", "/etc/foreign.linux-runtime-x.tar.gz", "linux.tar.gz", "foreign.linux-runtime-x.tar"] {
            assert!(Manifest::parse(manifest("x86_64", bad).as_bytes(), "x86_64").is_none(), "{bad}");
        }
        let other = manifest("x86_64", good).replace(ID, "editor");
        assert!(Manifest::parse(other.as_bytes(), "x86_64").is_none());
    }

    #[test]
    fn versions_compare_by_their_numbers() {
        assert!(version_key("26.04-20261201") > version_key("26.04-20261128"));
        assert!(version_key("26.10-1") > version_key("26.04-99999999"));
        assert!(version_key("26.04-2") > version_key(""));
    }

    #[test]
    fn entries_outside_the_area_or_inside_mounted_places_are_left_out() {
        for ok in ["usr/bin/sh", "./etc/passwd", "proc", "dev", "tmp/"] {
            assert!(entry_ok(Path::new(ok)), "{ok}");
        }
        for bad in ["", ".", "./", "/etc/passwd", "../x", "usr/../../x", "proc/1/mem", "dev/sda", "run/lock/x"] {
            assert!(!entry_ok(Path::new(bad)), "{bad}");
        }
    }

    #[test]
    fn unpacking_keeps_the_tree_and_drops_devices_links_out_and_set_id_bits() {
        use std::os::unix::fs::PermissionsExt;
        let work = std::env::temp_dir().join(format!("ultar-foreign-unpack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(work.join("into")).unwrap();
        let archive = work.join("a.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(
                std::fs::File::create(&archive).unwrap(),
                flate2::Compression::fast(),
            );
            let mut b = tar::Builder::new(gz);
            let mut add = |path: &str, kind: tar::EntryType, mode: u32, data: &[u8], link: Option<&str>| {
                let mut h = tar::Header::new_gnu();
                h.set_entry_type(kind);
                h.set_mode(mode);
                h.set_size(data.len() as u64);
                if let Some(l) = link {
                    h.set_link_name(l).unwrap();
                }
                h.set_cksum();
                b.append_data(&mut h, path, data).unwrap();
            };
            add("usr/", tar::EntryType::Directory, 0o755, b"", None);
            add("usr/bin/", tar::EntryType::Directory, 0o755, b"", None);
            add("usr/bin/su", tar::EntryType::Regular, 0o4755, b"#!", None);
            add("usr/bin/sh", tar::EntryType::Symlink, 0o777, b"", Some("dash"));
            add("dev/", tar::EntryType::Directory, 0o755, b"", None);
            add("dev/null", tar::EntryType::Char, 0o666, b"", None);
            add("proc/", tar::EntryType::Directory, 0o555, b"", None);
            add("etc/", tar::EntryType::Directory, 0o755, b"", None);
            add("etc/ultar-runtime", tar::EntryType::Regular, 0o644, b"version=1\n", None);
            b.into_inner().unwrap().finish().unwrap();
        }
        let into = work.join("into");
        unpack(&archive, &into, false).unwrap();
        let su = std::fs::metadata(into.join("usr/bin/su")).unwrap();
        assert_eq!(su.permissions().mode() & 0o7777, 0o755);
        assert_eq!(std::fs::read_link(into.join("usr/bin/sh")).unwrap(), Path::new("dash"));
        assert!(into.join("dev").is_dir() && !into.join("dev/null").exists());
        assert!(into.join("proc").is_dir());
        assert_eq!(field_in(&into, "version"), "1");
        empty(&into).unwrap();
        assert_eq!(std::fs::read_dir(&into).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&work);
    }

    #[test]
    fn components_are_found_below_the_sources_and_links_are_not_followed() {
        let work = std::env::temp_dir().join(format!("ultar-foreign-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        let stick = work.join("media/stick/ultar");
        std::fs::create_dir_all(&stick).unwrap();
        std::fs::write(stick.join("foreign.linux-runtime-1-x86_64.uap"), b"x").unwrap();
        std::fs::write(work.join("media/foreign.linux-runtime-2-x86_64.uap.part"), b"x").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", work.join("media/foreign.linux-runtime-3-x86_64.uap")).unwrap();
        let found = candidates(&[work.join("media")]);
        assert_eq!(found, vec![stick.join("foreign.linux-runtime-1-x86_64.uap")]);
        let _ = std::fs::remove_dir_all(&work);
    }
}
