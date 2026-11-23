//! r3 (passes – evidence, not a finding): a random walk over restarts, updater sessions,
//! cancel and "previous version", with fault points the sweep in `power_loss.rs` does not
//! have: a power loss directly before or directly after every boot-control call, a plain I/O
//! error from a boot-control call (clean-up code runs), failing health checks, and updater
//! restarts without a device restart.
//!
//! Invariants checked after every step: a slot the boot code may start always holds a
//! complete known system; some slot can always start; the rollback floor never goes down and
//! never exceeds the index of a release that ran healthy; no system below the floor starts.

mod common;

use common::*;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use ultar_update::bootctl::{BootControl, Slot, SlotMeta, SlotState, SlotWrite};
use ultar_update::engine::{Engine, Environment};
use ultar_update::error::UpdateError;
use ultar_update::fault::InterruptAfter;
use ultar_update::filectl::FileBootControl;
use ultar_update::health::FixedSpace;
use ultar_update::manifest::Role;
use ultar_update::stage::NoPacing;

#[derive(Clone, Copy, Debug)]
enum Mode {
    PowerLossBefore,
    PowerLossAfter,
    ErrorBefore,
}

struct Plan {
    calls: AtomicU64,
    at: u64,
    mode: Mode,
    dead: AtomicBool,
}

/// Boot control with one fault at the `at`-th state-changing call.
struct Faulty {
    inner: FileBootControl,
    plan: Arc<Plan>,
}

type Change<'a> = &'a mut dyn FnMut(&mut FileBootControl) -> Result<(), UpdateError>;

impl Faulty {
    fn change(&mut self, action: Change<'_>) -> Result<(), UpdateError> {
        if self.plan.dead.load(Ordering::SeqCst) {
            return Err(UpdateError::Interrupted);
        }
        if self.plan.calls.fetch_add(1, Ordering::SeqCst) != self.plan.at {
            return action(&mut self.inner);
        }
        match self.plan.mode {
            Mode::ErrorBefore => return Err(UpdateError::Io(std::io::Error::other("injected"))),
            Mode::PowerLossBefore => {}
            Mode::PowerLossAfter => action(&mut self.inner)?,
        }
        self.plan.dead.store(true, Ordering::SeqCst);
        Err(UpdateError::Interrupted)
    }
}

impl BootControl for Faulty {
    fn active_slot(&self) -> Result<Slot, UpdateError> {
        self.inner.active_slot()
    }
    fn slot_state(&self, slot: Slot) -> Result<SlotState, UpdateError> {
        self.inner.slot_state(slot)
    }
    fn roles(&self) -> Vec<Role> {
        self.inner.roles()
    }
    fn mark_unbootable(&mut self, slot: Slot) -> Result<(), UpdateError> {
        self.change(&mut |boot| boot.mark_unbootable(slot))
    }
    fn open_writer(
        &mut self,
        slot: Slot,
        role: Role,
        size: u64,
    ) -> Result<Box<dyn SlotWrite>, UpdateError> {
        if self.plan.dead.load(Ordering::SeqCst) {
            return Err(UpdateError::Interrupted);
        }
        self.inner.open_writer(slot, role, size)
    }
    fn open_reader(&self, slot: Slot, role: Role) -> Result<Box<dyn Read>, UpdateError> {
        self.inner.open_reader(slot, role)
    }
    fn mark_bootable(&mut self, slot: Slot, meta: &SlotMeta) -> Result<(), UpdateError> {
        self.change(&mut |boot| boot.mark_bootable(slot, meta))
    }
    fn set_next_boot_once(&mut self, slot: Slot, tries: u8) -> Result<(), UpdateError> {
        self.change(&mut |boot| boot.set_next_boot_once(slot, tries))
    }
    fn mark_successful(&mut self) -> Result<(), UpdateError> {
        self.change(&mut |boot| boot.mark_successful())
    }
    fn set_default(&mut self, slot: Slot) -> Result<(), UpdateError> {
        self.change(&mut |boot| boot.set_default(slot))
    }
}

struct World {
    device: Device,
    releases: Vec<Release>,
    floor: u64,
    proven: u64,
    log: Vec<String>,
}

impl World {
    fn new(releases: &[Release]) -> Self {
        Self {
            device: Device::new(),
            releases: releases.to_vec(),
            floor: 1,
            proven: 1,
            log: Vec::new(),
        }
    }

