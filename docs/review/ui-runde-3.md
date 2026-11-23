# Review round 3 – `platform/ultar-ui` after wave 3 (commit 157060b)

Reviewer: independent (Prüfer Bild, Prüfer Bedienung, Prüfer Zugang). Repo read-only; work on a copy, own target dir, `-j1`, deleted.
Baseline reproduced: 210 passed, 0 failed, 4 ignored. With the six new files in `tests/`: `cargo fmt --check` clean,
`cargo clippy --all-targets -D warnings` clean, the crate's token scan passes, functions <= 60 lines.

## Verdict
**Not ready for sign-off.** No crash and no critical defect: 150 x 600 hostile fuzz steps in release and 12 x 250 in debug
without a panic; modal surfaces really block input. But: the pictures are **not** the boards (27 disagreements, section 3),
the accessibility tree is not usable for radio groups, tabs and open selects, light-theme state graphics are at 2.0:1,
a dialog costs 10-18 ms per full-HD frame (budget 8 ms), and the toolkit lacks the input primitives the click list needs.
Counts: **critical 0, major 8, minor 17**. 24 failing regression tests delivered, 21 new tests pass (= verified behaviour).

Limits of this review (honest): the boards were compared through the generator (`design/boards/gen.py`, `b1-b6.py`) and the
styles in the 23 HTML files, not as rendered browser pictures. Of the 521 PNGs I looked at contact sheets of every group in
ar-dark, ar-light and en-light; en-dark only for buttons, check boxes and radios; of the round-1 widgets (select, text field,
switch, slider, icon button) one or two states each. x86_64 only. Arabic wording not judged by a native speaker.

## 1. Findings

Test = file in `tests/platform/ultar-ui/tests/` (target path `platform/ultar-ui/tests/`). "Side" = who must change.

### Major
| # | What | Where | Reproduce | Side / fix |
|---|---|---|---|---|
| M1 | A dialog or sheet makes every full repaint miss the frame budget: full HD median 9.7-9.9 ms, p99 14.0-18.5 ms; opening it p99 15.5-16.3 ms; 3840x2160 at scale 2 under a sheet median 53.5 ms, p99 72.7 ms. Budget (plan §3.6): 8 ms PC, 16 ms phone - measured on a desktop x86, the A30 is slower. Wave 3 only estimated this ("7-12 ms, not measured"). | `src/widgets/dialog.rs:121-122` (scrim blended over every pixel), `src/ui/render.rs:165` | `r3_perf.rs` p01, p02 (`--release -- --ignored --nocapture`); numbers in `evidence/perf.log` | toolkit: dim the background once into a buffer when the dialog opens and repaint only dialog damage; row-wise constant-colour blend; a layout change inside a dialog must not repaint the scrim |
| M2 | Layout is redone for every node on every invalidation and lists are not virtual: 2000 rows -> layout + repaint median 70 ms, p99 110 ms; build + first frame 88 ms. Any `set_label`, `set_error`, resize or language change with a long list drops to ~14 fps. (Scrolling 0.9 ms and arrow keys 1.1 ms are fine.) | `src/ui/measure.rs:306` | `r3_perf.rs` p03 | toolkit: keep measured sizes per node and width; virtual list rows |
| M3 | Accessibility tree: the options of a radio group, the tabs of a tab bar and the options of an open select are not nodes (only the chosen value is exposed), so a screen reader cannot read or count them. No relations at all in `A11yNode` (no parent, position in set, labelled-by, controls), slider without range, no "mixed" check state (boards use it), the clear button of a search field has no node or name, every info bar is an alert (boards: `status` for info/success). | `src/ui/access.rs:251-260`, `:140-160` | `r3_access.rs` z01, z02, z03, z04, z05 | toolkit |
| M4 | Light theme: the graphic that carries "on / chosen / how much" is drawn in `accent` at 2.03:1 against the ground (2.20:1 on a row): switch-on track, slider thumb and fill, checked box, chosen radio, tab indicator, list selection bar, edge of the primary button; progress track 1.19:1. The boards already follow the rule "accent only as a fill behind on_accent, accent_text for graphics and borders". | `switch.rs:102`, `slider.rs:135,152`, `checkbox.rs:44`, `radio.rs:95`, `tabs.rs:107`, `list.rs:192`, `button.rs:85`, `progress.rs:65`; pictures `*-light.png` of these widgets | `r3_design.rs` d01, d02 | toolkit: `accent_text` border around accent fills, `accent_text` for bars and indicators, `control_border` for the progress track |
| M5 | Hover is not recomputed when content moves under a resting pointer: after a wheel step the row that scrolled away stays highlighted, and its tooltip comes back although the pointer is no longer over it. | `src/ui/scrolling.rs:47-66` (`scroll_to` hides the tooltip but keeps `input.hover`) | `r3_behaviour.rs` b08, b09 | toolkit: remember the last pointer position and hit-test again after every scroll and layout |
| M6 | A menu that the limits allow does not fit a phone: 16 commands x 44 = 704 points on a 320-point-high surface; the panel is clamped to the top, the lower commands are outside and cannot be reached by pointer (keyboard highlights a command nobody sees). No long press, so by touch no context menu opens at all (spec §5.5: same shell on phone). | `src/widgets/menu.rs:254`, `src/event/pointer.rs:331` | b11, b15 | toolkit: scroll the panel; long press = secondary click |
| M7 | Input primitives the click list needs are absent, so `docs/ui/CLICKS.md` cannot be built from this toolkit: double click (`desk.icon`, `files.item` §13.7/15), modifier keys on pointer events (Ctrl/Shift selection), pressed/held actions (`login.show` §13.37, `phone.backspace` §13.59), dragging (`display.select` §13.38), password field (Login, Account, Installer boards), anchored panel for the shell sheets (§13.5/6), table, tree, sidebar, header bar, toast (spec §3 list). | `src/event/mod.rs:99-160` (events), `src/widgets/mod.rs:91` | by reading; no test can be written against a missing API | toolkit (next wave); spec lists them already |
| M8 | Board and toolkit disagree in 27 places (section 3). "Abweichungsliste = leer" is the gate of Prüfer Bild; it is not empty. | section 3 | `r3_design.rs` d03, d04 for two of them | see section 3 per line |

