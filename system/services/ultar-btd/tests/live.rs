//! The stack thread on a scripted controller: a scan's devices reach the service's snapshot
//! while the scan runs (the service publishes `devices_revision` from it), one entry per
//! address with the latest signal.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_btd::runner::{Stack, Transport};
use ultar_btd::stack::hci::op;
use ultar_btd::stack::hid::NoInput;
use ultar_btd::stack::host::{Cmd, Config};

fn evt(code: u8, params: &[u8]) -> Vec<u8> {
    [&[0x04, code, params.len() as u8][..], params].concat()
}

/// Answers every command; on LE Set Scan Enable (on) and Inquiry it sends what the air
/// holds; `later` is sent a little at a time (one packet per read).
struct Script {
    inbox: Arc<Mutex<VecDeque<Vec<u8>>>>,
    later: Arc<Mutex<VecDeque<Vec<u8>>>>,
}

fn le_report(addr: [u8; 6], rssi: i8, name: &str) -> Vec<u8> {
    let mut data = vec![2, 0x01, 0x06, name.len() as u8 + 1, 0x09];
    data.extend_from_slice(name.as_bytes());
    let mut p = vec![0x02, 1, 0, 1];
    p.extend_from_slice(&addr);
    p.push(data.len() as u8);
    p.extend_from_slice(&data);
    p.push(rssi as u8);
    evt(0x3E, &p)
}

fn eir(addr: [u8; 6], rssi: i8, name: &str) -> Vec<u8> {
    let mut p = vec![1];
    p.extend_from_slice(&addr);
    p.extend_from_slice(&[
        1,
        0,
        0x40,
        0x25,
        0,
        0,
        0,
        rssi as u8,
        name.len() as u8 + 1,
        9,
    ]);
    p.extend_from_slice(name.as_bytes());
    p.resize(255, 0);
    evt(0x2F, &p)
}

impl Transport for Script {
    fn read(&mut self, timeout: Duration) -> std::io::Result<Option<Vec<u8>>> {
        if let Some(p) = self.inbox.lock().unwrap().pop_front() {
            return Ok(Some(p));
        }
        if let Some(p) = self.later.lock().unwrap().pop_front() {
            return Ok(Some(p));
        }
        std::thread::sleep(timeout.min(Duration::from_millis(5)));
        Ok(None)
    }

    fn write(&mut self, p: &[u8]) -> std::io::Result<()> {
        let mut inbox = self.inbox.lock().unwrap();
        let opcode = u16::from_le_bytes([p[1], p[2]]);
        match opcode {
            op::INQUIRY => {
                inbox.push_back(evt(0x0F, &[0, 1, p[1], p[2]]));
                let mut later = self.later.lock().unwrap();
                later.push_back(eir([0xC1, 0, 0, 0, 0, 0x10], -60, "K380"));
                later.push_back(eir([0xC1, 0, 0, 0, 0, 0x10], -52, "K380"));
            }
            op::READ_BD_ADDR => inbox.push_back(evt(0x0E, &[1, p[1], p[2], 0, 1, 2, 3, 4, 5, 6])),
            _ => {
                inbox.push_back(evt(0x0E, &[1, p[1], p[2], 0]));
                if opcode == op::LE_SET_SCAN_ENABLE && p[4] == 1 {
                    let mut later = self.later.lock().unwrap();
                    later.push_back(le_report([0xD1, 0, 0, 0, 0, 0xC0], -70, "Mouse"));
                    later.push_back(le_report([0xD1, 0, 0, 0, 0, 0xC0], -66, "Mouse"));
                }
            }
        }
        Ok(())
    }
}

fn until(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(Instant::now() < end, "{what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn found_devices_reach_the_snapshot_while_the_scan_runs() {
    let script = Script {
        inbox: Arc::default(),
        later: Arc::default(),
    };
    let dir = std::env::temp_dir().join(format!("btd-live-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let stack = Stack::spawn(
        Box::new(script),
        dir.join("bonds"),
        Box::new(NoInput),
        Config::default(),
    )
    .unwrap();
    until("ready", || stack.snapshot().ready);
    let before = stack.snapshot().revision;
    stack.command(Cmd::Scan(true)).unwrap();
    until("two devices", || stack.snapshot().devices.len() == 2);
    let snap = stack.snapshot();
    assert!(snap.scanning, "found while the scan runs");
    assert_ne!(snap.revision, before, "the list changed");
    until("latest signal", || {
        let s = stack.snapshot();
        let rssi: Vec<_> = s.devices.iter().map(|d| (d.name.clone(), d.rssi)).collect();
        rssi.contains(&("K380".into(), Some(-52))) && rssi.contains(&("Mouse".into(), Some(-66)))
    });
    let _ = std::fs::remove_dir_all(&dir);
}
