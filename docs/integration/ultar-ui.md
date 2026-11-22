# ultar-ui – integration notes

Crate: `/home/claude/agents/ui/ultar-ui` (version 0.7.0, `#![forbid(unsafe_code)]`, pure Rust).
Area 5 "platform" (plan §3.0), target folder `platform/ultar-ui`. The one UI toolkit: tokens,
software canvas, Arabic-first text, logical layout, widgets, events, snapshots.
Run on 2026-11-22/23, cargo 1.97.0, x86_64 Linux, always `-j1`.

## Layout of the delivery
```
ultar-ui/
  build.rs            tokens.toml -> OUT_DIR/tokens_gen.rs
  tokens.toml         copy of design/tokens.toml (byte-identical; drift test below)
  assets/fonts/       4 font files + LIZENZ.txt, copied from the repo's assets/fonts
  locales/{ar,en,de}.ftl   17 keys each: ui.role.*, ui.state.*, ui.a11y.joiner
  src/                tokens, color, geom, canvas/, text/, icon/, paint, layout/, widgets/, ui/, event/, snapshot
  tests/              11 integration test files, tests/locales (demo.* keys), tests/snapshots (297 PNG)
ultar-i18n/           unmodified copy of system/core/ultar-i18n (path dependency)
```
Dependencies: `ultar-i18n` (path), `rustybuzz 0.20`, `unicode-bidi 0.3`, `ab_glyph_rasterizer 0.1`,
`png 0.17` (optional, feature `snapshot`, default on; also dev-dependency), build only: `toml 0.8`.
`ttf-parser` comes in through rustybuzz' re-export. `cargo tree` shows no `-sys` crate, no C.

## Wiring for the coordinator
1. Move `ultar-ui` to `platform/ultar-ui`, remove the empty `[workspace]` table, point the
   `ultar-i18n` path at `../../system/core/ultar-i18n`, delete my copy of `ultar-i18n`.
2. Fonts are read with `include_bytes!("../../assets/fonts/…")` relative to `src/text/fonts.rs`,
   i.e. from the crate's own `assets/fonts`. Either keep that copy or change the four paths in
   `src/text/fonts.rs` to the repository's `assets/fonts`.
3. CI: run tests with `ULTAR_TOKENS=<repo>/design/tokens.toml`. `tests/tokens.rs` then fails if
   the crate's `tokens.toml` differs (verified: passes with the repo file, fails with one
   changed digit). To update tokens: copy the file, rebuild, re-create snapshots.
4. Snapshots: `ULTAR_UPDATE_SNAPSHOTS=1 cargo test --test widget_snapshots --test screen`.
   A mismatch writes `tests/snapshots/failed/<name>.diff.png` and `.actual.png`.
5. Without PNG support (production): `default-features = false`.

