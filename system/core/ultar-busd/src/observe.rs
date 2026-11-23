//! Turns broker events into log records.

use ultar_bus::{BrokerEvent, BrokerObserver, ErrorCode};
use ultar_service::log::{Level, Logger, Record};

/// A fixed word for every error code (records carry no free text).
pub fn code_word(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::PermissionDenied => "permission_denied",
        ErrorCode::UnknownService => "unknown_service",
        ErrorCode::UnknownMethod => "unknown_method",
        ErrorCode::ServiceBusy => "service_busy",
        ErrorCode::ServiceGone => "service_gone",
        ErrorCode::SlowConsumer => "slow_consumer",
        ErrorCode::FrameTooLarge => "frame_too_large",
        ErrorCode::Malformed => "malformed",
        ErrorCode::ProtocolViolation => "protocol_violation",
        ErrorCode::NameTaken => "name_taken",
        ErrorCode::InvalidName => "invalid_name",
        ErrorCode::AuthFailed => "auth_failed",
        ErrorCode::LimitExceeded => "limit_exceeded",
        ErrorCode::ShuttingDown => "shutting_down",
        ErrorCode::VersionMismatch => "version_mismatch",
        ErrorCode::NoReply => "no_reply",
        ErrorCode::BadPayload => "bad_payload",
        ErrorCode::Service(_) => "service_error",
        _ => "other",
    }
}

/// The record for `event`. Operations that fail in normal operation
/// without anybody doing anything wrong (a service that is not running
/// yet) are `Info`; refusals by policy or limit are `Warn`.
pub fn record(event: &BrokerEvent<'_>) -> Record {
    match event {
        BrokerEvent::Refused {
            identity,
            operation,
            code,
        } => {
            let (level, key) = match code {
                ErrorCode::PermissionDenied => (Level::Warn, "bus.refused"),
                ErrorCode::LimitExceeded | ErrorCode::ServiceBusy => (Level::Warn, "bus.limit_hit"),
                _ => (Level::Info, "bus.failed"),
            };
            Record::new(level, key)
                .word("op", operation.name())
                .word("code", code_word(*code))
                .id_or_invalid("peer", identity)
        }
        BrokerEvent::SlowConsumer { identity } => {
            Record::new(Level::Warn, "bus.slow_consumer_dropped").id_or_invalid("peer", identity)
        }
        BrokerEvent::AuthFailed { listener } => {
            Record::new(Level::Warn, "bus.auth_failed").count("listener", *listener as u64)
        }
        BrokerEvent::ConnectionLimit { listener } => Record::new(Level::Warn, "bus.limit_hit")
            .word("op", "connect")
            .word("code", "limit_exceeded")
            .count("listener", *listener as u64),
        BrokerEvent::ProtocolError { identity, code } => {
            Record::new(Level::Warn, "bus.protocol_error")
                .word("code", code_word(*code))
                .id_or_invalid("peer", identity)
        }
    }
}

/// Logs every broker event. `Logger::emit` never blocks, which is what
/// [`BrokerObserver`] demands.
pub struct LogObserver(pub Logger);

impl BrokerObserver for LogObserver {
    fn event(&self, event: &BrokerEvent<'_>) {
        self.0.emit(record(event));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::Operation;
    use ultar_service::log::{wire, Value};

    #[test]
    fn every_event_becomes_an_encodable_record() {
        let events = [
            BrokerEvent::Refused {
                identity: "app.phone",
                operation: Operation::Publish,
                code: ErrorCode::PermissionDenied,
            },
            BrokerEvent::Refused {
                identity: "app.phone",
                operation: Operation::Subscribe,
                code: ErrorCode::LimitExceeded,
            },
            BrokerEvent::Refused {
                identity: "app.phone",
                operation: Operation::Call,
                code: ErrorCode::UnknownService,
            },
            BrokerEvent::SlowConsumer { identity: "ui" },
            BrokerEvent::AuthFailed { listener: 2 },
            BrokerEvent::ConnectionLimit { listener: 1 },
            BrokerEvent::ProtocolError {
                identity: "ui",
                code: ErrorCode::FrameTooLarge,
            },
        ];
        let keys: Vec<&str> = events
            .iter()
            .map(|event| {
                let record = record(event);
                assert!(wire::encode_frame(&record).is_ok(), "{event:?}");
                record.key()
            })
            .collect();
        assert_eq!(
            keys,
            [
                "bus.refused",
                "bus.limit_hit",
                "bus.failed",
                "bus.slow_consumer_dropped",
                "bus.auth_failed",
                "bus.limit_hit",
                "bus.protocol_error"
            ]
        );
        assert_eq!(record(&events[0]).level(), Level::Warn);
        assert_eq!(record(&events[2]).level(), Level::Info);
    }

    #[test]
    fn an_identity_that_is_no_identifier_is_not_logged_verbatim() {
        let long = "a".repeat(200);
        let record = record(&BrokerEvent::SlowConsumer { identity: &long });
        assert_eq!(record.fields()[0].1, Value::Word("invalid"));
    }

    #[test]
    fn code_words_are_keys() {
        for code in [
            ErrorCode::PermissionDenied,
            ErrorCode::UnknownService,
            ErrorCode::UnknownMethod,
            ErrorCode::ServiceBusy,
            ErrorCode::ServiceGone,
            ErrorCode::SlowConsumer,
            ErrorCode::FrameTooLarge,
            ErrorCode::Malformed,
            ErrorCode::ProtocolViolation,
            ErrorCode::NameTaken,
            ErrorCode::InvalidName,
            ErrorCode::AuthFailed,
            ErrorCode::LimitExceeded,
            ErrorCode::ShuttingDown,
            ErrorCode::VersionMismatch,
            ErrorCode::NoReply,
            ErrorCode::BadPayload,
            ErrorCode::Service(9),
        ] {
            assert!(ultar_service::log::record::is_key(code_word(code)));
        }
    }
}
