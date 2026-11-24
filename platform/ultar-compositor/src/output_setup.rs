//! A whole output arrangement from the display service: checked against
//! what is connected, applied at once, and remembered so that a
//! provisional arrangement can be taken back.

use crate::output::SCALE_STEP_MILLI;
use crate::output::{DisplayMode, Layout, Output, Placement, MAX_SCALE_MILLI, MIN_SCALE_MILLI};
use std::collections::BTreeSet;
use ultar_surface::proto::{
    Arrangement, OutputDetails, OutputId, OutputMode, Setup, SetupRefusal, Transform,
};

/// Largest distance of an output's corner from the origin (logical points).
pub const MAX_POSITION: i32 = 32_768;

/// An arrangement as it was, to go back to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    outputs: Vec<Output>,
    mode: DisplayMode,
}

fn mode_of(o: &Output) -> OutputMode {
    OutputMode {
        width: o.width,
        height: o.height,
        refresh_mhz: o.refresh_millihertz,
    }
}

impl Layout {
    /// The modes an output offers (at least the one it runs).
    pub fn modes_of(&self, id: OutputId) -> Vec<OutputMode> {
        let current = self.get(id).map(mode_of);
        match self.details.get(&id) {
            Some(d) if !d.modes.is_empty() => d.modes.clone(),
            _ => current.into_iter().collect(),
        }
    }

    /// Everything the display service learns about an output.
    pub fn details_of(&self, id: OutputId) -> Option<OutputDetails> {
        let o = self.get(id)?;
        let d = self.details.get(&id).cloned().unwrap_or_default();
        let current = mode_of(o);
        let mut modes = self.modes_of(id);
        modes.truncate(ultar_surface::proto::MAX_OUTPUT_MODES);
        Some(OutputDetails {
            id,
            connector: o.name.clone(),
            identity: d.identity,
            internal: d.internal,
            size_mm: (d.width_mm, d.height_mm),
            preferred: modes.first().copied().unwrap_or(current),
            current,
            modes,
        })
    }

    /// Checks a setup against the connected outputs; nothing changes.
    pub fn check_setup(&self, setup: &Setup) -> Result<(), SetupRefusal> {
        let ids: BTreeSet<OutputId> = setup.outputs.iter().map(|o| o.output).collect();
        let connected: BTreeSet<OutputId> = self.outputs().iter().map(|o| o.id).collect();
        if ids.len() != setup.outputs.len() || ids != connected {
            return Err(SetupRefusal::UnknownOutput);
        }
        for o in &setup.outputs {
            if !self.modes_of(o.output).contains(&o.mode) {
                return Err(SetupRefusal::UnknownMode);
            }
            let scale_ok = (MIN_SCALE_MILLI..=MAX_SCALE_MILLI).contains(&o.scale_milli)
                && o.scale_milli % SCALE_STEP_MILLI == 0;
            let place_ok = o.position.0.abs() <= MAX_POSITION && o.position.1.abs() <= MAX_POSITION;
            if !scale_ok || !place_ok {
                return Err(SetupRefusal::Invalid);
            }
            if o.transform != Transform::Normal {
                return Err(SetupRefusal::NotSupported);
            }
        }
        let included: Vec<_> = setup.outputs.iter().filter(|o| o.included).collect();
        let primaries = setup.outputs.iter().filter(|o| o.primary).count();
        let primary_included = included.iter().any(|o| o.primary);
        let single_ok = setup.arrangement != Arrangement::Single || included.len() == 1;
        if included.is_empty() || primaries != 1 || !primary_included || !single_ok {
            return Err(SetupRefusal::Invalid);
        }
        if setup.arrangement == Arrangement::Extend && overlapping(&included) {
            return Err(SetupRefusal::Invalid);
        }
        Ok(())
    }

