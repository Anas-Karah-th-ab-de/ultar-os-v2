//! Many clients at once, hostile input in bulk, and a micro-benchmark.

mod common;

use common::{client, eventually, open_broker, RawPeer, XorShift, WAIT};
use std::io::Write;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};
use ultar_bus::wire::{write_preamble, Envelope, Kind, PROTOCOL_VERSION};
use ultar_bus::{decode, encode, BrokerConfig, Client, ClientOptions, Policy};

const CLIENTS: usize = 50;
const EVENTS_PER_CLIENT: u32 = 10;
const CALLS_PER_CLIENT: u32 = 20;

fn spawn_echo(path: &std::path::Path) -> (Client, thread::JoinHandle<()>) {
    let options = ClientOptions {
        buffer: 4096,
        ..ClientOptions::named("echo")
    };
    let service = Client::connect(path, options).unwrap();
    let endpoint = service.register("echo").unwrap();
    let worker = thread::spawn(move || {
        endpoint.run(|request| {
            let value: u64 = request.decode_raw().unwrap_or(0);
            let _ = request.reply_raw(&value);
        });
    });
    (service, worker)
}

fn run_member(path: &std::path::Path, index: usize, ready: &Barrier, go: &Barrier) {
    let options = ClientOptions {
        buffer: 4096,
        ..ClientOptions::named("member")
    };
    let member = Client::connect(path, options).unwrap();
    let stream = member.subscribe_pattern("load.*").unwrap();
    ready.wait();
    go.wait();
    let topic = format!("load.c{index}");
    for sequence in 0..EVENTS_PER_CLIENT {
        member
            .publish_raw(&topic, 1, false, encode(&sequence).unwrap(), false)
            .unwrap();
        let token = (index as u64) << 32 | u64::from(sequence);
        assert_eq!(
            member
                .request::<_, u64>("echo", "same", &token, WAIT)
                .unwrap(),
            token
        );
    }
    for extra in EVENTS_PER_CLIENT..CALLS_PER_CLIENT {
        let token = u64::from(extra);
        assert_eq!(
            member
                .request::<_, u64>("echo", "same", &token, WAIT)
                .unwrap(),
            token
        );
    }
    // Every member must see every event of every member, in order per topic.
    let mut next = [0u32; CLIENTS];
    for _ in 0..CLIENTS as u32 * EVENTS_PER_CLIENT {
        let event = stream.recv_timeout(WAIT).unwrap();
        let sender: usize = event.topic["load.c".len()..].parse().unwrap();
        assert_eq!(
            decode::<u32>(&event.payload).unwrap(),
            next[sender],
            "topic {}",
            event.topic
        );
        next[sender] += 1;
    }
    assert!(next.iter().all(|count| *count == EVENTS_PER_CLIENT));
}

#[test]
fn fifty_concurrent_clients() {
    let (broker, path) = open_broker("fifty");
    let (service, worker) = spawn_echo(&path);
    let ready = Arc::new(Barrier::new(CLIENTS + 1));
    let go = Arc::new(Barrier::new(CLIENTS + 1));
    let members: Vec<_> = (0..CLIENTS)
        .map(|index| {
            let (path, ready, go) = (path.clone(), ready.clone(), go.clone());
            thread::spawn(move || run_member(&path, index, &ready, &go))
        })
        .collect();
    ready.wait();
    let stats = broker.stats();
    assert_eq!(
        (stats.connections, stats.subscriptions),
        (CLIENTS + 1, CLIENTS)
    );
    go.wait();
    for member in members {
        member.join().unwrap();
    }
    eventually("members gone", || broker.stats().connections == 1);
    let stats = broker.stats();
    assert_eq!((stats.dropped_slow, stats.pending_requests), (0, 0));
    service.close();
    worker.join().unwrap();
}

/// A valid frame with a few bytes damaged.
fn damaged_frame(random: &mut XorShift) -> Vec<u8> {
    let kinds = [
        Kind::Register,
        Kind::Request,
        Kind::Event,
        Kind::Subscribe,
        Kind::Response,
    ];
    let mut envelope = Envelope::new(kinds[random.below(5) as usize], random.next())
        .with_target("fuzz.topic")
        .with_payload(random.next() as u16, random.some_bytes(0, 64));
    envelope.method = "method".into();
    envelope.flags = random.next() as u8;
    let mut frame = envelope.to_frame(u32::MAX).unwrap();
    for _ in 0..=random.below(4) {
        let position = random.below(frame.len() as u64) as usize;
        frame[position] ^= 1 << random.below(8);
    }
    frame
}

