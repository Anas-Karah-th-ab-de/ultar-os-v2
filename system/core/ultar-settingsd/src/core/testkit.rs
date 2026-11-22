//! Shared fixture of the core's unit tests.

use super::{Core, CoreConfig, Event};
use crate::access::Caller;
use crate::apply::{Appliers, MockApplier};
use crate::clock::ManualClock;
use crate::schema::NoDefaults;
use crate::seed;
use crate::store::{Notice, StoreConfig};
use crate::testdir::TestDir;
use crate::value::Value;
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::settings::DisplayDescriptor;
use ultar_profile::{Capabilities, Capability, CapabilitySet};

pub(crate) const DELAY: Duration = Duration::from_millis(250);

pub(crate) struct Kit {
    pub(crate) dir: TestDir,
    pub(crate) clock: ManualClock,
    pub(crate) applier: MockApplier,
    pub(crate) capabilities: Capabilities,
}

pub(crate) fn laptop_capabilities() -> Capabilities {
    Capabilities {
        flags: CapabilitySet::of(&[
            Capability::Battery,
            Capability::Camera,
            Capability::AudioIn,
            Capability::AudioOut,
            Capability::Keyboard,
            Capability::Pointer,
        ]),
        display_count: 1,
        ..Capabilities::default()
    }
}

pub(crate) fn display(
    mfg: &str,
    product: &str,
    connector: &str,
    internal: bool,
) -> DisplayDescriptor {
    DisplayDescriptor {
        manufacturer: mfg.into(),
        product: product.into(),
        connector: connector.into(),
        internal,
        width_px: if internal { 2880 } else { 1920 },
        height_px: if internal { 1800 } else { 1080 },
        dpi: if internal { 192 } else { 96 },
        refresh_mhz: 60_000,
        ..DisplayDescriptor::default()
    }
}

/// The built-in panel `boe-ne135`.
pub(crate) fn panel() -> DisplayDescriptor {
    display("BOE", "NE135", "eDP-1", true)
}

/// The monitor at the dock at home, `del-u2720q`.
pub(crate) fn dock() -> DisplayDescriptor {
    display("DEL", "U2720Q", "DP-3", false)
}

/// The projector at the office, `eps-eb-u42`.
pub(crate) fn projector() -> DisplayDescriptor {
    display("EPS", "EB-U42", "HDMI-A-1", false)
}

impl Kit {
    pub(crate) fn new(tag: &str) -> Self {
        Self {
            dir: TestDir::new(tag),
            clock: ManualClock::new(),
            applier: MockApplier::new(),
            capabilities: laptop_capabilities(),
        }
    }

    pub(crate) fn config(&self) -> CoreConfig {
        CoreConfig {
            store: StoreConfig {
                dir: self.dir.path().to_path_buf(),
                user: "anas".into(),
                write_delay: DELAY,
            },
            registry: Arc::new(seed::registry().unwrap()),
            clock: Arc::new(self.clock.clone()),
            appliers: Appliers::new().with_all(&self.applier),
            defaults: Box::new(NoDefaults),
            capabilities: self.capabilities,
        }
    }

    pub(crate) fn open_with_notices(&self) -> (Core, Vec<Notice>) {
        Core::open(self.config()).unwrap()
    }

    /// Opens the core; a restart of the service when called again.
    pub(crate) fn open(&self) -> Core {
        self.open_with_notices().0
    }

    /// A core with the laptop panel reported, everything written and
    /// events cleared.
    pub(crate) fn open_with_panel(&self) -> Core {
        let mut core = self.open();
        core.set_displays(&admin(), &[panel()]).unwrap();
        core.flush().unwrap();
        core.take_events();
        self.applier.clear_calls();
        core
    }

    pub(crate) fn advance(&self, seconds: u64) {
        self.clock.advance(Duration::from_secs(seconds));
    }
}

pub(crate) fn admin() -> Caller {
    Caller::admin("app.settings")
}

pub(crate) fn user() -> Caller {
    Caller::user("app.game")
}

pub(crate) fn changed(key: &str, value: Value) -> Event {
    Event::Changed {
        key: key.to_owned(),
        value,
    }
}
