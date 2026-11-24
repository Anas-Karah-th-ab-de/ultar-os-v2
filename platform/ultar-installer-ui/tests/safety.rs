//! Installer safety: nothing is written before the explicit confirmation on the summary;
//! cancelling at every step writes nothing and ends with "cancelled".
mod common;
use common::*;
use ultar_i18n::Lang;
use ultar_installer_ui::action::Act;
use ultar_installer_ui::wizard::Step;
use ultar_installer_ui::Exit;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Size;

fn size() -> Size {
    Size::new(1280.0, 800.0)
}

#[test]
fn cancel_at_every_step_writes_nothing() {
    let dir = scratch("cancel");
    let src = medium(&dir);
    let tgt = target(&dir, 300 * MIB);
    let loopdev = Loop::attach(&tgt);
    let disk = loopdev.as_ref().map_or(tgt.clone(), |l| l.0.clone());
    let before = (sha(&tgt), sha(&src));
    for step in Step::ALL.iter().copied().filter(|s| *s <= Step::Summary) {
        let world = world();
        let mut w = wizard_at(&src, vec![disk_info(&disk, "Disk")], step);
        w.erase_confirmed = step == Step::Summary;
        let mut app = app_with(&world, w, Lang::Ar, ThemeKind::Dark, size());
        let cancel = app
            .view
            .registry
            .node(Act::Cancel)
            .expect("cancel button on every step");
        click(&mut app, cancel);
        assert!(app.model.cancel_dialog, "{step:?}: asks first");
        let yes = app.view.registry.node(Act::CancelYes).expect("dialog");
        click(&mut app, yes);
        assert_eq!(app.exit(), Some(Exit::Cancelled), "{step:?}");
        assert!(app.take_install().is_none(), "{step:?}: nothing to write");
    }
    drop(loopdev);
    assert_eq!(
        (sha(&tgt), sha(&src)),
        before,
        "target and medium unchanged"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cancel_dialog_no_goes_on() {
    let dir = scratch("cancel-no");
    let src = medium(&dir);
    let tgt = target(&dir, 300 * MIB);
    let world = world();
    let mut app = app_with(
        &world,
        wizard_at(&src, vec![disk_info(&tgt, "Disk")], Step::Disk),
        Lang::Ar,
        ThemeKind::Dark,
        size(),
    );
    let cancel = app.view.registry.node(Act::Cancel).unwrap();
    click(&mut app, cancel);
    let no = app.view.registry.node(Act::CancelNo).unwrap();
    click(&mut app, no);
    assert!(!app.model.cancel_dialog);
    assert_eq!(app.exit(), None);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn install_is_only_reachable_through_the_ticked_erase_line() {
    let dir = scratch("confirm");
    let src = medium(&dir);
    let tgt = target(&dir, 300 * MIB);
    let before = sha(&tgt);
    let world = world();
    let mut app = app_with(
        &world,
        wizard_at(&src, vec![disk_info(&tgt, "Disk")], Step::Summary),
        Lang::Ar,
        ThemeKind::Dark,
        size(),
    );
    let next = app.view.registry.node(Act::Next).unwrap();
    assert!(app.ui.is_disabled(next), "Install disabled before the tick");
    click(&mut app, next);
    assert!(app.take_install().is_none());
    let confirm = app.view.registry.node(Act::Confirm).unwrap();
    click(&mut app, confirm);
    assert!(app.model.wizard.erase_confirmed);
    assert!(!app.ui.is_disabled(next));
    click(&mut app, confirm);
    assert!(app.ui.is_disabled(next), "untick disables again");
    click(&mut app, confirm);
    let next = app.view.registry.node(Act::Next).unwrap();
    click(&mut app, next);
    let (confirmation, plan) = app.take_install().expect("confirmed install");
    assert_eq!(confirmation.target(), tgt.as_path());
    assert_eq!(plan.target, tgt);
    assert_eq!(app.model.wizard.step, Step::Install);
    assert!(
        app.view.registry.node(Act::Cancel).is_none(),
        "no cancel while writing"
    );
    assert_eq!(sha(&tgt), before, "the app itself never writes");
    app.finished(Ok(())).unwrap();
    assert_eq!(app.model.wizard.step, Step::Done);
    let restart = app.view.registry.node(Act::Restart).unwrap();
    click(&mut app, restart);
    assert_eq!(app.exit(), Some(Exit::Restart));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn choosing_another_disk_clears_the_tick() {
    let dir = scratch("redisk");
    let src = medium(&dir);
    let a = target(&dir, 300 * MIB);
    let b = dir.join("b.img");
    gpt_image(&b, 300 * MIB, &[]);
    let mut w = wizard_at(
        &src,
        vec![disk_info(&a, "A"), disk_info(&b, "B")],
        Step::Summary,
    );
    w.erase_confirmed = true;
    w.choose_disk(1);
    assert!(!w.erase_confirmed, "a new disk needs a new confirmation");
    std::fs::remove_dir_all(dir).unwrap();
}
