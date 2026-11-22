//! Shared fixture of the integration tests: an in-process broker with
//! token identities and a settings service with a manual clock.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::settings::{DisplayDescriptor, SetDisplays, SetDisplaysReply};
use ultar_bus::{Broker, Client, ClientOptions, Identity, Policy, TokenIdentity};
use ultar_profile::{Capabilities, Capability, CapabilitySet};
use ultar_settingsd::access::PolicyAccess;
use ultar_settingsd::apply::{Appliers, MockApplier};
use ultar_settingsd::clock::ManualClock;
use ultar_settingsd::core::{Core, CoreConfig};
use ultar_settingsd::schema::NoDefaults;
use ultar_settingsd::seed;
use ultar_settingsd::server::{Server, ServerParts};
use ultar_settingsd::store::StoreConfig;
use ultar_settingsd::testdir::TestDir;

/// Upper bound for anything that happens "at once".
pub const WAIT: Duration = Duration::from_secs(20);
/// Write delay of the store in these tests.
pub const DELAY: Duration = Duration::from_millis(250);

/// Identities of the token broker; the token is the identity name.
pub const IDENTITIES: [&str; 6] = [
    "system.settingsd",
    "system.devd",
    "system.displayd",
    "app.settings",
    "app.game",
    "app.viewer",
];

/// The policy of the tests: the settings app is an administrator, the game
/// may change plain settings, the viewer may only read.
pub const POLICY: &str = r#"
[[rule]]
identity  = "system.settingsd"
register  = ["settings"]
publish   = ["settings.*"]
subscribe = ["devices.*"]

[[rule]]
identity  = "system.devd"
publish   = ["devices.*"]

[[rule]]
identity  = "system.displayd"
call      = ["settings.set_displays", "settings.admin"]

[[rule]]
identity  = "app.*"
call      = ["settings.get", "settings.list"]
subscribe = ["settings.*"]

[[rule]]
identity  = "app.game"
call      = ["settings.set", "settings.reset", "settings.confirm", "settings.revert"]

[[rule]]
identity  = "app.settings"
call      = ["settings.*"]
"#;

pub struct Rig {
    // Declared first so that it is dropped first: the broker needs its
    // socket file, which lives in `dir`, to shut down.
    pub broker: Broker,
    pub socket: PathBuf,
    pub clock: ManualClock,
    pub applier: MockApplier,
    pub capabilities: Capabilities,
    pub dir: TestDir,
}

pub fn laptop() -> Capabilities {
    Capabilities {
        flags: CapabilitySet::of(&[Capability::Battery, Capability::Camera, Capability::AudioIn]),
        display_count: 1,
        ..Capabilities::default()
    }
}

pub fn policy() -> Policy {
    Policy::from_toml_str(POLICY).unwrap()
}

pub fn connect(socket: &Path, identity: &str) -> Client {
    let options = ClientOptions {
        token: Some(identity.to_owned()),
        ..ClientOptions::named(identity)
    };
    Client::connect(socket, options).unwrap()
}

pub fn panel() -> DisplayDescriptor {
    DisplayDescriptor {
        manufacturer: "BOE".into(),
        product: "NE135".into(),
        connector: "eDP-1".into(),
        internal: true,
        width_px: 2880,
        height_px: 1800,
        dpi: 192,
        refresh_mhz: 60_000,
        ..DisplayDescriptor::default()
    }
}

impl Rig {
    pub fn new(tag: &str) -> Self {
        let dir = TestDir::new(tag);
        let socket = dir.path().join("bus.sock");
        let mut auth = TokenIdentity::new();
        for name in IDENTITIES {
            auth = auth.with_token(name, Identity::new(name).unwrap());
        }
        let broker = Broker::builder(policy())
            .listen(&socket, Arc::new(auth))
            .start()
            .unwrap();
        Self {
            broker,
            socket,
            clock: ManualClock::new(),
            applier: MockApplier::new(),
            capabilities: laptop(),
            dir,
        }
    }

    pub fn store_dir(&self) -> PathBuf {
        self.dir.path().join("store")
    }

    /// Starts the settings service (again: a restart of the service).
    pub fn start(&self) -> Server {
        let clock = Arc::new(self.clock.clone());
        let config = CoreConfig {
            store: StoreConfig {
                dir: self.store_dir(),
                user: "anas".into(),
                write_delay: DELAY,
            },
            registry: Arc::new(seed::registry().unwrap()),
            clock: clock.clone(),
            appliers: Appliers::new().with_all(&self.applier),
            defaults: Box::new(NoDefaults),
            capabilities: self.capabilities,
        };
        let (core, notices) = Core::open(config).unwrap();
        Server::start(ServerParts {
            bus: connect(&self.socket, "system.settingsd"),
            core,
            access: Box::new(PolicyAccess(policy())),
            clock,
            notices,
            on_error: Box::new(|error| panic!("unexpected background failure: {error}")),
            register_wait: WAIT,
        })
        .unwrap()
    }

    pub fn client(&self, identity: &str) -> Client {
        connect(&self.socket, identity)
    }

    /// Reports the laptop panel as the display service would.
    pub fn report_panel(&self) -> SetDisplaysReply {
        let displayd = self.client("system.displayd");
        let call = SetDisplays {
            displays: vec![panel()],
        };
        displayd.call(&call, WAIT).unwrap()
    }

    pub fn advance(&self, seconds: u64) {
        self.clock.advance(Duration::from_secs(seconds));
    }
}
