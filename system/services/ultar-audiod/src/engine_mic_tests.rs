//! Flow tests of the microphone side of the engine: fan-out, bus topic, clips, latency, echo.

use super::*;

/// One second of a 1 kHz tone at about -6 dBFS on both channels, `frames` frames from `*n`.
fn tone(n: &mut u64, frames: usize) -> Vec<i16> {
    let out = (0..frames)
        .flat_map(|i| {
            let phase = 2.0 * std::f32::consts::PI * 1000.0 * (*n + i as u64) as f32 / 48_000.0;
            let s = (0.5 * phase.sin() * 32768.0) as i16;
            [s, s]
        })
        .collect();
    *n += frames as u64;
    out
}

/// A rig with the voice helper (16 kHz mono) and the mesh (48 kHz stereo) recording.
fn two_recorders(tag: &str) -> (Rig, Stream, Stream) {
    let mut r = Rig::new(
        tag,
        &[
            ("system.voiced", mic_rights()),
            ("system.mesh", mic_rights()),
        ],
    );
    let a = r.connect("system.voiced", capture(16_000, 1)).unwrap();
    let b = r.connect("system.mesh", capture(48_000, 2)).unwrap();
    (r, a, b)
}

fn volts(x: &[i16]) -> Vec<f32> {
    x.iter().map(|&s| f32::from(s) / 32768.0).collect()
}

#[test]
fn the_microphone_goes_to_every_recorder_at_its_own_rate() {
    let (mut r, mut a, mut b) = two_recorders("fan");
    let (mut ga, mut gb) = (Vec::new(), Vec::new());
    let mut n = 0u64;
    for _ in 0..200 {
        r.mic(&tone(&mut n, 240));
        r.step();
        a.pull(&mut ga).unwrap();
        b.pull(&mut gb).unwrap();
    }
    assert!(
        (ga.len() as i64 - 16_000).abs() < 600,
        "16 kHz mono: {}",
        ga.len()
    );
    assert!(
        (gb.len() as i64 - 96_000).abs() < 1_200,
        "48 kHz stereo: {}",
        gb.len()
    );
    let fb: Vec<f32> = volts(&gb[4800..]).into_iter().step_by(2).collect();
    let want = 0.5 / 2f32.sqrt();
    assert!((rms(&volts(&ga[800..])) - want).abs() < 0.02);
    assert!((rms(&fb) - want).abs() < 0.02);
}

#[test]
fn the_microphone_goes_to_the_bus_in_chunks_and_stops_when_nobody_records() {
    let (mut r, a, b) = two_recorders("bus");
    r.eng.set_bus_capture(vec!["system.voiced".into()]);
    r.step();
    assert!(r
        .events
        .iter()
        .any(|e| matches!(e, Event::Mic(u) if u.len() == 2)));
    let mut n = 0u64;
    for _ in 0..200 {
        r.mic(&tone(&mut n, 240));
        r.step();
    }
    let chunks: Vec<&Vec<i16>> = r
        .events
        .iter()
        .filter_map(|e| {
            if let Event::Chunk(c) = e {
                Some(c)
            } else {
                None
            }
        })
        .collect();
    assert!(
        (chunks.len() as i64 - 50).abs() <= 2,
        "{} chunks of 20 ms for the bus",
        chunks.len()
    );
    assert!(chunks.iter().all(|c| c.len() == CHUNK));
    // everybody stops: the microphone is not in use any more
    drop((a, b));
    r.eng.set_bus_capture(Vec::new());
    r.run(100);
    let last = r.events.iter().rev().find_map(|e| match e {
        Event::Mic(u) => Some(u.clone()),
        _ => None,
    });
    assert_eq!(last, Some(Vec::new()));
}

#[test]
fn a_clip_of_the_bus_is_played_as_a_prompt_and_reported_done() {
    let mut r = Rig::new("clip", &[]);
    let tone: Vec<i16> = (0..1600)
        .map(|i| ((i as f32 * 0.3).sin() * 12_000.0) as i16)
        .collect();
    r.eng.play_clip(7, 16_000, &tone);
    r.run(400);
    assert!(r.events.contains(&Event::ClipDone {
        request: 7,
        ok: true
    }));
    let played = r.played();
    assert!(played.len() > 4_000 && played.iter().any(|s| s.abs() > 8_000));
    assert!(
        (played.len() as i64 - 9_600).abs() < 1_000,
        "100 ms at 48 kHz stereo: {}",
        played.len()
    );
    // no card: the answer is "no"
    let mut n = Rig::without_card("clip2", &[]);
    n.eng.play_clip(8, 16_000, &tone);
    n.step();
    assert!(n.events.contains(&Event::ClipDone {
        request: 8,
        ok: false
    }));
}

