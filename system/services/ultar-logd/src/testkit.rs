//! Builders shared by the unit tests of this crate.

use crate::record::{SourceKind, Stored};
use ultar_service::log::{Level, OwnedValue};

/// A service record at position `seq`.
pub(crate) fn sample(seq: u64) -> Stored {
    Stored {
        seq,
        time_ms: 1_800_000_000_000 + seq,
        kind: SourceKind::Service,
        service: "system.devd".into(),
        level: Level::Warn,
        key: "devd.driver.failed".into(),
        fields: vec![
            ("n".into(), OwnedValue::Count(seq)),
            ("code".into(), OwnedValue::Signed(-5)),
            ("ok".into(), OwnedValue::Flag(false)),
            ("phase".into(), OwnedValue::Word("probe".into())),
            ("device".into(), OwnedValue::Id("pci:01:00.0".into())),
        ],
        text: String::new(),
    }
}
