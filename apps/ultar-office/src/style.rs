//! Zellformatierung und Zahlen-/Datumsformate der Tabellenkalkulation.
//!
//! Eine Zelle kann neben ihrem Rohinhalt eine Formatierung tragen: fett/kursiv, Schriftfarbe,
//! Füllfarbe, Ausrichtung, Zeilenumbruch und ein Excel-Zahlenformat (`0.00`, `0%`, `#,##0.00`,
//! `yyyy-mm-dd` …). Das Format wirkt nur auf die Anzeige; der Wert bleibt eine Zahl. Daten im
//! Excel-Format sind Serienzahlen (0 = 30.12.1899, gregorianisch).

/// Horizontal alignment of a cell's content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Align {
    /// Left for text, right for numbers (only the app decides; stored as "general").
    #[default]
    General,
    /// At the start edge (right in a right-to-left sheet).
    Start,
    /// Centred.
    Center,
    /// At the end edge.
    End,
}

/// The formatting of one cell. Colours are `0xRRGGBB`; `None` means the theme's default.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct CellStyle {
    /// Bold text.
    pub bold: bool,
    /// Italic text.
    pub italic: bool,
    /// Text colour.
    pub color: Option<u32>,
    /// Fill (background) colour.
    pub fill: Option<u32>,
    /// Horizontal alignment.
    pub align: Align,
    /// Wrap text onto several lines.
    pub wrap: bool,
    /// Excel number format code; empty means "General".
    pub num_fmt: String,
}

impl CellStyle {
    /// The default formatting (General, black on white).
    pub fn plain() -> CellStyle {
        CellStyle::default()
    }

    /// `true` when nothing is set (the cell uses the sheet's defaults).
    pub fn is_default(&self) -> bool {
        *self == CellStyle::default()
    }
}

/// Counts the places after the decimal point in a format body (the `0` and `#` after `.`).
fn decimals(fmt: &str) -> usize {
    fmt.split_once('.')
        .map(|(_, d)| d.chars().filter(|c| *c == '0' || *c == '#').count())
        .unwrap_or(0)
}

/// Is this a date format (`yyyy`, `dd`/`mm` combinations, or a month name)?
fn is_date(fmt: &str) -> bool {
    fmt.contains("yyyy")
        || fmt.contains("yy")
        || ((fmt.contains("dd") || fmt.contains("mmmm") || fmt.contains("mmm")) && fmt.contains("mm"))
}

/// The currency symbol (if any) held as a literal in front of a number format.
fn currency_prefix(fmt: &str) -> String {
    let prefix: String = fmt
        .chars()
        .take_while(|c| !matches!(*c, '0' | '#' | '.' | ',' | '%' | ';' | '['))
        .collect();
    prefix.trim().to_string()
}

/// Separates the integer part of a non-negative number into groups of three (`1234567` → `1,234,567`).
fn group_thousands(n: f64, dec: usize) -> String {
    let neg = n < 0.0;
    let a = n.abs();
    let whole = a.trunc();
    let frac = a - whole;
    let mut int: Vec<char> = format!("{}", whole as u64).chars().collect();
    let mut out = String::new();
    let mut count = 0;
    while let Some(c) = int.pop() {
        if count == 3 {
            out.push(',');
            count = 0;
        }
        out.push(c);
        count += 1;
    }
    let mut s: String = out.chars().rev().collect();
    if dec > 0 {
        s.push_str(&format!("{:.*}", dec, frac).trim_start_matches('0'));
    }
    if neg {
        format!("-{s}")
    } else {
        s
    }
}

/// Formats a number with an Excel number format code.
pub fn format_number(n: f64, fmt: &str) -> String {
    let f = fmt.trim();
    if f.is_empty() || f.eq_ignore_ascii_case("general") || f == "0" {
        return crate::sheet::fmt_num(n);
    }
    if f.contains('%') {
        let dec = decimals(f);
        return format!("{:.*}%", dec, n * 100.0);
    }
    if is_date(f) {
        return match excel_date(n) {
            Some((y, m, d)) => format_date(f, y, m, d),
            None => crate::sheet::fmt_num(n),
        };
    }
    let dec = decimals(f);
    let prefix = currency_prefix(f);
    let body = f.trim_start_matches(prefix.trim());
    let grouped = body.contains(',');
    let num = if grouped {
        group_thousands(n, dec)
    } else if dec > 0 {
        format!("{:.*}", dec, n)
    } else {
        format!("{}", n.round())
    };
    format!("{prefix}{num}")
}

