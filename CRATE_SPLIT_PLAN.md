# Splitting phpstan-rs into a Cargo workspace

The goal is the target architecture, not today's modules repackaged into
crates. It mirrors PHPStan's own split:
- an inference engine that emits no diagnostics;
- rules and collectors that observe it through a read-only `Scope`;
- extensions for library- and framework-specific typing and rules;
- a reflection layer behind an API, not a struct of public maps.

Crate boundaries are cut along those APIs, so the compiler enforces the
layering from then on.

## Where we are

One package, one library crate: 216k lines, of which about 98k are unit
tests. The debt the split has to remove, all measured on `89d468c`:

| Debt | Evidence |
|---|---|
| Engine reports diagnostics inline | 431 `report()`/`report_builder()` call sites under `checker/`; ~250 of them in `expr`, `stmt`, `calls`, `oop`, `function_decl`, `assignment`, `operators`, `control_flow`, `comparisons`, `coercion`, `resolve`, `narrowing` |
| `Checker` does everything | 70 fields: inference state, but also cross-file facts (`constant_facts`, `property_assign_facts`, `named_argument_call_facts`, `renamed_parameter_override_facts`, `cross_file_property_assign_facts`, `facts_only`), LSP features (`semantic_tokens`, `probe_*`, `inlay_*`, `member_ref_sites`, `collect_member_refs`), rule-only bookkeeping (`current_function_throw_points`, `current_function_explicit_throw_points`, ...), and pest state (`pest_this_type`, `pest_hook_properties`) |
| `Registry` does everything | 83 `pub` fields: reflection maps, **58 config toggles and settings** (`check_magic_numbers`, `restricted_symbols`, `php_version`, ...), and SQL state (`sql_reflection_config`, `sql_reflection_schema`, `sql_reflector`, `sql_reflection_cache`). The checker reads `registry.classes` directly 126 times |
| Library knowledge in the engine | Doctrine (`checker/doctrine`, the DBAL/ORM branches of `member_resolution`, `Locals.query_builder_aliases`), SQL/mysqli, pest, built-in function typing (`builtin_calls` 3.3k lines, `preg_shapes`, `date_time`), and PHPUnit spread across 18 files (assertion narrowing in `stmt.rs`/`narrowing/assertions.rs`, `risky_tests`, test-method entry points in `dead_members`, `TEST_BASE_CLASSES` in `access_attributes`, `parent_calls` defaults, `mock_hygiene`). Mockery lives in `assertion_tables` defaults and `mock_hygiene` |
| Project-wide checks are custom passes | `Project::check_{architecture,banned,class_shapes,mock_hygiene}`, `composer_hygiene`, `checker/whole_program/*`, the duplicate-declaration checks, plus the `facts_only` throwaway pass that exists only to harvest property-assignment facts |
| Rule config lives far from its rule | `restricted_symbols`, `banned`, `architecture`, `parent_calls`, `doctrine_functions`, `assertion_tables` are top-level modules that `config` depends on |
| Misplaced utilities | `text_split` re-lexes spans (a syntax utility); `deps` computes incremental re-check dependencies (a project concern) |

Module cycles today are all misplaced helpers:
- `registry` and `pest_config` import `union_types`, `unwrap_grouping`,
  `literal_string_value`, `is_superglobal` and
  `has_explicit_mixed_nested_somewhere` from the checker.
- Six modules import `Diagnostic*` from `checker.rs`.
- The checker imports the `project` indexes, for the whole-program
  passes only.

Verified non-blockers:
- No inherent `impl` block crosses a boundary. The `impl Parser` in
  `doctrine/dql.rs` and the `impl Lexer` in `phpdoc_grammar/lexer.rs` are
  local types.
- There are no orphan-rule trait impls.
- `CARGO_MANIFEST_DIR` is only read in build.rs.
- The cwd-sensitive tests are `checker/tests/includes.rs` (a
  `dirname(__DIR__, 3)` depth and a fake `src/checker/tests/main.php`
  path) and `benches/vendored_corpus.rs`. The rest are tempdir-relative.

## Target architecture

