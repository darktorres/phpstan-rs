# Stage B review: findings

Findings from reviewing Stage B's follow-ups: the B3 syntax items, the B4
type items before "Type behaviour lives on the type", that item itself, and
the fixes from its first review. Judged on runtime correctness and design,
not parity with PHPStan. Each task names where the problem is, what is
wrong, and what done looks like.

Reviews 1 and 2 sampled the work (targeted reads of some modules); they
were not a full review. The full review reads every commit's diff in
order; its coverage is tracked at the end, and its findings are the F
tasks.

Legend: `[x]` done (commit given), `[ ]` open.

## Validation

- [ ] **V1. Run the fixture suite.** `cargo test --workspace` skips it
  (`test = false` on the `fixtures` test in `crates/phpstan-rs/Cargo.toml`),
  so it has not run since before "Type behaviour lives on the type" began.
  Run `cargo test -q -p phpstan-rs --test fixtures` (about 3,700 fixtures)
  and judge every failure as a correct change to bless or a regression to
  fix.
  - Known expected change:
    `checker/phpdoc_native_compat/a_generic_callables_templates_are_validated`
    loses its `generics.notSupportedBound` line for a `callable` bound (see
    R1-6).
  - Expected shifts: nullable receivers reported at level 8 and partial
    unions at level 7 (R1-1), list keys shown as `int<0, max>` (R1-L),
    printf messages naming the whole argument type (R1-1).
- [ ] **V2. Corpus run and delta approval** after V1, saved to
  `.corpus-runs/<scope>/<hash>.txt`.

## Review 1: "Type behaviour lives on the type" (done)

- [x] **R1-1. Union answers overclaimed.** `Relation::answer` combined a
  union's members with `and` and set `null` aside, so `int|array` answered
  a definite `No`. A union now answers `Maybe` when its members disagree,
  and `RuleLevelHelper::failing_level` reports a type that fails in full at
  the rule's level, in some members at 7, and only in `null` at 8. This
  replaced four separate level choices. A partly failing receiver no longer
  ends a method call's inference. (`95724106`)
- [x] **R1-2. Duplicated type knowledge.** `is_single_value`,
  `constant_ints` (all-or-nothing), `has_unknown_part`, `without` in place
  of the strip helpers, `without_call_site_variance`; the thin class-name
  wrappers and the registry's `object_class_names` removed. (`1fe63136`)
- [x] **R1-3. Checker-side type questions.** Inlined `is_non_falsy_string`
  and `int_bound`; a receiver's class counts an enum case's enum.
  (`e53d7d7f`)
- [x] **R1-4. API shape.**
  - Free functions became methods on `Type`, `Relation` or `Shape`, and
    the views use `as_*` names.
  - `as_object` counts an enum case as an object of its enum.
  - `int_bounds` counts `int`, replacing two duplicate functions.
  - Crate-internal helpers and modules are private.
  - The `Type` methods in `lib.rs` moved to their modules. (`bfb797c8`)
- [x] **R1-5. Opacity hole.** The public `union_of`, `benevolent_union_of`,
  `intersection_of` (`5fc4e990`) and `with_members` (`34fd1014`) normalize.
- [x] **R1-6. Template bounds.** Any type describing values can bound a
  template (`can_bound_a_template`). The earlier whitelist was copied from
  PHPStan and rejected `callable` and `class-string` bounds. (`e53d7d7f`)
- [x] **R1-7. Representation leak.** Branch-implied facts are keyed by a
  `Discriminant` (subject and value type), not by `Debug` text matched
  against `"True"`/`"False"`. (`eeaf448c`)
- [x] **R1-8. Readability.** Inside `crates/types` a type is built with its
  constructors; `Type(Repr::X)` is left to patterns and raw unions.
  (`4dc6a4b7`)
- [x] **R1-9. Stale comments** from the renames. (`245f6b56`)
- [x] **R1-L. Two list encodings.** A list keys on `int<0, max>` whether
  parsed or built, `equals_for_union` is gone, and a union absorbs a refined
  array into any array covering it. (`247e5226`, `34fd1014`)

## Review 2: correctness (confirmed by probe)

Each was reproduced with an `assertType` probe through `check-project`.
Each fix needs a fixture.

- [ ] **C1. Narrowing keeps coercible members.** `Relation::narrow` in
  `crates/types/src/narrowing.rs` decides a member survives with `accepts`,
  which allows argument coercion. So `is_float($x)` on `int|float|string`
  gives `int|float` instead of `float`. Use `is_super_type_of`, which knows
  the class hierarchy, contrary to the doc comment's claim; fix that comment
  too.
- [ ] **C2. A write at a non-literal key loses the array's keys and
  non-emptiness.** `set_offset_value_type` in `crates/types/src/offsets.rs`,
  shape branch and refined non-list branch. `array{a: 1}` followed by
  `$arr[$int] = 2` gives `array<int, 2|1>` instead of
  `non-empty-array<int|string, 1|2>`. The key becomes the union of the
  existing keys and the written key's type, and the array is non-empty
  after the write.
- [ ] **C3. Truthiness doesn't remove whole kinds.** `Type::without_falsy`:
  `if ($b)` leaves `bool` (should be `true`), and `if ($i)` leaves `int`
  (should be `int<min, -1>|int<1, max>`). It also misses `0.0`, a range
  that includes `0`, and `false` inside a refined string. This is the plan's
  B4 item "Truthiness removes whole members"; do it here or there, not both.
