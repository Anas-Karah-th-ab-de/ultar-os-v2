use super::*;
use crate::engine::DiskInfo;

fn wizard() -> Wizard {
    let disk = DiskInfo {
        path: "/nonexistent-target".into(),
        name: "Disk".into(),
        bytes: 1 << 40,
        partitions: vec![],
        table: None,
    };
    Wizard::new("/nonexistent-source".into(), vec![disk])
}

#[test]
fn steps_go_forward_and_back_and_jump_only_back() {
    let mut w = wizard();
    for _ in 0..6 {
        w.advance().unwrap();
    }
    assert_eq!(w.step, Step::Drivers);
    assert!(!w.jump(Step::Summary), "later steps cannot be jumped to");
    assert!(w.jump(Step::Keyboard));
    assert_eq!(w.step, Step::Keyboard);
    w.prev();
    assert_eq!(w.step, Step::Access);
    assert_eq!(Step::Encryption.page(), Step::Disk);
    assert_eq!(Step::Disk.next(), Step::Account);
    assert_eq!(Step::Account.prev(), Step::Disk);
}

#[test]
fn disk_step_is_blocked_without_a_plan_and_with_unavailable_modes() {
    let mut w = wizard();
    w.step = Step::Disk;
    assert_eq!(w.blocked(), Some("installer.reason.disk"));
    w.choose_disk(0);
    assert!(matches!(w.plan, Some(Err(_))), "missing medium");
    assert_eq!(w.blocked(), Some("installer.reason.medium"));
    w.plan = None;
    w.layout.mode = DiskMode::Beside;
    assert_eq!(w.blocked(), Some("installer.reason.disk"));
}

#[test]
fn install_needs_summary_confirmation_and_plan() {
    let mut w = wizard();
    assert_eq!(
        w.confirm_install().err(),
        Some("installer.reason.not_summary")
    );
    w.step = Step::Summary;
    assert_eq!(w.confirm_install().err(), Some("installer.reason.confirm"));
    w.erase_confirmed = true;
    assert_eq!(w.confirm_install().err(), Some("installer.reason.disk"));
    assert_eq!(w.advance(), Err("installer.reason.disk"));
}

#[test]
fn names_and_answers() {
    assert!(valid_name("anas"));
    assert!(valid_name("ultar-pc2"));
    assert!(!valid_name("Anas"));
    assert!(!valid_name("1pc"));
    assert!(!valid_name(""));
    assert!(!valid_name(&"a".repeat(40)));
    let mut w = wizard();
    w.account = ["أنس".into(), "anas".into(), "ultar-pc".into()];
    let a = w.answers();
    assert_eq!(a.get("user"), Some("anas"));
    assert_eq!(a.get("timezone"), Some("Asia/Damascus"));
    assert!(a.get("components").unwrap().contains("firefox"));
    w.step = Step::Account;
    w.account[1] = "X".into();
    assert_eq!(w.blocked(), Some("installer.reason.user"));
}