    /// Applies a checked setup whose modes the backend already switched.
    pub fn apply_setup(&mut self, setup: &Setup) {
        let primary = setup.outputs.iter().find(|o| o.primary).map(|o| o.output);
        for s in &setup.outputs {
            let Some(o) = self.outputs.iter_mut().find(|o| o.id == s.output) else {
                continue;
            };
            o.width = s.mode.width;
            o.height = s.mode.height;
            o.refresh_millihertz = s.mode.refresh_mhz.max(1000);
            o.user_scale_milli = s.scale_milli;
            o.primary = Some(s.output) == primary;
            o.placement = Placement {
                position: Some(s.position),
                transform: s.transform,
                included: s.included,
            };
            if let Some(d) = self.details.get_mut(&s.output) {
                (d.width, d.height, d.refresh_millihertz) =
                    (s.mode.width, s.mode.height, s.mode.refresh_mhz);
            }
        }
        self.mode = match (setup.arrangement, primary) {
            (Arrangement::Mirror, _) => DisplayMode::Mirror,
            (Arrangement::Single, Some(id)) => DisplayMode::Single(id),
            _ => DisplayMode::Extend,
        };
        self.arrange();
    }

    /// The arrangement as it is now.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            outputs: self.outputs.clone(),
            mode: self.mode,
        }
    }

    /// The modes the backend must switch to before [`Layout::restore`]
    /// (outputs that are still connected and run another mode now).
    pub fn modes_to_restore(&self, snapshot: &Snapshot) -> Vec<(u32, OutputMode)> {
        snapshot
            .outputs
            .iter()
            .filter_map(|old| {
                let now = self.get(old.id)?;
                (mode_of(now) != mode_of(old)).then(|| (now.hal, mode_of(old)))
            })
            .collect()
    }

    /// Goes back to a snapshot for the outputs still connected; outputs
    /// connected since keep their place. `switched` says which modes the
    /// backend could switch back (the others keep their mode).
    pub fn restore(&mut self, snapshot: &Snapshot, switched: &[u32]) {
        for o in &mut self.outputs {
            let Some(old) = snapshot.outputs.iter().find(|s| s.id == o.id) else {
                continue;
            };
            if mode_of(o) == mode_of(old) || switched.contains(&o.hal) {
                (o.width, o.height, o.refresh_millihertz) =
                    (old.width, old.height, old.refresh_millihertz);
            }
            o.user_scale_milli = old.user_scale_milli;
            o.primary = old.primary;
            o.placement = old.placement;
        }
        for o in &self.outputs {
            if let Some(d) = self.details.get_mut(&o.id) {
                (d.width, d.height, d.refresh_millihertz) =
                    (o.width, o.height, o.refresh_millihertz);
            }
        }
        self.mode = match snapshot.mode {
            DisplayMode::Single(id) if self.get(id).is_none() => DisplayMode::Extend,
            mode => mode,
        };
        let primaries = self.outputs.iter().filter(|o| o.primary).count();
        if primaries != 1 {
            for (i, o) in self.outputs.iter_mut().enumerate() {
                o.primary = i == 0;
            }
        }
        if !self.outputs.iter().any(|o| o.placement.included) {
            self.outputs
                .iter_mut()
                .for_each(|o| o.placement.included = true);
        }
        self.arrange();
    }
}

fn overlapping(included: &[&ultar_surface::proto::OutputSetup]) -> bool {
    let rect = |o: &ultar_surface::proto::OutputSetup| {
        let s = f64::from(o.scale_milli) / 1000.0;
        crate::geom::Rect::new(
            o.position.0,
            o.position.1,
            (f64::from(o.mode.width) / s).ceil() as i32,
            (f64::from(o.mode.height) / s).ceil() as i32,
        )
    };
    included.iter().enumerate().any(|(i, a)| {
        included[i + 1..]
            .iter()
            .any(|b| rect(a).intersect(&rect(b)).is_some())
    })
}

#[cfg(test)]
#[path = "output_setup_tests.rs"]
mod tests;
