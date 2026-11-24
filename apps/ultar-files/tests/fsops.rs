//! File operations on a real temporary file system: copy, move, trash, restore, empty,
//! undo; conflicts, partial failures, cancel, permission errors and symlink safety.
mod common;

use common::{exists, loc, Root};
use std::os::unix::fs::{symlink, PermissionsExt};
use ultar_files::fs::copy::{Cancel, Job, Policy, Progress};
use ultar_files::fs::undo::{self, Undo};
use ultar_files::fs::{trash, AreaId, FsError, Kind, Loc};

fn job<'a>(fs: &'a ultar_files::fs::Fs, sink: &'a mut dyn FnMut(Progress)) -> Job<'a> {
    Job::new(fs, Cancel::default(), sink)
}

fn is_root() -> bool {
    rustix::process::geteuid().is_root()
}

#[test]
fn areas_are_found_and_listed() {
    let r = Root::new();
    r.write("documents/تقرير.docx", b"x");
    r.write("documents/a/b.txt", b"hello");
    let fs = r.fs();
    assert!(fs.has(&AreaId::Documents) && fs.has(&AreaId::Drive("USB".into())));
    let mut names: Vec<_> = fs
        .list(&Loc::root(AreaId::Documents))
        .unwrap()
        .into_iter()
        .map(|e| (e.name, e.kind))
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            ("a".to_owned(), Kind::Folder),
            ("تقرير.docx".to_owned(), Kind::File)
        ]
    );
    assert_eq!(
        fs.stat(&loc(AreaId::Documents, &["a", "b.txt"]))
            .unwrap()
            .size,
        5
    );
    assert_eq!(
        fs.list(&loc(AreaId::Documents, &["nope"])),
        Err(FsError::NotFound)
    );
}

#[test]
fn symlinks_never_lead_out_of_an_area() {
    let r = Root::new();
    let outside = Root::new();
    outside.write("secret/key", b"secret");
    symlink(outside.path("secret"), r.path("documents/out")).unwrap();
    symlink(outside.path("secret/key"), r.path("documents/key")).unwrap();
    let fs = r.fs();
    // listing shows the links as links
    let e = fs.stat(&loc(AreaId::Documents, &["out"])).unwrap();
    assert_eq!(e.kind, Kind::Link);
    // walking through or opening them is refused
    assert_eq!(
        fs.list(&loc(AreaId::Documents, &["out"])),
        Err(FsError::Link)
    );
    assert!(fs.open_read(&loc(AreaId::Documents, &["key"])).is_err());
    assert!(fs.stat(&loc(AreaId::Documents, &["out", "key"])).is_err());
    // copying a link copies the link, never what it points to
    let mut sink = |_p: Progress| {};
    let report = job(&fs, &mut sink).copy(
        &[loc(AreaId::Documents, &["out"])],
        &Loc::root(AreaId::Downloads),
        Policy::Ask,
    );
    assert_eq!(report.done.len(), 1);
    assert!(std::fs::symlink_metadata(r.path("downloads/out"))
        .unwrap()
        .file_type()
        .is_symlink());
    // deleting a link leaves its target alone
    let report = job(&fs, &mut sink).delete(&[loc(AreaId::Documents, &["out"])]);
    assert_eq!(report.done.len(), 1);
    assert!(exists(&outside.path("secret/key")));
    // an area that is itself a link is not granted
    let other = Root::new();
    std::fs::remove_dir(other.path("music")).ok();
    std::fs::remove_dir(other.path("media-music")).unwrap();
    symlink(outside.path("secret"), other.path("media-music")).unwrap();
    assert!(!other.fs().has(&AreaId::Music));
}

