//! Simulated register file for the tests: plain registers, values that change after a number of
//! reads, hooks that react to writes (AUX channel, PHY message bus, power wells) and a log of all
//! writes. Time advances only through `delay_us`.

use crate::hw::Hw;
use crate::mode::{Timing, FLAG_PHSYNC, FLAG_PVSYNC};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Reaction to a write: gets the register file, the register and the written value.
pub type Hook = Box<dyn FnMut(&mut BTreeMap<u32, u32>, u32, u32)>;

/// The simulated device.
pub struct Sim {
    /// Register values.
    pub regs: BTreeMap<u32, u32>,
    /// Every write in order.
    pub writes: Vec<(u32, u32)>,
    delayed: BTreeMap<u32, (u32, u32)>,
    hooks: BTreeMap<u32, Hook>,
    now: u64,
}

impl Default for Sim {
    fn default() -> Self {
        Self::new()
    }
}

impl Sim {
    /// Empty register file (every register reads 0).
    pub fn new() -> Sim {
        Sim {
            regs: BTreeMap::new(),
            writes: Vec::new(),
            delayed: BTreeMap::new(),
            hooks: BTreeMap::new(),
            now: 0,
        }
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

    /// The writes to `reg`, in order.
    pub fn writes_to(&self, reg: u32) -> Vec<u32> {
        self.writes
            .iter()
            .filter(|(r, _)| *r == reg)
            .map(|(_, v)| *v)
            .collect()
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
        self.regs.insert(reg, val);
        if let Some(mut h) = self.hooks.remove(&reg) {
            h(&mut self.regs, reg, val);
            self.hooks.insert(reg, h);
        }
    }

    fn delay_us(&mut self, us: u32) {
        self.now += us.max(1) as u64;
    }

    fn now_us(&mut self) -> u64 {
        self.now
    }
}

/// The Beelink's mode: 1920x1080@60, 148.5 MHz, positive syncs (as measured under Linux).
pub fn fhd() -> Timing {
    Timing {
        clock: 148_500,
        hdisplay: 1920,
        hsync_start: 2008,
        hsync_end: 2052,
        htotal: 2200,
        vdisplay: 1080,
        vsync_start: 1084,
        vsync_end: 1089,
        vtotal: 1125,
        flags: FLAG_PHSYNC | FLAG_PVSYNC,
    }
}
