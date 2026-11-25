//! The accessibility view reaches the render path: the display service configures
//! the magnifier and colour filter (`Request::ConfigureVisual`, right
//! `ui.output-config`), the shell asks for the visual-alert flash
//! (`Request::VisualAlert`, right `ui.shell`); an app may neither. The effects show
//! in the presented frame and go again when the setting changes or the flash expires.

mod common;

use common::{compositor, test_config, Peer};
use std::time::Duration;
use ultar_compositor::{Compositor, Config, RightsTable};
use ultar_surface::proto::{ColourFilter, ErrorCode, OutputId, Request, Rights};

const UID_DISPLAYD: u32 = 817;
const UID_SHELL: u32 = 816;
const UID_APP: u32 = 20_001;

/// One display; the display service holds the configuration right, the shell the
/// shell right.
fn rig() -> Compositor {
    let mut rights = RightsTable::default();
    rights.grant(UID_DISPLAYD, Rights::OUTPUT_CONFIG);
    rights.grant(UID_SHELL, Rights::SHELL);
    let config = Config {
        rights,
        ..test_config()
    };
    compositor(config, &[(1280, 800)])
}

fn output(comp: &Compositor) -> OutputId {
    comp.state.layout.enabled().next().expect("output").id
}

/// How many pixels of the frame are exactly `color`.
fn count_color(comp: &Compositor, id: OutputId, color: u32) -> usize {
    comp.frame(id)
        .expect("frame")
        .data()
        .iter()
        .filter(|&&p| p == color)
        .count()
}

/// A green window and the pointer at its content centre. The window's peer is
/// returned so the test keeps the connection (and the window) alive.
fn green_window(comp: &mut Compositor) -> (Peer, OutputId, i32, i32) {
    let mut w = Peer::hello(comp);
    w.window(comp, 1, 0x00ff00);
    w.pump(comp);
    w.take();
    let id = output(comp);
    let f = comp.state.windows[0].frame;
    let (cx, cy) = (f64::from(f.x + f.w / 2), f64::from(f.y + f.h / 2));
    common::move_to(comp, cx, cy);
    let (px, py) = (comp.state.pointer.x as i32, comp.state.pointer.y as i32);
    (w, id, px, py)
}

fn visual(zoom: bool, zoom_percent: u16, filter: ColourFilter) -> Request {
    Request::ConfigureVisual {
        zoom,
        zoom_percent,
        filter,
    }
}

#[test]
fn the_magnifier_scales_the_output_around_the_pointer() {
    let mut comp = rig();
    let (_w, id, px, py) = green_window(&mut comp);
    comp.render_now();
    let before = count_color(&comp, id, 0xff00_ff00);
    assert!(before > 0, "the window is drawn");

    let mut s = Peer::hello_as(&mut comp, UID_DISPLAYD);
    s.send(&visual(true, 200, ColourFilter::None));
    s.pump(&mut comp);
    assert!(comp.state.session.visual.zoom_active());
    comp.render_now();
    let after = count_color(&comp, id, 0xff00_ff00);
    assert!(
        after > before,
        "200 % spreads the colour: {after} > {before}"
    );
    // The point under the pointer stays fixed.
    assert_eq!(comp.frame(id).unwrap().pixel(px, py), Some(0xff00_ff00));
    // Incremental composition gives the same pixels as the full reference.
    let reference = comp.render_reference(id).expect("reference");
    assert_eq!(comp.frame(id).unwrap().data(), reference.data());

    // Off again: the frame is the clean composition once more.
    s.send(&visual(false, 200, ColourFilter::None));
    s.pump(&mut comp);
    comp.render_now();
    assert_eq!(count_color(&comp, id, 0xff00_ff00), before);
    assert!(!comp.state.session.visual.active(comp.state.now));
}

#[test]
fn the_colour_filter_reaches_every_pixel() {
    let mut comp = rig();
    let (_w, id, px, py) = green_window(&mut comp);
    let mut s = Peer::hello_as(&mut comp, UID_DISPLAYD);
    s.send(&visual(false, 200, ColourFilter::Greyscale));
    s.pump(&mut comp);
    comp.render_now();
    let expected = ultar_compositor::effects::filter_pixel(0xff00_ff00, ColourFilter::Greyscale);
    assert_eq!(comp.frame(id).unwrap().pixel(px, py), Some(expected));
    // No pure green is left anywhere.
    assert_eq!(count_color(&comp, id, 0xff00_ff00), 0);
    // Another filter at runtime.
    s.send(&visual(false, 200, ColourFilter::Invert));
    s.pump(&mut comp);
    comp.render_now();
    let inverted = ultar_compositor::effects::filter_pixel(0xff00_ff00, ColourFilter::Invert);
    assert_eq!(comp.frame(id).unwrap().pixel(px, py), Some(inverted));
    // None restores the colour.
    s.send(&visual(false, 200, ColourFilter::None));
    s.pump(&mut comp);
    comp.render_now();
    assert_eq!(comp.frame(id).unwrap().pixel(px, py), Some(0xff00_ff00));
}

#[test]
fn the_visual_alert_flashes_and_goes_by_itself() {
    let mut comp = rig();
    let id = output(&comp);
    comp.render_now();
    let base = comp.frame(id).unwrap().pixel(0, 0).expect("corner");
    let mut sh = Peer::hello_as(&mut comp, UID_SHELL);
    sh.send(&Request::VisualAlert { ms: 200 });
    sh.pump(&mut comp);
    let until = comp.state.session.visual.flash_until;
    assert!(until > comp.state.now);
    comp.render_now();
    let flashed = comp.frame(id).unwrap().pixel(0, 0).expect("corner");
    assert!(
        flashed > base,
        "the flash lightens the screen: {flashed:#010x}"
    );
    // When its time is over the flash goes without another request.
    comp.state.now = until;
    comp.dispatch(Some(Duration::ZERO)).unwrap();
    assert_eq!(comp.state.session.visual.flash_until, 0);
    comp.render_now();
    assert_eq!(comp.frame(id).unwrap().pixel(0, 0), Some(base));
}

#[test]
fn the_view_rights_are_separate_and_apps_have_neither() {
    let mut comp = rig();
    let (_w, _id, _, _) = green_window(&mut comp);
    // An app may neither configure the view nor ask for a flash.
    let mut app = Peer::hello_as(&mut comp, UID_APP);
    app.send(&visual(true, 400, ColourFilter::None));
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
    assert!(!comp.state.session.visual.zoom);
    // The display service may configure the view but not flash (no shell right).
    let mut displayd = Peer::hello_as(&mut comp, UID_DISPLAYD);
    displayd.send(&Request::VisualAlert { ms: 200 });
    displayd.pump(&mut comp);
    assert_eq!(displayd.error(), Some(ErrorCode::Permission));
    assert_eq!(comp.state.session.visual.flash_until, 0);
    // The shell may flash but not configure the view (no output-config right).
    let mut shell = Peer::hello_as(&mut comp, UID_SHELL);
    shell.send(&visual(true, 400, ColourFilter::None));
    shell.pump(&mut comp);
    assert_eq!(shell.error(), Some(ErrorCode::Permission));
    assert!(!comp.state.session.visual.zoom);
}