- [ ] **C4. Offset presence ignores what the type records.**
  `has_offset_value_type` doesn't answer `Yes` for an intersection
  carrying `hasOffset(k)`/`hasOffsetValue(k, _)`, and a template doesn't
  answer for its bound.
- [ ] **C5. `to_number` accepts non-numeric strings.** PHP 8 throws a
  `TypeError` for arithmetic on a non-numeric string (`'abc' + 1`), and a
  leading-numeric one warns. `Type::to_number` needs the PHP version to say
  so. Pair with version-aware work; only `castable_arguments` uses it today.

## Review 2: precision

- [ ] **P1. The union cap gives up to `mixed`.** Past `MAX_UNION_MEMBERS`
  (12), after shapes are generalized, `Type::union` returns `mixed`. So
  thirteen string literals become `mixed` rather than `string`. Generalize
  literals to their base type (and ranges to `int`) before giving up; `mixed`
  only when that still exceeds the cap.

## Review 2: structure, types crate

- [ ] **T1. One absorption rule for unions.** `union_types_impl` in
  `crates/types/src/union.rs` is a set of special cases:
  - `true|false` becomes `bool`;
  - a literal is absorbed into its base type;
  - the empty shape is absorbed into an array;
  - integer ranges are merged;
  - an extension payload is absorbed into its widened form;
  - `void` is absorbed into `null`.

  The general rule, dropping a member another strictly contains, exists as
  `Relation::absorb_union_subtypes` and only tidies one rule's message.
  Make it the union's rule, keeping merges that absorption can't express
  (ranges that touch, arrays by key set), and measure the cost: an O(n²)
  relation pass per union on a hot path.
- [ ] **T2. Refinements as intersection.** `to_non_empty_array`, `to_list`,
  `with_key_presence`, `to_isset_container` and `without_falsy` in
  `crates/types/src/narrowing.rs` each hand-write "intersect every member
  with one refinement". Express them through `Relation::intersect`,
  distributed over union members and dropping `never`; the plan's "A
  template narrows by intersection" item is the same operation.
- [ ] **T3. Union rebuilding.** Six copies of `match kept.len() { 0 =>
  never, 1 => the one, _ => raw union }` in `narrowing.rs`, plus raw unions
  in `transform.rs` (`unsealed_shapes`, `filter_array_values`). Use
  `Type::union_of`.
- [ ] **T4. `is_object_like`** in `narrowing.rs` duplicates `Type::is_object`.
- [ ] **T5. Two literal-key helpers disagree.** In `offsets.rs`,
  `literal_key_name` and `literal_key`: only the second sees through
  `to_array_key` (`true`, a float, `"1"`). Keep one, built on
  `to_array_key`.
- [ ] **T6. Redundant list arm.** The list-vs-list arm in
  `Relation::is_super_type_of` (`relation/super_type.rs`) is redundant now
  that lists have one encoding; the general intersection arms answer the
  same.
- [ ] **T7. Misplaced doc comments.** `object_super_type_of`'s doc is fused
  onto `absorb_union_subtypes`, as `resolve_conditional`'s was onto
  `conditional_branch`. Check the crate for other doc blocks sitting on the
  wrong item.
- [ ] **T8. `filter_array_values` skips intersections** (`transform.rs`). A
  `list<T>` passes through unfiltered. Harmless today, since `array_filter`
  strips refinements first, but the method's contract doesn't say so.

## Review 2: structure, syntax (B3)

- [ ] **S1. Spanned names are half done.** `Ident` (a name and its span)
  exists, yet these keep them apart:
  - `FunctionDecl`, `PropertyDecl`, `ClassConst` and `EnumCase` have
    `name` plus `name_span`;
  - `Param` has `name: Arc<str>` plus `name_span`;
  - `CatchClause` has `var` plus `var_span`;
  - `ClosureUse` has `name` plus `span`.

  Each becomes one `Ident`.
- [ ] **S2. `QualifiedName` has no span,** so `InstanceofTarget::Name`,
  `StaticTarget::Name` and `Attribute` pair it with one. Give it its span.
  The parallel vectors `CatchClause.types`/`types_spans` and
  `ClassDecl.extends`/`extends_spans`, `implements`/`implements_spans` then
  go.
- [ ] **S3. Trait uses keep their statements.** `ClassDecl` holds `uses`,
  `uses_spans`, `uses_doc_comments`, `trait_aliases`, `trait_precedences`
  and `resolved_trait_conflicts` as flat lists, losing which adaptations
  belong to which `use` statement. Replace them with one `trait_uses` list
  of `{ traits, doc_comment, adaptations }`. `resolved_trait_conflicts`
  derives from the adaptations.
- [ ] **S4. Variables are names.** `ExprKind::Variable`, `Param.name`,
  `ClosureUse.name` and `CatchClause.var` are `Arc<str>` while every other
  name is an interned `Name`.
- [ ] **S5. Promotion is one option.** `Param`'s `promoted_visibility`,
  `promoted_set_visibility`, `promoted_readonly` and `promoted_final` mean
  something only together; make them `promotion: Option<Promotion>`.
- [ ] **S6. A hook is one value.** `PropertyHooks` holds parallel fields
  (`get`, `get_by_ref`, `get_attrs`, `get_doc`, `get_final`, and the same
  set for `set`); make them `get: Option<Hook>` and `set: Option<Hook>`,
  with the `set` parameter on the hook.
- [ ] **S7. A body is optional.** `FunctionDecl.has_body` sits beside an
  always-present `body`; make `body` an `Option`.
