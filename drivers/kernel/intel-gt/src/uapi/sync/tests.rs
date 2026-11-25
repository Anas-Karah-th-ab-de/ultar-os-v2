//! Tests of the sync objects against a GPU that only keeps books.

use super::*;
use crate::uapi::file::tests::{bind_arg, create, exec_arg, queue, vm, Fake, Mem};
use alloc::vec;

fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn put64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

/// Memory with one sync-object entry at 0x7000: object `handle`, timeline point `point`.
fn obj_sync(handle: u32, point: u64, signal: bool) -> Mem {
    let mut sync = vec![0u8; 48];
    put32(&mut sync, 8, (point != 0) as u32);
    put32(&mut sync, 12, signal as u32);
    put64(&mut sync, 16, handle as u64);
    put64(&mut sync, 24, point);
    Mem(vec![(0x7000, sync), (0x6000, vec![0; 64])])
}

fn syncobj(f: &mut File<Fake>, signaled: bool) -> u32 {
    let mut arg = [0u8; 8];
    arg[4] = signaled as u8;
    f.syncobj_create(&mut arg).unwrap();
    u32_at(&arg, 0)
}

/// Waits for the binary point of `handle` until the absolute time `until`.
fn wait_obj(
    f: &mut File<Fake>,
    be: &mut Fake,
    handle: u32,
    flags: u32,
    until: i64,
) -> Result<(), Errno> {
    let mut mem = Mem(vec![(0x6000, handle.to_le_bytes().to_vec())]);
    let mut arg = [0u8; 40];
    put64(&mut arg, 0, 0x6000);
    put64(&mut arg, 8, until as u64);
    put32(&mut arg, 16, 1);
    put32(&mut arg, 20, flags);
    f.syncobj_wait(be, &mut arg, &mut mem, false)
}

#[test]
fn a_sync_object_carries_the_end_of_a_job_to_a_waiter_and_to_the_next_job() {
    let (mut f, mut be) = (File::<Fake>::default(), Fake::default());
    let v = vm(&mut f, &mut be);
    let q = queue(&mut f, &mut be, v, 4).unwrap();
    let s = syncobj(&mut f, false);
    // nothing was hung on it: waiting is an error unless the waiter says it will come
    assert_eq!(wait_obj(&mut f, &mut be, s, 0, 1_000), Err(Errno::Inval));
    be.now = 10;
    assert_eq!(wait_obj(&mut f, &mut be, s, 2, 1_000), Err(Errno::Again));
    // the first job signals it
    let mut mem = obj_sync(s, 0, true);
    f.exec(&mut be, &exec_arg(q, 0x1000, 0x7000, 1), &mut mem)
        .unwrap();
    assert_eq!(wait_obj(&mut f, &mut be, s, 0, 1_000), Err(Errno::Again));
    // a second job waits for it: its exec asks to be repeated and does not run
    let mut waits = obj_sync(s, 0, false);
    assert_eq!(
        f.exec(&mut be, &exec_arg(q, 0x2000, 0x7000, 1), &mut waits),
        Err(Errno::Again)
    );
    assert_eq!(be.runs.len(), 1);
    be.now = 2_000;
    assert_eq!(wait_obj(&mut f, &mut be, s, 0, 1_000), Err(Errno::Time));
    be.done = 1;
    assert_eq!(wait_obj(&mut f, &mut be, s, 0, 1_000), Ok(()));
    f.exec(&mut be, &exec_arg(q, 0x2000, 0x7000, 1), &mut waits)
        .unwrap();
    assert_eq!(be.runs.len(), 2);
    // reset: nothing hangs on it any more
    let mut m = Mem(vec![(0x6000, s.to_le_bytes().to_vec())]);
    let mut arr = [0u8; 16];
    put64(&mut arr, 0, 0x6000);
    put32(&mut arr, 8, 1);
    f.syncobj_set(&arr, &mut m, None).unwrap();
    assert_eq!(wait_obj(&mut f, &mut be, s, 0, 0), Err(Errno::Inval));
    f.syncobj_set(&arr, &mut m, Some(Point::Signaled)).unwrap();
    assert_eq!(wait_obj(&mut f, &mut be, s, 0, 0), Ok(()));
    let mut destroy = [0u8; 8];
    put32(&mut destroy, 0, s);
    f.syncobj_destroy(&destroy).unwrap();
    assert_eq!(f.syncobj_destroy(&destroy), Err(Errno::Inval));
    assert_eq!(wait_obj(&mut f, &mut be, s, 0, 0), Err(Errno::NoEnt));
}

