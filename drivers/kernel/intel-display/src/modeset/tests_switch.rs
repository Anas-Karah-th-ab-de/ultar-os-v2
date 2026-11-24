//! Link-training fallback, the switch with rollback and the disable path's waits, against the
//! simulated display engine of `tests.rs`.

use super::tests::{engine, request, TC4};
use super::*;
use crate::sim::fhd;
use alloc::boxed::Box;

/// The sink cannot train four lanes: the first attempt (RBR x4) fails, the DP fallback lowers
/// the lane count and the mode comes up on HBR x2.
#[test]
fn failed_first_attempt_comes_up_on_the_fallback_link() {
    let (mut s, sink) = engine();
    let caps = read_caps(&mut s, TC4, 14).unwrap();
    sink.borrow_mut().fail_lanes = Some(4);
    let cfg = enable_dp(&mut s, &request(caps)).unwrap();
    assert_eq!(
        cfg,
        link::LinkConfig {
            rate: 270_000,
            lanes: 2
        }
    );
    let k = sink.borrow();
    assert_eq!(
        k.lane_requests.first(),
        Some(&0x04),
        "first attempt on four lanes"
    );
    assert_eq!(k.lane_requests.last(), Some(&0x02), "trained on two lanes");
    assert_eq!(k.dpcd[0x100], 0x0a, "HBR");
    // DDI_FUNC_CTL port width x2, normal pattern, pipe running
    assert_eq!((s.get(0x60400) >> 1) & 7, 1);
    assert_eq!(
        s.get(ddi::dp_tp_ctl(0)) & ddi::TP_TRAIN_MASK,
        ddi::TrainPattern::Normal.bits()
    );
    assert!(s.get(0x70008) & regs::TRANSCONF_STATE != 0);
}

/// 1080p120 on a sink that cannot train HBR (the only rate that carries it): the new mode fails
/// after the old one was turned off, so the old one is lit again with its own buffer.
#[test]
fn failed_switch_restores_the_old_mode() {
    let (mut s, sink) = engine();
    let caps = read_caps(&mut s, TC4, 14).unwrap();
    let old = request(caps);
    enable_dp(&mut s, &old).unwrap();
    sink.borrow_mut().fail_rate = Some(0x0a);
    let mut new = request(caps);
    new.timing = Timing {
        clock: 297_000,
        ..fhd()
    };
    new.plane.surf = 0x9000_0000;
    let r = switch_dp(&mut s, Some(&old), &new);
    let rolled = link::LinkConfig {
        rate: 162_000,
        lanes: 4,
    };
    assert!(
        matches!(r, Err((Error::LinkTraining(_), Some(Ok(c)))) if c == rolled),
        "{r:?}"
    );
    let conf = s.writes_to(0x70008);
    assert!(
        conf.iter().any(|v| v & regs::TRANSCONF_ENABLE == 0),
        "the old pipe was turned off for the new mode"
    );
    assert_eq!(
        conf.last().map(|v| v & regs::TRANSCONF_ENABLE),
        Some(regs::TRANSCONF_ENABLE)
    );
    assert_eq!(
        s.get(0x7019c),
        0x8000_0000,
        "the old buffer is scanned out again"
    );
    assert_eq!(s.get(0x60000), 0x0897_077f, "old timing");
    assert!(s.get(0x70008) & regs::TRANSCONF_STATE != 0);
    assert!(s.get(port::buf_ctl1(TC4)) & port::BUF1_TC_PHY_OWNERSHIP != 0);
}

/// A new mode that fails the checks touches nothing: the old mode keeps running.
#[test]
fn refused_switch_touches_nothing() {
    let (mut s, _sink) = engine();
    let caps = read_caps(&mut s, TC4, 14).unwrap();
    let old = request(caps);
    enable_dp(&mut s, &old).unwrap();
    let before = s.writes.len();
    let mut bad = request(caps);
    bad.plane.pitch = 64; // narrower than a line
    let r = switch_dp(&mut s, Some(&old), &bad);
    assert!(matches!(r, Err((Error::Invalid(_), None))), "{r:?}");
    assert_eq!(s.writes.len(), before, "no register written");
    assert!(s.get(0x70008) & regs::TRANSCONF_STATE != 0);
}

/// A good switch: the old pipe goes off, the new mode runs on the new buffer.
#[test]
fn switch_lights_the_new_mode() {
    let (mut s, _sink) = engine();
    let caps = read_caps(&mut s, TC4, 14).unwrap();
    let old = request(caps);
    enable_dp(&mut s, &old).unwrap();
    let mut new = request(caps);
    new.plane.surf = 0x9000_0000;
    assert!(switch_dp(&mut s, Some(&old), &new).is_ok());
    assert_eq!(s.get(0x7019c), 0x9000_0000);
    assert!(s.get(0x70008) & regs::TRANSCONF_STATE != 0);
}

/// `Sim` answers 0 for registers it does not model, so a wait for a bit to clear passes at once.
/// Here the port's D2D link never reports off: the disable path must notice (a missing wait
/// would not).
#[test]
fn disable_reports_a_port_that_does_not_go_idle() {
    let (mut s, _sink) = engine();
    let caps = read_caps(&mut s, TC4, 14).unwrap();
    enable_dp(&mut s, &request(caps)).unwrap();
    let r = port::buf_ctl1(TC4);
    s.on_write(
        r,
        Box::new(|regs, reg, v| {
            regs.insert(reg, v | port::BUF1_D2D_STATE);
        }),
    );
    s.set(r, s.get(r) | port::BUF1_D2D_STATE);
    assert!(matches!(
        disable(&mut s, 0, TC4),
        Err(Error::Timeout { reg, .. }) if reg == r
    ));
    // every step ran anyway
    assert_eq!(s.get(ddi::func_ctl(0)), 0);
    assert!(!pll::enabled(&mut s, TC4));
}
