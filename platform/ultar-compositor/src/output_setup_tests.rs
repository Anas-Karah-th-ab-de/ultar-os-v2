use super::*;
use crate::backend::BackendOutput;
use ultar_surface::proto::OutputSetup;

fn m(w: u32, h: u32) -> OutputMode {
    OutputMode {
        width: w,
        height: h,
        refresh_mhz: 60_000,
    }
}

fn layout() -> Layout {
    let mut l = Layout::new();
    for hal in 0..2 {
        l.add_output(&BackendOutput {
            hal,
            name: format!("out-{hal}"),
            width: 1920,
            height: 1080,
            refresh_millihertz: 60_000,
            modes: vec![m(1920, 1080), m(1280, 720)],
            ..BackendOutput::default()
        });
    }
    l
}

fn setup(l: &Layout) -> Setup {
    Setup {
        serial: 1,
        arrangement: Arrangement::Extend,
        provisional_ms: 0,
        outputs: l
            .outputs()
            .iter()
            .enumerate()
            .map(|(i, o)| OutputSetup {
                output: o.id,
                included: true,
                primary: i == 1,
                mode: m(1280, 720),
                scale_milli: 1000,
                position: (if i == 1 { 0 } else { 1280 }, 0),
                transform: Transform::Normal,
            })
            .collect(),
    }
}

#[test]
fn a_setup_is_checked_applied_and_taken_back() {
    let mut l = layout();
    let before = l.snapshot();
    let s = setup(&l);
    assert_eq!(l.check_setup(&s), Ok(()));
    l.apply_setup(&s);
    let primary = l.primary().map(|o| (o.id, o.x, o.width));
    assert_eq!(primary, Some((OutputId(1), 0, 1280)));
    assert_eq!(l.get(OutputId(0)).map(|o| o.x), Some(1280));
    assert_eq!(l.modes_to_restore(&before).len(), 2);
    l.restore(&before, &[0, 1]);
    assert_eq!(l.snapshot(), before);
    assert_eq!(
        l.details_of(OutputId(0)).map(|d| d.current),
        Some(m(1920, 1080))
    );
}

#[test]
fn restore_keeps_a_mode_the_backend_could_not_switch_back() {
    let mut l = layout();
    let before = l.snapshot();
    l.apply_setup(&setup(&l));
    l.restore(&before, &[1]);
    assert_eq!(l.get(OutputId(0)).map(|o| o.width), Some(1280));
    assert_eq!(l.get(OutputId(1)).map(|o| o.width), Some(1920));
    assert_eq!(l.outputs().iter().filter(|o| o.primary).count(), 1);
}

#[test]
fn restore_after_an_unplug_and_details() {
    let mut l = layout();
    let mut single = setup(&l);
    single.arrangement = Arrangement::Single;
    single.outputs[0].included = false;
    let before = l.snapshot();
    l.apply_setup(&single);
    let snapshot = l.snapshot();
    assert_eq!(l.enabled().count(), 1);
    assert!(l.remove(OutputId(1)));
    l.restore(&snapshot, &[]);
    assert_eq!(l.enabled().count(), 1, "the remaining display is shown");
    assert_eq!(l.primary().map(|o| o.id), Some(OutputId(0)));
    assert!(l.details_of(OutputId(1)).is_none());
    assert_eq!(l.modes_of(OutputId(0)).len(), 2);
    assert_eq!(l.modes_of(OutputId(9)).len(), 0);
    let _ = before;
}
