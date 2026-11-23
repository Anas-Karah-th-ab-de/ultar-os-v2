# Self-review of round 2 against the round-1 lists

Sources: `docs/ui/audit/03-entwurfspruefung-runde-1.md` ("must" 1–10, "should" 11–17, "later" 18–22) and
`docs/review/ultar-ui-runde-1.md` section 3 (A1–A20, B5, C1). Nothing was rendered in a browser; sizes and line breaks
are calculated, not seen. Run `python3 build.py`: it regenerates the 23 boards, `icons.json`, `ICONS.md`,
`boards-layout.json`, `CLICKS.md` and runs the checks (0 failures).

## Where every value comes from

`gen.py` reads `design/tokens.toml` with `tomllib`. The board scripts (`b1.py`–`b6.py`) contain no number with a unit
and no colour; the check "source literals" fails on `12px`, `#RRGGBB`, `rgb(`, `opacity`, `em/rem/pt`.
The check "token scan" reads every `style` attribute, the `<style>` block and every `<svg>` of the generated files:

| Kind | Allowed |
|---|---|
| padding, margin, gap | `space.values` only |
| border-radius | `radius.control`, `radius.card`, `radius.round` only |
| font-size / weight / line-height / family | `type.sizes` / `type.weights` / `line_height_arabic` on Arabic boards, `line_height_latin` on English ones / the four token families |
| colours | only the tokens of the board's own theme; `rgba` only as the shadow and the scrim built from `shadow.alpha_*` |
| shadow | exactly `0 offset_y blur rgba(0,0,0,alpha)` |
| icons | 16 or 24, stroke `size.stroke`, mirrored exactly when the icon is directional and the board is right-to-left |
| width, height, position | a multiple of the base grid (`space.values[0]`) or a border width |
| borders, outline | hairline or `size.focus_ring` |
| opacity, rotate, any other px | not allowed |

Derivations (the only places where a value is not a token itself, all in `gen.py`, all marked "derived"):

