# UI toolkit – second review (round 4) of the round-3 fixes

Independent reviewer, 2026-11-23, commit a5eb384. Verdict: not ready for sign-off – 0 critical, 6 major, 9 minor.
Of the 25 round-3 findings 17 are fixed at the root, 6 partly (M1, M5, M7, M8, m5, m15), 2 open (m16, "virtual" `List`).
Timings from a loaded shared x86 machine (load 2.6–4); medians are the reliable figures.
Tests: `platform/ultar-ui/tests/w4_*.rs` (from `/home/claude/agents/w4/uirev/tests/`).

## Major

**W1 – M1 not fixed beyond full HD.** 4K dialog open median 17–20 ms, p99 26–39 ms (budget 8 ms PC); 4K repaint under a
sheet 10–17 ms; phone sheet open 8.9–14 ms on x86. `src/ui/render.rs:200-276` `backdrop_for` allocates 33 MB on every
open, repaints the whole tree, scrims and copies the whole buffer; sheet shadow full height.
Fix: dim the pixels already in the frame in one pass, keep the buffer allocated, copy only damaged rows, 9-slice shadow,
4K budget 8 ms. Tests bd04, r3_perf p02/p04.

**W2 – backdrop regression.** Every animating node behind an open dialog (busy button, indeterminate bar, switch in
transit) marks the whole backdrop stale on each `advance_to` (`src/ui/mod.rs:450-460`, `src/ui/clock.rs:39-43`): 3.6–4.1 ms
per frame instead of ~0.05 ms while the dialog is open. Fix: damage list for the backdrop, or freeze animations behind a
scrim. Tests bd02, bd03.

**W3 – CLICKS.md still not buildable.** (a) middle click dropped (`src/event/mod.rs:261`, 36 entries); (b) no
multi-selection in List/Table (`src/ui/state.rs:44-60`, 13 entries); (c) no drop target, drags report only
`DragEnded(delta)` (43 entries); (d) no API to change table rows, tree items, select options or remove/reorder nodes.
Fix: `ActionKind::MiddleClicked`, multi-selection (Shift range, Ctrl toggle, selected per row announced),
`Ui::node_at(pos)` + `DragEnded { target }`, `set_table_rows`, `set_tree_items`, `remove(node)`.

**W4 – table rows hide columns from screen readers** (`src/ui/access_tree.rs:244-258`): cells from column 3 on are never
announced. Fix: one cell part per cell, `labelled_by` its header. Test ax02.

**W5 – accessibility identities collide** with an open submenu (`access_tree.rs:296-335`). Fix: separate part spaces per
menu level or explicit `A11yId`, plus a uniqueness test. Test ax01.

**W6 – password field.** (a) stays revealed after pointer leave, disable, dialog open (`gesture.rs:76-79,150-157`,
`tree.rs:240-268`, `modal.rs:57-65`, `event/mod.rs:267-271`); (b) derived `Debug` prints the password
(`text_field.rs:47`); (c) eye button not focusable, no key to reveal. Fix: reset on leave/cancel/disable/dialog/focus
loss/clear, hand-written `Debug` with `<hidden>`, keyboard toggle as toggle button. Tests pw01–pw03.

## Minor

- w-m1 tabs overflow instead of scrolling (Arabic: 8th of 8 tabs at x = −56) – `tabs.rs:62-74`, `measure.rs:300-303`, tb01.
- w-m2 dropped hover returns late after re-enable, `set_direction`, popup close – call `rehover()`; hv01, hv02.
- w-m3 collapsing a folder hides the selected child; Up does nothing – `data_keys.rs:68-84`, ax03.
- w-m4 reference picture `password-shown-normal-*` shows the masked field (press before layout, `cases4.rs:68-86`).
- w-m5 board ↔ toolkit: field label colour (board `text` 12/500, toolkit `text_muted`), sidebar mark edge (board end,
  toolkit start), select check mark slot, toast size (board 58×400, toolkit 48×≤480 – add a width token), slider value text
  missing, check-box row padding.
- w-m6 frame budgets not enforced in CI (debug only); `r3_perf` softened (best of three, 4K at 16 ms). Add a release job.
- w-m7 `List` is not virtual (one node per row, `MAX_NODES` 4096) – data-driven list or document "Table for long lists".
- w-m8 `assets/fonts/SOURCES.md` says one commit, files come from two (identical versions).
- w-m9 light theme: status texts on `line` (selected/pressed) below 4.5:1 – darker selected token or a spec rule.

## Verified as correct

Incremental layout: 600 seeds × 120 steps differential fuzzing, no difference. Backdrop never stale across 12 kinds of
change. m1–m4, m6–m14, m17, M3, M4, M6 fixed. 16 font files match SOURCES.md (SHA-256, size, weights 400/500/600/700).
Full HD medians: full repaint 0.9–1.1 ms, dialog open 3.4–4.4 ms, repaint under dialog 1.2–1.5 ms, 2000 rows 0.7–1.2 ms.
Contrast: every text role ≥ 4.5:1 except on `line`. New widgets correct in both themes and languages.
r3 test edits judged legitimate (except the softenings in r3_perf).
