//! Tests of the per-file requests against a GPU that only keeps books.

use super::*;
use alloc::rc::Rc;
use alloc::vec;

/// A GPU that records what it is asked to do.
#[derive(Default)]
pub struct Fake {
    pub maps: Vec<(u32, u64, Vec<u64>)>,
    pub unmaps: Vec<(u32, u64, u64)>,
    pub vms: u32,
    pub freed_vms: Vec<u32>,
    pub null_maps: Vec<(u32, u64, u64)>,
    pub queues: Vec<(u32, EngineClass)>,
    pub freed_queues: Vec<u32>,
    pub runs: Vec<(u32, u64)>,
    pub done: u64,
    pub banned: bool,
    pub now: u64,
    next_page: u64,
}

impl Backend for Fake {
    type Buffer = Rc<Vec<u64>>;
    fn buffer_new(&mut self, size: u64) -> Result<Self::Buffer, Errno> {
        let n = size / 4096;
        let first = 0x10_0000 + self.next_page * 4096;
        self.next_page += n;
        Ok(Rc::new((0..n).map(|i| first + i * 4096).collect()))
    }
    fn buffer_pages(&self, b: &Self::Buffer) -> Vec<u64> {
        b.to_vec()
    }
    fn vm_new(&mut self) -> Result<u32, Errno> {
        self.vms += 1;
        Ok(100 + self.vms)
    }
    fn vm_map(&mut self, vm: u32, va: u64, pages: &[u64], _pat: u8) -> Result<(), Errno> {
        self.maps.push((vm, va, pages.to_vec()));
        Ok(())
    }
    fn vm_map_null(&mut self, vm: u32, va: u64, pages: u64) -> Result<(), Errno> {
        self.null_maps.push((vm, va, pages));
        Ok(())
    }
    fn vm_unmap(&mut self, vm: u32, va: u64, pages: u64) -> Result<(), Errno> {
        self.unmaps.push((vm, va, pages));
        Ok(())
    }
    fn vm_free(&mut self, vm: u32) {
        self.freed_vms.push(vm);
    }
    fn queue_new(&mut self, vm: u32, class: EngineClass) -> Result<u32, Errno> {
        self.queues.push((vm, class));
        Ok(200 + self.queues.len() as u32)
    }
    fn queue_run(&mut self, queue: u32, batch_va: u64) -> Result<u64, Errno> {
        self.runs.push((queue, batch_va));
        Ok(self.runs.len() as u64)
    }
    fn queue_done(&mut self, _queue: u32) -> u64 {
        if self.banned {
            u64::MAX
        } else {
            self.done
        }
    }
    fn queue_banned(&mut self, _queue: u32) -> bool {
        self.banned
    }
    fn queue_free(&mut self, queue: u32) {
        self.freed_queues.push(queue);
    }
    fn now_ns(&mut self) -> u64 {
        self.now
    }
}

/// A program's memory: (address, bytes) regions.
#[derive(Default)]
pub struct Mem(pub Vec<(u64, Vec<u8>)>);

impl User for Mem {
    fn write(&mut self, addr: u64, data: &[u8]) -> bool {
        for (a, b) in &mut self.0 {
            if addr >= *a && addr + data.len() as u64 <= *a + b.len() as u64 {
                let at = (addr - *a) as usize;
                b[at..at + data.len()].copy_from_slice(data);
                return true;
            }
        }
        false
    }
    fn read(&mut self, addr: u64, len: usize) -> Option<Vec<u8>> {
        self.0.iter().find_map(|(a, b)| {
            let inside = addr >= *a && addr + len as u64 <= *a + b.len() as u64;
            inside.then(|| b[(addr - *a) as usize..(addr - *a) as usize + len].to_vec())
        })
    }
}

pub fn create(f: &mut File<Fake>, be: &mut Fake, size: u64) -> Result<u32, Errno> {
    let mut arg = [0u8; 56];
    put64(&mut arg, 8, size);
    put32(&mut arg, 16, 1);
    arg[32] = 1;
    f.gem_create(be, &mut arg).map(|_| u32_at(&arg, 28))
}

