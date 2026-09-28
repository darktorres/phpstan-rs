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
| Size | 216k lines in one crate; 4,869 `#[test]` in `src/`, 3,441 of them in checker test modules |
| Diagnostic emission | 544 `report`/`report_builder` sites under `src/checker`, ~324 in inference modules; 13 raw `Diagnostic {}` literals; 3 vector splices (external `Rule` dispatch, the trait-host dedup) |
| Post-emission processing | extract()/eval() truncates every diagnostic of its body; loops run up to 6 passes, dropping non-final passes' diagnostics but leaking their return, throw and property facts; trait methods are checked per host, then deduplicated by (span, message) |
| `Checker` | 67 fields; 74 at `76e738b4` (Appendix C) |
| `Registry` | 90 fields, 67 of them settings (Appendix B), plus SQL fields, Doctrine metadata and provenance sets; ~173 `registry.classes` reads in checker code |
| Reflection data | `ClassInfo` 48 `pub` fields, `FunctionSig` 24, `PropertySig` 22, `ParamSig` 13; member facts as parallel sets, templates as five parallel maps, AST bodies in `FunctionSig.body` |
| Reflection pipeline | 15 whole-registry copy-down passes, duplicated across `finish_loading`, `update_file` and `Registry::build` and rerun by the LSP on every edit; stale trait/mixin members and anonymous classes; an unknown ancestor counts as an ancestor |
| Stubs | 2,392 embedded files parsed eagerly and cloned into every `Registry`; first-wins precedence; ten order-dependent Rust patch functions; version attributes rewritten at lex time, newest PHP only |
| Scope | `Locals`: types, native types, provenance, eleven maps with six merge policies (Appendix D), three flags; no certainty (a one-branch variable is dropped at the merge) |
| Inference | Two implementations: `check_expr` (~4.3k lines, `&mut`) and `infer_expr` (~520 lines, `&self` with RefCell effects); `expr.rs` 6,760 lines |
| Types | Pseudo-types as magic strings in `Type::Object`; open shapes by a `"..."` sentinel; ad hoc case-insensitive class names; `Mixed(bool)` without `Hash`; three subtyping implementations plus level-gated acceptance on `Registry` |
| PHPDoc | Two grammars: `phpdoc_grammar` (validation only) and `types::parse_type_string` (1,287 lines, 140 call sites) |
| AST | No node identity; string type hints and unresolved string names (~190 re-resolutions); no sub-name spans; child-vector walkers that skip closures, worked around by 18 hand-rolled walkers; a global never-evicting interner; the lexer `eprintln!`s |
| Levels | A CLI post-filter capped at 6, a separate LSP cutoff, and emission-time thresholds from `Registry::configured_level`; an unset level mixes rules up to 6 with level-8 acceptance |
| Post-filters | Level filter, `restricted_symbols` path zones, inline ignores, ignoreErrors and baseline, each implemented separately by the CLI and the LSP |
| Config | One flat serde struct of 83 keys without `deny_unknown_fields`; 72 clap args through a 60-parameter `run_check_project`; the LSP's own config ignores ~15 keys |
| CLI/LSP divergence | The LSP skips the whole-program passes, mock hygiene, class shapes, banned, architecture, composer hygiene and composer roots, validates config differently, and lets the last root's config win |
| Orchestration | `main/check_project.rs` (1,285 lines): post-processing order, `--fix`, panic isolation, context ordering, statistics; exit code 0 when diagnostics are found |
| Incremental | `deps.rs` scrapes names syntactically; no result cache |
| Library knowledge in core | Doctrine, SQL/DBAL and mysqli, PHPUnit (9 files), Codeception, Mockery, Nette Tester, Pest, the built-in function typing, and ported third-party rule sets all live in core modules |
| Cross-file facts | `property_assign_facts` feeds one rule through a `facts_only` pre-pass; purity (1,019 lines) and global-variable inference (711 lines) run inside reflection |
| Engine I/O | `phpstan_api` reads composer.json mid-analysis; `includes.rs` stats files |
| Tooling | No working Rust CI; the bench and fuzz go through `Project`; the LSP reports `CARGO_PKG_VERSION` |

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
                                    (syntax, types, phpdoc, stub-sources,
                                    composer, config)
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
                  collector rules                                 (analyser,
                  builtins)
    ext-*         one crate per mirrored package (list below)     (analyser,
    builtins, rules, sql)
L6  project       Session, SessionBuilder (generic registrations, no ext-*
                  deps), AnalysisResult, result finalizer, VFS, result
                  cache, dependency graph, LSP-facing indexes     (analyser)
    testing       RuleTestCase / TypeInferenceTestCase / LevelsTestCase /
                  Integration / codemod harness + libtest-mimic runner (project)
L7  app           composition root: registers builtins + rules + every ext-*,
                  ErrorFormatter set; `run(args, extra)`          (project, all
                  L5)
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
- `location` and `metadata` are removed. The baseline format is versioned,
  keyed like PHPStan (path, identifier, message, count), with a converter.
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
  - exit code nonzero on remaining diagnostics, its own commit;
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
  - the corpus oracle (gate 12, from Stage D).

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
   joins it at D5.
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

**Approved deltas.** A commit that intentionally changes output records its
expected diffs (corpus, fixtures, type snapshot, baseline) in the commit.
Every changed line is reviewed for runtime correctness. The gate is:
actual diff == approved diff.
Behaviour-changing commits are always separate from structural ones.

## 5. Stages and phases

One commit per item. Every commit passes every gate that exists at that
point. Each item states the behaviour change, one example of today's
output against the right one, and its oracle count; the fixtures and files
behind it are under the same phase in `CRATE_SPLIT_EVIDENCE.md`.

### Stage A: oracle and gates (structure unchanged)

**A0. Measurement and CI.** The release profile (§2); every §4 baseline
recorded (corpus at all configs and levels, type snapshot, Stage-A
baseline, performance, build time, test-name manifest); a Rust CI running
exactly the §4 gates, initializing only `vendor/phpstan-src`; the snapshot
format in `.corpus-runs/README.md`; `xtask gates` as the runner.

**A1. Total output order** in the CLI and LSP (an approved delta); the
unsorted corpus comparison starts here.

**A2. Fixture capture.** Every test that asserts on diagnostics, wherever
it lives, is dumped at runtime (name, sources, config as toml, full
output) and converted to a fixture, routed by kind (Integration for
empty and cross-rule tests, rule-scoped, directory, Codemod). The
harness-level family filter goes. For one commit the old Rust helpers read
from the fixture harness and every old assertion passes; then the Rust
tests are deleted. The harness backend is the Project API.

**A2 behaviour fixes**, one commit each with fixtures (approved delta
`gates/approved/a2-known-bugs`):

- class names resolve case-insensitively everywhere, `self`/`static`/
  `parent` included (`Self::X` is `class.notFound` today), and `self` in an
  anonymous class is that class;
- casts, `echo` and interpolation give precise types: `(array) true` is
  `array{true}` (`array` today), `(object)` of an array `stdClass`,
  `(array)` of an object its properties' values;
- the `array_udiff()` family's trailing callbacks are accepted;
  `list<int>&non-empty-array` is one type;
- a method called on an intersection uses its most visible declaration,
  independent of member order (`render($foo)` on `ViewB&ViewA` is
  `arguments.count` today);
- a body calling `func_get_args()`/`func_get_arg()`/`func_num_args()` makes
  its function variadic (`arguments.count` today); `new static()` isn't
  checked against the constructor;
- `@param-out` may be wider than the native by-reference type and holds
  after the call; an invalid inline `@var` is not applied; a native-typed
  property keeps its type after a rejected write, and a hooked property
  reads back as its declared type;
- an override without a return type has the builtin's tentative one; doc
  templates are marked before reconciling with the native type;
  `parent::`/`self::`/`static::` bind the caller's templates;
- each duplicate method declaration is checked against its own signature
  and every class-body duplicate is reported;
- `function_exists()`/`defined()` guards hold in a closure, and
  `class_exists(Bar::class)` guards as the string form does (in `if`,
  after an early return, in a ternary, in a closure);
- a child property inherits the hooks it doesn't redeclare;
  `parent::$prop::get()`/`set()` resolve; a parameterless set hook takes
  the property's type;
- the magic methods PHP fixes have those return types (`__isset` `bool`,
  `__sleep` `array<int, string>`, constructors and `__destruct` `void`, …)
  and their returns are checked (`__isset()` returning `42`, `return 1;`
  in `__construct`; 8 rules-oracle `return.type`, 12 `return.void`);
- a failing `array_is_list()` proves a non-empty non-list array; a boolean
  flag set in a branch implies that branch's narrowing over every path
  that sets it;
- a catch is entered only from points that can throw what it catches, and
  is dead without them; an unhandled `match` throws `UnhandledMatchError`;
  a dynamic method call is impure.

Follow-ups:

- **Full tier.** `xtask gates --full` runs at the end of every phase. The
  catch-up run (`25d63876` against `1e5e42be`) changed levels 0–8 as the
  quick tier's approved deltas did; its false positives were fixed with
  fixtures (`ac70c6eb`, `bda6df20`, `40e45bdc`, `520cd439`, `9193f6a9`,
  `48dc04b3`, `06ca4c48`, `2348279e`, `672e927a`) and the type deltas
  approved (`aa08c24f`, `40e45bdc`, `0d105e30`).
- **CI green.** The one recorded CI run (`867d9cb3`) failed at the
  no-default-features tests; every step passes locally, and a passing run
  is recorded.

### Stage B: workspace and leaf layers

Stage B's follow-ups land before C5: C5–C8 build on the AST and type API
they change. Each behaviour fix is its own approved-delta commit with a
fixture.

**B1. Workspace skeleton.** Virtual root and conventions (§2), the package
in `crates/phpstan-rs`, fuzz, bench and proptest regressions repointed, CI
path filters, docs switched to `-p phpstan-rs`.

**B2. Module graph.** A mechanical edge inventory against Appendix A;
helpers moved down (§3.1, §3.2), `Diagnostic*` into its module,
`whole_program/*` and `duplicate_{classes,functions}` under `project`
(removing both `checker ↔ project` edges); every remaining upward edge in
the gate's allow-list with the phase that removes it; the strict gate on.

**B3. syntax redesign**, one commit each: NodeId; the typed AST (hints,
resolved names, split StaticAccess, Cast enum, sub-name spans,
`text_split` deleted); Visitor/VisitorMut with the 18 walkers ported;
parse-time annotations; lexer warnings; stub-attribute rewriting as an AST
pass; the interned `Name` and sharded interner. Then extract `syntax`, with
its fuzz targets and a lex/parse microbench.

Follow-ups, behaviour fixes:

- **Interpolation.** `"${name}"` and `"${expr}"` interpolate (literal text
  today).
- **Integer literals.** Legacy octal `0775` is 509 (775 today); a
  hex/octal/binary literal past `i64` is a float (`0xFFFFFFFFFFFFFFFF` is
  `0` today).
- **Byte strings.** String literals and types hold bytes: `"\xff" ===
  "\u{ff}"` is always true today. A heredoc keeps `\"` as written.
- **Keywords fold case** (`<?PHP`, `DECLARE`, `VAR`, `INSTEADOF`,
  `PRIVATE(SET)`); **a shebang line** is not a statement.
- **PHP 8 precedence.** `+`/`-` bind tighter than `.`, `.` looser than
  `<<`/`>>`; an arrow function's body and `throw`/`include` operands bind
  looser than `and`/`or`/`xor` (`fn (…) => $m xor $c->evaluate()` ends at
  `xor` today, `LogicalXor.php:64`).