/// Excel serial date → Gregorian (year, month, day); serial 0 is 1899-12-30 (the day before
/// the fake 1900 leap day, so 1900-01-01 is serial 1 and 1900-03-01 is serial 61).
pub fn excel_date(serial: f64) -> Option<(i32, u32, u32)> {
    if !serial.is_finite() || serial < 0.0 {
        return None;
    }
    // Serial 60 is the fictional 1900-02-29; it is not a real date.
    let days = if serial < 60.0 {
        serial.floor() as i64 - 25_568
    } else {
        serial.floor() as i64 - 25_569
    };
    days_to_ymd(days)
}

/// Gregorian (year, month, day) → Excel serial date.
pub fn to_excel_date(y: i32, m: u32, d: u32) -> f64 {
    let days = ymd_to_days(y, m, d);
    let base = days + 25_568;
    if base > 59 {
        (base + 1) as f64
    } else {
        base as f64
    }
}

/// Days since 1899-12-30 for a Gregorian date (Howard Hinnant's civil algorithm).
fn ymd_to_days(y: i32, m: u32, d: u32) -> i64 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719_468
}

fn days_to_ymd(days: i64) -> Option<(i32, u32, u32)> {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    Some((y as i32, m, d))
}

/// A date formatted from a format code (`yyyy`, `yy`, `mm`, `m`, `dd`, `d`, `mmm`, `mmmm`).
fn format_date(fmt: &str, y: i32, m: u32, d: u32) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let mut out = fmt.to_string();
    out = out.replace("yyyy", &format!("{y:04}"));
    out = out.replace("yy", &format!("{:02}", y.rem_euclid(100)));
    out = out.replace("mmmm", MONTHS[m as usize - 1]);
    out = out.replace("mmm", &MONTHS[m as usize - 1][..3]);
    out = out.replace("mm", &format!("{m:02}"));
    out = out.replace('m', &m.to_string());
    out = out.replace("dd", &format!("{d:02}"));
    out = out.replace('d', &d.to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zahlenformate() {
        assert_eq!(format_number(1234.5, ""), "1234.5");
        assert_eq!(format_number(1234.0, "0"), "1234");
        assert_eq!(format_number(1234.567, "0.00"), "1234.57");
        assert_eq!(format_number(0.125, "0%"), "12%");
        assert_eq!(format_number(0.125, "0.00%"), "12.50%");
        assert_eq!(format_number(1234.5, "€#,##0.00"), "€1,234.50");
        assert_eq!(format_number(-1234.5, "#,##0.00"), "-1,234.50");
    }

    #[test]
    fn datumsformate() {
        assert_eq!(format_number(1.0, "yyyy-mm-dd"), "1900-01-01");
        assert_eq!(format_number(61.0, "yyyy-mm-dd"), "1900-03-01");
        assert_eq!(format_number(45292.0, "dd.mm.yyyy"), "01.01.2024");
        assert_eq!(format_number(45292.0, "yyyy/mm/dd"), "2024/01/01");
        assert_eq!(format_number(1.0, "d.m.yyyy"), "1.1.1900");
    }

    #[test]
    fn datum_serie_hin_und_zurueck() {
        assert_eq!(excel_date(1.0), Some((1900, 1, 1)));
        assert_eq!(excel_date(60.0), Some((1900, 2, 28)));
        assert_eq!(to_excel_date(1900, 1, 1), 1.0);
        assert_eq!(to_excel_date(2024, 1, 1), 45292.0);
        // round trip over a range of real dates
        for (y, m, d) in [(1970, 1, 1), (2000, 2, 29), (2024, 12, 31), (2100, 6, 15)] {
            let s = to_excel_date(y, m, d);
            assert_eq!(excel_date(s), Some((y, m, d)), "{y}-{m}-{d}");
        }
    }
}