pub fn vm(f: &mut File<Fake>, be: &mut Fake) -> u32 {
    let mut arg = [0u8; 32];
    f.vm_create(be, &mut arg).unwrap();
    u32_at(&arg, 12)
}

pub fn bind_arg(vm: u32, handle: u32, offset: u64, range: u64, addr: u64, op: u32) -> [u8; 136] {
    let mut arg = [0u8; 136];
    put32(&mut arg, 8, vm);
    put32(&mut arg, 20, 1);
    put32(&mut arg, 24 + 8, handle);
    put64(&mut arg, 24 + 16, offset);
    put64(&mut arg, 24 + 24, range);
    put64(&mut arg, 24 + 32, addr);
    put32(&mut arg, 24 + 40, op);
    arg
}

pub fn queue(f: &mut File<Fake>, be: &mut Fake, vm: u32, class: u16) -> Result<u32, Errno> {
    let mut mem = Mem(vec![(0x9000, vec![0; 8])]);
    mem.0[0].1[0] = class as u8;
    let mut arg = [0u8; 48];
    arg[8] = 1;
    arg[10] = 1;
    put32(&mut arg, 12, vm);
    put64(&mut arg, 24, 0x9000);
    f.queue_create(be, &mut arg, &mut mem)
        .map(|_| u32_at(&arg, 20))
}

pub fn exec_arg(queue: u32, batch: u64, syncs: u64, n: u32) -> [u8; 56] {
    let mut arg = [0u8; 56];
    put32(&mut arg, 8, queue);
    put32(&mut arg, 12, n);
    put64(&mut arg, 16, syncs);
    put64(&mut arg, 24, batch);
    arg[32] = 1;
    arg
}

/// Memory with one user-fence sync entry at 0x7000 that stores `value` at 0x8000.
pub fn fence_mem(value: u64) -> Mem {
    let mut sync = vec![0u8; 48];
    put32(&mut sync, 8, 2);
    put32(&mut sync, 12, 1);
    put64(&mut sync, 16, 0x8000);
    put64(&mut sync, 24, value);
    Mem(vec![(0x7000, sync), (0x8000, vec![0; 8])])
}

#[test]
fn buffers_get_handles_offsets_and_are_closed() {
    let (mut f, mut be) = (File::<Fake>::default(), Fake::default());
    let a = create(&mut f, &mut be, 8192).unwrap();
    let b = create(&mut f, &mut be, 4096).unwrap();
    assert_ne!(a, b);
    let mut arg = [0u8; 40];
    put32(&mut arg, 8, a);
    f.gem_mmap_offset(&mut arg).unwrap();
    let off = u64_at(&arg, 16);
    assert_eq!(off % 4096, 0);
    let (buf, size) = f.buffer_at(off).unwrap();
    assert_eq!((buf.len(), size), (2, 8192));
    assert!(f.buffer_at(off + 4096).is_none());
    let mut close = [0u8; 8];
    put32(&mut close, 0, a);
    f.gem_close(&close).unwrap();
    assert_eq!(f.gem_close(&close), Err(Errno::Inval));
    assert!(f.buffer_at(off).is_none());
    assert_eq!(f.gem_mmap_offset(&mut arg), Err(Errno::NoEnt));
}

#[test]
fn bad_buffer_requests_are_refused() {
    let (mut f, mut be) = (File::<Fake>::default(), Fake::default());
    assert_eq!(create(&mut f, &mut be, 0), Err(Errno::Inval));
    assert_eq!(create(&mut f, &mut be, 100), Err(Errno::Inval));
    assert_eq!(create(&mut f, &mut be, 8 << 30), Err(Errno::Inval));
    let mut arg = [0u8; 56];
    put64(&mut arg, 8, 4096);
    put32(&mut arg, 16, 2); // a region that does not exist
    arg[32] = 1;
    assert_eq!(f.gem_create(&mut be, &mut arg), Err(Errno::Inval));
    put32(&mut arg, 16, 1);
    arg[32] = 0; // no caching mode
    assert_eq!(f.gem_create(&mut be, &mut arg), Err(Errno::Inval));
    arg[32] = 1;
    put32(&mut arg, 24, 77); // an address space that does not exist
    assert_eq!(f.gem_create(&mut be, &mut arg), Err(Errno::Inval));
    put32(&mut arg, 24, 0);
    arg[48] = 1; // reserved
    assert_eq!(f.gem_create(&mut be, &mut arg), Err(Errno::Inval));
}

