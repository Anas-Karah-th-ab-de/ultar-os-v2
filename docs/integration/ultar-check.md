# ultar-check – integration notes

Repository quality gate (plan §3.0 + §7). Area: developer tooling, target location `tools/ultar-check/`.

## Deliverables

| File | Goes to |
|---|---|
| `ultar-check/` (crate: lib `ultar_check` + bin `ultar-check`) | `tools/ultar-check/` |
| `config/check-layers.toml` | `tools/check-layers.toml` (optional – identical table is built in via `defaults/check-layers.toml`) |
| `config/check-baseline.toml` | `tools/check-baseline.toml` |
| `config/budgets.toml` | `tools/budgets.toml` (numbers are placeholders – set real budgets) |
| `ci.yml` | `.github/workflows/ci.yml` |

Integration steps:
1. Copy the crate, remove the empty `[workspace]` table from its `Cargo.toml`, add `tools/ultar-check` to the
   workspace `members`. The root manifest currently has `exclude = [..., "tools"]`; check that this does not
   keep the member out (if it does, list the other tool folders individually). `cargo run -p ultar-check` in CI
   needs it as a member.
2. Copy the three config files to `tools/`.
3. Record today's legacy violations: `cargo run -p ultar-check -- baseline > tools/check-baseline.toml`.
4. From then on `cargo run -p ultar-check -- all` must exit 0; counts may only go down.

Dependencies: `toml 0.8`, `serde 1` (derive), `serde_json 1`. No `cargo`/shell calls, no unsafe, pure Rust.

## Command line

```
ultar-check [options] <command>...
commands: all | layers | paths | edition | text | size | unsafe | unwrap | logical-layout | linux-calls | budget
          baseline   (prints a baseline for the current state as TOML)
options:  --root <dir>  --format text|json  --baseline <file>  --no-baseline  --target-dir <dir>
          --lang ar|en|de (or env ULTAR_LANG; default Arabic)  --help
```
Exit codes: 0 ok, 1 violations, 2 usage/config/I/O error (message on stderr).
Text output: `path:line: rule: detail`, warnings as `path:line: rule: warning: detail`, then `note: …` lines and a
summary line. JSON: `{passed, baselined, violations[], warnings[], notes[], summary}`; each finding has
`rule, path, line, key, args, detail` (`key` is language independent). Several rule names can be combined.

## Rules as implemented

- **layers** – reads every `Cargo.toml` with the `toml` crate. Checks `[dependencies]`, `[dev-dependencies]`,
  `[build-dependencies]` and `[target.*.…]`, `path = …` entries and `workspace = true` entries resolved through the
  root `[workspace.dependencies]`. Table in `tools/check-layers.toml` (areas with prefix/rank/internal/unrestricted,
  `[[allow]]` from/to/crates). A crate outside every area, a dependency into a non-area folder and a path leaving
  the repository are violations. Default table: apps/external → `platform/app-api`, `platform/ultar-ui`,
  `system/core/ultar-{bus,i18n,profile}`; platform → platform, system/core, `system/hal/ultar-hal`;
  services → system/core, `ultar-hal` (not each other); HAL → `ultar-hal`, system/core; core → core;
  drivers → drivers, `system/core/ultar-driverpkg`; kernel → kernel, `drivers/kernel/*`; tools → anything.
- **paths** – string literals containing `/sys/`, `/dev/`, `/proc/` outside `system/hal/`, `drivers/`, `kernel/`.
- **edition** – `Edition::…`, `ultar_profile::Edition` and `use ultar_profile::{… Edition …}` outside
  `system/core/ultar-profile/` and `system/core/ultar-init/`.
- **text** – literals with Arabic script or `äöüÄÖÜßẞ`. Exempt: tests, comments, `msg.rs` / `msg/` modules
  (the internal message tables from RULES.md) and `system/core/ultar-i18n/`.
- **size** – file > 600 lines (warning > 400), function > 60 lines (from `fn` to closing brace), struct > 15 fields
  (named and tuple). Applies to test code too.
- **unsafe** – outside `kernel/`, `drivers/`, `system/hal/`: always a violation. Inside: needs a comment containing
  `SAFETY:` on the same line or up to 3 lines above.
- **unwrap** – `.unwrap()`, `.expect(`, `panic!`, `todo!`, `unimplemented!` outside test code.
- **logical-layout** – in `platform/` and `apps/`: identifiers with a `left`/`right` word (snake and camel case),
  one finding per line, unless the line carries a comment containing `physical`.
