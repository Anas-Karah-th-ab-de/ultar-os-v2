//! Text form of records for `ulog` and for exports.
//!
//! One record per line, fields separated by tabs:
//!
//! ```text
//! seq  time(UTC)  source  name  level  key  field=value …  [kernel text]
//! ```
//!
//! Everything except the kernel text is made of validated identifiers and
//! numbers, so a line can never contain a tab or a line break from data.

use crate::record::is_unsafe_char;
use ultar_bus::messages::log::{FieldValue, LogRecord, SourceKind};
use ultar_service::log::Level;

/// `2026-11-23T01:02:03.456Z` for milliseconds since 1970 UTC.
pub fn format_time(ms: u64) -> String {
    let (seconds, millis) = (ms / 1000, ms % 1000);
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // Civil date from days since 1970-01-01 (proleptic Gregorian).
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Name of a source kind.
pub const fn source_name(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Service => "service",
        SourceKind::Kernel => "kernel",
        SourceKind::Internal => "internal",
    }
}

/// Name of a level number (`?` for numbers outside the range).
pub fn level_name(level: u8) -> &'static str {
    Level::from_number(level).map_or("?", Level::name)
}

fn value_text(value: &FieldValue) -> String {
    match value {
        FieldValue::Count(n) => n.to_string(),
        FieldValue::Signed(n) => n.to_string(),
        FieldValue::Flag(flag) => flag.to_string(),
        FieldValue::Word(text) | FieldValue::Id(text) => text.clone(),
    }
}

/// The line of `record`, without line feed.
pub fn format_record(record: &LogRecord) -> String {
    let fields: Vec<String> = record
        .fields
        .iter()
        .map(|(name, value)| format!("{name}={}", value_text(value)))
        .collect();
    let mut line = format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        record.seq,
        format_time(record.time_ms),
        source_name(record.source),
        record.service,
        level_name(record.level),
        record.key,
        fields.join(" ")
    );
    if let Some(text) = &record.text {
        line.push('\t');
        // Kernel text was cleaned of control characters when stored; a
        // record from elsewhere is cleaned again here.
        line.extend(
            text.chars()
                .map(|c| if is_unsafe_char(c) { ' ' } else { c }),
        );
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_iso_utc() {
        assert_eq!(format_time(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_time(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(format_time(1_800_000_000_123), "2027-01-15T08:00:00.123Z");
        assert_eq!(format_time(1_795_395_723_456), "2026-11-23T01:02:03.456Z");
        assert_eq!(format_time(4_102_444_799_999), "2099-12-31T23:59:59.999Z");
    }

    #[test]
    fn records_become_one_tab_separated_line() {
        let record = LogRecord {
            seq: 12,
            time_ms: 0,
            source: SourceKind::Service,
            service: "system.devd".into(),
            level: 3,
            key: "devd.driver.failed".into(),
            fields: vec![
                ("n".into(), FieldValue::Count(2)),
                ("code".into(), FieldValue::Signed(-5)),
                ("ok".into(), FieldValue::Flag(false)),
                ("device".into(), FieldValue::Id("pci:01:00.0".into())),
            ],
            text: None,
        };
        assert_eq!(
            format_record(&record),
            "12\t1970-01-01T00:00:00.000Z\tservice\tsystem.devd\twarn\tdevd.driver.failed\tn=2 code=-5 ok=false device=pci:01:00.0"
        );
        let kernel = LogRecord {
            source: SourceKind::Kernel,
            service: "kernel".into(),
            level: 9,
            key: "kernel.line".into(),
            fields: vec![],
            text: Some("ready\tnow\n".into()),
            ..record
        };
        let line = format_record(&kernel);
        assert!(line.ends_with("\tkernel\tkernel\t?\tkernel.line\t\tready now "));
        assert_eq!(line.lines().count(), 1);
        assert_eq!(source_name(SourceKind::Internal), "internal");
    }
}