## Public API (summary)
| Module | Items |
|---|---|
| `tokens` | generated: `Space::{S4..S48}`, `TypeSize::{S12..S32}`, `Radius::{Control,Card,Round}` (each `.points()`, `ALL`), `ColorRole` (16 roles, `ALL`, `name()`), `DARK`/`LIGHT: Palette` (`get(role)`), modules `size`, `motion`, `layout`, `shadow`, `typography`; hand-written: `Theme { dark, light }`, `THEME`, `ThemeKind`, `SizeClass::for_width`, `InputMode::min_target()`, `SOURCE` |
| `color`, `geom` | `Color` (`with_alpha`, `mix`, `relative_luminance`, `contrast_ratio`), `Point`, `Size`, `Rect`, `PixelRect` |
| `canvas` | `Canvas::new(w, h, PixelFormat::{Rgba8,Bgrx8})`, `clear`, `fill_rrect`, `stroke_rrect`, `shadow`, `line`, `stroke_polyline`, `fill_mask`, `blit`, `set_clip/clip/reset_clip`, `damage/take_damage`, `pixel`, `to_rgba`, `data`; `Damage` (≤ 16 rects, merges touching ones), `CanvasError` |
| `text` | `TextEngine::{new, with_limits, layout, draw, nominal_glyph, glyph_cache_stats, layout_cache_stats, clear_caches}`, `TextStyle`, `Overflow::{Clip,Wrap,Ellipsis}`, `TextAlign::{Start,Center,End}`, `TextLayout` (`lines`, `glyphs`, `width`, `height`, `truncated`, `caret_x`, `index_at_x`, `line_offset`, `visual_clusters`), `FontId`, `FontRole`, `Weight`, `isolate()`, `is_bidi_control()`, `missing_families()`, `TextError` |
| `icon` | `IconId` (47 icons, `ALL`, `name`, `path_data`, `mirrored`, `outline`), `path::parse` (M L H V C A Z, absolute/relative, implicit repeats) with `PathError`, `IconCache`, `QuarterTurns` |
| `paint` | `Painter` (logical points + roles → pixels: `fill`, `stroke`, `shadow`, `line`, `spinner`, `layout`, `text`, `icon`, `clip`, `set_opacity`, `hairline`), `PaintEnv { lang, direction, theme, scale }`, `TextSpec`, `Resources` |
| `layout` | `Flex::row()/column()` + `.gap(Space)`, `.padding(Padding)`, `.main(MainAlign)`, `.cross(CrossAlign)`; `Padding { start, end, top, bottom: Option<Space> }`; `Sizing::{AUTO, grow(n), width(pt).with_height(pt)}`; `LogicalRect { start, top, width, height }.resolve(direction, surface_width)`; `distribute()` |
| `widgets` | `Button` (+`ButtonVariant::{Primary,Plain,Quiet,Danger}`, `.icon()`), `IconButton`, `Switch`, `Select`, `Slider`, `TextField`, `Row`, `Card`, `Icon`, `Text`, `Label::{key, key_with, key_count, data}`, `Visual`, `Widget` |
| `ui` | `Ui::new(env, mode, size, translator)`, `add/add_sized/clear/set_root_flex`, `layout`, `paint/paint_all/needs_paint`, `handle(Event) -> Response`, `rect/logical_rect/content_size/size_class`, `set_lang/set_direction/set_theme/set_scale/set_input_mode/set_size/set_background/set_reduce_motion`, `advance_to(ms)/animating`, state: `is_on/set_on`, `value/set_value`, `selected/set_selected/is_open`, `text/set_text`, `set_label`, `set_disabled/is_disabled`, `set_loading`, `set_error/error`, `visual`, `focused/hovered/focus_order`, `a11y_node/a11y_tree`; `NodeId`, `UiError`, `A11yNode { role, name, value, error, state, rect, focusable }.describe(&Translator)`, `Role::name_key()` |
| `event` | `Event::{PointerMove, PointerDown, PointerUp, PointerLeave, TouchDown, TouchMove, TouchUp, TouchCancel, Key, Scroll}`, `Key`, `Modifiers`, `PointerButton`, `Action { node, kind }`, `ActionKind::{Activated, Toggled, Selected, ValueChanged, TextChanged, Submitted, Opened, Closed}`, `Response { consumed, actions }` |
| `snapshot` | `Snapshots::new(dir).catalog(lang, src).input_mode(m).update(b)`, `.assert_snapshot(name, tree, size, lang, theme)`, `.assert_ui(name, &mut ui)`, `.ui(...)`, `render(&mut ui)`, `Image::{from_canvas, encode_png, decode_png, diff}`, `write_atomic`, `SnapshotError` |

### How an app uses it
```rust
let mut tr = Translator::new(lang);                       // + app catalogs
let mut ui = Ui::new(PaintEnv::new(lang, ThemeKind::Dark), InputMode::Pointer, size, tr)?;
let card = ui.add(ui.root(), Card::new())?;
let row  = ui.add(card, Row::new(Label::key("settings.display.night")).sublabel(Label::key("…")))?;
let night = ui.add(row, Switch::new(Label::key("settings.display.night"), false))?;
// per input event:  for a in ui.handle(event).actions { match (a.node, a.kind) { … } }
// per frame:        ui.advance_to(now_ms); if ui.needs_paint() { ui.paint(&mut canvas); present(canvas.take_damage()) }
```
The `tree` argument of `assert_snapshot` is a closure `FnOnce(&mut Ui) -> Result<(), UiError>` that
builds the tree; the spec signature `assert_snapshot(name, tree, size, lang, theme)` is a method
of `Snapshots` (which holds the directory and catalogs) and returns `Result` instead of panicking.

## Decisions
- **Tokens.** `build.rs` parses `tokens.toml` and writes enums/constants; scales are enums
  (`Space::S16`), so a padding, gap or radius outside the scale cannot be written. Light and
  dark must list the same roles or the build fails. `src/color.rs` holds only the type plus
  `TRANSPARENT` and `BLACK` (shadow base; opacity from the shadow tokens).
  Sizes the artboards use but the tokens do not have (36 high controls, 44×24 switch, 13 px
  text) were **not** copied; widgets derive from tokens: control height = target minimum
  (32 / 44), switch 48×24 with a 16 knob, row minimum 48+8, body text 14.
  Hover/pressed of filled colours (primary button, switch track, slider thumb) mix 10 % / 20 %
  of the foreground into the token colour; neutral controls go raised → hover → line.
