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

#![allow(clippy::unwrap_used, clippy::expect_used)]

use sha2::{Digest, Sha256};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use ultar_bus::messages::foreign::Failure;
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

/// One entry of the area's archive.
fn add(b: &mut tar::Builder<impl std::io::Write>, path: &str, kind: tar::EntryType, mode: u32, uid: u64, data: &[u8], link: Option<&str>) {
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(kind);
    h.set_mode(mode);
    h.set_uid(uid);
    h.set_gid(uid);
    h.set_mtime(1_700_000_000);
    h.set_size(data.len() as u64);
    if let Some(l) = link {
        h.set_link_name(l).unwrap();
    }
    h.set_cksum();
    b.append_data(&mut h, path, data).unwrap();
}

/// A component `version` below `media/stick` signed with `key`; returns the archive.
fn component(work: &Path, version: &str, key: &str, name: &str) -> PathBuf {
    use tar::EntryType as E;
    let stick = work.join("media/stick");
    std::fs::create_dir_all(&stick).unwrap();
    let stem = format!("{}{version}-{}", component::PREFIX, std::env::consts::ARCH);
    let archive = stick.join(format!("{stem}.tar.gz"));
    {
        let gz = flate2::write::GzEncoder::new(std::fs::File::create(&archive).unwrap(), flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        let runtime = format!("version={version}\nsuite=test\narch={}\ncomponents=\n", std::env::consts::ARCH);
        add(&mut b, "./", E::Directory, 0o755, 0, b"", None);
        for d in ["usr/", "usr/bin/", "etc/", "var/", "var/lib/", "var/lib/dpkg/", "proc/", "dev/", "tmp/", "home/", "home/app/"] {
            let mode = if d == "tmp/" { 0o1777 } else { 0o755 };
            add(&mut b, d, E::Directory, mode, 0, b"", None);
        }
        add(&mut b, "home/app/owned", E::Regular, 0o644, 1234, b"x", None);
        add(&mut b, "usr/bin/hello", E::Regular, 0o755, 0, b"#!/bin/sh\necho hello\n", None);
        add(&mut b, "usr/bin/su", E::Regular, 0o4755, 0, b"#!/bin/sh\n", None);
        add(&mut b, "usr/bin/sh", E::Symlink, 0o777, 0, b"", Some("hello"));
        add(&mut b, "usr/bin/hello2", E::Link, 0o755, 0, b"", Some("usr/bin/hello"));
        add(&mut b, "dev/null", E::Char, 0o666, 0, b"", None);
        add(&mut b, "proc/1/mem", E::Regular, 0o600, 0, b"x", None);
        add(&mut b, "var/lib/dpkg/status", E::Regular, 0o644, 0, b"", None);
        add(&mut b, "etc/ultar-runtime", E::Regular, 0o644, 0, runtime.as_bytes(), None);
        b.into_inner().unwrap().finish().unwrap();
    }
    let bytes = std::fs::read(&archive).unwrap();
    let digest: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    let p = work.join(format!("pkg-{name}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(p.join("payload")).unwrap();
    std::fs::write(p.join("payload/runtime.txt"), b"test").unwrap();
    std::fs::write(
        p.join("manifest.toml"),
        format!(
            "id = \"{}\"\nname = \"Linux area (test)\"\nversion = \"{version}\"\narch = \"{}\"\nkind = \"data\"\n\
             publisher = \"KM Code Craft\"\nsize = {}\n\n[component]\narchive = \"{stem}.tar.gz\"\nsize = {}\n\
             sha256 = \"{digest}\"\nunpacked = 65536\ncomponents = \"\"\n",
            component::ID,
            std::env::consts::ARCH,
            bytes.len(),
            bytes.len()
        ),
    )
    .unwrap();
    ultar_pkg::pack_and_sign(&p.join("manifest.toml"), &p.join("payload"), &stick.join(format!("{stem}.uap")), key).unwrap();
    archive
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let arg = |name: &str| words.iter().position(|w| w == name).and_then(|i| words.get(i + 1)).cloned();
    let helper = PathBuf::from(arg("--helper").unwrap_or_else(|| fail("--helper fehlt")));
    let work = PathBuf::from(arg("--work").unwrap_or_else(|| "/data/componenttest".into()));
    let _ = std::fs::remove_dir_all(&work);
    for d in ["root", "keys", "media"] {
        std::fs::create_dir_all(work.join(d)).unwrap();
    }
    let (secret, public) = ultar_pkg::keygen();
    let (stranger, _) = ultar_pkg::keygen();
    std::fs::write(work.join("keys/test.ed25519"), public).unwrap();
    let ctx = Ctx {
        layout: Layout::new(&work.join("root")),
        helper,
        sources: vec![work.join("media")],
        trusted: work.join("keys"),
    };
    ctx.layout.ensure().unwrap();
    let dir = ctx.layout.inbox().join("componenttest");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let none = |_, _| {};

    // 1. a component of an unknown publisher (even a newer one) is not taken
    component(&work, "26.04.1", &secret, "one");
    component(&work, "26.04.9", &stranger, "stranger");
    let found: Found = component::newest(&ctx, &dir).unwrap_or_else(|| fail("the component is not found"));
    check(found.manifest.version == "26.04.1", &format!("taken: {} (the unknown key was trusted)", found.manifest.version));
    check(found.signer == "test", &format!("signer {}", found.signer));
    println!("COMPONENT: signature ok");

    // 2. installed: owners kept, set-id bits, device nodes and the insides of proc gone
    let version = component::put_in_place(&ctx, &found, &dir, &none).unwrap_or_else(|e| fail(&format!("install: {e:?}")));
    let area = ctx.layout.runtime();
    check(version == "26.04.1" && area_ok(&area) && field_in(&area, "version") == "26.04.1", "the area is not in place");
    let meta = |p: &str| std::fs::symlink_metadata(area.join(p)).unwrap_or_else(|e| fail(&format!("{p}: {e}")));
    check(meta("usr/bin/hello").uid() == 0 && meta("usr").uid() == 0, "the area's files do not belong to root");
    check(std::fs::metadata(&area).unwrap().uid() == 0, "the area's root does not belong to root");
    check(meta("home/app/owned").uid() == 1234, "an owner was not kept");
    check(meta("usr/bin/su").mode() & 0o6000 == 0, "a set-id bit stayed");
    check(meta("usr/bin/sh").file_type().is_symlink() && meta("usr/bin/hello2").nlink() == 2, "links are wrong");
    check(meta("tmp").mode() & 0o7777 == 0o1777, "the modes are wrong");
    check(!area.join("dev/null").exists() && !area.join("proc/1").exists(), "a device node or the inside of proc came in");
    println!("COMPONENT: unpacked ok");

    // 3. a damaged archive (same size, other bytes) changes nothing
    let archive = component(&work, "26.04.2", &secret, "two");
    let found2 = component::newest(&ctx, &dir).unwrap_or_else(|| fail("the newer one is not found"));
    check(found2.manifest.version == "26.04.2", "the newer one is not taken");
    let mut bytes = std::fs::read(&archive).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    std::fs::write(&archive, &bytes).unwrap();
    let damaged = component::put_in_place(&ctx, &found2, &dir, &none);
    check(damaged == Err(Failure::Damaged), &format!("damaged archive: {damaged:?}"));
    check(field_in(&area, "version") == "26.04.1" && !ctx.layout.root.join("linux.new").exists(), "a damaged archive changed the area");
    println!("COMPONENT: damaged ok");

    // 4. replaced by a newer one; the old one is emptied (at the next start: tidy)
    component(&work, "26.04.3", &secret, "three");
    let found3 = component::newest(&ctx, &dir).unwrap_or_else(|| fail("the newest is not found"));
    let version = component::put_in_place(&ctx, &found3, &dir, &none).unwrap_or_else(|e| fail(&format!("replace: {e:?}")));
    check(version == "26.04.3" && field_in(&area, "version") == "26.04.3", "not replaced");
    check(ctx.layout.root.join("linux.old/usr/bin/hello").exists(), "the old area is not aside");
    component::tidy(&ctx);
    check(!ctx.layout.root.join("linux.old").exists(), "the old area stayed");
    check(std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(1) == 0, "the copy stayed in the inbox");
    println!("COMPONENT: replaced ok");

    let _ = std::fs::remove_dir_all(&work);
    println!("COMPONENT: ALLES OK");
}