- [ ] **S8. Nullsafe is named.** `PropertyAccess` and `MethodCall` carry
  the nullsafe flag as a bare positional `bool`; give it a name (a named
  field or an enum).
- [ ] **S9. The `"?"` sentinel survives.**
  `target.as_name().unwrap_or("?")` in `registry/global_vars.rs`,
  `checker/phpstan_api.rs` and `checker/calls/misc_rules.rs`, and
  `resolve_static_class_name`'s `class_name == "?"` check. Take the
  `Option`.
- [ ] **S10. The `class@anonymous` placeholder survives.**
  `registry/anonymous_class.rs` rebuilds it, and `checker/unused.rs`,
  `checker/resolve.rs` and `checker/expr.rs` string-match it. Key anonymous
  classes by what identifies them instead of a shared name.
- [ ] **S11. Stale syntax docs.**
  - `ClassDecl.name_span` still describes the `class@anonymous`
    placeholder;
  - `Literal::String` documents an `Arc<str>` it no longer holds;
  - `StaticTarget::as_name` narrates the removed `"?"` sentinel.

## Full review: findings

Each was found reading the commit named; "probed" means reproduced with
an `assertType` or diagnostic probe through `check-project`.

### Correctness (probed)

- [ ] **F1. `exit(...)` is typed `never`.** Since PHP 8.4 `exit`/`die`
  are functions and `exit(...)` is a first-class callable. The parser
  (`parser/primary.rs`, exit arm) drops `parse_args`' first-class-callable
  flag, so `$f = exit(...);` is `never` and the rest of the function is
  reported unreachable. Extra arguments (`exit(1, 2)`) are dropped silently,
  and the status (`string|int` since 8.4, a `TypeError` otherwise) is never
  checked. (`f9757b0a`)
- [ ] **F2. `isset($x)` declares `$x` for the rest of the function.**
  `check_existence_probe` (`checker/expr.rs`) declares a probed bare
  variable in the enclosing scope, so `if (isset($maybe)) {} echo $maybe;`
  reports nothing. Define it in the true branch only. (`6bdce4ce`)
- [ ] **F3. `unset($a['k'])` doesn't remove the offset.** The `Unset`
  statement (`checker/stmt.rs`) only checks non-variable targets as
  expressions: `array{x: 1, y: 2}` stays `array{x: 1, y: 2}` after
  `unset($b['x'])`. The offset goes (a list stops being a list, an array
  may become empty), and an unset property is uninitialized again.
  (`6bdce4ce`)
- [ ] **F4. Method names are compared case-sensitively for unused private
  members.** `$this->HELPER()` calling `private function helper()` reports
  `helper()` unused (`checker/unused.rs`, `usage.methods`). PHP method names
  are case-insensitive. (`481b224b` area)

### Correctness (from reading)

- [ ] **F5. Non-UTF-8 array keys collide.** `ShapeKey::Str` holds a
  `String`, and keys from string literals go through `to_str_lossy`
  (`shape.rs` `from_type`, `checker/assignment.rs`,
  `checker/expr/array_ops.rs`, `coercion.rs` `to_array_key`), so `"\xff"`
  and `"\xfe"` become one `U+FFFD` key where PHP has two. `ShapeKey::Str`
  holds a `PhpString`. (`3af64337`)
- [ ] **F6. A constant's bytes round-trip through lossy text.**
  `registry/function_sig.rs` `infer_const_type_with` builds
  `Type::string_literal(s.to_string().into())`; use `s.clone()`.
  (`3af64337`)