#[test]
fn the_latency_of_a_stream_is_inside_the_budget() {
    let mut r = Rig::new("lat", &[("system.web", play_rights())]);
    let mut s = r.connect("system.web", media()).unwrap();
    let mut first_write = None;
    let mut marker_at = 0u64;
    for step in 0..400u64 {
        let v = if step < 200 { 0.1 } else { 0.6 };
        if step == 200 {
            marker_at = step;
        }
        s.push(&level(240, v)).unwrap();
        r.step();
        if first_write.is_none() && !r.card.lock().unwrap().played.is_empty() {
            first_write = Some(step);
        }
    }
    let played = r.played();
    // frame index of the first loud frame; the card plays frame j at start + j / 48000
    let j = played
        .chunks_exact(2)
        .position(|f| f[0] > (0.3 * 32768.0) as i16)
        .unwrap() as f64;
    let start_ms = first_write.unwrap() as f64 * 5.0;
    let latency = start_ms + j / 48.0 - marker_at as f64 * 5.0;
    println!("latency of a media stream in steady state: {latency:.1} ms (cushion 10 ms + queue 10 ms + 5 ms step)");
    assert!(latency <= 40.0, "{latency} ms");
    // and the start of sound after the first byte
    assert!(
        first_write.unwrap() <= 4,
        "sound starts within {} steps",
        first_write.unwrap()
    );
}

/// Five ms of speech-like noise at 16 kHz (a first-order low pass of white noise).
fn far_chunk(seed: &mut u64, far: &mut f32) -> Vec<i16> {
    (0..80)
        .map(|_| {
            *seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let n = ((*seed >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0;
            *far = 0.9 * *far + 0.25 * n;
            (*far * 32768.0) as i16
        })
        .collect()
}

/// The room: what the card played (mono, by frame) comes back 30 ms and 55 ms later, weaker.
fn room_mic(src: &[f32], mic_at: &mut usize) -> Vec<i16> {
    let mut mic = Vec::new();
    for _ in 0..240 {
        let at = |back: usize| {
            mic_at
                .checked_sub(back)
                .and_then(|i| src.get(i))
                .copied()
                .unwrap_or(0.0)
        };
        let s = ((0.30 * at(1440) + 0.15 * at(2640)) * 32768.0) as i16;
        mic.extend([s, s]);
        *mic_at += 1;
    }
    mic
}

#[test]
fn echo_cancellation_through_the_engine_removes_the_speaker_from_the_call_microphone() {
    let mut r = Rig::new(
        "echo",
        &[
            ("system.calld", call_rights()),
            ("system.mesh", mic_rights()),
        ],
    );
    let mut hello = Hello::new(Class::Call, Direction::Duplex, 16_000, 1);
    hello.echo_cancel = true;
    let mut call = r.connect("system.calld", hello).unwrap();
    // a plain recorder hears the same microphone: what the call would hear without it
    let mut plain = r.connect("system.mesh", capture(16_000, 1)).unwrap();
    let (mut with, mut without) = (Vec::new(), Vec::new());
    let (mut seed, mut far) = (5u64, 0.0f32);
    let (mut src, mut done, mut mic_at) = (Vec::<f32>::new(), 0usize, 0usize);
    for _ in 0..1800 {
        call.push(&far_chunk(&mut seed, &mut far)).unwrap();
        r.step();
        let played = r.played();
        src.extend(
            played[done * 2..]
                .chunks_exact(2)
                .map(|f| (f32::from(f[0]) + f32::from(f[1])) / 65536.0),
        );
        done = played.len() / 2;
        r.mic(&room_mic(&src, &mut mic_at));
        call.pull(&mut with).unwrap();
        plain.pull(&mut without).unwrap();
    }
    let n = with.len().min(without.len());
    let (w, wo) = (
        rms(&volts(&with[n - 16_000..n])),
        rms(&volts(&without[n - 16_000..n])),
    );
    let erle = 20.0 * (wo / w.max(1e-9)).log10();
    println!(
        "ERLE through the engine after 9 s: {erle:.1} dB (echo {:.1} dBFS)",
        20.0 * wo.log10()
    );
    assert!(wo > 0.01, "there is an echo to remove");
    assert!(erle > 20.0, "ERLE {erle} dB");
}

#[test]
fn a_stream_that_runs_dry_gets_a_bigger_cushion_and_never_clicks() {
    let mut r = Rig::new("dry", &[("system.web", play_rights())]);
    let mut s = r.connect("system.web", media()).unwrap();
    // a client that sends 20 ms every 40 ms: it runs dry again and again
    for i in 0..400 {
        if i % 8 == 0 {
            s.push(&level(960, 0.3)).unwrap();
        }
        r.step();
    }
    let played = r.played();
    let left: Vec<f32> = played
        .chunks_exact(2)
        .map(|f| f32::from(f[0]) / 32768.0)
        .collect();
    let jump = left
        .windows(2)
        .fold(0.0f32, |a, w| a.max((w[1] - w[0]).abs()));
    println!("largest step between samples with an irregular client: {jump:.4}");
    assert!(jump < 0.3 / 96.0 * 1.3, "no click: {jump}");
}
