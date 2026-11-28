//! Number formats of cells: Excel's format codes (`0.00`, `#,##0`, `0%`, `"EUR" 0.00`,
//! `[$€-407]#,##0.00`, `yyyy-mm-dd`, `d mmmm yyyy`, `hh:mm`, up to four sections
//! `positive;negative;zero;text`). Digits are always 0-9 (APP-RULES §10); month and weekday
//! names come from [`Names`] so the app can show them in the language of the user.

use crate::sheet::fmt_num;
use crate::style::excel_date;

/// The names a date format prints.
#[derive(Clone, Debug)]
pub struct Names {
    /// January … December.
    pub months: [String; 12],
    /// Short forms of the months.
    pub months_short: [String; 12],
    /// Monday … Sunday.
    pub days: [String; 7],
    /// Short forms of the weekdays.
    pub days_short: [String; 7],
    /// Before noon.
    pub am: String,
    /// After noon.
    pub pm: String,
}

fn strings<const N: usize>(items: [&str; N]) -> [String; N] {
    items.map(str::to_owned)
}

impl Default for Names {
    fn default() -> Names {
        Names {
            months: strings([
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
            ]),
            months_short: strings([
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ]),
            days: strings([
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday",
            ]),
            days_short: strings(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]),
            am: "AM".into(),
            pm: "PM".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Lit(String),
    Digit(char),
    Point,
    Group,
    Percent,
    Year(usize),
    Month(usize),
    Day(usize),
    Hour(usize),
    Second(usize),
    AmPm,
}

/// Splits a format code into its sections (`;` outside quotes).
fn sections(code: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted, mut esc) = (Vec::new(), String::new(), false, false);
    for c in code.chars() {
        if esc {
            esc = false;
        } else if c == '\\' && !quoted {
            esc = true;
        } else if c == '"' {
            quoted = !quoted;
        } else if c == ';' && !quoted {
            out.push(std::mem::take(&mut cur));
            continue;
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

fn push_lit(out: &mut Vec<Tok>, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(Tok::Lit(last)) = out.last_mut() {
        last.push_str(s);
    } else {
        out.push(Tok::Lit(s.to_owned()));
    }
}

fn tokens(section: &str) -> Vec<Tok> {
    let ch: Vec<char> = section.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let run = |i: usize, c: char| {
        ch[i..]
            .iter()
            .take_while(|x| x.eq_ignore_ascii_case(&c))
            .count()
    };
    while i < ch.len() {
        let c = ch[i];
        match c {
            '"' => {
                let end = ch[i + 1..]
                    .iter()
                    .position(|x| *x == '"')
                    .map_or(ch.len(), |p| i + 1 + p);
                push_lit(&mut out, &ch[i + 1..end].iter().collect::<String>());
                i = end + 1;
            }
            '\\' | '_' => {
                if let Some(n) = ch.get(i + 1) {
                    push_lit(
                        &mut out,
                        &if c == '_' {
                            " ".to_owned()
                        } else {
                            n.to_string()
                        },
                    );
                }
                i += 2;
            }
            '*' => i += 2,
            '[' => {
                let end = ch[i..]
                    .iter()
                    .position(|x| *x == ']')
                    .map_or(ch.len(), |p| i + p);
                let body: String = ch[i + 1..end.min(ch.len())].iter().collect();
                if let Some(cur) = body.strip_prefix('$') {
                    push_lit(&mut out, cur.split('-').next().unwrap_or(""));
                }
                i = end + 1;
            }
            '0' | '#' | '?' => {
                out.push(Tok::Digit(c));
                i += 1;
            }
            '.' => {
                out.push(Tok::Point);
                i += 1;
            }
            ',' => {
                out.push(Tok::Group);
                i += 1;
            }
            '%' => {
                out.push(Tok::Percent);
                i += 1;
            }
            'y' | 'Y' | 'm' | 'M' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' => {
                let n = run(i, c);
                out.push(match c.to_ascii_lowercase() {
                    'y' => Tok::Year(n),
                    'm' => Tok::Month(n),
                    'd' => Tok::Day(n),
                    'h' => Tok::Hour(n),
                    _ => Tok::Second(n),
                });
                i += n;
            }
            'a' | 'A' => {
                let rest: String = ch[i..].iter().take(5).collect::<String>().to_uppercase();
                if rest.starts_with("AM/PM") {
                    out.push(Tok::AmPm);
                    i += 5;
                } else if rest.starts_with("A/P") {
                    out.push(Tok::AmPm);
                    i += 3;
                } else {
                    push_lit(&mut out, &c.to_string());
                    i += 1;
                }
            }
            _ => {
                push_lit(&mut out, &c.to_string());
                i += 1;
            }
        }
    }
    out
}

fn is_date(toks: &[Tok]) -> bool {
    toks.iter().any(|t| {
        matches!(
            t,
            Tok::Year(_) | Tok::Month(_) | Tok::Day(_) | Tok::Hour(_) | Tok::Second(_) | Tok::AmPm
        )
    })
}

/// Rounds half away from zero on the decimal value (as Excel does), `places` after the point.
fn round_half_up(x: f64, places: usize) -> f64 {
    let k = 10f64.powi(places as i32);
    ((x.abs() * k) * (1.0 + 4.0 * f64::EPSILON) + 0.5)
        .floor()
        .copysign(x)
        / k
}

fn group(int: &str) -> String {
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn lit_text(toks: &[Tok]) -> String {
    toks.iter()
        .map(|t| match t {
            Tok::Lit(s) => s.clone(),
            Tok::Percent => "%".to_owned(),
            _ => String::new(),
        })
        .collect()
}

fn numeric(n: f64, toks: &[Tok], negative_sign: bool) -> String {
    let first = toks
        .iter()
        .position(|t| matches!(t, Tok::Digit(_) | Tok::Point));
    let Some(first) = first else {
        return lit_text(toks);
    };
    let last = toks
        .iter()
        .rposition(|t| matches!(t, Tok::Digit(_) | Tok::Point | Tok::Group))
        .unwrap_or(first);
    let region = &toks[first..=last];
    let percents = toks.iter().filter(|t| **t == Tok::Percent).count();
    let mut x = n.abs() * 100f64.powi(percents as i32);
    let scale = region
        .iter()
        .rev()
        .take_while(|t| **t == Tok::Group)
        .count();
    x /= 1000f64.powi(scale as i32);
    let region = &region[..region.len() - scale];
    let point = region.iter().position(|t| *t == Tok::Point);
    let (int_t, dec_t) = match point {
        Some(p) => (&region[..p], &region[p + 1..]),
        None => (region, &region[..0]),
    };
    let digits = |t: &[Tok], c: char| t.iter().filter(|x| **x == Tok::Digit(c)).count();
    let dec = digits(dec_t, '0') + digits(dec_t, '#') + digits(dec_t, '?');
    let min_dec = digits(dec_t, '0');
    let min_int = digits(int_t, '0');
    let grouped = int_t.contains(&Tok::Group);
    let r = round_half_up(x, dec);
    let text = format!("{r:.dec$}");
    let (int, frac) = text.split_once('.').unwrap_or((&text, ""));
    let mut int = int.trim_start_matches('0').to_owned();
    while int.len() < min_int {
        int.insert(0, '0');
    }
    let mut frac = frac.to_owned();
    while frac.len() > min_dec && frac.ends_with('0') {
        frac.pop();
    }
    let int = if grouped { group(&int) } else { int };
    let mut body = int;
    if !frac.is_empty() || (point.is_some() && min_dec > 0) {
        body.push('.');
        body.push_str(&frac);
    }
    let neg = negative_sign && n < 0.0 && r != 0.0;
    format!(
        "{}{}{}{}",
        if neg { "-" } else { "" },
        lit_text(&toks[..first]),
        body,
        lit_text(&toks[last + 1..])
    )
}

fn date(n: f64, toks: &[Tok], names: &Names) -> String {
    let secs = (n * 86_400.0).round();
    let serial = (secs / 86_400.0).floor();
    let day_secs = (secs - serial * 86_400.0) as i64;
    let (y, mo, d) = excel_date(serial).unwrap_or((1899, 12, 30));
    let (h, mi, s) = (day_secs / 3600, day_secs % 3600 / 60, day_secs % 60);
    let weekday = ((serial as i64 + 5).rem_euclid(7)) as usize;
    let ampm = toks.contains(&Tok::AmPm);
    let mut out = String::new();
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Lit(s) => out.push_str(s),
            Tok::Percent => out.push('%'),
            Tok::Digit(c) => out.push(*c),
            Tok::Point => out.push('.'),
            Tok::Group => out.push(','),
            Tok::Year(n) if *n <= 2 => out.push_str(&format!("{:02}", y.rem_euclid(100))),
            Tok::Year(_) => out.push_str(&format!("{y:04}")),
            Tok::Month(n) => {
                // after an hour or before a second the m means minutes
                let before = toks[..i].iter().rev().find(|t| !matches!(t, Tok::Lit(_)));
                let after = toks[i + 1..].iter().find(|t| !matches!(t, Tok::Lit(_)));
                if matches!(before, Some(Tok::Hour(_))) || matches!(after, Some(Tok::Second(_))) {
                    out.push_str(&if *n == 1 {
                        mi.to_string()
                    } else {
                        format!("{mi:02}")
                    });
                } else {
                    let k = mo as usize - 1;
                    match n {
                        1 => out.push_str(&mo.to_string()),
                        2 => out.push_str(&format!("{mo:02}")),
                        3 => out.push_str(&names.months_short[k]),
                        _ => out.push_str(&names.months[k]),
                    }
                }
            }
            Tok::Day(1) => out.push_str(&d.to_string()),
            Tok::Day(2) => out.push_str(&format!("{d:02}")),
            Tok::Day(3) => out.push_str(&names.days_short[weekday]),
            Tok::Day(_) => out.push_str(&names.days[weekday]),
            Tok::Hour(n) => {
                let hh = if ampm { (h + 11) % 12 + 1 } else { h };
                out.push_str(&if *n == 1 {
                    hh.to_string()
                } else {
                    format!("{hh:02}")
                });
            }
            Tok::Second(n) => out.push_str(&if *n == 1 {
                s.to_string()
            } else {
                format!("{s:02}")
            }),
            Tok::AmPm => out.push_str(if h < 12 { &names.am } else { &names.pm }),
        }
    }
    out
}

/// `n` as the format `code` shows it (`""` and `General` show the number as it is).
pub fn format(n: f64, code: &str, names: &Names) -> String {
    let code = code.trim();
    if code.is_empty() || code.eq_ignore_ascii_case("general") || !n.is_finite() {
        return fmt_num(n);
    }
    let secs = sections(code);
    let pick = if n > 0.0 || secs.len() < 2 {
        0
    } else if n < 0.0 {
        1
    } else {
        2.min(secs.len() - 1)
    };
    let toks = tokens(&secs[pick]);
    if is_date(&toks) {
        return if n < 0.0 {
            fmt_num(n)
        } else {
            date(n, &toks, names)
        };
    }
    numeric(n, &toks, pick == 0)
}

/// The text section of a format (the fourth), applied to text; `None` without one.
pub fn format_text(text: &str, code: &str) -> Option<String> {
    let secs = sections(code);
    let sec = secs.get(3)?;
    Some(sec.replace('@', text).replace('"', ""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(n: f64, code: &str) -> String {
        format(n, code, &Names::default())
    }

    #[test]
    fn numbers_decimals_and_groups() {
        assert_eq!(f(1234.5, ""), "1234.5");
        assert_eq!(f(1234.5, "General"), "1234.5");
        assert_eq!(f(1234.0, "0"), "1234");
        assert_eq!(f(2.5, "0"), "3");
        assert_eq!(f(-2.5, "0"), "-3");
        assert_eq!(f(1234.567, "0.00"), "1234.57");
        assert_eq!(f(1.005, "0.00"), "1.01");
        assert_eq!(f(0.5, "0.00"), "0.50");
        assert_eq!(f(0.5, "#.##"), ".5");
        assert_eq!(f(3.0, "0.0#"), "3.0");
        assert_eq!(f(3.14259, "0.0#"), "3.14");
        assert_eq!(f(7.0, "000"), "007");
        assert_eq!(f(1234567.891, "#,##0.00"), "1,234,567.89");
        assert_eq!(f(-1234.5, "#,##0.00"), "-1,234.50");
        assert_eq!(f(999.0, "#,##0"), "999");
        assert_eq!(f(1234567.0, "#,##0,"), "1,235");
        assert_eq!(f(0.0, "#,##0"), "0");
    }

    #[test]
    fn percent_and_currency() {
        assert_eq!(f(0.125, "0%"), "13%");
        assert_eq!(f(0.125, "0.00%"), "12.50%");
        assert_eq!(f(0.6, "0.0%"), "60.0%");
        assert_eq!(f(1234.5, "$#,##0.00"), "$1,234.50");
        assert_eq!(f(-1234.5, "$#,##0.00"), "-$1,234.50");
        assert_eq!(f(1234.5, "#,##0.00\\ \"EUR\""), "1,234.50 EUR");
        assert_eq!(f(1234.5, "[$€-407]#,##0.00"), "€1,234.50");
        assert_eq!(f(1234.5, "#,##0.00 [$ر.س-401]"), "1,234.50 ر.س");
    }

    #[test]
    fn sections_pick_by_sign() {
        assert_eq!(f(5.0, "#,##0;(#,##0)"), "5");
        assert_eq!(f(-5.0, "#,##0;(#,##0)"), "(5)");
        assert_eq!(f(0.0, "0.0;-0.0;\"zero\""), "zero");
        assert_eq!(f(-1.5, "0.0;-0.0;\"zero\""), "-1.5");
        assert_eq!(
            format_text("abc", "0;0;0;\"<\"@\">\"").as_deref(),
            Some("<abc>")
        );
        assert_eq!(format_text("abc", "0"), None);
    }

    #[test]
    fn dates_and_times() {
        assert_eq!(f(45292.0, "yyyy-mm-dd"), "2024-01-01");
        assert_eq!(f(45292.0, "dd.mm.yyyy"), "01.01.2024");
        assert_eq!(f(45292.0, "d/m/yy"), "1/1/24");
        assert_eq!(f(45292.0, "d mmmm yyyy"), "1 January 2024");
        assert_eq!(f(45292.0, "d-mmm-yy"), "1-Jan-24");
        assert_eq!(f(45292.0, "dddd"), "Monday");
        assert_eq!(f(45296.0, "ddd"), "Fri");
        assert_eq!(f(45366.75, "yyyy-mm-dd hh:mm"), "2024-03-15 18:00");
        assert_eq!(f(45366.75, "h:mm AM/PM"), "6:00 PM");
        assert_eq!(f(45366.0 + 3723.0 / 86400.0, "hh:mm:ss"), "01:02:03");
        assert_eq!(f(61.0, "yyyy-mm-dd"), "1900-03-01");
        assert_eq!(f(-1.0, "yyyy-mm-dd"), "-1");
    }

    #[test]
    fn names_come_from_the_caller() {
        let mut n = Names::default();
        n.months[0] = "يناير".into();
        n.days[0] = "الاثنين".into();
        assert_eq!(format(45292.0, "d mmmm yyyy", &n), "1 يناير 2024");
        assert_eq!(format(45292.0, "dddd", &n), "الاثنين");
    }
}
