//! The snapshot helper itself: update mode, mismatch with diff image and pixel count,
//! missing and corrupt references, atomic writing.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use ultar_ui::snapshot::{write_atomic, Image, SnapshotError, Snapshots};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::ui::{Ui, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Label};
use ultar_ui::{Lang, Size};

const SIZE: Size = Size::new(160.0, 60.0);

/// A fresh, empty directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("ultar-ui-snapshot")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn set(dir: &Path, update: bool) -> Snapshots {
    Snapshots::new(dir)
        .catalog(Lang::Ar, common::AR)
        .catalog(Lang::En, common::EN)
        .update(update)
}

fn button(variant: ButtonVariant) -> impl Fn(&mut Ui) -> Result<(), UiError> {
    move |ui| {
        let root = ui.root();
        let button = Button::new(Label::key("demo.save")).variant(variant);
        ui.add(root, button).map(|_| ())
    }
}

fn check(dir: &Path, update: bool, variant: ButtonVariant) -> Result<(), SnapshotError> {
    set(dir, update).assert_snapshot("button", button(variant), SIZE, Lang::Ar, ThemeKind::Dark)
}

#[test]
fn update_writes_and_an_equal_render_passes() {
    let dir = scratch("update");
    let missing = check(&dir, false, ButtonVariant::Plain).unwrap_err();
    assert!(
        matches!(&missing, SnapshotError::MissingReference(p) if p.ends_with("button-ar-dark.png"))
    );
    assert!(missing.to_string().contains("ULTAR_UPDATE_SNAPSHOTS"));
    check(&dir, true, ButtonVariant::Plain).unwrap();
    let reference = dir.join("button-ar-dark.png");
    let image = Image::decode_png(&fs::read(&reference).unwrap(), &reference).unwrap();
    assert_eq!((image.width, image.height), (160, 60));
    check(&dir, false, ButtonVariant::Plain).unwrap();
    assert!(
        !dir.join("failed").exists(),
        "no diff files when everything matches"
    );
    let leftovers = fs::read_dir(&dir).unwrap().filter(|e| {
        let name = e.as_ref().unwrap().file_name();
        name.to_string_lossy().ends_with(".tmp")
    });
    assert_eq!(leftovers.count(), 0, "no temporary files stay behind");
}

#[test]
fn a_changed_render_fails_with_pixel_count_and_diff_image() {
    let dir = scratch("mismatch");
    check(&dir, true, ButtonVariant::Plain).unwrap();
    let error = check(&dir, false, ButtonVariant::Primary).unwrap_err();
    let SnapshotError::Mismatch {
        reference,
        differing,
        diff,
    } = &error
    else {
        panic!("expected a mismatch, got {error}");
    };
    assert!(reference.ends_with("button-ar-dark.png"));
    assert!(
        *differing > 500 && *differing < 160 * 60,
        "{differing} pixels differ"
    );
    assert!(error.to_string().contains(&differing.to_string()));
    let diff_image = Image::decode_png(&fs::read(diff).unwrap(), diff).unwrap();
    let marked = diff_image.rgba.chunks(4).filter(|p| p[3] == 255).count();
    assert_eq!(
        marked, *differing,
        "the diff image marks exactly the differing pixels"
    );
    assert!(dir.join("failed/button-ar-dark.actual.png").exists());
    // A reference of another size counts every pixel.
    let other = set(&dir, false).assert_snapshot(
        "button",
        button(ButtonVariant::Plain),
        Size::new(100.0, 50.0),
        Lang::Ar,
        ThemeKind::Dark,
    );
    assert!(matches!(
        other,
        Err(SnapshotError::Mismatch {
            differing: 5000,
            ..
        })
    ));
}

