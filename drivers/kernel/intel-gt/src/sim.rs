//! Simulated register file for the tests: plain registers, values that change after a number of
//! reads, reactions to writes and a log of all writes. Time advances only through `delay_us`.

use crate::Hw;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Reaction to a write: gets the register file, the register and the written value.
pub type Hook = Box<dyn FnMut(&mut BTreeMap<u32, u32>, u32, u32)>;

/// The simulated device.
#[derive(Default)]
pub struct Sim {
    /// Register values.
    pub regs: BTreeMap<u32, u32>,
    /// Every write in order.
    pub writes: Vec<(u32, u32)>,
    delayed: BTreeMap<u32, (u32, u32)>,
    hooks: BTreeMap<u32, Hook>,
    read_only: Vec<u32>,
    now: u64,
}

impl Sim {
    /// Empty register file (every register reads 0).
    pub fn new() -> Sim {
        Sim::default()
    }

    /// Sets a register without logging a write.
    pub fn set(&mut self, reg: u32, val: u32) {
        self.regs.insert(reg, val);
    }

    /// Current value.
    pub fn get(&self, reg: u32) -> u32 {
        self.regs.get(&reg).copied().unwrap_or(0)
    }

    /// After `reads` more reads, `reg` reads `val`.
    pub fn after_reads(&mut self, reg: u32, reads: u32, val: u32) {
        self.delayed.insert(reg, (reads, val));
    }

    /// Runs `hook` after every write to `reg` (the written value is stored first).
    pub fn on_write(&mut self, reg: u32, hook: Hook) {
        self.hooks.insert(reg, hook);
    }

    /// Writes to `reg` are logged but do not change its value.
    pub fn read_only(&mut self, reg: u32) {
        self.read_only.push(reg);
    }

    /// The writes to `reg`, in order.
    pub fn writes_to(&self, reg: u32) -> Vec<u32> {
        self.writes
            .iter()
            .filter(|(r, _)| *r == reg)
            .map(|(_, v)| *v)
            .collect()
    }

    /// A GT whose power domains acknowledge wake and sleep requests at once (`base` 0: primary
    /// GT, [`crate::regs::MEDIA_GSI_OFFSET`]: media GT).
    pub fn with_forcewake(mut self, base: u32) -> Sim {
        use crate::forcewake::Domain;
        let mut all = alloc::vec![Domain::Gt, Domain::Render, Domain::Gsc];
        all.extend((0..8).map(Domain::MediaVdbox));
        all.extend((0..4).map(Domain::MediaVebox));
        for d in all {
            let ack = d.ack() + base;
            self.on_write(
                d.ctl() + base,
                Box::new(move |regs, _, v| {
                    // masked register: the upper half selects which lower bits change
                    let old = regs.get(&ack).copied().unwrap_or(0);
                    let mask = v >> 16;
                    regs.insert(ack, (old & !mask) | (v & mask));
                }),
            );
        }
        self
    }
}

impl Hw for Sim {
    fn read(&mut self, reg: u32) -> u32 {
        if let Some((n, v)) = self.delayed.get_mut(&reg) {
            if *n == 0 {
                let v = *v;
                self.delayed.remove(&reg);
                self.regs.insert(reg, v);
            } else {
                *n -= 1;
            }
        }
        self.get(reg)
    }

    fn write(&mut self, reg: u32, val: u32) {
        self.writes.push((reg, val));
        if !self.read_only.contains(&reg) {
            self.regs.insert(reg, val);
        }
        if let Some(mut h) = self.hooks.remove(&reg) {
            h(&mut self.regs, reg, val);
            self.hooks.insert(reg, h);
        }
    }

    fn delay_us(&mut self, us: u32) {
        self.now += us as u64;
    }

    fn now_us(&mut self) -> u64 {
        // every look at the clock costs a little, so a wait loop without a delay still ends
        self.now += 1;
        self.now
    }
}
