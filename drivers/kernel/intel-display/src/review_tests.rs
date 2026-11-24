//! Reviewer tests (round 1): hostile input to the parsers, the link fallback and plane checks.
//! These are expected to expose findings; failing ones are marked in the review report.

use crate::dp::dpcd::{Caps, LinkStatus};
use crate::dp::link::{self, LinkConfig, RATES};
use crate::dp::sink::test_edid;
use crate::mode::Timing;
use crate::plane::{PlaneConfig, XRGB8888};
use crate::{edid, vbt};
use alloc::vec::Vec;

/// Small deterministic PRNG (xorshift64*).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn byte(&mut self) -> u8 {
        self.next() as u8
    }
}

fn fix_checksum(block: &mut [u8]) {
    let s = block[..127].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    block[127] = 0u8.wrapping_sub(s);
}

/// Random and mutated EDIDs (with valid checksums, so the parser goes past the header checks)
/// never panic, and every timing that passes `validate` gives register values without overflow.
#[test]
fn edid_parser_survives_hostile_monitors() {
    let mut r = Rng(0x1234_5678_9abc_def1);
    for round in 0..20_000 {
        let mut e = test_edid();
        if round % 3 == 0 {
            for b in e.iter_mut() {
                *b = r.byte();
            }
            e[..8].copy_from_slice(&edid::HEADER);
        } else {
            for _ in 0..(r.next() % 40) {
                let i = (r.next() % e.len() as u64) as usize;
                e[i] = r.byte();
            }
            e[..8].copy_from_slice(&edid::HEADER);
        }
        e[126] = (r.next() % 3) as u8; // 0..2 extensions (only one present: exercises the length check)
        fix_checksum(&mut e[..128]);
        fix_checksum(&mut e[128..256]);
        if let Ok(info) = edid::parse(&e) {
            for t in info.timings {
                let _ = t.refresh_mhz();
                let _ = t.frame_ns();
                if t.validate().is_ok() {
                    let _ = t.regs();
                }
            }
        }
        // the short reads of the driver: any prefix
        let n = (r.next() % 300) as usize;
        let _ = edid::parse(&e[..n.min(e.len())]);
        let _ = edid::detailed_timing(&e[54..54 + (r.next() % 19) as usize]);
    }
}

/// CTA-861: `d` = 0 means "no detailed timing descriptors and no data blocks". The parser starts
/// at byte 4 anyway and decodes whatever is there as DTDs.
#[test]
fn cta_block_with_d_zero_has_no_detailed_timings() {
    // timings of the base block alone: byte 126 = 0, so it parses on its own (the first version
    // parsed the base block with byte 126 = 1, which is refused, and counted 0)
    let mut alone = test_edid()[..128].to_vec();
    alone[126] = 0;
    fix_checksum(&mut alone);
    let base = edid::parse(&alone).map(|i| i.timings.len()).unwrap_or(0);
    assert_eq!(base, 1, "the base block carries one detailed timing");
    let mut e = test_edid();
    e[126] = 1;
    fix_checksum(&mut e[..128]);
    let mut ext = [0u8; 128];
    ext[0] = 0x02;
    ext[1] = 0x03;
    ext[2] = 0; // d = 0: no DTDs, no data blocks
                // bytes that look like a 1080p DTD where none may be
    ext[4..22].copy_from_slice(&[
        0x02, 0x3a, 0x80, 0x18, 0x71, 0x38, 0x2d, 0x40, 0x58, 0x2c, 0x45, 0x00, 0, 0, 0, 0, 0, 0x1e,
    ]);
    fix_checksum(&mut ext);
    e.truncate(128);
    e.extend_from_slice(&ext);
    let info = edid::parse(&e).unwrap();
    assert_eq!(
        info.timings.len(),
        base,
        "timings decoded from a CTA block with d = 0"
    );
}

/// Random VBT images (with valid signatures and plausible offsets) never panic.
#[test]
fn vbt_parser_survives_hostile_firmware() {
    let mut r = Rng(0xdead_beef_cafe_f00d);
    for _ in 0..20_000 {
        let len = 48 + (r.next() % 400) as usize;
        let mut v: Vec<u8> = (0..len).map(|_| r.byte()).collect();
        v[..4].copy_from_slice(b"$VBT");
        let bdb = 48usize;
        v[28..32].copy_from_slice(&(bdb as u32).to_le_bytes());
        if v.len() > bdb + 22 {
            v[bdb..bdb + 16].copy_from_slice(b"BIOS_DATA_BLOCK ");
            v[bdb + 18..bdb + 20].copy_from_slice(&((r.next() % 40) as u16).to_le_bytes());
            v[bdb + 20..bdb + 22].copy_from_slice(&((r.next() % 600) as u16).to_le_bytes());
        }
        let _ = vbt::outputs(&v);
    }
}

