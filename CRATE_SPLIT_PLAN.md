# Splitting phpstan-rs into a Cargo workspace

## Where we are

One package, one library crate: 216k lines, of which about 98k are unit
tests. Production code by area:

| Proposed crate | Current modules | Prod LOC | Test LOC |
|---|---|---:|---:|
| `syntax` | token, intern, lexer, ast, parser, ast_walk, printer | 9.2k | 3.6k |
| `phpdoc` | phpdoc_grammar, docblock | 3.4k | 1.8k |
| `types` | types (+ type algebra hoisted from checker) | 3.2k (+~0.5k) | 2.0k |
| `php-runtime` | php_version, php_float_format, php_date_parser, regex_groups, pcre_validate | 2.4k | 0.9k |
| `stubs` | build.rs + the generated `include_str!` arrays | build script | - |
| `sql` | sql_reflection_{config,schema,reflector,schema_drift} | 1.3k | 1.3k |
| `diagnostics` | Diagnostic types (from checker.rs), fix, output, baseline, ignore_errors, ignore_comments | 1.8k | 0.8k |
| `config` | config, assertion_tables, architecture, parent_calls, doctrine_functions, restricted_symbols, banned | 1.8k | 0.2k |
| `composer` | composer, deps, text_split | 1.7k | 0.4k |
| `registry` | registry, php_stubs (loader/patches) | 14.4k | 4.3k |
| `analyser` | checker (minus whole-program passes), rule, reflection_rule, pest_config | 63.5k | 69.9k |
| `rules` | reporting-only rule modules pulled out of checker (Phase 11) | ~14.7k | - |
| `project` | project + checker/whole_program + duplicate_{classes,functions} | 7.2k | 5.4k |
| `lsp` | lsp | 6.6k | 6.6k |
| `phpstan-rs` (bin) | main.rs, main/ | 2.3k | 0.1k |

## Module graph today

Computed from `crate::<module>` references. It is almost a clean DAG; the
only cycles are misplaced helpers:

| Edge | What crosses it | Fix |
|---|---|---|
| registry → checker | `union_types`, `unwrap_grouping`, `is_superglobal`, `has_explicit_mixed_nested_somewhere` | `union_types` family and `has_explicit_mixed_nested_somewhere` → `types`; `unwrap_grouping`, `literal_string_value`, `is_superglobal` → `syntax` (AST helpers) |
| pest_config → checker | `literal_string_value`, `unwrap_grouping` | same move |
| baseline, output, fix, ignore_errors, restricted_symbols, reflection_rule → checker | `Diagnostic`, `DiagnosticBuilder`, `DiagnosticFix` | move the three types into `diagnostics` (they only need `Span` + `serde_json`) |
| checker → project | `use_sites`, `duplicate_{class,fn}_sites` | move `checker/whole_program/*`, `checker/duplicate_classes.rs`, `checker/duplicate_functions.rs` into `project`: they consume project-level indexes, never `Checker` |
| checker ↔ rule, checker ↔ pest_config | `Checker`/`Locals`/`Scope` | same crate (`analyser`) |
| registry ↔ php_stubs | `Registry` | same crate (`registry`) |

Also verified:
- No inherent `impl` block crosses a proposed crate boundary (the
  `impl Parser` in `checker/doctrine/dql.rs` and `impl Lexer` in
  `phpdoc_grammar/lexer.rs` are local DQL/phpdoc types).
- No trait-impl orphan-rule problems: every `impl Display for Type`-style
  impl lives beside its type.
- `CARGO_MANIFEST_DIR` isn't read anywhere except build.rs. The
  cwd-sensitive places are: `checker/tests/includes.rs`
  (`dirname(__DIR__, 3) . '/Cargo.toml'`, `src/checker/tests/main.php`),
  `baseline_filtering.rs` (`src/../t.php`, pure string normalization),
  `benches/vendored_corpus.rs` (`vendor/...`), and the tempdir fixtures
  in project/lsp/composer tests (relative to their tempdir, unaffected).

## Target layout

Virtual workspace root; every crate under `crates/`, `publish = false`,
package names prefixed `phpstan-rs-`, depended on under a short alias
through `[workspace.dependencies]` so code reads `syntax::ast::Expr`,
`types::Type`, `registry::Registry`:

```toml
[workspace.dependencies]
syntax = { package = "phpstan-rs-syntax", path = "crates/syntax" }
types  = { package = "phpstan-rs-types",  path = "crates/types" }
# ...
```

```
L0  syntax   types   php-runtime   stubs
L1  phpdoc(syntax)  sql(types)  diagnostics(syntax)  composer(syntax)
L2  config(sql, diagnostics, php-runtime)
L3  registry(syntax, phpdoc, types, php-runtime, sql, config, stubs)
L4  analyser(registry + everything below, composer)
L5  rules(analyser)
L6  project(analyser, rules, composer, config)
L7  lsp(project)
L8  phpstan-rs [bin](lsp, project, ...)
```

