//! The shared clipboard between two paired nodes (docs/integration/mesh-clipboard.md):
//! text and pictures arrive, never without the right, never a confidential entry, no echo,
//! entries expire after two minutes, one notice in ten seconds.

mod common;
use common::World;
use std::time::Duration;
use ultar_bus::messages::mesh::{DeviceKind, Right};
use ultar_mesh::clip_image::MAX_IMAGE;
use ultar_mesh::node::Effect;
use ultar_mesh::wire::Frame;

const PHONE: (DeviceKind, &[&str]) = (DeviceKind::Phone, &[]);
const PC: (DeviceKind, &[&str]) = (DeviceKind::Desktop, &[]);

fn png(len: usize) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    bytes.extend((0..len.saturating_sub(8)).map(|i| (i % 253) as u8));
    bytes
}

/// Two paired devices; `share` switches "Share the clipboard" on for both.
fn pair(tag: &str, share: bool) -> World {
    let mut w = World::new(tag, &[PHONE, PC]);
    w.link(0, 1);
    w.pair(0, 1, None);
    if share {
        grant(&mut w, true);
    }
    w.take(0);
    w.take(1);
    w
}

fn grant(w: &mut World, on: bool) {
    let rights: &[Right] = if on {
        &[Right::Handoff, Right::Files, Right::Clipboard]
    } else {
        &[Right::Handoff, Right::Files]
    };
    let (a, b) = (w.id(0), w.id(1));
    assert!(w.nodes[0].set_rights(&b, rights).0);
    assert!(w.nodes[1].set_rights(&a, rights).0);
}

fn texts(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Clipboard(c) => Some(c.text.clone()),
            _ => None,
        })
        .collect()
}

fn pictures(effects: &[Effect]) -> Vec<Vec<u8>> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ClipboardImage { png, .. } => Some(png.clone()),
            _ => None,
        })
        .collect()
}

fn notices(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ClipNotice { from_name } => Some(from_name.clone()),
            _ => None,
        })
        .collect()
}

fn copy_text(w: &mut World, from: usize, text: &str) {
    let effects = w.nodes[from].clipboard_set(text, false);
    w.run(from, effects);
}

fn copy_image(w: &mut World, from: usize, image: &[u8]) {
    let effects = w.nodes[from].clipboard_image_set(image);
    w.run(from, effects);
}

#[test]
fn text_arrives_on_the_paired_device_with_a_notice() {
    let mut w = pair("clip-text", true);
    copy_text(&mut w, 0, "hallo – مرحبا");
    let got = w.take(1);
    assert_eq!(texts(&got), ["hallo – مرحبا"]);
    assert_eq!(notices(&got), ["dev0"], "the notice names the device");
    assert!(w.take(0).is_empty(), "the sender hears nothing back");
}

#[test]
fn a_picture_arrives_whole_and_four_mib_is_the_limit() {
    let mut w = pair("clip-image", true);
    let image = png(300_000);
    copy_image(&mut w, 0, &image);
    let got = w.take(1);
    assert_eq!(pictures(&got), [image]);
    assert_eq!(notices(&got).len(), 1);

    let biggest = png(MAX_IMAGE);
    copy_image(&mut w, 1, &biggest);
    assert_eq!(pictures(&w.take(0)), [biggest]);

    let too_big = png(MAX_IMAGE + 1);
    assert!(w.nodes[0].clipboard_image_set(&too_big).is_empty());
    assert!(w.nodes[0].clipboard_image_set(b"no picture").is_empty());
}

#[test]
fn nothing_travels_without_the_right_on_either_side() {
    // switched off from the start: the default of a new pairing
    let mut w = pair("clip-off", false);
    assert!(w.nodes[0].clipboard_set("secret plan", false).is_empty());
    assert!(w.nodes[0].clipboard_image_set(&png(1000)).is_empty());

    // the receiver has the right off: a frame that is sent anyway is dropped
    let mut w = pair("clip-recv", true);
    let a = w.id(0);
    assert!(w.nodes[1].set_rights(&a, &[Right::Handoff]).0);
    let frame = Frame::Clip {
        seq: 1,
        text: "forced".into(),
    };
    let now = w.now;
    assert!(w.nodes[1].frame(&a, frame, now).is_empty());

    // the same for a picture
    let mut w = pair("clip-send", true);
    let a = w.id(0);
    let pieces = w.nodes[0].clipboard_image_set(&png(100_000));
    assert!(!pieces.is_empty());
    assert!(w.nodes[1].set_rights(&a, &[]).0);
    w.run(0, pieces);
    assert!(pictures(&w.take(1)).is_empty());
    // switching it off later stops it at once
    let mut w = pair("clip-later", true);
    grant(&mut w, false);
    assert!(w.nodes[0].clipboard_set("late", false).is_empty());
}