/// DPCD and link status parsing take any bytes.
#[test]
fn dpcd_parsers_take_any_bytes() {
    let mut r = Rng(42);
    for _ in 0..50_000 {
        let mut d = [0u8; 16];
        d.iter_mut().for_each(|b| *b = r.byte());
        if let Ok(c) = Caps::parse(&d) {
            assert!(matches!(c.max_lanes, 1 | 2 | 4));
            assert!(c.aux_rd_interval_us.1 <= 16_000);
        }
        let mut s = [0u8; 6];
        s.iter_mut().for_each(|b| *b = r.byte());
        let st = LinkStatus::parse(&s);
        let _ = (st.cr_ok(4), st.eq_ok(4));
    }
}

/// `PlaneConfig::validate` must refuse a huge width (from a KMS framebuffer) instead of
/// overflowing in `width * bpp` (a panic in debug builds, a wrapped compare in release).
#[test]
fn plane_validate_does_not_overflow_on_huge_width() {
    let cfg = PlaneConfig {
        surf: 0x8000_0000,
        pitch: 64,
        width: 0x4000_0010,
        height: 1,
        format: XRGB8888,
    };
    let r = std::panic::catch_unwind(|| cfg.validate());
    assert!(
        matches!(r, Ok(Err(_))),
        "validate panicked (u32 overflow in width * bpp)"
    );
}

/// Finding I-1 (fixed): after a failed training the fallback (lower rate, then fewer lanes at the
/// sink's highest rate) reaches every narrower link that still carries the mode – in particular
/// 2 lanes for a DP-alt sink with a 2-lane pin assignment that advertises 4 lanes.
#[test]
fn link_fallback_reaches_every_narrower_link_that_fits() {
    let mut checked = 0;
    for clock in (25_000..=1_300_000).step_by(2_500) {
        for bpp in [18u32, 24, 30] {
            for sink in RATES {
                let Ok(first) = link::choose(clock, bpp, sink, 4) else {
                    continue;
                };
                // walk the whole fallback chain; every step trains fewer lanes or a lower rate
                let mut lim = link::Limits {
                    max_rate: sink,
                    max_lanes: 4,
                };
                let mut seen_two = first.lanes <= 2;
                let mut cur = first;
                while let Some(n) = link::next_after_failure(cur, &mut lim, sink, clock, bpp) {
                    assert!(
                        n.lanes < cur.lanes || n.rate < cur.rate,
                        "no progress: {cur:?} -> {n:?}"
                    );
                    assert!(link::data_rate(clock, bpp) * 1000 <= link::capacity(n) * 995);
                    seen_two |= n.lanes == 2;
                    cur = n;
                }
                let two_fits = link::choose(clock, bpp, sink, 2).is_ok();
                assert_eq!(seen_two, two_fits, "clock {clock} bpp {bpp} sink {sink}");
                checked += 1;
            }
        }
    }
    assert!(checked > 1000);
    // the example of the review: 1080p60 on a HBR2 x4 sink trains RBR x4 first, then HBR x2
    let cfg = link::choose(148_500, 24, 540_000, 4).unwrap();
    assert_eq!(
        cfg,
        LinkConfig {
            rate: 162_000,
            lanes: 4
        }
    );
    let mut lim = link::Limits {
        max_rate: 540_000,
        max_lanes: 4,
    };
    assert_eq!(
        link::next_after_failure(cfg, &mut lim, 540_000, 148_500, 24),
        Some(LinkConfig {
            rate: 270_000,
            lanes: 2
        })
    );
}

/// `Timing::regs` on a timing that was not validated underflows (`hdisplay - 1`): pub API,
/// documented nowhere as "validate first".
#[test]
fn timing_regs_on_unvalidated_timing() {
    let t = Timing::default();
    let r = std::panic::catch_unwind(|| t.regs());
    assert!(r.is_ok(), "Timing::regs panics on a zero timing");
}
