//! Fixed-seed fuzzing of everything that reads bytes from outside: manifests, envelopes,
//! rotations, bundle headers, slot tables, state files, configuration – and whole bundles
//! through the engine. Deterministic: the generator is seeded with constants.

mod common;

use common::*;
use std::io::Read;
use ultar_update::bundle::{Header, MAX_HEADER};
use ultar_update::config::DaemonConfig;
use ultar_update::error::UpdateError;
use ultar_update::journal::Journal;
use ultar_update::keys::Rotation;
use ultar_update::manifest::{Manifest, MAX_MANIFEST};
use ultar_update::policy::Policy;
use ultar_update::sign::{self, public_of, KEYS_MAGIC, MANIFEST_MAGIC};
use ultar_update::slottable::SlotTable;
use ultar_update::verify::authenticate;

/// Random mutation of `data`: flip, replace, insert, delete, duplicate, truncate.
fn mutate(rng: &mut Rng, data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    for _ in 0..=rng.below(3) {
        if out.is_empty() {
            out.push(rng.next() as u8);
            continue;
        }
        let at = rng.below(out.len());
        match rng.below(7) {
            0 => out[at] ^= 1 << rng.below(8),
            1 => out[at] = rng.next() as u8,
            2 => out.insert(at, rng.next() as u8),
            3 => {
                out.remove(at);
            }
            4 => {
                let end = (at + rng.below(40)).min(out.len());
                let piece = out[at..end].to_vec();
                out.splice(at..at, piece);
            }
            5 => out.truncate(at),
            _ => {
                let interesting = [
                    0x00, 0xff, 0x7f, 0x80, b'\n', b'=', b',', b'/', b'.', b'-', b' ',
                ];
                out[at] = interesting[rng.below(interesting.len())];
            }
        }
    }
    out
}

#[test]
fn manifest_parser_accepts_only_canonical_bytes() {
    let good = release((0, 7, 1), 3).manifest.encode().unwrap();
    let mut rng = Rng(0x1111_2222_3333_4444);
    let mut accepted_other = 0;
    for _ in 0..40_000 {
        let candidate = mutate(&mut rng, &good);
        if let Ok(manifest) = Manifest::parse(&candidate) {
            // Whatever parses is canonical: it encodes to exactly the same bytes.
            assert_eq!(manifest.encode().unwrap(), candidate);
            assert!(candidate.len() <= MAX_MANIFEST);
            if candidate != good {
                accepted_other += 1;
            }
        }
    }
    // Mutations that happen to give another valid manifest exist (e.g. a changed digit) –
    // they are harmless because the signature covers the bytes – but they must be rare.
    assert!(accepted_other < 4000, "{accepted_other}");
    for _ in 0..5_000 {
        let len = rng.below(600);
        let _ = Manifest::parse(&rng.bytes(len));
    }
}

#[test]
fn hostile_manifest_fields_never_parse() {
    let good = String::from_utf8(release((0, 7, 1), 3).manifest.encode().unwrap()).unwrap();
    let nasty = [
        "../../etc/passwd",
        "/etc/shadow",
        "..",
        "a/../b",
        "a\\b",
        "a\0b",
        "%2e%2e",
        "~root",
        "${HOME}",
        "$(reboot)",
        "`id`",
        "a;b",
        "a|b",
        "a&b",
        "a b",
        "-",
        "*.*",
        "",
    ];
    for field in [
        "channel=stable",
        "edition=desktop",
        "arch=x86_64",
        "devices=pc-uefi",
        "notes_key=update.notes.test",
    ] {
        let (name, _) = field.split_once('=').unwrap();
        for value in nasty {
            let text = good.replace(field, &format!("{name}={value}"));
            assert!(
                Manifest::parse(text.as_bytes()).is_err(),
                "{name}={value:?}"
            );
        }
    }
    for size in [
        "18446744073709551615",
        "99999999999999999999999",
        "-1",
        "0x10",
        "1e9",
        "68719476737",
    ] {
        let text = good.replace(
            &format!("payload=kernel {KERNEL_LEN}"),
            &format!("payload=kernel {size}"),
        );
        assert!(Manifest::parse(text.as_bytes()).is_err(), "size {size}");
    }
}

#[test]
fn signed_envelopes_never_verify_after_any_mutation() {
    let release = release((0, 7, 1), 3);
    let keys = [public_of(&PUBLISHER)];
    authenticate(&release.signed, &keys).unwrap();
    let mut rng = Rng(0xAAAA_BBBB_CCCC_0001);
    for _ in 0..6_000 {
        let candidate = mutate(&mut rng, &release.signed);
        if candidate == release.signed {
            continue;
        }
        assert!(authenticate(&candidate, &keys).is_err());
        let _ = sign::open(&MANIFEST_MAGIC, &candidate, MAX_MANIFEST);
        let _ = sign::open(&KEYS_MAGIC, &candidate, 1024);
    }
    for _ in 0..3_000 {
        let len = rng.below(300);
        let noise = rng.bytes(len);
        assert!(authenticate(&noise, &keys).is_err());
        let _ = Rotation::parse(&noise);
    }
}