- [ ] **F7. A string literal's type text is ambiguous.** `display.rs`
  prints `'{s}'` lossily and unescaped: `"\xff"` and `"\xfe"` both print
  `'�'` (the `string_literals_are_bytes` fixture shows it), and `it's`
  prints `'it's'`. Escape `'` and `\`, and show non-UTF-8 bytes as `\xNN`
  in a double-quoted form. (`3af64337`)
- [ ] **F8. `mb_*` case functions fold ASCII only.**
  `infer_case_function_call` (`checker/expr/builtin_calls.rs`) folds a
  literal for `mb_strtolower`/`mb_strtoupper`/`mb_lcfirst`/`mb_ucfirst`/
  `mb_convert_case` with ASCII rules: `mb_strtolower('ÉCOLE')` gives
  `'École'`, PHP gives `'école'`; `MB_CASE_TITLE` isn't folded. Use Unicode
  case mapping for the `mb_` family, or don't fold non-ASCII input.
  (`3af64337`)
- [ ] **F9. Escapes are decided per heredoc flag, not per delimiter.**
  `scan_interpolated` takes `heredoc: bool`, but PHP decodes by delimiter:
  in `"…"` `\"` is `"`; in `` `…` `` `` \` `` is `` ` `` and `\"` stays
  `\"`; in a heredoc both stay. Backtick strings decode `\"` and keep
  `` \` `` as two bytes. The printer's `ShellExec` arm escapes with
  double-quote rules. (`3af64337`, `75d3096f`)
- [ ] **F10. `08` is read as 8.** A leading-zero literal with a non-octal
  digit is a PHP parse error ("Invalid numeric literal"); the lexer reads
  it as decimal. (`8f5b2af7`)

- [ ] **F29. Integer overflow crashes the checker.** Each of these aborts
  checking the file ("internal error while checking: attempt to … with
  overflow"); a release build wraps silently to the wrong literal instead:
  - `-$m` for `$m = PHP_INT_MIN` (`arithmetic.rs` `negate_numeric_literal`
    and `-m` on range bounds; PHP gives a float);
  - `$m % -1` (`mod_int_range` `l % r`; PHP gives `0`);
  - `$i++` at `PHP_INT_MAX` (`transform.rs` `step`, `n + by`; PHP gives a
    float).

  Audit every i64 operation in `arithmetic.rs` and `transform.rs` (negation,
  remainder, `MIN / -1`, `m - 1` at `MIN`); an overflowing result is a
  float, as PHP makes it. (`d0d331dd`, `b62a1131`, `fd5c800e`)
- [ ] **F30. A modulo by a negative divisor is `never`.**
  `int<0, max> % -3` infers `never`; PHP gives a value in `[0, 2]`.
  `mod_int_range`'s range fallback uses the divisor's signed maximum
  (`r_max - 1`) where it needs its magnitude over both bounds.
  (`b62a1131`)
- [ ] **F31. Bitwise operators on strings are typed `int`.** `$a ^ $b`,
  `$a | $b` and `$a & $b` over strings give a string in PHP (bytewise;
  `'a' ^ "\x02"` is `'c'`). `binary_operation_type`'s bitwise arm returns
  `int`. (`615ad35d`)
- [ ] **F32. `++` on a string keeps its value.** `$s = 'a'; $s++;` stays
  `'a'` where PHP gives `'b'`: `step` leaves a string unchanged instead of
  applying PHP's string increment (alphanumeric carry; a numeric string
  steps as a number). `null++` gives `int`; PHP gives exactly `1`.
  (`fd5c800e`)
- [ ] **F33. A string written as an array key is taken as a string key.**
  `$b = []; $b[$s] = 1;` with `$s: string` infers `array<string, 1>`, but
  PHP turns a numeric string into an int key (`["5" => 1]` has key `5`).
  The literal form `[$s => 1]` gets `int|string`, so the write path
  (`write_key_type`) and `to_array_key` (a non-literal string passes
  through) disagree with it. A string key is `int|string` unless provably
  non-numeric; a decimal-int-string is `int`. The lost non-emptiness is C2.
  (`323eeb36`, `e024c0bc`)
- [ ] **F34. Unknown classes in generic arguments and callable signatures
  go unreported.** `@param Box<NoSuchA>` and `@param callable(NoSuchB):
  void` give no `class.notFound` (`array<NoSuchC>` and `NoSuchD` do).
  `Type::referenced_class_names` walks only array, class-string,
  union/intersection and shape positions, while `nested_class_names`
  walks every child. The unknown-class, trait, parent and internal-prefix
  checks use the partial one. Keep one walker over every child
  (`has_parent_ref` too). (`3ac29a9a`)
- [ ] **F35. A non-final class is treated as final.** For a non-final
  `class Foo` whose subclass `Bar` implements `IteratorAggregate`,
  `ArrayAccess` and `__invoke`, three things are reported on `$foo`, yet
  `f(new Bar())` runs:
  - `foreach ($foo …)`: `foreach.nonIterable`;
  - `$foo()`: `callable.nonCallable`;
  - `$foo['k']`: `offsetAccess.nonOffsetAccessible`.

  `relation/kinds.rs` `class_is` answers No for any known non-interface
  class that isn't a subclass, and `is_callable`'s missing `__invoke` does
  the same. Likewise `function g(stdClass $s) { $s->foo(); }` reports
  "call to method foo() on a non-object type stdClass": `can_call_methods`
  says No for `stdClass`, which isn't final, and the message calls an
  object a non-object. No only for a final class or an enum, Maybe
  otherwise. (A `foreach` over any object also iterates its visible
  properties, so even a final class's No for iterability is doubtful.)
  (`9819e786`, `3bdb7505`)

### Precision (probed)

- [ ] **F11. `isset`/`empty` narrow one branch only.** `!isset($i)` on a
  defined `?int` leaves `int|null` (should be `null`); `empty($j)` true
  leaves `int|string|null` (should be `0|''|'0'|null`). For a single
  operand that is a defined variable, both branches narrow. (`6bdce4ce`)

- [ ] **F22. `=== SomeEnum::Case` against an interface is "always
  false".** With `Suit implements HasColor`, `$x === Suit::Hearts` for `$x:
  HasColor`, `UnitEnum` or `HasColor|int` reports `identical.alwaysFalse`.
  The `(Object, EnumCase)` arm in `relation/super_type.rs` calls
  `object_super_type_of`, which only answers the not-a-subclass cases (it
  relies on `proves_super_type` having returned Yes first) and returns No
  for an interface against a final class (an enum) implementing it. The arm
  also turns a Yes into Maybe, so `HasColor ⊇ Suit::Hearts` is never Yes. A
  case relates to a class exactly as its enum's object type does; fold the
  subclass Yes into `object_super_type_of` so no caller can skip it.
  (`9355e811`)
- [ ] **F23. Deep nesting crashes the process.** 200,000 nested `{ }`
  blocks overflow the parser's stack and abort the whole run ("has
  overflowed its stack", exit 134). The depth guard (`MAX_EXPR_DEPTH`)
  covers expressions only; statement nesting (blocks, control-structure
  bodies, nested declarations) needs it too. No file may take the run
  down. (`16dd573b`)
- [ ] **F24. Property-hook bodies are skipped by "all code" walks.** A
  banned `eval()` in a `get { }` hook isn't reported, while the same call
  in a function is (`project/banned.rs`). `composer_hygiene.rs`'s `Facts`
  skips hooks and parameter defaults on purpose, so `extension_loaded()`
  and `function_exists()` probes in a hook are missed. Hook bodies are
  code; audit every walker that claims to see all code (see F27).
  (`0e294c5b`)

### Structure

- [ ] **F12. Two name-resolution paths.** The resolve pass fills
  `QualifiedName::resolved`, yet about 59 sites re-resolve written names
  through a `NameContext`: the checker's `name_ctx.resolve`,
  `resolve_function_candidates` and `resolve_constant_candidates`, and the
  registry's `ctx.resolve` for attributes, access attributes, reflection
  attributes, trait-alias trait names (`class_info.rs`), `phpstan_api.rs`
  and `member_usage.rs`. AST names are read from `resolved` (the
  function/constant global fallback belongs in `ResolvedName`); a
  `NameContext` stays only for PHPDoc text. `unused/rebind.rs` matches
  `Closure` on the written name, missing `\Closure` and aliases.
  (`271fa92a`, `c935b1be`)
- [ ] **F13. Declarations' namespaced names are computed and ignored.**
  `namespaced_name` is read only by xtask's stub index. The registry
  (`class_info.rs` `ctx.declared_name`, `index.rs`'s namespace walk,
  `global_vars.rs`, `resolve.rs` `declared_name_in_namespace`, which
  replicates `NameContext::declared_name`) and about 12 project and LSP
  walkers derive it again, each with its own namespace and `use` tracking.
  One iterator over a file's declarations, yielding each with its
  namespaced name and whether it is conditional, replaces them; it also
  removes the `Block(s) | Declare { body: Some(s) }` arm repeated at about
  20 sites. (`47f973e9`, `cd23db61`)
- [ ] **F14. Names leave the project's interner.** AST names are `Name`s
  from the project's `Interner`, but the registry re-interns them as
  `Arc<str>` through `syntax::intern::intern`, the process-global interner
  (93 call sites in `registry/`), as do variable names in the checker and
  LSP. `name.rs` promises a project's names are freed with it; in a
  language-server session they never are. Registry keys become `Name`
  (keyed by `lower()` where PHP is case-insensitive). The parser also
  interns each name twice (`interner.str`, then `interner.name`).
  (`481b224b`, `b7c04ec9`)
- [ ] **F15. "Is this the constructor" is written 12 times.**
  `name.eq_ignore_ascii_case("__construct")` in `checker/oop.rs`,
  `unused.rs`, `generics/variance.rs`, `oop/property_hooks.rs`,
  `oop/property_overrides.rs`, `class_info.rs` (3), `doctrine_entity.rs`,
  `noop.rs`, `function_decl.rs`. One `ClassDecl::constructor()`.
  (`481b224b`)
- [ ] **F16. The enum/class merge is nominal.** 16
  `ClassDecl(e) if e.kind == ClassKind::Enum` arms sit ahead of the class
  arm and duplicate it: the checker's top-level check list (about 80 lines,
  twice), `stmt.rs`, `phpstan_api.rs` `check_enum`, the LSP and use-site
  `enum_decl` walkers, `anonymous_class.rs` `walk_enum`, and more. The
  registry keeps two builders (`from_enum_decl`/`from_decl`,
  `MemberSpans::of_enum`/`of_class`, `BuiltDecl::Enum`/`Class`), and the
  parser two body loops (`parse_enum_decl`/`parse_class_body`). Use one
  class-like arm that varies by kind where PHP does. An enum's `ClassDecl`
  also says `is_final: false`, but an enum is implicitly final.
  (`8e373302`)
- [ ] **F17. Destructuring targets are two node kinds.** `[...]` on the
  left is an `ArrayLiteral` and `list(...)` a `List`, so about 15 sites
  match `ArrayLiteral(items) | List(items)`. Make an assignment or
  `foreach` target its own pattern node, keeping `ArrayLiteral` for values;
  the parser then rejects `list()` as a value (the checker's `List` arm now
  returns `mixed`). (`f9651cf4`)
- [ ] **F18. Transparent wrappers are matched by hand.**
  `Grouping(inner) | ErrorSuppress(inner)` appears at about 30 sites
  instead of one peel helper. `unwrap_grouping` also peels `@` but keeps
  the grouping-only name. Nothing reads `ErrorSuppress`. (`5d93a01c`)
- [ ] **F19. Variable names have two representations.** `StaticVar` and
  `GlobalVar::Name` are `Ident`s (a `Name`), while `ExprKind::Variable` is
  `Arc<str>`, so call sites convert through the global interner. Extends
  S4. `strict_rules.rs` still gives a static variable the statement's span.
  (`b7c04ec9`)
- [ ] **F20. `exit` records its form by span width.** `banned.rs` tells a
  bare `exit` from `exit()` by measuring the span; the node doesn't record
  it. (`f9757b0a`)

- [ ] **F25. The error type is produced by nothing.** `Type::error()`
  (`*ERROR*`, "for expressions a rule reports as invalid") has union,
  relation and accepts arms, but no rule or inference returns it. Either
  the checker returns it for an expression it reports invalid (undefined
  variable, method or property; invalid operands), so one error doesn't
  cascade, or the variant goes. (`6fe620f7`)
- [ ] **F26. `visit_name` misses type-hint names.** Its doc promises every
  class, function and constant name, but `visit_type_hint` is a leaf with
  no `walk_type_hint`. `TypeHintKind::Name` also holds keywords (`int`,
  `static`, `self`) and class names alike; a keyword type should be its own
  variant. (`0b579ab1`)
- [ ] **F27. Hand-rolled traversals remain.** `any_child_expr` and
  `any_child_stmt` serve as a for-each with an always-`false` closure at 21
  sites (a `for_each_child_*` is missing). Twelve files still walk children
  with their own per-node-kind handling:
  - `lsp/navigation.rs`, `lsp/signature_help.rs`, `project/use_sites.rs`;
  - `unused/member_usage.rs`, `unused/var_usage.rs`;
  - `checker/phpstan_api.rs`, `registry/purity.rs`, `project/banned.rs`;
  - `includes.rs`, `oop.rs`, `function_like_declarations.rs`,
    `uninitialized_dataflow.rs`.

  F24 is the kind of miss this produces. Each becomes a `Visitor` that
  overrides only what it special-cases. Comments renamed mechanically still
  describe the old API ("`any_child_expr` deliberately returns no
  children", "the generic `Vec<&Expr>` shape"). (`167ff73d`)
- [ ] **F28. Minor syntax and walker gaps.**
  - A second `default` match arm is a PHP compile error; it's accepted,
    and `match_default` takes the first (`bc28373d`).
  - The new `NodeId`s on declarations and clauses are read by nothing, and
    13 synthesized nodes share `NodeId::SYNTHETIC`, so a side table keyed
    by id would conflate them (`158f843b`).
  - `rebind.rs` compares `bind`/`bindTo` case-sensitively and misses
    `$closure->call($obj)`, which also rebinds scope (`786f6639`).
  - `annotations.rs` walks an arrow-form hook body outside
    `function_scope`, so a class declared in a `try` gives its `get => …`
    hook the enclosing catch types (`34571a3c`).
  - `global_vars.rs`: `handle_top_expr_statement`'s doc was deleted, and
    `walk_function_body` takes `&[&Stmt]`, so every caller allocates
    (`f4c6eb9f`).
  - `Relation::intersect` leaves `list & array{a: int}` as an intersection
    instead of `never`: `ArrayList` isn't applied to a shape (`102b907f`).

- [ ] **F36. Structural leftovers from the "Type behaviour lives on the
  type" series.**
  - `Type::is_within` is public and general but uses
    `Relation::structural()` (blind to the class hierarchy), and answers
    `never ⊆ X` as No where the relation says Yes. Every caller passes a
    kind; make it the kind predicates' private helper (`b489dd00`).
  - `Repr::Conditional` has `subject: String` plus two positional bools
    (`negated`, `template_subject`), and `Type::conditional` takes them
    positionally. The subject should be an enum (template or parameter, by
    `Name`). `ReflectedMember`'s member and `ClassConstant`'s pattern are
    `String`s where names are `Name`s (`5798014b`).
  - A `Closure(...)` signature is the two-member intersection
    `[Object("Closure"), Callable(sig)]`, recognized by pattern:
    `map_class_names` special-cases it so name resolution doesn't rename
    `Closure`. `registry/name_context.rs` skips resolving `"Traversable"`
    by name so `iterable`'s member isn't namespaced, which misresolves a
    user class named `Traversable`. Give both their own representation, or
    make the names fully qualified (`8437f51f`).
  - Type equality and hashing compare union and intersection members as
    ordered vectors, so `int|string` differs from `string|int`; every
    `==` between types is order-sensitive. Keep members in a canonical
    order, or compare as sets (`096812c5`).
  - `types::arithmetic` takes `syntax::ast::BinOp`, tying the types
    crate's API to the AST's operator enum (`b62a1131`).
  - `failing_level` splits only top-level union members, so a template
    bounded by a union is judged whole (Maybe, nothing reported). The
    property-access arm uses an empty then-branch
    (`if non_object_level == Some(0) {} else if …`) (`95724106`).

### Comments

- [ ] **F21. Stale or misplaced comments.**
  - `lexer/strings.rs` heredoc branch still calls positions "an honest,
    coarser approximation"; offsets now map exactly (`0c4c62b3`). A
    `{$…}` fragment spanning heredoc lines is re-lexed from stripped text,
    so its later lines' columns are short by the indent.
  - `parser/tokens.rs:242`: a `//` block about qualified names sits on
    `take_name_segment`, fused with its doc (`b7c04ec9`).
  - Nonsense from the enum rename (`8e373302`): "`ClassDecl` is its own
    AST node, not a `ClassDecl` with `ClassKind::Enum`" (`duplicates.rs:24`),
    "`ClassDecl` and `ClassDecl` are different AST types" (`oop.rs:542`),
    "`ClassDecl`/`ClassDecl`" (`unused.rs:141`, `navigation.rs:25`,
    `phpstan_api.rs:1072`, `stmt.rs:860`, `stmt.rs:974`, `oop.rs:188`).
  - After the relation merge (`91bc9e49`), comments renamed mechanically
    from `subtype` state falsehoods:
    - "`is_super_type_of`'s int-to-float widening" (`union.rs:386`,
      `narrowing.rs:197`), while the relation answers Float ⊇ Int = No;
    - `subtype.rs:92` still has the int-to-float paragraph above an arm it
      no longer describes;
    - "`is_super_type_of`'s Object handling is name-equality only" (or has
      no class-hierarchy awareness), in `decide.rs:99`, `decide.rs:178`,
      `type_resolution.rs:110`, `narrowing.rs:120`, `narrowing.rs:128`,
      `accepts.rs:240`, `accepts.rs:262` and
      `registry/tests/function_signatures.rs:512`; only
      `Relation::structural()` lacks the hierarchy;
    - comments with swapped argument order.
  - `normalize_generic_args` (`a7f5bc27`) repairs raw unions inside a
    top-level object's generic arguments on every union call; it goes with
    T3.

  - The comment rewrite to type language (`245f6b56`) left broken or false
    text:
    - "whose declared type is conditional type and" (`checker.rs:203`);
    - "the coarse `Closure`| -" (`pipe_operator.rs:13`);
    - "a literal integer literal" (`restricted_symbol_usage.rs:138`);
    - "True`/`false` matches", with broken backticks (`narrowing.rs:294`);
    - "a already-array-shaped" (`missing_typehint.rs:379`);
    - a citation of `Type::equals_for_union`, deleted in `247e5226`
      (`control_flow.rs:689`);
    - "an enum case's enum case" (`registry/resolve.rs:1284`);
    - "object directly", "reuses object's existing slot", and "reflected
      member's own doc comment", a doc that no longer exists
      (`member_resolution.rs:173`, `:535`, `:622`, `:631`, `:806`);
    - "conditional type/`resolve_conditional`" (phpdoc `docblock.rs:807`,
      `docblock/tests/param_and_return_tags.rs:283`).

    Re-read every comment that commit touched.