    /// Rollback index of the complete system in `slot`, if it holds one.
    fn index_in(&self, slot: Slot) -> Option<u64> {
        let images = self.device.images(slot);
        if images == self.device.factory {
            return Some(1);
        }
        let release = self.releases.iter().find(|r| r.payloads == images);
        release.map(|r| r.manifest.rollback_index)
    }

    fn check(&mut self, seed: u64) {
        let floor = self.device.engine().status().unwrap().rollback_floor;
        let log = &self.log;
        assert!(floor >= self.floor, "seed {seed}: floor fell\n{log:#?}");
        assert!(
            floor <= self.proven,
            "seed {seed}: floor unproven\n{log:#?}"
        );
        self.floor = floor;
        for slot in Slot::BOTH {
            let startable = self.device.boot.slot_state(slot).unwrap().bootable;
            let complete = self.index_in(slot).is_some();
            assert!(!startable || complete, "seed {seed}: {slot:?}\n{log:#?}");
        }
    }

    fn restart(&mut self, seed: u64) {
        let slot = self.device.boot.simulate_boot();
        let log = &self.log;
        let slot = slot.unwrap_or_else(|e| panic!("seed {seed}: no start: {e}\n{log:#?}"));
        let index = self.index_in(slot);
        let index = index.unwrap_or_else(|| panic!("seed {seed}: broken {slot:?}\n{log:#?}"));
        assert!(
            index >= self.floor,
            "seed {seed}: below the floor\n{log:#?}"
        );
        self.log.push(format!("start {slot:?} (index {index})"));
    }

    fn engine(&self, checkpoints: u64, plan: Arc<Plan>) -> Result<Engine<Faulty>, UpdateError> {
        let env = Environment {
            clock: self.device.clock.clone(),
            space: Arc::new(FixedSpace(u64::MAX)),
            control: Arc::new(InterruptAfter::new(checkpoints)),
        };
        let inner = self.device.boot.clone();
        Engine::open(self.device.config.clone(), env, Faulty { inner, plan })
    }
}

fn install_step(engine: &mut Engine<Faulty>, release: &Release) -> Result<(), UpdateError> {
    let mut source = MemSource::new(&release.bundle);
    let Some(offer) = engine.check(&mut source, "stable")? else {
        return Ok(());
    };
    engine.download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})?;
    engine.apply(&mut |_, _, _| {}).map(|_| ())
}

/// One life of the updater process, with at most one fault of each kind.
fn session(world: &mut World, rng: &mut Rng) {
    let checkpoints = [u64::MAX, u64::MAX, rng.below(40) as u64][rng.below(3)];
    let modes = [
        Mode::PowerLossBefore,
        Mode::PowerLossAfter,
        Mode::ErrorBefore,
    ];
    let plan = Arc::new(Plan {
        calls: AtomicU64::new(0),
        at: [u64::MAX, rng.below(8) as u64][rng.below(2)],
        mode: modes[rng.below(3)],
        dead: AtomicBool::new(false),
    });
    let healthy = rng.below(5) != 0;
    let Ok(mut engine) = world.engine(checkpoints, plan) else {
        return;
    };
    let running = world.index_in(engine.boot().active_slot().unwrap());
    if healthy {
        world.proven = world.proven.max(running.unwrap_or(0));
    }
    let outcome = engine.resume_after_boot(&Health(healthy));
    world.log.push(format!("resume({healthy}) {outcome:?}"));
    let mut dead = matches!(outcome, Err(UpdateError::Interrupted));
    for _ in 0..rng.below(3) {
        if dead {
            return;
        }
        let step = match rng.below(6) {
            0 => format!("cancel {:?}", engine.cancel_pending()),
            1 => format!("previous {:?}", engine.boot_previous()),
            _ => {
                let release = world.releases[rng.below(world.releases.len())].clone();
                let result = install_step(&mut engine, &release);
                format!("install {} {result:?}", release.manifest.version)
            }
        };
        dead = step.contains("Interrupted");
        world.log.push(step);
    }
}

#[test]
fn random_walk_with_faults_around_every_boot_control_call() {
    let releases = [
        release((0, 7, 1), 1),
        release((0, 7, 2), 2),
        release((0, 7, 3), 2),
        release((0, 7, 4), 3),
    ];
    let mut committed = 0;
    for seed in 1..=30u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut world = World::new(&releases);
        for _ in 0..25 {
            if rng.below(3) != 0 {
                world.restart(seed);
            }
            session(&mut world, &mut rng);
            world.check(seed);
        }
        committed += world.log.iter().filter(|l| l.contains("Committed")).count();
    }
    assert!(committed > 20, "the walk hardly installed anything");
}
