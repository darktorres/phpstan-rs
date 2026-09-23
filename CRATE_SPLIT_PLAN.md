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
| Ad-hoc scope model | besides `types`/`native`/`provenance`, `Locals` has nine special-purpose maps: `guarded_functions`, `guarded_constants`, `guarded_methods`, `guarded_properties`, `condition_aliases`, `branch_implied_facts`, `container_field_facts`, `discriminant_array_facts` and `query_builder_aliases`. Each has its own merge rule: union for some, intersection for others, `Rc::ptr_eq` for aliases. PHPStan covers every one with two mechanisms: expression-keyed type holders with certainty (a `function_exists('foo')` guard is just that call expression known to be `true`) and conditional-expression holders |
| Eager, copied stubs | `generated_stub_data()` parses all ~2,351 embedded stub/phpstan-src files on first use, and every `Registry` then clones the full function/class/constant sets into itself (`registry.merge_stubs(generated_stubs())`), including every test's `diagnostics_for` |
| Levels are a post-filter | every diagnostic carries `level: u8` set where it's emitted; `main/check_project.rs::apply_level_filter` drops diagnostics above `--level` after the run, so rules above the level still run. In PHPStan a level is a set of rules and parameters |
| Fat binary | `main/check_project.rs` holds 1,285 lines of analysis orchestration (file collection, composer roots, parse fan-out, level filter, baseline generation) that the LSP partly duplicates; `main/incremental_bench.rs` is a benchmark living in the CLI |
| Tests are compiled Rust | 3,250 checker test functions, ~65k lines of Rust that each wrap a PHP string and mostly assert `any()`/`contains` over the diagnostics. PHPStan's suite is PHP fixture files: `assertType()` data files for inference, and per-rule fixtures with the exact expected error list |
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
    stubs         build.rs: embedded phpstorm-stubs / phpstan-src sources
                  + a generated symbol → source-file index
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
                  resolution. Layered source locators (process-wide
                  lazily-parsed stubs, then the project). Private storage  (syntax, phpdoc, types, php-runtime, stubs)
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
L6  project       the analysis Session API shared by CLI and LSP (config →
                  file discovery, parse fan-out, scheduling, the collection pass,
                  baseline generation), the
                  incremental dependency graph (deps), LSP indexes
                  (use_sites, decl_locations); wires rules + extensions
                  from config
L7  lsp           protocol + features; semantic tokens, inlay hints,
                  probes, member references as analyser Observers
L8  phpstan-rs    the binary: clap + output only (package name kept)
    analysis-tests  test-only crate: data-driven fixture suite (PHP files, not Rust)
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

**Reflection sources.** `stubs`' build.rs also generates a sorted
symbol → source-file index (functions, classes, constants, with their
lowercase keys). `ReflectionProvider` is layered:
- a process-wide stub layer that parses a stub file the first time one
  of its symbols is requested, applies the stub patches to it, and
  caches the result behind `Arc` (thread-safe, shared by every project,
  LSP session and test);
- the per-project layer on top.

Nothing is cloned per registry. Whole-program iteration (`classes()`)
covers the project layer, plus the stub layer only where a collector
explicitly asks for it.

**Scope model.** `Locals` becomes PHPStan's model:
- expression-keyed type holders, each with native type and certainty,
  covering variables, property/offset paths and guard calls
  (`function_exists('foo')`, `defined('X')`, `method_exists($o, 'm')`,
  `property_exists(...)` held as known-`true` call expressions);
- conditional-expression holders ("if E has type T then F has type U")
  that replace `condition_aliases`, `branch_implied_facts`,
  `container_field_facts` and `discriminant_array_facts`.

There is one merge rule and one invalidation rule (rebinding a variable
drops every holder whose expression or condition mentions it) instead of
per-map rules. `query_builder_aliases` is replaced by extension type
data (below). The `Scope` API exposes this model directly.

**Levels.** A level is a rule set plus a parameter set, as in PHPStan's
`level0.neon`…`level10.neon`. Each rule declares the lowest level it's
registered at; level-dependent behavior (e.g. reporting "maybe"
certainties, nullable checks) becomes a named parameter the level sets and
the config can override. `Diagnostic.level` and `apply_level_filter` go
away, and rules above the level never run.

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

Each extension may also ship rules and collectors.

**Extension type data.** Extension state rides on the type, as PHPStan's
`QueryBuilderType` does, not on a variable. `types` gains an opaque
payload on object types: `Arc<dyn ExtensionTypeData>` with
equality/hash, display, subtyping and a merge function the extension
supplies. `ext-doctrine` stores the QueryBuilder chain there, so it
survives method returns, properties, arguments and array elements, which
`Locals.query_builder_aliases` (keyed by variable name) loses today. The
engine needs no per-extension scope slots.

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
denied workspace-wide once Phase 18 lands.

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
  is handled by Phase 13, so it's temporarily allowed.

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
to its current consumer (it follows the owner again in Phases 12–14). The
top-level `restricted_symbols`, `banned`, `architecture`, `parent_calls`,
`doctrine_functions` and `assertion_tables` modules stop being `config`
dependencies.