### Minor
| # | What | Where | Test | Side / fix |
|---|---|---|---|---|
| m1 | Disabling the focused widget drops the focus position: the next Tab starts at the first widget instead of continuing. | `src/ui/tree.rs:219` | b05 | toolkit: move focus to the next stop |
| m2 | Spec §13.13 "Esc clears and leaves the field": Esc clears, the caret stays in the search field. | `src/event/keys.rs:247` | b07 | toolkit, or reword the spec |
| m3 | No type-ahead in menus and in the open select (a letter is swallowed). | `src/ui/menus.rs:367`, `src/event/keys.rs:112` | b12, b13 | toolkit |
| m4 | A right click outside an open context menu only closes it; the menu of the new place needs a second click. | `src/event/pointer.rs:317` | b14 | toolkit |
| m5 | Tabs shrink below the touch target (5 tabs on 200 points = 40 each); the bar neither scrolls nor wraps. Clear button of the search field is 40 wide by touch (44 needed). Scrollbar grip 12 points. | `tabs.rs:46`, `search_field.rs:95`, `scroll.rs:32` | z09, z10 | toolkit |
| m6 | Token weights 500 and 600 are not drawn: Latin 400 = 500 and 600 = 700; Arabic 600 = 700. The boards use 500 for labels and 600 for buttons and headings, so no picture can match them. | `src/text/fonts.rs` | `r3_arabic.rs` a02 | toolkit: embed Medium/SemiBold, or tokens drop the two weights and boards follow |
| m7 | Marks leave the line box: damma on alef-hamza 3 px above at 14 pt Naskh, Kufi titles 1-6 px above at every size, marks below the line 2-3 px in the English interface (Arabic file names). Clipped wherever the line box touches a clip (scroll edge, list row), collides between lines. Plain vocalised Naskh text stays inside. Contradicts the wave-3 note "stays inside at all six sizes". | `src/text/mod.rs` (line metrics) | a03 (a08 prints the cases) | toolkit |
| m8 | 12-point Arabic is not smaller than Latin (letter body 8 px vs x-height 7 px) but 30 % lighter (ink per pixel of width 401 vs 568): sublabels look faint in `screen-display-ar-*`, `listitem-*-ar-*`. | tokens / `style.rs:30` | a05 (passes, prints) | design decision: weight 500 for Arabic secondary text |
| m9 | `Ui::set_size` takes NaN, infinity and negative sizes (`set_scale` refuses such values): rectangles become NaN. | `src/ui/mod.rs:258` | `r3_fuzz.rs` f03 | toolkit: typed error |
| m10 | Stale pixels: in a window narrower than its content (181 points, the focused icon button reaches past the edge) the incremental frame differs from a full repaint after Tab. Found by the fuzzer (seed 527), reduced to a fixed test. | `src/ui/render.rs:165-196` | f04 | toolkit |
| m11 | Values that are not tokens: tooltip delay `2 x motion.overview` = 600 ms and spinner period `4 x` (`ui/clock.rs:8,10`), spinner sweep 0.75 (`paint.rs:255`), busy segment 1/3 (`progress.rs:22`), empty-state icon 48 = a space token used as an icon size (`info_bar.rs:144`; spec §4: 16 and 24 only), row minimum `S48 + S8` (`containers.rs:139`; boards use `bar_height`), hairline = 1 device pixel (`paint.rs:181`), border of check box and radio = icon stroke 1.5 (`checkbox.rs:51`, `radio.rs:102`), wheel step and touch slop from space tokens (`scroll_input.rs`). Boards: no raw value in the 23 files (own scan), only the derivations listed in REVIEW-SELF. | as named | - | add tokens `size.border`, `size.row_min`, `size.icon_large`, `motion.tooltip_delay`; then both sides use them |
| m12 | The reference picture teaches a dangerous default: in `dialog-open-normal-*` the destructive button "Delete" is the default action and has the focus when the dialog opens, so Enter deletes. Spec §13.42 puts the focus on the safe action. | `tests/common/gallery.rs:108-109`, `src/ui/modal.rs:60-63` | - | toolkit: when the default action is a danger button, initial focus goes to the cancel action |
| m13 | Seven states per brick (spec §3): `set_loading` / `set_error` answer `NotApplicable` for check box, radio, list item, tabs; the board "States" draws loading and error for check row, list row and slider. No picture for search field pressed, disabled scroll. | `src/ui/tree.rs:259-283` | - | toolkit, or the spec names the exceptions |
| m14 | Icons: 51 of the 115 icons of `icons.json`; `battery` is mirrored on the boards, not in the toolkit; names differ (`chev` vs `chevron-down`); `update` means "updates / reload" (boards split it); nine toolkit icons are not in the register (box, chev, file, globe, image, music, star, success, window). | `src/icon/mod.rs` | - | toolkit follows ICONS.md |
| m15 | Pictures: the focus ring of a check box touches its label (no inline padding); the ring of a radio row overlaps the rows above and below; the background of a hovered, pressed or selected first/last list item is square and covers the rounded corner and border of the card. | `checkbox-*-focused-*`, `radio-focused-*`, `listitem-{normal,hovered,pressed}-*` | - | toolkit |
| m16 | Frame budget is not enforced: `tools/budgets.toml` holds binary sizes only, `perf.rs` is ignored and asserts nothing. First Arabic frame cold: 31 ms (once). | `tools/budgets.toml`, `tests/perf.rs` | - | add frame budgets and a CI job on a quiet machine |
| m17 | Spoken states: an unchecked check box says "off" / "متوقف" (switch wording); expected "not checked" / "غير محدَّد". With a tooltip showing, the first Escape only hides it; a dialog needs a second Escape. | `locales/*.ftl`, `src/event/keys.rs:91` | - | toolkit |