- **`new`** takes PHP's `new_variable` (`new $b[0]()` parses as `(new
  $b)[0]()` today) and a constant class name (`new $cls()` after `$cls =
  Foo::class` is `Foo`).
- **Statement grammar.** `(void)` casts the whole expression statement (12
  rules-oracle `cast.void`); `yield $x = 1` is `yield ($x = 1)`; an
  assignment is an operand at every binary operator (`$a ?? $b = 1`; 22
  corpus `assign.invalidExpr`).
- **Kept syntax.** Trait adaptations keep `insteadof` and `as` visibility;
  promoted parameters keep `(set)` visibility; `&get` is by reference;
  `$obj->$$name` keeps both indirections; interpolated parts and heredoc
  `{$…}` get their own spans (LSP references land on the string's start
  today).

Follow-ups, the typed AST (§3.1):

- **Resolved names are read**: 109 re-resolutions in 43 files convert
  (C4 the builders, D3 the checker, the rest with their phases);
  `NameContext::build` (last namespace wins) goes first, and `NameContext`
  becomes private to `syntax`.
- **Declarations carry their namespaced name** before C4 reads the AST.
- **Names**: declaration and member names are `Name`s; named arguments,
  `static`/`global` variables, labels and `use` imports are spanned names;
  the `"class@anonymous"` placeholder is `None`.
- **Language constructs are nodes** (`isset`, `empty`, `unset`, `exit`,
  `eval`, `include`/`require`, `print`, `list()`, `@`, `Declare` with its
  block, shell-exec), not calls to resolved function names.
- **`EnumDecl`** folds into `ClassDecl`; match arms are spanned nodes;
  `NodeId`s on every node rules and side tables key on.
- **One parse entry point**, `syntax::parse(source, &Interner) ->
  ParsedFile` (statements, comments, lexer warnings, annotations), for the
  ten call sites; the separate comment re-lexes (`ignore_comments.rs`,
  `output/codeframe.rs`) go; nesting depth is a `ParseError`.
- **Walkers.** `Visitor` gains `visit_name` and `visit_closure_use`; the
  ~80 self-recursive `child_exprs` walkers in 29 files become `Visitor`
  impls and the allocating helpers go.
- **Annotations**: an arrow function is a catch-scope boundary.
  **Interner**: the global `From<&str>` for `Name` goes once C4 routes
  names through the project's interner.

**B4. types redesign**, one commit each (§3.2): variants; shape keys and
unsealed tail; `Name`-based classes; `Hash`; accessory types;
TrinaryLogic; the ClassHierarchy trait with the unified
`is_super_type_of`/`accepts` (an adapter over Registry until C8); union
algebra and coercions; PhpVersion(s); float formatting;
ExtensionTypeData. Then extract `types`, with its proptest seeds.

Follow-ups:

- **One subtyping implementation.** `Relation::subtype` (56 callers) is
  deleted for `is_super_type_of(..).yes()`; `accepts` is built on
  `is_super_type_of`; the disjointness helpers become `.no()` both ways,
  and `conditional_branch` reads the relation.
- **One union**, following `TypeCombinator::union`: `union_of_branches` and
  `late::join` go through `union_types`; an explicit `mixed` stays explicit
  (`@var array|mixed|null` is a declared type); `never` is absorbed
  (`$c ? 1 : throw …` is `1|never` today, 48 nsrt); integer ranges merge when
  they touch; constant arrays union by `reduceArrays`, keeping tagged unions
  (`$c ? ['t' => 'a', 'v' => 1] : ['t' => 'b', 'v' => 's']` merges to one shape
  today, 163 nsrt); members deduplicate, generic arguments included, and `bool`
  absorbs `true`/`false`; `union_types_keep_shapes_distinct` is deleted.
- **Intersections reduce** by the hierarchy (`Expr&Dim` is `Dim`), two
  shapes intersect into one, a shape with `non-empty-array` stays a
  shape, and `HasOffset` makes an array non-empty (`argument.type` today,
  `bug-12847.php`).
- **Error type.** PHPStan's `ErrorType` for an invalid operation, an
  undefined member or destructuring a non-array; it accepts and is
  accepted by everything, so a reported error doesn't cascade (`-'blabla'`
  is `int` today; 289 nsrt `*ERROR*`, Appendix H's `error-type` 65).
- **Type behaviour lives on the type.** Each question a rule asks of a
  type (offsets, iteration, `is_array`/`is_string`/…, constants,
  `generalize`, coercions, `looseCompare`, `tryRemove`) is one `Type`
  method, trinary where PHPStan's is, or a `Relation` method where it needs
  the hierarchy. Today 302 of the 373 functions taking a `Type` are such
  questions, answered again in each checker file (four `flatten_union`s),
  plus ~1,055 inline inspections. The helpers are deleted, disagreements
  follow PHPStan with fixtures, and `Type` becomes opaque: an xtask gate
  counts `Type::` patterns outside `crates/types` until they reach zero.
  Subtraction, `Relation::remove` and the coercions follow the same shape;
  member lookup (`getMethod`/`getProperty`/`getConstant`) moves once C8's
  reflection exists.
- **Subtracted types** (`mixed~null`, `object~T`, `Foo~Bar`,
  `$this(Foo~Bar)`) as a field of the subtractable variants: narrowing away
  from `mixed`, `object`, a class, `$this` or a template keeps what was
  removed (`isset($m)` leaves plain `mixed` today; Appendix H's
  `subtracted`, 54). `==` is structural and PHPStan's `equals()` is
  `Type::equals`; narrowing is `Relation::intersect`/`Relation::remove`,
  whose rules absorb today's special cases. A `@phpstan-sealed` class is
  its permitted subclasses (`match.unhandled` on an exhaustive match
  today).
- **Conditionals on a template distribute** (`(T is false ? never : T)`
  with `stdClass|false` is `stdClass`).
- **Built-in offset-accessible classes**
  (`ObjectType::EXTRA_OFFSET_CLASSES`: `DOMNamedNodeMap`, `DOMNodeList`,
  the `Dom\` node lists and maps, `PDORow`, `ResourceBundle`, `FFI\CData`,
  `SimpleXMLElement`, `Threaded`) need no `ArrayAccess` for reading and
  existence (80 rules-oracle findings).
- **Removing an enum case** leaves the others (`FE::B|FE` today, a wrong
  value); **truthiness removes whole members** (`array{}|true` survives
  the falsey side today).
- **Template inference** is PHPStan's structural `inferTemplateTypes`,
  unioning every binding: `@param T ...$v` binds only the first argument
  today (`collect('a', 'b')` is `'a'`, a wrong value); an inferred literal
  generalizes unless covariant or scalar-bounded (`new Box(1)` is
  `Box<1>` today); a union parameter binds only what its other members
  don't cover; `mixed` binds `mixed`; inference reads through
  intersections and template arguments. Appendix H's template causes are
  265 nsrt mismatches (`generic-new-infer` 54, `callable-template-infer`
  35, …).
- **A template narrows by intersection**: `$b instanceof A` on `T` is
  `A&T`, its else `T~A`.
- **Late types**: `new<T>`, `template-type<…>`, `int-mask<>`,
  `key-of<C::ARR>`, and `key-of<T>`/`value-of<T>`/`Bob[TKey]` over a
  template stay late and resolve per call (`$y->x('b')` is
  `string|bool|int|float` today where it is `bool`, `bug-13652.php`).
- **Callability and class-strings in `accepts`**: a string or array is
  callable only when it names a callable; a class-string literal must lie
  within its bound; a closure fits a callable type only by signature (32
  corpus findings); `string` accepts a `Stringable` outside
  `strict_types`; `DateTime|DateTimeImmutable` accepts
  `DateTimeInterface`; a union of literals covering a shape's keys accepts
  it; a range is accepted by the constants it spans. Accepted today and
  not: `T` of `mixed` for `int`, `object` for an object shape, an
  uncovered key type, an invariant generic with `mixed`.
- **Conditional parameter types** resolve per call and narrow with their
  parameter; conditional returns and `@param-out` resolve on emptiness,
  templates, `$this` and pre-call argument types (Appendix H's
  `conditional-*`, 81).
- **Class-aware unions** drop covered members (`A|B` for `B extends A`)
  with C8's hierarchy.
- **Display is unambiguous**: quotes and control characters escaped,
  callable returns parenthesized, `0.0` keeps its point (Appendix H's
  `type-print`, 127).
- **Callable parameters** carry names and purity; **`iterable<K, V>`**
  keeps its arguments (`array<V>|Traversable` today, so a `foreach` yields
  `mixed`); `T[]` in a union with an iterable types its values.
- **Typed templates** (a `Name` and a scope enum); **patch versions** in
  `PhpVersion` (`PHP_VERSION_ID >= 80105`'s false branch keeps 8.1).
- **Tests move in**: the `types` unit tests living in `checker/tests`.

**B5. phpdoc redesign.** `DocBlock` stores `TypeNode`s; resolution behind
an interface C7 fills (an adapter until then); extract `phpdoc`, with its
fuzz target.

Follow-ups:

- **One tag splitter**: the grammar cuts lines at `//` (breaking a
  `'http://a'` type) while `docblock::parse` doesn't; one splitter feeds
  `DocBlock` and the rules that re-parse.
- **Number literals** follow phpdoc-parser (`int<0, 0xFF>`, `1_000`, `1e3`
  are `phpDoc.parseError` today).
- **The rest of the type grammar**: nameless by-reference callable
  parameters, offset-access conditional subjects, chained conditionals,
  `$this` as a callable parameter type (each `phpDoc.parseError` today).
- **What phpdoc-parser rejects is rejected** (`@use FooTrait` without
  arguments, `A & B | C`, `\\Foo`; 11 rules-oracle findings), and a
  dropped tag leaves the property untyped (`missingType.property`).
- **A tag's diagnostics sit on its line**, so `@phpstan-ignore-line` inside
  the doc block applies (12 phpstan-rs-only findings).
- **Shape keys** take phpdoc-parser's identifier token (`array{psr-4?:
  …}` fails today; 13 cascading findings).
- **Type parsing is linear** and depth-limited (1,000-deep `array<…>`
  takes 3 s today).

**B6. sql.** Its own column-type model, `[sql_reflection]` parsing,
`SqlContext` as a service; SQL state off `Registry`; the SQL → PHP type
mapping to `ext-sql`; extract `sql`.

Follow-ups: identifiers fold case where the driver does; nullability
follows outer joins and each driver's `NOT NULL`; a failed connection is a
startup warning (silently dropped today).

**B7. diagnostics.** The §3.7 model (`Identifier`, tips; `location` and
`metadata` dropped; `RuleError`, file, families and AST fixes with E3's
dispatcher); the versioned baseline and converter (an approved delta);
extract `diagnostics`.

Follow-ups: `DiagnosticFix::within` (a text search for a missing span) is
deleted; baseline paths are relative to the baseline file, so another
working directory doesn't unbaseline everything.

**B8. composer.** The package context is computed once and extracted; the
default `PhpVersions` range comes from `config.platform.php`, else
`require.php` (an approved delta).

Follow-ups:

- **Constraints follow Composer's grammar** (`">= 8.1"` gives no range
  today, silencing every version-gated rule), and an open upper bound runs
  to 80699 (`^8.5` stops at 80599, making `>= 80600` branches dead).
- **No global panic hook** during vendor discovery (it races analysis).
- **`config.vendor-dir`** and **`psr-0`** are read.
- **The root package's own autoload is context**: a project class that
  isn't named is unknown today (1,253 of the corpus oracle's 1,855
  phpstan-rs-only `class.notFound` name a `Piwigo\` class).
- **A package's bundled `vendor/` is context** (rector and ECS register
  theirs at runtime; 495 `class.notFound` in `vendor/rector/rector`).

**B9. stub-sources.** The generated index, its xtask and regeneration
test; the crate owns the `include!`; extract.

Follow-up for every extracted crate: comments follow the comment rules
(about 130 sites cite package modules, phases or history).

### Stage C: config and reflection

A bug below that sits in code a later Stage C phase rewrites gets a fixture
first and is fixed in that phase, as its own behaviour commit after the
structural one (Appendix F).

**C1. config.** The schema, owner sections, the shim, `--set`, deletion of
the per-rule flags and setters (an approved delta for the CLI surface),
extension activation; the settings split per Appendix B, each type in its
owner module; the `config` crate holds loader and schema; extract.

Follow-ups, one commit each with its test:

- **Nested validation**: `deny_unknown_fields` on every nested struct.
  `[[ignore_errors]]` takes PHPStan's whole shape (`raw_message`,
  `messages`, `identifiers`, `paths`, per-entry `report_unmatched`;
  same-key entries merge counts), compiles once at load through PCRE2 with
  the pattern's delimiters (`regex` without `pcre-validate`, divergences
  documented), and applies `IgnoredRegexValidator`'s checks as load
  errors. **Error paths** name the full dotted key.
- **Missing settings**, each with the phase that implements it: mixin exclude
  classes (`ReflectionSettings`, C8), universal object crates
  (`ReflectionSettings`, C8; replacing the hard-coded `stdClass` in
  `checker/expr.rs`), global type aliases (C7), a `php_version` range and patch
  version (C5), treatPhpDocTypesAsCertain, polluteScopeWith* and
  rememberPossiblyImpureFunctionValues (D5), early-terminating calls (D2), the
  exception model (E3f), additional constructors and always-read/written
  property tags (D6's `AdditionalConstructorsExtension` and dead-code-roots
  implementors), `exclude_paths.analyse`/`analyse_and_scan` (PHPStan's
  `excludePaths`: a flat list means `analyse_and_scan`, an optional entry may be
  missing, another missing literal path fails the load; applied by the Session,
  the LSP's workspace load and incremental adds, replacing
  `DEFAULT_SCAN_EXCLUDED_DIR_NAMES`; Composer-loaded vendor context stays
  loaded), `file_extensions`, `report_ignores_without_comments`,
  `use_path_constants_as_constant_string`,
  `check_internal_class_case_sensitivity`, and the rest of
  `parametersSchema.neon` that changes analysis, checked one by one:
  `polluteScopeWithBlock` and `inferPrivatePropertyTypeFromConstructor` (D5),
  `checkExplicitMixedMissingReturn` (E3f),
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
  `unnecessaryNullCoalesce`, `sortWithoutEffect` and the rest; about 70
  rules-oracle findings); `skipCheckGenericClasses` (`DOMNamedNodeMap`,
  `ParentIterator`, `RecursiveCachingIterator`, `RecursiveFilterIterator`,
  `RecursiveRegexIterator`, `ReflectionObject`) and
  `checkGenericIterableClasses` exempt their classes from `missingType.generics`
  (17 findings). PHPStan's runtime parameters (`tmpDir`, `resultCache*`,
  `bootstrapFiles`, …) are reported as ignored.
- **One settings assembly**: `lsp/workspace_config.rs` uses
  `Session::resolve`'s function (same validation, `stub_paths` meaning,
  relative paths; an error, not an empty config); F1 still deletes
  `ResolvedWorkspaceConfig`.
- **Activation reaches library typing** (PHPUnit narrowing and mocks, test
  base classes, parent-call and assertion defaults), with `Extension` impls
  for mockery, nette-tester, codeception, type-perfect, disallowed-calls
  and sql; `test.risky` and Pest risky tests (`[ext.pest]`) get their own
  keys; strict-rules activation sets the core parameters its `rules.neon`
  sets (polluteScopeWith* off, dynamic properties, explicit-mixed missing
  return, function and internal class name case, maybes in method
  signatures and property PHPDoc types, static method signatures, wrong
  PHPDoc type in `@var`, strict printf placeholders).
- **Paths and matching**: `ignore_errors.path` resolves against the config
  file and matches as PHPStan's `FileExcluder` (a literal file, a
  directory prefix or an fnmatch pattern, shared with `exclude_paths`;
  `glob_matches` goes); `--set` paths resolve against the working directory;
  `psr3_context_key_pattern` matches through PCRE2.
- **Entries validate at load**: a `restricted_symbols` member without
  `class` fails, a user entry beats a preset's; assertion and
  class-argument-return entries parse their types and must name existing
  symbols.
- **Config discovery**: `check-project` without `--config` reads the first
  of PHPStan's four name forms (`.phpstan-rs.toml`, …, `phpstan-rs.dist.toml`)
  as the LSP does, and the LSP watches config files.
- **One walk per file**: the walker dedups canonical paths (a symlink
  loop checks one file 41 times today).
- **Keys**: grouped keys name the upstream parameters they cover
  (`ext.shipmonk.check_null_coercion` the three `forbidNullIn*` rules,
  `ext.shipmonk.check_enum_exhaustiveness`, `rules.check_dead_members`,
  `ext.psr3.check_psr3_logging`); one casing, no key repeating its section
  (`ext.psr3.check_psr3_logging`, `ext.psr3.psr3_context_key_pattern`,
  `ext.sidz.magic_number_exemptions`), PHPStan's name where one exists,
  settled before the shim goes; every discovered composer root feeds PHP
  versions and activation, as `phpstan_api_check_enabled` does.
- **Tests and docs**: `config/src/tests.rs` and `app/tests.rs` cover nested
  keys, unknown `--set` keys and error paths, and
  `loads_every_ignore_error_field_combination` drops its `path`-only entry;
  a config reference is generated from the section docs; `check-project`
  help is corrected (`--php-version` defaults from composer, `--level`
  unset caps at `DEFAULT_REPORT_LEVEL` 6, `--stub-path` and `--format` stop
  citing deleted flags), and `declared_php_versions` gets its doc comment.

**C2. Settings leave `Registry`** (Appendix B): `RuleLevelHelper` to the
future `analyser`, `ConstantResolver` to `reflection`, rule and extension
settings to their owners, `phpstan_api` recording unconditional. Gate:
reflection names no settings type but `ReflectionSettings` (`xtask arch`).

Follow-ups:

- **The gate is implemented** (`xtask/src/arch.rs` has no settings rule).
- **Predefined constants over a version range** (fixed first):
  `PHP_VERSION_ID` is `int<min.id, max.id + 99>`, major/minor are ranges,
  no fallback to the stub's `define()`, and the integer-size constants
  cover 32- and 64-bit builds (`PHP_INT_MAX` is 64-bit only today).
- **`ConstantResolver` in `reflection`**, with `dynamic_constant_names`.
- **Initializers evaluate as `InitializerExprTypeResolver`**, one evaluator
  for constants, enum case values and defaults: `const A = 2 + 3` is
  `mixed` today, so DBAL's `Converter::CASE_LOWER` (a `use const` alias)
  makes its `match` `match.unhandled`.
- **One folded constant key** at every lookup; **`generalize`** keeps
  list-ness and the key type; **`RuleLevelHelper` owns the level rule**
  (the thresholds and benevolent/strict-mixed handling leave
  `types::Relation`); assertion tables reapply on reconfigure until C6.

**C3. Reflection data model** (§3.5): opaque reflections, attributes with
arguments, provenance, no AST bodies.

Follow-ups: C3a's `property.unused` delta fixed first (ORM-mapped
properties count as used, view objects passed to a renderer aren't
reported, every line of `gates/approved/c3-provenance-declarations`
re-reviewed); `is_phpstan_code` a provenance query (an `Origin::Phpstan`
class counts, `phpstan_class_files` deleted); attribute arguments
evaluate to types through the ConstantResolver, `self`/`static` bound
(`#[Table(self::TABLE_NAME)]` is `argument.type` "mixed given" today, and
an undefined or private constant in one unreported); the rest of the data
model lands in C4.