- **linux-calls** – `Command::new("literal")`. Deviation from the task text: instead of a fixed tool list, every
  literal program counts except names starting with `ultar-`/`ultar_`; `tools/` and tests are exempt. Reason: a list
  always misses tools (mount, systemctl …). The baseline is the reduction list.
- **budget** – `tools/budgets.toml` (`[binaries] name = max_bytes`), looks in `<target>/release/` and
  `<target>/<triple>/release/`. Missing table or unbuilt binary → note, not a failure.

Test code = `#[cfg(test)]` / `#[test]` / `#[…::test]` items, `#![cfg(test)]` files, paths with `tests/` or
`benches/`, files named `tests.rs`, `*_test.rs`, `*_tests.rs`.
Skipped directories: `target`, `vendor`, `node_modules`, hidden ones.

## Baseline (ratchet)

`tools/check-baseline.toml`: `[[entry]] rule = "…" path = "…" count = N`. Per rule + path: passes while the count
is ≤ N; above N every finding of that group is listed plus a note. Notes also say when an entry can be lowered or
removed. Warnings are never baselined. Unknown rule or duplicate entry → exit 2. For `budget` the path is the
binary name.

## Library API (`ultar_check`)

- `run(args, env_lang) -> RunResult { stdout, stderr, exit_code }` – whole CLI behaviour.
- `execute(&cli::Options) -> Result<report::Report, error::CheckError>`; `baseline_toml(&Options)`.
- `cli::{parse, Options, Command, Format}`, `rules::{Rule, Context, Outcome, run_rules}`,
  `finding::{Finding, Message, Severity}`, `baseline::Baseline`, `layers_config::LayersConfig`,
  `manifest::{parse_manifest, normalize, …}`, `scan::{SourceFile, Token, TokenKind, lexer::lex}`,
  `msg::{Key, Lang, render}` (all output text, ar/en/de, Arabic first), `error::{CheckError, ConfigIssue, UsageError}`.

## Test results (this machine, `-j1`)

- `cargo fmt --check`: clean. `cargo clippy --all-targets -- -D warnings`: clean. `cargo doc`: no warnings.
- `cargo test`: **129 passed, 0 failed** – 71 unit tests, 58 integration tests
  (`tests/layers.rs` 13, `tests/scanner_rules.rs` 13, `tests/code_rules.rs` 18, `tests/cli.rs` 14).
  Every rule has positive, negative and baseline-ratchet tests on fixture trees in temp dirs; scanner edge cases
  (paths in comments, nested block comments, raw strings with hashes, byte/C strings, escapes, lifetimes vs. char
  literals, multi-line literals) are covered; the binary is tested for exit codes, JSON and the three languages.
  One test runs all rules on this crate's own sources (it passes its own gate).
- Smoke run on the current `/home/claude/ultar-os-v2` tree (read only, about 1.5 s): 5483 violations without a
  baseline (text 3154, unwrap 646, unsafe 638, paths 398, size 362, logical-layout 254, edition 33, linux-calls 21,
  layers 12), so step 3 above is required before CI can be green.

## Open points / not done

- **`ci.yml` was not executed** (no GitHub runner here). The aarch64 step could not be tried locally: the
  `aarch64-unknown-linux-musl` target is not installed. It links with `rust-lld` and installs no C cross
  compiler, so a crate with a C dependency fails there by design. `STATIC_PACKAGES` in the workflow lists
  today's core crates (`ultar-init ultar-ctl ultar-pkg ultar-secure ultar-apps`) – adjust after the rebuild.
  Clippy/tests run with `--workspace`; crates outside the workspace (kernel, server, tools) are not covered.
- `server/ultraos-server` is reported as "not inside any area" (the task's area list has no `server/`). Either add
  an area or an `exclude` entry in `tools/check-layers.toml`.
- The edition rule allows `system/core/ultar-profile`; the tree still has `system/core/ultar-edition`, which is
  flagged until it is renamed (or baselined).
- Default output language is Arabic (RULES.md); the workflow sets `ULTAR_LANG=en` for readable logs.
- Heuristic limits: Arabic written only as `\u{…}` escapes is not seen by `text`; macros that generate
  `unwrap` calls are not expanded; a `SAFETY:` comment trailing the previous statement within 3 lines also
  justifies the next `unsafe`; `Command::new(variable)` is not counted; function length inside macro bodies is
  measured like normal code; renamed dependencies are matched by their manifest key only.
- Exempt lists of the source rules (HAL prefixes, `msg` modules, i18n crate, own program prefixes) are constants in
  `src/rules/*.rs`, only the layer table is configurable.
- `budgets.toml` values are placeholders, not measured budgets.
- No QEMU boot / screenshot comparison / kernel test stage in the workflow (plan §7 pipeline tail) – out of scope.