```
L0  syntax        token, intern, lexer, ast, parser, walk, printer, text_split, AST helpers
    types         Type, parsing, display, subtyping, union/merge algebra
    php-runtime   PHP version, float formatting, date parsing, regex groups, PCRE validation
    stubs         build.rs + embedded phpstorm-stubs / phpstan-src sources
L1  phpdoc        PHPDoc grammar + docblock model                        (syntax)
    sql           standalone SQL reflection: config, DDL schema, live
                  reflector, drift. Knows nothing about PHP analysis      (types)
    diagnostics   Diagnostic/Builder/Fix, fixes, output formats,
                  baseline, ignoreErrors, inline ignore comments          (syntax)
    composer      composer.json/installed.json model, vendor context      (syntax)
L2  config        loads phpstan-rs.toml: global settings (paths, level,
                  PHP version, ignores, baseline). Every other section
                  stays a raw toml::Value for its owner to parse          (diagnostics, php-runtime)
L3  reflection    (was registry) ReflectionProvider API: class/function/
                  constant reflection, name resolution, PHPDoc→type
                  resolution, stub loading. Private storage           (syntax, phpdoc, types, php-runtime, stubs)
L4  analyser      the engine: traversal, Scope, expression/statement
                  inference, narrowing, control flow, generics; the
                  Rule / Collector / Observer / extension traits;
                  AnalysisContext. No diagnostics of its own           (reflection, composer, config, diagnostics)
L5  rules                 every core PHPStan-equivalent rule + its config,
                          incl. project-level collector rules (dead code,
                          duplicates, circular namespaces, architecture,
                          banned, class_shape, composer hygiene,
                          restricted symbols, parent calls)
    ext-strict-rules      phpstan-strict-rules equivalent (strict_rules, strict_idioms, ...)
    ext-deprecation-rules phpstan-deprecation-rules equivalent
    ext-php               built-in function/method return typing + narrowing
    ext-sql               SQL row typing, parameter validation, mysqli     (+ sql)
    ext-doctrine          DQL/QueryBuilder typing, DQL rules, doctrine_functions
    ext-pest              `$this` resolution, hook properties
    ext-phpunit           assertion narrowing, test-method entry points,
                          risky tests, TestCase parent-call defaults,
                          mock hygiene (PHPUnit half)
    ext-mockery           mock typing (built-in assertion-table entries),
                          mock hygiene (Mockery half)
L6  project       file discovery, scheduling, the collection pass, the
                  incremental dependency graph (deps), LSP indexes
                  (use_sites, decl_locations); wires rules + extensions
                  from config
L7  lsp           protocol + features; semantic tokens, inlay hints,
                  probes, member references as analyser Observers
L8  phpstan-rs    the binary (package name kept)
    analysis-tests  test-only crate: the black-box engine+rules suite
```

Conventions:
- Virtual workspace root; `crates/<name>`; package `phpstan-rs-<name>`,
  `publish = false`, used under a short alias via
  `[workspace.dependencies]` (`syntax = { package = "phpstan-rs-syntax",
  path = "crates/syntax" }`), so code reads `syntax::ast::Expr`,
  `reflection::ClassReflection`.
- Module names drop the redundant prefixes (`sql::{config, schema,
  reflector, drift}`, `php_runtime::{version, float_format, ...}`,
  `syntax::walk`, `reflection::stubs`, `analyser::expr`).
- No re-export shims keeping old `crate::` paths alive; every path is
  rewritten to its real home.
- The root `lib.rs` and its re-exports go away. Fuzz, benches and CI
  depend on the crates they actually use. The binary keeps the package name
  `phpstan-rs`, so `target/release/phpstan-rs`, `cargo install`
  and the VSCode extension are unaffected.
- `[workspace.package]`, `[workspace.dependencies]`, root-level
  `[profile.*]` and `[patch.crates-io] hotpath`, and
  `exclude = ["fuzz", "third_party/hotpath"]`.
- Features: `pcre-validate` in `php-runtime` (default on, forwarded by
  the bin); `fuzzing` in `syntax`; `hotpath`/`hotpath-alloc` in every
  crate with `#[hotpath::measure]`, forwarded by the bin; `dhat-heap`
  bin-only.
- Heavy deps land where they're used: DB drivers, bundled SQLite and
  sqlparser in `sql`; `lsp-server`/`lsp-types` in `lsp`; `clap` in
  the bin.

### The APIs the boundaries are cut along

**`reflection::ReflectionProvider`.** Storage is private. Lookups are
methods: `class(name)`, `function(name)`, `constant(name)`,
`is_subclass_of`, `ancestors`, `classes()` iteration for whole-program
collectors, member lookup with owner, assignability and super-type
queries. Config toggles and settings leave it entirely: each goes to the
rule or extension that reads it, and `php_version` moves to
`AnalysisContext`. Caches stay internal. Mutation happens only during the
build (stub merge, project scan), through a separate `ReflectionBuilder`,
so an analysis can't mutate reflection.