**C4. Snapshots and lazy resolution**, one commit each, in this order:

- **`xtask gatetime`** records the gated steps' medians (N = 3, prebuilt,
  the `gates` profile) per commit and fails `--since <hash>` when a
  median isn't lower; the phase start is recorded first.
- **Keys and data model** (structural, except that names differing only in
  case are one name under the collision policy, an approved delta):
  - classes keyed by the folded `Name::lower` holding the declared `Name`;
    methods fold; properties and constants exact; folded lookups never
    allocate (`FunctionTable` moves onto the same key);
  - every spelling-sensitive lookup goes through an explicit exact
    accessor until the behaviour commits delete them (a type's or literal's
    class name; magic-method comparisons; `final_methods`,
    `abstract_methods`, `has_concrete_implementation`,
    `trait_declares_abstract_method`, `dependency_is_pure`,
    `lookup_ancestor_method_with_owner`); only today's case-insensitive
    fallbacks (`lookup_class`, `lookup_method`, `lookup_method_with_owner`)
    fold;
  - `Vec<TemplateParam>` and per-member flags (a property's finality moves
    off `final_properties`), read by the four template-map rebuilders
    (`own_template_args`, `class_template_params_for`,
    `build_template_params`, the `ReflectionClass` patch); every native type
    an `Option<Type>` (`has_native_type` goes, `explicit_mixed_hint`
    derives), accessors returning `Option<&T>`/`&[T]`; doc comments parsed
    once;
  - provenance the only location record (`is_stub`,
    `function_decl_sites`, `Origin::Unknown` gone);
  - query accessors replace the storage maps (~280 direct accesses
    converted);
  - builders read the AST's resolved names (the anonymous-class builder
    after its fix; PHPDoc names until C7);
  - nothing depends on map iteration order (`classes_using_trait` sorted;
    `doctrine_entity_for_table` by name, an approved delta).
- **Written results leave the signatures**: purity results to a table
  keyed by function and `(class, method)` (`purity_dependencies` and
  `is_transitively_pure` leave `FunctionReflection`); `names_with_assert_tags`
  seeded from the assertion entries and checked by folded name (an
  approved delta).
- **The checker test harness builds like a project**, merging the stubs
  before resolving (a test-expectation delta).
- **Per-class memoized resolution** replacing the passes: eleven per-class
  memos (`flatten_traits`, `resolve_method_tags`,
  `precompute_member_owners`, `resolve_deferred_type_imports`,
  `resolve_deferred_value_of`, `flatten_phpdoc_inheritance`,
  `inherit_property_hooks`, `resolve_deferred_mixins`,
  `precompute_ancestor_names`, `drop_doc_members_excluded_by_native`,
  `resolve_constant_named_types`); `apply_assertion_table` an interim step
  of the own level; `resolve_transitive_purity` and
  `infer_global_variables` whole-program until C9; each name keeps its
  merged declaration (build scratch such as `method_tags`,
  `trait_aliases` and `type_imports` leaves `ClassReflection`, and the ~25
  re-parsing rules read its doc blocks); `finish_loading` keeps only the
  whole-program passes and stays the batch entry point (until F1), and the
  ~100 comments naming a deleted pass are rewritten; `index_and_flatten` is
  deleted, and `Registry::build` runs the whole-program passes until its
  callers move onto `apply` (the stub pass resolving each stub file's
  classes within that file until C5). The memo has five levels, each a per-class
  `OnceLock` reading declarations, lower levels, and its own level only
  along an ancestor edge:
  - **ancestors** (declarations only): `extends`/`implements` in
    resolution order, `use` in composition order, mixin targets,
    unresolved edges, constant declarations with their traits';
  - **own**: own and trait members, `@method` tags, types resolved in the
    class's context, the interim assertion step;
  - **inherited**: PHPDoc inheritance and property hooks;
  - **members**: mixin targets' public members where the own level has
    none (until C8), then member owners;
  - **final**: native-excluded PHPDoc members dropped, and the
    native/PHPDoc combination is `TypehintHelper::decideType` whole
    (native members the PHPDoc doesn't cover come back: `@param string[]`
    on `string|array|A` loses `A` today, 15 corpus
    `instanceof.alwaysFalse` in symfony/string; an inherited PHPDoc naming
    a subclass of the native type applies; aliases resolve before
    combining; an array PHPDoc on an untyped variadic is the element type;
    a `never` PHPDoc return over `void` applies; a built-in parent's stub
    PHPDoc is inherited). Appendix H's C4 causes are the nsrt side
    (`@inheritdoc` forms 12, `iterable<T>&Collection` 19, child templates
    21, `@use` templates 11).

  Class constants type through the ConstantResolver (C2's follow-up
  first); a constant reference cycle types as `mixed` (nine nested
  `array{0: …}` today); enum case values are constants; a function
  resolves lazily the same way. A cycle through
  `extends`/`implements`/`use` is broken on the ancestor level,
  order-independently; a reentrant resolution panics and the cell stays
  retryable (panic-injection and parallel tests). Levels share what they
  don't change; any reflection input change resets every memo; trait and
  mixin members stop outliving their source (the file-removal test
  inverts). `has_unresolvable_ancestor` reads the unresolved edges;
  `is_subclass_of` keeps counting unresolved names until C8.
  Whole-registry iterations decide from declarations and the ancestor
  level until C5's scoped queries (the `demo` command resolves what it
  prints). Approved deltas: nested-trait methods by `use` order, constant
  cycles as `mixed`; purity moving after resolution changes nothing (a
  fixture pins `@return null` on `: void`). Full gate tier; peak RSS at
  or below the phase start; a test pins that checking one file resolves
  only what it reaches.
- **`ReflectionBuilder::apply` snapshots.** `apply` takes `FileDelta`s
  (with the file's role); a name's entry is recomputed from its declaring
  files in canonical order (stubs, context, analysed, by path), so a delta
  gives what a fresh load gives (an approved delta: a late file no longer
  wins a collision by arriving last). Declarations are shared by `Arc`;
  memo cells are fresh per snapshot, seeded with every still-valid value;
  each memo records its read set of folded names, absent ones included,
  and a delta invalidates their transitive readers. Snapshots own their
  names through the project interner. A closure gate compares every query
  after a delta sequence with a fresh build (a property test plus one per
  staleness case). The checker stops rebuilding reflection from the AST;
  hook signatures are built with their property (a set hook's parameter
  takes a narrowing PHPDoc: `array` today); the duplicate-site indexes
  become queries over the per-file sets. `Registry` splits into
  `ReflectionBuilder` and the `ReflectionProvider` snapshot, and its 22
  fields go: `classes`/`functions`/`global_constants` to the declaration
  sets and memos; `ancestor_names` and the five `*_owner_by_class` maps to
  memo levels; `classes_lowercase` gone with folded keys; the
  unconditional-declaration and stub-provision sets to declaration-set facts
  (C5 makes the last two provenance queries); `names_with_assert_tags`
  derived; `function_decl_sites` gone; `doctrine_entity_table_index` a
  provider memo (C8's data extension takes it); `php_versions` and the two
  configured tables builder inputs (C6 compiles the tables); global
  variables and purity per snapshot until C9.
- **Staleness fixes** (approved deltas, each with a closure gate):
  anonymous classes and global constants leave with their file; removing a
  declaration merged onto a built-in keeps the built-in; removing one of
  two same-named declarations keeps the other; the escape sets follow
  their files.
- **Registry tests** (188) and the other `Registry::build` callers move to
  `apply` and the accessors; `Registry::build` is deleted.
- **Behaviour fixes**, one commit each after the structural ones:
  - declarations below the top level are indexed, conditional, through one
    Visitor walk;
  - class names resolve case-insensitively and take the declared spelling
    (`$s->nope()` on a `square`-typed parameter is unreported today);
  - a conditional or duplicate class-like is checked against its own
    hierarchy; duplicate declarations compare case-folded;
  - member names fold case everywhere (a trait's `__toString` makes its
    host `Stringable`, unreported today);
  - member resolution follows PHP (class chain, then interfaces; enums'
    native `cases()`/`from()` implement interfaces, 6 rules-oracle
    findings); a `@property` over a private property is readable outside,
    and inside the class the native property wins over a child's tag;
  - trait composition: `insteadof`, visibility-changing `as`, `final`
    trait methods, abstract methods implemented by another trait, `@use`
    bindings (renamed members keep them), aliases resolved in the trait's
    scope; a method the host declares isn't the trait's there;
  - each `@extends`/`@implements` binds only its parent (with two tags,
    `Coll<int>::get()` is `string` today, a wrong value: every Symfony
    `ServiceLocator<…>::get()`);
  - PHPDoc inheritance carries asserts, `@param-out`, `@phpstan-self-out`,
    `@throws`, purity (an implementation of a `@phpstan-pure` interface
    method is checked for impure points, and a child's `void` method is
    `pureMethod.void`) and method templates; a property redeclared without
    `@var` keeps the parent's (`missingType.property` today) and its
    default is checked against it; inheritance searches every parent
    interface;
  - `@phpstan-type` aliases apply in `@property`/`@method`/assert tags and
    on enums, chained; enums read `@deprecated` cases, `@var` constants and
    `@phpstan-require-*`; promoted properties carry asymmetric visibility
    and their own tags;
  - anonymous classes resolve per namespace block and are named
    `class@anonymous`; `merge` splits per case under one collision policy;
    LSP navigation uses folded keys; a table two entities map resolves to
    none.

Gate: the gated medians are lower than the phase start's; no stub class
resolves unless reached. The keys, memo and snapshot commits run the full
tier.

**C5. Locator chain and stub layer**, one commit each:

- the stub layer's version key over a `PhpVersions` range, decided first
  (Appendix F): 2,234 `LanguageLevelTypeAware` and 598
  `PhpStormStubsElementAvailable` attributes, the functionMap deltas and
  the versioned `.stub`s; a function removed before the analysed range is
  unknown (`mysql_connect()` under 8.5 resolves today; 42 corpus and 9
  rules-oracle `function.notFound`);
- `PS_UNRESERVE_PREFIX_` names are their reserved words
  (`Generator::throw()` is `method.notFound` today), and the prefixed
  language-construct functions leave the index;
- `stub_attributes` fixes (a property's type is `@var`; every entry read;
  per-overload deprecation);
- locator order; the lazy composer locator (an approved delta);
- the version-keyed stub layer holding C4's memo levels, copy-on-merge,
  a per-file parse memo; `rayon` in the index build decided (Appendix E
  follows);
- provenance queries replacing `built_in_stub_classes` and
  `stub_provided_functions`, each consumer naming its question;
- the enumeration APIs, every whole-registry iteration moved onto a
  scoped query (`native_type_hints.rs`'s two scans onto `subclasses_of`;
  `Project::run_reflection_rules` and the LSP's workspace symbols and
  completion onto the project layer or `names(kind, layer)`;
  `repository_entity` onto ext-doctrine's data); the index records parents
  per declaration;
- version-based skip-polyfill and version-dependent signature selection
  (approved deltas); a panic-injection test.

Gate: the gated wall times are lower than the phase start's; the stub layer
replaces the per-project clone and per-process parse; for the unset key the
lazily resolved symbols equal `generated_stub_data()`.

**C6. Overlay layer**, one commit each, every type parsed with the phpdoc
grammar:

- phpstan-src `.stub` overlays (split per file): `conf/config.neon`'s 30
  plus the 10 behind the six gated `StubFilesExtension`s; all function and
  method PHPDoc applies and reaches implementations' untyped parameters
  (`offsetSet($offset, $value)` under `@implements ArrayAccess<string,
  int>` reads `mixed` today);
- declarative patch entries replacing the Rust patch functions;
- the `functionMap.php` layer with its seven version deltas (the two
  `bleedingEdge` files decided), read by a `SignatureMapParser` port replacing
  `patch_functionmap_array_shapes`, merged as
  `Php8SignatureMapProvider::mergeSignatures`; the native signature base decided
  (Appendix F) and `registry/stubs.rs`'s module doc corrected. Its signatures
  replace phpstorm-stubs' PHPDoc where they differ (about 100 rules-oracle
  `argument.type`: `curl_init()`'s benevolent `CurlHandle|false` 39, printf's
  `Stringable` values 32, …; Appendix H's `stub-return` 111 and `stub-asserts`
  39), and its narrower parameters are the ~60 other C6 `argument.type` misses
  (`array_chunk()`/`str_split()` `positive-int` lengths, `max()`/`min()` of
  `non-empty-array`, `json_validate()` depth and flags, `explode()` with
  `''`, `chr(256)`, `error_log()` type `2`, `filter_input()` types,
  `flock()`'s `int-mask`, `unserialize()` options, `fputcsv()` fields,
  `strval()`/`intval()`/`floatval()` of arrays and objects,
  `Randomizer::pickArrayKeys()`, the `RecursiveIteratorIterator` flags, the
  `Reflection*` constructors' class-strings);
- several signatures per function kept, a call taking the variant its
  arguments fit, named ones included (24 rules-oracle `argument.unknown`),
  and fitting none is `arguments.count` (`rand(3)`, 6);
- `functionMetadata.php` embedded for purity (C9 reads it);
- user `stub_paths` with PHPStan `stubFiles` semantics (an approved
  delta); `StubFilesExtension`;
- assertion tables compiled to overlays (`apply_assertion_table` and the
  interim step deleted); the Mockery and Nette Tester defaults move to their
  ext modules, and `gates/module-graph.txt` splits `assertion_tables`.

**C7. TypeNodeResolver** with `TypeNodeResolverExtension`, replacing the B5
adapter and the checker's `resolve_type_node`:

- one name scope (namespace, imports, active templates, aliases, `self`)
  for the registry and the checker: an inline `@var self::COLUMN_*` is
  `mixed` today; PHPDoc keywords resolve before namespace qualification
  (`pure-callable` in a namespace is `N\pure-callable` today; 18 findings
  for `__benevolent`); an enclosing function's templates reach an
  anonymous class;
- native `TypeHint`s resolved directly (~85 print-and-reparse sites);
- an array PHPDoc unioned with an iterable gives its value type
  (`string[]|iterable` is `iterable<string>`);
- a template name is a template as a shape key too (`array{TKey: int}`
  has the literal key today, a wrong value);
- missing generic arguments fill as PHPStan does (`IteratorAggregate<T>`
  is `IteratorAggregate<mixed, T>`; defaults apply);
- a self-referencing alias is `typeAlias.circular`; a constant name in a
  PHPDoc type resolves through `use const` and its initializer;
- `types::parse` deleted (its ~150 test users build types by
  constructor), the ~35 other call sites converted; assert targets
  `$param->prop`, `$param->method()` and `$this->method()` kept; `@throws`
  and assert types get alias substitution; a written `Traversable`
  resolves like any name.

**C8. Reflection extension points** (§3.5), with the built-in implementors;
reflection implements ClassHierarchy (taking `Name`s, lending memoized
views, exposing templates and enum cases).

- The `Relation::structural()` callers (24 package, 3 in `types`) switch to
  the hierarchy-aware relation (an approved delta: `instanceof Child`
  narrows `Base`, `Foo&SubFoo` collapses, …).
- The boolean `is_subclass_of` is deleted; an ancestor behind an
  unresolved edge, or a missing class, relates `Maybe` (an approved delta:
  `argument.type` against a mistyped namespaced `SplObjectStorage` today).
- The `@property`/`@method`/`@mixin` implementors replace C4's copies:
  native members first, a tag at the same or a closer distance winning.
  Behaviour fixes, one commit each: `@mixin` on a trait reaches its hosts;
  a mixin never shadows a native member; a mixin brings inherited members,
  the target's own mixins (recursion-guarded) and a used trait's;
  `@mixin Box<int>` binds templates and `@mixin T` the argument
  (`RecursiveIteratorIterator` forwards to its iterator:
  `getSubPathname()` is `method.notFound` today); a mixin method is static
  with `__callStatic`; `mixin_exclude_classes`; a trait's
  `@method`/`@property` reach hosts; a `@method` tag is no override
  prototype; `@property-read int` with `@property-write string` is one
  property; enums read `@method`/`@mixin`; `@phpstan-require-extends` on an
  interface exposes the base's members and subtyping.
- The class-reflection data extensions take `access_attributes`
  (ext-language-extensions) and `doctrine_entity` (ext-doctrine), their
  module-graph edges re-dated to C8; `access_attributes` and
  `compute_attribute_targets` read `ReflectionAttribute`s; attribute
  arguments evaluate to types.

**C9. Summaries.** Purity and global-variable inference leave reflection for
the summaries API in `AnalysisContext` (created here, completed in D6);
builtin purity from `functionMetadata`; stubs get no global summaries.
ROADMAP mapping (§3.8): summaries H2, H3, U1–U4, U6, P1; enumeration A5,
U5; collector rules (E1) H1, V1, V2, Phase 90; neither A2–A4, A6, C1, P2,
D1.

Fixes, each its own behaviour commit:

- global-variable sites include `foreach`, destructuring, by-reference
  outs and appends; call returns resolve;
- a global's summary holds every value it can have when read: `null`
  (a function can run before the assignment: `f(); $g = 'foo'; f();`
  reads `null`, then `'foo'`), and `mixed` once a write the fold can't see
  may reach it (a dynamic-key `$GLOBALS` write, `extract()`, `parse_str()`,
  a file-scope variable variable, an unresolved include). `global
  $global;` after `$global = "foo"` is `'foo'` today;
- purity: shell-exec and `throw` are impure, constant reads pure, calls
  resolve by parse-time names, method dependencies follow inheritance, a
  static call has its method's purity, a pure callable parameter is pure
  (79 rules-oracle `possiblyImpure.*`), and a user call is judged by its
  body (`pureFunction();` is "unknown function" today).

**C10. Extract `reflection`.** No module-graph edge remains; `cargo machete`
checks Appendix E's rows for it.

### Stage D: engine (in the package, module-gated)

The reference is PHPStan 2.2.16's engine (`NodeScopeResolver`,
`MutatingScope`, `StmtHandler/*`, `ExprHandler/*`, `TypeSpecifier`).
Structure lands first and semantics last, so every behaviour commit is
written once, on the final structure: D1–D4 are structural (each structural
commit byte-identical, its behaviour commits following as approved
deltas), D5 is the semantics phase. Every behaviour bug is reproduced and
fixtured before its phase's structural commit.

Gate 12 runs both oracles. **The corpus oracle** is the PHPStan 2.2.16 phar
over the corpus's engine-sensitive files, outside `vendor/` (so the corpus's
extensions don't load), with Piwigo's autoloader as the only bootstrap,
level 10, and `phpVersion` pinned to the composer range phpstan-rs reads
(`80500–80599`); both sides are filtered to engine identifiers, files with
internal errors excluded (941 left), trait context suffixes stripped. At
`76e738b4` phpstan-rs has 1,620 findings PHPStan doesn't (1,056
`offsetAccess.nonOffsetAccessible`) and misses 2,039 (741
`offsetAccess.nonOffsetAccessible`, 284 `foreach.nonIterable`, 263
`offsetAccess.notFound`, 244 `function.alreadyNarrowedType`, …). Counts
citing snapshot 0a8fe69 come from the earlier unpinned run. Each phase
reruns it, and every phpstan-rs-only finding it names is a fixture.

**The nsrt oracle** is PHPStan's inference suite (1,688 files, 18,480
`assertType`, 313 `assertNativeType`, 681 `assertVariableCertainty`),
checked by `check-project --php-version 8.5` with `tests/PHPStan` scanned
and phpstan-src's dependencies installed; a file whose `// lint` excludes
8.5 runs at the highest version it allows. Mismatches compare after a
normalization of equivalent display forms (union and shape key order,
benevolent parentheses, `array<(int|string), T>`, a list shape's explicit
keys, `true|false`, template scope suffixes, accessory-string names, small
ranges against literals, `*NEVER*`, string backslashes). The files asserting
an unpinned version range are approved deviations. phpstan-rs misses 11,106
at `76e738b4` (4,788 variables, 3,399 extension-typed calls, 1,016
operators, 707 method calls, …). Lines where a literal is missing, or
phpstan-rs has literals PHPStan generalizes, are flagged as possible wrong
values: each is fixed first in its phase or approved as right at runtime,
and the count is tracked per phase. The assertion DSL reads
`assertNativeType` and `assertVariableCertainty` before D1, so D5's natives
and certainty are measured by the suite. `cargo xtask nsrt` and `cargo xtask
oracle` land with it; their first counts are D1's baseline.

