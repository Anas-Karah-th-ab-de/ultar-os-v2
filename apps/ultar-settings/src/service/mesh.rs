//! The device mesh behind the page Settings › Mesh, as a port: over the bus ([`BusMesh`])
//! and for tests ([`FakeMesh`]).

use super::bus::{call, Retained};
use super::fake::Shared;
use super::{PortError, PortResult};
use std::time::Duration;
use ultar_bus::messages::mesh::{
    Done, Grant, MeshState, PairBegin, PairCode, PairJoin, SetEnabled, Unpair,
};
use ultar_bus::{Client, Text};

const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// What the page asks of the mesh.
pub trait MeshPort {
    /// The mesh as it is now.
    fn state(&mut self) -> PortResult<MeshState>;
    /// Mesh on or off.
    fn set_enabled(&mut self, on: bool) -> PortResult<()>;
    /// This device shows a code.
    fn pair_begin(&mut self) -> PortResult<PairCode>;
    /// The code another device shows was typed here.
    fn pair_join(&mut self, device: &str, code: &str) -> PortResult<()>;
    /// Forgets a device.
    fn unpair(&mut self, device: &str) -> PortResult<()>;
    /// What a paired device may do.
    fn set_grants(&mut self, device: &str, grants: Vec<Grant>) -> PortResult<()>;
}

/// `mesh` over the bus.
pub struct BusMesh {
    client: Client,
    state: Retained<MeshState>,
}

impl std::fmt::Debug for BusMesh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusMesh")
    }
}

impl BusMesh {
    /// Subscribes to `mesh.state`.
    pub fn new(client: &Client) -> BusMesh {
        BusMesh {
            client: client.clone(),
            state: Retained::new(client),
        }
    }
}

fn done(reply: PortResult<Done>, refused: &str) -> PortResult<()> {
    match reply {
        Ok(Done { ok: true }) => Ok(()),
        Ok(_) => Err(PortError::Refused(Text::key(refused))),
        Err(e) => Err(e),
    }
}

impl MeshPort for BusMesh {
    fn state(&mut self) -> PortResult<MeshState> {
        self.state.get()
    }

    fn set_enabled(&mut self, enabled: bool) -> PortResult<()> {
        let reply = call(&self.client, &SetEnabled { enabled }, CALL_TIMEOUT);
        done(reply, "app.settings.mesh.error.port")
    }

    fn pair_begin(&mut self) -> PortResult<PairCode> {
        call(&self.client, &PairBegin, CALL_TIMEOUT)
    }

    fn pair_join(&mut self, device: &str, code: &str) -> PortResult<()> {
        let join = PairJoin {
            device: device.to_owned(),
            code: code.to_owned(),
        };
        // the other device checks the code: longer than a local call
        let reply = call(&self.client, &join, 4 * CALL_TIMEOUT);
        done(reply, "app.settings.mesh.error.code")
    }

    fn unpair(&mut self, device: &str) -> PortResult<()> {
        let device = device.to_owned();
        let reply = call(&self.client, &Unpair { device }, CALL_TIMEOUT);
        done(reply, "app.settings.error.failed")
    }

    fn set_grants(&mut self, device: &str, grants: Vec<Grant>) -> PortResult<()> {
        let set = ultar_bus::messages::mesh::SetRights::of(device, grants); // physical
        let reply = call(&self.client, &set, CALL_TIMEOUT);
        done(reply, "app.settings.error.failed")
    }
}

/// The code the fake mesh shows and accepts.
pub const FAKE_CODE: &str = "482913";

/// The mesh of the test world (`World::system.mesh`).
#[derive(Debug, Clone)]
pub struct FakeMesh(pub Shared);

impl FakeMesh {
    fn change(&mut self, log: String, f: impl FnOnce(&mut MeshState)) -> PortResult<()> {
        let mut world = self.0.borrow_mut();
        world.log.push(log);
        if let Some(reason) = world.refuse.take() {
            return Err(PortError::Refused(reason));
        }
        world
            .system
            .mesh
            .as_mut()
            .map(f)
            .ok_or(PortError::Unavailable)
    }
}

impl MeshPort for FakeMesh {
    fn state(&mut self) -> PortResult<MeshState> {
        self.0
            .borrow()
            .system
            .mesh
            .clone()
            .ok_or(PortError::Unavailable)
    }

    fn set_enabled(&mut self, on: bool) -> PortResult<()> {
        self.change(format!("mesh.set_enabled {on}"), |m| m.enabled = on)
    }

    fn pair_begin(&mut self) -> PortResult<PairCode> {
        self.change("mesh.pair_begin".into(), |_| {})?;
        Ok(PairCode {
            code: FAKE_CODE.into(),
            seconds: 300,
        })
    }

    fn pair_join(&mut self, device: &str, code: &str) -> PortResult<()> {
        if code != FAKE_CODE {
            self.0
                .borrow_mut()
                .log
                .push(format!("mesh.pair_join {device} wrong"));
            return Err(PortError::Refused(Text::key(
                "app.settings.mesh.error.code",
            )));
        }
        self.change(format!("mesh.pair_join {device}"), |m| {
            if let Some(p) = m.peers.iter_mut().find(|p| p.id == device) {
                p.paired = true;
            }
        })
    }

    fn unpair(&mut self, device: &str) -> PortResult<()> {
        self.change(format!("mesh.unpair {device}"), |m| {
            m.peers.retain(|p| p.id != device);
        })
    }

    fn set_grants(&mut self, device: &str, grants: Vec<Grant>) -> PortResult<()> {
        self.change(format!("mesh.set_grants {device} {grants:?}"), |m| {
            if let Some(p) = m.peers.iter_mut().find(|p| p.id == device) {
                p.set_grants(grants);
            }
        })
    }
}