- **Rasteriser: `ab_glyph_rasterizer` on ttf-parser outlines** (not fontdue, not swash).
  rustybuzz already parses the font with ttf-parser and returns glyph ids; drawing those ids
  from the same `Face` avoids a second font parser with its own glyph numbering (fontdue) and a
  much larger dependency (swash/skrifa/read-fonts). The rasteriser is one small crate with no
  dependencies, exact area coverage, quadratic and cubic outlines. No hinting – fine at 12 px
  and above with 4 horizontal sub-pixel positions.
- **Old shell.** It used cosmic-text (which brings fontdb, swash, its own layout). Reused from
  it: the embedded fonts, the two-generation cache idea (now with a byte limit instead of an
  entry count), the 8-bit coverage mask blended in one pass. Not reused: cosmic-text itself –
  line layout, BiDi runs and ellipsis are done here so that start/end alignment, caret
  positions and isolates are under our control.
- **BiDi and data values.** Plain UAX #9 shows `3840 × 2160` inside Arabic as `2160 × 3840`
  and `60 Hz` as `Hz 60` (a test documents this). Therefore `Label::data(..)` and every
  argument of `Label::key_with(..)` are wrapped in first-strong isolates (U+2068/U+2069), as
  the artboards do with `<bdi dir="ltr">`.
- **Direction.** Layout stores `LogicalRect`s (start-relative). `Ui::set_direction` does not
  lay out again; rectangles, icon mirroring, slider/switch direction and arrow keys resolve
  when painting or handling input.
- **Mirrored icons** (spec §4): `chev` (as `ChevronEnd`), `volume`, `spark` (assistant bubble).
  Rotation is applied before mirroring, so the select's downward chevron stays downward.
- **Scale.** All widget geometry is logical points; `PaintEnv::scale` (0.5–8) multiplies at the
  painter, edges are snapped to device pixels.
- **States per widget.** Button ×4, IconButton, Switch: normal, hovered, pressed, focused,
  disabled, loading. Select: those plus error and open. Slider: normal, hovered, pressed,
  focused, disabled. TextField: normal, hovered, pressed (caret placed), focused, disabled,
  error (also filled). Row: normal, disabled. Card: normal, floating. Loading and error are
  refused with `UiError::NotApplicable` where they make no sense.
- **Keyboard.** Tab / Shift+Tab walk the logical order and wrap; arrows move focus (left/right
  mapped by direction, no wrap) unless the widget uses them. Enter/Space activate; select:
  Enter/Space opens, Up/Down/Home/End move, Enter/Space choose, Escape closes; slider: arrows,
  Home/End; text field: characters, Backspace, Delete, arrows, Home/End, Enter = `Submitted`.
  The focus ring shows for keyboard focus only.

## Fonts used (all from the repository, nothing downloaded)
| File | Used for |
|---|---|
| `NotoSansArabic-Regular.ttf` | all Arabic text, every weight |
| `NotoSans-Regular.ttf` | Latin, digits, symbols at 400/500 |
| `NotoSans-Bold.ttf` | Latin at 600/700 |
| `DejaVuSansMono.ttf` | `FontRole::Mono` |

**Gaps** (`text::missing_families()` returns them, a test pins the list): the token families
**Noto Naskh Arabic**, **Noto Kufi Arabic** and **Noto Sans Mono** are not in the repository.
Arabic text and titles are drawn with Noto Sans Arabic; there is **no bold Arabic file**, so
Arabic looks the same at all weights (visible in the screen snapshot title).

## Measured numbers
Frame time (`cargo test --release --test perf -- --ignored --nocapture`, 1920×1080 BGRX,
display-settings screen plus two more cards, 50 nodes, three runs on the shared 2-core machine):
| | Arabic | English |
|---|---|---|
| full repaint | 3.8 – 5.7 ms | 3.6 – 4.9 ms |
| layout + full repaint | 3.7 – 5.5 ms | 3.3 – 5.1 ms |
| hover repaint (damage only) | 0.012 – 0.028 ms | 0.012 – 0.015 ms |
| first frame (cold caches) | 5.6 – 23.4 ms | 4.1 – 7.7 ms |