#[test]
fn bind_maps_the_pages_and_unbind_removes_them() {
    let (mut f, mut be, mut mem) = (File::<Fake>::default(), Fake::default(), Mem::default());
    let h = create(&mut f, &mut be, 3 * 4096).unwrap();
    let v = vm(&mut f, &mut be);
    // the last two pages of the buffer at 1 GiB
    let arg = bind_arg(v, h, 4096, 8192, 1 << 30, 0);
    f.vm_bind(&mut be, &arg, &mut mem).unwrap();
    assert_eq!(be.maps, [(101, 1 << 30, vec![0x10_1000, 0x10_2000])]);
    // closing the handle keeps the bound memory alive
    let mut close = [0u8; 8];
    put32(&mut close, 0, h);
    f.gem_close(&close).unwrap();
    // removing part of a binding leaves the rest
    let part = bind_arg(v, 0, 0, 4096, 1 << 30, 1);
    f.vm_bind(&mut be, &part, &mut mem).unwrap();
    assert_eq!(be.unmaps, [(101, 1 << 30, 1)]);
    let whole = bind_arg(v, 0, 0, 1 << 20, 1 << 30, 1);
    f.vm_bind(&mut be, &whole, &mut mem).unwrap();
    assert_eq!(be.unmaps[1..], [(101, (1 << 30) + 4096, 1)]);
    // nothing is left: removing again touches nothing
    f.vm_bind(&mut be, &whole, &mut mem).unwrap();
    assert_eq!(be.unmaps.len(), 2);
}

#[test]
fn a_range_bound_to_nothing_is_split_by_what_is_bound_into_it() {
    let (mut f, mut be, mut mem) = (File::<Fake>::default(), Fake::default(), Mem::default());
    let v = vm(&mut f, &mut be);
    // what Mesa does first: 8 GiB of nothing at 17 GiB
    let (base, len) = (17u64 << 30, 8u64 << 30);
    let mut null = bind_arg(v, 0, 0, len, base, 0);
    put32(&mut null, 24 + 44, 1 << 2);
    f.vm_bind(&mut be, &null, &mut mem).unwrap();
    assert_eq!(be.null_maps, [(101, base, len / 4096)]);
    // a buffer in the middle takes its pages out of the null range
    let h = create(&mut f, &mut be, 8192).unwrap();
    f.vm_bind(&mut be, &bind_arg(v, h, 0, 8192, base + 0x10000, 0), &mut mem).unwrap();
    assert_eq!(be.unmaps, [(101, base + 0x10000, 2)]);
    assert_eq!(be.maps.len(), 1);
    // removing the whole range removes the three pieces: the buffer and both null parts
    f.vm_bind(&mut be, &bind_arg(v, 0, 0, len, base, 1), &mut mem).unwrap();
    let mut removed = be.unmaps[1..].to_vec();
    removed.sort();
    assert_eq!(removed, [(101, base, 16), (101, base + 0x10000, 2), (101, base + 0x12000, len / 4096 - 18)]);
    // a null binding names no buffer
    let mut bad = bind_arg(v, h, 0, 4096, 0x10000, 0);
    put32(&mut bad, 24 + 44, 1 << 2);
    assert_eq!(f.vm_bind(&mut be, &bad, &mut mem), Err(Errno::Inval));
}

#[test]
fn a_new_binding_replaces_the_one_it_covers_and_unmap_all_finds_by_handle() {
    let (mut f, mut be, mut mem) = (File::<Fake>::default(), Fake::default(), Mem::default());
    let (a, b) = (
        create(&mut f, &mut be, 4096).unwrap(),
        create(&mut f, &mut be, 8192).unwrap(),
    );
    let v = vm(&mut f, &mut be);
    f.vm_bind(&mut be, &bind_arg(v, a, 0, 4096, 0x10000, 0), &mut mem)
        .unwrap();
    f.vm_bind(&mut be, &bind_arg(v, b, 0, 8192, 0x10000, 0), &mut mem)
        .unwrap();
    assert_eq!(be.unmaps, [(101, 0x10000, 1)]);
    assert_eq!(be.maps.len(), 2);
    f.vm_bind(&mut be, &bind_arg(v, b, 0, 4096, 0x40000, 0), &mut mem)
        .unwrap();
    let mut all = bind_arg(v, b, 0, 0, 0, 3);
    put32(&mut all, 24 + 8, b);
    f.vm_bind(&mut be, &all, &mut mem).unwrap();
    assert_eq!(be.unmaps[1..], [(101, 0x10000, 2), (101, 0x40000, 1)]);
}

