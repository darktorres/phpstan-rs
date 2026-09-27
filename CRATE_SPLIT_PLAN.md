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

This document was reviewed by a 21-agent workflow (10 review dimensions,
each adversarially verified, plus a completeness critic): 249 findings, 248
upheld. This revision incorporates every upheld finding. Contradictions
between findings are resolved in Appendix F.

## 1. Measured state (HEAD `89d468c` + docs)

| Area | Measured |
|---|---|
| Size | 216k lines in one crate; 4,869 `#[test]` in `src/`; `src/checker/tests/` holds 3,297 tests in 67,140 lines, and all checker test modules together hold 3,441 |
| Diagnostic emission | 544 `report`/`report_builder` call sites under `src/checker` (433 + 111), ~324 of them in the inference modules (`expr`, `stmt`, `calls`, `oop`, `function_decl`, `assignment`, `operators`, `control_flow`, `comparisons`, `coercion`, `resolve`, `narrowing`). Plus 13 raw `Diagnostic {}` literals (`phpstan_api.rs` ×11, `duplicate_classes.rs`, `duplicate_functions.rs`) and 3 vector splices: external-`Rule` dispatch at `expr.rs:248`/`stmt.rs:393`, and the trait-host dedup at `function_decl.rs:126` |
| Post-emission processing | extract()/eval() truncates *every* diagnostic of the enclosing body (`saw_dynamic_var_construct`). Loop bodies run up to 6 passes, and `while`/`for` conditions get a quiet pre-pass; diagnostics from non-final passes are truncated, but return points, throw points and property facts leak across passes. Trait methods are checked once per host, then deduplicated by (span, message) and filtered by a cross-host constant-condition verdict |
| `Checker` | 67 fields (Appendix C) |
| `Registry` | 90 fields (74 `pub`, 12 `pub(crate)`, 4 private). They include 67 settings (Appendix B), 4 SQL fields, Doctrine entity metadata (`doctrine_entity_table_index`) and provenance sets (`built_in_stub_classes`, `unconditionally_declared_*`, `stub_provided_functions`). There are ~173 `registry.classes` reads in non-test checker code |
| Reflection data | `ClassInfo` has 48 `pub` fields, `FunctionSig` 24, `PropertySig` 22 and `ParamSig` 13. Member facts are stored as parallel sets, templates as five parallel maps, and `FunctionSig.body` holds AST bodies. `ClassInfo` also carries `doctrine_entity` and php-language-extensions access data |
| Reflection pipeline | 15 whole-registry copy-down passes (`flatten_traits`, `resolve_method_tags`, `precompute_member_owners` ×2, `flatten_phpdoc_inheritance`, `resolve_deferred_*`, `precompute_ancestor_names`, `resolve_constant_named_types`, `drop_doc_members_excluded_by_native`, `apply_assertion_table`, `resolve_transitive_purity`, `infer_global_variables`, ...), duplicated across `finish_loading`, `update_file` and `Registry::build`. The LSP reruns all of them on every edit. Three staleness bugs are documented: a removed base class keeps `is_subclass_of` true; trait/mixin members stay on hosts; anonymous classes from old file versions are never removed |
| Stubs | 2,392 embedded files (558 phpstorm-stubs + 1,793 phpstan-src/src + 41 phpstan-src `stubs/*.stub`), all parsed eagerly in one `LazyLock` and cloned into every `Registry`. Precedence is first-wins (phpstorm before phpstan-src); a constant declared differently in several files collapses to `mixed`. Ten hand-written, order-dependent Rust patch functions follow. The lexer rewrites `#[PhpStormStubsElementAvailable]`/`#[LanguageLevelTypeAware]`/`#[PhpVersionAware]` at lex time, and only the newest PHP version is read |
| Scope | `Locals` holds `types`, `native` (variables only, never narrowed) and `provenance`. It also has nine maps with six different merge policies (Appendix D) and three flags (`any_variable_may_exist`, `may_have_member_narrowing`, `track`). Its three invalidation paths each purge a different subset, and the guard sets are never invalidated. There is no certainty: a variable defined on one side of a branch is dropped at the merge |
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

