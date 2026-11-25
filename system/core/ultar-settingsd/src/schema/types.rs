//! Value types of settings and their validation.

use super::key::is_valid_segment;
use crate::value::Value;
use std::fmt;

/// Why a value is not acceptable for a setting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueError {
    /// The value has another type than the declared one.
    TypeMismatch {
        /// Declared type.
        expected: &'static str,
        /// Type of the offered value.
        found: &'static str,
    },
    /// A number lies outside the declared range or is not finite.
    OutOfRange,
    /// A number does not lie on the declared step.
    OffStep,
    /// The text is not one of the declared choices.
    UnknownChoice,
    /// The text does not follow the declared format.
    BadFormat {
        /// Name of the format.
        format: &'static str,
    },
    /// The list has more entries than declared.
    TooLong,
    /// The list names an entry twice.
    Duplicate,
}

impl ValueError {
    /// Key of the user-facing message.
    pub const fn message_key(&self) -> &'static str {
        match self {
            ValueError::TypeMismatch { .. } => "settingsd.error.type_mismatch",
            ValueError::OutOfRange | ValueError::OffStep => "settingsd.error.out_of_range",
            ValueError::UnknownChoice
            | ValueError::BadFormat { .. }
            | ValueError::TooLong
            | ValueError::Duplicate => "settingsd.error.invalid_value",
        }
    }
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueError::TypeMismatch { expected, found } => {
                write!(f, "expected {expected}, found {found}")
            }
            ValueError::OutOfRange => f.write_str("number out of range"),
            ValueError::OffStep => f.write_str("number not on the declared step"),
            ValueError::UnknownChoice => f.write_str("not one of the declared choices"),
            ValueError::BadFormat { format } => write!(f, "text is not a valid {format}"),
            ValueError::TooLong => f.write_str("list has too many entries"),
            ValueError::Duplicate => f.write_str("list names an entry twice"),
        }
    }
}

impl std::error::Error for ValueError {}

/// Format a text setting must follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFormat {
    /// Any text without control characters, up to a number of characters.
    Free {
        /// Largest accepted number of characters.
        max_chars: usize,
    },
    /// Time of day `HH:MM` (24 hours, digits 0–9).
    Time,
    /// `auto` or `<width>x<height>` in pixels.
    Resolution,
    /// Identifier of a display; empty means "choose automatically".
    DisplayId,
    /// Lower-case identifier (`a-z`, `0-9`, `_`, `-`).
    Name,
    /// An app id: one or two lower-case segments (`a-z`, `0-9`, `-`) joined by a dot,
    /// as the launcher names an installed app (`browser`, `store.web`).
    AppId,
    /// Currency code of three capital letters.
    Currency,
    /// Time zone name such as `Asia/Damascus`, or `UTC`.
    TimeZone,
    /// Host name with optional port.
    Host,
    /// Empty, or an absolute file path.
    Path,
}

fn digits(text: &str, len: usize) -> Option<u32> {
    (text.len() == len && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn is_time(text: &str) -> bool {
    text.split_once(':').is_some_and(|(hours, minutes)| {
        digits(hours, 2).is_some_and(|h| h < 24) && digits(minutes, 2).is_some_and(|m| m < 60)
    })
}

fn is_resolution(text: &str) -> bool {
    let side = |part: &str| {
        (1..=5).contains(&part.len())
            && !part.starts_with('0')
            && digits(part, part.len()).is_some_and(|n| n <= 65_535)
    };
    text == "auto"
        || text
            .split_once('x')
            .is_some_and(|(width, height)| side(width) && side(height))
}

fn is_time_zone(text: &str) -> bool {
    let part = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'+' || b == b'-')
    };
    text == "UTC" || (text.len() <= 64 && text.contains('/') && text.split('/').all(part))
}

fn is_host(text: &str) -> bool {
    let (host, port) = match text.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (text, None),
    };
    let label = |label: &str| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    let port_ok = port.is_none_or(|port| {
        (1..=5).contains(&port.len())
            && digits(port, port.len()).is_some_and(|n| (1..=65_535).contains(&n))
    });
    port_ok && !host.is_empty() && host.len() <= 253 && host.split('.').all(label)
}

/// Whether `text` is an app id as the launcher names one: 1–28 bytes, one or two segments
/// joined by a dot, each starting with a lower-case letter, `a-z 0-9 -`, no `--`, not ending
/// with `-` (the rules of `ultar-launcher`'s `AppId`).
fn is_app_id(text: &str) -> bool {
    let segment = |s: &str| {
        let b = s.as_bytes();
        b.first().is_some_and(u8::is_ascii_lowercase)
            && b.last().is_some_and(|c| *c != b'-')
            && !s.contains("--")
            && b.iter()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
    };
    !text.is_empty()
        && text.len() <= 28
        && text.split('.').count() <= 2
        && text.split('.').all(segment)
}

