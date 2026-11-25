//! Tests of the command buffers against a simulated GuC.

use super::*;
use crate::regs::vf_sw_flag;
use crate::sim::Sim;
use alloc::boxed::Box;
use alloc::vec;

/// The shared buffer as plain memory.
struct Mem(Vec<u32>);

impl Mem {
    fn new() -> Mem {
        Mem(vec![0xdead_beef; SIZE_BYTES as usize / 4])
    }
    /// The GuC appends a message to the G2H ring.
    fn guc_says(&mut self, fence: u16, hxg: &[u32]) {
        let mut tail = self.0[G2H_DESC + DESC_TAIL];
        let mut put = |m: &mut Vec<u32>, v| {
            m[G2H_CMDS + tail as usize] = v;
            tail = (tail + 1) % G2H_WORDS;
        };
        put(&mut self.0, (fence as u32) << 16 | hxg.len() as u32);
        for w in hxg {
            put(&mut self.0, *w);
        }
        self.0[G2H_DESC + DESC_TAIL] = tail;
    }
}

impl Shared for Mem {
    fn read(&mut self, index: usize) -> u32 {
        self.0[index]
    }
    fn write(&mut self, index: usize, val: u32) {
        self.0[index] = val;
    }
}

const SUCCESS: u32 = 1 << 31 | 7 << 28;

/// A GuC that accepts every register request (answers "1 key taken").
fn guc() -> Sim {
    let mut s = Sim::new();
    s.on_write(
        GUC_HOST_INTERRUPT,
        Box::new(|r, _, _| {
            r.insert(vf_sw_flag(0), SUCCESS | 1);
        }),
    );
    s
}

fn enabled() -> (Sim, Mem, Ct) {
    let (mut s, mut m) = (guc(), Mem::new());
    let ct = Ct::enable(&mut s, &mut m, 0x0100_0000).unwrap();
    s.writes.clear();
    (s, m, ct)
}

#[test]
fn layout() {
    assert_eq!(
        (G2H_DESC * 4, H2G_CMDS * 4, G2H_CMDS * 4),
        (2048, 4096, 20480)
    );
    assert_eq!(SIZE_BYTES, 148 << 10);
}

#[test]
fn enable_registers_both_rings_then_switches_on() {
    let (mut s, mut m) = (guc(), Mem::new());
    Ct::enable(&mut s, &mut m, 0x0100_0000).unwrap();
    assert!(m.0[..16].iter().chain(&m.0[512..528]).all(|w| *w == 0));
    // seven requests: six keys and the switch
    let first: Vec<(u32, u32)> = s.writes.chunks(5).map(|c| (c[0].1, c[1].1)).collect();
    assert_eq!(
        first,
        [
            (0x0508, 0x0903_0002),
            (0x0508, 0x0902_0002),
            (0x0508, 0x0904_0001),
            (0x0508, 0x0906_0002),
            (0x0508, 0x0905_0002),
            (0x0508, 0x0907_0001),
            (0x4509, 1),
        ]
    );
    let value: Vec<u32> = s.writes.chunks(5).map(|c| c[2].1).collect();
    assert_eq!(
        value[..6],
        [
            0x0100_0000,
            0x0100_1000,
            0x4000,
            0x0100_0800,
            0x0100_5000,
            0x2_0000
        ]
    );
}

#[test]
fn enable_fails_when_a_key_is_not_taken() {
    let mut s = Sim::new();
    s.on_write(
        GUC_HOST_INTERRUPT,
        Box::new(|r, _, _| {
            r.insert(vf_sw_flag(0), SUCCESS);
        }),
    );
    let e = Ct::enable(&mut s, &mut Mem::new(), 0x0100_0000);
    assert_eq!(
        e,
        Err(Error::GucProtocol {
            request: 0x0903,
            reply: 0
        })
    );
    assert!(Ct::enable(&mut guc(), &mut Mem::new(), 0x123).is_err());
}

#[test]
fn send_writes_header_action_tail_and_notifies() {
    let (mut s, mut m, mut ct) = enabled();
    let fence = ct
        .send(&mut s, &mut m, &[0x550e, 0x0200_0000, 0, 1])
        .unwrap();
    assert_eq!(fence, 1);
    assert_eq!(
        m.0[H2G_CMDS..H2G_CMDS + 5],
        [1 << 16 | 4, 0x550e, 0x0200_0000, 0, 1]
    );
    assert_eq!(m.0[H2G_DESC + DESC_TAIL], 5);
    assert_eq!(s.writes, [(GUC_HOST_INTERRUPT, 0)]);
    assert_eq!(ct.send(&mut s, &mut m, &[0x1]).unwrap(), 2);
    assert_eq!(m.0[H2G_CMDS + 5..H2G_CMDS + 7], [2 << 16 | 1, 1]);
}

