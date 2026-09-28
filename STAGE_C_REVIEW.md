# Stage C review: findings

Findings from reviewing the Stage C work done so far: C1 and C2 (one
commit), C3's four commits, the rename to reflection names, and the two
fixes that followed on the reflection types. Judged on runtime correctness
and design, not parity with PHPStan. Each task names where the problem is,
what is wrong, and what done looks like.

The earlier Stage C reviews sampled the work and were folded into the
crate-split plan as the C1–C4 follow-ups; those tasks stay in the plan and
are listed below for reference. The full review reads every commit's diff
in order; its coverage is tracked at the end, and its findings are the F
tasks.

Legend: `[x]` done (commit given), `[>]` moved to the crate-split plan
(where given), `[ ]` open.

## Validation

- [ ] **V1. The fixture suite and corpus** run once, at the HEAD the Stage B
  review's V1 and V2 use, which includes Stage C; a failure or delta traced
  to a commit below is judged here.

## Earlier reviews (in the crate-split plan)

- [>] **C1 follow-ups:** nested validation and the full `ignore_errors`
  shape; missing settings; one settings assembly for the CLI and the LSP;
  activation reaching library typing; paths and matching; entries validated
  at load; config discovery; one walk per file; key names; PHPStan configs
  (ROADMAP Phase 183); tests and docs.
- [>] **C2 follow-ups:** the settings gate; predefined constants over a
  version range and both integer sizes; `ConstantResolver` in `reflection`;
  initializers evaluated as `InitializerExprTypeResolver`; one folded
  constant key; `generalize`; `RuleLevelHelper` owning the rule.
- [>] **C3 follow-ups:** C3a's `property.unused` delta; `is_phpstan_code` as
  a provenance query; attribute arguments evaluated to types (C8); the rest
  of the data model in C4.

## Full review: findings

Grouped by severity, most severe first. Each names the commit that
introduced it or, for a module read in its final state, the module;
"probed" means reproduced with an `assertType` or diagnostic probe
through `check-project`.

### False positives (probed)

- [ ] **F1. A project class named like a built-in stub class takes the
  stub's identity.** (`dcc67ce5`; `ClassReflection::merge`)
  `Registry::merge_stub_classes` merges the project's declaration into the
  stub's (`info.merge(existing)`), so the stub's provenance and attributes
  win. C3a made provenance the only record of where a class is declared,
  so for a global `class Event` (the event extension's stub class):
  - an unused `Event` is never reported `class.unused` while `Evnt` is:
    `class_declaration` finds no project file;
  - `#[Allowed] class Event` still has its `var_dump()` reported under
    `allow_in_class_with_attributes = ["Allowed"]`: the class's attributes
    are the stub's;
  - go-to-definition, references, rename and workspace symbols find no
    declaration either (from reading).

  `a11f7012` fixed this for composer hygiene only, by reading the file
  table instead. Done: the project's declaration keeps its own
  provenance, attributes, access attributes and flags when a stub of the
  same name is merged in, and one fixture per consumer (dead code,
  restricted-symbol attribute contexts) covers it.

- [ ] **F2. An attribute pattern written with a leading backslash stops
  matching.** (`49e6d9b1`) Attribute contexts compare against resolved
  names now, without the leading `\`, but the configured patterns are
  compared as written: `allow_in_methods_with_attributes =
  ["\\Lib\\Route"]` no longer allows a call inside `#[Route]` (imported
  from `Lib\Route`), while `"Lib\\Route"` does. A pattern names a class,
  so its leading `\` is dropped when the entry is compiled, as for every
  other class name the entries take.

- [ ] **F3. Attribute markers match the name as written, not the class it
  resolves to.** (`function_sig.rs` final state;
  `is_no_discard_attribute_name`, `is_override_attribute_name`,
  `is_deprecated_attribute_name`,
  `is_allow_dynamic_properties_attribute_name`, and `has_attr` for
  `ReturnTypeWillChange`/`TentativeType`) In `namespace App;` without an
  import, `#[Override]` names `App\Override` (reported
  `attribute.notFound` on the same line), yet the method is reported
  `method.override`; `#[NoDiscard] function g()` makes `g();` report
  `function.resultDiscarded`. PHP runs the file without a warning. C3c gave
  every declaration resolved attributes; each marker asks the
  `ReflectionAttribute` for its resolved name instead.