**Phase 6: `reflection`.** Still inside the package, turn `Registry`
into `ReflectionProvider` + `ReflectionBuilder` with private storage, and
move the 58 settings off it (to a `Settings`/`AnalysisContext` the
checker holds, until Phases 12–15 hand each one to its owner). Replace
eager stub parsing and per-registry cloning with the build-time symbol
index and the lazily-populated shared stub layer. Then extract it as the
`reflection` crate, with `php_stubs` → `reflection::stubs`.

**Phase 7: `analyser` extraction.** `checker.rs` → `lib.rs`,
`crate::checker::X` → `crate::X`, together with `rule`, `reflection_rule` and
`pest_config`. The rest of the package uses its existing public entry
points. Fix the includes-test paths.

**Phase 8: `project`, `lsp` extraction, thin binary.** `deps` joins
`project`. `main/check_project.rs`'s orchestration becomes
`project::Session`, which the CLI and the LSP both drive (the LSP
drops its duplicate discovery and config plumbing). The bin keeps clap,
output selection and exit codes. `incremental_bench` becomes a
`project` criterion bench, and `demo` goes wherever its logic belongs.

**Phase 9: tidy the extraction.** CI builds `-p phpstan-rs`; README,
ROADMAP, the fuzz guide, and every doc comment naming a `src/`/`crate::`
path get updated; the memory test command becomes
`cargo test -p <crate> --lib -- <filter>`; re-measure and record the
Phase 0 timings.

**Phase 10: scope model.** `Locals` → expression holders + conditional
expressions, as above. The eight narrowing maps and their per-map merge
and purge code go away. `query_builder_aliases` becomes Doctrine
extension type data on the builder's object type (still inside the
analyser until Phase 12 moves it out).

**Phase 11: engine APIs (no moves).** `Scope` over the new model;
`Rule` with the derived nodes and declared identifiers and levels;
`Collector`, `Observer`, the extension traits, extension type data, the
dispatch tables, and `AnalysisContext`. Port the existing
`Rule`/`ReflectionRule` users.

**Phase 12: extension crates, one commit each:** `ext-php`, `ext-sql`
(takes over the `SqlContext`), `ext-doctrine` (its builder type data, takes
`doctrine_functions`), `ext-pest` (takes `pest_config` and the pest
`Checker` fields), `ext-phpunit`, `ext-mockery` (takes the Mockery
assertion entries), `ext-strict-rules`, `ext-deprecation-rules`. Each
takes its config section, settings and diagnostics with it.

**Phase 13: collectors.** The whole-program passes, duplicate checks,
circular namespaces, architecture, banned, class_shape and composer
hygiene become collector-based rules in `rules`. The property-assignment
pre-pass becomes the empty-rule-set collection pass. `facts_only` and
every `*_facts` field disappear from `Checker`.

**Phase 14: every diagnostic to `rules`.** Sub-phases, one commit each,
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

**Phase 15: levels as rule and parameter sets.** Each rule's emission-site
`level` becomes either its registration level or a level-set parameter;
`Diagnostic.level` and `apply_level_filter` are deleted. Gate: at every
level 0–6, the scoped Piwigo output equals the Phase 14 output filtered
to that level.

**Phase 16: observers.** Semantic tokens, inlay hints, probes and
member-reference collection move to `lsp`.

**Phase 17: `Checker` decomposition.** Split the remaining state into
`AnalysisContext` / file state / function-like state stack. Gate: every
field has an inference-side reader.

**Phase 18: fixture-based test suite.** `crates/analysis-tests` becomes a
data-driven harness (`libtest-mimic`, so every fixture is still a named,
filterable test) over PHP files:
- **Inference:** `assertType()`/`assertNativeType()` data files.
- **Rules:** a fixture plus the *exact* expected diagnostic list
  (line, identifier, message, tip), scoped to the rules or extension under
  test, as PHPStan's `RuleTestCase` does. Header comments set config,
  PHP version and level.
- **Codemods:** a `.php` file plus a `.fixed.php` file.

The 3,250 black-box checker test functions are converted by a tool that
extracts each test's PHP source and config and records the rule's current
output as the expectation. Tests that asserted less than the full list
(`any()`/`contains`) get their extra diagnostics reviewed, not blindly
recorded: each is either a correct expectation or a bug to fix. The
white-box tests go to the crate owning what they reach into. Gates:
fixture count ≥ converted test count, with a mapping file from old
test name to fixture. ~65k lines of Rust test code drop out of
compilation. Deny `unreachable_pub` workspace-wide.

From Phase 6 on, each commit's scoped wall time must also stay within
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
  The payoff comes from the ~30k lines of rules and extensions leaving
  the engine crate, ~65k lines of Rust tests becoming fixtures that
  don't compile at all, and lower crates no longer rebuilding the world.
- **Scope-model semantics.** The nine maps' merge rules differ on
  purpose (union vs intersection). The unified conditional-expression
  merge has to reproduce each case, so Phase 10 is gated on
  byte-identical Piwigo output, and any intentional semantic change
  goes in its own commit with its own test.
- **Fixture conversion.** Converting weak assertions to exact lists will
  surface diagnostics the old tests silently tolerated; each gets triaged
  like a corpus finding.