#[test]
fn a_confidential_entry_never_leaves_and_an_echo_is_not_sent_back() {
    let mut w = pair("clip-conf", true);
    assert!(w.nodes[0].clipboard_set("hunter2", true).is_empty());
    assert!(w.take(1).is_empty());

    copy_text(&mut w, 0, "shared");
    assert_eq!(texts(&w.take(1)), ["shared"]);
    // the other device's clipboard owner sets it: that is the same text, not a new copy
    assert!(w.nodes[1].clipboard_set("shared", false).is_empty());

    let image = png(5000);
    copy_image(&mut w, 0, &image);
    assert_eq!(pictures(&w.take(1)).len(), 1);
    assert!(w.nodes[1].clipboard_image_set(&image).is_empty());
    // a different picture is a new copy
    assert!(!w.nodes[1].clipboard_image_set(&png(6000)).is_empty());
}

#[test]
fn an_entry_expires_after_two_minutes() {
    let mut w = pair("clip-expire", true);
    copy_text(&mut w, 0, "short lived");
    w.take(1);
    let start = w.now;
    let early = w.nodes[1].tick(start + Duration::from_secs(119));
    assert!(!early.contains(&Effect::ClipboardClear));
    let late = w.nodes[1].tick(start + Duration::from_secs(121));
    assert!(late.contains(&Effect::ClipboardClear));
    // once is enough
    assert!(w.nodes[1].tick(start + Duration::from_secs(130)).is_empty());
    // the same text can be copied and sent again afterwards
    assert!(!w.nodes[1].clipboard_set("short lived", false).is_empty());
}

#[test]
fn a_picture_expires_too() {
    let mut w = pair("clip-expire-img", true);
    let image = png(9000);
    copy_image(&mut w, 0, &image);
    w.take(1);
    let late = w.nodes[1].tick(w.now + Duration::from_secs(121));
    assert!(late.contains(&Effect::ClipboardClear));
    assert!(!w.nodes[1].clipboard_image_set(&image).is_empty());
}

#[test]
fn at_most_one_notice_in_ten_seconds() {
    let mut w = pair("clip-notice", true);
    copy_text(&mut w, 0, "one");
    assert_eq!(notices(&w.take(1)).len(), 1);
    w.now += Duration::from_secs(9);
    copy_text(&mut w, 0, "two");
    let got = w.take(1);
    assert_eq!(texts(&got), ["two"], "the entry still arrives");
    assert!(
        notices(&got).is_empty(),
        "but no second notice within ten seconds"
    );
    w.now += Duration::from_secs(2);
    copy_image(&mut w, 0, &png(2000));
    assert_eq!(
        notices(&w.take(1)).len(),
        1,
        "after ten seconds the next one"
    );
}

#[test]
fn a_damaged_picture_is_dropped_and_the_node_stays_usable() {
    let mut w = pair("clip-damaged", true);
    let image = png(100_000);
    let mut effects = w.nodes[0].clipboard_image_set(&image);
    // lose the second piece
    effects.remove(1);
    w.run(0, effects);
    assert!(pictures(&w.take(1)).is_empty());
    // a stale half picture is forgotten after half a minute
    w.nodes[1].tick(w.now + Duration::from_secs(40));
    // the next copy works
    copy_image(&mut w, 0, &png(3000));
    assert_eq!(pictures(&w.take(1)).len(), 1);
}

#[test]
fn an_unpaired_device_gets_no_clipboard() {
    let mut w = World::new("clip-unpaired", &[PHONE, PC]);
    w.link(0, 1);
    w.take(0);
    w.take(1);
    assert!(w.nodes[0].clipboard_set("hello", false).is_empty());
    assert!(w.nodes[0].clipboard_image_set(&png(100)).is_empty());
}
