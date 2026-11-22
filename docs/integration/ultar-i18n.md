# ultar-i18n – integration notes

Crate: `/home/claude/agents/i18n/ultar-i18n` (version 0.7.0, zero dependencies, `#![forbid(unsafe_code)]`).
Area: system core (plan §3.0 area 3, target folder `system/kern/ultar-i18n`). Replaces
`ultar-shell/src/i18n.rs`, `ultar-sprache` and `t()` in `ultar-kern/src/install.rs`.

## Features
- `std` (default): `Lang::from_env()`, module `check_dir`, binary `ultar-i18n-check`.
- Without `std` the library is `#![no_std]` + `alloc` (kernel / boot chain): `default-features = false`.

## Catalog format (`locales/*.ftl`, also for app catalogs)
```
# comment
key.with.namespace = text with {placeholder}
multi = first line
    indented continuation (joined with a newline)
count.files = {count ->
    [zero] لا ملفات
    [one] ملف واحد
   *[other] {count} ملف
}
brace = literal {"{"} brace
```
- Keys: ASCII letter first, then letters/digits/`_`/`-`/`.`. `{$name}` (Fluent style) is accepted too.
- Variant keys: plural categories (`zero one two few many other`), exact numbers (`[0]`, win over
  categories) or names (`[wifi]`, matched against `Arg::Str`). Exactly one default `*[...]`.
- Nesting of selections is limited to 4. Duplicate keys and empty values are errors.

## Public API (summary)
| Item | Purpose |
|---|---|
| `Lang {Ar, En, De}` | `parse("ar_SY.UTF-8")`, `FromStr`, `code()`, `locale()`, `native_name()`, `direction()`, `index()`, `from_candidates(iter)`, `from_env()` (std; default **Arabic**), `Lang::ALL` |
| `Direction {Rtl, Ltr}`, `direction(lang)` | `is_rtl()`, `opposite()`, `html_dir()` |
| `Side {Start, End}` → `PhysicalSide {Left, Right}` | `side.resolve(direction)`, `resolve_for(lang)`, `PhysicalSide::to_logical` |
| `Catalog` | `parse(src)`, `parse_namespaced(ns, src)`, `merge`, `insert`, `get`, `contains`, `keys`, `iter`, `keys_with_prefix`, `len`, `is_empty` |
| `Message`, `pattern::{Part, Select, Variant, VariantKey}` | `format(lang, args)`, `format_into`, `as_static_text`, `placeholders`, `plural_categories` |
| `Arg {Str, Int, Grouped}` | `From<&str>` and `From<i8..i64, u8..u32>`; `Grouped` writes `1,234,567` |
| `Translator` | `new(lang)` (embedded catalogs), `empty(lang)`, `set_lang`, `t(key) -> Cow<str>`, `t_args(key, &[("name", Arg)]) -> String`, `try_t`, `has`, `lookup`, `add_catalog`, `add_source`, `add_namespaced_source`, `catalog(lang)`, `direction()`, `side(Side)` |
| `plural` | `category(lang, n)`, `categories(lang)`, `PluralCategory` |
| `numbers` | `to_western_digits`, `western_digit`, `normalize_number_input`, `parse_int`, `format_int/uint/fixed`, `group_digits`, `separators(lang)` |
| `calendar` | `Date`, `HijriDate` (tabular civil), `to_hijri`/`to_gregorian`, `Weekday`, `WeekStart` (`default_for(lang)`, `days()`, `parse`), `Date::from_unix_seconds(secs, offset)` |
| `datefmt` | `month_name` (`MonthNames::Levantine`/`Alternative`), `hijri_month_name`, `weekday_name`, `weekday_short_name`, `format_date_numeric/long/with_weekday`, `format_hijri_long`, `format_time` (`ClockStyle`) |
| `units` | `format_size(bytes, &tr)`, `format_percent(n, &tr)` |
| `arabic` | `normalize_for_search`, `search_matches`, `is_tashkeel`, `is_ignorable`, `fold_letter`, `decompose_presentation_form` |
| `collation` | `collation_key`, `compare`, `sort` |
| `bidi` | `contains_rtl`, `first_strong_direction`, `direction_of(text, fallback)`, `is_rtl_char`, `is_ltr_char` |
| `check` | `missing_keys(reference, other)`, `placeholder_mismatches`, `incomplete_plurals(catalog, lang)`, `compare_catalogs` |
| `check_dir` (std) | `check_directory(path)`, `check_sources`, `Report`, `Problem::describe(&tr)`, `CheckError` |
| Errors | `ParseError {line, kind, key}`, `ParseErrorKind`, `NumberError`, `DateError`, `UnknownLangError`, `CheckError` – all implement `Error` + `Display` and have `message_key()` (catalog keys `i18n.*`) for user-facing text |

