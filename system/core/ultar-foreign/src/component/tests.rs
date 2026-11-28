//! Tests of the Linux area component.

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
    for bad in [
        "../foreign.linux-runtime-x.tar.gz",
        "/etc/foreign.linux-runtime-x.tar.gz",
        "linux.tar.gz",
        "foreign.linux-runtime-x.tar",
    ] {
        assert!(
            Manifest::parse(manifest("x86_64", bad).as_bytes(), "x86_64").is_none(),
            "{bad}"
        );
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
    for bad in [
        "",
        ".",
        "./",
        "/etc/passwd",
        "../x",
        "usr/../../x",
        "proc/1/mem",
        "dev/sda",
        "run/lock/x",
    ] {
        assert!(!entry_ok(Path::new(bad)), "{bad}");
    }
}

/// A small Linux area as `archive`: a set-id program, a link, a device node and
/// directories of mounted places.
fn write_test_archive(archive: &Path) {
    {
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(archive).unwrap(),
            flate2::Compression::fast(),
        );
        let mut b = tar::Builder::new(gz);
        let mut add =
            |path: &str, kind: tar::EntryType, mode: u32, data: &[u8], link: Option<&str>| {
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
        add(
            "usr/bin/sh",
            tar::EntryType::Symlink,
            0o777,
            b"",
            Some("dash"),
        );
        add("dev/", tar::EntryType::Directory, 0o755, b"", None);
        add("dev/null", tar::EntryType::Char, 0o666, b"", None);
        add("proc/", tar::EntryType::Directory, 0o555, b"", None);
        add("etc/", tar::EntryType::Directory, 0o755, b"", None);
        add(
            "etc/ultar-runtime",
            tar::EntryType::Regular,
            0o644,
            b"version=1\n",
            None,
        );
        b.into_inner().unwrap().finish().unwrap();
    }
}

#[test]
fn unpacking_keeps_the_tree_and_drops_devices_links_out_and_set_id_bits() {
    use std::os::unix::fs::PermissionsExt;
    let work = std::env::temp_dir().join(format!("ultar-foreign-unpack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(work.join("into")).unwrap();
    let archive = work.join("a.tar.gz");
    write_test_archive(&archive);
    let into = work.join("into");
    unpack(&archive, &into, false).unwrap();
    let su = std::fs::metadata(into.join("usr/bin/su")).unwrap();
    assert_eq!(su.permissions().mode() & 0o7777, 0o755);
    assert_eq!(
        std::fs::read_link(into.join("usr/bin/sh")).unwrap(),
        Path::new("dash")
    );
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
    std::fs::write(
        work.join("media/foreign.linux-runtime-2-x86_64.uap.part"),
        b"x",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        "/etc/passwd",
        work.join("media/foreign.linux-runtime-3-x86_64.uap"),
    )
    .unwrap();
    let found = candidates(&[work.join("media")]);
    assert_eq!(
        found,
        vec![stick.join("foreign.linux-runtime-1-x86_64.uap")]
    );
    let _ = std::fs::remove_dir_all(&work);
}