#[test]
fn bad_binds_are_refused() {
    let (mut f, mut be, mut mem) = (File::<Fake>::default(), Fake::default(), Mem::default());
    let h = create(&mut f, &mut be, 4096).unwrap();
    let v = vm(&mut f, &mut be);
    for (arg, want) in [
        (bind_arg(v, h, 0, 8192, 0x10000, 0), Errno::Inval), // beyond the buffer
        (bind_arg(v, h, 0, 4096, 0x10800, 0), Errno::Inval), // unaligned
        (bind_arg(v, h, 0, 4096, 1 << 48, 0), Errno::Inval), // beyond the address space
        (bind_arg(v, 999, 0, 4096, 0x10000, 0), Errno::NoEnt),
        (bind_arg(999, h, 0, 4096, 0x10000, 0), Errno::NoEnt),
        (bind_arg(v, h, 0, 4096, 0x10000, 2), Errno::Inval), // user pointers
    ] {
        assert_eq!(f.vm_bind(&mut be, &arg, &mut mem), Err(want));
    }
    let mut sparse = bind_arg(v, h, 0, 4096, 0x10000, 0);
    put32(&mut sparse, 24 + 44, 1 << 2);
    assert_eq!(f.vm_bind(&mut be, &sparse, &mut mem), Err(Errno::Inval));
    assert!(be.maps.is_empty());
}

#[test]
fn several_binds_come_from_the_program_and_fences_are_stored() {
    let (mut f, mut be) = (File::<Fake>::default(), Fake::default());
    let h = create(&mut f, &mut be, 8192).unwrap();
    let v = vm(&mut f, &mut be);
    let one = bind_arg(v, h, 0, 4096, 0x10000, 0);
    let two = bind_arg(v, h, 4096, 4096, 0x20000, 0);
    let mut vector = one[24..104].to_vec();
    vector.extend_from_slice(&two[24..104]);
    let mut mem = fence_mem(0xfeed);
    mem.0.push((0x5000, vector));
    let mut arg = [0u8; 136];
    put32(&mut arg, 8, v);
    put32(&mut arg, 20, 2);
    put64(&mut arg, 24, 0x5000);
    put32(&mut arg, 108, 1);
    put64(&mut arg, 112, 0x7000);
    f.vm_bind(&mut be, &arg, &mut mem).unwrap();
    assert_eq!(be.maps.len(), 2);
    assert_eq!(
        u64_at(&mem.0[1].1, 0),
        0xfeed,
        "binds are done when the call returns"
    );
}

#[test]
fn a_queue_runs_batches_and_stores_fences_when_they_finish() {
    let (mut f, mut be) = (File::<Fake>::default(), Fake::default());
    let v = vm(&mut f, &mut be);
    let q = queue(&mut f, &mut be, v, 4).unwrap();
    assert_eq!(be.queues, [(101, EngineClass::Compute)]);
    let mut mem = fence_mem(0x1234);
    f.exec(&mut be, &exec_arg(q, 0x10_0000, 0x7000, 1), &mut mem)
        .unwrap();
    assert_eq!(be.runs, [(201, 0x10_0000)]);
    f.reap(&mut be, &mut mem);
    assert_eq!(u64_at(&mem.0[1].1, 0), 0, "the job has not finished");
    be.done = 1;
    f.reap(&mut be, &mut mem);
    assert_eq!(u64_at(&mem.0[1].1, 0), 0x1234);
    // no batch: only the fence
    let mut mem = fence_mem(7);
    let mut none = exec_arg(q, 0, 0x7000, 1);
    none[32] = 0;
    f.exec(&mut be, &none, &mut mem).unwrap();
    assert_eq!((be.runs.len(), u64_at(&mem.0[1].1, 0)), (1, 7));
}