**`analyser::Scope`.** A read-only view at a node:
- the inferred type of any sub-expression (the engine caches each
  expression's type while inferring it, so rules never re-infer), its
  native type, and certainty;
- the current class/function/closure, `$this`;
- `AnalysisContext` (reflection, PHP version);
- `report()`.

**`analyser::Rule`.** Typed node callbacks, each with a `wants_*`
prefilter compiled once into a per-node-kind dispatch table:
- expression, statement, function-like enter/exit, class, file;
- the derived nodes the engine already computes, as PHPStan does with
  `FunctionReturnStatementsNode`, `ClassPropertiesNode`, ...:
  - a resolved call (callee + mapped arguments);
  - a function exit (return points, throw points, by-ref exit states);
  - a class exit (property initialization state);
  - a resolved property/constant fetch;
  - an assignment (target + assigned type);
  - an unreachable statement;
  - a read of an undefined or maybe-undefined variable.

Each rule declares its identifiers statically, so `ignoreErrors`
identifiers and `--generate-baseline` can be validated against the real
set.

**`analyser::Collector`.** The same callbacks, but instead of reporting it
returns per-file data. Project-level rules consume the collected data
from every file after the pass. This replaces the `Project::check_*`
passes, `whole_program/*`, the duplicate checks, and the `*_facts` fields
on `Checker`. Cross-file facts that feed *inference* (property-assignment
facts) come from a collection pass run with an empty rule set. That is
what `facts_only` was emulating, and the flag goes away.

**`analyser::Observer`.** The same callbacks with no output contract,
for editor features: semantic tokens, inlay hints, hover probes, member
references. These leave `Checker` for `lsp`.

**Extension traits:**
- dynamic function/method/static-method return type;
- function/method/static-method type-specifying (narrowing);
- class reflection (magic properties/methods);
- `$this`-type resolver;
- always-used / entry-point members (dead-code roots such as PHPUnit test
  methods);
- read/write property extension.

Each extension may also ship rules and collectors. Extensions that carry
state through a function body (Doctrine query-builder aliases) get a
typed per-extension slot in the scope, merged at control-flow joins by
the extension's own merge function. That replaces
`Locals.query_builder_aliases`.

Built-in and third-party rules, collectors, observers and extensions all
register the same way. The existing external `Rule`/`ReflectionRule`
users port onto the new traits.

**Engine state after the moves.** `Checker` keeps only what inference
reads:
- `AnalysisContext` (reflection, config view, registered extensions,
  dispatch tables);
- per-file state (name context, include-resolution state);
- a per-function-like state stack (declared return/throws types, loop
  break/continue states, by-ref bookkeeping the inference itself uses).

Gate: every remaining field has an inference-side reader.

**Assertion tables.** The user-configured `[[assertions]]` and
`[[class_argument_returns]]` mechanism becomes a config-driven
type-specifying/return-type extension in `analyser::extensions`, the
engine's one built-in extension (like PHPStan's own stub-file support).
Its built-in Mockery entries move to `ext-mockery`.

## Visibility

Crates export their designed APIs; nothing is widened just because the
compiler asks. When a cross-crate use shows up that the API doesn't
cover, the API gets the missing operation. `unreachable_pub` is
denied workspace-wide once Phase 16 lands.

## Phases

Every commit builds, is clippy-clean, and passes the whole test suite
with **the same passed-test count as before** (so a move can't silently drop
a test). Every commit from Phase 7 on also keeps the Piwigo scoped and
unscoped snapshots byte-identical (after `LC_ALL=C sort`). Moves use
`git mv` so blame follows.

**Phase 0: baseline and de-cycle (one crate).**
- Record the baseline: clean `cargo build --timings`, incremental
  `cargo test --lib --no-run` after touching `checker/expr.rs` and
  `lexer.rs`, and the Piwigo scoped/unscoped snapshots and wall times.
- Hoist the helpers above to `syntax`/`types`, `Diagnostic*` into a
  `diagnostic` module, and `text_split` under the syntax modules.
- Gate: a module graph with no upward edge. The whole-program-pass edge
  is handled by Phase 12, so it's temporarily allowed.

**Phase 1: workspace skeleton + `stubs`.** The package moves to
`crates/phpstan-rs` inside a virtual workspace. `stubs` gets build.rs
(`../../vendor`, the submodule check, `rerun-if-changed`).

**Phase 2: `syntax`.** Carries `fuzzing`. The fuzz targets repoint to
`syntax`/`types`; the bench moves to `syntax/benches`.

**Phase 3: `types`, `php-runtime`.**

**Phase 4: `phpdoc`, `sql`, `diagnostics`, `composer`.** `[sql_reflection]`
parsing moves into `sql`. The SQL state leaves `Registry` for an
`SqlContext` that the project owns and hands to the checker.

