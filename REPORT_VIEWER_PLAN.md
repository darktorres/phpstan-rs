# The report viewer: plan

The browser feature is ROADMAP Phase 181, "Scan once, then browse and triage in
a local web viewer" (`ROADMAP.md`, Part 12). This plan turns that phase into
rows built in this checkout. The WebAssembly playground (ROADMAP Part 15,
"Later") is not in it.

## State (2026-10-18)

- Planned; no row is built. `WORK_STACK.md` lists it as level 10.
- Built here, not in a worktree, on the current branch. One commit per row,
  made with a pathspec, because another session may share the checkout until
  level 9 is pushed.
- Starts before Stage F. The roadmap asks for F1 (the Session and
  `AnalysisResult`), E5 (levels) and F3 (the JSON schema) first. The read-only
  rows (V1 to V6) need none of them. The write rows (V7 to V11) need the
  incremental update that the post-level-9 plan's H4 replaces, so they wait for
  H4's design (see Coordination).
- Crates: approved in principle on 2026-10-18 ("we can add deps"). Each is
  judged on merit at its row.
- Not reviewed by a workflow.

## What the code gives today

Read at HEAD `a0d26204`.

Reading results:

- `AnalysisResult` (`crates/phpstan-rs/src/project/session.rs:428`) owns the
  `Project` privately. `finalize` consumes it (`session.rs:496`); `finalize_copy`
  leaves it alone (`session.rs:507`).
- `finalize_results` (`session.rs:522`) runs the level retain,
  `Project::apply_inline_ignores` (`project.rs:785`), `ignore_errors::filter`,
  `baseline::filter`, then the sort. The three filters drop findings without
  recording why.
- Inline-ignore records are keyed by line (`ignores.lines`,
  `ignores.identifiers.get(&line)`). The comment's position is not kept, so
  "which comment hid this" needs `ignore_comments.rs` to change.
- Report level and configured level are separate. `Overrides.report_level`
  reports up to a level without configuring it, and the fixture harness sets it
  to `u8::MAX` (`session.rs:1089`). The configured level also drives type
  acceptance (`session.rs:88-91`, `checker/rule_level_helper.rs`). So one
  max-level scan may not match a real run at each level. V1 measures this.
- The JSON output (`output.rs`, `JsonMessage`) has message, line, column, level,
  identifier and tip. It has no versioned schema; F3 adds one.

Writing:

- `DiagnosticFix` (`crates/diagnostics/src/lib.rs:177`) holds span, replacement,
  description and `within`. `diagnostics::fix::apply_fixes`
  (`crates/diagnostics/src/fix.rs:27`) returns the new bytes and a count. A
  zero-width span inserts text, so an ignore comment can use the same function.
- `Project::update_file_cancellable` (`project.rs:1966`) is the incremental path.
  `AnalysisResult` keeps its project private, so the viewer needs a method on it.
- The baseline (`baseline.rs`) is JSON, version 2, with entries of file, message,
  identifier and count. It has `load`, `generate` (writes the whole file) and
  `filter` (returns stale entries). It has no add-one call.
- `[[ignore_errors]]` entries are read into `SessionSettings.ignore_errors`
  (`session.rs:69`). The lock has `toml` 1.1.6 and no `toml_edit`, so a
  comment-keeping edit needs a new crate.
- The LSP offers only quick fixes from `Diagnostic.fix`
  (`diagnostic_fix_to_code_action`). There is no ignore action.
- The CLI `Command` enum (`crates/phpstan-rs/src/main.rs:33`) has no report
  command. `CheckProjectArgs` (`main/check_project.rs`) is the set of inputs to
  mirror. `keeps_freed_memory` in `main.rs` needs an arm for the new command.

Crates and gates:

- Lock: `tiny_http` (through `hotpath`), `getrandom` (three versions), `serde`
  and `serde_json`. `twox-hash` is already a dependency; its xxh3 gives the
  content hash.
- Module graph (`gates/module-graph.txt`): `crate app` may depend on `project`,
  `config`, `diagnostics` and others. A module under `app` falls under the `app`
  row by prefix, so no new row is needed.
- Arch gate (`xtask/src/arch.rs`): outside the session, the binary and the LSP,
  no `env::current_dir`, `fs::canonicalize` or `path::absolute`. The viewer takes
  its absolute root from the session.
- CI (`rust-ci.yml`): fmt, clippy with `-D warnings`, tests, fixtures, module
  graph, arch, the test-name manifest, tests and fixtures without default
  features, docs, benches and fuzz compile, each feature alone, `cargo machete`,
  `cargo deny`. The Windows and macOS legs run the tests and fixtures.