The first nsrt run checks in `gates/nsrt/owners.toml`, naming the owning
phase for each of the 1,360 mismatching files, split by line where a file
spans phases; Appendices G–J (`CRATE_SPLIT_CAUSES.md`) are its draft. A
phase is done when its files have no mismatches, or each remaining line is
an approved deviation naming its runtime reason. `cargo xtask nsrt` reports
by owner, wrong values first, and the corpus oracle's phpstan-rs-only
identifiers get owners the same way.

**D1. Holder storage (byte-identical).** Every `Locals` map moves into
holder storage with its current merge and invalidation policy; ExprKey is
interned and the maps persistent; benchmarked on the merge/clone path
(`merge_scopes`'s measure attribute moves onto it first).

**D2. Statement results, contexts and StmtHandlers.**

- The annotation kinds the handlers need land in `syntax::annotations`
  first, following 2.2.16's parser visitors (goto labels, last condition,
  try-catch types, parent statement types, declare position, version-dead
  code, the closure-argument contexts D3's callback overrides read, the
  pipe transformer, magic-constant defaults, immediately invoked closures,
  `new` assigned to a property, standalone throw); the `Checker` fields
  they replace go.
- `StatementResult`/`ExpressionResult` carry exit, throw and impure points,
  `hasYield`, end statements and `isAlwaysTerminating`.
- One `StmtHandler` per statement kind splits `stmt.rs`;
  `ExpressionContext`/`StatementContext` replace the take-idiom flags; a
  statement's `@throws` overrides its throw points; an always-false branch
  isn't analysed (`foreach.emptyArray` inside `if (!empty(X))` for `X = []`
  today).
- The statement after a terminating one gets the unreachable node
  (functions, classes and labels still processed; `;` and closing HTML not
  reported); the top level and the statement after `goto` too; an
  expression terminates when any evaluated part does (`echo nr();`, `f(nr())`;
  ~20 of 54 rules-oracle `deadCode.unreachable`); an always-terminating
  loop body skips its condition. Each end statement gets its own
  `ExecutionEndNode`; per-function mutable fields go.
