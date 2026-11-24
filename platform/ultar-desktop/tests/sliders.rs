//! Regression (owner, VMware, Arabic): the volume and brightness sliders of the quick
//! settings said "0%" with the knob at the far end although no audio device and no display
//! setting exist. A slider without a device claims no value ("—", no knob); with a device it
//! shows the real level. REAL-INPUT: the sheet is opened by a pointer click on the bar.

mod common;

use common::*;
use ultar_bus::messages::audio::AudioState;
use ultar_desktop::model::{Open, QuickPage};
use ultar_desktop::view::Target;
use ultar_i18n::Lang;
use ultar_ui::geom::Rect;
use ultar_ui::tokens::ThemeKind;

fn audio(volume_percent: u8) -> AudioState {
    AudioState {
        present: true,
        volume_percent,
        muted: false,
        output_name: "Speakers".into(),
        output_id: Some("hw0".into()),
        has_volume: true,
    }
}

/// The tiles sheet of `d`, opened with a click beside the status icons.
fn sheet(d: ultar_desktop::model::Desktop) -> Harness {
    let mut h = Harness::with(d, Lang::Ar, ThemeKind::Dark);
    h.click_bar(Target::Status);
    assert_eq!(h.shell.d.open, Open::Quick(QuickPage::Tiles));
    h
}

/// What the slider says to assistive technology, and whether it can be moved.
fn said(h: &mut Harness, target: &Target) -> (Option<String>, bool) {
    let view = &mut h.popup.as_mut().expect("the sheet").view;
    let n = view.node(target).expect("the slider");
    let tree = view.ui.a11y_tree();
    let node = tree
        .iter()
        .find(|x| x.id == n && x.part.is_none())
        .expect("its accessibility node");
    let value = node.value.as_ref().map(|v| format!("{v:?}"));
    (value, view.ui.is_disabled(n))
}

/// The pixels of the slider's place in the painted sheet.
fn painted(h: &mut Harness, target: &Target) -> Vec<u8> {
    let view = &mut h.popup.as_mut().expect("the sheet").view;
    let n = view.node(target).expect("the slider");
    view.ui.layout();
    let r: Rect = view.ui.rect(n).unwrap();
    let size = view.ui.size();
    let canvas = render(view, size);
    let image = ultar_ui::snapshot::Image::from_canvas(&canvas);
    let (x0, y0) = (r.x.floor() as usize, r.y.floor() as usize);
    let (x1, y1) = ((r.x + r.w).ceil() as usize, (r.y + r.h).ceil() as usize);
    let mut out = Vec::new();
    for y in y0..y1.min(image.height as usize) {
        for x in x0..x1.min(image.width as usize) {
            let i = (y * image.width as usize + x) * 4;
            out.extend_from_slice(&image.rgba[i..i + 4]);
        }
    }
    assert!(!out.is_empty(), "the slider is on the sheet");
    out
}

#[test]
fn sliders_without_a_device_claim_no_value() {
    // the owner's virtual machine: no audio service, no display that can be dimmed
    let mut d = board_desktop(Lang::Ar, ThemeKind::Dark);
    d.svc.audio = None;
    d.values.brightness = None;
    let mut none = sheet(d);
    for t in [Target::Volume, Target::Brightness] {
        let (value, disabled) = said(&mut none, &t);
        assert_eq!(value, None, "{t:?} claims no value");
        assert!(disabled, "{t:?} cannot be moved");
    }
    // a device that is there but has no volume of its own
    let mut d = board_desktop(Lang::Ar, ThemeKind::Dark);
    d.svc.audio = Some(AudioState {
        has_volume: false,
        ..audio(0)
    });
    d.presence.audio_service = true;
    let mut fixed = sheet(d);
    assert_eq!(said(&mut fixed, &Target::Volume), (None, true));
    // no knob: the picture is neither the one of 0 % nor the one of 100 %
    let unknown = painted(&mut none, &Target::Volume);
    for level in [0, 100] {
        let mut d = board_desktop(Lang::Ar, ThemeKind::Dark);
        d.svc.audio = Some(audio(level));
        d.presence.audio_service = true;
        d.values.brightness = None;
        let mut with = sheet(d);
        assert_ne!(
            unknown,
            painted(&mut with, &Target::Volume),
            "no device looks like {level} %"
        );
    }
}

#[test]
fn sliders_with_a_device_show_the_real_level() {
    let mut d = board_desktop(Lang::Ar, ThemeKind::Dark);
    d.svc.audio = Some(audio(40));
    d.presence.audio_service = true;
    // (the slider moves in steps of 5: a level on a step is written as it is)
    d.values.brightness = Some(70);
    let mut h = sheet(d);
    let (volume, disabled) = said(&mut h, &Target::Volume);
    assert!(
        volume.as_deref().is_some_and(|v| v.contains("40")),
        "{volume:?}"
    );
    assert!(!disabled, "the volume can be set");
    let (brightness, disabled) = said(&mut h, &Target::Brightness);
    assert!(
        brightness.as_deref().is_some_and(|v| v.contains("70")),
        "{brightness:?}"
    );
    assert!(!disabled, "the brightness can be set");
}
