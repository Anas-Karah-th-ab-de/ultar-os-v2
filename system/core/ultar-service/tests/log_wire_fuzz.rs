//! Fuzz of the log wire format with a fixed-seed generator: whatever bytes
//! a (hostile) service sends, the decoder returns a typed result, never
//! panics, never holds more than one frame, and everything it accepts
//! satisfies the rules of the record types.

use ultar_service::log::record::{is_key, ShortId, MAX_FIELDS};
use ultar_service::log::wire::{decode_body, encode_body, FrameReader, MAX_BODY};
use ultar_service::log::{Level, OwnedRecord, OwnedValue, Record};

/// xorshift64*: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next() as u8).collect()
    }
}

const WORDS: [&str; 6] = ["publish", "a", "x-1", "limit_exceeded", "z.y.x", "ok"];
const IDS: [&str; 5] = ["app.phone", "pci:01:00.0", "wlan0", "a", "system.settingsd"];

fn random_record(rng: &mut Rng) -> Record {
    let level = Level::ALL[rng.below(Level::ALL.len())];
    let mut record = Record::new(level, WORDS[rng.below(WORDS.len())]);
    for _ in 0..rng.below(MAX_FIELDS + 1) {
        let name = WORDS[rng.below(WORDS.len())];
        record = match rng.below(5) {
            0 => record.count(name, rng.next()),
            1 => record.signed(name, rng.next() as i64),
            2 => record.flag(name, rng.next() & 1 == 1),
            3 => record.word(name, WORDS[rng.below(WORDS.len())]),
            _ => record.id(name, ShortId::new(IDS[rng.below(IDS.len())]).unwrap()),
        };
    }
    record
}

/// Everything the decoder lets through must obey the type rules.
fn assert_clean(record: &OwnedRecord) {
    assert!(is_key(&record.key), "{:?}", record.key);
    assert!(record.fields.len() <= MAX_FIELDS);
    for (name, value) in &record.fields {
        assert!(is_key(name), "{name:?}");
        match value {
            OwnedValue::Word(word) => assert!(is_key(word), "{word:?}"),
            OwnedValue::Id(id) => assert!(ShortId::is_valid(id), "{id:?}"),
            OwnedValue::Count(_) | OwnedValue::Signed(_) | OwnedValue::Flag(_) => {}
        }
    }
}

#[test]
fn random_bytes_never_panic_and_accepted_records_are_clean() {
    let mut rng = Rng(0x5EED_0001);
    let mut accepted = 0;
    for _ in 0..200_000 {
        let len = rng.below(96);
        let mut body = rng.bytes(len);
        // Half of the inputs get a plausible header so that the field
        // parser is reached often.
        if rng.next() & 1 == 0 && body.len() >= 2 {
            body[0] = 1;
            body[1] = (rng.next() % 7) as u8;
        }
        if let Ok(record) = decode_body(&body) {
            assert_clean(&record);
            accepted += 1;
        }
    }
    // Random bytes almost never form a record; the number is fixed by the
    // seed and only documents that the accepting path was reached at all.
    assert!(accepted < 2_000, "{accepted}");
}

#[test]
fn mutated_valid_records_are_refused_or_still_clean() {
    let mut rng = Rng(0x5EED_0002);
    let (mut refused, mut accepted) = (0u32, 0u32);
    for _ in 0..50_000 {
        let record = random_record(&mut rng);
        let mut body = encode_body(&record).unwrap();
        assert!(body.len() <= MAX_BODY);
        let original = decode_body(&body).unwrap();
        assert_clean(&original);
        assert_eq!(original.level, record.level());
        assert_eq!(original.key, record.key());
        assert_eq!(original.fields.len(), record.fields().len());
        match rng.below(4) {
            0 => {
                let index = rng.below(body.len());
                body[index] ^= 1 << rng.below(8);
            }
            1 => body.truncate(rng.below(body.len())),
            2 => {
                let extra = 1 + rng.below(8);
                body.extend(rng.bytes(extra));
            }
            _ => {
                let index = rng.below(body.len());
                body[index] = rng.next() as u8;
            }
        }
        match decode_body(&body) {
            Ok(mutated) => {
                assert_clean(&mutated);
                accepted += 1;
            }
            Err(_) => refused += 1,
        }
    }
    assert!(refused > 20_000, "refused {refused}, accepted {accepted}");
}

#[test]
fn a_hostile_stream_never_makes_the_reader_grow() {
    let mut rng = Rng(0x5EED_0003);
    for _ in 0..2_000 {
        let mut reader = FrameReader::new();
        let mut frames = 0u32;
        for _ in 0..64 {
            let len = 1 + rng.below(300);
            let chunk = rng.bytes(len);
            let result = reader.push(&chunk, |body| {
                assert!(!body.is_empty() && body.len() <= MAX_BODY);
                let _ = decode_body(body);
                frames += 1;
            });
            assert!(reader.buffered() <= MAX_BODY + 2);
            if result.is_err() {
                // The stream is dead; a real server closes the connection.
                assert_eq!(reader.buffered(), 0);
                break;
            }
        }
        let _ = frames;
    }
}

#[test]
fn valid_streams_survive_any_chunking() {
    let mut rng = Rng(0x5EED_0004);
    for _ in 0..500 {
        let records: Vec<Record> = (0..1 + rng.below(20))
            .map(|_| random_record(&mut rng))
            .collect();
        let mut stream = Vec::new();
        for record in &records {
            let body = encode_body(record).unwrap();
            stream.extend_from_slice(&(body.len() as u16).to_le_bytes());
            stream.extend_from_slice(&body);
        }
        let mut reader = FrameReader::new();
        let mut keys = Vec::new();
        let mut rest = stream.as_slice();
        while !rest.is_empty() {
            let take = (1 + rng.below(97)).min(rest.len());
            reader
                .push(&rest[..take], |body| {
                    keys.push(decode_body(body).unwrap().key)
                })
                .unwrap();
            rest = &rest[take..];
        }
        let expected: Vec<&str> = records.iter().map(Record::key).collect();
        assert_eq!(keys, expected);
    }
}
