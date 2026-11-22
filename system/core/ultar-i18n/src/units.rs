//! Formatting of sizes and percentages through the catalog keys `unit.*`.

use alloc::string::String;

use crate::numbers::{format_fixed, format_int, format_uint};
use crate::pattern::Arg;
use crate::translator::Translator;

/// Catalog keys of the size units, smallest first; each step is a factor of 1024.
const SIZE_KEYS: [&str; 5] = [
    "unit.size.b",
    "unit.size.kb",
    "unit.size.mb",
    "unit.size.gb",
    "unit.size.tb",
];

/// Formats a size in bytes with a fitting unit (factor 1024), e.g. `1.5 GB` / `1,5 GB` / `1.5 غيغابايت`.
///
/// Bytes are shown as whole numbers, larger units with one decimal (rounded down).
pub fn format_size(bytes: u64, translator: &Translator) -> String {
    let lang = translator.lang();
    if bytes < 1024 {
        let value = format_uint(bytes, lang);
        return translator.t_args(SIZE_KEYS[0], &[("value", Arg::Str(&value))]);
    }
    let mut unit = 1;
    let mut divisor: u64 = 1024;
    while unit + 1 < SIZE_KEYS.len() && bytes / divisor >= 1024 {
        divisor *= 1024;
        unit += 1;
    }
    let tenths = u128::from(bytes) * 10 / u128::from(divisor);
    let value = format_fixed(i64::try_from(tenths).unwrap_or(i64::MAX), 1, lang);
    translator.t_args(SIZE_KEYS[unit], &[("value", Arg::Str(&value))])
}

/// Formats a percentage, e.g. `85%` (German: `85 %`).
pub fn format_percent(percent: i64, translator: &Translator) -> String {
    let value = format_int(percent, translator.lang());
    translator.t_args("unit.percent", &[("value", Arg::Str(&value))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang;

    #[test]
    fn sizes_pick_a_fitting_unit() {
        let en = Translator::new(Lang::En);
        assert_eq!(format_size(0, &en), "0 B");
        assert_eq!(format_size(1023, &en), "1,023 B");
        assert_eq!(format_size(1024, &en), "1.0 KB");
        assert_eq!(format_size(1536, &en), "1.5 KB");
        assert_eq!(format_size(1_048_576, &en), "1.0 MB");
        assert_eq!(format_size(1_610_612_736, &en), "1.5 GB");
        assert_eq!(format_size(1 << 40, &en), "1.0 TB");
        assert_eq!(format_size(u64::MAX, &en), "16,777,215.9 TB");
    }

    #[test]
    fn sizes_follow_the_language() {
        let de = Translator::new(Lang::De);
        assert_eq!(format_size(1_610_612_736, &de), "1,5 GB");
        let ar = Translator::new(Lang::Ar);
        assert_eq!(format_size(1_610_612_736, &ar), "1.5 غيغابايت");
        assert_eq!(format_size(512, &ar), "512 بايت");
    }

    #[test]
    fn percentages() {
        assert_eq!(format_percent(85, &Translator::new(Lang::Ar)), "85%");
        assert_eq!(format_percent(85, &Translator::new(Lang::De)), "85 %");
        assert_eq!(format_percent(-5, &Translator::new(Lang::En)), "-5%");
    }
}