#[test]
fn copy_with_conflicts_and_policies() {
    let r = Root::new();
    r.write("documents/r.txt", b"new");
    r.write("downloads/r.txt", b"old");
    r.write("documents/d/x.txt", b"1");
    let fs = r.fs();
    let src = [
        loc(AreaId::Documents, &["r.txt"]),
        loc(AreaId::Documents, &["d"]),
    ];
    let dest = Loc::root(AreaId::Downloads);
    let mut seen = Vec::new();
    let mut sink = |p: Progress| seen.push(p);
    let report = job(&fs, &mut sink).copy(&src, &dest, Policy::Ask);
    assert_eq!(
        report.conflicts,
        [(src[0].clone(), loc(AreaId::Downloads, &["r.txt"]))]
    );
    assert_eq!(report.done.len(), 1);
    assert_eq!(std::fs::read(r.path("downloads/d/x.txt")).unwrap(), b"1");
    assert_eq!(std::fs::read(r.path("downloads/r.txt")).unwrap(), b"old");
    let last = seen.last().copied().unwrap();
    assert!(
        last.done_items >= 2 && last.done_items < last.total_items,
        "the conflict was not done"
    );
    let mut sink = |_p: Progress| {};
    let report = job(&fs, &mut sink).copy(&src[..1], &dest, Policy::KeepBoth);
    assert_eq!(report.done[0].1, loc(AreaId::Downloads, &["r 2.txt"]));
    let report = job(&fs, &mut sink).copy(&src[..1], &dest, Policy::Replace);
    assert_eq!(report.done.len(), 1);
    assert_eq!(std::fs::read(r.path("downloads/r.txt")).unwrap(), b"new");
    let report = job(&fs, &mut sink).copy(&src[..1], &dest, Policy::Skip);
    assert_eq!(report.skipped.len(), 1);
    // no temporary file is left behind
    assert!(std::fs::read_dir(r.path("downloads")).unwrap().all(|e| !e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".part")));
}

#[test]
fn a_folder_cannot_go_into_itself_and_failures_are_partial() {
    let r = Root::new();
    r.write("documents/d/x", b"1");
    r.write("documents/ok.txt", b"2");
    let fs = r.fs();
    let mut sink = |_p: Progress| {};
    let d = loc(AreaId::Documents, &["d"]);
    let report = job(&fs, &mut sink).copy(
        &[
            d.clone(),
            loc(AreaId::Documents, &["gone"]),
            loc(AreaId::Documents, &["ok.txt"]),
        ],
        &d,
        Policy::Ask,
    );
    assert_eq!(report.failed[0], (d.clone(), FsError::IntoItself));
    assert_eq!(report.failed[1].1, FsError::NotFound);
    assert_eq!(report.done.len(), 1, "the other items still work");
}

