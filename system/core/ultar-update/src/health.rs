//! Health checks after the first start of a new release, and the free-space probe.

use crate::error::UpdateError;
use std::path::Path;
use std::time::Duration;
use ultar_bus::{BusError, Client, ErrorCode};

/// Decides whether the freshly started system works.
pub trait HealthCheck {
    /// `Ok` when the system is healthy; [`UpdateError::Unhealthy`] otherwise.
    fn check(&self) -> Result<(), UpdateError>;
}

/// Health = every required service answers on the bus.
///
/// A service counts as alive when a request reaches it: it answers, or it refuses the method
/// itself. No broker answer proves that (unknown service, permission denied, time-out all
/// count as "not alive" – the check fails closed), so the bus policy must allow the updater
/// to call `<service>.ping` for every listed service.
pub struct BusHealth {
    client: Client,
    services: Vec<String>,
    timeout: Duration,
}

/// Method name used to probe a service.
pub const PING_METHOD: &str = "ping";

impl BusHealth {
    /// A check of `services` through `client`, waiting at most `timeout` per service.
    pub fn new(client: Client, services: Vec<String>, timeout: Duration) -> Self {
        Self {
            client,
            services,
            timeout,
        }
    }

    /// True when the answer proves that the service's own code handled the request.
    pub fn proves_alive(answer: &Result<(), BusError>) -> bool {
        match answer {
            Ok(()) => true,
            Err(BusError::Remote(code)) => matches!(
                code,
                ErrorCode::UnknownMethod
                    | ErrorCode::BadPayload
                    | ErrorCode::VersionMismatch
                    | ErrorCode::NoReply
                    | ErrorCode::Service(_)
            ),
            // An answer arrived but is not the empty reply: the service handled the request.
            Err(BusError::Malformed | BusError::MessageVersion { .. }) => true,
            Err(_) => false,
        }
    }
}

impl HealthCheck for BusHealth {
    fn check(&self) -> Result<(), UpdateError> {
        for service in &self.services {
            let answer = self
                .client
                .request::<(), ()>(service, PING_METHOD, &(), self.timeout);
            if !Self::proves_alive(&answer) {
                return Err(UpdateError::Unhealthy);
            }
        }
        Ok(())
    }
}

/// Tells how much space is free.
pub trait SpaceProbe: Send + Sync {
    /// Free bytes available to the updater on the file system that holds `dir`.
    fn free_bytes(&self, dir: &Path) -> Result<u64, UpdateError>;
}

/// The real probe (`statvfs`).
#[derive(Debug, Default, Clone, Copy)]
pub struct FsSpace;

impl SpaceProbe for FsSpace {
    fn free_bytes(&self, dir: &Path) -> Result<u64, UpdateError> {
        let stat = rustix::fs::statvfs(dir).map_err(|e| UpdateError::Io(e.into()))?;
        Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
    }
}

/// A probe with a fixed answer (tests, simulations).
#[derive(Debug, Clone, Copy)]
pub struct FixedSpace(pub u64);

impl SpaceProbe for FixedSpace {
    fn free_bytes(&self, _dir: &Path) -> Result<u64, UpdateError> {
        Ok(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_answers_from_the_service_itself_prove_life() {
        for code in [
            ErrorCode::UnknownMethod,
            ErrorCode::BadPayload,
            ErrorCode::VersionMismatch,
            ErrorCode::NoReply,
            ErrorCode::Service(3),
        ] {
            assert!(
                BusHealth::proves_alive(&Err(BusError::Remote(code))),
                "{code}"
            );
        }
        assert!(BusHealth::proves_alive(&Ok(())));
        for code in [
            ErrorCode::UnknownService,
            ErrorCode::PermissionDenied,
            ErrorCode::ServiceGone,
            ErrorCode::ServiceBusy,
            ErrorCode::ShuttingDown,
        ] {
            assert!(
                !BusHealth::proves_alive(&Err(BusError::Remote(code))),
                "{code}"
            );
        }
        assert!(BusHealth::proves_alive(&Err(BusError::Malformed)));
        assert!(!BusHealth::proves_alive(&Err(BusError::Timeout)));
        assert!(!BusHealth::proves_alive(&Err(BusError::NotConnected)));
    }

    #[test]
    fn real_probe_reports_space_and_fails_for_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        assert!(FsSpace.free_bytes(dir.path()).unwrap() > 0);
        assert!(FsSpace.free_bytes(&dir.path().join("missing")).is_err());
        assert_eq!(FixedSpace(7).free_bytes(dir.path()).unwrap(), 7);
    }
}