#[test]
fn send_wraps_with_filler_and_respects_the_reader() {
    let (mut s, mut m, mut ct) = enabled();
    // the GuC has read everything up to word 4090; our tail is there too
    ct.tail = 4090;
    m.0[H2G_DESC + DESC_HEAD] = 4090;
    ct.send(&mut s, &mut m, &[1, 2, 3, 4, 5, 6, 7]).unwrap();
    assert!(m.0[H2G_CMDS + 4090..H2G_CMDS + 4096]
        .iter()
        .all(|w| *w == 0));
    assert_eq!(m.0[H2G_CMDS], 1 << 16 | 7);
    assert_eq!(m.0[H2G_DESC + DESC_TAIL], 8);
    // reader stuck right behind the tail: no room
    let (mut s, mut m, mut ct) = enabled();
    ct.tail = 100;
    m.0[H2G_DESC + DESC_HEAD] = 103;
    assert_eq!(
        ct.send(&mut s, &mut m, &[1, 2]),
        Err(Error::Ctb("H2G ring full"))
    );
    assert!(s.writes.is_empty());
    assert!(ct.send(&mut s, &mut m, &[1]).is_ok());
}

#[test]
fn send_refuses_a_broken_ring_and_bad_requests() {
    let (mut s, mut m, mut ct) = enabled();
    assert!(ct.send(&mut s, &mut m, &[]).is_err());
    assert!(ct.send(&mut s, &mut m, &[1 << 31]).is_err());
    assert!(ct.send(&mut s, &mut m, &[0; 65]).is_err());
    m.0[H2G_DESC + DESC_STATUS] = 1;
    assert_eq!(
        ct.send(&mut s, &mut m, &[1]),
        Err(Error::Ctb("H2G ring reports an error"))
    );
    m.0[H2G_DESC + DESC_STATUS] = 0;
    m.0[H2G_DESC + DESC_HEAD] = 4096;
    assert_eq!(
        ct.send(&mut s, &mut m, &[1]),
        Err(Error::Ctb("H2G head out of range"))
    );
}

#[test]
fn recv_decodes_the_message_kinds() {
    let (_, mut m, mut ct) = enabled();
    assert_eq!(ct.recv(&mut m), Ok(None));
    m.guc_says(7, &[SUCCESS | 0x21, 0xaa]);
    m.guc_says(8, &[1 << 31 | 6 << 28 | 3 << 16 | 0x30]);
    m.guc_says(0, &[1 << 31 | 1 << 28 | 0x1002, 5, 6]);
    m.guc_says(9, &[1 << 31 | 5 << 28]);
    m.guc_says(9, &[1 << 31 | 3 << 28]);
    assert_eq!(
        ct.recv(&mut m),
        Ok(Some(G2h::Response {
            fence: 7,
            data0: 0x21,
            payload: vec![0xaa]
        }))
    );
    assert_eq!(m.0[G2H_DESC + DESC_HEAD], 3);
    assert_eq!(
        ct.recv(&mut m),
        Ok(Some(G2h::Failure {
            fence: 8,
            error: 0x30,
            hint: 3
        }))
    );
    assert_eq!(
        ct.recv(&mut m),
        Ok(Some(G2h::Event {
            action: 0x1002,
            payload: vec![5, 6]
        }))
    );
    assert_eq!(ct.recv(&mut m), Ok(Some(G2h::Retry { fence: 9 })));
    assert_eq!(ct.recv(&mut m), Ok(Some(G2h::Busy { fence: 9 })));
    assert_eq!(ct.recv(&mut m), Ok(None));
}