The post-level-9 plan (`~/.claude/plans/another-session-is-finishing-majestic-comet.md`)
redesigns the session as a salsa pipeline (its item 15, "Designed: H4") and keeps
`AnalysisResult` as its value, owning the inline-ignore records. Its LOAD-2.2
last part also touches `project/session.rs` and `project.rs`.

## Decisions

Each default is the recommendation. Change any of them. Answered on 2026-10-18:
decisions 2 (writes after H4), 4 (trailing comment) and 5 (served writes on), as
recommended. The other defaults stand until changed. Decision 3 waits for V1.

| # | Decision | Default |
|---|---|---|
| 1 | Feature | Phase 181 only. The playground is not in this plan. |
| 2 | Timing | V1 now, since it only measures. V2 onward after level 9 is pushed, because V2 and V9 touch files that batch changes. V7 to V11 (writes) also wait for H4's seam to settle. |
| 3 | Level strategy | Set by V1. One scan if it matches every real run; otherwise one analysis per level (eleven in all) until E5 makes levels parameters. |
| 4 | Ignore placement | A trailing `// @phpstan-ignore <identifier>` on the finding's start line. It moves no line, so the file's other findings keep their positions. Refused for non-ignorable findings, and for a line that ends inside a string, a heredoc, a block comment or after `?>`. |
| 5 | Writes | On in served mode, behind the token. `--read-only` turns them off. |
| 6 | Missing config | The first config edit creates `phpstan-rs.toml`. Undo removes it. |
| 7 | Apply fix | The whole run is re-scanned after the write, because a fix can change code other files see. |
| 8 | Undo | In memory, for the run. |
| 9 | Browser | The URL is printed. No automatic open in v1. |
| 10 | Crates | `tiny_http` and `getrandom` at their locked versions. `toml_edit` is new, chosen at V9. |
| 11 | Export | Each finding embeds three lines of source around its start. No absolute root. No copies of PHP files. |

## Rows

Each row is checked before its commit: `cargo check`, clippy on the crate it
touches, and its own tests. The checkpoints run the full suites.