Fallback chain of `t`/`t_args`: requested language → Arabic → the key itself. A missing argument is
rendered as `{name}`. Plural rules follow the language in which the message was found.

## How other crates use it
```rust
let mut tr = Translator::new(Lang::from_env());            // system/CLI; kernel: Translator::new(Lang::Ar)
tr.add_namespaced_source(Lang::Ar, "apps.notes", include_str!("../locales/ar.ftl"))?;   // app catalogs
let title = tr.t("settings.display.title");
let line  = tr.t_args("count.files", &[("count", Arg::Int(n))]);
let edge  = Side::Start.resolve(tr.direction());           // UI code never says left/right
```
- Hardware/driver classes: `hw.class.<id>`, states `hw.state.<id>`; editions `edition.*`, form factors `form.*`.
- CI: `ultar-i18n-check <locales-dir>` – exit 0 clean, 1 problems, 2 usage/IO error; reference is `ar.ftl`.
- `settingsd` should own language, week start, 12/24 h, month-name style and Hijri display and pass them in;
  this crate reads no settings files and no system paths.

## Embedded catalogs
`locales/ar.ftl`, `en.ftl`, `de.ftl`: 344 keys each (action 54, state 27, installer 36, settings 38,
hw 27, error 22, edition/form 10, unit 12, count/time 11, cal 62, check 12, i18n 23, system/lang 10).
Texts were taken over from the three old sources where they existed.

## Test results (run on 2026-11-22, cargo 1.97.0, `-j1`)
- `cargo fmt --check`: clean.
- `cargo clippy --all-targets -- -D warnings`: clean; also `--no-default-features --lib`: clean.
- `cargo test`: **117 passed, 0 failed** – 97 unit tests, 9 `tests/api.rs`, 4 `tests/check_bin.rs`
  (runs the binary), 6 `tests/completeness.rs`, 1 doc test.
- `tests/completeness.rs` fails when a key is missing in any direction or placeholders differ – verified
  once by hand by deleting a key and renaming a placeholder in `en.ftl` (2 tests failed), then restored.
- Hijri reference dates were cross-checked against an independent implementation of the standard
  tabular formula (Python, not part of the crate); the crate tests hold 10 fixed date pairs, a
  day-by-day round trip over ~1700 Hijri years and year lengths for 3000 years.

## Open points / not done (honest list)
- **Not migrated:** the ~1,400 UI strings of `ultar-shell/src/i18n.rs` (English-text keys), the CLI help
  texts and phrase tables of `ultar-sprache/src/tabellen.rs`, and the localized command words (`BEFEHLE`:
  جديد/new/neu …). Only the base set was seeded. The phrase-replacement approach of `ultar-sprache`
  (translating German output after the fact) is intentionally not carried over; tools must use keys.
- `no_std` was only verified by building `--no-default-features` on the host target; no bare-metal
  target is installed here, and it was not linked into the kernel.
- `Lang::from_env()` is only tested indirectly through the binary tests.
- Default language without any setting is now Arabic (old `ultar-sprache` defaulted to German).
- Arabic texts were written by the agent, not reviewed by a native speaker. Driver is rendered as
  «تعريف» as in the old installer.
- Plural rules cover whole numbers only (no decimals); `Arg` has no float variant.
- Hijri is the tabular civil calendar; Umm al-Qura / moon sighting can differ by 1–2 days. No user
  offset (±days) yet.
- Collation is a compact own table (Arabic before Latin, ة after ه, hamza forms with their carrier,
  Latin-1 diacritics folded); it is not full UCA/CLDR. Digit strings sort character by character, not by value.
- `first_strong_direction` only picks a base direction; full BiDi reordering and shaping belong to `ultar-ui`.
- No currency formatting, no relative-time helper beyond the `time.minutes_ago` key, no time zones
  (only a fixed offset in `Date::from_unix_seconds`).
- `Translator::new` parses all three embedded catalogs at construction (no lazy loading, no global instance).
- The `[workspace]` table in `Cargo.toml` must be removed by the coordinator on integration.