#[test]
fn rotation_parser_is_canonical_too() {
    let rotation = Rotation {
        set: ultar_update::keys::KeySet {
            serial: 4,
            keys: vec![[1; 32], [2; 32]],
        },
        created: 99,
    };
    let good = rotation.encode().unwrap();
    let mut rng = Rng(0x0BAD_5EED_0000_0002);
    for _ in 0..20_000 {
        let candidate = mutate(&mut rng, &good);
        if let Ok(parsed) = Rotation::parse(&candidate) {
            assert_eq!(parsed.encode().unwrap(), candidate);
        }
    }
}

/// A reader that never ends (a "bundle" that claims to be endless).
struct Endless(u8, u64);

impl Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        buf.fill(self.0);
        self.1 += buf.len() as u64;
        Ok(buf.len())
    }
}

#[test]
fn bundle_header_reader_is_bounded_whatever_the_lengths_say() {
    let good = release((0, 7, 1), 3).header.raw;
    let mut rng = Rng(0x5EED_0000_0000_0003);
    for _ in 0..20_000 {
        let mut candidate = mutate(&mut rng, &good);
        // Often aim at the two length fields directly.
        if rng.below(3) == 0 && candidate.len() > 8 {
            let value = [
                0u32,
                1,
                0xFFFF,
                0x1_0000,
                0x1_0001,
                u32::MAX,
                rng.next() as u32,
            ][rng.below(7)];
            candidate[4..8].copy_from_slice(&value.to_le_bytes());
        }
        let mut reader: &[u8] = &candidate;
        if let Ok(header) = Header::read(&mut reader) {
            assert!(header.raw.len() <= MAX_HEADER);
            assert_eq!(header.raw, candidate[..header.raw.len()]);
            assert!(header.rotations.len() <= 64);
        }
    }
    // Endless input: the reader stops by itself after a bounded number of bytes.
    for byte in [0u8, 0xff, b'U'] {
        let mut endless = Endless(byte, 0);
        assert!(Header::read(&mut endless).is_err());
        assert!(endless.1 <= MAX_HEADER as u64, "read {} bytes", endless.1);
    }
    let mut endless = std::io::Read::chain(&b"UUP1\xff\xff\xff\x7f"[..], Endless(0, 0));
    assert!(matches!(
        Header::read(&mut endless),
        Err(UpdateError::Malformed(_))
    ));
    // Valid magic and a plausible section length, then endless garbage.
    let mut endless = std::io::Read::chain(&b"UUP1\x00\x00\x01\x00"[..], Endless(b'U', 0));
    assert!(Header::read(&mut endless).is_err());
}

/// A small valid bundle (keeps hundreds of engine runs fast).
fn small_release() -> Release {
    let good = release_with((0, 7, 1), 3, &PUBLISHER, |_| {});
    let mut manifest = good.manifest.clone();
    let payloads = vec![vec![7u8; 300], vec![9u8; 500]];
    manifest.payloads[0].size = 300;
    manifest.payloads[0].sha256 = sha(&payloads[0]);
    manifest.payloads[1].size = 500;
    manifest.payloads[1].sha256 = sha(&payloads[1]);
    assemble(manifest, payloads, &[], &PUBLISHER)
}

type TestEngine = ultar_update::engine::Engine<ultar_update::filectl::FileBootControl>;

fn candidate_file(device: &Device) -> std::path::PathBuf {
    let path = device.dir.path().join("usb");
    std::fs::create_dir_all(&path).unwrap();
    path.join("candidate.uup")
}

#[test]
fn mutated_bundles_never_reach_a_slot() {
    // Structure-aware: every mutation of a valid bundle goes through the complete offline
    // path of the engine (the same code as a download). Nothing may be installed.
    let device = Device::new();
    let small = small_release();
    let mut engine = device.engine();
    let mut rng = Rng(0xF00D_F00D_0000_0004);
    let file = candidate_file(&device);
    let mut rejected = std::collections::BTreeMap::new();
    for round in 0..600 {
        let candidate = mutate(&mut rng, &small.bundle);
        if candidate == small.bundle {
            continue;
        }
        std::fs::write(&file, &candidate).unwrap();
        // Each round starts with an empty staging area. (Without this, a truncated candidate
        // can leave a correct prefix behind which a later candidate – differing only inside
        // that prefix – completes to the genuine bundle. That is sound: what gets installed
        // is byte for byte the signed content. But here every candidate is judged alone.)
        engine.cancel_pending().unwrap();
        let error = engine
            .stage_offline(&file, true, &mut |_, _| {})
            .and_then(|_| engine.apply(&mut |_, _, _| {}).map(|_| ()))
            .expect_err("a mutated bundle was installed");
        *rejected.entry(error.key()).or_insert(0u32) += 1;
        assert!(device.slot_b_untouched(), "round {round}: {error}");
    }
    // The mutations reached the different defences, not only the first one.
    for key in [
        "update.error.bad_signature",
        "update.error.hash_mismatch",
        "update.error.malformed",
    ] {
        assert!(rejected.contains_key(key), "{key} never hit: {rejected:?}");
    }
    genuine_content_still_installs(&device, &mut engine, &small);
}

