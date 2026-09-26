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
    diagnostics   Diagnostic/RuleError, builder, tips, fixes (text + AST),
                  Identifier (+ families)                         (syntax)
    composer      composer.json / installed.json model, package context
                                                                  (syntax)
L2  config        loader, global settings, section-owner registry with
                  schema validation                               (diagnostics)
L3  reflection    ReflectionProvider snapshots, locator chain, stub layer
                  (PHP-version keyed), PHPDoc overlay layer, TypeNodeResolver,
                  class-reflection / deprecation extension traits,
                  ReflectionAttribute, provenance, implements ClassHierarchy
                                    (syntax, types, phpdoc, stub-sources, composer, config)
L4  analyser      engine: holder scope, ExprHandlers, results, TypeSpecifier,
                  NodeCallback sink, Scope API, Rule/Collector/Observer,
                  derived nodes, extension-trait registry, RuleLevelHelper,
                  ConstantResolver, AnalysisContext, summaries API, reference
                  collector. Emits nothing
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
- parent and interfaces (for `implementors_of`/`subclasses_of`);
- PHP-version gates.

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
  - a case-folded function key map replacing the linear scan;
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
- **No settings.** Level-dependent acceptance leaves for `RuleLevelHelper`,
  dynamic constants for `ConstantResolver`, and the `phpstan_api` file
  recording becomes unconditional. Gate: no setting read under
  `crates/reflection`.

### 3.6 config
- Global keys plus per-owner tables (`[rules.*]`, `[ext.<name>]`, the
  existing tables re-homed).
- Each owner registers a typed serde section with `deny_unknown_fields`.
  The loader rejects unowned sections and keys, reporting the full key path
  (a `parametersSchema` equivalent).
- A one-release compatibility shim maps the old flat `check_*` keys, with a
  warning. The Piwigo toml, README and VSCode docs migrate in the same
  commit.
- The ~60 per-rule CLI flags and all `Project::set_*` setters are deleted,
  replaced by a generic two-way `--set key=value`.
- Settings split three ways, following Appendix B:
  - `SessionSettings`;
  - `ReflectionSettings` (scan/stub paths, composer toggle, universal
    object crates, type aliases, PHP version range);
  - `EngineSettings` (treatPhpDocTypesAsCertain, polluteScope*,
    early-terminating calls, exception model,
    rememberPossiblyImpureFunctionValues).