```
L0  syntax        typed AST with NodeIds, lexer (returns warnings), parser,
                  name resolution, Visitor/VisitorMut, parse-time node
                  annotations, printer, interned Name
    types         the type model, TrinaryLogic, ClassHierarchy oracle trait,
                  the one is_super_type_of / accepts, union algebra,
                  coercions, accessory types, ExtensionTypeData, PhpVersion(s),
                  float formatting
    stub-sources  embedded stub sources + checked-in generated symbol index
L1  phpdoc        the single PHPDoc grammar -> TypeNode AST; DocBlock  (syntax)
    sql           standalone SQL reflection with its own column-type model:
                  config section, DDL schema, live reflector (DB drivers,
                  sqlparser), drift, SqlContext service           (no deps)
    diagnostics   Diagnostic, RuleError, builders, tips, fixes (text + AST),
                  Identifier (+ families)                         (syntax)
    composer      composer.json / installed.json model, package context
                                                                  (syntax)
L2  config        loader and section-owner registry with schema
                  validation; each settings type lives with its owner
                  module                                          (diagnostics)
L3  reflection    ReflectionProvider snapshots, locator chain, stub layer
                  (PHP-version keyed), PHPDoc overlay layer, TypeNodeResolver,
                  class-reflection / deprecation extension traits,
                  ReflectionAttribute, provenance, ConstantResolver,
                  implements ClassHierarchy
                                    (syntax, types, phpdoc, stub-sources, composer, config)
L4  analyser      engine: holder scope, ExprHandlers, results, TypeSpecifier,
                  NodeCallback sink, Scope API, Rule/Collector/Observer,
                  derived nodes, extension-trait registry, RuleLevelHelper,
                  AnalysisContext, summaries API, reference collector.
                  Emits nothing
                                    (reflection and below; no composer, no I/O)
L5  builtins      = phpstan-src/src/Type/Php: always registered, can't be
                  switched off; incl. regex + PCRE validation (`pcre-validate`)
                  and the date parser                             (analyser)
    rules         phpstan-src/src/Rules equivalents + project-level
                  collector rules                                 (analyser, builtins)
    ext-*         one crate per mirrored package (list below)     (analyser, builtins, rules, sql)
L6  project       Session, SessionBuilder (generic registrations, no ext-*
                  deps), AnalysisResult, result finalizer, VFS, result
                  cache, dependency graph, LSP-facing indexes     (analyser)
    testing       RuleTestCase / TypeInferenceTestCase / LevelsTestCase /
                  Integration / codemod harness + libtest-mimic runner (project)
L7  app           composition root: registers builtins + rules + every ext-*,
                  ErrorFormatter set; `run(args, extra)`          (project, all L5)
    lsp           protocol and features over Session + observers  (project)
L8  phpstan-rs    bin: clap global flags + `--set`, calls app::run
```

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
|---|---|
| `ext-strict-rules` | phpstan-strict-rules, exactly. Includes porting its remaining families: disallowedBacktick, disallowedEmpty, disallowedShortTernary, closureUsesThis, matchingInheritedMethodNames, noVariableVariables, booleansInLoopConditions |
| `ext-deprecation-rules` | phpstan-deprecation-rules (built on the restricted-usage framework) |
| `ext-doctrine` | phpstan-doctrine: ORM + DQL, entity metadata, `doctrine_functions` |
| `ext-sql` | phpstan-dba-style SQL/DBAL row typing, parameter validation, mysqli |
| `ext-phpunit` | phpstan-phpunit |
| `ext-codeception` | Codeception test base class and parent-call defaults |
| `ext-mockery` | phpstan-mockery |
| `ext-nette-tester` | Tester\Assert overlays |
| `ext-pest` | pest's PHPStan plugin |
| `ext-mockstan` | rector/mockstan mock hygiene |
| `ext-disallowed-calls` | spaze/phpstan-disallowed-calls (today's `restricted_symbols`) |
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
    and license, inherited by every member (so the LSP's
    `serverInfo.version` matches the bin; the bin also passes its version
    into the LSP entry point);
  - `default-members = ["crates/phpstan-rs"]`;
  - `exclude = ["fuzz", "third_party/hotpath"]`;
  - root `[profile.*]` and `[patch.crates-io] hotpath`.
- **Dependencies.** `[workspace.dependencies]` sets
  `default-features = false` wherever a member must not inherit defaults.
  Appendix E maps every third-party dependency to its crate(s).
- **Lints.** `[workspace.lints]`, with every member setting
  `[lints] workspace = true`, active from the commit that extracts each
  crate:
  - `unreachable_pub`, `unsafe_code` and `rustdoc::broken_intra_doc_links`
    at `deny` (with a scoped `allow` on the PCRE2 FFI module);
  - `clippy::print_stdout` / `print_stderr` at `deny` in every library
    crate;
  - `missing_docs` on the API crates (`reflection`, `analyser`, `project`,
    `testing`).
- **Features.**
  - `pcre-validate` lives in `builtins` with no default. The bin declares
    `default = ["pcre-validate"]` and forwards it. Tests that need real
    PCRE2 are `#[cfg(feature = "pcre-validate")]`, with complements for
    the disabled path.
  - `fuzzing` lives in `syntax`.
  - `hotpath`/`hotpath-alloc` are declared only by the bin. Other crates
    take a plain `hotpath` dependency; macro behaviour follows the hotpath
    crate's own features. hotpath measure labels change with module paths,
    and the timing re-measurement maps old labels to new ones.
  - `dhat-heap` is bin-only and mutually exclusive with `hotpath-alloc`
    (`compile_error!`).
- **Release profile.** Decided in Stage A: `lto = "thin"`,
  `codegen-units = 1`, with `dhat` inheriting it. The timing baseline is
  recorded under that same profile.
- **Name collision.** The embedded-stub crate is named `stub-sources`, so
  it doesn't collide with the `reflection::stubs` module. It owns the
  `include!` of its generated statics and exports them.

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
  across syntax, types and reflection. The interner is sharded and
  Session-scoped, so the LSP can reclaim memory.
- **Helpers:** `is_primitive_type_name`, `unwrap_grouping`,
  `literal_string_value`, `is_superglobal` and `body_contains_yield` move
  here.

### 3.2 types
- **Dedicated variants:**
  - variance-annotated generic arguments;
  - late-resolvable types (value-of, key-of, offset-access, class-constant
    access, conditional);
  - `Static`/`This` with their bound class (B4 ships them unbound, with
    `self<T>`/`static<T>` arguments on the variants and `self` bound to the
    declaring class at a member read; the bound form, `static(Base)`/
    `$this(Base)` and `@return $this` refusing `new static()`, lands in D4,
    where member access goes through one object view instead of ~540
    `Type::Object` matches);
  - `ObjectWithoutClass`, `Resource`, `BenevolentUnion`.
- **Shapes and names:**
  - shape keys use a typed key enum, with an explicit unsealed tail
    replacing the `"..."` sentinel;
  - class names are the interned case-insensitive `Name`.
- **Equality and hashing:** consistent `PartialEq`/`Hash`.
- **Accessory types:** `HasMethod`, `HasProperty`, `HasOffset` and
  `HasOffsetValue`.
- **One subtyping implementation.**
  - `TrinaryLogic` moves here.
  - A `ClassHierarchy` oracle trait (is_subclass_of, ancestors, is_final,
    enum cases, template bounds), which reflection implements.
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
  `config.platform`/`require.php`.
- **`ExtensionTypeData`:**
  `Arc<dyn ExtensionTypeData + Send + Sync>`, with hooks for eq, hash,
  display, and payload-vs-payload subtyping taking `&dyn ClassHierarchy`.
  There is no mandatory merge: a join unions distinct payload-carrying
  types (PHPStan keeps each `QueryBuilderType` distinct), and an optional
  extension widening applies only past the union cap.

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
too, once C6 embeds them. Its generator and reflection's declaration walk
are one Visitor-based walk, so the two can't disagree on what a file
declares.

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
  along dependent symbols. The 15 copy-down passes and their three
  duplicated lists are deleted. Anonymous classes are per-file entries,
  removed with their file. The three staleness bugs are closure gates.
- **Locator chain (PHPStan order):**
  1. analysed files;
  2. `scan_paths`;
  3. a lazy composer locator (installed.json classmap/PSR-4/files), wrapped
     in a version-aware skip-polyfill;
  4. the stub layer, filtered by a PHP-version blacklist.

  The embedded phpstan-src layer stays unconditionally available, matching
  PHPStan's in-process autoloader.
- **Stub layer:**
  - process-wide and keyed by PHP version (`version -> Arc<StubLayer>`,
    with "unset" meaning today's newest reading);
  - its unit of resolution is a class plus its transitive ancestors, traits
    and mixins, resolved through a per-class `OnceLock` (no shared Mutex,
    so it can't be poisoned; a panic leaves the unit retryable);
  - a project declaration that merges with a stub symbol becomes a
    per-project materialized entry (copy-on-merge);
  - the four seeded tracking sets become provenance queries against the
    index;
  - polyfill arbitration first keeps today's quality tiebreak
    (byte-identical); version-based skip-polyfill follows as its own
    commit.
- **Signature and overlay pipeline:**
  - the base is phpstorm-stubs, then the phpstan-src `functionMap.php` plus
    per-version deltas;
  - a PHPDoc overlay layer on top holds phpstan-src `stubs/*.stub` (full
    PHPDoc), user `stub_paths` (PHPStan `stubFiles` semantics: a PHPDoc
    overlay, or a declaration when none exists, as its own behaviour-change
    commit) and `StubFilesExtension` stubs;
  - every hand-written Rust patch becomes a declarative overlay entry,
    ordered and with cross-symbol dependencies recorded;
  - `[[assertions]]`/`[[class_argument_returns]]` compile to overlay entries
    at config load, so every assert flows through the one `@phpstan-assert`
    path;
  - the Nette Tester and Mockery defaults ship as overlays from their `ext-*`
    crates;
  - `apply_assertion_table` is deleted.
- **Enumeration APIs:**
  - `names(kind, layer)` from the index, without parsing (completion);
  - `subclasses_of`/`implementors_of` from the index hierarchy plus the
    project;
  - a case-folded function key map replacing the linear scan (landed
    early as GATE_SPEED_PLAN S6: `registry::FunctionTable`; class keys
    stay with C3);
  - every existing iteration site states its layer scope, including the
    ported `ReflectionRule` API.
- **Extension points owned here:**
  - Properties / Methods / AllowedSubTypes class-reflection extensions,
    consulted by member lookup so every consumer sees them;
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
- **Keys.** Class and function names are the case-insensitive `Name`;
  method names fold case; property and constant names stay exact, and a
  global constant folds only its namespace prefix - PHP's own rules. There
  is one lookup per kind and no side map.
- **`ConstantResolver`** (declared, predefined and version-aware over the
  whole `PhpVersions` range, plus `dynamicConstantNames`) lives here, as
  the reflection-side initializer and PHPDoc-type resolution needs it;
  the analyser reads constants through it.

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
- A one-release compatibility shim maps the old flat `check_*` keys, with a
  warning. The Piwigo toml, README and VSCode docs migrate in the same
  commit.
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
- `location` and `metadata` are removed. Dropping `location` changes text
  output and the baseline key, so the baseline format is versioned, keyed
  like PHPStan (path, identifier, message, count), with a converter.
- No `clap` in any library crate.

### 3.8 analyser
- **Scope storage.**
  - Expression-keyed holders (type, native type, TrinaryLogic certainty)
    over an interned structured `ExprKey` (variable, property path, offset
    path, static property, call), in persistent maps with O(1) clone.
  - Virtual keys: property-initialized, parameter original value, by-ref
    intertwining, always-remembered, possibly-impure call.
  - Conditional holders, frozen at creation.
  - Non-holder scope state: currently-assigned, allowed-undefined,
    function-call stack, closure-bind scope classes, first-level statement,
    native-types-promoted, `afterExtractCall`.
  - Guards follow PHPStan:
    - `function_exists` is a FuncCall holder with a specified name
      normalization;
    - `class_exists` is an always-remembered call;
    - `defined('X')` is a ConstFetch holder;
    - `method_exists`/`property_exists` narrow the receiver with
      `HasMethod`/`HasProperty`, falling back to a call holder for magic
      members.
  - Maps move as follows:
    - `container_field_facts` folds into element-type refinement;
    - `condition_aliases`, `branch_implied_facts`,
      `discriminant_array_facts` and `pending_or_disjunctions` become
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
  generalization/fixpoint, finally, closure capture, and always-iterable
  foreach. Convergence equality compares holders, certainty and natives,
  excluding conditionals.
- **Invalidation** follows PHPStan `invalidateExpression`: a write to any
  expression (variable, property/dim fetch, by-ref argument, unset,
  foreach binding) drops every holder and conditional that contains it.
  May-alias dim siblings are an explicit sub-rule. Other triggers: impure
  calls on the receiver, volatile/static expressions, existence checks, and
  extract/include/eval via `afterExtractCall`. eval is treated like extract,
  beyond PHPStan.
- **Natives.** Natives are full holders, narrowed in lockstep, with a
  complete `native_type_of` inference path.
- **Results.**
  - Statement and expression inference return `StatementResult` /
    `ExpressionResult` (exit, throw and impure points; truthy/falsey
    scopes); no mutable per-function fields.
  - `ExpressionContext`/`StatementContext` are passed down the recursion,
    replacing the take-idiom flags.
- **One inference path.**
  - Per-kind `ExprHandler` modules behind a dispatch (splitting `expr.rs`);
    one implementation records per-`NodeId` results, and `infer_expr` is
    deleted.
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

  The scope handed to a callback is the pre-node state; post-node data
  comes from derived nodes. For ported external rules this is a behaviour
  change, recorded as such.
- **TypeSpecifier** is public: `specify_types_in_condition(scope, expr,
  ctx) -> SpecifiedTypes` (sure / sure-not). It is passed to rules and to
  type-specifying extensions (TypeSpecifierAware). The impossible-check
  family is ported generically on it (ImpossibleCheckTypeHelper), replacing
  the hard-coded predicate table.
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
  (~60), each with its payload and the scopes it carries. Among them:
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
    incrementally, which ext-pest needs);
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
- **Summaries API.** Whole-program summaries that feed inference, such as
  purity and global-variable types (and later ROADMAP 85/87/88/90/94), run
  as a fixed point before analysis. They are stored per reflection snapshot
  in `AnalysisContext` and declare invalidation keys. The invariant: per-file
  inference reads reflection plus summaries, and never collector output.
- **No I/O.** The package context comes from `AnalysisContext`, and file
  existence comes from the Session VFS, which records a dependency.

### 3.9 Levels
- PHPStan's 0–10 plus `max` ladder, following `conf/config.level0.neon`…
  `config.level10.neon` and `config.levelmax.neon`.
- Each rule has a registration level. Computed-level sites become named
  parameters: reportMaybes, checkUnionTypes, checkNullables,
  checkExplicitMixed, checkImplicitMixed, checkPhpDocMissingReturn,
  checkMaybeUndefinedVariables, and so on.
- A rule → level table is cross-checked against the neon files, including
  conditional tags.
- One documented default level for CLI and LSP.
- Rules above the level never run.
- `Diagnostic.level` goes. The JSON `level` field is filled from rule
  metadata under a versioned schema, and the LSP severity comes from
  rule-declared severity.

### 3.10 project, app, lsp, bin
- **`Session::run() -> AnalysisResult`**, containing:
  - file errors, not-file errors and internal errors;
  - collected data and dependencies;
  - stats (checked, skipped context / analysed, panics, context loaded);
  - stale baseline entries and unmatched ignores;
  - startup warnings (SQL drift, lexer warnings).
- **Result finalizer**, a fixed, documented order:
  1. inline ignores (with unmatched reporting);
  2. ignoreErrors (with `IgnoreErrorExtension`);
  3. baseline filter / generate;
  4. (span, message) dedup;
  5. a total order (path, line, col, identifier, message, tip) for every
     consumer.

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
    process (`project::worker_pool`, landed early as GATE_SPEED_PLAN S4);
  - context-before-analysed ordering;
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
  - an `ErrorFormatter` trait over `AnalysisResult`, with PHPStan's set
    (raw, table, github, gitlab, teamcity, ci-detected, json
    PHPStan-compatible, baseline neon/php) plus the existing text,
    checkstyle and junit;
  - registrable by third parties.
- **lsp:**
  - uses the same rule and collector set (incrementally), the same
    discovery, config-file-relative paths, validation surfaced as
    diagnostics / showMessage, and the same default level;
  - `ResolvedWorkspaceConfig` is deleted;
  - hover is `Scope::type_of` at the node, and definition probes use
    provenance metadata.
- **bin:**
  - global flags plus `--set`;
  - exit code nonzero on remaining diagnostics, as PHPStan does, landed as
    its own commit;
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

`cargo xtask gates` runs them in two tiers, because one scoped Piwigo run
takes ~30 s and the full per-level matrix is 25 runs:

- **quick** (every commit):
  - gates 1–3 without `cargo hack`/`machete`/`deny` or `cargo bench
    --no-run` (`test --all-targets` compiles the benches), and with the
    `--no-default-features` build checked rather than built;
  - the scoped and all-on corpus runs;
  - the type snapshots;
  - baseline compatibility.
- **full** (`--full`, run at the end of every phase and on any commit
  that touches levels, acceptance or performance-sensitive code):
  - everything in quick;
  - the unscoped run;
  - levels 0–10 for scoped and all-on;
  - `cargo hack`, `machete`, `deny` and `cargo bench --no-run`;
  - performance and build time.

CI (`.github/workflows/rust-ci.yml`) runs the build and test half. The
corpus, type, baseline, performance and build-time gates need the frozen
Piwigo checkout and a stable machine, so they run locally.

1. **Build and lint:**
   - fmt;
   - clippy `--workspace --all-targets -D warnings`;
   - `cargo doc --workspace --no-deps`;
   - the feature matrix via `cargo hack --each-feature
     --mutually-exclusive-features dhat-heap,hotpath-alloc`;
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
3. **Tests.** The set of test names (`cargo test --workspace --all-targets
   -- --list` plus the fixture lists) must be a superset of the previous
   commit's set mapped through a committed rename/removal manifest (old →
   new, or removed with a reason). Fixtures must match exactly.
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
   joins it at D2.
6. **Baseline compatibility.** The baselines generated by the Stage A
   binary (scoped and all-on) are applied to every commit. The unmatched
   diagnostics and stale entries they report are snapshotted and held to
   the parent commit's like the corpus, changing only by approved delta;
   the counts are printed for information.
7. **LSP replay** (from Stage F). Open every Piwigo file through the
   Session in LSP mode, then run an edit/revert pass. The result must equal
   the CLI snapshot.
8. **Performance**, measured against the previous commit, one-sided:
   - wall time: hyperfine, warmup, 10 runs; the mean may not exceed the
     previous mean + max(2%, 3σ);
   - peak RSS (`/usr/bin/time -v`) and dhat peak;
   - LSP steady-state RSS and incremental re-check latency.

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
    - no engine I/O (its path-resolution part enforced early by
      GATE_SPEED_PLAN S7: no `current_dir`, `canonicalize` or
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

**Approved deltas.** A commit that intentionally changes output records its
expected diffs (corpus, fixtures, type snapshot, baseline) in the commit.
Every changed line is reviewed for runtime correctness. The gate is:
actual diff == approved diff.
Behaviour-changing commits are always separate from structural ones.

## 5. Stages and phases

One commit per item. Every commit passes every gate that exists at that
point.

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
- The harness backend at this point is today's Project API, in the bin
  package's integration tests.

**Bugs found during A2**, and the purity bug found during B3, were fixed
after B7, one commit each with fixtures; the Piwigo delta is approved as
`gates/approved/a2-known-bugs`. The fixes changed these behaviours:
- class names resolve case-insensitively, in `catch`, static-call targets
  and types;
- casts, `echo` and interpolation give precise types, a resource included;
- the `array_udiff()` family's trailing callbacks are accepted;
- `list<int>&non-empty-array` is one intersected type;
- a method called on an intersection uses its most visible declaration;
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
- a child property inherits the hooks it does not redeclare;
  `parent::$prop::get()`/`set()` resolve; a set hook without a parameter
  list takes the property's type;
- `__set()` needs no return type;
- a failing `array_is_list()` proves a non-empty non-list array;
- a boolean flag set in a branch implies that branch's narrowing, unioned
  over every path that sets it;
- a catch is entered from the points in its try that can throw what it
  catches, and is dead when there are none; an unhandled `match` throws
  `UnhandledMatchError`;
- a dynamic method call is impure.

Two reported bugs were not bugs: the sort family and `array_splice()`
were already right, and an interpolated string statement has no effect.

### Stage B: workspace and leaf layers

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
- A mechanical edge inventory against Appendix A (it includes the edges
  missed so far: `text_split → deps`, `registry → {restricted_symbols,
  doctrine_functions, assertion_tables, sql_*}`, `config → rule configs`,
  `checker ↔ rule`, `checker → project`, engine → rule-module references).
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

**B5. phpdoc redesign.**
- `DocBlock` stores `TypeNode`s.
- TypeNode → Type resolution goes behind an interface that reflection
  fills in C7; until then a resolver adapter.
- Extract `phpdoc`; the `parse_type_string` fuzz target repoints to the
  phpdoc parser.

**B6. sql.**
- Its own column-type model, `[sql_reflection]` parsing, and `SqlContext`
  as a service.
- The SQL state moves off `Registry` (byte-identical).
- The SQL → PHP `Type` mapping moves to the package's future `ext-sql`
  module.
- Extract `sql`.

**B7. diagnostics.**
- The diagnostic model (§3.7). `RuleError`, the file and trait file,
  identifier families declared per rule, and AST-node fixes arrive with
  the rule dispatcher (E), which has the rule declarations and the analysed
  file to attach them from; B7 gives `Diagnostic` its validated
  `Identifier`, a list of tips, and drops `location` and `metadata`.
- The versioned baseline plus converter, as an approved-delta commit
  (location dropped, text format change).
- Extract `diagnostics`. The ignore / baseline pipeline stays in the
  package's future `project` module.

**B8. composer.** The package context is computed once. Extract. The
default `PhpVersions` range comes from it (§3.2): composer
`config.platform.php`, else `require.php`; today an unconfigured
`php_version` leaves every version-gated rule silent. An approved-delta
commit of its own.

**B9. stub-sources.**
- The generated index plus the xtask plus the regeneration test.
- The crate owns the `include!`.
- Extract.

### Stage C: config and reflection

**C1. config.**
- Schema, owner sections, the shim, `--set`, deletion of per-rule flags
  and setters (approved delta for the CLI surface), extension activation.
- The setting split per Appendix B; each owner module exists in the
  package by now.
- Extract `config`.

Landed with C2 in one commit (`4c18d661`), with a scoped-corpus delta of
+405 (`gates/approved/c1-c2-config-and-settings`: 197 `test.risky`, 136
`cast.useless`, the rest strict-rules families), because an installed
package now activates its extension. The global settings live in their
owner modules (`project::session::SessionSettings`,
`registry::settings::ReflectionSettings`, `checker::settings::EngineSettings`),
not in the `config` crate as §2's L2 line says; the crate holds only the
loader and the schema. Open (review of 2026-09-27), one commit each:
- **Nested keys are not validated.** The schema rejects unknown keys only at
  a section's own level (`config/src/lib.rs` `check_table`); the structs
  inside arrays of tables have no `deny_unknown_fields`:
  `IgnoreErrorEntry`, `RestrictedSymbolEntry` and its `ParamCondition`/
  `ParamRef`/`ParamFlagsCondition`, `DoctrineCustomFunctionEntry`,
  `sql::config::SqlReflectionConfig`. Every `IgnoreErrorEntry` field is
  optional and an entry with none set matches every diagnostic, so a
  misspelled `messages =` ignores everything, and `identifer =` beside
  `path =` ignores everything under the path. The fix: `deny_unknown_fields`
  on every nested struct, an ignore entry needs `message` or `identifier`
  (as PHPStan's schema requires), its regex is compiled and rejected at
  load instead of never matching, compiled once rather than per
  diagnostic, and `count` is enforced (unmatched counts reported as
  PHPStan does).
- **Type errors name the key.** A section's type error is reported as
  `config at <section>: <serde error>`; each must carry the full dotted
  key path (§3.6), with a test per section shape.
- **Settings §3.6 assigns to the split don't exist**, and each lands with
  the phase that implements its behaviour:
  - universal object crates (hard-coded as `stdClass` at
    `checker/expr.rs:200`) - a `ReflectionSettings` key read by C8's
    built-in implementor;
  - global type aliases (`typeAliases`) - `ReflectionSettings`, resolved by
    C7's TypeNodeResolver;
  - a PHP version range: `php_version` accepts only `"major.minor"`
    (`types::php_version::parse_php_version`), not PHPStan's `{min, max}`
    or a patch version - C5, with the stub-layer key;
  - treatPhpDocTypesAsCertain (D2 natives), polluteScopeWith* (D2
    transitions), rememberPossiblyImpureFunctionValues (D2 virtual keys),
    early-terminating calls (D3), the exception model (E3f).
- **The LSP resolves settings through a second copy.**
  `lsp/workspace_config.rs` assembles `ProjectSettings` by hand beside
  `Session::resolve`, and they differ: the LSP skips the overlay,
  architecture, banned-construct and psr3-pattern validation, folds
  `stub_paths` into plain context directories (losing their designated-
  override meaning), resolves config paths against the workspace root
  rather than the config file's directory, and falls back to an empty
  config on any error. One settings-assembly function serves both now;
  F1 still deletes `ResolvedWorkspaceConfig`.
- **Activation doesn't reach library typing.** Only Doctrine result types,
  Pest's `$this` and the language-extensions attributes follow activation.
  PHPUnit assertion narrowing and mock types, the test base classes, the
  parent-call defaults and the Mockery / Nette Tester assertion defaults
  apply whether or not their package is installed; mockery,
  nette-tester, codeception, type-perfect, disallowed-calls and sql have
  no `Extension` impl. Each gets one, and its typing follows it.
- **`test.risky` is activated by `phpstan/phpstan-phpunit`,** which has no
  such rule; it is a phpstan-rs rule and its activation moves to its own
  owner (ext-extras or an explicit key), with the corpus delta re-reviewed.
- **`ignore_errors.path` depends on how the run was invoked.** The glob is
  matched against the analysed path as spelled on the command line
  (`ignore_errors::filter` uses `file.display()`): `path = "src/*"` ignores
  the error for `check-project src` but not for `./src` or from the
  parent directory (reproduced), while the baseline normalizes the same
  paths. `path` resolves against the config file's directory, as PHPStan
  resolves it, and is compared as a normalized path.
- **`restricted_symbols` entries fail silently too.** A `method`,
  `staticMethod`, `property`, `staticProperty` or `classConstant` entry
  without `class` is dropped by `RestrictedSymbols::compile`; and
  `compile` keys entries by symbol with last-insert-wins while
  `DisallowedCallsSettings::entries` appends the presets after the user's
  own entries, so a preset silently replaces a user's entry for the same
  symbol (traced). A missing `class` is a load error, and a user entry wins
  over a preset's for its symbol.
- **The C1 tests don't cover what C1 promised.** `config/src/tests.rs` and
  `app/tests.rs` test top-level unknown keys only - no nested key, no
  `--set` of an unknown key, no type error's key path - and
  `loads_every_ignore_error_field_combination` loads a `path`-only
  `[[ignore_errors]]` entry, which the requirement above makes a load
  error (no config in the fixtures or the corpus uses one). Each open item
  above lands with its test. The activation package names were checked
  against Packagist; all seventeen exist.
- **`[[assertions]]`/`[[class_argument_returns]]` fail silently.** A type
  string is not parsed at load (a typo becomes a class name), and an
  entry naming a missing function, method or parameter is skipped
  without a word at apply time. Both are load errors.
- **`--set` paths** resolve against the config file's directory, unlike
  every other command-line path; they resolve against the working
  directory.
- **Docs:** no config reference exists (the README documents no keys);
  one is written from the section doc comments, and §2's L2 line is
  corrected. The `check-project` help is stale: `--php-version` says an
  unset version leaves the version rules nothing to check, though B8
  defaults it from composer, and `--level` says unset reports everything,
  though the session caps it at `DEFAULT_REPORT_LEVEL` (6); `--stub-path`
  points at a `check_duplicate_class_declarations` flag "above" that no
  longer exists, and `--format` at the deleted `config.rs`. The
  CHANGELOG's "an unknown key is reported by its full dotted path" holds
  only at a section's own level (the nested-keys item), and it doesn't
  mention C3a's output change. `declared_php_versions` carries
  `composer_roots`' doc comment (`project/session.rs`).
- **One composer root decides activation.** `Session::resolve` discovers a
  composer root per analysed directory and loads each one's vendor
  context, and `phpstan_api_check_enabled` ORs over all of them, but the
  PHP versions and extension activation come from the root that is the
  working directory only. One rule for all three: every discovered root's
  manifest counts.
- **Pest's risky tests are switched by `ext.phpunit.check_risky_tests`.**
  Appendix A splits risky tests between ext-phpunit (methods) and ext-pest
  (`it`/`test` closures); `[ext.pest]` has only `enabled`. Each owner gets
  its own key.
- **Key names.** Several keys repeat their section
  (`ext.psr3.check_psr3_logging`, `ext.psr3.psr3_context_key_pattern`,
  `ext.sidz.magic_number_exemptions`), and section names are kebab-case
  (`strict-rules`, `deprecation-rules`) while `rules.class_shape` and
  every key are snake_case. The shim is the one chance to settle names
  without a second migration: one casing, no repeated prefixes, and a
  PHPStan parameter's own name wherever one exists.

**C2. Settings leave `Registry`**, following Appendix B:
- `RuleLevelHelper` lands in the package's future `analyser` module and
  `ConstantResolver` in the checker (landed); `ConstantResolver` moves on
  to `reflection` (below);
- rule and extension settings go to their owner modules;
- `phpstan_api` recording becomes unconditional.

Gate: reflection reads no settings type but `ReflectionSettings`.

Open (review of 2026-09-27):
- **A version range types `PHP_VERSION_ID` as the stub's literal.**
  `ConstantResolver::global_constant_type` asks
  `Registry::php_version_consensus`, which answers only when the range's
  two ends agree; for any range they differ, so the lookup falls through
  to the phpstorm-stubs `define('PHP_VERSION_ID', 50306)`. With composer's
  `"php": "^7.4 || ^8.0"` (B8's default range), `if (PHP_VERSION_ID >=
  80000)` is reported `greaterOrEqual.alwaysFalse` (reproduced); the same
  holds for `PHP_MAJOR_VERSION`/`PHP_MINOR_VERSION`. Fixed first, as its
  own behaviour commit: a predefined constant is typed over the whole
  range (`int<min.id, max.id + 99>`, the major and minor as ranges or
  unions) and never falls back to the stub's value.
- **`ConstantResolver` is in the wrong layer.** Reflection resolves
  constants too, bypassing it: `resolve_constant_named_types` substitutes a
  constant-named PHPDoc type straight from `Registry::global_constants`
  (`registry/resolve.rs` `constant_named_type`), and class-constant
  initializers are typed at registry build. PHPStan's `ConstantResolver`
  is what its reflection-side `InitializerExprTypeResolver` calls. It
  moves to `reflection` (L3), `dynamic_constant_names` becomes a
  `ReflectionSettings` key, Appendix B's row follows, and reflection's
  constant reads go through it.
- **Constant lookups outside the resolver fold nothing.**
  `checker/imports.rs` checks `use const` with an exact
  `global_constants.contains_key`, but PHP folds a constant's namespace
  prefix: `use const foo\BAR;` against `namespace Foo { const BAR = 1; }`
  reports `constant.notFound` twice, at the import and at the use
  (reproduced). Every constant lookup goes through one folded key.
- **`dynamic_constant_names` details:** a global name is matched exactly,
  where PHP folds a constant's namespace prefix (the stub index's own
  rule); and `generalize` turns a shape into `array<array-key, …>`,
  dropping list-ness and the key type, and builds an unflattened union of
  its values.
- **The gate doesn't exist.** `xtask arch` enforces the `Diagnostic`
  literal, `$$`, print and path rules only; "no setting read under
  reflection" is added (no `*Settings` type, `CheckSettings` or
  `ProjectSettings` named under `registry`/`stub_attributes`).
- **`RuleLevelHelper` holds the settings, not the rule.** It only builds a
  `types::Relation` (`checker/rule_level_helper.rs`); the nullable
  thresholds, benevolent unions and strict mixed arrays are still decided
  inside `types::Relation::accepts` from its `configured_level`,
  `strict_mixed_arrays` and `check_benevolent_union_types` fields. E5
  moves that logic into `RuleLevelHelper::accepts`/`find_type_to_check`
  and deletes the three fields from `types::Relation`.
- **The registry still receives configuration.** `apply_settings` writes
  `php_versions` and the assertion tables into `Registry`, and the tables
  only take effect at the next `apply_assertion_table` pass, so a
  reconfigure (the LSP) keeps the old ones until a rebuild. C6 compiles
  them to overlays; until then a reconfigure reapplies them.

**C3. Reflection data model** (§3.5): opaque reflections, attributes with
arguments, provenance, no AST bodies.

Landed as C3a-C3d (`dcc67ce5`, `bfce0e9f`, `49e6d9b1`, `41874e77`,
`ca63ef85`). Open (review of 2026-09-27):
- **C3a changed output as a refactor.** Its message claims no behaviour
  change, but `gates/approved/c3-provenance-declarations` adds 985 all-on
  diagnostics (978 `property.unused`, 7 `method.unused`): promoted
  properties got a declaration location and the dead-members rule began
  reporting them without the exemptions they need. Confirmed wrong at
  runtime: the Projection view objects' properties (about 200, e.g.
  `SlideshowView::navCurrent`, passed to `Renderer::render()` and read by
  `slideshow.latte`) and 77 Doctrine-mapped columns (`ActivityEntity::
  $objectId`), which phpstan-doctrine's read/write extension counts as
  read and written. Fixed first, as its own behaviour commit: mapped
  properties are used while ext-doctrine is active, and every line of the
  delta is re-reviewed for runtime correctness.
- **Class keys are not `Name`.** `Registry::classes` is keyed by `Arc<str>`
  with a `classes_lowercase` side map and a `canonicalize_class_name_case`
  pass; `class_declaration` looks up case-sensitively. §3.5 left class
  keys to C3. Scheduled in C4.
- **Member names are matched by declared spelling.** Methods are keyed by
  declared spelling; `lookup_method` falls back to a linear scan on a
  miss, but the flag sets and the 12 direct `methods().get()` sites in
  the checker don't. Reproduced: `class D extends C { function BAR() }`
  over `final public function bar()` in `C` reports only
  `method.nameCase`, not the override of a final method (a PHP fatal
  error). Method keys fold case (like `FunctionTable`); property and
  constant keys stay exact. Scheduled in C4.
- **Accessors return the storage.** C3d's accessors (47 of them) hand out
  `&FxHashMap`/`&FxHashSet`/`&Vec`/`&IndexMap` (`methods()`,
  `properties()`, `consts()`, `final_methods()`, `template_bounds()`,
  ...), so the storage shape is still the API, and `Registry::classes`,
  `functions` and `global_constants` are `pub` maps read directly (124
  `registry.classes.get`/`contains_key` in the checker, case-sensitive).
  Opaque means queries (`method(name)`, `is_final_method(name)`,
  `template_params()`), which C4's snapshots need anyway. Scheduled in C4.
- **Parallel collections remain.** Templates are five parallel structures
  (`templates`, `template_bounds`, `template_defaults`,
  `covariant_templates`, `contravariant_templates`), and member flags are
  class-level sets (`abstract_methods`, `final_methods`, `static_methods`,
  `final_properties`, `final_*_by_phpdoc`); `FunctionReflection` has no
  `is_static`/`is_abstract`/`is_final`. §3.5 wants per-member flags and
  `Vec<TemplateParam>`. Scheduled in C4.
- **Build scratch is in the reflections:** `purity_dependencies` and
  `is_transitively_pure` (C9), `method_tags`, `trait_aliases`,
  `resolved_trait_conflicts`, `redeclared_methods`, `type_alias_case_refs`,
  `return_case_refs` / `case_refs` and `type_imports`. They move to the
  builder in C4.
- **Provenance is not the only location record.** `Origin::Unknown` is
  still produced (`Registry::build`, the `.stub` class pass); `is_stub`
  duplicates the origin; `phpstan_class_files` and `function_decl_sites`
  are parallel location stores; comments cite `project/decl_locations.rs`
  and `Project::decl_locations`, which no longer exist. Scheduled in C4.
- **Parse-time resolved names are never read.** B3's `QualifiedName`
  keeps `written` and `resolved`, and the audit marks it OK, but nothing
  outside `syntax` reads `.resolved` or `ResolvedName`: about 90 sites
  re-resolve the written name through a `NameContext`, among them the
  reflection builders (`class_info.rs` 9, `class_info/doctrine_entity.rs`
  5, `reflection_attribute.rs` 3, `access_attributes.rs` 3,
  `global_vars.rs` 3, `function_sig/generics.rs`). One visible effect:
  `NameContext::resolve` has no `self`/`parent`/`static` case, so
  `#[Attr(self::X)]` in namespace `App` records the class `App\self`
  (traced through `reflection_attribute.rs`). The reflection sites read
  `resolved` in C4; the checker's go with D4, and Appendix C's `name_ctx`
  is deleted rather than moved to file state.
- **No-native-type is encoded three ways:** `PropertyReflection::
  has_native_type` beside `ty_native: Type`, the
  `explicit_mixed_hint`/`return_explicit_mixed_hint` flags, and
  `ConstantReflection::native_ty: Option<Type>`. Each native type is an
  `Option<Type>`. Flags are split too: `PropertyReflection` carries
  `is_static`/`is_abstract`, but its finality lives in the class's
  `final_properties` set. Both go with C4's per-member flags.
- **PHPStan's own embedded classes are never "PHPStan code".**
  `checker::phpstan_api::is_phpstan_code` looks the class up in
  `phpstan_class_files` (exact case) and requires a `vendor/` path
  segment, so a class known only from the embedded phpstan-src sources
  (provenance `Origin::Phpstan`) doesn't count (traced). It becomes a
  provenance query with the map's deletion.
- **Attribute arguments are partly evaluated:** constants and class
  constants stay unresolved names, a bare constant name is not
  namespace-resolved, and `new` (allowed in attribute arguments since PHP
  8.1) or concatenation become `Unsupported`. Scheduled in C8, where the
  ConstantResolver and the hierarchy are at hand.

**Registry read-through (review of 2026-09-27).** Every file under
`registry/`, `stub_attributes`, the stub-sources build script, the
stub-index generator and the project load/update paths was read line by
line. The bugs below were each reproduced with `check-project` (marked
"traced" where only the code path was followed). Each reproduced one gets
a fixture now, recording the correct behaviour; it is fixed in the phase
named, as its own behaviour commit after that phase's structural commit
(approved delta), since C4 rewrites most of this code.

- Fixed in C4 (the builder, per-file declaration sets, lazy member
  resolution):
  - **Declarations below the top level are never indexed.**
    `build_declarations` recurses only through namespaces, blocks and
    `if`: a function or class declared inside a function body, a class in
    a top-level `try`, a function in a `switch` case all give
    `function.notFound`/`class.notFound`. The walk becomes a Visitor over
    every statement, marking each such declaration conditional.
    `project.rs`'s `declared_names` and the stub index generator walk the
    same subset and move with it.
  - **Member names by declared spelling** (beyond the final-method case
    in C3's list): an interface, abstract-parent or enum method
    implemented with other casing is `class.abstractMethodNotImplemented`
    (three shapes reproduced); a constructor spelled `__Construct` loses
    its promoted properties (`property.notFound`); a class declaring
    `__tostring()` is not `Stringable` (`argument.type`); about 49 sites
    compare magic-method names exactly. Case-folded method keys fix all.
  - **Type names keep their written spelling, and exact lookups skip on
    them.** A parameter typed `square` (class `Square`) carries
    `Object("square")`; the member-existence checks look that up exactly
    (`checker/expr.rs` `registry.classes.contains_key(class)` before
    `method.notFound`/`property.notFound`) and skip on a miss, so
    `$s->nope()` - a runtime `Error` - and `$t->missingProp` go unreported
    (both reproduced; reported with `Square`). Messages print the written
    spelling too ("Instanceof between App\square and App\Square"). Of the
    126 direct `registry.classes.get`/`contains_key` sites outside the
    registry, those keyed by a declaration or an owner lookup are canonical;
    those keyed by a type's or a literal's name (`expr.rs` 1401/2747/2761/
    4203/4266/6702, `operators.rs:68`, `throws.rs:196`, `visibility.rs:256`,
    `var_tag_types.rs`, `phpdoc_unresolvable.rs`, `missing_typehint.rs`,
    `strict_idioms.rs`) are not. `Name`-keyed lookups fix every one, and a
    type's class name is canonicalized where the type is resolved, so
    messages print the declared name.
  - **Member resolution order.** `own_owner_names` walks `extends` and
    `implements` breadth-first together, so for `C extends B implements I`
    with `A::foo(): int` two levels up and `I::foo(): int|string`, `$c->foo()`
    is `int|string`. PHP resolves the class chain first, then interfaces.
  - **Trait composition.** `insteadof` is ignored (the first trait's method
    wins: `TB::hello insteadof TA` still types `hello()` from `TA`, and a
    wrong `function.alreadyNarrowedType` follows); an abstract method from
    one trait implemented by another is `class.abstractMethodNotImplemented`;
    `shown as protected` doesn't change visibility; a trait's `final`
    methods don't reach the using class, so overriding one is missed (a PHP
    fatal error); `@use Trait<int>` never binds the trait's templates
    (`class_template_bindings` ignores `uses`).
  - **PHPDoc inheritance carries only `@param`/`@return`.** An interface's
    `@phpstan-assert-if-true` doesn't reach an undocumented implementation
    (`argument.type` after the guard); `@param-out`, `@phpstan-self-out`,
    `@throws` and method templates aren't inherited either (traced).
  - **Mixins.** A `@mixin` brings in only the target's own members, not
    those it inherits, so the call falls to `__call` (`return.type`); one
    level only.
  - **`@method` tags act as override prototypes**, their PHPDoc types
    stored as native types: a real `items(): array` under `@method
    list<int> items()` reports "return type list<int> is not compatible
    with the inherited return type list<int>".
  - **Type aliases.** `@property` and `@method` tags skip
    `@phpstan-type` substitution (`class.notFound` twice); enums support no
    aliases at all (`class.notFound`, `missingType.iterableValue`,
    `return.phpDocType` on one method); an alias of an alias resolves one
    level only (traced).
  - **Enums** also ignore `@deprecated` on a case and `@var` on a
    constant, and drop `@method`/`@mixin`/`@phpstan-require-*` (traced).
  - **Promoted properties** ignore asymmetric visibility (`public
    private(set)`: an outside write, a runtime `Error`, isn't reported) and
    the promoted parameter's own `@deprecated`/`@internal`.
  - **Anonymous classes** resolve names through one whole-file
    `NameContext`, so in the first of two namespace blocks `extends Base`
    finds the last block's `Base` (`method.notFound`); their diagnostics
    print the internal `AnonymousClass<md5>` name where PHPStan prints
    `class@anonymous`. `anonymous_class.rs` is one more hand-rolled walker
    beside the Visitor (it walks `new class(...)` arguments twice).
  - **`merge` has no single meaning.** `ClassReflection::merge` and
    `FunctionReflection::merge` serve four cases - phpstorm-stubs overloads,
    a `stubFiles`-style override, a polyfill onto a built-in, a duplicate
    declaration - with per-field precedence that differs (methods unioned,
    properties and constants later-wins, hierarchy first-wins-if-set, flag
    sets unioned so `final`/`abstract` leak across). Each case gets its own
    operation; `insert_built_decl`'s enum arm bypasses the collision policy
    entirely (traced). The registry tests that pin today's union
    (`two_unconditional_class_declarations_merge_instead_of_replacing`,
    `two_unconditional_function_declarations_with_a_disjoint_type_at_one_position_union_it`
    and their siblings in `stubs_and_conditional_declarations.rs`) are
    rewritten per case, each keeping its name only where its case keeps
    its meaning.
  - **More incremental staleness:** the dead-code escape sets
    (`Project::escaped_enum_cases`, `escaped_member_classes`) only grow;
    an edit or a removal never takes a file's contributions out.
  - **The registry's own tests are white-box on what C4 replaces:** the
    188 tests under `registry/tests` and `registry/stubs/tests` build
    through `Registry::build` (provenance `Origin::Unknown`) and index
    `.classes[...]`/`.functions[...]` directly at 98 sites. They move onto
    `ReflectionBuilder::apply` and the query accessors with their names
    kept (gate 3).
  - **The language server's navigation is exact-case too:**
    `class_declaration`/`member_declaration` look up by exact name, so
    go-to-definition on a differently cased reference finds nothing
    (traced).
  - **`TemplateParam`s are rebuilt from the parallel maps in four places**
    (`own_template_args`, `class_template_params_for` in `checker/
    resolve.rs`, the ReflectionClass patch, `build_template_params`);
    `Vec<TemplateParam>` leaves one.
  - Smaller: `method_access` is keyed lowercase (a third key convention);
    doc comments are re-parsed per method; accessors return `&Option<T>`
    and `&Vec<T>` rather than `Option<&T>` and `&[T]`; the stub pass runs
    global-variable inference on each of 2,351 stub files and discards it.
- Fixed with C2's `ConstantResolver` move: `infer_const_type` evaluates
  only literals, negation, concatenation, arrays and class-constant
  references - bitwise and arithmetic operators, ternaries, `??`, global
  constants, enum cases and `new` give `mixed` (traced).
- Fixed in C5: `stub_attributes` appends a property's or class constant's
  `#[LanguageLevelTypeAware]` type as `@return`, which neither reads
  (they read `@var`), so `mysqli::$affected_rows` is untyped where PHP 8.1
  makes it `int|string` (reproduced); `newest_type` stops at the first
  non-string entry; `FunctionReflection::merge` marks a function deprecated
  when any version-split overload is.
- Fixed in C7: an assert target `$param->prop`, `$param->method()` or
  `$this->method()` is dropped as unknown; `@throws` and assert types
  skip alias substitution; `resolve_names_in_type` never namespace-resolves
  a written `Traversable` (traced).
- Fixed in C8: `access_attributes` and `compute_attribute_targets` read
  AST attributes themselves beside C3c's `ReflectionAttribute`, the first
  matching any name containing `PhpLanguageExtensions`, the second any
  attribute short-named `Attribute` (traced).
- Fixed in C9:
  - `global_vars` counts only assignment statements, so a global also
    bound by `foreach`, destructuring or a by-reference argument keeps the
    literal of its first assignment (`function.alreadyNarrowedType` three
    times, reproduced), and a top-level append (`$items[] = 'x'`) adds
    nothing, so `$items = ['a' => 1]` stays `array{a: 1}` and a correct
    `is_string()` over its values is `function.impossibleType`
    (reproduced) - the registry test
    `a_bare_top_level_append_contributes_nothing` asserts that behaviour
    and flips with the fix; its expression typing drops unkeyed array items
    (`['en', 'fr']` is `array{}`), takes call returns raw (templates,
    `static`, dynamic return types unresolved) and resolves `self::` as
    `Ns\self` (traced);
  - purity misses backtick execution and `throw` expressions, classifies
    calls and `new` by written name, not the resolved one (a same-named
    global function becomes the dependency), treats a class-constant read
    as impure, and looks method dependencies up on the named class only
    (traced).
- Fixed in E4: `doctrine_entity` records every property as a DQL field,
  mapped or not, misses fields from a `#[MappedSuperclass]` parent or a
  trait, and reads no docblock `@ORM` annotations (traced).
- Outside Stage C, for the checker: a by-reference argument PHP creates on
  the call is type-checked against the parameter (`preg_match('/a/', 'a',
  $z)` reports `argument.type` "mixed given"); PHPStan skips it.

**C4. Snapshots and lazy resolution**, one commit each:
- per-class memoized resolution replacing the passes. Today's 15 passes
  (16 calls) and where each goes:
  - per-class memos here: `flatten_traits`, `resolve_method_tags`,
    `precompute_member_owners` (twice), `resolve_deferred_type_imports`,
    `resolve_deferred_value_of`, `flatten_phpdoc_inheritance`,
    `inherit_property_hooks`, `resolve_deferred_mixins`,
    `precompute_ancestor_names`, `drop_doc_members_excluded_by_native`,
    `resolve_constant_named_types` (through the ConstantResolver, C2);
  - deleted by `Name` keys: `canonicalize_class_name_case`;
  - C6: `apply_assertion_table`;
  - C9: `resolve_transitive_purity`, `infer_global_variables`.

  The three lists are not the same list: `index_and_flatten` (each stub
  file, `Registry::build`) runs the second `precompute_member_owners`
  last, after `drop_doc_members_excluded_by_native`, while
  `finish_loading` and `update_file` run it before, with purity between,
  so their owner maps can name a member the drop removed. Stub classes
  are flattened within their own file at stub load and again per
  project. The memo has one order, and a fixture pins the dropped-member
  case.
  The memo has levels - ancestors, then members, then inherited PHPDoc -
  and a hierarchy query made while resolving members reads only the
  ancestor level, so resolution never re-enters a class's own cell
  (`OnceLock` re-entrance deadlocks). A cyclic `extends`/`implements`/
  `use` is detected and resolved as broken, with a test;
- the C3 residue: class keys as case-insensitive `Name` (the lowercase side
  map and `canonicalize_class_name_case` deleted) and case-folded method
  keys, `Vec<TemplateParam>` and per-member flags, build scratch in the
  builder, provenance as the only location record, and query accessors
  in place of the storage-returning ones and the `pub` maps (every direct
  `registry.classes`/`functions`/`global_constants` access converted);
- `ReflectionBuilder::apply` snapshots, which own their names: the
  registry's and checker's calls to the process-wide
  `syntax::intern::intern` (about 100) go through the project's interner.
  Names of the process-wide stub layer (C5) stay global; `Name` equality
  falls back to text, so mixing is correct, and the hot comparisons are
  measured. A session-scoped interner that holds strong references frees
  nothing until the session drops, which in the LSP is only on
  reconfigure, so it evicts names no longer referenced elsewhere;
- the staleness fixes (approved deltas). Beyond the three in §1, the
  incremental path (`Project::unindex_file`,
  `Registry::remove_declarations`) has these:
  - a file's global constants are never removed;
  - removing a file whose declaration merged onto a built-in stub (the
    `str_contains` polyfill, a user `Imagick` stub) deletes the built-in;
  - removing one of two files declaring the same name removes it for both;
  - `stub_provided_functions`, `names_with_assert_tags` and
    `function_decl_sites` are never cleared, and
    `doctrine_entity_table_index` is never reset.

  Per-file declaration sets fix each one structurally; each gets a
  closure gate. `unconditionally_declared_classes`/`_functions` record
  whether a project declaration is conditional and become part of those
  per-file sets.

Gate: the fixture suite's and the lib tests' wall times (`gatetime`) are
recorded and lower than at the phase's start; per-class resolution no
longer re-runs every pass over the stub classes for each project. Today
`xtask` only writes `gatetime/<hash>.<command>.tsv`; nothing compares two
of them, so this gate (and C5's) gets a command first: `xtask gatetime
--since <hash>`, the median of N runs per step against the phase-start
commit's, failing on any step not lower.

**C5. Locator chain and stub layer**, one commit each:
- the stub layer's version key, decided first and recorded in Appendix F:
  a project has a `PhpVersions` range (B8), not one version, and the key
  decides signature selection, the functionMap deltas (C6) and the
  version-specific `.stub` files (`socket_select.stub` vs
  `socket_select_php8.stub`). `php_version` accepts PHPStan's range and
  patch forms in the same commit;
- locator order;
- the lazy composer locator (approved delta): today
  `discover_vendor_context` loads whole PSR-4 directories eagerly, and
  lazy location changes what `subclasses_of`/`implementors_of`, the
  dead-code and duplicate-class rules and the purity / global-variable
  inputs see, and which of two vendor declarations wins. Functions,
  constants and `files` autoload entries stay eager;
- the version-keyed, per-class `OnceLock` stub layer with copy-on-merge,
  plus a per-file parse memo (one phpstorm file declares many symbols);
  whether the index build keeps `rayon` is decided here, and Appendix E
  follows;
- provenance queries: `built_in_stub_classes` and `stub_provided_functions`
  become queries against the index (the other two sets went to C4);
- the enumeration APIs. Each whole-registry iteration would force every
  lazy stub class, so each moves onto a scoped query:
  `native_type_hints.rs`'s two scans (every class, `is_subclass_of`, per
  member) onto `subclasses_of`; `Project::run_reflection_rules` (every
  class and function, stubs included) onto the project layer;
  `repository_entity`'s scan onto ext-doctrine's data (E4 keeps it until
  then); the LSP workspace-symbol function list onto the project layer;
  LSP completion's class and function candidate lists (`lsp/completion.rs`,
  every class and function, stubs included) onto `names(kind, layer)`.
  The index records `parent`/`interfaces` for its first declaration only,
  so for a class declared differently per version
  (`couchbase`/`couchbase_v2`) `subclasses_of` must read each
  declaration's ancestors: the index records them per declaration;
- version-based skip-polyfill (approved delta);
- version-dependent signature selection (approved delta);
- a panic-injection test.

Gate: the fixture suite's, the lib tests' and the cli tests' wall times
(`gatetime`) are recorded and lower than at the phase's start; the stub
layer replaces the per-project stub clone and the per-process stub
parse. And the union of all lazily resolved symbols equals today's
`generated_stub_data()`, extension maps included, for the unset version
key, checked before the two approved-delta commits, which change it by
design.

**C6. Overlay layer**, one commit each. Every entry's types are parsed as
PHPDoc through the phpdoc grammar (TypeNode), never `parse_type_string`,
which C7 deletes:
- phpstan-src `.stub` PHPDoc overlays (approved delta, split per stub
  file): today `patch_phpstan_stub_classes` takes only class templates,
  bounds, variance and property types; the 144 top-level functions in 13
  stub files (`core.stub` 63, `file.stub` 31, `arrayFunctions.stub` 16,
  `typeCheckingFunctions.stub` 16, `ctype.stub` 10, ...) and all method
  PHPDoc are new, and overlap the built-in typing E2 moves;
- declarative patch entries;
- the `functionMap.php` + deltas layer: `stub-sources` embeds
  `resources/functionMap.php` and its six `functionMap_php*delta.php`
  files (the `bleedingEdge` variants decided and recorded), the index and
  its regeneration test cover them, and a `SignatureMapParser` port reads
  them. It replaces `patch_functionmap_array_shapes`;
- user `stub_paths` PHPStan semantics (approved delta);
- `StubFilesExtension`;
- assertion tables compiled to overlays (`apply_assertion_table` deleted).
  The Mockery and Nette Tester defaults move to their ext modules, and
  `gates/module-graph.txt` splits `assertion_tables` so the defaults can't
  be extracted with `reflection`.

**C7. TypeNodeResolver** with `TypeNodeResolverExtension`;
`parse_type_string` is deleted. It replaces the B5 adapter
(`registry::type_node`: `parse_type_node` and `mentions_mixed`, both still
going through the node's printed text) and the checker's
`resolve_type_node`. The deletion reaches the extracted `types` crate:
- `types::parse` (1,286 lines, with `parse_native_type_string` and the
  string-based `has_explicit_mixed_nested_somewhere`) goes;
- about 150 `types` tests build their types through it
  (`tests/parsing.rs` 67, `tests/accessory_and_refinement_types.rs` 60,
  `tests/display_and_misc.rs` 17, `tests/subtyping.rs` 7). The resolver
  sits above `types`, so they can't use it without a dev-dependency
  cycle: the relation and display tests build types with constructors,
  and the parsing tests move to `reflection` as resolver tests;
- the other package call sites (about 35, besides the adapter): the
  `stubs.rs` patches (C6 has replaced them), `predefined_constants.rs`
  (10), `class_info.rs` (4), `expr/mysqli.rs` (3), and one or two each in
  `restricted_symbol_usage`, `phpdoc_unresolvable`, `missing_typehint`,
  `native_type_hints`, `narrowing`, `preg_shapes`, `array_ops`,
  `doctrine/result_shape`, `constant_resolver`, `name_context`,
  `function_sig`, `assertion_table` and `deps`, each listed in the commit;
- native type hints go the same way: about 85 sites print a `TypeHint`
  node (`TypeHint::to_string`) and re-parse it with
  `parse_native_type_string`. The resolver takes `syntax` `TypeHint`
  nodes directly, and `parse_native_type_string` goes with
  `parse_type_string`;
- one resolver, one scope: today the registry (`from_decl`: templates
  marked, names resolved, class aliases substituted, deferred value-of)
  and the checker (`resolve_written_type`: templates and names, with
  aliases applied separately at three call sites) each assemble their own
  pipeline. The TypeNodeResolver takes one name scope - namespace and
  `use` imports, active templates, class aliases, the current class for
  `self` - and both use it.

**C8. Reflection extension points** (§3.5), with the built-in implementors
(universal object crates among them, from C1's `ReflectionSettings` key).
Reflection implements ClassHierarchy: today's `impl ClassHierarchy for
Registry` (`registry/relation.rs`) becomes reflection's; B4 already
deleted the duplicate subtyping implementations. The callers still on
`Relation::structural()` (a class a subtype only of itself) switch to the
hierarchy-aware relation, which lazy reflection makes available there - an
approved-delta commit. They are the 24 package sites (among them the
narrowing exclusion in `narrowing/type_ops.rs` and the doc-vs-native
reconciliation in `function_sig/type_resolution.rs`, which runs at
registry build) and three inside `types`, whose API takes a
`&dyn ClassHierarchy` for them: `conditional_branch`
(`types/src/conditional.rs`; `($x is Foo ? A : B)` with `$x` bound to a
subclass of `Foo` doesn't pick `A`), `intersection.rs`'s `narrower`
(`Foo&SubFoo` isn't collapsed) and `coercion.rs`'s string check.
The delta, predicted site by site (the scalar-only sites change nothing:
`operators.rs`, `accessory_range.rs`, `assignment.rs`, `builtin_calls.rs`,
`coercion.rs`, and `type_resolution.rs`, whose object members return
before reaching the relation):
- `narrow_instanceof_true` (`type_ops.rs`) narrows `Base` by `instanceof
  Child` to `Base&Child` instead of `Child` (reproduced);
- `narrow_type_excluding` keeps `Child` in `Child|Other` after `!$x
  instanceof Base`, so `$x->only()` there reports `method.notFound` on
  `Child|Other` (reproduced);
- `conditional_branch` doesn't pick `int` from `($x is Base ? int :
  string)` for a `Child` argument, a false `return.type` at the caller
  (reproduced);
- `intersection.rs`'s `narrower` leaves `Foo&SubFoo` uncollapsed;
- the DQL entity-field refinement in `sql_reflection/resolve.rs` compares
  object field types structurally.

`ClassHierarchy` gains the enum-cases query §3.2 lists, with the first
relation that needs it. The trait itself is redone for the reflection
it now fronts: it takes `&str` names where §3.2 has `Name`; it allocates
on every call (`ancestor_names` returns a `Vec`,
`ancestor_template_bindings` an `FxHashMap<String, Type>`) where the
memoized views can lend; and `ClassFacts::templates` is `&[Arc<str>]`,
not the `TemplateParam`s C4 introduces. The class-reflection data extensions take their
first non-built-in implementors here: `registry::access_attributes`
(ext-language-extensions) and `registry::class_info::doctrine_entity`
(ext-doctrine) move onto them in the package's future ext modules, and
their module-graph edges, dated E4 today, are re-dated to C8, so C10 can
extract. Attribute arguments are evaluated to types (constants through
the ConstantResolver, `new` and constant expressions included).

**C9. Summaries.** Purity and global-variable inference leave reflection for
the summaries API. The ROADMAP mapping is recorded. C9 creates
`AnalysisContext` in the package's future `analyser` module to hold them
per reflection snapshot (§3.8); D6 extends it. `purity_dependencies` and
`is_transitively_pure` leave `FunctionReflection`. Whether stubs have
global-variable summaries is decided: today `index_and_flatten` runs
`infer_global_variables` for each of the 2,351 stub files.

**C10. Extract `reflection`.** Its module-graph edges are all gone by now
(C8 moved the two ext data modules, C9 the summaries); `cargo machete`
checks Appendix E's rows for it.

### Stage D: engine (in the package, module-gated)

**D1. Holder storage (byte-identical).** Every `Locals` map moves into holder
storage with its current per-kind merge and invalidation policy; interned
ExprKey; persistent maps. Benchmarked on the merge/clone hot path.

**D2. Convergence onto PHPStan semantics**, one approved-delta commit each,
with a fixture per transition and trigger:
- guards (each kind), `method_exists()`/`property_exists()`/
  `array_key_exists()` producing B4's `HasMethod`/`HasProperty`/`HasOffset`
  intersection members instead of the `guarded_methods`/
  `guarded_properties` side sets;
- `container_field_facts` folding;
- conditional holders with `mergeWith`;
- frozen condition aliases;
- certainty plus maybe-undefined (level-gated), with each variable's
  certainty added to the type snapshot;
- `afterExtractCall` replacing extract/eval truncation;
- invalidation unification (receiver rebind, by-ref, declare purges);
- the transition set;
- natives as full holders plus `native_type_of`;
- convergence equality;
- virtual keys;
- provenance as holder metadata;
- non-holder scope state;
- the PHP version via `PHP_VERSION_ID`.

**D3. Results and contexts.** `StatementResult`/`ExpressionResult`; context
passing replaces the take-idiom flags; per-function mutable fields are
deleted.

**D4. One inference path.** ExprHandlers, per-`NodeId` results, `infer_expr`
deleted, RefCell channels removed; the file-size gate.
- `$this`/`static` inside a class body carry their bound class
  (`Type::This`/`StaticRef` bound), displayed `$this(Base)`/`static(Base)`,
  and `@return $this` rejects a returned `new static()`.

**D5. NodeCallback sink** (approved deltas for `return.unusedType`,
`throws.*` and `property.unusedType`).

**D6. Engine APIs**, one commit each:
- Scope API;
- TypeSpecifier;
- returned-error `Rule` and dispatcher;
- derived-node catalogue plus the site-mapping table;
- Collector with collected data;
- Observer;
- the extension-trait registry;
- the concurrency / state model;
- `AnalysisContext` completed (C9 created it for the summaries);
- `InternalRule` deletion;
- porting the external `Rule`/`ReflectionRule` users.

**D7. Parse-time annotations consumed.** The corresponding `Checker` fields
are deleted.

### Stage E: everything out of the engine (in the package, module-gated)

**E1. Collectors and project-level rules**, one commit each:
- the reference-site collector (a built-in collector, consumed by the dead
  code rules and, later, the LSP);
- dead code / members / enum cases / abstract classes;
- duplicates;
- circular namespaces;
- architecture;
- banned;
- class_shape with the parent-call registration API and the Doctrine
  exemption hook;
- composer hygiene (package context, no I/O);
- `property.unusedType` as a ClassPropertiesNode rule over host-class
  assigns, including trait bodies per host (`facts_only`, the pre-pass
  and every `*_facts` field deleted);
- renamed named arguments;
- the trait-host constant-condition verdict as a collector plus rule;
- the (span, message) dedup moved to the finalizer;
- `constant_facts` as per-file rule state.

**E2. builtins.**
- Moves onto the extension traits: `throws/dynamic.rs`, `builtin_calls`,
  `array_ops` returns, `preg_shapes`, `date_time`, the contextual-callback
  block, preg `$matches` out-types, GMP / BcMath operators, the built-in
  narrowing predicates, the date / xml / DateInterval validators (as
  throw-type extensions), regex / PCRE, and the date parser.
- `builtins` is always registered, including by the testing harness.

Gate: the analyser's built-in-name allowlist.

**E3. Every diagnostic to `rules`/ext modules.** Sub-phases grouped by
derived node, each listing its Appendix A rows:
- (a) file-level (`keywords`, `whitespace`, RequireFileExists over the VFS
  from `includes.rs`);
- (b) expression-level;
- (c) calls and member access;
- (d) assignments and properties (`uninitialized` and
  `uninitialized_dataflow` rewritten over the ClassPropertiesNode
  initialization state, with the dataflow walker deleted);
- (e) declarations, generics checks and PHPDoc;
- (f) control flow, returns, throws and generators;
- (g) the restricted-usage framework: `@internal` core, deprecation
  (ext-deprecation-rules), disallowed-calls (its path zones already
  decided at the report site since B7), and `phpstan_api`;
- (h) `FileAssertRule`, the dump rules and the four asserts;
- (i) semantic-token predicates exposed.

Mixed modules are split row by row (Appendix A). Gate: architecture rule 10
for the analyser.

The rest of the diagnostic model (§3.7) that B7 left for the dispatcher
lands here, each its own commit:
- `RuleError` and `RuleErrorBuilder`: rules return them, and the dispatcher
  turns each into a `Diagnostic`;
- file and trait file on every diagnostic, attached by the dispatcher (a
  trait method's error analysed in a using class carries the trait's
  file);
- identifier families: each rule declares its identifiers as prefix ×
  suffix sets, the dispatcher debug-asserts every error against its rule's
  declaration, and the `format!`-built identifiers (about 40 sites today)
  become family members;
- AST-node fixes beside text fixes, rendered to diffs through
  `syntax::printer`;
- `accepts` reasons: `Relation::accepts` fills `AcceptsResult.reasons`
  (B4 ships the field empty), and the argument, return and property type
  rules show them as tips;
- PHPStan's standard tip helpers (§3.7: discovering symbols,
  `treatPhpDocTypesAsCertain`, and the rest), used by the rules that show
  those tips.

**E4. Extension modules.**
- Order:
  1. ext-doctrine's remaining modules (its reflection data extension moved
     in C8, and E4 fixes its entity metadata: only mapped properties are
     fields, `#[MappedSuperclass]` and trait fields are included, docblock
     `@ORM` annotations are read);
  2. ext-sql;
  3. ext-phpunit, ext-codeception, ext-mockery and ext-mockstan, after E1;
  4. ext-pest (closure-this, the file-set hook, pest risky tests);
  5. ext-nette-tester;
  6. ext-strict-rules, which also completes the port;
  7. ext-deprecation-rules;
  8. ext-disallowed-calls;
  9. ext-shipmonk, ext-type-perfect, ext-voku, ext-sidz, ext-slam,
     ext-psr3, ext-language-extensions and ext-extras.
- Extensions register parse-time visitors of their own (§3.1's
  annotations), through the registration API.
- ext-sql resolves static-schema SQL (SELECT/CTE/derived-table columns
  against the catalog) and validates parameters through an `sql` API;
  `sqlparser` leaves the package.
- The Doctrine QueryBuilder chain becomes extension type data, using
  method type-specifying (statement-level `$qb->andWhere()`) and an
  `ExpressionTypeResolverExtension`. This is an approved-delta commit with
  fixtures for aliasing, statement mutation and arguments that change
  between assignment and use.

**E5. Levels** (§3.9): the ladder, `RuleLevelHelper` parameters,
computed-level sites, the default level, and the replacement consumers for
JSON and the LSP. The level-dependent acceptance still inside
`types::Relation::accepts` moves into `RuleLevelHelper::accepts`/
`find_type_to_check`, and `types::Relation` loses its
`configured_level`, `strict_mixed_arrays` and
`check_benevolent_union_types` fields. The level tests become level-header fixtures.
LevelsTestCase fixtures are added. Gate: for every level in 0..=10 plus
unset, before == after, except approved deltas.

**E6. Observers.** LSP-only presentation (semantic tokens, inlay hints,
hover, definition probes) on observer events.

**E7. Checker decomposition.** Gate: the exact enumerated field sets for
engine context, file state, function frame and traversal context
(Appendix C), with interior mutability only in pure memo caches.

### Stage F: Session and LSP (in the package)

**F1. Session** (§3.10), one commit each:
- `AnalysisResult` (landed early as GATE_SPEED_PLAN S9a;
  `project::session::AnalysisResult`);
- the finalizer (landed early as S9a, `AnalysisResult::finalize`, with a
  temporary level stage E5 removes; the LSP moves onto it in this
  phase's parity commits);
- entry points;
- the VFS;
- panic isolation;
- cancellation, last-good AST, `reconfigure`;
- one Session per root;
- LSP parity for each divergence listed in §1;
- `ResolvedWorkspaceConfig` deleted;
- the exit code (approved delta).

The fixture harness backend switches to Session.

**F2. Result cache and dependencies**: resolved dependencies, fingerprints,
the persistent cache, incremental collectors, `deps.rs` deleted. The LSP
replay gate goes on.

**F3. ErrorFormatter** set (JSON schema versioned).

**F4. Binary**: check-project's resolution already lives in the library
(`app::check_project`, landed early as GATE_SPEED_PLAN S9b); global
flags, `--set`, `check`/`demo` deleted,
`incremental-bench` → project example plus bench, `format_signature` test
moved.

### Stage G: extraction wave and tidy

**G1. Extraction** of `analyser`, `builtins`, `rules`, each `ext-*`,
`project`, `testing`, `app`, `lsp` and the bin. Each extraction lists its
public-API diff against the design and passes the rename check; the lints
are active per crate.

**G2. Fixtures to their owners.** Each crate gets its own
`tests/fixtures`. `analysis-tests` keeps only cross-extension integration
fixtures. The bench moves to `project/benches` over the embedded sources.
The `structured_php` fuzz target moves to `project`.

**G3. Docs.**
- README;
- ROADMAP (56 `src/` citations);
- the fuzz guide;
- `.claude/workflows/*.js`;
- `.corpus-runs/README.md`;
- `editors/vscode/README.md`;
- the triage report;
- the `.cargo/config.toml` comment;
- the memory test command, which becomes
  `cargo test -p <crate> --test fixtures -- <filter>` or `--lib` for
  white-box tests.

**G4.** Assert the build-time targets.

## 6. Risks

- **Semantic drift in D2.** Each convergence is an approved-delta commit
  with its own fixtures, a triaged Piwigo diff and a type-snapshot diff. D1
  must be byte-identical.
- **Dispatch and memory overhead.** Prefilter tables, lazy derived nodes
  and O(1)-clone holder maps, guarded by the per-commit time and memory
  gates relative to the previous commit.
- **Cross-crate inlining.** Thin LTO plus `codegen-units = 1`, decided and
  baselined in A0. The timing gate applies from B1 on.
- **Fixture conversion.** Weak assertions surface extra diagnostics, which
  the fixtures now record; the dual-run gate ensures no old assertion is
  lost.
- **User-visible changes.** Config schema (with a shim), CLI flags,
  baseline format (with a converter), JSON schema (versioned), exit code,
  output order. Each is its own changelog-noted commit.
- **Package build time during D–F.** The engine redesign happens in the
  package before its extraction. That is deliberate (§ ordering principle),
  and the per-commit build-time gate tracks it.

## Appendix A: module assignments

"Split" means rows with different destinations inside one file. Each row
moves in the phase named.

| Module | Destination | Phase |
|---|---|---|
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
| php_version | types | B4 |
| php_float_format | types | B4 |
| php_date_parser | builtins | E2 |
| regex_groups, pcre_validate | builtins::regex | E2 |
| php_stubs + build.rs | stub-sources (sources, index) + reflection::stubs (layer) | B9/C5 |
| registry/* (declarations, hierarchy, index, resolve, name_context, class_info, function_sig, generics, super_type, anonymous_class) | reflection | C3–C10 |
| registry/assignability | types (unified relation) + analyser RuleLevelHelper | B4/C2 |
| registry/assertion_table, assertion_tables | reflection overlays; defaults → ext-nette-tester / ext-mockery | C6 |
| checker/predefined_constants, checker/constant_resolver (registry/dynamic_constants was folded in at C2) | reflection ConstantResolver | C2 |
| checker/rule_level_helper | analyser RuleLevelHelper | C2/E5 |
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
| project/use_sites, decl_locations | built from the reference-site collector | E1 |
| lsp, lsp/* | lsp | G1 |
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
| expr | analyser ExprHandlers + rules (inline reports); useless cast / strict array filter / strict function calls → ext-strict-rules; loose haystack → ext-extras | D4/E3 |
| expr/array_ops | builtins (returns) + rules (array reports); array key cast → ext-extras | E2/E3 |
| expr/builtin_calls, expr/preg_shapes | builtins | E2 |
| expr/member_resolution | analyser (member resolution); Doctrine / DBAL branches → ext-doctrine / ext-sql | D4/E4 |
| expr/mysqli | ext-sql | E4 |
| expr/probe_checks | ConstantResolver (global_constant_type) + rules (null probes, via TypeSpecifier) | C2/E3b |
| expr/ranges | analyser (int-range arithmetic) | D4 |
| function_decl | analyser (function entry) + rules; empty bodies, never-return suggestions → ext-extras; checked exceptions → ext-extras (throws/missing) | E3e |
| function_like_declarations | rules | E3e |
| generics.rs, generics/substitution | analyser | D4 |
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
| narrowing, narrowing/{accessory_range,array_shape,type_ops} | analyser (TypeSpecifier) | D2 |
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
| var_tag_types | analyser (native_expr_type → native_type_of) + rules::phpdoc (VarTag checks) | D2/E3e |
| whole_program/* | rules (collector rules) | E1 |
| access_attributes | ext-language-extensions; TEST_BASE_CLASSES → test-framework hook (ext-phpunit / ext-codeception) | E4 |
| tests/cli.rs, version.rs, lsp_exit_code.rs | bin package | B1 |
| benches/vendored_corpus.rs | project/benches | G2 |
| fuzz targets | lex_parse / token_stream_parse → syntax; parse_type_string → phpdoc; structured_php → project | B3/B5/G2 |
| examples/stress*.php | fixtures or deleted | A2 |
| proptest-regressions | bin package, then types | B1/B4 |

## Appendix B: settings

Every `Registry` setting, with its current readers and its owner.
Non-Registry keys follow the table.

| Setting | Read today in | Owner |
|---|---|---|
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
|---|---|
| paths, level, ignore_errors, baseline_*, report_unmatched_ignored_errors | SessionSettings |
| scan_paths, stub_paths, no_composer | ReflectionSettings |
| architecture, banned_*, check_closed_classes, check_composer_hygiene | rules sections |
| mock_banned_classes | ext-mockstan |
| parent_calls, default_parent_calls | rules::class_shape (+ ext defaults) |
| assertions, class_argument_returns | reflection overlays |

## Appendix C: `Checker` fields (67) and where they go

| Fields | Disposition |
|---|---|
| registry | AnalysisContext (reflection snapshot) |
| diagnostics | deleted (returned errors) |
| semantic_tokens, probe_span, probe_hit, probe_var_provenance, collect_inlay_hints, inlay_param_name_hints, inlay_local_type_hints | LSP observers |
| collect_member_refs, member_ref_sites, escaped_enums, escaped_member_classes | reference-site collector |
| name_ctx | deleted (parse-time resolved names, D4) |
| current_file, anonymous_class_names, current_anonymous_class | file state (anonymous names from reflection) |
| current_class, current_method, current_method_templates, current_class_attrs, current_method_attrs, current_trait | Scope (class / function / trait context) |
| current_scope_deprecated | Scope query (deprecation predicate) |
| saw_dynamic_var_construct | scope `afterExtractCall` |
| current_function_is_generator, current_declared_return_type, current_declared_throws_type, current_function_saw_explicit_return | function frame / function-exit node |
| current_catch_types, current_function_goto_targets | parse-time annotations |
| current_function_throw_points, current_function_explicit_throw_points, current_function_has_unannotated_call, current_function_return_points | StatementResult / function-exit node |
| current_function_by_ref_params, current_function_by_ref_exit_snapshots, current_function_by_ref_params_with_tag, current_function_byref_snapshot_names | virtual keys (ParameterOriginalValue) + function-exit node |
| current_function_assigned_variables | ext-strict-rules file state |
| in_bare_statement_position, in_condition_position, in_exempt_last_condition | ExpressionContext / StatementContext / parse-time attributes |
| pest_this_type, pest_hook_properties | ext-pest (closure-this ext + file-set hook) |
| include_guarded_paths, include_path_dirs, include_constant_initializers | RequireFileExists rule state |
| enum_chain_tails | rules (enum exhaustiveness, ext-shipmonk) file state |
| constant_facts | magic-constant rule file state |
| by_ref_empty_dim_args | by-ref argument context on the resolved-call node |
| property_assign_facts, cross_file_property_assign_facts, named_argument_call_facts, renamed_parameter_override_facts, facts_only | deleted (ClassPropertiesNode rule / collectors) |
| external_rules | rule registry in AnalysisContext |
| loop_break_states, loop_continue_states, loop_depth | StatementResult exit points + traversal context |
| in_range_index_writes, active_discriminant, pending_or_disjunctions | conditional holders / loop context |
| static_var_names | scope state (static-variable status) |
| rebind_target_classes, rebind_escape_all | reference-site collector (escape analysis) |
| call_candidate_cache | pure memo cache (allowed) |

## Appendix D: `Locals` and its merge policies

| Member | Merge today | Target |
|---|---|---|
| types | only if on both sides, unioned | holders with certainty (Maybe for one side) |
| native | only if on both sides, unioned | native holders, narrowed |
| provenance | per-variable union | holder metadata (on observer request) |
| guarded_functions / guarded_constants | intersection | FuncCall / ConstFetch holders |
| guarded_methods / guarded_properties | intersection, never invalidated | HasMethod / HasProperty accessories |
| condition_aliases | `Rc::ptr_eq` (AST replayed lazily) | frozen conditional holders |
| branch_implied_facts | union | conditional holders (mergeWith intersection) |
| container_field_facts | `Rc::ptr_eq` | element-type refinement |
| discriminant_array_facts | union | conditional holders |
| query_builder_aliases | `Rc::ptr_eq` | ext-doctrine extension type data |
| any_variable_may_exist | OR | `afterExtractCall` (AND) |
| may_have_member_narrowing | derived | deleted |
| track | flag | engine mode |

## Appendix E: third-party dependencies

| Dependency | Crate(s) |
|---|---|
| clap | bin |
| crossbeam-channel, lsp-server, lsp-types | lsp |
| dhat | bin |
| hotpath | every measured crate (plain dependency); features only on the bin |
| imbl | syntax (name resolution's `use` map); analyser (holder maps); reflection only if its snapshots use it |
| indexmap | reflection |
| md5 | reflection (anonymous-class naming) |
| pcre2-sys | builtins (`pcre-validate`) |
| rayon | project; reflection while it indexes stubs and runs purity in parallel (decided in C5) |
| regex | config (globs), app, reflection where used, lsp |
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

- **Scope reporting.** Rules return `Vec<RuleError>`, as in PHPStan and
  today's external `Rule`. `Scope::report` does not exist.
- **Diagnostic naming.** The reported item is `Diagnostic` (the compiler
  and LSP term, and the crate's name), not `RuleError`: it also carries
  parse errors, unmatched ignores, project-level findings and stale
  baseline entries. `RuleError` names only what a rule returns, before the
  dispatcher locates it - PHPStan's `RuleError` → `Error` split.
- **ConstantResolver's layer.** In `reflection`, with
  `dynamic_constant_names` a `ReflectionSettings` key: reflection's own
  constant-initializer and PHPDoc-type resolution needs it, as PHPStan's
  `InitializerExprTypeResolver` calls it. Supersedes C2's analyser
  placement.
- **Stage C ordering.** C6 stays before C7: its overlay entries and the
  functionMap layer parse types with the phpdoc grammar (TypeNode), never
  `parse_type_string`, so C7 deletes nothing C6 depends on. C9 creates
  `AnalysisContext` (for the summaries) and D6 completes it, so the
  summaries have no interim home. The two ext data modules
  (`access_attributes`, `doctrine_entity`) move onto class-reflection data
  extensions in C8, so C10 extracts a `reflection` with no ext code in it.
- **Review findings are fixed where the code is rewritten.** A bug found in
  code a later Stage C phase rewrites gets a fixture now and is fixed as
  that phase's own behaviour commit (approved delta) after its structural
  commit, not patched into code about to be replaced. Exceptions, fixed
  first because they are wrong output today in code that stays: the
  `PHP_VERSION_ID` range typing (C2) and C3a's `property.unused` delta.
- **Subtyping home.** A ClassHierarchy oracle trait in `types` with one
  tri-state relation, implemented by reflection. ExtensionTypeData hooks
  take `&dyn ClassHierarchy`. Level-dependent acceptance lives in
  `RuleLevelHelper` (analyser).
- **Assertion tables.** PHPDoc overlays in reflection, compiled at config
  load. There is no engine extension.
- **ext-sql and ext-doctrine.** No crate edge between them. ext-sql owns the
  DBAL Connection / QueryBuilder typing. ext-doctrine publishes entity
  metadata through a reflection data extension, which lands in C8 so
  `reflection` can be extracted in C10. DQL typing reads the schema from
  `sql`.
- **Stub index.** Checked in, generated by an xtask, and verified by a
  regeneration test. `stub-sources` has no build-dependency on the parser.
- **Fixture timing.** Recorded in A2 from the Stage-A binary, behind a
  harness whose backend moves Project API → Session (F1), and moved to
  owning crates in G2. Expectations change only through approved deltas.
- **Refactor-then-extract.** Chosen over extract-first: it avoids interim
  homes, double visibility changes and dev-dependency cycles. Its cost is a
  larger package during Stages D–F, tracked by the build-time gate.
- **Publishing.** Crates stay `publish = false`. Third parties consume them
  via git against `app`. Publishing to crates.io would require stable APIs
  and a semver policy, and is out of this plan's structural scope.
- **Scope API exclusions.** Excluded PHPStan `Scope` methods are listed with
  reasons in the D6 commit.
- **`Type` as a closed enum.** Kept: exhaustive matching is compiler-checked
  coverage. The redesign gives every PHPStan type concept its own variant,
  and `ExtensionTypeData` covers open extension data.
