//! Tests of the plan's command line and the status lines.

use super::*;

fn plan() -> CellPlan {
    CellPlan {
        identity: Some((20001, 20001)),
        group_dir: Some("/g/apps/camera".into()),
        namespaces: Namespaces {
            mount: true,
            pid: true,
            net: true,
            ipc: false,
        },
        root: "/run/c/camera-ui".into(),
        data: "/data/apps/camera".into(),
        view: vec!["/usr".into(), "/bin".into()],
        shares: vec!["/run/ultar/ui".into()],
        areas: vec![
            ("/data/ultar/areas/home".into(), "home".into(), true),
            (
                "/data/ultar/areas/media-music".into(),
                "media-music".into(),
                false,
            ),
        ],
        publish: Some("/data/ultar/publish/camera".into()),
        groups: vec![833],
        dev: DevMode::Binds,
        tmp_bytes: 64 << 20,
        drop_rights: Some(u64::MAX),
        nproc: Some(64),
        runtime: Some("/data/ultar/foreign/linux".into()),
        bridge: Some("/usr/lib/ultar/foreign".into()),
        setup: Some("/data/ultar/foreign/setup/foreign.exe-x".into()),
        net_barrier: Some(true),
        program: Program {
            exec: "/usr/bin/camera".into(),
            args: vec!["--".into(), "".into(), "--uid".into()],
        },
    }
}

#[test]
fn a_plan_survives_its_command_line() {
    let p = plan();
    assert_eq!(CellPlan::parse(&p.to_args()), Some(p.clone()));
    let bare = CellPlan {
        identity: None,
        group_dir: None,
        namespaces: Namespaces::default(),
        view: Vec::new(),
        shares: Vec::new(),
        areas: Vec::new(),
        publish: None,
        groups: Vec::new(),
        dev: DevMode::None,
        drop_rights: None,
        nproc: None,
        runtime: None,
        bridge: None,
        setup: None,
        net_barrier: None,
        ..p
    };
    let args = bare.to_args();
    assert!(args.contains(&"none".to_owned()));
    assert_eq!(CellPlan::parse(&args), Some(bare));
}

#[test]
fn the_barrier_has_two_words_only() {
    let mut p = plan();
    p.net_barrier = Some(false);
    let args = p.to_args();
    assert!(args.contains(&"internet".to_owned()));
    assert_eq!(CellPlan::parse(&args), Some(p));
    let mut bad = plan().to_args();
    let at = bad.iter().position(|w| w == "lan").unwrap();
    bad[at] = "all".into();
    assert_eq!(CellPlan::parse(&bad), None);
}

#[test]
fn command_lines_the_launcher_never_writes_are_refused() {
    let good = plan().to_args();
    let replace = |from: &str, to: &str| -> Vec<String> {
        let mut words = good.clone();
        let at = words.iter().position(|w| w == from).unwrap();
        words[at] = to.to_owned();
        words
    };
    let cases = [
        replace("20001", "x"),
        replace("/usr", "usr"),
        replace("/usr", "/usr/../etc"),
        replace("mount,pid,net", "mount,mount"),
        replace("mount,pid,net", "user"),
        replace("binds", "all"),
        replace("--tmp", "--frob"),
        replace(
            "home:rw:/data/ultar/areas/home",
            "home:rx:/data/ultar/areas/home",
        ),
        replace(
            "home:rw:/data/ultar/areas/home",
            "../x:rw:/data/ultar/areas/home",
        ),
        replace("home:rw:/data/ultar/areas/home", "home:rw:data/home"),
        replace("home:rw:/data/ultar/areas/home", "home:rw:/data/../etc"),
        replace("home:rw:/data/ultar/areas/home", "Home:rw:/x"),
        replace("833", "x"),
        replace("/usr/bin/camera", "camera"),
        good[..good.len() - 4].to_vec(),
        good[2..].to_vec(),
        Vec::new(),
        vec!["--".into(), "/bin/x".into()],
    ];
    for case in cases {
        assert_eq!(CellPlan::parse(&case), None, "{case:?}");
    }
}

#[test]
fn status_lines_round_trip_and_junk_is_refused() {
    let lines = [
        StatusLine::Cell(42),
        StatusLine::Ready,
        StatusLine::Fail(CellFailure::Mounts),
        StatusLine::Exit(0),
        StatusLine::Exit(-1),
        StatusLine::Signal(9),
    ];
    for line in lines {
        assert_eq!(StatusLine::parse(&line.render()), Some(line));
        assert!(line.render().len() <= 64);
    }
    for junk in [
        "",
        "CELL 0",
        "CELL -1",
        "READY now",
        "FAIL 1",
        "SIGNAL 99",
        "EXIT x",
        "HELLO",
    ] {
        assert_eq!(StatusLine::parse(junk), None, "{junk}");
    }
    for failure in CellFailure::ALL {
        assert_eq!(CellFailure::from_code(failure as i32), Some(failure));
        assert!(failure.word().starts_with("cell_"));
    }
    assert_eq!(CellFailure::from_code(0), None);
}

#[test]
fn stages_without_their_status_pipe_end_with_the_argument_code() {
    // no status descriptor in the environment (this test process has none)
    assert_eq!(stage_one(&[]), CellFailure::Arguments as i32);
    assert_eq!(stage_two(&[]), CellFailure::Arguments as i32);
}
