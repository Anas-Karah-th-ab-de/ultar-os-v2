//! The loop's part of output management: switching the backend's modes,
//! applying a setup, and going back to the previous arrangement when a
//! provisional setup was not confirmed in time or its client ended.

use super::Compositor;
use crate::state::{ClientId, Provisional};
use ultar_surface::proto::{Event, OutputMode, Setup, SetupOutcome, SetupRefusal};

impl Compositor {
    /// Runs what the display service asked for since the last turn.
    pub(super) fn run_output_jobs(&mut self) {
        self.state.tick_outputs();
        if let Some(reason) = self.state.session.manage.revert_due.take() {
            self.revert_provisional(reason);
        }
        while let Some((client, setup)) = self.state.session.manage.queued.pop_front() {
            let serial = setup.serial;
            let outcome = match self.apply_setup(client, &setup) {
                Ok(()) => SetupOutcome::Applied,
                Err(refusal) => SetupOutcome::Refused(refusal),
            };
            self.state
                .send(client, Event::OutputSetupDone { serial, outcome });
        }
        let detect = std::mem::take(&mut self.state.session.manage.detect);
        if !detect.is_empty() {
            self.rescan_outputs();
            let count = self.state.layout.outputs().len() as u32;
            for client in detect {
                self.state.send_output_details(client);
                self.state.send(client, Event::OutputsDetected { count });
            }
        }
    }

    /// Switches modes (all or none), then applies the setup.
    fn apply_setup(&mut self, client: ClientId, setup: &Setup) -> Result<(), SetupRefusal> {
        self.state.layout.check_setup(setup)?;
        let changes: Vec<(u32, OutputMode, OutputMode)> = setup
            .outputs
            .iter()
            .filter_map(|s| {
                let o = self.state.layout.get(s.output)?;
                let now = OutputMode {
                    width: o.width,
                    height: o.height,
                    refresh_mhz: o.refresh_millihertz,
                };
                (now != s.mode).then_some((o.hal, now, s.mode))
            })
            .collect();
        let mut done: Vec<(u32, OutputMode)> = Vec::new();
        for &(hal, old, new) in &changes {
            if self.backend.set_mode(hal, new).is_err() {
                for &(hal, old) in done.iter().rev() {
                    let _ = self.backend.set_mode(hal, old);
                }
                return Err(SetupRefusal::NotSupported);
            }
            done.push((hal, old));
        }
        let previous = match self.state.session.manage.provisional.take() {
            // A second setup while waiting keeps the first "before".
            Some(p) => Some(p.previous),
            None => (setup.provisional_ms > 0).then(|| self.state.layout.snapshot()),
        };
        self.state.layout.apply_setup(setup);
        if let (Some(previous), true) = (previous, setup.provisional_ms > 0) {
            self.state.session.manage.provisional = Some(Provisional {
                owner: client,
                serial: setup.serial,
                deadline: self.state.now + u64::from(setup.provisional_ms),
                previous,
            });
        }
        self.after_output_change(changes.iter().map(|c| c.0).collect());
        Ok(())
    }

    /// Goes back to the arrangement before the provisional setup.
    pub(super) fn revert_provisional(&mut self, outcome: SetupOutcome) {
        let Some(p) = self.state.session.manage.provisional.take() else {
            return;
        };
        let mut switched = Vec::new();
        for (hal, mode) in self.state.layout.modes_to_restore(&p.previous) {
            if self.backend.set_mode(hal, mode).is_ok() {
                switched.push(hal);
            }
        }
        self.state.layout.restore(&p.previous, &switched);
        self.state.send(
            p.owner,
            Event::OutputSetupDone {
                serial: p.serial,
                outcome,
            },
        );
        self.after_output_change(switched);
    }

    /// Frames of resized outputs are drawn anew; clients learn the layout.
    fn after_output_change(&mut self, resized: Vec<u32>) {
        for hal in resized {
            if let Some(id) = self.state.layout.by_hal(hal).map(|o| o.id) {
                self.renderer.forget(id.0);
                self.last_present.remove(&id);
            }
        }
        self.state.outputs_changed();
    }
}
