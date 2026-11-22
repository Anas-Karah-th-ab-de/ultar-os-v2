# Review report – agent "ui" – `platform/ultar-ui` (ultar-os-v2)

Reviewed read-only; all experiments in `/home/claude/agents/review/ui/scratch` (copy of the crate, `ultar-i18n`
and `assets/fonts`, own target dir, `-j1`, `ULTAR_TOKENS=/home/claude/ultar-os-v2/design/tokens.toml`).
Baseline before any change: 127 passed, 0 failed, 1 ignored (as the integration notes state).

New tests (copy into `platform/ultar-ui/tests/` to run; they use `mod common;`):
`tests/review_findings.rs` (4 major findings, all fail on the reviewed code, all pass with the patches),
`tests/review_minor.rs` (10 minor findings, all fail on the reviewed code), `tests/review_fuzz.rs` (damage fuzzer),
`tests/review_probe.rs`, `tests/review_arabic.rs`, `tests/review_design.rs`, `tests/review_dbg.rs` (measurement probes that print).
Note: `review_probe.rs` contains `Color::rgba(..)` literals, so the crate's own token-scan test flags it while it sits in `tests/`.

Counts: **critical 0 · major 4 · minor 16** (10 behaviour + 6 design/token deviations on the toolkit side).
The artboard itself deviates from `tokens.toml` in 20 measurable places (section 3).

## Findings