fn fuzz_one_connection(path: &std::path::Path, random: &mut XorShift) {
    let mode = random.below(4);
    let mut peer = if mode == 0 {
        RawPeer::open(path)
    } else {
        RawPeer::connect(path)
    };
    if mode == 0 && random.below(2) == 0 {
        let _ = write_preamble(&mut peer.stream, PROTOCOL_VERSION);
    }
    for _ in 0..=random.below(6) {
        let bytes = match random.below(3) {
            0 => random.some_bytes(0, 200),
            1 => damaged_frame(random),
            _ => {
                // Correct header, random body.
                let body = random.some_bytes(1, 120);
                let mut frame = (body.len() as u32).to_le_bytes().to_vec();
                frame.extend_from_slice(&body);
                frame
            }
        };
        // The broker may already have hung up; that is a valid reaction.
        if peer.stream.write_all(&bytes).is_err() {
            break;
        }
    }
}

#[test]
fn malformed_input_in_bulk_never_hurts_the_broker() {
    let (broker, path) = open_broker("fuzz");
    let (service, worker) = spawn_echo(&path);
    let witness = client(&path, "witness");
    let stream = witness.subscribe_pattern("witness.*").unwrap();
    let mut random = XorShift(0x9E37_79B9_7F4A_7C15);
    for round in 0..400u64 {
        fuzz_one_connection(&path, &mut random);
        if round % 50 == 0 {
            assert_eq!(
                witness
                    .request::<_, u64>("echo", "same", &round, WAIT)
                    .unwrap(),
                round
            );
            witness
                .publish_raw("witness.alive", 1, false, vec![round as u8], true)
                .unwrap();
            let event = loop {
                // Fuzzed peers may legitimately publish to `fuzz.topic` only.
                let event = stream.recv_timeout(WAIT).unwrap();
                if event.topic == "witness.alive" {
                    break event;
                }
            };
            assert_eq!(event.payload, vec![round as u8]);
        }
    }
    eventually("fuzz peers cleaned up", || broker.stats().connections == 2);
    let stats = broker.stats();
    assert_eq!(stats.pending_requests, 0);
    assert_eq!(
        witness
            .request::<_, u64>("echo", "same", &7u64, WAIT)
            .unwrap(),
        7
    );
    service.close();
    worker.join().unwrap();
}

fn percentile(sorted: &[Duration], percent: usize) -> Duration {
    sorted[(sorted.len() * percent / 100).min(sorted.len() - 1)]
}

fn bench_calls(caller: &Client, rounds: usize) {
    let mut samples = Vec::with_capacity(rounds);
    let started = Instant::now();
    for round in 0..rounds as u64 {
        let before = Instant::now();
        assert_eq!(
            caller
                .request::<_, u64>("echo", "same", &round, WAIT)
                .unwrap(),
            round
        );
        samples.push(before.elapsed());
    }
    let total = started.elapsed();
    samples.sort();
    println!(
        "request/response: {rounds} round trips, mean {:?}, p50 {:?}, p99 {:?}, {:.0} calls/s",
        total / rounds as u32,
        percentile(&samples, 50),
        percentile(&samples, 99),
        rounds as f64 / total.as_secs_f64()
    );
}

fn bench_events(path: &std::path::Path, events: usize) {
    let options = ClientOptions {
        buffer: events,
        ..ClientOptions::named("sink")
    };
    let sink = Client::connect(path, options).unwrap();
    let stream = sink.subscribe_pattern("bench.topic").unwrap();
    let publisher = client(path, "source");
    let payload = vec![7u8; 64];
    let started = Instant::now();
    let receiver = thread::spawn(move || {
        for _ in 0..events {
            stream.recv_timeout(WAIT).unwrap();
        }
    });
    for sent in 0..events {
        // Confirm every 256th event to bound what is in flight.
        let confirm = sent % 256 == 255;
        publisher
            .publish_raw("bench.topic", 1, false, payload.clone(), confirm)
            .unwrap();
    }
    receiver.join().unwrap();
    let total = started.elapsed();
    println!(
        "publish/subscribe: {events} events of 64 bytes in {total:?}, {:.0} messages/s",
        events as f64 / total.as_secs_f64()
    );
}

/// Run with `cargo test --release --test load -- --ignored --nocapture`.
#[test]
#[ignore = "micro-benchmark, prints numbers instead of asserting"]
fn bench_round_trip_and_throughput() {
    // Queue large enough for the whole burst: the benchmark measures the
    // transport, not the slow-consumer protection.
    let config = BrokerConfig {
        queue_capacity: 250_000,
        ..BrokerConfig::default()
    };
    let (_broker, path) = common::start("bench", Policy::allow_all(), config);
    let (service, worker) = spawn_echo(&path);
    let caller = client(&path, "caller");
    bench_calls(&caller, 20_000);
    bench_events(&path, 200_000);
    service.close();
    worker.join().unwrap();
}