| Value | Derived as | Token that should exist instead |
|---|---|---|
| hairline 1 | whole part of `size.stroke` | `size.border = 1` |
| control height 32 | `size.target_pointer_min` | `size.control_height` |
| switch 48×24, knob 16, inset 4 (target 48×32) | `space[6]` × `size.icon`, `size.icon_small`, `space[0]`, `target_pointer_min` – same as the toolkit | `[switch]` |
| row minimum 56 | value of `size.bar_height` (the toolkit's row constant) | `size.row_min` |
| layout lengths (window, column, board) | `g(n)` = n × base grid | – (layout, not a token) |
| scrim behind a dialog | black with `shadow.alpha_*` | `theme.*.scrim` |
| hover | `theme.hover` (main button: border in `text`) | `[state]` |
| pressed | `theme.line` as background (main button: 2 border in `text`) | `[state]` |
| disabled | background `surface`, text `control_border`, border `line` – no opacity | `[state]` |
| focus ring | 2 solid `info`, offset 2 | `theme.*.focus` |
| accent as a graphic (mark, running dots, selected border, switch and main-button border) | `accent_text` (≥ 3:1 in both themes); `accent` only as a fill behind `on_accent` | – |

## "Must" list

| # | Item (findings) | State | How |
|---|---|---|---|
| 1 | Back arrow in RTL, mirroring bound to direction, decision for two-display pictograms and loudspeaker (R1, R10–R12) | fixed | `ic()` mirrors from the board direction and the icon's own flag; no call site can mirror. `back` points right in Arabic. `volume` is mirrored. Pictograms are physical and never mirrored, the arrow is gone. Check "icon mirroring by direction". R13: `kind-text`, `spark` etc. mirrored (list in `ICONS.md`). |
| 2 | Number and unit islands (R16–R18, R20, R21, R25) | fixed | `tx()` turns every Latin word, number and unit inside Arabic into `<bdi dir="ltr">`; keys are a `<kbd dir="ltr">` brick; file names are in `<bdi>`. Unit rule: Latin symbols (`1 Gbit/s`, `60 Hz`). Check fails on Latin or digits outside an island and on Arabic-Indic digits. |
| 3 | Terms (R19, R26–R31) | fixed | إظهار أرقام الشاشات · البحث عن شاشات · تكرار everywhere · العتاد والتعريفات everywhere · خيارات الطاقة · واي فاي. R28: الأدوات المكتبية, the term fixed in spec §12.4 (the review proposed التطبيقات المكتبية). Banned old wordings are checked. Glossary: `glossary.json`. |
| 4 | Icons with double meaning; VPN; do not disturb; missing icons (D46–D58) | fixed | 115 icons, each one meaning; table of every split in `ICONS.md`, register in `icons.json`. VPN = shield with key, do not disturb = crossed bell. |
| 5 | Undefined clicks | fixed | 761 elements, 169 actions; none without a definition (build fails otherwise). 59 definitions are new and listed in `CLICKS.md` "Spec additions needed" for the coordinator. |
| 6 | Type scale, families, line height (D26–D33) | fixed | only 12/14/16/20/24/32; Noto Sans, Noto Naskh Arabic (text), Noto Kufi Arabic (titles), Noto Sans Mono; 1.6 Arabic, 1.5 Latin. Fonts still come from the Google Fonts link in `<helmet>` as the task says (finding "offline" stays open, "later" 21). |
| 7 | Grid and radii, bar 32, dock 56 (D1–D25) | fixed | by construction and by the token scan. Bar at the top (variant) 32, dock 56, bottom bar 56, title bar 40 with 32 buttons. D25: "round" is also used for the knob of the slider, radio and step dots, running dots, progress – section 3 should name them. |
| 8 | Content contradictions (D59–D70) | fixed, 2 parts open | D59 dots = windows open. D60 mounted drive in bar and dock. D61 quick settings and calendar are two boards, one sheet each. D62 no empty app menu. D63 calendar: today's appointments, do-not-disturb switch, month buttons; quick settings: dark, power mode, output device. D64 all areas of §8 and §12.3 in the (scrolling) sidebar, areas without hardware absent. D65 brightness per display, night-light page, lid behaviour. D66 15 s. D67 Firefox ticks Linux programs, badge says so. D68 all components in installer and Store. D69 no placeholder: "—" with a visible note, because the spec defines no sizes. D70 no "Ultar components" area, "Install from a file" added, cards open the app page, "Open" on installed ones. **Open:** overscan compensation (TV only) and the rows for mirroring three displays are defined in `CLICKS.md` but not drawn. |
| 9 | "Keep?" dialog (A25, A32, D66) | fixed | dialog brick `role="alertdialog"`, first in the focus order of the window, dims only its window, 15 s as text and progress, "I need more time", Esc = revert. |
| 10 | Light theme ≥ 3:1 (A1–A6) | fixed | free app colours removed; controls use `control_border`; gold graphics use `accent_text`; selected state has border + check + weight. Check "contrast": worst text pair 4.88 (light, success), worst control border 3.41. |

## "Should" list

| # | Item | State | How |
|---|---|---|---|
| 11 | Gold only for one main action, never colour alone (D35–D41) | fixed | Store cards have no gold; hints use info/warning/danger with icon; one main button per surface. Selected = border + check icon; today = filled + bold + `aria-current`. D40: app family is monochrome until it has tokens – **open**. |
| 12 | Targets ≥ 32 (A8–A15) | fixed | all controls 32, tiles 44, switch target 48×32, check box = whole row, slider target 32, calendar days 32. Check "targets". |
| 13 | States (2.2, A30) | fixed in part | board `States`: 12 bricks × 7 states with the focus ring. `SurfaceStates`: Store loading, empty search, offline, error with cause, not enough space. `Account`: offline, wrong password, locked. **Open:** the other surfaces (Files empty/error, quick settings without hardware, one display only, installer without space, phone states) are not drawn. |
| 14 | Roles and names (A17–A29) | fixed | real `<button>`/`<input>`, search fields with names, `nav` + `aria-current`, `radiogroup`, grid for the calendar, list for notifications, `h1`/`h2`, slider with `aria-valuetext`, steps as `<ol>` with `aria-current="step"` and the word "مكتمل". **Not done:** `dir` on `<html>` (the fixed file skeleton keeps it on the root `div`). |
| 15 | One shadow, stroke 1.5, icon sizes 16/24 (D42–D44) | fixed | checked. |
| 16 | Bottom bar after §11 (D16) | fixed | start: Trash and mounted drive · centre: Overview, all apps, search, apps · end: input language, status, clock. |
| 17 | Remaining wording (R24, R32–R42) | fixed | تشرين الثاني; مُطفأ / غير مفعّل; شاشة داخلية; تُلغى تلقائياً خلال 15 ثانية; …وشاشة تسجيل الدخول; الذكاء الاصطناعي المحلي; دون إعادة تثبيت النظام; السابق/التالي; اكتمل التثبيت; `aria-label` لغة الإدخال; الترابط with "ربط أجهزتك"; الجهاز. |

## "Later" list (not asked for; state for information)

| # | State |
|---|---|
| 18 | Info, warning, danger are now drawn. High-contrast palette: **open**, `tokens.toml` has none. |
| 19 | Size classes compact/medium: **open**. One light English counterpart added (`SettingsDisplayLight`). |
| 20 | fixed: only `inset-inline`, `border-inline-*`; windows are placed from the start side. `left` remains only in the display arrangement, which is physical on purpose. |
| 21 | `canvas.json` untouched (coordinator). Fonts from the network: as instructed. "Sa 22 Nov" in the spec: for the coordinator. |
| 22 | fixed: title-bar buttons out of the Tab order, active window first, one Tab stop per radio group. |

## Design fidelity (`ultar-ui-runde-1.md` §3)

A1/A2 text sizes – fixed. A3 control height 32 – fixed. A4 sidebar item 32 – fixed. A5 window buttons 32 – fixed.
A6 switch 48×24, knob 16, inset 4 – fixed. A7 select: content width – fixed. A8 line height 1.6 – fixed. A9 Noto Sans – fixed.
A10 stroke 1.5 – fixed. A11 icons 16/24 – fixed. A12–A15 spacing – fixed. A16/A17 radii – fixed. A18 shadow alpha – fixed.
A19 header badge: 32×32, text 16, neutral border – fixed. A20 border widths: hairline is derived, a token is proposed above.
B5: the confirmation is the dialog brick (raised, line border), no gold border. C1: families as in the tokens.
Consequence for the toolkit: a row with sublabel is 57.6 high with line height 1.6 (minimum 56), as the toolkit measures.

## Open points for the coordinator

1. Tokens to add: border width, control height, switch, row minimum, `[state]` (hover, pressed, disabled), focus colour, scrim, large icon, app-family colours, high-contrast theme.
2. 59 behaviour definitions in `CLICKS.md` need a place in the specification.
3. Not rendered: the layout was calculated (heights, widths, wrapping); a look at the boards on the canvas is still needed, most of all `Account`, `InstallerSummary`, `States` (tightest).
4. Not drawn: per-surface states outside Store and Account, compact/medium sizes, mobile extras of Phone (USSD, mast choice), the app page of the Store.
5. One extra board beyond the brief: `CalendarSheet` (needed to resolve D61/D63).