| ID | Row | Gate |
|---|---|---|
| V1 | Level strategy. For each project in the corpus and each level 0 to 10, compare a real analysis at that level with one max-level scan, filtered by each finding's level tag. The max scan is run twice: with the settings the fixture harness uses (report level max, configured level unset), and with configured level max. Corpus: the fixture suite and general PHP packages under `vendor/` (symfony, laravel, guzzle, monolog, psalm). Piwigo is a regression check only. | The mismatch table is written into this plan, and decision 3 is recorded from it. |
| V2 | Hidden-by record. `finalize_results` records what each filter removed: the inline comment's position (the ignore records gain it), the `ignore_errors` entry index, and the baseline entry with its count. `Finalize` gets a flag, off by default. | Each mechanism has a fixture that asserts its record. The fixture suite and CLI output are unchanged. One callgrind count of a short fixture run is unchanged (CLAUDE.md's short-run rule). |
| V3 | Data model. `app::report::data`: `ViewerData` with `schema_version` 1. Files carry their report path with `/`, an xxh3 hash and a checked flag. Findings carry an id, file, start and end, identifier, message, tips, level tag, non-ignorable flag, fix preview and hidden record. Notices carry unmatched ignores, stale baseline entries and lexer warnings. Under per-level scans, findings merge by file, start, identifier and message, and a message that differs by level keeps each variant with its level range. | Unit tests for each field. CRLF and BOM fixtures hash the bytes they read. Output is sorted and deterministic. |
| V4 | Export. `phpstan-rs report --export <dir>` writes `index.html`, `app.css`, `app.js` (embedded with `include_str!`) and `data.js` (`window.REPORT = ...`). Read-only. | A test writes under `~/.cache`, not `/tmp`. A second export is byte-identical. `data.js` parses. No absolute path appears. |
| V5 | The page. Vanilla HTML, CSS and JavaScript, no toolchain. Data is rendered only with `textContent`. Filters: level slider (default: the level `check-project` reports), rule family, path tree, search, new versus baselined, hidden toggle with its reason, unmatched ignores. A virtualized list, and a detail pane with snippet, tips and fix preview. | A Playwright pass on an export of a fixture: each filter's count equals the `check-project --format json` count at that level. A message holding `<script>` shows as text. |
| V6 | Served, read-only. `phpstan-rs report` serves the same page on 127.0.0.1 (`--port`, default ephemeral). A 128-bit token from `getrandom` travels in the URL fragment, and the page sends it in an `X-Phpstan-Token` header on every API call. Routes: `/` (page), `/api/data`, `/api/source`. In served mode the data never loads as a script, so another site cannot read it through a script tag. The Host header must be 127.0.0.1 or localhost with the port. An Origin header, when present, must be this server's own. Responses carry a CSP header, `frame-ancestors 'none'`, `nosniff` and `no-referrer`. | Raw-socket tests: no token gives 403, wrong Host 403, wrong Origin 403, a `..` path refused, the right token 200, headers present. Checkpoint A runs here. |
| V7 | Write guard and undo. The content hash at scan time is kept per file. Before each write the disk hash must match, or the write is refused. Each write returns an undo id. Undo restores the previous bytes only if the file still holds what the write left. | An edit between scan and write is refused. Undo restores the bytes exactly, CRLF and BOM included. |
| V8 | Ignore here. A trailing `// @phpstan-ignore <identifier>` goes before the line terminator, through `apply_fixes` with a zero-width span. Refused for non-ignorable findings, for lines that end inside a string, a heredoc, a block comment or after `?>` (the lexer's token spans decide), and when the identifier is already ignored. The file is then re-read through `update_file_cancellable`, and each result is merged through one new method on `AnalysisResult`. The edit is comment-only, so no other file's findings change. Under per-level scans every level's result is updated. | Fixtures for CRLF, BOM, a heredoc, `?>`, a line with a trailing comment and a non-ignorable finding. After the write, the finding shows as hidden by that comment. |
| V9 | Ignore identifier, or a path glob. Appends an `[[ignore_errors]]` table to `phpstan-rs.toml` with `toml_edit`: the identifier, and a path spelled with `/` and matched by the rule that R6 and R7 settle. Other bytes are unchanged. Creates the file on first use (decision 6). | A commented config keeps every comment and every other table byte for byte. A path written with a backslash matches on Linux. Undo restores the file. |
| V10 | Add to baseline. `Baseline::add` (count plus one, or a new entry), and a save that keeps includes. A per-identifier baseline is an index of files, so it is refused with a message. | An include chain keeps its entries. The per-identifier layout is refused. Undo restores the file. |
| V11 | Apply fix. Applies the finding's fix with `apply_fixes` under the write guard, then re-scans the run (decision 7). | The fixed finding is gone after the re-scan. Undo restores the source and re-scans. |
| V12 | Cross-platform. Loopback tests and the CRLF and BOM tests run on the Windows and macOS legs. Browser opening is out of v1. Each push is asked first (`CROSS_PLATFORM_PLAN.md`). | The legs pass at checkpoint C. |
| V13 | Checks and docs. New crates pass `cargo deny` and `cargo machete`. Tests pass without default features. The module graph and the arch gate pass. A test rename needs an entry in `gates/test-renames`. README gets a `report` section. `ROADMAP.md`'s Phase 181 gets a status line when V1 starts (the phase has none today). | All of CI at checkpoint C. |

Later, not in this plan: watch mode (`notify`, F2, the VFS); E5's single scan,
if V1 chooses per-level runs; F3 adopting `ViewerData`; the playground.

## Dependencies

Judged at their rows. The workspace pins every dependency exactly (`=x.y.z`, as
`Cargo.toml` does), and new ones follow suit.

- `tiny_http`: already locked through `hotpath`. Alternative: a hand-written
  HTTP/1.1 reader on `std::net`, about 200 lines to test.
- `getrandom`: already locked. Alternative: `rand`, also locked, heavier.
- `toml_edit`: new. Alternative: string edits, which cannot keep comments inside
  a table being edited.
- `twox-hash`: already a dependency.

## Coordination

- H4 (the other plan's item 15): `AnalysisResult` stays a value that holds the
  inline-ignore records. V2 adds the comment position to those records, and H4
  must keep it. V7 onward use `update_file_cancellable` through one method on
  `AnalysisResult`; H4's `update(FileDelta)` replaces that method.
- Level 8's LOAD-2.2 (`project/session.rs`, `project.rs`, `project/walk.rs`) and
  level 9's R6 to R8 (`diagnostics` path rules, `ignore_errors`): do not start V2
  or V9 while either is in flight.
- Phase 183's `init` uses the same per-level function as V1 ("findings per
  level", `ROADMAP.md` line 5313).
- Phase 188: V2 is its "what it hid" record. Its stderr count lines are a
  separate row with approved CLI output changes.
- No crate yet. A viewer crate that depends on the `phpstan-rs` package cannot be
  a dependency of the package's binary; Cargo rejects the cycle. The module graph
  already names a future `app` crate, and the viewer lives under `app` until then.

## Checkpoints and conventions

- A (after V6): full `cargo test --workspace`, fixtures, module graph, arch gate,
  `cargo deny`, `cargo machete`, clippy, fmt, and the V5 browser pass.
- B (after V11): the same, with V1's corpus rerun on the final data path.
- C (after V12): CI on all three systems, after the push is approved.
- Scratch and test temp dirs go under `~/.cache`. Heavy jobs run one at a time.
- Code comments state what the code does, with no phase or task references.
  No `innerHTML`. No `unsafe`. Snapshot diffs use `LC_ALL=C sort`.