### 3.7 diagnostics
- `RuleError`, built by `RuleErrorBuilder`, carries:
  - message;
  - line/column/end;
  - file and trait file;
  - a static `Identifier` from declared families (prefix × suffix sets,
    syntax-validated, debug-asserted against the rule's declaration);
  - a list of tips, with PHPStan's standard tip helpers;
  - text fixes and AST-node fixes (rendered to diffs via
    `syntax::printer`);
  - `non_ignorable`.
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
  - The dispatcher attaches file / trait file / line, validates
    identifiers, applies `IgnoreErrorExtension`, and drops non-final-pass
    output.
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
  - `ConstantResolver` (declared, predefined and version-aware, plus
    `dynamicConstantNames`);
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
  - panic isolation and the 64 MiB pool;
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
  - gates 1–3 without `cargo hack`/`machete`/`deny`;
  - the scoped and all-on corpus runs;
  - the type snapshots;
  - baseline compatibility.
- **full** (`--full`, run at the end of every phase and on any commit
  that touches levels, acceptance or performance-sensitive code):
  - everything in quick;
  - the unscoped run;
  - levels 0–10 for scoped and all-on;
  - `cargo hack`, `machete` and `deny`;
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
   not depend on. From Stage B on, zero exceptions.
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
5. **Type snapshot.** Inferred type, native type and certainty for every
   variable at every statement boundary, over Piwigo and the fixture
   corpus.
6. **Baseline compatibility.** A baseline generated by the Stage A binary
   (scoped and all-on) must keep producing zero diagnostics and zero stale
   entries.
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
    - no setting is read under reflection;
    - no `Locals` map outside the holder model;
    - no engine I/O;
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

**Known bugs found during A2**, each fixed later in its own commit with an
approved delta (none blocks the split):
- false positives: `array_udiff()`-family trailing callbacks rejected;
  `(string)` of a resource; a differently cased `catch` class; a method
  called on an intersection checked against a private declaration an
  instance cannot have; `@param-out` wider than the native by-ref type;
  `list<int>&non-empty-array` reported as missing a value type; a method
  overriding a builtin's tentative return type reported as untyped;
  duplicate method declarations checked against each other's signature;
  a class name resolved case-sensitively as a static-call target or in a
  return type; `function_exists()` narrowing lost inside a closure; a
  child hooked property losing the hooks it inherits; a set hook without
  a parameter list or a short `set => expr` treated as untyped and
  non-assigning; `__set()` required to declare a return type; a pipe
  expression and an interpolated string reported as no-op statements;
- imprecise inference: a native typed property narrowed to an invalid
  written value; an invalid inline `@var` type applied; the sort family's
  by-ref argument not becoming a list; `array_splice()`'s by-ref argument
  not spliced; a failing `array_is_list()` not proving non-emptiness; a
  flag variable set beside a side effect not narrowing; a try block that
  cannot throw leaving its catch live; a non-exhaustive match not
  throwing `UnhandledMatchError`;
- found during B3: the purity classifier (`registry/purity.rs`) treats a
  dynamic method call `$obj->{$m}()` as pure when its receiver and
  arguments are.

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
- The `RuleError` model (§3.7). The file and trait file, identifier
  families declared per rule, and AST-node fixes arrive with the rule
  dispatcher (E), which has the rule declarations and the analysed file to
  attach them from; B7 gives the model its validated `Identifier`, a list
  of tips, and drops `location` and `metadata`.
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

**C2. Settings leave `Registry`**, following Appendix B:
- `RuleLevelHelper` and `ConstantResolver` land in the package's future
  `analyser` modules;
- rule and extension settings go to their owner modules;
- `phpstan_api` recording becomes unconditional.

Gate: no setting read under reflection.

**C3. Reflection data model** (§3.5): opaque reflections, attributes with
arguments, provenance, no AST bodies.

**C4. Snapshots and lazy resolution**, one commit each:
- per-class memoized resolution replacing the passes;
- `ReflectionBuilder::apply` snapshots;
- the three staleness fixes (approved deltas).

**C5. Locator chain and stub layer**, one commit each:
- locator order;
- the lazy composer locator;
- the version-keyed, per-class `OnceLock` stub layer with copy-on-merge;
- provenance queries;
- the enumeration APIs;
- version-based skip-polyfill (approved delta);
- version-dependent signature selection (approved delta);
- a panic-injection test.

Gate: the union of all lazily resolved symbols equals today's
`generated_stub_data()`, extension maps included.

**C6. Overlay layer**, one commit each:
- phpstan-src `.stub` PHPDoc overlays;
- declarative patch entries;
- the `functionMap.php` + deltas layer;
- user `stub_paths` PHPStan semantics (approved delta);
- `StubFilesExtension`;
- assertion tables compiled to overlays (`apply_assertion_table` deleted).

**C7. TypeNodeResolver** with `TypeNodeResolverExtension`;
`parse_type_string` is deleted. It replaces the B5 adapter
(`registry::type_node`: `parse_type_node` and `mentions_mixed`, both still
going through the node's printed text) and the checker's
`resolve_type_node`.

**C8. Reflection extension points** (§3.5), with the built-in implementors.
Reflection implements ClassHierarchy. The adapter and the duplicate
subtyping implementations are deleted. The callers still on
`Relation::structural()` (a class a subtype only of itself; about 24
sites, among them the narrowing exclusion in `narrowing/type_ops.rs` and
the doc-vs-native reconciliation in `function_sig/type_resolution.rs`,
which runs at registry build) switch to the hierarchy-aware relation, which
lazy reflection makes available there - an approved-delta commit.

**C9. Summaries.** Purity and global-variable inference leave reflection for
the summaries API. The ROADMAP mapping is recorded.

**C10. Extract `reflection`.**

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
- certainty plus maybe-undefined (level-gated);
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
- `AnalysisContext`;
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

The rest of the `RuleError` model (§3.7) that B7 left for the dispatcher
lands here, each its own commit:
- file and trait file on every error, attached by the dispatcher (a trait
  method's error analysed in a using class carries the trait's file);
- identifier families: each rule declares its identifiers as prefix ×
  suffix sets, the dispatcher debug-asserts every error against its rule's
  declaration, and the `format!`-built identifiers (about 40 sites today)
  become family members;
- AST-node fixes beside text fixes, rendered to diffs through
  `syntax::printer`;
- `accepts` reasons: `Relation::accepts` fills `AcceptsResult.reasons`
  (B4 ships the field empty), and the argument, return and property type
  rules show them as tips.

**E4. Extension modules.**
- Order:
  1. ext-doctrine's reflection data extension;
  2. ext-sql;
  3. ext-phpunit, ext-codeception, ext-mockery and ext-mockstan, after E1;
  4. ext-pest (closure-this, the file-set hook, pest risky tests);
  5. ext-nette-tester;
  6. ext-strict-rules, which also completes the port;
  7. ext-deprecation-rules;
  8. ext-disallowed-calls;
  9. ext-shipmonk, ext-type-perfect, ext-voku, ext-sidz, ext-slam,
     ext-psr3, ext-language-extensions and ext-extras.
- The Doctrine QueryBuilder chain becomes extension type data, using
  method type-specifying (statement-level `$qb->andWhere()`) and an
  `ExpressionTypeResolverExtension`. This is an approved-delta commit with
  fixtures for aliasing, statement mutation and arguments that change
  between assignment and use.

**E5. Levels** (§3.9): the ladder, `RuleLevelHelper` parameters,
computed-level sites, the default level, and the replacement consumers for
JSON and the LSP. The level tests become level-header fixtures.
LevelsTestCase fixtures are added. Gate: for every level in 0..=10 plus
unset, before == after, except approved deltas.

**E6. Observers.** LSP-only presentation (semantic tokens, inlay hints,
hover, definition probes) on observer events.

**E7. Checker decomposition.** Gate: the exact enumerated field sets for
engine context, file state, function frame and traversal context
(Appendix C), with interior mutability only in pure memo caches.

### Stage F: Session and LSP (in the package)

**F1. Session** (§3.10), one commit each:
- `AnalysisResult`;
- the finalizer;
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

**F4. Binary**: global flags, `--set`, `check`/`demo` deleted,
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
| registry/dynamic_constants, checker/predefined_constants | analyser ConstantResolver | C2 |
| registry/purity, registry/global_vars | analyser summaries | C9 |
| registry/access_attributes | ext-language-extensions (class-reflection data ext) | E4 |
| registry/class_info/doctrine_entity | ext-doctrine (class-reflection data ext) | E4 |
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
| dynamic_constant_names | registry/dynamic_constants, oop | ConstantResolver (EngineSettings) |
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
| name_ctx, current_file, anonymous_class_names, current_anonymous_class | file state (anonymous names from reflection) |
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
| imbl | analyser (holder maps); reflection only if its snapshots use it |
| indexmap | reflection |
| md5 | reflection (anonymous-class naming) |
| pcre2-sys | builtins (`pcre-validate`) |
| rayon | project |
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
- **Subtyping home.** A ClassHierarchy oracle trait in `types` with one
  tri-state relation, implemented by reflection. ExtensionTypeData hooks
  take `&dyn ClassHierarchy`. Level-dependent acceptance lives in
  `RuleLevelHelper` (analyser).
- **Assertion tables.** PHPDoc overlays in reflection, compiled at config
  load. There is no engine extension.
- **ext-sql and ext-doctrine.** No crate edge between them. ext-sql owns the
  DBAL Connection / QueryBuilder typing. ext-doctrine publishes entity
  metadata through a reflection data extension. DQL typing reads the schema
  from `sql`. Commit order: ext-doctrine's reflection extension first.
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