| # | Severity | Where (file:line) | What is wrong | How proved | Suggested fix |
|---|---|---|---|---|---|
| 1 | major | `src/paint.rs:144` (`Painter::color`) with `fill/stroke/line/spinner/text/icon` (`:189,197,218,239,262,291`) | Opacity is applied twice: `color()` multiplies alpha by the painter opacity and every draw call multiplies again. Disabled widgets are painted at 0.4² = 0.16 instead of `DISABLED_OPACITY` 0.4 – labels of disabled controls are nearly invisible. The 44 `*-disabled-*` reference snapshots encode the wrong value. | proved by test `f1_disabled_widget_is_painted_at_forty_percent`: disabled primary button background is (43,39,23) = accent at 16 % over ground; 40 % would be (91,78,32) | `fixes/1.patch` (apply opacity only in the draw calls) |
| 2 | major | `src/ui/tree.rs:247` (`Ui::set_selected`) | With the options list open, `set_selected` damages only the field. The list keeps the old check mark and bold option until something else repaints it: the incremental frame differs from a full repaint. Only state change found that does not damage what it changes. | proved by test `f2_set_selected_repaints_the_open_popup`: 1446 stale pixels in (62,270)–(230,345); found by the fuzzer (seeds 23, 39, 40) | `fixes/2.patch` (`touch_popup()` when the select is open) |
| 3 | major | `src/event/keys.rs:165` (`field_key`), `src/widgets/text_field.rs:138` | Left/Right in a text field follow the UI direction, not the text. Latin text (host name, e-mail, URL) in the Arabic UI: Right at the left end does nothing, Left moves the caret to the right. Arabic text in the English UI is inverted the same way. The notes only admit "logical in mixed-direction text"; this is uniform-direction text. | proved by test `f3_arrow_keys_move_the_caret_visually` (Arabic UI, text `hello`, Home, ArrowRight: caret stays 0, expected 1) | `fixes/3.patch` (direction from the BiDi level of the neighbouring character; at run ends it still falls back to logical movement) |
| 4 | major | `src/text/layout.rs:326` (`truncate`) | The ellipsis is appended as a bare neutral character. After a run against the paragraph direction UAX #9 resolves it to the paragraph direction, so it is drawn at the far end of that run, away from the cut: `الرقم 1234567890123456789` becomes visually `…123456 الرقم` (reads as if leading digits were dropped; the trailing ones were). Same for Latin words in Arabic (`…Hello wo`) and Arabic in an English paragraph. Isolated values (`Label::data`) are correct. | proved by test `f4_ellipsis_stays_at_the_cut` (ellipsis at x=0, cut at x=59.1) | `fixes/4.patch` (LRM/RLM of the last kept run's direction on both sides of the ellipsis) |
| 5 | minor | `src/widgets/slider.rs:58` | `Slider::new` with a NaN bound panics in `f32::clamp` (`min`/`max`/`value` are also `pub`, so `min > max` can be set later with the same result). | proved by test `m1_…` | reject non-finite bounds in `new`/`snap`; use `max(min).min(max)` instead of `clamp` |
| 6 | minor | `src/widgets/slider.rs:54` | Infinite `step` makes the value NaN (`0 × inf`); every pointer event then emits `ValueChanged(NaN)` because `NaN != NaN`. | proved by test `m2_…` and probe P2 | treat non-finite step as 0 |
| 7 | minor | `src/ui/mod.rs:387-398` (`knob_position`), `src/event/pointer.rs:129` | Toggling a switch again while its knob travels restarts the animation from the opposite end: 10 ms after switching on, switching off makes the knob jump from x=183 to x=205 (fully on) and then travel back. | proved by test `m3_…` | start the new animation at the current position (store start position or `toggled_at - (FEEDBACK - elapsed)`) |
| 8 | minor | `src/widgets/text_field.rs:30` (`allowed`) | U+2028/U+2029 pass the filter (`char::is_control` is false for them). The single-line field then holds two paragraphs; caret and hit-testing only know the first line. | proved by test `m4_…` | also refuse `'\u{2028}' \| '\u{2029}'` |
| 9 | minor | `src/text/layout.rs:321` | Ellipsis on multi-line text: `cluster_start` is false at the line break, so the last cluster of a first line that fits is dropped (`سطر أول\n…` → `سطر أو…`, `أ\nب` → `…`). | proved by test `m5_…` | accept `cut == first_line.len()` as a boundary |
| 10 | minor | `src/text/layout.rs:87` (`caret_x`), `:125` (`index_at_x`) | Inside a ligature cluster (lam-alef, base + tashkeel) the caret has no position of its own: after one arrow press it does not move, and a click can never place it between lam and alef. | proved by test `m6_…` (caret x 8.15 / 8.15 / 0) | divide the cluster advance by its number of base characters |
| 11 | minor | `src/text/cache.rs:63` | The glyph cache limit holds for normal sizes (measured ≤ 1.95 × limit), but an entry larger than the limit is kept in each generation: 1.5 MB held at a 64 KiB limit, above the documented "2 × limit plus one entry". With the default 1 MiB limit a 2048² glyph (4 MB) can be held twice. | proved by test `m7_…` | do not cache entries larger than the limit (or drop the old generation when one is inserted) |
| 12 | minor | `src/canvas/mod.rs:232,251,252`, `src/canvas/shapes.rs:102,114,129`, `src/geom.rs:142`, `src/text/mod.rs:236` | `i32` arithmetic on unclamped coordinates overflows: panic in builds with overflow checks (debug, tests), silent wrap in release. Reachable through public canvas calls with extreme arguments and through `Ui::paint` on a surface about 3e9 points wide. Release build: no panic, nothing drawn (checked). | proved by test `m8_…` (9 cases) | saturating/`i64` arithmetic; clamp float→pixel conversions to the clip first |
| 13 | minor | `src/widgets/select.rs:174` | When the list fits neither below nor above the field it is placed below and runs off the surface; on a 120-point-high surface only options 1 and 2 of 6 can be reached by pointer. | proved by test `m9_…` | shrink the visible count to the space available (the list already scrolls) |
| 14 | minor | `src/ui/mod.rs:142,145`, `src/event/pointer.rs:217-250` | Mouse and touch share one `pressed` slot: a touch tap on one widget while a mouse button is held on another drops both activations. | proved by test `m10_…` | one press slot per pointer |
| 15 | minor | `src/canvas/shapes.rs:122-134` (`shadow`), `src/widgets/containers.rs:310` (`shadow_reach`) | Shadow reaches 16 pt beside and 28 pt below the popup (measured 13/23 px visible). The artboard's CSS `0 12px 32px` has a transition of 2 × blur centred on the edge, i.e. 32 pt beside and 44 pt below. Same token, half the spread. | proved by test (`review_design.rs`, changed-pixel box) + reading | fade over `2 × blur` and enlarge `shadow_reach` accordingly |
| 16 | minor | `src/text/fonts.rs:10,58` | No Arabic file for weights 600/700: bold Arabic is shaped with the regular file (identical glyph list and width for 24 pt regular and bold). Title and semibold labels look regular; the artboard shows them bold. Listed as a gap in the notes. | proved by test (`review_design.rs`) | embed the bold Arabic files of the token families |
| 17 | minor | `src/text/fonts.rs:10-13` | Token families `Noto Naskh Arabic` / `Noto Kufi Arabic` / `Noto Sans Mono` are not embedded; Noto Sans Arabic and DejaVu Sans Mono are drawn instead (listed as a gap in the notes). | by reading (`missing_families()`) | embed the token fonts or change the tokens |
| 18 | minor | `src/widgets/style.rs:13,15,17` | Visual values outside `tokens.toml`: disabled opacity 0.4, hover mix 0.10, pressed mix 0.20. The scan test does not catch them. | by reading | add `[state]` tokens |
| 19 | minor | `src/widgets/text_field.rs:1-30` | A text field always uses the UI direction as paragraph direction and has no option for a fixed one. A phone number typed in the Arabic UI is shown as UAX #9 requires for an RTL paragraph (`4567 123 11 963+`), which is correct BiDi but unusable for phone/URL/e-mail fields. | proved by test (`review_arabic.rs`, case `phone`) + reading | `TextField::direction(Auto \| Ltr \| Rtl)` |
| 20 | minor | `tests/common/mod.rs` (`display_settings`) | The composed "display settings" screen is not the artboard's layout: one 656-wide column instead of 640 + side card, monitor icon instead of the gold "1" badge, extra rows (night light, brightness, device name), action buttons instead of the floating confirmation card. Snapshots of it cannot show that the implementation equals the approved design. | by reading both | compose the artboard 1:1 once sections 3A/3B are decided |

