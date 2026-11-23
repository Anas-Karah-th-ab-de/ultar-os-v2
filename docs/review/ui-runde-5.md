# UI toolkit – review round 5 (re-check of the round-4 fixes)

Independent reviewer, 2026-11-23, HEAD 0f4846e. Verdict: not ready – 0 critical, 5 major, 6 minor. All 15 round-4
findings fixed at the root except W1 (still open at 4K and on the phone) and w-m1 (half fixed). Tests:
`platform/ultar-ui/tests/w5_*.rs` (from `/home/claude/agents/w4/uirev2/tests/`).

## Major
- **R1 – full repaint 2.7–3.5× slower (regression).** A/B minimum of 60 frames: full HD 0.73–0.81 → 2.03–2.77 ms;
  4K scale 2 3.0–3.3 → 10.0–12.4 ms (over the 8 ms budget). Likely `canvas/shapes.rs` `paint_coverage` → `blend_alphas`
  blends every pixel; the old code skipped alpha 0. Fix: skip alpha-0 runs, write alpha-255 runs directly; keep the A/B bench.
- **W1 – still open at 4K and phone.** 4K dialog open median 5.6–13.3 ms, p99 10.4–23.9 ms; phone sheet p99 25 ms (budget 16).
- **D1 – `MAX_NODES` exhausted in long-running apps** (removed ids never reused; fails after ~1364 rebuilds). Fix: slot reuse
  with a generation number in `NodeId`. Test da02.
- **D2 – sorting moves the selection to other rows** (`ui/edit.rs:14-29` keeps selection by index). Fix: stable row keys
  (`set_table_rows_keyed`) or clear + `SelectionChanged`. Test da03.
- **D3 – panic in the paint path when the OS refuses a thread** (`canvas/shapes.rs:153-160`, `thread::scope` spawn).
  Fix: `Builder::spawn_scoped`, draw inline on `Err`; cache `available_parallelism`.

## Minor
D4 focus after `remove` restarts at the first widget (da01) · D5 held button keeps repeating after disable/remove, no
`Released` (da05) · D6 hover not restored when a menu closes by a press beside it (da07) · D7 `open_menu` on hidden owners
(da08) · D8 tabs unreachable by touch – no swipe/wheel/overflow (da09) · D9 accessibility tree of a long table: 10 000×4 →
50 005 nodes, 16.8 MB, 25 ms – create cell nodes only near view (pf01).

## Notes
`control_border` on `line` 2.82/2.88. Select check mark and sidebar mark: boards were moved to the toolkit (against the
round-3 rule); menus put the check at the start, the select list at the end – make them consistent. CI asserts medians;
p99 must be part of the release checklist.

## Verified correct
W2 (spinner behind dialog 0.13–0.25 ms), W3, W4, W5, W6, w-m3/4/5/7/8/9; dimming the existing frame across 8×4 states;
600-seed differential fuzzer with clock, toasts, menus, remove, move, modifier clicks, fractional scale – no difference.