**Phase 5: `config` slimmed to global settings.** Every rule/extension
section is kept as a raw `toml::Value`, and its current parser moves next
to its current consumer (it follows the owner again in Phases 11–12). The
top-level `restricted_symbols`, `banned`, `architecture`, `parent_calls`,
`doctrine_functions` and `assertion_tables` modules stop being `config`
dependencies.

**Phase 6: `reflection`.** Still inside the package, turn `Registry`
into `ReflectionProvider` + `ReflectionBuilder` with private storage, and
move the 58 settings off it (to a `Settings`/`AnalysisContext` the
checker holds, until Phases 11–12 hand each one to its owner). Then
extract it as the `reflection` crate with `php_stubs` → `reflection::stubs`.

**Phase 7: `analyser` extraction.** `checker.rs` → `lib.rs`,
`crate::checker::X` → `crate::X`, together with `rule`, `reflection_rule` and
`pest_config`. The rest of the package (`project`, `lsp`, the bin) uses
its existing public entry points. Fix the includes-test paths.

**Phase 8: `project`, `lsp` extraction** (`deps` joins `project`).
The package is now just the bin.

**Phase 9: tidy the extraction.** CI builds `-p phpstan-rs`; README,
ROADMAP, the fuzz guide, and every doc comment naming a `src/`/`crate::`
path get updated; the memory test command becomes
`cargo test -p <crate> --lib -- <filter>`; re-measure and record the
Phase 0 timings.

**Phase 10: engine APIs (no moves).** `Scope`, `Rule` with the derived
nodes, `Collector`, `Observer`, the extension traits, per-extension scope
slots, the dispatch tables, and `AnalysisContext`. Port the existing
`Rule`/`ReflectionRule` users.

**Phase 11: extension crates, one commit each:** `ext-php`, `ext-sql`
(takes over the `SqlContext`), `ext-doctrine` (scope slot replaces
`query_builder_aliases`; takes `doctrine_functions`), `ext-pest` (takes
`pest_config` and the pest `Checker` fields), `ext-phpunit`,
`ext-mockery` (takes the Mockery assertion entries), then
`ext-strict-rules` and `ext-deprecation-rules`. Each takes its config
section, settings, and diagnostics with it.

**Phase 12: collectors.** The whole-program passes, duplicate checks,
circular namespaces, architecture, banned, class_shape and composer hygiene
become collector-based rules in `rules`. The property-assignment pre-pass becomes the
empty-rule-set collection pass. `facts_only` and every `*_facts` field
disappear from `Checker`.

**Phase 13: every diagnostic to `rules`.** Sub-phases, one commit each,
grouped by the node they hook:
- (a) file-level (incl. `keywords`, `whitespace`, and
  `uninitialized_dataflow` as a file rule with its own walk);
- (b) expression-level;
- (c) calls and member access;
- (d) assignments and properties;
- (e) declarations and PHPDoc;
- (f) control flow, returns and throws, through the function-exit node;
- (g) usage restrictions (`restricted_symbols` with its config,
  internal usage, `phpstan_api`, unused).

Gate: no `report` site in `crates/analyser/src` outside `Scope`.

**Phase 14: observers.** Semantic tokens, inlay hints, probes and
member-reference collection move to `lsp`.

**Phase 15: `Checker` decomposition.** Split the remaining state into
`AnalysisContext` / file state / function-like state stack. Gate: every
field has an inference-side reader.

**Phase 16: split the test suite.** The 92 black-box `checker/tests`
files (they only use the `diagnostics_for*` helpers) move to
`crates/analysis-tests`: one integration binary whose helpers assemble
the engine with every built-in rule, collector and extension. The 26
white-box files go to whichever crate owns what they reach into. This
comes last so each file moves once. Deny `unreachable_pub` workspace-wide.

From Phase 10 on, each commit's scoped wall time must also stay within
noise of Phase 0, to catch dispatch overhead.

## Risks

- **Dispatch overhead.** Hundreds of inline checks become table-dispatched
  callbacks. The prefilter tables and the per-commit wall-time gate
  guard it; derived nodes are only built when a registered callback
  wants them.
- **Rule ordering.** Diagnostics come out in a different order;
  snapshots are compared sorted, and output formats already sort by
  path.
- **Test-path fallout.** Unit-test cwd becomes the crate dir; known sites
  are listed above, and the test-count gate catches silently skipped
  tests.
- **Build times.** An analyser edit still rebuilds everything above it.
  The payoff comes from the ~30k lines of rules and extensions and
  ~65k lines of tests leaving the engine crate, and from lower crates
  no longer rebuilding the world.