## 2. Contrast (computed, `evidence/contrast.txt`, 176 pairs)
- Text on ground, bar, surface, raised: every role >= 4.5:1 in both themes; smallest 4.50 (success on ground, light).
- Below 4.5 only on state backgrounds, light theme: success on hover 4.38, on pressed 3.79; info 4.30, warning 4.19, accent_text 4.42 on pressed.
- on_accent on accent: 7.79 / 8.96, hovered 6.53 / 7.42, pressed 5.36 / 6.07.
- control_border: 3.16-4.33 on every background except the pressed one (2.82 / 2.88).
- Focus ring (accent_text): 4.42-11.48 against every background. Against the gold it surrounds: 1.24 dark, 2.58 light (a 2-point gap separates them) - gold ring around gold button reads as a border; the boards chose blue (`info`).
- `accent` as a graphic, light: 1.71-2.42 on every background (M4). `line` as the only separator: 1.12-1.54.
- Disabled (0.40 opacity): text 2.48-2.55 light, muted text 1.78-1.86 light / 2.28-2.33 dark, danger 1.96-2.27. Exempt in WCAG, but sublabels of disabled rows are close to invisible; the boards' variant (control_border text, no opacity) gives about 4:1.
- Text behind the scrim: 3.35-4.91 (still readable behind a dialog, light theme most).
- Colour alone: not found. Tones have icon and spoken name, selection has mark and weight, errors have text. Weak spots: a danger button differs from a plain one by colour only; a field error has no icon (boards have one).