- [ ] **F4. A lowercase `__tostring()` doesn't make a class `Stringable`.**
  (`class_info.rs` final state, `from_decl`) The implicit `Stringable` is
  added only when `methods.contains_key("__toString")`, an exact-case key:
  `class A { function __tostring(): string {} }` passed to a `Stringable`
  parameter is `argument.type`. PHP says `new A() instanceof Stringable`.
  Method names compare case-insensitively.

- [ ] **F5. An enum ignores its own `@phpstan-type` aliases and its
  constants' `@var`.** (`class_info.rs` final state, `from_enum_decl`) It
  builds `type_aliases`, `type_imports`, `method_tags` and every constant's
  type without the enum's docblock. An enum declaring `@phpstan-type Row
  array{id: int}` and a method `@return Row` gets `class.notFound`,
  `missingType.iterableValue` and `return.phpDocType`, and `E::A->row()`
  is `array`; `/** @var list<string> */ const NAMES = ['a']` reads as
  `array{0: 'a'}`. The Stage B review's single-builder item (the two
  builders, `from_decl`/`from_enum_decl`) removes the cause; until then,
  each probe is a fixture.

- [ ] **F6. Build-dependent constants are read as the stub generator's
  values.** (`predefined_constants.rs` final state) `PHP_EXTRA_VERSION` is
  `'-13ubuntu3.2'`, so `PHP_EXTRA_VERSION === ''` is
  `identical.alwaysFalse` (it's `''` on this machine). The same holds for
  `ZEND_THREAD_SAFE` and `ZEND_DEBUG_BUILD` (`false`), `PCRE_JIT_SUPPORT`,
  `DEFAULT_INCLUDE_PATH`, `PEAR_INSTALL_DIR`, `PEAR_EXTENSION_DIR`,
  `PHP_CONFIG_FILE_SCAN_DIR`, `PHP_BUILD_PROVIDER`, `INTL_ICU_VERSION`,
  `INTL_ICU_DATA_VERSION`, `LIBXSLT_DOTTED_VERSION`,
  `LIBEXSLT_DOTTED_VERSION`, `LIBXML_LOADED_VERSION`,
  `OPENSSL_VERSION_TEXT`, the `GD_*` version constants, `ZLIB_VERSION` and
  `CURLVERSION_NOW`. Each gets its general type in
  `predefined_constant_type`, checked one by one against the stubs'
  `define()`s.

### Wrong types and missed reports (probed)

- [ ] **F7. A namespaced constant is shadowed by a built-in of the same
  name.** (`4c18d661`, `ConstantResolver::global_constant_type`) The
  predefined lookup runs for the unqualified name's global fallback
  before the namespaced declaration is looked up: in `namespace Foo;
  const PHP_EOL = 5;`, `PHP_EOL` is `"\n"|"\r\n"` (PHP gives `5`). The
  namespaced candidate is resolved first, predefined or declared, then
  the global one, as PHP resolves an unqualified constant.

- [ ] **F8. A promoted property drops its `(set)` visibility and its
  `@deprecated`/`@internal`.** (`class_info.rs` final state, the promoted
  branch of `from_decl`) `write_visibility` is `promoted_visibility`, and
  `implicit_protected_set` ignores the set visibility, so for
  `__construct(public private(set) int $x)` the write `$a->x = 2` outside
  the class is unreported; PHP throws "Cannot modify private(set)
  property". The AST has carried `promoted_set_visibility` since Stage B.
  `is_internal: false` and `deprecated: None` ignore the parameter's own
  docblock, so reading a promoted property marked `@deprecated` is
  unreported while a declared one is `property.deprecated`.

### Design

- [ ] **F9. The old flat keys still load.** (`4c18d661`) §3.6 says there's
  no compatibility shim ("the old flat `check_*` keys go outright ...
  `app.rs`'s `MOVED_KEYS` table with them"), and no C1 follow-up tracks
  it, but `MOVED_KEYS` moves 72 old keys with a deprecation warning, on the
  command line too (`--set check_dead_code=true`). Delete `MOVED_KEYS`,
  `Schema::moved`, `apply_moves`, `resolve_key`'s moves, `deprecation` and
  `ConfigFile::warnings`, the tests that load old keys
  (`app/tests.rs`'s flat-layout test, the three old-key tests in
  `config/src/tests.rs`), and the fixture harness's warning check; an old
  key is an unknown key.

- [ ] **F10. The opt-in checks lost their reasons.** (`4c18d661`) The
  `Registry` fields, `Config` fields and CLI flags C1 deleted each said
  why their check is off by default (the disclosed false-positive shapes
  of dead code and dead members, the stub-override case of duplicate
  classes, the `@throws` tagging of missing checked exceptions). The
  settings fields that replaced them are one line each, and five comments
  still send the reader to them for the reason
  (`checker/oop/method_overrides.rs:69`, `checker/throws/missing.rs:10`,
  `project/whole_program/dead_code.rs:20`,
  `project/duplicate_class_sites.rs:31`, `checker/internal_usage.rs:2`).
  Each settings field says what its check reports and why it's off by
  default; C1's generated config reference is built from these.

- [ ] **F11. A single program's bodies answer for any declaration.**
  (`bfce0e9f`) `impl BodySource for FileBodies` looks a body up by name
  span alone, whatever the provenance's origin, so a stub's or another
  file's declaration at the same span gets this program's body. Only
  `Registry::build` and the checker's test helper use it, where stub
  declarations are merged in. It answers only for the program's own
  origin.

- [ ] **F12. The all-on gate's keys are a hand-kept list.** (`4c18d661`,
  `xtask` `ALL_ON_SETS`) A key added to a section is exercised by the
  all-on configuration only if someone adds it here too. The list comes
  from the schema: every boolean key of every registered section on, and
  every extension enabled.

### Comments

- [ ] **F13. Stale and misplaced text.**
  - `ClassReflection::from_enum_decl`'s doc comment says cases aren't
    registered and nothing infers static access; both are done.
  - The reflection types live in `class_info.rs`, `function_sig.rs` and
    `class_info/`, named after the types the rename replaced.
  - `registry/tests/stubs_and_conditional_declarations.rs:726` cites
    `registry/function_sig.rs:606`, a line number.
  - The CLI tests are still named for flags (`check_project_*_flag_*`),
    though no flag remains; each sets its key.

### Outside Stage C

- [ ] **O1. Control characters in a string literal type are printed raw.**
  (`types` display, Stage B) `PHP_EOL` displays as `' '|' '` (a newline
  printed as a space), and `"a\tb"` and `"a\0b"` put a raw tab and NUL
  in the output, so an escape character in the analysed code reaches the
  terminal. A literal with a control character is quoted with its
  escapes (`"\n"`, `"\t"`, `"\0"`), and two different literals never
  display alike.

## Full review: coverage

Every Stage C commit's full diff (`crates`, `xtask`, `gates`), read in
order. A commit is ticked once its whole diff has been read and its
findings added above.

- [x] `4c18d661` refactor: config owner sections, settings leave the registry
- [x] `dcc67ce5` refactor: every declaration records its provenance (C3a)
- [x] `bfce0e9f` refactor: the registry holds no function bodies (C3b)
- [x] `49e6d9b1` refactor: attributes carry resolved names and arguments (C3c)
- [x] `41874e77` refactor: reflection types are read through accessors only (C3d)
- [x] `ca63ef85` refactor: the reflection types take their reflection names
- [x] `a11f7012` fix: composer hygiene credits a package with what its files declare
- [x] `69b97cb1` fix: an anonymous class is never reported unused

### Final state, module by module

Each module Stage C created or reshaped, read in its final state after
the commit pass: whole, or the parts named where a module is mostly
accessors or code Stage C didn't touch.

- [x] config: `lib`, `tests`
- [x] app: the schema (`app.rs`), `app/check_project.rs`, `app/tests.rs`
- [x] settings owners: `SessionSettings`, `ReflectionSettings`,
  `EngineSettings`, `OverlaySettings`, `RulesSettings`, `BannedSettings`,
  `ArchitectureSettings`, `ClassShapeSettings`, every `ext::*` section,
  `ActiveExtensions` and `CheckSettings`
- [x] checker: `constant_resolver`, `predefined_constants`,
  `rule_level_helper`
- [x] registry: `provenance`, `bodies`, `reflection_attribute`,
  `settings`, and the builders of `class_info` (`from_decl`,
  `from_enum_decl`, `merge`, `record_provenance`) and `function_sig`
  (`from_decl`, `map_types`, `infer_const_type`, the attribute markers)
