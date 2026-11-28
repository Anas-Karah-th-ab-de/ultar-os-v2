//! `componenttest` – the component „Linux-Programme“ the way the service installs it, on the
//! kernel under it (`tools/kern-test/foreign-app-kern.sh` runs it on the Ultar kernel; on
//! Linux as root it runs alone): a small Linux area as a signed component on a "drive",
//! found and checked against a key of the "device", its archive copied while its SHA-256
//! is counted, unpacked in the unpacking cell (owners kept, set-id bits, device nodes and
//! the insides of `proc` gone), put in place, replaced by a newer one and the old one
//! emptied; a component signed by an unknown key and a damaged archive change nothing.
//!
//! ```text
//! componenttest --helper /bin/ultar-foreign [--work DIR]
//! ```
//!
//! Runs as root. Last line `COMPONENT: ALLES OK` or `COMPONENT: FEHLER <what>`.

use sha2::{Digest, Sha256};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use ultar_bus::messages::foreign::{Failure, Step};
use ultar_foreign::component::{self, Found};
use ultar_foreign::jobs::Ctx;
use ultar_foreign::layout::{area_ok, field_in, Layout};

fn fail(what: &str) -> ! {
    println!("COMPONENT: FEHLER {what}");
    std::thread::sleep(std::time::Duration::from_secs(1));
    std::process::exit(1);
}

fn check(ok: bool, what: &str) {
    if !ok {
        fail(what);
    }
}

/// `.or_fail(what)`: the value, or the last line `COMPONENT: FEHLER what` and the end.
trait OrFail<T> {
    fn or_fail(self, what: &str) -> T;
}

impl<T, E: std::fmt::Debug> OrFail<T> for Result<T, E> {
    fn or_fail(self, what: &str) -> T {
        self.unwrap_or_else(|e| fail(&format!("{what}: {e:?}")))
    }
}

impl<T> OrFail<T> for Option<T> {
    fn or_fail(self, what: &str) -> T {
        self.unwrap_or_else(|| fail(what))
    }
}

/// One entry of the area's archive.
fn add(
    b: &mut tar::Builder<impl std::io::Write>,
    path: &str,
    kind: tar::EntryType,
    mode: u32,
    uid: u64,
    data: &[u8],
    link: Option<&str>,
) {
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(kind);
    h.set_mode(mode);
    h.set_uid(uid);
    h.set_gid(uid);
    h.set_mtime(1_700_000_000);
    h.set_size(data.len() as u64);
    if let Some(l) = link {
        h.set_link_name(l).or_fail("tar link");
    }
    h.set_cksum();
    b.append_data(&mut h, path, data).or_fail("tar entry");
}

/// The directories of the test area.
const DIRS: [&str; 11] = [
    "usr/",
    "usr/bin/",
    "etc/",
    "var/",
    "var/lib/",
    "var/lib/dpkg/",
    "proc/",
    "dev/",
    "tmp/",
    "home/",
    "home/app/",
];

/// One entry: path, kind, mode, owner, content, link target.
type Entry = (
    &'static str,
    tar::EntryType,
    u32,
    u64,
    &'static [u8],
    Option<&'static str>,
);

/// The other entries of the test area: an owned file, a program, a set-id program, a
/// symbolic and a hard link, a device node, the inside of `proc` and the package state.
const ENTRIES: [Entry; 8] = [
    (
        "home/app/owned",
        tar::EntryType::Regular,
        0o644,
        1234,
        b"x",
        None,
    ),
    (
        "usr/bin/hello",
        tar::EntryType::Regular,
        0o755,
        0,
        b"#!/bin/sh\necho hello\n",
        None,
    ),
    (
        "usr/bin/su",
        tar::EntryType::Regular,
        0o4755,
        0,
        b"#!/bin/sh\n",
        None,
    ),
    (
        "usr/bin/sh",
        tar::EntryType::Symlink,
        0o777,
        0,
        b"",
        Some("hello"),
    ),
    (
        "usr/bin/hello2",
        tar::EntryType::Link,
        0o755,
        0,
        b"",
        Some("usr/bin/hello"),
    ),
    ("dev/null", tar::EntryType::Char, 0o666, 0, b"", None),
    ("proc/1/mem", tar::EntryType::Regular, 0o600, 0, b"x", None),
    (
        "var/lib/dpkg/status",
        tar::EntryType::Regular,
        0o644,
        0,
        b"",
        None,
    ),
];