## 3. Board vs toolkit - which side changes
Rule used: a value that is a token wins; where no token exists the approved board is the reference.
| # | Element | Board (`gen.py`) | Toolkit | Change |
|---|---|---|---|---|
| 1 | focus ring colour | `info`, 2 px, offset 2 | `state.focus_ring = accent_text` | boards (or decide to change the token to `info`: clearer next to gold) |
| 2 | hover, primary | border in `text` | 10 % `on_accent` overlay (`hover_overlay`) | boards |
| 3 | pressed, primary | 2 px border in `text` | 20 % overlay (`pressed_overlay`) | boards |
| 4 | disabled | surface fill, control_border text, line border | whole widget 0.40 (`disabled_opacity`) | boards; see contrast note |
| 5 | scrim | black at shadow alpha (0.40 / 0.18) | ground at 0.60 / 0.40 (`scrim_*`) | boards |
| 6 | primary button | hairline border `accent_text` | none | toolkit (M4) |
| 7 | switch | off: outline `control_border`, knob `text`; on: accent + `accent_text` border | off: filled `control_border`, knob `surface`; on: no border | toolkit |
| 8 | slider | knob `text` with `accent_text` ring, fill `accent`, value text at the end | knob and fill `accent` | toolkit knob; fill `accent_text` on both |
| 9 | check box | box 24, radius 8, hairline, row target 48 high, padding 8/12, gap 12, label 500, row hover | box 16, square, border 1.5, gap 8, label 400, 32 high | toolkit (d03) |
| 10 | radio | circle 24, ring 2 px `accent_text`, dot 8 `accent_text`; cards with title and text | circle 16 filled `accent`, dark dot | toolkit |
| 11 | badge | radius 8, no fill, text in tone colour (neutral: text_muted) | pill, raised fill, neutral text in `text` | toolkit (d04) |
| 12 | hint / info bar | radius 12 | radius 8 | toolkit |
| 13 | progress | track `control_border`, fill `accent` | track `line`, fill `accent_text` | track: toolkit; fill: boards |
| 14 | empty state | icon 24, gap 12, padding 24 | icon 48, gap 16, padding 32 | toolkit, or new token `icon_large` and boards |
| 15 | dialog | raised fill, title 16/600, text muted, `alertdialog` | surface fill, title 20/700 | toolkit |
| 16 | sheet | floating panel above the bar at the end, margin 8, padding 16 | full-height edge sheet 384 / bottom sheet, padding 24 | toolkit adds the anchored panel; boards draw the compact bottom sheet |
| 17 | list selection mark | `accent_text`, 4 x 16 | `accent`, 4 x (height - 16) | toolkit |
| 18 | navigation item | 32 high, radius 8, padding 0/12 | list item 48 min, square, padding 16 | toolkit (sidebar brick is missing) |
| 19 | tabs | only browser tabs (filled, bordered) | underline tabs | boards draw the brick; toolkit indicator `accent_text` |
| 20 | field | label above 12/500, error with icon | no label, error text only | toolkit: error icon |
| 21 | raised card | "no gold border" (REVIEW-SELF B5) | `Card::raised()` accent border, `card-raised-normal-*` | toolkit |
| 22 | weights | 400/500/600/700 | 400 and 700 (Arabic text also 500) | toolkit (m6) |
| 23 | menu, context menu, tooltip, scrollbar, select list, bottom sheet | not drawn on any board | designed by the build agent | boards |
| 24 | row minimum | `size.bar_height` | `S48 + S8` | both: token `size.row_min` |
| 25 | border width | whole part of `size.stroke` | 1 device pixel; 1.5 for check box, radio, focused field | both: token `size.border` |
| 26 | states in the light theme | board "States" is dark only | 4 variants | boards |
| 27 | loading / error of check row, list row, slider | drawn | `NotApplicable` | toolkit (m13) |
Equal on both sides (checked): control height 32, button padding 16 and radius 8, button weight 600, select padding 12 and chevron 16, card radius 12 with hairline `line`, row padding 8/16 gap 16 label 500 minimum 56, badge height 24 / 12 pt / 600, dialog width 480 padding 24 gap 16, shadow `0 12 32`, neutral hover = `hover` and pressed = `line`, icon sizes 16 / 24, stroke 1.5.