#[test]
fn timeline_points_are_done_in_order_and_can_be_queried_and_transferred() {
    let (mut f, mut be) = (File::<Fake>::default(), Fake::default());
    let v = vm(&mut f, &mut be);
    let q = queue(&mut f, &mut be, v, 0).unwrap();
    let (t, b) = (syncobj(&mut f, false), syncobj(&mut f, false));
    // job 1 is point 5, job 2 is point 9
    for point in [5u64, 9] {
        let mut mem = obj_sync(t, point, true);
        f.exec(&mut be, &exec_arg(q, 0x1000, 0x7000, 1), &mut mem)
            .unwrap();
    }
    let wait = |f: &mut File<Fake>, be: &mut Fake, point: u64| {
        let mut mem = Mem(vec![
            (0x6000, t.to_le_bytes().to_vec()),
            (0x6100, point.to_le_bytes().to_vec()),
        ]);
        let mut arg = [0u8; 48];
        put64(&mut arg, 0, 0x6000);
        put64(&mut arg, 8, 0x6100);
        put32(&mut arg, 24, 1);
        f.syncobj_wait(be, &mut arg, &mut mem, true)
    };
    assert_eq!(
        wait(&mut f, &mut be, 3),
        Err(Errno::Time),
        "point 3 ends with job 1"
    );
    be.done = 1;
    assert_eq!(
        (wait(&mut f, &mut be, 3), wait(&mut f, &mut be, 5)),
        (Ok(()), Ok(()))
    );
    assert_eq!(wait(&mut f, &mut be, 6), Err(Errno::Time));
    assert_eq!(
        wait(&mut f, &mut be, 10),
        Err(Errno::Inval),
        "no such point yet"
    );
    // query: the last finished point
    let mut mem = Mem(vec![
        (0x6000, t.to_le_bytes().to_vec()),
        (0x6100, vec![0; 8]),
    ]);
    let mut arg = [0u8; 24];
    put64(&mut arg, 0, 0x6000);
    put64(&mut arg, 8, 0x6100);
    put32(&mut arg, 16, 1);
    f.syncobj_query(&mut be, &arg, &mut mem).unwrap();
    assert_eq!(u64_at(&mem.0[1].1, 0), 5);
    // point 9 of the timeline becomes the binary point of another object
    let mut tr = [0u8; 32];
    put32(&mut tr, 0, t);
    put32(&mut tr, 4, b);
    put64(&mut tr, 8, 9);
    f.syncobj_transfer(&tr).unwrap();
    assert_eq!(wait_obj(&mut f, &mut be, b, 0, 0), Err(Errno::Time));
    be.done = 2;
    assert_eq!(wait_obj(&mut f, &mut be, b, 0, 0), Ok(()));
}

#[test]
fn binds_signal_their_sync_objects_at_once_and_bad_syncs_are_refused() {
    let (mut f, mut be) = (File::<Fake>::default(), Fake::default());
    let h = create(&mut f, &mut be, 4096).unwrap();
    let v = vm(&mut f, &mut be);
    let s = syncobj(&mut f, false);
    let mut arg = bind_arg(v, h, 0, 4096, 0x10000, 0);
    put32(&mut arg, 108, 1);
    put64(&mut arg, 112, 0x7000);
    f.vm_bind(&mut be, &arg, &mut obj_sync(s, 0, true)).unwrap();
    assert_eq!(wait_obj(&mut f, &mut be, s, 0, 0), Ok(()));
    // an object that does not exist; a binary entry with a point; a timeline signal without one
    assert_eq!(
        f.vm_bind(&mut be, &arg, &mut obj_sync(999, 0, true)),
        Err(Errno::NoEnt)
    );
    let mut bad = obj_sync(s, 0, true);
    put64(&mut bad.0[0].1, 24, 4);
    assert_eq!(f.vm_bind(&mut be, &arg, &mut bad), Err(Errno::Inval));
    let mut bad = obj_sync(s, 0, true);
    put32(&mut bad.0[0].1, 8, 1);
    assert_eq!(f.vm_bind(&mut be, &arg, &mut bad), Err(Errno::Inval));
}

#[test]
fn a_timeline_point_can_be_signaled_by_hand() {
    let (mut f, mut be) = (File::<Fake>::default(), Fake::default());
    let t = syncobj(&mut f, false);
    let mut mem = Mem(vec![
        (0x6000, t.to_le_bytes().to_vec()),
        (0x6100, 20u64.to_le_bytes().to_vec()),
    ]);
    let mut arg = [0u8; 24];
    put64(&mut arg, 0, 0x6000);
    put64(&mut arg, 8, 0x6100);
    put32(&mut arg, 16, 1);
    f.syncobj_timeline_signal(&arg, &mut mem).unwrap();
    // a waiter for point 15 is satisfied by the later point 20
    put64(&mut mem.0[1].1, 0, 15);
    let mut wait = [0u8; 48];
    put64(&mut wait, 0, 0x6000);
    put64(&mut wait, 8, 0x6100);
    put32(&mut wait, 24, 1);
    assert_eq!(f.syncobj_wait(&mut be, &mut wait, &mut mem, true), Ok(()));
}