#[test]
fn bad_queue_and_exec_requests_are_refused() {
    let (mut f, mut be, mut mem) = (File::<Fake>::default(), Fake::default(), Mem::default());
    let v = vm(&mut f, &mut be);
    assert_eq!(
        queue(&mut f, &mut be, v, 2),
        Err(Errno::Inval),
        "no video engine"
    );
    assert_eq!(queue(&mut f, &mut be, 999, 0), Err(Errno::NoEnt));
    let q = queue(&mut f, &mut be, v, 0).unwrap();
    assert_eq!(
        f.exec(&mut be, &exec_arg(999, 0, 0, 0), &mut mem),
        Err(Errno::NoEnt)
    );
    let mut two = exec_arg(q, 0, 0, 0);
    two[32] = 2;
    assert_eq!(f.exec(&mut be, &two, &mut mem), Err(Errno::Inval));
    // a sync type that does not exist
    let mut m = fence_mem(1);
    put32(&mut m.0[0].1, 8, 7);
    assert_eq!(
        f.exec(&mut be, &exec_arg(q, 0, 0x7000, 1), &mut m),
        Err(Errno::Inval)
    );
    assert_eq!(
        f.exec(&mut be, &exec_arg(q, 0, 0x6000, 1), &mut mem),
        Err(Errno::Fault)
    );
    assert!(be.runs.is_empty());
    // an address space with a queue cannot go
    let mut destroy = [0u8; 24];
    put32(&mut destroy, 0, v);
    assert_eq!(f.vm_destroy(&mut be, &destroy), Err(Errno::Busy));
    let mut qd = [0u8; 24];
    put32(&mut qd, 0, q);
    f.queue_destroy(&mut be, &qd).unwrap();
    f.vm_destroy(&mut be, &destroy).unwrap();
    assert_eq!(
        (be.freed_queues.clone(), be.freed_vms.clone()),
        (vec![201], vec![101])
    );
}

fn wait_arg(op: u16, value: u64, timeout: i64) -> [u8; 72] {
    let mut arg = [0u8; 72];
    put64(&mut arg, 8, 0x8000);
    arg[16] = op as u8;
    put64(&mut arg, 24, value);
    put64(&mut arg, 32, u64::MAX);
    put64(&mut arg, 40, timeout as u64);
    arg
}

#[test]
fn waiting_for_a_fence_asks_again_until_the_value_or_the_time() {
    let (mut f, mut be) = (File::<Fake>::default(), Fake::default());
    let mut mem = Mem(vec![(0x8000, vec![0; 8])]);
    be.now = 1_000;
    let arg = wait_arg(0, 5, 10_000);
    assert_eq!(
        f.wait_user_fence(&mut be, &arg, &mut mem),
        Err(Errno::Again)
    );
    be.now = 5_000;
    assert_eq!(
        f.wait_user_fence(&mut be, &arg, &mut mem),
        Err(Errno::Again)
    );
    mem.0[0].1[0] = 5;
    assert_eq!(f.wait_user_fence(&mut be, &arg, &mut mem), Ok(()));
    // a new wait gets its own time
    mem.0[0].1[0] = 0;
    assert_eq!(
        f.wait_user_fence(&mut be, &arg, &mut mem),
        Err(Errno::Again)
    );
    be.now = 15_000;
    assert_eq!(f.wait_user_fence(&mut be, &arg, &mut mem), Err(Errno::Time));
    // no time at all: one look
    assert_eq!(
        f.wait_user_fence(&mut be, &wait_arg(0, 5, 0), &mut mem),
        Err(Errno::Time)
    );
    // other comparisons, mask
    assert_eq!(
        f.wait_user_fence(&mut be, &wait_arg(1, 5, 0), &mut mem),
        Ok(())
    );
    assert_eq!(
        f.wait_user_fence(&mut be, &wait_arg(5, 0, 0), &mut mem),
        Ok(())
    );
    assert_eq!(
        f.wait_user_fence(&mut be, &wait_arg(2, 0, 0), &mut mem),
        Err(Errno::Time)
    );
    assert_eq!(
        f.wait_user_fence(&mut be, &wait_arg(9, 0, 0), &mut mem),
        Err(Errno::Inval)
    );
    assert_eq!(
        f.wait_user_fence(&mut be, &wait_arg(0, 0, -1), &mut mem),
        Err(Errno::Inval)
    );
}