/// The archive of a small Linux area of `version` at `archive`.
fn write_area(archive: &Path, version: &str) {
    let gz = flate2::write::GzEncoder::new(
        std::fs::File::create(archive).or_fail("archive"),
        flate2::Compression::fast(),
    );
    let mut b = tar::Builder::new(gz);
    add(&mut b, "./", tar::EntryType::Directory, 0o755, 0, b"", None);
    for d in DIRS {
        let mode = if d == "tmp/" { 0o1777 } else { 0o755 };
        add(&mut b, d, tar::EntryType::Directory, mode, 0, b"", None);
    }
    for (path, kind, mode, uid, data, link) in ENTRIES {
        add(&mut b, path, kind, mode, uid, data, link);
    }
    let runtime = format!(
        "version={version}\nsuite=test\narch={}\ncomponents=\n",
        std::env::consts::ARCH
    );
    let kind = tar::EntryType::Regular;
    add(
        &mut b,
        "etc/ultar-runtime",
        kind,
        0o644,
        0,
        runtime.as_bytes(),
        None,
    );
    b.into_inner()
        .or_fail("archive")
        .finish()
        .or_fail("archive");
}

/// A component `version` below `media/stick` signed with `key`; returns the archive.
fn component(work: &Path, version: &str, key: &str, name: &str) -> PathBuf {
    let stick = work.join("media/stick");
    std::fs::create_dir_all(&stick).or_fail("media/stick");
    let stem = format!("{}{version}-{}", component::PREFIX, std::env::consts::ARCH);
    let archive = stick.join(format!("{stem}.tar.gz"));
    write_area(&archive, version);
    let bytes = std::fs::read(&archive).or_fail("archive");
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let p = work.join(format!("pkg-{name}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(p.join("payload")).or_fail("payload");
    std::fs::write(p.join("payload/runtime.txt"), b"test").or_fail("payload");
    std::fs::write(
        p.join("manifest.toml"),
        format!(
            "id = \"{}\"\nname = \"Linux area (test)\"\nversion = \"{version}\"\narch = \"{}\"\nkind = \"data\"\n\
             publisher = \"{}\"\nsize = {}\n\n[component]\narchive = \"{stem}.tar.gz\"\nsize = {}\n\
             sha256 = \"{digest}\"\nunpacked = 65536\ncomponents = \"\"\n",
            component::ID,
            std::env::consts::ARCH,
            ultar_brand::VENDOR,
            bytes.len(),
            bytes.len()
        ),
    )
    .or_fail("manifest");
    ultar_pkg::pack_and_sign(
        &p.join("manifest.toml"),
        &p.join("payload"),
        &stick.join(format!("{stem}.uap")),
        key,
    )
    .or_fail("sign");
    archive
}

/// The work directory with the service's root, the device's keys and a "drive"; stops
/// with the step of the kernel's file system that refuses.
fn prepare(work: &Path) {
    let _ = std::fs::remove_dir_all(work);
    for d in ["root", "keys", "media"] {
        let at = work.join(d);
        let made = std::fs::create_dir_all(&at);
        let probe = std::fs::write(at.join(".probe"), b"x");
        if made.is_err() || probe.is_err() {
            // which step of the kernel's file system refuses (a directory the disk does not take)
            let parent = std::fs::write(work.join(format!(".probe-{d}")), b"x");
            fail(&format!(
                "{}: mkdir {made:?}, a file in it {probe:?}, a file next to it {parent:?}",
                at.display()
            ));
        }
        let _ = std::fs::remove_file(at.join(".probe"));
    }
}

/// Nothing to report while a step runs.
fn quiet(_: Step, _: u8) {}

/// 1. a component of an unknown publisher (even a newer one) is not taken.
fn signature(ctx: &Ctx, dir: &Path, work: &Path, keys: (&str, &str)) -> Found {
    component(work, "26.04.1", keys.0, "one");
    component(work, "26.04.9", keys.1, "stranger");
    let found: Found = component::newest(ctx, dir).or_fail("the component is not found");
    check(
        found.manifest.version == "26.04.1",
        &format!(
            "taken: {} (the unknown key was trusted)",
            found.manifest.version
        ),
    );
    check(found.signer == "test", &format!("signer {}", found.signer));
    println!("COMPONENT: signature ok");
    found
}

/// 2. installed: owners kept, set-id bits, device nodes and the insides of proc gone.
fn unpacked(ctx: &Ctx, dir: &Path, found: &Found) {
    let version = component::put_in_place(ctx, found, dir, &quiet).or_fail("install");
    let area = ctx.layout.runtime();
    check(
        version == "26.04.1" && area_ok(&area) && field_in(&area, "version") == "26.04.1",
        "the area is not in place",
    );
    let meta = |p: &str| std::fs::symlink_metadata(area.join(p)).or_fail(p);
    check(
        meta("usr/bin/hello").uid() == 0 && meta("usr").uid() == 0,
        "the area's files do not belong to root",
    );
    check(
        std::fs::metadata(&area).or_fail("the area").uid() == 0,
        "the area's root does not belong to root",
    );
    check(
        meta("home/app/owned").uid() == 1234,
        "an owner was not kept",
    );
    check(
        meta("usr/bin/su").mode() & 0o6000 == 0,
        "a set-id bit stayed",
    );
    check(
        meta("usr/bin/sh").file_type().is_symlink() && meta("usr/bin/hello2").nlink() == 2,
        "links are wrong",
    );
    check(meta("tmp").mode() & 0o7777 == 0o1777, "the modes are wrong");
    check(
        !area.join("dev/null").exists() && !area.join("proc/1").exists(),
        "a device node or the inside of proc came in",
    );
    println!("COMPONENT: unpacked ok");
}

/// 3. a damaged archive (same size, other bytes) changes nothing.
fn damaged(ctx: &Ctx, dir: &Path, work: &Path, key: &str) {
    let archive = component(work, "26.04.2", key, "two");
    let found = component::newest(ctx, dir).or_fail("the newer one is not found");
    check(
        found.manifest.version == "26.04.2",
        "the newer one is not taken",
    );
    let mut bytes = std::fs::read(&archive).or_fail("archive");
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    std::fs::write(&archive, &bytes).or_fail("archive");
    let damaged = component::put_in_place(ctx, &found, dir, &quiet);
    check(
        damaged == Err(Failure::Damaged),
        &format!("damaged archive: {damaged:?}"),
    );
    let area = ctx.layout.runtime();
    check(
        field_in(&area, "version") == "26.04.1" && !ctx.layout.root.join("linux.new").exists(),
        "a damaged archive changed the area",
    );
    println!("COMPONENT: damaged ok");
}

/// 4. replaced by a newer one; the old one is emptied (at the next start: tidy).
fn replaced(ctx: &Ctx, dir: &Path, work: &Path, key: &str) {
    component(work, "26.04.3", key, "three");
    let found = component::newest(ctx, dir).or_fail("the newest is not found");
    let version = component::put_in_place(ctx, &found, dir, &quiet).or_fail("replace");
    let area = ctx.layout.runtime();
    check(
        version == "26.04.3" && field_in(&area, "version") == "26.04.3",
        "not replaced",
    );
    check(
        ctx.layout.root.join("linux.old/usr/bin/hello").exists(),
        "the old area is not aside",
    );
    component::tidy(ctx);
    check(
        !ctx.layout.root.join("linux.old").exists(),
        "the old area stayed",
    );
    check(
        std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(1) == 0,
        "the copy stayed in the inbox",
    );
    println!("COMPONENT: replaced ok");
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let arg = |name: &str| {
        words
            .iter()
            .position(|w| w == name)
            .and_then(|i| words.get(i + 1))
            .cloned()
    };
    let helper = PathBuf::from(arg("--helper").unwrap_or_else(|| fail("--helper fehlt")));
    let work = PathBuf::from(arg("--work").unwrap_or_else(|| "/data/componenttest".into()));
    prepare(&work);
    let (secret, public) = ultar_pkg::keygen();
    let (stranger, _) = ultar_pkg::keygen();
    std::fs::write(work.join("keys/test.ed25519"), public).or_fail("key");
    let ctx = Ctx {
        layout: Layout::new(&work.join("root")),
        helper,
        sources: vec![work.join("media")],
        trusted: work.join("keys"),
    };
    ctx.layout.ensure().or_fail("layout");
    let dir = ctx.layout.inbox().join("componenttest");
    std::fs::create_dir_all(&dir).or_fail("inbox");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).or_fail("inbox");
    let found = signature(&ctx, &dir, &work, (&secret, &stranger));
    unpacked(&ctx, &dir, &found);
    damaged(&ctx, &dir, &work, &secret);
    replaced(&ctx, &dir, &work, &secret);
    let _ = std::fs::remove_dir_all(&work);
    println!("COMPONENT: ALLES OK");
}