#[test]
fn permission_errors_are_reported() {
    if is_root() {
        return; // root ignores permission bits
    }
    let r = Root::new();
    r.write("documents/a.txt", b"x");
    std::fs::set_permissions(r.path("downloads"), std::fs::Permissions::from_mode(0o500)).unwrap();
    let fs = r.fs();
    let mut sink = |_p: Progress| {};
    let report = job(&fs, &mut sink).copy(
        &[loc(AreaId::Documents, &["a.txt"])],
        &Loc::root(AreaId::Downloads),
        Policy::Ask,
    );
    assert_eq!(report.failed[0].1, FsError::Permission);
    assert_eq!(
        fs.mkdir(&loc(AreaId::Downloads, &["n"])),
        Err(FsError::Permission)
    );
    std::fs::set_permissions(r.path("downloads"), std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn cancel_stops_and_leaves_no_partial_file() {
    let r = Root::new();
    r.write(
        "documents/big.bin",
        &vec![7u8; 3 * ultar_files::fs::copy::CHUNK],
    );
    r.write("documents/later.txt", b"x");
    let fs = r.fs();
    let cancel = Cancel::default();
    let c2 = cancel.clone();
    let mut chunks = 0;
    let mut sink = |p: Progress| {
        if p.done_bytes > 0 {
            chunks += 1;
            c2.cancel();
        }
    };
    let mut j = Job::new(&fs, cancel, &mut sink);
    let report = j.copy(
        &[
            loc(AreaId::Documents, &["big.bin"]),
            loc(AreaId::Documents, &["later.txt"]),
        ],
        &Loc::root(AreaId::Downloads),
        Policy::Ask,
    );
    assert!(report.cancelled && report.done.is_empty());
    assert!(chunks >= 1);
    assert_eq!(
        std::fs::read_dir(r.path("downloads")).unwrap().count(),
        0,
        "nothing half-written stays"
    );
}

#[test]
fn move_renames_within_and_copies_across_mounts() {
    let r = Root::new();
    r.write("documents/a/x.txt", b"x");
    let fs = r.fs();
    let mut sink = |_p: Progress| {};
    let report = job(&fs, &mut sink).move_to(
        &[loc(AreaId::Documents, &["a"])],
        &Loc::root(AreaId::Downloads),
        Policy::Ask,
    );
    assert_eq!(report.done.len(), 1);
    assert!(!exists(&r.path("documents/a")) && exists(&r.path("downloads/a/x.txt")));
    // moving into the same folder does nothing
    let report = job(&fs, &mut sink).move_to(
        &[loc(AreaId::Downloads, &["a"])],
        &Loc::root(AreaId::Downloads),
        Policy::Ask,
    );
    assert_eq!(report.skipped.len(), 1);
}

#[test]
fn trash_restore_purge_and_empty() {
    let r = Root::new();
    r.write("documents/عقود/عقد 1.pdf", b"pdf");
    r.write("downloads/عقد 1.pdf", b"other");
    let fs = r.fs();
    let mut sink = |_p: Progress| {};
    let a = loc(AreaId::Documents, &["عقود", "عقد 1.pdf"]);
    let b = loc(AreaId::Downloads, &["عقد 1.pdf"]);
    let report = trash::put(&mut job(&fs, &mut sink), &fs, &[a.clone(), b.clone()], 100);
    assert_eq!(report.done.len(), 2);
    let ids: Vec<String> = report
        .done
        .iter()
        .map(|(_, t)| t.name().unwrap().to_owned())
        .collect();
    assert_eq!(
        ids,
        ["عقد 1.pdf", "عقد 1 2.pdf"],
        "the second gets a free name in the trash"
    );
    let listed = trash::list(&fs).unwrap();
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().any(|t| t.origin == a && t.deleted == 100));
    // the folder is gone meanwhile: restore makes it again
    std::fs::remove_dir(r.path("documents/عقود")).unwrap();
    let report = trash::restore(&mut job(&fs, &mut sink), &fs, &ids[..1]);
    assert_eq!(report.done[0].1, a);
    assert_eq!(
        std::fs::read(r.path("documents/عقود/عقد 1.pdf")).unwrap(),
        b"pdf"
    );
    // a taken name is kept: the restored item gets a free one
    r.write("downloads/عقد 1.pdf", b"new");
    let report = trash::restore(&mut job(&fs, &mut sink), &fs, &ids[1..]);
    assert_eq!(report.done[0].1, loc(AreaId::Downloads, &["عقد 1 2.pdf"]));
    // purge and empty
    trash::put(&mut job(&fs, &mut sink), &fs, std::slice::from_ref(&a), 200);
    trash::put(&mut job(&fs, &mut sink), &fs, std::slice::from_ref(&b), 300);
    let report = trash::delete(
        &mut job(&fs, &mut sink),
        &[trash::list(&fs).unwrap()[0].id.clone()],
    );
    assert_eq!(report.done.len(), 1);
    assert_eq!(trash::list(&fs).unwrap().len(), 1);
    trash::empty(&mut job(&fs, &mut sink), &fs);
    assert!(trash::list(&fs).unwrap().is_empty());
    assert_eq!(std::fs::read_dir(r.path("trash/info")).unwrap().count(), 0);
    // nothing in the trash is put into the trash again, an area root never
    let report = trash::put(
        &mut job(&fs, &mut sink),
        &fs,
        &[Loc::root(AreaId::Documents)],
        1,
    );
    assert_eq!(report.failed[0].1, FsError::Permission);
}

#[test]
fn damaged_trash_records_are_refused() {
    for bad in [
        "",
        "[Trash Info]\nPath=documents:../x\nDeletionDate=1",
        "[Trash Info]\nPath=etc:passwd\nDeletionDate=1",
        "[Trash Info]\nPath=documents:a%2",
        "[Trash Info]\nDeletionDate=1",
        "[Trash Info]\nPath=documents:a",
    ] {
        assert_eq!(trash::parse_record(bad), Err(FsError::BadRecord), "{bad:?}");
    }
    let l = loc(AreaId::Documents, &["a%b", "c\u{200f}d"]);
    assert_eq!(trash::parse_record(&trash::record(&l, 5)).unwrap(), (l, 5));
    // fuzz: random bytes never panic (fixed seed)
    let mut x: u64 = 0x5eed;
    for _ in 0..2000 {
        let mut s = String::from("[Trash Info]\nPath=");
        for _ in 0..(x % 40) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            s.push(char::from(b" %/:.adeimnocrt0129\n"[(x % 20) as usize]));
        }
        let _ = trash::parse_record(&s);
    }
}

