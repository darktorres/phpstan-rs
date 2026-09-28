# Stage B review: findings

Findings from reviewing Stage B's follow-ups: the B3 syntax items, the B4
type items before "Type behaviour lives on the type", that item itself, and
the fixes from its first review. Judged on runtime correctness and design,
not parity with PHPStan. Each task names where the problem is, what is
wrong, and what done looks like.

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

## Deferred to C8 (reflection)

- [ ] **D1. PHPDoc/native reconciliation needs the hierarchy.**
  `Type::reconcile_with_native` (`crates/types/src/decide.rs`) runs before
  classes are indexed, so every object member is trusted or never re-added,
  and `Registry::drop_doc_members_excluded_by_native` corrects it
  afterwards. Reconcile once, with the hierarchy.
- [ ] **D2. Member lookup** still walks unions and intersections in
  `checker/expr/member_resolution.rs`; it moves to the types side with C8's
  reflection.
