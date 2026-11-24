//! Simulated register file for the tests: plain registers, values that change after a number of
//! reads, reactions to writes and a log of all writes. Time advances only through `delay_us`.

use crate::hw::{Hw, Reg};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Reaction to a write: gets the register file, the register and the written value.
pub type Hook = Box<dyn FnMut(&mut BTreeMap<Reg, u32>, Reg, u32)>;

/// The simulated device.
#[derive(Default)]
pub struct Sim {
    /// Register values.
    pub regs: BTreeMap<Reg, u32>,
    /// Every write in order.
    pub writes: Vec<(Reg, u32)>,
    delayed: BTreeMap<Reg, (u32, u32)>,
    hooks: BTreeMap<Reg, Hook>,
    now: u64,
}

impl Sim {
    /// Empty register file (every register reads 0).
    pub fn new() -> Sim {
        Sim::default()
    }

    /// Sets a register without logging a write.
    pub fn set(&mut self, reg: Reg, val: u32) {
        self.regs.insert(reg, val);
    }

    /// Current value.
    pub fn get(&self, reg: Reg) -> u32 {
        self.regs.get(&reg).copied().unwrap_or(0)
    }

    /// After `reads` more reads, `reg` reads `val`.
    pub fn after_reads(&mut self, reg: Reg, reads: u32, val: u32) {
        self.delayed.insert(reg, (reads, val));
    }

    /// Runs `hook` after every write to `reg` (the written value is stored first).
    pub fn on_write(&mut self, reg: Reg, hook: Hook) {
        self.hooks.insert(reg, hook);
    }

    /// The writes to `reg`, in order.
    pub fn writes_to(&self, reg: Reg) -> Vec<u32> {
        self.writes
            .iter()
            .filter(|(r, _)| *r == reg)
            .map(|(_, v)| *v)
            .collect()
    }

    /// Position of the first write to `reg` in the write log.
    pub fn first_write(&self, reg: Reg) -> Option<usize> {
        self.writes.iter().position(|(r, _)| *r == reg)
    }
}

impl Hw for Sim {
    fn read(&mut self, reg: Reg) -> u32 {
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

    fn write(&mut self, reg: Reg, val: u32) {
        self.writes.push((reg, val));
        self.regs.insert(reg, val);
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