No allocation per pixel or per shape; per frame there are small allocations (region list,
resolved label strings with arguments, flex slot vectors during layout only).
Limits: glyph cache 2 × 1 MiB, layout cache 2 × 1 MiB, icon masks 512 KiB, damage 16 rects,
4096 nodes, depth 64, text field 4096 bytes (configurable), 6 visible select options.
Contrast (WCAG formula, `tests/tokens.rs`): all 21 text/background pairs the toolkit paints are
≥ 4.5:1 in both themes; control border and focus ring pairs ≥ 3:1.

## Test results
- `cargo fmt --check`: clean. `cargo clippy --all-targets -- -D warnings`: clean (also
  `--no-default-features --lib`). `cargo doc --no-deps`: no warnings.
- `cargo test -j1` three times: **127 passed, 0 failed, 1 ignored** each time
  (also green with `--release`; the snapshots are identical at opt-level 0, 2 and 3).

| Target | Tests |
|---|---|
| unit tests in `src/` | 62 |
| `tests/a11y.rs` | 6 |
| `tests/canvas.rs` | 11 |
| `tests/keyboard.rs` | 9 |
| `tests/layout.rs` | 8 |
| `tests/pointer.rs` | 9 |
| `tests/repaint.rs` | 3 |
| `tests/screen.rs` | 2 |
| `tests/snapshot.rs` | 6 |
| `tests/tokens.rs` | 9 |
| `tests/widget_snapshots.rs` | 2 (73 widget/state cases × ar/en × dark/light = 292 images) |
| `tests/perf.rs` | 1 ignored (benchmark) |

Snapshots: 292 widget images + 4 of the composed display-settings screen + 1 at scale 2 in
touch mode = 297 files, 1.8 MB. The widget test also fails if two states of one widget render
identically or if a reference file is missing or left over.
What the required proofs are: shaping – `text::tests` (glyph ids differ from the isolated
forms, lam-alef = 1 glyph, joined word narrower, mark has zero advance and a GPOS offset);
BiDi – `isolated_numbers_keep_ltr_order_inside_arabic`, `mixed_arabic_and_latin_runs`;
mirroring – `the_same_tree_is_mirrored_in_rtl`; keyboard – `tests/keyboard.rs`;
contrast and target sizes – `tests/tokens.rs`.
Checked by hand once (mutations, then restored): a colour literal, a numeric gap and a hex
string added to a source file make the scan test fail; a changed switch colour makes 32
snapshot comparisons fail with pixel counts and diff images.

## Not done / not tested (honest list)
- **Widgets** of spec §3 outside this delivery: search field, list, sidebar, header bar, tabs,
  menu, context menu, dialog, sheet, toast, progress bar, empty state, table, tree, tooltip,
  badge. No scroll container (the `Scroll` event only scrolls an open select list).
- **High-contrast theme**: `tokens.toml` has only dark and light.
- **Renderer trait / GPU** (plan §3.6): this is the software path only; no HAL trait is
  implemented or consumed. No SIMD code; blending is plain scalar Rust.
- **Tree editing**: nodes can be added and the whole tree cleared; single nodes cannot be
  removed or reordered. Select options cannot be changed after creation.
- **Text**: line breaks only at whitespace (no UAX #14, no hyphenation, no kashida
  justification); ellipsis may cut inside a word; caret movement is logical, not visual, in
  mixed-direction text; no selection, clipboard, undo or input-method pre-edit in `TextField`;
  no hinting, no synthetic bold or italic, no colour glyphs (emoji), no font fallback beyond
  the four files (other scripts render as missing glyphs).
- **Icons**: only the 47 icons of `gen.py` `IC`. Spec §4 lists more (pencil, clipboard,
  scissors, eye, cloud, …) – absent. No filled variants for "on". The registry file
  `docs/ui/icons.toml` was not written. Which icons mirror is my reading of §4.
- **Motion**: only the switch knob (120 ms) and the progress ring; `fade` and `overview` are
  generated but unused. Hover/press changes are immediate.
- **Events**: no key-release events, no double click, no long press, no multi-touch gestures,
  no drag threshold, no pointer cursor shapes.
- **Accessibility**: nodes and spoken sentences exist; nothing connects them to a screen
  reader or to the bus. Slider values are announced as plain numbers without unit.
- **Arabic texts** (17 toolkit keys, demo texts) were written by the agent, not reviewed by a
  native speaker.
- **Platforms**: tested on x86_64 only. Whether aarch64 (A30) produces bit-identical snapshots
  was not tested. Not run on a real display; not linked into the shell or compositor.
- Frame times were measured on a shared machine and vary (see ranges); no CI budget check.
- `Ui` is single-threaded (`Rc`, `RefCell` inside).
- The `[workspace]` table and the `ultar-i18n` copy must be removed on integration (see Wiring).
