//! What a broker tells its host program about refused operations, dropped
//! clients and limits – so that the daemon can log them.

use crate::error::ErrorCode;

/// The kind of operation a client attempted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Operation {
    /// Register a service name.
    Register,
    /// Give a service name back.
    Unregister,
    /// Subscribe to a topic or pattern.
    Subscribe,
    /// Publish an event.
    Publish,
    /// Call a service method.
    Call,
}

impl Operation {
    /// Stable lower-case name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Register => "register",
            Self::Unregister => "unregister",
            Self::Subscribe => "subscribe",
            Self::Publish => "publish",
            Self::Call => "call",
        }
    }
}

/// Something the broker did to protect itself or the policy.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrokerEvent<'a> {
    /// An operation of an authenticated client was refused.
    Refused {
        /// Identity of the client.
        identity: &'a str,
        /// What it tried.
        operation: Operation,
        /// Why it was refused (`PermissionDenied`, `LimitExceeded`, …).
        code: ErrorCode,
    },
    /// A client did not read fast enough and was disconnected.
    SlowConsumer {
        /// Identity of the client.
        identity: &'a str,
    },
    /// A connection could not be mapped to an identity.
    AuthFailed {
        /// Index of the listener (order of the `listen` calls, from 1).
        listener: usize,
    },
    /// A listener is at its connection limit and turned a client away.
    ConnectionLimit {
        /// Index of the listener.
        listener: usize,
    },
    /// A client broke the protocol (bad frame, oversized frame) and was
    /// disconnected.
    ProtocolError {
        /// Identity of the client.
        identity: &'a str,
        /// What was wrong.
        code: ErrorCode,
    },
}

/// Receives [`BrokerEvent`]s.
///
/// Called on broker threads, some of them **while the routing lock is
/// held**: an implementation must return quickly and must never call back
/// into the broker or block (queue the event, count it, hand it to a
/// non-blocking logger).
pub trait BrokerObserver: Send + Sync {
    /// One event.
    fn event(&self, event: &BrokerEvent<'_>);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_names_are_distinct() {
        let names = [
            Operation::Register,
            Operation::Unregister,
            Operation::Subscribe,
            Operation::Publish,
            Operation::Call,
        ]
        .map(Operation::name);
        let unique: std::collections::BTreeSet<&str> = names.iter().copied().collect();
        assert_eq!(unique.len(), names.len());
    }
}