/// What follows the mutation rounds on the same device: only the signed content installs.
fn genuine_content_still_installs(device: &Device, engine: &mut TestEngine, small: &Release) {
    let file = candidate_file(device);
    // A correct prefix left by one source and the rest from another give the signed bundle –
    // and only then is anything installed, and it is exactly the signed content.
    engine.cancel_pending().unwrap();
    let cut = small.header.raw.len() + 400;
    std::fs::write(&file, &small.bundle[..cut]).unwrap();
    assert!(matches!(
        engine.stage_offline(&file, false, &mut |_, _| {}),
        Err(UpdateError::Truncated)
    ));
    let mut other = small.bundle.clone();
    other[small.header.raw.len() + 5] ^= 0xFF; // differs inside the part already fetched
    std::fs::write(&file, &other).unwrap();
    engine.stage_offline(&file, false, &mut |_, _| {}).unwrap();
    engine.apply(&mut |_, _, _| {}).unwrap();
    assert_eq!(
        device.images(ultar_update::bootctl::Slot::B),
        small.payloads
    );
    engine.cancel_pending().unwrap();
    // And the untouched original still installs afterwards.
    std::fs::write(&file, &small.bundle).unwrap();
    engine.stage_offline(&file, false, &mut |_, _| {}).unwrap();
    engine.apply(&mut |_, _, _| {}).unwrap();
}

#[test]
fn slot_table_and_state_files_survive_garbage() {
    let mut rng = Rng(0x0000_7AB1_E000_0005);
    let good = SlotTable::factory().encode();
    for _ in 0..50_000 {
        let candidate = mutate(&mut rng, &good);
        if let Ok(table) = SlotTable::decode(&candidate) {
            assert_eq!(table.encode().as_slice(), candidate.as_slice());
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    let seeds: [&[u8]; 4] = [
        b"trial b 0.7.0\n",
        b"automatic=1\nwindow=60-120\nchannel=stable\nallow_metered=0\nrate_limit_kib=0\n",
        b"1796000000 failed 0.7.1 update.error.expired\n",
        b"5 0000000000000000000000000000000000000000000000000000000000000000\n",
    ];
    for _ in 0..3_000 {
        let seed = seeds[rng.below(seeds.len())];
        std::fs::write(&path, mutate(&mut rng, seed)).unwrap();
        let _ = Journal::load(&path).unwrap();
        let (policy, _) = Policy::load(&path);
        assert!(policy.validate().is_ok());
        assert!(ultar_update::history::load(&path).len() <= 200);
    }
}

#[test]
fn damaged_state_directory_never_panics_and_never_installs_blindly() {
    // Random damage to random state files between operations of a real update cycle.
    let mut rng = Rng(0xD15C_0000_0000_0006);
    for round in 0..40 {
        let device = Device::new();
        let new = release((0, 7, 1), 2);
        let _ = install(&mut device.engine(), &new);
        let state = device.config.state_dir.clone();
        let mut files = Vec::new();
        for dir in ["", "state", "slots", "staging", "trust"] {
            if let Ok(entries) = std::fs::read_dir(state.join(dir)) {
                files.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_file()));
            }
        }
        files.sort();
        for _ in 0..=rng.below(3) {
            let victim = &files[rng.below(files.len())];
            let content = std::fs::read(victim).unwrap_or_default();
            let cut = content.len().min(4096);
            std::fs::write(victim, mutate(&mut rng, &content[..cut])).unwrap();
        }
        // Whatever was damaged: opening, resuming and a new attempt return errors or work –
        // and the device still starts a complete system.
        if let Ok(mut engine) = device.engine_with(std::sync::Arc::new(
            ultar_update::fault::InterruptAfter::never(),
        )) {
            let _ = engine.resume_after_boot(&Health(true));
            let _ = engine.status();
            let _ = install(&mut engine, &new);
        }
        let (_, which) = device.reboot(&[&new]);
        assert!(which <= 1, "round {round}");
    }
}

#[test]
fn configuration_parser_survives_garbage() {
    let good = "device=pc\narch=x86_64\nedition=desktop\nversion=0.7.0\nroles=kernel,system\nboot_dir=/b\nstate_dir=/s\nkeys_dir=/k\nadmin=system.settings\n";
    let mut rng = Rng(0xC0F1_6000_0000_0007);
    for _ in 0..20_000 {
        let candidate = mutate(&mut rng, good.as_bytes());
        if let Ok(text) = String::from_utf8(candidate) {
            if let Ok(config) = DaemonConfig::parse(&text) {
                assert!(config.engine.trial_tries > 0);
                assert!(!config.roles.is_empty());
            }
        }
    }
}