impl TextFormat {
    /// Stable name of the format (also sent to clients).
    pub const fn name(&self) -> &'static str {
        match self {
            TextFormat::Free { .. } => "text",
            TextFormat::Time => "time",
            TextFormat::Resolution => "resolution",
            TextFormat::DisplayId => "display",
            TextFormat::Name => "name",
            TextFormat::AppId => "app_id",
            TextFormat::Currency => "currency",
            TextFormat::TimeZone => "time_zone",
            TextFormat::Host => "host",
            TextFormat::Path => "path",
        }
    }

    /// Whether `text` follows the format.
    pub fn accepts(&self, text: &str) -> bool {
        let clean = || !text.chars().any(char::is_control);
        match self {
            TextFormat::Free { max_chars } => text.chars().count() <= *max_chars && clean(),
            TextFormat::Time => is_time(text),
            TextFormat::Resolution => is_resolution(text),
            TextFormat::DisplayId => text.is_empty() || is_valid_segment(text),
            TextFormat::Name => is_valid_segment(text),
            TextFormat::AppId => is_app_id(text),
            TextFormat::Currency => {
                text.len() == 3 && text.bytes().all(|byte| byte.is_ascii_uppercase())
            }
            TextFormat::TimeZone => is_time_zone(text),
            TextFormat::Host => is_host(text),
            TextFormat::Path => {
                text.is_empty() || (text.starts_with('/') && text.len() <= 4096 && clean())
            }
        }
    }

    fn check(&self, text: &str) -> Result<(), ValueError> {
        if self.accepts(text) {
            Ok(())
        } else {
            Err(ValueError::BadFormat {
                format: self.name(),
            })
        }
    }
}

/// Declared type of a setting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ValueType {
    /// On/off switch.
    Bool,
    /// Whole number in `min..=max` on multiples of `step` counted from `min`.
    Int {
        /// Smallest accepted value.
        min: i64,
        /// Largest accepted value.
        max: i64,
        /// Distance between accepted values (at least 1).
        step: i64,
    },
    /// Fractional number in `min..=max`; `step > 0` restricts it to steps.
    Float {
        /// Smallest accepted value.
        min: f64,
        /// Largest accepted value.
        max: f64,
        /// Distance between accepted values, `0.0` for any value.
        step: f64,
    },
    /// One of a fixed list of identifiers.
    Enum(&'static [&'static str]),
    /// Text in a format.
    Text(TextFormat),
    /// Ordered list of distinct texts in a format.
    List {
        /// Format of each entry.
        item: TextFormat,
        /// Largest accepted number of entries.
        max_len: usize,
    },
}

fn on_float_step(value: f64, min: f64, step: f64) -> bool {
    if step <= 0.0 {
        return true;
    }
    let steps = (value - min) / step;
    (steps - steps.round()).abs() <= 1e-6
}

impl ValueType {
    /// Short stable name of the type.
    pub const fn name(&self) -> &'static str {
        match self {
            ValueType::Bool => "bool",
            ValueType::Int { .. } => "int",
            ValueType::Float { .. } => "float",
            ValueType::Enum(_) => "enum",
            ValueType::Text(_) => "string",
            ValueType::List { .. } => "list",
        }
    }

    fn mismatch(&self, value: &Value) -> ValueError {
        ValueError::TypeMismatch {
            expected: self.name(),
            found: value.type_name(),
        }
    }

    /// Checks a value against the type.
    pub fn validate(&self, value: &Value) -> Result<(), ValueError> {
        match (self, value) {
            (ValueType::Bool, Value::Bool(_)) => Ok(()),
            (ValueType::Int { min, max, step }, Value::Int(number)) => {
                if number < min || number > max {
                    Err(ValueError::OutOfRange)
                } else if number.abs_diff(*min) % step.unsigned_abs().max(1) != 0 {
                    Err(ValueError::OffStep)
                } else {
                    Ok(())
                }
            }
            (ValueType::Float { min, max, step }, Value::Float(number)) => {
                if !number.is_finite() || number < min || number > max {
                    Err(ValueError::OutOfRange)
                } else if !on_float_step(*number, *min, *step) {
                    Err(ValueError::OffStep)
                } else {
                    Ok(())
                }
            }
            (ValueType::Enum(choices), Value::Str(text)) => choices
                .contains(&text.as_str())
                .then_some(())
                .ok_or(ValueError::UnknownChoice),
            (ValueType::Text(format), Value::Str(text)) => format.check(text),
            (ValueType::List { item, max_len }, Value::List(items)) => {
                if items.len() > *max_len {
                    return Err(ValueError::TooLong);
                }
                for (index, entry) in items.iter().enumerate() {
                    if entry.is_empty() {
                        return Err(ValueError::BadFormat {
                            format: item.name(),
                        });
                    }
                    item.check(entry)?;
                    if items[..index].contains(entry) {
                        return Err(ValueError::Duplicate);
                    }
                }
                Ok(())
            }
            _ => Err(self.mismatch(value)),
        }
    }

    /// Adapts a value of a compatible type (a whole number offered for a
    /// fractional setting) and validates it.
    pub fn accept(&self, value: Value) -> Result<Value, ValueError> {
        let value = match (self, value) {
            // Whole numbers are exact in f64 far beyond every declared range.
            (ValueType::Float { .. }, Value::Int(number)) => Value::Float(number as f64),
            (_, value) => value,
        };
        self.validate(&value)?;
        Ok(value)
    }

    /// Parses text typed by a person (command line) into a value of the type.
    pub fn parse_text(&self, text: &str) -> Result<Value, ValueError> {
        let wrong = || ValueError::TypeMismatch {
            expected: self.name(),
            found: "string",
        };
        let value = match self {
            ValueType::Bool => match text {
                "true" | "on" | "yes" | "1" => Value::Bool(true),
                "false" | "off" | "no" | "0" => Value::Bool(false),
                _ => return Err(wrong()),
            },
            ValueType::Int { .. } => Value::Int(text.parse().map_err(|_| wrong())?),
            ValueType::Float { .. } => Value::Float(text.parse().map_err(|_| wrong())?),
            ValueType::Enum(_) | ValueType::Text(_) => Value::text(text),
            ValueType::List { .. } => Value::List(
                text.split(',')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(str::to_owned)
                    .collect(),
            ),
        };
        self.validate(&value)?;
        Ok(value)
    }
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