- Behaviour commits, each with its fixture:
  - **Termination from exit points** (fixed first): a loop with a `true`
    condition and no `break` targeting it terminates; a switch without
    `default` whose cases leave `never`; a `finally` that returns; a `new`
    whose constructor is `never`. Today these are `return.missing`
    (Appendix I's `switch-exhaustive` 13; 40 of snapshot 0a8fe69's 68
    corpus `return.missing`, e.g. `while (\true)` in php-cs-fixer's
    `Tokens.php`). An interim fix before D2 teaches `always_terminates`
    `DoWhile`, inferred conditions, break levels and covered enum switches.
  - **Last conditions** follow `LastConditionVisitor` (an `elseif` whose
    `else` only throws, an `if` followed by its block's final `throw`, a
    `match`'s last arm): 30 corpus `alreadyNarrowedType`/`alwaysTrue` of the
    `if … throw` shape.
  - **Constant conditions prune branches** in `if`, `elseif`, ternaries and
    `&&`/`||`, by the PHPDoc type (native without
    `treatPhpDocTypesAsCertain`): after `$flag = true; if ($flag) { $x = 1;
    }`, `echo $x` is `variable.undefined` today; the code after `if (true) {
    return; }` or a break-less `do … while (true)` is unreachable (~25
    rules-oracle `deadCode.unreachable`).
  - **A nameless `@var`** above `throw` or `foreach` types the thrown
    expression or the iteratee (`throw.notThrowable` today).

**D3. One inference path.** ExprHandlers, per-`NodeId` results, `infer_expr`
deleted; the file-size gate.

- An ExprHandler has processing, type resolution and `specifyTypes`; the
  TypeSpecifier is built here as their dispatcher (D6 makes it public).
  `SpecifiedTypes` carries sure/sure-not/alternative types, overwrite and
  equality, the root expression, new conditional holders, and the
  `&&`/`||` holder recipes and augments, which replace
  `branch_implied_facts` and `pending_or_disjunctions` (Appendix H's
  `conditional-holders`, 57: a bool holding a check narrows its source when
  tested; a holder never applies outside its condition, `bug-14891.php`).
- Assignment is two-phase: the target's sub-expressions in PHP's order,
  then the value, then the write.
- isset/`empty`/`??` resolve through one issetability descriptor shared
  with the isset rules: `empty($a['nope'])` on a constant array is `true`
  (`bool` today); `isset($this->p)` on an initialized non-nullable property
  is `true` (one that may be uninitialized isn't); the `nullsafe.neverNull`,
  `isset.*`, `empty.*` and `nullCoalesce.*` families (`variable`,
  `property`, `expr`, `initializedProperty`, `offset`) are reported for
  every left side, unreported today (~50 rules-oracle findings; an untyped
  property without a default starting as `null` stays unreported,
  approved); a falsey `isset($v)` narrows to `null`; `isset($m->foo)` on
  `mixed` gives `object&hasProperty(foo)`, on `self|false` leaves `self`;
  `isset($t['foo'])` drops a `string` member.
- AST names read their parse-time resolution; `name_ctx` is deleted.
- **Disagreements.** Before the merge, a commit records every node where
  `check_expr` and `infer_expr` disagree over the fixtures and the corpus;
  the inventory is checked in and the merge lands as one approved delta per
  class. Known already: `infer_expr` lacks `instanceof`, compound
  assignment, dynamic names and `list()`, and gives `mixed` for a short
  ternary (Appendix I's `infer-expr-gaps`).
- Behaviour commits, soundness first (a wrong type is worse than an
  imprecise one), then by handler family:
  - **Soundness**, each a wrong value:
    - `is_resource()` on a `resource` is never always true (52 corpus
      `alreadyNarrowedType` today; fixed first);
    - an assertion intersects with the current type and its negation
      narrows the other branch (`@phpstan-assert-if-true int $v` on
      `string` is `int` today, not `*NEVER*`); variadic and equality
      assertions apply; Appendix H's `assert-tags` (58) are the call forms
      that don't narrow;
    - unary minus on a literal union is ignored (`-$c` for `1|2` is `1|2`);
      a float result typed as an integer (`(float) 5` is `5`); `===`
      equating `0.0` with `0`;
    - loose comparison uses the wrong table (`'0' == null` is reported
      always true; PHP 8 gives `false`), and follows the analysed version;
      beyond two constants `==` is each type's `looseCompare` (25
      rules-oracle `equal.always*`);
    - a string offset write leaves the string unchanged;
    - `static::`/`$this::` of a non-final constant is its declared type,
      not the parent's value (`static::FOO_BAR` gives the parent's `'foo'`
      today), while `self::T` reads the value;
    - narrowing to an excluded value gives the value instead of `never`
      (`$i === 2` on `int<3, 5>` is `2`), and an always-false guard's
      branch types its subject as the guard's type;
    - a nested write through a non-constant key is dropped
      (`$array[$name][] = $v` leaves `$array` unchanged);
    - a property write narrows only where `canChangeTypeAfterAssignment`
      holds (`$xml->foo = 'foo'` reads back `'foo'` on a
      `SimpleXMLElement` today); a `new` into a generic property takes its
      declared arguments;
    - a by-reference argument to a closure or variable function keeps its
      value (`$save === false` reported always false after `$cb($save)`),
      and one is checked with its value before the call: the pre-call
      widening to `mixed` makes `r($e)` for `string &$x` "mixed given"
      (Appendix I's `byref-out-undefined` 60 and `byref-arg-type`);
    - a by-reference variadic argument is read with its type;
    - `||` narrowing on an unnormalized union loses members (a rector
      `instanceof.alwaysFalse`);
    - the falsey side of `A && B` narrows by `!A || (A && !B)`;
    - `is_a($cls, A::class, true)` on a string gives the object `A`;
    - `%` giving `never` and a pre-increment's old value (below).
  - **Operators and literals:**
    - **Arithmetic** follows `InitializerExprTypeResolver` (fixed first):
      `int|float * 2` is `int` today; overflow is a float; a float literal
      is its value; `%` follows PHP's sign rule; shifts, `**` and bitwise
      string operations as PHP (Appendix H's `operator-ranges`, 87); `%` by
      a constant is bounded; bitwise operators on ranges stay in range;
      `<=>` is `-1|0|1`; `+` with `mixed` is `array|float|int`.
    - **Concatenation** covers every member of a union (`($a ?? 'x') .
      '-'` on `?int` is `'x-'` today, a wrong value).
    - **Increments** follow PHP (`'a'++` leaves `'a'` today, a
      pre-increment's value is the new one), and array-literal keys are
      evaluated in order.
    - **`NAN`** is never identical to itself.
    - **Identical** follows `resolveIdenticalType`: non-intersecting
      operands make `===` false (`1|2 === 7` unreported today; most of the
      80 rules-oracle `identical.alwaysFalse` and 26 `notIdentical`); the
      truthy branch intersects.
    - **Constant comparisons fold**, `null`/`bool` included (96 nsrt);
      **ranges narrow at their ends**, and a comparison narrows each
      union member (`$foo > 1` on `?int` is `int<2, max>`).
    - **Short ternary**, **compound assignments** and **pipes** have their
      handlers' types (each `mixed` today); a pipe is a call, checked for
      argument type, arity and `#[\NoDiscard]` (31 rules-oracle findings).
    - **Magic constants** are their values (`mixed` today, so
      `dirname(__DIR__)` is "mixed given" at level 10).
  - **Arrays and offsets:**
    - **Offset reads** follow `ArrayDimFetchHandler`: a non-literal key on
      a constant array reads by the key's type (`$t[$j]` is `mixed` today;
      Appendix I's `const-offset-nonliteral` 18); string offsets are
      typed; an `ArrayAccess` reads `offsetGet` with its template
      arguments (37 level-10 findings in one bacon-qr-code file); a union
      container reads each member.
    - **Literals and appends** follow the next-key rule (after a negative
      key per PHP version; none after `PHP_INT_MAX`), appending keeps the
      shape, and a non-constant key gives a general array.
    - **`$o::class`** and `get_class()` are literals for final classes and
      `class-string<C>&literal-string` otherwise (`match.unhandled` over
      class names today).
    - **Key-producing assignments** prove the key (`$a[$k]` after `$k =
      array_key_first($a)` exists).
    - **Array keys are cast** on every write, read and `unset()` (`$a[true]
      = 'x'` then `$a['1']` gives `array<(int|string)|string, …>` today);
      `array.duplicateKey` sees cast keys, spreads and colliding variable
      keys (13 rules-oracle findings).
  - **Calls, closures and objects:**
    - `$this`/`static` carry their bound class (`$this(Base)`), `@return
      $this` rejects `new static()` (17 rules-oracle `return.type`), and a
      generic class's `$this` carries its templates (27 "expected T"
      findings).
    - A class-string expression as a static receiver resolves; `static` in
      a return type is the whole receiver; a template-typed receiver reads
      through its bound; a union receiver combines its members' methods
      (`combineAcceptors`).
    - **Type-check calls** type as the constant the impossible-check logic
      decides (`is_string($s)` on a string is `true`; `instanceof` is
      `mixed` today), asserting calls included: 105 corpus
      `booleanAnd.alwaysFalse` and 57 `deadCode.unreachable` depend on it.
    - **Enums**: `cases()` is the shape of the cases, `->name`/`->value`
      their unions.
    - **First-class callables** carry the callee's signature, conditional
      returns resolving per call.
    - **Arguments** follow `processArgs`: each on the scope the previous
      left; a closure's parameters resolve from sibling arguments whatever
      the order; the intrinsic callback overrides (`array_map`,
      `array_filter` by mode, `array_walk` by reference, `preg_replace_callback`,
      …) rewrite callback parameters (Appendix H's `closure-param-infer`,
      77; `array_map(fn ($x) => $x * 2, [1, 2])` is `array<int>` today);
      `self`/`static` in a callable type bind to the called class.
    - An inherited constructor takes the child's bindings; a template's
      bound is checked at the call; `clone($o, $with)` is checked as a
      function; a constant dynamic method name resolves;
      `call_user_func*()` check and count arguments (18 findings); every
      receiver form resolves the callee, and a spread of a shape union
      passes by position.
    - A variadic parameter is `list<T>` under `@no-named-arguments`,
      `array<int|string, T>` otherwise; unpacked constant arrays bind to
      the parameters they reach.
    - **By-reference out types of built-ins** apply (after `exec($c,
      $out)`, `$out` is `mixed` today; 41 findings in `composer/semver`),
      a by-reference parameter's own type is its out type, and the array
      functions that modify their argument give it the new array
      (Appendix H's `byref-array-fns`, 210).
    - **Closure types** follow `ClosureTypeResolver` (a yielding closure
      returns a `Generator`; a body that never returns is `never`); a
      closure's `string $t = null` is `?string`, a promoted one never
      implicitly nullable.
    - A call's value has `void` as `null`; `K::{'X'}` is `K::X`; `new $c()`
      for `class-string<T>` is `T` (Appendix I's `new-class-string`, 13);
      `new C()` is exactly `C` (most of 109 rules-oracle
      `method.resultUnused`).
    - **A `static` variable** is its initializer unioned with every value
      the body assigns, `mixed` once it escapes by reference.
    - **Function scope effects** follow `FuncCallScopeEffectsHelper`
      (`array_pop` keeps a shape, `$http_response_header` defined,
      `json_last_error()` dropped).
  - **Narrowing gaps** in `specifyTypes`: `$e === E::A` subtracts the case;
    `!empty($v)` on `int`; a condition on an increment; `instanceof
    $classString`; `gettype()` comparisons; loose `==` with a constant
    (Appendix H's `loose-eq-narrow`, 78); a list offset implies the lower
    ones; a cast narrows its operand; `!is_scalar` on `string`; boolean
    composition through the recipes; nullsafe chains; `if ($this->prop)` and
    `match (get_class($obj))`; a template keeps `null`; conditional return
    types and conditional assertions narrow arguments; and
    `EqualityTypeSpecifyingHelper`'s comparisons: `count()` (Appendix H's
    `count-narrow`, 217), `strlen()` (`strlen-narrow`, 59),
    `array_key_first() !== null`, `preg_match() === 1`, `get_class() ===`,
    string functions against non-empty literals (`string-fn-narrow`, 52).
  - **Deep expressions** don't overflow the stack: a 32,000-term `.` chain
    aborts `check-project` today, and 8,000 terms take 58 s.

**D4. NodeCallback sink, observers and result storage** (approved deltas
for `return.unusedType`, `throws.*` and `property.unusedType`).

- The RefCell recording channels become observer events (internal until
  D6); `by_ref_empty_dim_args` becomes the by-ref argument context.
- `ExpressionResultStorage` keeps each node's before-scope and answers rule
  type queries; callbacks fire post-order with it, a call emitting its
  call-expression node.
- Non-final passes run with a Noop sink; gatherer frames collect returns,
  yields, execution ends and impure points outside the rule sink; a
  converged replayable pass is replayed instead of walked again.
- The five `diagnostics.truncate` sites are deleted.

**D5. Convergence onto PHPStan semantics**, one approved-delta commit each,
with a fixture per transition and trigger. Where PHPStan's rule is less
correct at runtime, runtime correctness wins and the deviation is named:
eval defining variables (§3.8), assignments inside `assert()`, and a
by-reference `use`d variable inside a closure body. In order:

- **Holder model**: natives as full holders plus `native_type_of`; the
  virtual keys; convergence equality (context, holders with certainty,
  natives, conditional holders); provenance as metadata; non-holder state;
  guards (`method_exists()` and friends producing B4's `HasMethod`/
  `HasProperty`/`HasOffset` members instead of side sets);
  `container_field_facts` folded.
- **Certainty** plus maybe-undefined, level-gated, in the type snapshot:
  - a maybe-defined variable is "might not be defined" at level 1 (`undefined`
    at level 0 today, and its reads `mixed`); a top-level unknown variable
    is Maybe and a `@var` names it (75 corpus `<main>` findings, 8
    rules-oracle `varTag.variableNotFound`); `$argc`/`$argv` are typed;
  - a merge keeps Maybe only for variables, calls and virtual keys;
  - a by-reference argument defines its variable only where the parameter
    creates it (`reset($u)` reads undefined);
  - every read counts: constant variable-variables, `compact()` names,
    destructuring keys, a closure bound to no object, a correlated
    `unset()`;
  - `isset()`/`empty()` don't define; a truthy `??` proves its left side;
  - an assignment inside `assert()` is Maybe (compiled out under
    `zend.assertions = -1`; PHPStan treats it as definite).
- **Conditional holders**: `ScopeOps::createConditionalExpressions` with
  `mergeWith`, replacing `discriminant_array_facts` (a variable defined under
  one condition and read under a correlated one is `variable.undefined`
  today, symfony's `Filesystem.php:190`); holders from `&&`/`||` conditions
  (`bug-9946.php`); holders at assignment replacing `condition_aliases`
  (sure types by truthiness, one-armed ternary values, `$v = match (…)`
  narrowing its subject, derived assignments, `in_array`/`preg_match`/`===
  []` results), projected only through safe expressions.
- **Invalidation**:
  - receiver rebind, by-ref and declare purges unify: an impure method call
    drops its receiver's holders, a `self`/`static`/`parent` call `$this`'s
    (ten rector `if.alwaysFalse` in the corpus; a pre-D5 fix drops `$this->`
    and `self::$` entries first), and the comparison rules' property guards
    that compensate go;
  - an immediately called closure's property writes invalidate the outer
    scope, and so does a closure a callee may run later
    (`foreach.emptyArray` today);
  - a typed-property write stores the coerced value (`'123'` into `int` is
    `123`);
  - offset writes: an existing offset keeps a list; a non-constant key or
    append makes the array non-empty (Appendix H's `list-write`, 114); a
    literal-union key builds optional keys (10); a property's array updates
    like a variable's; a template-typed array intersects; only members that
    can take the write stay; narrowing an offset narrows its container (368
    level-10 findings in the bundled `php_codesniffer` alone); a nested
    write keeps the element's keys; `unset()` removes the key, a non-constant
    one making a list an array;
  - volatile expressions drop after an impure call, `new`, `eval` or
    `include`; `clearstatcache()` drops the stat family; a declaration drops
    its negative existence check; the output-buffer level delta;
  - references (`IntertwinedVariableByReferenceWithExpr`): `$b = &$a; $b =
    'x'` leaves `$a` as `1` today, array-literal references included;
  - `afterExtractCall` replaces extract/eval truncation (an `eval()` drops
    every finding of its body, or the whole file at top level, today; 18
    rules-oracle findings lost); `extract()` of a constant array assigns
    its keys;
  - rememberPossiblyImpureFunctionValues with PHPStan's `true` default:
    call results aren't remembered today (17 nsrt certainty assertions, 10
    rules-oracle `identical.alwaysFalse`; Appendix H's `guards` 54,
    `method-call-memory` 29, `impure-invalidate` 42, `readonly-remember`
    25, `ob-level` 47).
- **Control-flow transitions**:
  - **Loops** run `LOOP_SCOPE_ITERATIONS = 3` with `generalizeWith`, then
    the final walk: `$i++` in a `while` gives `0|…|6` today where PHPStan
    has `int<0, max>` (Appendix H's `loop-generalize`, 208); the 14
    `loop_depth` guards go; a narrowing every iteration makes holds after
    the loop.
  - **For**: a first condition that is `true` runs the body (`$x` after it
    is `variable.undefined` today), `false` makes it unreachable; the exit
    scope applies the falsey narrowing; `$a[$i]` is known under `$i <
    count($a)`; `polluteScopeWithLoopInitialAssignments`.
  - **Foreach**: an unsealed tail is iterated (a wrong value today); the
    body sees the iteratee non-empty and a non-empty iteratee defines the
    body's variables after it (11 rules-oracle findings); `$arr[$k]` is the
    value type; constant arrays of up to 16 keys unroll; object iteratees
    read their `Traversable` arguments or `key()`/`current()`; a constant
    array's key is the union of its keys; by-reference values write the
    element; the post-loop scope includes never entering (a wrong value
    today); `polluteScopeWithAlwaysIterableForeach`.
  - **Goto**: forward gotos merge at the label, backward ones run a
    fixpoint.
  - **Short-circuit scopes**: an assignment on the right of `&&`/`||`/`??=`
    is Maybe; an increment there counts (`$foo && $j++` leaves `0` today, a
    wrong value); a `never` side keeps the other's scope; a nullsafe call's
    arguments run only on a non-null receiver.
  - **Switch**: each case entered with `$subject == $case`; later cases see
    terminated ones excluded (`switch (true)` by falsey narrowing);
    `default` excludes handled cases, and an exhausting switch has no
    fall-through exit (a rector `variable.undefined`); `continue` acts as
    `break`; a `break` nested in a case's `if` leaves with that scope.
  - **Match**: the subject is remembered, arm scopes merge, a
    non-exhaustive match throws `UnhandledMatchError`; `match (true)` is
    exhaustive when the falsey narrowings leave nothing (Appendix I's
    `match-exhaustive` 12; nine corpus findings); repeated and out-of-type
    arms are `match.alwaysFalse`, a non-last always-true arm
    `match.alwaysTrue`; `default` sees every arm excluded; an arm is entered
    from its condition's truthy scope; array subjects, small ranges and
    template bounds are exhausted by their values; a caught
    `UnhandledMatchError` isn't reported.
  - **Try/catch/finally**: a catch is entered from its throw points; its
    variable is the catch type minus earlier catches, a non-`Throwable`
    interface intersected with `Throwable`; `finally` merges every exit
    (`try { $bar = 1; } finally { echo $bar; }` is `variable.undefined`
    today; ~40 nsrt certainty assertions; a pre-D5 fix makes it level 1);
    exits leave through `finally`.
  - **Expression statements** apply `specifyTypes` in the null context.
- **Class and function context**:
  - **Member order** follows `ClassLikeHandler`, and **the constructor
    first** seeds the other methods with readonly properties, initialized
    properties, existence guards and constants (a constructor's
    `function_exists('f')` doesn't reach other methods today).
  - **Closures**: `->call()`, `bind()` and `bindTo()` bind `$this` and scope
    and pass arguments (Appendix I's `closure-bind-scope` 11); an
    immediately invoked closure runs definitely; by-reference `use` follows
    `processClosureScope` outside the body (`byref-closure-use`, 27), and
    inside it is the union of every value the enclosing function or its
    closures assign (the deviation: PHPStan reports react's shared `$timer`
    `if.alwaysFalse`, 8 corpus findings); a closure with by-reference uses
    converges like a loop, any other is entered once from its declared
    types (six re-entries today give `Node|Node|…&Return_`); a static
    closure has no `$this`; holders on `use`d variables carry in, and an
    arrow function's parameter shadows the outer holders; `$f = function ()
    use (&$f)` types `$f`; a closure carries its throw points, impure points
    and invalidations to immediate invocations.
  - **PHP version** narrows in a version branch: inside `if
    (PHP_VERSION_ID >= 80400)` the scope's range is cut at 80400.

**D6. Engine APIs**, one commit each: the Scope API (§3.8); the public
TypeSpecifier with its extension hooks; the returned-error `Rule` and
dispatcher (rules return `RuleError`s, the dispatcher attaches file and
trait file, each rule declares its identifier families and every error is
debug-asserted against them, the ~40 `format!` identifiers become family
members, and an ignore entry whose identifiers lie outside every family is
never reported unmatched); the rest of the derived-node catalogue with the
site mapping (543 report sites); Collector; Observer; the extension-trait
registry; the concurrency/state model; `AnalysisContext` completed;
`InternalRule` deleted; the external `Rule`/`ReflectionRule` users ported.

### Stage E: everything out of the engine (in the package, module-gated)

Each rule moves once, onto its final API: D6's returned-error `Rule`,
`RuleErrorBuilder` and identifier families, and E3's prologue
(`RuleLevelHelper::find_type_to_check` and the rule → level table), which
lands before any rule moves. The level-dependent checks are then one
mechanism: about 2,950 of the corpus oracle's 9,623 PHPStan-only findings
are a rule not treating an implicit `mixed` operand as reportable (E3b).
The corpus's phpstan-rs-only findings are mostly its own strict-rules-style
extensions, which the oracle doesn't load, and B8's context gaps (1,748 of
1,855 phpstan-rs-only `class.notFound`), so E's comparisons are read after
B8's two context fixes.

Gate 12's **rules oracle** is Stage E's measure: over PHPStan's rule-test data
at `76e738b4`, PHPStan reports 15,476 findings across 633 identifiers (trait
context copies collapsed) and phpstan-rs 11,428; 8,840 match, 6,636 are
PHPStan-only, and 152 identifiers never match. Findings match by file and
identifier on the same line first, then within three lines, and within one
declaration for the signature-level identifiers. Every PHPStan-only finding
of a matched identifier was traced to a plan item. An E3 sub-phase ends when
its identifiers have no PHPStan-only findings, or each remaining one is an
approved deviation: an identifier named differently
(`class.abstractMethodNotImplemented` for `method.abstract`,
`class.extendsEnum`, `generics.wrongParent`, `requireExtends.nonObject`,
`intersectionType.int`, `enum.duplicateConstant`, `property.private` where a
`@property-write` shadows a private property, a trait's `@use` errors on the
trait's line), or a PHPStan-specific check (`phpstan.*` asserts, E3h;
`phpParser.*`; `phpstanApi.*` outside an extension package;
`function.inner`; `class.prefixed` for `_PHPStan_`, the other tool prefixes
shipping opt-in in ext-extras). `void.pure` is a real dead-code check and
ships opt-in in ext-extras (the oracle's 81 are test scaffolding). A call
phpstan-rs reports `possiblyImpure.*` where PHPStan knows it is `impure.*`
(23 findings) is a wrong verdict that C5, C9 and E3(e) fix, not a rename.

The oracle's other side is the false-positive measure: phpstan-rs's 2,588
findings PHPStan doesn't report were each traced to a cause (the items cite
their files; Appendix I lists the 1,019 in 410 uncited files and the 444
their citing item doesn't explain), and the phase fixing a cause reruns the
oracle. What stays is approved: stricter checks right at runtime
(`array_sum()` of arrays, printf placeholders casting objects, `new self()`
statements without effects, `static::$p;`, a never-written `private(set)`
property, `5 / 0` ending reachability, a shadowed `catch (\Throwable)`, …),
the `featureToggles` C1 turns on, 246 `phpstanRs.assertType` mismatches the D
and E2 items name, and 145 findings in 13 files using php-parser and PHPStan
classes, excluded and listed by the gate.

**E1. Collectors and project-level rules**, one commit each:

- the **reference-site collector** (built-in; consumed by the dead-code,
  architecture and circular-namespace rules and later the LSP);
- **dead code / members / enum cases / abstract classes** with every use the
  collector sees: a private constant read in an enum case value, an
  attribute argument, `$this::C`, a class-string or a used trait (16
  rules-oracle `classConstant.unused`); a property read through `?->`,
  `get_object_vars($this)`, its own hook, a host's trait or a `clone()`
  write (30 `property.unused`/`onlyWritten`); a private method called in
  another case, through `?->`, as a callable, from a trait or a
  class-string (11 `method.unused`). A private hooked property whose `get`
  no one calls is `property.neverRead`, a get-only one isn't
  `property.onlyRead`; a method used only by itself or by unused ones is
  unused, and `$x->$y()` counts only for the names `$y` can be;
- **duplicate declarations across the project** (`function.duplicate`,
  `class.duplicate` and the other kinds) as a collector over C4's per-file
  sets, every site listed with paths relative to the project root (absolute
  today). Unconditional project declarations count; a stub file never does,
  replacing the signature-difference grouping (a workaround for
  phpstorm-stubs' two `strtr()` that also hides real duplicates PHP
  rejects). Case-folded; both stay opt-in (two test suites' helpers never
  loaded together look like duplicates);
- **`namespace.circularDependency`** over the collector's class
  dependencies (only `extends`/`implements`/`use` count today), naming the
  class and dependency behind each edge, analysed namespaces only;
- **`[[rules.architecture]]`** as a collector rule: a class's dependencies
  are the collector's references in it plus every class its resolved PHPDoc
  names (inline `@var`, `@throws`, `@extends` arguments, bounds, `@method`,
  `@property`, `@mixin`); today `check_architecture` rewalks each file and
  reads PHPDoc only through signatures, so `/** @var Infra\Repo $r */` in a
  domain method is no dependency. It runs in the LSP too;
- **`[rules.banned]`** as per-node rules (`banned.<construct>`,
  `banned.type`) in the rules walk, not a second CLI-only walk; `compact()`
  and `extract()` matched by resolved name (a namespace's own `compact()`
  is banned today);
- **class shapes** (`check_closed_classes`: `class.notFinal` and the
  protected-in-final and not-final-in-abstract checks; `[[parent_calls]]`:
  `parentCall.*`) as a collector rule, in the LSP too. A class stays open
  where something subclasses it at runtime, as extensions report through a
  hook: a project subclass, a Doctrine proxy (ext-doctrine's metadata, not
  today's match on `@Entity` text or any attribute named `Entity`), a test
  double (PHPUnit can't double a final class; `createMock(Foo::class)`'s
  `Foo` is `class.notFinal` today). Required parent calls register from
  their extensions, not core defaults;
- **composer hygiene** (`*.composerDependency`,
  `misplaced.autoloadDevSymbol`, `psr4.*`, `provide.unimplemented`,
  `ext.notRequired`, `composerScript.*`,
  `composer.platformPhpInconsistentWithRequire`) as a collector rule over
  the reference sites and B8's package context, without I/O, in the LSP
  too. A package's global constants count as its symbols (a package used
  only through them is `unused.composerDependency` today);
- **`property.unusedType`** as a ClassPropertiesNode rule over host-class
  assigns, trait bodies per host (`facts_only` and every `*_facts` field
  deleted): an untyped property starts as `null` (46 corpus and 11
  rules-oracle phpstan-rs-only findings, e.g. xdebug-handler's `$debug`); a
  by-reference `&get` isn't too wide; a default and an assigned range count
  as assignments; a property no code assigns is checked from its default
  (all 42 rules-oracle PHPStan-only findings);
- **`argument.parameterRenamedInSubtype`**, on by default (off today): a
  named argument naming a parameter an override renames throws "Unknown
  named parameter" once dispatched there. A collector records named
  arguments against overridable methods by declaring class, late-bound
  `static::m(old: 1)` and `new static(old: 1)` included (both throw in php
  8.5; only instance calls are recorded today), and the renaming overrides
  come from reflection, not a second AST walk;
- **the trait-host verdict** as a collector plus rule: a trait method is
  analysed per host, and the constant-condition and type-check families
  report a trait's finding only when every host gives the same verdict
  (`ConstantConditionInTraitRule`). Today the every-host filter covers only
  `*.always{True,False}`, so `function.alreadyNarrowedType` escapes it
  (a false positive in `impossible-function-call-in-trait.php`), and a
  condition true in every host is unreported (16 rules-oracle findings);
- **the trait-host merge**: a trait's findings carry their host, and the
  same finding (span, identifier, message) from several hosts is reported
  once, naming them; a finding some hosts lack stays with those that have
  it. It replaces `check_trait_methods`' `dedup_by` on (span, message);
  nothing else deduplicates, and gate 13 fails a repeated finding.

**E2. builtins.**

- Moves onto the extension traits: `throws/dynamic.rs`, `builtin_calls`,
  `array_ops` returns, `preg_shapes`, `date_time`, the contextual-callback
  block, preg `$matches` out-types, GMP/BcMath operators, the built-in
  narrowing predicates, the date/xml/DateInterval validators (as throw-type
  extensions), regex/PCRE and the date parser. `builtins` is always
  registered, the testing harness included.
- Every return-type extension in Appendix G reaches the most precise type
  that holds at runtime in the nsrt suite (3,015 mismatches over 93
  extensions today). PHPStan's value is the candidate, not the target: ten
  of Appendix G's are wrong at runtime and stay approved deviations. 2,498
  are wider than PHPStan's type; 396 of them ran in php 8.5 and PHPStan's
  value held, and the commit adopting each of the other 2,102 runs it in php
  8.5 first. The other 517 are wrong at runtime (220 missing a value, 190
  typing a call PHP rejects as returning, 107 another version's return),
  named by the items below and fixed first.
- The throw-type extensions `throws/dynamic.rs` lacks, each throwing only
  for arguments that can fail (`array_chunk()`/`str_split()` lengths,
  `array_combine()` sizes, `min()`/`max()` of an empty array, printf
  formats, `get_class()`, `round()` modes, `trigger_error()` levels,
  `unserialize()` options, `version_compare()` operators, `Ds\Map::get()`,
  `DateTime::modify()`, …); without them `catch.neverThrown` misses about
  50 rules-oracle findings.
- Behaviour commits, each with its fixture, after the move. Wrong values
  first:
  - `get_parent_class($this)` is `false` in a class with subclasses
    (`identical.alwaysTrue` today) and on an interface-typed object;
  - `filter_var()` reads flags and options that aren't constant: `$strict ?
    FILTER_NULL_ON_FAILURE : 0` gives `bool|null` (`bool` today), a
    non-constant options array `mixed`, `FILTER_FORCE_ARRAY` an array (35
    nsrt);
  - `array_filter` makes a shape's keys optional and removes falsy values,
    a narrowing callback removes what it rules out (`isFoo(...)` over
    `array{Foo|int, Foo|int}` keeps the `int`s today; 16 nsrt);
  - `array_combine($keys, $values)` is keyed by the key array's values as
    PHP casts them, not by positions (`array_combine(['a', 'b'], …)` is
    `array{0: …, 1: …}` today; 29 nsrt);
  - `array_intersect_key($a, [])` and the other intersect functions with
    `[]` are `array{}`, and a key the other array may lack becomes optional;
  - `array_map()` over several arrays is a list; a spread argument passes
    its elements (`array_replace(...$paths)`); `str_replace()` on a string is
    `string`; `array_reverse()` of a shape reverses it; `array_sum()` of
    ints is `int`;
  - a regex built from parts is validated per constant alternative, and
    `preg_quote()`'s delimiter checked through named arguments (19
    rules-oracle findings);
  - `json_decode()` of a constant string and `E::from()`/`tryFrom()` of a
    constant fold to their values;
  - the printf and scanf format parsers follow PHP's (`%5%`, `%'%5d`,
    `%*2$d`; NUL ends a scanf format), and the doubled scanf report becomes
    one (11 rules-oracle findings);
  - `Closure::bind()` checks `@param-closure-this` and returns `Closure`
    for a literal; `XMLReader::open()` is `XMLReader|false` statically and
    `bool` on an instance;
  - `class_implements()` of an object has no `false`;
    `DateInterval::format('%a')` is numeric; `BcMath\Number` supports
    `++`/`--`; `PDOStatement` is generic; `SimpleXMLElement` compares with
    scalars and takes appends;
  - `count()` of a non-empty array is `int<1, max>` (after `$w[] = $v` too);
    the hash functions return lowercase non-falsy strings; `(int)` of a
    string past `PHP_INT_MAX` is `PHP_INT_MAX`;
  - `array_rand($a, $n)` is `int<0, max>|list<…>`, the list kept for a count
    that isn't known;
  - `array_map($cb, $l)` with `callable(V): M` over `list<V>` is `list<M>`,
    `array_filter` keeps its callback's narrowing, and `array_map($cb, [])`
    is `array{}`;
  - `array_slice()` may be empty (`array_slice($arr, 0, 0)` is
    `non-empty-array` today; 22 nsrt);
  - `array_change_key_case()` follows its mode (`CASE_UPPER` gives
    lowercase keys today) and changes shape keys and tails (15 nsrt);
  - `array_flip`, `array_search`, `array_fill_keys` and `array_reverse`,
    `array_values`, `array_keys`, `array_slice`, `array_chunk` and
    `array_replace` keep an unsealed shape's tail (the tail's values are
    dropped today);
  - `array_key_last`, `array_pop` and `array_shift` on a possibly empty or
    all-optional array include `null`;
  - `array_column($rows, null, 'key')` keys whole rows, typed as the column
    today;
  - keys built from values are cast as PHP casts them
    (`array_fill_keys([true, 1.5, '999'], …)` has `1`, `'1.5'`, `999`; an
    object key is its `__toString()`; 18 nsrt);
  - `array_merge()` is non-empty only when an argument is;
  - `explode()` with a negative limit may return `[]` (11 nsrt);
  - `get_defined_vars()` lists what can be defined (anything at file level,
    optional keys for one-branch variables, no `this`);
  - `preg_split()`'s `PREG_SPLIT_OFFSET_CAPTURE` makes each element an
    `array{string, int<0, max>}` (22 nsrt);
  - `rand()` takes its bounds in either order (`rand(5, 1)` is the empty
    `int<5, 1>` today);
  - `count()` of a `Countable` is its method's return; `compact()` keeps
    maybe-defined variables and reads names from array values;
    `array_keys()` of an unknown array is `list<int|string>`;
  - a return that changed between PHP versions follows the analysed range
    (the union over a range; PHP 7's `null`/`false` returns in a file or
    branch narrowed below 8: BcMath, `round()`, hash, `substr()`,
    `explode('')`, `highlight_string()` before 8.4; 107 nsrt);
  - `gettype()` of a `resource` includes `'resource (closed)'`;
    `Throwable::getCode()` is benevolent `int|string`;
  - `trigger_error('…', E_USER_ERROR)` is `never`;
  - `strtr()` of literals folds; the case functions fold each member of a
    constant-string union; `mb_*` case functions fold only ASCII, keep the
    literal for an unknown mode and don't fold another encoding as UTF-8;
  - a call PHP 8 rejects returns `*NEVER*`, so the code after it is
    unreachable: the 190 in Appendix G, each checked in php 8.5 (array
    functions given non-arrays, `array_fill()` past `PHP_INT_MAX`, BcMath
    by zero, `explode('')`, invalid hash algorithms, `random_int(10, 1)`,
    wrong types under `strict_types`, invalid `str_increment()` input,
    `str_repeat()` with a negative count, …);
  - the narrowing predicates follow 2.2.16's `*TypeSpecifyingExtension`s:
    `array_key_exists()` narrows the key and a `mixed` array (Appendix H's
    `key-exists-fold`, 153); `ctype_digit()`, `function_exists()`,
    `class_exists()`, `is_subclass_of()` give their string kinds;
    `array_search() !== false` and `count($a) === 1` make the array
    non-empty; `in_array()` narrows through enum and integer haystacks and
    narrows the haystack (`in-array-haystack`, 65), intersecting a strict
    subject; `count()`/`strlen()` of a union of constants are unions;
    `is_numeric()` drops non-numeric literals; `is_callable([$this, 'm'])`,
    `defined('Foo::X')`, `property_exists(static::class, …)` and
    `method_exists(static::class, …)` guard their members; `is_callable()`
    keeps `Class&callable` and gives `callable-array`; `is_a($s, A::class,
    true)` on a string is `class-string<A>` (the object `A` today, a wrong
    value); `str_contains()` and friends make the haystack non-empty (29
    nsrt).

  Then the rest, by each function's count in Appendix G: `filter_var` and
  friends (290, 194 `mixed`), `min`/`max` (217, 198 `mixed`),
  `array_column` (211), `sprintf`/`vsprintf` (135), BcMath (134), `count`
  (114), `array_slice` (101); beside Appendix G, `settype`'s narrowing (156),
  the preg `$matches` shapes (`preg_match_shapes.php` 204, Appendix H's
  `preg-shapes` 155) and the `BcMath\Number` operators (170, Appendix J's
  `bcmath-*`). Appendix H's `ext:` entries are the values these extensions
  flow into (621); `ext:PgDml`, `ext:Randomizer` and `ext:FilterInputArray`
  have no counterpart yet, and `getAttributes()` returns an array, not a
  list.

Gate: the analyser's built-in-name allowlist.

**E3. Every diagnostic to `rules`/ext modules.**

The prologue, one commit each, before any rule moves:

- `RuleLevelHelper::find_type_to_check` (PHPStan's `findTypeToCheck`): a rule
  asks it for the operand type and gets `FoundTypeResult` (the level's
  mixed, union and nullable handling applied, plus unknown-class errors),
  where each rule decides for itself today. The implicit `mixed` inside a
  type counts: at level 10 a plain `array` passed to `array<string>` is
  `argument.type`, and `array<int, mixed>` returned under `@return
  array<int, string>` is `return.type`, both unreported today;
- `RuleLevelHelper::accepts` takes the level-dependent acceptance out of
  `types::Relation::accepts`;
- the rule → level table (§3.9), cross-checked against the level neon
  files, with the level filter in the dispatcher (rules above the level
  never run), replacing the CLI's `MAX_LEVEL = 6` filter and the LSP's
  cutoff. Its approved deltas are the measured divergences (first reporting
  level, PHPStan / phpstan-rs): `isset.variable`/`nullCoalesce.variable`
  1/0; maybe-defined or top-level undefined variables 1/0;
  `method.notFound` off `$this` 2/0; members only through `__call`/`__get`
  2/never; union arguments 7/5 and returns 7/3; maybe-non-offset-accessible
  7/3; maybe-missing constant-array offsets 7/never; PHPDoc-only child
  parameter conflicts 3/never; `@var array` without a value type 6/never;
- the rest of the diagnostic model (§3.7): AST-node fixes rendered through
  a format-preserving `syntax::printer` that parenthesizes by precedence;
  `accepts` reasons as tips; PHPStan's standard tip helpers; messages
  naming a namespaced function by its namespaced name (576 rules-oracle rows
  print `as_array2()` for `Bug13247\as_array2()`); an integer shape offset
  printed as `2`, not `'2'`.

Then the sub-phases, grouped by derived node, each listing its Appendix A
rows, and each rule's behaviour commits after its move, each with its
fixture:

- **(a) per-file and per-body rules**: `keywords` (`break`/`continue`
  levels, `goto` and labels, `declare`), checking every body, property
  hooks included (a hook's `break` outside a loop and `goto` to a missing
  label are unreported today); `whitespace` (BOM, leading and trailing
  output); `RequireFileExistsRule` looking files up through the session's
  VFS instead of `Path::is_file`.
- **(b) expression-level**:
  - every operand check goes through `find_type_to_check`, so level 10
    reports an implicit `mixed` everywhere PHPStan does (corpus misses:
    `offsetAccess.invalidOffset` 954, `binaryOp.invalid` 652,
    `encapsedStringPart.nonString` 356, `foreach.nonIterable` 284,
    `assignOp.invalid` 223, `array.invalidKey` 223, casts 135, …);
  - the same helper reports an operand with one invalid member ("Possibly
    invalid", "might not be", per `checkUnionTypes`/`checkNullables`):
    `echo $a?->foo` on `Foo|null`, `foreach` over `array<int>|null`,
    `clone` of `Foo|null`, unpacking or destructuring a nullable, `.` on
    `array<string>|string`, …; with the `mixed` rows, the rules oracle's
    PHPStan-only `echo.nonString` 375, `binaryOp.invalid` 93,
    `offsetAccess.invalidOffset` 79, `assignOp.invalid` 77 and smaller rows;
    an `object` operand of `+`, `-` or `%` is invalid too;
  - `++`/`--` on `bool`, `null`, a resource or a non-numeric string is
    `postInc.type` and the rest (PHP leaves booleans unchanged, `null` for
    `--` and a non-numeric string for `--`, and throws for a resource);
    `[]` for reading inside `isset()`/`empty()`/`??` is `offsetAccess.noDim`;
  - an offset or key of an invalid type is reported for every type (an
    object offset throws a `TypeError`, unreported today);
  - `offsetAccess.notFound` covers every container and offset: a
    maybe-missing offset is "might not exist" at level 7, and definite
    misses beyond a constant shape (a disjoint key type, `null['t']`,
    `'foo'['foo']`, a `foreach` destructuring key, a folded `explode()`)
    are reported (47 rules-oracle findings);
  - arithmetic on a non-numeric `string` is `binaryOp.invalid` (PHP 8
    throws); a non-numeric string offset on a string throws;
  - the class-access checks cover every receiver: `$i::X` on an `int`,
    `static` access on a nullable or `int|string`, `T::C` on a trait,
    `instanceof` a trait, a static property read through `->`, an instance
    property read statically (`property.staticAccess`; 37 findings);
  - the constant-operand checks of `&&`, `||`, `and`, `or`, `xor` and `?:`
    fire on each side and on the result (`$b && true` isn't reported today;
    the rules oracle's 164 `boolean*`/`logical*` and 17 `ternary.always*`,
    of which phpstan-rs matches 2 and 0; the corpus oracle's 21
    `booleanAnd.leftAlwaysTrue` and 23 `booleanOr.alwaysFalse`), and a
    condition is decided by truthiness, not only a `true` type (`if ($std)`
    on an object is `if.alwaysTrue`; 37 rules-oracle `if.alwaysTrue`);
  - `instanceof` on an exhausted subject is reported, a `callable` is never
    an instance of a final class without `__invoke()`, and the `elseif`
    after `$x instanceof A && $x instanceof B` isn't wrongly always true
    (12 rules-oracle `instanceof.alwaysFalse`);
  - `unset()` of an undefined variable or of an offset on a non-array, and
    `new class {};` as a statement, are reported;
  - statements with and without effects: a call to a `@phpstan-impure`,
    abstract or interface method has effects and an abstract
    `@phpstan-impure` method isn't `impureMethod.pure` (15 findings); a
    `match` that may throw, a division, a `(string)` cast and a pipe are
    not `expr.resultUnused` (38); a statement that only reads is, whatever
    else the read reports (~45; with `clone`, `$f(...);` and bare closure
    calls, the oracle's 168 `expr.resultUnused`); `new static(…);` isn't
    `new.resultUnused` (13 of 19 phpstan-rs-only; `new self()` without
    effects stays reported, stricter than PHPStan), while a discarded `new`
    of a class without a constructor, a pure built-in or a final `static`
    is (most of 40); instantiating an abstract class throws and isn't also
    `new.resultUnused` (approved); `static::$p;` stays reported;
  - `yield` outside a function is `generator.outOfFunction`; PHP 8.5's
    `(void)` cast on a call without `#[\NoDiscard]` is pointless (10); a
    discarded bare closure call is `expr.resultUnused`;
  - `self`/`static`/`parent` outside a class are `outOfClass.*` in every
    position (5).
- **(c) calls and member access**:
  - a method call or property read on a plain `object`, a template bounded
    by `object`, or a static call on `object` is `*.notFound` (59 corpus
    findings);
  - a property fetch inside `isset()`/`empty()`/`??` checks its receiver
    (`isset($s->p)` on a string; `mixed` at level 10; 20 findings), and a
    missing property is `property.notFound` there too where nothing can
    declare it (a final class, a `new` object, an object shape's missing
    key; ~23);
  - visibility follows PHP: a protected method in a common ancestor is
    accessible from a sibling; a `private(set)` property is writable
    anywhere in its class; a union of literal constant names is a string;
    `static::FOO` of a non-final constant isn't always true;
  - a by-reference-returning call is a valid by-reference argument (5); a
    missing method is `method.notFound`, never `method.nonObject`;
  - a method call or property read on a nullable receiver is reported from
    level 8 (161 `method.nonObject` and 110 `property.nonObject` in the
    corpus), its arguments then not checked, and a nullsafe link makes the
    rest of the chain nullable;
  - a method call on a receiver that can't be an object is reported for
    every receiver (`T of mixed`, an `int` first-class callable, an
    always-`null` `?->`, a narrowed class-string, a level-10 `global`
    `mixed`, a static call on `mixed`);
  - a constant `define()`d in a non-constructor method isn't known
    elsewhere (the constructor's is, typed; `constant.notFound` everywhere
    today), `__COMPILER_HALT_OFFSET__` needs `__halt_compiler()`, and a
    missing `use const` is reported where read;
  - `filterVar.nullOnFailureAndThrowOnFailure` reads named, variable and
    per-key options; `staticClassAccess.privateProperty` at every access;
  - magic constants in the wrong context are reported in parameter and
    property defaults too (`__TRAIT__` outside a trait, `__CLASS__` outside
    a class, …; 12), reading the scope, so `Checker::constant_facts` (a
    pre-walk marking `__FUNCTION__` inside closures) is deleted;
  - `null <= $object` is always true, enum cases compared by `<` are always
    false, and a ternary statement is `ternary.resultUnused`;
  - a parent's private constant through a child, or a member some part of a
    union or intersection lacks, is `*.notFound` as PHP reports it;
  - named arguments that leave a required parameter unset are
    `argument.missing`, and a name the callee lacks `argument.unknown`
    (15);
  - a private property under `??` is `property.private`, and PHP 8.5's
    `clone($foo, [...])` checks visibility (8);
  - `self::{$mixed}` is `classConstant.nameType` at level 10;
  - invoking a non-callable value is `callable.nonCallable` (a constant
    string naming no function, a union with an uncallable member);
  - a by-reference argument PHP creates isn't type-checked;
  - flag parameters are checked against `constantToFunctionParameterMap.php`
    at every call form (`argument.invalidConstant`,
    `argument.exclusiveConstants`, `argument.bitmaskNotAllowed`; 33);
  - `new DateTime('2020.11.17')` is `new.dateTime` (the rule misses both
    rows today);
  - named arguments inside `PHP_VERSION_ID < 80000` are
    `argument.namedNotSupported` once D5 narrows the version; static
    closures for `@param-closure-this`, `callable.void`,
    `callable.resultDiscarded` and unresolvable call types are reported;
    `@no-named-arguments` forbids named arguments and string-keyed unpacking
    (11 corpus, 10 rules-oracle);
  - a `void` call's result is reported wherever it is used (54 corpus
    findings; 49 rules-oracle across the use forms);
  - a discarded pure call is reported for built-ins (through
    `functionMetadata`), namespaced and cross-file user functions (by
    resolved name through C9's summaries), first-class callables, nullsafe
    calls, pipes and `functionMetadata`'s methods (409 + 109 rules-oracle
    `function.resultUnused`, 109 `method.resultUnused`, 61
    `staticMethod.resultUnused`);
  - arrays whose values PHP casts to string are checked for castable values
    (`ParameterCastableToStringRule`; ~35);
  - `random_int()` with ranges that can't satisfy `$min <= $max` is
    reported (8); a template the call can't resolve is
    `argument.templateType` (17 corpus, 9 rules-oracle).
- **(d) assignments and properties**:
  - `uninitialized` and `uninitialized_dataflow` are rewritten over the
    ClassPropertiesNode state, the dataflow walker deleted;
  - a write to an undeclared property is `property.notFound`; a typed
    static property checks its value;
  - every write form is checked against the property's type (appends,
    offsets, nested writes, by-reference array functions, compound
    assignments, increments past a range; most of 138 rules-oracle
    `assign.propertyType`);
  - `unset()` of a readonly, hooked or possibly-hooked property is reported
    (a `@readonly` one too), and a `@readonly` property's own set hook may
    write it;
  - write-only and read-only detection: an append-only property is
    write-only; a read through a `$this` alias counts, a read feeding the
    property back through its own constructor or hook doesn't, and a
    dynamic read counts only for the names it can be; every write form
    counts as a write for `property.onlyRead`/`neverWritten` (64
    rules-oracle findings, Appendix I's `property-write-forms` 24 more);
  - a property that can't be written is `assign.propertyReadOnly` for every
    kind (enum `name`/`value`, object shapes, `@property-read` from
    outside, readonly in `clone()` from outside; 11); a readonly assigned
    twice is `assign.readOnlyProperty` (12); `$this` is written by every
    write form (12); a readonly property is modified by every write form
    outside the constructor, and taking a reference to one is reported
    (~30);
  - property hooks: overriding keeps writability and readability, a
    non-virtual hook reads and assigns its value (a short set hook
    assigns), a get-only virtual property may narrow the overridden type,
    asymmetric static visibility and `@readonly` twice-assignment are
    reported, and an `@immutable` class's properties are `@readonly`;
  - a readonly property read before initialization, or one the constructor
    doesn't assign on every path, is `property.uninitializedReadonly` (the
    rule doesn't exist; 84 and 30 rules-oracle findings, 12 corpus);
  - an `ArrayAccess` offset write checks the key against `offsetSet`
    (`offsetAssign.dimType`) and the value (`offsetAssign.valueType`; 20);
  - an offset write or append checks its container as a read does (383
    corpus findings on `mixed`), and a read inside `isset()`/`??`/`empty()`
    or an `unset()` on an object without `ArrayAccess` throws and is
    reported (341 on `mixed`, ~60 rules-oracle).
- **(e) declarations, generics checks and PHPDoc**:
  - class existence is checked everywhere PHPStan checks it (173
    rules-oracle `class.notFound`): use sites through `find_type_to_check`
    (~90), `Unk::class`, constant-string `new $a()`, template bounds and
    defaults, `@param-out`, `@phpstan-self-out`, conditional branches,
    generic arguments, a promoted parameter's property, a trait named as a
    type; a wrong-case name is `class.nameCase`/`interface.nameCase`;
  - `parameterByRef.tooWide` becomes PHPStan's
    `TooWideParameterOutTypeCheck` (`parameterByRef.unusedType`,
    `paramOut.unusedType`; 10);
  - an empty array under any array `@var` is widening, not `varTag.type`;
  - generic arguments built from a template are checked through its bound
    (8); a template inside a callable parameter's signature is "in a
    parameter" (7); an assertion of a template type, or of an array over
    `mixed` values, narrows (`assert.alreadyNarrowedType` today; Appendix
    J's `assert-array-mixed`);
  - a covariant template may type a property that can't be written after
    construction, and the message names a property;
  - compile-time class errors are reported (`parent::` without a parent,
    `implements \iterable`, unknown `@phpstan-*` tags);
    `generics.existingClass` wherever templates are declared;
  - the remaining one- and two-finding gaps, each named with its identifier:
    a `private(set)` property is implicitly final; `print_r(return:
    false)`; `@param-out` on a set hook; `array != int` always true;
    unopenable stream-wrapper includes; nullsafe from a `&get` hook; built-in
    constant overrides' native types; union `@phpstan-require-extends`;
    `self::doBar(...)` from static context; abstract static callables;
    trait-typed promoted properties; `generator.void`; enum case value
    types (a string-concatenated case wrongly `enum.caseType` today);
    `new` of a constant interface class-string; `private(set)` appends;
    `arrayFilter.same`; `pipe.byRef`; `BcMath\Number` comparisons;
    `doWhile.alwaysTrue`; `@phpstan-assert` on `$this` in a static method;
    `generics.interfaceConflict`; a `catch` writing a by-reference
    parameter;
  - variance through composition (`VarianceCheck`), each parameter on its
    own, skipping private methods (18 rules-oracle findings);
  - imports are per namespace block (`use.nameInUse`, 7);
  - overrides: optional parameters before a variadic and a variadic
    absorbing parameters are allowed (false positives today); an added
    parameter must be optional (`parameter.notOptional`); contravariance
    against trait abstracts, built-in parents and abstract constructors
    (12 `method.childParameterType`); `new static()` is safe under a final
    private constructor;
  - a conditional return on a template is decided by its whole bound (4 +
    4);
  - `@var` above `global` and `static` sets the type, and `@var` is checked
    against the native type wherever it applies;
  - `#[\Override]` on a constructor is `method.override`, and on a trait
    member is checked per host;
  - conditional returns may test `$this` and a callable's parameter;
    assertions may name a method call; a `@var` naming a variable the right
    side reads types it at file level (`varTag.differentVariable` inside a
    function); `static` in `@phpstan-assert` resolves;
  - `@param-closure-this` on a `callable` isn't `nonClosure` (15); `self`
    and `key-of<…>` are supported template bounds (15);
  - unresolvable types are reported for every tag kind (`never`,
    non-intersecting shapes, a non-object `@param-closure-this`; 32 corpus,
    14 rules-oracle), and an unresolvable `@param` still types its
    parameter;
  - a constant's `@var` is checked against its native type
    (`classConstant.phpDocType`);
  - type-alias checks (`typeAlias.circular`, `unresolvableType`,
    `invalidName`, `notFound`, `invalidType`; 18) and unresolvable
    `@method`/`@property`/`@mixin`/constant tags (8);
  - `classConstant.class` and `classConstant.dynamicString`; `@mixin int`;
    an interface, trait or enum is never an attribute class; tags a trait
    can't carry are reported;
  - purity: property hooks, `pureFunction.redundantUnlessCallable`, `@throws
    void` no longer exempting `pureFunction.void`, purity through an
    ancestor's tag, immediately invoked callbacks as part of the pure body,
    implicit `__toString()` and unmarked methods as possibly impure,
    references to properties, `exit` in an immediately invoked closure,
    first-class callables pure and taking the callee's purity (19), a
    string naming a function isn't a call, and `impureMethod.pure` needing
    a final method or class;
  - an enum's duplicate case value is found through constant expressions;
  - set hook parameter contravariance, `generics.existingTypeAlias` for a
    local alias, `typeAlias.duplicate`, the `requireExtends.on*` placement
    checks, `class.missingImplements` through an enum's trait;
    `consistentConstructor.private`;
  - an overriding property's PHPDoc type is covariant (5), and an
    override's return type is checked in every form, tentative types
    included (24 + 22);
  - `#[\NoDiscard]` on `void`/`never` is `attribute.target` (14);
  - `requiredAfterOptional`, `implicitlyNullable` and unresolvable native
    types cover closures and arrow functions, with per-version wording (25);
  - a trait's private constant is checked per host, and a dynamic
    `static::{$name}` counts only for its names;
  - `throws.notThrowable` for every non-`Throwable` `@throws` form (10);
    `method.templateTypeNotInParameter` wherever the template appears (80);
  - an anonymous class is a declaration like any other (41 rules-oracle
    findings name a `class@anonymous`); `abstract` on an interface method is
    reported;
  - `missingType.iterableValue` walks every type `MissingTypehintCheck`
    walks (~75 of 118), and the generics checks the same sites
    (`missingType.generics` 18, `generics.notGeneric` 15, `lessTypes` 10,
    `moreTypes` 7, `notSubtype` 15, call-site variance 14).
- **(f) control flow, returns, throws and generators**:
  - an empty body is checked like any other: a non-`void` function with an
    empty body is `return.missing` (507 of 517 rules-oracle PHPStan-only),
    `return.never` under `never`, `impureMethod.pure` under
    `@phpstan-impure`; the same exemption skips `@param-out`'s end check,
    which sits on each end statement (45 `paramOut.type`);
  - `return.byRefFinally` (13) and `finally.exitPoint` (25);
  - `return.missing` is reported where execution falls off, once per end
    (36 of 53 phpstan-rs-only are the same finding on another line);
  - `return.unusedType` isn't reported for a union of constants when every
    return is a constant (10), and is for closures, bodies that can fall
    off and conditional return types (22);
  - a return type including `void` accepts falling off and `null` (10 of 38
    phpstan-rs-only `return.missing`, the php_codesniffer sniffs);
  - a conditional return type resolves at each `return` against the
    scope's parameter types (12 corpus `return.type`);
  - `yield` is checked against the declared key and value types
    (`generator.keyType`/`valueType`; 28), and `generator.returnType` at
    each `yield`;
  - a returned value is checked against a template return type (9);
  - a concrete empty body checks its `@throws` (all 44 PHPStan-only
    `throws.unusedType`), while a call without `@throws` is an implicit
    throw point that satisfies it (5);
  - `throw` checks its operand (`throw.notThrowable`, 6); a conditional
    `@throws` resolves per call (12);
  - `catch.neverThrown` from D2's throw points (the rule doesn't exist; 189
    rules-oracle findings), reading PHPStan's throw points: a typed
    property write throws only when the value may not fit, a division only
    by a possible zero, `match` only when inexhaustive, `__toString()` and
    `__call()` by their `@throws`, `isset()` on `ArrayAccess` by
    `offsetExists()`, `new $class` by its constructor, hooks by their own
    `@throws`, each multi-catch member alone.
- **(g) the restricted-usage framework**: one set of usage sites (calls,
  property and constant reads, `new`, `instanceof`, `catch`,
  `extends`/`implements`/`use`, attributes, native and PHPDoc types,
  first-class callables, implicit `__toString()`), each asking every
  registered `Restricted*UsageExtension`: `@internal` (core), `@deprecated`
  (ext-deprecation-rules), `restricted_symbols` (ext-disallowed-calls) and
  PHPStan's `@api` (`phpstan_api`). Today each has its own subset:
  `deprecated_usage` covers only calls, property and constant reads and
  `new`, so a deprecated class in a type hint, `extends`, `instanceof` or
  `catch` is unreported.
- **(h)** `FileAssertRule` with `assertNativeType` and
  `assertVariableCertainty`, the dump rules and the four asserts:
  `dumpType()` and friends report under PHPStan's `phpstan.*` identifiers
  (76 rules-oracle findings, none reported today); the asserts' mismatches
  keep `phpstanRs.*` (approved).
- **(i)** the "is this usage internal / deprecated from this scope"
  predicate (§3) exposed from (g), so E6's semantic tokens mark every usage
  site the rules check (function, member and `new` sites only today).

Mixed modules are split row by row (Appendix A). Gate: architecture rule 10
for the analyser.

**E4. Extension modules.**

- Order:
  1. ext-doctrine's remaining modules (its reflection data extension moved
     in C8; E4 fixes its entity metadata: only mapped properties are fields,
     `#[MappedSuperclass]` and trait fields included, `@ORM` docblock
     annotations read), then phpstan-doctrine's rules, which it has none of
     today (DQL syntax and semantics in `createQuery()` and QueryBuilder
     chains, `findBy()` fields, magic `findOneBy*()`, the entity checks);
  2. ext-sql;
  3. ext-phpunit, ext-codeception, ext-mockery and ext-mockstan, after E1;
     ext-phpunit gains phpstan-phpunit's rules (`@covers` targets,
     assertion simplifications, `->method()` on mocks, data providers),
     keeping its own `test.risky`;
  4. ext-pest, then pest-plugin-phpstan's 20 identifiers (duplicate
     descriptions, group names, test closures, `repeat()`, hooks in
     `describe`, `covers()`/`throws()` targets, expectation checks, local
     `uses()`);
  5. ext-nette-tester;
  6. ext-strict-rules, completing phpstan-strict-rules: today's 11 toggles
     plus the missing `disallowedLooseComparison`, `disallowedBacktick`,
     `disallowedEmpty`, `disallowedShortTernary`, `noVariableVariables` and
     `closureUsesThis`, and the core parameters the package sets (C1);
  7. ext-deprecation-rules;
  8. ext-disallowed-calls;
  9. ext-shipmonk, ext-type-perfect, ext-voku, ext-sidz, ext-slam,
     ext-psr3, ext-language-extensions and ext-extras.

  Each crate mirrors its package completely (§2). Compared rule class by
  rule class against the packages' current releases, today's coverage:

  - ext-shipmonk has comparable types, null in operations and
    interpolation, and enum match exhaustiveness (with ext-extras and
    `restricted_symbols` covering four more); missing:
    `BackedEnumGenerics`, `ClassSuffixNaming`,
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
  - ext-type-perfect has `NoIssetOnObject` and `NoEmptyOnObject`; missing
    `NoArrayAccessOnObject`, `NoMixedMethodCaller`, `NoMixedPropertyFetcher`,
    `NoParamTypeRemoval`, `NarrowPublicClassMethodParamType`,
    `NarrowPrivateClassMethodParamType`, `NarrowReturnObjectType`,
    `ReturnNullOverFalse`;
  - ext-voku has the mixed-operand checks; missing the `IfCondition*`
    rules, `ElseIfConditionBasic`, `WrongCast` and
    `DisallowedCallMethodOnNull`;
  - ext-mockstan has eight of ten (missing `NoEntityMocking`,
    `NoDocumentMocking`);
  - ext-disallowed-calls maps functions, methods and classes; missing
    namespaces, constants, properties, superglobals, attributes, enums,
    `new`, the control-structure kinds and the ROADMAP's entry fields;
  - ext-sidz, ext-psr3 and ext-language-extensions are complete; slam and
    phpstan-mockery have no rules.
- Extensions register their own parse-time visitors (§3.1).
- ext-extras' `@see` check reads a reference as its author means it
  (`@see Assert::assertSame` names the method; free text isn't a
  reference): 241 phpstan-rs-only corpus findings today, 176 in PHPUnit's
  `Functions.php`.
- ext-sql resolves static-schema SQL against the catalog and validates
  parameters through an `sql` API; `sqlparser` leaves the package.
- The Doctrine QueryBuilder chain becomes extension type data, through
  method type-specifying and an `ExpressionTypeResolverExtension` (an
  approved delta, fixtured for aliasing, mutation and changing arguments).

**E5. Levels** (§3.9), after E3 moved every rule at its level: the `max`
alias, the default level for CLI and LSP, and the replacement consumers for
JSON and the LSP (`Diagnostic.level` goes). The level tests become
level-header fixtures, and LevelsTestCase fixtures are added. Gate: for
every level in 0..=10 plus unset, before == after, except the approved
deltas E3's table names.

**E6. Observers.** LSP-only presentation (semantic tokens from E3(i), inlay
hints, hover, definition probes) on observer events.

**E7. Checker decomposition.** Gate: the exact enumerated field sets for
engine context, file state, function frame and traversal context
(Appendix C), with interior mutability only in pure memo caches.

### Stage F: Session and LSP (in the package)

**F1. Session** (§3.10), one commit each:

- `AnalysisResult` (`project::session::AnalysisResult`);
- the finalizer (`AnalysisResult::finalize`, with no level stage: E3's
  dispatcher already filters by level; the LSP moves onto it in this
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

**F2. Result cache and dependencies**: resolved dependencies (reflection's
memo read sets, C4, give the declaration-level edges), fingerprints,
the persistent cache, incremental collectors, `deps.rs` deleted. The LSP
replay gate goes on.

**F3. ErrorFormatter** set (JSON schema versioned).

**F4. Binary**: global flags over `app::check_project`, `--set`,
`check`/`demo` deleted, `incremental-bench` → project example plus bench,
`format_signature` test moved.

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
- **User-visible changes.** Config schema (with a shim), CLI flags,
  baseline format (with a converter), JSON schema (versioned), exit code,
  output order. Each is its own changelog-noted commit.
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
| tests/cli.rs, version.rs, lsp_exit_code.rs | bin package | B1 |
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
| type_dump | `--dump-types` observer |

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
| clap | bin |
| crossbeam-channel, lsp-server, lsp-types | lsp |
| dhat | bin |
| hotpath | every measured crate (plain dependency); features only on the bin |
| imbl | syntax (name resolution's `use` map); analyser (holder maps); reflection only if its snapshots use it |
| indexmap | reflection |
| md5 | reflection (anonymous-class naming) |
| pcre2-sys | builtins (`pcre-validate`) |
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
  phase's own behaviour commit (approved delta) after its structural
  commit. Fixed first instead: the `PHP_VERSION_ID` range typing (C2),
  C3a's `property.unused` delta, `int|float` arithmetic (D3), top-level
  variables as maybe-defined (D5), loop/switch termination (D2) and
  `is_resource()` never always true (D3), impure `$this` calls
  dropping `$this->…` narrowing (D5) and `finally` reads of variables the
  `try` assigns (D5) and `return.missing` on a `void|…` return type (E3f),
  each a false positive on ordinary code; D2 and D5 name the interim fix.
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
there. The full evidence the stage items were written from (every fixture,
file and count) is in `CRATE_SPLIT_EVIDENCE.md`, by stage and phase.