Patches (all `diff -u`, apply with `patch -p1` in the repo root; each checked with `--dry-run` against the repo):
- `fixes/1.patch`: all existing tests pass **except** `every_widget_in_every_state_matches_its_reference`: the 44 `*-disabled-*` references (11 states × ar/en × dark/light) must be regenerated (`ULTAR_UPDATE_SNAPSHOTS=1`). No other snapshot changes.
- `fixes/2.patch`, `fixes/3.patch`, `fixes/4.patch`: all 127 existing tests still pass (run with 2+3+4 applied, and with all four applied).

## 1. Tester – what was attacked and found correct

- Damage: 480 fuzz runs × 120 random steps (pointer, touch, keys, scroll, clock, `set_on/value/text/disabled/loading/error/direction`) on the display screen, Arabic and English, scale 1 / 1.25 / 1.5, pointer and touch. After every step: every changed pixel lies inside the canvas damage, and the incremental frame is byte-identical to a full repaint. Only finding 2 failed. A text taller than its box next to a repainted button is also correct.
- Pointer: leave while pressed keeps the press, release outside does not activate; duplicate down/up, up without down, secondary button, second finger, touch cancel; disabled mid-press; loading widgets consume input without acting.
- Focus: Tab/Shift+Tab skip disabled items and wrap, arrows follow the direction (also with `set_direction` against the language), disabling the focused widget clears focus, all disabled → Tab not consumed. There is no "hidden" state in the toolkit, so nothing to test there.
- Select with 0 options (does not open, no panic), 1 option (opens, closes, no `Selected`), many (scroll clamps, non-finite scroll ignored).
- Slider: clamping, step grid (0..1 step 0.1 reaches exactly 1), tiny step, huge ranges; a maximum not on the step grid cannot be reached (0..100 step 30 ends at 90) – same as HTML range, not counted.
- Text field: 4096-byte limit, 500 stacked combining marks, ZWJ/ZWNJ accepted, BiDi controls refused, `max_len(0)`; release build: paint of a full field 3 ms, edit + paint 0.4 ms.
- Layouts 0×0, 1×1, negative, NaN, infinite: no panic. Huge sizes: only finding 12.
- Layout cache stays within 2 × limit (measured 1.997 ×).
- Not available in the API, therefore not testable: key-repeat flag, pointer button state in `PointerMove`.

## 2. Arabic reviewer

Checked 38 shaping strings and 43 BiDi strings (`tests/review_arabic.rs`) against UAX #9 by hand (no second BiDi implementation is installed here). **No wrong visual order found in the BiDi layer**; the two Arabic defects are findings 4 (ellipsis) and 3/10 (caret).

Correct:
- Lam-alef: لا لأ لإ لآ are one glyph each, final form in بلا / كلام, with a mark in between (لَا) ligature plus positioned mark, U+FEFB.
- Tashkeel: shadda + fatha/kasra/damma/tanwin combine into one mark glyph with zero advance and GPOS offsets; full مُحَمَّدٌ correct; truncation never separates a mark from its base.
- Allah: the font ligates only the vocalised form اللّٰه (one glyph, same as U+FDF2); plain الله stays four joined glyphs. This is the font's behaviour (Noto Sans Arabic), not the toolkit's.
- Tatweel joins (ســـلام), initial/medial/final forms, Persian letters, Arabic punctuation and `٪`.
- Mirroring: `( ) [ ] { } < > « »` use the mirrored glyph in right-to-left runs and the normal one inside left-to-right runs; arrows are not mirrored (correct, they are not Bidi_Mirrored).
- Order as UAX #9 requires, including the uncomfortable cases: `النسبة 50%` → `%50` left of the word (digits after Arabic letters are AN, `%` is not absorbed), `50% من…` keeps `50%`, phone number groups reversed, `10-20` → `20-10`, `3840 × 2160` → `2160 × 3840`, `-5°` → `°5-`, trailing `/` of a URL and trailing `.` of Latin text move to the left, dates/times/versions/IP addresses stay intact, nested Latin with numbers, brackets around Latin inside Arabic.
- First-strong isolates (`Label::data`, `key_with` arguments) repair every one of those cases (resolution, phone, `60 Hz`, negative number, URL, nested, empty, unclosed).
- Digits: the toolkit and `ultar-i18n` only produce 0–9. Arabic-Indic digits in data or typed by the user are shown unchanged (not converted, not filtered).
- Ellipsis in same-direction text: at the logical end (left in Arabic, right in English), width never above the limit.
- Wrapping: breaks at spaces, each line reordered on its own, emergency break inside a long word.