#[test]
fn recv_follows_the_ring_around_and_refuses_damage() {
    let (_, mut m, mut ct) = enabled();
    ct.head = G2H_WORDS - 2;
    m.0[G2H_DESC + DESC_TAIL] = G2H_WORDS - 2;
    m.guc_says(4, &[SUCCESS, 1, 2]);
    assert_eq!(
        ct.recv(&mut m),
        Ok(Some(G2h::Response {
            fence: 4,
            data0: 0,
            payload: vec![1, 2]
        }))
    );
    assert_eq!(ct.head, 2);
    // a length that runs past the tail
    m.guc_says(5, &[SUCCESS]);
    m.0[G2H_CMDS + 2] = 5 << 16 | 9;
    assert_eq!(ct.recv(&mut m), Err(Error::Ctb("G2H message cut off")));
    let (_, mut m, mut ct) = enabled();
    m.0[G2H_DESC + DESC_STATUS] = 4;
    assert_eq!(
        ct.recv(&mut m),
        Err(Error::Ctb("G2H ring reports an error"))
    );
    m.0[G2H_DESC + DESC_STATUS] = 0;
    m.0[G2H_DESC + DESC_TAIL] = G2H_WORDS;
    assert_eq!(ct.recv(&mut m), Err(Error::Ctb("G2H tail out of range")));
    let (_, mut m, mut ct) = enabled();
    m.guc_says(1, &[7 << 28]);
    assert_eq!(
        ct.recv(&mut m),
        Err(Error::Ctb("G2H message not from the GuC"))
    );
}

#[test]
fn request_matches_the_fence_and_collects_events() {
    let (mut s, mut m, mut ct) = enabled();
    // an event and a stale response wait already; the answer to fence 1 comes last
    m.guc_says(0, &[1 << 31 | 1 << 28 | 0x1002]);
    m.guc_says(1, &[SUCCESS | 3]);
    let (data0, events) = ct
        .request(&mut s, &mut m, &[0x550e, 0, 0, 1], 1000)
        .unwrap();
    assert_eq!(data0, 3);
    assert_eq!(
        events,
        [G2h::Event {
            action: 0x1002,
            payload: vec![]
        }]
    );
}

#[test]
fn request_failure_retry_and_timeout() {
    let (mut s, mut m, mut ct) = enabled();
    m.guc_says(1, &[1 << 31 | 6 << 28 | 0x44]);
    assert_eq!(
        ct.request(&mut s, &mut m, &[0x550e], 1000),
        Err(Error::GucRequest {
            request: 0x550e,
            error: 0x44,
            hint: 0
        })
    );
    // retry: the request is sent again with a new fence
    let (mut s, mut m, mut ct) = enabled();
    m.guc_says(1, &[1 << 31 | 5 << 28]);
    m.guc_says(2, &[SUCCESS | 9]);
    assert_eq!(ct.request(&mut s, &mut m, &[0x550e], 1000).unwrap().0, 9);
    assert_eq!(s.writes_to(GUC_HOST_INTERRUPT).len(), 2);
    // silence
    let (mut s, mut m, mut ct) = enabled();
    assert_eq!(
        ct.request(&mut s, &mut m, &[0x550e], 1000),
        Err(Error::GucProtocol {
            request: 0x550e,
            reply: 0
        })
    );
    assert!(s.now_us() >= 1000);
}

#[test]
fn post_marks_the_request_as_unanswered() {
    let (mut s, mut m, mut ct) = enabled();
    ct.post(&mut s, &mut m, &[0x1001, 7, 1]).unwrap();
    assert_eq!(
        m.0[H2G_CMDS..H2G_CMDS + 4],
        [1 << 16 | 3, 2 << 28 | 0x1001, 7, 1]
    );
    assert_eq!(s.writes, [(GUC_HOST_INTERRUPT, 0)]);
    assert!(ct.post(&mut s, &mut m, &[]).is_err());
    assert!(ct.post(&mut s, &mut m, &[1 << 28]).is_err());
}

#[test]
fn wait_event_finds_the_event_and_stops_at_a_failure() {
    const EVENT: u32 = 1 << 31 | 1 << 28;
    let (mut s, mut m, mut ct) = enabled();
    m.guc_says(0, &[EVENT | 0x8002, 9]);
    m.guc_says(0, &[EVENT | 0x1002, 7]);
    assert_eq!(ct.wait_event(&mut s, &mut m, 0x1002, 1000).unwrap(), [7]);
    // the event that came first is kept for the owner
    assert_eq!(ct.take_backlog(), [(0x8002, vec![9])]);
    assert!(ct.take_backlog().is_empty());
    // nothing more: the wait runs out
    assert!(matches!(
        ct.wait_event(&mut s, &mut m, 0x1002, 1000),
        Err(Error::GucProtocol {
            request: 0x1002,
            reply: 0
        })
    ));
    m.guc_says(3, &[1 << 31 | 6 << 28 | 0x30]);
    assert!(matches!(
        ct.wait_event(&mut s, &mut m, 0x1002, 1000),
        Err(Error::GucRequest { error: 0x30, .. })
    ));
}