#[test]
fn undo_goes_back_step_by_step() {
    let r = Root::new();
    r.write("documents/a.txt", b"a");
    let fs = r.fs();
    let mut sink = |_p: Progress| {};
    let a = loc(AreaId::Documents, &["a.txt"]);
    let report = trash::put(&mut job(&fs, &mut sink), &fs, std::slice::from_ref(&a), 1);
    let step = Undo::Trashed(
        report
            .done
            .iter()
            .map(|(_, t)| t.name().unwrap().to_owned())
            .collect(),
    );
    undo::apply(&mut job(&fs, &mut sink), &fs, &step, 2);
    assert!(exists(&r.path("documents/a.txt")));
    let report = job(&fs, &mut sink).copy(
        std::slice::from_ref(&a),
        &Loc::root(AreaId::Downloads),
        Policy::Ask,
    );
    let step = Undo::Copied(report.done.iter().map(|(_, t)| t.clone()).collect());
    undo::apply(&mut job(&fs, &mut sink), &fs, &step, 3);
    assert!(
        !exists(&r.path("downloads/a.txt")),
        "an undone copy goes to the trash"
    );
    assert_eq!(trash::list(&fs).unwrap().len(), 1);
    let report = job(&fs, &mut sink).move_to(
        std::slice::from_ref(&a),
        &Loc::root(AreaId::Downloads),
        Policy::Ask,
    );
    undo::apply(&mut job(&fs, &mut sink), &fs, &Undo::Moved(report.done), 4);
    assert!(exists(&r.path("documents/a.txt")));
    let b = loc(AreaId::Documents, &["b.txt"]);
    fs.rename(&a, &b, false).unwrap();
    undo::apply(
        &mut job(&fs, &mut sink),
        &fs,
        &Undo::Renamed(a.clone(), b),
        5,
    );
    assert!(exists(&r.path("documents/a.txt")));
    let n = loc(AreaId::Documents, &["n"]);
    fs.mkdir(&n).unwrap();
    undo::apply(&mut job(&fs, &mut sink), &fs, &Undo::Created(n), 6);
    assert!(!exists(&r.path("documents/n")));
    let mut stack = undo::UndoStack::default();
    for i in 0..40 {
        stack.push(Undo::Created(loc(AreaId::Home, &[&i.to_string()])));
    }
    stack.push(Undo::Copied(Vec::new()));
    assert_eq!(stack.len(), undo::MAX_UNDO);
}

#[test]
fn search_by_name_and_content_tolerates_arabic_variants() {
    let r = Root::new();
    r.write("documents/أحمد.txt", b"x");
    r.write("documents/notes.txt", "اجتماع الميزانيّة".as_bytes());
    r.write(
        "documents/bin.dat",
        &[b"\0".as_slice(), "ميزانية".as_bytes()].concat(),
    );
    let fs = r.fs();
    let folder = Loc::root(AreaId::Documents);
    let hits = |q: &str| {
        let mut v: Vec<String> = fs
            .list(&folder)
            .unwrap()
            .into_iter()
            .filter(|e| ultar_files::fs::search::matches(&fs, &folder, e, q))
            .map(|e| e.name)
            .collect();
        v.sort();
        v
    };
    assert_eq!(hits("احمد"), ["أحمد.txt"]);
    assert_eq!(hits("ميزانية"), ["notes.txt"]);
    assert_eq!(hits("").len(), 3);
}

#[test]
fn thumbnails_are_decoded_within_limits() {
    use ultar_files::fs::thumb::{self, ThumbError};
    // a 200 × 100 red PNG
    let mut png_bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png_bytes, 200, 100);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[255, 0, 0].repeat(200 * 100)).unwrap();
    }
    let p = thumb::from_bytes(&png_bytes).unwrap();
    assert_eq!((p.width, p.height), (96, 48));
    assert_eq!(&p.rgba[..4], &[255, 0, 0, 255]);
    assert_eq!(thumb::from_bytes(b"GIF89a"), Err(ThumbError::Unsupported));
    assert_eq!(
        thumb::from_bytes(&png_bytes[..40]),
        Err(ThumbError::Damaged)
    );
    assert!(thumb::from_bytes(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]).is_err());
    // fuzz: mutated pictures never panic (fixed seed)
    let mut x: u64 = 42;
    for _ in 0..300 {
        let mut b = png_bytes.clone();
        for _ in 0..8 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let i = (x as usize) % b.len();
            b[i] = (x >> 32) as u8;
        }
        let _ = thumb::from_bytes(&b);
    }
    // an oversize header is refused before decoding
    let mut huge = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut huge, 9000, 1);
        enc.set_color(png::ColorType::Grayscale);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&vec![0; 9000]).unwrap();
    }
    assert_eq!(thumb::from_bytes(&huge), Err(ThumbError::TooLarge));
    let r = Root::new();
    r.write("media-pictures/مخطط.png", &png_bytes);
    assert!(thumb::thumbnail(&r.fs(), &loc(AreaId::Pictures, &["مخطط.png"])).is_ok());
}
