# Splitting phpstan-rs into a Cargo workspace

This plan moves phpstan-rs to its target architecture, and the crate split
falls out of that. The target mirrors PHPStan:

- an inference engine that emits no diagnostics;
- rules and collectors that receive nodes plus a read-only `Scope`, and
  return errors;
- extensions, behind PHPStan's full set of extension points, for built-in
  and library typing;
- a lazily resolved, snapshot-based reflection layer behind an API;
- a `Session` shared by the CLI and the LSP, with a result cache.

**Ordering principle: redesign a layer, then extract it.** Each layer is
brought to its target shape while it is still inside the package, with its
layering enforced by a mechanical module-graph gate. It becomes a crate only
once its API is final. So:

- no crate is ever cut along a temporary boundary;
- nothing is parked in an interim home;
- nothing is widened to `pub` and then narrowed again;
- no test is stranded behind a dev-dependency cycle.

Lower layers (syntax, types, phpdoc, stub-sources, sql, diagnostics,
composer, config, reflection) are extracted as soon as they're finished.
The engine and everything above it is redesigned in the package and
extracted in one final wave.

## 1. Measured state (HEAD `89d468c` + docs)

| Area | Measured |
| --- | --- |
| Size | 216k lines in one crate; 4,869 `#[test]` in `src/`; `src/checker/tests/` holds 3,297 tests in 67,140 lines, and all checker test modules together hold 3,441 |
| Diagnostic emission | 544 `report`/`report_builder` call sites under `src/checker` (433 + 111), ~324 of them in the inference modules (`expr`, `stmt`, `calls`, `oop`, `function_decl`, `assignment`, `operators`, `control_flow`, `comparisons`, `coercion`, `resolve`, `narrowing`). Plus 13 raw `Diagnostic {}` literals (`phpstan_api.rs` ×11, `duplicate_classes.rs`, `duplicate_functions.rs`) and 3 vector splices: external-`Rule` dispatch at `expr.rs:248`/`stmt.rs:393`, and the trait-host dedup at `function_decl.rs:126` |
| Post-emission processing | extract()/eval() truncates *every* diagnostic of the enclosing body (`saw_dynamic_var_construct`). Loop bodies run up to 6 passes, and `while`/`for` conditions get a quiet pre-pass; diagnostics from non-final passes are truncated, but return points, throw points and property facts leak across passes. Trait methods are checked once per host, then deduplicated by (span, message) and filtered by a cross-host constant-condition verdict |
| `Checker` | 67 fields; 74 at `76e738b4` (Appendix C) |
| `Registry` | 90 fields (74 `pub`, 12 `pub(crate)`, 4 private). They include 67 settings (Appendix B), 4 SQL fields, Doctrine entity metadata (`doctrine_entity_table_index`) and provenance sets (`built_in_stub_classes`, `unconditionally_declared_*`, `stub_provided_functions`). There are ~173 `registry.classes` reads in non-test checker code |
| Reflection data | `ClassInfo` has 48 `pub` fields, `FunctionSig` 24, `PropertySig` 22 and `ParamSig` 13. Member facts are stored as parallel sets, templates as five parallel maps, and `FunctionSig.body` holds AST bodies. `ClassInfo` also carries `doctrine_entity` and php-language-extensions access data |
| Reflection pipeline | 15 whole-registry copy-down passes (`flatten_traits`, `resolve_method_tags`, `precompute_member_owners` ×2, `flatten_phpdoc_inheritance`, `resolve_deferred_*`, `precompute_ancestor_names`, `resolve_constant_named_types`, `drop_doc_members_excluded_by_native`, `apply_assertion_table`, `resolve_transitive_purity`, `infer_global_variables`, ...), duplicated across `finish_loading`, `update_file` and `Registry::build`. The LSP reruns all of them on every edit. Two staleness bugs are documented: trait/mixin members stay on hosts; anonymous classes from old file versions are never removed. An unknown ancestor counts as an ancestor (`is_subclass_of` true), after a fresh load as after removing its file |
| Stubs | 2,392 embedded files (558 phpstorm-stubs + 1,793 phpstan-src/src + 41 phpstan-src `stubs/*.stub`), all parsed eagerly in one `LazyLock` and cloned into every `Registry`. Precedence is first-wins (phpstorm before phpstan-src); a constant declared differently in several files collapses to `mixed`. Ten hand-written, order-dependent Rust patch functions follow. The lexer rewrites `#[PhpStormStubsElementAvailable]`/`#[LanguageLevelTypeAware]`/`#[PhpVersionAware]` at lex time, and only the newest PHP version is read |
| Scope | `Locals` holds `types`, `native` (variables only, never narrowed) and `provenance`. It also has eleven maps with six different merge policies (Appendix D) and three flags (`any_variable_may_exist`, `may_have_member_narrowing`, `track`). Its three invalidation paths each purge a different subset, and the guard sets are never invalidated. There is no certainty: a variable defined on one side of a branch is dropped at the merge |
| Inference | Two implementations: `check_expr` (~4.3k lines, `&mut`) and `infer_expr` (~520 lines, `&self`, with RefCell side effects). `expr.rs` is 6,760 lines with ~181 `ExprKind` arms |
| Types | Pseudo-types are stored as magic strings inside `Type::Object` (variance, value-of, class-constant access, self/static, `object`, `resource`, unresolved `Foo::BAR`). Open shapes are marked by a `"..."` sentinel field. Class names are `String` compared case-insensitively ad hoc. A benevolent union is an accessory marker. `Mixed(bool)` has a hand-written `PartialEq` and there is no `Hash`. There are three subtyping implementations: `types::is_subtype` (bool, class-blind), `Registry::is_assignable` (bool) and `Registry::is_super_type_of` (Trinary), plus level-gated acceptance on `Registry` |
| PHPDoc | Two type grammars: `phpdoc_grammar` (a real parser, used only for validation) and `types::parse_type_string` (1,287 lines, string splitting, 140 call sites) |
| AST | No node identity (`Expr {kind, span}`). Type hints are `Option<String>`; names are unresolved `String`s (~190 re-resolutions); `StaticAccess(String, String)` covers three constructs; `Cast(String, ..)`; there are no sub-name spans (`text_split` re-lexes them). `ast_walk` returns allocated child vectors that skip closures, and 18 hand-rolled walkers work around that. The interner is a global never-evicting `Mutex<HashSet>`. The lexer `eprintln!`s warnings |
| Levels | Partly a post-filter: `apply_level_filter` in the CLI with `MAX_LEVEL = 6`, and a separate uncapped cutoff in the LSP. Partly emission-time: `Registry::configured_level` drives `accepts_at_configured_level` with thresholds 7/8/9/10, and some sites compute their level at runtime (`every_member ? 0 : 7`). An unset level mixes rules up to 6 with level-8 acceptance. `Diagnostic.level` is also read by the JSON output and the LSP severity mapping |
| Post-filters | `apply_level_filter`, `restricted_symbols::filter_by_path` (path zones passed via `Diagnostic.metadata`), inline ignores, ignoreErrors and baseline. The CLI and LSP each implement them separately |
| Config | One flat serde struct of 83 top-level keys with no `deny_unknown_fields`. The bin has 72 clap args (~60 per-rule `--check-*`, merged as `cli ‖ config`) threaded through a 60-parameter `run_check_project` into `Project::set_*` setters. The LSP keeps its own `ResolvedWorkspaceConfig`, which silently ignores ~15 keys and resolves paths differently |
| CLI/LSP divergence | The LSP never runs the whole-program passes, mock hygiene, class shapes, banned, architecture or composer hygiene. It skips composer roots and path-repo promotion, doesn't validate config the way the CLI does, and lets the last workspace root's config win for all roots |
| Orchestration | `main/check_project.rs` (1,285 lines) runs the post-processing order, `--fix`, per-file panic isolation with a 64 MiB-stack pool, context-before-analysed ordering, run statistics, SQL drift warnings and tier-2 member-ref seeding. The exit code is always 0 when diagnostics are found |
| Incremental | `deps.rs` is a syntactic, namespace-unaware name scraper keyed by bare strings. There is no persistent result cache |
| Library knowledge in core | Doctrine: `checker/doctrine`, member_resolution branches, `Locals.query_builder_aliases`, `registry/class_info/doctrine_entity.rs`, `doctrine_entity_for_table`, and the class_shape exemption. SQL/DBAL: `checker/sql_reflection` (DBAL `QueryBuilder` folding reuses the Doctrine alias map) and `expr/mysqli`. PHPUnit code in 9 files: `stmt.rs`, `narrowing/assertions.rs`, `risky_tests.rs`, `whole_program/dead_members.rs`, `access_attributes.rs`, `parent_calls.rs`, `project/mock_hygiene.rs`, `imports.rs` (PHAR prefix) and `pest_config.rs`. Codeception in `TEST_BASE_CLASSES` and the parent-call defaults. Mockery and Nette Tester in the assertion-table defaults. Pest in `pest_config` plus Checker fields plus `check_pest_risky_test`. Built-ins in `builtin_calls`, `array_ops`, `preg_shapes`, `date_time`, `throws/dynamic.rs`, the narrowing predicates, the contextual-callback block, and GMP/BcMath operator typing. Third-party rule ports: phpstan-strict-rules, shipmonk, voku, type-perfect, sidz, slam, staabm/sfp psr-3 (plus the Laravel `Log` facade), php-language-extensions, and spaze disallowed-calls (`restricted_symbols`) |
| Cross-file facts | `property_assign_facts` feeds only the `property.unusedType` rule, not inference; the `facts_only` pre-pass exists only for that rule. Two cross-file analyses do feed inference, and both run inside reflection: `registry/purity.rs` (a 1,019-line call-graph fixed point) and `registry/global_vars.rs` (711 lines) |
| Engine I/O | The `phpstan_api` rule reads composer.json from disk mid-analysis; `includes.rs` stats files |
| Tooling | No Rust CI (the workflows only build a release, have never run, and fail because the `vendor/phpstan-src` submodule isn't initialized). The bench does a full `Project` check. The fuzz lib goes through `project::parse_file`. `tests/*.rs` use `CARGO_BIN_EXE_phpstan-rs`. The LSP `serverInfo.version` is `CARGO_PKG_VERSION`. The proptest seeds sit at the root |

## 2. Target architecture

### Crates and layers

A virtual workspace with every crate under `crates/`. Package names are
`phpstan-rs-<name>` (the bin keeps `phpstan-rs`), aliased in
`[workspace.dependencies]` so code reads `syntax::ast::Expr`.

```text
L0  syntax        typed AST with NodeIds, lexer (returns warnings), parser,
                  name resolution, Visitor/VisitorMut, parse-time node
                  annotations, printer, interned Name
    types         the type model, TrinaryLogic, ClassHierarchy oracle trait,
                  the one is_super_type_of / accepts, union algebra,
                  coercions, accessory types, ExtensionTypeData, PhpVersion(s),
                  float formatting
    stub-sources  embedded stub sources + checked-in generated symbol index
    crash-context panic isolation: the hook, `isolated`, InternalError,
                  the Reporter trait                              (backtrace)
    pcre          PCRE2 behind a safe API, knowing nothing of PHP (pcre2-sys)
    crash-sys     the crash reporter's process FFI                (no deps)
L1  phpdoc        the single PHPDoc grammar -> TypeNode AST; DocBlock  (syntax)
    sql           standalone SQL reflection with its own column-type model:
                  config section, DDL schema, live reflector (DB drivers,
                  sqlparser), drift, SqlContext service           (no deps)
    diagnostics   Diagnostic, RuleError, builders, tips, fixes (text + AST),
                  Identifier (+ families)                         (syntax)
    composer      composer.json / installed.json model, package context
                                                                  (syntax)
    neon          NEON decoder (ROADMAP Phase 183)                (no deps)
L2  config        loader and section-owner registry with schema
                  validation; each settings type lives with its owner
                  module                                          (diagnostics)
L3  reflection    ReflectionProvider snapshots, locator chain, stub layer
                  (PHP-version keyed), PHPDoc overlay layer, TypeNodeResolver,
                  class-reflection / deprecation extension traits,
                  ReflectionAttribute, provenance, ConstantResolver,
                  implements ClassHierarchy
                                    (syntax, types, phpdoc, stub-sources,
                                    composer, config)
L4  analyser      engine: holder scope, ExprHandlers, results, TypeSpecifier,
                  NodeCallback sink, Scope API, Rule/Collector/Observer,
                  derived nodes, extension-trait registry, RuleLevelHelper,
                  AnalysisContext, summaries API, reference collector.
                  Emits nothing
                                    (reflection and below; no composer, no I/O)
L5  builtins      = phpstan-src/src/Type/Php: always registered, can't be
                  switched off; incl. regex + PCRE validation (`pcre-validate`,
                  over `pcre`) and the date parser                (analyser)
    rules         phpstan-src/src/Rules equivalents + project-level
                  collector rules                                 (analyser,
                  builtins)
    ext-*         one crate per mirrored package (list below)
                                    (analyser, builtins, rules, sql)
L6  project       Session, SessionBuilder (generic registrations, no ext-*
                  deps), AnalysisResult, result finalizer, VFS, result
                  cache, dependency graph, LSP-facing indexes     (analyser)
    testing       RuleTestCase / TypeInferenceTestCase / LevelsTestCase /
                  Integration / codemod harness + libtest-mimic runner (project)
L7  app           composition root: registers builtins + rules + every ext-*,
                  ErrorFormatter set, crash reporting; `run(args, extra)`
                                    (project, all L5, crash-sys)
    lsp           protocol and features over Session + observers  (project)
L8  phpstan-rs    bin: clap global flags + `--set`, calls app::run, or
                  lsp's entry point with app's registrations      (app, lsp)
```

`crash-context`, `pcre` and `crash-sys` come from the sibling plans
(`CRASH_REPORTING_PLAN.md` §5, `UNSAFE_PLAN.md`): the engine crates and
`project` call `crash-context`, `builtins` alone links `pcre` (optionally, under
`pcre-validate`), and `app` alone links `crash-sys` (under its
`crash-reporting` feature).

**L5 sub-DAG.** `builtins ← rules ← ext-*`. An `ext-*` crate may depend on
`analyser`, `builtins`, `rules`, `reflection` and `sql`, and never on
another `ext-*`. Shared concepts go through registration APIs:

- parent-call rules register into `rules::class_shape`;
- mock types register through a mock-type provider hook;
- test base classes register through a test-framework hook;
- Doctrine entity metadata is published via a class-reflection data
  extension that `ext-sql` reads through `reflection`.

**Extension crates**, each mirroring a real package and carrying its config
section, settings, fixtures and diagnostics (Appendix A lists every module):

| Crate | Mirrors |
| --- | --- |
| `ext-strict-rules` | phpstan-strict-rules, exactly. Includes porting its remaining families: disallowedLooseComparison, disallowedBacktick, disallowedEmpty, disallowedShortTernary, closureUsesThis, noVariableVariables |
| `ext-deprecation-rules` | phpstan-deprecation-rules (built on the restricted-usage framework) |
| `ext-doctrine` | phpstan-doctrine: ORM + DQL, entity metadata, `doctrine_functions` |
| `ext-sql` | phpstan-dba-style SQL/DBAL row typing, parameter validation, mysqli |
| `ext-phpunit` | phpstan-phpunit |
| `ext-codeception` | Codeception test base class and parent-call defaults |
| `ext-mockery` | phpstan-mockery |
| `ext-nette-tester` | Tester\Assert overlays |
| `ext-pest` | pest's PHPStan plugin |
| `ext-mockstan` | rector/mockstan mock hygiene |
| `ext-disallowed-calls` | spaze/phpstan-disallowed-calls (`restricted_symbols`) |
| `ext-shipmonk` | shipmonk/phpstan-rules |
| `ext-type-perfect` | rector/type-perfect |
| `ext-voku` | voku/phpstan-rules |
| `ext-sidz` | sidz/phpstan-rules |
| `ext-slam` | slam/phpstan-extensions |
| `ext-psr3` | staabm/phpstan-psr3 and sfp-phpstan-psr-log, incl. the Laravel `Log` facade |
| `ext-language-extensions` | dave-liddament/php-language-extensions |
| `ext-extras` | phpstan-rs originals with no upstream package (Appendix A) |

**Composition model.**

- `project::SessionBuilder` takes registrations: rules, collectors,
  observers, extensions and config-section owners. It depends on no
  `ext-*` crate.
- `app` is the composition root. It registers every built-in and each
  enabled extension. Extensions are activated by an explicit config list
  plus auto-detection from composer `installed.json` (PHPStan's
  extension-installer).
- A third party builds its own binary against `app` and adds registrations
  in `run(args, |b| ...)`.
- Crates are `publish = false` and consumed via git. Publishing is a
  separate decision, recorded in Appendix F.

### Workspace conventions

- **Workspace manifest:**
  - `resolver = "3"`;
  - `[workspace.package]` with edition 2024, version, `publish = false`
    and license, inherited by every member; the bin passes its version
    into the LSP entry point;
  - `default-members = ["crates/phpstan-rs"]`;
  - `exclude = ["fuzz"]`;
  - root `[profile.*]`.
- **Dependencies.** `[workspace.dependencies]` sets
  `default-features = false` wherever a member must not inherit defaults.
  Appendix E maps every third-party dependency to its crate(s).
- **Lints.** `[workspace.lints]`, with every member setting
  `[lints] workspace = true`, active from the commit that extracts each
  crate:
  - `unreachable_pub` and `rustdoc::broken_intra_doc_links` at `deny`;
  - `unsafe_code` at `forbid` in every crate but the two FFI crates, `pcre`
    and `crash-sys`, whose each `unsafe` block carries a clippy-checked
    `// SAFETY:` comment (`UNSAFE_PLAN.md`); no `allow(unsafe_code)` anywhere;
  - `clippy::print_stdout` / `print_stderr` at `deny` in every library
    crate;
  - `missing_docs` on the API crates (`reflection`, `analyser`, `project`,
    `testing`).
- **Features.**
  - `pcre-validate` lives in `builtins` with no default, and links `pcre`.
    The bin declares `default = ["pcre-validate"]` and forwards it. Tests that
    need real PCRE2 are `#[cfg(feature = "pcre-validate")]`, with complements
    for the disabled path.
  - `fuzzing` lives in `syntax`.
  - `hotpath`/`hotpath-alloc` are the bin's. Other crates take a plain
    `hotpath` dependency, and hotpath's own macros follow the hotpath crate's
    features. A crate whose own code tests the feature declares it too, and
    the bin's forwards to it: `analyser`'s `measure_outermost!` expands a
    `#[cfg(feature = "hotpath")]` in the crate that calls it, which without
    the feature compiles the measurement out.
  - `crash-injection` (`CRASH_REPORTING_PLAN.md`'s test hook, today
    `project/tests/panic_injection.rs` under `#[cfg(test)]`) lives in
    `project`, and `app` and the bin forward it, so the bin's end-to-end tests
    reach the hook in the library. hotpath measure labels change with module paths,
    and the timing re-measurement maps old labels to new ones. hotpath
    counts a function measured at every level of a recursion once per level
    and holds at most 64 measured frames, so a recursive function is
    measured at its outermost call only (`measure_outermost!`), and the
    breakdown inside a recursion comes from a sampling profiler (README).
  - `dhat-heap` is bin-only and mutually exclusive with `hotpath-alloc`
    (`compile_error!`).
- **Release profile.** Decided in Stage A: `lto = "thin"`,
  `codegen-units = 1`, with `dhat` inheriting it. The timing baseline is
  recorded under that same profile.
- **Names.** The embedded-stub crate is `stub-sources` (`reflection::stubs`
  is the module). It owns the `include!` of its generated statics and
  exports them.

## 3. Designs

### 3.1 syntax

- **Node identity:** a dense `NodeId` on every `Expr`/`Stmt`, assigned by
  the parser.
- **Typed AST:**
  - spanned `TypeHint` nodes, including DNF;
  - names resolved by a parse-time name-resolution pass (namespace + `use`),
    keeping both the original and the resolved name;
  - `StaticAccess` split into class-constant, static-property and `::class`
    variants;
  - a `Cast` enum;
  - sub-name spans, so `text_split` is deleted.
- **Traversal:** a complete `Visitor`/`VisitorMut` (syn-style, whose
  default `walk_*` covers closures, arrow functions, anonymous-class
  bodies, parameter defaults and constant initializers) replaces
  `ast_walk`. All 18 hand-rolled walkers are ported to it.
- **Parse-time annotations:** a side table keyed by `NodeId` holding the
  last condition, catch types, goto labels, first-level statement, parent
  statement kinds, declare position and `PHP_VERSION_ID`-dead branches.
  Extensions can register extra visitors.
- **Lexer:** returns `Vec<LexWarning>` instead of printing.
  Stub-attribute rewriting moves to `reflection::stubs` as an AST pass.
- **Names:** an interned `Name` with a precomputed lowercase key, used
  across syntax, types and reflection. The interner is sharded,
  Session-scoped and evicts unreferenced names.
- **Helpers:** `is_primitive_type_name`, `unwrap_grouping`,
  `literal_string_value`, `is_superglobal` and `body_contains_yield` move
  here.

### 3.2 types

- **Dedicated variants:**
  - variance-annotated generic arguments;
  - late-resolvable types (value-of, key-of, offset-access, class-constant
    access, conditional);
  - `Static`/`This` with `self<T>`/`static<T>` arguments, `self` bound to
    the declaring class at a member read; the bound form (`static(Base)`/
    `$this(Base)`, `@return $this` refusing `new static()`) in D3;
  - `ObjectWithoutClass`, `Resource`, `BenevolentUnion`.
- **Shapes and names:**
  - shape keys use a typed key enum, with an explicit unsealed tail
    replacing the `"..."` sentinel;
  - class names are the interned `Name` (declared spelling, equal only
    to the same spelling; `lower()` is its folded key, `eq_ignore_case`
    the PHP comparison).
- **Equality and hashing:** consistent `PartialEq`/`Hash`.
- **Accessory types:** `HasMethod`, `HasProperty`, `HasOffset` and
  `HasOffsetValue`.
- **One subtyping implementation.**
  - `TrinaryLogic` moves here.
  - A `ClassHierarchy` oracle trait (is_subclass_of, ancestors, is_final,
    enum cases, template bounds), which reflection implements. An
    ancestor reached through a name no class answers to is `Maybe`, never
    `Yes` or `No`.
  - One tri-state `is_super_type_of` and one `accepts` (with reasons),
    both generic over the oracle.
  - `is_subtype`, `is_assignable` and the Registry duplicates are deleted.
- **Moves in:**
  - union algebra: all of `type_union.rs`, including `MAX_UNION_MEMBERS`;
  - coercions as methods (`to_integer`/`to_float`/`to_string`,
    `to_absolute_number`, `to_bitwise_not`);
  - `Type::list_of` (was `doctrine_query_result_list_type_of`);
  - float formatting.
- **PHP versions:** `PhpVersion` plus a `PhpVersions` range, narrowable by
  `PHP_VERSION_ID` in the Scope. The default comes from composer
  `config.platform`/`require.php`; with neither and no configured
  version, it is PHPStan's whole supported range (`50207`–`80699`).
  There is no unknown-version state: `Registry::php_versions` is an
  `Option` today, and with it `None` (no `composer.json`, no
  `--php-version`) every one of the 62 version-gated call sites is silent,
  so `function g(int $a = 1, int $b)` isn't
  `parameter.requiredAfterOptional`.
- **`ExtensionTypeData`:**
  `Arc<dyn ExtensionTypeData + Send + Sync>`, with hooks for eq, hash,
  display, and payload-vs-payload subtyping taking `&dyn ClassHierarchy`.
  There is no mandatory merge: a join unions distinct payload-carrying
  types, and an optional extension widening applies only past the union
  cap.

### 3.3 phpdoc

One grammar produces a `TypeNode` AST, which `DocBlock` stores. The
docblock string tokenizer and `types::parse_type_string` are deleted.
TypeNode → Type resolution lives in reflection. Doctrine-annotation
argument parsing stays in the grammar.

### 3.4 stub-sources

A checked-in, generated symbol index, produced by an `xtask` that uses
`syntax`. A test in `reflection` regenerates it from the embedded sources
and asserts it is identical. build.rs stays a parser-free file list plus
`include_str!` generator.

Each index entry records:

- kind;
- case rules: classes and functions fully case-folded; constants exact,
  with only the namespace prefix folded; legacy `define(..., true)`
  handled explicitly;
- the ordered list of declaring files, with corpus priority;
- the precomputed constant collapse;
- extension name;
- parent and interfaces per declaration (for `implementors_of`/
  `subclasses_of`; a class declared per PHP version can differ);
- PHP-version gates.

The index covers `resources/functionMap.php` and its per-version deltas
(C6). Its generator and reflection's declaration walk are one
Visitor-based walk.

### 3.5 reflection

- **Data model.** Opaque `ClassReflection`, `MethodReflection`,
  `PropertyReflection`, `ParameterReflection`, `FunctionReflection` and
  `ConstantReflection`, with accessors:
  - per-member flags and `Vec<TemplateParam>`;
  - `ReflectionAttribute` with resolved arguments;
  - provenance: `file()`, `is_builtin()`, `extension_name()`,
    `declaration_span()`, recorded unconditionally. `decl_locations` is
    derived from these.

  There are no AST bodies in reflection, and build scratch stays in the
  builder.
- **Snapshots.** Reflection is per-file declaration sets plus derived views
  that record each member's provenance.
  `ReflectionBuilder::apply(FileDelta) -> Arc<ReflectionProvider>`
  produces immutable snapshots. Ancestors, members with their owner,
  traits and mixins, and PHPDoc inheritance are resolved lazily per class
  and memoized per snapshot (PhpDocInheritanceResolver-style), invalidated
  along dependent symbols. The memo has five levels - ancestors, own,
  inherited, members, final - each reading the declarations, a lower level,
  and its own level only along an ancestor edge; the ancestor level is
  built from declarations alone, records unresolved edges, and breaks a
  cycle through `extends`/`implements`/`use`.
  The 15 copy-down passes and their three duplicated lists are deleted.
  Anonymous classes are per-file entries, removed with their file. The
  staleness bugs (§1) are closure gates.
- **Locator chain (PHPStan order):**
  1. analysed files;
  2. `scan_paths`;
  3. a lazy composer locator (installed.json classmap/PSR-4/files), wrapped
     in a version-aware skip-polyfill;
  4. the stub layer, filtered by a PHP-version blacklist.

  The embedded phpstan-src layer stays unconditionally available.
- **Stub layer:**
  - process-wide and keyed by PHP version (`version -> Arc<StubLayer>`,
    "unset" being the newest reading);
  - its unit of resolution is a class plus its transitive ancestors, traits
    and mixins, resolved through C4's memo levels, one per-class `OnceLock`
    each (no shared Mutex; a panic leaves the unit retryable);
  - a project declaration that merges with a stub symbol becomes a
    per-project materialized entry (copy-on-merge), and so does a stub
    memo whose read set names a class or constant the project declares:
    only a memo reading stub names alone is shared across projects;
  - the four seeded tracking sets become provenance queries against the
    index;
  - polyfill arbitration first keeps the current quality tiebreak
    (byte-identical); version-based skip-polyfill follows as its own
    commit.
- **Signature and overlay pipeline:**
  - the native signature base (phpstorm-stubs or `phpstan/php-8-stubs`,
    decided in C6), then the phpstan-src `functionMap.php` plus per-version
    deltas, merged as `Php8SignatureMapProvider::mergeSignatures` does;
  - `functionMetadata.php` for builtin purity;
  - a PHPDoc overlay layer on top holds phpstan-src `stubs/*.stub` (full
    PHPDoc), user `stub_paths` (PHPStan `stubFiles` semantics: a PHPDoc
    overlay, or a declaration when none exists, as its own behaviour-change
    commit) and `StubFilesExtension` stubs;
  - every hand-written Rust patch becomes a declarative overlay entry,
    ordered and with cross-symbol dependencies recorded;
  - `[[assertions]]`/`[[class_argument_returns]]` compile to overlay entries
    at config load, read through the one `@phpstan-assert` path;
  - the Nette Tester and Mockery defaults ship as overlays from their `ext-*`
    crates;
  - `apply_assertion_table` is deleted.
- **Enumeration APIs:**
  - `names(kind, layer)` from the index, without parsing (completion);
  - `subclasses_of`/`implementors_of` from the index hierarchy plus the
    project;
  - case-folded key maps (functions: `registry::FunctionTable`; classes
    and methods: C4);
  - every existing iteration site states its layer scope, including the
    ported `ReflectionRule` API.
- **Extension points owned here:**
  - Properties / Methods / AllowedSubTypes class-reflection extensions,
    consulted by member lookup;
  - class-reflection *data* extensions (typed per-class data computed at
    build);
  - Deprecation extensions plus a `DeprecationProvider`;
  - `TypeNodeResolverExtension`;
  - `StubFilesExtension`.

  Built-in implementors: `@property`/`@method`/`@mixin`/require-extends/
  universal object crates.
- **No settings, only reflection inputs.** Level-dependent acceptance
  leaves for `RuleLevelHelper`, and the `phpstan_api` file recording
  becomes unconditional. What reflection reads is `ReflectionSettings`
  alone (§3.6); nothing else from the configuration. Gate: no other
  settings type named under `crates/reflection`.
- **Keys.** Class and function entries are keyed by the folded name
  (`Name::lower`) and hold the declared `Name`;
  method names fold case; property and constant names stay exact, and a
  global constant folds only its namespace prefix. There is one lookup per
  kind and no side map.
- **`ConstantResolver`** (declared, predefined and version-aware over the
  whole `PhpVersions` range, plus `dynamicConstantNames`) lives here;
  reflection's initializer and PHPDoc-type resolution and the analyser read
  constants through it.

### 3.6 config

- Global keys plus per-owner tables (`[rules.*]`, `[ext.<name>]`, the
  existing tables re-homed).
- Each owner registers a typed serde section with `deny_unknown_fields`,
  on the section and on every struct nested in it (arrays of tables
  included). The loader rejects unowned sections and keys, and reports
  every error - an unknown key, a type mismatch, an invalid regex or type
  string, an entry naming a symbol that doesn't exist - by its full key
  path (a `parametersSchema` equivalent).
- Every path in the file (`paths`, `scan_paths`, `stub_paths`,
  `ignore_errors.path`, the baseline, the SQL schema) resolves against the
  config file's directory; paths given on the command line resolve against
  the working directory. Both the CLI and the LSP resolve them through the
  same function.
- No compatibility shim: nothing is released, so the old flat `check_*` keys go
  outright (an unknown key like any other), `app.rs`'s `MOVED_KEYS` table with
  them, and the Piwigo toml, README and VSCode docs migrate in the same commit.
- The ~60 per-rule CLI flags and all `Project::set_*` setters are deleted,
  replaced by a generic two-way `--set key=value`.
- Settings split three ways, following Appendix B:
  - `SessionSettings`;
  - `ReflectionSettings` (scan/stub paths, composer toggle, universal
    object crates, type aliases, PHP version range, dynamic constant
    names);
  - `EngineSettings` (treatPhpDocTypesAsCertain, polluteScope*,
    early-terminating calls, exception model,
    rememberPossiblyImpureFunctionValues).

### 3.7 diagnostics

- `Diagnostic`, built by `DiagnosticBuilder`, is the reported item: what
  the output formats, the baseline, ignores and the LSP consume, whatever
  produced it (a rule, a parse error, an unmatched ignore, a project-level
  check, a stale baseline entry). It carries:
  - message;
  - line/column/end;
  - file and trait file;
  - a static `Identifier` from declared families (prefix × suffix sets,
    syntax-validated, debug-asserted against the rule's declaration);
  - a list of tips, with PHPStan's standard tip helpers;
  - text fixes and AST-node fixes (rendered to diffs via
    `syntax::printer`);
  - `non_ignorable`.
- `RuleError`, built by `RuleErrorBuilder`, is what a rule returns: the
  same fields minus file and trait file, which a rule doesn't know. The
  dispatcher turns each into a `Diagnostic` (PHPStan's `RuleError` →
  `Error`).
- `location` and `metadata` are removed. The baseline is keyed like PHPStan's
  (path relative to the baseline file, identifier, message, count). Nothing is
  released, so it changes in place, with no version field or converter.
- No `clap` in any library crate.

### 3.8 analyser

- **Scope storage.**
  - Expression-keyed holders (type, native type, TrinaryLogic certainty)
    over an interned structured `ExprKey` (variable, property path, offset
    path, static property, call), in persistent maps with O(1) clone.
  - Virtual keys: property-initialized, parameter original value, by-ref
    intertwining, always-remembered, possibly-impure call, clone
    reinitialization, original foreach key and value, foreach value by
    reference.
  - Conditional holders, frozen at creation.
  - Non-holder scope state: currently-assigned, allowed-undefined,
    function-call stack, closure-bind scope classes, first-level statement,
    native-types-promoted, `afterExtractCall`.
  - Guards follow PHPStan:
    - `function_exists` is a FuncCall holder with a specified name
      normalization;
    - `class_exists` is an always-remembered call;
    - `defined('X')` is a ConstFetch holder, and a compared constant
      narrows like a variable: after `DIRECTORY_SEPARATOR === '/'` fails,
      `DIRECTORY_SEPARATOR` is `'\\'`, so `|| DIRECTORY_SEPARATOR ===
      '\\'` is `identical.alwaysTrue` (the rules oracle's `boolean-or.php`,
      `boolean-logical-or.php`, 4);
    - `method_exists`/`property_exists` narrow the receiver with
      `HasMethod`/`HasProperty`, falling back to a call holder for magic
      members.
  - Maps move as follows:
    - `container_field_facts` folds into element-type refinement;
    - `condition_aliases` becomes the conditional holders an assignment
      creates (`AssignHandler`); `branch_implied_facts` and
      `pending_or_disjunctions` become `&&`/`||` holder recipes and
      augments; `discriminant_array_facts` becomes merge-created
      conditional holders;
    - `active_discriminant`/`in_range_index_writes` become conditional
      creation / loop context;
    - `provenance` becomes holder metadata, maintained only when an
      observer asks;
    - `may_have_member_narrowing` is deleted;
    - `track` becomes an engine mode flag.
- **Transitions** mirror MutatingScope: merge (PHPStan `mergeWith`:
  intersect conditionals, `createConditionalExpressions`, Maybe certainty
  for one-sided variables, AND `afterExtractCall`), loop
  generalization/fixpoint, backward-goto fixpoint and forward-goto merge,
  finally, closure capture and closure binding, the constructor's
  remembered scope, and always-iterable foreach. Convergence equality
  compares the context, holders with certainty, natives and conditional
  holders (`MutatingScope::equals`).
- **Invalidation** follows PHPStan `invalidateExpression`: a write to any
  expression (variable, property/dim fetch, by-ref argument, unset,
  foreach binding) drops every holder and conditional that contains it.
  May-alias dim siblings are an explicit sub-rule. Other triggers: impure
  calls on the receiver, volatile/static expressions (`ob_get_level()`,
  `openssl_error_string()`, superglobals, negative existence checks; the
  stat family after `clearstatcache()`/`unlink()`), existence checks, and
  extract/include/eval via `afterExtractCall`. Eval is treated like
  extract, although 2.2.16's `EvalHandler` only invalidates the volatile
  expressions: eval can define variables.
- **Natives.** Natives are full holders, narrowed in lockstep, with a
  complete `native_type_of` inference path.
- **Results.**
  - Statement and expression inference return `StatementResult` /
    `ExpressionResult` (exit, throw and impure points; truthy/falsey
    scopes); no mutable per-function fields.
  - `ExpressionContext`/`StatementContext` are passed down the recursion,
    replacing the take-idiom flags.
- **One inference path.**
  - Per-kind `ExprHandler` modules behind a dispatch (splitting `expr.rs`),
    each with processing, type resolution and `specifyTypes`, and per-kind
    `StmtHandler` modules (splitting `stmt.rs`). One implementation records
    per-`NodeId` before-scopes (`ExpressionResultStorage`), and
    `infer_expr` is deleted.
  - Inference is side-effect-free apart from pure memo caches; the RefCell
    recording channels become observer events.
  - A maximum analyser source-file size is enforced.
- **NodeCallback sink.** Non-final loop passes and quiet evaluations run
  with a Noop sink. The final pass uses the real sink. This covers rules,
  collectors, observers and derived-node construction, including
  return/throw gathering.
- **Scope API: a deliberate port of PHPStan's `Scope` interface.** Each
  excluded method is justified in Appendix F.
  - `type_of`/`native_type_of` do pure inference against the scope
    snapshot, memoized per (snapshot, ExprKey), and accept synthesized
    expressions.
  - `filter_by_truthy`/`filter_by_falsey`.
  - File and namespace.
  - Name resolution.
  - Trait / host context.
  - `can_access_*` / `can_call_*`.
  - Defined and maybe-defined variables.
  - `can_any_variable_exist`.
  - `is_in_first_level_statement`, the is-last-condition attribute, and
    loop context.
  - Static-variable status.
  - Function call stack.
  - `is_declare_strict_types`.
  - `php_version()`, narrowed by `PHP_VERSION_ID`.
  - Iterable key/value types.
  - `is_in_closure_bind`.
  - The parent scope; `has_constant`; member reflections by type.
  - The anonymous function's reflection and return type.
  - The keep-void type; the stored scope type and native type
    (`getScopeType`/`getScopeNativeType`).
  - `resolve_type_by_name`, `type_from_value`, `has_expression_type`.
  - `is_in_class_exists`/`is_in_function_exists`,
    `is_parameter_value_nullable`, `function_type` (a type hint's type).
  - `is_in_expression_assign`, `is_undefined_expression_allowed`.
  - `invoke_node_callback` (`NodeCallbackInvoker`), which runs the rules on
    a virtual node, and `emit_collected_data`.

  The scope handed to a callback is the pre-node state; post-node data
  comes from derived nodes (a behaviour change for ported external rules).
- **TypeSpecifier** is public: `specify_types_in_condition(scope, expr,
  ctx) -> SpecifiedTypes` (sure / sure-not). It is passed to rules and to
  type-specifying extensions (TypeSpecifierAware). The impossible-check
  family is ported generically on it (ImpossibleCheckTypeHelper), replacing
  the hard-coded predicate table. That table knows only the `is_*`
  functions: of the rules oracle's 176 PHPStan-only
  `function.impossibleType`/`function.alreadyNarrowedType`, `in_array` is
  59 (strict and loose, enum subjects and haystacks included), `method_exists`
  35, `assert` 15 (`assert(false)`, `assert($int instanceof \stdClass)`),
  `is_subclass_of`/`is_a` 13, `is_numeric` 8, `property_exists` 7 (object
  shapes), `is_callable` 7, `class_exists`/`interface_exists`/
  `trait_exists`/`enum_exists` 11 (a name of another kind of structure),
  `array_is_list` 3, `array_key_exists` 2, `str_contains`/`str_ends_with`
  2, and user functions with `@phpstan-assert` 3.
- **Rules.**
  - `Rule` callbacks return `Vec<RuleError>`; there is no `Scope::report`.
  - The dispatcher turns each `RuleError` into a `Diagnostic`, attaching
    file / trait file / line, validates identifiers, applies
    `IgnoreErrorExtension`, and drops non-final-pass output.
  - Rules are `Send + Sync + &self`. Per-file state comes from derived
    nodes or engine-created typed `FileState` slots.
  - Collectors return owned, serde-serializable per-file data, which is
    merged on the main thread. They have registration levels, and collected
    data carries `is_only_files_analysis()`.
  - Observers are created per file by a factory and drained after the file.
  - `InternalRule` is deleted, and the `rule::Scope` guard queries migrate
    to expression-type queries (documented for external users).
- **Derived nodes** map one-to-one to PHPStan's `src/Node` catalogue
  at 2.2.16 (52 node classes plus the `Node/Expr` virtual expressions), each
  with its payload and the scopes it carries. Among them:
  - Boolean And/Or with the right scope;
  - match expression / arms with the remaining subject;
  - catch with thrown types;
  - finally exit points;
  - closure / arrow-fn / property-hook return statements;
  - generator yields;
  - isset / coalesce / empty contexts;
  - `@var`-changed types;
  - by-ref argument context;
  - ClassProperties (with initialization state and usages) /
    ClassMethods / ClassConstants;
  - InClass / InClassMethod / InFunction / InClosure / InTrait entry nodes;
  - literal arrays, switch, noop;
  - execution end;
  - resolved call (acceptor plus per-argument contextual types).

  Parameter defaults and constant initializers are traversed as scoped
  expressions. A node is built only when some callback wants it. A
  report-site → node mapping table covering all 544 + 13 + 3 sites is a
  deliverable; no site may be unmapped.
- **Extension traits (the full PHPStan surface):**
  - dynamic function / method / static-method return type;
  - dynamic function / method / static-method throw type;
  - function / method / static-method type-specifying;
  - operator and unary-operator type-specifying;
  - `ExpressionTypeResolverExtension`;
  - parameter-out type;
  - parameter closure type and closure-this (function / method /
    static-method);
  - `AdditionalConstructorsExtension`;
  - always-used members / read-write properties (dead-code roots);
  - restricted-usage (function / method / property / class name /
    class constant, including callable and string-cast variants), with the
    generic `Restricted*UsageRule` set;
  - `ResultCacheMetaExtension`;
  - `IgnoreErrorExtension`;
  - a project-level file-set hook (per-file extension context, refreshed
    incrementally; ext-pest);
  - `DiagnoseExtension`;
  - plus the reflection-owned kinds in §3.5.
- **Built-in services:**
  - `RuleLevelHelper` (checkNullables, checkThisOnly, checkUnionTypes,
    checkExplicitMixed, checkImplicitMixed, checkBenevolentUnionTypes,
    strictMixedArrays; `accepts`/`find_type_to_check`);
  - the reference-site collector (class / function / member refs, enum and
    member-class escapes, declaration locations), below both `rules` and
    `lsp`;
  - a public "is this usage internal / deprecated from this scope"
    predicate, used by rules and by the LSP semantic-token observer.
- **Summaries API.** Whole-program summaries that feed inference (purity,
  global-variable types, and the ROADMAP items C9 lists) run as a fixed
  point before analysis. They are stored per reflection snapshot
  in `AnalysisContext` and declare invalidation keys. The invariant: per-file
  inference reads reflection plus summaries, and never collector output.
- **No I/O.** The package context comes from `AnalysisContext`, and file
  existence comes from the Session VFS, which records a dependency.

### 3.9 Levels

- PHPStan's 0–10 plus `max` ladder, following `conf/config.level0.neon`…
  `config.level10.neon` and `config.levelmax.neon`.
- Each rule has a registration level. Computed-level sites become named
  parameters, one config key each: the 18 the level files set -
  checkMaybeUndefinedVariables, checkExtraArguments, reportMagicMethods,
  reportMagicProperties (1); checkClassCaseSensitivity, checkThisOnly,
  checkPhpDocMissingReturn (2); checkPhpDocMethodSignatures (3);
  checkAdvancedIsset (4); checkFunctionArgumentTypes,
  checkArgumentsPassedByReference (5); checkMissingVarTagTypehint,
  checkMissingTypehints (6); checkUnionTypes, reportMaybes (7);
  checkNullables (8); checkExplicitMixed (9); checkImplicitMixed (10).
- A rule → level table is cross-checked against the neon files, including
  conditional tags.
- One documented default level for CLI and LSP.
- Rules above the level never run.
- `Diagnostic.level` goes. The JSON `level` field is filled from rule metadata,
  and the LSP severity comes from rule-declared severity.

### 3.10 project, app, lsp, bin

- **`Session::run() -> AnalysisResult`**, containing:
  - file errors, not-file errors and internal errors;
  - collected data and dependencies;
  - stats (checked, skipped context / analysed, panics, context loaded);
  - stale baseline entries and unmatched ignores;
  - startup warnings (SQL drift, lexer warnings).
- **Result finalizer**, a fixed, documented order:
  1. inline ignores (with unmatched reporting, on by default as PHPStan's
     `reportUnmatchedIgnoredErrors`: today
     `Session::report_unmatched_ignored_errors` has no config key and is
     always `false`, so a stale `@phpstan-ignore` or
     `@phpstan-ignore-line` is never reported; the corpus oracle finds 67);
  2. ignoreErrors (with `IgnoreErrorExtension`), reporting unmatched
     entries as PHPStan does: `ignore.unmatched` (non-ignorable, skipped
     after internal errors, a path entry only when its file was analysed)
     under `reportUnmatched`; `ignore.count` for a count exceeded (always,
     the surplus reported again) or not reached (under `reportUnmatched`).
     An unmatched entry with only a message, whose pattern may be worded
     after PHPStan's messages, is a notice rather than an error;
  3. baseline filter / generate;
  4. a total order (path, line, col, identifier, message, tip) for every
     consumer.

  Nothing is deduplicated: a finding reported twice is a rule's bug, fixed
  where it reports (gate 13), and a trait's per-host findings are merged
  by E1's trait-host merge, not by the finalizer.

  Parse errors and lexer warnings are non-ignorable file errors.
  Framework identifiers (`ignore.*`, parse errors) belong to the validation
  set.
- **Session entry points:**
  - `generate_baseline`, `apply_fixes`;
  - `update(FileDelta, &CancellationToken)`, cancellable inside and
    between files, keeping the last good AST on a parse failure;
  - `reconfigure(Config)`, which rebuilds registrations and the file set
    and reuses unchanged reflection and stub layers (no window reload).

  The Session also provides:
  - panic isolation, and the 64 MiB worker pool, injected: one per
    process (`project::worker_pool`);
    - the file set's membership (analysed, context, excluded), with no load
      order to keep: C4's collision policy orders declarations canonically, not
      by arrival, and C5's locator reads context classes on demand;
  - one Session per workspace root;
  - a single engine-construction path (gate: none outside `project`).
- **Result cache.**
  - An on-disk cache holding per-file errors, collected data, dependencies
    and exported-node fingerprints.
  - It is keyed by content hash plus a meta hash (config minus
    non-invalidating keys, PHP version, stub index, extension
    `ResultCacheMeta`).
  - Dependencies come from analyser-resolved references (an observer), and
    `deps.rs` is deleted.
  - Incremental collectors: recollect the affected files, rerun the
    project-level rules, and follow fact-producer → consumer edges keyed by
    host class.
- **app:**
  - an `ErrorFormatter` trait over `AnalysisResult`, with PHPStan's set (raw,
    table, github, gitlab, teamcity, ci-detected, json PHPStan-compatible,
    checkstyle, junit) plus sarif and the full-record `phpstan-rs-json`; today's
    text and friendly fold into raw and table (F3); baselines are phpstan-rs's
    own JSON only, neither PHPStan's written nor read;
  - registrable by third parties.
- **lsp:**
  - uses the same rule and collector set (incrementally), the same
    discovery, config-file-relative paths, validation surfaced as
    diagnostics / showMessage, and the same default level;
    - `ResolvedWorkspaceConfig` is deleted;
  - one Session per workspace root, sharing the pool and the stub layer; an
    edit's text applies at once and its recheck runs on the pool, publishing the
    edited file first; requests run on the pool against the last published
    snapshot, answered in any order (F1);
  - positions in the negotiated encoding through each file's line index over
    `Span`'s byte offsets (F1);
  - every language feature finds what is at a position through one
    node-at-position query and reads the analyser's resolved names and types:
    hover is `Scope::type_of` at the node, definition reads provenance,
    references read the reference-site collector (F5).
- **bin:**
  - global flags plus `--set`;
    - exit code 0 when nothing remains, 1 on remaining errors, 2 when the
      result can't be trusted (an invalid config, no files, an internal error),
      141 on a closed output pipe, 130 on an interrupt, its own commit (F1, F3,
      F4);
  - the parse-only `check` subcommand and `demo` are deleted: their roles
    move to the syntax bench, the fixtures and a project example;
  - `incremental-bench` becomes a `project` example (arbitrary directory)
    plus a criterion bench;
  - the `format_signature` test moves to `reflection`.

### 3.11 Tests

- **The `testing` library crate** provides RuleTestCase (with collectors),
  TypeInferenceTestCase, LevelsTestCase, Integration (every rule at a level
  and config, an exact list) and Codemod kinds.
- **Runner:** libtest-mimic, one `[[test]] name = "fixtures",
  harness = false` target per owning crate, with discovery anchored on
  `CARGO_MANIFEST_DIR`. Tests are named `<crate>::<path>`, and the command
  is `cargo test -p <crate> --test fixtures -- <filter>`.
- **Fixture format.** A single file or a directory: several analysed files,
  context-only files, and optional
  composer.json / installed.json / phpstan-rs.toml / Pest.php / DDL. The
  header uses the phpstan-rs.toml schema plus php-version, level and
  requires-feature.
- **Expectations.**
  - Each entry is (line:col-end, identifier, message, tips, fix presence /
    description, non_ignorable), sorted by (line, col, identifier, message,
    tip).
  - An explicit empty list is required; a fixture with no expectations or
    asserts fails.
  - Parse errors, internal errors and panics fail a fixture.
  - Inline-ignore processing runs.
  - The harness drives `Session`, with a rule-registry override for
    scoping.
- **Inference asserts.** `assertType`, `assertNativeType`,
  `assertSuperType` and `assertVariableCertainty` resolve to
  `PHPStan\Testing\*`, via a `FileAssertRule` port plus
  `dumpType`/`dumpNativeType`/`dumpPhpDocType`/`debugScope` rules, with
  PHPStan identifiers and precise type display.

## 4. Gates (permanent, per commit)

Every gate below, once it exists, stays forever. Feature work (ROADMAP)
keeps landing, but only on the APIs that have landed. While a stage is in
progress, feature work touching that stage's modules waits for it.

`cargo xtask gates` runs them in two tiers:

- **quick** (every commit):
  - gates 1–3 without `cargo hack`/`machete`/`deny` or `cargo bench
    --no-run` (`test --all-targets` compiles the benches), and with the
    `--no-default-features` build checked rather than built;
  - the scoped and all-on corpus runs;
  - the type snapshots;
  - baseline compatibility;
  - the nsrt oracle (gate 12, from Stage D);
  - no repeated findings (gate 13, from E1).
- **full** (`--full`, run at the end of every phase and on any commit
  that touches levels, acceptance or performance-sensitive code):
  - everything in quick;
  - the unscoped run;
  - levels 0–10 for scoped and all-on;
  - `cargo hack`, `machete`, `deny` and `cargo bench --no-run`;
  - performance and build time;
  - the corpus oracle (gate 12, from Stage D);
  - the public API snapshots (gate 14, from G1).

CI (`.github/workflows/rust-ci.yml`) runs the build and test half. The
corpus, type, baseline, performance and build-time gates run locally
(frozen Piwigo checkout, stable machine).

1. **Build and lint:**
   - fmt;
   - clippy `--workspace --all-targets -D warnings`;
   - `cargo doc --workspace --no-deps`;
   - the feature matrix via `cargo hack check --workspace --each-feature
     --exclude-all-features` (`dhat-heap` and `hotpath-alloc` exclude each
     other);
   - `--no-default-features` build and test;
   - per-crate standalone check (`cargo hack check --workspace
     --no-dev-deps`);
   - `cargo machete`;
   - `cargo deny check`;
   - fuzz compile (`--manifest-path fuzz/Cargo.toml`);
   - `cargo bench --no-run`.
2. **Module graph.** Every module is assigned a target crate (Appendix A).
   No module imports a module assigned to a higher crate or a sibling it may
   not depend on, except edges listed in `gates/module-graph.txt`, each with
   the phase that removes it. The list only shrinks: an entry that no
   longer matches an edge fails the gate, and none outlives its phase.
   The same list covers where an `impl` sits, which an edge doesn't show (a
   `rules` module reaching `analyser` is an allowed edge, yet its
   `impl Checker { … }` can't compile once the two are crates): an inherent
   `impl` must be in the module assignment of its type's crate, and a trait
   `impl` in that of its trait's crate or its type's (the orphan rule, a
   generic type's own parameters counting as neither). The inventory lands
   before C4, each entry with the phase that moves its module; on today's
   package, 84 inherent `impl` blocks of seven core types alone sit in
   another crate's modules (52 of `Checker` in modules assigned to `rules`, 7
   in `builtins`, 17 in the `ext-*` modules, one in `lsp`; 5 of `Project`; 2
   of `Registry` in `analyser`'s `global_vars`).
3. **Tests.** The set of test names (`cargo test --workspace --all-targets
   -- --list` plus the fixture lists) must be a superset of the previous
   commit's set mapped through a committed rename/removal manifest (old →
   new, or removed with a reason). Fixtures must match exactly. A name is
   qualified by its package and target (`phpstan-rs-syntax lib
   parser::tests::…`, `phpstan-rs test fixtures checker/…`): libtest's
   `--list` prints bare names, and the gate's set holds them unqualified
   today, so two crates' `tests::parses_x` are one entry and a test deleted
   from one hides behind the other's. The qualified listing lands with its
   first manifest before C4.
4. **Corpus**, using `target/release/phpstan-rs` on the frozen Piwigo
   checkout with commands pinned in `.corpus-runs/README.md`, JSON
   normalized to one line per diagnostic (path, line, col, identifier,
   message, tips), stdout and stderr stored separately:
   - scoped;
   - unscoped;
   - an all-on configuration (every opt-in rule, whole-program checks,
     SQL / Doctrine, max level);
   - levels 0–10 and unset;
   - from Stage A1 on, compared *unsorted* (the total order is part of the
     output).
5. **Type snapshot.** Inferred type and native type for every variable at
   every statement boundary, over Piwigo and the fixture corpus; certainty
   joins it at D5.
6. **Baseline compatibility.** The baselines generated by the Stage A
   binary (scoped and all-on) are applied to every commit. The unmatched
   diagnostics and stale entries they report are snapshotted and held to
   the parent commit's like the corpus, changing only by approved delta;
   the counts are printed for information.
7. **LSP replay** (from Stage F). Open every Piwigo file through the Session in
   LSP mode, then run an edit/revert pass, and compare after every step, not
   only at the end: after each edit, the published diagnostics of every file
   equal a CLI run over the edited tree, and after the revert, the CLI snapshot.
   The edits are scripted: a declaration's signature changed, a class renamed
   and its file moved, a file deleted and restored, a PHPDoc-only reference's
   target changed, a syntax error typed and fixed character by character. The
   replay runs once with one workspace folder and once with the corpus split
   into two folders with different configs, each folder's diagnostics equal to
   its own CLI run.
8. **Performance**, measured against the previous commit, one-sided:
   - wall time: hyperfine, warmup, 10 runs; the mean may not exceed the
     previous mean + max(2%, 3σ); from F2, cold (`--no-result-cache`), warm
     with nothing changed, and warm after one file's change, each held to its
     own previous mean;
   - peak RSS (`/usr/bin/time -v`) and dhat peak;
   - LSP steady-state RSS, incremental re-check latency (the edited file's
     publish and the whole affected set's), and the 95th-percentile latency
     of hover, definition, completion and references over a fixed sample of
     corpus positions.

   Results are stored under `.corpus-runs/perf/<hash>`.
9. **Build time.** Scenarios: touch an engine file, a rule file, a lexer
   file and a stub file, then run `cargo test --workspace --no-run` and
   `cargo build -p phpstan-rs`. Recorded every commit. Targets are
   asserted at the end of Stages B, E and G: for example, a rule edit
   rebuilds neither analyser nor reflection, and a stub edit rebuilds only
   stub-sources and its dependents. `-Z threads` is re-evaluated per crate.
10. **Architecture.** Each gate is added in the phase that lands it and
    stays:
    - analyser constructs no `Diagnostic`/`RuleError`, contains no
      identifier literals and does no diagnostics-vector mutation;
    - reflection reads no settings type but `ReflectionSettings`;
    - no `Locals` map outside the holder model;
    - no engine I/O (including no `current_dir`, `canonicalize` or
      `path::absolute` outside the session, binary, LSP and tests);
    - no engine construction outside `project`;
    - the built-in function names the analyser may mention form an
      allowlist (scope-effect built-ins only: extract, compact,
      get_defined_vars, array_push/unshift, clearstatcache, the openssl
      error functions, func_get_args and similar);
    - no `"$$` type-string sentinels;
    - no `Diagnostic {` literal outside the builder;
    - no print macros in library crates.
11. **Renames.** `git diff -M --summary` shows every moved file as `R`.
    Otherwise the change is split into a pure rename commit (exempt from
    the build gate) and a path-rewrite commit, recorded in
    `.git-blame-ignore-revs`.
12. **Oracles** (from Stage D), both against PHPStan 2.2.16:
    - `cargo xtask nsrt` runs `check-project` over
      `vendor/phpstan-src/tests/PHPStan/Analyser/nsrt` (15 s), with
      `tests/PHPStan` as a scan directory, phpstan-src's composer
      dependencies installed and each file at a PHP version its `// lint`
      header allows, and counts the `assertType`,
      `assertNativeType` and `assertVariableCertainty` mismatches after
      display normalization (Stage D lists the forms), by file and owning
      phase (`gates/nsrt/owners.toml`), wrong values first;
    - `cargo xtask oracle` runs the phar (copied out of the corpus's
      `vendor/phpstan/phpstan`) over the corpus files with
      engine-sensitive diagnostics, with the corpus autoloader as the only
      bootstrap and `phpVersion` pinned to the composer range phpstan-rs
      reads, runs phpstan-rs over the same files at the same level (10),
      and lists the phpstan-rs-only and PHPStan-only findings by
      identifier; files that raise PHPStan internal errors are excluded
      and listed;
    - `cargo xtask rules-oracle` (from Stage E) runs the phar and
      phpstan-rs over PHPStan's own rule-test data
      (`vendor/phpstan-src/tests/PHPStan/Rules/**/data`, 2,936 files, the
      five PHPStan can't parse excluded), level 10, PHP 8.5 given to the
      phar as the range `phpVersion: {min: 80500, max: 80599}` (a single
      `phpVersion` leaves `PHP_VERSION_ID` unbounded in PHPStan, so
      phpstan-rs's `PHP_VERSION_ID >= 80400` is `greaterOrEqual.alwaysTrue`
      where the phar is silent: 6 of the first run's phpstan-rs-only
      findings), and matches findings by file and identifier, on the same
      line first so a near finding can't take another's exact match, then
      within three lines, or within one declaration for the signature-level
      identifiers (`missingType.*`, `return.missing`,
      `constructor.unusedParameter`, `parameter.phpDocType`,
      `phpDoc.parseError`, which the two tools place on different lines of
      a declaration). PHPStan's "(in context of class …)" rows of a trait's
      findings are stripped and collapsed first. It lists, per identifier,
      the PHPStan-only, matched and phpstan-rs-only counts (phpstan-rs
      takes 2 s).

    All three record under `.corpus-runs/gate/{nsrt,oracle,rules}/<hash>`. A
    count rises only through an approved delta that names the cause.
13. **No repeated findings** (from E1). No result of the corpus runs, the
    fixtures or the rules oracle's phpstan-rs side holds two findings with the
    same file, span (start and end), identifier and message. A rule that reports
    once per offending element spans that element, so two real findings never
    share one: PHPStan's `generics.noParent` per `@implements` tag and
    `varTag.noVariable` per static variable span the tag and the variable, where
    phpstan-rs spans the declaration today. The rules oracle's phpstan-rs side
    has 7 findings sharing another's line and column today: 3 are nested or
    chained expressions (`$array[][]`, `->find(1)->find(1)`) whose spans end
    apart, 3 are those two rules, and one is a real repeat, `phpDoc.parseError`
    reported twice for one tag at `ignore-line-within-phpdoc.php:26`.
14. **Public API** (from G1). Each library crate's public API, as
    `cargo public-api` lists it from rustdoc's JSON (the pinned nightly), is
    snapshotted under `gates/public-api/<crate>.txt`, the crates extracted in B
    and C included from G1's first commit. A change is an approved delta,
    reviewed against the crate's design in §3.

**Approved deltas.** A commit that intentionally changes output records its
expected diffs (corpus, fixtures, type snapshot, baseline) in the commit.
Every changed line is reviewed for runtime correctness. The gate is:
actual diff == approved diff.
Behaviour-changing commits are always separate from structural ones.

## 5. Stages and phases

One commit per item. Every commit passes every gate that exists at that
point. `CRATE_SPLIT_EVIDENCE.md` is a frozen copy of these stages from
before a condensing pass, kept to check that nothing was dropped.

### Stage A: oracle and gates (structure unchanged)

**A0. Measurement and CI.**

- Release profile decision (§2).
- Record every baseline in §4: corpus (all configs and levels), type
  snapshot, Stage-A baseline file, performance, build-time scenarios, the
  test-name manifest.
- The Rust CI workflow runs exactly the §4 gates. Every workflow runs
  `git submodule update --init --depth 1 vendor/phpstan-src` and only that.
- The corpus snapshot format and commands are written into
  `.corpus-runs/README.md`.
- The gate runner is `xtask gates`.

**A1. Total output order.** Imposed in the CLI and LSP (an approved-delta
commit); the unsorted corpus comparison starts here.

**A2. Fixture capture.**

- Instrument every `diagnostics_for*`, local test helper and project-test
  entry point to dump each analysis call at runtime: test name, call
  index, final sources, config as a toml / fixture header, full output.
  One test may map to N fixtures. Settings with no toml key get one
  first: `phpstan_api_check_enabled` via a companion composer.json, the SQL
  schema via a DDL file, and "no configured level" by omitting `level`.
- Convert every test that asserts on diagnostics, wherever it lives:
  `checker/tests`, `project/tests`, `phpstan_api/tests`, `rule/tests`,
  `restricted_symbols`, sql / doctrine module tests.
- Route by kind:
  - `is_empty()` and cross-rule negative tests go to Integration;
  - single-identifier-family tests go to rule-scoped fixtures;
  - multi-file and composer tests go to directory fixtures;
  - codemod tests go to the Codemod kind.
- The harness-level family filter is removed; fixtures record those
  diagnostics or disable the rules via config.
- **Dual run.** For one commit, the old Rust helpers read from the fixture
  harness, and every old assertion must pass. Only then are the converted
  Rust tests deleted.
- `examples/stress*.php` become fixtures or are deleted.
- The harness backend is the Project API, in the bin package's
  integration tests.

**A2 behaviour fixes**, one commit each with fixtures (approved delta
`gates/approved/a2-known-bugs`):

- class names resolve case-insensitively, in `catch`, static-call targets
  and types, `self`/`static`/`parent` included: `Self::X`, `SELF::$p`,
  `Self::m()` and `instanceof SELF` are `class.notFound` today (the rules
  oracle's `class-constant-visibility.php`, `instanceof.php`,
  `call-static-methods.php`, `access-static-properties.php`);
- `self` inside an anonymous class is that class: `self::Y`,
  `self::$p`, `self::m()` and `new self()` there are `class.notFound`
  "class@anonymous not found" today (`bug-13042.php`, `bug-7904.php`);
- casts, `echo` and interpolation give precise types, a resource included:
  `(array)` of a scalar is a one-element list and of a list stays one, so
  `(array) true` and `(array) $v` on `string|list<string>` are `array`
  today where they are `array{true}` and `list<string>`, and passing them
  to a `list` parameter is `argument.type` (the rules oracle's
  `array-cast-list-types.php`, `bug-8280.php`); `(object)` of an array is
  `stdClass`, `object` today (`bug-12755.php`); `(array)` of an object is
  its properties' values: `(array) $this` in a final class with a `string`
  and an `int` property is `mixed` today, so `is_object($v)` after
  `is_string($v)` fails isn't `function.impossibleType` (`bug-2730.php`);
- the `array_udiff()` family's trailing callbacks are accepted;
- `list<int>&non-empty-array` is one intersected type;
- a method called on an intersection uses its most visible declaration,
  and its parameters don't depend on the members' order: `render($foo)`
  on `ViewB&ViewA` (`render(string $foo = '')` and `render()`) is
  `arguments.count` today while `ViewA&ViewB` passes (the rules oracle's
  `bug-11978.php`);
- a function or method whose body calls `func_get_args()` (or
  `func_get_arg()`/`func_num_args()`) is variadic, as PHPStan's
  `isVariadic()` reads it: `some_variadic_function('a', 'b')` for
  `function some_variadic_function() { $v = func_get_args(); }` is
  `arguments.count` today (the rules oracle's `bug-11559.php`,
  `bug-11559c.php`, `bug-3448.php`, `call-variadic-methods.php`,
  `function-with-variadic-parameters.php`, `instantiation.php:35`);
- `new static()` isn't checked against the constructor (a subclass may
  change it): `new static()` is `arguments.count` today where the
  constructor takes one (`instantiation.php:17`);
- `@param-out` may be wider than the native by-reference type, and holds
  after the call;
- an invalid inline `@var` is not applied;
- a native-typed property keeps its type after a write it rejects, and a
  hooked property reads back as its declared type;
- an override without a return type has the builtin's tentative one: doc
  templates are marked before reconciling with the native type, and
  `parent::`/`self::`/`static::` bind the calling class's templates;
- each declaration of a duplicate method is checked against its own
  signature, and every class-body duplicate is reported;
- `function_exists()` and `defined()` guards hold inside a closure;
- `class_exists(Bar::class)` guards as `class_exists('N\Bar')` does (only
  the string form guards today): under it, `Bar::set()`, `new Bar()`,
  `instanceof Bar` and `@var Bar` are `class.notFound`, in an `if`, after
  `if (!class_exists(…)) { return; }`, in a ternary's true branch and in a
  closure inside the guard (the rules oracle's `*-class-exists.php`
  files, `class-exists-on-static-call.php`, `bug-9055.php`);
- a child property inherits the hooks it does not redeclare;
  `parent::$prop::get()`/`set()` resolve; a set hook without a parameter
  list takes the property's type;
- `__set()` needs no return type, and neither does any magic method PHP
  fixes (`PhpMethodFromParserNodeReflection`): `__destruct`, `__unset`,
  `__wakeup`, `__clone`, `__unserialize` and constructors are `void`,
  `__toString` `string`, `__isset` `bool`, `__sleep` `array<int,
  string>`, `__serialize` `array`, `__set_state` an object and
  `__debugInfo` `array|null`; `__unserialize()` without a return type and
  `__sleep(): array` are `missingType.return`/`missingType.iterableValue`
  today (the rules oracle's `magic-signatures.php`,
  `magic-serialization.php`, `missing-serialization.php`,
  `enum-sanity.php`, `bug-15056.php`), and their returns are checked
  against those types: `__isset()` returning `42`, `__sleep()` returning
  `[new stdClass()]`, `__serialize()` or `__debugInfo()` returning `''`
  (a `TypeError` and a fatal error) and `__set_state()` returning an array,
  and `return 1;` in an implicitly `void` one (`__construct`, `__clone`,
  `__set`, `__unset`, `__wakeup`, `__destruct`, `__unserialize`;
  `return.void`, 12) are unreported
  today (8 of the rules oracle's PHPStan-only `return.type`);
- a failing `array_is_list()` proves a non-empty non-list array;
- a boolean flag set in a branch implies that branch's narrowing, unioned
  over every path that sets it;
- a catch is entered from the points in its try that can throw what it
  catches, and is dead when there are none; an unhandled `match` throws
  `UnhandledMatchError`;
- a dynamic method call is impure.

Follow-ups:

- **Full tier.** `xtask gates --full` runs at the end of every phase. The
  catch-up run (`25d63876` against `1e5e42be`) is reviewed: levels 0–8 of
  `scoped` and `all-on` change exactly as the quick tier's approved deltas
  do; levels 9–10 and `unscoped` add the call-parameters wording
  (`976956bc`), sub-name spans, extension activation by installed package
  (`unscoped` runs without composer, so Doctrine result typing is off), the
  A2 fixes and better inference. Its false positives are fixed, each with
  its fixture: `printf`'s doubled value report (`ac70c6eb`), integer range
  unions (`bda6df20`), a class's own templates inside its body
  (`40e45bdc`), class constants in ancestor tags (`520cd439`), `foreach` as
  a throw point (`9193f6a9`), `__invoke` and `callable` (`48dc04b3`),
  narrowing a union to a target (`06ca4c48`), dynamic-name calls as throw
  points (`2348279e`) and type aliases at any depth (`672e927a`); the type
  deltas are approved in `aa08c24f`, `40e45bdc` and `0d105e30`.
  `method_exists()` on a property not narrowing it (Symfony's `Router`) is
  D's `HasMethod` narrowing.
- **CI green.** The one recorded Rust CI run (`867d9cb3`) failed at the
  no-default-features tests, so the docs, machete and deny steps never ran,
  and 162 commits since are unpushed. Every step passes locally; a passing
  run is recorded.

### Stage B: workspace and leaf layers

Stage B's follow-ups land before C5: C5–C8 build on the AST and type API
they change. Each behaviour fix is its own approved-delta commit with a
fixture.

**B1. Workspace skeleton.**

- Virtual root, conventions (§2), and the package moved to
  `crates/phpstan-rs`.
- Fuzz repointed to `../crates/phpstan-rs`.
- The bench's vendor paths anchored on `CARGO_MANIFEST_DIR`.
- proptest regressions moved with the package, verified to replay.
- CI path filters: `crates/**`, `**/Cargo.toml`, `Cargo.lock`,
  `rust-toolchain.toml`, `.cargo/**`, `editors/vscode/**` and the workflow
  files.
- README, the VSCode README and the profile comments switch to
  `-p phpstan-rs` now.

**B2. Module graph.**

- A mechanical edge inventory against Appendix A, including
  `text_split → deps`, `registry → {restricted_symbols, doctrine_functions,
  assertion_tables, sql_*}`, `config → rule configs`, `checker ↔ rule`,
  `checker → project` and engine → rule-module references.
- Move the helpers down (§3.1, §3.2) and `Diagnostic*` into its module.
- Move `whole_program/*` and `duplicate_{classes,functions}` under the
  `project` module (they use no `Checker`), which removes both
  `checker ↔ project` edges.
- Every remaining edge that points up is listed in the gate's allow-list
  with the phase that removes it.
- The strict gate is on from here.

**B3. syntax redesign**, one commit each:

- NodeId;
- the typed AST (hints, resolved names, split StaticAccess, Cast enum,
  sub-name spans; `text_split` deleted);
- Visitor / VisitorMut plus porting the 18 walkers;
- parse-time annotations;
- lexer warnings;
- stub-attribute rewriting moved to an AST pass (it lands in reflection at
  C);
- the interned `Name` and sharded interner.

Then extract `syntax`: the `lex_parse`/`token_stream_parse` fuzz targets
repoint to it, and a new lex / parse microbench is added.

Follow-ups, behaviour fixes:

- **Interpolation.** `"${name}"` and `"${expr}"` interpolate (read as
  literal text today; nsrt `literal-string.php` types
  `"foo ${literalString}"` as the literal `'foo ${literalString}'`).
- **Integer literals.** A legacy octal `0775` is 509 (775 today); a hex,
  octal or binary literal past `i64` is a float (`0xFFFFFFFFFFFFFFFF` is
  `0` today).
- **Byte strings.** `Literal::String` and `Type::StringLiteral` hold bytes:
  `"\xff"`, octal escapes and non-UTF-8 source bytes keep their bytes
  (`"\xff" === "\u{ff}"` is reported always true today). A heredoc keeps
  `\"` as written.
- **Keywords fold case** everywhere: `<?PHP` (the file is inline HTML
  today), `DECLARE(strict_types=1)` (three false positives), `VAR`
  (`class.notFound` for `VAR`), `INSTEADOF`, `PRIVATE(SET)` (a parse error).
- **Shebang.** A leading `#!` line is not a statement
  (`declareStrictTypes.notFirst` after one today).
- **PHP 8 precedence.** `+`/`-` bind tighter than `.`, and `.` looser than
  `<<`/`>>` (`"sum: " . $a + $b` reports `binaryOp.invalid` today). An
  arrow function's body, `throw`'s operand and `include`/`require`'s
  operand bind looser than `and`/`or`/`xor`, as PHP's grammar orders
  `T_THROW`, `PREC_ARROW_FUNCTION` and `T_INCLUDE` below `T_LOGICAL_OR`;
  the parser reads them with `parse_assignment`, which stops before those
  operators. `static fn (?bool $m, Constraint $c): bool => $m xor
  $c->evaluate($other)` ends the body at `xor`, so `$c` is reported
  `variable.undefined` and the `xor` `logicalXor.resultUnused`
  (`phpunit/src/Framework/Constraint/Operator/LogicalXor.php:64`), and
  `include 'a.php' or die()` is read as including `a.php` where PHP
  includes `'1'`.
- **`new $cls()` with a constant class name** is that class: after `$cls =
  Foo::class`, and after a `@return class-string<KeywordList>` call, `!
  $keywords instanceof KeywordList` is `instanceof.alwaysTrue`,
  unreported today (`bug-4689.php`).
- **`new` takes PHP's `new_variable`**: `new $b[0]()`, `new
  $c[$key]()`, `new $o->cls()` and `new $o::$cls()` instantiate the class
  the whole expression names. Today `new $b[0]()` parses as `(new
  $b)[0]()`, so it is `new.nonObject` "cannot instantiate class using
  array{0: 'K'}" plus `offsetAccess.nonOffsetAccessible` (the rules
  oracle's `instantiation.php:172`, `:179`), `new $o->cls()` as `(new
  $o)->cls()` ("Cannot call method cls() on mixed") and `new $o::$s()`
  reports `$s` undefined.
- **`(void)` casts a whole expression statement**: `(void) 5 |>
  f(...);` is `(void) (5 |> f(...))`; today it parses as `((void) 5) |>
  f(...)` and is `cast.void` "cannot be used within an expression" (12
  rules-oracle findings in the `*-statement-result-discarded-php85.php`
  files).
- **`yield` takes an assignment**: `yield $x = 1;` is `yield ($x = 1)`;
  today it parses as `(yield $x) = 1`, `assign.invalidExpr` plus
  `$x` undefined there and on the next `yield $x + 1` (the rules oracle's
  `defined-variables.php:391`).
- **Assignment as an operand** at every binary operator, by one grammar rule
  instead of the per-level patches: `$a ?? $b = 1`, `1 | $g = 2` and
  `2 * $i = 3` report `assign.invalidExpr` and `variable.undefined` today.
  Snapshot 0a8fe69 has 22 `assign.invalidExpr`, e.g. `$checkpoint ??
  $checkpoint = $this->position` in `gettext/src/Loader/StrictPoLoader.php:122`.
- **Trait adaptations** keep the `insteadof` choice and the `as`
  visibility, with resolved trait names (`use T { foo as private; }` leaves
  `foo` public, and `B::hello insteadof A` still picks `A`'s, today); C4's
  trait flattening reads them.
- **Promoted parameters** keep their `(set)` visibility (parsed and dropped
  today; C4's asymmetric-visibility fix reads it). A `&get` hook is
  recorded as by reference.
- **`$obj->$$name` and `$obj->${expr}`** keep both levels of indirection.
- **Spans inside strings.** Interpolated `$var` parts and literal chunks get
  their own spans (each spans the whole string today, so LSP references and
  renames of a `$var` in a string land on its start); heredoc `{$...}`
  positions account for the stripped indentation.

Follow-ups, the typed AST (§3.1):

- **Resolved names are read.** No engine code reads
  `QualifiedName::resolved`; 109 calls in 43 files re-resolve through a
  `NameContext`. C4 converts the builders and D3 the checker; the project
  rules, the LSP, `pest_config` and `deps` convert with the phases that move
  them. `NameContext::build` (the last namespace wins, used at six sites,
  anonymous classes among them) is deleted first, and `NameContext` becomes
  private to `syntax`. Its per-branch copy of the context for an `if` goes
  (PHP allows `namespace` and `use` only at the top level), and with it
  `imbl` in `syntax`.
- **Declarations carry their namespaced name** (class-likes, functions,
  global constants), before C4's builder commit reads the AST, so nothing
  tracks namespaces positionally.
- **Names.** Declaration and member names are `Name`s; named arguments,
  `static` and `global` variables, labels and `use` imports are spanned
  names (`Arg.name: Option<String>` and `UseImport.path: String` today); the
  `"class@anonymous"` placeholder becomes `None`.
- **Language constructs are nodes**: `isset`, `empty`, `unset` (a
  statement), `exit`/`die`, `eval`, `include`/`require` with their kind,
  `print`, `list()` apart from array literals, `@` (dropped today), a
  `Declare` holding its block (a `Block` led by a `Declare` today) and
  shell-exec without a synthesized `shell_exec()` call. Today they are calls
  to names that name resolution resolves as functions.
- **`EnumDecl`** folds into `ClassDecl`, which already has `ClassKind::Enum`.
  Match arms are nodes with spans, the default arm in its place.
- **`NodeId`s** on the nodes rules and side tables key on beyond
  `Stmt`/`Expr`: class-likes, methods, properties, constants, enum cases,
  parameters, arguments, match arms, catch clauses, closure uses.
- **One parse entry point**, `syntax::parse(source, &Interner) ->
  ParsedFile` (statements, comments, lexer warnings, annotations). The ten
  lex-and-parse call sites use it, and the separate comment re-lexes
  (`ignore_comments.rs`, `output/codeframe.rs`) go. The nesting-depth limit
  is a `ParseError`, not a panic.
- **Walkers.** `Visitor` gains `visit_name` and `visit_closure_use`. The
  self-recursive walkers built on `child_exprs`/`child_stmts`/`stmt_exprs`
  (in 29 files, about 80 calls, each deciding closure and anonymous-class
  descent on its own;
  `architecture.rs`'s `new` scan skips parameter defaults, hooks and
  constants) become `Visitor` impls, and the three allocating helpers are
  deleted.
- **Annotations.** An arrow function is a catch-scope boundary.
- **Interner.** `From<&str>`/`From<String>` for `Name` intern globally
  behind a conversion; they are deleted once C4 routes names through the
  project's interner.

**B4. types redesign**, one commit each (§3.2):

- variants;
- shape keys and unsealed tail;
- `Name`-based classes;
- `Hash`;
- accessory types;
- TrinaryLogic;
- the ClassHierarchy trait with the unified `is_super_type_of`/`accepts`;
  reflection's implementation arrives in C8, and until then an adapter over
  Registry implements the trait;
- union algebra and coercions moved in;
- PhpVersion(s);
- float formatting;
- ExtensionTypeData.

Then extract `types`, with the proptest seeds moved to
`crates/types/proptest-regressions/`.

Follow-ups:

- **One subtyping implementation.** `Relation::subtype` (a bool, the old
  `is_subtype` renamed, 56 callers) is deleted; its callers use
  `is_super_type_of(..).yes()`. `is_super_type_of` stands on its own and
  `accepts` is built on it (today its `Yes` follows `accepts`).
  `types_definitely_disjoint` and `types_provably_disjoint` become
  `is_super_type_of(..).no()` both ways, and `conditional_branch` reads the
  relation.
- **One union.** `union_of_branches` and `late::join` go through
  `union_types`. A union keeps an explicit `mixed` explicit (behaviour fix:
  `$b ? $mixedParam : null` is reported at level 10 only, not 9; `@var
  array|mixed|null` is a declared type, `missingType.property` today, the
  rules oracle's `bug-5834.php`). It
  follows `TypeCombinator::union` (behaviour fixes, each reproduced):
  `never` is absorbed (`$c ? 1 : throw new E()` is `1|never` today, 48
  nsrt mismatches), and integer ranges merge when they overlap or touch
  (`int<10, 20>|int<5, 25>` stays unmerged today, and
  `@param int<0, 5>|int<6, 9>` widens to `int`). Constant arrays union by
  `processArrayTypes`/`reduceArrays` with tagged unions preserved: shapes
  merge only when lossless, `array{}` stays its own member unless an
  all-optional shape absorbs it, and same-key-set variants merge per
  position only past `ARRAY_COUNT_LIMIT`. Today `merge_array_shape_pair`
  merges every pair: `$c ? ['t' => 'a', 'v' => 1] : ['t' => 'b', 'v' =>
  's']` is `array{t: 'a'|'b', v: 1|'s'}`, so `$a['t'] === 'a'` leaves
  `$a['v']` as `1|'s'`, and `if ($c) { $l = ['x']; }` gives `array{0?:
  'x'}` instead of `array{}|array{'x'}` (163 nsrt mismatches);
  `union_types_keep_shapes_distinct` is deleted. Members are
  deduplicated, generic arguments included (`Collection<Dog|Dog>`,
  `Collection<int|int|string, …>` and `Map<string|int|string, …>` today,
  `bug-2735.php`, `bug-13192.php`, `bug-10871.php`), and `bool` absorbs
  `true` and `false` (`bool|int|false` is `bool|int`, `bug-15268.php`;
  `false|bool` and `true|bool` are `bool`, `bug-9634.php`,
  `bug-10938.php`).
- **Intersections reduce** by the class hierarchy (`TypeCombinator::intersect`):
  an intersection of a class and its subclass is the subclass. In
  `while ($foo instanceof Dim) { …; $foo = $bar; }` with `Dim extends Expr`,
  the second pass sees `$foo` as `Expr&Dim` today (and `$node instanceof
  MethodCall` on a `CallLike` leaves `CallLike&MethodCall`, nsrt
  `pr-5596.php`). Two shapes, or an array and a shape, intersect into one
  shape: `array{0: 0|1|2|3, 1: …}&array{0: int, 1: Payload}` is
  `array{0|1|2|3, Payload}` (`bug-11234.php`, `constant-array-intersect.php`).
  A shape intersected
  with `non-empty-array` stays a shape that can't be empty: `array{foo?:
  string, bar?: string}&non-empty-array` is plain `non-empty-array` today,
  its keys lost, and `missingType.iterableValue` (the rules oracle's
  `bug-7143.php`). An array with
  `HasOffset`/`HasOffsetValue` is non-empty: after `isset($array['def'])`
  on `array<mixed>`, `$array` is `array&hasOffsetValue('def', mixed)`
  today, so passing it to a `non-empty-array` parameter is `argument.type`
  (the rules oracle's `bug-12847.php`).
- **Error type.** PHPStan's `ErrorType` is a variant: the result of an
  operation the rules report as invalid, a read of an undefined member, or
  destructuring a non-array. It accepts and is accepted by everything, so a
  reported error doesn't cascade. Today the rule reports and the expression
  keeps a guessed type: `-'blabla'` is `int`, `$c / 0` is `int|float`,
  `$o->missing` and `[$a] = 5` give `mixed` (289 nsrt `*ERROR*`
  expectations; Appendix H's `error-type` has 65 of them in files cited
  nowhere else: a missing member on a union or intersection, GMP operators
  with a non-numeric string, a PHPDoc for a missing parameter, `$this` in a
  static closure). D3's handlers produce it.
- **Type behaviour lives on the type.** PHPStan asks a type what it does
  (`getOffsetValueType`, `hasOffsetValueType`, `setOffsetValueType`,
  `getIterableKeyType`/`getIterableValueType`, `isArray`/`isList`/
  `isString`/`isObject`/`isCallable`/`isIterable`, `isNull`/`isTrue`/
  `isFalse`/`toBoolean`, `isConstantValue`, `getConstantStrings`/
  `getConstantArrays`/`getConstantScalarValues`, `generalize`,
  `toNumber`/`toArray`/`toArrayKey`, `looseCompare`, `tryRemove`); here each
  question is a `match ty` in whichever checker file needed it, answered
  again wherever else it came up, and not always the same way: four
  `flatten_union`s (`by_ref_params`, `property_type_too_wide`,
  `return_type_too_wide`, `throws/too_wide`), two `is_array_like`
  (`expr/array_ops`, `var_tag_types`), two `is_object_like`
  (`narrowing/type_ops`, `strict_idioms`), two `generalize` (`comparisons`,
  `constant_resolver`), `iterable_element_type` (`stmt/loops`) beside
  `iterable_value_of`/`iterable_key_of` (`var_tag_types`), and
  `is_string`/`is_string_type`/`is_exactly_string_operand`/
  `is_string_shaped`/`is_string_or_nullable_string` across `strict_rules`,
  `offset_assignment`, `operators` and `expr`. Measured: 373 functions in
  `crates/phpstan-rs/src` take a `Type` (204 free, 169 methods): 63 rules
  that report, 8 scope storage, and 302 questions - 52 `is_*`/`has_*`
  predicates, 41 template binding, 32 offset and key, 28 constant values,
  22 iteration, 22 name resolution, 21 narrowing, 15 coercion, 14 union
  flattening, 13 transformations (accessories stripped and rewrapped,
  nullability added), 12 function return types, 10 member lookup, 9
  relation wrappers, 7 comparison, 4 generalization. 108 files hold about
  1,055 more inline inspections (`matches!(.., Type::..)`, `if let
  Type::..`, match arms).
  - Each question becomes one `Type` method (a trinary one where PHPStan's
    is: `is_array()` of `array|int` is `Maybe`), or a `Relation` method
    where it needs the class hierarchy, in `crates/types`.
  - The checker's helpers are deleted, and every inline inspection that
    asks one of these questions calls the method. What stays in the
    checker is building types from syntax and deciding what to report.
  - Where two copies disagreed, the method follows PHPStan and the
    difference is fixtured.
  - `Type` becomes opaque: its representation is private to
    `crates/types`, and other crates build types through constructors
    and read them only through methods, so matching on a type outside the
    crate doesn't compile. While sites move, an xtask gate counts `Type::`
    patterns outside `crates/types` and fails when the count grows; once
    it reaches zero the variants go private and the gate is deleted.
  - Subtraction, `Relation::remove` and the coercions already moving in
    follow the same shape. Member lookup (`getMethod`/`getProperty`/
    `getConstant`) moves once C8's reflection exists.
- **Subtracted types.** `mixed~T`, `object~T`, `Foo~Bar` and
  `$this(Foo~Bar)` (PHPStan's `SubtractableType`): narrowing a type away
  from `mixed`, `object`, a class, `static`/`$this` or a template keeps
  what was removed. Today `$m !== null`, `isset($m)` and `!is_int($m)` on a
  `mixed` all leave plain `mixed`, so a level-9/10 check can't see that
  `null` is gone, and after `if ($o instanceof \Exception) { return; }` a
  later `$o instanceof \InvalidArgumentException` isn't
  `instanceof.alwaysFalse` (`impossible-instanceof.php:303`, `:307`,
  `:318`, `:322`, `match-expr.php:86`, `:92`; Appendix H's `subtracted`,
  54 nsrt mismatches: `mixed~null` after `isset`, `mixed~bool` after
  `!is_bool`, `$this(Foo~(Bar|Baz))`, `mixed~object`,
  `bug-15060.php`, `mixed-subtract.php`, `this-subtractable.php`).
  - The subtraction is a field of the subtractable variants, not a
    wrapper: `Mixed { explicit, subtracted }`, `Object { class, args,
    subtracted }`, `ObjectWithoutClass { subtracted }`, and the same on
    `This`/`StaticRef` and `TemplateParam`. With `Type` opaque (the item
    above), these are fields of its private representation: `mixed~null`
    answers every question a `mixed` does, and only `crates/types`'s own
    patterns change, none of them left to a `_` arm.
  - `==` on `Type` is structural, every field included; PHPStan's
    `equals()` (which ignores `explicit`) is `Type::equals`. Each `==` on
    types today is audited into one or the other, and each `==
    Type::Mixed(false)` into `is_mixed()` or an exact comparison.
  - One set of `Type` methods (`subtracted`, `subtract`,
    `with_subtracted`, `without_subtracted`) carries display
    (`mixed~null`, `Foo~(Bar|Baz)`), the relations, union
    (`mixed~null|null` is `mixed`) and intersection.
  - Narrowing is `Relation::intersect` on the true branch and
    `Relation::remove` (`TypeCombinator::remove`) on the false one, in
    `crates/types`: `narrow_type_to` and `narrow_type_excluding` become
    calls to them, and their special cases (`string` less `''`, `bool`
    less `true`, `Traversable` less `Iterator`) become rules of `remove`.
    Removing an enum case and truthiness below are further rules of it.
  A `@phpstan-sealed a|b|c`
  class is its permitted subclasses for narrowing (validated today,
  oop.rs and `sealed()`, but never read by inference): after `if ($s
  instanceof a) { return; }`, `$s` is `s~a` where it stays `s` today,
  excluding all three leaves nothing, and `match (true)` over the
  `instanceof` arms or `match ($s::class)` over the class names is
  exhaustive; today each is `match.unhandled`
  (the rules oracle's `sealed_concrete.php`, `bug-12241.php`).
- **A conditional on a template distributes over its argument**:
  `(T is false ? never : T)` with `T` bound to `stdClass|false` is
  `stdClass`, `never|stdClass|false` today (the rules oracle's
  `bug-10653.php`).
- **Built-in classes with offset access** are offset-accessible without
  `ArrayAccess` (`ObjectType::EXTRA_OFFSET_CLASSES`: `DOMNamedNodeMap`,
  `DOMNodeList`, the `Dom\` node lists and maps, `PDORow`,
  `ResourceBundle`, `FFI\CData`, `SimpleXMLElement`, `Threaded`), for
  reading and existence: `$v[0]` on a `DOMNodeList` is
  `offsetAccess.nonOffsetAccessible` today (80 rules-oracle findings in
  `internal-classes-overload-offset-access*.php`).
- **Removing an enum case** leaves the enum's other cases
  (`TypeCombinator::remove` through `getEnumCases()`): after `if ($s ===
  FE::A) { return; }` on a three-case `FE`, `$s` is `FE::B|FE` today, a
  wrong value, and the `else` of an `A`/`B` `elseif` chain is `FE` where it
  is `FE::C` (the rules oracle's `bug-8485.php`).
- **Truthiness removes whole members**: the falsey side of a union drops
  its always-truthy members and the truthy side its always-falsy ones.
  After `$a = $c ? [] : true`, `if ($a) {…} else {…}` leaves `array{}|true`
  in the `else`, and `$b = $c ? ['foo'] : false` keeps `array{'foo'}|false`
  there, so passing either on is `argument.type` (the rules oracle's
  `call-methods.php:1414`, `:1425`).
- **Template inference** is PHPStan's structural `inferTemplateTypes`:
  through generic arguments (by ancestor bindings), callables, shapes, array
  keys and iterables, unioning every binding (behaviour fix, each case
  fixtured: `@param T $a, T $b` infers `1` for `(1, 2)`; `array<K, V>` keys,
  `ArrayObject<int, T>`, `callable(): T` and `array{x: T}` infer nothing;
  `@param T ...$v` binds only the first argument, so `collect('a', 'b',
  'c')` returning `T` is `'a'`, a wrong value, where PHPStan has
  `'a'|'b'|'c'`: nsrt `bug-2572.php`, `bug-7704.php`). An inferred
  template type is generalized as `generalizeInferredTemplateType` does:
  a literal becomes its general type unless the template is covariant or
  its bound is scalar. Today `new Box(1)` is `Box<1>`, so `$b->set(2)` on
  `@param T $v` is reported `argument.type` (nsrt `generics.php`,
  `nested-generic-incomplete-constructor.php`). A union parameter binds `T`
  only from what its other members don't cover: `@param T|null $x` called
  with `null` or nothing, and `@param T|int|float $p` called with `1`,
  leave `T` unbound (`mixed`); today they bind `null` and `1` (nsrt
  `bug-8441.php`, `generic-unions.php`). A `mixed` argument binds `T` to
  `mixed`: `$this->doFoo($mixed)` for `@param T $t` is `argument.type`
  "expects T, mixed given" today (the rules oracle's
  `check-explicit-mixed.php`, `check-implicit-mixed.php`). Inference reads
  through intersections: `m($i)` for `@param Sel<V> $s` on
  `Col<stdClass>&Sel<stdClass>` is `mixed` today (`bug-6904.php`). A
  generic argument that is itself a template substitutes as that template:
  `$s->first()` for `@return T` on `Sel<V>` inside a function templated on
  `V` is `mixed` today, so returning it under `@return V` is
  `return.type` "expected V, got mixed" (the rules oracle's
  `bug-10698.php`, `bug-14638.php` among the 24). Appendix H's template
  causes are the nsrt side (265 mismatches): `new` infers the class's
  templates from its arguments (`generic-new-infer`, 54: `new WeakMap()`,
  `new ArrayObject([...])`, `new ReflectionClass($this)`), a callable
  argument infers from its parameters and return
  (`callable-template-infer`, 35), bounds that are generics or unions stay
  `T` (`template-bound-generic`, 23; `template-bound-union`, 15), and a
  `never` or empty-array argument infers `never` (`template-never`, 14).
- **A template narrows by intersection**: `$b instanceof A` on a `T` gives
  `A&T` and its `else` `T~A`, merging back to `T`; today the branch is `A`
  and the merge `A|T` (the rules oracle's `bug-6635.php`).
- **Late types.** `new<T>` and `template-type<...>` get variants (PHPStan's
  `NewObjectType` and `GetTemplateTypeType`); `int-mask<>`/`int-mask-of<>`
  resolve in C7's resolver instead of reading as `int`; `key-of<C::ARR>`
  resolves like `value-of<C::ARR>` (it reads as `mixed`; `@param
  key-of<self::ALL> $code` on a `string` is `parameter.phpDocType` today,
  `call-methods.php:1769`). `key-of<T>` and
  `value-of<T>` of a template stay late (`KeyOfType`/`ValueOfType`) and
  answer through the template's bound: a template `Offset of
  key-of<DataArray>` with `DataArray of array<string, mixed>` passed to
  `array_key_exists()` is `argument.type` "Offset given" today (the rules
  oracle's `overriden-abstract-trait-method-phpdoc.php`). An offset access type
  over a template (`Bob[TKey]`, `Data[value-of<T>]`) stays late too
  (`OffsetAccessType`) and resolves per call and per object: `$y->x('b')`
  for `@return Bob[TKey]` with `@template TKey of key-of<Bob>` is the
  native `string|bool|int|float` today where it is `bool`
  (`bug-13652.php`; so its `@return` against the native type is
  `return.phpDocType` today), and `$goto->data` typed `Data[value-of<T>]` on
  `GotoRoute<GotoTarget::PERSONALMANAGER>` is `mixed|null` where it is
  `array{token: string}` (`bug-11033.php`).
- **Callability and class-strings in `accepts`** (behaviour fix): a string
  literal is callable only when it names an existing function or
  `Class::method`, an array only as `[object|class-string, method]` with the
  method existing (and static when the first element is a class-string:
  `Closure::fromCallable([self::class, 'sayHello'])` for an instance
  method is accepted today, `bug-1971.php`), and a class-string literal
  must name a class within the bound. A `static` parameter type is the
  called class and its subclasses: `B::staticTest($b)` with `B|string`
  for `@param static|string` passes today (`bug-2164.php`). Today
  `'no_such_function'`, `[1, 2]` and `'stdClass'` for
  `class-string<DateTimeInterface>` are accepted. A plain `string` isn't a
  `class-string` either: `make($s)` for `@param class-string<T> $c` is
  `argument.type` in PHPStan and accepted today. A closure is accepted by a
  callable type only when its signature fits (parameters contravariant,
  the return covariant): `set_error_handler(function (int $no, string
  $str): void {})` against `(callable(int, string, string, int): bool)|null`
  is accepted today (32 corpus oracle findings). Without
  `strict_types=1`, a `string` parameter accepts an object with
  `__toString()`, as PHP converts it: `strnatcasecmp($foo, $bar)` on
  `Stringable` objects is `argument.type` today (the rules oracle's
  `bug-11619.php`). A union covering every class that can implement an
  interface userland can't accepts that interface
  (`UnionType::EQUAL_UNION_CLASSES`): `DateTime|DateTimeImmutable` accepts a
  `DateTimeInterface` and `Error|Exception` a `Throwable`, both
  `argument.type` today (the rules oracle's `call-methods.php:1696`,
  `:1714`). A shape is accepted by a general array whose key type is a
  union of literals covering its keys: `['a' => 4, 'b' => 52]` returned
  under `@return array<'a'|'b', int>` is `return.type` "got array{a: 4, b:
  52}" today (the rules oracle's `bug-8683.php`). An integer range is
  accepted by a union of the constants it spans: `int<0, 3>` by `0|1|2|3`
  is `assign.propertyType` today (`bug-3383.php`,
  `int-ranges-and-constants.php`). In the other direction these are
  accepted today and are not: a template `T` (bound `mixed`) passed to an
  `int` parameter (`check-explicit-mixed.php:65`,
  `check-implicit-mixed.php:65`, `call-static-method-mixed.php:81`); a
  `stdClass` or plain `object` for an object shape (`object{foo: int, bar:
  string}`; `object-shapes.php`, 7); an array whose key type isn't covered
  (`array<int>`, keys `int|string`, for `array<string, int>`,
  `bug-3107.php`; `array<mixed>` for `array<string, mixed>`,
  `bug-9732.php`; `array<int<0, max>, bool>` for `array<0|1|…|17, bool>`,
  `bug-7594.php`), or lacking an unsealed shape's required key
  (`non-empty-array<mixed>` for `array{bar: string, ...}`,
  `bug-2911.php`); and an invariant generic with a `mixed` argument
  (`Inv<mixed>` for `Inv<int>`, `bug-11935.php`, `bug-3589.php`), or with
  a template bounded by the argument (`Format<O of Output>` for
  `Format<Output>`, `bug-3028.php`).
- **Conditional parameter types** resolve per call: `@param ($flags is 256
  ? string : bool) $demoArg` takes a `string` only when `$flags` is `256`;
  `cond('s')` and `cond(true, 256)` are unreported today
  (`conditional-param.php`, 3). A conditional parameter type narrows with
  the parameter it names (`$k is 'a' ? int<0, 1> : non-falsy-string` is
  `int<0, 1>` under `$k === 'a'`, `bug-7915.php`), and conditional returns
  and `@param-out` types resolve the same way: on an array's emptiness, a
  template, `$this`, nested conditions picking the first match, and the
  argument's type before the call reassigns it (`$items =
  iteratorToList($items)`, `bug-11565.php`); Appendix H's
  `conditional-return`, `conditional-param` and `conditional-param-out`
  are 81 nsrt mismatches.
- **Class-aware unions.** With C8's hierarchy, a union drops members another
  member covers (`A|B` for `B extends A` stays two members). Identical
  members collapse too: a closure parameter today reaches
  `Node|Node|Node|Node|Node|Node&Return_&Return_&Return_&Return_&Return_`
  (D5), and `is_a()` on unknown classes gives the same shape (nsrt
  `is-a.php:38`).
- **Display is unambiguous**: a string literal escapes its quotes (`'a\'b'`
  displays as `'a'b'`), and a callable's union return and a callable inside
  a union are parenthesized (`Closure(int): (int|string)` displays as
  `Closure(int): int|string`, `(callable(): void)|null` as
  `callable(): void|null`). A string with control characters displays
  escaped in double quotes: `PHP_EOL` is `' '|' '` today, with the raw
  line breaks between the quotes. A float literal keeps its decimal point:
  `@param 0|0.0|1.5 $x` displays as `0|0|1.5` today. Appendix H's
  `type-print` (127 nsrt mismatches, 42 files) are the display forms left:
  each becomes this printer's form or a documented normalization in the
  nsrt gate.
- **Callable parameters** carry their names, and a callable its purity.
  `CallableParam` has no name today, so `$f(valu: 1)` on `fn (int $value)`
  isn't reported and a conditional return `($value is int ? …)` can't bind
  its parameter; `pure-callable(int): int` reads as `callable(int): int`.
- **`iterable<K, V>`** keeps its arguments (behaviour fix). It is
  `array<V>|Traversable` today, the key and `Traversable`'s arguments lost,
  so a `foreach` over `iterable<int, string>` yields `mixed` key and value,
  and a generator declared `iterable<int>` may yield `'x'` unreported. A
  `T[]` in a PHPDoc union with an iterable type gives that type `T` values
  (`TypeNodeResolver::resolveUnionTypeNode`): `int[]|\Traversable` is
  `Traversable&iterable<int>`, where today unpacking it into `int
  ...$foo` is `argument.type` "mixed given" (the rules oracle's
  `function-with-variadic-parameters-7.1.php`).
- **Typed templates.** `TemplateParam`'s name is a `Name` and its scope an
  enum (class, function, method, callable) instead of `Option<String>` with
  a `"callable"` sentinel; a `Conditional`'s subject is an enum (parameter
  or template) instead of a string and `template_subject`.
- **Patch versions.** `PhpVersion` carries the patch: the false branch of
  `PHP_VERSION_ID >= 80105` keeps 8.1 (`narrow_at_least` drops it, which
  D5's scope narrowing would read as dead code).
- **Tests move in.** `checker/tests/accessory_and_refinement_types.rs`
  (12 tests of `types::union_types`) and
  `checker/tests/array_literals_and_shapes.rs` (2 of
  `generalize_by_key_set`) move to `crates/types`.

**B5. phpdoc redesign.**

- `DocBlock` stores `TypeNode`s.
- TypeNode → Type resolution goes behind an interface that reflection
  fills in C7; until then a resolver adapter.
- Extract `phpdoc`; the `parse_type_string` fuzz target repoints to the
  phpdoc parser.

Follow-up: **one tag splitter.** `grammar::parse` joins wrapped lines with a
space and cuts each line at `//` (so a `'http://a'` type breaks);
`docblock::parse` joins with a newline and leaves `//` comments to the
lexer. One splitter remains, `DocBlock` is built from its `PhpDocNode`, and
`phpdoc_unresolvable`, `generics/ancestors` and `unknown_phpstan_tags` read
that parse (C4's store of parsed doc blocks holds it).

Follow-up: **PHPDoc number literals** follow phpdoc-parser: hex, octal and
binary, `_` separators and exponents (`int<0, 0xFF>`, `int<0, 1_000>` and
`1e3` report `phpDoc.parseError` and read as `mixed` today).

Follow-up: **the rest of phpdoc-parser's type grammar**, each today a
`phpDoc.parseError`: a by-reference callable parameter without a name
(`callable(string &): void`, the rules oracle's `pipe-operator.php`), an
offset access as a conditional's subject (`(T[0] is string ? … : …)`,
`(M[K] is not null ? M[K] : D)`: `bug-8609-function.php`, `bug-7310.php`)
and a conditional chained in the else branch without parentheses (`(T is
positive-int ? 1 : T is negative-int ? -1 : 0)`, `bug-8609.php`); and
`$this` as a callable parameter's type (`\Closure($this): object` reads as
plain `callable` today, so it is `parameter.phpDocType` "callable is not
subtype of native type Closure", `bug-9009.php`).

Follow-up: **what phpdoc-parser rejects is `phpDoc.parseError`**, where
the parser today accepts it: `@use FooTrait` without generic arguments
(expected `<`), `callable() $cb` without a return type, `A & B | C`
without parentheses, `\\Foo` with a doubled backslash, `@phpstan-type
InvalidFoo what{}`, and a hook's `@return Test(` (the rules oracle's 11
PHPStan-only, `impossible-instanceof-in-trait.php`,
`check-implicit-mixed.php:102`, `invalid-phpdoc*.php`,
`invalid-type-type-alias.php`). A tag that doesn't parse or resolve is
dropped, so an untyped property under `@var callable(int)` or `@var
class-string<int>` is `missingType.property` too (`invalid-phpdoc.php:59`,
`incompatible-property-phpdoc.php:18`).

Follow-up: **a tag's diagnostics sit on the tag's line**: an invalid
`@param`, `@var` or `@return` is `phpDoc.parseError` on its own line, not
the declaration's, so an `@phpstan-ignore-line` or `-next-line` inside the
doc block applies to it (the rules oracle's `ignore-line-within-phpdoc.php`,
`invalid-phpdoc.php`: 12 phpstan-rs-only findings that PHPStan reports on
the tag lines or ignores). `DocBlock` keeps each tag's line for this.

Follow-up: **shape keys follow phpdoc-parser's identifier token**, which
takes `-` and `\` anywhere after the first character: `array{psr-4?:
list<string>}` and `array{x-1: int}` report `phpDoc.parseError` today (a
`-` before a digit ends the key), and `array{e\f: int}` parses as
`array{0: e\f: int}` with a `class.notFound`. The lost type cascades: the
rules oracle's `bug-6000.php` and `bug-8467a.php` go on to
`foreach.emptyArray`, `offsetAccess.notFound` and a `class.notFound` on
the alias (13 findings).

Follow-up: **type parsing is linear** in a type's length and depth-limited
with a parse error: an `array<...>` nested 1,000 deep takes 3 s and 2,000
deep 9 s today (20,000 deep does not finish).

**B6. sql.**

- Its own column-type model, `[sql_reflection]` parsing, and `SqlContext`
  as a service.
- The SQL state moves off `Registry` (byte-identical).
- The SQL → PHP `Type` mapping moves to the package's future `ext-sql`
  module.
- Extract `sql`.

Follow-ups:

- **Identifier case per driver.** `SchemaCatalog` compares table and column
  names exactly; lookups fold case where the driver does (MySQL and SQLite
  column names; Postgres folds unquoted identifiers to lowercase), fixture
  first.
- **Nullability through outer joins.** The Postgres reflector reads
  `pg_attribute.attnotnull` and the SQLite one the origin column's
  `NOT NULL`, so a column reached through a `LEFT JOIN` is typed non-null;
  a column on the nullable side of an outer join is nullable.
- **Connection errors are reported.** `build_reflector` drops a failed
  connection's error (`.ok()`), so a wrong password or host silently turns
  reflection off; it becomes a startup warning.

**B7. diagnostics.**

- The diagnostic model (§3.7): `Diagnostic` gets its validated
  `Identifier` and a list of tips, and drops `location` and `metadata`.
  `RuleError`, the file and trait file, identifier families and AST-node
  fixes arrive with the rule dispatcher (E3).
- The versioned baseline plus converter, as an approved-delta commit
  (location dropped, text format change).
- Extract `diagnostics`. The ignore / baseline pipeline stays in the
  package's future `project` module.

Follow-ups:

- `DiagnosticFix::within` (a text search for a name whose span is missing)
  is deleted; its two sites use the name's span.
- **Baseline paths** are relative to the baseline file's directory, as in
  PHPStan. Today they are the analysed paths as spelled, so running from
  another directory with the same baseline reports every baselined error.

**B8. composer.** The package context is computed once. Extract. The
default `PhpVersions` range comes from it (§3.2): composer
`config.platform.php`, else `require.php`. An approved-delta commit of its
own.

Follow-ups:

- **Constraints follow Composer's grammar**, whitespace after an operator
  included: `"php": ">= 8.1"` gives no range today, so every
  version-gated rule is silent. An open upper bound runs to PHPStan's
  `MAX_PHP_VERSION` (80699, PHP 8.6): `^8.5` is `80500–80599` today, so
  a `PHP_VERSION_ID >= 80600` branch reads as dead code.
- **No global panic hook.** `discover_vendor_context` replaces the process
  panic hook while it parses, which races parallel analysis; the parse
  already returns its errors.
- **`config.vendor-dir`** is read: the manifest and the vendor discovery
  hardcode `vendor/`, so a project with its own vendor directory gets no
  installed packages and no vendor context.
- **`psr-0` autoload** entries count in `Autoload` (only `psr-4`, `classmap`
  and `files` are read).
- **The root package's own autoload is context.** Analysing named files
  loads `vendor/` but not the root package's `autoload`/`autoload-dev`
  targets, so a class of the project that isn't named is unknown:
  `check-project ./src/Piwigo/Activity/ActivityRepository.php` reports
  `class.notFound` for `ActivityObject` (in `src/`, `Piwigo\` → `src/`),
  `argument.type` with `mixed`, and `match.unhandled` on an exhaustive
  `match` over it, 12 findings that naming the enum's file too removes.
  PHPStan's autoloader covers the root package. The corpus oracle passes
  phpstan-rs the files by name, and at `76e738b4` 1,253 of its 1,855
  phpstan-rs-only `class.notFound` name a `Piwigo\` class.
- **A package's bundled `vendor/` is context.** `rector/rector` and
  `symplify/easy-coding-standard` autoload only a `bootstrap.php` that
  registers their own `vendor/autoload.php` at runtime, so their classes
  (`Rector\NodeTypeResolver\…`, `PHP_CodeSniffer\…`) are unknown to
  phpstan-rs, which reads composer metadata without running it: 495
  phpstan-rs-only `class.notFound` in `vendor/rector/rector` alone. An
  installed package's own `vendor/composer/autoload_*.php` is read like the
  root's.

**B9. stub-sources.**

- The generated index plus the xtask plus the regeneration test.
- The crate owns the `include!`.
- Extract.

Follow-up for every extracted crate: comments follow the comment rules.
About 130 sites cite package modules the crate can't see, phases, fix
numbers or investigation history (syntax 19, types 73, phpdoc 29, sql 8,
diagnostics 2, composer 2).

### Stage C: config and reflection

A bug below that sits in code a later Stage C phase rewrites gets a fixture
first and is fixed in that phase, as its own behaviour commit after the
structural one (Appendix F).

**C1. config.** The schema, owner sections, `--set`, deletion of
the per-rule flags and setters (an approved delta for the CLI surface),
extension activation; the settings split per Appendix B, each type in its
owner module (`project::session::SessionSettings`,
`registry::settings::ReflectionSettings`,
`checker::settings::EngineSettings`); the `config` crate holds loader and
schema; extract.

Follow-ups, one commit each with its test:

- **Nested validation**: `deny_unknown_fields` on every nested struct
  (`IgnoreErrorEntry`, `RestrictedSymbolEntry`, `ParamCondition`, `ParamRef`,
  `ParamFlagsCondition`, `DoctrineCustomFunctionEntry`,
  `sql::config::SqlReflectionConfig`). An `[[ignore_errors]]` entry needs
  `message` or `identifier`, and `count` is enforced. It takes PHPStan's whole
  shape and semantics (`Analyser/Ignore/IgnoredErrorHelper.php`,
  `IgnoredErrorHelperResult.php`): `raw_message`, the plural `messages`,
  `raw_messages`, `identifiers` (only the first present of the three expands)
  and `paths`, the string form, per-entry `report_unmatched`; entries with the
  same path, message and identifier merge with their counts summed (a missing
  count is 1); `count` applies to single-path entries only; a malformed entry
  fails the load. Messages compile once, at load, through PCRE2 (the
  `pcre2-sys` path of `pcre_validate`) with the pattern's own delimiters and
  modifiers, an invalid one failing the load; without `pcre-validate` they
  compile with `regex`, a pattern it rejects failing the load, and its
  divergences (`$` before a final newline, Unicode `\d`/`\w`) documented.
  `IgnoredRegexValidator`'s checks are load errors (an unescaped `||` or `()`,
  a top-level alternation starting with a type name, a mid-pattern anchor;
  entries with `count` skipped); they need a PCRE-syntax parser and the phpdoc
  type parser. **Error paths**: every type error names its full dotted key.
- **Missing settings**, each with the phase that implements it: mixin exclude
  classes (`ReflectionSettings`, C8), universal object crates
  (`ReflectionSettings`, C8; replacing the hard-coded `stdClass` in
  `checker/expr.rs`), global type aliases (`ReflectionSettings`, C7), a
  `php_version` range and patch version (C5), treatPhpDocTypesAsCertain,
  polluteScopeWith* and rememberPossiblyImpureFunctionValues (D5),
  early-terminating calls (D2), the exception model (E3f), additional
  constructors and always-read/written property tags (keys for D6's built-in
  `AdditionalConstructorsExtension` and dead-code-roots implementors),
  `exclude_paths.analyse`/`exclude_paths.analyse_and_scan` (PHPStan's
  `excludePaths`: a flat list means `analyse_and_scan`, an entry marked optional
  may be missing, any other missing literal path fails the load; applied by
  `Session`, the LSP's workspace load and incremental file adds, replacing
  `DEFAULT_SCAN_EXCLUDED_DIR_NAMES`; vendor context loaded through Composer
  stays loaded), `file_extensions`, `report_ignores_without_comments`,
  `use_path_constants_as_constant_string`,
  `check_internal_class_case_sensitivity`, and the rest of PHPStan 2.2.16's
  `parametersSchema.neon` that changes analysis, checked one by one against the
  code: `polluteScopeWithBlock` and `inferPrivatePropertyTypeFromConstructor`
  (D5), `checkExplicitMixedMissingReturn` (E3f),
  `checkMissingOverrideMethodAttribute`/`PropertyAttribute`,
  `reportMaybesInMethodSignatures`, `reportStaticMethodSignatures`,
  `reportMaybesInPropertyPhpDocTypes`, `reportWrongPhpDocTypeInVarTag` and
  `reportAnyTypeWideningInVarTag` (E3e), `reportNonIntStringArrayKey`,
  `reportUnsafeArrayStringKeyCasting` and
  `reportPossiblyNonexistent{Constant,General}ArrayOffset` (E3b),
  `editorUrl`/`editorUrlTitle` (F3). Each bleeding-edge check phpstan-rs runs
  unconditionally becomes a `featureToggles` setting defaulting on
  (`unusedLabel`, `newOnNonObject`, `checkDateIntervalConstructor`,
  `newStaticInAbstractClassStaticMethod`, `checkPrintfParameterTypes`,
  `checkParameterCastableToNumberFunctions`, `internalTag`,
  `unnecessaryNullCoalesce`, `sortWithoutEffect` and the rest, checked one by
  one; about 70 of the rules oracle's phpstan-rs-only findings);
  `skipCheckGenericClasses` (`DOMNamedNodeMap`, `ParentIterator`,
  `RecursiveCachingIterator`, `RecursiveFilterIterator`,
  `RecursiveRegexIterator`, `ReflectionObject`) exempts those classes from
  `missingType.generics`, reported for them today (11 findings, `bug-9615.php`
  and `internal-classes-overload-offset-access*.php`), and
  `checkGenericIterableClasses` (off by default) exempts every generic
  `Traversable` subtype the same way (`bug-9981.php`, 6). phpstan-strict-rules
  turns several of these on (`polluteScopeWithBlock: false` among them), so E4's
  ext-strict-rules reads them. PHPStan's runtime parameters (`tmpDir`,
  `resultCache*`, `productionMode`, `bootstrapFiles`, the playground and CLI
  ones) have no phpstan-rs key; `init` lists them as not converted (ROADMAP
  Phase 183).
- **One settings assembly**: `lsp/workspace_config.rs` stops assembling
  `ProjectSettings` itself and uses `Session::resolve`'s function: the same
  validation (overlays, architecture, banned constructs, psr3 pattern), the
  same `stub_paths` meaning, config-relative paths, and an error surfaced
  instead of an empty config; F1 still deletes `ResolvedWorkspaceConfig`.
- **Activation reaches library typing**: PHPUnit assertion narrowing and mock
  types, the test base classes, the parent-call defaults and the Mockery /
  Nette Tester assertion defaults follow their extension's activation;
  mockery, nette-tester, codeception, type-perfect, disallowed-calls and sql
  get `Extension` impls. `test.risky` gets its own activation (ext-extras or
  an explicit key) instead of `phpstan/phpstan-phpunit`'s, its corpus delta
  re-reviewed; Pest risky tests get their own `[ext.pest]` key.
  Strict-rules activation also sets the core settings phpstan-strict-rules'
  `rules.neon` sets: polluteScopeWith* off, dynamic properties, explicit-mixed
  missing return, function name case, internal class case, maybes in method
  signatures and property PHPDoc types, static method signatures, wrong PHPDoc
  type in `@var`, strict printf placeholders.
- **Paths and matching**: `ignore_errors.path` resolves against the config
  file's directory and is compared as a normalized path, by one matcher with
  PHPStan's `File/FileExcluder` semantics (a literal file, a literal directory
  as a prefix, or an fnmatch pattern: `*?[]`, `*` crossing `/`) shared with
  `exclude_paths`; `glob_matches` goes. `--set` paths resolve against the
  working directory, like every other command-line path.
  `psr3_context_key_pattern` is a PCRE pattern matched through PCRE2, as
  ignore messages are, not `regex` syntax.
- **Entries validate at load**: a `restricted_symbols` member entry without
  `class` is a load error, and a user entry wins over a preset's for the same
  symbol; `[[assertions]]`/`[[class_argument_returns]]` type strings parse at
  load, and an entry naming a missing function, method or parameter is a load
  error.
- **Config discovery**: `check-project` without `--config` reads a config in
  the working directory, as the LSP does at a workspace root (the CLI reads
  none without `--config` today): the first of `.phpstan-rs.toml`,
  `phpstan-rs.toml`, `.phpstan-rs.dist.toml`, `phpstan-rs.dist.toml` wins, as
  PHPStan's six `phpstan.neon` names do, one lookup serving the CLI and the
  LSP. The LSP watches the config files (it registers `**/*.php` only today),
  so an edit reconfigures without a client configuration push.
- **One walk per file**: the file walker dedups by canonical path, so a
  symlink back into the tree (`src/up -> ..` checks one file 41 times today)
  or an alias of an analysed directory is walked once.
- **Keys**: grouped keys name the upstream parameters they cover
  (`ext.shipmonk.check_null_coercion` the three `forbidNullIn*` rules,
  `ext.shipmonk.check_enum_exhaustiveness`, `rules.check_dead_members`,
  `ext.psr3.check_psr3_logging`); one casing, no key repeating its section's
  name (`ext.psr3.check_psr3_logging`, `ext.psr3.psr3_context_key_pattern`,
  `ext.sidz.magic_number_exemptions`), a PHPStan parameter's own name where
  one exists, settled in C1. PHP versions and extension
  activation read every discovered composer root's manifest, as
  `phpstan_api_check_enabled` does.
- **PHPStan configs** are ROADMAP Phase 183's (the `neon` crate,
  `phpstan-rs init`), built on this schema: every key records the PHPStan
  parameter it stands for (the keys follow-up), which its parameter table maps
  by.
- **Tests and docs**: `config/src/tests.rs` and `app/tests.rs` cover nested
  keys, `--set` of an unknown key and error key paths, and
  `loads_every_ignore_error_field_combination` drops its `path`-only entry; a
  config reference is generated from the section doc comments;
  `check-project` help is corrected (`--php-version` defaults from composer,
  `--level` unset caps at `DEFAULT_REPORT_LEVEL` 6, `--stub-path` and
  `--format` stop citing deleted flags and `config.rs`), and
  `declared_php_versions` gets its own doc comment (`project/session.rs`).

**C2. Settings leave `Registry`** (Appendix B): `RuleLevelHelper` to the
package's future `analyser` module, `ConstantResolver` to `reflection`, rule
and extension settings to their owner modules, `phpstan_api` recording
unconditional. Gate: reflection reads no settings type but
`ReflectionSettings` (an `xtask arch` rule: no `*Settings`, `CheckSettings`
or `ProjectSettings` named under `registry`/`stub_attributes`).

Follow-ups:

- **The gate is implemented** (`xtask/src/arch.rs` has no settings rule).
- **Predefined constants over a version range** (fixed first):
  `PHP_VERSION_ID` is `int<min.id, max.id + 99>`,
  `PHP_MAJOR_VERSION`/`PHP_MINOR_VERSION` are ranges or unions over the
  range, and a predefined constant never falls back to the stub's `define()`
  value. The integer-size constants cover 32- and 64-bit builds: `PHP_INT_MAX`
  is `2147483647|9223372036854775807` (the 64-bit value only today; nsrt
  `constant-phpdoc-type.php`).
- **`ConstantResolver` in `reflection`**, with `dynamic_constant_names` a
  `ReflectionSettings` key; `resolve_constant_named_types` and class-constant
  initializer typing read constants through it.
- **Initializers evaluate as `InitializerExprTypeResolver`**, one evaluator
  for class and global constants, enum case values and parameter defaults
  (D3's arithmetic shares it): arithmetic, bitwise operators, ternaries, `??`,
  global and class constants, enum cases and `new` each give `mixed` today
  (`const A = 2 + 3`, `const G = 1 << 2`, `const E = PHP_INT_SIZE`), and a
  backed enum's `case A = self::X` or `case B = 2 + 3` types `->value` and
  `value-of` as `int`. DBAL's `Converter` has `public const CASE_LOWER =
  CASE_LOWER;` (a `use const` import; `\CASE_LOWER` is `mixed` too), so
  `@param self::CASE_LOWER|self::CASE_UPPER|null $case` is
  `mixed|mixed|null` and its `match` on `$case` is `match.unhandled`
  (`doctrine/dbal/src/Portability/Converter.php:55`).
- **One folded constant key**: a constant's namespace prefix folds case at
  every lookup, `checker/imports.rs`'s `use const` check and
  `dynamic_constant_names` included. **`generalize`** keeps list-ness and the
  key type and flattens unions.
- **`RuleLevelHelper` owns the rule** (E3's prologue): the level thresholds,
  benevolent unions and strict mixed arrays leave `types::Relation::accepts`,
  and `types::Relation` loses `configured_level`, `strict_mixed_arrays` and
  `check_benevolent_union_types`. Assertion tables are reapplied on
  reconfigure until C6 compiles them to overlays.

**C3. Reflection data model** (§3.5): opaque reflections, attributes with
arguments, provenance, no AST bodies.

Follow-ups:

- **C3a's `property.unused` delta** (fixed first): ORM-mapped properties
  count as used while ext-doctrine is active, view objects passed to a
  template renderer aren't reported, and every line of
  `gates/approved/c3-provenance-declarations` is re-reviewed.
- **`is_phpstan_code`** becomes a provenance query (an `Origin::Phpstan`
  class counts), `phpstan_class_files` deleted.
- **Attribute arguments** evaluate to types: constants and class constants
  through the ConstantResolver, bare constant names namespace-resolved,
  `self`/`parent`/`static` bound to the class, `new` and constant expressions
  included (C8). `#[Table(self::TABLE_NAME)]` on a class is `argument.type`
  "mixed given" today (the rules oracle's `bug-12011.php`), and an undefined
  constant (`#[MyAttr(self::BAR)]`, `classConstant.notFound`) or another
  class's private one (`#[MyAttr(Foo::FOO)]`, `classConstant.private`) is
  unreported (`class-constant-attribute.php`, 6 and 1).
- The rest of C3's data model lands in C4: class and method keys, per-member
  flags and `Vec<TemplateParam>`, build scratch in the builder, provenance as
  the only location record, query accessors, parse-time resolved names, one
  no-native-type encoding (`Option<Type>`).

**C4. Snapshots and lazy resolution**, one commit each, in this order:

- **`xtask gatetime`**: `--record` runs HEAD's gated steps (the fixture suite
  and the lib tests; C5 adds the cli tests) N = 3 times each, binaries
  prebuilt, with the profile and environment `gates` uses (the fixture suite
  with type dumps on), and writes their medians to
  `.corpus-runs/gate/gatetime/<hash>.gatetime.tsv`; `--since <hash>` does the
  same for HEAD and fails on any gated step whose median isn't lower than
  `<hash>`'s. The phase start is recorded before the next commit, so no old
  commit is rebuilt.
- **Keys and data model** (structural, except that two class-like
  declarations, or two methods of one class, whose names differ only in case
  are one name, as in PHP, and meet under the collision policy instead of
  standing as two: an approved delta, fixtured):
  - class entries keyed by the folded name (`Name::lower`, as
    `FunctionTable` keys functions) hold the declared `Name`; the side map and
    `canonicalize_class_name_case` are deleted; method keys fold,
    `method_access` included; property and constant keys stay exact;
  - a folded key never allocates at lookup: a `&str` is hashed and compared
    case-insensitively in place (FxHash over ASCII-lowercased bytes,
    `eq_ignore_ascii_case`), a `Name` through its stored `lower()`;
    `FunctionTable`, which lowercases into a new `String` for any name with an
    uppercase letter today, moves onto the same key type;
  - every lookup that compares spelling today goes through an explicit
    exact-spelling accessor, so nothing changes until the behaviour commits
    delete them: a type's or a literal's class name; a method name in the
    magic-method comparisons, the override, final-method and
    abstract-implementation checks (`final_methods`, `abstract_methods`,
    `has_concrete_implementation`, `trait_declares_abstract_method`),
    purity's dependency lookup (`dependency_is_pure`) and the ancestor walk
    behind override checks and PHPDoc inheritance
    (`lookup_ancestor_method_with_owner`); only the lookups with a
    case-insensitive fallback today (`lookup_class`, `lookup_method`,
    `lookup_method_with_owner`) fold;
  - `Vec<TemplateParam>` and per-member flags (a property's finality moves
    off the class's `final_properties` set onto it), read by the four places
    rebuilding `TemplateParam`s from the parallel template maps
    (`own_template_args`, `checker/resolve.rs`'s `class_template_params_for`,
    `build_template_params`, the `ReflectionClass` stub patch); every native
    type an `Option<Type>` (`PropertyReflection::has_native_type` goes, the
    `explicit_mixed_hint` flags derive from the native and PHPDoc `Option`s);
    accessors return `Option<&T>`/`&[T]`; the builder parses each doc comment
    once (a method's is parsed per use today);
  - provenance is the only location record (`is_stub` and
    `function_decl_sites` gone; the `.stub` class pass records each embedded
    `.stub` file as `Origin::Phpstan`; `Origin::Unknown` goes with
    `Registry::build`), and the six comments citing the deleted
    `project/decl_locations.rs`/`Project::decl_locations` are corrected;
  - query accessors replace the storage-returning ones and the `pub` maps;
    every direct `registry.classes`/`functions`/`global_constants` access
    outside `registry/` is converted (about 280, 23 of them in tests);
  - the builders read the AST's parse-time `QualifiedName::resolved`
    (`extends`, `implements`, `use`, native type hints) instead of
    re-resolving through a `NameContext`, except the anonymous-class builder,
    whose whole-file context disagrees with it, until its behaviour fix;
    PHPDoc names, which are text, keep the file's name scope until C7's one
    scope;
  - no result depends on the class map's iteration order, which folded keys
    change: `classes_using_trait` returns its hosts sorted (they order the
    per-host trait checks), and `doctrine_entity_for_table` picks among
    entities mapping one table by name order instead of the first found (an
    approved delta, fixtured).
- **Written results leave the signatures**, everything a remaining pass
  writes moving out of the views the memo will own: purity results go to a
  registry table keyed by function and by `(class, method)`
  (`purity_dependencies` and `is_transitively_pure` leave
  `FunctionReflection`, the checker asks the table, C9 moves it to
  `AnalysisContext`); `names_with_assert_tags` is seeded from the assertion
  entries when the tables are set, not by the pass, and holds and is checked
  by folded name, so a call spelled in another case than its method reaches
  the assert lookup, which already falls back case-insensitively (an approved
  delta, fixtured; today the prefilter compares spellings exactly and skips
  the narrowing).
- **The checker test harness builds like a project** (a test-expectation
  delta, listed in the commit): it merges the stubs before resolving, instead
  of resolving the test program alone and then recomputing only ancestors and
  member owners, so its classes resolve against the stubs. Until the memo
  lands each harness run pays the passes over the stubs; only the phase end
  is timed.
- **Per-class memoized resolution** replacing the passes:
  - per-class memos: `flatten_traits`, `resolve_method_tags`,
    `precompute_member_owners`, `resolve_deferred_type_imports`,
    `resolve_deferred_value_of`, `flatten_phpdoc_inheritance`,
    `inherit_property_hooks`, `resolve_deferred_mixins`,
    `precompute_ancestor_names`, `drop_doc_members_excluded_by_native`,
    `resolve_constant_named_types`;
  - interim until C6: `apply_assertion_table` becomes a step of the own
    level, its entries matched by folded class and method keys;
  - whole-program passes until C9, run after resolution:
    `resolve_transitive_purity` (classifying only functions, and classes
    with a member, trait or mixin, from an analysed or context file) and
    `infer_global_variables`;
  - each name keeps its merged declaration beside its memo, the memo's input:
    build scratch (`method_tags`, `trait_aliases`, `redeclared_methods`,
    `resolved_trait_conflicts`, the `*case_refs`, `type_imports`) leaves
    `ClassReflection` for it, and the rules reading that scratch (duplicate
    methods, trait conflicts and aliases, class-name case and internal-usage
    checks on PHPDoc types) query the declaration; the declaration keeps its
    parsed doc blocks, which the rules re-parsing a declaration's doc comment
    (about 25 sites in `checker/`) read; the snapshots split it per file;
  - `Project::finish_loading` keeps only the whole-program passes and stays
    the batch entry point (69 call sites, until F1's Session), and every
    comment naming a deleted pass (about 100 outside `registry/`) is
    rewritten;
  - `index_and_flatten` is deleted; `Registry::build` indexes and runs the
    whole-program passes as `finish_loading` does (its callers' purity and
    global-variable tests read them) until its callers move onto `apply`;
    the stub pass still resolves each stub file's classes within that file
    (until C5's stub layer), under default reflection inputs (it is
    process-wide), without global-variable inference.

  The memo has five levels, each a per-class `OnceLock`. A level reads the
  declarations, a lower level of any class, and the same level only along an
  ancestor edge:
  - **ancestors** (declarations only): the `extends`/`implements` closure in
    resolution order, the `use` closure in composition order, the `@mixin`
    targets (each edge naming its target by the declared spelling the folded
    lookup finds), the unresolved edges (a declared name no class answers
    to, as written, with the class declaring it), and the class's constant
    declarations composed with its traits';
  - **own**: own members composed with the `use` closure's declared members
    (a trait's `@method` tags stay its own), `@method` tags, then every type
    resolved in the class's context (imported aliases, `value-of` and
    class-constant types, in `extends`/`implements` arguments too,
    constant-named types), and the interim assertion step;
  - **inherited**: PHPDoc inheritance and property hooks from the ancestors'
    inherited level;
  - **members**: each `@mixin` target's public methods and properties, at the
    target's inherited level, added where the class's own level has none of
    that name (an inherited one doesn't stop it, as today, until C8's mixin
    extension replaces the copy and fixes that); then the member owners;
  - **final**: PHPDoc members the native type excludes dropped, the relation
    reading other classes' members level. The native/PHPDoc combination is
    `TypehintHelper::decideType` whole: native union members the PHPDoc type
    doesn't cover are added back, and an `array` PHPDoc with a mixed key under a
    native `iterable` becomes `iterable`. Today `@param string[]` on
    `string|array|A` loses `A`, `@param array<int>` on `iterable` is
    `array<int>`, and the parent's `@param string|string[]` inherited onto
    `string|iterable|AbstractString` loses `AbstractString` (snapshot 0a8fe69's
    15 `instanceof.alwaysFalse` in `symfony/string/ByteString.php` and its
    siblings, which the oracle doesn't report). An inherited PHPDoc type naming
    a subclass of the child's native class type applies as an own one would: for
    a parent's `@return Baz` on `: Bar`, the child's `r(): Bar` returns `Bar`
    today, and a parent's `@param Baz $o` leaves `$o` as `Bar` in the child's
    body (an inherited `@return array<int>` on `: array` already applies; nsrt
    `method-phpDocs-inheritdoc.php`, `methodPhpDocs-implicitInheritance.php`).
    Appendix H's C4 causes are the rest: `@inheritdoc` without braces and from a
    trait's abstract method, 12; `iterable<T>&Collection` returns kept whole,
    19; a child's templates substituted into the parent's methods, 21; trait
    templates through `@use`, 11. The combination reads the PHPDoc with its
    aliases resolved: `@param OwnerBackup|null` for a
    `@phpstan-type OwnerBackup array{name: string}` on `?array` is
    `array{name: string}|null|array` today, the native `array` added back beside
    the alias, so it is `missingType.iterableValue` (a written
    `array{name: string}|null` already works; `bug-8190.php`, `bug-13654.php`,
    `bug-7954.php`). On a variadic parameter with no native type, an array
    PHPDoc type is the element type (`decideTypeFromReflection`):
    `@param array ...$args` takes any argument and `@param string[] ...$args`
    takes strings, where today `variadicMethod(1, 2, 3)` is three
    `argument.type` "expects array" (`call-methods.php`). A PHPDoc return over a
    native `void` applies when it is `never` or a conditional of `void` and
    `never`: `@return ($c is true ? void : never)` makes `callExit(false)`
    `never` where it is `void` today (`bug-10337.php`). A built-in parent's stub
    PHPDoc is inherited the same way: an untyped
    `#[\ReturnTypeWillChange] getChildren()` under `RecursiveFilterIterator`
    returns `static` and an untyped `key()` under
    `@extends \IteratorIterator<array-key, …>` returns `array-key`, so
    `return null;` in them is `return.type`, unreported today (`bug-9615.php`,
    3; `bug-6249.php`).

  Class constants type through the ConstantResolver (C2's follow-up lands
  first), reading the ancestor level's constant declarations with the
  class's imported aliases substituted; a constant reference cycle, found
  from the declarations with `*` patterns expanded against the ancestor
  level, types as `mixed`, with a test. An enum's case values are constants
  to the resolver too, so `value-of` and `->value` read them evaluated. A
  function resolves lazily the same way (value types, constant-named types,
  native-excluded members, the interim assertion step);
  `finalize_standalone_sig` is that resolver.

  A cycle through `extends`, `implements` or `use` is detected on the
  ancestor level and treated as broken: every edge between two classes of
  the same cycle is dropped, the same whichever class resolves first, with a
  test. `@mixin` edges are never followed at their own level, so a mixin
  cycle needs no breaking. No `OnceLock` initialization then waits on itself;
  a reentrant resolution panics, and a panic leaves the cell retryable and
  pops the reentrancy guard's frame as it unwinds, so `check-project`'s
  per-file `catch_unwind` resumes cleanly (a panic-injection test and a
  parallel-resolution test over cyclic fixtures). A level stores what it
  changes and shares the rest with the level below (member maps behind
  `Arc`). Within this commit a change to any reflection input (a
  declaration, `ReflectionSettings`, the resolved `PhpVersions` range, the
  assertion tables until C6) resets every memo,
  `doctrine_entity_table_index` included, at no cost while none has been
  read; the collision policy reads declarations only. Trait and mixin members
  no longer outlive a change to their source (§1), the closure test landing
  with the snapshots:
  `removing_a_files_trait_does_not_retroactively_unbake_its_members_from_existing_users`
  (`project/tests/file_removal.rs`) inverts, renamed through
  `gates/test-renames`, and `remove_files`' known-limitation doc comment
  goes.

  `has_unresolvable_ancestor` reads the unresolved edges; the boolean
  `is_subclass_of` keeps counting an unresolved name as an ancestor until C8.
  A whole-registry iteration decides from declarations, provenance and the
  ancestor level before resolving a class, until C5's scoped queries replace
  it: the two `native_type_hints.rs` scans, the concrete-implementor scan in
  `checker.rs`, the whole-program passes (dead abstract classes, dead enum
  cases, dead members, namespace cycles), `class_shape`,
  `classes_using_trait`, `doctrine_entity_for_table`, `repository_entity`,
  `Project::run_reflection_rules` (nothing resolved when no rule is
  registered), LSP workspace symbols and completion; the `demo` command
  resolves everything it prints.

  Approved deltas, each fixtured: a method two nested traits both supply
  resolves by `use` order, depth-first; a constant on a reference cycle types
  as `mixed` (today `const A = [self::B]; const B = self::A;` types `A` as
  nine nested `array{0: ...}` around `mixed`). Purity moving after resolution
  changes nothing (its `void` gate reads a return type the declaration
  already reconciled with the native one, and its lookups read names,
  visibility and finality, which no later step rewrites), with a fixture
  pinning `@return null` on `: void`. The commit runs the full gate tier;
  peak RSS stays at or below the phase start; a test pins that loading a
  project and checking one file resolves no class that file doesn't reach.
- **`ReflectionBuilder::apply` snapshots.** `apply` takes any number of
  `FileDelta`s (a file's new declarations, or its removal); the initial load
  is one apply, its declarations built per file in parallel as
  `build_declarations` does today. A `FileDelta` carries the file's role
  (analysed, context, or a `stub_paths` file), which the collision policy
  reads. A name's entry is recomputed from its declaring files by the
  collision policy in a canonical order (the embedded stubs, then context
  files, then analysed files, each by path: the order `check-project` and the
  LSP's workspace load use today), not in arrival order, so a file created or
  opened later lands where a fresh load would put it and a delta gives what a
  fresh load gives (an approved delta the language server shows: such a file
  no longer wins a name collision by arriving last). Declarations are shared
  between snapshots by `Arc`; memo cells are not: a new snapshot starts with
  fresh cells, seeded with every value already computed and not invalidated,
  so a cell an older snapshot fills after the apply never reaches the new
  one. Each memo records every folded class and constant name it looked up,
  absent ones included, through a per-resolution read set the hierarchy and
  the lookups append to; a delta invalidates the transitive readers of the
  names it changes, a change to any other reflection input invalidates
  everything, and the new snapshot inherits every other memo. Purity and
  global-variable inference rerun per snapshot until C9. Snapshots own their
  names: the registry's and checker's `syntax::intern::intern` calls (about
  100) go through the project's interner; the stub layer's names stay global;
  `apply` sweeps the session interner's entries nothing else holds. A closure
  gate is a test that applies a sequence of deltas and compares every query on
  the result with a fresh build of the final file set: a property test over
  the fixture files, and one test per staleness case below.

  With declarations kept per file, the checker stops rebuilding reflection
  from the AST: the rebuild-and-compare sites (`ClassReflection::from_decl`
  and `from_enum_decl` in `oop.rs`, `from_decl` for a trait in
  `function_decl.rs`) and a losing duplicate function's rebuilt signature
  read the file's own declaration, comparing as today; the builder builds
  property-hook signatures with their property, the signature
  `check_hook_phpdoc_types` synthesizes a `FunctionDecl` for today; a set
  hook's parameter takes the property's PHPDoc type where it narrows the
  native one (`set (array $val)` on an `@var array<string>` property sees
  `array<string>`, `array` today; nsrt `property-hooks.php`). The per-file
  sets also answer what `Project` indexes separately today:
  `DuplicateClassSites` and `DuplicateFunctionSites` (with their own
  declaration walks) are deleted for a query over them that groups by exact
  spelling, as the indexes do, and `FileState.functions`/`classes` become the
  file's set.

  `Registry` splits in this commit: `ReflectionBuilder` holds the per-file
  declaration sets and `apply`; `ReflectionProvider` is the snapshot, and
  `Registry`'s query methods move onto it. Its 22 fields go:
  - `classes`, `functions`, `global_constants`: the declaration sets plus the
    provider's memos;
  - `ancestor_names`, the five `*_owner_by_class` maps: memo levels;
    `classes_lowercase`: gone with folded keys (keys commit);
  - `unconditionally_declared_classes`/`_functions`,
    `stub_provided_functions`, `built_in_stub_classes`: facts of the
    declaration sets (C5 turns the last two into provenance queries);
    `names_with_assert_tags`: derived from the sets and the assertion
    entries; `function_decl_sites`: gone for provenance (keys commit);
  - `doctrine_entity_table_index`: a provider memo (C8's data extension
    takes it);
  - `php_versions`, `configured_assertions`,
    `configured_class_argument_returns`: reflection inputs the builder holds
    (C6 compiles the last two to overlays);
  - `global_variables` and the purity table: per snapshot on the provider
    until C9's `AnalysisContext`;
  - `phpstan_class_files`: already gone (C3's follow-up).
- **Staleness fixes** (approved deltas), each with a closure gate, through the
  per-file declaration sets (which also hold what
  `unconditionally_declared_classes`/`_functions` record): anonymous classes
  removed with their file version (§1); a file's global constants removed
  with it; removing a class or function declaration merged onto, or
  replacing, a built-in keeps the built-in; removing one of two same-named
  declarations keeps the other; `stub_provided_functions`,
  `names_with_assert_tags` and the dead-code escape sets
  (`escaped_enum_cases`, `escaped_member_classes`) follow their files.
- **Registry tests** (188, `registry/tests` and `registry/stubs/tests`) and
  the other `Registry::build` callers (the checker test harness,
  `phpstan_api`, `rule`, `doctrine` result-shape and `main` tests, the demo)
  move onto `ReflectionBuilder::apply` and the query accessors, names kept;
  `Registry::build` and `Origin::Unknown` are deleted.
- **Behaviour fixes**, one commit each after the structural ones:
  - declarations below the top level are indexed (inside function bodies,
    `try`, `switch`), conditional, through one Visitor walk that also
    replaces `declared_names`, `collect_anonymous_classes` and the
    stub-index generator's walk;
  - class names resolve case-insensitively where a type's or a literal's
    name is looked up, and a type's class name takes the declared spelling
    where the type is resolved: `$s->nope()` on a parameter typed `square`
    (class `Square`) reports `method.notFound`, and messages print `Square`;
    a written union collapses members naming one class (`@param Foo|foo` is
    `Foo|foo` today); the exact class accessors are deleted;
  - a conditional or duplicate class-like declaration is checked against its
    own declaration's hierarchy, not only when its edges happen to match the
    declaration holding the name (`oop.rs`'s `registry_matches`);
  - duplicate class-like and function declarations compare names
    case-insensitively (`class Foo` beside `class FOO`, `bar()` beside
    `BAR()` go unreported by `class.duplicate` and the function check today):
    the snapshot commit's duplicate query groups by folded key;
  - member names fold case everywhere: abstract-method implementation,
    final-method overrides, constructors (`__Construct` promotes its
    properties), `__toString` making a class `Stringable` (a trait's
    included: `new Foo() instanceof Stringable` for a `Foo` using one is
    `instanceof.alwaysTrue`, unreported today, `bug-13469.php`), the
    magic-method comparisons; the exact method accessors are deleted;
  - member resolution follows PHP: the class chain first, then interfaces;
    an enum's native `cases()`, `from()` and `tryFrom()` implement an
    interface declaring them (`class.abstractMethodNotImplemented` today,
    the rules oracle's `bug-11592.php`, 6); a `@property` tag over a private
    property makes it readable from outside through `__get`
    (`private-property-with-allowed-property-tag-is-public.php`), and inside
    the class the native property wins over a child's tag, so
    `$this->attributes = [...]` on the native `array` isn't
    `assign.propertyType` against `@property stdClass` (`bug-13537.php`);
  - trait composition: `insteadof`, visibility-changing `as` (`hello as
    private somethingElse` makes `$bar->somethingElse()` from outside
    `method.private`, unreported today; `bug-12544.php`), a trait's `final`
    methods, an abstract method implemented by another trait, `@use
    Trait<T>` template bindings, and a trait member's aliases resolved in the
    trait's scope (`self` still the host); a trait method the host declares
    itself is not the host's and isn't analysed there (`$this->doBar()` in a
    shadowed trait `doFoo()` is `method.notFound` today,
    `shadowed-trait-method.php`); under `@use TA<B>` with `a as renamed`,
    both `$this->a()` and `$this->renamed()` for `@return T` are `mixed`
    today (`bug-6856.php`);
  - each `@extends`/`@implements` tag binds only the parent it names: with
    `@extends Prov<T>` and `@extends \IteratorAggregate<string, T>` on one
    interface, `Coll<int>::get()` (`@return T` in `Prov`) is `string` today,
    a wrong value, and iterating it gives `mixed` instead of `int`.
    Symfony's `ServiceCollectionInterface` has that shape, so every
    `ServiceLocator<…>::get()` is `string`, and deptrac's
    `$collector instanceof CollectorInterface` after one is
    `instanceof.alwaysFalse`
    (`deptrac/src/Core/Layer/CollectorProvider.php:27`,
    `Supportive/OutputFormatter/FormatterProvider.php:28`);
  - PHPDoc inheritance carries `@phpstan-assert*`, `@param-out`,
    `@phpstan-self-out`, `@throws`, `@phpstan-pure`/`@phpstan-impure` (an
    implementation of a `@phpstan-pure` interface method is checked for
    impure points: an `echo` in it is `impure.echo`, unreported today) and
    method templates, and a property redeclared without `@var` keeps its
    parent's (`mixed` today; nsrt `inherit-phpdoc-merging-var.php`), so it
    isn't `missingType.property` (`protected $message = '…'` in an
    `Exception` subclass, `public $layout = null` over a typed parent:
    `bug-11761-bis.php`, `bug-10987.php`, `bug-7074.php`, `bug-7692.php`,
    `bug-7839.php`); its default is checked against the inherited type
    (`$primaryKey = ['abc']` under the parent's `@var string` is
    `property.defaultValue`, `bug-7074.php`), and an inherited
    `@phpstan-pure` makes a child's `void` method `pureMethod.void`
    (`bug-14563.php`). Inheritance searches every parent interface: for
    `interface MC extends CC, M` with `add()` declared on the second, an
    untyped `add($item)` in an implementing class reads `$item` as `mixed`
    and is `missingType.parameter` today, so `$item->getName()` is
    `method.nonObject` (`bug-4415.php`);
  - `@phpstan-type` aliases apply in `@property`/`@method` and
    `@phpstan-assert*` tags (`@phpstan-assert-if-true Callback1 $callback`
    is `class.notFound` today, `bug-10594.php`) and on enums, chained to any
    depth; an alias on an import cycle stays unresolved;
  - enums read `@deprecated` on cases, `@var` on constants, and
    `@phpstan-require-*`;
  - promoted properties carry asymmetric visibility and their own
    `@deprecated`/`@internal`;
  - anonymous classes resolve names per namespace block, and diagnostics
    name them `class@anonymous`;
  - `merge` splits into one operation per case (phpstorm-stubs overload,
    stub override, polyfill onto a built-in, duplicate declaration), with the
    enum arm under the same collision policy; the registry tests pinning the
    union (`two_unconditional_*` in `stubs_and_conditional_declarations.rs`)
    are rewritten per case;
  - LSP navigation (`class_declaration`, `member_declaration`) uses the
    folded keys;
  - a table two entities map resolves to no entity, as `repository_entity`
    already treats a repository two entities name.

Gate: the gated steps' medians are lower than the phase start's (`xtask
gatetime --since`); no stub class resolves unless something reaches it. The
keys, memo and snapshot commits each run the full gate tier (§4): they change
every lookup's hashing, the resolution path, and the LSP's per-edit
invalidation.

**C5. Locator chain and stub layer**, one commit each:

- the stub layer's version key over a `PhpVersions` range, decided first and
  recorded in Appendix F. It governs 2,234 `LanguageLevelTypeAware` and 598
  `PhpStormStubsElementAvailable` attributes in 88 phpstorm-stubs files (185
  declarations change type at 8.4 or later), the functionMap deltas (C6) and
  the version-specific `.stub` files; member and parameter gates are inside
  the files, so the layer parses per version. `php_version` accepts the range
  and patch forms in the same commit. A function removed before the analysed
  range is unknown: `mysql_connect()` under PHP 8.5 resolves today where
  PHPStan reports `function.notFound` (the corpus oracle's 42 calls, all
  `mysql_*` or `ldap_control_paged_*`; the rules oracle's 9 PHPStan-only are
  `create_function()`, `convert_cyr_string()`, `fgetss()`,
  `get_magic_quotes_gpc()`, `hebrevc()`, `restore_include_path()` and the
  rest of `removed-functions-from-php8.php`);
- phpstorm-stubs' `PS_UNRESERVE_PREFIX_` names are the reserved words they
  spell (`Generator::throw()` and `IntlCalendar::isSet()` are
  `method.notFound` today, `bug-3683.php`), and the prefixed functions
  (`array`, `list`, `die`, `exit`, `empty`, `isset`, `unset`, `eval`,
  `__halt_compiler`) and the `___PHPSTORM_HELPERS` classes, which are
  language constructs, leave the index (it lists them today);
- `stub_attributes` fixes: a property's or class constant's
  `LanguageLevelTypeAware` type becomes `@var`, not `@return`; `newest_type`
  reads every entry; a function deprecated in one version-split overload
  isn't deprecated in the others;
- locator order;
- the lazy composer locator (approved delta): classes load on demand;
  functions, constants and `files` autoload entries stay eager;
- the version-keyed stub layer holding C4's memo levels for stub classes,
  with copy-on-merge (a merged name, or a stub memo whose read set names a
  project declaration, materializes per project) and a per-file parse memo;
  `rayon` in the index build decided, Appendix E follows;
- provenance queries replacing `built_in_stub_classes` and
  `stub_provided_functions`, each consumer naming its question: "a stub
  declares it and no analysed code redeclares it" (the restricted-usage
  `is_builtin` flags in `expr.rs` and `oop/attributes.rs`,
  `checker/resolve.rs`) or "an extension in the stub corpus declares it"
  (composer hygiene, the architecture rules' `is_standard_class`);
- the enumeration APIs, every whole-registry iteration (C4's ancestor-level
  filters included) moved onto a scoped query: `native_type_hints.rs`'s two
  scans onto `subclasses_of`; `Project::run_reflection_rules`, the LSP
  workspace-symbol function list and LSP completion's candidate lists onto
  the project layer or `names(kind, layer)`; `repository_entity` onto
  ext-doctrine's data (E4). The index records `parent`/`interfaces` per
  declaration;
- version-based skip-polyfill (approved delta);
- version-dependent signature selection (approved delta);
- a panic-injection test.

Gate: the fixture suite's, the lib tests' and the cli tests' wall times are
lower than at the phase's start (`xtask gatetime --since`); the stub layer
replaces the per-project stub clone and the per-process stub parse; the
union of all lazily resolved symbols equals `generated_stub_data()`,
extension maps included, for the unset version key, checked before the two
approved-delta commits.

**C6. Overlay layer**, one commit each. Every entry's types are parsed with
the phpdoc grammar (TypeNode), never `parse_type_string`:

- phpstan-src `.stub` PHPDoc overlays (approved delta, split per stub file):
  the 30 files `conf/config.neon` lists (phpstan-src 2.2.16), plus the 10
  selected by the six gated `StubFilesExtension`s (`BcMathNumber`,
  `JsonValidate`, `ReflectionClass`, `ReflectionEnum`, `SocketSelect` on the PHP
  version; `ExtDs`, which took `ext-ds.stub` out of `config.neon` in 2.2.16, on
  the `ds` extension's major version), which port as built-in implementors. All
  146 functions and all method PHPDoc apply, not only class templates and
  property types, and reach an implementation's untyped parameters through
  PHPDoc inheritance: in a class with `@implements ArrayAccess<string, int>`,
  `offsetExists($offset)` and `offsetSet($offset, $value)` read
  `$offset`/`$value` as `mixed` today where the stub's `TKey`/`TValue` give
  `string`/`int`, so `key_exists($offset, $this->args)` is `argument.type`
  (`bug-12984.php`); a generic built-in's methods read its arguments
  (`$ao->append(new Exception())` on `ArrayObject<int, stdClass>` is unreported
  today; 3 findings in `calling-method-with-phpDocs-implicit-inheritance.php`);
- declarative patch entries replacing the Rust patch functions;
- the `functionMap.php` layer: `stub-sources` embeds
  `resources/functionMap.php` and the seven
  `functionMap_php{74,80,81,82,83,84,85}delta.php` files (the two
  `bleedingEdge` files decided), the index covers them, and a
  `SignatureMapParser` port reads them, replacing
  `patch_functionmap_array_shapes`; the merge follows
  `Php8SignatureMapProvider::mergeSignatures`. The native signature base
  (`phpstan/php-8-stubs` or phpstorm-stubs) is decided in the same commit and
  recorded in Appendix F; `registry/stubs.rs`'s module doc is corrected;
- the functionMap's signatures replace phpstorm-stubs' PHPDoc where they
  differ, removing the rules oracle's `argument.type`: `curl_init()`'s
  `__benevolent<CurlHandle|false>` passed to `curl_setopt`, `curl_exec` and the
  other `curl_*` handle parameters (39, "CurlHandle|false given"), the
  `__stringAndStringable` values of `printf`/`sprintf`/`vsprintf` (32, a
  `Stringable` object given), `round()`'s `RoundingMode` from PHP 8.4 (9),
  `time_nanosleep()`'s `int` parameters where the stub says `positive-int` (3,
  `0` given), `strpos()`'s `int` offset (1), `PDO::prepare()`'s and
  `DOMDocument::createElement()`'s benevolent `…|false` (6,
  `method.nonObject`/`property.nonObject` "non-object type PDOStatement|false"
  today), `DOMElement::getAttributeNode()`'s (2),
  `DateTimeImmutable::createFromFormat()`'s `static|false` on a subclass (1,
  `DateTimeImmutable|false` today), `mysqli::$affected_rows`'s
  `int<0, max>|numeric-string` (1, `int<-1, max>` today),
  `curl_share_setopt()`'s `mixed` value (3) and `stream_get_contents()`'s
  `__benevolent<string|false>` (3); the nsrt suite's are Appendix H's
  `stub-return` (111 mismatches, 33 files: `RedisArray::keys()`, the SPL
  recursive iterators' generics, `ReflectionEnum`, `Ds\Map::get()`,
  `ImagickPixel::getColor()`, `getopt()`, SQLite3) and `stub-asserts` (39:
  `dir()`, `chdir()`, `chroot()` and `is_readable()` narrowing the path);
- the functionMap's and the overlay stubs' narrower parameters, which PHP
  rejects at runtime or which throw, are the rules oracle's other C6
  `argument.type` misses (about 60): `array_chunk()`/`str_split()`
  `positive-int` lengths, `max()`/`min()` of `non-empty-array`,
  `json_validate()` depth and flags, `explode()` with `''`, `chr(256)`,
  `error_log()` with type `2`, `filter_input()`/`filter_input_array()` types,
  `flock()`'s `int-mask`, `unserialize()`'s options shape, `fputcsv()` fields,
  `strval()`/`intval()`/`floatval()` of arrays and objects,
  `Random\Randomizer::pickArrayKeys()`, the `RecursiveIteratorIterator` flags,
  and the `Reflection*` constructors' `class-string`
  (`new ReflectionClass($string)`, `new ReflectionProperty($string, 'foo')`,
  `new ReflectionEnum(stdClass::class)` against `class-string<T of UnitEnum>`);
- a function or method with several signatures (a phpstorm-stubs overload, a
  functionMap `'1` variant) keeps them all, and a call takes the variant its
  arguments fit, named ones included
  (`ParametersAcceptorSelector::selectFromArgs`): `strtr(string: $s, from:
  'a', to: 'b')`, `new DatePeriod(start: …, interval: …, end: …)`,
  `setcookie(expires_or_options: …)`, `stream_context_set_option(context:
  …)` and `session_set_save_handler(open: …)` are `argument.unknown` today
  (24 rules-oracle findings); a call that fits no variant is
  `arguments.count`: `rand(3)` and `mt_rand(1)` (zero or two arguments),
  `implode()` and `stream_context_set_option()` with three are unreported
  today (`arguments-count-accepted-by-no-variant.php`,
  `call-to-weird-functions.php`, 6);
- `resources/functionMetadata.php` embedded as the builtin purity source (C9
  reads it);
- user `stub_paths` with PHPStan `stubFiles` semantics (approved delta):
  `.stub` files load; a stub's PHPDoc replaces the declaration's, the native
  signature stays the real one, omitted members are kept, built-in functions
  are overridable, and a stub declares a symbol only when nothing else does;
- `StubFilesExtension`;
- assertion tables compiled to overlays (`apply_assertion_table` and the own
  level's interim assertion step deleted); the Mockery and Nette Tester
  defaults move to their ext modules, and `gates/module-graph.txt` splits
  `assertion_tables`.

**C7. TypeNodeResolver** with `TypeNodeResolverExtension`, replacing the B5
adapter (`registry::type_node`) and the checker's `resolve_type_node`:

- one name scope (namespace and `use` imports, active templates, class
  aliases, the current class for `self`) used by both the registry and the
  checker (`resolve_written_type`). An inline `@var self::COLUMN_A` or `@var
  self::COLUMN_*` reads as `mixed` today while `K::COLUMN_*` and a `@param
  self::COLUMN_*` resolve, so `sprintf('%s', $value)` after one is
  `argument.type` (`bug-8074.php`). PHPDoc keyword types resolve before
  namespace qualification: in a namespace, `pure-callable`, `pure-Closure`,
  `callable-array`, `callable-object`, `non-empty-literal-string` and
  `non-decimal-int-string` resolve to `N\…` classes today (`class.notFound`
  plus `parameter.unresolvableType`), and `enum-string` to `N\UnitEnum`
  (`bug-14557.php`, `bug-14549.php`, `bug-13114.php`, `bug-13067.php`,
  `pure-function.php`); `__benevolent<…>` is `class.notFound` in any
  namespace (18 findings, `bug-11663.php`). The active templates include an
  enclosing function's inside an anonymous class: `@implements Option<T>` and
  `@return T` in `new class($t) implements Option` read `T` as a class named
  `T` today (`bug-6353.php`);
- native `TypeHint` nodes are resolved directly, not printed and re-parsed
  (about 85 sites);
- an array PHPDoc type unioned with an iterable type gives it its value type
  (`TypeNodeResolver`'s union handling): `string[]|iterable` is
  `iterable<string>`, `Foo[]|Collection` is `Collection&iterable<Foo>`, and
  `array|int[]` is `array<int>` (`missingType.iterableValue` on it today,
  `missing-function-parameter-typehint.php`,
  `properties-assigned-types.php`), where today they are
  `array<string>|array|Traversable` and a union, so `return [1];` under
  `@return string[]|iterable` is unreported (`returnTypes-iterable.php`,
  `returnTypes.php:151`, `:166`);
- a template name is a template wherever a type is expected, shape keys
  included: `@return array{TKey: int}` under `@template TKey of string` has
  the literal key `'TKey'` today, so `sealed('a')` is `array{TKey: int}`
  where PHPStan has `array{a: int}`, a wrong value (nsrt
  `array-shape-template-key.php`);
- a generic with fewer arguments than templates fills them as
  `TypeNodeResolver` does: for an iterable class one argument is the value
  (`IteratorAggregate<T>` is `IteratorAggregate<mixed, T>`), where today it
  binds the first template, so a `foreach` over a class with `@extends
  \IteratorAggregate<T>` types the key as `Category` and the value as `mixed`
  (`bug-4415.php`); a missing argument with a default (`@template TResult =
  T`) takes it: `$i->work(null)` for `@return I<TResult>` on `I<string>` is
  `I<T>` today (`bug-4801.php`);
- a type alias defined through itself (`@phpstan-type A = array{next: B}`
  beside `B = array{next: A}`) is reported (`typeAlias.circular`); it reads
  as `mixed` inside the cycle today;
- a constant name in a PHPDoc type resolves through the name scope's `use
  const` imports and its initializer: `@param PREG_SPLIT_NO_EMPTY_ALIAS`
  under `use const PREG_SPLIT_NO_EMPTY as PREG_SPLIT_NO_EMPTY_ALIAS` reads as
  a class today, and `@param COPY` for `const COPY = PREG_SPLIT_NO_EMPTY` is
  `mixed`, where both are `1` (a class of the same name still takes
  precedence; nsrt `constant-phpdoc-type.php`);
- `types::parse` is deleted (`parse_type_string`,
  `parse_native_type_string`, `has_explicit_mixed_nested_somewhere`); the
  `types` tests that build types through it (about 150) use constructors, and
  its parsing tests move to `reflection` as resolver tests;
- the other package call sites (about 35) are converted, each listed in the
  commit;
- assert targets `$param->prop`, `$param->method()` and `$this->method()` are
  kept; `@throws` and assert types get alias substitution; a written
  `Traversable` resolves like any name.

**C8. Reflection extension points** (§3.5), with the built-in implementors
(universal object crates among them). Reflection implements ClassHierarchy
(`registry/relation.rs`'s impl moves); the trait takes `Name`s, lends
memoized views instead of allocating, exposes `TemplateParam`s, and gains
the enum-cases query.

The `Relation::structural()` callers switch to the hierarchy-aware relation
(approved delta): the 24 package sites and three in `types`
(`conditional_branch`, `intersection.rs`'s `narrower`, `coercion.rs`), whose
API takes a `&dyn ClassHierarchy`. Expected delta: `instanceof Child`
narrows `Base` to `Child`; `!$x instanceof Base` removes `Child` from
`Child|Other`; `($x is Base ? A : B)` picks `A` for a `Child`; `Foo&SubFoo`
collapses; the DQL entity-field refinement compares object types with the
hierarchy. The scalar-only sites don't change.

The boolean `is_subclass_of` is deleted for the tri-state relation, and an
ancestor reached through an unresolved edge (C4's ancestor level) answers
`Maybe` (approved delta, fixture first): `instanceof`, override, `final` and
`#[\Override]` checks through an unvendored parent stop treating it as
known. Consumers needing certainty treat `Maybe` as unknown. A class that
doesn't exist relates `Maybe` to everything too, so an argument check
against a parameter typed with it stays silent (the PHPDoc rule reports the
name): `@param SplObjectStorage<Cat, null>` in a namespace names the missing
`Bug10884\SplObjectStorage`, and passing a `\SplObjectStorage` is
`argument.type` today (`bug-10884.php`).
`removing_a_files_direct_ancestor_does_not_unstick_the_subclass_relationship_either`
and the `Grandchild`/`Child` assertion of
`removing_a_files_class_does_stop_a_transitive_ancestor_relationship_through_it`
(`project/tests/file_removal.rs`) answer `Maybe`, renamed through
`gates/test-renames`.

The `@property`, `@method` and `@mixin` implementors replace C4's copies (the
own level's tags, the members level's mixins): member lookup asks the native
members (own, traits, ancestors) first and these extensions after, as
`ClassReflection::hasMethod` does; a tag declared at the same or a closer
hierarchy distance than the native member wins, as in
`PhpClassReflectionExtension`. Behaviour fixes, one commit each after the
structural one, each fixtured:

- `@mixin` on a trait applies to the classes that use it, besides being
  `mixin.trait` (E3e): `$u->get()` and `$u->a` through `@mixin Foo<static>`
  on a used trait are `method.notFound`/`property.notFound` today (the rules
  oracle's two `trait-mixin.php`);
- a mixin member never shadows a native one, inherited ones included
  (`@mixin Mx` on a class whose parent declares `m()` types `$h->m()` by
  `Mx::m()` today);
- a mixin brings in the target's inherited members, the target's own mixins
  (guarded per type and member against recursion, as PHPStan's `inProcess`),
  and a used trait's mixins;
- `@mixin Box<int>` binds `Box`'s templates, and `@mixin T` for a class
  template mixes in the bound argument: `@mixin T` under `@template T of
  object` is `class.notFound` "unknown class T" today (`mixin.php` in
  Classes, Methods and Properties). The stubs' `@mixin T` on
  `RecursiveIteratorIterator` (and `@mixin TIterator` on `IteratorIterator`)
  forwards to the inner iterator: `$it->getSubPathname()` over a
  `RecursiveDirectoryIterator` is `method.notFound` today
  (`recursive-iterator-iterator.php`, 2), where it resolves and counts its
  arguments;
- a mixin method is static on a class with a native `__callStatic`
  (`method.staticCall` today);
- `mixin_exclude_classes` (`ReflectionSettings`) skips mixin types naming
  those classes;
- a trait's `@method`/`@property` tags reach its hosts (`method.notFound` and
  `property.notFound` today);
- a `@method` tag is not an override prototype and has no native types;
- `@property-read int $r` and `@property-write string $r` on one class are
  one property readable as `int` and writable as `string`: `$m->r = 'x'` is
  `assign.propertyReadOnly` and `assign.propertyType` today;
- enums read `@method` and `@mixin`;
- `@phpstan-require-extends Base` on an interface makes `Base`'s members
  visible through the interface (`$i->x` for an `I $i` is
  `property.notFound` today, nsrt `bug-10302-interface-extends.php`) and
  makes the interface a subtype of `Base` (`@throws I` for an interface
  requiring `\Exception` is `throws.notThrowable` today,
  `throws-with-require.php`).

The class-reflection data extensions take `registry::access_attributes`
(ext-language-extensions) and `registry::class_info::doctrine_entity`
(ext-doctrine), with their module-graph edges re-dated to C8.
`access_attributes` and `compute_attribute_targets` read
`ReflectionAttribute`s. Attribute arguments evaluate to types.

**C9. Summaries.** Purity and global-variable inference leave reflection for
the summaries API; C9 creates `AnalysisContext` in the package's future
`analyser` module to hold them per snapshot, and D6 completes it. The purity
table C4 moved out of `FunctionReflection` moves to `AnalysisContext`. Stubs
get no global-variable summaries. Builtin purity comes from
`functionMetadata`. ROADMAP mapping (§3.8): summaries H2, H3, U1–U4, U6, P1;
reflection enumeration A5, U5; collector rules (E1) H1, V1, V2, Phase 90;
neither A2–A4, A6, C1, P2, D1.

Fixes, each its own behaviour commit:

- global-variable sites include `foreach` bindings, destructuring,
  by-reference out arguments and appends (the registry test
  `a_bare_top_level_append_contributes_nothing` flips); expression typing
  keeps unkeyed array items, resolves call returns (templates, `static`,
  dynamic return types) and binds `self::`;
- a global's summary holds every value it can have when a function reads it:
  `null` too, since a function can run before the top-level assignment (`f();
  $g = 'foo'; f();` reads `null`, then `'foo'`), and `mixed` once a write the
  fold can't see may reach the name: a `$GLOBALS` write with a key that isn't
  constant, `extract()`, `parse_str()` without a result argument, a variable
  variable or `global ${…}` at file scope, or an include the resolver can't
  follow. `global $global;` after a file-scope `$global = "foo"` is `'foo'`
  today (nsrt `get-defined-vars.php:20`, where PHPStan has `mixed`), and
  `$GLOBALS['g'] = 42` elsewhere doesn't widen it;
- purity: shell-exec and `throw` expressions are impure, a class-constant
  read is pure, calls and `new` resolve through parse-time names, and method
  dependencies follow inheritance. A static call's purity is its method's:
  `PC::p()` on a `@phpstan-pure` method, `$class::pure()` on a
  `class-string`, `E::from('a')` and `$d->format('Y')` on a
  `DateTimeImmutable` are "Possibly impure call to unknown method" today, as
  are calls through a `pure-callable`/`pure-Closure` parameter (`$cb()`) and
  a user function taking a pure callback; the rules oracle's 79
  `possiblyImpure.*` phpstan-rs-only findings are these and the built-ins
  `functionMetadata` decides (`is_string`, `array_key_exists`). A plain call
  to a user function is judged by its body too: `pureFunction();` is silent
  and `impureFunction();` `impure.functionCall` where both are "unknown
  function" today (`pure-function.php:62`-`:64`), and a static method
  calling an impure function is impure, not `staticMethod.resultUnused`
  (`call-to-static-method-without-impure-points.php`).

**C10. Extract `reflection`.** No module-graph edge remains; `cargo machete`
checks Appendix E's rows for it.

### Stage D: engine (in the package, module-gated)

The reference is PHPStan 2.2.16's engine (`vendor/phpstan-src/src/Analyser`:
`NodeScopeResolver`, `MutatingScope`, `ScopeOps`, `StmtHandler/*`,
`ExprHandler/*`, `TypeSpecifier`). Structure lands first and semantics
last, so every behaviour commit is written once, on the final structure.
D1–D4 are structural: each structural commit is byte-identical, and the
behaviour commits a phase lists follow it as approved deltas. D5 is the
semantics phase. Every behaviour bug below is reproduced and gets its
fixture before its phase's structural commit.

Gate 12 runs both oracles below. The oracle is real PHPStan 2.2.16 over the
corpus files with engine-sensitive diagnostics: the phar copied outside
`vendor/` (so the corpus's extensions don't load), Piwigo's
`vendor/autoload.php` as the only bootstrap file, level 10, run from a directory
with no `vendor/`, with `phpVersion` set to the range phpstan-rs reads from the
corpus's `composer.json` (`min: 80500, max: 80599`; PHPStan's own reading of
`^8.5` runs to 80699, B8). Without it the phar uses its runtime version, and at
least 63 of phpstan-rs's constant comparisons (`PHP_VERSION_ID < 70400` always
false, `symfony/polyfill-*`) and the `&&` results built on them show as
phpstan-rs-only. phpstan-rs runs at `--level 10` too: with the level unset it
skips the level-10 implicit-`mixed` checks (§1's level row), which turned 2,519
`offsetAccess.nonOffsetAccessible` into apparent misses. Both sides are filtered
to the engine identifiers. phpstan-rs gets the files by name from the corpus
root, so until B8's root-autoload follow-up lands, a project class under `src/`
that isn't named is unknown to it. Files that raise internal errors (missing
optional classes, php-standard-library function redeclaration) are excluded,
which leaves 941 files. PHPStan reports an error found in a trait as
`File.php (in context of class C)`, which the comparison strips. At
`76e738b4` phpstan-rs has 1,620 findings PHPStan doesn't, 1,056 of them
`offsetAccess.nonOffsetAccessible` (1,010 on `mixed`), and misses 2,039,
among them 741 `offsetAccess.nonOffsetAccessible`, 284
`foreach.nonIterable`, 263 `offsetAccess.notFound`, 244
`function.alreadyNarrowedType` (188 of those are `assert()` of `true`, which
phpstan-rs reports as `assert.alreadyNarrowedType`), 115
`booleanAnd.alwaysFalse` and 63 `deadCode.unreachable`. The counts that cite
snapshot 0a8fe69 below come from the earlier run with the level and the PHP
version unpinned. Each phase reruns the comparison, and every phpstan-rs-only
finding it names is a fixture.

The second oracle is PHPStan's own inference suite,
`vendor/phpstan-src/tests/PHPStan/Analyser/nsrt` (1,688 files, 18,480
`assertType` and 313 `assertNativeType` calls), checked by
`check-project --php-version 8.5` with `tests/PHPStan` as a scan directory (the
fixtures' classes live in `tests/PHPStan/Analyser/data`, which PHPStan loads
through its `autoload-dev` classmap) and phpstan-src's composer dependencies
installed (`nikic/php-parser`'s node classes). A file whose `// lint` header
excludes 8.5 runs at the highest version it allows (60 files, 47 of them `< 8.0`
at 7.4). Mismatches are compared after a normalization of display forms that
carry the same information: union member order, benevolent parentheses, shape
key order, `array<(int|string), T>` as `array<T>`, a list shape's explicit keys
(`array{0: 'a'}` is `array{'a'}`), `true|false` as `bool`, a template's scope
suffix (`T (function f(), argument)`), an accessory string's combined name
(`non-falsy-lowercase-string` is `lowercase-string&non-falsy-string`), a small
integer range against its literals (`int<0, 1>` is `0|1`), PHPStan's implicit
`*NEVER*` against `never`, and backslashes in a string literal (`'A\\B'` is
`'A\B'`). The files that assert an unpinned version range
(`predefined-constants.php`, `bug-4434.php`, and
`node-callback-scope.php:482`'s `PHP_VERSION_ID`) are the pinned version's
approved deviations. phpstan-rs misses 11,106 at `76e738b4`. By asserted expression:
4,788 are variables, 3,399 are calls to functions PHPStan types through an
extension (E2; 413 of them `mixed`, among them `filter_var` 191 and `min`/`max`
198), 1,016 are operators (D3), 707 are method calls, 347 other function calls,
300 properties, 179 offsets and 35 `new`. Two classes are flagged as possible
wrong values: 100 where every member of both types is a literal and an expected
literal is missing from phpstan-rs's type, and 65 where phpstan-rs has literals
and PHPStan the general type covering them. Each flagged line is either a wrong
value, fixed first in its owning phase and listed there, or phpstan-rs being
right at runtime (after `assert(($a[1] ?? null) === null)`, `$a[1]` is `null`,
where PHPStan keeps `object|null`), an approved deviation. The count is a
tracked number: each phase records it, and a phase that raises it has a named
reason in its commit. The suite's other two assertions, `assertNativeType` (313
calls) and `assertVariableCertainty` (681), are ignored today; the assertion DSL
reads both before D1, so D5's natives and certainty are measured by the same
suite. Until then, `variable.undefined` approximates certainty: of the 327 `Yes`
assertions phpstan-rs reports 97 as undefined (17 in
`dependent-expression-certainty.php`, about 40 in `throw-points/*`: correlated
conditions, `finally` scopes and possibly-impure calls, D5), and of the 351
`No`/`Maybe` it leaves 112 unreported. `cargo xtask nsrt` and
`cargo xtask oracle` land with it, and their first recorded counts are D1's
baseline.

The mismatches span stages: `filterVar.php` (156 `mixed`),
`set-type-type-specifying.php` (149) and the `minmax-*` files (140) are E2's
built-in extensions, and the `methodPhpDocs-*` files are C's PHPDoc inheritance.
The first `cargo xtask nsrt` run therefore checks in `gates/nsrt/owners.toml`,
which names the owning phase (B3, B4, C1–C10, D1–D6, E2) for each of the 1,360
mismatching files, split by line where a file spans phases. Appendix G
(`CRATE_SPLIT_CAUSES.md`, as are H-J) is its draft for the extension calls and
Appendix H for the 1,066 files the phases don't cite, by cause; Appendix J sorts
the cited files' lines their citing items don't describe. A phase is done when
its files have no mismatches, or each remaining line is an approved deviation
that names its runtime reason. `cargo xtask nsrt` reports by owner and lists the
wrong values first, and the corpus oracle's phpstan-rs-only identifiers get
owners the same way.

**D1. Holder storage (byte-identical).**

- Every `Locals` map moves into holder storage with its current per-kind
  merge and invalidation policy, `guarded_classes` and
  `clone_reinitialized` included.
- ExprKey is interned; the maps are persistent.
- Benchmarked on the merge/clone hot path.
  `merge_scopes`'s doc comment and `#[hotpath::measure]` sit on
  `add_implied_fact` (`control_flow.rs:394-412`), so `merge_scopes` is
  unmeasured; they move onto it before the baseline is recorded.

**D2. Statement results, contexts and StmtHandlers.**

- The annotation kinds the handlers need are added to `syntax::annotations`
  first, following 2.2.16's parser visitors:
  - `GotoLabelVisitor`, `LastConditionVisitor`, `TryCatchTypeVisitor`;
  - `ParentStmtTypesVisitor`, `DeclarePositionVisitor`;
  - `RemoveUnusedCodeByPhpVersionIdVisitor`;
  - the closure-argument context (`ClosureArgVisitor`,
    `ArrayMapArgVisitor`, `ArrayFilterArgVisitor`, `ArrayFindArgVisitor`,
    `ArrayWalkArgVisitor`, `CurlSetOptArgVisitor`,
    `CurlSetOptArrayArgVisitor`, `ImplodeArgVisitor`,
    `ArrowFunctionArgVisitor`, `ClosureBindArgVisitor`,
    `ClosureBindToVarVisitor`), which D3's intrinsic callback overrides
    read;
  - `PipeTransformerVisitor` (`$x |> f(...)` as `f($x)`, keeping the pipe
    for messages) and `MagicConstantParamDefaultVisitor`;
  - `ImmediatelyInvokedClosureVisitor`, `NewAssignedToPropertyVisitor`,
    `StandaloneThrowExprVisitor`.

  The handlers read them, and the `Checker` fields they replace
  (`current_catch_types`, `current_function_goto_targets`, the
  first-level and last-condition flags) are deleted.
- `StatementResult`/`ExpressionResult` carry exit points by statement kind
  and loop level, throw points, impure points, `hasYield`, end statements,
  and `isAlwaysTerminating` computed from them.
- One `StmtHandler` per statement kind splits `stmt.rs`, following
  2.2.16's `StmtHandler/*`.
- Context passing replaces the take-idiom flags:
  - `ExpressionContext` holds deep, in-throw and the assignment's
    right-side variable and expression.
  - `StatementContext` holds top level and the foreach unroll factor.
- A statement's `/** @throws */` overrides its throw points.
- A branch whose condition is always false isn't analysed: a `foreach`
  inside `if (!empty(X))` for `const X = []` is `foreach.emptyArray` today
  (the rules oracle's `bug-7913.php`).
- The statement after a terminating one gets the unreachable-statement
  node. Functions, classes and labels after it are still processed, and
  an empty statement (`;`) or a closing inline-HTML tag isn't reported
  (`deadCode.unreachable` today, the rules oracle's `unreachable.php:50`,
  `bug-7531.php`). A file's top level is checked too (`exit(0); echo 1;`
  is unreported today; `bug-11179-no-namespace.php`, `bug-8966.php`,
  `unreachable-top-level.php`), so is the statement after a `goto`
  (`goto-undefined-label*.php`, `unused-label*.php`, `bug-11731.php`, 7),
  and an expression terminates when any part it evaluates does: `echo
  nr();`, `'a' . nr()`, `[nr()]`, `f(nr())`, `Foo::s(nr())->t()`, a
  dynamic name `$foo->{exit()}()`, `(exit())->m(...)` and `$t ??= throw`
  on an always-`null` `$t` end the statement, where only a statement
  `nr();` does today (`bug-13232*.php`, `bug-14328.php`, `bug-11909.php`,
  `bug-14369.php`, `dynamic-name-always-terminating.php`,
  `first-class-callable-terminating-var.php`; about 20 of the rules
  oracle's 54 PHPStan-only `deadCode.unreachable`). A loop condition
  after an always-terminating body
  isn't evaluated (`do { exit; } while (true);` is `doWhile.alwaysTrue`
  today, `do-while-loop.php:145`).
- Each end statement gets its own `ExecutionEndNode`.
- Per-function mutable fields are deleted.
- Behaviour commits, each with its fixture:
  - **Termination** comes from the exit points (fixed first).
    `always_terminates`/`exits_function_or_loop` are deleted.
    - A loop always iterates when its condition's type is `true`, and it
      terminates when no `break` targets it (`WhileHandler`,
      `DoWhileHandler`, `ForHandler`).
    - A switch without `default` terminates when its cases leave the
      subject `never` (`SwitchHandler`).
    - A `finally` that always returns terminates its `try`, and so does a
      `new` whose constructor is `@return never`: `try {} finally { return
      'foo'; }` under `: string` and `new self();` under `: bool` are
      `return.missing` today (the rules oracle's
      `overwritten-exit-point.php`, `missing-return.php:206`,
      `bug-4455-constructor.php`).
    - Reproduced `return.missing` today: a `break` in a nested loop inside
      `while (true)`; `while (FOREVER)` with a `true` constant;
      `do { … } while (true)`; and a switch covering every case of an enum
      with `return`s and no `default`, or over every value of a constant
      or PHPDoc union, where a variable each case assigns is also
      `variable.undefined` after it (Appendix I's `switch-exhaustive`, 13
      findings in 7 files: `bug-12722.php`, `bug-8719.php`,
      `bug-1016b.php`, `defined-variables-enum.php`).
    - The oracle reports 28 of snapshot 0a8fe69's 68 checked
      `return.missing`; the other 40 are these shapes, among them
      `while (\true)` with a qualified constant (four in php-cs-fixer's
      `Tokens.php`), `while (true)` whose inner loop breaks
      (`symfony/mailer/Transport.php:121`), `do { … } while (true)`
      (`amphp/socket/RetrySocketConnector.php:37`), an `if` chain whose
      last condition is always true
      (`doctrine/migrations/InlineParameterFormatter.php:69`) and `goto`
      (`symfony/cache/PhpArrayAdapter.php:82`).
    - The interim fix before D2: `always_terminates` gains a `DoWhile` arm;
      `while`/`for`/`do` read the condition's inferred type instead of
      `is_truthy_literal`, which misses `\true`; a `break` counts only when its
      level reaches the loop; and `Switch` without `default` counts when the
      subject's enum cases are all covered.
  - **Last conditions** follow `LastConditionVisitor`: besides an `elseif`
    with no `else`, an `elseif` whose `else` only throws, an `if` without
    `else` followed by its block's final `throw`, and a `match`'s last arm
    are last conditions, whose always-true result isn't reported. The oracle
    finds 30 `function.alreadyNarrowedType`/`instanceof.alwaysTrue` in
    snapshot 0a8fe69 with the `if … throw` shape, e.g.
    `nikic/php-parser/BuilderFactory.php:369`.
  - **Constant conditions prune branches** (`IfHandler`): an `if` whose
    condition type is `true` skips the `else`/`elseif` merge, and a `false`
    condition drops its branch. An always-true `elseif` ends the chain the same
    way: `symfony/cache/Traits/RedisTrait.php:631` reports `$prefix` undefined
    after `if (…) {…} elseif (!$host instanceof Predis) {…}` where an earlier
    `continue` already excluded Predis. The condition's type is the PHPDoc type,
    or the native type when `treatPhpDocTypesAsCertain` is off. After
    `$flag = true; if ($flag) { $x = 1; }`, `echo $x` is `variable.undefined`
    today. A ternary's and a `&&`/`||` operand's dead side is pruned the
    same way: in `isset($array['string']) ? $array['string'] : 0` on
    `array{1, 2, 3}`, the read is `offsetAccess.notFound` today (the rules
    oracle's `isset.php`). The code after an `if` that returns under an
    always-true condition is unreachable, `if (true) { return; }`
    included (about 25 of the rules oracle's PHPStan-only
    `deadCode.unreachable`: `bug-4505.php`, `bug-3632.php`,
    `bug-11903.php`, `strict-comparison-71.php`, `true-typehint.php`,
    `multiple_unreachable*.php`), and so is the code after `do { … }
    while (true);` without a `break` (`call-methods.php:1028`); a `do`
    loop's `while (false)` is `doWhile.alwaysFalse` even when a `foreach`
    inside breaks (the `break` leaves the `foreach`;
    `do-while-loop.php:115`), and `while ($object = null)` is falsy
    (`Functions/data/do-while-loop.php`).
  - **A nameless `@var`** above `throw` applies to the thrown expression as
    it does above `return`: `/** @var \Exception */ throw $foo;` on a
    `stdClass $foo` is `throw.notThrowable` today (the rules oracle's
    `throw-values.php`); above `foreach` it types the iteratee, so `/**
    @var int */ foreach ($list as $val)` is `foreach.nonIterable` beside
    `varTag.noVariable`, unreported today (`wrong-variable-name-var.php`,
    2).

**D3. One inference path.** ExprHandlers, per-`NodeId` results, `infer_expr`
deleted; the file-size gate.

- An ExprHandler has three parts, as in 2.2.16: processing, type
  resolution, and its `specifyTypes`. The TypeSpecifier is built here, as
  the dispatcher over the handlers' `specifyTypes`; D6 only makes it
  public and adds its extension hooks.
  - `SpecifiedTypes` carries:
    - sure, sure-not and alternative types;
    - overwrite and equality;
    - the root expression and new conditional holders;
    - `&&`/`||` holder recipes and deferred augments
      (`ConditionalExpressionHolderRecipe`,
      `DisjunctionBranchUnionAugment`,
      `DisjunctionHolderProjectionAugment`), evaluated on the scope they
      are applied to.
  - The recipes replace `branch_implied_facts` and
    `pending_or_disjunctions`. Appendix H's `conditional-holders` (57 nsrt
    mismatches, 28 files) is what they add: a bool holding a check narrows
    its source when tested (`bug-14545.php`, `bug-2980.php`), a variable
    defined under a condition is certain and typed when the condition holds
    again (`bug-10090.php`, `bug-2816.php`, `bug-7948.php`), and variables
    holding `instanceof` results narrow in `||`
    (`disjunction-holder-projection.php`, `bug-9519.php`). A holder is
    never applied outside its condition: `bug-14891.php` pins a holder
    that would narrow `$a` to `''` where `$a` is still `string`.
- Assignment is two-phase (`PreparedAssignTarget`/`AssignTargetWalkMode`):
  the target's sub-expressions in PHP's evaluation order, then the value,
  then the write.
- isset/`empty`/`??` resolve through one issetability descriptor, which the
  engine and the isset rules share. On a constant array, `empty($a['nope'])`
  and `empty($a[3])` for a falsy value are `true` (`bool` today; nsrt
  `isset-coalesce-empty-type.php`), and `isset($this->p)` on an
  initialized non-nullable property is `true` (a typed property that may
  be uninitialized isn't: `isset($this->i)` on `int $i` without a default,
  or in a trait whose hosts differ, is `isset.property` "not nullable"
  today, the rules oracle's `bug-8668.php`, `bug-14416.php`,
  `isset-in-trait.php`), so `isset($this->r) ||
  isset($this->i)` is `booleanOr.alwaysTrue` in PHPStan and `bool` today;
  `$a?->b ?? $c` is `nullsafe.neverNull` ("unnecessary on left side of
  ??"), and so are `isset($a?->b)` and `empty($a?->b)` ("unnecessary in
  isset()/empty()"), nested chains included (`bug-7109.php`, 13;
  `bug-7291.php`, `isset-nullsafe.php`, `bug-12574c.php`); a dynamic
  name on a non-nullable receiver is "on non-nullable type" as a plain
  name is (`$this?->{'n'}`, `$this?->{$stringable}`;
  `dynamic-stringable-nullsafe-access.php`, 11); only `$d?->n` on a
  non-nullable `$d` is reported today. The isset rules read the same
  certainty: `isset($bar)`, `empty($foo)` and `$baz ?? 1` on a variable
  that is never defined are `isset.variable`/`empty.variable`/
  `nullCoalesce.variable` "is never defined", unreported today.
  `nullCoalesce.property`, and `isset.property` alike (14), cover static
  properties, always-`null` ones and a virtual hooked property
  (`FooCoalesce::$staticString ?? ''`, `self::$date ?? …`,
  `$foo->alwaysNull ?? ''` for `@var null`, `isset($this->noon)` for a
  get-only `DateTimeImmutable`; `null-coalesce*.php`, `isset*.php`,
  `bug-3636.php`, `bug-14393.php`), unreported today; an untyped
  property without a default starts as `null` whatever its `@var`, so
  `??` and `isset()` on it stay unreported where PHPStan reports "not
  nullable" (approved; `bug-3636.php:13`, `bug-14393.php:36`).
  `nullCoalesce.expr` covers any other left side: `rand() ?? 0`, `(new
  Foo()) ?? 'foo'` and `array_shift()` of a non-empty list are "not
  nullable", `null ?? 'foo'` "always null" (the rules oracle's 8,
  `null-coalesce.php`, `bug-13438*.php`); and `nullCoalesce.initializedProperty`
  a non-nullable property the constructor always assigns ("not nullable
  nor uninitialized"), unless custom serialization (`__unserialize()`,
  `__wakeup()`) may skip the constructor (the rules oracle's 6,
  `property-initialization-custom-serialization.php`,
  `isset-after-remembered-constructor.php`); `isset()` and `empty()`
  alike (`isset.expr`, `empty.expr`, `isset.initializedProperty`,
  `empty.initializedProperty`, and `empty.property` for a property that
  can't be falsy, `int<1, max>`; `empty(X)` of `const X = []` is
  "always falsy"; 11 in all); all unreported today.
  `nullCoalesce.offset` covers a literal or `null` container (`[0][0] ??
  'ok'` always exists, `(null)[0] ?? 'ok'` doesn't; `offset-access-legal.php`),
  unreported today. The
  descriptor covers a nullsafe chain: `$f?->bar() ?? 0` and
  `empty($i?->bar())` on an undefined `$f`/`$i` are `variable.undefined`
  and `method.nonObject` today (the rules oracle's `bug-7291.php`), and
  `isset($u::$p['x'])` on an undefined `$u` is
  (`variable-certainty-isset.php`). A falsey
  `isset($v)` on a defined variable narrows it to `null`: after `if
  (isset($v)) { return; }` or inside `if (!isset($v))` on
  `int|string|float|null` (or `?int`), `$v` keeps its type today, so
  `onlyNull($v)` is `argument.type` (the rules oracle's `bug-9580.php`);
  `$v !== null` already narrows; `isset($m->foo)` on `mixed` gives
  `object&hasProperty(foo)` (`echo $m->foo` after it is
  `property.nonObject` "on mixed" today, `access-properties.php:437`,
  `bug-13539.php`) and on an object of a class without that property
  `C&hasProperty(foo)` (inside `if (isset($foo->baz))` and `while
  (isset($this->foo))`, the read is `property.notFound` today,
  `access-properties.php:47`, `:286`), and `isset($o->foo)` on `self|false`
  leaves
  `self` (`echo $o->foo` after it is `property.nonObject` "non-object
  type …|false" today, the rules oracle's `access-properties.php:320`).
  `isset($t['foo'])` with a non-numeric
  key drops a `string` member (a string has only integer offsets): on
  `string|array`
  it leaves `string|array&hasOffsetValue('foo', mixed)` today, so the
  value returned under `: array` is `return.type` (the rules oracle's
  `bug-1714.php`).
- AST names read their parse-time resolution; `name_ctx` is deleted.
- **Disagreements.** `check_expr` and `infer_expr` compute the same
  expression twice. Before the merge, a commit instruments both paths to
  record every node where they disagree, over the fixtures and the
  Piwigo corpus. The inventory is checked in. The merge then lands as one
  approved delta per class of disagreement, each with a fixture. Known
  before it: `infer_expr` has no arm for `instanceof`, a compound
  assignment, a dynamic variable, property fetch or method call, or
  `list()`, and gives `mixed` for a short ternary, so `return $o
  instanceof X` is `return.type` "got mixed", `match ($a instanceof B)`
  isn't exhaustive over `true`/`false`, and `$s ?: ''` on `string|false`
  is `mixed` (Appendix I's `infer-expr-gaps`: `bug-12827-instanceof.php`,
  `bug-10214.php`, `bug-6910.php`, `bug-4188.php`, `bug-5164.php`,
  `bug-9991.php`, `bug-5382.php`).
- Behaviour commits, each with its fixture, soundness first (a wrong
  type is worse than an imprecise one), then by handler family:
  - **Soundness.** Each is a wrong value, not a lost precision:
    - `is_resource()` on a `resource` is never always true, because a
      resource may have been closed (fixed first). Snapshot 0a8fe69 has 52
      `function.alreadyNarrowedType` for it that the oracle doesn't report,
      e.g. `sebastian/environment/src/Console.php` and
      `maxmind-db/reader/src/MaxMind/Db/Reader.php`;
    - an assertion intersects with the current type and its negation
      narrows the other branch (`specifyTypesFromAsserts`): for
      `@phpstan-assert-if-true int $v`, the true branch on a `string $s`
      is `int` today where PHPStan has `*NEVER*` (the assertion replaces
      the type), and the false branch on `int|string` stays `int|string`
      instead of `string` (nsrt `assert-docblock.php`); an assertion on a
      variadic parameter applies to every argument it collects: after
      `if (!check($a, $b)) { return; }` for `@phpstan-assert-if-false null
      $values` on `check(...$values)`, `$a` stays `?string` today (the
      rules oracle's `bug-8389.php`); an equality assertion
      (`@phpstan-assert-if-true =int $x`) narrows the true branch only, and
      today it narrows neither (`docblock-assert-equality.php`). Appendix
      H's `assert-tags` (58 nsrt mismatches, 19 files) are the call forms
      that don't narrow today: a method call's `-if-true` on its argument
      (`bug-14705.php`), a first-class or string callable of an asserting
      function (`bug-14249.php`, `bug-11472.php`), a union receiver
      (`bug-14108.php`), constructors, and conditional, template and
      `empty` assertion types;
    - unary minus on a literal union is ignored: `-$c` for `1|2` is `1|2`;
    - a float result is typed as an integer literal: `(float) 5` is `5`
      and `abs(0.0)` is `0`;
    - `===` treats a float literal as the equal integer: on
      `@param 0|0.0|1.5 $x`, `if ($x === 0) { return; }` leaves `1.5`, so
      the `$x === 0.0` branch that follows is typed `1.5`;
    - loose comparison is evaluated with the wrong table: `'0' == null` is
      `true` and reported `equal.alwaysTrue`, where PHP 8 gives `false`
      (nsrt `loose-comparisons.php:325`, `:460`; the table's other 192
      verdicts in the suite agree). The table follows the analysed PHP
      version: before 8.0, `0 == 'php'` is `true`, and it is `bool` at 7.4
      today (nsrt `loose-comparisons-php7.php`). Beyond two constants,
      `==` is decided by each type's `looseCompare`: `int<10, 20>` against
      `3`, `null`, `false`, `' 3'` or `'13foo'`, a `non-falsy-string`
      against `''`, `[]` or `false|null`, `0|1|false` against `2`, an enum
      against another enum's case, and `array == int` are
      `equal.alwaysFalse`, and `true == int<10, 20>` and a case against
      itself `equal.alwaysTrue` (3), unreported today (the rules oracle's 25
      PHPStan-only: `bug-11694.php`, 14, `bug-14606.php`, `bug-8485.php`,
      `bug-8800.php`, `invalid-comparison.php`);
    - a string offset write doesn't change the string: `$s[6] = 'b'` on
      `'foo   '` leaves `'foo'`;
    - late static binding of a constant in an inherited method:
      `$bar->test()` returning `static::FOO_BAR` gives the parent's
      `'foo'` instead of the subclass's `'bar'` (nsrt `bug-13828.php`).
      Inside the class, `static::`/`$this::` of a constant that isn't
      final gives its value where a subclass may override it: `static::X`
      for `const X = 1` is `1` where PHPStan has `mixed`, and for
      `/** @var string */ const T = 'bar'` is `'bar'` where it has
      `string`; `self::T` is `'foo'`, the value, and it is `string` today.
      A native-typed `const int X = 1` gives `int` through `static::`
      (`ClassConstFetchHandler`; nsrt `class-constant-types.php`,
      `class-constant-native-type.php`);
    - narrowing to a value the type excludes gives the value instead of
      `never`: after a `match ($i) { 0 => … }` caught in a `try` on
      `1|2|3|4`, `$i` is `0|1|2|3|4` (nsrt `match-expr.php:19`), and
      `$i === 2` on `int<3, 5>` gives `2`; a type guard does the same, so
      the branch of an always-false guard types its subject as the guard's
      type: inside `if (is_string($i))` on `int`, `if ($i instanceof
      stdClass)` and `if (is_resource($c))` on `CurlHandle|false`, the
      subject is `string`, `stdClass` and `resource` (then `curl_exec($c)`
      is `argument.type`, the rules oracle's `bug-15139.php`);
    - a nested write through a non-constant key is dropped: after
      `$array = ['foo' => [], 'bar' => []]; $array[$name][] = $v;`, `$array`
      is unchanged, so a later `count($elements) > 0` over it is reported
      always false (`AssignHandler`; nsrt `bug-5316.php`);
    - a property write narrows the property only where
      `canChangeTypeAfterAssignment` holds (readable type equals writable
      type, no hook, not virtual, not a `SimpleXMLElement` element): after
      `$xml->foo = 'foo'` on a `SimpleXMLElement`, `$xml->foo` is `'foo'`
      today, so passing it to a `SimpleXMLElement` parameter is
      `argument.type` (the rules oracle's `properties-assigned-types.php`,
      5), and a `@property-read int`/`@property-write string` pair reads
      back the written `'x'`. A generic object of the property's own
      class takes the declared arguments: after `$this->foo = new Foo()` on
      `@var Foo<stdClass>`, `$this->foo` is `Foo` today where it is
      `Foo<stdClass>`, and `$test->name = Option::fromValue('test')` on
      `Option<string|null>` reads back `Option<'test'>` (the rules
      oracle's `bug-3777.php`, `bug-9096.php`); a template `new` can't
      infer takes its bound, spelled out: `new Lorem2(new stdClass)` is
      `Lorem2<stdClass>` today where it is `Lorem2<stdClass, object>`;
    - a by-reference argument to a closure or a variable-function call keeps
      its value: after `$cb = function (bool &$save) {…}; $save = true;
      $cb($save);`, and after `$f = 'h'; $f($s);` for `h(bool &$x)`, the
      variable is still `true` and `$save === false` is reported
      `identical.alwaysFalse` (`processArgs`; nsrt `bug-5615.php`). An
      offset passed by reference keeps its value too: after
      `test($array['key'])` for `test(array &$a)`, `$array['key']` is still
      `array{}` (nsrt `bug-5077.php`; a plain variable argument gets the
      parameter's type);
    - a by-reference argument is checked with its value before the call:
      `predefine_byref_or_unknown_args` widens the variable to `mixed` (or
      an array-like parameter type) first, so at level 10 `$e = '';
      r($e)` for `r(string &$x)` is `argument.type` "mixed given", as is
      every defined variable passed to a non-array by-reference parameter
      (`openssl_public_encrypt($d, $encrypted, …)`, `pam_auth`, `getmxrr`
      in the rules oracle's `bug-4004.php`, `bug-10499.php`,
      `bug-10704.php`), and an undefined variable passed to an out
      parameter is checked as `mixed` before the call defines it:
      `preg_match($p, $s, $m)` is `argument.type` "expects
      null|array<(int|string), string>, mixed given" (Appendix I's
      `byref-out-undefined`, 60 findings in 19 files, and
      `byref-arg-type`); the widening moves after the check and after the
      call's return type, which reads the widened argument today where the
      assigned value is a separate path: `$this->p = array_pop($objs)` on a
      `non-empty-list<O>` is `O|null` and `assign.propertyType` (the rules
      oracle's `bug-7844b.php`), while a bare `array_pop($objs)` is `O`;
    - an argument to a by-reference variadic parameter is read with its
      type: `$b = 2; $o->bar(1, $b)` for `bar(int $a, int &...$rest)` is
      `argument.type` "mixed given" today (the rules oracle's
      `bug-15251.php`);
    - `||` narrowing on a union that isn't normalized loses members: on
      `@param Node|Return_ $n` with `Return_ implements Node`, falling past
      `if (!$n instanceof Expression && !$n instanceof Return_) { return; }`
      leaves `Return_` where it is `Expression|Return_`, so a later
      `$n instanceof Expression` is `instanceof.alwaysFalse`
      (`rector/rules/DeadCode/Rector/Property/RemoveUnusedPrivatePropertyRector.php:166`,
      where D5's closure re-entry produces the unnormalized union);
    - the falsey side of `A && B` narrows by `!A || (A && !B)`
      (`TypeSpecifier`'s `BooleanAnd` in a falsey context): after `if ($t
      && !is_array($t)) { return; }` on `null|int|int[]`, `$t` keeps the
      whole union today where it is `0|array<int>|null`, and after `if ($t
      !== null && $t > 5) { return; }` on `?int` it is `int|null` where it
      is `int<min, 5>|null` (the rules oracle's `bug-5749.php`);
    - `is_a($cls, A::class, true)` on a `string` narrows it to the object
      `A` instead of `class-string<A>`, so `strlen($cls)` in that branch is
      `argument.type` "A given" and the merge after it leaves `A|string`
      (`IsAFunctionTypeSpecifyingExtension`);
    - the wrong values inside the families below land here too: `%`
      giving `never` (Arithmetic) and a pre-increment's old value
      (Increments).
  - **Operators and literals:**
    - **Arithmetic** follows PHPStan's `InitializerExprTypeResolver` (fixed
      first): `int|float * 2` and `int|float + 1` are `int` today, so
      `is_float($n * 2)` is reported always false; integer overflow is a
      float. A float literal types as its value; `1.5` is `float` today.
      `%` on ranges follows PHP's sign rule: `int<0, 30> % -9` and
      `int<-30, 30> % -9` are `never` today where PHPStan has `int<0, 8>` and
      `int<-8, 8>`, and a range divisor gives `int` (nsrt `bug-15242.php`;
      with `%` of a range divisor, bitwise `|`/`^`/`&` on ranges and
      numeric-string arithmetic, Appendix H's `operator-ranges` is 87
      mismatches: `modulo-operator.php`, `bug-14654.php`, `bug-15243.php`,
      `bug-9384.php`, `mixed-to-number.php`).
      A shift covers every operand: `int<1, 3> << 63` is
      `-9223372036854775808` today, missing the `0` that `2 << 63` gives.
      `int ** int` is `int` today, where a negative exponent or an overflow
      gives a float (nsrt `pow.php`). A bitwise operator on two strings
      works bytewise and gives a string: `$s & $s` is `int` today (nsrt
      `bitwise.php`), and so does `mixed ^ string`, so after `$d ^= $mask`
      on an untyped `$d`, `base64_encode($d)` is `argument.type` "int
      given" (the rules oracle's `bug-8094.php`); a string with an int is
      `*ERROR*` (B4).
    - **`%` by a constant** is bounded by it: `$k % 4` on `int` is
      `int<-3, 3>` and on a non-negative `int<0, 3>`, `int` today, so a
      `match` over its values is `match.unhandled` (the rules oracle's
      `bug-8900.php`, `bug-9879.php`).
    - **Bitwise operators on ranges** stay in range: `$a | $b` on two
      `int<0, 1>` is `int<0, 1>`, `int` today (the rules oracle's
      `bug-7912.php`).
    - **`<=>`** is `-1|0|1`, `int` today, so it doesn't satisfy `@return
      -1|0|1` (the rules oracle's `bug-5008.php`). `+` with a `mixed`
      operand is `array|float|int`, `int` today (`bug-3759.php`).
    - **Concatenation** follows `InitializerExprTypeResolver::getConcatType`
      over every member of a union: `($a ?? 'x') . '-'` on `?int` is `'x-'`
      today, a wrong value that drops the `int` member (so `$key ===
      'x-'` reads as always true; the rules oracle's `bug-8620.php`), and
      `$c . '-'` on `1|'x'` is `non-falsy-string` where it is `'1-'|'x-'`.
    - **Increments** follow PHP: `$k = 'a'; $k++` leaves `'a'` today, and
      `$n = null; $n++` leaves `int` where PHP gives `1`. The value of a
      pre-increment is the new value: `$one = 1; ++$one` is `1` today, and
      `'foo'` pre-incremented stays `'foo'` where PHP gives `'fop'`
      (`''` → `'1'`, `'123'` → `124`; nsrt `pre-inc.php`, `pre-dec.php`,
      `binary.php`). An array literal's keys are evaluated in order, each
      increment seen by the next: `[$idx++ => 'a', $idx++ => 'b']` has keys
      `0` and `1`, and is `array.duplicateKey` "duplicate key 2" today (the
      rules oracle's `duplicate-keys.php:43`).
    - **`NAN`** is never identical or equal to itself: `NAN === NAN` is
      reported `identical.alwaysTrue` today, a wrong value (and `NAN !==
      NAN` `notIdentical.alwaysFalse`; the rules oracle's
      `strict-comparison.php:980`, `:983`).
    - **Identical** follows `resolveIdenticalType`: operand types that don't
      intersect make `===` constant false. `identical_result` tests type
      families only, so `1|2 === 7` isn't reported today, nor are `int<0,
      1>` against `100`, `'/'|'\\'` against `'//'`, `array{X: 1}` against
      `array{X: 2}`, a `non-empty-array` against `[]`, `lowercase-string`
      against `'AB'`, `uppercase-string` against `'ab'` or
      `class-string<stdClass>` against `'Exception'` (most of the rules
      oracle's 80 PHPStan-only `identical.alwaysFalse`, `strict-comparison.php`,
      `lowercase-string.php`, `uppercase-string.php`, `hashing.php`). The
      truthy branch intersects: after `if ($x === 3) {}` on `1|2`, `$x` is
      `3|1|2` today. The `!==` mirror of each is `notIdentical.alwaysTrue`
      (the rules oracle's 26 PHPStan-only, the same files plus
      `bug-7844b.php`'s `assert($objs !== [])` and `bug-5474.php`'s
      differing shapes). An enum's `$this` narrows like any enum value: after
      `if ($this === self::A) { return; }` it is still the whole enum today
      (`bug-9142.php`).
    - **Comparisons between constants fold**, including `null` and `bool`
      operands by PHP's comparison table: `1 < null` and `null < 1` are
      `bool` today where PHPStan has `false` and `true` (96 nsrt mismatches
      in `comparison-operators.php`).
    - **Integer ranges** narrow at their ends: `$i !== 3` on `int<3, 5>`
      leaves `int<3, 5>` today instead of `int<4, 5>` (nsrt
      `integer-range-types.php`, whose other mismatches are B4's range
      union) A comparison narrows each member of a union by PHP's
      comparison table: `$foo > 1` on `?int` leaves `int|null` today where
      it is `int<2, max>` (`null > 1` is false), `$foo >= 0` gives
      `int<0, max>|null`, and `$f > 1` on `?float` drops `null`; on a
      plain `int` it already narrows (the rules oracle's `bug-577.php`).
    - **Short ternary** follows `TernaryHandler`: `$a ?: $b` is `mixed` today
      for every left operand (`int`, `string`, `?array`, `bool`, objects).
    - **Compound assignments** have the assigned value as their type
      (`AssignOpHandler`): `$x = 1; $x += 1` as an expression, `$i /= 2` and
      `$n ??= 1` are all `mixed` today where PHPStan has `2`, `(float|int)`
      and `1|string` (nsrt `binary.php`, `coalesce-assign.php`), so
      `($this->lazy ??= new \DateTimeImmutable())->format('Y')` is
      `method.nonObject` "on mixed" today, a false positive.
    - **Pipes** follow `PipeHandler`: `$s |> strlen(...)` is `mixed` today.
      The right side is called with the left side as its argument, so an
      arrow function's parameter takes its type and the statement is a
      call: `5 |> (fn ($x) => $foo->m($x))` gives `$x` `mixed` (an
      `argument.type` at level 10) and is `expr.resultUnused` today (the
      rules oracle's `call-to-method-without-impure-points-pipe.php`). The
      left side is checked against the callable's first parameter: `'Hello'
      |> doBar(...)` and `$this->doFoo(1) |> $this->doBar(...)` for
      `doBar(int $i)` are unreported today (10 rules-oracle findings,
      `func-call-pipe.php`, `method-call-pipe.php`, `static-call-pipe.php`,
      `call-callable-pipe.php`, `pipe-operator.php`), and so is its arity:
      `1 |> doFoo(...)` for a two-parameter `doFoo()` and `5 |>
      $foo->canDiscard(...)` for one without parameters are
      `arguments.count` (15 findings, the same files and the
      `*-statement-result-discarded-php85.php` ones); a `#[\NoDiscard]`
      result dropped through a pipe (`5 |> $o->instanceMethod(...);`) is
      `method.resultDiscarded`/`staticMethod.resultDiscarded`/
      `function.resultDiscarded`, unreported today (6).
    - **Magic constants** type as their values: `__DIR__`, `__FILE__`,
      `__LINE__`, `__CLASS__`, `__FUNCTION__`, `__METHOD__`,
      `__NAMESPACE__`, `__TRAIT__` and `__PROPERTY__` are all `mixed` today
      (nsrt `node-callback-scope.php`, `property-hooks.php`), so
      `dirname(__DIR__)` is `argument.type` "mixed given" at level 10 (the
      rules oracle's `bug-13981.php`, `value-assigned-to-define.php`).
  - **Arrays and offsets:**
    - **Offset reads** follow `ArrayDimFetchHandler`: a constant array read
      with a non-literal key uses the key's type. Today `$t = [10, 20];
      $j = 1; $t[$j]` is `mixed` where PHPStan has `20`, `$t[$i]` for an
      `int` is `mixed` where it has `10|20`, and on `array{a: int, b: string}`
      both `$s[$v]` with `$v = 'a'` and `$s[$u]` with `'a'|'b'` are `mixed`
      (Appendix I's `const-offset-nonliteral`, 18 findings in 9 files:
      class constants read by a method's result, `array_rand()` keys,
      `'a_' . $d`).
      `array_key_first()` of that shape is `string` instead of `'a'`.
      String offsets are typed: `$s[$i]` on a `string` and even `'hello'[0]`
      are `mixed` today, where PHPStan has `string` and `'h'` (and the union
      of the characters for a non-literal key). An `ArrayAccess` object's
      offset reads `offsetGet`'s return type with the class's template
      arguments: `$a[0]` on `SplFixedArray<SplFixedArray<int>>`,
      `ArrayAccess<int, string>` or `ArrayObject<string, int>` is `mixed`
      today, so every nested read is a level-10
      `offsetAccess.nonOffsetAccessible` (37 in
      `bacon-qr-code/src/Encoder/MaskUtil.php` alone). A union container
      reads each member: `$list[$k]` on `array<string, C>|null` is `mixed`
      today where it is `C|null` (a `null` container reads `null`), so
      `$list[$k]->isHidden()` is `method.nonObject` "on mixed".
    - **Array literals and appends** follow `ConstantArrayType`'s next-key
      rule: after a negative key the next implicit key is the key + 1 on PHP
      8.3+ and `0` before, after `PHP_INT_MAX` there is none (the append
      fails), and appending to a shape keeps the shape. Today
      `[-5 => 'a', 'b']` is plain `array`, appending `'y'` to `array{3: 'x'}`
      gives `array<'y'|'x'>` instead of `array{3: 'x', 4: 'y'}`, and
      `[PHP_INT_MAX => 1, 2]` gains a key `0` (nsrt `bug-15248.php`). A
      literal with a non-constant key is a general array of its key and
      value types: `[$key => []]` for a `string $key` is `array` today
      where PHPStan has `non-empty-array<string, array{}>`, so
      `array_push($array[$key], $item)` reads `mixed` (the rules oracle's
      `bug-3387.php`).
    - **`$o::class`** and `get_class($o)` are the class name literal for an
      object of a final class (`'FA'|'FB'` on `FA|FB`, `string` today, so a
      `match` over the names is `match.unhandled`: the rules oracle's
      `bug-7698.php`, `bug-7746.php`, `bug-8536.php`, `bug-9534.php`,
      `match-expr.php:199`), and `get_debug_type()` likewise.
      `get_class()` without an argument is the declaring class's name
      (`string` today), and `get_class($this)` is `class-string<$this>`,
      which an unrelated class's name can't equal (`bug-3633.php`, 8).
      `$o::class` is `class-string<C>&literal-string` for an object of
      class `C`, `class-string<T>&literal-string` for a template `T`, and
      `class-string&literal-string` for `object`: the template and `object`
      cases are `string` today and the `literal-string` part is missing, so
      passing `$t::class` to a `literal-string` parameter is `argument.type`
      (the rules oracle's `bug-7823.php`).
    - **Key-producing assignments** follow `AssignHandler::specifyTypes`:
      after `$k = array_key_first($a)` (and `array_key_last`,
      `array_search`, `array_find_key`, `array_rand`), `$a[$k]` is known to
      exist with the element type; after `$i = array_search(5, $a, true)`
      and `$i !== false`, `$a[$i]` is `int` today where PHPStan has `5`.
      `$c = count($l) - 1` makes `$l[$c]` exist on a non-empty list.
  - **Calls, closures and objects:**
    - `$this`/`static` inside a class body carry their bound class
      (`Type::This`/`StaticRef` bound), displayed `$this(Base)`/
      `static(Base)`, and `@return $this` rejects a returned `new static()`
      or `new self()`, and `: static` a returned `new self()` (a
      `TypeError` when called on a subclass); 17 of the rules oracle's
      PHPStan-only `return.type` are these (`bug-10580.php`,
      `returnTypes.php`, `bug-3523.php`, `bug-14150-static.php`,
      `bug-5946.php`).
      In a generic class `$this` carries the class's own templates as its
      arguments (`D<T>`, where it is plain `D` today), so a property typed
      `T` reads as `T`: `return $this->value;` under `@return T` is
      `return.type` "expected T, got mixed" today, whether the property is
      promoted with `@param T` or declared with `@var T` (the rules
      oracle's 24 "expected T"/`TValue` and 3 "expected T|null" findings,
      most of them this shape).
    - **A class-string expression as a static receiver** resolves:
      `$class::FOO`, `$class::$foo` and `$class::s()` after `$class =
      self::class`, and `self::class::FOO`, are `mixed` today, so returning
      them under `: string` is `return.type` and the private members they
      reach are `classConstant.unused`/`property.onlyWritten`/
      `method.unused` (the rules oracle's `bug-14880*.php`).
    - **`static` in a return type** is the whole receiver type: `bat()`
      with `@return I2&static` called on `I1&Foo` is `I2&I1&Foo`, where
      today it is `I2&Foo` and a following `->someMethod()` from `I1` is
      `method.notFound` (the rules oracle's `bug-1661.php`).
    - **A template-typed receiver** reads members through its bound, and
      a `$this`/`static` return on it is the template: on `B of BA`,
      `$t->n()` and `$t->p` are `mixed` today where they are `BA`'s
      `string` and `int`, and `$t->where('x')` for `@return $this` is
      `mixed` where it is `B`, so `return $t->where('x')` under `@return
      B` is `return.type` (the rules oracle's `bug-11663.php`).
    - **A call on a union receiver** combines its members' methods
      (`UnionTypeMethodReflection`,
      `ParametersAcceptorSelector::combineAcceptors`): each parameter takes
      the union of the members'
      parameter types. `$u->apply($i)` on `Bar|Baz` with `apply(B)` and
      `apply(C)` checks against the first member only today, so `B|C` is
      `argument.type` (the rules oracle's `bug-6946.php`).
    - **Type-check calls** type as the constant the impossible-check logic
      decides: `is_string($s)` with a `string` is `true`, `is_string($i)` with
      an `int` is `false`, and `!`/`method_exists`/`instanceof` follow (an
      `instanceof` expression is `mixed` today, not even `bool`, so on
      `?Req $x`, `$x instanceof Req || $x === null` isn't
      `booleanOr.alwaysTrue`, one of the 11 the corpus oracle finds and
      phpstan-rs misses,
      `amphp/http-client/src/Interceptor/ModifyRequest.php:35`). A call with
      `@phpstan-assert*` tags is a type-check call too: PHPUnit's
      `assertNull($bool)` is `staticMethod.impossibleType` and
      `$a->assertInt($int)` `method.alreadyNarrowedType` (and a static
      one `staticMethod.alreadyNarrowedType`), unreported today (16
      `method.impossibleType` the corpus oracle finds in
      `php-openapi/openapi/tests/spec/SchemaTest.php`). Today the calls are
      `bool` although the rules report them, so `!is_int($c) && !is_string($c)`
      on `int|string` isn't `booleanAnd.alwaysFalse` (105 oracle findings
      phpstan-rs misses in snapshot 0a8fe69, e.g.
      `nyholm/psr7/src/Response.php:75`), and the `throw` after an exhausted
      `if` chain isn't `deadCode.unreachable` (57, e.g.
      `nikic/php-parser/BuilderFactory.php:373`).
    - **Enums**: `Foo::cases()` is the shape of its cases in declaration
      order, and `$e->name`/`$e->value` are the unions of the cases' names
      and values; today they are `list<Foo>`, `string` and `mixed`, and a
      pure enum's `->value` isn't `*ERROR*` (nsrt `enums.php`).
    - **First-class callables** carry the callee's signature: `strlen(...)`
      is a bare `Closure` today. A conditional return type in the signature
      resolves per call: `array_map(createA(...), [A::class, B::class])`
      for `@return ($class is class-string<T> ? T : mixed)` gives
      `array<($class is class-string<mixed> ? mixed : mixed)>` today (the
      rules oracle's `bug-11056.php`).
    - **Arguments** follow `processArgs`: each argument is processed on the
      scope the previous one left, and a closure argument's parameters are
      resolved from its sibling arguments whatever their order.
      `apply(function ($x) { … }, 1)` for `@param callable(T): void $cb` and
      `@param T $v` gives `$x` the unresolved `T` today where PHPStan has
      `1`. The intrinsic callback overrides (`array_map`, `array_filter`,
      `array_walk`, `array_find`, `curl_setopt`, `implode`, `Closure::bind`)
      rewrite the callback's parameters from the sibling arguments (with
      untyped closure parameters taken from a union of callables, a
      variadic, a constructor or an attribute, Appendix H's
      `closure-param-infer` is 77 nsrt mismatches in 13 files);
      `array_map(fn ($x) => $x * 2, [1, 2])` is `array<int>` today where
      PHPStan keeps `array{2, 4}`. A by-reference callback parameter is
      typed from the context too: `array_walk($l, function (&$item) {…})`
      over `list<string>` gives `$item` `mixed` today (a by-value one is
      `string`), so `trim($item)` is `argument.type` (the rules oracle's
      `bug-7469.php`), and the key parameter is `int` where it is
      `int<0, max>`. `array_filter`'s callback parameters follow its mode:
      under `ARRAY_FILTER_USE_KEY` and `ARRAY_FILTER_USE_BOTH` the key and
      value parameters are `mixed` today (`stripos($key, …)` is
      `argument.type`, the rules oracle's `bug-10903.php`), and
      `preg_replace_callback`'s callback gets the pattern's matches shape
      where it gets `array` today (`bug-14177.php`). `self` and `static` in
      the callable type resolve
      against the called method's class: for `@param callable(static):
      array $s` on `A::c()`, `$a->c(function ($who) { return []; })` gives
      `$who` `mixed` and is `argument.type` "expects callable(A): array,
      Closure(static): array{} given" today (the rules oracle's
      `bug-11591.php`, `bug-14893.php`).
    - **An inherited constructor** takes the child's ancestor bindings:
      `new Foo(['foo' => 1])` for a parent constructor's `@param T $d`
      under `@extends P<array{foo: int, bar: int}>` is unreported today
      (a method call already binds; the rules oracle's `bug-14138.php`).
    - **`static::C`** of a constant a subclass may override (a non-final
      constant of a non-final class) is its declared type (`mixed` when
      untyped), not its value: `static::KEY` is `'v'` today, so
      `sprintf('%s', static::KEY)` and `array_key_exists(static::KEY, $a)`
      pass where PHPStan checks `mixed` (`bug-6181.php`); `$this::C` and
      `$self::C` are the same fetch (`bug-6758.php`, 3). `self::C` isn't
      late-bound and reads the value: `@var int` on `IPSUM = self::LOREM`
      for `/** @var string|int */ LOREM = 1` is `classConstant.phpDocType`
      today (`value-assigned-to-class-constant.php:20`).
    - **A template's bound** is checked at the call: `g(new
      DateTimeImmutable())` for `@template T of DateTime @param T $a` is
      unreported today (`call-generic-function.php`).
    - **PHP 8.5's `clone($o, $withProperties)`** is checked as a function:
      `clone($foo, 1)` and `clone($mixed, [])` at level 10 are unreported
      today (`clone-function.php`, 4).
    - **A dynamic method name** that is a constant string resolves the
      method: `$name = 'doFoo'; $this->$name('x')` and `self::$name(…)`
      check the arguments against `doFoo()`, unreported today
      (`dynamic-call.php`, 5).
    - **`call_user_func()`/`call_user_func_array()`** check their arguments
      against the callable's parameters, named ones and array keys
      included (`call_user_func($f, 'foo')` for `function (int $i)` is
      unreported today; 8 rules-oracle findings, `call-user-func.php`,
      `call-user-func-array.php`), and count them: `call_user_func($f)`
      and `call_user_func_array($f, [1])` for a callable needing two are
      `arguments.count`, unreported today (10 findings).
    - **Every receiver form resolves the callee**: a static call through
      a `class-string<Foo>` variable or on an `A|B` receiver checks its
      arguments (`$cs::test(1, 2, 3)`, `$ab::sfoo()`;
      `call-static-methods.php`, `bug-15251.php`, 5); a spread of a union
      of shapes passes position by position (`...$args` on `array{'foo',
      1}|array{'foo', 1, 'bar'}` passes `'foo'` first and maybe only two
      arguments, `arguments.count`, where today the first is
      `'bar'|'foo'|1`, a false `argument.type`; `bug-3481.php`); and a
      parameter with a default before a required one is required
      (`function ($a, $b = 'b', $c)` needs three, `callables.php:157`); all
      unreported today.
    - **A variadic parameter** is `list<T>` in the body under
      `@no-named-arguments` and `array<int|string, T>` otherwise; it is
      `array<T>` today, so `$this->b = $b` into `list<B>` is
      `assign.propertyType` (the rules oracle's `bug-11275.php`).
    - **Unpacked constant arrays** bind their elements to the parameters
      they reach: `$c->access(...['view', null, true])` for `@return
      ($return_as_object is true ? AccessResultInterface : bool)` is
      `bool` today, so `->isAllowed()` on it is `method.nonObject` (the
      rules oracle's `bug-7369.php`; the literal-argument call already
      resolves).
    - **By-reference out types of built-in functions** apply to the
      argument: after `exec($c, $out)`, `parse_str($s, $ps)`,
      `preg_match_all($p, $s, $mm)` or `preg_match` with a non-constant
      pattern, the variable is `mixed` today where PHPStan has the
      signature's out type (`array<string>` for `$out`), and at level 9+
      the call itself is reported `argument.type` "mixed given" for the
      variable PHP creates (E3c). A user function's `@param-out` already
      applies; without one, the by-reference parameter's own type is the
      out type: after `byRefParam($v)` for `@param int $x` on `&$x`, `$v`
      is `mixed` today where it is `int` (`bug-15250.php`). 25 of the 26
      lines of `composer/semver`'s `VersionParser.php` with level-10
      `offsetAccess.nonOffsetAccessible` read such a
      `$matches` (41 findings, in each of its three copies). The array
      functions that modify their argument give it the new array
      (`array_push`, `array_unshift`, `array_pop`, `array_shift`,
      `array_splice`, `sort` and its family): after `array_push($k, [$a])`
      on `array<A[]>`, `$k` is `array` today, so `array_push($k[0], …)`
      is `argument.type` "mixed given" (the rules oracle's
      `bug-8430b.php`), and `array_push($a, ...$b)` pushes `$b`'s elements
      where today it pushes `$b` itself: `non-empty-list<int|list<int>>`
      on `list<int>` (`array-push-preserves-list.php`, 6), and a spread
      that may be empty leaves the array possibly empty (`empty($a)` after
      `array_push($a, ...$b)` on `$a = []` is `empty.variable` "not
      falsy" today, `bug-6974.php`). Appendix H's `byref-array-fns` is
      the rest of it (210 nsrt mismatches, 24 files): `array_pop` and
      `array_shift` move `hasOffset` facts down and keep non-emptiness
      (`array-pop-shift-has-offset.php`), `array_splice` keeps the list
      (`array_splice.php`, 104), the sort family and `array_pop` keep a
      template's `T&list<int>` (`bug-14631.php`, `bug-14633.php`), and a
      nested `array_push($a['k'], …)` keeps the shape (`bug-11846.php`).
    - **Closure types** follow `ClosureTypeResolver`: a closure that yields
      is `Closure(): mixed` today instead of returning a `Generator`
      (`closure-generator`, 6 nsrt mismatches). A
      closure or arrow function whose body never returns returns `never`
      under any native return type: `fn (): null => $this->redirect('/')`
      for a `never` method is `Closure(): never|null` today, which a
      `(Closure(): never)|null` parameter rejects (the rules oracle's
      `bug-15285.php`). A closure parameter `string $t = null` is
      implicitly `?string`, as a function's already is: in the body `$t`
      is `string` today, so `$t ?? ''` is `nullCoalesce.variable` and
      `$a(null)` is `argument.type` (`bug-6701.php`), and the closure isn't
      `parameter.implicitlyNullable`. A promoted constructor parameter is
      never implicitly nullable (PHP rejects `private int $p = null` at
      compile time): it is `parameter.defaultValue`, and `new P(null)` is
      `argument.type`; today both are silent and the parameter reads as
      `?int` (`instantiation-promoted-properties.php`).
    - **A call's value** has `void` as `null` (`getType`; the keep-void
      type is only for the void-result rules): `$a = a()` for `@return
      string|void` is `string|void` today, so `b($a)` into `?string` is
      `argument.type` (the rules oracle's `bug-6720.php`).
    - **Dynamic class constant names** (PHP 8.3): `K::{'X'}` with a literal
      name is `K::X`'s type; it is `mixed` today.
    - **`new $c()`** for a `class-string<T> $c` is `T` (`NewHandler`); it is
      `mixed` today, so `function make(string $c) { return new $c(); }`
      with `@return T` is `return.type` "expected T, got mixed"; so is
      `new $class` for `$class = static::class`, which is `static`, and
      after `is_subclass_of($c, A::class)`, which is `A` (Appendix I's
      `new-class-string`, 13 findings in 9 files).
    - **`new C()` is exactly `C`** (`ClassReflection::asFinal()`, dropped
      again where a template is inferred from it): after `$foo = new
      Foo()`, `$foo instanceof Sub` is `instanceof.alwaysFalse` and a
      discarded `$foo->load(1)` of a method without impure points is
      `method.resultUnused`, as for a final class; both are silent today
      for a non-final `Foo` (most of the rules oracle's 109 PHPStan-only
      `method.resultUnused`, e.g. `accept-throwable.php`, `bug-3589.php`,
      `bug-8048.php`).
    - **`new` into a generic property**: `$this->c = new Coll();` for a
      `@var Coll<int>` property types the `new` as `Coll<int>`
      (`NewHandler`, `NewAssignedToPropertyVisitor`); it is `Coll` today.
    - **`array.duplicateKey`** sees keys as PHP casts them (`false` beside
      `0`, `'1'` beside `true`), keys after a spread (`['a' => 1, ...$rest,
      'a' => 2]`, `['a', ...$b, 0 => 'x']`) and a variable key whose
      constant values collide (`$key` of `'foo'|'bar'` beside `'foo'`);
      unreported today (the rules oracle's 13 PHPStan-only,
      `duplicate-keys.php`, `bug-15244.php`, `bug-15247.php`,
      `bug-15248.php`, `literal-array-key-cast.php`).
    - **Array keys are cast as PHP casts them** on every write, read and
      `unset()`: `true`/`false` to `1`/`0`, a decimal-integer string to an
      `int`, a float truncated, `null` to `''`. After `$a[true] = 'x';
      $a['1'] = 'y'; $a[1.7] = 'z';`, `$a` is `array<(int|string)|string,
      …>` today where it is `array{1: 'z'}`; `$b[true]` on `[1 => 2]` is
      `mixed`; and `unset($m[false])` leaves key `0`, so the rules oracle's
      `unset-false-key.php`, where a missed `array_search()` unsets key `0`,
      isn't `identical.alwaysFalse`.
    - **A `static` variable** starts as its initializer's type unioned
      with every value the body assigns it (a later call sees the earlier
      call's last value), and as `mixed` once it escapes by reference:
      after `static $v = 1; …; $v = 'x';` it is `int` today, and `static
      $s = 'bar'` in `function &refFunction(): string` stays a string
      though a caller may store an `int` through the returned reference
      (`bug-757.php`, `return-type-after-finally.php`; PHPStan has `mixed`
      for every static variable).
    - **Function scope effects** follow `FuncCallScopeEffectsHelper`:
      - `array_pop`/`array_shift`/`array_push`/`array_unshift` keep a
        constant array's shape: after `array_pop($l)` on `[1, 2, 3]`, `$l` is
        `list<1|2|3>` today;
      - `fopen()`/`file_get_contents()` define `$http_response_header`,
        which is `variable.undefined` today;
      - `json_encode`/`json_decode` drop `json_last_error()`;
        `file_put_contents` drops `file_get_contents()` of the same path.
  - **Narrowing gaps** in the handlers' `specifyTypes`:
    - `$e === E::A` doesn't subtract the case, so `$e` stays `E` after
      `if ($e === E::A) { return; }` and in a `match` `default` arm (with
      `$this` as the subject inside the enum too: nsrt `bug-8486.php`);
    - `!empty($v)` on an `int` leaves `int`;
    - a condition on an increment narrows the variable: `if (++$a < 0)`
      leaves `$a` as `int` in both branches today where PHPStan has
      `int<min, -1>`/`int<0, max>`, and `$a++ < 0` narrows by the old value
      (nsrt `inc-dec-in-conditions.php`);
    - `$x instanceof $classString` narrows by the class-string's class:
      with `class-string<Foo> $cs`, `$subject` stays `mixed` today (nsrt
      `instanceof.php`);
    - `gettype($y)` compared with a type name narrows, in `===` and in a
      `switch`: `case "integer"` leaves `$y` as the whole union today (nsrt
      `bug-6901.php`);
    - loose `==` with a constant narrows (`specifyTypesForEqual`):
      `$x == false` on `0|1|''|'x'|null|bool` leaves the whole union today
      instead of `0|''|false|null`, and `($foo[$k] ?? null) == true` (or
      `!= false`) proves `$foo[$k]` exists while `== false` doesn't (the
      rules oracle's `bug-13488-loose.php`, 4); `$s != ''` makes a
      `lowercase-string` non-empty and `$s == 'one'` narrows a string or a
      union to the literal (Appendix H's `loose-eq-narrow`, 78 nsrt
      mismatches: `equal-narrow.php`, `equal.php`, `bug-12312.php`,
      `bug-14047.php`);
    - a `list` offset implies the lower ones: after `isset($list[3])`,
      `isset($list[2])` is always true (`isset-constant-array.php`), and a
      decimal-integer string can't be a key of `array<string, …>` (it is
      cast to `int`), so `isset($strings[(string) $int])` is "does not
      exist" (`bug-4671.php`);
    - a cast narrows its operand (`CastHandler::specifyTypes`):
      `(string) $s !== ''` leaves `$s` as `string` today;
    - `!is_scalar($s)` on a `string` leaves `string` (`!is_string` already
      gives `never`), so the checks inside that branch are reported
      against `string`: nine `function.impossibleType` in `composer/pcre`'s
      `Preg.php` copies where the oracle has `*NEVER*`;
    - boolean composition through `!`/`&&`/`||` (the recipes above):
      `assert(!(is_int($v) && $v < 0) && !(is_int($v) && $v >= 1))` leaves
      `$v` as `int` today where PHPStan has `0` (nsrt `bug-15039.php`);
    - `$b?->c->n === 1` doesn't narrow `$b` to non-null
      (`createNullsafeTypes`);
    - `if ($this->only)` doesn't narrow an untyped `?string` property to
      `non-empty-string` inside (the rules oracle's `bug-6486.php`), and
      `match (get_class($obj))` arms don't narrow `$obj`, so `$obj->id` in
      the `A::class` arm is `property.notFound` on `A|B` (`bug-8536.php`);
    - a template keeps `null` where the narrowing can't remove it: after
      `if ($foo !== null && !$foo instanceof Foo) { throw …; }`, `$foo` of
      `T` may still be `null`, so `$foo?->getValue()` isn't
      `nullsafe.neverNull` (`nullsafe-method-call-template-type.php`);
    - a call's conditional return type narrows its arguments
      (`specifyTypesFromConditionalReturnType`): after
      `if (testIsInt($v))` with `@return ($value is int ? true : false)`,
      `$v` is `int`, and `assertIsInt($w)` with `($value is int ? void :
      never)` asserts `$w` is `int`; both stay `mixed` today (nsrt
      `conditional-types-inference.php`);
    - a conditional `@phpstan-assert` type resolves against the call's
      arguments and parameter defaults
      (`ConditionalTypeResolver::resolveForCall`):
      `@phpstan-assert ($allow_null is true ? string|null : string) $input`
      called as `trueCheck($a)` leaves `$a` as the unresolved conditional
      today (nsrt `bug-9435.php`);
    - none of `EqualityTypeSpecifyingHelper`'s call comparisons narrow
      today (each reproduced):
      - `count($a) === 3` → a 3-element shape, and on a union of shapes
        the members of that size (`count($a) === 2` on `array{short:
        string}|array{long: string, details: string}` makes
        `isset($a['short'])` `isset.offset` "does not exist", the rules
        oracle's `bug-11494.php`);
      - `count($a) === count($b)` → shared non-emptiness (Appendix H's
        `count-narrow` has these and the other `count()` narrowings: 217
        nsrt mismatches, 27 files, `list-count.php` 79);
      - `count($l) > 2` on a list → the offsets it proves
        (`specifyTypesForCountFuncCall`): today `$l` stays `list<int>`
        instead of `non-empty-list<int>&hasOffsetValue(1, int)&
        hasOffsetValue(2, int)`, and `array_key_exists(1, $l)` stays
        `bool` (nsrt `bug-13747.php`);
      - `strlen($s) === 0` → `''`, and `strlen($s) > 0` → non-empty
        (nsrt `non-empty-string.php`), through a stored `$len`, against a
        range and against another `strlen()` (`strlen-narrow`, 59);
      - `array_key_first($a) !== null` (and `array_key_last`,
        `array_find_key`) → non-empty;
      - `preg_match(…) === 1` → the matches shape;
      - `get_class($o) === Bar::class`, `get_debug_type()` and
        `$o::class === Bar::class` → `Bar`;
      - `substr($s, 0, 1) === 'x'` and the other string functions compared
        with a non-empty literal → `$s` non-empty or non-falsy
        (`string-fn-narrow`, 52: `strstr`, `strrchr`, `strtolower`,
        `trim($s) !== ''`);
      - `strtolower($s) === 'abc'` → additionally lowercase on the other
        side.
  - **Deep expressions** don't overflow the stack: the handlers iterate
    over left-deep binary chains. A 32,000-term `.` chain aborts
    `check-project` with a stack overflow today, though the parser handles
    it; 8,000 terms take 29 s with literals and 58 s with variables
    (quadratic).

**D4. NodeCallback sink, observers and result storage** (approved deltas
for `return.unusedType`, `throws.*` and `property.unusedType`).

- The RefCell recording channels (`probe_hit`, `probe_var_provenance`,
  `member_ref_sites`, `escaped_enums`, `escaped_member_classes`) become
  observer events; `by_ref_empty_dim_args` becomes the by-ref argument
  context on the resolved call (Appendix C). The observer interface is
  internal here; D6 publishes it.
- `ExpressionResultStorage` keeps each node's before-scope, and rule type
  queries are answered from it.
- A node's callback fires post-order with its before-scope. A call then
  emits its call-expression node, so the impossible-check rules see the
  processed call.
- Non-final passes and quiet evaluations run with a Noop sink.
- Gatherer frames collect return statements, yields, execution ends and
  impure points outside the rule sink, as `NodeScopeResolver::pushNodeGatherer`
  does.
- A converged pass is recorded and replayed instead of a final walk when
  its body is replayable (`RecordingNodeCallback`,
  `isReplayableConvergenceBody`).
- The five `diagnostics.truncate` sites are deleted:
  `control_flow.rs:387`, `loops.rs:101`/`368`, `function_decl.rs:1297`
  and `checker.rs:2127`.

**D5. Convergence onto PHPStan semantics**, one approved-delta commit each,
with a fixture per transition and trigger, on D1–D4's structure. Where
PHPStan's rule is less correct at runtime, runtime correctness wins and
the deviation is named here: eval defining variables (§3.8), assignments
inside `assert()`, and a by-reference `use`d variable inside a closure
body (below). The groups land in this order, each
building on the previous one:

- **Holder model:**
  - natives as full holders plus `native_type_of`;
  - virtual keys: property-initialized, parameter original value, by-ref
    intertwining, always-remembered, possibly-impure call, clone
    reinitialization, original foreach key and value, foreach value by
    reference;
  - convergence equality: `MutatingScope::equals` compares the context,
    holders with certainty, natives and the conditional holders;
  - provenance as holder metadata;
  - non-holder scope state;
  - guards (each kind), `method_exists()`/`property_exists()`/
    `array_key_exists()` producing B4's `HasMethod`/`HasProperty`/`HasOffset`
    intersection members instead of the `guarded_methods`/
    `guarded_properties` side sets;
  - `container_field_facts` folding;
- **Certainty:**
  - certainty plus maybe-undefined (level-gated), with each variable's
    certainty added to the type snapshot:
    - A maybe-defined variable is reported today as `undefined variable` at
      level 0, where PHPStan reports "might not be defined" at level 1.
      `$x ?? null` on one types as `mixed`, and so does any read of it: a
      maybe-defined `Foo|null` makes `$a?->foo` `property.nonObject` on
      `mixed` (the rules oracle's `bug-7291.php`, `bug-12179.php`,
      `bug-4337.php`).
    - After a foreach that may not run, the value variable is dropped, and
      a read of it is `variable.undefined`.
    - At a file's top level every unknown variable is Maybe
      (`canAnyVariableExist`), because the including scope may define it,
      and a `@var` there names it: `/** @var \PDOStatement $stmt */` above
      `$stmt->fetch()` is `varTag.variableNotFound` today (8 rules-oracle
      findings, `bug-6688.php`, `discussion-10252.php`,
      `constant-parameter-check-methods.php`, `var-above-declare.php`).
      `echo $x;` in a script file is `variable.undefined` at level 0 today;
      snapshot 0a8fe69 has 75 of them in `<main>`. `$argc`/`$argv` there
      are `int<1, max>`/`non-empty-list<string>` (they are `int` and
      `array<(int|string), mixed>` today). A call to `file_get_contents()`
      or `fopen()` defines `$http_response_header` as `list<string>`
      (`FuncCallScopeEffectsHelper`); it is `variable.undefined` today (the
      rules oracle's `defined-variables.php:187`). Fixed first: before D5,
      an unknown variable read outside any function or closure is reported
      at level 1 as "Variable $x might not be defined."
    - A merge keeps a Maybe holder only for variables, function calls and
      virtual keys, and drops superglobals.
    - A by-reference argument defines the variable only where the
      parameter creates it (`PassedByReference::createsNewVariable()`):
      `reset($u)`, `array_shift($argv)` in a function and the first,
      by-value argument of `parse_str($ps, $ps)` read an undefined
      variable, and an undefined variable passed to a by-reference `int`
      parameter arrives as `null`, a `TypeError`
      (`pass-by-reference-into-not-nullable.php`); all unreported today.
    - The other reads count too: a variable variable with a constant name
      (`$foo = 'bar'; echo $$foo;`, a union of names read each;
      `dynamic-access.php`, 3), a name in `compact()`, nested arrays of
      names included (`compact-variables.php`, 3), a destructuring key
      (`[$f?->foo => $b] = …`), `$this` in a closure `Closure::bind()`
      binds to no object (`defined-variables.php:96`, `:99`), and a
      variable a correlated condition `unset()` (`if ($k === 1) {
      unset($v); } if ($k === 1) { echo $v; }`, `bug-14117.php`); all
      unreported today.
    - `isset()`/`empty()` in a condition don't define the variable: after
      `if (empty($foo)) {…} else {…}` or `if (isset($bar)) {…}` on a
      variable never assigned, `echo $foo` is unreported today where PHP
      warns (nsrt `falsey-empty-certainty.php`, `falsey-isset-certainty.php`:
      part of the 112 `No`/`Maybe` certainty assertions left unreported).
    - A truthy `??` proves its left side defined and non-null in that branch,
      as `isset()` already does: after `if ($c) { $foo = 'bar'; }`,
      `if ($foo ?? null) { return $foo; }` reports `$foo` undefined today
      (nsrt `bug-10224.php`; the suite's certainty files
      `dependent-variable-certainty.php`, `falsey-isset-certainty.php`,
      `falsey-empty-certainty.php` and `falsey-ternary-certainty.php` are
      D5's certainty fixtures).
    - An assignment inside `assert()` is Maybe afterwards, because
      `zend.assertions = -1` compiles the call out; PHPStan treats it as
      definite. Today `\assert((bool) ($trace = debug_backtrace()));`
      followed by `$trace ?? false` is `nullCoalesce.variable`
      (`amphp/amp/src/TimeoutCancellation.php:30`,
      `SignalCancellation.php:41`).
- **Conditional holders:**
  - conditional holders with `mergeWith`:
    `ScopeOps::createConditionalExpressions`,
    including the holders that record a variable as undefined on the other
    side, `preserveVacuousConditionals` and
    `withoutPreciseClassConstantFetches`;
    they replace `discriminant_array_facts` (nsrt `bug-5051.php` checks the
    flag-correlation cases). A variable defined under one
    condition and read under a correlated one is `variable.undefined` today:
    `symfony/filesystem/Filesystem.php:190` reads `$origFile`, which only the
    `$isRecursive`-dependent branch defines (and `$sb` in `if ($foo !==
    null && $sb === false)` after `if ($foo !== null) { $sb = false; }`,
    the rules oracle's `bug-1306.php`, where PHPStan finds the comparison
    always true);
  - conditional holders created by `&&`/`||` conditions
    (`TypeSpecifier::processBooleanSureConditionalTypes`/
    `processBooleanNotSureConditionalTypes`): after `if (!$a && !$b) {
    throw …; }` on `?DateTimeImmutable $a, ?string $b`, `if (!$a)` leaves
    `$b` as `string|null` today, so `new DateTimeImmutable($b)` is
    `argument.type` (the rules oracle's `bug-9946.php`), and after
    `assert($bar !== null || $baz !== null)` the right side of `$bar ??
    f($baz)` sees `$baz` as `?string` (`bug-8659.php`);
  - conditional holders created at assignment (`AssignHandler`), replacing
    `condition_aliases`, its lazy AST replay and `record_if_body_flag_alias`:
    - the assigned expression's sure and sure-not types, guarded by the
      variable's truthiness;
    - a ternary whose arm value only one arm produces;
    - `$v = match (…)`, reusing the `if`-chain merge, so narrowing `$v`
      narrows the subject: after `$v = match ($k) { 1 => 'a', default =>
      'b' }`, `$v === 'a'` leaves `$k` as `int` today;
    - derived assignments carrying the source's holders;
    - `$has = in_array($n, [1, 2], true)` implying `$n` on `!$has`: `$n` stays
      `1|2|3` today;
    - `$ok = preg_match(…, $m)` narrowing `$m` on `$ok`: it doesn't today;
    - `$first = $arr === [] ? null : 1` narrowing `$arr` on
      `$first !== null`: it doesn't today;
    - projection limited by `isExprSafeToProjectThroughVariable`: variables,
      property and dim fetches and calls always, anything else only when the
      right side has no impure points.
- **Invalidation:**
  - invalidation unification (receiver rebind, by-ref, declare purges):
    - An impure method call drops its receiver's holders
      (`MethodCallHandler`); a static call through `self`/`static`/`parent`
      drops `$this`'s. Today `if ($p->v !== null) { $p->reset(); }` keeps
      `$p->v` as `int`, and the same holds on `$this`, after
      `parent::reset()`, after `self::reset()` and after a dynamic name,
      `$this->{'reset'}()` (nsrt `bug-3831.php`). On ordinary code this is
      `$this->hasChanged = false; $this->refactorCall($node);
      if ($this->hasChanged)` reported `if.alwaysFalse`: ten in rector's rules
      (e.g.
      `rector/rules/Arguments/Rector/ClassMethod/ArgumentAdderRector.php:120`),
      none of which the oracle reports. Fixed first: before D5, a call on
      `$this` not known to be pure drops the `$this->…` narrowing entries,
      and a static call drops `self::$…`/`static::$…` ones: after
      `self::$i = new H(); self::bazz();` where `bazz()` resets it,
      `self::$i?->foo()` is `nullsafe.neverNull` today (the rules oracle's
      `bug-8523*.php`, and `bug-4443.php`'s `return.type`).
      With invalidation in place, the constant-comparison rules' property
      guards that compensate for it are deleted: `expr_reads_a_property`
      (the loose and numeric comparisons in `probe_checks.rs`) and
      `expr_reads_a_mutable_property` (`===`). Today even a readonly
      promoted `positive-int` isn't checked: `$this->maxAttempts < 1` in
      the constructor isn't `smaller.alwaysFalse`
      (`amphp/socket/src/RetrySocketConnector.php:24`, `:28`).
    - A closure's property writes invalidate the outer scope where it is
      called immediately (`InvalidateExprNode`): after
      `array_map(function () { $this->p = null; }, …)`, `$this->p` keeps
      its narrowing today; a closure passed to a call that may invoke it
      later counts too: after `$this->callbacks = [];
      $this->runner->wrap(function () { $this->callbacks[] = …; });`,
      iterating `$this->callbacks` is `foreach.emptyArray` today (the
      rules oracle's `bug-14888.php`).
    - A write to a typed property stores the value as PHP coerces it to the
      native type (`toCoercedArgumentType` under coercive typing) and
      intersected with it: after `$this->foo = '123'` on an `int` property
      `$this->foo` is `123`, `$this->f = 1` on a `float` is `1.0`, and
      `$this->data = $m` with `$m !== null` on `?array` is `array`. Today they
      stay `int`, `float` and `array|null` (nsrt `bug-12393b.php`).
    - A write to an offset that exists keeps the array a list
      (`AssignHandler::shouldKeepList`). Today both `$l[0] = 7` on a
      `non-empty-list<int>` and `foreach ($l as $i => $v) { $l[$i] = $v + 1; }`
      leave `array<int, int>`, and passing `$l` on to a `list<int>`
      parameter is `argument.type`.
    - An offset write through a non-constant key or an append makes the
      array non-empty, nested levels included (`$a[] = $v` on an `array`
      leaves `array` today): after `$logs = []; $logs[$i] =
      '';` `$logs` is `array<int, ''>` today where it is
      `non-empty-array<int, ''>`, and `$l[$i][$s] = ''` gives
      `array<int, array<string, ''>>` (the rules oracle's `bug-13538.php`).
      With the list cases above this is Appendix H's `list-write` (114 nsrt
      mismatches, 15 files: `bug-14245.php`, `bug-12274.php`,
      `list-type.php`, `bug-15245.php`, `bug-13857.php`); an append to
      `mixed` stays `mixed` (`bug-12447.php`). A write with a literal-union
      key builds optional keys, `$a[$k] = 256` with `$k` `'a'|'b'` on
      `array{foo: int}` giving `array{foo: int, a?: 256, b?: 256}` where it
      is `array<string, int>` today (`literal-key-writes`, 10:
      `set-constant-union-offset-on-constant-array.php`, `bug-11716.php`).
      An offset write on a property's array updates the property as one on
      a variable does: after `if (!isset($this->arr['foo'])) {
      $this->arr['foo'] = true; }` on `array{foo?: bool}`, `$this->arr` is
      still `array{foo?: bool}` inside and after the `if` today, where a
      variable gives `array{foo: true}` and `array{foo: bool}`
      (`bug-11679.php`, `pr-4374.php`). A write on a template-typed array
      intersects:
      after `$arr['mykey'] = $v` on `T of array`, `$arr` is still `T`
      today where it is `T&hasOffsetValue('mykey', int)&non-empty-array`
      (`bug-3931.php`).
    - An offset write keeps only the members that can take it (a scalar
      other than `null`, `false` and `''` throws): after `$r['bsw'] = 1` on
      `array<string>|true`, `$r` still has `true` today, so
      `array_key_exists($f, $r)` is `argument.type` (the rules oracle's
      `bug-4708.php`).
    - Narrowing an offset narrows its container
      (`specifyExpressionTypeInPlace`): the container gains
      `HasOffsetValue` for a constant key and becomes offset-accessible
      when it wasn't known to be an array. After
      `if (is_int($a['k']))`, `$a` is still `array<string, mixed>` today,
      and a `mixed` container stays `mixed`: after
      `isset($tokens[$i]['open'])`, `$tokens[$i]` is `mixed`, and reading
      `$tokens[$i]['open']` is a level-10 `offsetAccess.nonOffsetAccessible`
      PHPStan doesn't report. The bundled `php_codesniffer` alone has 368 of
      them at `76e738b4` (18 of 20 sampled follow such an `isset()`). A
      union container keeps only the members whose offset can hold the
      narrowed value: on `array{type: 'a', ap: string}|array{type: 'b', bp:
      string}`, `$t['type'] === 'a'` leaves the whole union in both
      branches today (the rules oracle's `bug-5758.php`, `bug-6364.php`).
    - A nested write keeps the element's other keys: after
      `$r[$key]['n'] += $n` on `array<string, array{n: mixed, i: mixed}>`,
      the element is `array{n: mixed, i?: …}` today, `i` made optional
      (the rules oracle's `slevomat-foreach-array-key-exists-bug.php`).
    - `unset($arr['a'])` removes the key from the shape; today the shape
      keeps `a: int`, and `$arr['a']` afterwards isn't reported. Through a
      non-constant key it applies too: `unset($list[$k])` makes a `list`
      an `array<int<0, max>, …>` (it stays a `list` today, a wrong value),
      and in `foreach ($list as $k => $v)` over `list<array{abc: string}>`
      both `unset($list[$k]['abc'])` and `$list[$k]['abc'] = 'world'`
      leave the element type unchanged today (the rules oracle's
      `bug-12927.php`).
    - Volatile expressions, dropped after an impure call, `new`, `eval` or
      `include`: `ob_get_level()`, `openssl_error_string()`, superglobals and
      their offsets, and negative existence checks.
    - `clearstatcache()`/`unlink()` drop the stat family (`file_exists`,
      `is_file`, `filemtime`, …); `openssl_*` drops `openssl_error_string()`.
    - A function, class or trait declaration drops the negative existence
      check for its name.
    - The output-buffer level delta.
  - references (`IntertwinedVariableByReferenceWithExpr`):
    - After `$b = &$a; $b = 'x';`, `$a` is still `1`, and
      `if ($a === 'x')` is reported `identical.alwaysFalse`.
    - After `$b = &$a[0]; $b = 'x';`, `$a` is still `array{1, 2}`.
    - References inside array literals (`processArrayByRefItems`): after
      `$b = ['key' => &$a]; $b['key'] = 42;`, `$a` is still `'hello'`, and
      `$a = [1, 2, &$x]; $x = 5;` leaves `array{1, 2, 0}` (nsrt
      `bug-14333.php`, `bug-15246.php`).
    - `$block = &$this->blocks[$l][$n]` types `$block` as the element type,
      although a reference to a missing offset creates it as `null`, so
      `$block ??= new Block()` is `nullCoalesce.variable` today
      (`latte/Runtime/Template.php:326`).
    - After `$ref = &$list; $ref[] = 5;`, `count($list) > 0` is reported
      `greater.alwaysFalse`.
  - `afterExtractCall` replacing extract/eval truncation:
    - Today an `extract()`/`eval()` drops every diagnostic of its function
      body. At file top level it drops every diagnostic after the file
      pre-scan, so the class methods' `return.type` errors disappear too,
      and an `eval()` inside a top-level closure does the same to the whole
      file: `unused-closure-uses.php` reports nothing, 14 of the rules
      oracle's findings lost (`closure.unusedUse`, `include.fileNotFound`
      among them), with `bug-12364.php`'s top-level `extract()`
      (3) and `bug-3690.php`'s `return eval('')` (1).
    - `extract()` of a constant array assigns its keys as variables, with
      optional keys Maybe; only another argument sets `afterExtractCall`.
    - `afterExtractCall` clears the conditional holders and makes every
      unknown variable Maybe. It doesn't invalidate the holders.
  - rememberPossiblyImpureFunctionValues: the analysis behaves as `false`
    today (of the 12 lines flipping it changes in PHPStan 2.2.16's output,
    over `gates/phpstan-oracle/`'s probes and its test fixtures, phpstan-rs
    agrees with the flipped run on 9); the `true`
    behaviour, PHPStan's default, comes with the key. Call results aren't
    remembered at all today: after `if ($x->getY() !== null)`,
    `$x->getY()` is still `Y|null`, even when the method is `@phpstan-pure`,
    and a variable defined under `if (returnsBool($b))` is
    `variable.undefined` under the next `if (returnsBool($b))` (17 `Yes`
    certainty assertions in nsrt `dependent-expression-certainty.php`; 10
    of the rules oracle's PHPStan-only `identical.alwaysFalse`, with the
    tip naming the possibly impure call, `possibly-impure-tip.php`).
    Appendix H counts the nsrt side: remembered built-in checks
    (`function_exists`, `is_file` until `clearstatcache()`, `ftell`; `guards`,
    54), remembered method calls (`hasAttributes()` then `->attributes`,
    `eof()` on `SplFileObject`; `method-call-memory`, 29), what an impure call
    or an object passed to one invalidates (`impure-invalidate`, 42;
    `invalidate-object-argument*.php`), readonly properties that keep their
    constructor values in other methods and hooks (`readonly-remember`, 25),
    and `ob_start()`/`ob_end_*()` moving `ob_get_level()` (`ob-level`, 47,
    `output-buffering.php`).
- **Control-flow transitions:**
  - **Loops** follow `LOOP_SCOPE_ITERATIONS = 3` with `generalizeWith` after
    the first pass, then the final walk. Today there are up to 6 passes and
    no generalization: `$i = 0; while (c()) { $i++; }` leaves `$i` as
    `0|1|2|3|4|5|6`, where PHPStan has `int<0, max>`, and `.=` in a loop
    leaves seven literal strings. The 14 `loop_depth` guards that suppress
    rules inside loops are deleted with it. The doc comments saying 3
    passes (`converge_loop_body`, `check_foreach`) go too. A narrowing
    every iteration makes holds after the loop: when a `foreach` body's
    `else` throws for `$number <= $compareTo`, `$compareTo` is `int<min,
    0>` after it, so a second loop's `$number > $compareTo` is
    `greater.alwaysTrue` (the rules oracle's `bug-7531.php`). Appendix H's
    `loop-generalize` is this item's nsrt count (208 mismatches, 70
    files): counters (`for-loop-i-type.php`, `bug-12163.php`,
    `bug-2835.php`), conditional appends that make an optional-key list
    rather than a union of lists (`constant-array-optional-set.php`,
    `bug-9753.php`, `bug-14297.php`), nested offset writes that keep a
    non-empty optional-key shape (`bug-14551.php`, `bug-13786.php`,
    `bug-6173.php`), constant floats (`bug-5309.php`) and appends that
    generalize to a list (`bug-13705.php`, `bug-7996.php`).
  - **For** follows `ForHandler`:
    - a loop whose first condition is `true` runs at least once: after
      `for ($i = 0; $i < 10; $i++) { $x = 1; }`, `echo $x` is
      `variable.undefined` today; one whose first condition is `false`
      never runs, so its body is unreachable: in `for ($i = 1; $i <
      count([]); $i++)` every expression is `*NEVER*` where `$i` is
      `1|2|3|4|5|never` and `$i + 2` is `int` today (nsrt `math.php`);
    - the exit scope applies the condition's falsey narrowing after
      generalization: `$i` is `0|1|2|3|4|5` in that body and `0|…|6` after
      it today, where PHP ends at 10;
    - `for ($i = 0; $i < count($a); $i++)` knows `$a[$i]`
      (`inferForLoopExpressions`);
    - `polluteScopeWithLoopInitialAssignments` decides whether the init
      assignments survive a loop that may not run.
  - **Foreach** follows `ForeachHandler`:
    - an unsealed shape's tail is iterated too: over `array{a: int,
      ...<string, float>}`, `$v` is `int` today where it is `float|int`, a
      wrong value (nsrt `unsealed-array-shapes.php`);
    - the body sees the iteratee narrowed by `!== []`, and the post-loop
      scope merges `=== [] || is_object()`; today `$arr` in the body is
      `array<string, int>`, not non-empty (nsrt `bug-13312.php`,
      `bug-13270a.php`, `foreach-with-specified-key-type.php`), and a
      `for` loop over `count($arr)` sees it non-empty the same way; a
      non-empty iteratee runs the body at least once, so its value variable
      and the body's assignments are defined after the loop (after
      `isset($arr['foo'])`, `echo $val` is `variable.undefined` today, the
      rules oracle's `foreach.php`, 11, `bug-8467c.php`);
    - `$arr[$k]` is the value type in the body, through the
      `OriginalForeachKey`/`OriginalForeachValue` virtual keys;
    - after the loop, the iteratee's value and key types are rewritten from
      the narrowed loop variables;
    - constant arrays of up to 16 keys are unrolled (8 when nested), so
      `foreach (['123', 'abc'] as $id) { $p[$id] = true; }` builds `array{123:
      true, abc: true}` (the rules oracle's `bug-6163.php`; and with
      `array_push($arr[$i], 'foo')` over `[0, 1]`, `array_values($arr)` is
      `arrayValues.list`, `bug-2560.php`), and
      constant-array iteratees add key → value conditional holders: over
      `array{psr4?: array<string, string>, classmap?: list<string>}`,
      `$key === 'classmap'` leaves `$value` as the union and `$data[$key]`
      as `mixed` today, so `implode(', ', $data[$key])` is `argument.type`
      (the rules oracle's `bug-6000.php`, `bug-8467a.php`);
    - an object iteratee reads its key and value from its `Traversable`
      ancestor's arguments: over `Dir<int, File>` for an interface with
      `@extends \Traversable<TKey, T>`, `$v` is `mixed` today (`Iterator`
      and `IteratorAggregate` arguments already apply; the rules oracle's
      `bug-9981.php`), and without arguments from its `key()`/`current()`
      return types: an interface extending `Iterator` whose `current()`
      and `key()` return `int` iterates `mixed` today
      (`filter-iterator-child-class.php`);
    - the key variable over a constant array is the union of its keys:
      over `['ENABLED' => 1, 'DISABLED' => 2]`, `$key` is `string` today;
    - `foreach (array_keys($a) as $k)` knows `$a[$k]`;
    - destructuring a tagged union of shapes adds cross-variable
      conditional holders;
    - a Traversable iteratee adds `getIterator()`'s throw point;
    - by-reference foreach leaves `ForeachValueByRefExpr`, so a write to
      the value variable is a write to the element: in `foreach ($a as $k
      => &$v) { $v = 1; }`, `$a[$k]` is still `string` in the body today, a
      wrong value (nsrt `overwritten-arrays.php`);
    - the loop always terminates only when the iteratee is non-empty and
      the body always terminates;
    - the post-loop scope includes the scope that never entered the loop:
      after `$e = null; foreach ($maybeEmpty as $e) {}`, `$e` is `1|2|3`
      today, a wrong value (nsrt `foreach-loop-variables.php`);
    - with `polluteScopeWithAlwaysIterableForeach` off (strict-rules), the
      body's assignments after a non-empty foreach are Maybe
      (`processAlwaysIterableForeachScopeWithoutPollute`), and the body
      doesn't see the iteratee narrowed to non-empty. Today the setting is
      read nowhere.
  - **Goto**: forward gotos merge at their label; a backward goto runs a
    fixpoint like a loop (`resolveBackwardGotoScope`). In
    `retry: $i++; if ($i < 3) { goto retry; }`, `$i < 3` is reported
    `smaller.alwaysTrue` today and `$i` is `1|never`.
  - **Short-circuit scopes** follow `BooleanAndHandler`/`BooleanOrHandler`/
    `TernaryHandler`/`NullsafeShortCircuitingHelper`:
    - an assignment on the right of `&&`/`||`/`??=` is Maybe afterwards:
      `$c || ($y = 1); echo $y ?? 2;` is `nullCoalesce.variable` today,
      `if ($c && ($z = 2) > 1) {}` leaves `$z` as `2`, and after `$r =
      $c[$k] ??= ($new = count($c) + 1);`, `isset($new)` is
      `isset.variable` "always exists" (the rules oracle's
      `bug-10151.php`); in the `else` of `if ($a && preg_match($p, $s,
      $m))` and under `if (rand(0, 1) || f($str))` a read is unreported
      today, as is `$w` in `while ($w && $w = 1)`, whose condition first
      runs on the scope before the loop (`boolean-op-truthy-falsey.php`,
      `defined-variables.php:356`, `:360`, `:428`);
    - an increment on the right of `&&` counts: `$j = 0; $foo && $j++;`
      leaves `$j` as `0` today, a wrong value, and in a loop `$i` stays `0`
      on every pass (nsrt `foreach-loop-variables.php`);
    - a `never` operand or arm keeps the other side's scope:
      `$v = $x !== null ? $x : throw new E();` leaves `$x` as `int|null`
      today (`$x !== null || throw` already narrows);
    - a nullsafe call's arguments are evaluated only when the receiver
      isn't null: after `$q?->m($e ??= new E())`, `$e ?? …` is
      `nullCoalesce.variable` today (`amphp/http-server/Http2Driver.php:726`),
      and after `$d?->format($f = 'Y')` a read of `$f` is unreported (Maybe,
      or undefined when `$d` is always `null`; the rules oracle's
      `bug-10729.php`, 5);
  - **Switch** follows `SwitchHandler`:
    - each case body is entered with `$subject == $case` true, merged with
      the previous case's fall-through;
    - after a terminating case, later cases and the post-switch scope see
      the subject with that case excluded: after `case 1: return;`, `$k`
      is `int` today; in `switch (true)` the case condition's falsey
      narrowing applies instead, so after `case $v === null: throw …;`
      `default` sees `$v` as `string|null` today (the rules oracle's
      `call-methods.php:1106`);
    - `default` excludes the handled cases, and a switch without one
      whose cases leave the subject `never` has no fall-through exit: a
      variable every case assigns is defined after it (`$method` after a
      `switch` over all four values of `@param MagicPropertyHandler::*`,
      the constants of a final class, is `variable.undefined` today,
      `rector/rules/Transform/Rector/ArrayDimFetch/ArrayDimFetchToMethodCallRector.php:171`);
    - `continue` inside a switch acts like `break`;
    - a `break` nested in a case's `if` leaves the switch with that scope:
      in `case 1: if ($c) { $v = true; break; } default: $v = false;`, `$v`
      is `false` after the switch today and `if ($v)` is reported
      `if.alwaysFalse` (nsrt `if.php`).
  - **Match** follows `MatchHandler`: the subject is always remembered, each
    arm's body scope is merged into the scope after the match, and a
    non-exhaustive match adds an `UnhandledMatchError` throw point. A
    `match (true)` is exhaustive when its arms' falsey narrowings leave the
    scope impossible (`isScopeConditionallyImpossible`): arms
    `$b instanceof Pay`, `is_string($b)`, `$b === null` over
    `Pay|string|null` report `match.unhandled` "remaining value: true"
    today; the oracle finds nine in snapshot 0a8fe69, e.g.
    `amphp/http-client/src/Response.php:218`, and the rules oracle 12 in
    7 files (Appendix I's `match-exhaustive`: `instanceof` arms over a
    union, `===` arms over literals or enum cases, every combination of
    two bools). Each arm's condition is
    evaluated on the scope the earlier arms' falsey narrowing leaves, so a
    repeated condition is `match.alwaysFalse`: `match ($i) { 1 => 'a', 1 =>
    'b', default => 'c' }` isn't reported today, nor is an arm outside the
    subject's type (`0 =>` on `int<1, max>` or `1|2|3`, a repeated enum
    case; the rules oracle's 7 PHPStan-only, `bug-11310.php`,
    `match-enums.php`, `match-expr.php`), and an arm the earlier ones leave
    always true is `match.alwaysTrue` unless it is the last arm (`3 =>` on
    `1|2|3` followed by `4 =>` or `default`; `self::FOO, self::BAR` over a
    two-case enum before `default`; `null` against a `void` call's `null`;
    `match-expr.php:28`, `:35`, `match-always-true-last-arm.php`,
    `match-expr-void-used.php`), unreported today. `default` sees the subject
    with every arm excluded: in `match ($this) { self::TWO => …, default =>
    $this->requireOne($this) }` on a two-case enum, `$this` is the whole
    enum today where it is `E::ONE` (the rules oracle's
    `call-method-in-enum.php`). An arm's body is entered from its
    condition's truthy scope, assignments included: in `match (true) {
    is_dir($dir = dirname(__DIR__)) => $dir, … }` and `match (1) {
    preg_match($p, $s, $m) => $m[1], … }`, `$dir` and `$m` are
    `variable.undefined` today (the rules oracle's `bug-13981.php`,
    `bug-10418.php`), as are the same shapes in a `switch (true)` case
    (`bug-5326.php`). An array subject is exhaustive when the arms cover
    its constant combinations: `match ([$b1, $b2])` with the four `[true,
    true]`…`[false, false]` arms, and `[$first, $second]` over enums or
    `1|2|3` × `'a'|'b'|'c'`, report `match.unhandled` "remaining value:
    array{0: bool, 1: bool}" today (the rules oracle's `bug-10128.php`,
    `bug-11453.php`, `bug-13029.php`, `bug-4451.php`). A subject over a
    small integer range is exhausted by its values: `match (rand(1, 3)) {
    1 => …, 2 => …, 3 => … }` leaves `int<1, 3>` today (`bug-6064.php`). An
    unhandled match isn't reported where an enclosing `try` catches
    `UnhandledMatchError` or the function declares it in `@throws`
    (`MatchExpressionRule::isUnhandledMatchErrorCaught`,
    `hasUnhandledMatchErrorThrowsTag`; `match-expr-property-hooks.php`).
    A template-typed subject is exhausted through its bound: `match
    ($indexBy)` on `T of IndexBy|null` with every case and `null`, and
    `match ($class)` on `class-string<T>` for `T of A|B` (both final) with
    `A::class`/`B::class`, report "remaining value: T" and
    "`class-string<T>`" today (the rules oracle's `bug-13048.php`,
    `bug-12998.php`).
  - **Try/catch/finally** follows `TryCatchHandler`:
    - a catch is entered from the merge of the throw points it catches;
    - its variable is the catch type minus the earlier catches, and an
      interface that doesn't extend `Throwable` is intersected with it:
      `catch (SomeInterface $e)` gives `SomeInterface&Throwable`. Today
      `$e` is `SomeInterface`, so passing it to a `Throwable` parameter is
      `argument.type`, and the catch is `catch.notThrowable`, which only a
      class that can't be `Throwable` is (the rules oracle's
      `accept-throwable.php`);
    - the finally scope merges the try end, the catch ends, their exit
      points and the remaining throw points, so a variable the try assigns
      before any throw point is defined there: `try { $bar = 1; } finally
      { echo $bar; }` is `variable.undefined` today, as is `$foo =
      doesntThrow()` with `@throws void` (about 40 of nsrt's `throw-points/*`
      certainty assertions, and
      `rector/vendor/symfony/console/Application.php:188`). Fixed first:
      before D5, a read in `finally` of a variable the `try` assigns is
      reported at level 1 as "might not be defined", not at level 0;
    - exit points leave through the finally block
      (`processFinallyScope`, `ReturnAfterFinallyNode`).
  - **Expression statements** apply their `specifyTypes` with the null
    context, so assertion calls narrow, and an equality assertion's result
    is remembered as `true`.
- **Class and function context:**
  - **Class member order** follows `ClassLikeHandler`: static methods,
    then the constructor, then instance methods, with properties and
    their hooks last.
  - **Constructor first**: the constructor is analysed before the other
    methods, and its end scope seeds them with readonly `$this`
    properties, `PropertyInitializationExpr`, `class_exists`/
    `function_exists` holders and constants (`rememberConstructorScope`).
    A class with custom serialization is excluded. A private method called
    on `$this` from the constructor contributes the properties it
    initializes (`CalledMethodProcessor`, `mergeInitializedProperties`).
    Today a `function_exists('f')` guard in the constructor still leaves
    `f()` in another method as `function.notFound`.
  - **Closures**:
    - `->call($obj)`, `Closure::bind()` and `bindTo()` bind `$this` and the
      scope class (`enterClosureCall`, `enterClosureBind`, `restoreThis`),
      and `->call()` passes its other arguments to the closure:
      `$predicate->call($newThis)` for a one-parameter closure is
      `arguments.count`, unreported today (`closure-call.php`, 3); today
      the bound closure keeps its old `$this` and scope, so its private
      access is `method.private` and `$this->m()` is `method.nonObject`
      (Appendix I's `closure-bind-scope`, 11 findings in 4 files);
    - an immediately invoked closure's body runs definitely: after
      `(function () use (&$x) { $x = 'a'; })()`, `$x` is `0|'a'` today;
    - by-reference `use` follows `processClosureScope`: an undefined
      variable is `null` in the outer scope, and afterwards it is the
      closure's type unioned with the previous one and generalized
      (`byref-closure-use`, 27 nsrt mismatches: the exact shape, no
      `never`, `closure-passed-by-reference.php`, `bug-11561.php`). Inside
      the body, PHPStan gives such a variable its type where the closure is
      created, but other closures and the enclosing function may assign it
      before the call: react's `TimeoutConnector`/`TimeoutExecutor` set
      `$timer = null`, share it with `use (&$timer)` between two
      callbacks, and PHPStan reports `if ($timer)` in one of them
      `if.alwaysFalse` (8 corpus findings). The deviation: inside the body
      it is the union of its type at creation and every value the
      enclosing function or its closures assign to it (today `null|never`,
      and unreported). A later argument of the call the closure is passed
      to reads the variable before the closure can run:
      `$this->foo(function () use (&$x): void { $x = 42; }, $x)` after
      `$x = 'hello'` passes `'hello'|42` to a `string` parameter today (the
      rules oracle's `discussion-14038.php`);
    - a closure with by-reference uses converges like a loop; any other
      closure's body is entered once, from its declared parameter types.
      Today a closure whose body returns after `if (!$node instanceof
      Return_)` is re-entered six times with the narrowed `$node` unioned
      into the parameter, which reads
      `Node|Node|Node|Node|Node|Node&Return_&…` at the body's start. In
      `function (\stdClass $std) { if ($std instanceof \stdClass) {} if
      ($std instanceof \Exception) {} }`, `$std` reads
      `stdClass|stdClass&Exception`, so neither check is reported
      (`unreachable-if-branches*.php`, 8, among the rules oracle's
      PHPStan-only `instanceof.alwaysTrue`);
    - a static closure has no `$this`: `static fn () => $this->p` isn't
      reported today (a static `function` already is `variable.undefined`),
      and PHP throws on it;
    - holders rooted at a `use`d variable carry into the closure body
      (`enterAnonymousFunction`): after `if ($call->name instanceof Id)`,
      `function () use ($call) { … }` sees `$call->name` as `Id|string`
      today, while an arrow function already keeps `Id`; an arrow
      function's own parameter shadows the outer variable's holders: after
      `$p->visited = true`, `fn (Point $p) => !$p->visited` is
      `booleanNot.alwaysFalse` today (the rules oracle's `bug-6473.php`);
    - `$f = function () use (&$f)` types `$f` as the closure inside its
      own body;
    - a closure's type carries its throw points, impure points and
      invalidated expressions, applied where the callee calls it
      immediately. `@param-immediately-invoked-callable`,
      `@param-later-invoked-callable` and the built-in defaults
      (`callCallbackImmediately`) decide which calls those are.
    - `(fn () => $this->n)->call($r)` is `undefined variable $this` today.
      Snapshot 0a8fe69 has 12 `$this` hits, among them
      `collision/ResultReflection.php:19`. The bound closure's read also
      leaves `property.onlyWritten` on `$n`.
  - **PHP version** narrows in a version branch: inside
    `if (PHP_VERSION_ID >= 80400)`, `PHP_VERSION_ID` and the scope's
    `php_version()` (§3.8) are the configured range cut at 80400, so
    version-gated checks there read the narrowed range. C2 first types the
    constant over the configured range; today `"php": "^8.1"` or
    `">=8.1"` leaves it the stub's `50306` (`~8.3.0` gives
    `int<80300, 80399>`, and `">= 8.1"` gives `int<50207, max>`, B8's
    grammar item).

**D6. Engine APIs**, one commit each:

- Scope API (§3.8's list);
- TypeSpecifier made public, with the type-specifying extension hooks;
- returned-error `Rule` and dispatcher: rules return `RuleError`s built by
  `RuleErrorBuilder`, and the dispatcher turns each into a `Diagnostic`,
  attaching the file and the trait file (a trait method's error analysed in
  a using class carries the trait's file). Each rule declares its
  identifiers as prefix × suffix sets, the dispatcher debug-asserts every
  error against its rule's declaration, and the `format!`-built
  identifiers (about 40 sites) become family members. An
  `[[ignore_errors]]` entry whose identifiers all lie outside every family
  is never reported unmatched: it names a check phpstan-rs doesn't have,
  not a stale entry;
- the rest of the derived-node catalogue (D2 and D4 emit the execution-end,
  unreachable-statement and call-expression nodes) plus the site-mapping
  table, recounted at D6 (543 report sites at `76e738b4`);
- Collector with collected data;
- Observer (published from D4's internal interface);
- the extension-trait registry;
- the concurrency / state model;
- `AnalysisContext` completed (C9 created it for the summaries);
- `InternalRule` deletion;
- porting the external `Rule`/`ReflectionRule` users.

### Stage E: everything out of the engine (in the package, module-gated)

Each rule is moved once, onto its final API: D6's returned-error `Rule`,
`RuleErrorBuilder` and identifier families, and E3's prologue, which lands
`RuleLevelHelper::find_type_to_check` and the rule → level table before any
rule moves. The level-dependent checks are then one mechanism instead of a
per-rule decision: on the corpus oracle's 941 files at `76e738b4` (level 10,
PHP version pinned), about 2,950 of the 9,623 findings PHPStan reports and
phpstan-rs doesn't are a rule that doesn't treat an implicit `mixed`
operand as reportable (listed under E3b). Findings phpstan-rs reports
that PHPStan doesn't are mostly the corpus's own strict-rules-style
extensions (`*.condNotBoolean`, `staticMethod.dynamicCall`,
`possiblyImpure.*`), which the extension-free oracle doesn't load, and
B8's context gaps: 1,748 of the 1,855 phpstan-rs-only `class.notFound`
name a class of the root package or of a package's bundled `vendor/`, and
the `mixed` those unknown classes produce feeds the phpstan-rs-only
`argument.type`/`return.type` "mixed given" findings. E's comparisons are
read after B8's two context fixes land.

Gate 12's rules oracle is Stage E's measure: over PHPStan's rule-test data
at `76e738b4`, PHPStan reports 15,476 findings across 633 identifiers (its
"(in context of class …)" copies of a trait's findings stripped and
collapsed) and phpstan-rs 11,428; 8,840 match, 6,636 are PHPStan-only,
and phpstan-rs never matches 152 of the identifiers. A finding matches
one of the same identifier in the same file on the same line first, then
within three lines, and within 60 for the signature-level identifiers the
two tools place differently (`missingType.*`, `return.missing`,
`constructor.unusedParameter`, `parameter.phpDocType`,
`phpDoc.parseError`), each finding used once. Every PHPStan-only finding
of an identifier phpstan-rs matches at least once was traced to its
cause, a plan item citing the file and count. Each E3 sub-phase ends when its
identifiers have no PHPStan-only findings left, or each remaining one is an
approved deviation (an identifier named differently, such as
`class.abstractMethodNotImplemented` for `method.abstract`,
`class.extendsEnum` for `class.disallowedSubtype`, `generics.wrongParent`
for a self-referencing `@extends`/`@implements`/`@use`'s
`generics.notCompatible`, `requireExtends.nonObject` for
`requireExtends.interface`, `intersectionType.int` for
`property.unresolvableNativeType`, and `enum.duplicateConstant` on the
constant for `enum.duplicateEnumCase` on the case, `property.private`
for `property.writeOnly` on a private native property a `@property-write`
tag shadows (PHP throws on the private read; `private-property-tag-write.php`),
and a trait's `@use` tag errors on the trait's line rather than the `use`
statement's (`generics.wrongParent`, `generics.callSiteVarianceNotAllowed`;
`used-traits.php`), or a
PHPStan-specific check: the assert mismatches of `phpstan.*` (E3h),
`phpParser.*`,
`phpstanApi.*` outside a PHPStan extension's package, `function.inner`,
PHPStan's "inner named functions are not supported", and `class.prefixed`
for a `_PHPStan_` prefix; the Rector, PHP-Scoper, PHPUnit PHAR and Box
prefixes it also reports name classes that change with each release of
those tools and ship opt-in in ext-extras, the rules oracle's other 4 of
9). `void.pure` ("returns void but does not have any side effects") is a
real dead-code check, not PHPStan-specific: it ships opt-in in ext-extras,
the rules oracle's 81 being mostly test scaffolding
(`filter-var-throw-on-failure.php`, `goto-undefined-label.php`,
`inc-dec-gmp.php`). A call phpstan-rs reports as `possiblyImpure.*`
"unknown" where PHPStan knows it is impure (`impure.*`, 23 rules-oracle
findings) is a wrong verdict, not a rename: C5's `functionMetadata.php` and
C9's purity summaries decide built-in, user and static calls, and E3(e)'s
purity items first-class callables and closures.

The oracle's other side is the false-positive measure: phpstan-rs reports
2,588 findings PHPStan doesn't, and every one was traced to its cause.
Each cause is a plan item where it lives (the items cite the rules
oracle's file; Appendix I lists, by cause and phase, the 1,019 in 410
files no item names and the 444 in 105 cited files their item doesn't
explain), and the phase that fixes it re-runs the oracle and removes
its findings; what stays is approved: stricter checks that are right at
runtime (`array_sum()` of arrays, printf placeholders that cast objects,
`new self()` statements with no effects, `static::$p;` statements, a
`private(set)` property never written, `5 / 0` ending reachability, a
`catch (\Throwable)` after one that catches it, an `@var` naming an unknown
class above an assignment operator, a promoted property never read,
`#[\Override]` on a trait's `__get` in a `stdClass` subclass, a PHP fatal
error), the
bleeding-edge `featureToggles` C1 turns on by default, 246
`phpstanRs.assertType` mismatches in rule-test data that the D and E2
items name, and 145 findings in 13 files that use php-parser and PHPStan
classes the phar loads and phpstan-rs doesn't (`self-contradiction.php`,
`TestMethodTypeSpecifyingExtensions.php`, `rule-error-signature.php` and
the rest), which the gate excludes and lists.

**E1. Collectors and project-level rules**, one commit each:

- the reference-site collector (a built-in collector, consumed by the dead code,
  architecture and circular-namespace rules and, later, the LSP);
- dead code / members / enum cases / abstract classes, with every use
  site the reference-site collector sees: a private constant read in an
  enum case value (`case One = self::MY_CONST`), an attribute argument
  (`#[Table(self::TABLE_NAME)]`), through `$this::C`/`$self::C` or a
  class-string (`$class::C`) and from a used trait's method is
  `classConstant.unused` today (16 rules-oracle findings: `bug-9402.php`,
  `bug-9005.php`, `bug-5651.php`, `bug-12011.php`, `bug-6758.php`,
  `bug-4570.php`, `class-constant-attribute.php`; a read the rule reports
  as `classConstant.private` is still a use, `class-constant-visibility.php:36`);
  a property read through
  `?->`, `get_object_vars($this)`, `$this::$p`, a hooked property's own
  get or set, a trait method of the host, and a PHP 8.5 `clone($o, ['p'
  => …])` write are uses too, `property.unused`/`property.onlyWritten`
  today (the rules oracle's 30: `nullsafe-property-fetch.php`,
  `bug-3654.php`, `bug-9409.php`, `bug-12702.php`, `bug-7198.php`,
  `clone-with.php`); and a private method called with another case
  (`$this->sayhello()` for `sayHello()`), through `?->`, as a callable
  (`[self::class, 'callBack']`), from a used trait (a trait's abstract
  private method the host implements) or through a class-string is a
  use, `method.unused` today (11: `bug-8346.php`, `bug-8071.php`,
  `bug-6039.php`, `nullsafe-unused-private-method.php`,
  `unused-private-method.php`); a private hooked property whose `get` no
  one calls is `property.neverRead` ("Readable property … is never read",
  an abstract private one included; `bug-12702.php`,
  `abstract-private-property-hook.php`, 3), unreported today, and a
  get-only one has no backing store to write, so it isn't
  `property.onlyRead` (`bug-12702.php:11`); a private
  method used only by itself (a
  recursive call, a `[$this, 'doBaz']` callable in its own body) or only
  from other unused ones is `method.unused`, and a dynamic `$x->$y()`
  counts only for the names `$y` can be (after `$y !== 'doBar'` it
  doesn't use `doBar()`); both unreported today (`unused-private-method.php`,
  `bug-11802b.php`);
- duplicate declarations across the project (`project::duplicate_classes` and
  `duplicate_functions`; a class body's repeated members are E3(e)'s
  `checker/duplicates.rs`): a collector over C4's per-file declaration sets and
  a rule reporting `function.duplicate` and
  `class.duplicate`/`interface.duplicate`/`trait.duplicate`/`enum.duplicate` on
  every site, the message listing all sites with paths relative to the project
  root, as PHPStan's `RelativePathHelper` prints them (absolute today). A
  declaration counts when it is unconditional (under `if (!function_exists(…))`
  or `class_exists()` it is a polyfill) and is project code: a file phpstan-rs
  reads as a stub (its phpstorm-stubs, `stub_paths`) never counts. That replaces
  the function check's signature-difference grouping, a workaround for
  phpstorm-stubs declaring `strtr()` twice that also hides two project functions
  with different signatures, which PHP rejects all the same ("Cannot
  redeclare"). Names compare case-folded (C4's duplicate query). Both stay
  opt-in: helpers of two test suites that never load into one process can't be
  told from a real duplicate;
- `namespace.circularDependency` over the reference-site collector's class
  dependencies, the set the architecture rules read: today only `extends`,
  `implements` and `use` edges count, so a cycle through type hints, `new`,
  static calls, constants or PHPDoc types is unreported. The finding names the
  namespaces and, for each edge of the cycle, the class and dependency that make
  it (today one representative class), and only analysed files' namespaces form
  cycles;
- `[[rules.architecture]]` as a collector rule: a class's dependencies are the
  reference-site collector's class references inside it (native hints, `new`,
  static access, `instanceof`, `catch`, attributes, parameter defaults, hooks
  and constants) and every class its resolved PHPDoc names, inline `@var`,
  `@throws`, `@extends`/`@implements`/`@use` arguments, template bounds,
  `@method`, `@property` and `@mixin` included. Today
  `Project::check_architecture` walks each file again with its own reference
  walker and reads PHPDoc only through method and property signatures, so `/**
  @var Infra\Repo $r */` in a domain method is no dependency. It runs in the LSP
  too (CLI-only today, §1);
- `[rules.banned]`'s constructs and types as per-node rules in the rules walk,
  each reporting `banned.<construct>` or `banned.type` where the node is: they
  are per-file checks, not project-level, though `Project::check_banned` walks
  each file again today and runs in the CLI only. `compact()` and `extract()`
  are matched by the resolved function name (a raw-name match today, so a
  namespace's own `compact()` is banned too), and `banned.type` reads the rules'
  resolved types;
- class shapes as a collector rule over the project's class declarations, run in
  the LSP too (CLI-only today, §1): `check_closed_classes` (`class.notFinal`,
  `method.protectedInFinalClass`, `property.protectedInFinalClass`,
  `classConstant.protectedInFinalClass`, `method.notFinalInAbstractClass`) and
  `[[parent_calls]]` (`parentCall.*`). A class stays open where something
  subclasses it at runtime, and extensions say what through a registration hook:
  a project class extending it (the one exemption `class_shape.rs` derives
  itself), a Doctrine proxy of an entity or mapped superclass (ext-doctrine's
  metadata, E4, instead of today's match on `@Entity` or `@ORM\Entity` in the
  doc text or any attribute whose short name is `Entity`), and a test double
  (`createMock(Foo::class)`, Mockery and Prophecy through ext-phpunit and
  ext-mockery: PHPUnit can't double a final class, and today such a class is
  `class.notFinal`). The required parent calls (`parent::setUp()` and the like)
  register the same way, from ext-phpunit, ext-codeception and the rest, instead
  of core defaults (§1's `TEST_BASE_CLASSES`);
- composer hygiene as a collector rule over the reference-site collector and the
  package context (the manifests B8 loads; the rule does no I/O), run in the LSP
  too (CLI-only today, §1): `undeclared.`, `misplaced.`, `devOnly.` and
  `unused.composerDependency`, `misplaced.autoloadDevSymbol`,
  `psr4.pathMismatch`/`psr4.caseMismatch`, `provide.unimplemented`,
  `ext.notRequired`, `composerScript.notFound`/`composerScript.notPublicStatic`
  and `composer.platformPhpInconsistentWithRequire`. A package's symbols include
  its global constants (`define()` in its autoloaded files, `const`
  declarations), which the reference sites don't record today, so a package used
  only through its constants is `unused.composerDependency`, and one whose
  constants are used undeclared isn't `undeclared.composerDependency`; today the
  check walks every file again with its own reference walker;
- `property.unusedType` as a ClassPropertiesNode rule over host-class
  assigns, including trait bodies per host (`facts_only`, the pre-pass
  and every `*_facts` field deleted). An untyped property without a
  default starts as `null`: `/** @var string|null */ private $debug;`
  assigned a string only under `PHP_SAPI === 'cli'` is "never assigned
  null" today (composer/xdebug-handler's `XdebugHandler.php:57`, one of 46
  phpstan-rs-only `property.unusedType` on the corpus oracle's files, and
  11 of the rules oracle's 16: `too-wide-property-type.php`,
  `bug-4526.php`, `bug-4680.php`, `bug-6230.php`). A by-reference get
  hook (`&get`) lets callers write any type, so the backing property
  isn't too wide (`return-type-after-finally-property-hooks.php`). A
  default counts as an assignment, a constant-expression one included
  (`= self::NUMBER_TYPE_OFF`), and an assigned range covers each constant it
  spans: `@var 0|1|2|3` with `=
  0` and `$this->c = random_int(0, 3)` is "never assigned 0/1/2/3" today
  (the rules oracle's `bug-4174.php`). A property no code assigns is
  checked too, from its default alone: `private ?string $s = null` read
  but never written is "never assigned string", and a `@var A|B` property
  without a default "never assigned A" and "never assigned B"; today a
  property with no assignment is skipped (all 42 of the rules oracle's
  PHPStan-only `property.unusedType`, e.g. `bug-6486.php`,
  `bug-9155.php`, `call-methods.php`, `access-properties.php`);
- `argument.parameterRenamedInSubtype`, on by default (behind
  `check_renamed_named_arguments`, off, today): a named argument naming a
  parameter an override renames throws "Unknown named parameter" once the call
  dispatches to that override. A collector records each named argument against
  an overridable method (public or protected, neither it nor its class final),
  keyed by the declaring class, and the rule reports the sites whose name an
  override anywhere renames. Late-bound `static::m(old: 1)` and `new static(old:
  1)` dispatch the same way (both throw in php 8.5) and are recorded too, where
  only instance method calls are today, and the renaming overrides come from
  reflection's declarations instead of `renamed_parameter_overrides`' second
  walk over every file's AST;
- the trait-host verdict as a collector plus rule: a trait method is analysed
  once per host (PHPStan's "in context of class …"), and the constant-condition
  and type-check rules (`if.alwaysTrue`, `function.alreadyNarrowedType` and
  their families) report a trait's finding only when every host gives it the
  same verdict, as `ConstantConditionInTraitRule`'s collector does, since the
  trait's code is shared: `is_int($this->g())` in a trait used by hosts whose
  `g()` returns `int` and `?int` is `function.alreadyNarrowedType` today, a
  false positive (`impossible-function-call-in-trait.php`), and a condition true
  in every host is unreported (the rules oracle's `*-in-trait.php`, 16 beside
  their empty-body `return.missing`). Today `check_trait_methods` applies the
  every-host rule only to identifiers named `*.always{True,False}`, so the
  type-check family (`function.alreadyNarrowedType`, `function.impossibleType`
  and the method and static-method forms) escapes it;
- the trait-host merge: a trait method's findings carry the host class they were
  found in, and the same finding (span, identifier and message) from several
  hosts is reported once, naming its hosts as PHPStan's "in context of class …"
  does; a finding some hosts lack stays with the hosts that have it. It replaces
  `check_trait_methods`' sort and `dedup_by` on (span, message), which merges
  any two findings sharing text and span whatever reported them, and no other
  deduplication exists (the finalizer has none, and gate 13 fails a repeated
  finding).

**E2. builtins.**

- Moves onto the extension traits: `throws/dynamic.rs`, `builtin_calls`,
  `array_ops` returns, `preg_shapes`, `date_time`, the contextual-callback
  block, preg `$matches` out-types, GMP / BcMath operators, the built-in
  narrowing predicates, the date / xml / DateInterval validators (as
  throw-type extensions), regex / PCRE, and the date parser.
- `builtins` is always registered, including by the testing harness.
- Every return-type extension in Appendix G reaches the most precise type that
  holds at runtime in the nsrt suite (3,015 mismatches over 93 extensions
  today). PHPStan's value is the candidate, not the target: ten of Appendix G's
  are wrong at runtime and stay approved deviations. Of the 3,015, 2,498 are
  wider than PHPStan's type and hold every value it holds; the 396 of them php
  can evaluate outside their fixture were run in php 8.5 and PHPStan's value
  held, and the other 2,102 need their fixture's context, so the commit adopting
  each value runs it in php 8.5 first and keeps phpstan-rs's type where
  PHPStan's is wrong. The other 517 are wrong at runtime, 220 missing a value
  the call can return, 190 typing a call PHP rejects as returning and 107 giving
  another PHP version's return, and the items below name every one of them,
  fixed first.
- The throw-type extensions `throws/dynamic.rs` lacks are added, each
  throwing only for the arguments that can fail: `array_chunk()`/
  `str_split()` with a length that may be below 1 (`ValueError`),
  `array_combine()` of unequal sizes, `min()`/`max()` of one possibly
  empty array, the printf family for a format that may be invalid or
  short of arguments (`ValueError`, `ArgumentCountError`), `get_class()`
  without an argument outside a class (`Error`) and of a non-object
  (`TypeError`), `round()` with an invalid mode, `trigger_error()` with a
  level other than `E_USER_*`, `unserialize()` with invalid options,
  `version_compare()` with an unknown operator, `Ds\Map::get()`/
  `remove()` without a default, `DateTime::modify()`,
  `DateInterval::createFromDateString()` and the `ReflectionClassConstant`
  constructor (PHPStan's `*ThrowTypeExtension`s). Without them a `catch`
  around such a call can't be judged, so `catch.neverThrown` (D2's throw
  points, E3f) misses them: the rules oracle's `printf-throw-type.php`
  (20), `min-max-throw-type.php`, `round-throw-type.php`,
  `get-class-throw-type.php`, `unserialize-throw-type.php`,
  `str-split-throw-type.php`, `array-chunk-throw-type.php`,
  `trigger-error-throw-type.php`, `ds-map-void-throw-type.php`,
  `bug-9066.php` (about 50).
- Behaviour commits, each with its fixture, after the move. Wrong values
  first:
  - `get_parent_class($this)` in a class with subclasses is
    `class-string<A>|false` (nsrt `bug-14951.php`), and on an interface-typed
    object it may be a class-string; today both are `false`, so
    `get_parent_class($this) === false` is `identical.alwaysTrue` and
    `if (get_parent_class($object))` is `if.alwaysFalse` (the rules oracle's
    `bug-4302.php`);
  - `filter_var()` with a flags argument that isn't constant but may hold
    `FILTER_NULL_ON_FAILURE` (`$strict ? FILTER_NULL_ON_FAILURE : 0`) returns
    `bool|null`; it is `bool` today, so the `=== null` check is
    `identical.alwaysFalse` (the rules oracle's `bug-11485.php`). An options
    argument that isn't constant, and flags inside it that aren't, are read the
    same way: `filter_var($m, FILTER_VALIDATE_INT, $options)` is `mixed` (a
    `default` option can return anything), `['flags' => $nullFilter]` gives
    `int|null` and a `FILTER_FORCE_ARRAY` flag an array, where each is the plain
    filter's `int|false` today (35 nsrt mismatches, `filterVar.php` 32,
    `filter-var.php` 3);
  - `array_filter` keeps a shape's keys required: with a callback,
    `array_filter([1, 2, 3], fn ($x) => $x > rand(0, 3))` is `array{0: 1, 1: 2,
    2: 3}`, and without one `array{a: int}|array{b: string|null}` comes back
    unchanged, where each key becomes optional and falsy values go; a narrowing
    callback keeps what it rules out (`array_filter($data, isFoo(...))` over
    `array{Foo|int, Foo|int}` keeps the `int`s), and an unsealed shape's key
    stays required with its falsy values (16 nsrt mismatches: `bug-11730.php` 9,
    `bug-6715.php` 2, `array-filter-constant.php`, `array-functions.php`,
    `array-filter-string-callables.php`, `minmax.php`,
    `unsealed-derivations.php`);
  - `array_combine($keys, $values)` is keyed by the positions `0`, `1`, `2`
    instead of the key array's values as PHP casts them: `array_combine(['a',
    'b', 'c'], ['d', 'e', 'f'])` is `array{0: 'd', 1: 'e', 2: 'f'}` where PHP
    gives `array{a: 'd', b: 'e', c: 'f'}`, and `[true, 'red', 1.5]` keys `1`,
    `'red'` and `'1.5'` (29 nsrt mismatches: `array-combine-php8.php` 15,
    `array-combine-php7.php` 8, `array-functions.php` 3, `bug-3351.php`,
    `bug-14980.php`, `non-empty-array.php`);
  - `array_intersect_key($a, [])` is `$a` instead of `array{}`, and so are
    `array_intersect`, `array_intersect_assoc`, `array_intersect_uassoc`,
    `array_intersect_ukey` and the `array_uintersect*` functions (nsrt
    `array-functions.php`, `array-intersect-key-constant.php`); a key the other
    array may lack stays required: `array_intersect_key(['a' => 1, 'b' => 2],
    $a)` with `$a['b']` set on one path keeps `b: 2` where it is `b?: 2`
    (`array-intersect-key-constant.php`, `unsealed-derivations.php`, 3);
  - `array_map()` over several arrays is a `list`: `array_map(fn ($s,
    $i) => …, $a, array_keys($a))` is `array` today (the rules oracle's
    `bug-12223.php`); a spread argument passes its elements:
    `array_replace(...$paths)` on `array<int, string> ...$paths` is
    `array<array<int, string>>` today where it is `array<int, string>`
    (`bug-7020.php`); `str_replace()` on a string subject is `string` (and
    keeps `non-empty`/`non-falsy`), `string|array<string>` today
    (`bug-8636.php`, `bug-12928.php`, `bug-13043.php`); `count($v) !== 3`
    throwing leaves an `explode()` list an `array{string, string, string}`
    (`bug-3499.php`);
  - `array_reverse()` of a shape is the reversed shape: on `array{0:
    string, 1?: string}` it is `array<string>` today (the rules oracle's
    `bug-11549.php`); `array_sum()` of ints is `int`, not `int|float`
    (`bug-9699.php`);
  - a pattern built from parts is validated as `RegularExpressionPatternRule`
    does: each constant alternative of the concatenation, with a
    non-constant part standing in as `(?:.*)` (`'/' . preg_quote($s, '/')
    . '}'` has no ending delimiter), and the empty pattern (`preg_replace('',
    …)`) is invalid; only a whole constant pattern is checked today (the
    rules oracle's 16 PHPStan-only `regexp.pattern`, `preg-quote.php`,
    `valid-regex-pattern.php`, `null-coalesce.php:83`); `preg_quote()`'s
    delimiter is checked the same way through named arguments
    (`preg_split(subject: $s, pattern: '&' . preg_quote(delimiter: '/',
    str: $s) …)`) and a delimiter taken from a prefix variable
    (`argument.invalidPregQuote`, `preg-quote*.php`, 3);
  - `json_decode()` of a constant string folds to its value
    (`JsonThrowOnErrorDynamicReturnTypeExtension`): `json_decode('{"class":
    "x"}')->class` is `property.nonObject` "on mixed" today (the rules
    oracle's `bug-12273.php`);
  - `E::from()`/`E::tryFrom()` of a constant resolve to its case or `null`
    (`BackedEnumFromMethodDynamicReturnTypeExtension`): `Button::tryFrom(10)`
    over the cases `0` and `1` is `Button|null` today, so `is_null($v =
    Button::tryFrom(10))` isn't `function.alreadyNarrowedType` (the rules
    oracle's `bug-12087*.php`, 4);
  - the printf format parser follows PHP's: `%5%` and `%1$%` are a
    literal `%`, `%ls` ignores the length modifier, `%'%5d` pads with
    `%`, flags combine (`%-+5d`), and `*` takes a positional width or
    precision (`%*2$d`, `%1$*2$d`, `%.*3$f`); each is `argument.sprintf`
    "invalid placeholder" today (11 rules-oracle findings,
    `printf-format-parser.php`, `printf-param-types-format-parser.php`);
  - the scanf format parser follows PHP's: a NUL byte ends the format
    (`"%d\0%d"` has one conversion), `%n` is a conversion and `l` a
    modifier (`"%s%n"` takes two values, `"%ld %lf %s"` three); each is
    `argument.sscanf` today, reported twice by two rules with different
    messages ("contains 4 placeholders" and "contains 4 placeholder(s)"),
    which become one (`bug-14567.php`, `printf.php`);
  - `Closure::bind()`/`bindTo()` check the new `$this` against the
    closure's `@param-closure-this` type (`Closure::bind($c, new self())`
    for a `stdClass` closure-this is unreported today,
    `closure-bind-param-closure-this.php`,
    `closure-bind-to-param-closure-this.php`);
  - `XMLReader::open()` is `XMLReader|false` called statically and `bool`
    on an instance (`XMLReaderOpenReturnTypeExtension`); it is
    `XMLReader|bool` today, so `->read()` after `!== false` is
    `method.nonObject` "XMLReader|true" (the rules oracle's
    `call-methods.php:1676`);
  - `class_implements()`, `class_parents()` and `class_uses()` of an
    object are `array`, without `false`, so a `foreach` over them isn't
    `foreach.nonIterable` (`bug-4335.php`); `DateInterval::format('%a')`
    is a numeric string, so `* 60` on it isn't `binaryOp.invalid`
    (`bug-1452.php`); `BcMath\Number` supports `++`/`--`
    (`inc-dec-bcmath-number.php`, four findings); `PDOStatement` is generic
    through PHPStan's stub (`PDOStatement<int>` is `generics.notGeneric`
    today, `missing-function-parameter-typehint.php`); a `SimpleXMLElement`
    compares to scalars and takes `$xml->a[] = …` appends
    (`invalid-comparison.php`, `bug-14054.php`);
  - `Closure::bind()`/`bindTo()` of a closure literal is `Closure`, not
    `Closure|null` (`ClosureBindDynamicReturnTypeExtension`; the rules
    oracle's `bug-7675.php`);
  - `count()` of a non-empty array is `int<1, max>`: after `$w[] = $v` it
    is `int<0, max>` today (the rules oracle's `bug-8277.php`), and so it
    is for a constant shape (`count($a) > 0` on `array{1}` is
    `greater.alwaysTrue`, `strict-comparison.php:118`), a
    `non-empty-array<int>` parameter or after `if ($a === [])
    { return; }` (`bug-6260.php`, `bug-7578.php`, `bug-9104.php`);
  - the hash functions return what they produce: `md5()` is
    `lowercase-string&non-falsy-string` and `md5_file()` that or `false`,
    `string` today (`hashing.php`); `(int)` of a decimal string past
    `PHP_INT_MAX` folds to `PHP_INT_MAX` (`bug-4848.php`);
  - `array_rand($a, $n)` is `int` instead of `int<0, max>|list<int<0, max>>` (a
    list's keys; a list of them for a count other than 1), so `array_keys()` of
    the result is `argument.type` (the rules oracle's `bug-9803.php`); with a
    count that isn't known it leaves out the list: `array_rand([1 => 1, 2 =>
    'b'], $mixed)` is `1|2` where it is `1|2|non-empty-list<1|2>` (nsrt
    `array-functions.php`, 4);
  - `array_map($cb, $l)` with a `callable(V): M` callback over `list<V>` is
    `array` instead of `list<M>`, so `new self(array_map($callback,
    $this->items))` into a `list<TValue>` parameter is `argument.type`
    (the rules oracle's `bug-10732.php`), `array_filter($l, fn ($e) => $e
    === '23423')` keeps `string` where the callback's narrowing gives
    `'23423'` (`bug-8174.php`), and `array_map($cb, [])` is
    `array` instead of `array{}` (`bug-14582.php`);
  - `array_slice($nonEmptyList, 1)` is `non-empty-list` where a one-element list
    slices to `[]`, and `array_slice($arr, 0, 0)` is `non-empty-array<string>`
    where it is always `[]` (22 nsrt mismatches: `bug-10721.php` 17,
    `array-slice.php` 5);
  - `array_change_key_case()` ignores its mode: `CASE_UPPER` gives lowercase
    keys (`array<lowercase-string, string>`, and `array{foo: 1, bar?: 2}` where
    PHP gives `array{FOO: 1, BAR?: 2}`), a mode that isn't known assumes
    `CASE_LOWER` where either case can come back, and a shape's keys and an
    unsealed tail's key type keep their case: `array{Foo: int, ...<string,
    float>}` with `CASE_LOWER` stays `Foo` where PHP gives `foo` (15 nsrt
    mismatches: `array-change-key-case.php` 8, `unsealed-derivations.php` 7);
  - `array_flip` of an unsealed shape ignores the tail: on `array{0: 1, 1: 2, 2:
    3, ...<int, bool>}`, `array_flip($u)[1]` is `0` where the tail's `true`
    values map to key `1`, and on `array{0: 'a'|'b', 1: 'c', ...<int, string>}`
    the key `c` is `1` where a later `'c'` in the tail replaces it (nsrt
    `array-flip.php`, `bug-14656.php`, `unsealed-derivations.php`);
  - `array_key_last`, `array_pop` and `array_shift` on a possibly empty array
    omit `null`: on `$c ? ['one', 'two'] : []` they give `0|1` and
    `'one'|'two'`, and on an all-optional shape `array{a?: 0, b?: 1, c?: 2}`
    `0|1|2` (nsrt `bug-14081.php`, `array-pop.php`, `array-shift.php`);
  - `array_column($rows, null, 'key')` keys the whole rows by the column, where
    it is typed as the column: `array<string>` for `array<int, array<string,
    string>>`, where it is `array<int|string, array<string, string>>` (nsrt
    `array-column.php`, `array-column-php82.php`);
  - array keys built from values are cast as PHP casts them:
    `array_fill_keys([true, false, 1.5, '999'], 'b')` has the keys `1`, `''`,
    `'1.5'` and `999`, and `array_flip(['1', '2'])` the keys `1` and `2`, where
    today the key type is the value type (`array<true, 'b'>`, `array<'1'|'2',
    int>`); an object key is its `__toString()` (an `Error` without one), and an
    unsealed shape's tail keeps its keys (`array_fill_keys()` of `array{0:
    'a'|'b', 1: 'c', ...<int, string>}` drops them today; 18 nsrt mismatches:
    `array-fill-keys.php` 13, `array-flip.php` 2, `bug-14980.php`,
    `bug-14656.php`, `unsealed-derivations.php`);
  - `array_merge()` is non-empty only when an argument is: of a possibly empty
    `$arr2` and an `array`, or of an all-optional shape, it is `non-empty-array`
    today (nsrt `array-merge-const-non-const.php` 4, `array-merge2.php`);
  - `explode()` with a negative limit may return `[]` (`explode(',', 'a,b', -5)`
    is `array{}`); it is `non-empty-list<string>` today (11 nsrt mismatches:
    `bug-14651.php` 7, `explode.php` 3, `bug-3961-php8.php`);
  - `get_defined_vars()` lists what can be defined: at file level any variable
    can be (`array<string, mixed>`; `array{global: 'foo'}` today), a variable
    assigned on one branch is an optional key (dropped today), and `$this` isn't
    a key (it is today; nsrt `get-defined-vars.php`, 3);
  - `preg_split()`'s flags decide its elements: `PREG_SPLIT_OFFSET_CAPTURE`,
    constant or possibly set in a flags argument that isn't constant, makes each
    an `array{string, int<0, max>}`; today every call is `list<string>|false`
    (22 nsrt mismatches, `preg_split.php`);
  - `rand()` takes its bounds in either order: `rand(5, 1)` is `int<1, 5>` and
    `rand($i, 0)` `int<0, max>`, where today they are the empty `int<5, 1>` and
    `int<0, 0>` (nsrt `rand-swapped-arguments.php`, 4);
  - `array_search()` with `$strict` over an unsealed shape takes the key from
    the entries whose value can be the needle: `array_search(true, $arr, true)`
    on `array{a: int, b: string, ...<int, bool>}` is `string|false` today where
    only a tail key, an `int`, can match (nsrt `unsealed-array-shapes.php`);
  - `count()` of a `Countable` object is its `count()` method's return type:
    `count(new Bar())` for a `count()` declared `@return -1` is `int<0, max>`
    today (nsrt `countable.php`);
  - `compact()` includes a variable that may be defined as an optional key, and
    reads the names in an array's values (`compact(['foo' => 'bar'])` names
    `$bar`); today both are dropped (nsrt `compact.php`, 2);
  - `array_keys()` of an array that isn't otherwise known is `list<int|string>`
    (non-empty once a key is known to exist); it is `array<(int|string),
    int>|array<(int|string), string>` today (nsrt `bug-6859.php`);
  - an unsealed shape's tail passes through `array_reverse`, `array_values`,
    `array_keys`, `array_slice`, `array_chunk` and `array_replace`:
    `array_values()` of `array{1, 2, ...<string>}` is `list<1|2>` today,
    dropping the tail's values (nsrt `unsealed-derivations.php`, 6);
  - a return that changed between PHP versions follows the analysed version
    range (`PhpVersions`, narrowed by `PHP_VERSION_ID`): over a range it is the
    union of each version's (under `^7.4 || ^8.0`, `bcdiv('10', '0')` returns
    `null` or throws, so it is `null`), and a file or branch narrowed below 8
    gets PHP 7's alone: the BcMath functions return `null` or `false` for a zero
    divisor or a bad operand, `round()`, `ceil()` and `floor()` `false` for an
    array, the hash functions `false` for an unknown algorithm, `substr()`
    `false` past the end, the array functions `null` for a non-array,
    `explode('')` `false`, and `count_chars()`, `get_class()`,
    `version_compare()` and `trigger_error()` their PHP 7 results; before 8.4
    `highlight_string()` returns `bool`. Today every call gets PHP 8.5's return
    (107 nsrt mismatches in files run at 7.4 or 8.3 and in `PHP_VERSION_ID <
    80000` branches: `bcmath-dynamic-return-php7.php` 32,
    `hash-functions-74.php` 15, `round.php` 12, `count-chars-7.4.php` 8 and the
    rest);
  - `gettype()` of a `resource` is `'resource'|'resource (closed)'`, since a
    resource may have been closed (D3's `is_resource()` soundness item is the
    same fact); it is `'resource'` today (nsrt `gettype.php`, 2);
  - `Throwable::getCode()` is `int`, but a `PDOException`'s code is a
    string, so `is_string($t->getCode())` reads as always false;
    `ThrowableReturnTypeExtension` gives `int|string` (benevolent) unless
    the class narrows it. `Exception::getCode()` is `mixed` today (nsrt
    `bug-6001.php`);
  - `trigger_error('…', E_USER_ERROR)` is `true` where it ends the script
    (`never`), so the code after it is reachable (the rules oracle's
    `bug-7799.php`); before PHP 8.0 a level
    other than `E_USER_*` returns `false` (nsrt `trigger-error-php8.php`,
    `trigger-error-php7.php`);
  - `strtr()` of literals folds to the result, and of a non-empty string
    with a pair array is non-empty: `strtr('change {me}', ['{me}' =>
    'me'])` is `string` today, so passing it to a `non-empty-string`
    parameter is `argument.type` (the rules oracle's `bug-13708.php`);
  - `strtolower`, `strtoupper`, `ucfirst` and the other case functions fold
    each member of a constant-string union: `strtolower($u)` on `'A'|'B'`
    is `non-falsy-lowercase-string` today where it is `'a'|'b'`, so passing
    `strtolower($key)` to an `'enabled'|'disabled'` parameter is
    `argument.type` (the rules oracle's `bug-6987.php`, with the foreach
    key above);
  - `mb_strtolower`, `mb_strtoupper` and `mb_convert_case` fold a literal as
    PHP does, give a string of any case for a mode that isn't known, and don't
    fold another encoding's bytes as UTF-8. Today they fold only ASCII in a
    literal (`mb_strtolower('Ａｂｃ')` is `'Ａｂｃ'` and
    `mb_convert_case('Česká republika', MB_CASE_UPPER)` is `'ČESKá REPUBLIKA'`),
    with a mode that isn't known (`mb_convert_case($s, $mixed)`) the literal
    comes back unchanged where any case can, and with another encoding
    (`mb_strtolower($s, 'CP1252')`) the bytes are folded as UTF-8 (nsrt
    `str-casing.php`, 7);
  - a call PHP 8 rejects returns `*NEVER*`, so the code after it is unreachable;
    today each returns its ordinary type. Checked one by one against php 8.5,
    the 190 in Appendix G throw a `TypeError`, `ValueError`,
    `DivisionByZeroError` or `Error`: an array function given a non-array
    (`array_flip`, `array_keys`, `array_values`, `array_reverse`,
    `array_search`, `array_intersect_key`, `array_fill_keys`) or an invalid
    callback (`array_filter($a, '')`); `array_combine()` of unequal sizes,
    `array_chunk()` with a length below 1, `array_fill()` past `PHP_INT_MAX` and
    `range()` past the largest array; the BcMath functions with a zero divisor
    or modulus, a non-numeric operand or a negative root (`bcdiv('10', '0')`,
    `bcpowmod('10', '-2', '1')`, `bcsqrt('-10')`); `explode('')`;
    `filter_input()` of an invalid input type; `get_class()` without an argument
    outside a class; the hash functions with an unknown or non-cryptographic
    algorithm (`hash_hmac('crc32', …)`); `iterator_to_array()` of a generator
    yielding array keys; `mb_strlen()` and `mb_substitute_character()` with an
    invalid encoding or character, and `mb_convert_kana()` with an invalid mode;
    `mt_rand()` and `random_int()` with `$min > $max` (`random_int(10, 1)` is
    the empty `int<10, 1>` today); under `strict_types`, `abs()`, `mb_strlen()`,
    `round()`, `ceil()` and `floor()` of a value of the wrong type
    (`abs('123')`, where PHPStan has `123` too); `str_increment()` and
    `str_decrement()` of an empty or non-alphanumeric string; `str_repeat()`
    with a negative count (`''` today); `trigger_error()` with a level other
    than `E_USER_*`; `version_compare()` with an unknown operator; `count()` of
    an object that isn't `Countable`; `pow()` of an object or array operand;
    `preg_split()` with a non-integer limit or flags; and `strval()`,
    `intval()`, `floatval()` and `boolval()` without an argument or of an object
    without `__toString()` (`round-php8-strict-types.php` 30,
    `bcmath-dynamic-return-php8.php` 27, `round-php8.php` 15,
    `hash-functions-80.php` 11, `mb_substitute_character-php8.php` 9,
    `mb-strlen-php83.php` 8, `str_decrement.php` 8, `abs.php` 7 and the rest);
  - the built-in narrowing predicates follow the 2.2.16
    `*TypeSpecifyingExtension`s; compared one by one, these leave the type
    wider today: `array_key_exists($k, $a)` doesn't narrow `$k` to the key
    type nor a `mixed` `$a` to an array (so a second `array_key_exists($b,
    $o)` is `argument.type`, the rules oracle's `bug-14312.php`; with the
    folds on constant arrays, the optional key made required and the key
    removed in the else, Appendix H's `key-exists-fold` is 153 nsrt
    mismatches in 29 files, `key-exists.php` and `array-key-exists.php`
    the most),
    `ctype_digit($s)` doesn't give `numeric-string`, `function_exists($s)`
    doesn't give `callable-string`, `class_exists($c)` and
    `is_subclass_of($c, A::class)` on a string don't give
    `class-string`/`class-string<A>`, `array_search($v, $a, true) !== false`
    doesn't make `$a` non-empty, and `ReflectionClass::isSubclassOf(A::class)`
    leaves `ReflectionClass<A|object>`. `count($a)` compared by `===`, `==`
    or `!==` doesn't make `$a` non-empty (`count($a) === 1`, `!== 0`, and
    `match (count($a))`'s arms; `>` and `>=` already do), so `reset($a)` in
    the `1` arm is `string|false` (the rules oracle's `bug-10974.php`).
    `in_array()` narrows through an enum-case haystack (`in_array($x,
    [Suit::S], true)` returning leaves `Suit`, `bug-7176.php`) and, loose,
    an integer subject over an integer haystack (after `if
    (!in_array($foo, [0, 1, 2])) { exit(); }` on `int<0, 100>`, `$foo` is
    `0|1|2`, `bug-9436.php`), and a true `in_array()` makes the haystack
    non-empty and a false one removes the needle from it (Appendix H's
    `in-array-haystack`, 65 nsrt mismatches: `bug-9662.php`,
    `bug-9662-enums.php`, `enum-in-array.php`, `bug-3013.php`). A strict
    `in_array()` intersects the subject
    with the haystack's value type instead of replacing it: on `int $i`
    against `array<string>` the true branch is `string` today where it is
    `*NEVER*`, and `$i` is `string|int` after an empty `if`. `count()` and
    `strlen()` of a union of
    constant shapes or strings are the union of their lengths (`count($r)`
    on `array{'a'}|array{'a', 'b'}` is `1|2`, `int<0, max>` today,
    `bug-12790.php`; `strlen('ab')` is `2`, `bug-7555.php`).
    `is_numeric()` drops non-numeric constant strings: on `1|'a'` it keeps
    `1|'a'` today (the rules oracle's `bug-2550.php`).
    `is_callable([$this, 'm'])` guards `$this->m()` as `method_exists($this,
    'm')` does, and both guard a ternary's true branch and a first-class
    callable (`method_exists($this, 'm') ? $this->m(...) : null`); each is
    `method.notFound` today (the rules oracle's `bug-13596.php`);
    `defined('Foo::TEST')` and `defined('static::FIELDS')` guard the class
    constant (`classConstant.notFound` under them today,
    `class-const-fetch-defined.php`, `bug-8034.php`);
    `property_exists(static::class, 'd')` guards `static::$d` and
    `method_exists(static::class, 'm')` or `method_exists(X::class, 'm')`
    a static call (each is `staticProperty.notFound`/
    `staticMethod.notFound` today, `bug-2861.php`, `bug-1267.php`,
    `static-has-method.php`).
    `is_callable()` keeps a non-final class as `Class&callable(): mixed`:
    on `Route|callable(): Route` it leaves `callable(): Route` today (the
    rules oracle's `bug-5333.php`); on an array it gives `callable-array`,
    a two-element list with `0|1` keys, `class-string|object` at `[0]`
    and `non-falsy-string` at `[1]`, which a `callable-array` parameter
    reads as too (`bug-14549.php`). `is_a($s, A::class, true)` on a
    string narrows to `class-string<A>`: today it gives the object `A`,
    a wrong value (on `class-string<Bar>` against `Foo` it gives `Foo`;
    `is-a.php`, `bug-6404.php`). On an object with a `class-string<A>`
    variable it gives `A` (`object` today), with a plain string
    `class-string`, and between class-strings that can't relate it is
    `false` (`bug-4371.php`); `!is_a($a, B::class, true)` removes
    `class-string<B>` from a union (`bug-6704.php`). An `is_a` on an
    unknown class, repeated, isn't deduplicated: `$b` shows as
    `Bar|Bar|Bar|Bar|Bar|Bar&Foo&Foo&Foo&Foo&Foo&Foo` (`is-a.php:38`, B4).
    `str_contains`, `str_starts_with` and `str_ends_with` with a
    non-empty needle make the haystack non-empty, and non-falsy when the
    needle is non-falsy, keeping its numeric and case accessories; today
    they leave `string` (29 mismatches,
    `non-empty-string-str-containing-fns.php`).
    `is_real` is `is_float`'s alias, `is_array` on `mixed` prints
    `array<mixed, mixed>` and `is_callable` `callable(): mixed`
    (`specifiedTypesUsingIsFunctions.php`, 24). `defined`/`define` and
    the rest of the `is_*` family already match.

  Then the rest, by each function's count in Appendix G (the wrong ones among
  them are the items above): `filter_var`, `filter_var_array` and `filter_input`
  (290, 194 of them `mixed`), `min`/`max` (217, 198 `mixed`), `array_column`
  (211), `sprintf`/`vsprintf` (135), the BcMath functions (134), `count` (114)
  and `array_slice` (101); beside Appendix G, `settype`'s narrowing
  (`set-type-type-specifying.php`, 156), the preg `$matches` shapes
  (`preg_match_shapes.php`, 204, and the 155 variable mismatches of Appendix H's
  `preg-shapes` in 24 more files: `preg_match_all`, the `/n` flag, group 0
  non-falsy when the pattern has literals, `> 0` and `=== 1` making groups
  required) and the `BcMath\Number` operators (170 more in the `bcmath-*.php`
  files, Appendix J's `bcmath-*` causes). Appendix H's `ext:` entries are the
  variables these extensions' values flow into, 621 mismatches beyond the
  calls themselves. Three have no counterpart in phpstan-rs today
  (`ext:PgDml`, `ext:Randomizer`, `ext:FilterInputArray`), and
  `getAttributes()` substitutes its template but returns an array, not a
  list (`ext:ReflectionGetAttributes`).

Gate: the analyser's built-in-name allowlist.

**E3. Every diagnostic to `rules`/ext modules.**

The prologue, one commit each, before any rule moves:

- `RuleLevelHelper::find_type_to_check` (PHPStan's `findTypeToCheck`): a
  rule asks it for the operand type to check and gets PHPStan's
  `FoundTypeResult` (the type with the level's mixed, union and nullable
  handling applied, plus the unknown-class errors). Today
  `RuleLevelHelper` only carries the three settings into
  `types::Relation`, and each rule decides for itself whether a `mixed`
  operand is reportable. The implicit `mixed` inside a type counts too:
  at level 10 a plain `array` (or `array<mixed>` from untyped data)
  passed to `array<string>`, `array<literal-string>` or a shape is
  `argument.type`, unreported today (`bug-14234.php`, `bug-6443.php`,
  `vprintf.php`, `discussion-7004.php`, `literal-string.php`,
  `bug-14549-bis.php`, `call-methods.php:1332`, `invalid-binary.php:127`
  among the rules oracle's PHPStan-only `argument.type`), and so is an
  explicit one: `array<int, mixed>` returned under `@return array<int,
  string>` is `return.type`, unreported today (`bug-8166.php`,
  `bug-10349.php`, `bug-5218.php`, `compact-variables.php`,
  `pure-unless-callable-is-impure.php`, `bug-9377.php`, `bug-11386.php`,
  `bug-5065.php`, `invalid-lexical-variables-in-closure-use.php`);
- `RuleLevelHelper::accepts` takes the level-dependent acceptance out of
  `types::Relation::accepts` (`configured_level`, `strict_mixed_arrays`
  and `check_benevolent_union_types` leave `types::Relation`);
- the rule → level table (§3.9), cross-checked against the level neon files,
  each rule registered at its level, and the level filter in the dispatcher:
  rules above the level never run. It replaces the CLI's
  `apply_level_filter` (`MAX_LEVEL = 6`) and the LSP's separate cutoff (§1's
  level row). It starts from these divergences, measured by
  running the same probes through PHPStan 2.2.16 and phpstan-rs at every level
  (first level reporting, PHPStan / phpstan-rs): `isset.variable` and
  `nullCoalesce.variable` 1 / 0; a maybe-defined or top-level undefined variable
  1 / 0; `method.notFound` on a receiver other than `$this` 2 / 0; a method or
  property reached only through `__call`/`__get` 2 / never; a union argument
  7 / 5 and a union return 7 / 3 (the `.all()` arm in
  `types::Relation::accepts` below level 7); a maybe-non-offset-accessible
  7 / 3; a maybe-missing constant-array offset 7 / never; a PHPDoc-only child
  parameter type conflict 3 / never; `@var array` without a value type
  6 / never. The approved deltas are these rows.
- the rest of the diagnostic model (§3.7): AST-node fixes beside text
  fixes, rendered to diffs through `syntax::printer`, which gains
  statements and declarations, keeps the surrounding formatting
  (php-parser's format-preserving printer) and parenthesizes a generated
  node by precedence (it prints expressions only, as written, and a
  closure without its body); `accepts` reasons, `Relation::accepts`
  filling `AcceptsResult.reasons` for the argument, return and property
  rules to show as tips; PHPStan's standard tip helpers (§3.7:
  discovering symbols, `treatPhpDocTypesAsCertain`, and the rest);
  messages name a namespaced function by its namespaced name, as they
  already name methods: 576 of the rules oracle's phpstan-rs rows print
  `Function as_array2()` for `Bug13247\as_array2()` (`missingType.*` 522,
  `return.unusedType` 18, `throws.*` 15, the purity rules 4); an integer
  shape offset prints as `2`, not `'2'` ("Offset '2' on array{0: string,
  …} in isset() always exists" today).

Then the sub-phases, grouped by derived node, each listing its Appendix A
rows, and each rule's behaviour commits after its move, each with its
fixture:

- (a) per-file and per-body rules: `keywords` (`break`/`continue` levels, `goto`
  and labels, `declare` directives: `goto.*`, `label.*`, `declare.*`,
  `declareStrictTypes.*`), checking every body, property hooks included, where
  today a hook's `break`/`continue` outside a loop (`break.outOfLoop`,
  `continue.outOfLoop`, `continue-break-property-hook.php`, 3) and its `goto` to
  an undefined label (`goto.labelUndefined`) go unreported; `whitespace`
  (`whitespace.bom`, `whitespace.fileStart`, `whitespace.fileEnd`); and
  `RequireFileExistsRule` (`includes.rs`), whose lookups go through the
  session's VFS instead of `Path::is_file`, so a file the editor holds unsaved,
  or has deleted, counts as the editor has it.
- (b) expression-level:
  - every operand check goes through `find_type_to_check`, so at level 10
    an implicit `mixed` is reported everywhere PHPStan reports it. The
    corpus oracle's misses, by identifier: `binaryOp.invalid` (652, `.`
    and `+` on `mixed`), `assignOp.invalid` (223), a `mixed` array key or
    offset (`offsetAccess.invalidOffset` 954 and `array.invalidKey` 223
    "Possibly invalid array key type mixed"), `encapsedStringPart.nonString`
    (356), `foreach.nonIterable` (284), `echo.nonString` (39),
    `cast.string`/`cast.int` (135), `++`/`--` on `mixed` (55) and
    `callable.nonCallable` (30);
  - the same helper reports an operand with one invalid member ("Possibly
    invalid", "might not be", at the level `checkUnionTypes` and
    `checkNullables` set), where today only a wholly invalid operand is:
    `echo $a?->foo` on `Foo|null`, `foreach` over `array<int>|null`,
    `(string)` of `stdClass|null`, `object|string` or a plain `object`,
    `clone` of `Foo|null` or `mixed` (`clone.nonObject`,
    `variable-cloning*.php`, 3),
    `[$a] = $arrayOrNull`,
    `...$integersOrNull` (`argument.unpackNonIterable`,
    `arrayUnpacking.nonIterable`), destructuring an `int|list<int>` `foreach`
    value
    or a nested `stdClass` (`[[$a]] = [new \stdClass()]`, also definite),
    `new stdClass instanceof $class` (`instanceof.invalidExprType`) with
    `string|null`, `$ob < $n` and `$ob == $n` for `stdClass` and `float|null`
    (`smaller.invalid`, `equal.invalid`), `$string()`
    and `$o(...)` for a `string` or `object` that may not be callable, an
    array key of `int|null`, `string|null` or `int|object`, and `.` on
    `array<string>|string` (with the `mixed` rows above, these are the rules
    oracle's PHPStan-only `echo.nonString` 375, `binaryOp.invalid` 93,
    `assignOp.invalid` 77, `offsetAccess.invalidOffset` 79,
    `foreach.nonIterable` 27, `callable.nonCallable` 25, `cast.*` 42
    (`cast.string`, `cast.int`, `cast.double`),
    `unaryOp.invalid` 18, `offsetAccess.nonArray` 16 and smaller rows,
    `print.nonString` 8 among them, `print` of a closure included);
    an `object` operand of `+`, `-` or `%` is invalid too (`+$o`, `$o % 3`);
  - `++`/`--` on `bool`, `null`, a `resource` or a non-numeric string is
    `postInc.type`/`preDec.type` and the rest (PHP leaves `true` and `false`
    unchanged, `null` for `--` and a non-numeric string for `--` (`$s = 'a';
    $s--;`), and throws for a resource), unreported today
    (`invalid-inc-dec.php`, `inc-non-numeric-string.php`,
    `dec-non-numeric-string.php`); `[]` for reading inside `isset()`,
    `empty()` and `??` is `offsetAccess.noDim`, a compile error, unreported
    today (`offset-access-without-dim-for-reading.php`);
  - an offset or array key of an invalid type is reported for every
    type: `$a[$o]` and `$a[$o] = 1` with an object offset and `[$o => 1]`
    throw a `TypeError` and aren't reported today (an `array` offset and,
    with a PHP version known, a `float` one already are);
  - `offsetAccess.notFound` covers every container and offset. A
    maybe-missing offset (an optional shape key, a union of keys some of
    which are missing, a nullable offset) is "might not exist" at level
    7, never reported today (the level table's row). Definite misses
    beyond a constant shape are unreported today too: a key type disjoint
    from the array's (`$ss[$int]` on `array<string, string>`, `'b'` on
    `array<'a', string>`), `null['t']`, a constant string's non-integer
    offset (`'foo'['foo']`, `'foo'[12.34]`), `'foo'` on an
    `ArrayAccess<int, …>`, a `foreach` value's destructuring key
    (`foreach ($rows as ['b' => $v])` over `list<array{a: 0}>`,
    `bug-8075.php`, 5), and a destructuring past a folded `explode()`
    (`[$x, $y] = explode(':::', 'a')`, `bug-15013.php`); these are the
    rules oracle's 47 PHPStan-only;
  - arithmetic with a `string` operand that isn't `numeric-string` is
    `binaryOp.invalid` (PHP 8 throws a `TypeError` for a non-numeric
    string): `$s / 2`, `$s * 2`, `2 / $s` on `string $s` are unreported
    today (B4's `ErrorType` gives the result);
  - a non-numeric string offset on a string throws a `TypeError`:
    `$s['x']` on a `string` isn't reported today, nor `$d['priority']` on
    `array<string>|string` (`react/dns/src/Protocol/BinaryDumper.php:86`);
  - the class-access checks cover every receiver and target, each a
    runtime `Error` unreported today: `$i::X` and `$i::$y` on an `int`,
    and possibly on `stdClass|null`, `string|null` or `int|string`
    (`$stdOrNull::class`, `$stringOrInt::FOO`; `classConstant.nonObject`,
    `staticProperty.nonObject`, 7 each), `T::C` on a
    trait (`classConstant.onTrait`), and a static property read through
    `->` (`$p->st`, `staticProperty.nonStaticAccess`, the rules oracle's
    7); `$m instanceof T` with a trait is always false (`instanceof.trait`);
    an instance property read statically (`F::$bar`, `static::$bar`,
    `\stdClass::$foo = …`) is `property.staticAccess` "Static access to
    instance property", `staticProperty.notFound` today (the rules
    oracle's 16);
  - the constant-operand checks of `&&`, `||`, `and`, `or`, `xor` and
    `?:` fire on each side and on the result (`$b && true` isn't
    `booleanAnd.rightAlwaysTrue` today): the rules oracle has 164
    `boolean{And,Or}.*`/`logical{And,Or,Xor}.*` findings and 17
    `ternary.always{True,False}`, of which phpstan-rs matches 2 and 0, and
    the corpus oracle 21 `booleanAnd.leftAlwaysTrue` and 23
    `booleanOr.alwaysFalse` phpstan-rs misses (D3's type-check folding
    feeds them); and a condition is decided by its
    truthiness, not only by a `true` type: `if ($std)` on an object, `if
    (retObj())`, `if ($arr)` after `$arr = [1]`, `if ($one or $i)` on `1`,
    `if (1 xor 0)` and `if (function_exists('strlen'))` are
    `if.alwaysTrue` (and in an `elseif` or `while`: `elseif.alwaysTrue`,
    `while.alwaysTrue`, `while.alwaysFalse`), `!$std` on an object is
    `booleanNot.alwaysFalse`
    (in `if`, `elseif` and `?:`; `boolean-not*.php`, `ternary.php`,
    `elseif-condition.php`), and `!$b` after `if ($b) { return; }` is
    `booleanNot.alwaysTrue`, and on the falsy side `if ($zero)`, `if ($i
    and $zero)`, `if ($a xor $b)` on two objects and `if
    ($this->returnsVoid())` are `if.alwaysFalse` (13), `elseif ($zero)`
    `elseif.alwaysFalse` (9) and `!$zero` `booleanNot.alwaysTrue` (8), all
    unreported today (the rules oracle's 37
    PHPStan-only `if.alwaysTrue`: `if-condition.php`,
    `if-condition-not-phpdoc.php`, `boolean-logical-*.php`,
    `logical-xor.php`, `foreach.php`, `bug-6211.php`, `bug-8980.php`);
  - `instanceof` is reported on a subject already `*NEVER*` (a union an
    `elseif` chain exhausted: `if ($u instanceof Foo) {} elseif ($u
    instanceof Bar) {} elseif ($u instanceof Lorem)` on `Foo|Bar`; 12 of
    the rules oracle's 37 PHPStan-only `instanceof.alwaysFalse`,
    `impossible-instanceof.php`, `boolean-*.php`, `elseif-condition.php`,
    `if-condition.php`), and a `callable` is never an instance of a final
    class without `__invoke()`; both unreported today; where the `if` was
    `$x instanceof Lorem && $x instanceof Ipsum`, the `elseif`'s `$x
    instanceof Lorem` is reported `instanceof.alwaysTrue` on
    `Lorem&Ipsum` today, a wrong verdict (`boolean-and.php:66`,
    `boolean-logical-and.php:66`, `if-condition.php:61`);
  - `unset()` of an undefined variable
    (`unset.variable`), `unset()` of an offset on a non-array
    (`unset.offset`), and a statement with no effect for `new class {};`
    (`expr.resultUnused`) are unreported today;
  - a call to a method declared `@phpstan-impure`, abstract or on an
    interface has effects: `$x->impureFunc();`, `X::impureFunc();`,
    `self::fooStaticMethod();`
    of an interface and `$foo::doFoo();` are `staticMethod.resultUnused`
    today (8 rules-oracle findings,
    `call-to-static-method-without-impure-points.php`,
    `call-interface-methods.php`, `call-static-methods.php`),
    and an interface or abstract method marked `@phpstan-impure` is not
    `impureMethod.pure` (it has no body to judge; 7 findings,
    `bug-5984.php`, `bug-6702.php`, `strict-comparison.php`);
  - a `match` statement that may throw `UnhandledMatchError` has an effect
    and isn't `expr.resultUnused` (`last-match-arm-always-true.php:69`);
  - a statement that only reads is `expr.resultUnused` whatever else the
    read reports: `$foo->test;` on an unknown class or undefined property,
    `$mixed?->foo;`, `$null->foo;`, `$resource[42];`,
    `$arrayDoesNotExist['foo'];`, `isset($string->prop);`, `$this->foo;`
    are silent today where the read is otherwise reported or its receiver
    is `mixed` (about 45 rules-oracle findings, `access-properties.php`,
    `bug-6899.php`, `this.php`, `nullsafe-property-fetch*.php`,
    `nonexistent-offset.php`); `clone $p;` of a class without `__clone()`,
    `clone 'abc';` and a first-class callable made from a variable
    (`$f(...);`) are unreported too (the rules oracle's 168 PHPStan-only
    `expr.resultUnused` are these, `new class {};`, bare closure calls and
    `match` statements);
  - `new static(…);` as a statement is not `new.resultUnused`: a subclass's
    constructor may have effects (13 of the rules oracle's 19
    phpstan-rs-only `new.resultUnused`, in `instantiation.php` and
    `new-static.php`; the other 6, `new self()`/`new parent()` of a
    constructor with no side effects, stay reported, stricter than
    PHPStan);
  - a discarded `new` is `new.resultUnused` whenever constructing has no
    effect, where today only a user constructor without impure points is:
    a class without a constructor (`new NoCtor();`, `bug-4002_class.php`,
    `pure-method.php:389`), a built-in class through
    `functionMetadata.php` (`new \stdClass()`, `new \Exception('test')`,
    `new \PDOStatement()`, `new \SimpleXMLElement(…)`, `new
    \IntlDateFormatter(…)`), `new static()` in a final class, and a
    constructor whose only impurity is a pure callable argument
    (`pure-unless-callable-is-impure.php`), `new NoCtor(1)` included (PHP
    ignores the arguments; it is also `new.noConstructor`); most of the
    rules oracle's 40 PHPStan-only. Instantiating an abstract class or an
    interface throws, so it isn't also `new.resultUnused` as PHPStan
    reports it (approved);
  - a statement that can throw or runs code is not `expr.resultUnused`
    (`NoopRule` reads the expression's throw and impure points): `$a /
    $b;`, `$a % $b;` and `99 / $v;` (`DivisionByZeroError`), a `match`
    that can miss (`UnhandledMatchError`), `(string) $o;` (it calls
    `__toString()`), and a pipe (`1 |> 'f';`, `5 |> (fn ($x) => f($x));`,
    a call) are all reported today (the rules oracle's
    `noop-error-throws.php`, `bug-6349.php`, `bug-9534.php`,
    `match-expr.php`, `deprecated-cast.php`, the `*-pipe.php` files: 38
    findings); `static::$p;` stays reported, stricter than PHPStan;
  - `yield` outside a function is a compile error
    (`generator.outOfFunction`), unreported today;
  - PHP 8.5's `(void)` cast on a call without `#[\NoDiscard]` is
    pointless ("in (void) cast but function allows discarding return
    value"; `function.inVoidCast`, `method.inVoidCast`,
    `staticMethod.inVoidCast`, `callable.inVoidCast`, 10 in the rules
    oracle), and a discarded pure call as a bare closure call (`$c();`) is
    `expr.resultUnused`; both unreported today;
  - `self`/`static`/`parent` outside a class throw an `Error`:
    `self::class` in a function isn't `outOfClass.self` today, nor are
    `parent::LOREM`, `$foo instanceof parent` and `parent::$p` outside a
    class `outOfClass.parent` (3); `outOfClass.static` is reported, but
    not for `static::LOREM` and `static::$p` (2);
- (c) calls and member access:
  - a method call or property read on a plain `object` is
    `method.notFound`/`property.notFound` (it declares no members):
    `$o->getItem()` and `$o->x` on `object $o` are unreported today (the
    oracle finds 59 "undefined method/property object::"), and so is
    `$p->test()` on a template bounded by `object`
    (`php-82-dynamic-properties-allow.php`) or a static call `$o::doFoo()`
    on an `object` (`staticMethod.notFound`, `first-class-callables.php`,
    4);
  - a property fetch inside `isset()`, `empty()` and `??` checks its
    receiver as the offset checks do: on a `string` it is
    `property.nonObject`, the fetch never finding anything (`isset($s->p)`,
    `$s?->bar ?? 4`; `bug-6899.php`, `nullsafe-property-fetch.php`,
    `*-nullsafe-isset.php`, 10), and so it is on `mixed` at level 10
    (`isset($m->c->d)`; `bug-13539.php`, `bug-4559.php`, `bug-14393.php`,
    `isset.php`, 10), as is a property write on `mixed` (`$m->w = 1`, an
    `Error` on a non-object; `bug-9765.php`, `bug-2748.php`); all
    unreported today;
  - a missing property is `property.notFound` inside `isset()`, `empty()`
    and `??` too where nothing can declare it: on a final class or an
    object known exactly from `new` (`isset($final->dyn)`, `(new
    Bar())->zz ?? 'no'`; `dynamic-properties.php`, 9,
    `null-coalesce-new-is-always-final.php`); an object shape's optional
    property is `property.notFound` read or written outside `isset()`,
    and a missing one anywhere (`echo $o->bar` on `object{foo: int, bar?:
    string}`; `properties-object-shapes.php`, `bug-11609.php`,
    `bug-11424.php`, `isset-object-shapes.php`, 8); and `$o->{$name}` with
    a constant-string name resolves the property: over `foreach ($data as
    $property => $value)` on `array{a: int, noop: int}`, `$self->{$property}
    = $value` writes the undeclared `$noop`
    (`properties-from-*-into-*object.php`, 6); all unreported today;
  - a protected method declared in a common ancestor is accessible from a
    sibling class (PHP checks the prototype's declaring class): `$other->foo()`
    from a class sharing `Base` with `Other` is `method.protected` today
    (`sibling-method-prototype.php`); a `private(set)`/`protected(set)`
    property is writable anywhere in its class, through another instance
    too (`$self->asymmetricProperty = 1` is `assign.propertyReadOnly`
    today, `writing-to-read-only-properties.php`); a dynamic class
    constant name that is a union of string literals is a string
    (`self::{$name}` on `'FOO'|'BAR'` is `classConstant.nameType` today,
    `dynamic-constant-access.php`); a class constant read through
    `static::` can be overridden, so `if (static::FOO)` isn't
    `if.alwaysTrue` unless the constant or class is final
    (`if-condition.php:130`);
  - a call that returns by reference is a valid by-reference argument
    (`callReturnsByReference`): `useString($a->getString())` for `function
    &getString()`, `reset($this->getList())` for a by-reference
    `getList()`, are `argument.byRef` "expects variables only" today (5
    rules-oracle findings, `bug-757.php`, `bug-13711.php`,
    `bug-1501.php`);
  - a call to a method a class doesn't have is `method.notFound`, never
    `method.nonObject`: `(new stdClass())->foo()` is "call to method foo()
    on a non-object type stdClass" today (the rules oracle's
    `bug-12548.php`, `invalid-assign-var.php`);
  - a method call or property read on a possibly-null receiver is
    reported from level 8 (`findTypeToCheck`'s `checkNullables`), as a
    `null` receiver throws an `Error`: `$c->has()` and `$c->p` on `?C` are
    unreported today at every level, since `method.nonObject` and
    `property.nonObject` fire only for a receiver that can never be an
    object (161 `method.nonObject` and 110 `property.nonObject` "on
    X|null" the corpus oracle finds and phpstan-rs misses; nullable
    arguments and returns are already reported); once the receiver is
    reported, the call's arguments aren't checked against either member
    (`$this->foo->find(1)` on `self|null` is `arguments.count` today, the
    rules oracle's `call-methods.php:516`). A nullsafe link makes the rest
    of the chain possibly null: `$h->n()?->n()->m()` calls `m()` on
    `H|null` (`bug-5868.php`, `bug-4199.php`, `bug-7109.php`);
  - a method call on a receiver that can't be an object is reported for
    every receiver type: a template bounded by `mixed` (`$t->foo()` on `T
    of mixed`, `check-explicit-mixed.php`, `check-implicit-mixed.php`), an
    `int` in a first-class callable (`$i->doFoo(...)`), an always-`null`
    receiver of `?->` (`$null?->foo()`, `nullsafe-method-call*.php`,
    `bug-10729.php`), a `class-string` that `method_exists()` narrowed
    (`$s->nonExistent()`,
    `impossible-method-exists-on-generic-class-string.php`, 6), and a
    `global $pdo` variable, `mixed` at level 10 (`bug-14318.php`, 4), and
    a static call on `mixed`, `T of mixed` or an `int` (`$m::foo()`,
    `$i::doFoo(...)`; `staticMethod.nonObject`, 6); all unreported today;
  - a constant `define()`d in a static or non-constructor method isn't
    known in another method (only the constructor's scope is remembered,
    with the value's type; today even the constructor's is forgotten, so
    each read of `REMEMBERED_FOO` is `constant.notFound` and `mixed`,
    `remembered-constructor-scope.php:11`, `:18`, `:43`, `:67`):
    `echo XYZ22;` after `static function s() { define('XYZ22', 1); }` is
    `constant.notFound`, as is `__COMPILER_HALT_OFFSET__` in a file
    without `__halt_compiler()`; both unreported today
    (`remembered-constructor-scope.php`, 3,
    `compiler-halt-offset-const-not-defined.php`); a `use const` of a
    constant that doesn't exist is reported where the constant is read,
    not on the `use` (`group-uses.php:9`);
  - `filterVar.nullOnFailureAndThrowOnFailure` reads the flags through a
    named `options:` argument, a variable, `filter_input()` and the
    per-key options of `filter_var_array()`/`filter_input_array()`;
    only the positional form is reported today
    (`filter-functions-null-and-throw.php`, `filter_var_null_and_throw.php`,
    6);
  - `static::$p` of a private static property is
    `staticClassAccess.privateProperty` at every access; today one per
    line is, and none after an assignment to it in the same method
    (`bug-3777-static.php`, `bug-8333.php`, 10);
  - `__TRAIT__` outside a trait is `magicConstant.outOfTrait` in a
    parameter's or a property's default too (`string $trait =
    __TRAIT__`, `public $t = __TRAIT__;`), `__CLASS__` outside a class
    `magicConstant.outOfClass`, `__METHOD__`/`__FUNCTION__` in a property
    default `magicConstant.outOfFunction`, and `__NAMESPACE__` in the
    global namespace `magicConstant.outOfNamespace`, unreported today
    (`magic-constant*.php`, 6, 3, 2 and 1); the rule reads where it is from
    the scope (a closure, a function, a class, a trait), so
    `Checker::constant_facts`, a pre-walk of each file marking
    `__FUNCTION__`/`__METHOD__` inside closures, is deleted;
  - `null <= $object` is always true (`smallerOrEqual.alwaysTrue`,
    `bug-13001.php`), and enum cases compared with `<`, `>`, `<=` or `>=`
    are always false
    (`Foo::A > Foo::B`; `greater.alwaysFalse`, `smaller.alwaysFalse`,
    `greaterOrEqual.alwaysFalse`, `smallerOrEqual.alwaysFalse`;
    `bug-7052.php`), and a ternary as a statement is
    `ternary.resultUnused` (`isset($d['x']) ? 'foo' : 'bar';`,
    `access-properties.php:127`, `bug-7109.php`), both unreported today;
  - a parent's private constant read through a child (`self::PRIVATE_FOO`
    in `Bar extends P`, `$bar::PRIVATE_FOO`) is `classConstant.notFound`
    "undefined constant Bar::PRIVATE_FOO", as PHP reports it, not
    `classConstant.private` as today, and a constant some member of a
    union or intersection receiver lacks is `classConstant.notFound`
    (`$fooOrString::DOLOR`; `class-constant-visibility.php`,
    `class-constant.php:33`), a static property alike
    (`$foo::$nonexistent` on `Foo&SomeInterface`, `$class::$unknown` on
    `ClassOrString|string`, and `self::${$property}` over constant names;
    `staticProperty.notFound`, 3);
  - named arguments that leave a required parameter unset are
    `argument.missing`: `$f(i: 1)` for a two-parameter closure or a
    `callable(int, int)`, `implode(array: [''])` and `join(array: …)`,
    and `new \DatePeriod(…)` without `start` are unreported today (the
    rules oracle's 10 PHPStan-only with the `call_user_func` ones,
    `callables-named-arguments.php`, `implode-named-parameters.php`,
    `bug-7048.php`), and a name the callee lacks is `argument.unknown`
    (`$cb(i: 1)` on a `callable(int, int)`, whose parameters have no
    names, `$cb2(i: 1, j: 2, z: 3)`, `call_user_func($f, j: 1)`, `new
    \DatePeriod(isostr: …)`; 5);
  - a private property read from outside is `property.private` under
    `??` too (`$foo->id ?? 2` always gives `2`, PHP reading it as unset;
    `bug-12645.php`), and PHP 8.5's `clone($foo, ['priv' => …, 'prot' =>
    …])` from outside is `property.private`/`property.protected`
    (`clone-with.php`, `*-clone-with.php`, 4 each); both unreported today;
  - a dynamic class constant name must be a string: `self::{$mixed}`,
    `parent::{$mixed}` and `$obj::{$mixed}` are `classConstant.nameType`
    at level 10, unreported today (`dynamic-constant-stringable-access.php`,
    5);
  - invoking a value that isn't callable is `callable.nonCallable`: a
    constant string naming no function (`$n = 'nonexistent'; $n();`,
    `''`), `$i(...)` on `1`, `5 |> 'doFoo'` with no function `doFoo`, and
    a constant array whose class or method union has a member that can't
    be called (`[CU::class, 'doFoo'|'doBaz']`, `callables.php:188`-`:229`,
    `maybe-not-callable.php`), all unreported today (with (b)'s
    might-not-be and `mixed` rows, the rules oracle's 25 PHPStan-only);
  - a by-reference argument PHP creates on the call is not type-checked
    against the parameter;
  - flag parameters are checked against PHPStan's
    `constantToFunctionParameterMap.php`: a constant the parameter doesn't
    accept (`RecursiveIteratorIterator::CHILD_FIRST` as its `$flags`,
    `argument.invalidConstant`), constants that can't be combined
    (`LOCK_EX | LOCK_SH`, `argument.exclusiveConstants`) and `|` where the
    parameter takes one constant (`array_unique($a, SORT_STRING |
    SORT_NUMERIC)`, `argument.bitmaskNotAllowed`), unreported today (33
    rules-oracle findings), at every call form: functions, constructors
    (`new \finfo(SORT_REGULAR)`), methods and static methods
    (`PDOStatement::fetch()`, `IntlDateFormatter::create()`), named
    arguments (`json_encode('{}', flags: SORT_REGULAR)`), and a closure or
    `call_user_func()` callable that forwards to a mapped function;
  - `new DateTime('2020.11.17')` and `new DateTimeImmutable('asdfasdf')`
    with a literal PHP rejects are `new.dateTime`/`new.dateTimeImmutable`:
    the rule exists (`date_time.rs`, E2's date parser) and misses both
    today;
  - named arguments inside a `PHP_VERSION_ID < 80000` branch are
    `argument.namedNotSupported` once D5 narrows the version there;
  - a static closure passed where `@param-closure-this` needs a bindable
    one (`argument.staticClosure`), a `void` closure's result used or a
    callable's result discarded (`callable.void`,
    `callable.resultDiscarded`), and a call whose return or argument type
    contains an unresolvable type (`method.unresolvableReturnType`,
    `argument.unresolvableType`) are unreported today;
  - `@no-named-arguments` forbids named arguments and string-keyed
    unpacking: `nn(b: 1, a: 2)` for a `@no-named-arguments` function isn't
    `argument.named` today (11 oracle findings, PHPUnit's
    `Generator::testDouble()` among them; the rules oracle's 10, a named
    argument through `call_user_func()` and an unpacked array with a
    string or possibly-string key included);
  - a `void` call's result is reported where it is used (`method.void`,
    `staticMethod.void`, `function.void`): `$x = $o->v();` for `v(): void`
    isn't reported today (54 oracle findings, e.g. PSR-3
    `LoggerInterface::debug()` results). Every use counts: an argument, a
    `foreach` iteratee, an `if`, `switch`/`case`, `match` subject or arm
    condition, a ternary or `&&`/`||` operand, `??=`/`.=`, a
    destructuring source, a pipe's left side or result, and the result of
    `call_user_func()`/`call_user_func_array()` (reported for the function
    and the callable), `Closure::call()` and an invoked closure or string
    callable (`callable.void`); the rules oracle's 26, 19 and 4
    (`call-methods.php:66`-`:91`, `wrong-variable-name-var*.php`,
    `*-call-pipe.php`, `match-expr-void-used.php`, `call-user-func*.php`);
  - a discarded pure call is reported for built-ins too
    (`function.resultUnused`, `staticMethod.resultUnused`): the rule
    exists (`noop.rs`) but reads purity, and a built-in has none until C5
    embeds `functionMetadata.php`, so `strlen($s);` and `E::tryFrom($v);`
    on their own line are unreported today (a `@phpstan-pure` user method
    already is). User functions are read by their resolved name through
    the project-wide purity summaries (C9): a function in a namespace, or
    declared in another file, is never reported today, even one marked
    `@phpstan-pure` (`N\g();` for `function g(): int { return 1; }` is
    silent where the global-namespace `g();` in the same file is
    reported); a first-class callable made as a statement (`foo(...);`,
    `strlen(...);`, `$this->doFoo(...);`, `Foo::doFoo(...);`) is reported
    too, as are a nullsafe call (`$e?->getMessage();`), a pipe into a pure
    callable (`5 |> $foo->doPure(...);`) and the built-in methods
    `functionMetadata` lists (`Closure::bind()` 20, `Closure::bindTo()`,
    `finfo::file()`, `ImagickPixel::getColor()`, `Throwable::getCode()`:
    with the `new` exactness in D3 these are the rules oracle's 109
    `method.resultUnused` and 61 `staticMethod.resultUnused` PHPStan-only
    findings). The rules oracle's 518 PHPStan-only
    `function.resultUnused` are these three shapes: 409 built-ins, 109
    namespaced or cross-file user functions (`missing-return.php`,
    `function-with-variadic-parameters.php`, `bug-15168.php`,
    `first-class-callable-function-without-side-effect.php` and the
    rest);
  - arrays passed where PHP casts their values to string are checked for
    castable values (`ParameterCastableToStringRule` and its sort
    variant): `array_fill_keys([E::A], …)`, `array_diff`/`array_intersect`
    and their `_assoc` forms, `array_combine`, `array_count_values`,
    `array_unique`, `natsort`/`natcasesort`, `sort`/`rsort`/`asort`/
    `arsort` of enum cases or arrays, and `implode`/`join` of such an
    array, are unreported today (about 35 rules-oracle findings,
    `param-castable-to-string-functions*.php`,
    `sort-param-castable-to-string-functions-enum.php`, `bug-11111.php`,
    `bug-11141.php`, `bug-3946.php`); phpstan-rs checks the
    castable-to-number variant already;
  - `random_int($min, $max)` whose ranges can't satisfy `$min <= $max` is
    reported (`RandomIntParametersRule`; `random-int.php`, `bug-14468.php`:
    8 findings, unreported today);
  - a template the call can't resolve is `argument.templateType`:
    `make($s)` for `@param class-string<T> $c` with a plain `string` is
    unreported today (17 oracle findings), nor are a first-class callable
    or closure of a generic function called with `mixed` (`$f($mixed)`)
    and `f(1, 2)` binding `A` two incompatible ways (the rules oracle's 9,
    `call-first-class-callables.php`, `call-generic-function.php`).
- (d) assignments and properties:
  - `uninitialized` and `uninitialized_dataflow` are rewritten over the
    ClassPropertiesNode initialization state, with the dataflow walker
    deleted;
  - a write to an undeclared property is `property.notFound` as a read
    is: `$r->missing = 1` is unreported today on any class, though PHP 8.2
    deprecates creating a dynamic property and a readonly class throws;
  - a typed static property checks its assigned value: `R::$s = 'q'` on a
    `static int` is unreported today (PHP throws a `TypeError`);
  - every write form is checked against the property's type
    (`TypesAssignedToPropertiesRule` over each `PropertyAssignNode`, which
    carries the whole new value): an append or
    offset write (`$this->ints[] = 'foo'` on `array<int>`,
    `$this->byStr[$n] = 1` on `array<string, int>`, `$this->d['age'] =
    'x'` on a shape, nested `$foo->bar['a']['b'][] = 'x'`), a
    by-reference array function (`array_push($this->ints, 'x')`,
    `array_shift($this->q)` on a `non-empty-list`), a compound assignment
    (`$this->i .= 'test'` on `int`, `$a->has |= 999` on `int<0, 1>`) and
    an increment past a range (`$this->r++` on `int<min, 3>`) are all
    unreported today (most of the rules oracle's 138 PHPStan-only
    `assign.propertyType`: `appended-array-item.php`,
    `appended-array-key.php`, `bug-2888.php`, `bug-3703.php`,
    `bug-6286.php`, `bug-6356b.php`, `bug-13438*.php`, `bug-7912.php`,
    `properties-assigned-types.php`); the rest are the level-10 implicit
    `mixed` (31) and the generic and template rows D3 and B4 name;
  - `unset()` of a readonly property is `unset.readOnlyProperty` and of a
    hooked one `unset.hookedProperty` (PHP throws an `Error` for both), and
    of a non-final class's property `unset.possiblyHookedProperty` (a
    subclass may hook it); all three are unreported today, as is
    `unset.readOnlyPropertyByPhpDoc` for a `@readonly` one (and every
    property of a `@readonly` or `@immutable` class; `bug-12421.php`, 3);
    a `@readonly` property's own set hook writes it without
    `property.readOnlyByPhpDocAssignNotInConstructor`
    (`property-hooks-readonly-by-phpdoc-assign.php:18`);
  - a property that is only appended to is write-only:
    `$this->only[] = $v` with no read isn't `property.onlyWritten` today,
    where a plain assignment is; a read through an alias of `$this` counts
    as a read: `$that = $this; function () use ($that) { return
    $that->p; }` leaves `$p` `property.onlyWritten` today, a false
    positive. Only a real read counts: not a read that only feeds the
    property back through its own class's constructor (`new
    self($this->test)`, `new static(…)`, named arguments included;
    `bug-14573.php`, 9, `bug-13440.php`), a read in the property's own
    hook (`property-hooks-unused-property.php`,
    `private-final-property-hooks.php`), or a dynamic read whose name
    can't be the property (`$this->$name` over `['foo', 'bar']` leaves
    `$baz` write-only, and so does `$x->{$y}` after `$y !== 'isFinal'`;
    `unused-private-property.php:117`, `bug-11802.php`), where today any
    dynamic read counts for every property. The writes that count include
    an offset write
    (`$this->tuple[$i] = false`, `bug-3339.php`) and a static write
    through a constant dynamic name (`self::${$name} = …`,
    `bug-13093c.php`, `bug-13093d.php`);
  - every write form counts as a write for `property.onlyRead`/
    `property.neverWritten` (`PropertyWrite` through `PropertyAssignNode`
    and the by-reference and foreach paths): an offset write, append or
    `unset()` of an offset (`$this->p[] = 4`, `$this->p['k'] = 5`,
    `unset($this->p[$k])`), an increment (`$this->bar++`), a by-reference
    argument (`foo($this->p)`, `exec($c, $this->out)`, `proc_open(…,
    $this->pipes)`, `uksort($this->keys, …)`), a reference (`$x =
    &$this->p`), a `foreach` target (`foreach ($c as $this->k =>
    $this->v)`), destructuring (`[$this->foo] = [1]`) and a chained static
    write (`self::$p::$p = …`) all leave the property "never written, only
    read" today (the rules oracle's 64 `property.onlyRead`, among them
    `appended-array-item.php`, `appended-array-key.php`, `bug-9213.php`,
    `bug-7119.php`, `uksort.php`, `property-type-after-unset.php`,
    `properties-assigned-types.php`; Appendix I's `property-write-forms`
    has 24 more in 13 files, `unused-private-property.php` 6);
  - a property that can't be written is `assign.propertyReadOnly` for
    every kind: an enum's `name` and `value` (`$this->name = 'ONE'`, an
    `Error`), an object shape's property (`$o->foo = 1` on `object{foo:
    int}`), a `@property-read` tag reached from another class, and a
    readonly property in PHP 8.5's `clone($f, ['i' => …])` from outside
    (and a `protected(set)` one, `assign.propertyProtectedSet`,
    `bug-14063.php`, `clone-with.php`, 5);
    unreported today (the rules oracle's 11 PHPStan-only,
    `readonly-assign.php`, `properties-object-shapes.php`,
    `writing-to-read-only-properties.php`, `bug-11241.php`,
    `writing-to-readonly-properties-clone-with.php`);
  - a readonly property assigned twice is `assign.readOnlyProperty`
    "already assigned" ("Cannot modify readonly property" at runtime): two
    assignments in the constructor, a child's promoted redeclaration
    followed by `parent::__construct($myProp)`, and an append after the
    assignment; unreported today (the rules oracle's 12 PHPStan-only,
    `missing-readonly-property-assign*.php`, `redeclare-readonly-property.php`,
    `bug-8101.php`, `bug-9863.php`, `bug-13856.php`);
  - `$this` is written by every write form, each `assign.this` (a compile
    error): a `foreach` key, value or by-reference target, `.=` and `??=`,
    destructuring and a `catch` variable, and an offset write on a class
    without `ArrayAccess` (an `Error` at runtime rather than at compile
    time); only a plain `$this = …` is reported today (the
    rules oracle's 12 PHPStan-only, `bug-3585.php`, `bug-14349.php`,
    `bug-14351.php`, `bug-3782.php`, `bug-14352.php`);
  - a readonly property is modified by every write form, so outside the
    constructor each is `property.readOnlyAssignNotInConstructor` ("Cannot
    indirectly modify readonly property" at runtime): an offset write or
    `unset()` of an offset, `++`/`--`, a compound assignment,
    destructuring, a set hook writing it, and a by-reference argument
    (also `argument.byRef`); only a plain `=` is reported today (the rules
    oracle's 16 PHPStan-only, `readonly-assign.php`, `bug-6773.php`,
    `bug-7198.php`, `readonly-property-hooks.php`,
    `readonly-property-passed-by-reference.php`,
    `unset-hooked-property.php`; `ksort($this->a)` on a `@readonly` one
    is `argument.byRef` too, `bug-12676.php`), and a `@readonly` one alike
    (`property.readOnlyByPhpDocAssignNotInConstructor`, 7,
    `readonly-assign-phpdoc.php`); taking a reference to one (`$a =
    &$this->a`) is `property.readOnlyAssignByRef`/
    `property.readOnlyByPhpDocAssignByRef`, unreported today
    (`readonly-assign-ref-phpdoc.php`, 6);
  - property hooks: the checks that a property overriding a writable or
    readable one stays so (`property.notWritable`, `property.notReadable`)
    and that
    a get hook of a non-virtual property reads its value and a set hook
    assigns it (`propertyGetHook.noRead`, `propertySetHook.noAssign`)
    exist and miss the rules oracle's cases (an interface property
    prototype, a hook inherited through a parent:
    `property-prototype-from-interface.php`, `bug-14457.php`,
    `set-non-virtual-property-hook-assign.php`), and report a short set
    hook (`set => $value + 1`, which assigns its expression) as
    `propertySetHook.noAssign` (`get-non-virtual-property-hook-read.php`);
    a `private(set)`/`protected(set)` static property assigned from outside
    (`assign.staticPropertyPrivateSet`, `assign.staticPropertyProtectedSet`)
    and a `@readonly`
    property assigned twice (`assign.readOnlyPropertyByPhpDoc`) are
    unreported today; a get-only virtual property may narrow the
    overridden type (`int` over `int|string`, as PHP allows), which is
    `property.nativeType` "not the same" today, a false positive, while
    widening it is `property.covariant` and widening a set-only one
    `property.contravariant` (`bug-12466.php`,
    `property-prototype-from-interface.php`); a `@readonly` property with a
    default value is `property.readOnlyByPhpDocDefaultValue`, and so is
    every property of an `@immutable` class and of its subclasses, which
    are `@readonly` by inheritance; only a property tagged `@readonly`
    itself is reported today (`read-only-property-phpdoc.php`, 3);
  - a readonly property, native or `@readonly`, read before its
    initialization is reported
    (`property.uninitializedReadonly`; PHP throws an `Error`): `echo
    $this->x; $this->x = 1;` in a constructor isn't reported today (12
    oracle findings in revolt's event loop drivers). A readonly property
    the constructor doesn't assign on every path is reported on the class
    ("Class A has an uninitialized readonly property $x. Assign it in the
    constructor."; `property.uninitializedReadonly`, and
    `property.uninitializedReadonlyByPhpDoc` for `@readonly`): a class
    without a constructor, one that assigns it on some paths only, and the
    properties of an anonymous class, a readonly class, a trait's host and
    a child whose parent constructor isn't called are unreported today;
    the rule doesn't exist (all 84 and 30 of the rules oracle's
    findings, `property-variance-readonly.php`,
    `missing-readonly-property-assign*.php`, `readonly-assign*.php`,
    `readonly-property-hooks.php`, `clone-with.php`, `bug-9863.php`);
  - an offset write into an `ArrayAccess` object checks the key against
    `offsetSet`'s key type: `$w['str'] = 1` on `WeakMap<stdClass, int>`
    and `$s[5] = 1` on `SplObjectStorage` are unreported today
    (`offsetAssign.dimType`), as are `'a'` on an `ArrayAccess<int, int>`,
    an array key on `SplObjectStorage`, and `$a[] = 5` after the key
    `PHP_INT_MAX` (no next key; `offset-access-assignment-php-int-max.php`);
    and the value against its value type (`offsetAssign.valueType`):
    `$aa[] = 'foo'` and `$aa[1] += 2.5` on `ArrayAccess<int, int>`, `$c[]
    = 1` on a class declaring `@implements \ArrayAccess<int, string>`,
    `$a['foo'] ??= $a['foo'] ?? null` on `ArrayAccess<string, string>`,
    and writes into an `ArrayObject<never, never>` (and `unset()` of such
    an offset, `unset.offset`, 7) are unreported today
    (all 20 of the rules oracle's PHPStan-only
    `offset-access-value-assignment*.php`,
    `null-coalesce-assign-right-side-scope.php`, `readonly-assign.php`);
  - an offset write or append checks its container as a read does:
    `$i['x'] = 3` on an `int` throws at runtime and isn't reported today,
    and at level 10 neither is `$m['k'] = 1` or `$m[] = 2` on `mixed`
    (383 oracle findings). A read inside `isset()`/`??`/`empty()` and an
    `unset()` are checked too: they are quiet on a scalar or `null`, but
    an object without `ArrayAccess` throws "Cannot use object of type
    stdClass as array" there (`isset($o['a'])`, `$closure['a'] ?? 1`,
    `unset($iterable['s'])` on an iterable that may be a `Traversable`),
    so those, and a `mixed` container at level 10, are reported (341
    PHPStan reports on `mixed`; about 60 of the rules oracle's
    PHPStan-only `offsetAccess.nonOffsetAccessible`). Also unreported
    today: `$this[$k]` in a class without `ArrayAccess` (`bug-3782.php`,
    `bug-14352.php`), `(new parent())[0]`, an offset read on a `resource`,
    and a container typed by a template bounded by `mixed`
    (`offset-access-mixed.php`).
- (e) declarations, generics checks and PHPDoc:
  - class existence is checked everywhere PHPStan checks it (the rules
    oracle's 173 PHPStan-only `class.notFound`): at use sites through
    `find_type_to_check`'s unknown-class errors ("Call to method doFoo()
    on an unknown class", "Throwing object of an unknown class",
    "Access to property/offset/constant on an unknown class", "Iterating
    over", "Cloning", "Invoking callable on", "Creating callable from" an
    unknown class: about 90, the declaration being reported today but no
    use); `Unk::class`; `new $a()` for a constant-string `$a`; a template's
    bound and default (`@template T of Zazzzu`, on classes, interfaces,
    traits, functions, methods, `@method` tags and callable types);
    `@param-out`, `@phpstan-self-out`, a conditional return type's
    branches, generic arguments of a property type (`Foo<Barrrr>` and
    `WeakMap<stdClass, …>` in a namespace; each class named, not only the
    first), a `@var` naming `$this`, and both the parameter and the
    property of a promoted parameter, and a trait named as a type in
    `@param-out` or `@param-closure-this` (`parameter.trait`,
    `param-out-classes.php`, `param-closure-this-classes.php`, 4); each is
    unreported today (a
    property hook's types and a doc block's several `@var` tags already
    are); a known class named with the wrong case is `class.nameCase` at
    the same sites (`FOO::class`, a native `POST` parameter and return,
    `@param-out fOO`, `@param-closure-this fOO`, `@phpstan-self-out fOO`,
    a template bound `baz`, and for an interface `interface.nameCase` at an
    `instanceof` and a `use` import, grouped ones included; the rules
    oracle's 13 and 3 PHPStan-only);
  - `by-ref parameter's declared type is never narrower than 'foo'`
    (`parameterByRef.tooWide`) becomes PHPStan's
    `TooWideParameterOutTypeCheck`: a union member the function never
    assigns is reported, not a missing narrowing to a literal (10
    rules-oracle findings, `too-wide-*-parameter-out.php`), as
    `parameterByRef.unusedType`, or `paramOut.unusedType` for a
    `@param-out` type, where it is `parameterByRef.tooWide` today;
  - an empty array assigned under any array `@var` is widening, not
    `varTag.type`: `/** @var array{string, int} $a */ $a = [];` is
    reported today (`var-above-empty-array-widening.php`,
    `bug-5356.php`);
  - a generic argument built from a template is checked through its
    bound: `EnumAsFilterInterface<(int|string)&TOperation>` for
    `TOperation of scalar`, a conditional `(TKey is object ? TKey :
    Box<TKey>)` and `FooBoxedArray<T>` inside the class's own `T` are
    `generics.notSubtype` today (8 rules-oracle findings,
    `bug-11776.php`, `bug-10622b.php`);
  - a template referenced inside a callable parameter's signature is in a
    parameter: `@param callable(T, K): iterable<L, U> $fn` references `L`,
    `method.templateTypeNotInParameter` today (7 rules-oracle findings,
    `bug-10297.php`, `bug-8074.php`, `bug-13247.php`, `yield.php`);
  - an assertion of a template type narrows (`@phpstan-assert T $value` on
    `mixed` is `assert.alreadyNarrowedType` today,
    `assert-unresolved-generic.php`, `bug-10573.php`), and so does an
    array assertion over `mixed` values: `@phpstan-assert string[] $arr`
    on a `mixed[]` parameter is `assert.alreadyNarrowedType` "does not
    narrow down the type" today and leaves the argument `array<mixed>`
    (nsrt `assert-docblock.php`, Appendix J's `assert-array-mixed`);
  - a covariant template may type a property that can't be written from
    outside after construction (readonly, `private(set)`, a class marked
    `@immutable`/`@readonly`): `public $value` typed `Value` in an
    `@immutable` class is `generics.variance` "input (parameter) position"
    today, and the message names a property, not a parameter
    (`bug-13049.php`, `bug-9153.php`, `bug-12964.php`);
  - class structure errors PHP rejects at compile time: `parent::` in a
    class without a parent (`class.noParent`; `class-constant-visibility.php`,
    `bug-12272.php`, `access-static-properties.php`, 3), `implements
    \iterable` (not an interface; `interface.notFound`, `bug-8889.php`, 2),
    and an unknown `@phpstan-*` tag above a statement or on a hook
    (`phpDoc.phpstanTag`, `invalid-phpstan-doc*.php`, 3); all unreported
    today;
  - a `@template` named like an existing class is
    `generics.existingClass` wherever templates are declared: a class
    template named after the class itself (`bug-10049.php`), an anonymous
    class's (`new /** @template stdClass */ class`) and a callable
    type's (`callable<\stdClass of mixed>(T): T`); reported today only on
    functions and methods (3);
  - the rules oracle's remaining one- and two-finding gaps, each
    unreported today: a `private(set)` property is implicitly final (PHP
    8.4), so redeclaring it is `property.parentPropertyFinal`
    (`overriding-final-property.php:31`, `:33`); `print_r(return: false,
    value: …)` in an expression is `function.uselessReturnValue` as the
    positional form is (`useless-fn-return-php8.php`); `@param-out` on a
    set hook's `$value` is `parameter.notByRef` as on a function's
    by-value parameter (`incompatible-property-hook-phpdoc-types.php`);
    `array != int` is `notEqual.alwaysTrue` beside `notEqual.invalid`
    (`invalid-comparison.php`, `bug-3364.php`); an `include`/`require` of
    a stream-wrapper path PHP can't open (`vfs://…` unregistered,
    `not-a-registered-wrapper://…`) is
    `include.fileNotFound`/`includeOnce.fileNotFound`/`require.fileNotFound`
    (`require-file-stream-wrapper.php`); a nullsafe returned from a `&get`
    hook is `nullsafe.byRef` as from a by-reference function
    (`return-null-safe-by-ref-property-hooks.php`); a constant overriding
    a built-in's with a wider native type is `classConstant.nativeType`
    (`const int|string NONE` in a `Phar` subclass,
    `overriding-constant-native-types.php:48`); a
    `@phpstan-require-extends` union naming an unknown class still checks
    the known member (`class.missingExtends`,
    `incompatible-require-extends.php:185`-`:198`); `self::doBar(...)` of an
    instance method from a static method (`method.staticCall`),
    `Ab::doBaz(...)` of an abstract
    static method (`staticMethod.callToAbstract`), a trait-typed promoted
    parameter as a property too (`property.trait`), `$x = yield 1` under a
    `void` send type (`generator.void`), an enum case whose value is a
    constant of the wrong type (`case Two = self::S;` with a string
    `S`, `enum.caseType`; the right type isn't: `case TESTING =
    self::PREFIX . 'test'` on a string enum is `enum.caseType` "value
    string does not match the \"string\" type" today, `bug-9005.php`),
    `new $bar` of a constant class-string naming an
    interface or abstract class (`new.interface`, `new.abstract`), an
    append to a `private(set)` array from outside
    (`assign.propertyPrivateSet`), `array_filter()` without a callback on
    an array with no falsy values (`arrayFilter.same`), `$a |> $cb` into a
    by-reference parameter (`pipe.byRef`), `BcMath\Number` compared with a
    float (`greater.invalid`, `spaceship.invalid`; compared `==` with an
    object it is `false` at runtime, not `equal.invalid` as today,
    `bug-13001.php`), a constant overriding
    a built-in typed one without the native type (`const BZ2` in a `Phar`
    subclass, `classConstant.missingNativeType`), `do {} while ($arr =
    [1])` (`doWhile.alwaysTrue`), `@phpstan-assert … $this`
    on a static method (`parameter.notFound`), an interface binding one
    ancestor's template two ways through two parents (`TValue` as `Item`
    via `Traversable` and as `string` via `IteratorAggregate`;
    `generics.interfaceConflict`), and a `catch (\Exception $p)` writing a
    by-reference `int &$p` (`parameterByRef.type`);
  - a template's variance is checked through composition, each parameter
    on its own (`VarianceCheck`, `TemplateTypeVariance::compose`): a
    covariant `X` in `Out<X>`, `In<Out<X>>`, `Inv<X>` (an invariant
    position), a closure's parameter (`\Closure(X): X`) or a `@param-out`
    type is `generics.variance`; today only a bare `X` parameter is
    reported, once per method (the rules oracle's 18 PHPStan-only,
    `method-signature-variance-*.php`, `bug-10609.php`), and on private
    methods too, which PHPStan skips as they can't be called through the
    parent type (`method-signature-variance-*.php:74`);
  - imports are per namespace block: repeating `use X\Y;` in a second
    `namespace N;` block, and `use \stdClass;` in the global namespace
    (a PHP warning, "no effect"), are `use.nameInUse` today (7 findings,
    `repeated-namespaces.php`, `repeated-braced-namespaces.php`,
    `bug-15215.php`);
  - an override may put optional parameters before the variadic one:
    `translate($message, $lang = 'cs', string ...$parameters)` over
    `translate($message, ...$parameters)` is `parameter.notVariadic`
    today (`overriding-variadics.php`), and a variadic may absorb the
    parent's parameters: `doFoo(int ...$everything)` over `doFoo(int $a,
    int $b)` is `parameter.missing` today, a false positive
    (`less-parameters-variadics.php`, where `int ...$everything` over
    `doFoo(int $many, string $parameters, string $here)` is
    `method.childParameterType` against each `string` parameter);
  - an override that adds a parameter must make it optional (PHP rejects
    `doBar(int $i)` over `doBar()` and `doFoo(int $i, int $j)` over
    `doFoo(int $i)`), and so must a child constructor under
    `@phpstan-consistent-constructor`; `parameter.notOptional`,
    unreported today (`overriding-method.php:205`, `:225`,
    `consistent-constructor.php`), and a required parameter before an
    inherited variadic is `parameter.notOptional`, where phpstan-rs says
    `parameter.notVariadic` today (`overriding-variadics.php`);
  - a parameter is checked for contravariance against a trait's abstract
    method (private ones included), a built-in parent's native signature
    (`offsetExists(int $index)` in an `SplFixedArray` subclass, a compile
    error) and an abstract constructor (`overriding-method.php:110`),
    unreported today; with the PHPDoc-only conflicts of the level table
    these are the rules oracle's 12 PHPStan-only
    `method.childParameterType`;
  - `new static(…)` is safe under a final constructor (`final private
    function __construct`) as under a final class or
    `@phpstan-consistent-constructor`; `new.static` today (`bug-4396.php`,
    `bug-2861.php`);
  - a conditional return on a template is decided by its whole bound:
    `(T is object ? … : …)` under `T of TKey|TValue`, each `of
    object|array<mixed>`, is only maybe true, `conditionalType.alwaysTrue`
    today (`bug-10622b.php`), as is `(int is int ? …)` shadowing a
    parameter (`callable-type-predicate-rule.php`); a bound inside the
    tested type makes it always true and a disjoint one always false: `(T
    is int ? …)` under `T of int` is `conditionalType.alwaysTrue` and `(T
    is string ? …)` under `T of int` `conditionalType.alwaysFalse`, both
    unreported today (`*-conditional-return-type.php`, 4 each);
  - `@var` above `global $x;` and `static $x = 1;` sets the variable's
    type; it is `varTag.nativeType` against the default today
    (`bug-4500.php`, `invalid-var-tag-type.php`). A `@var` is checked
    against the native type wherever it applies: a nameless one above
    `return` (`/** @var list<string> */ return [1 => 'b', 2 => 'c'];`,
    `bug-12708.php`, 4), above `??=` and `.=` of a `void` call
    (`wrong-variable-name-var-assign-op.php`), and above a closure
    (`bug-11535.php`), all unreported today; a named one above a
    destructuring whose native type decides is `varTag.nativeType`, not
    `varTag.type` (`wrong-var-native-type.php:109`);
  - `#[\Override]` on a constructor is `method.override` (PHP: "no
    matching parent method exists", constructors don't override;
    `override-attribute.php:42`), unreported today;
  - `#[\Override]` on a trait member is checked in each host: a trait
    property overriding the host's parent's is `property.override` "no
    matching parent property" today (`property-override-attr.php`);
  - a conditional return type may test `$this` (`($this is self::This ?
    'here' : 'there')` on an enum) and a callable parameter's own
    parameter; both are `parameter.notFound` "references unknown
    parameter" today (`bug-11939.php`, `method-conditional-return-type.php`,
    `callable-type-predicate-rule.php`); an assertion may name a method
    call (`@phpstan-assert-if-false non-empty-list<static>
    $this->getValues()`), `assert.unknownExpr` today (and
    `assert.impossibleType` against the method's return type)
    (`incompatible-assert-type-with-method-return-type.php`); a `@var`
    naming a variable the assignment's right side reads types that
    variable (`/** @var int $foo */ $bar = $foo + 1;`) at a file's top
    level, not `varTag.differentVariable` (`bug-3515.php`, 4 findings);
    inside a function it is `varTag.differentVariable` and the read is
    `variable.undefined`, unreported today (`bug-3515.php:19`, `:20`);
  - `static` in `@phpstan-assert static $object` resolves to the called
    class; `assert.unresolvableType` today (`bug-12548.php`);
  - `@param-closure-this` is `paramClosureThis.nonClosure` only where the
    native type can't be a `Closure` (`IncompatiblePhpDocTypeCheck`): on a
    `callable $cb` it is reported today (15 rules-oracle findings);
  - a template bound is supported when `TemplateTypeCheck` lists its type:
    `self` (an object type) and `key-of<…>` are, and are
    `generics.notSupportedBound` today (15 rules-oracle findings,
    `bug-4550.php`, `bug-7652.php`, `bug-13652.php`, `bug-10749.php`,
    `method-assert.php`, `param-out.php`);
  - a PHPDoc type resolving to `never` (`int&string`, a template whose
    bound makes it empty) is `return.unresolvableType`/
    `parameter.unresolvableType`/`assert.unresolvableType`, unreported
    today (32 oracle findings), and so is one that can't intersect
    (`array{foo: int}&array{bar: string}`), a set hook's `@param`, a
    `@param-closure-this` of a non-object (the rules oracle's 7
    PHPStan-only, `bug-3753.php`, `explain-unresolvable-method-parameter.php`,
    `param-closure-this.php`, `incompatible-types.php:126`), and a
    property's `@var never` or `class-string<int>`
    (`property.unresolvableType`, promoted ones included, 4), and an
    inline `@var self&\stdClass` inside a class
    (`varTag.unresolvableType`, `invalid-var-tag-type.php`, 3); a parameter
    whose PHPDoc type doesn't resolve still has one, so `@param
    value-of<T> $v` on an untyped `$v` is not also `missingType.parameter`
    (the rules oracle's `unresolvable-parameter.php`);
  - a constant's `@var` is checked against its native type (`@var string`
    on `const int BAZ` is `classConstant.phpDocType`), where today the
    value is checked against the `@var` instead (`classConstant.value`;
    `incompatible-class-constant-phpdoc-native-type.php`,
    `value-assigned-to-class-constant*.php`, 4);
  - the type-alias checks: a cycle (`@phpstan-type A = B`, `B = A`) is
    `typeAlias.circular`, and `class.notFound` today; an alias resolving
    to `never` (`int&string`) is `typeAlias.unresolvableType`; an invalid
    alias name (`123bad`, or `int`: `typeAlias.invalidName`) and an
    `@phpstan-import-type` of an alias the class doesn't define
    (`typeAlias.notFound`) are reported, as is an alias whose definition
    doesn't parse (`typeAlias.invalidType`); all unreported today (the
    rules oracle's 10, 4, 2 and 2);
  - `@method`, `@property` and `@mixin` tags and a constant's `@var` with
    an unresolvable type are `methodTag.unresolvableType` (a parameter or
    its default), `propertyTag.unresolvableType`, `mixin.unresolvableType`
    and `classConstant.unresolvableType`, unreported today (`method-tag*.php`,
    `property-tag.php`, `mixin*.php`, `incompatible-class-constant-phpdoc.php`;
    8 in all);
  - a class constant named `class` is `classConstant.class`, a compile
    error (`class-as-class-constant.php`, 2), and `$string::class` on a
    string `classConstant.dynamicString` ("not supported in PHP",
    `class-constant-on-expr.php`), both unreported today;
  - `@mixin int` is `mixin.nonObject`, unreported today;
  - an interface, trait or enum is never an attribute class, even marked
    `#[\Attribute]` (PHP rejects the mark): using one is
    `attribute.notAttribute` (`class-attributes.php`,
    `enum-attributes.php`, 3; a trait is `attribute.trait` today);
  - a PHPDoc tag a trait can't carry is reported: `@phpstan-self-out`,
    `@phpstan-sealed`, `@mixin`, `@method`, `@property` and
    `@phpstan-type` on a trait (`selfOut.trait`, `sealed.onTrait`,
    `mixin.trait`, `methodTag.trait`, `propertyTag.trait`,
    `typeAlias.trait`), unreported today, and a type resolving to `never`
    in `@mixin`, `@method`, `@property` or a class constant's `@var` is
    `*.unresolvableType` as in parameters and returns;
  - the purity checks cover property hooks (`impure.propertyHookCall`,
    `purePropertyHook.void`, `impurePropertyHook.pure`) and a pure
    function's callable-only redundancy
    (`pureFunction.redundantUnlessCallable`), unreported today; a pure
    `void` function or method is `pureFunction.void`/`pureMethod.void`
    under `@throws void` too, which today exempts it as any `@throws`
    does (`bug-12224.php`, 2); a method
    is pure through an ancestor's `@phpstan-pure`, a hook through its own,
    and a callback a built-in invokes at once (`array_map()`, `array_any()`,
    `array_find()`) is part of the pure caller's body, so an `echo` in any
    of them is `impure.echo`, unreported today (the rules oracle's 12
    PHPStan-only, `pure-method.php`, `pure-property-hook.php`,
    `pure-unless-callable-is-impure*.php`); an object's implicit
    `__toString()` (in `echo`, `print`, `.`, `.=`, interpolation and a
    `(string)` cast) is a call there, `possiblyImpure.methodCall` when
    that method isn't known pure, as is a call to a method with no purity
    marker (`$this->returningMethod();`), unreported today
    (`pure-method.php`, 8); and `array_map()` given an opaque, maybe-null
    or maybe-non-callable callback is `possiblyImpure.functionCall`, as is
    the callable itself (`pure-unless-callable-is-impure*.php`, 7);
    taking a reference to a property (`$value = &$this->p`) is
    `possiblyImpure.propertyAssignByRef` (`impure-assign-ref.php`); a
    closure passed to a `@param-immediately-invoked-callable` parameter
    (or a maybe-callable one) runs there, so its `exit` is `impure.exit`
    in the pure caller (`pure-function.php`, `bug13288*`, 3); creating a
    first-class callable (`$foo->pure(...)`, `pureFunction(...)`) is pure
    and calling it takes the callee's purity, where today both are
    `possiblyImpure.*` "unknown" (19 rules-oracle findings,
    `first-class-callable-pure-function.php`); a string literal argument
    that names a function (`$this->m('random_int')`) isn't a call
    (`bug-12119.php`); and `impureMethod.pure` needs the method or its
    class final, since a child may add the side effect
    (`FunctionPurityCheck`; `bug-12382.php`, `noop-impure-points.php`);
  - an enum's duplicate case value is found through constant expressions:
    `case A = 42; case B = 40 + 2;` isn't `enum.duplicateValue` today (a
    literal duplicate already is, on the later case, where PHPStan reports
    it once on the enum naming both cases, `enum-sanity.php:86`);
  - a set hook's parameter type is contravariant with the property's
    (`propertySetHook.parameterType`/`propertySetHook.nativeParameterType`), a
    template
    doesn't reuse a type alias's name (`generics.existingTypeAlias`: the
    check exists in `generics/bounds.rs` and misses a local alias), an
    alias doesn't shadow a class (`typeAlias.duplicate`), and
    `@phpstan-require-extends`/`-implements` sit only where PHPStan allows
    them (`requireExtends.onClass`, `requireExtends.onEnum`,
    `requireImplements.onClass`, `requireImplements.onEnum`, …); all
    unreported today; an enum using a trait that requires an interface is
    `class.missingImplements` (`incompatible-require-implements.php:52`);
  - `@phpstan-consistent-constructor` on a class with a private
    constructor is `consistentConstructor.private`, unreported today;
  - an overriding property's PHPDoc type is covariant with the
    overridden one's (contravariant for a set-only hooked property):
    `@var int` over `@var string`, `@var 4` over `@var 1|2|3`, `array`
    over `array<class-string>` are `property.phpDocType`, unreported today
    (the rules oracle's 5 PHPStan-only, `overriding-property*.php`,
    `bug-12466.php`);
  - an override's return type is checked in every form (PHPStan's
    `MethodSignatureRule`), where today only a native type against a
    native type is: a child without a return type under a parent's native
    one is a compile error ("must be compatible with A::foo(): array";
    `bug-8081.php`, `bug-8500.php`, `bug-9014.php`,
    `overriding-indirect-prototype.php`, `return-type-covariance.php`,
    `enum-sanity.php`, 8), a PHPDoc return incompatible with the parent's
    is `method.childReturnType` (`@return string` under `@return int`,
    `@return Cat` under `@return Dog`; `method-signature*.php`,
    `merge-inherited-return.php`, `bug-3922-ancestors*.php`,
    `bug-4003.php`, 12), and an untyped override of a built-in method
    with a tentative return type is `method.tentativeReturnType` even when
    that type is `mixed` (PHP 8.1 deprecates it without
    `#[\ReturnTypeWillChange]`: `current()`, `key()`, `offsetGet()`;
    `bug-4852.php`, `bug-5288.php`, `bug-7519.php`,
    `tentative-return-types.php`, and the anonymous classes of
    `offset-access-legal.php`); the rules oracle's 24 and 22 PHPStan-only;
  - `#[\NoDiscard]` on a `void` or `never` function, method, closure or
    arrow function is `attribute.target`, a PHP 8.5 compile error ("A void
    function does not return a value, but #[\NoDiscard] requires a return
    value"), magic methods returning `void` included; unreported today (14
    of the rules oracle's 16 PHPStan-only, `typehints-nodiscard.php`,
    `closure-typehints-nodiscard.php`,
    `arrow-function-typehints-nodiscard.php`);
  - `parameter.requiredAfterOptional`, `parameter.implicitlyNullable`
    ("Deprecated in PHP 8.4: … implicitly nullable via default value
    null", 9), `parameter.unresolvableNativeType` (`fn (Lorem&Ipsum
    $a)`, 4) and `return.unresolvableNativeType` (`: int&mixed`, also
    unreported on functions and methods, 6) cover closures and arrow
    functions as they do functions and methods, `requiredAfterOptional`
    with the same per-version wording
    (8.0, 8.1 for `?int $foo = null`, 8.3 for `int|null $foo = null`),
    and none for an implicitly nullable `int $foo = null`, which is
    `parameter.requiredAfterOptional` "Deprecated in PHP 8.0" today (the
    rules oracle's two `required-parameter-after-optional.php` files);
    an untyped parameter whose `@param` excludes `null` may default to
    `null` (`incompatible-default-parameter-type-methods.php:54`);
    unreported today (all 25 of the rules oracle's PHPStan-only,
    `required-parameter-after-optional-closures.php`,
    `required-parameter-after-optional-arrow.php`, `callables.php:151`);
  - a trait's private constant is checked for use in each host, as a
    trait's private method and property are: unused in `Bar`, it is
    `classConstant.unused` for `Bar::PRIVATE_CONSTANT` (the rules oracle's
    `conflicting-trait-constants.php`, 10, `bug-12011.php`), and a dynamic
    fetch `static::{$name}` counts only for the names `$name` can be
    (`unused-private-constant-dynamic-fetch.php`); both unreported today;
  - a `@throws` of any type that isn't `Throwable` is
    `throws.notThrowable`: a scalar (`bool`), an unknown class, an
    interface not extending `Throwable`, an intersection with a
    non-throwable member, a union with `void`, a conditional with a
    non-throwable branch, and a statement's `@throws` (`/** @throws
    \stdClass */ $i = 1;`); only a union with a non-throwable class member
    is reported today (the rules oracle's 10 PHPStan-only,
    `incompatible-throws.php`, `throws-with-require.php`,
    `missing-exception-method-throws.php`);
  - a template no parameter references is
    `method.templateTypeNotInParameter` wherever else it appears: today it
    is reported only when the return type uses it, so a template used
    nowhere, or only by `@param-out`, a conditional return, a
    `@phpstan-assert` or a `@throws`, isn't (the rules oracle's 80
    PHPStan-only, `function-template.php`, `method-template.php`,
    `*-conditional-return-type.php`, `*-assert.php`, `param-out.php`);
  - an anonymous class is a declaration like any other: the class, member
    and constructor rules run on it and calls see its members' signatures.
    Today `new class() implements F {}` missing `doFoo()`, an untyped or
    unused property, a method missing its `return`, `new class(1, 2, 3)`
    against a one-parameter constructor, `new class(1) {}` with no
    constructor, a `@phpstan-require-extends` trait it uses, `ArrayAccess`
    methods without tentative return types, an uninitialized readonly
    property, `echo $o->v()` of a `void` method and an unknown trait it
    uses (`trait.notFound`) are all unreported (41
    of the rules oracle's PHPStan-only findings name a `class@anonymous`);
  - `abstract` on an interface method is a compile error ("Interface
    method Baz::doBar() must not be abstract", `method.abstract` in
    `abstract-method.php` and `abstract-private-method.php`), unreported
    today;
  - `missingType.iterableValue` walks every type `MissingTypehintCheck`
    walks (about 75 of the rules oracle's 118 PHPStan-only): `@var` above
    an assignment, named or not, a by-reference one and a closure's
    `\Closure(array): array` included (22); each branch of a conditional
    return (`($i is positive-int ? non-empty-array : array)`, 23 in
    `function-`/`method-conditional-return-type.php`); a bare
    `Traversable` and intersections with it (`\Throwable&\Traversable`),
    as `iterable` already is (13); class `@method`, `@property` and
    `@phpstan-type` tags; `@param-out`; generic arguments (`static<array>`,
    `@phpstan-self-out self<array>`, `@mixin Dolor<array>`); a callable's
    parameters (`callable(array): int`); a set hook's own wider parameter
    (`set (int|array $v)` on `int`); and the native type where the PHPDoc
    doesn't apply (`@param Foo|Bar[]` on `array`, an unresolvable
    `array<int, int, int>` or `array{foo: int}&array{bar: string}`).
    The generics checks walk the same sites, `@param-closure-this`, `@use`
    and a template bound included: `missingType.generics` (18),
    `generics.notGeneric` (15; `Exception<int>` in an inline `@var`),
    `generics.lessTypes` (10), `generics.moreTypes` (7; `static<int,
    string>` in `@return`), `generics.notSubtype` (15; a `mixed` or
    `string` argument outside the template's bound, as in `@extends
    FooGeneric<int, mixed>`) and `generics.callSiteVarianceRedundant`/
    `generics.callSiteVarianceConflict` (7 each; `Covariant<covariant int>`,
    `<contravariant
    int>`); each count is the rules oracle's PHPStan-only, at class tags,
    `@param-out`, `@phpstan-self-out`, an inline `@var`, a conditional
    return and a set hook's parameter, unreported today.
- (f) control flow, returns, throws and generators:
  - an empty body is checked like any other: a function or method with a
    non-`void` return type and an empty body (or only a comment) throws a
    `TypeError` ("none returned") and isn't `return.missing` today, and
    neither is a get hook with an empty body (`get {}`); 507 of the rules
    oracle's 517 PHPStan-only `return.missing` are this (matched per
    declaration). An empty body under `@return never` is `return.never`
    (`bug-5627.php`, `missing-return.php:473`, `:481`,
    `typehints-nodiscard.php`), one marked `@phpstan-impure` or under
    `@phpstan-all-methods-impure` is `impureMethod.pure`
    (`all-methods-are-pure.php`, `pure-constructor.php`, 8), and `fn ():
    never => 1` is `return.never` ("should never return"), `return.type`
    today. The
    same exemption skips the `@param-out` end-of-execution check: `/**
    @param-out positive-int $i */ function a(int &$i): void {}` isn't
    reported, while a non-empty body that never assigns `$i` is
    (`parameterByRef.executionEnd`, PHPStan's `paramOut.type`). The check
    sits on each end statement (PHPStan's `ExecutionEndNode`), so an
    `if`/`else` whose branches both fall off is reported twice, not once
    on the declaration (the rules oracle's `parameter-out-execution-end.php`,
    7), and it misses two shapes today: `unset($p[1])` on a `list<int>`
    out-parameter leaves `array<0|int<2, max>, int>`, not a list
    (`parameter-out-assigned-type.php`), and a template `@param-out S`
    isn't satisfied by a concrete value (`param-out.php`, `method-assert.php`,
    `generic-callables-incompatible.php`; 45 `paramOut.type` in all, most
    of them empty bodies);
  - a finally block that changes a value returned by reference is
    `return.byRefFinally` (13 rules-oracle findings), and every exit point
    a finally block overrides is `finally.exitPoint` (25 of 42 missed);
  - `return.missing` is reported where execution falls off, the body's
    last statement (PHPStan's `ExecutionEndNode`), not on the declaration:
    36 of the rules oracle's 53 phpstan-rs-only `return.missing` are the
    same finding on the declaration's line, and each end that falls off
    is reported (an `if` without `else` before the end falls off twice, 7
    more lines in the oracle);
  - `return.unusedType` isn't reported for a union of constants when every
    return is a constant: `@return 1|2|3` returning `1`, or a
    `'Test'|'Projekt in Umsetzung'` domain, stays
    (`TooWideTypeCheck::narrowType`; 10 rules-oracle findings,
    `bug-11453.php`,
    `bug-10312*.php`); it is reported where today it is skipped: for
    closures and arrow functions (`fn (): ?string => 'foo'`,
    `tooWide*ReturnType.php`, `closure*ReturnTypes.php`,
    `native-union-types.php`, 8), for a body that can fall off the end,
    which is a `TypeError` and never a `null` return (`function (): ?string
    { if (rand()) { return 'foo'; } }` never returns `null`; `dolor2()`,
    `dolor4()`), and for a conditional return type, whose branches are
    members (`($flag is 1 ? string : int)` returning `$flag` never returns
    `string`); the rules oracle's 22 PHPStan-only;
  - a return type that includes `void` accepts falling off the end (fixed
    first): `@return void|int` on a function that returns only on some
    paths is `return.missing` today, 10 of the oracle's 38 phpstan-rs-only
    `return.missing` (the `php_codesniffer` sniffs' `process()`), and
    returning `null` under it is accepted: `return rand(0, 1) ? 17 :
    null;` under `@return void|int` is `return.type` today (the rules
    oracle's `bug-10291.php`), and a bare `return;` is `return.empty` (4
    findings, `bug-11980.php`, `returnTypes.php:652`, `bug-13566.php`);
  - a conditional return type is resolved at each `return` against the
    scope's parameter types: after `if ($lazy) { return …; }`, nette's
    `Strings::matchAll` returns a `list` against `($lazy is true ?
    Generator : list)`, and `return is_int($value);` under `@return ($value
    is int ? true : false)` returns `bool` (`return $value === null` under
    `($value is null ? true : false)` too, the rules oracle's
    `bug-12087b.php`); both are `return.type` today
    (phpstan-rs reports 12 against a conditional in the corpus; PHPStan
    reports 1 of the 5 checked, e.g. not
    `nette/utils/src/Utils/Strings.php:538` or
    `latte/Essential/Filters.php:728`);
  - a `yield` is checked against the declared generator or iterable key
    and value types (`YieldTypeRule`): `yield 1` under `@return
    Generator<int, string>` or `iterable<int, string>` is unreported today,
    and so is a key: `yield 1;` and a bare `yield;` produce `int` keys a
    `Generator<string, …>` rejects (`generator.keyType`, `yield.php`, and
    a template key `K of int|string` against `(K of int)|string`,
    `bug-7484.php`);
    only `yield from` delegation is checked (28 oracle
    `generator.valueType` findings), and a delegated value mismatch is
    `generator.valueType`, where phpstan-rs reports it as
    `generator.keyType` today (`yield-from.php:74`); a `yield` under a
    return type that isn't a generator is `generator.returnType` at each
    `yield` and `yield from`, as PHPStan reports it, not once on the declaration
    (the rules
    oracle's 10 PHPStan-only, `yield-in-generator.php`, `returnTypes.php`);
  - a returned value is checked against a template return type:
    `return 5;` under `@template T @param T $x @return T`, and `return new
    stdClass();` under `@template T of object @return T`, are unreported
    today (`T` can be any type the caller picks), and neither are `T|null`
    or `T|false` returned under `@return T` (`bug-15168.php`, 6) or a
    concrete class under `T of Field2` (`bug-6589.php`, `bug-6787.php`,
    `bug-8048.php`, `short-get-property-hook-return.php`, 3);
  - a concrete function with an empty body still checks its `@throws`:
    `/** @throws \Exception */ function ex(): void {}` isn't
    `throws.unusedType` today (the empty-body exemption is for abstract and
    interface methods; all 44 of the rules oracle's PHPStan-only
    `throws.unusedType`, each member of a union, intersection, conditional
    or template tag reported, `incompatible-throws.php`,
    `throws-with-require.php`, `too-wide-throws-*.php`,
    `unthrown-exception-multi.php`), while a call to a method without
    `@throws` is an implicit throw point that satisfies it
    (`exceptions.implicitThrows`,
    on by default): `@throws \Exception` over `$this->doBar();` is
    `throws.unusedType` today, as is one over an immediately called arrow
    function that throws (5 rules-oracle findings,
    `too-wide-throws-explicit.php`, `too-wide-throws-method.php`,
    `immediately-called-arrow-function.php`);
  - `throw` checks its operand: `throw 123;` ("Can only throw objects"),
    `throw $nullableException`, `$foo ?? throw 1` and, at level 10, `throw`
    of `mixed` are `throw.notThrowable`, unreported today (the rules
    oracle's 6 PHPStan-only, `throw-values*.php`,
    `wrong-variable-name-var.php:156`);
  - a conditional `@throws ($x is 0 ? Exception : void)` resolves at each
    call: `inverse(0)` under `@throws void` is `throws.void`, through
    function, method, static and constructor calls, nested and inherited
    ones, unreported today (`conditional-throws-function.php`,
    `conditional-throws-method.php`, 12), as is `array_combine()` of
    `mixed` arrays throwing `ValueError` (`bug-13642.php`);
  - a `catch` its `try` can't reach is `catch.neverThrown` (from D2's
    throw points): `try { $q = 1; } catch (\Throwable $e) {}` and a
    `catch (\LogicException)` around a call declared `@throws
    \RuntimeException` are unreported today; the rule doesn't exist (the
    rules oracle's 189). The throw points it reads are PHPStan's: a write
    to a typed property throws `TypeError` only when the value may not fit
    after coercion (`int` into `?float` doesn't, a PHPDoc-only mismatch
    doesn't; `bug-6256.php`, `bug-6791.php`, `bug-9146*.php`,
    `union-type-error.php`); `99 / $v` on `int<min, -1>|int<1, max>`
    can't throw `DivisionByZeroError` (`bug-6349.php`, 20); a `match` with
    `default` or exhausted arms can't throw `UnhandledMatchError`
    (`bug-6115.php`); `echo`/`(string)` of an object and a magic call
    follow `__toString()`'s and `__call()`'s `@throws void`
    (`bug-5952.php`, `bug-13806.php`, `dead-catch-magic-methods.php`),
    `isset()` on an `ArrayAccess` its `offsetExists()`
    (`bug-11427.php`), `new $class` on a `class-string<T>` the
    constructor of `T` when final or consistent (`bug-4806.php`,
    `bug-6574.php`), and a static call its method's conditional `@throws`
    (`bug-4805.php`); `foreach` over an array, creating a first-class
    callable and `assert(false)` (an `AssertionError`) throw nothing a
    `catch (\RuntimeException)` takes (`bug-5903.php`,
    `dead-catch-first-class-callables.php`, `bug-5650.php`); property
    hooks are throw points only through their own `@throws` when
    `exceptions.implicitThrows` is off
    (`unthrown-exception-property-hooks*.php`); each member of a
    multi-catch is judged alone (`unthrown-exception-multi.php`); a read
    of the backing property inside its own hook can't throw, so `try {
    return $this->i; } catch (…) {}` in a get hook returns on every path
    (`return.missing` today, `unthrown-exception-property-hooks.php:18`);
    and a caught
    class that isn't an exception, or is guarded by `class_exists()`, is
    judged as well (`catch.php`, `class-exists.php`);
- (g) the restricted-usage framework: one set of usage sites (function, method
  and static calls, property and constant reads, `new`, `instanceof`, `catch`,
  `extends`/`implements`/trait `use`, attributes, native type hints, PHPDoc
  types, first-class callables and the implicit `__toString()` call), each
  asking every registered restricted-usage extension (PHPStan's
  `Restricted*UsageExtension`s), which `@internal` (core), `@deprecated`
  (ext-deprecation-rules), `restricted_symbols` (ext-disallowed-calls) and
  PHPStan's `@api` promise (`phpstan_api`) implement. Today each has its own
  subset: `internal_usage` covers all but attributes, `deprecated_usage` only
  calls, property and constant reads and `new` (a deprecated class in a type
  hint, an `extends`, an `instanceof` or a `catch` is unreported),
  `restricted_symbol_usage` reuses `internal_usage`'s call sites, and
  `phpstan_api` walks its own.
- (h) `FileAssertRule` (with the `assertNativeType` and
  `assertVariableCertainty` support Stage D's oracle added), the dump rules
  and the four asserts. `\PHPStan\dumpType()`, `dumpNativeType()`,
  `dumpPhpDocType()` and `debugScope()` report under PHPStan's
  identifiers (`phpstan.dumpType` and the rest; the rules oracle's 39, 9,
  27 and 1, none reported today); the asserts' mismatches keep
  `phpstanRs.*` (approved: PHPStan's `phpstan.type`, `phpstan.nativeType`,
  `phpstan.superType`, `phpstan.variable`).
- (i) the "is this usage internal / deprecated from this scope" predicate (§3)
  exposed from (g)'s framework, so E6's semantic tokens mark every usage site
  the rules check; today `semantic_tokens.rs` re-derives the gating at function,
  member and `new` sites only.

Mixed modules are split row by row (Appendix A). Gate: architecture rule 10
for the analyser.

**E4. Extension modules.**

- Order:
  1. ext-doctrine's remaining modules (its reflection data extension moved
     in C8, and E4 fixes its entity metadata: only mapped properties are
     fields, `#[MappedSuperclass]` and trait fields are included, docblock
     `@ORM` annotations are read), then phpstan-doctrine's rules, which
     ext-doctrine has none of today (it only types results; no
     `doctrine.*` identifier exists). Probed through the corpus's own
     `phpstan.neon` against `UserEntity`, each of these is unreported
     today: DQL syntax and semantic errors in `createQuery()` (`SELEC …`,
     an unknown field) and in a QueryBuilder chain (`doctrine.dql`), an
     unknown field in `findBy(['nope' => 1])` (`doctrine.findByArgument`),
     and a magic `findOneByNope()` (`method.notFound`). The entity checks
     (`doctrine.columnType`, `associationType`, `enumType`, `finalEntity`,
     `finalConstructor`, `mapping`, `queryBuilderDynamic`) have no
     counterpart either;
  2. ext-sql;
  3. ext-phpunit, ext-codeception, ext-mockery and ext-mockstan, after E1.
     ext-phpunit gains phpstan-phpunit's rules, probed against the
     vendored package and each unreported today: `@covers`/
     `#[CoversClass]` naming a missing class or method
     (`phpunit.covers*`), the assertion simplifications
     (`assertSame(false, $x)` → `assertFalse`, `assertSame(1, count($a))`
     → `assertCount`, and `assertTrue`/`assertNull`/`assertEquals`),
     `->method('nope')` on a mock of a class without it
     (`phpunit.mockMethod`), and data providers that are missing, private
     or non-static (`phpunit.dataProvider*`). Its `test.risky` has no
     phpstan-phpunit counterpart and stays;
  4. ext-pest (closure-this, the file-set hook, pest risky tests), then
     pest-plugin-phpstan's rules, none of which exist today (checked
     against the vendored plugin's 20 identifiers): duplicate test
     descriptions, invalid group names, static or empty test closures,
     invalid `->repeat()` values, `beforeAll`/`afterAll` inside `describe`
     and their `$this` use, `covers()` of a missing class or function,
     `throws()` of a missing or non-`Throwable` class, `describe()` without
     tests, the expectation checks (`toBeString()`-style requirements on
     the value, impossible and redundant expectations) and a redundant
     local `uses()`;
  5. ext-nette-tester;
  6. ext-strict-rules, completing phpstan-strict-rules: today's 11 toggles
     cover parent-constructor calls, boolean conditions, useless casts,
     strict `array_filter`, strict function calls, switch case types, loop
     variable overwrite, dynamic static calls, illegal constructor calls,
     implicit array creation and arithmetic operands; the package's other
     groups are missing (each unreported today, checked against the
     vendored package): `disallowedLooseComparison` (`==`/`!=`),
     `disallowedBacktick`, `disallowedEmpty`, `disallowedShortTernary`,
     `noVariableVariables` (with the dynamic method, property, static
     method and static property names) and `closureUsesThis`, plus the
     core parameters the package sets (C1's list).
     `matchingInheritedMethodNames` (through `check_function_name_case`)
     and `booleansInLoopConditions` already exist;
  7. ext-deprecation-rules;
  8. ext-disallowed-calls;
  9. ext-shipmonk, ext-type-perfect, ext-voku, ext-sidz, ext-slam,
     ext-psr3, ext-language-extensions and ext-extras.

  Each crate mirrors its package completely, as §2's table says. Compared
  rule class by rule class against the packages' current releases
  (downloaded for the comparison), today's coverage is:

  - ext-shipmonk has comparable types, null in binary and assign
    operations and interpolated strings, and enum match exhaustiveness
    (`EnforceEnumMatch`, `ForbidMatchDefaultArmForEnums`); ext-extras and
    `restricted_symbols` cover `ForbidUnusedException`,
    `RequirePreviousExceptionPass` and `EnforceNativeReturnTypehint`
    (partly) and `ForbidCustomFunctions`, and ext-strict-rules' strict
    function calls require `iterator_to_array()`'s `$preserve_keys` as
    `EnforceIteratorToArrayPreserveKeys` does. Missing: `BackedEnumGenerics`,
    `ClassSuffixNaming`,
    `EnforceClosureParamNativeTypehint`, `EnforceListReturn`,
    `EnforceReadonlyPublicProperty`, `ForbidArithmeticOperationOnNonNumber`,
    `ForbidCast`, `ForbidCheckedExceptionInCallable`,
    `ForbidCheckedExceptionInYieldingMethod`, `ForbidEnumInFunctionArguments`,
    `ForbidFetchOnMixed`, `ForbidIdenticalClassComparison`,
    `ForbidIncrementDecrementOnNonInteger`, `ForbidMethodCallOnMixed`,
    `ForbidNotNormalizedType`,
    `ForbidPhpDocNullabilityMismatchWithNativeTypehint`,
    `ForbidProtectedEnumMethod`, `ForbidReturnInConstructor`,
    `ForbidReturnValueInYieldingMethod`, `ForbidUnsafeArrayKey`,
    `ForbidUnsetClassField`, `ForbidUnusedClosureParameters`,
    `ForbidUnusedMatchResult`, `ForbidUselessNullableReturn`,
    `ForbidVariableTypeOverwriting`, `UselessPrivatePropertyDefaultValue`;
  - ext-type-perfect has `NoIssetOnObject` and `NoEmptyOnObject` (in
    `strict_idioms`); missing: `NoArrayAccessOnObject`,
    `NoMixedMethodCaller`, `NoMixedPropertyFetcher`, `NoParamTypeRemoval`,
    `NarrowPublicClassMethodParamType`, `NarrowPrivateClassMethodParamType`,
    `NarrowReturnObjectType`, `ReturnNullOverFalse`;
  - ext-voku has the mixed-operand checks (`ExtendedBinaryOp`,
    `ExtendedAssignOp`); missing: the eight `IfCondition*` rules,
    `ElseIfConditionBasic`, `WrongCast` and `DisallowedCallMethodOnNull`;
  - ext-mockstan has eight of rector/mockstan's ten (`mock_hygiene`);
    missing: `NoEntityMocking` and `NoDocumentMocking`;
  - ext-disallowed-calls maps functions, methods and classes onto
    `restricted_symbols`; missing: namespaces, constants, class
    constants, properties, superglobals, attributes, enums, `new`, the
    control-structure and keyword kinds (`if`, `goto`, `eval`, `echo`,
    `print`, `exit`/`die`, `include`/`require`, …), and the entry fields
    the ROADMAP lists (`allowInFunctions*`, parameter conditions,
    `definedIn`, `errorIdentifier`, `errorTip`);
  - ext-sidz (all thirteen magic-number rules), ext-psr3 (all of
    staabm/phpstan-psr3 and sfp-phpstan-psr-log) and
    ext-language-extensions (all eight attributes) are complete;
    slam/phpstan-extensions and phpstan-mockery have no rules (types and
    configuration only).
- Extensions register parse-time visitors of their own (§3.1's
  annotations), through the registration API.
- ext-extras' `@see` check (`phpDoc.seeTag`) reads a reference as its
  author means it: `@see Assert::assertSame` without parentheses names the
  method when the class has one (phpDocumentor's strict reading is a
  constant), and free text (`@see RFC 7541 Appendix A` on a class
  constant) isn't a reference. Today both are "unknown reference/class":
  241 phpstan-rs-only findings on the corpus oracle's files, 176 of them
  PHPUnit's `@see Assert::…` in `Framework/Assert/Functions.php`.
- ext-sql resolves static-schema SQL (SELECT/CTE/derived-table columns
  against the catalog) and validates parameters through an `sql` API;
  `sqlparser` leaves the package.
- The Doctrine QueryBuilder chain becomes extension type data, using
  method type-specifying (statement-level `$qb->andWhere()`) and an
  `ExpressionTypeResolverExtension`. This is an approved-delta commit with
  fixtures for aliasing, statement mutation and arguments that change
  between assignment and use.

**E5. Levels** (§3.9), after E3 moved every rule at its level: the `max`
alias, the default level for CLI and LSP, and the replacement consumers for
JSON and the LSP (`Diagnostic.level` goes). The level tests become
level-header fixtures, and LevelsTestCase fixtures are added. Gate: for
every level in 0..=10 plus unset, before == after, except the approved
deltas E3's table names.

**E6. Observers.** LSP-only presentation (semantic tokens, inlay hints, hover,
definition probes) on observer events, the mechanism F5's language features are
built on.

**E7. Checker decomposition.** Gate: the exact enumerated field sets for
engine context, file state, function frame and traversal context
(Appendix C), with interior mutability only in pure memo caches.

### Stage F: Session and LSP (in the package)

Stage F starts from a partial Session. `project::session::Session` already
resolves a config into a run (`resolve`, `for_fixture`), loads and checks it
(`load`, `analyse`, `run`), and `AnalysisResult::finalize` applies the level
filter, inline ignores, `ignore_errors`, the baseline and the order;
`app::check_project` prepares a CLI run, and the fixture harness
(`tests/fixtures.rs`) drives `Session::for_fixture`. The rest sits outside it:

- the bin's `report` (`main/check_project.rs`) generates baselines, applies
  fixes, prints and exits;
- `AnalysisResult` holds the whole `Project`, because the finalizer reads
  inline ignores from it;
- the language server (`lsp.rs`, 2,842 lines) builds its own `Project`, with its
  own settings assembly (`lsp/workspace_config.rs`), post-filters
  (`apply_post_filters`: the level, `ignore_errors` and the baseline; inline
  ignores are applied inside the checker, `defer_inline_ignores` being off),
  file walk and incremental path (`Project::update_file_cancellable`,
  `remove_files`, `deps.rs`).

The LSP and CLI observations below come from the debug binary at `56c3beab`,
the LSP driven over stdio; the line-terminator ones from `php`; the Windows path
one from reading the code, not a run.

F1 builds the Session and moves the CLI and the LSP onto it; F2 persists its
results; F3 and F4 are the formats and the binary over it; F5 is the language
features over F1's snapshots. Within each, the structural commits land before
the behaviour commits, and a behaviour commit carries its fixture and, where the
output changes, its approved deltas for the corpus, type-snapshot and baseline
gates (§4).

**F1. Session** (§3.10).

*Fixed first.* Each of these ends the language server, loses data or corrupts
files, so each is an interim commit with its test on today's code, long before
the phase (Appendix F); the F1 or F5 item that names it later replaces the
interim fix with the general one:

- the LSP's debounce thread is spawned with `WORKER_STACK_SIZE`, and the
  workspace scan's parses run on the worker pool, so a file of 60 or 120 nested
  arrays no longer aborts the server (panic isolation, below);
- `queue_change` writes `latest_text` when `didChange` arrives, so completion
  triggered by `>` reads the text with the `->` (an edit's text applies at
  once, below);
- `didClose` restores the file's disk content (below);
- `--generate-baseline` keeps the entries and `includes` of files the run didn't
  analyse, where it erases them today (below);
- `--fix` skips a file whose content changed since it was analysed, rather than
  patching it at stale offsets (below);
- a closed output pipe ends the run quietly (`CRASH_REPORTING_PLAN.md`'s step
  1b; F3);
- rename refuses any rename whose sites include a qualified name, an import or
  an alias, instead of returning an edit that leaves code resolving elsewhere,
  until F5's rewrite lands.

*Structural commits*, one each:

- **`AnalysisResult` owns its data** instead of the `Project`:
  - file errors, not-file errors and internal errors, as separate lists;
  - each analysed file's inline-ignore records (the lines and identifiers its
    `@phpstan-ignore*` comments cover and where each comment sits, PHPStan's
    `linesToIgnore`/`unmatchedLineIgnores`), which the finalizer reads instead
    of `Project::apply_inline_ignores` walking `FileState.ignores` and each
    file's AST for trait ranges, so a result F2 reads from the cache finalizes
    without parsing its file;
  - collected data and dependencies;
  - stats (checked, skipped, context loaded, panicked);
  - warnings: config warnings, SQL schema drift and lexer warnings, today
    `Session.warnings`, `AnalysisResult.lex_warnings` and `Report::notices`,
    which the bin prints to stderr and no structured format carries;
  - the type dump.
- **The finalizer** in §3.10's order, with no level stage: E5 runs no rule above
  the level, so `Finalize.level` and `Overrides.report_level` go, and the
  fixture harness, which records every level at once through
  `report_level: u8::MAX` today, runs at `max`. Stale baseline entries are
  finalizer output under `reportUnmatched`, like any unmatched ignore, instead
  of `Report::notices` lines.
- **Entry points**: `Session::run`, `generate_baseline` and `apply_fixes` (both
  in the bin's `report` today), `update(FileDelta, &CancellationToken)` and
  `reconfigure(Config)`. `Project` folds into the Session: its file table
  becomes the VFS, `registry` C4's builder and snapshots, `dependents` F2's
  graph, `use_sites` with `member_index_built` and the escape sets E1's
  reference-site collector, `pest_config` ext-pest's and `sql` ext-sql's
  context. Its entry points (`load_file`, `load_analyzed_file`,
  `insert_parsed_file`, `finish_loading`, `update_file`,
  `update_file_cancellable`, `remove_file`, `remove_files`, `check_file`,
  `check_paths`, `check_paths_isolating_panics`, `check_all`,
  `check_file_with_property_assign_facts`, `check_analysed_file`,
  `check_whole_program`) collapse into `run` and `update`, and the
  `project/tests` and `lsp/tests` that drive `Project` move onto them, names
  kept through `gates/test-renames`.
- **The VFS**: one source of file contents and one file set.
  - *Contents* come from disk or an editor overlay (open buffers), the overlay
    first; the walk, the LSP and `--fix` read through it. They are read when
    needed and kept only for overlays and the files a request is working on,
    with each file's line index; today `retain_source` keeps every loaded
    file's text for the whole LSP session, vendor context included.
  - *Membership* (analysed; context from scan, stub or composer vendor paths;
    or neither) is decided once, from `paths` and C1's `exclude_paths`, for the
    batch walk, a file the editor opens and a watched-file event alike. Today
    `update_file` makes any file it hasn't seen analysed unless it sits under
    `vendor`, `.git` or `node_modules`: with `paths = ["src"]`, opening
    `tests/t.php` publishes its `return.type` in the editor, where
    `check-project` reports nothing, and the file stays analysed for the
    session.
  - *Identity* is the canonical path (C1's walker dedup), so two spellings of
    one file are one file, a case-insensitive filesystem's `Foo.php` and
    `foo.php` included. It is used for comparing, never printed. On Windows it
    drops `canonicalize`'s verbatim `\\?\` prefix where that keeps the path
    valid, as the `dunce` crate does: today `Session::resolve` and
    `composer_roots` canonicalize the root and compare it with composer's
    unprefixed manifest paths, so on Windows (a release target) no file would
    match its package's directory, and every package-aware check (package
    names, composer hygiene, `phpstan_api` gating) would see no package.
  - *URIs*: each open document keeps the URI its client used, which its
    diagnostics are published under (a workspace opened through a symlink sees
    its own paths), and a URI the server makes for a file never opened follows
    its workspace folder's spelling (a drive letter's case, an encoded `:`). A
    path that isn't valid UTF-8 round-trips through its URI as bytes, where
    `path_to_uri` and `percent_decode` go through `to_string_lossy` and
    `from_utf8_lossy` today and name a file that doesn't exist.
  - *Lines* end at `\n`, `\r\n` and a lone `\r`, as in PHP and LSP (`php`
    reports the syntax error in `<?php\r$a = 1;\r$b = ;` on line 3), and the
    lexer's line starts and the line index follow them. The lexer's
    `compute_line_starts` ends a line at `\n` alone today, so in a file with a
    lone `\r` every line phpstan-rs reports after it is too low, and inline
    ignores match the wrong lines.
  - *File events*: the walk and the LSP's file watchers follow C1's
    `file_extensions`, where both hard-code `.php` today (`collect_php_files`,
    the `**/*.php` registration). Scan and stub paths outside every workspace
    folder get watchers of their own (a relative pattern on each base
    directory), since a change there changes what analysed files resolve. A
    deleted directory removes every file under it: a client may report a
    folder's deletion as one event for the folder, and
    `handle_watched_files_changed` passes that path to `remove_file`, which
    removes nothing, so the folder's files stay analysed and published.
- **Panic isolation, on the pool.** Every parse and check runs on the process's
  worker pool (`project::worker_pool`, 64 MiB stacks) under `catch_unwind`, and
  a panic becomes that file's internal error.
  - *Where*: the batch run and, in the LSP, the workspace scan, `didOpen`, the
    debounced `didChange` recheck, watched-file events, folder removal, and the
    requests that run the checker (hover, inlay hints, code actions, completion
    probes, references' first member index). Today only the batch check and the
    LSP's whole-workspace check are isolated. The rest run on the LSP's main
    thread or its debounce thread, spawned with the default 2 MiB stack although
    `WORKER_STACK_SIZE`'s doc says the checker needs far more: a file holding 60
    nested arrays, which `check-project` checks cleanly, aborts the language
    server on `didChange` ("thread 'phpstan-rs-lsp-debounce' has overflowed its
    stack"), and one holding 120 aborts it during the startup scan, on the main
    thread. A panic there unwinds while holding the `Server` mutex, and the next
    `lock().unwrap()` panics too.
  - *How*: the hook, the isolated units and the internal-error record land
    early, in `CRASH_REPORTING_PLAN.md`'s step 1 (its `crash-context` crate).
    One hook, installed once and quiet on a thread inside an isolated unit,
    replaces the process-global swaps (`take_hook`/`set_hook` around the CLI
    run, in `check_paths_isolating_panics` and in vendor discovery, racing the
    LSP's other threads); `crash_context::isolated` wraps each parse,
    property-facts collection, check, whole-program pass and load step; and
    `AnalysisResult`'s internal errors are `InternalError`s (the unit, the file
    the panicking thread was on, the message, the location in phpstan-rs's
    source and an unresolved backtrace), where today a check keeps its message
    alone and a parse or facts panic keeps nothing (`read_and_parse_file`
    returns `None`, counted as a parse error). The backtrace is captured under
    `RUST_BACKTRACE`, with a crash-reporting client, or with `-v` (F4).
  - *Requests*: F1 runs the LSP's handlers on the pool through `isolated` as
    well. A request whose handler panics answers `InternalError` and the server
    keeps serving; today a panicking hover or completion unwinds through the
    dispatch loop and ends the process.
- **Cancellation and the event loop.** A `CancellationToken` is checked between
  files and at statement boundaries within one, replacing
  `update_file_cancellable`'s per-file `should_abort` closure, and it backs
  `$/cancelRequest`, whose flags no handler reads today. The loop is ROADMAP
  Phase 187's, event-driven with no timed wakeups (the dispatch loop blocking on
  its channel, the shutdown flag set under the debounce queue's mutex), and the
  Session's work leaves it:
  - requests run on the pool against the last published snapshot (C4) while an
    update builds the next, answered in whatever order they finish, while
    document notifications apply in arrival order. Hover and completion stop
    waiting behind a recheck that holds the `Server` lock (`DebounceHandle`'s
    documented trade-off), and a slow request (references' first member index,
    a workspace symbol search) stops delaying the next;
  - an update publishes the edited file as soon as it is checked, then open
    documents, then the rest of the affected files; today
    `update_file_cancellable` checks the whole affected set before anything is
    published, so the file being typed in waits for its dependents.
- **Last good AST.** A file that stops parsing keeps its last good AST, as
  `update_file` already does by parsing before it unindexes, while its parse
  errors are published (below). Once the parser recovers (below), a declaration
  the recovered tree still holds is read from it, and one the error swallowed
  keeps its last good version, so the file's dependents don't flood with errors
  mid-edit.
- **`reconfigure`** rebuilds registrations and the file set and reuses unchanged
  reflection and stub layers. Today `reload_configuration` applies settings but
  can't change the loaded files (`Project` has no unload), so a changed
  `scan_paths`, `stub_paths` or `no_composer` only logs "reload the window". It
  runs on a settings change, on a change to the config file, on a change to
  `composer.json`, `composer.lock` or `vendor/composer/installed.json`
  (unwatched today: the watcher registers `**/*.php` only), and on a change to
  the baseline or a file it includes, so regenerating the baseline updates the
  editor (the LSP reads it once per configuration today).
- **One Session per root.** Each workspace folder has its own Session and
  config, and a file belongs to the innermost root containing it; the Sessions
  share the worker pool and C5's version-keyed stub layer, so a second root
  costs its own files, not another copy of the stubs. Today `root`, `baseline`,
  `ignore_error_entries`, `level_cutoff` and the project's settings are
  last-root-wins across folders. The client settings are pulled per folder
  (`workspace/configuration` with each folder's `scopeUri`), so
  `phpstan-rs.configPath` and `phpstan-rs.enable` can differ between folders,
  where today one unscoped pull serves every folder; and a folder's first load
  uses them, read from `initializationOptions` and each folder's pull before it
  loads. Today the server never reads `initialization_options` (a test pins
  only that it tolerates them), so the startup scan uses the root's
  `phpstan-rs.toml` and the override arrives with the first configuration pull,
  after the load, where a changed file set only logs "reload the window".
- **Files outside every folder.** A file opened outside every workspace folder
  gets a Session rooted at its nearest ancestor holding a config file or
  `composer.json`, else one for the file alone with the default settings, as
  does an unsaved `untitled:` buffer, which is analysed from its overlay and
  never cached. Today such a file joins the one `Project` with whatever the last
  root configured, or with no configuration when no folder is open, and
  `uri_to_path` drops every non-`file:` URI, so a new PHP buffer gets no
  diagnostics until saved.
- **The workspace load runs as the CLI's does.** The analysed files and the
  context the locators load eagerly (Composer `files` entries, functions and
  constants, C5) are parsed in parallel on the pool, in no particular order
  (C4's collision policy doesn't depend on it), and context classes are read on
  demand through C5's locators instead of every vendor file up front, as
  `load_workspace_files` and `Session::load` both parse today. Today
  `load_workspace_files` parses each file in turn on the main thread inside the
  `initialized` notification, then checks, and no request is answered until
  both finish. The load is reported through `$/progress` on a token the server
  creates (`window/workDoneProgress/create`) when the client supports it; today
  progress shows only when the client happened to send a token with
  `initialize`, and a recheck after `reconfigure` or a folder change reports
  none.
- **LSP parity**, one commit per §1 divergence, each fixtured and held to gate
  7:
  - the project-level rules (E1's collector rules: dead code, members, enum
    cases and abstract classes, duplicates, namespace cycles, architecture,
    class shapes, composer hygiene; ext-mockstan's mock hygiene) run on every
    update, their collected data kept per file; `property.unusedType` reads
    every file's property assigns, where the LSP's `check_file` sees only the
    checked file's;
  - composer roots and path-repository promotion (the LSP loads one
    `PackageContext`, at the workspace root);
  - config errors surface as diagnostics on the config file, each at the
    offending key's or value's position (the loader keeps the TOML spans beside
    C1's dotted key path), and a `window/showMessage` (today an `eprintln!` to
    the server's stderr and an empty config);
  - paths resolve against the config file's directory: the LSP resolves
    `paths`, `scan_paths`, `stub_paths`, the baseline and the SQL schema against
    the workspace root, which differs when `phpstan-rs.configPath` names a file
    elsewhere;
  - one default level (E5; the LSP has no cutoff when `level` is unset, the CLI
    caps at 6);
  - inline ignores go through the finalizer, `ignore.unmatched*` included when
    configured (the LSP applies them inside the checker and never reports an
    unmatched one);
  - stale baseline entries and unmatched `ignore_errors` entries are reported
    after a whole-workspace check, on the baseline or config file, not per edit
    (`apply_post_filters` discards them today).
- **`ResolvedWorkspaceConfig` deleted**, with `workspace_config.rs` and the
  `Server` fields it fed.

The fixture harness, already on `Session::for_fixture`, gains edit fixtures: a
scripted sequence of deltas through `update`, each step compared with a fresh
run (gate 7 in miniature). Gate: nothing outside `project` constructs a
`Checker` or a `Project` (gate 10's engine-construction rule, over `lsp`, whose
hover, inlay and completion probes build `Checker`s until E6's observers).

*Behaviour commits*, one each. The run's result:

- **The exit code** (approved delta, `tests/cli.rs`) is decided from the result
  (Appendix F): 0 when nothing remains after the finalizer; 1 when a file or
  not-file error remains; 2 when the result can't be trusted, because the run
  couldn't start (an invalid config, no files under the paths, an unreadable
  baseline) or finished incomplete (an internal error), so CI tells a broken run
  from a failing one. F3 adds 141 and F4 130. Today every run exits 0 but a
  config or `--set` error (1): a finding, a parse error, a panic and
  `check-project nosuchdir` ("no .php files found") all do.
- **Parse errors are reported.** A file that fails to parse gets a
  non-ignorable `phpstan.parse` error at the parser's position, in every format
  and in the editor. Today `check-project` counts it ("1 file(s) skipped: parse
  error") with no path, line or message, the JSON output omits it, and the LSP
  sends only a `window/logMessage`, so a syntax error never reaches the Problems
  panel. A context file that fails to parse is a warning. Lexer warnings are
  non-ignorable file errors (§3.10).
- **The parser recovers.** It resynchronizes at statement and class-member
  boundaries and returns the recovered tree with every error, where
  `parse_program` returns the first error alone today. Every error is reported;
  a file with parse errors is still not analysed (its findings would be noise);
  and reflection and navigation read the recovered declarations, so a method
  added above a syntax error resolves at once, where the last good AST alone
  keeps the file's state from before the edit.
- **Deep nesting works on any thread.** The parser's recursive productions, the
  analyser's expression and statement handlers and the type operations over
  nested types grow the stack on demand (`stacker::maybe_grow` at each recursion
  point), and `MAX_EXPR_DEPTH` goes with the `ParseError` B made of it: PHP
  parses 2,000 nested arrays, where phpstan-rs reports 400 as a parse error
  today (unreported, like every parse error), and the pool's 64 MiB stays a
  throughput choice, not what correctness rests on. D3's iterative binary chains
  stay.
- **Internal errors are reported in every format** and fail the run, with a
  note that the result is incomplete; the JSON, checkstyle and JUnit formats
  drop `check_panics` today.

The baseline:

- **Keyed relative to its own directory**, as PHPStan's baseline paths are.
  Today an entry holds the run's own spelling of the path, normalized lexically
  only, so a baseline generated at the project root matches nothing in a run
  from a subdirectory with `--config ../phpstan-rs.toml`, whose paths are
  `../src/…`. Nothing is released, so the format changes in place, with no new
  version: gate 6's Stage A baselines are rewritten once in this commit (an
  approved delta), and the old-format support goes with it, since no released
  build ever wrote an old file: the version-1 loader (`EntryV1`,
  `BaselineFileV1`), the `version` field's check and `convert-baseline`.
- **Non-ignorable errors bypass it**: `entries_for` writes them into a generated
  baseline, and `baseline::filter` suppresses them (it has no `non_ignorable`
  check; `ignore_errors::filter` has one). Generation reports how many it left
  out.
- **Generating it from part of the project keeps the rest.**
  `--generate-baseline` over fewer files than the config's paths replaces only
  the analysed files' entries, and a baseline split into included files keeps
  its `includes`. Today `write_entries` rewrites the file from the run's entries
  alone, so `check-project --generate-baseline bl.json src/a.php` erases every
  other file's entries, and the `includes` list goes with them.
- **Only an analysed file's entries can be stale.** `baseline::filter` reports
  every entry left unmatched, so checking one file of a baselined project calls
  every other file's entries stale: after a baseline of `src`,
  `check-project --baseline bl.json src/a.php` reports `src/b.php`'s entry "no
  longer matches any diagnostic". As §3.10 has for `ignore_errors` path entries,
  an entry counts as unmatched only when its file was analysed.
- **One that can't be loaded fails the run** (today the error is printed and the
  run reports unfiltered, exit 0). A clean run's `--generate-baseline` writes an
  empty baseline, a valid result; paths that match nothing already fail the run.

Fixes:

- **`--fix` reports what it left.** The diagnostics it fixed leave the printed
  report (today the pre-fix report prints after fixing, as if nothing had been
  fixed); a file whose content changed since it was analysed is skipped, not
  patched at stale offsets (`apply_fixes` re-reads the file and applies the
  analysis-time offsets to whatever is there); a fix overlapping one already
  applied is reported as skipped (`apply_fixes` drops it silently, and the
  summary counts only what applied); and the fixed files are analysed again and
  fixed again until no fix applies, a fix exposed by another landing in the
  same run. The exit code reflects what remains.

The language server:

- **Positions follow the negotiated encoding.** A `Span` column counts code
  points. The server announces `utf-8` whenever the client offers it and
  otherwise leaves UTF-16, but sends code-point columns either way
  (`to_lsp_position`, `from_lsp_position`): for `return $s;` after `"é𝄞"` on its
  line it sends character 38, where UTF-8 is 42 and UTF-16 39. `Span` carries
  byte offsets beside line and column (syntax), and the VFS's line index
  converts both ways in the negotiated encoding (UTF-8 preferred, then UTF-32,
  UTF-16 otherwise) and replaces `diagnostics::fix`'s `byte_offset` rescan of
  the source. Text sync becomes incremental, the edits applied to the overlay
  through the same index (full-document sync today).
- **An edit's text applies at once; only its recheck waits.** `didChange`
  updates the overlay and its line index immediately, and the debounce delays
  only the analysis. Today the text itself waits: `queue_change` only queues it,
  and `latest_text` is written by `handle_debounced_change` after the 200 ms
  window. So a completion request, which a client sends the moment `>` is typed,
  reads the text from before the `->`, and a hover or definition inside the
  window maps the new position onto the old text. A request on a document with
  an unanalysed change runs that document's recheck first (the edited file is
  checked first anyway) or answers from the snapshot with positions mapped
  through the pending edits.
- **`didClose` restores the file's disk content**, or removes the file when none
  exists (an unsaved new file). It is a no-op today, so a file closed without
  saving keeps being analysed, and its dependents checked, against text that
  exists nowhere.
- **Protocol conformance.** A request before `initialize` answers
  `ServerNotInitialized` and one after `shutdown` `InvalidRequest`; a request
  whose params don't deserialize answers `InvalidParams` (today each handler's
  `serde_json::from_value(…).ok()` turns it into a `null` result,
  indistinguishable from "nothing here"). `lsp` accepts `--stdio`, which generic
  clients (a `vscode-languageclient` using `TransportKind.stdio`) append; the VS
  Code extension omits it today to avoid clap's rejection. Every
  capability-dependent reply checks what the client declared and falls back:
  `documentChanges` and resource operations, diagnostic `relatedInformation`,
  `codeDescription` and `tags`, pull diagnostics, work-done progress,
  `window/showMessageRequest`, snippet completion text. The server's own log
  goes through `window/logMessage` and `$/logTrace` at the level `$/setTrace`
  asks for, where today most of it is `eprintln!` to stderr, which a client
  shows, if at all, in a raw output channel.
- **Untrusted workspaces.** In an untrusted workspace (the client's trust state,
  passed as an initialization option) the server doesn't connect to a database
  the workspace's config names (ext-sql's live reflector reads the DDL schema
  only) and says so once. Today opening a repository whose `phpstan-rs.toml`
  names a database connects to it with the credentials in the file.
- **The VS Code extension follows the server** (`editors/vscode`), one commit
  with its `extension.test.ts` cases:
  - the document selector adds `untitled:` PHP buffers, which the server now
    analyses, and `phpstan-rs.toml`, so an unsaved config edit is validated
    live; today it is `{ scheme: "file", language: "php" }` only;
  - `configPath` and `enable` are read per workspace folder, as their
    `"scope": "resource"` declares, and the server pulls them per folder; today
    `startClient` calls `getSettings()` with no folder, reading one value, and
    `buildInitializationOptions` resolves `configPath` against
    `firstFolderPath()`, the first folder alone;
  - `enable` has one meaning, a folder's analysis off: the server keeps no
    Session for a disabled folder, and the extension stops the server only when
    every folder is disabled. Today the extension stops the whole server, while
    the server's own `enable` flag ("stop showing squiggles", hover kept) is
    never reached;
  - a changed `configPath` or `enable` applies through the server's
    `reconfigure` with no restart; only `serverPath` still offers one (today all
    three prompt "Restart Server");
  - activation adds `workspaceContains:**/phpstan-rs.toml` and
    `workspaceContains:**/composer.json`, so a workspace's diagnostics appear
    before a PHP file is opened; today only `onLanguage:php` activates it;
  - a server found through `serverPath` or `PATH` whose `serverInfo.version`
    differs from the extension's (they are released in lockstep, and the
    downloaded one always matches) is named once in a warning;
  - `capabilities.untrustedWorkspaces` is `limited`, the trust state passed as
    an initialization option, and the stub content provider (F5) is declared.

**F2. Result cache and dependencies.** Today nothing persists between runs, and
the LSP's invalidation is syntactic. `deps.rs` collects the names a file spells
in expressions, declarations, catch clauses, attributes and native type hints,
as written (no namespace or `use` resolution, nothing from PHPDoc);
`Project::dependents` maps their bare names to files;
`transitive_affected_files` rechecks the closure of files reachable through
declares, is-referenced-by. That misses a file naming a class only in PHPDoc:
after `class Foo` loses `x()`,
`/** @param Foo $f */ function a($f) { return $f->x(); }` keeps showing nothing
in the editor while `check-project` reports `method.notFound` (a file naming
`Foo` in a native hint is rechecked). It also over-rechecks, a bare `Foo`
matching every namespace's. And every edit reruns all the registry passes over
the whole registry (§1), which C4's memo already removes.

- **Resolved dependencies.** A file's dependencies are what its analysis
  resolved: the folded class, function and constant names its reflection lookups
  read, absent ones included (C4's memo read sets, recorded per analysed file by
  a dependency observer), mapped to their declaring files through the VFS's
  declaration sets; the files its `include`s resolve to and the paths its
  existence checks probe (`includes.rs`); and, for a trait's host, the trait's
  file, since the trait's methods are analysed in the host. A name that didn't
  resolve stays an edge, so a file created later that declares it rechecks its
  readers, and a deleted file's readers are rechecked and its cache entry
  dropped. `deps.rs`, `Project::dependents`, `register_references`,
  `transitive_affected_files`, `transitive_affected_files_from` and `bare_name`
  are deleted.
- **Fingerprints.** A file's exported fingerprint covers everything another
  file's analysis can read from it: its declarations with native and PHPDoc
  signatures, constant and enum case initializers and attributes; its traits'
  method bodies; the summaries C9 computes from its bodies (purity,
  global-variable writes); and the collected data other files' rules consume.
  Fingerprints are kept per exported name, as dependency edges are: an edit that
  keeps every one rechecks that file only, and one that changes a class's
  signature rechecks that class's readers, not every reader of the file's other
  declarations; each rechecked file's own fingerprints decide whether it goes
  further.
- **Incremental collectors.** Collected data is cached per file; a run
  recollects the analysed files and reruns the project-level rules over the
  merged data. A fact producer → consumer edge keyed by host class (another
  file's writes to a private property, read by `property.unusedType`; a trait's
  findings per host) rechecks the consumer when the producer's contribution
  changes.
- **The persistent cache.** Per analysed file it stores the content hash
  (BLAKE3), the raw errors (before the finalizer), the inline-ignore records,
  collected data, dependencies, the fingerprint and the summaries. A run
  analyses only the files whose content, or whose dependencies' fingerprints,
  changed. Context files (scan and stub paths, vendor code outside the lock) are
  nodes of the same graph, hashed and fingerprinted, so editing one rechecks its
  readers instead of discarding the cache. The rest come from the cache, and
  their declarations are read lazily through the locator (C5) from a cached
  per-file name index, not parsed.
  - *Key*: a meta hash of the binary's identity, as
    `CRASH_REPORTING_PLAN.md` §4.2 defines it (the version, the release commit
    from `PHPSTAN_RS_BUILD_COMMIT` when the release workflow set it, the GNU
    build ID, and the executable's size and modification time), so a rebuilt
    development binary never reads a stale cache, and a cache format version;
    the stub index (stub-sources) and the user stub files; the config, minus
    the keys only the finalizer or the formatters read (`ignore_errors`, the
    baseline, `report_unmatched_ignored_errors`, the output settings); the PHP
    version range and the level; the analysed paths; each discovered root's
    `composer.json`, `composer.lock` and `installed.json` (composer hygiene and
    the PHP version range read the manifests); the SQL schema file and, when
    ext-sql reflects a live database, its schema fingerprint, a hidden input
    otherwise; and each extension's `ResultCacheMeta`.
  - *Entry sets*: the project's cache directory holds one entry set per meta
    hash, not one overall, so configurations that alternate on one machine (the
    editor at the config's level beside `analyse --set level=9`, a PHP-version
    matrix run locally) each keep theirs instead of evicting one another. The
    four most recently used sets are kept, an older one is deleted when a fifth
    is written, and a set's entries for files that no longer exist are dropped
    when it is written. Entries hold project-relative paths, so a cache restored
    into another checkout location (a CI cache) still applies.
  - *Location*: under `tmp_dir`, one cache per canonical project root and
    config file, or at `result_cache_path` when set (phpstan-rs keys; `init`
    converts PHPStan's `tmpDir` and `resultCachePath` to them). `tmp_dir`
    defaults to the user cache directory, `$XDG_CACHE_HOME/phpstan-rs` or its
    platform equivalent, from `CRASH_REPORTING_PLAN.md`'s
    `crash_report::cache_dir()`, which F2 moves down into `project`, where the
    result cache can reach it, the crash reporter's outbox calling it there. It
    keeps that function's fallback to a per-user `0700` temporary directory when
    no cache directory is writable and its `_PHPSTAN_RS_CACHE_DIR` override for
    parallel tests; with neither writable the run goes uncached, with a warning.
  - *Safety*: the directory is created with mode `0700` on Unix, since cached
    errors quote source. A config file travels with its repository, so a cloned
    repository's `phpstan-rs.toml` must not choose where its reader's machine
    writes: a `tmp_dir` or `result_cache_path` from a config file resolves
    inside the project root or the user cache directory and is refused
    otherwise, while the same keys from `--set` or the environment may point
    anywhere. The cache writes only inside a directory it created, marked by a
    `CACHEDIR.TAG`, and never replaces a file it didn't write, so
    `result_cache_path = "~/.bashrc"` fails the run instead of overwriting the
    file; `clear-result-cache` deletes only such a marked directory.
  - *Integrity*: the cache is written atomically (a temporary file renamed over
    it) under a lock (`std::fs::File::lock`), so concurrent runs can't corrupt
    it; an unreadable or version-mismatched cache is discarded with a warning;
    a file with an internal error isn't cached.
  - *Bypass*: `--no-result-cache` skips it and the `clear-result-cache` command
    deletes it. A run whose output needs every file walked (the type dump,
    `--emission-order`, `--debug`) reads no cached result but still writes the
    cache, and the fixture harness never uses one.
- **The LSP uses the same graph** for `update`, loads the cache at workspace
  open, so reopening an unchanged workspace analyses nothing, and writes back
  the results of files whose overlay matches the disk; an overlay's results are
  never cached.

The gates run cold (`--no-result-cache`), so their snapshots and timings never
depend on what a previous run left on disk; the cache has its own gates. Gate:
the cache closure gate. A sequence of edits applied through cached runs gives,
at every step, exactly a cold run's result (a property test over the fixture
directories); on the corpus, a warm run after no change equals the cold run and
analyses no file. The LSP replay gate (gate 7) goes on.

**F3. ErrorFormatter set.** Today's five formats (`text`, `friendly`, `json`,
`checkstyle`, `junit`) live in `output.rs` and see only per-file diagnostics.
An `ErrorFormatter` (§3.10) takes the finalized result (file, not-file and
internal errors, warnings, stats) and an `io::Write`, and returns its write
errors; the exit code is the app's, decided from the result (F1), never a
formatter's. Third parties register formatters through `app::run`'s
registration closure.

The formats:

- **Two human formats, not four**: nothing is released, so today's `text` and
  `friendly` merge into PHPStan's names instead of standing beside them. `raw`
  is one line per error, `path:line:col: message (identifier)` as `text` prints
  it today (PHPStan's `raw` has no column, and the identifier only when
  verbose), a tip line under it, and not-file errors, internal errors and
  warnings as `?:?:` lines. `table` groups per file with tips, identifiers and
  `editorUrl`/`editorUrlTitle` links (`%file%`, `%relFile%`, `%line%`), and on a
  terminal shows each error's code frame as `friendly` does today. The default
  is `table` when stdout is a terminal and `raw` otherwise, so a pipe or an
  editor's problem matcher gets one line per error with no flag.
- **CI formats**: `github`, workflow commands carrying `file`, `line`, `col`,
  `endLine` and `endColumn` from the span and the identifier as `title`
  (PHPStan writes `col=0`), warnings as `::warning`, and the whole report as a
  Markdown table in `$GITHUB_STEP_SUMMARY` when it is set, since GitHub shows at
  most 10 error and 10 warning annotations per step; `gitlab` Code Quality JSON,
  whose fingerprint hashes the relative path, identifier, message and the
  finding's index among equal ones, so it survives a line shift that PHPStan's
  file-line-message hash doesn't, a non-ignorable error being `blocker`;
  `teamcity`; `ci-detected`, GitHub Actions or TeamCity from the environment and
  `table` otherwise; `junit` and `checkstyle` with real columns, warnings and
  not-file errors included.
- **`json` and `prettyJson`** in PHPStan's shape: `totals.errors`/`file_errors`,
  `files.<path>.errors` and `messages[]` (`message`, `line`, `ignorable`, `tip`,
  `identifier`), and `errors[]` for not-file errors.
- **`phpstan-rs-json`**, today's JSON (`totals.files_checked`/`diagnostics`,
  `column`, `level`) renamed and completed into the full record (span start and
  end, tips, fixes, non-ignorable, internal errors, warnings, stats), the
  schema ROADMAP Phase 181's `report` serves. The corpus gate switches to it in
  the same commit: `xtask`'s per-level and performance runs (`--format json`
  today), `tools/gate-runner`'s `render_json` call, and the commands in
  `.corpus-runs/README.md`; Phase 181's `report --export` later replaces the
  runner (`GATE_SPEED_PLAN.md` S9c).
- **`sarif`** (SARIF 2.1.0, which GitHub code scanning and most security
  dashboards ingest): a rule per identifier, each result with its region from
  the span's start and end, its tip, and its fix as a SARIF `fix`.

Across formats:

- **Paths**: the human and CI formats print a path relative to the working
  directory when it lies under it, else absolute; `json`, as PHPStan's, keys
  files by absolute path; `phpstan-rs-json` keeps paths relative to the project
  root, so the corpus gate's output doesn't depend on the checkout's location
  (today each format prints the run's own spelling, `./b.php`).
- **Warnings**, SQL schema drift among them, reach the CI formats; today they
  go to stderr only.
- **Several outputs from one run**: `--format` repeats, each `name[=path]`, so
  one analysis writes the console report and a CI file
  (`--format table --format junit=report.xml`) instead of running twice; one
  without a path writes to stdout, and at most one may.
- **Write errors**: a closed pipe ends the run quietly with exit code 141 (128 +
  `SIGPIPE`, what a shell reports for a program the signal ended), and any
  other write error is printed and exits 2. `CRASH_REPORTING_PLAN.md`'s step 1b
  lands this early for today's formats, and every F3 formatter keeps the
  contract. Today `check-project | head -1` panics ("failed printing to stdout:
  Broken pipe") and exits 101.
- **Colour** follows the terminal: on when the output is a terminal, off under
  `NO_COLOR`, forced by `FORCE_COLOR` or `--ansi`, disabled by `--no-ansi` (only
  `friendly` reads `NO_COLOR` today).

**F4. Binary.** Today the bin has six subcommands (`check`, `check-project`,
`incremental-bench`, `lsp`, `demo`, `convert-baseline`), and `check-project` has
15 flags. `main/check_project.rs` holds reporting logic (baseline generation,
fixes, the text summary, the format dispatch), and its error paths call
`process::exit`, skipping the dhat and hotpath flushes `main`'s comments rely
on.

- **`main` parses, `app::run` runs.** The flags are parsed in the bin and passed
  to `app::run(args, extra)`, which returns the exit code; `main` returns an
  `ExitCode`, and nothing calls `process::exit` outside the crash reporter's own
  roles (`CRASH_REPORTING_PLAN.md` §4.1: the reporter and the finisher exit
  through `crash_report::exit`, which flushes first).
- **The commands.** The analysis command is `analyse`, with `analyze` as its
  spelling alias, as PHPStan names it; nothing is released, so `check-project`
  goes rather than staying beside them, and xtask, the gate runner's and
  `.corpus-runs/README.md`'s commands, `tests/cli.rs`, the README and the VS
  Code docs move in the same commit. Beside it: `lsp` (accepting `--stdio`),
  `clear-result-cache` and `completions <shell>` (`clap_complete`), plus the
  crash reporter's hidden `__crash-finish` and `__crash-replay` and its
  supervisor and worker roles (`CRASH_REPORTING_PLAN.md` §4.1, §4.10, §6.5);
  Phase 183's `init` joins them. `convert-baseline` goes with the old baseline
  format (F1). `check` and `demo` are deleted, their roles moving to the syntax
  bench, the fixtures and a `project` example; the tests that run `check` move
  onto `syntax::parse` in the same commit (`tests/cli.rs`'s every-file-parses
  case, and `tests/scaling.rs`'s long-line case, which times the parse of 20,000
  and 80,000 statements on one line and becomes a `syntax` test).
  `incremental-bench` becomes `project/examples/incremental.rs`, over any
  directory, plus a criterion bench.
- **A flag sets a run's shape; a config key goes through `--set`**, as §3.6 has
  for the per-rule flags. The flags are `--config`, `--set`, `--level`,
  `--format`, `--generate-baseline`, `--fix`, `--no-result-cache`,
  `--no-progress`, `--ansi`/`--no-ansi`, `-v`, `--debug`, and the hidden
  `--emission-order`, which the determinism check runs on the shipped binary.
  Today's `--php-version`, `--no-composer`, `--scan-dir`, `--stub-path`,
  `--baseline`, `--baseline-match-identifier` and `--baseline-per-identifier` go
  for `--set php_version=8.4`, `--set no_composer=true`,
  `--set 'scan_paths=["lib"]'` and the rest (nothing is released), and xtask,
  the gate runner and the tests that pass them move in the same commit. The only
  flags that set a config value are PHPStan's own (below): `--level`, its `-l`,
  and `--autoload-file`. The library follows: `app::check_project::CheckRequest`
  keeps the config file, the `--set` assignments, the paths, the level and the
  emission order (its `scan_paths`, `stub_paths`, `php_version`, `no_composer`,
  `baseline`, `baseline_match_identifier` and `baseline_per_identifier` go);
  `project::session::Overrides` keeps the paths, the level and the emission
  order (F1 already removes `report_level`); and `Session::for_fixture` turns
  composer discovery off by setting `no_composer` on the fixture's config
  instead of through an override. Every other value reaches the Session as a
  config key, one path for the command line, the fixtures and the gate runner
  alike.
- **PHPStan's command line works as is**, so a CI script or tool that runs
  `vendor/bin/phpstan analyse` can run phpstan-rs instead: `analyse`/`analyze`
  is the command itself; `-c`/`--configuration`, `-l`/`--level`,
  `-b`/`--generate-baseline` (its value optional: the config's `baseline` path
  when set, else `phpstan-rs-baseline.json`), `--error-format` (`--format`) and
  `--autoload-file` (scanned as context) take PHPStan's meaning; `-v` adds the
  run's statistics, elapsed time and peak memory, and internal errors'
  backtraces without `RUST_BACKTRACE`, `-vv` each phase's timing, `-vvv` each
  file's; `--debug` analyses one file at a time on one worker, printing each
  path before it starts, so a crash no isolation can catch (a stack overflow
  aborts the process) names its file; `--memory-limit` and `--xdebug` are
  accepted and reported once as ignored. `-c` naming a `.neon` file fails,
  pointing at `init` (ROADMAP Phase 183).
- **No gate-only flag** in the shipped binary (`GATE_SPEED_PLAN.md` S9c): the
  type dump is a Session option the gate runner sets through the library (and
  ROADMAP Phase 181's export after it), so the binary's hidden `--dump-types`
  and its `tests/cli.rs` case go; xtask already passes `--dump-types` to the
  runner, not the binary.
- **Contradicting flags are rejected**: `--fix` with `--generate-baseline`
  (today the baseline is written and `--fix` silently does nothing).
- **Progress**: a line of files analysed of total goes to stderr when it's a
  terminal, off with `--no-progress` and when stderr isn't one; today a run
  prints nothing until it finishes.
- **Ctrl-C** cancels the run's token: analysis stops at the next statement
  boundary, the files finished so far are cached, and the process exits 130.
  Today an interrupted run loses everything.
- **`--version`**, `-V` as PHPStan's console spells it, prints the version
  and, for a release build, its commit
  (`PHPSTAN_RS_BUILD_COMMIT`), else `+dev` with the build ID; no build script
  reads `git rev-parse`, whose output changing on every commit would recompile
  the crate and everything above it (`CRASH_REPORTING_PLAN.md` §4.2). It is the
    identity F2's cache key records. `-v` is verbosity (above), so today's `-v`
  alias of `--version` goes, and `tests/version.rs`'s
  `dash_v_behaves_identically_to_dash_dash_version` becomes the `-V` case.
- The `format_signature` test moves to `reflection`.
- `tests/cli.rs` covers every format's output and exit code on a fixture tree
  holding a finding, a parse error, an internal error (the panic-injection
  hook) and a warning.

**F5. Language features**, on F1's Session and snapshots, F2's dependency graph,
E1's reference-site collector and E6's observers (Appendix F), each its own
commit with its `lsp/tests` cases (the harness driving the Session in LSP mode)
and held to gate 7. The structural query comes first:

- **One position query.** Hover, definition, highlight, signature help and
  completion find what is at a position through one node-at-position query over
  the `Visitor` (the innermost node with its ancestor chain) on the recovered
  tree, answered from the analyser's resolved names and types. It replaces the
  hand-written per-statement walkers each keeps today (`navigation::Walker`,
  whose `collect_all` also serves highlights, `signature_help`'s own walker and
  completion's token scan), which the module docs say are "Structured after (not
  sharing code with)" `deps.rs` and each other, so a node one of them doesn't
  descend into is invisible to that feature alone. The use-site index is E1's
  reference-site collector (Appendix A).

Navigation:

- **Every name.** Definition, type definition, hover, document highlight,
  references and rename resolve a name in a PHPDoc type (C7's name scope:
  `@param`, `@var`, `@return`, `@throws`, `@extends`/`@implements`/`@use`
  arguments, `@method`, `@property`, `@mixin`, template bounds) and in a `use`
  import as they do one in code. Today, in a file with `use App\Foo;` and
  `/** @param Foo $f */`, definition and hover answer `null` on both, and
  references to `App\Foo` list neither.
- **The member kind the syntax names.** `Foo::X` is a constant, `Foo::x()` a
  method and `Foo::$x` a property, separate namespaces in PHP; today
  `Resolved::Member` carries no kind and the caller tries a method, then a
  property, then a constant, so in a class declaring both `x()` and `const x`,
  definition and hover on `Foo::x` go to the method.
- **Every declaration.** A name several files declare (a polyfill beside the
  built-in, conditional declarations, the duplicates C4's collision policy
  merges) answers each site, and a built-in's or embedded stub's declaration
  opens as a read-only document under a `phpstan-rs-stub:` URI (LSP 3.18's
  `workspace/textDocumentContent`, and the VS Code extension's content provider
  for older clients). Today `class_declaration` and `function_declaration`
  answer only an `Origin::File`, so definition on `DateTime` or `array_map`
  answers nothing, and a name declared twice answers one site.
- **Hierarchy**: `textDocument/implementation` (an interface's or abstract
  method's implementations, a class's subclasses), `typeHierarchy` (supertypes
  and subtypes) and `callHierarchy` (callers and callees), answered from
  reflection's `ClassHierarchy` and E1's reference-site collector; none is
  served today.
- **The outline is complete**: promoted constructor properties are listed as
  the class's properties (`private float $r` in `__construct` is missing
  today), and declarations below the top level (C4's indexed ones in function
  bodies, `try` and `switch`) are listed where they sit; today only `if` and
  block nesting is followed.

Hover:

- **The declaration**, as Markdown when the client's `hover.contentFormat`
  accepts it: the declaration written as PHP (modifiers, templates, parameters
  with their native and PHPDoc types and defaults, the return type), a
  constant's or enum case's value, the doc comment's description with its
  `@deprecated` and `@throws` tags, and where the symbol comes from (the
  project, a composer package, a built-in). Today it is a bare
  `MarkedString::String` (`class App\Foo extends Base`, `Foo::X: int`) with
  nothing from the doc comment.
- **At a write, the written value's type**: on `$x` in
  `$x = make(2.0, true);` it answers `mixed` today, while the inlay hint on the
  same `$x` says `App\Circle`.

References and rename:

- **References are complete; rename touches only what it may rewrite.** Targets
  are keyed by C4's folded names, so `new foo()` is a reference to `App\Foo`:
  today the index keys each site by the name as written, and a differently cased
  spelling is neither found by references nor rewritten by rename. `self::`,
  `static::` and `parent::` sites are references to their class, listed by
  references and highlights but marked read-only for rename, which keeps them as
  written; today `use_sites::resolve_class_ref` drops them from the index
  altogether, so references to `Foo` miss every `self::` in its body and every
  `parent::` in its subclasses.
- **Rename produces code that still resolves.**
  - Each site is rewritten in its written form. A fully-qualified or
    namespace-relative name keeps its qualifier; a name reached through an
    import renames the import, and the uses under an alias stay; a name in its
    own namespace renames its last segment. Today every site is replaced by the
    bare new name: renaming `App\Foo` to `Bar` turns `\App\Foo` in
    `namespace Other` into `Bar`, which resolves to `Other\Bar`, and leaves
    `use App\Foo;` pointing at nothing while `b(Foo $g)` becomes an unimported
    `b(Bar $g)`. This replaces F1's interim refusal.
  - A rename that would collide with a name already declared or imported in a
    file is refused, naming the file.
  - The new name is checked as an identifier PHP accepts for that kind of
    symbol (`1 bad` is applied as is today, and a class can't be named `self`,
    `int` or another reserved name).
  - `prepareRename` answers the range and placeholder, and refuses a symbol
    declared in a stub or a context file, whose rename would edit vendor code
    (`method not found` today).
  - Methods, properties, class constants and enum cases rename through the
    member index, across the hierarchy (an interface method with its
    implementations and overrides, a `@method` or `@property` tag with its
    uses); today a member rename returns `null`. Local variables and parameters
    rename within their function, a parameter's named arguments at its call
    sites included.
  - A class whose file is named after it under a PSR-4 root renames the file
    too, when the client supports resource operations, and
    `workspace/willRenameFiles` renaming such a file offers the class rename as
    its edit.
  - The edit is a versioned `documentChanges` edit, so a client rejects it
    against a document that changed since, where today it is an unversioned
    `changes` map.

Editing aids:

- **Completion reads the scope at the cursor.** It asks the Session for the D6
  scope at the position in the recovered tree, where today `lsp/completion.rs`
  re-lexes the live buffer, finds the enclosing function by name, clones its
  last good body, truncates it at the edit line and splices in a probe
  statement. In `function use_it(Shape $s, Circle $c)`, `$c->` offers nothing,
  as does `$c->a` (unedited, just edited and half typed alike), and `Circle::`
  offers `__construct` and the instance method `area`, both of which throw an
  `Error` when called statically, but not `class`. The target:
  - `->` offers the instance members visible from the calling scope (a private
    one only inside its class, a protected one inside the hierarchy),
    `@method`/`@property` tags and mixins included;
  - `::` offers static methods, static properties, constants, enum cases and
    `class`, with `parent::`/`self::` also reaching the parent's instance
    methods, as PHP allows;
  - the names after `new`, `instanceof`, `extends`, `implements`, `catch` and
    type positions are class-likes of the fitting kind, with the auto-import
    edit the bare completion already makes;
  - a call's argument list offers its parameters' names for named arguments,
    and `$` offers the variables defined at that point.
- **Signature help shows the whole signature**, callable-typed values included
  (`$cb(` on a `Closure(int $n): void`, a first-class callable, an invokable
  object's `__invoke`, which have no signature to show today since only a named
  callee resolves): default values, by-reference and variadic markers and the
  PHPDoc types (`make(float $radius, bool $cached)` for `bool $cached = false`
  today), the active parameter following named arguments as well as positions,
  and every variant of a function C6 keeps several of.
- **Semantic tokens classify every resolved name**: namespaces, classes,
  interfaces, enums and enum cases, traits, templates as type parameters,
  functions, methods, properties, constants, parameters and variables, with the
  `declaration`, `static`, `readonly`, `abstract`, `deprecated`, `internal` and
  `defaultLibrary` (a built-in) modifiers, served for a range and as deltas too.
  Today a token exists only for a deprecated or internal usage (E's shared
  predicate widens which), so a file using neither answers `{"data": []}` and
  the editor keeps its grammar's guesses (a constant and a class look alike
  after `::`).
- **Inlay hints come once per site, from the final pass.**
  `inlay_param_name_hints` and `inlay_local_type_hints` are pushed on every
  walk: each of a loop body's convergence passes (up to six), the quiet pre-pass
  over a `while` or `for` condition, and each host of a trait method. Nothing
  drops the earlier walks' hints the way `converge_loop_body` truncates their
  diagnostics, and `filter_to_range` doesn't deduplicate, so a `$x = f();`
  inside a loop gets one type hint per pass, each with that pass's intermediate
  type. With E6's observers they come from the kept walk's events only, a trait
  method's merged across its hosts.
- **Inlay hints say what the text doesn't.** No parameter-name hint on an
  argument that is a variable of the parameter's name (`make($radius)`), and no
  type hint where the right side is a literal or a `new Foo` (today every
  non-`mixed` assignment gets one); names print as the file imports them
  (`Circle`, not `App\Circle`), the full name in the tooltip and the label part
  linking to the declaration; a by-reference parameter's hint reads `&name:`;
  and type hints also cover `foreach` keys and values, `list()`/`[...]`
  destructuring, `catch` variables and untyped closure and arrow-function
  parameters, where today only an assignment statement's plain variable gets
  one.

Code actions:

- **Quick fixes follow §3.7's fix list**: each of a diagnostic's fixes is its
  own action, the first preferred, and a fix editing several places (an import
  added beside the use it enables) is one edit. Today `Diagnostic.fix` is an
  `Option`, so a diagnostic offers at most one single-edit action, always marked
  preferred.
- **Fix all** (`source.fixAll.phpstan-rs`) applies every fix in the document at
  once, which editors can run on save; today each quick fix is offered alone.
- **Ignore**: every ignorable diagnostic offers "Ignore `<identifier>` on this
  line", adding the identifier to a `@phpstan-ignore` comment on the line (or
  the line above), extending one already there; non-ignorable ones don't.

Diagnostics:

- **Pull diagnostics.** A client offering `textDocument/diagnostic` and
  `workspace/diagnostic` pulls them, with `interFileDependencies` and
  `workspace/diagnostic/refresh` after an update changes other files' results;
  push stays for the rest.
- **Diagnostics carry what the protocol has fields for**:
  `DiagnosticTag::Deprecated` on the deprecation rules' findings and
  `Unnecessary` on dead and unused code (identifiers declared with the tag by
  their rules); a finding naming other sites (a duplicate declaration's other
  declarations, a namespace cycle's edges) lists them as `relatedInformation`,
  where today they are text in the message; `codeDescription` links each
  identifier to its page in the rule reference (ROADMAP Phase 185); and the tip
  is a `relatedInformation` entry at the finding's own span instead of being
  appended to the message text.

Gates, over the corpus, each recorded under `.corpus-runs/gate/lsp/<hash>` and
held to the previous commit's:

- **navigation consistency**: for every site E1's reference-site collector
  records, definition answers the site's declaration, and references from that
  declaration list the site;
- **rename round trip**: for a fixed sample of classes, functions, methods,
  properties and constants, applying rename's edit leaves every file's
  diagnostics unchanged except for the renamed name in messages, and renaming
  back restores the original bytes;
- **completion recall**: at every `->`, `?->` and `::` site in the analysed
  files, with the text cut after the operator, the member the site names is
  offered, and nothing offered would be an error there (a private member outside
  its class, an instance method after `::` outside `parent::`/`self::`);
- **positions**: fixtures with multi-byte and astral characters in names,
  strings and comments round-trip every published range and every request
  position in UTF-8, UTF-16 and UTF-32.

### Stage G: extraction wave and tidy

Stage G starts when F is done. The lower layers are crates already: `syntax`,
`types`, `phpdoc`, `stub-sources`, `sql`, `diagnostics`, `composer`, `config`
and `reflection` (B, C10), with the sibling plans' `crash-context`, `pcre` and
`crash-sys`, and `neon` once Phase 183 lands. Everything from `analyser` up is
still modules of the one `phpstan-rs` package (`crates/phpstan-rs/src`), each
assigned its crate by Appendix A and held to it by the module-graph gate, whose
`allow` list is empty: every edge and misplaced `impl` it listed (235 edges
today) named a phase up to F, and each phase removed its own. The package's
library is used by its binary, its integration tests (`tests/cli.rs`,
`version.rs`, `lsp_exit_code.rs`, the fixture harness `tests/fixtures.rs` over
8,347 fixture files in `tests/fixtures/{checker,project}`), its bench
(`benches/vendored_corpus.rs`), `tools/gate-runner` and `tools/crash-repro`
(`CRASH_REPORTING_PLAN.md` §5), and the fuzz package's `structured_php` target,
all through `phpstan_rs::…` paths.

G makes a crate of each remaining layer, moves the tests, benches and fuzz
targets to the crates they test, retargets the gates from the package's paths
to the crates', rewrites the docs, and asserts the build-time targets. G
changes no behaviour: the corpus, type-snapshot and baseline gates (4–6) and
every fixture's expectations are byte-identical through every G commit, and a
fixture's or test's name changes only through gate 3's rename manifest. The
performance gate (8) holds each extraction to the previous commit; a function
it shows losing its inlining across the new boundary gets `#[inline]` in the
extraction's commit (the thin-LTO profile, decided in A0, covers the rest).

**G1. Extraction**, one crate at a time in dependency order: `analyser`,
`builtins`, `rules`, the nineteen `ext-*` (one each, in E4's order), `project`,
`testing`, `app`, `lsp`. What remains in `crates/phpstan-rs` is the binary.

Each extraction is one commit that builds and passes every gate: the modules
move with `git mv` to `crates/<name>/src/` and are rewritten in place, and
`git diff -M --summary` shows every moved file as a rename, since rewriting
paths leaves a file far above git's similarity threshold. A file that falls
below it moves first in a pure rename commit, content unchanged (gate 11),
which doesn't build and which a bisect skips. Each module's path in the crate
is Appendix A's module path without the package prefix that named the layer
(`checker::unused` is `rules::unused`), unless §3 names another, and a new
`lib.rs` declares them. The commit holds:

- the crate's manifest (package `phpstan-rs-<name>`, inheriting
  `[workspace.package]`, with `[lints] workspace = true`), its
  `[workspace.dependencies]` alias, and its third-party dependencies moved off
  the package's manifest by Appendix E (`cargo machete` fails on one left
  behind);
- `crate::` paths rewritten to `<name>::` on both sides of the new boundary,
  and intra-doc links with them (gate 1's `cargo doc` denies a broken one);
- the module-graph spec's `module` rows for the crate replaced by an `extern`
  line;
- `gates/test-renames` entries for every test whose qualified name changed
  (`phpstan-rs lib checker::unused::tests::…` becoming
  `phpstan-rs-rules lib unused::tests::…`);
- the crate in gate 9's scenarios, its rebuilds recorded from then on, and the
  hotpath label map for the timing re-measurement (§2);
- `Cargo.lock` with the new member (CI runs `--locked`), and the commit's hash
  in `.git-blame-ignore-revs`.

What an extraction checks:

- **Visibility.** An item used across the new boundary becomes `pub` only when
  §3's design names it as the crate's API. Anything else the other side still
  uses is a design gap, closed before the extraction (the user moves, or §3
  gains the API, in a commit of its own), never by widening. `unreachable_pub`
  then denies any `pub` item the crate doesn't export.
- **`impl` placement.** No entry of gate 2's list names the crate's modules,
  so every `impl` it holds or that others hold of its types compiles where it
  is.
- **Features.** A feature the crate's code tests is declared by the crate and
  forwarded by the bin (§2: `hotpath` for `analyser`'s `measure_outermost!`,
  `crash-injection` for `project`'s hook, `pcre-validate` for `builtins`); a
  `cfg(feature)` naming a feature its crate lacks is an `unexpected_cfgs` error
  under clippy's `-D warnings`.
- **The public API** is snapshotted from the extraction commit on (gate 14), and
  the commit lists its difference from §3's design: an item the design names
  that the crate lacks, or one it exports that the design doesn't name.
- **Standalone build.** The crate builds alone (gate 1's `cargo hack check
  --workspace --no-dev-deps`), so a dependency feature it needs is in its own
  manifest rather than unified in from another member.
- **Tests.** Each `#[cfg(test)]` module moves with its module. A unit test may
  dev-depend only on crates that don't depend on its own: a dev-dependency on
  one that does links a second copy of the crate, whose types don't match the
  test's. So `project`'s unit tests (70 today) may register `builtins`, `rules`
  and any `ext-*`, and `lsp`'s (263) may use `app`, but an `analyser` test
  needing a built-in's typing can't stay a unit test (169 `#[test]` sit under
  `checker` today, its rule modules' included, each checked). Before the
  extraction it becomes a fixture, or a test through the crate's own API, named
  in the rename manifest. An integration test has no such limit: it links the
  crate's ordinary library, so a fixtures target may dev-depend on crates above
  its own (G2).
- **Lints**, active from the extraction commit (§2): `missing_docs` on `analyser`,
  `project` and `testing`; `clippy::print_stdout`/`print_stderr` at `deny` in
  every library crate, the arch gate's `LIBRARY_CRATES` gaining the crate.
- **Comments** follow the comment rules, as Stage B's crates did: a comment
  citing a module the crate can't see, a phase, a fix number or an
  investigation's history is rewritten, the count per crate listed in the
  commit. The package's manifest comments go with the features they describe
  (`pcre-validate`'s cites a phase and a plan file today).

Per crate:

- **`analyser`**: the engine, the Scope API, the `TypeSpecifier`, the
  Rule/Collector/Observer traits and the extension-trait registry,
  `RuleLevelHelper`, `AnalysisContext` and the summaries API, the
  reference-site collector (§3.8). No `composer` dependency and no I/O (its
  module-graph row, and gate 10's rules, already hold it to both).
- **`builtins`**: the `pcre-validate` feature, off by default, linking `pcre`;
  the bin's `default = ["pcre-validate"]` forwards `builtins/pcre-validate`, and
  the tests gated on it move with their modules. `roxmltree` moves here.
- **`rules`**, with the `[rules]` section's settings type (`rules_settings`).
- **Each `ext-*`**: its rules, collectors, extensions, config section and
  settings, and the composer packages whose installation activates it (C1),
  behind one registration function that `app` calls. No `ext-*` crate depends
  on another; the Cargo graph now enforces what the module graph did.
- **`project`**: the Session, `SessionBuilder`, the VFS, the finalizer, the
  result cache and dependency graph, the worker pool, `cache_dir()` (moved down
  in F2); `blake3`, `dunce` and `rayon`.
- **`testing`**: the harness leaves the package's `tests/fixtures.rs` for a
  library: RuleTestCase (with collectors), TypeInferenceTestCase,
  LevelsTestCase, Integration and Codemod (§3.11), and the libtest-mimic runner,
  `testing::run(registrations, fixtures_dir)`. It depends on `project` alone and
  takes its registrations from the caller, so a fixtures target registers what
  its crate needs, builtins always (E2). TypeInferenceTestCase evaluates the
  asserts through a node callback, as PHPStan's `gatherAssertTypes` does,
  rather than through `rules`' `FileAssertRule`, which is what keeps `rules` out
  of its dependencies.
- **`app`**: the composition root (every registration, extension activation),
  the ErrorFormatter set, `run(args, extra)` over already parsed arguments (no
  `clap` in a library crate, §3.7), and the crash reporter's `crash_report`
  module linking `crash-sys` under `crash-reporting`
  (`CRASH_REPORTING_PLAN.md` §5).
- **`lsp`**: the server and its features over the Session and observers
  (`lsp-server`, `lsp-types`, `crossbeam-channel`). Its entry point takes the
  registrations `app` assembles and the server info the bin passes (§2):
  `serverInfo.version` is the binary's version and build identity (F4), where
  today it is the package's `CARGO_PKG_VERSION`.
- **The bin** keeps `main.rs` alone, with `clap`, `clap_complete`, `ctrlc` and
  `dhat`, and the `hotpath`, `hotpath-alloc` and `dhat-heap` features; the
  package has no library target. Every `phpstan_rs::` user is repointed:
  `tools/gate-runner` to `app` and `project`, `tools/crash-repro` to `app`, the
  bench and fuzz target (G2) to `project`.

After the last extraction the gates that read the package's source move to the
crates, in one commit:

- **Module graph (gate 2).** The spec keeps its `crate` rows and loses the
  `module`, `allow`, `extern` and `finished` rows, and `xtask modgraph` stops
  walking `crates/phpstan-rs/src`: the gate checks that each workspace crate's
  normal and build dependencies on other workspace crates are within its row.
  `testing` appears in no row, being a dev-dependency only, and the bin's row
  shrinks to what `main` uses (`app`, `lsp`), where today's lists every
  crate, `testing` included.
- **Architecture (gate 10).** The engine-I/O rule covers every crate but
  `project`'s session module, `app`, `lsp`, the bin and tests, where
  `is_engine_source` reads paths under `crates/phpstan-rs/src` today; the rules
  scoped to the analyser read `crates/analyser`.
- **Build time (gate 9).** Each scenario's file moves to the crate it stands
  for (G4).

**G2. Tests, benches and fuzz targets to their owners.**

- **Fixtures.** Each fixture moves, by one mechanical map, to the lowest crate
  whose fixtures target registers everything it needs: the crate declaring each
  expected identifier's rule, or the rules its header scopes it to when it
  expects none. Type-inference fixtures (asserts only) go to `analyser`, whose
  fixtures target dev-depends on `testing` and `builtins`; fixtures of core
  rules to `rules`; fixtures of one extension, with or without core rules, to
  that `ext-*`, whose target registers `builtins`, `rules` and itself;
  Integration fixtures (every rule at a level) and fixtures expecting several
  extensions' identifiers to `app`, the one crate that registers everything, so
  no separate test crate is needed. The directory layout under each crate's
  `tests/fixtures` keeps the file's path below its old family directory.
- **Fixture targets.** Each owning crate has `[[test]] name = "fixtures"`,
  `harness = false`, a `tests/fixtures.rs` of a few lines calling
  `testing::run` with its registrations, and discovery anchored on its
  `CARGO_MANIFEST_DIR`; tests are named `<crate>::<path>` and run with
  `cargo test -p <crate> --test fixtures -- <filter>` (§3.11). Gate 3's
  manifest maps every old fixture name to its new one, and the fixtures' own
  expectations don't change.
- **Fixtures in plain `cargo test`.** The package's fixtures target is
  `test = false` today and runs in the `gate` profile, because each fixture
  builds a whole `Project`, stubs included (about 210 ms of its setup clones the
  stubs and reruns the registry passes over them): a debug run takes about 26
  minutes, the gate profile 483 s (`GATE_SPEED_PLAN.md` §2). With C5's
  process-wide stub layer a fixture's setup no longer builds the stubs, so the
  owning crates' targets drop `test = false` and `cargo test --workspace` runs
  every fixture in the debug profile, once that run takes no longer than the
  gate profile's suite does at the time; the commit records each crate's debug
  wall time, and until the bar is met the targets stay in the gate profile.
  `default-members` keeps the bin, so a bare `cargo test` runs only the binary's
  tests, and the documented command is `cargo test --workspace`. CI's two
  fixture steps (`cargo test --profile gate --test fixtures`, with and without
  default features) fold into its workspace test steps, and
  `GATE_SPEED_PLAN.md`'s fixture command follows.
- **The binary's own tests** stay in the bin package: `tests/cli.rs`,
  `version.rs`, `lsp_exit_code.rs` and their `common` helper, which run the
  built binary (`CARGO_BIN_EXE_phpstan-rs`).
- **The bench.** `benches/vendored_corpus.rs` moves to `project/benches` and
  runs `Session::run` over `stub-sources`' embedded sources, read from memory,
  where today it walks `vendor/phpstorm-stubs` and `vendor/phpstan-src/src` on
  disk and drives `Project::load_file`, `finish_loading` and `check_all`. With
  the syntax microbench (B3) and F4's `project/examples/incremental.rs` and
  criterion bench, gate 1's `cargo bench --no-run` becomes
  `cargo bench --workspace --no-run`.
- **Fuzz.** `structured_php` depends on `project` (its one-file parse through
  the Session) instead of the package, and its hook swallowing the
  `MAX_EXPR_DEPTH` panic goes: B made the limit a `ParseError`, and F1 removed
  it. `fuzz/Cargo.toml` depends on `syntax`, `phpdoc` and `project`.
- **Property-test regressions** live with their tests' crates (`types`' since
  B4; any the package still holds move with their modules).

**G3. Docs.** Every document that describes the package's layout or cites its
paths is rewritten against the crates:

- the README: the build, test and profiling commands (the fixture command per
  crate, `--lib` for white-box tests, the bin forwarding the profiling
  features), and an architecture section from §2's crate map;
- ROADMAP: 61 lines cite `src/` paths today, and its current-state text
  describes one package;
- the fuzz guide (`fuzz/GUIDING_THE_FUZZER.md`);
- the five `.claude/workflows/*.js` files, which cite package paths and cargo
  commands;
- `.corpus-runs/README.md`;
- `editors/vscode/README.md`;
- the `.cargo/config.toml` comment, which justifies mold and `-Z threads` by "a
  crate this large" (G4 measures what holds now);
- the sibling plans still open (`CRASH_REPORTING_PLAN.md`, `UNSAFE_PLAN.md`,
  `GATE_SPEED_PLAN.md`), which cite `crates/phpstan-rs/src` paths;
- the documented test commands, the agent memory's included: the fixture
  command becomes `cargo test -p <crate> --test fixtures -- <filter>`, or
  `--lib` for white-box tests.

A report named by the commit it describes (`PIWIGO_CORPUS_TRIAGE_ab4c2b5.md`)
keeps the paths of that commit. A check joins gate 1's quick tier: every
repository path the living Markdown docs cite exists, so a later move can't
leave them stale; the commit-named reports are exempt.

**G4. Build time.** Gate 9's targets are asserted, over scenarios that now
stand for crates:

- an engine edit (a file in `analyser`) rebuilds nothing below `analyser`;
- a rule edit (a file in `rules`) rebuilds neither `analyser`, `builtins` nor
  `reflection`;
- an extension edit (a file in an `ext-*` crate) rebuilds that crate, `app` and
  the bin only;
- a stub edit rebuilds `stub-sources` and its dependents only, never `syntax`,
  `types`, `phpdoc`, `sql`, `diagnostics`, `composer` or `config`;
- a lexer edit rebuilds everything above `syntax`, and its time is recorded.

Each scenario's `cargo test --workspace --no-run` and
`cargo build -p phpstan-rs` times are compared with the Stage A baseline.
`-Z threads` and mold are measured again, clean and incremental, with and
without, and `.cargo/config.toml` keeps what is faster, its comment stating the
measurement.

**G5. Done.** The plan is finished when:

- `crates/phpstan-rs` holds the binary alone, with no library target;
- the module-graph spec holds `crate` rows only, and every crate's
  dependencies are within its row;
- every Appendix A row names an existing crate, and no row's module remains
  in the package;
- `cargo xtask gates --full` passes, with no approved delta pending, gate 14
  holds a snapshot for every library crate, and CI's run on the final commit
  is green;
- G4's build-time targets hold.

The plan then folds into ROADMAP's Done section, its open items moved into
ROADMAP's phases first, and its working documents go with it once each open
item in them has a ROADMAP phase: `CRATE_SPLIT_EVIDENCE.md` (the
frozen copy kept to check that condensing dropped nothing),
`CRATE_SPLIT_CAUSES.md` (Appendices G–J's generated cause lists),
`CRATE_SPLIT_AUDIT.md` and `STAGE_B_REVIEW.md`.

## 6. Risks

- **Semantic drift in D5.** Each convergence is an approved-delta commit
  with its own fixtures, a triaged Piwigo diff and a type-snapshot diff.
  D1–D4's structural commits must be byte-identical; D3's merge of the two
  inference paths lands only through its checked-in disagreement
  inventory.
- **Dispatch and memory overhead.** Prefilter tables, lazy derived nodes
  and O(1)-clone holder maps, guarded by the per-commit time and memory
  gates relative to the previous commit.
- **Cross-crate inlining.** Thin LTO plus `codegen-units = 1`, decided and
  baselined in A0. The timing gate applies from B1 on.
- **User-visible changes.** Config schema, CLI flags, baseline format, JSON
  output, exit code, output order. Nothing is released, so each changes in
  place, with no shim, converter or version field; each is still its own commit,
  with the Piwigo toml, the gates' snapshots and the docs moved in it.
- **Package build time during D–F.** The engine is redesigned in the
  package before its extraction; the per-commit build-time gate tracks it.

## Appendix A: module assignments

"Split" means rows with different destinations inside one file. Each row
moves in the phase named.

| Module | Destination | Phase |
| --- | --- | --- |
| token, lexer, parser, ast, printer, intern, ast_walk | syntax (ast_walk → Visitor) | B3 |
| text_split | deleted (typed AST sub-name spans) | B3 |
| types/* | types | B4 |
| phpdoc_grammar, docblock | phpdoc | B5 |
| sql_reflection_{config,schema,reflector,schema_drift} | sql | B6 |
| output (formats) | app formatters; `OutputFormat` loses the clap derive | F3 |
| output/codeframe | app formatters | F3 |
| fix | diagnostics (fix model) + project (apply) | B7/F1 |
| baseline, ignore_errors, ignore_comments | project finalizer; glob util → config | F1 |
| composer, composer/* | composer | B8 |
| deps | deleted (resolved dependencies) | F2 |
| config | config | C1 |
| rules_settings | rules (the `[rules]` section's settings type) | C1 |
| php_version | types | B4 |
| php_float_format | types | B4 |
| php_date_parser | builtins | E2 |
| regex_groups, pcre_validate | builtins::regex | E2 |
| checker/preg_pattern_validity | rules (`RegularExpressionPatternRule`, over builtins::regex) | E3c |
| php_stubs + build.rs | stub-sources (sources, index) + reflection::stubs (layer) | B9/C5 |
| stub_attributes | reflection::stubs (the version layer applies them per `PhpVersions` range) | C5 |
| registry/* (index, resolve, hierarchy, generics, relation, name_context, class_info, function_sig, function_table, provenance, reflection_attribute, bodies, type_node, settings, stubs, anonymous_class; C4 adds the builder, the memo levels and the snapshot) | reflection | C3–C10 |
| registry/assignability | types (unified relation) + analyser RuleLevelHelper | B4/C2 |
| registry/assertion_table, assertion_tables | reflection overlays; defaults → ext-nette-tester / ext-mockery | C6 |
| checker/predefined_constants, checker/constant_resolver | reflection ConstantResolver | C2 |
| checker/rule_level_helper | analyser RuleLevelHelper | C2/E3 |
| checker/profiling | analyser (the `measure_outermost!` hotpath helpers) | D1 |
| registry/purity, registry/global_vars | analyser summaries | C9 |
| registry/access_attributes | ext-language-extensions (class-reflection data ext) | C8 |
| registry/class_info/doctrine_entity | ext-doctrine (class-reflection data ext) | C8 |
| restricted_symbols, banned | ext-disallowed-calls (restricted-usage ext); banned constructs → rules | E3g/E1 |
| architecture | rules (collector rule) | E1 |
| parent_calls | rules::class_shape mechanism; defaults → ext-phpunit / ext-codeception | E1/E4 |
| doctrine_functions | ext-doctrine | E4 |
| pest_config | ext-pest | E4 |
| rule, reflection_rule | analyser (Rule API; ported) | D6 |
| project, project/* | project; architecture / banned / class_shape / composer_hygiene / mock_hygiene → rules / ext-mockstan | E1/F1 |
| project/use_sites | built from the reference-site collector (`decl_locations` is gone; the doc comments in `registry.rs`, `dead_members.rs`, `dead_enum_cases.rs` and `use_site_index.rs` that name it are corrected) | E1 |
| lsp (the `Server`, dispatch, `workspace_config`, `settings`, `debounce`) | lsp over the Session: settings assembly deleted, per-root Sessions, the debounce queue folded into the Session's update | F1 |
| lsp/protocol | lsp (positions through the line index, conformance) | F1 |
| lsp/navigation, lsp/signature_help, completion's token scan | replaced by the node-at-position query | F5 |
| lsp/{completion, signature_help, document_symbol, workspace_symbol, inlay_hints, semantic_tokens} and the request handlers | lsp features over the query, the reference-site collector and observers | E6/F5 |
| lsp, lsp/* (what remains) | lsp crate | G1 |
| main, main/* | bin (check_project → project::Session; check / demo deleted; incremental_bench → project example) | F4 |
| checker.rs (Checker, Locals) | analyser (decomposed) | D1–E7 |
| assignment | analyser; the invalid-assignment reports → rules | E3d |
| by_ref_params | rules (parameterByRef.type) | E3d |
| callable_values | rules | E3c |
| calls | analyser (resolution) + rules (argument / arity reports); boolean trap, swapped args → ext-extras | E3c |
| calls/closure_context | analyser (closure typing) + builtins (parameter-closure-type exts) | E2 |
| calls/format_string | rules (printf; strict placeholders as a param) | E3c |
| calls/misc_rules | rules | E3c |
| castable_arguments | rules | E3c |
| coercion | types (methods) + rules (cast / echo / print reports) | B4/E3b |
| comparisons | rules (+ builtins for the in_array / haystack typing) | E3b |
| control_flow, control_flow/boolean_conditions | analyser (flow) + rules; booleansInConditions → ext-strict-rules | E3f/E4 |
| control_flow/generators | syntax (body_contains_yield) + rules (generator.*) | B3/E3f |
| current_time | ext-slam | E4 |
| date_interval, date_time, xml_name, xml_wellformedness | builtins (throw-type extensions) | E2 |
| deprecated_usage | ext-deprecation-rules | E3g |
| doctrine/* | ext-doctrine | E4 |
| duplicate_classes, duplicate_functions | rules (collector rules) | E1 |
| duplicates | rules | E3e |
| expr | analyser ExprHandlers + rules (inline reports); useless cast / strict array filter / strict function calls → ext-strict-rules; loose haystack → ext-extras | D3/E3 |
| expr/array_ops | builtins (returns) + rules (array reports); array key cast → ext-extras | E2/E3 |
| expr/builtin_calls, expr/preg_shapes | builtins | E2 |
| expr/member_resolution | analyser (member resolution); Doctrine / DBAL branches → ext-doctrine / ext-sql | D3/E4 |
| expr/mysqli | ext-sql | E4 |
| expr/probe_checks | ConstantResolver (global_constant_type) + rules (null probes, via TypeSpecifier) | C2/E3b |
| expr/ranges | analyser (int-range arithmetic) | D3 |
| function_decl | analyser (function entry) + rules; empty bodies, never-return suggestions → ext-extras; checked exceptions → ext-extras (throws/missing) | E3e |
| function_like_declarations | rules | E3e |
| generics.rs, generics/substitution | analyser | D3 |
| generics/ancestors, bounds, interfaces, variance | rules::generics | E3e |
| global_constants | rules (constant.notFound) + ConstantResolver | E3b |
| imports | analyser (resolution) + rules; PHPUnit PHAR prefix → ext-phpunit | E3a/E4 |
| impossible_instanceof | rules (ImpossibleCheck on TypeSpecifier) | E3b |
| includes | rules (RequireFileExists over the VFS) | E3a |
| ini_directives | rules (ini.removed) + ext-deprecation-rules (ini.deprecated) | E3b/E3g |
| instantiation, static_call_context, visibility, offset_assignment, operators, pipe_operator, type_declarations, property_default_types, phpdoc_type_compat, phpdoc_unresolvable, missing_typehint, noop | rules | E3 |
| internal_rules | deleted (`InternalRule`); MethodThrowTypeCovarianceRule → rules | D6 |
| internal_usage | rules (core restricted-usage ext for @internal) | E3g |
| keywords, whitespace | rules (file-level) | E3a |
| magic_numbers | ext-sidz | E4 |
| named_argument_renames | rules (collector) | E1 |
| narrowing, narrowing/{accessory_range,array_shape,type_ops} | analyser (ExprHandlers' `specifyTypes`, TypeSpecifier) | D3/D5 |
| narrowing/predicates, narrowing/assertions | analyser (engine predicates) + builtins (built-in type-specifying) + ext-phpunit (assertion narrowing) | E2/E4 |
| native_type_hints | ext-extras | E4 |
| oop, oop/* | analyser (class entry) + rules; parent-constructor-call → ext-strict-rules; promoted-property suggestions → ext-extras; deprecation-gated attribute reports → ext-deprecation-rules | E3e |
| phpstan_api | rules (restricted-usage extension over the package context) | E3g |
| property_assignment | rules | E3d |
| property_type_too_wide | rules (ClassPropertiesNode) | E1 |
| psr3 | ext-psr3 | E4 |
| resolve | analyser (name resolution) + rules (class-existence reports) | E3c |
| restricted_symbol_usage | ext-disallowed-calls | E3g |
| return_type_too_wide | rules | E3f |
| risky_tests | ext-phpunit (methods) + ext-pest (`it` / `test` closures) | E4 |
| semantic_tokens | lsp observer + analyser predicate | E6 |
| sql_reflection/* | ext-sql (DBAL chain folding owned by ext-sql) | E4 |
| stmt, stmt/loops, stmt/switch | analyser + rules; switch missing default → ext-extras; switchConditionsMatchingType, overwriteVariablesWithLoop → ext-strict-rules | E3f |
| stmt/try_catch | analyser (catch) + rules; throw without message, throw base class, exception hygiene → ext-extras | E3f |
| stmt/new_exception_discarded | ext-extras | E4 |
| strict_rules | ext-strict-rules (strict-rules families); comparable types → ext-shipmonk; mixed type operands → ext-voku | E4 |
| strict_idioms | isset / empty on objects → ext-type-perfect; null coercion and enum exhaustiveness → ext-shipmonk; identity vs equals → ext-extras | E4 |
| suggest | rules utility | E3c |
| throws | analyser (throw points) + rules (throws.notThrowable / void); throws/too_wide → rules; throws/missing → ext-extras; throws/dynamic → builtins | E2/E3f |
| type_union | types | B4 |
| uninitialized, uninitialized_dataflow | rules over ClassPropertiesNode (walker deleted) | E3d |
| unused, unused/* | rules | E3e |
| var_tag_types | analyser (native_expr_type → native_type_of) + rules::phpdoc (VarTag checks) | D5/E3e |
| whole_program/* | rules (collector rules) | E1 |
| access_attributes | ext-language-extensions; TEST_BASE_CLASSES → test-framework hook (ext-phpunit / ext-codeception) | E4 |
| tests/cli.rs, version.rs, lsp_exit_code.rs, common | bin package | B1 |
| tests/scaling.rs | syntax (a test over `syntax::parse`; F4 deletes `check`) | F4 |
| tests/stub_index.rs | reflection (§3.4's regeneration test) | C10 |
| tests/fixtures.rs (the harness) | testing (the runner); each owner's `tests/fixtures.rs` calls it | G1/G2 |
| benches/vendored_corpus.rs | project/benches | G2 |
| fuzz targets | lex_parse / token_stream_parse → syntax; parse_type_string → phpdoc; structured_php → project | B3/B5/G2 |
| examples/stress*.php | fixtures or deleted | A2 |
| proptest-regressions | bin package, then types | B1/B4 |

## Appendix B: settings

Every setting that was on `Registry` before C2, with its readers then and
its owner. Non-Registry keys follow the table.

| Setting | Read in | Owner |
| --- | --- | --- |
| strict_mixed_arrays | registry/assignability | RuleLevelHelper param |
| configured_level, check_benevolent_union_types | registry/assignability | RuleLevelHelper / level param set |
| check_uninitialized_properties | checker, uninitialized* | rules param (PHPStan core `checkUninitializedProperties`) |
| check_internal_usage | internal_usage | rules (core restricted usage) |
| check_function_name_case | expr, imports, resolve | rules param (`checkFunctionNameCase`) |
| check_deprecated_usage | deprecated_usage, ini_directives, oop/attributes | ext-deprecation-rules activation |
| check_missing_checked_exceptions | function_decl, property_hooks, throws/missing | ext-extras |
| check_empty_method_bodies | function_decl | ext-extras |
| check_strict_printf_placeholder_types | calls/format_string | rules param (`checkStrictPrintfPlaceholderTypes`) |
| check_too_wide_{throw,return,by_ref}_type_in_protected_and_public_methods | throws/too_wide, return_type_too_wide, by_ref_params | rules params (PHPStan core) |
| check_method_throw_type_covariance | method_overrides, internal_rules | rules param |
| check_duplicate_{function,class}_declarations | project | rules (collector rules) |
| check_boolean_trap, check_swapped_arguments, check_array_key_cast, check_never_return_suggestions, check_promoted_property_suggestions, check_switch_missing_default, check_throw_without_message, check_throw_base_class, check_exception_hygiene, check_new_exception_discarded, check_loose_haystack_comparison, check_identity_vs_equals, check_native_type_hints | calls, array_ops, function_decl, oop, stmt/*, expr, strict_idioms, closure_context | ext-extras |
| check_parent_constructor_call, check_boolean_conditions, check_useless_casts, check_strict_array_filter, check_strict_function_calls, check_switch_case_types, check_loop_variable_overwrite, check_dynamic_static_calls, check_illegal_constructor_calls, check_implicit_array_creation, check_arithmetic_operands | oop, operators, expr, stmt, strict_rules | ext-strict-rules |
| check_dynamic_properties | expr/probe_checks | rules param (`checkDynamicProperties`) |
| check_comparable_types, check_null_coercion, check_enum_exhaustiveness | strict_rules, strict_idioms, expr, stmt | ext-shipmonk |
| check_mixed_type_operands | strict_rules, expr | ext-voku |
| check_current_time_dependence | expr | ext-slam |
| check_psr3_logging, check_psr3_exception_context, psr3_context_key_pattern | psr3, expr | ext-psr3 |
| check_magic_numbers, magic_number_exemptions, magic_numbers_ignore_numeric_strings | magic_numbers, checker, expr, function_decl, stmt | ext-sidz |
| check_risky_tests | checker, expr | ext-phpunit + ext-pest |
| check_dead_{enum_cases,code,abstract_classes,members}, check_circular_dependencies, check_renamed_named_arguments | project | rules (collector rules) |
| check_missing_callable_signature | missing_typehint | rules param (`checkMissingCallableSignature`) |
| phpstan_api_check_enabled | phpstan_api, registry/index | rules (derived from the package context; recording unconditional) |
| restricted_symbols | restricted_symbol_usage, filter_by_path | ext-disallowed-calls |
| dynamic_constant_names | checker/constant_resolver, oop | ConstantResolver (ReflectionSettings) |
| doctrine_custom_functions | doctrine/result_shape | ext-doctrine |
| php_version | everywhere | ReflectionSettings / AnalysisContext (a `PhpVersions` range) |
| configured_assertions, configured_class_argument_returns | registry/assertion_table | reflection overlay compilation |
| sql_reflection_config / schema / reflector / cache | sql_reflection | sql `SqlContext` (Session-owned), read by ext-sql |

**Non-Registry keys.**

| Keys | Owner |
| --- | --- |
| paths, level, ignore_errors, baseline_*, report_unmatched_ignored_errors | SessionSettings |
| scan_paths, stub_paths, no_composer | ReflectionSettings |
| architecture, banned_*, check_closed_classes, check_composer_hygiene | rules sections |
| mock_banned_classes | ext-mockstan |
| parent_calls, default_parent_calls | rules::class_shape (+ ext defaults) |
| assertions, class_argument_returns | reflection overlays |

## Appendix C: `Checker` fields (74) and where they go

| Fields | Disposition |
| --- | --- |
| registry | AnalysisContext (reflection snapshot) |
| settings, bodies | AnalysisContext (settings, body source) |
| sql | ext-sql (the `sql` service in AnalysisContext) |
| diagnostics | deleted (returned errors) |
| semantic_tokens, probe_span, probe_hit, probe_var_provenance, collect_inlay_hints, inlay_param_name_hints, inlay_local_type_hints | LSP observers |
| collect_member_refs, member_ref_sites, escaped_enums, escaped_member_classes | reference-site collector |
| name_ctx | deleted: AST names read their parse-time resolution (D3), PHPDoc names resolve through C7's name scope |
| current_file, current_file_absolute, anonymous_class_names, current_anonymous_class | file state (anonymous names from reflection) |
| current_class, current_method, current_method_templates, current_class_attrs, current_method_attrs, current_trait | Scope (class / function / trait context) |
| current_scope_deprecated | Scope query (deprecation predicate) |
| saw_dynamic_var_construct | scope `afterExtractCall` |
| current_function_is_generator, current_declared_return_type, current_declared_throws_type, current_function_saw_explicit_return | function frame / function-exit node |
| current_catch_types, current_function_goto_targets | parse-time annotations (D2) |
| current_function_throw_points, current_function_explicit_throw_points, current_function_has_unannotated_call, current_function_return_points | StatementResult / function-exit node |
| current_function_by_ref_params, current_function_by_ref_exit_snapshots, current_function_by_ref_params_with_tag, current_function_byref_snapshot_names | virtual keys (ParameterOriginalValue) + function-exit node |
| current_function_assigned_variables | ext-strict-rules file state |
| in_bare_statement_position, in_condition_position, in_exempt_last_condition | ExpressionContext / StatementContext / parse-time attributes |
| pest_this_type, pest_hook_properties | ext-pest (closure-this ext + file-set hook) |
| include_guarded_paths, include_path_dirs, include_constant_initializers, working_dir | RequireFileExists rule state |
| enum_chain_tails | rules (enum exhaustiveness, ext-shipmonk) file state |
| constant_facts | deleted (the magic-constant rule reads the scope) |
| by_ref_empty_dim_args | by-ref argument context on the resolved-call node |
| property_assign_facts, cross_file_property_assign_facts, named_argument_call_facts, facts_only | deleted (ClassPropertiesNode rule / collectors) |
| external_rules | rule registry in AnalysisContext |
| loop_break_states, loop_continue_states, loop_depth | StatementResult exit points + traversal context |
| try_throw_states, call_throw_types | StatementResult / ExpressionResult throw points |
| in_range_index_writes, active_discriminant | conditional holders / loop context |
| pending_or_disjunctions | `&&`/`\|\|` holder recipes and augments (D3) |
| static_var_names | scope state (static-variable status) |
| rebind_target_classes, rebind_escape_all | reference-site collector (escape analysis) |
| call_candidate_cache | pure memo cache (allowed) |
| type_dump | the type-dump observer (a Session option the gate runner sets) |

## Appendix D: `Locals` and its merge policies

| Member | Current merge | Target |
| --- | --- | --- |
| types | only if on both sides, unioned | holders with certainty (Maybe for one side) |
| native | only if on both sides, unioned | native holders, narrowed |
| provenance | per-variable union | holder metadata (on observer request) |
| guarded_functions / guarded_constants / guarded_classes | intersection | FuncCall / ConstFetch holders; `class_exists` always remembered |
| guarded_methods / guarded_properties | intersection, never invalidated | HasMethod / HasProperty accessories |
| clone_reinitialized | union | `CloneReinitializationExpr` virtual key |
| condition_aliases | `Rc::ptr_eq` (AST replayed lazily) | conditional holders created at assignment (`AssignHandler`) |
| branch_implied_facts | union | `&&`/`\|\|` holder recipes and merge-created conditional holders |
| container_field_facts | `Rc::ptr_eq` | element-type refinement |
| discriminant_array_facts | union | conditional holders |
| query_builder_aliases | `Rc::ptr_eq` | ext-doctrine extension type data |
| any_variable_may_exist | OR | `afterExtractCall` (AND) |
| may_have_member_narrowing | derived | deleted |
| track | flag | engine mode |

## Appendix E: third-party dependencies

| Dependency | Crate(s) |
| --- | --- |
| clap, clap_complete (new) | bin |
| ctrlc (new) | bin (F4's interrupt handling) |
| blake3 (new) | project (F2's content hashes and fingerprints) |
| dunce (new) | project (F1's path identity without Windows' verbatim prefix) |
| stacker (new) | syntax, types, analyser (F1's stack growth at recursion points) |
| crossbeam-channel, lsp-server, lsp-types | lsp |
| dhat | bin |
| hotpath | every measured crate (plain dependency); features only on the bin |
| imbl | syntax (name resolution's `use` map); analyser (holder maps); reflection only if its snapshots use it |
| indexmap | reflection |
| md5 | reflection (anonymous-class naming) |
| pcre2-sys | pcre, which `builtins` links under `pcre-validate` (`UNSAFE_PLAN.md`) |
| backtrace | crash-context |
| the crash reporter's (sentry and the rest `CRASH_REPORTING_PLAN.md` lists) | app, under `crash-reporting`; crash-sys |
| rayon | project; reflection while it indexes stubs and runs purity in parallel (decided in C5) |
| regex | config (globs), app, lsp (reflection uses none) |
| roxmltree | builtins (xml validators) |
| rustc-hash | wherever used |
| serde | every crate with a config section or serialized output |
| serde_json | diagnostics / app formatters, project (cache), lsp, composer |
| toml | config |
| sqlparser, mysql, postgres, rusqlite, r2d2, r2d2_postgres, r2d2_sqlite | sql (sqlparser also in ext-sql only if it keeps checker-side parsing; the target is an `sql` API) |
| arbitrary | syntax (`fuzzing`) |
| criterion, proptest (dev) | crates with benches / property tests |
| libtest-mimic (new, dev) | testing and every fixture-owning crate |

## Appendix F: decisions

- **Scope reporting.** Rules return `Vec<RuleError>`, as in PHPStan and the
  external `Rule` API. `Scope::report` does not exist.
- **Diagnostic naming.** The reported item is `Diagnostic` (the compiler
  and LSP term, and the crate's name), not `RuleError`: it also carries
  parse errors, unmatched ignores, project-level findings and stale
  baseline entries. `RuleError` names only what a rule returns, before the
  dispatcher locates it - PHPStan's `RuleError` → `Error` split.
- **ConstantResolver's layer.** `reflection`, with `dynamic_constant_names`
  a `ReflectionSettings` key; reflection and the analyser both read
  constants through it.
- **Stage C ordering.** C6 before C7, with C6 parsing types through the
  phpdoc grammar (TypeNode). C9 creates `AnalysisContext` and D6
  completes it. The two ext data modules (`access_attributes`,
  `doctrine_entity`) move onto class-reflection data extensions in C8.
- **Bugs in code a later phase rewrites.** A fixture first; the fix is that
  phase's own behaviour commit (approved delta) after its structural commit.
  Fixed first instead: the `PHP_VERSION_ID` range typing (C2), C3a's
  `property.unused` delta, `int|float` arithmetic (D3), top-level variables as
  maybe-defined (D5), loop/switch termination (D2) and `is_resource()` never
  always true (D3), impure `$this` calls dropping `$this->…` narrowing (D5) and
  `finally` reads of variables the `try` assigns (D5) and `return.missing` on a
  `void|…` return type (E3f), each a false positive on ordinary code; D2 and D5
  name the interim fix. Also fixed first, as Stage F's items name them: bugs
  that end the language server, lose a user's data or write broken code,
  whatever phase rewrites the code (F1's "Fixed first").
- **phpstan-symfony** has no counterpart crate: the ROADMAP's extension
  config mapping marks it unsupported (its rules need the container XML or
  a console application loader).
- **Stage E order.** Each rule moves once, onto its final API: D6 lands
  the returned-error `Rule` with `RuleErrorBuilder` and the identifier
  families, E3's prologue lands `RuleLevelHelper::find_type_to_check`,
  `RuleLevelHelper::accepts` and the rule → level table, and only then do
  the sub-phases move rules. Moving rules first and adding levels or the
  error builder after would edit each of them twice.
- **Stage D order.** Structure before semantics: holders (D1), statement
  results and handlers (D2), one inference path with the TypeSpecifier
  (D3), the sink (D4), then the semantics (D5) and the public APIs (D6).
  A transition built on `check_stmt`/`check_expr` would be written twice;
  goto, foreach termination and `break` levels need D2's exit points,
  constructor-first needs its execution ends, and narrowing needs D3's
  handlers.
- **Stage B follow-ups before C5.** The AST and type API changes they make
  (declaration names, trait adaptations, language-construct nodes, byte
  strings, one subtyping relation) land before C5–C8 build on them; the
  declarations' namespaced names before C4's builder commit.
- **Subtyping home.** A ClassHierarchy oracle trait in `types` with one
  tri-state relation, implemented by reflection. ExtensionTypeData hooks
  take `&dyn ClassHierarchy`. Level-dependent acceptance lives in
  `RuleLevelHelper` (analyser).
- **Assertion tables.** PHPDoc overlays in reflection, compiled at config
  load. There is no engine extension.
- **Unknown ancestors.** A declared `extends`/`implements` name no class
  answers to is an unresolved edge, and a relation through it is `Maybe`
  (C8). Neither asserting the relationship nor denying it: PHP can't load
  such a class, and PHPStan's ancestor chain stops at it.
- **Reflection cycles.** A cycle through `extends`/`implements`/`use` is
  broken on the ancestor level by dropping every edge inside it; `@mixin`
  targets are read one level down, and constant reference cycles type as
  `mixed`. No memo initialization waits on itself, so each level is a
  per-class `OnceLock`.
- **ext-sql and ext-doctrine.** No crate edge between them. ext-sql owns the
  DBAL Connection / QueryBuilder typing. ext-doctrine publishes entity
  metadata through a reflection data extension (C8). DQL typing reads the
  schema from `sql`.
- **Stub index.** Checked in, generated by an xtask, and verified by a
  regeneration test. `stub-sources` has no build-dependency on the parser.
- **Fixture timing.** Recorded in A2 from the Stage-A binary, behind a
  harness whose backend moves Project API → Session (F1), and moved to
  owning crates in G2. Expectations change only through approved deltas.
- **Refactor-then-extract**, not extract-first. The larger package during
  Stages D–F is tracked by the build-time gate.
- **Publishing.** Crates stay `publish = false`. Third parties consume them
  via git against `app`. Publishing to crates.io is out of scope.
- **PHPStan's files.** A PHPStan baseline is neither read nor written:
  baselines are phpstan-rs's own JSON, which `--generate-baseline` makes. A
  PHPStan config is converted once into `phpstan-rs.toml` by `init` (ROADMAP
  Phase 183), never read as a live source; the `neon` crate exists for that
  conversion.
- **One analysis command.** `analyse` (with `analyze`), PHPStan's name, is the
  command; `check-project` goes in F4 rather than staying as an alias, since
  nothing is released.
- **Exit codes.** 0 when nothing remains after the finalizer, 1 when errors
  remain, 2 when the result can't be trusted (a run that couldn't start, or one
  with an internal error), 130 on an interrupt, 141 on a closed output pipe. A
  formatter never decides the code.
- **Language features after the Session.** The LSP's features (navigation,
  references and rename, completion, signature help, tokens, inlay hints, code
  actions, hierarchy) are F5, after F1–F4: they read F1's snapshots through one
  node-at-position query, F2's dependency graph, E1's reference-site collector
  and E6's observers, so building them earlier would build each twice.
- **Scope API exclusions.** Excluded PHPStan `Scope` methods are listed with
  reasons in the D6 commit.
- **`Type` as a closed enum.** Kept. Every PHPStan type concept has its own
  variant, and `ExtensionTypeData` covers open extension data.

## Appendices G–J: cause lists

Appendices G (built-in return types against the nsrt suite), H (the nsrt
suite's uncited files by cause), I (the rules oracle's unexplained
phpstan-rs-only findings by cause) and J (the nsrt suite's cited files by
cause) are in `CRATE_SPLIT_CAUSES.md`. An item here that names a cause key
("Appendix H's `count-narrow`") finds its count, description and files
there.