## 3. Design fidelity – artboard `SettingsDisplay.dc.html` vs tokens vs toolkit

All 12 colours of the artboard are token colours of the dark theme; the toolkit paints the same roles (measured: card `#171A21` / border `#2E333E`, select and plain button `#1F232C` / border `#6F7789`, primary `#D4AF37`, switch on `#D4AF37` with knob `#14110A`). Row geometry agrees: minimum height 56, padding 8 × 16, gap 16, 1 px separator, label 14/500, sublabel 12 muted, card radius 12, control radius 8, button padding 16, select padding 12.

### A. The artboard violates the tokens (artboard must change, or the token must be added and the toolkit follow)

| # | Item | Artboard | Tokens | Toolkit |
|---|---|---|---|---|
| A1 | Control text (buttons, selects, search, sidebar) | 13 px (31 places) | sizes 12/14/16/20/24/32 | 14 |
| A2 | Other text sizes | 11 px (6), 10 px (1), 26 px (3), 18 px (1) | not in the scale | – |
| A3 | Control height (buttons, selects, search) | 36 | only `target_pointer_min = 32` | 32 |
| A4 | Sidebar item height | 38 | none | – |
| A5 | Window buttons | 30 × 30 | below `target_pointer_min = 32` | – |
| A6 | Switch | 44 × 24, knob 18, inset 3 | none of 44/18/3 | 48 × 24, knob 16, inset 4 (S48, icon, icon_small, S4) |
| A7 | Select width | fixed 220 / 190 | none | content width (195/152/93/118 in Arabic) |
| A8 | Arabic line height | 1.5 | `line_height_arabic = 1.6` | 1.6 → rows with sublabel are 57 high, artboard 56 |
| A9 | Latin family | Inter | `family_latin = Noto Sans` | Noto Sans |
| A10 | Icon stroke | 1.6 | `stroke = 1.5` | 1.5 (scaled with icon size) |
| A11 | Icon sizes | 18, 14, 10, 44×20 | `icon_small = 16`, `icon = 24` | 16 / 24 (select chevron 16, artboard 14) |
| A12 | Page padding | 20 × 32 | 20 not in space scale | 24 × 32 |
| A13 | Title bottom margin, column gap | 14 | not in scale | 16 |
| A14 | Card header padding | 14 × 16 | 14 not in scale | 16 × 16 |
| A15 | Gaps | 2, 6, 20 | not in scale | – |
| A16 | Radii | 10 (4), 6 (1) | control 8 / card 12 / round | – |
| A17 | Switch radius 12, knob 9 | literal | `round = 9999` | round (looks the same) |
| A18 | Shadow alpha | 0.45 | `alpha_dark = 0.40` | 0.40 |
| A19 | Header badge | 36 × 36, text 18 | none | not a toolkit widget |
| A20 | Border widths | 1 px and 2 px literals | no border-width token (only `stroke 1.5`, `focus_ring 2`) | 1 device pixel |

### B. The toolkit deviates (toolkit must change)

| # | Item | Toolkit | Artboard / tokens | Finding |
|---|---|---|---|---|
| B1 | Disabled opacity | effective 0.16 | intended 0.4 (not a token) | 1, 18 |
| B2 | Bold / semibold Arabic | regular glyphs | bold (weights 600/700 are tokens) | 16 |
| B3 | Shadow spread | 16 beside / 28 below | 32 / 44 with the same token values | 15 |
| B4 | State values outside the tokens | 0.4 / 0.10 / 0.20 | "no value outside the tokens" | 18 |
| B5 | Floating card | surface background, line border | confirmation card: raised `#1F232C`, 1 px accent border | no variant exists |

### C. Both deviate from the tokens

| # | Item | Artboard | Toolkit | Tokens |
|---|---|---|---|---|
| C1 | Arabic family | Noto Sans Arabic | Noto Sans Arabic | Noto Naskh Arabic (text), Noto Kufi Arabic (titles) |

Artboard and toolkit agree with each other here; the token (or both sides) must change (finding 17).

### What decides "cannot look different"
A1, A3, A6, A7, A8 and B2 are the differences a user sees at once (text size, control height, switch shape, select width, row height, bold titles). For each, either the artboard takes the token value or a token is added (`control_height`, switch dimensions, select width) and the toolkit reads it. After that the display-settings test screen has to be rebuilt as the artboard (finding 20) before its snapshot can serve as proof.
