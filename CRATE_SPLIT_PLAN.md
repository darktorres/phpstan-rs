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
| `sql` | standalone SQL reflection library: sql_reflection_{config,schema,reflector,schema_drift} (DDL parsing, live DB reflection, drift). No knowledge of the analyser | 1.3k | 1.3k |
| `diagnostics` | Diagnostic types (from checker.rs), fix, output, baseline, ignore_errors, ignore_comments | 1.8k | 0.8k |
| `config` | config, assertion_tables, architecture, parent_calls, doctrine_functions, restricted_symbols, banned | 1.8k | 0.2k |
| `composer` | composer, deps, text_split | 1.7k | 0.4k |
| `registry` | registry, php_stubs (loader/patches) | 14.4k | 4.3k |
| `analyser` | the inference engine: scope, expression/statement typing, narrowing, control flow, generics, the `Rule`/`Scope` API and the extension traits. It emits **no** diagnostics | rest of checker | white-box tests only |
| `rules` | every diagnostic the checker reports today (431 `report()` sites: the ~40 rule modules plus the ~250 inline sites in expr/stmt/calls/oop/...) | ~25k | moves to `analysis-tests` |
| `ext-php` | built-in function/method typing: `expr/builtin_calls`, `expr/array_ops` return typing, `expr/preg_shapes`, `date_time`, built-in assertion narrowing | ~5k | " |
| `ext-sql` | analyser integration: `checker/sql_reflection` (row typing, parameter validation) + `expr/mysqli` | ~1.9k | " |
| `ext-doctrine` | `checker/doctrine`, doctrine_functions (leaves `config` in Phase 11), the Doctrine branches of `member_resolution` | ~2.5k | " |
| `ext-pest` | pest_config + the pest `$this`/hook-property plumbing | ~0.5k | " |
| `analysis-tests` | test-only crate: the black-box checker suite | - | ~65k |
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
| checker ↔ rule | `Checker`/`Locals`/`Scope` | same crate (`analyser`) |
| checker ↔ pest_config | `literal_string_value`, `Checker` | helpers → `syntax`; pest becomes `ext-pest` behind the extension traits (Phase 11) |
| registry → sql_reflection_* | `sql_reflection_config`/`schema`/`sql_reflector` fields on `Registry` | removed from `Registry` in Phase 0: an `SqlContext` owned by the project and handed to the checker (to `ext-sql` from Phase 11 on) |
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
L2  config(diagnostics, php-runtime)
L3  registry(syntax, phpdoc, types, php-runtime, config, stubs)
L4  analyser(registry + everything below, composer)
L5  rules(analyser)   ext-php   ext-sql(+sql)   ext-doctrine   ext-pest   (each: analyser)
L6  project(analyser, rules, ext-*, composer, config)
L7  lsp(project)
L8  phpstan-rs [bin](lsp, project, ...)
    analysis-tests [test-only](analyser, rules, ext-*, registry)