Inside each crate the module names drop the now-redundant prefixes:
`sql::{config, schema, reflector, drift}`, `php_runtime::{version,
float_format, date_parser, regex_groups, pcre}`, `syntax::walk` (was
`ast_walk`), `registry::stubs` (was `php_stubs`), `analyser::expr`
(was `checker::expr`), and so on. There are no re-export shims (`pub(crate) use
syntax::lexer;`) to keep old `crate::lexer` paths compiling: every path
is rewritten to its real home.

The root `phpstan-rs` package disappears as a library: `lib.rs` and its
re-exports go. The binary moves to `crates/phpstan-rs` (package name
unchanged, so `target/release/phpstan-rs`, `cargo install`, and the
VSCode extension's binary lookup are unaffected). Fuzz and bench depend
on the crates they actually exercise.

Workspace-level:
- `[workspace.package]` (edition 2024, version) and
  `[workspace.dependencies]` for every third-party dep; members use
  `.workspace = true`.
- `[profile.*]` and `[patch.crates-io] hotpath` stay at the root.
- `exclude = ["fuzz", "third_party/hotpath"]`.
- Features: `pcre-validate` lives in `php-runtime` (default on) and is
  forwarded by the bin; `fuzzing` (`arbitrary` derives on
  `Token`/`Keyword`/`StringPart`/`Position`) in `syntax`; `hotpath` /
  `hotpath-alloc` declared in every crate that uses `#[hotpath::measure]`
  (26 files) and forwarded by the bin; `dhat-heap` stays bin-only
  (global allocator).
- Heavy C/DB deps (`mysql`, `postgres`, `rusqlite` bundled SQLite,
  `r2d2*`, `sqlparser`) become `sql`-only; `lsp-server`/`lsp-types`
  `lsp`-only; `clap` bin-only; `roxmltree` wherever the XML modules land.

## Visibility

`pub(crate)` counts today: checker 805, registry 143, lsp 178, parser
112, project 50, types 25. Only items that actually cross a new crate
boundary become `pub`; the compiler enumerates them at each step. Nothing
is widened speculatively. Registry fields read directly by the checker
become `pub` fields; where a field is only ever read through one pattern,
add an accessor instead.

## Phases

One commit per phase. Each commit builds, is clippy-clean, and passes the
whole test suite, and **the passed-test count must equal the
baseline** (4866 lib + integration tests today), so no test gets lost in
a move. Use `git mv` so rename detection keeps blame. Content edits in
the same commit are limited to path rewrites.

**Phase 0: baseline and de-cycle (still one crate).**
Record the baseline: clean `cargo build --timings`, incremental
`cargo test --lib --no-run` after touching `checker/expr.rs` and after
touching `lexer.rs`, and the Piwigo scoped/unscoped snapshots at HEAD.
Then make the module graph a DAG matching the layering above: hoist the
helpers listed under *Module graph today*, move `Diagnostic*` into a
`diagnostic` module, and move the whole-program passes and duplicate
checks under `project`. Gate: the graph script shows no edge pointing
upward.

**Phase 1: workspace skeleton + `stubs`.**
Root `Cargo.toml` becomes a workspace that still contains the current package
as a member (moved to `crates/phpstan-rs`). Extract `stubs` with
build.rs: `vendor/` paths become `CARGO_MANIFEST_DIR/../../vendor`, and
the submodule-empty check and `rerun-if-changed` lines move with it. From
now on the embedded stub blob only recompiles when the vendored stubs
change.

**Phase 2: `syntax`.** token, intern, lexer, ast, parser, ast_walk →
`walk`, printer, plus the hoisted AST helpers. Carries the `fuzzing`
feature. Repoint `fuzz/Cargo.toml` at it (and at `types` for the
`parse_type_string` target), and `benches/vendored_corpus.rs` (moves
into `syntax/benches`, path `../../vendor/...`).

**Phase 3: `types`, `php-runtime`.** Both are leaves after Phase 0.

**Phase 4: `phpdoc`, `sql`, `diagnostics`, `composer`.** `sql` takes
the DB deps; `sql_reflection_schema_drift`'s live-DB tests move with it.

**Phase 5: `config`.**

**Phase 6: `registry`.** registry + php_stubs → `registry::stubs`.

**Phase 7: `analyser`.** `checker/*` becomes the crate root (`checker.rs`
→ `lib.rs`, `crate::checker::X` → `crate::X`), with rule, reflection_rule
and pest_config. Update `checker/tests/includes.rs` (the
`dirname(__DIR__, n)` depth and the fake `main.php` path).

**Phase 8: `project`, `lsp`.** What remains in the package is the bin.

**Phase 9: tidy.**
- CI: `release.yml` and `extension-ci.yml` `cargo build --release`
  → `-p phpstan-rs` (same artifact path).
- Docs: `README.md`, `ROADMAP.md`, the fuzz guide and every doc comment
  that names a `src/...` path or `crate::` path gets the new location.
- Memory: the test command becomes `cargo test -p <crate> --lib -- <filter>`.
- Corpus: the scoped and unscoped snapshots must be byte-identical to
  Phase 0's.
- Re-measure the build timings from Phase 0 and record them in this file.

**Phase 10: split the analyser's test suite.**
92 of the 118 `checker/tests/*` files only call the black-box helpers
(`diagnostics_for*`, `all_diagnostics_for*`, `diagnostics_for_file`).
Move them to a dedicated test-only crate `crates/analysis-tests`: one
integration binary, `tests/checker/main.rs` + mods. It dev-depends on
`analyser`, `rules` and `registry`, so the helpers can assemble the full
built-in rule set. The 26 files that reach internals (`Locals`,
`super::super::*`, private fns) stay as unit tests. After that,
`cargo build` of the analyser no longer type-checks ~60k lines of tests,
and test binaries compile in parallel with the lib's codegen.

**Phase 11: extract the reporting rules into `rules`.**
This is the change that shrinks the engine. The checker currently runs
~40 reporting-only modules as inherent `impl Checker` methods called inline
from `check_expr`/`check_stmt`. Inherent impls can't leave the crate, so
the move goes through the existing `rule::Rule` extension point, which
becomes the *only* mechanism: built-in and third-party rules alike, as in
PHPStan.

- Engine side: extend `Rule` with `check_stmt`, `check_function_like`,
  `check_class`, `check_file` hooks next to `wants_expr`/`check_expr`,
  each with its own `wants_*` prefilter. Precompute the prefilters into a
  per-node-kind dispatch table once per `Checker`, so an unused hook costs
  nothing. `Scope` exposes the read-only queries the rules use:
  the inferred type of an expression, locals lookups, registry, current
  class/function, PHP version, config toggles, and `report()`.
- `rules::builtin(&Config) -> Vec<Box<dyn Rule>>` replaces the inline
  calls, and `project`/`lsp` wire it in.
- Eligible modules (~14.7k LOC) are the ones that only read engine state and
  emit diagnostics: phpstan_api, unused, strict_rules,
  restricted_symbol_usage, missing_typehint, phpdoc_type_compat,
  internal_usage, noop, throws, psr3, native_type_hints,
  phpdoc_unresolvable, keywords, strict_idioms, castable_arguments,
  access_attributes, visibility, deprecated_usage, magic_numbers,
  duplicates, current_time, named_argument_renames, ini_directives,
  impossible_instanceof, date_interval, return_type_too_wide,
  pipe_operator, property_type_too_wide, xml_name, preg_pattern_validity,
  xml_wellformedness, whitespace, internal_rules, risky_tests. Anything
  that turns out to write inference state (e.g. `uninitialized_dataflow`,
  `semantic_tokens` accumulation) stays in the engine. Decide per module
  against that criterion while migrating it, not up front.
- Sub-phases, one commit each:
  (a) file-level rules (whitespace, xml_wellformedness, keywords);
  (b) self-contained expression rules (magic_numbers, psr3,
  preg_pattern_validity, ini_directives, xml_name, current_time,
  date_interval, castable_arguments, pipe_operator,
  impossible_instanceof);
  (c) strictness rules (strict_rules, strict_idioms, noop);
  (d) declaration rules (missing_typehint, native_type_hints, phpdoc_*,
  visibility, access_attributes, *_type_too_wide, duplicates,
  named_argument_renames, throws, risky_tests);
  (e) usage rules (deprecated/internal/restricted_symbol_usage,
  phpstan_api, unused).
- Gates per sub-phase: test count unchanged; Piwigo scoped and unscoped
  snapshots byte-identical (after `LC_ALL=C sort`); scoped wall time
  within noise of the Phase 9 measurement (dyn dispatch overhead check).

## Risks

- **Test-path fallout.** Unit-test cwd becomes the crate directory. The
  known sites are listed above; any other breakage shows up as a test
  failure, and the unchanged-test-count gate catches silently skipped
  ones.
- **Incremental rebuild of the engine is not faster by itself.** An edit to
  the analyser still rebuilds analyser → rules → project → lsp → bin.
  The wins are: edits below the analyser (types, registry, stubs) no
  longer recompile unrelated leaves; `cargo test -p analyser` stops at
  the analyser; Phase 10 takes 60k test lines out of the lib's check; and
  Phase 11 takes ~15k lines out of the engine, so rule edits rebuild only
  `rules` and above. Phase 9's measurements decide whether any further
  engine split (e.g. `doctrine`/`sql_reflection` typing behind a
  trait) is worth a follow-up.
- **Visibility creep.** Widening only what the compiler demands keeps the
  public surface honest. Re-check in Phase 9 with
  `cargo clippy -- -W unreachable_pub` per crate.