#[test]
fn closing_the_file_ends_queues_and_address_spaces() {
    let (mut f, mut be) = (File::<Fake>::default(), Fake::default());
    let v = vm(&mut f, &mut be);
    queue(&mut f, &mut be, v, 1).unwrap();
    create(&mut f, &mut be, 4096).unwrap();
    f.close(&mut be);
    assert_eq!(
        (be.freed_queues.clone(), be.freed_vms.clone()),
        (vec![201], vec![101])
    );
    assert!(f.buffer_at(1 << 32).is_none());
}

#[test]
fn a_stopped_queue_releases_its_fences_and_takes_no_more_jobs() {
    let (mut f, mut be) = (File::<Fake>::default(), Fake::default());
    let v = vm(&mut f, &mut be);
    let q = queue(&mut f, &mut be, v, 4).unwrap();
    let mut mem = fence_mem(0x99);
    f.exec(&mut be, &exec_arg(q, 0x10_0000, 0x7000, 1), &mut mem)
        .unwrap();
    let mut prop = [0u8; 40];
    put32(&mut prop, 8, q);
    f.queue_get_property(&mut be, &mut prop).unwrap();
    assert_eq!(u64_at(&prop, 16), 0);
    be.banned = true;
    f.reap(&mut be, &mut mem);
    assert_eq!(u64_at(&mem.0[1].1, 0), 0x99, "the waiter is let go");
    f.queue_get_property(&mut be, &mut prop).unwrap();
    assert_eq!(u64_at(&prop, 16), 1);
    assert_eq!(
        f.exec(&mut be, &exec_arg(q, 0x10_0000, 0, 0), &mut mem),
        Err(Errno::Canceled)
    );
    assert_eq!(be.runs.len(), 1);
}

#[test]
fn a_bind_queue_names_binds_and_an_empty_exec_follows_the_last_job() {
    let (mut f, mut be, mut mem) = (File::<Fake>::default(), Fake::default(), Mem::default());
    let v = vm(&mut f, &mut be);
    // class 5 is no engine: nothing is asked of the GPU
    let bq = queue(&mut f, &mut be, v, 5).unwrap();
    assert!(be.queues.is_empty());
    let h = create(&mut f, &mut be, 4096).unwrap();
    let mut arg = bind_arg(v, h, 0, 4096, 0x10000, 0);
    put32(&mut arg, 12, bq);
    f.vm_bind(&mut be, &arg, &mut mem).unwrap();
    // an engine queue or a queue of another address space cannot be named for a bind
    let q = queue(&mut f, &mut be, v, 0).unwrap();
    put32(&mut arg, 12, q);
    assert_eq!(f.vm_bind(&mut be, &arg, &mut mem), Err(Errno::Inval));
    // and a bind queue runs nothing
    assert_eq!(f.exec(&mut be, &exec_arg(bq, 0x10000, 0, 0), &mut mem), Err(Errno::Inval));
    // waiting for idle: the fence of an exec without batch is stored when the last job is over
    f.exec(&mut be, &exec_arg(q, 0x10000, 0, 0), &mut mem).unwrap();
    let mut mem = fence_mem(9);
    let mut idle = exec_arg(q, 0, 0x7000, 1);
    idle[32] = 0;
    f.exec(&mut be, &idle, &mut mem).unwrap();
    f.reap(&mut be, &mut mem);
    assert_eq!((be.runs.len(), u64_at(&mem.0[1].1, 0)), (1, 0));
    be.done = 1;
    f.reap(&mut be, &mut mem);
    assert_eq!(u64_at(&mem.0[1].1, 0), 9);
    let mut destroy = [0u8; 24];
    put32(&mut destroy, 0, bq);
    f.queue_destroy(&mut be, &destroy).unwrap();
    assert!(be.freed_queues.is_empty());
}