## Full review: coverage

Every Stage B commit's full diff (`crates`, `xtask`), read in order. A
commit is ticked once its whole diff has been read and its findings added
above.

- [x] `26f2267b` fix: dollar-brace interpolation ("${name}", "${name[expr]}", "${expr}") reads the varia...
- [x] `8f5b2af7` fix: a legacy octal literal (0775) is octal, 509, and a hex, binary or octal literal pa...
- [x] `3af64337` fix: string literals are bytes
- [x] `fe4db52d` fix: keywords fold case everywhere
- [x] `e27cb3f8` fix: a leading #! line is skipped as the CLI skips it, so declare(strict_types=1) after...
- [x] `ffdd7d8c` fix: concatenation takes its PHP 8 precedence, looser than +/- and <</>>, so "sum: " . ...
- [x] `7b2fd49e` fix: an assignment is an operand wherever an operand stands, by one grammar rule applie...
- [x] `c935b1be` refactor: trait adaptations keep the insteadof choice and the as visibility
- [x] `a4691beb` refactor: a promoted parameter keeps its (set) visibility and a &get hook is recorded a...
- [x] `e3e3afc6` fix: $obj->$$name and $obj->${expr} keep both levels of indirection
- [x] `0c4c62b3` fix: every part of an interpolated string spans its own text
- [x] `4fbe8a7c` refactor: a string literal token moves into Literal::String without a conversion, and a...
- [x] `271fa92a` fix: names resolve where they sit, not by the file's last namespace
- [x] `47f973e9` refactor: declarations carry their namespaced name
- [x] `481b224b` refactor: declaration and member names are interned Names
- [x] `9742818c` refactor: an anonymous class has no name
- [x] `b7c04ec9` refactor: named arguments, static and global variables, goto labels and use imports are...
- [x] `cd23db61` refactor: a declare statement holds its block
- [x] `75d3096f` refactor: a backtick command is its own node
- [x] `f9757b0a` refactor: print, exit/die, eval and include/require are their own nodes instead of call...
- [x] `6bdce4ce` refactor: isset, empty and unset are their own nodes
- [x] `f9651cf4` refactor: list() is its own destructuring node
- [x] `5d93a01c` refactor: the @ operator is kept as ExprKind::ErrorSuppress instead of dropped
- [x] `8e373302` refactor: an enum is a ClassDecl of kind Enum
- [x] `bc28373d` refactor: match arms are spanned MatchArm nodes in written order, the default arm among...
- [x] `158f843b` refactor: class-likes, methods, properties, constants, enum cases, parameters, argument...
- [x] `16dd573b` fix: an expression nested past the depth limit is a ParseError, not a panic
- [x] `b4939433` refactor: one parse entry point
- [x] `0b579ab1` feat: the visitor gains visit_name, called for every class, function and constant name ...
- [x] `2b00a8d4` refactor: the side-effect, hidden-effect, keyword, declare, magic-constant, member-defa...
- [x] `854fdd86` refactor: the explicit-throw, parent-call, anonymous-class and nested-define walkers ar...
- [x] `786f6639` refactor: the promotion, assigned-variable, property-assignment, self-instance and rebi...
- [x] `a2cb7a42` refactor: the purity walkers recurse through callback walks, and the include-constant, ...
- [x] `f4c6eb9f` refactor: the global-variable scope walk is a visitor, and the backing-storage and pare...
- [x] `0e294c5b` refactor: the composer-fact and banned-code walks are visitors, and the architecture ne...
- [x] `167ff73d` refactor: the allocating child_exprs, child_stmts and stmt_exprs helpers are gone
- [x] `34571a3c` fix: an arrow function's body is outside the enclosing try's catch types, as a closure'...
- [x] `91bc9e49` refactor: one subtyping relation
- [x] `ab8df0b0` fix: an intersection is within whatever one of its members is, ahead of the void and ne...
- [x] `9355e811` fix: an interpolated string is its parts' string forms concatenated, a bare return coun...
- [x] `54fea123` fix: an isset or empty condition stored in a variable narrows when the variable is test...
- [x] `0e309c03` refactor: a conditional's two branches and a late key-of or value-of join through union...
- [x] `a7f5bc27` fix: a union drops never, reads true|false as bool and lets bool absorb either, and ded...
- [x] `510a7ecb` fix: a union with an explicit mixed stays explicit, in PHPDoc too, so a property declar...
- [x] `56c6e420` fix: a union merges array shapes only when neither loses a key's meaning, so tagged uni...
- [x] `102b907f` fix: an intersection reduces by the class hierarchy and folds shapes, so a class and it...
- [x] `6fe620f7` feat: an error type, displayed *ERROR*, that accepts and is accepted by everything and ...
- [x] `6429c47e` chore: an arch ratchet on paths naming a types::Type variant outside crates/types, coun...
- [x] `4362acc7` chore: the type-access baseline splits its total into patterns, comparisons and constru...
- [x] `5798014b` refactor: every type built outside crates/types goes through a Type constructor (Type::...
- [x] `089dae96` refactor: whether a type is mixed, explicit or not, is asked through Type::is_mixed/is_...
- [x] `a6dd2cc7` refactor: a union's members are Type::union_members, replacing four checker copies of f...
- [x] `b489dd00` refactor: the kind predicates of PHPStan (is_null, is_bool, is_int, is_float, is_string...
- [x] `20680eff` refactor: array and truthiness questions are Type methods
- [x] `56e1f538` refactor: concatenation, string refinements and integer bounds live in crates/types
- [x] `b62a1131` refactor: integer-range and comparison arithmetic moves to types::arithmetic
- [x] `e2bccf45` refactor: narrowing's type operations move to types::narrowing
- [x] `6acf61e9` refactor: generalize and finite_types are Type methods, replacing two disagreeing gener...
- [x] `325a8889` refactor: operand validity asks the kind predicates and whether the relation proves an ...
- [x] `323eeb36` refactor: array literal keys go through Type::to_array_key and a spread merges a single...
- [x] `3ac29a9a` refactor: the class names a type refers to come from Type::referenced_class_names and h...
- [x] `0b385315` refactor: deciding a declared type from its PHPDoc and native parts moves to types::rec...
- [x] `9819e786` refactor: whether a type can be iterated, offset-accessed, destructured or called is a ...
- [x] `e024c0bc` refactor: writing an array offset and asking whether one exists move to Type::set_offse...
- [x] `df967fc1` refactor: reading an array offset is Type::offset_value_type, replacing index_result_ty...
- [x] `3bdb7505` refactor: whether a value can access properties, call methods or name a class is a Rela...
- [x] `d0d331dd` refactor: null-exclusion, array certainty and array-key legality ask the type's questio...
- [x] `615ad35d` refactor: the result type of arithmetic, bitwise, shift and spaceship operators moves t...
- [x] `9046b99f` refactor: receiver classes come from object_class_names, literal method and constant na...
- [x] `f6d6b1e3` style: drop needless borrows of a resolved instanceof class name
- [x] `fd5c800e` refactor: binding static, unsealing shapes and stepping by ++/-- are Type methods (bind...
- [x] `20ee94a0` refactor: gettype/get_debug_type answer from the kind predicates, and array_filter/arra...
- [x] `3881d045` refactor: built-in call return types read literals, objects and shapes through int_lite...
- [x] `56c3beab` refactor: the native declaration a type can be written as moves to types::native_declar...
- [x] `dbe500c5` refactor: the missing-typehint walks move to Type methods (iterable_types_missing_value...
- [x] `e940abd8` refactor: psr3, no-op, strict-idiom, unresolvable-PHPDoc and banned-type checks ask Typ...
- [x] `31a2fd14` refactor: the template variance walks become Type::referenced_templates with PositionVa...
- [x] `1fe021dc` refactor: registry resolution, var-tag, generator, offset-assignment, narrowing, castab...
- [x] `7a2938f1` refactor: tests, Doctrine result shapes, mysqli, control flow and expression inference ...
- [x] `8437f51f` refactor: parameter-type resolution, foreach, restricted-symbol, instanceof, global-con...
- [x] `71cd4d4a` refactor: the remaining checker, registry, LSP and project code outside member lookup a...
- [x] `347ae90f` refactor: member lookup and method-call return inference ask Type (Type::instance_class...
- [x] `096812c5` refactor: the Type enum becomes opaque
- [x] `95724106` fix: a union answers the usage questions as PHPStan does (members agree or Maybe, null ...
- [x] `1fe63136` refactor: one answer per type question
- [x] `e53d7d7f` refactor: a template can be bounded by any type describing values (Type::can_bound_a_te...
- [x] `bfb797c8` refactor: the types crate's API is methods on what they ask about
- [x] `5fc4e990` refactor: the public union_of, benevolent_union_of and intersection_of normalize, so no...
- [x] `eeaf448c` refactor: branch-implied facts are keyed by a Discriminant (subject and value type) ins...
- [x] `4dc6a4b7` refactor: inside crates/types a type is built through its constructors (Type::int(), Ty...
- [x] `245f6b56` docs: comments follow the review's renames
- [x] `247e5226` refactor: a list has one encoding, its array member keyed on int<0, max> wherever it's ...
- [x] `34fd1014` fix: a union absorbs a refined array member into any present array that covers it, not ...

## Deferred to C8 (reflection)

- [ ] **D1. PHPDoc/native reconciliation needs the hierarchy.**
  `Type::reconcile_with_native` (`crates/types/src/decide.rs`) runs before
  classes are indexed, so every object member is trusted or never re-added,
  and `Registry::drop_doc_members_excluded_by_native` corrects it
  afterwards. Reconcile once, with the hierarchy.
- [ ] **D2. Member lookup** still walks unions and intersections in
  `checker/expr/member_resolution.rs`; it moves to the types side with C8's
  reflection.