## 4. Verified as correct
- Arabic rendering (measured, `r3_arabic.rs`): letters join in Naskh, Kufi and Noto Sans Arabic; bold has 1.43-1.80 x the ink of regular in all three faces at 12, 14, 24; digits 0-9 come from the Latin face in reading order; an isolated "60 Hz" is one left-to-right island left of the Arabic words; Arabic-Indic digits keep their order; no ink beyond the measured width.
- Pictures: chevrons, volume and assistant icons mirrored, check and gear not; brackets, full stop and ellipsis on the correct side; phone number readable in the left-to-right field; switch, slider, scrollbar, side sheet, submenu and button order mirrored; titles in Kufi, text in Naskh; no clipped glyph in the pictures viewed; disabled at 0.40 (round-1 finding 1 is fixed).
- Behaviour (passing new tests): wheel, finger and scrollbar do not move content behind a dialog (b01, b02); a press beside a sheet closes it, focus returns to the trigger and the trigger is not operated (b03); Enter and Space on the focused Cancel never run the default action (b04); focus is back on the button after its menu closes by Escape, Enter or Tab (b06); no tooltip after a touch tap (b10); a submenu at 320 points width does not hide its command (b16); 2000-row list: arrows keep the focused row fully in view, Home/End reach both ends, 0.22 ms per step in debug (b17); a click after scrolling hits the row drawn there (b18); Escape closes the select before the dialog (b19); disabled list item, tabs, radio, check box, search ignore pointer, touch and Tab (b20).
- Access (passing): every node of gallery, open menu and dialog speaks a non-empty sentence without raw keys, in Arabic letters in Arabic and the same node order in both languages (z06); checked, selected, disabled, busy, focused follow the widgets, percent in 0-9, dialog first and alone in the tree (z07); every operable node of gallery, sheet and menu >= 32 (pointer) / 44 (touch) in both directions (z08).
- Robustness: no panic in 90 000 release and 3 000 debug fuzz steps with NaN/infinite coordinates, wheel values, setter values, out-of-range indices, bidi controls, clock jumps to `u64::MAX`, language/theme/scale/mode changes mid-gesture, zero and absurd surface sizes. The only stale-pixel case is m10.
- Frame time within 8 ms (release, full HD): full repaint median 0.85-0.99 ms (p99 1.2-4.2), layout + repaint 1.0 ms, menu open + close 0.9 ms, Tab in a dialog 0.14 ms, 4K at scale 2 median 3.6 ms / p99 5.8 ms, existing `perf.rs` screen 2.9-3.8 ms, hover 0.02 ms.
- Tokens: `design/tokens.toml` = `platform/ultar-ui/tokens.toml`; the 23 boards hold no colour, spacing, radius, type size or weight outside the tokens.

## 5. Files
- `tests/platform/ultar-ui/tests/r3_behaviour.rs` - 20 tests, 9 fail (b05, b07, b08, b09, b11, b12, b13, b14, b15)
- `.../r3_access.rs` - 10 tests, 7 fail (z01-z05, z09, z10)
- `.../r3_arabic.rs` - 8 tests, 2 fail (a02, a03)
- `.../r3_design.rs` - 4 tests, 4 fail
- `.../r3_fuzz.rs` - f01 passes, f02 (ignored, long) passes in release with `R3_SEEDS=150`; in debug it stops at seed 527 = m10; f03, f04 fail
- `.../r3_perf.rs` - 3 measurements (ignored)
- `evidence/` - contrast table and script, perf logs, board scan and contact-sheet scripts
All use `mod common;` of the crate and need no new dependency.
