//! What the broker may cost, checked before it starts (review round 3,
//! R3-B0/R3-B1).
//!
//! Every listener has its own connection bound (`--max-connections`,
//! default [`DEFAULT_CONNECTIONS`]). An admitted connection costs one
//! descriptor and two threads, a connection in the handshake one
//! descriptor and no thread (see `ultar_bus::BrokerBuilder::needs`). The
//! sum over all listeners plus a reserve for the process itself must stay
//! below the broker's own limits; otherwise the peers of one listener could
//! use up what another listener (the administrator's) needs, and the start
//! is refused.
//!
//! There is no address-space limit: with two threads per connection the C
//! library's per-thread memory pools reserve address space far beyond what
//! is used, and a refused reservation ended the broker (R3-B0). The memory
//! bound is the group's `memory.max` in the service file.

use std::fmt;
use ultar_bus::{BrokerConfig, Needs};
use ultar_service::Limits;

/// Connections per listener unless `--max-connections` says otherwise.
pub const DEFAULT_CONNECTIONS: usize = 64;
/// Bytes that may wait for one client (one frame always fits).
pub const QUEUE_BYTES: usize = 1 << 20;
/// Descriptors the process needs besides the listeners and connections:
/// standard streams, signal descriptor, log socket, files read at start
/// and on reload.
pub const RESERVE_DESCRIPTORS: usize = 32;
/// Threads besides the gates and connections: main, work, signal and
/// logger threads, with room.
pub const RESERVE_THREADS: usize = 16;
/// Descriptor limit of the broker.
pub const OPEN_FILES: u64 = 1024;
/// Thread limit of the broker (`RLIMIT_NPROC`; the service file's
/// `pids.max` must be at least as large).
pub const PROCESSES: u64 = 1024;

/// Limits of the broker.
pub const LIMITS: Limits = Limits {
    address_space: None,
    open_files: Some(OPEN_FILES),
    processes: Some(PROCESSES),
    core_dumps: false,
};

/// The broker's limits and timeouts.
pub fn config() -> BrokerConfig {
    BrokerConfig {
        max_connections: DEFAULT_CONNECTIONS,
        queue_bytes: QUEUE_BYTES,
        ..BrokerConfig::default()
    }
}

/// A resource the configuration needs more of than the process may have.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shortfall {
    /// `descriptors` or `threads`.
    pub what: &'static str,
    /// Needed, reserve included.
    pub need: usize,
    /// The limit.
    pub limit: u64,
}

impl fmt::Display for Shortfall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} > {}", self.what, self.need, self.limit)
    }
}

/// Descriptors and threads needed, reserve included.
pub fn with_reserve(needs: Needs) -> Needs {
    Needs {
        descriptors: needs.descriptors.saturating_add(RESERVE_DESCRIPTORS),
        threads: needs.threads.saturating_add(RESERVE_THREADS),
    }
}

/// Checks `needs` against the broker's limits.
pub fn check(needs: Needs) -> Result<(), Shortfall> {
    let total = with_reserve(needs);
    let fits = |need: usize, limit: u64| u64::try_from(need).is_ok_and(|need| need <= limit);
    if !fits(total.descriptors, OPEN_FILES) {
        return Err(Shortfall {
            what: "descriptors",
            need: total.descriptors,
            limit: OPEN_FILES,
        });
    }
    if !fits(total.threads, PROCESSES) {
        return Err(Shortfall {
            what: "threads",
            need: total.threads,
            limit: PROCESSES,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_defaults_fit_with_room() {
        // Four listeners with the default bound, as the service file has.
        let per = DEFAULT_CONNECTIONS + config().max_handshakes + 1;
        let needs = Needs {
            descriptors: 4 * per,
            threads: 4 * (2 * DEFAULT_CONNECTIONS + 1),
        };
        assert_eq!(check(needs), Ok(()));
    }

    #[test]
    fn a_configuration_beyond_the_limits_is_refused() {
        let too_many = Needs {
            descriptors: 1000,
            threads: 10,
        };
        let shortfall = check(too_many).unwrap_err();
        assert_eq!(shortfall.what, "descriptors");
        assert_eq!(shortfall.to_string(), "descriptors 1032 > 1024");
        let threads = Needs {
            descriptors: 10,
            threads: 1009,
        };
        assert_eq!(check(threads).unwrap_err().what, "threads");
        assert!(check(Needs {
            descriptors: 992,
            threads: 1008
        })
        .is_ok());
    }
}