```

Inside each crate the module names drop the now-redundant prefixes:
`sql::{config, schema, reflector, drift}`, `ext_sql::{resolve, parameter_validation, mysqli}`, `php_runtime::{version,
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
- Each extension owns its config section: `config` keeps the raw
  `toml::Value` for `[sql_reflection]`/`[doctrine]`/`[pest]`, and the
  owning crate (`sql` for `[sql_reflection]`, the `ext-*` crates for
  the rest) parses and validates it, so `config` depends on no domain types.
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
touching `lexer.rs`, and the Piwigo scoped/unscoped snapshots and wall
times at HEAD.
Then make the module graph a DAG matching the layering above: hoist the
helpers listed under *Module graph today*, move the SQL state off
`Registry` into an `SqlContext`, move `Diagnostic*` into a
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
the DB deps and the live-DB drift tests.

**Phase 5: `config`.** `[sql_reflection]` parsing moves to `sql`, and
`config` keeps it as a raw `toml::Value`.

**Phase 6: `registry`.** registry + php_stubs → `registry::stubs`.

**Phase 7: `analyser`.** `checker/*` becomes the crate root (`checker.rs`
→ `lib.rs`, `crate::checker::X` → `crate::X`), with rule, reflection_rule
and pest_config. Extension code (SQL, Doctrine, pest, built-in function typing)
rides along inside the analyser until Phase 11 gives it the traits to
move out through. Update `checker/tests/includes.rs` (the
`dirname(__DIR__, n)` depth and the fake `main.php` path).

**Phase 8: `project`, `lsp`.** What remains in the package is the bin.

**Phase 9: tidy the extraction.**
- CI: `release.yml` and `extension-ci.yml` `cargo build --release`
  → `-p phpstan-rs` (same artifact path).
- Docs: `README.md`, `ROADMAP.md`, the fuzz guide and every doc comment
  that names a `src/...` path or `crate::` path gets the new location.
- Memory: the test command becomes `cargo test -p <crate> --lib -- <filter>`.
- Corpus: the scoped and unscoped snapshots must be byte-identical to
  Phase 0's.
- Re-measure the build timings from Phase 0 and record them in this file.

**Phase 10: the `Scope`, `Rule` and extension APIs (engine only, no moves).**
Mirror PHPStan's split between `NodeScopeResolver` (inference) and rules
and extensions:
- `Scope`: a read-only view at a node. It exposes the inferred type of any
  sub-expression (the engine caches each expression's type as it infers it,
  so rules never re-infer), locals and their certainty, the current
  class/function/closure, `$this`, PHP version, registry, config toggles,
  and `report()`.
- `Rule`: typed node callbacks for every node the 431 report sites observe
  today. Beyond expression, statement, function-like enter/exit, class and
  file, that includes the derived nodes the engine already computes: a
  resolved call (callee + mapped arguments), a return (declared return
  type), a resolved property fetch, an assignment (target + assigned
  type), an unreachable statement, a read of an undefined or maybe-undefined
  variable, and so on. Each callback has a `wants_*` prefilter, compiled once
  into a per-node-kind dispatch table.
- Extension traits: dynamic function/method/static-method return type,
  function/method type-specifying (narrowing), class reflection (magic
  properties/methods), a `$this`-type resolver, plus each extension's own
  rules.
- Built-in and third-party rules and extensions go through the same
  registration path. The existing external `Rule` users port onto the
  richer trait.

**Phase 11: extension crates.** Move `ext-php`, `ext-sql`,
`ext-doctrine` and `ext-pest` out onto the Phase 10 traits, together with
their diagnostics (e.g. SQL parameter validation, Doctrine DQL errors).
`ext-sql` takes over the `SqlContext`. One commit per extension.

**Phase 12: every diagnostic to `rules`.** The engine ends with **zero**
`report()`/`report_builder()` call sites: the only way a diagnostic gets
created is `Scope::report` from a rule. That covers the ~40 rule modules
*and* the inline checks in `expr`, `stmt`, `calls`, `oop`,
`function_decl`, `assignment`, `operators`, `control_flow`,
`comparisons`, `coercion`, `resolve` and `narrowing`. Passes that exist
only to report (`uninitialized_dataflow`, `keywords`, `whitespace`)
become file-level rules that run their own walk. `semantic_tokens`
accumulation isn't a diagnostic at all: it becomes an `lsp`-side
collector on the same node callbacks. Sub-phases, one commit each,
grouped by the node they hook:
(a) file-level; (b) expression-level; (c) calls and member access;
(d) assignments and properties; (e) declarations (functions, classes,
PHPDoc); (f) control flow and returns; (g) usage restrictions
(deprecated/internal/restricted/phpstan_api/unused).
Gate: `grep` finds no report site in `crates/analyser/src` outside the
`Scope` impl.

**Phase 13: split the test suite.**
92 of the 118 `checker/tests/*` files only call the black-box helpers
(`diagnostics_for*`, `all_diagnostics_for*`, `diagnostics_for_file`).
They move to `crates/analysis-tests`: one integration binary,
`tests/checker/main.rs` + mods, whose helpers assemble the engine with
the full built-in rule and extension set. The 26 white-box files are
split by the code they reach into: engine internals stay as `analyser`
unit tests, rule and extension internals go to their own crate. Doing this last
means each test file moves once.

Gates for Phases 10–13, per commit: test count unchanged; Piwigo scoped
and unscoped snapshots byte-identical (after `LC_ALL=C sort`); scoped
wall time within noise of Phase 0 (the dyn-dispatch overhead check).

## Risks

- **Test-path fallout.** Unit-test cwd becomes the crate directory. The
  known sites are listed above; any other breakage shows up as a test
  failure, and the unchanged-test-count gate catches silently skipped
  ones.
- **Incremental rebuild of the engine is not faster by itself.** An edit to
  the analyser still rebuilds analyser → rules/ext-* → project → lsp → bin.
  The wins are: edits below the analyser (types, registry, stubs) no
  longer recompile unrelated leaves; `cargo test -p analyser` stops at
  the analyser; Phases 11–12 take ~32k lines out of the engine, so rule
  and extension edits rebuild only their own crate and the crates above it; and
  Phase 13 takes ~65k test lines out of the engine's own compile.
- **Visibility creep.** Widening only what the compiler demands keeps the
  public surface honest. Re-check in Phase 9 with
  `cargo clippy -- -W unreachable_pub` per crate.
