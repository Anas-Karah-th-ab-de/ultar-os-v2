use super::*;

fn tone(hz: f32, n: usize) -> Vec<i16> {
    (0..n)
        .map(|i| (8000.0 * (i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin()) as i16)
        .collect()
}

/// Signal-to-noise ratio in dB of `got` against `want`.
fn snr(want: &[i16], got: &[i16]) -> f64 {
    let (mut s, mut n) = (0f64, 0f64);
    for (a, b) in want.iter().zip(got) {
        s += f64::from(*a).powi(2);
        n += (f64::from(*a) - f64::from(*b)).powi(2);
    }
    10.0 * (s / n.max(1.0)).log10()
}

#[test]
fn a_voice_frame_survives_the_codec() {
    let t = tone(440.0, FRAME);
    let c = encode(&t);
    assert_eq!(c.len(), CODED);
    let d = decode(&c).unwrap();
    assert_eq!(d.len(), FRAME);
    assert!(snr(&t, &d) > 20.0, "snr {}", snr(&t, &d));
    assert_eq!(decode(&c[..10]), None);
    assert_eq!(decode(&encode(&[])).unwrap(), vec![0; FRAME]);
}

#[test]
fn card_conversion_keeps_the_signal() {
    let t = tone(300.0, FRAME);
    let card = to_card(&t);
    assert_eq!(card.len(), FRAME * 6);
    let back = from_card(&card);
    assert_eq!(back.len(), FRAME);
    assert!(snr(&t, &back) > 15.0, "snr {}", snr(&t, &back));
}

#[test]
fn the_jitter_buffer_orders_conceals_and_does_not_lag() {
    let mut j = Jitter::default();
    let f = |v: i16| vec![v; FRAME];
    assert_eq!(j.take(), f(0), "silence before enough is there");
    for seq in [2u64, 1, 0] {
        j.put(seq, f(seq as i16 + 10));
    }
    assert_eq!(j.take(), f(10));
    assert_eq!(j.take(), f(11));
    j.put(4, f(14)); // 3 is lost
    assert_eq!(j.take(), f(12));
    assert_eq!(j.take(), f(6), "lost frame: the last one, quieter");
    assert_eq!(j.take(), f(14));
    j.put(1, f(99)); // far too late
    assert_eq!(j.take(), f(0), "empty: silence, then it waits again");
    for seq in 100..120 {
        j.put(seq, f(1));
    }
    assert!(j.frames.len() <= MAX_HELD, "old frames are dropped");
}