#[test]
fn corrupt_and_foreign_references_are_detected() {
    let dir = scratch("corrupt");
    check(&dir, true, ButtonVariant::Plain).unwrap();
    let reference = dir.join("button-ar-dark.png");
    let good = fs::read(&reference).unwrap();
    // Truncated file (as after a power loss without atomic writing).
    fs::write(&reference, &good[..good.len() / 2]).unwrap();
    let error = check(&dir, false, ButtonVariant::Plain).unwrap_err();
    assert!(matches!(error, SnapshotError::Corrupt { .. }), "{error}");
    // A flipped byte in the compressed data fails the checksum.
    let mut flipped = good.clone();
    let middle = flipped.len() / 2;
    flipped[middle] ^= 0xFF;
    fs::write(&reference, &flipped).unwrap();
    assert!(matches!(
        check(&dir, false, ButtonVariant::Plain),
        Err(SnapshotError::Corrupt { .. })
    ));
    fs::write(&reference, b"not a png").unwrap();
    assert!(matches!(
        check(&dir, false, ButtonVariant::Plain),
        Err(SnapshotError::Corrupt { .. })
    ));
    // A valid PNG that is not 8-bit RGBA is refused, not silently converted.
    let mut grey = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut grey, 2, 2);
        encoder.set_color(png::ColorType::Grayscale);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[0, 1, 2, 3]).unwrap();
    }
    let error = Image::decode_png(&grey, &reference).unwrap_err();
    assert!(error.to_string().contains("RGBA"), "{error}");
    // Updating repairs the reference.
    check(&dir, true, ButtonVariant::Plain).unwrap();
    check(&dir, false, ButtonVariant::Plain).unwrap();
}

#[test]
fn names_are_validated_and_tree_errors_are_reported() {
    let dir = scratch("names");
    let snapshots = set(&dir, true);
    for name in ["", "../escape", "Upper", "with space", "a/b", "dot.png"] {
        let result = snapshots.assert_snapshot(
            name,
            button(ButtonVariant::Plain),
            SIZE,
            Lang::En,
            ThemeKind::Light,
        );
        assert!(
            matches!(result, Err(SnapshotError::InvalidName(_))),
            "{name:?}"
        );
    }
    assert_eq!(
        fs::read_dir(&dir).unwrap().count(),
        0,
        "nothing was written"
    );
    let failing = snapshots.assert_snapshot(
        "tree",
        |ui| {
            let root = ui.root();
            let leaf = ui.add(root, Button::new(Label::key("demo.save")))?;
            ui.add(leaf, Button::new(Label::key("demo.save")))
                .map(|_| ())
        },
        SIZE,
        Lang::En,
        ThemeKind::Light,
    );
    assert!(matches!(
        failing,
        Err(SnapshotError::Ui(UiError::NotAContainer(_)))
    ));
    let zero = snapshots.assert_snapshot(
        "zero",
        button(ButtonVariant::Plain),
        Size::new(0.0, 10.0),
        Lang::En,
        ThemeKind::Light,
    );
    assert!(matches!(zero, Err(SnapshotError::Canvas(_))));
    let bad_catalog = Snapshots::new(&dir).catalog(Lang::En, "this is not a catalog");
    let result = bad_catalog.ui(SIZE, Lang::En, ThemeKind::Dark);
    assert!(matches!(
        result,
        Err(SnapshotError::Ui(UiError::Catalog(_)))
    ));
    let path = snapshots.path("x", Lang::De, ThemeKind::Light);
    assert!(path.ends_with("x-de-light.png"));
}

#[test]
fn atomic_write_replaces_and_reports_errors() {
    let dir = scratch("atomic");
    let target = dir.join("nested/deeper/file.bin");
    write_atomic(&target, b"first").unwrap();
    write_atomic(&target, b"second").unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"second");
    assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 1);
    // A path whose parent is a file cannot be written; the error names the path.
    let blocked = target.join("child.bin");
    let error = write_atomic(&blocked, b"x").unwrap_err();
    assert!(matches!(&error, SnapshotError::Io { path, .. } if path == &blocked));
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn image_round_trip_and_diff() {
    let a = Image {
        width: 2,
        height: 1,
        rgba: vec![1, 2, 3, 255, 4, 5, 6, 255],
    };
    let path = Path::new("memory.png");
    let decoded = Image::decode_png(&a.encode_png().unwrap(), path).unwrap();
    assert_eq!(decoded, a);
    let mut b = a.clone();
    b.rgba[4] = 9;
    let (count, picture) = a.diff(&b);
    assert_eq!(count, 1);
    assert_eq!(picture.rgba[3], 255 / 4, "equal pixels are dimmed");
    assert_eq!(picture.rgba[7], 255, "the differing pixel is marked");
    assert_eq!(a.diff(&a).0, 0);
    let wide = Image {
        width: 1,
        height: 2,
        rgba: a.rgba.clone(),
    };
    assert_eq!(a.diff(&wide).0, 2);
}
