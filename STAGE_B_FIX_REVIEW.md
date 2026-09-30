# Stage B fix review: findings

Findings from reviewing the work done on the Stage B review
(`STAGE_B_REVIEW.md`): every commit from its F23 fix (`041fcc46`) to
`31d6c5a7` and the five after it (`d4ae7678`, `5400b17b`, `96027afd`,
`9cef98e6`, `3cea2a20`), the thirteen code commits made while this
review ran (`9cfad246` to `783727da`) and `cc4e6b5e` after them, and
every item it marks done.
Judged on
runtime correctness and design, not parity with PHPStan. Each task names
where the problem is, what is wrong, and what done looks like.

The review reads every commit's diff in order, checks each done item's
claim against the code, and reads the modules the fixes reshaped in their
final state; its coverage is tracked at the end, and its findings are the F
tasks.

Each open finding carries a "Design" paragraph: the fix worked out
against the code at `3eaddbd1`, before any of it is built, naming the
functions it changes, the approach and the fixtures that pin it.

Legend: `[x]` done (commit given), `[>]` moved to the crate-split plan
(where given), `[ ]` open.

## Validation

- [ ] **V1. The fixture suite and corpus** are the Stage B review's V1 and
  V2; a failure or delta traced to a commit below is judged here.

## Full review: findings

Grouped by severity, most severe first. Each names the commit that
introduced it or, for a module read in its final state, the module;
"probed" means reproduced with an `assertType` or diagnostic probe
through `check-project`.

### Crashes and false positives (probed)

- [x] **F1. A template bound in a namespace resolves twice** (`9cef98e6`,
  with `3cea2a20`'s namespaced fixture). Since
  `ac00409c` a type's traversal reaches a template's bound and default,
  and `declared_type` (`a70a5b42`), `function_sig.rs:611-613`,
  `generics.rs:67-73`, `class_info.rs:1126`/`1213` and `asserts.rs:114-118`
  run `mark_template_params` - whose bounds are already resolved - and then
  `resolve_names_in_type` over the whole type. In `namespace N;`,
  `@template T of Foo` becomes `N\N\Foo` and `T of \DateTimeInterface`
  becomes `N\DateTimeInterface`: every signature using `T` reports
  `class.notFound` at level 0, and `f($foo)` returns `N\N\Foo`. Method and
  class templates alike. The pre-window binary reports none of it. The
  fixture only covers the global namespace, where resolving twice changes
  nothing. Done: a bound is resolved once - either
  `mark_template_params` takes unresolved bounds, or
  `resolve_names_in_type` leaves a template parameter's bound and default
  alone - with a namespaced fixture.

  `map_class_names` now leaves a template placeholder as it is, and a
  callable's own templates are still renamed in its template list and
  rebound into its parameters. Probed after the fix: function, method,
  class, defaulted, chained (`V of K`), callable and `@phpstan-assert`
  templates in a namespace all resolve once.
- [ ] **F2. A first-class callable on a generic receiver leaks the class's
  templates** (`b3412c22`). `first_class_callable_type` builds the closure
  from the method's declared signature and resolves only `self`/`static`,
  so for `Box<int> $b`, `$b->get(...)` is `Closure(): T` and `$g()` is `T`.
  `h($g())` with `h(int)` reports "expects int, T given". The pre-window
  binary gave a plain `Closure`, so there was no report. Done: the
  receiver's class template bindings are substituted into the signature
  (as a method call's return is) before `with_own_templates`, for static
  calls too.

  Design: the substitution belongs in `first_class_callable_target`
  (`checker/expr.rs`), not in `first_class_callable_type`, because the
  callback path in `infer_closure_return_with_param_overrides` (its
  `ExprKind::FirstClassCallable` arm near `expr.rs:5073`) reads the same
  target and so gives `array_map($b->get(...), …)` the same `T`.
  `FirstClassTarget::sig` becomes a `Cow<'a, FunctionReflection>`, and
  it carries the declaring `owner` next to `class`. The method-call arm
  keeps the receiver's `type_args` from `as_object()`, looks the method
  up with `lookup_method_with_owner` and runs
  `substitute_class_templates(sig, class, type_args, owner)`, as
  `generic_object_method_return_type` does. A union receiver builds one
  closure per member that has the method and unions them, rather than
  taking `find_method_receiver_class`'s first hit. The static-call arm
  does the same when the target is an expression (`$b::get(...)`: infer
  it and keep its type arguments). For a class-name target (`Box::get`)
  the class's templates erase through `substitute_class_templates` with
  no arguments (default, then bound). `self::`, `static::` and
  `parent::` keep the placeholders, since they are the enclosing class's
  templates in scope. `with_own_templates` then sees only the method's
  own templates. Its scope test should compare against the scope
  `build_template_params` gave them rather than the bare method name
  (F22's third bullet; the same edit). Fixtures: `$b->get(...)` on
  `Box<int>` is `Closure(): int`, `$g()` passed to `h(int)` is clean,
  `array_map($b->get(...), $xs)` maps to `int`, and a union receiver
  `Box<int>|Box<string>` gives `Closure(): int|Closure(): string`.
- [ ] **F3. `foreach` over a generic `IteratorAggregate` yields template
  names.** `iterable_element_type` (`loops.rs:493-517`) takes
  `getIterator()`'s declared return without the receiver's bindings.
  - A value typed `IteratorAggregate<int, string>` iterates as
    `TKey`/`TValue`. This is a regression from `321d388c`: `Traversable`
    bindings are now found for the stub's `Traversable<TKey, TValue>`,
    where before the code fell through to `agg_bindings`.
  - A class `Coll<K, V>` implementing `IteratorAggregate<K, V>` iterates
    as `K`/`V`, so `h($k, $v)` reports "expects int, K given". This was
    already the case before the fix window, but the commit rewrote the
    function and left it in; its new gate
    `agg_bindings.is_empty() || !(both mixed)` only catches `mixed`.

  Done: the declared return has the receiver's bindings substituted, or
  non-empty `agg_bindings` win. Fixtures cover both shapes.

  Design: in `iterable_element_type`'s `IteratorAggregate` arm, look
  `getIterator` up with `lookup_method_with_owner(name, "getIterator")`.
  Run `substitute_class_templates(sig, name, args, &owner)` on it, then
  `resolve_pseudo_at(return, &owner, name)`. This is the path a
  `$x->getIterator()` call's return already takes through
  `generic_object_method_return_type`, so the loop and the call agree.
  Both bullets then come out right: the stub's `Traversable<TKey,
  TValue>` becomes `Traversable<int, string>`, and `Coll<K, V>`'s
  `Iterator<K, V>` becomes its receiver's arguments. When the receiver
  has no arguments, the erasure gives the default or bound, as for a
  method call. The gate becomes: the substituted return wins unless it
  resolves to `mixed` for both key and value while `agg_bindings` (now
  substituted too) knows better. The self-recursion guard compares only
  the returned class with `name`. Replace it with a visited set of class
  names threaded through the recursion (a private
  `iterable_element_type_in(&self, ty, seen)`), so `A::getIterator(): B`
  with `B::getIterator(): A` also stops. Fixtures:
  `IteratorAggregate<int, string>` iterates as `int`/`string`;
  `Coll<int, string>` implementing `IteratorAggregate<K, V>` with
  `getIterator(): ArrayIterator<K, V>` passes `h(int $k, string $v)`
  clean; a bare `Coll` iterates as the bounds; a `getIterator():
  Generator` that yields keeps the generator's types.
- [ ] **F4. A type check that can't pass narrows to the checked type.**
  `Relation::narrow` falls back to `target` when `original` and `target`
  are disjoint (`narrowing.rs`, the `_ => target.clone()` arm and the
  union arm's `0 => target.clone()`). The resulting type merges back after
  the `if`.
  - Since `f18bc022` (subtyping instead of acceptance), `is_float($int)`
    is `float` in its true branch, and after `if (!is_float($i)) {...}`
    `$i` is `int|float`. The pre-window binary kept `int`.
  - Already the case before the window: `if (is_string($j)) {}` makes
    `$j` `string|int`, so `h($j)` reports "expects int, string|int
    given"; `is_int(?string)` gives `strlen()` the same false positive.

  The check itself already reports `function.impossibleType`. Done: the
  disjoint case is `never` in both arms, with a fixture asserting the type
  after the `if`.

  Design: `Relation::narrow` (`types/src/narrowing.rs`) becomes a
  per-member intersection. For each member `m` of `original` (one member
  for a non-union) and each member `t` of `target`, the result is:

  - `m` when `m` is a subtype of `t`;
  - `t` when `t` is a subtype of `m`;
  - otherwise `self.intersect(vec![m, t])`, which is `never` for disjoint
    scalars and the overlap for ranges (`int<0, 10>` narrowed to
    `int<5, 20>` is `int<5, 10>`, where today it is the target).

  The results are unioned, dropping `never`. When every pair is disjoint
  the result is `never`: both the union arm's `0 => target.clone()` and
  the non-union `_ => target.clone()` go. `Mixed(_, Some(lacks))` keeps
  `target.without(lacks)`. A template placeholder `original` narrows as
  its bound does, and returns `target` when that is not `never`, so a
  `T` still narrows to the checked type. This is the same rule
  `narrow_to_instance` needs in its disjoint fallback, since
  `null&ImageId` is exactly what it avoids, so both share one helper.
  The eight `narrow` callers (the type-check predicates, `===` in
  `narrowing.rs`, the assertions and `array_filter`'s callback) all take
  the new rule. `$i === null` on an `int` then gives `never` in its true
  branch, where today it gives `null`.

  The hazard is the dead branch. Today `if (!is_int($i)) { $y = $i + 1;
  echo 'x'; }` on `int $i` already reports `deadCode.unreachable` beside
  `function.alreadyNarrowedType` (probed): an expression statement typed
  `never` counts as terminating (`always_terminates`' `Expression` arm).
  F4 makes that happen in every true branch of an impossible check. So
  `never` gains PHPStan's explicit/implicit split:

  - `Repr::Never` carries a flag. `Type::never()` is implicit and a new
    `Type::explicit_never()` is explicit.
  - A declared `never` return, `throw` as an expression, and
    `exit`/`die` give the explicit form.
  - Narrowing, arithmetic and every operation over an implicit `never`
    give the implicit one.
  - `always_terminates` and the checks that ask "does this call end
    execution" read only the explicit form.
  - Equality, hashing and interning keep the flag, and a union of the two
    is the explicit one, since that is what a declared-`never` join
    means.

  F44's `never` from crossed bounds and F39's pow `never` then stop
  producing unreachable reports even before their own fixes. Fixtures:
  `int` after `if (is_float($i)) {}` stays `int`; `?string` after `if
  (is_int($j)) {}` stays `?string`; `strlen($j)` clean; the impossible
  branch has `$i` typed `*NEVER*` with no `deadCode.unreachable`; a
  function returning `never` still ends its branch.
- [ ] **F5. The error type silences reads that report nothing**
  (`11b428f4` with `43349c1a`). `unassigned_variable_type` gives
  `*ERROR*` for every read of a never-assigned variable. That includes
  the quiet reads, which report nothing, so the error type follows no
  report:
  - `$y = $undef ?? 'a'` is `*ERROR*` (PHP gives `'a'`), and
    `empty($u) ? 1 : $u` is `*ERROR*`;
  - inside `if (isset($m))`, or after `if (empty($x)) return;`, `$m`/`$x`
    are `*ERROR*`, not `mixed~null`, so `$m->foo()` is unchecked at level
    9.

  The pre-window binary gave `mixed`. Done: only a reported read yields
  the error type. A quiet read yields `mixed`, or whatever the
  `??`/`isset` semantics give.

  Design: the error type comes from `unassigned_variable_type`, reached
  by both `check_expr`'s and `infer_expr`'s `Variable` arms and by every
  narrowing lookup of an absent variable. It has no idea whether the
  read it answers reported. A probe also shows the harm is wider than
  never-assigned names. `merge_scopes` keeps only the variables both
  branches hold, so after `if ($c) { $v = 1; }` the variable `$v` is
  absent. Then `$v ?? 2` and `$v` inside `isset($v)` are `*ERROR*` too,
  where PHP gives `1|2` and `1`: the common maybe-defined shape.

  The fix gives `Locals` PHPStan's certainty:

  - An entry is `Yes` or `Maybe` defined: a `maybe_undefined` name set
    beside the types, bumped with the version.
  - `merge_scopes` keeps a one-sided variable with its type, marked
    `Maybe`, and `Maybe` on either side stays `Maybe`. The loop
    convergence's "may run zero times" join goes through the same rule.
  - `insert`/`declare` clear the mark. `unset` removes the entry, making
    it certainly undefined.

  Then the reads:

  - `check_expr`'s `Variable` arm reports `variable.undefined` when the
    name is absent or `Maybe` (the message can say "might not be defined"
    for `Maybe`). It returns `Type::error()` only for an absent name; a
    `Maybe` read returns the known type, since the value is that type
    whenever the read succeeds.
  - `infer_expr`'s `Variable` arm keeps the error type for an absent
    name, which only follows a reported read.
  - A new `quiet_variable_type(name, locals)` answers the quiet
    positions: `null` for an absent name (PHP's `isset`/`??` see an
    undefined variable as null), the type unioned with `null` for a
    `Maybe` one, and `mixed` when `any_variable_may_exist` is set. It is
    used by `widen_coalesce_result`'s left side (a `quiet_infer` that
    walks the `Index`/`PropertyAccess` chain as `check_expr_quiet_root`
    does and roots in this lookup), by `current_narrowed_type` when
    `isset`/`empty`/`??=` narrow a variable, and by `empty()`'s operand.

  So `$undef ?? 'a'` is `'a'`, `$v ?? 2` is `1|2`, `isset($v)` narrows
  `$v` to `1`, and inside `if (isset($m))` on a never-assigned `$m` the
  branch has `$m` as `never`, dead, which F4's implicit `never` keeps
  silent. A never-assigned name in `isset()` or `??` may get its own
  report later (PHPStan's "is never defined"); that is not part of this.
  Fixtures: the five expressions above; `$m->foo()` inside `if
  (isset($m))` after a conditional `$m = new Foo` is checked against
  `Foo`; a plain `echo $v` after the conditional definition still
  reports.
- [ ] **F6. A rebound closure's `$this` is blessed as undefined**
  (`fda237d8`). The commit's own fixture expects `variable.undefined
  $this` (level 0) inside the two closures it rebinds with `->call(new
  Foo)` and `Closure::bind`. PHP runs both with `$this` bound. The rebind
  scan feeds only unused-member counting. Done: a closure rebound to an
  object has `$this` of that object's type (or `object`), and the fixture
  expects no `variable.undefined`.

  Design: the checker already seeds `$this` for a literal it can see at
  the rebind. `check_closure_literal_with_this_and_scope`
  (`calls/closure_context.rs`) is called for `Closure::bind(<literal>,
  …)` (`expr.rs` near 2620), `(<literal>)->bindTo(…)` (near 1140),
  `@param-closure-this` and Pest. Three gaps remain:

  - `(<literal>)->call($obj, …)` has no arm.
  - The two existing arms compare `method == "bind"` and `**method ==
    *"bindTo"` by case, where `unused/rebind.rs` folds it.
  - A closure held in a variable and rebound later, the fixture's shape,
    is checked at its definition with no `$this`.

  The fix adds the `call` arm, with `$this` the first argument's type
  and the scope that object's class (PHP's `call` rebinds both), and
  folds both names' case. For the variable shape, each function-like
  body gets a pre-scan: the `ClosureVariables`/`Rebinds` walk in
  `unused/rebind.rs`, made to return a map from a closure variable to
  its rebind sites (`$v->call(E)`, `$v->bindTo(E, S)`,
  `Closure::bind($v, E, S)`), not only the scope classes. The checker
  keeps that map per body, as it keeps the other per-function pre-scans.
  When the closure literal assigned to such a variable is checked,
  `$this` is the union of each site's new-this, read syntactically since
  the site comes later in the flow: `new C(…)` is `C`, a `$var` already
  assigned at the definition takes that variable's type there, and
  anything else is `object`. The scope class is the site's scope
  argument (a class-string literal or `C::class`), or the object's class
  for `call`. A rebind to `null` (`bindTo(null)`) adds nothing, and a
  closure that is never rebound keeps reporting `$this` undefined, which
  is a runtime `Error`. The fixture then expects no
  `variable.undefined`. New fixtures: `$fn->call(new Foo())` reaching
  `Foo`'s private method with `$this` typed `Foo`, a literal `->call()`,
  and `BINDTO` in upper case.
- [ ] **F7. An unqualified call to a global function from a namespace is
  indexed under the namespaced name.** `use_sites::function_at`
  (`use_sites.rs:176-179`) records `name.candidates().0`. With dead code
  on, a global `helper()` called as `helper()` from `namespace App;`
  reports `Function helper is never referenced`. LSP references and
  rename (`lsp.rs` 1262/1321/1360) and composer attribution read the same
  index; `navigation.rs` uses `registry.called_function_name`, which is
  right. This was already the case before the fix window; `d82888a8`
  moved the index and kept it. Done: the index records the function the
  call reaches (`called_function_name`'s fallback rule).

  The key is also the name as written, so `Helper()` doesn't count as a
  reference to `function helper` and `function.unused` fires for it
  (probed with `check_dead_code` on). Done also needs the key
  case-folded.

  Design: the index (`UseSiteIndex`) is built per file and without the
  registry, so that an LSP edit re-indexes one file. The resolution that
  depends on which functions exist therefore has to happen when the
  index is queried, not when it is built. This also stays right when a
  namespaced `App\helper` is added or removed in another file.

  - `RefTarget::Function` holds the name lowercased.
  - `function_at` records a qualified or `use function`-imported name
    under its one candidate. An unqualified name in a namespace is
    recorded under both candidates: the namespaced one as a plain site,
    and the global one as a fallback site tagged with the namespaced name
    it yields to. The tag is a new `RefTarget::FunctionFallback(global,
    namespaced)` stored in `by_target` under the global key.
  - `references(&RefTarget::Function(f))` gains a sibling
    `function_references(f, is_declared: &dyn Fn(&str) -> bool)`. It
    returns the plain sites for `f` plus the fallback sites whose
    namespaced name is not declared.

  Its three callers already have the registry: `dead_code`, the LSP
  references and rename (`lsp.rs` 1262/1321/1360), and composer
  attribution. The last one's own fallback in `composer_hygiene.rs:308`
  then goes. `registry.function_declaration(fqn)` at `lsp.rs:1321` looks
  up folded.

  While designing this, a probe showed that classes have the same case
  gap: `new WIDGET()` for `final class Widget` with `check_dead_code` on
  reports `class.unused` beside `class.nameCase`. So `RefTarget::Class`
  is keyed lowercased too (`class_at`), and `RefTarget::Member`'s owner
  and member folding is F66's. Rename still rewrites each site's own
  span, so folding the key loses nothing. Fixtures: a global `helper()`
  called from `namespace App;` isn't `function.unused`, and neither is
  `Helper()` or `new WIDGET()`. A namespaced `App\helper` declared
  beside a global `helper` takes the unqualified call, so the global one
  is unused. LSP references on the global function list the namespaced
  file's call site.
- [ ] **F8. An attribute class used only as an attribute is `class.unused`**
  (`addc7c39` kept this). The use-site walker records extends, implements,
  trait use, catch, type-hint, call, `new` and fetch names, but not
  attribute names. `#[Marker]` on a class leaves `Marker` unreferenced,
  and LSP references miss attribute sites. Done: attribute names are
  indexed as class references.
- [ ] **F9. A pipe into an impure callable is "does not do anything."**
  `$x |> 'var_dump';`, `$x |> 'printf';` and `$x |> fn($v) =>
  print($v);` as statements report `expr.resultUnused`. A pipe calls its
  right side. This was already the case before the fix window; the
  window's two pipe commits (`1cb1e25d`, `b3412c22`) didn't touch it.
  Done: a pipe statement is a no-op only when its callee is known pure.
- [ ] **F10. A bare `yield` followed by a binary-operator keyword is a
  parse error** (`ad2a5635` is incomplete). `(yield and $b)`,
  `or`/`xor`, `(yield & $b)`, `(yield |> $b)` and `(yield instanceof
  Foo)` all pass `php -l` but skip the file. The tokens that end a bare
  `yield` are a hand-written list. Done: a bare `yield` ends at any token
  that can't start an expression.
- [ ] **F11. `NAN === NAN` is reported always true.** PHP gives `false`
  (and `NAN == NAN` is false, which is reported right). The NaN literal
  compares equal to itself, as `scalars_disjoint`'s "NAN is itself" says.
  This was already the case before the fix window; `89da4c25` and
  `d89ea9e5` kept it. Done: a NaN literal is never identical to anything.
- [ ] **F12. An unresolvable property type is reported up to three times**
  (`22d4ff21`, F63's property half). `/** @var non-empty-foo */` on a
  property gives `class.notFound`, `property.unresolvableType`, and - new
  in the window - `assign.propertyType` "expects non-empty-foo, 2 given".
  `@var parent` gives the unresolvable report plus the assignment one.
  The Stage B ledger's F63 said the same goes for properties, and its
  done note omits them. Done: an unresolvable property type is ignored
  after its one report, as a parameter's is.

- [ ] **F25. A shape's carried next index goes wrong** (`25ee9d62`).
  - A literal-key write leaves `next_index` where it was.
    `$c = [1]; $c[] = 2; $c[7] = 3; $c[] = 4;` gives
    `array{0: 1, 1: 2, 7: 3, 2: 4}` (PHP gives key 8 for the `4`), so
    `return $c[8];` reports `offsetAccess.notFound`. The same happens
    after an `unset`: `$b[10] = 'x'; $b[] = 'y';` lands `'y'` at 3, not
    11. The pre-window binary gave a general array and no report.
  - `Shape::next_index` is left out of equality and hashing, so two
    branches' shapes that differ only in it merge as one union member,
    keeping either branch's index.
    `$b = [1, 2, 3]; if ($c) { unset($b[2]); } else { $b = [1, 2]; }
    $b[] = 9;` gives `array{0: 1, 1: 2, 3: 9}` alone, where PHP gives
    `[1, 2, 9]` or `{0: 1, 1: 2, 3: 9}`, and `echo $b[2]` reports
    `offsetAccess.notFound`. The interner (`a25d77c6`) keeps the index
    apart, but equality doesn't.
  - `generalize_by_key_set` drops the index of the shapes it merges.

  Done: a write past the carried index moves it (PHP's `nextAutoIndexes`
  after an explicit key). Two shapes with different next indexes are
  distinct types, or their merge gives each appended shape (or an
  unknown index). Fixtures cover both reports.
- [ ] **F26. `mixed~S` joined with a value inside `S` stays `mixed~S`**
  (`67ed2a1d`). `union_with_mixed` takes each joined member out of the
  subtracted type with `without`, which can't take a literal or a subtype
  out of a general type (`string.without('x')` is `string`).
  `if (!is_string($z)) { $v = $z; } else { $v = 'x'; }` makes `$v`
  `mixed~string`, and `$v === 'x'` reports `identical.alwaysFalse`. The
  same happens with `mixed~int` and `5`, and `mixed~array` and `[1]`.
  Done: the subtracted part keeps only what no joined member overlaps,
  with fixtures for a literal and a refined join.
- [ ] **F27. `is_resource()` of a resource is "always true."**
  `while (is_resource($fp)) { fclose($fp); }` reports
  `function.alreadyNarrowedType`, but a closed resource is not
  `is_resource` (`gettype` gives "resource (closed)"). This was already
  the case before the fix window; since `c78732bc` the call is also typed
  `true`, so the loop reads as `while (true)`, and after an `fclose` an
  `if (is_resource($fp))` has a dead `else`. Done: `resource` answers
  `is_resource` maybe, or `fclose()` and the like leave a closed
  resource.
- [ ] **F28. A guarded polyfill's body is checked against the stub's
  signature.** `if (!function_exists('array_key_first')) { function
  array_key_first(array $a) { … $a … } }` reports `undefined variable $a`,
  because the stub names the parameter `$array`, and
  `missingType.iterableValue` names `$array` too. This was already the
  case before the fix window; `f51447bd` relies on the stub winning.
  Done: a declaration's body is checked against its own parameters.
- [ ] **F29. The trait-collision check gets PHP's rule wrong**
  (`check_trait_conflicts`, `oop.rs:626`). All three were already the
  case before the fix window; `4ed3cfe3` rebuilt the conflict set.
  - A false positive: a class declaring the method itself
    (`use A, B; public function foo()`) still gets
    `trait.methodConflict`, though PHP runs it (the class's method wins).
  - A miss: two traits with the same method and compatible signatures
    are a PHP fatal too ("has not been applied … because of collision"),
    unreported.
  - An `as` alias counts as resolving the collision
    (`resolved_trait_conflicts` holds every method an adaptation
    mentions), but PHP still fails.

  Done: a method name from two used traits collides unless the class
  declares it or an `insteadof` picks one; the same rule for properties,
  where identical declarations are allowed.

- [ ] **F35. Abstract-method and final-override checks compare method
  names by case.** `abstract_methods()` and `final_methods()` are
  case-sensitive sets.
  - A false positive at level 0: `abstract function run()` implemented as
    `RUN()` reports `class.abstractMethodNotImplemented` ("does not
    implement abstract method(s): run"), though PHP runs it.
  - A miss: `final function Foo()` overridden as `foo()` reports only
    `method.nameCase`, not `method.parentMethodFinal`, where PHP fails
    with "Cannot override final method". A `final __Construct` behaves the
    same.

  This was already the case before the fix window. The Stage B review's
  F4 fixed unused private members, and its F14 made class and function
  tables fold case; member sets were left out. Done: member-name sets and
  lookups fold case as PHP does.

- [ ] **F38. A template in `@phpstan-assert` narrows to the bare
  placeholder.** `apply_assert_infos` (`narrowing/assertions.rs`)
  narrows the target to `info.ty` without substituting the call's
  template bindings. Since `f18bc022` `narrow(mixed, T)` is no longer
  kept as `mixed` but becomes `T` (the fallback in F4), so:
  - `assertInstance(Sub::class, $m)` with
    `@template T of object @param class-string<T> $c
    @phpstan-assert T $x` leaves `$m` as `T`, and `?Foo $f` becomes `T`
    where the pre-window binary kept `Foo|null`.
  - After `assertSame(5, $n)` (`@template T @param T $expected
    @phpstan-assert T $actual`), `needsInt($n)` reports "expects int, T
    given" at level 9; `$m->nope` on the `T of object` value goes
    unreported.
  - Before the window the assertion narrowed nothing, and the declaration
    already reported `assert.alreadyNarrowedType` ("Asserted type T for $x
    with type mixed does not narrow down the type") for each of these, a
    false positive at level 0 that is still there.

  Done: the assertion's type has the call's template bindings, or the
  bounds of the unbound ones, substituted before narrowing, and the
  declaration-side check compares against the bound; fixtures for
  `class-string<T>` and `T $expected` assertions.

- [ ] **F39. Integer arithmetic over a union or a signed range gives the
  wrong type** (`types::arithmetic`, read whole; the pre-window binary
  gives the same types).
  - `pow_int_range` pairs `min ** min` with `max ** max`, which only holds
    for a base that can't be negative. `int<-3, 2> ** 2` is `never`, so
    the statement after `$sq = $a ** 2;` reports `deadCode.unreachable`;
    `int<-2, -1> ** int<1, 2>` is `int<-2, 1>` (PHP gives `4` for
    `(-2) ** 2`); `int<-5, max> ** 2` is `int<25, max>`, though `0 ** 2` is
    `0`. A bound that overflows is dropped as unbounded instead of adding
    `float`.
  - `binary_operation`'s `+`, `-`, `*` and `**` fall back to plain `int`
    whenever an operand isn't one integer type: `either_float` only
    catches an operand that is surely a float. `int|float + 1` is `int`,
    and so are `$numericString + 1` and `'1.5' + $i` (`to_number` makes
    them `int|float` first). `is_int($if + 1)` reports
    `function.alreadyNarrowedType` and `is_float($s + 1)`
    `function.impossibleType`; PHP gives `true` for
    `is_float("1.5" + 1)`.
  - Unary `-`/`+` (`checker/expr.rs`, both arms) make any string
    operand `int`: `-'1.5'` is `-1.5`.
  - `**` with an exponent that can be negative stays `int`: `int<1, 3> **
    int<-1, 1>` is `int<min, 3>`, and `3 ** -1` is a float. A negative
    base over a range of exponents keeps the negative corner: `int<min,
    -3> ** int<1, 3>` is `int<min, -27>`, though `(-3) ** 2` is `9`.
  - An unbounded end never adds `float`: `int<min, -3> + int<min, -3>` is
    `int<min, -6>`, and `PHP_INT_MIN + -3` is a float. F60's sweep finds
    this at the ends for `+`, `-`, `*` and `**`.

  Done: a base range that spans zero or is negative is split by sign
  (even and odd exponents differ), an overflowing bound adds `float`, and
  an operand that may be a float makes the result `int|float` (or `float`
  when every member is one), with fixtures for each.
- [ ] **F40. `null == '0'` is taken as always true** (`loosely_equals`,
  read whole; also before the window). `null` against anything is
  compared as two booleans, but PHP compares `null` with a string as
  `''` with that string: `null == '0'` is `false`, and the check reports
  `equal.alwaysTrue` and treats the `else` branch as dead. Done: `null`
  against a string literal compares `''` with it; the other `null` and
  `bool` pairs stay as they are.
- [ ] **F41. A PHPDoc type that names a class or a literal the parser
  mistakes** (`types::parse`, read whole; also before the window).
  - `parse_type_atom` tries `s.parse::<f64>()` before class names, and
    Rust reads `NaN`, `inf` and `infinity` in any case. `@param Infinity
    $i` for a class `Infinity` is the float `INF`, so a native
    `Infinity $i` reports `parameter.phpDocType` ("type INF is
    incompatible with native type N\Infinity"); `Nan` becomes `NAN` and
    `$n->v` reports `property.nonObject`. Real PHPStan reads all three as
    classes.
  - `@param 1_000 $u` and `@param 0x1F $h` report `phpDoc.parseError`
    ("Unexpected token \"_000\", expected variable"); PHPDoc's grammar
    takes both (real PHPStan reads `1_000` as `1000`).
  - A conditional return type splits its branches on the first top-level
    `:`, so `($flag is true ? \Closure(): int : string)` falls through to
    a class name: `class.notFound` for `N\$flag is true ? \Closure(): int :
    string`, `return.phpDocType`, and since the window
    `return.unusedType`. A branch with `self::A` falls back to the union
    of both branches. A `?T` branch splits on its `?` the same way.

  Done: a float literal is read only when the text starts with a digit,
  a sign or a dot; integer literals take PHP's separators and prefixes;
  the conditional's branches are split past the callable return and `::`
  (and a nullable branch); fixtures for each.

- [ ] **F44. `rand()` with its bounds reversed is `never`** (`dedc1ad8`).
  `Type::int_range` now gives `never` when the bounds cross, and the
  `rand`/`mt_rand`/`random_int` arm (`builtin_calls.rs`) builds its range
  from the first argument's minimum and the second's maximum. PHP's
  `rand()` accepts `rand(10, 1)` and returns a value in `[1, 10]`, so
  `$r = rand(10, 1);` is `never` and the next statement reports
  `deadCode.unreachable`; the pre-window binary gave `int<10, 1>` and no
  report. `rand($a, $b)` with `$a` in `int<5, 10>` and `$b` in
  `int<0, 3>` is `never` the same way. (`mt_rand` and `random_int` throw
  on crossed bounds, so `never` is right for them.) Done: `rand`'s range
  spans both arguments' bounds in either order, with a fixture.
- [ ] **F45. A write through a reference doesn't reach the variable it
  aliases** (also before the window). After `$t = &$s; $t = null;`, `$s`
  keeps its old type (`'x'`), so `$s === null` reports
  `identical.alwaysFalse`; after `$r = &$x; $r++;`, `$x === 1` reports
  `identical.alwaysTrue`. A fact the branch-implied-facts mechanism
  recorded for `$s` is replayed across such a write too, and
  `check_pointless_null_probe` reports `$x ?? 'd'` after `$x = null; $r =
  &$x; $r = 'a';` as `nullCoalesce.variable` ("always null"). Done: an
  assignment by reference records the alias, and a write to either name
  updates (or at least forgets the narrowing of) the other, with
  fixtures for both shapes.
- [ ] **F46. A flag set to `false` in an `if` replays the condition as
  true** (`record_if_body_flag_alias`, `checker/narrowing.rs`; also before
  the window). The alias is recorded whatever literal the body assigns,
  and testing the flag replays the condition at the flag's own polarity.
  `$ok = true; if ($x === null) { $ok = false; } if ($ok) { strlen($x);
  }` narrows `$x` to `null` inside `if ($ok)` and reports "Parameter #1
  $string of function strlen expects string, null given" - `$x` is a
  `string` there. Done: the alias is recorded only when the assigned
  literal is truthy and the prior value falsy (or the replay is negated
  to match), with a fixture for the `true`-then-`false` shape.
- [ ] **F47. A loop that can't exit on its own isn't seen as
  terminating** (`always_terminates`, `checker/control_flow.rs`; also
  before the window). Both report `return.missing` at level 0:
  - `do { if (...) { return 1; } } while (true);` - there is no
    `DoWhile` arm.
  - `while (true) { foreach (...) { break; } if (...) { return 1; } }` -
    `stmt_contains_break` counts a `break` that only leaves the nested
    loop. Its doc says the over-count "only ever causes a missed
    simplification, never a wrong termination conclusion", which is not
    so for `return.missing`.

  Done: a `do`-`while` with a truthy literal condition is handled like
  `while`, and only a `break` that targets the loop itself counts, with
  fixtures.
- [ ] **F48. A write through a key that is one of several literals
  overwrites every one of them** (`bind_assignment_target`,
  `checker/assignment.rs`; also before the window).
  `purge_aliased_index_entries` drops the container's per-key entries
  first, then each literal the key may be gets the written type outright;
  the union with the prior value the code intends never happens. With
  `$key` `'id'|'name'`, `$row[$key] = null;` makes `$row['id']` and
  `$row['name']` both `null`, so `$row['id'] === null` reports
  `identical.alwaysTrue`. Done: when the key may be more than one
  literal, each entry becomes the union of its prior value and the
  written one, with a fixture.
- [ ] **F49. Builtin return types that claim more than the function
  gives** (`checker/expr/builtin_calls.rs`, read whole; all but the
  report noted were already there before the window):
  - `array_combine` returns the values array's type without its
    accessories, keeping the values' keys:
    `array_combine(['a', 'b'], [1, 2])` is `array{0: 1, 1: 2}`, and
    `$c['a']` reports `offsetAccess.notFound`.
  - `array_change_key_case` returns its input unchanged, though the keys
    are re-cased: `array_change_key_case(['Content-Type' => 'x'])['content-type']`
    reports `offsetAccess.notFound`.
  - `array_fill_keys` keys the result by the keys array's value type
    without converting it to an array key: `array_fill_keys(['1', '2'],
    0)` is `array<'1'|'2', 0>`, and `$k[1]` reports
    `offsetAccess.notFound`.
  - `get_parent_class($o)` answers from `$o`'s declared class, so for a
    non-final `Foo` with no parent it is `false` and `=== false` reports
    `identical.alwaysTrue`; a `Bar extends Foo` instance gives `'Foo'`.
  - `array_slice` keeps the input's non-emptiness, and `explode` is
    `non-empty-list<string>` even with a negative limit
    (`explode(',', 'a', -1)` is `[]`). Since the window, comparing either
    with `[]` reports `identical.alwaysFalse`.
  - `parse_sscanf_format` doesn't count `%X` or `%n`, so
    `sscanf($s, "%d %X", $a, $b)` reports `argument.sscanf` ("contains 1
    placeholder(s), 2 value(s) given") at level 0.

  Done: each gives what PHP returns (keys from the keys array, re-cased
  shape keys, array-key conversion, the declared parent only for a final
  class, non-emptiness only when it survives, every scanf conversion
  counted), with a fixture for each.

- [ ] **F52. A closure argument whose last statement is an `if` with a
  `return` in each branch is typed as possibly returning `null`**
  (`closure_syntactic_return_type`, `checker/expr.rs`; also before the
  window). The argument path decides "falls off the end" from the last
  statement's kind alone, so passing
  `function (int $x) { if ($x > 0) { return 1; } else { return 2; } }`
  to a `Closure(int): int` parameter reports `argument.type` ("expects
  Closure(int): int, Closure(int): 2|1|null given"). The same closure
  assigned to a variable, typed through `check_expr`'s converged body,
  is `Closure(int): 1|2`. Done: falling off the end is decided with
  `always_terminates` over the body, as `check_expr`'s arm does, with a
  fixture.
- [ ] **F53. An exhaustive `match (true)` is reported unhandled**
  (`condition_is_provably_true`, `checker/expr.rs`; also before the
  window). The last arm counts as covering the rest only when
  `inferred_constant_condition` or an integer ordering comparison proves
  it true. `match (true) { $x instanceof A => 1, $x instanceof B => 2 }`
  on `A|B`, `{ is_int($v) => 1, is_string($v) => 2 }` on `int|string`
  and `{ $b === true => 1, $b === false => 2 }` on `bool` all report
  `match.unhandled` ("does not handle remaining value: true") at level 4.
  Done: the last arm's condition is judged against the scope where every
  earlier arm failed, with `instanceof`, the type-check predicates and
  `===` recognized (the same narrowing the arm bodies already get), with
  fixtures.

- [ ] **F55. A class declaring `__tostring()` in lower case isn't
  `Stringable`** (`ClassReflection::from_def`; also before the window).
  The implicit interface is added on `methods.contains_key("__toString")`,
  an exact-case lookup, and PHP method names ignore case: for `final class
  A { public function __tostring(): string {...} }`, passing `new A()` to
  a `Stringable` parameter reports `argument.type` and
  `new A() instanceof Stringable` reports `instanceof.alwaysFalse`; PHP
  says `true`. Done: the lookup folds case, with a fixture.
- [ ] **F56. A method calling `static::` is proven pure though a subclass
  can override the callee** (`resolve_static_call`, `registry/purity.rs`;
  also before the window). `static::m()` is resolved like `self::m()`,
  with no overridability check, so `final public function run(): int {
  return static::compute(); }` is pure when `compute()` is, and
  `$b->run();` reports `method.resultUnused` ("has no effect") even with a
  `Child` whose `compute()` echoes. The same walk disqualifies every
  `ClassConstFetch` and `ClassNameFetch` under a comment about static
  property access, so a getter returning `self::X` is never pure and a
  bare call to it goes unreported. Done: `static::` needs the same
  non-overridable target `$this->` does, and a class constant or `::class`
  read doesn't disqualify, with fixtures.
- [ ] **F57. An arrow function's body stops before `and`/`or`/`xor`**
  (`parse_arrow_fn`, `parser/closures_and_match.rs`; also before the
  window). The body is parsed with `parse_assignment`, but PHP's
  `fn() => expr` takes a whole `expr`: `$f = fn() => false or $b;` is a
  closure returning `false or $b` (PHP gives `true` for
  `(fn() => false or true)()`). prs reads `($f = fn() => false) or $b`,
  reports `logicalOr.resultUnused`, types `$f` as `Closure(): false`, and
  reports `if ($f())` as `if.alwaysFalse`. Separately, an unparenthesized
  nested ternary (`$b ? 1 : 2 ? 3 : 4`), a compile error since PHP 8,
  parses as a right-nested one and nothing reports it. Done: the arrow
  body is a full expression (`parse_expression`), and a ternary in the
  `else` of another without parentheses is a parse error, with fixtures.

- [ ] **F60. Subtracting a half-unbounded range from a constant, or
  dividing one by a negative constant, flips the result**
  (`types::arithmetic`; also before the window). Checked against PHP for
  `+`, `-`, `*`, `/`, `%` and `**` over every pair of ranges with ends in
  {`min`, -3, -1, 0, 1, 3, `max`}, and exhaustively over bounded ranges
  in [-4, 4], where every result was right.
  - `sub_int_range_full`'s first branch gives `int<min, c>` for a
    constant `c` minus any range with an unbounded end: `-1 - int<min,
    -1>` is `int<min, -1>` (PHP gives 0 and up), and `3 - int<-1, max>`
    is `int<min, 3>`, though `3 - (-1)` is `4`. With `int<0, max> $a`,
    `-1 - (-$a - 1) < 0` reports `smaller.alwaysTrue`; the value is `$a`.
  - `div_int_range`'s swap for a negative constant divisor makes
    `int<0, max> / -1` `int<0, max>|float` and `int<min, -3> / -3`
    `int<min, 1>|float`; the int results are `int<min, 0>` and
    `int<1, max>`.

  Done: `c - int<lo, hi>` is `int<c - hi, c - lo>`, an unbounded end
  staying unbounded; a negative constant divisor mirrors the range; the
  pair sweep is a unit test.
- [ ] **F61. `isset()` and `??` on a property of a nullable object are
  reported pointless** (`resolve_always_exists`,
  `checker/expr/probe_checks.rs`; also before the window). The
  `PropertyAccess` arm finds the property on the receiver's type without
  asking whether the receiver can be null: with `?Foo $x` and `Foo::$p`
  an `int` with a default, `isset($x->p)` reports `isset.property` ("is
  not nullable") and `$x->p ?? 2` `nullCoalesce.property`, though a null
  `$x` makes them `false` and `2`. `unset($x->p); isset($x->p)` reports
  the same. Done: a receiver that may be null, or a property unset on the
  path, makes the chain not always present, with fixtures.
- [ ] **F62. A property keeps its narrowed type across a call that can
  change it** (also before the window). After `$this->x = null;
  $this->load();`, where `load()` assigns `$this->x`, `$this->x` is still
  `null`. `expr_reads_a_property` (`checker/expr/ranges.rs`) documents
  the gap, and the constant-comparison rules skip an operand that reads a
  property, but the walk only descends through calls, binary and unary
  operators and indexes: `(string) $this->x === ''` reports
  `identical.alwaysTrue`. Done: a call not known to be pure forgets the
  narrowed properties of `$this` (and of the receiver, for a method call
  on another object), with fixtures; the syntactic guards can then go.
- [ ] **F63. Two loop exits misread** (`checker/stmt/loops.rs`; also
  before the window):
  - `for (;;) { if ($c) { $y = 1; break; } } return $y;` reports
    `variable.undefined`: the condition-less arm merges the body's end
    state, which has no `$y`, with the `break` states. `while (true)`
    folds only the `break` states and gets it right.
  - `do { if ($c) { continue; } ... } while (false);` reports
    `doWhile.alwaysFalse`. A `break` in the body exempts this run-once
    idiom; a `continue`, which leaves the loop the same way here, doesn't.

  Done: `for (;;)` exits through its `break` states only, and a
  `continue` exempts `doWhile.alwaysFalse` as a `break` does, with
  fixtures.
- [ ] **F64. A `mysqli` query that doesn't start with its verb is typed
  `true`** (`returns_result_set`, `checker/expr/mysqli.rs`; also before
  the window). Only the first word after any `(` is read:
  `$db->query('/* report */ SELECT 1 AS a')`, `"-- c\nSELECT 1 AS a"`,
  `'CALL list_users()'` and `'CHECK TABLE t'` are `true`, so
  `$r->fetch_assoc()` reports `method.nonObject`. Done: leading comments
  are skipped, and `CALL`, `CHECK`, `ANALYZE`, `OPTIMIZE`, `REPAIR`,
  `HELP` or a verb the list doesn't know give `mysqli_result|true`, with a
  fixture.
- [ ] **F65. A spread argument counts as one argument**
  (`checker/expr/array_ops.rs`; also before the window).
  `check_useless_return_value_call` reads `args.len()`, so `$x =
  var_export(...$args)` reports `function.uselessReturnValue` though
  `$args` may hold `true` for `$return`. `check_array_filter_no_op` and
  `check_array_values_no_op` do the same and judge `array_filter(...$args)`
  by the type of `$args` itself (read, not probed). Done: a spread
  argument makes each check bail, with a fixture.
- [ ] **F66. Two live shapes the opt-in dead-code rules report**
  (`project::whole_program`; also before the window):
  - `class.unused` for a class named only in PHPDoc (`@param DocOnly
    $d`). `dead_code`'s module doc lists attributes, string callables and
    framework magic as known gaps, not PHPDoc.
  - `method.unused` for `A::mixedCase()` called as `$a->MIXEDCASE()`:
    member references are recorded under the name as written.

  Done: a PHPDoc type counts as a reference and member references fold
  case, with fixtures.

- [ ] **F81. A loop's shape widening builds a self-nesting array to a
  fixed depth and types the bottom wrong** (`widen_towards`,
  `types::queries`; since `f0ad71f8`, which lets a shape gain string keys
  while widening; the 18:15 binary before it is sound). `$a = ['x' => 1];
  while ($c) { $a = ['x' => $a['x'] * 2, 'y' => $a]; }` gives `$a` six
  levels of `y?:`, the innermost `array{x: 1}`, where the binary before
  gave `non-empty-array<string, ...>` below the first level. Each pass
  widens the nested field field by field, one level deeper, until the
  passes run out. After the loop, `($a['y']['y']['y']['y']['y']['y']['x']
  ?? 1) === 1` reports `identical.alwaysTrue`; PHP gives `4` after eight
  iterations. `$a = ['prev' => $a, 'i' => $i]` bottoms out the same way,
  at `prev?: array{}`. Separately (read, not probed), `combined_shape`
  makes a key some unsealed member lacks optional with the other members'
  values only, not that member's tail value. Done: a nested field that
  gains a level widens by generalizing, so that pass sequence reaches a
  fixed point, and `combined_shape` joins an unsealed member's tail into
  the keys it lacks, with fixtures.

### Wrong types and missed reports (probed)

- [ ] **F13. An array literal's positional key after an unknown key**
  (`8febbd1e`). Once a key's value isn't known, later int keys aren't
  observed, and a positional item still takes `next_index`:
  - `[$ab => 1, 5 => 2, 'y']`, with `$ab` of type `'a'|'b'`, gives keys
    `'a'|'b'|5|0`, where PHP gives 6;
  - after `[$r => 1, 'y']`, with `$r` an `int<10, 20>`, the positional key
    is 0, where PHP gives 11..21.

  Done: the next index is tracked as a range; an unknown int key makes it
  unknown (`int`), not 0.
- [ ] **F14. `iterable<K, V>`'s array member drops `K`** (`321d388c`).
  The `"iterable"` arm in `parse.rs` builds
  `array(unknown_array_key, V)`, though it parsed `K`. For
  `iterable<string, int>`, the `foreach` key is `int|string` and
  `is_array($it)` gives `array<string>`. The commit's fixture asserts
  `int|string`. Done: `array<K, V>`.
- [ ] **F15. A negated `instanceof` keeps a generic member.**
  - For `Iterator<int, string>|IteratorAggregate<int, string>`,
    `!$x instanceof Iterator` keeps the whole union, because
    `is_super_type_of(Iterator, Iterator<int, string>)` isn't a yes in
    `without`'s structural relation.
  - In the positive branch, the aggregate member becomes
    `IteratorAggregate<int, string>&Iterator`, which PHP forbids.
  - `Traversable<int, string>` narrowed to `Iterator` is
    `Traversable<int, string>&Iterator`, not `Iterator<int, string>`.

  Done: exclusion matches a generic member by class through the
  hierarchy, and the two exclusive interfaces don't intersect.
- [ ] **F16. Division by zero misses the divisors PHP converts to zero**
  (`b1dddb6c`). `$i / false`, `/ null`, `/ '0'`, `% 0.5`, `% false` and
  `% '0'` all throw `DivisionByZeroError` (checked with `php -r`), but
  `is_zero` compares only with int `0` and float `0.0`. Done: a divisor
  whose every value converts to zero is reported - to a number for `/`,
  to an int for `%`.
- [ ] **F17. `new` with a non-name class expression isn't checked.**
  `check_expr`'s `New` arm (`expr.rs:2218`) runs `infer_expr` on the whole
  `new` and handles a `Name` class only. `new $undef()`,
  `new $undef->cls()` and `new $undef['k']()` report no undefined
  variable. This was already the case before the fix window; `3f04ac3f`
  widened the grammar there without checking it. Done: the class
  expression goes through `check_expr`.
- [ ] **F18. A flat binary chain is still superlinear.** P2's iterative
  chain covers `.` and `+` over variables; left-associative chains
  elsewhere recurse and re-infer. Timings with the release binary, one
  file, startup about 1.4 s:
  - `'a' . 'a' . …`: 1,000 terms 1.6 s, 4,000 terms 9.8 s, 8,000 terms
    51.8 s; 20,000 terms was still running after 192 s;
  - `$b && $b && …`: 1,000 terms 1.8 s, 2,000 terms 5.1 s, 4,000 terms
    15.8 s;
  - an `||` of 10,000 `instanceof` checks: 705 s;
  - 1,000 nested `match`: 71 s.

  The fixes did cut both chains a long way: on the pre-window binary,
  4,000 concatenated literals took 21.4 s (9.8 s now), and a 4,000-term
  `&&` chain was still running after 46 minutes (15.8 s now).

  A chain of traits each using the previous one (`trait T1 { use T0; }`
  … `class A { use Tn; }`) grows about cubically, and the pre-window
  binary grows the same way (400 traits: 9.4 s before, 9.0 s now):
  - 100 traits: 1.7 s;
  - 200: 2.7 s;
  - 400: 9.0 s;
  - 800: 27.7 s;
  - 3,000: 63 minutes.

  `check_if`/`walk_if_chain` recompute `rest.iter().all(...)` at each
  `elseif` (`288f4fda`), so an `elseif` chain is O(n²) too. `2750baf2`'s
  "nesting is linear" holds for nested calls. Done: each chain shape is
  linear, measured at 20,000 terms.
- [ ] **F19. Smaller type gaps met while probing:**
  - `89da4c25`: loop widening generalizes a growing literal to its base
    type and drops non-empty (`'a'` then `.= 'b'` is `string`, not
    `non-empty-string`).
  - `40e086a6`: an unsealed shape with a required string key intersected
    with `list` isn't `never`.
  - `bd7d7a89`: a native `\int` hint is read as a class `int` and reports
    `class.notFound` plus "expects int, 1 given" (PHP refuses to compile
    it).
  - `$i / 0` infers `int|float`, not `never`.
  - Already the case before the fix window: a PHPDoc `1e25` is a parse
    error, though phpdoc-parser's float lexeme accepts it; `array_pop($b)`
    leaves `$b` a bare `array`; `$x['a']` on
    `array{a: 1}|array{b: 2}` is `mixed`, with no "might not exist" report
    (`offset_value_type` gives up on the whole union when one member lacks
    the key).

  Done: each is fixed or ledgered where it belongs.

- [ ] **F30. A promoted property's set visibility is ignored.**
  `class_info.rs:1264` sets `write_visibility` from
  `promotion.visibility`, and nothing reads `promotion.set_visibility`
  (parsed, and carried into `Promotion` by `ac5240a9`). For a promoted
  `public private(set) int $x`, `$p->x = 2` from outside is unreported,
  where PHP gives "Cannot modify private(set) property"; a declared
  property's `private(set)` is reported. This was already the case
  before the fix window. Done: `set_visibility`, falling back to the
  visibility, as a declared property has it.
- [ ] **F31. An anonymous class skips the class-level checks.**
  `check_anonymous_class` (`027811e4`) runs only inheritance kinds,
  magic serialization, property declarations, readonly version, methods
  and attributes. None of these is reported (all were missed before the
  window too):
  - `new class implements Countable {}` (a PHP fatal: it must implement
    `count`);
  - `new class implements NoSuchInterface {}`;
  - `new class extends AbstractThing {}` without the abstract method.

  Now that an anonymous class is a registered class with a name, it can
  go through the declared class's checks: completeness, class
  references, ancestors and generics, and property and constant checks.
  Done: it does.
- [ ] **F32. Smaller gaps from commits 100 to 155:**
  - `b1dddb6c`'s companion `binaryOp.invalid` says `'abc' + 1` "results
    in an error" on `--php-version 7.4` too, where it is `1` with a
    warning; the version flag `1f4b45bf` added isn't consulted there.
  - `1cb1e25d`: a union of callable names isn't resolved for the pipe's
    by-reference check.

  Done: each is fixed or ledgered.

- [ ] **F36. An argument outside its template's bound isn't reported.**
  With `@template T of \Countable @param T $c`, `cnt(5)` gives no
  `argument.type` at level 9, and returns `Countable`, where the
  pre-window binary returned `5`. `@template T of int` with
  `onlyInt('s')` isn't reported either. The Stage B review's F38 names
  this ("an argument outside `T of Foo` binds anyway") and is marked done
  without it. Done: an argument is checked against the bound of the
  template its parameter names.

  `3cea2a20` checks the bound with the bindings found so far substituted
  in (`R of Repo<E>` holds a `Repo<Entity>` once `E` is `Entity`), in
  either parameter order. A rejected argument still binds the bound's
  own templates first: with `E` bound to `stdClass`, passing
  `Other::class` (a `Repo<DateTime>`) for `class-string<R>` makes `E`
  `stdClass|DateTime` and the call return `Repo<stdClass|DateTime>`,
  with no report.

- [ ] **F42. Smaller gaps from the final-state pass:**
  - `promote_class_string_literals` (`types::templates`) walks an `Array`
    argument but not a shape, so with `@param array<class-string<T>> $cs
    @return T`, `first([Foo::class])` is `Foo` and `first(['Foo'])` is
    `object`.
  - `describe_accessory_intersection` (`types::display`) renders any
    intersection with an `Array` member from that member and the list or
    non-empty accessory alone: after `$a['x'] = 1` on a
    `non-empty-array<string, mixed>`, `$a` shows as
    `non-empty-array<string, mixed>`, its `hasOffsetValue('x', 1)`
    dropped from messages and `assertType`.
  - Since `2b1ce22b` a native `?iterable` parameter's
    `missingType.iterableValue` reads "in iterable type array" (the walk
    reaches `iterable`'s `array` member first); the pre-window binary said
    "iterable type iterable".
  - `syntax::ast::is_superglobal` counts `argv` and `argc`, which exist
    only in the global scope, so `$argv` read inside a function isn't
    `variable.undefined`.

  Done: each gives the type or message it names, with a fixture.

- [ ] **F50. Calls that narrow are matched by the name's case** (also
  before the window). PHP function and method names ignore case, but:
  - `names_with_assert_tags` (`apply_bare_statement_assert_narrowing`)
    holds names as declared, so `A::ISSTR($a)`, `$o->ISINT($b)` and
    `ISFL($c)` narrow nothing where the declared spelling does.
  - A call to `extract` drops the body's diagnostics only when written
    exactly `extract(...)` (`name == "extract"`), though
    `any_variable_may_exist` is set for any spelling.
  - `bare_assert_condition` compares with `"assert"`, and the narrowing
    arms for `count`, `in_array`, `array_key_exists`, `str_contains`,
    `str_starts_with`, `str_ends_with` and `is_a` compare the name as
    written, where the single-argument predicates go through
    `called_function_name`.
  - `narrow_enum_from_call` compares `from`/`tryFrom` exactly
    (`Suit::FROM(1)` is `Suit`), `or_chain_narrowing_target` looks the
    predicate up unfolded (`IS_INT($x) || is_string($x)` narrows
    nothing), `preg_match_condition` compares with `"preg_match"`, and the
    PHPUnit assertion narrowing matches method names exactly.

  Done: the tagged-name set and every name comparison fold case (and
  resolve the function the way `called_function_name` does).

- [ ] **F54. Two expressions that are always `mixed`** (`checker/expr.rs`;
  also before the window):
  - The short ternary: `$s ?: 'd'` with `?string $s` is `mixed` in both
    `check_expr` and `infer_expr` (the `then` branch is `None`, and the
    arm returns `mixed` for it), so what's assigned from it goes
    unchecked. It is `$s` without its falsy values, or `'d'`.
  - A compound assignment's value: `$z = ($n += 1)` makes `$z` `mixed`
    (`check_expr`'s arm returns `mixed` after binding the result).

  Done: the short ternary is the condition's truthy type unioned with
  the `else` type, and a compound assignment is the value it assigns,
  with fixtures.

- [ ] **F67. A loose `==` narrows as `===`** (also before the window).
  - `check_switch` (`checker/stmt/switch.rs`) narrows each case through
    `apply_equality_narrowing`, which is `===`'s narrowing, but `switch`
    compares with `==`. With `string $x`, `case 0:` types `$x` as the int
    `0` (`'0'` and `'0.0'` match); with `?string $x`, `case null:` types
    it `null` (`''` matches too); with `int|string $x`, `case '1':` types
    it `'1'` (`1`, `'01'` and `'1.0'` match too).
  - `or_chain_narrowing_target` (`checker/narrowing/predicates.rs`) takes
    a `BinOp::Eq` leaf as `===`: `$x == null || $x == 'a'` narrows a
    `?string` to `'a'|null`, and `$x == 1 || $x == 2` an `int|string` to
    `1|2`.

  Done: a loose comparison narrows to every value of the operand's type
  that loosely equals the other side, or not at all, and a `switch`
  narrows its cases that way, with fixtures.
- [ ] **F68. A `break` inside an `if` in a `switch` case loses its path**
  (`check_switch`; also before the window). The frame pushed onto
  `loop_break_states` is popped and discarded, and only a case's last
  statement is read for a `break`: after `case 1: if ($c) { $a = 'one';
  break; } $a = 'two'; break; default: $a = 'd';`, `$a` is `'two'|'d'`.
  Done: the popped `break` states join the switch's exit states, with a
  fixture.
- [ ] **F69. A `catch` is judged unreachable when the `try` throws only
  through magic** (`check_try`, `checker/stmt/try_catch.rs`; also before
  the window). A clause is entered only from recorded throw points, and
  reading an `ArrayAccess` offset, converting an object to string,
  reading a property through `__get` and `include` record none: `$ok =
  true; try { $r = $bag['x']; } catch (RuntimeException $e) { $ok =
  false; }` leaves `$ok` `true`, as do `'a' . $s`, `$m->foo` and `include
  $f`. Done: each records a `Throwable` throw point (the offset read and
  the conversion only for an object with the method), with fixtures.
- [ ] **F70. Array literal and spread keys** (`checker/expr/array_ops.rs`):
  - Since `8febbd1e`, a literal whose key isn't known goes general but
    keeps taking positional keys from an index that no longer sees int
    keys: with `'a'|'b' $s`, `[$s => 1, 10 => 2, 'c']` is
    `non-empty-array<'a'|'b'|10|0, 1|2|'c'>`; PHP puts `'c'` at `11`. The
    pre-window binary gave `array`.
  - An optional int field is renumbered as if present: `[...$a, 'c']` and
    `array_merge($a, ['c'])` with `array{0: 'a', 1?: 'b'} $a` are
    `array{0: 'a', 1?: 'b', 2: 'c'}`; without `1`, `'c'` lands at `1`
    (also before the window).
  - An optional string field replaces a required one: `['x' => 1,
    ...$a]` and `array_merge(['x' => 1], $a)` with `array{x?: 2} $a` are
    `array{x?: 2}`; `x` is always there, `1|2` (also before the window).
  - A string key from an earlier known spread doesn't stop the result
    being a list: `[...['a' => 1], ...$l]` with `list<int> $l` is
    `list<int>`, and not non-empty (also before the window).

  Done: general mode observes every int key and makes a later positional
  key `int` once an unknown key may be an int; an optional int field makes
  the keys renumbered after it uncertain; an optional field keeps a
  required one's key required, its type joined; list-ness and
  non-emptiness count the fields already merged, with fixtures.
- [ ] **F71. `preg_match_all`'s pattern-order groups never hold `''`**
  (`preg_match_all_pattern_order_shape`, `checker/expr/preg_shapes.rs`;
  also before the window). A group that doesn't take part in a match
  still gets a slot, `''` without `PREG_UNMATCHED_AS_NULL`, but the
  element type is the group's own: `preg_match_all('/(a)?b/', $s, $m)`
  types `$m[1]` `list<'a'>` (PHP gives `['', 'a']` for `'b ab'`), and
  `/(\d+)?b/` `list<numeric-string>`. The `PREG_SET_ORDER` result is
  `array<...>`, though it is a list. Done: an optional group's element
  adds `''`, and the set-order result is a list, with fixtures.
- [ ] **F72. Narrowing that picks the wrong argument or value** (also
  before the window):
  - `assert_target_key_and_current` (`checker/narrowing/assertions.rs`)
    finds an `@phpstan-assert` parameter's argument by position: with
    `@phpstan-assert int $a` on `checkA(mixed $a, mixed $b)`,
    `checkA(b: $x, a: $y)` makes `$x` `int` and leaves `$y` `mixed`.
  - `in_array_narrowing` rules out every field value of a constant
    haystack in the not-found branch, optional fields included: with
    `'a'|'b' $x` and `array{0: 'a', 1?: 'b'} $h`, `!in_array($x, $h,
    true)` makes `$x` `never`.

  Done: a named argument maps by name (a spread bails), and the not-found
  branch rules out only required fields' values, with fixtures.
- [ ] **F73. A `for` loop's in-range writes keep a list for a negative
  index** (`in_range_index_write_loop_vars`, `checker/stmt/loops.rs`;
  also before the window). `for ($i = -1; $i < count($x); $i++) { $x[$i]
  = 5; }` keeps `list<int> $x` a list, though key `-1` ends that. Done:
  the index must also be non-negative at the write, with a fixture.
- [ ] **F74. A `config.platform.php` given as a full version is ignored**
  (`declared_php_versions`, `project/session.rs`; also before the
  window). Composer's `config.platform.php` is an exact version
  (`"8.1.12"`), and a release is kept only when its `.0` or `.99`
  satisfies the constraint, so none is: with `"8.1.12"` the run has no
  PHP version and `PHP_VERSION_ID >= 80200` goes unreported, where `"8.1"`
  reports it always false. Done: an exact version selects its release,
  and a release counts when any of its patches satisfies the constraint,
  with a fixture.
- [ ] **F75. The duplicate-declaration checks miss case variants and
  split groups** (`project/duplicate_classes.rs`,
  `duplicate_functions.rs`; also before the window). Sites are keyed by
  the name as written, so `class Foo` and `class foo`, or `function
  helper` and `function HELPER`, in two files aren't reported; PHP
  refuses the second. Function sites are grouped by comparing each with
  the previous one in path order, so `twice(int)` in `c.php` and `e.php`
  with `twice(string)` in `d.php` between them are never compared; with
  another `twice(int)` in `d0.php`, the list names `d0.php` and `e.php`
  but not `c.php`. Done: sites are keyed by the lowercased name and
  grouped by signature, with fixtures.
- [ ] **F76. A namespace cycle through a context file can go unreported**
  (`find_namespace_cycles`; also before the window). The cycle is
  reported at the alphabetically-first class contributing an edge, chosen
  among every registry class, and dropped when that class's file isn't
  analysed: `App\Foo extends Acme\Base` with `Acme\Plugin implements
  App\Contract` in a scan path reports nothing, and the same code with
  `Zed` for `Acme` reports `App, Zed`. Done: the representative is the
  first analysed class contributing an edge, with a fixture.

- [ ] **F82. Forgetting properties after a call (F62's fix) misses
  whole kinds of writes** (`forget_members_a_call_may_change`,
  `checker/expr.rs`, and `registry/property_writes.rs`; `1c33a059`,
  `310b2205`). Each keeps `$this->x` `null` after `$this->x = null;`
  and a call that sets it:
  - an alias: `$o = $this; $o->load();` forgets `$o`'s properties only;
  - a call whose verdict is `Anything` forgets only its receiver's and
    arguments': `$this->h->run()`, where `Helper::run()` writes
    `$this->c->x` and `c` is the caller, forgets `$this->h`'s;
  - static properties: `self::$cache = null; self::warm();` keeps
    `self::$cache` `null` (a static property write isn't a write in
    `property_target_owner`, and nothing forgets static keys);
  - a builtin taking `mixed` is `Nothing` though it runs the object's
    code: `json_encode($j)` calls `jsonSerialize()` (probed), and
    `var_dump`, `print_r`, `serialize`, `strval` and `sprintf('%s')` run
    `__debugInfo`, `__serialize` or `__toString` (by the stub rule);
  - code run without a call: `echo $s` (`__toString`, probed), an
    undeclared property read (`__get`), an `ArrayAccess` offset,
    `foreach` over an `Iterator`.

  Done: an `Anything` call forgets every object's property facts (static
  ones included), a static property write classifies as a write, a
  `mixed` parameter counts as code-running, and the implicit calls above
  forget like a call on that object, with fixtures.
- [ ] **F83. A `foreach` that rewrites its elements drops a write to
  another element** (`converge_loop_body`, `checker/control_flow.rs`;
  `91666577`). The subject becomes the element the body leaves at
  `$subject[$key]`: with `array<int, array{nb: 1}> $items`,
  `foreach ($items as $day => $d) { $items[$day + 1]['file'] = 'x';
  $items[$day]['file'] = 'y'; }` types every element `array{nb: 1,
  file: 'y'}`, though the last iteration leaves `$items[$last + 1]` as
  `['file' => 'x']`. The binary before gave `array{nb: 1, file?:
  'y'|'x'}` (itself missing the element without `nb`). Done: the element
  override applies only when the body writes the subject at no other key,
  with a fixture.
- [ ] **F84. A template bound two siblings away still reads as `mixed`**
  (`build_template_params`; `2a31cc0c` substitutes one level). With
  `@template X of int`, `@template E of X`, `@template R of Holder<E>`
  and `Holder`'s `T of int`, `R` still reports `generics.notSubtype` on
  `@param` and `@return`: `E`'s substituted bound is `X`'s placeholder,
  whose bound is `mixed`. Done: siblings substitute to a fixed point
  (a cycle stopping at the placeholder), with a fixture.
- [ ] **F85. A write through several literal keys fixes the shape but
  not the keys' entries** (`783727da`, with F48). `$row[$key] = null`
  with `'id'|'name' $key` now types `$row` as `array{id: 1|null, name:
  'n'|null}`, but `$row['id']` still reads its per-key entry, `null`, so
  `$row['id'] === null` still reports `identical.alwaysTrue`, and the
  container and its element now disagree. A key the shape lacks becomes
  optional with the written value only, even when the shape is open:
  `array{a: 1, ...<string, int>}` written at `'a'|'b'` gains `b?: 'z'`,
  though `b` may hold an `int` from the tail. Done: F48's entry union,
  and a missing key taking the tail's value type too, with fixtures.

### Design

- [ ] **F20. Nothing enforces the stack guard** (`041fcc46`). It is a
  convention on about 200 hand-picked recursive functions, with no lint or
  test catching a new recursive walk without one. The types crate depends
  on `syntax` for `stack::guard` alone. Done: a test drives every
  `Visitor`/type traversal at depth, or a clippy-style check lists the
  recursive functions, and the guard lives in a crate both can use without
  the parser.
- [ ] **F21. The error type has no central rule** (`7ec336de`). Each
  "is this unknown" check has to learn `is_error()` by hand;
  `7ec336de` adds one more. Done: `is_error` folds into the one
  unknown-type predicate the checks already share, so a new check can't
  miss it.
- [ ] **F22. Smaller design points:**
  - `1c301f1b`: `walk_expr_in_scope` skips a closure's `use (...)` list,
    which PHP reads in the enclosing scope.
  - `f81c5e70`: equal unions in different orders display differently, so
    a `Type`-keyed cache can make a message's member order depend on
    evaluation order.
  - `b3412c22`: `with_own_templates` matches a template's scope by the
    bare method name, so a class template scoped to a same-named class is
    taken as the method's own.
  - `ab8d480d`: `allow_except_param_flags` (disallow when a flag is set)
    requires every value to match, so a flag that may be set is let
    through.
  - `e3b91513`: `ImpurePoints` looks up `__construct` by exact case, and
    counts a class without a constructor as an unknown instantiation.
  - `1cb1e25d`: a union of callable names (`$b ? 'sort' : 'count'`)
    isn't resolved for the by-reference check.
  - `daa88e7c`: case folding is certain regardless of the configured PHP
    version, but final sigma differs before 8.3.

  Done: each is decided and fixed or ledgered.
- [ ] **F23. Fixtures that lost what they tested.**
  - `15b5b494`: the arithmetic-operand fixture's string cases now see
    `int|float|string` (after `$s++`), so it no longer tests a string
    operand for post-decrement or `**=`. The `**=` report vanished for
    that reason.
  - `43758de2`: the default-value message shows the bound-resolved
    `array<(int|string), int>` rather than the declared `array<T>`.

  Done: the fixtures test what their names say.

- [ ] **F33. Design points from commits 100 to 155:**
  - `2750baf2`/`1b7022ad`: the infer cache assumes `infer_expr` depends
    only on the node and the scope version. Checker state it also reads
    (the current class and function, `$this`, the facts-only and
    member-reference flags) isn't in the key, and nothing checks the
    assumption. It holds today because each outermost `check_expr`
    resets the cache. A debug-build cross-check (recompute on a hit and
    compare) would keep it true.
  - `027811e4`: an anonymous class's name embeds the file path as given,
    so a message depends on how the project argument was spelled (and is
    absolute under the LSP). `anonymous_class_type` falls back to `mixed`
    silently when a caller skipped `name_anonymous_classes`.
  - `a25d77c6`: the per-thread memo clears all 65,536 entries at once
    when full (a cliff), and a shard's dead-weak buckets wait for the
    shard to double.
  - `ba7ffe83`: lossy character counting from a mark inside an invalid
    UTF-8 run can differ from counting from the line start.

  Done: each is decided and fixed or ledgered.

- [ ] **F58. A hover or definition request clones the enclosing scope
  once per variable in the file** (`ae18f21d`). `lsp::navigation`'s
  `record_var_read`, `record_var_self`, `instance` and
  `record_receiver_member` build their `Resolved` value - including
  `probe_scope()`, a clone of the enclosing `FunctionDecl`, or of the
  file's whole top-level statement list in a script - before `record`
  checks whether the span is at the cursor. The pre-window walker checked
  the anchor first for variable reads and instance members. So
  `resolve_at` clones that scope once for every variable read and member
  access in the file, not once for the one under the cursor: quadratic in
  a long top-level script. Done: the value is built only when the anchor
  matches (a closure passed to `record`, or the check hoisted), and
  `collect_all` builds the probe scope once per scope.

- [ ] **F77. A file that doesn't parse is skipped without a report**
  (`Session::load`; also before the window). `read_and_parse_file` gives
  `None` for a parse error and the file is only counted: `0 file(s)
  checked, 0 diagnostic(s) (2 file(s) skipped: parse error)`, with no
  location and exit status 0. Each commit in the window that makes more
  input a parse error (`474920e5`, `f4128dec`) turns a file that was
  checked into one skipped silently. Done: a parse error is a
  non-ignorable diagnostic at its span, and it fails the run.
- [ ] **F78. The duplicate-declaration checks walk every name for every
  file** (`finish_diagnostics`, `project.rs`). With either opt-in on,
  each file's diagnostics loop over every recorded name in the project
  and keep the sites in that file: files times names. Done: the checks
  run once over the sites and hand each file its diagnostics.
- [ ] **F79. Smaller gaps read in the submodules** (read, not probed):
  - `compute_doctrine_entity` makes every property a mapped field,
    `#[ORM\Column]` or not, so a DQL path to a transient property isn't
    reported; a positional `#[ORM\Column('col')]` or `#[ORM\Table('t')]`
    name is ignored.
  - `check_composer_hygiene` records `extension_loaded('Zend OPcache')` as
    `zend opcache`, but the extension is named `zend-opcache`, so the
    probe doesn't silence `ext.notRequired`.
  - `check_yield_from_compatibility` compares key and value types with
    `rule_level_accepts` and the send type with `accepts`.
  - `check_architecture`'s `should_use_trait` reads the class's own
    `uses()`, while the `uses_trait` selector reads every ancestor.
  - `compute_attribute_targets` takes any attribute whose last segment is
    `Attribute` for the meta-attribute.
  - `mock.withWithoutExpects` reads one statement's chain, so `$m =
    $mock->expects($this->once()); $m->method('x')->with(1);` is
    reported.

  Done: each as stated, with a fixture where the output changes.

- [ ] **F86. `c0c51bca` landed with two fixtures failing.** Its
  `unseal_with` gives an open shape's tail its types, which changes the
  expected output of `an_open_shape_join_keeps_the_tail_type_for_a_one_sided_key`
  and `array_shape_writes_across_several_independent_branches_converge_instead_of_blowing_up`
  (both reach `array_merge`); `41e2198d` updated them two hours later.
  Done: nothing to change in the code; a fix commit's scoped test run
  includes the fixtures its function reaches.

- [ ] **F87. A discarded pass still leaves some records behind**
  (`Checker::pass_mark`, `cc4e6b5e`; read, not probed). `rewind_to`
  covers diagnostics, semantic tokens, throw and return points and inlay
  hints. What a discarded loop pass adds elsewhere stays:
  `record_throw_state`'s entries in the enclosing `try`'s frame and
  `property_assign_facts`. Both only gain the narrower types of earlier
  passes, so neither changes an outcome today. `named_argument_call_facts`
  is deduplicated by a linear `contains` per push instead of being
  rewound. `check_trait_methods` checks a trait's methods once per host
  and dedups the diagnostics only, so its semantic tokens and inlay hints
  still repeat per host. Done: every per-pass record is in `PassMark`
  (or keyed so a repeat can't add), and the trait re-check rewinds the
  LSP records after its first host.

### Comments

- [ ] **F24. Comments the fixes left wrong:**
  - `parser/tokens.rs:114-119`: `take_token`'s soundness note cites
    "`control_flow.rs`'s `elseif`-rewrite backtrack" as the one index
    other than `self.pos`. No backtrack exists; the parser reads only at
    `pos`/`pos + k`.
  - `parser.rs:26-27`: "every existing `self.tokens[...]` access
    throughout this file (there are many)" - there is one
    (`tokens.rs:121`).
  - `408caaf4` left 29 roadmap sub-phase codes (`168a`-`168k`) as the only
    name for a component. They are in `doctrine/dql.rs`,
    `doctrine/query_builder.rs:39-62`, `doctrine/result_shape.rs`,
    `result_shape/tests.rs:123`, `member_resolution.rs:715`,
    `sql_reflection/resolve.rs:332`, and
    `sql_reflection/query_builder.rs:358` - that one in a line the commit
    itself edited.
  - `union.rs:663-667` (`637f1b67`): `shape_as_general_array`'s doc is
    glued above `general_array_parts`', so one function has two docs and
    the other none.
  - `15b5b494`: "expects resource, resource given" - a class named
    `resource` and the resource type print alike.

  Done: each says what the code does.

- [ ] **F34. Comments from commits 100 to 155:**
  - `coercion.rs` `to_number`'s doc says a non-numeric literal like
    `'5 apples'` throws on PHP 8. PHP 8.5 gives `"5 apples" + 1` as `6`
    with a warning; only a wholly non-numeric `'abc'` throws. Reporting
    the leading-numeric case may stay as a choice; the doc's claim goes.

  Done: the doc says what PHP does.

- [ ] **F37. Comments the final state still gets wrong** (read whole,
  below).
  - `union.rs` (lines ~161-184): the 23-line `array{}`-absorption
    paragraph still sits above `merge_int_ranges`, describes
    `reduce_array_members`, and cites the deleted `index_result_type`. The
    Stage B review's F53 lists this exact bullet and is marked done.
  - `union.rs`: the `mixed` absorption comment narrates history ("before
    its own analogous fix", a `nullsafe.neverNull` false positive).
    `merge_has_offset_value_pair`'s doc tells a corpus case
    (`ActivityService::record()`), claims PHPStan's "observed behavior
    exactly", and names a "merge multiple Array members" step that is now
    `merge_general_arrays`.
  - `narrowing.rs`: `narrow_to_instance`'s doc opens with
    `Relation::narrow`'s old doc, fused on. It says `narrow`
    "unconditionally trusts `target`" for a non-union, which is false now:
    it keeps a subtype `original`.
  - `narrowing.rs`: `narrow`'s fallback comment tells a Piwigo case
    (`$image_id === $this->sessionService->getRefererImageId()`);
    `without`'s arms carry narration ("confirmed via `php -r`", "no case
    this could ever get wrong", "a real gap ... this project doesn't
    attempt yet"); and `without_falsy`'s last arm describes "the old
    hand-rolled arm".

  - `relation/accepts.rs`, the generic-arguments arm:
    - It says a missing slot can't take a default, because
      `ClassReflection` "only persists `template_bounds`, never a
      class-level `@template T = Default` map". `template_defaults` exist,
      and `4c88bf44`/`5e109cbd` read them.
    - It says unions have "no canonical member order", which is false
      since `f81c5e70`.
    - It says call-argument binding "only ever binds a template used as a
      bare param type", which is false since `b274f947`.
  - `accepts.rs`: the `Array(Mixed)` leniency's comment says an array
    literal's element type "is never inferred precisely by this checker",
    while the arm two below says a literal now carries its shape. The
    Intersection-vs-Intersection arm "mirrors" an `is_super_type_of`
    list arm that `69c76cef` removed.
  - `accepts.rs`: the object-shape arm compares `class == "stdClass"` by
    case (code rather than a comment, but the same sweep).

  Done: each says what the code does, on the item it documents.

- [ ] **F43. Comments in the types modules read whole in the second
  final-state pass:**
  - `lib.rs`: `Repr::HasOffsetValue` names its construction site
    `checker::assignment::write_literal_key_aware`, which is now in
    `types::offsets`; `Repr::IntRange` opens "A `T` range" for an int
    range.
  - `display.rs`: the `HasOffsetValue` arm says no fast path of
    `describe_accessory_intersection` matches an intersection holding one;
    the `Array` path does (F42).

  Done: each says what the code does.

- [ ] **F51. Comments in the checker modules read whole:**
  - `control_flow.rs`: `merge_scopes`'s doc ("Joins two branch-local
    scopes...") and its `#[hotpath::measure]` sit above
    `add_implied_fact`, which was inserted between them, so the profile
    measures `add_implied_fact` under that doc and `merge_scopes` has
    neither.
  - `stmt.rs`: `statement_defined_variable_names`'s doc ("Every
    bare-variable name ... Deliberately doesn't cover `foreach`") is
    fused above `bare_assert_condition`'s. That paragraph and the
    post-dispatch comment in `check_stmt_impl` ("Deliberately does NOT
    extend this to a tag sitting directly on a `foreach` header") are
    stale: `check_foreach` takes the statement's inline `@var`s, and
    `statement_defined_variable_names` has a `Foreach` arm.
  - `member_resolution.rs`: `method_call_return_type`'s doc
    ("`infer_expr`'s own `ExprKind::MethodCall` return-type
    resolution...") is fused above `class_argument_return`'s;
    `object_method_call_return_type` cites "the same
    synthesized-generic-slot convention 168e already established", a
    task reference.
  - `builtin_calls.rs`: the comment above the `range` arm says
    `array_pop`/`array_shift` are "deliberately NOT special-cased", and
    an `array_pop | array_shift` arm follows; `get_defined_vars`'s
    comment sits above the `mysqli_query` arm; `array_merge`'s comments
    name `checker::assignment::write_literal_key_aware` (now in
    `types::offsets`) and narrate ("closing the gap this arm used to
    leave", "Loosened from an exact `string` match");
    `sscanf_result_fields` says its fields are keyed by strings
    (`"0"`, `"1"`), and it builds `ShapeKey::Int`.
  - `expr.rs`: the `Index` arm says it "calls the shared
    `index_result_type`", which no longer exists (it calls
    `offset_value_type`); `widen_coalesce_result`'s doc says `??`'s
    result "is usually just `r`'s type" and that widening is "not a
    blanket always-union", while the code unions the left side's
    non-null type with `r` in the general case; "A synthetic
    `target::method(args)` call" is the doc of `unhandled_match_error`
    rather than `static_call`; the first-class-callable arm narrates
    ("Closes a real, live bypass ... previously went unchecked").

  Done: each doc sits on its item and says what the code does.

- [ ] **F59. Comments in the registry, project, LSP and syntax modules
  read whole:**
  - `project.rs`: `update_file`'s doc ("Re-parses one already-loaded
    file...") is fused above `unindex_file`'s; plan references ("VSCode
    plan item 2.2", "(3.2)", "this item's own open-risks note", "1.1's own
    `phpstan-rs.toml` `paths`"); narration ("`unindex_file` used to run
    first unconditionally", "a real bug on an earlier pass of this exact
    method").
  - `lsp/navigation.rs`: the module doc corrects itself mid-sentence
    ("`lsp::document_highlight`... no such module - see
    `Server::document_highlight`").
  - `project/use_sites.rs`: the `Name` arm says a bare `Name` there is
    "e.g. a first-class-callable target `foo(...)`"; that is a `Call`
    (handled above), so a bare `Name` is a constant, recorded as a
    function.
  - `registry/purity.rs`: "Any static property access at all" labels the
    arm that also disqualifies `ClassConstFetch` and `ClassNameFetch`
    (F56).
  - `syntax/ast.rs`: `PropertyDecl::hooks` says hook bodies "aren't
    type-checked" (`checker/oop/property_hooks.rs` checks them);
    `FunctionDecl::body` says `FunctionReflection::from_decl` shares it
    into a `body` field (there is none; bodies are found by provenance)
    and cites `src/intern.rs`; the `PropertyAccess` comment describes a
    trailing `bool` that is now `ObjectOperator`; `StaticCall` narrates
    the `"?"` sentinel it replaced; `LogicalAnd` narrates "the way this
    comment used to argue for"; `UseImport::kind` says there is no
    global-constant registry; `Stmt::doc_comment` cites
    `inline_var_annotation` (now `inline_var_annotations`).
  - `syntax/parser`: `parse_function_decl` says methods come from
    "`parse_class_body`/`parse_enum_decl`'s own member loops" (an enum
    body has used `parse_class_body` since `1cdfe80d`); `parse_enum_decl`
    cites "`ClassDecl.attrs`'s identical note" (the field is
    `ClassDef::attrs`).

  Done: each says what the code does, on the item it documents.

- [ ] **F80. Comments in the submodules read whole:**
  - `checker/expr/array_ops.rs`: a paragraph on `array_element_type` for
    literal shapes heads `check_array_item_key_type`'s doc; the spread
    comment narrates "the original check" and "confirmed via
    `dumpType()`".
  - `checker/expr/ranges.rs`: `null_probe_verdict` says it serves a bare
    variable only; offset and property chains use it too.
  - `checker/control_flow/generators.rs`: the `generator.void` message
    says "it's return type".
  - `registry/class_info/doctrine_entity.rs`: the module doc cites Piwigo
    and "the real corpus this phase targets"; `DoctrineEntity`'s doc says
    inheritance isn't modeled, where the module doc says the discriminator
    map is.
  - `project/duplicate_fn_sites.rs` says the sites are "always populated,
    since `check_duplicate_function_declarations` needs to run by default
    ... always-on"; the check is opt-in. `duplicate_functions.rs` sends the
    reader there for "the concrete reason (phpstorm-stubs' own real `strtr`
    ...)", which isn't given. `duplicate_classes.rs` and
    `duplicate_class_sites.rs` argue from corpus evidence.
  - `project/whole_program/dead_members.rs`: the module doc narrates a
    Piwigo measurement and past fixes ("used to have", "used to be a real
    gap here too", "Fixed at the parser level").
  - `project/mock_hygiene.rs`: `native_members`' doc starts with
    `union_members`' first line.
  - `project/session.rs`: `composer_roots`' doc sits on
    `declared_php_versions`.
  - `syntax/parser/tokens.rs`: `take_name_segment` cites
    `Piwigo\Common\Enum\Foo`; `parse_leading_attributes` says "before
    this phase".
  - `syntax/visit.rs`: the module doc says every `visit_*` default walks
    and the walk reaches type declarations; `VisitorMut::visit_type_hint`
    is a no-op and the mutating walk visits no names.

  Done: each says what the code does, on the item it documents.

- [ ] **F88. Comments that still describe the truncation `cc4e6b5e`
  replaced:** `converge_loop_body`'s doc calls discarding a pass "the
  existing `diagnostics.truncate` pattern", and `check_while`'s comment
  says every pass but the kept one "has its diagnostics discarded via
  `converge_loop_body`'s own `diagnostics.truncate`"; both now go
  through `rewind_to`, which drops more than diagnostics. Done: both
  name `rewind_to`.

## The Stage B review's done items

Checked against the commits each item names, the diffs read above, and
probes. An item in bold is the Stage B review's (its own numbering); a
finding in parentheses is this review's. A `[ ]` marks an item whose
claim doesn't hold in full; the finding names what is missing. V1 wasn't
re-run here, since this review builds nothing.

### Validation and Review 1

- [x] **V1** (`a782b0d3`, `96027afd`): taken as recorded, not re-run.
  The fixtures bless two findings: F6's `$this` expectation and F23's
  lost operand coverage.
- [x] **R1-1** to **R1-9** and **R1-L**. These predate the window; the
  final state agrees:
  - `failing_level` is the one level rule, with a template's bound split
    by `4499b58d`;
  - `equals_for_union` is gone;
  - lists key on `int<0, max>`, and the fixture changes in `f9be84b3`
    follow from it.

### Review 2

- [ ] **C1** (`f18bc022`). The union case is right. A lone member
  outside the checked type now narrows to that type, not `never`: F4.
- [x] **C2** (`637f1b67`).
- [x] **C3** (`a5c24e13`).
- [x] **C4** (`32929ce4`).
- [x] **C5** (`1f4b45bf`). Only its doc is wrong about `'5 apples'`
  (F34), and `binaryOp.invalid` ignores the version it adds (F32).
- [x] **P1** (`b9ca6c05`, then `607a43b3`).
- [ ] **P2**. The nested-call and nested-array figures hold. "A
  20,000-term `.`/`+` chain 4 s" holds for variable chains only:
  literal concatenation and `&&`/`||` chains are superlinear (F18).
- [ ] **P3** (`25ee9d62`). The append is exact, but a next index that
  equality ignores merges branches wrongly (F25).
- [x] **P4** (`2f1e256a`).
- [ ] **P5** (`b3412c22`). A method on a generic receiver keeps the
  class's templates (F2).
- [ ] **P6** (`67ed2a1d`). `mixed~S` survives a join with a member inside
  `S` (F26).
- [x] **P7** (`ea4d7175`).
- [x] **P8** (`c78732bc`). Typing every settled type-check call also
  turns F27's `is_resource` into a constant.
- [x] **P9** (`d4ae7678`). The cross-file trait case was probed; the CPU
  figures were not re-measured.
- [x] **T1** to **T6** and **T8**.
- [ ] **T7**. `union.rs` has a new fused doc from `637f1b67` (F24).
- [x] **S1** to **S11**. No `"?"` sentinel and no `"class@anonymous"`
  string match remain; `ExprKind::Variable` holds a `Name`.

### Full review

- [x] **F23** (`041fcc46`, `288f4fda`). No deep input crashes (67 deep
  probes); the costs left are F18 and F20.
- [x] **F29** (`e5db6811`).
- [x] **F22** (`d816c5f7`).
- [x] **F35** (`327da348`, `f9be84b3`).
- [x] **F37** (`0fbd42f2`).
- [ ] **F38** (`b274f947`). Every listed position binds, but an argument
  outside the bound isn't reported (F36).
- [x] **F39** (`97472c63`).
- [x] **F41** (`04e500b3`).
- [x] **F54** (`3f04ac3f`). The parse is right; the class expression goes
  unchecked (F17).
- [x] **F55** (`1c301f1b`).
- [x] **F56** (`a62b2c77`).
- [x] **F58** (`139709f2`).
- [x] **F59** (`b895fa8d`).
- [x] **F60** (`89da4c25`). Loop widening drops non-empty (F19).
- [x] **F62** (`48f02f39`).
- [ ] **F63** (`22d4ff21`). A property's unresolvable type is still
  declared and reported three ways (F12).
- [x] **F64** (`7d08a968`).
- [ ] **F65** (`b3412c22`): F2.
- [x] **F42** (`516fc901`).
- [x] **F1** (`b09b0cea`).
- [ ] **F2** (`43349c1a`). The variable is declared in the true branch
  only, but typed as the error type there (F5).
- [x] **F3** (`6e274bf3`), **F4** (`8e4ef41e`), **F24** (`8a5ece52`),
  **F30** (`ff36a896`), **F31** (`48b1f80f`), **F32** (`b5ffd642`).
- [ ] **F33** (`8febbd1e`). There is one resolver, but a positional key
  after an unknown one is wrong (F13).
- [x] **F34** (`d41a83bf`).
- [x] **F43** (`15b5b494`). Its fixture lost the string-operand cases
  (F23).
- [x] **F44** (`88effc4f`), **F46** (`2b1ce22b`), **F47** (`dedc1ad8`),
  **F48** (`05fb20f6`), **F57** (`16dd8365`).
- [x] **F5** to **F10** (`e06f470c`, `daa88e7c`, `1c0a6670`,
  `f4128dec`).
- [x] **F11** (`b224eb02`).
- [ ] **F49** (`d89ea9e5`). `NAN === NAN` is reported always true (F11).
- [ ] **F50** (`ac00409c`). Reaching a template's bound makes the
  already-resolved bound resolve twice (F1).
- [x] **F51**, **F52**.
- [x] **F12** (`a70a5b42`). It is also where F1's second resolution
  sits.
- [x] **F13** (`d82888a8`). The use-site index it moved keeps F7.
- [x] **F14** (`a0d1db44`, `5f84933b`). Member-name sets still compare
  by case (F35).
- [x] **F15** to **F20**.
- [ ] **F25** (`11b428f4`). The error type comes from quiet reads too
  (F5), and has no central rule (F21).
- [x] **F26** (`bd7d7a89`). A native `\int` hint is read as a class
  (F19).
- [ ] **F27** (`f25c642c`..`ce055dde`). The use-site Visitor skips
  attribute names (F8).
- [x] **F28**. The `:` case is fixed; other bare-`yield` endings still
  fail (F10).
- [x] **F36** (`f50e2bd0` and the rest). `321d388c`'s iterable and
  `IteratorAggregate` changes are F3 and F14.
- [x] **F21**.
- [ ] **F53** (`408caaf4`). 29 roadmap sub-phase codes remain (F24),
  and its `union.rs` empty-shape paragraph is still there (F37).

### The Stage C review

Since the Stage C review was written, `e06f470c` fixed its O1 (control
characters print escaped), and `1cdfe80d` fixed its F5 (an enum's
`@phpstan-type` and constant `@var` probe clean) and one bullet of its
F13 (`from_enum_decl`'s doc: the function is gone). That ledger doesn't
say so yet.

## Full review: coverage

Every commit's full diff (`crates`, `xtask`, `gates`), read in order,
whitespace changes aside. A commit is ticked once its whole diff has been
read and its findings added above.

- [x] `041fcc46` fix: no input's depth can crash the process
- [x] `288f4fda` refactor: an if statement holds its elseif chain flat...
- [x] `e5db6811` fix: integer overflow no longer crashes checking
- [x] `d816c5f7` fix: an object compared with an enum case its enum...
- [x] `327da348` fix: a non-final class is no longer treated as final
- [x] `0fbd42f2` fix: a shape's key type is its literal keys, not string
- [x] `b274f947` fix: template inference reads every position and joins its...
- [x] `97472c63` fix: `value-of<T>` over a template is kept until a call binds...
- [x] `5c94a727` docs: the crate-split plan's C7 resolver takes the PHPDoc...
- [x] `04e500b3` fix: below level 7 a union argument is accepted when some...
- [x] `3f04ac3f` fix: new takes a variable with its property, offset and...
- [x] `1c301f1b` fix: an anonymous class's constructor arguments belong to...
- [x] `a62b2c77` fix: simple string interpolation reads PHP 8's nullsafe...
- [x] `516fc901` fix: native-type suggestions are declarations PHP compiles
- [x] `b09b0cea` fix: exit and die are the functions PHP 8.4 makes them
- [x] `43349c1a` fix: isset($x) and empty($x) no longer declare $x for the...
- [x] `6e274bf3` fix: unset($a[k]) removes the offset from the array's type
- [x] `8e4ef41e` fix: private methods are used whatever case the call spells...
- [x] `8a5ece52` fix: the banned-code and composer-fact walks see all code
- [x] `ff36a896` fix: a remainder's range follows PHP's rule
- [x] `48b1f80f` fix: &, | and ^ over two strings are byte-wise strings ('a'...
- [x] `b5ffd642` fix: ++ and -- follow PHP on strings and null
- [x] `f7def3af` style: the numeric-string test writes 2^63 without excess...
- [x] `8febbd1e` fix: array keys resolve as PHP stores them
- [x] `d41a83bf` fix: an unknown class anywhere in a type is reported
- [x] `15b5b494` fix: accepts rejects what PHP would
- [x] `88effc4f` fix: a conditional type over a class resolves against the...
- [x] `2b1ce22b` fix: the missing-typehint walks reach every nested type
- [x] `dedc1ad8` fix: an int range has one constructor, and it normalizes
- [x] `05fb20f6` fix: numeric strings are recognized as PHP's is_numeric() does
- [x] `16dd8365` fix: an interpolated "$a[01]" reads the string key '01'
- [x] `139709f2` fix: the key appended after a negative key follows the PHP...
- [x] `b895fa8d` fix: a declaration anywhere in a file is registered
- [x] `89da4c25` fix: a float literal is its value
- [x] `48f02f39` fix: parent without a parent class is reported once
- [x] `22d4ff21` fix: an unresolvable PHPDoc type is ignored
- [x] `73c5cd11` docs: the resolver list of C7 takes the caller side of F63...
- [x] `e06f470c` fix: a string key is its bytes and a string literal...
- [x] `daa88e7c` fix: mb_ case functions fold by Unicode where the answer is...
- [x] `1c0a6670` fix: a string's quote escapes follow its delimiter
- [x] `f4128dec` fix: a leading-zero literal with an 8 or 9 is a parse error
- [x] `b224eb02` fix: isset() and empty() narrow both branches
- [x] `d89ea9e5` fix: disjointness proves what the types rule out
- [x] `ac00409c` fix: late types and traversal see every part of a type
- [x] `30f9652c` fix: an accessory-refined string shows every refinement it has
- [x] `aa193395` fix: the type model's small gaps close
- [x] `52740c36` style: a format-string test passes its format without a...
- [x] `e2c11010` refactor: one test says what a constructor is
- [x] `57b7f6e7` refactor: parentheses and @ are peeled in one place
- [x] `474920e5` fix: what PHP refuses to parse is a parse error
- [x] `ad2a5635` fix: the parser refuses what PHP refuses
- [x] `df5e82f3` test: (real) on PHP 8.5 is reported removed, not deprecated
- [x] `40e086a6` fix: a list intersected with a shape keeps to list keys
- [x] `fda237d8` fix: a closure rebinding reaches private members however...
- [x] `680bd8ef` fix: an arrow-form hook body is its own function scope in...
- [x] `ab9054f8` refactor: the global-variable walk takes statement slices...
- [x] `fcdd7f0a` refactor: only nodes something reads carry a NodeId
- [x] `11b428f4` fix: an expression reported invalid is the error type, so...
- [x] `bd7d7a89` refactor: a native type keyword is its own type-hint node...
- [x] `e5301ef6` refactor: a destructuring target is its own pattern node
- [x] `a70a5b42` refactor: names written in code are read from what name...
- [x] `d82888a8` refactor: one iterator over a file's declarations
- [x] `a0d1db44` perf: an interner holds its entries weakly, so text nothing...
- [x] `5f84933b` refactor: class, function, member and variable names are...
- [x] `bfde069e` fix: a static variable's assignment site is the variable...
- [x] `1cdfe80d` refactor: an enum is a class-like like any other
- [x] `f25c642c` refactor: signature help walks with a Visitor, overriding...
- [x] `addc7c39` refactor: the class and function use-site index walks with...
- [x] `ae18f21d` refactor: navigation for the LSP walks with a Visitor...
- [x] `6f08ee63` refactor: the unused-member and unused-variable scans walk...
- [x] `31cd6f2f` refactor: include-path guards are found by a Visitor over...
- [x] `07675ccc` refactor: function-like declarations are found by one...
- [x] `6e52272a` refactor: syntax::visit gains for_each_child_expr and...
- [x] `2c5a3b5c` refactor: the PHPStan API rules walk with a Visitor that...
- [x] `e3b91513` refactor: purity is classified and its impure points...
- [x] `ce055dde` docs: the navigation and signature-help module docs...
- [x] `f50e2bd0` refactor: the checker asks kind questions through the...
- [x] `9b331b26` refactor: a conditional type's subject is a...
- [x] `7ec336de` fix: a dynamic class-constant fetch whose name is already...
- [x] `321d388c` refactor: a type string parses with its class names...
- [x] `f81c5e70` refactor: a union's or an intersection's members compare...
- [x] `bc7bd7c1` refactor: the types crate's operator arithmetic and range...
- [x] `4499b58d` fix: a rule's level judges a template bounded by a union by...
- [x] `465ad3ec` fix: a {$expr} or ${expr} in a string takes its text from...
- [x] `fbf3811f` fix: excluding a type drops a lone value that is a subtype...
- [x] `1cb1e25d` fix: the pipe operator's by-reference check resolves its...
- [x] `b10c01cc` docs: the comments the type-language rewrite left broken or...
- [x] `b1dddb6c` fix: division by zero is judged on the divisor's type, so...
- [x] `43758de2` fix: a parameter's default is checked against its type with...
- [x] `ab8d480d` fix: a disallowed-call parameter-flags condition reads the...
- [x] `d82fe912` docs: the types crate's comments from the type-language...
- [x] `3e80be89` fix: the numeric pseudo-type is int|float|numeric-string...
- [x] `45ea7652` docs: comments cite functions and types that exist, drop...
- [x] `408caaf4` docs: comments name the components they mean instead of...
- [x] `7d08a968` fix: a braced {$...} in a string must hold a variable...
- [x] `b3412c22` fix: a first-class callable's type is a Closure with the...
- [x] `f18bc022` fix: narrowing keeps the members that are subtypes of the...
- [x] `637f1b67` fix: a write at a dynamic key keeps the array's existing...
- [x] `9a2c53e7` fix: a PHPDoc float literal keeps its float spelling when...
- [x] `a5c24e13` fix: truthiness removes every falsy value of a kind
- [x] `32929ce4` fix: an array intersection carrying hasOffset or...
- [x] `1f4b45bf` fix: on PHP 8 a non-numeric string isn't castable to a...
- [x] `b9ca6c05` fix: a union past its member cap generalizes its literals...
- [x] `c78732bc` fix: a type-check call whose answer the argument's type...
- [x] `ea4d7175` fix: a value inferred for a class template that isn't...
- [x] `2f1e256a` fix: excluding an enum case from its enum leaves the other...
- [x] `c58bba2a` fix: the assertIsX and assertIsNotX assertions of PHPUnit...
- [x] `25ee9d62` fix: an append onto a sealed shape takes its next index...
- [x] `67ed2a1d` fix: mixed remembers the values a check excluded (isset($x)...
- [x] `1ecad5c5` test: a union past its cap of distinct shapes generalizes...
- [x] `2750baf2` perf: infer_expr remembers each expression's type by scope...
- [x] `d71abba1` fix: cloning or debug-printing a deeply nested type goes...
- [x] `07648b5e` perf: a type's nested types and shapes are shared (Arc), so...
- [x] `8c36d84f` perf: the constant-comparison checks look at the operator...
- [x] `f51447bd` perf: a call to a function declared outside the stubs skips...
- [x] `65a73cd1` refactor: the types crate rebuilds a filtered union through...
- [x] `6ecec75d` refactor: a union is normalized where it's built
- [x] `69c76cef` refactor: the list-against-list arm of is_super_type_of...
- [x] `c6c59353` refactor: making an array non-empty, a list, or sure of a...
- [x] `a3853ad8` fix: a union drops a member another member contains...
- [x] `18708302` refactor: a member access names its operator...
- [x] `e03f6ac9` fix: a dynamic member access ($x->$name, $x->{$name}())...
- [x] `096a444f` refactor: a function's body is optional (FunctionDecl.body...
- [x] `ac5240a9` refactor: a promoted constructor parameter carries one...
- [x] `8fdbd80a` refactor: a property's get and set hooks are each one...
- [x] `a7a86e6f` docs: a string literal's and an anonymous class's AST docs...
- [x] `3a5b36ca` refactor: function, property, constant, enum-case...
- [x] `ab1dcd7f` refactor: a QualifiedName carries its own span, so...
- [x] `4ed3cfe3` refactor: a class keeps each trait use statement with its...
- [x] `243a4971` test: a caught unknown class and @use tag diagnostics...
- [x] `027811e4` refactor: a class declaration is a name bound to a ClassDef...
- [x] `252ce9f9` style: the anonymous-class signature help changes are formatted
- [x] `9b3b12ff` fix: ++ and -- on a never-typed operand (unreachable code...
- [x] `daecae6a` fix: a nullable native type adds null to a PHPDoc type...
- [x] `707776e2` fix: the registry's class-keyed owner maps, ancestor...
- [x] `4c88bf44` fix: a generic class given no type arguments binds each...
- [x] `3b84c78f` feat: array_key_first and array_key_last of a sealed array...
- [x] `a5b545ce` fix: a nullsafe access short-circuits the rest of its chain...
- [x] `82de9b70` test: the assertion-table fixtures assert the types they...
- [x] `f9be84b3` test: fixtures expect the settled behaviour
- [x] `5e109cbd` fix: an erased receiver's template is its default or else...
- [x] `80ae3e56` fix: the pre-padded Generator and iterable short forms are...
- [x] `224216dd` perf: union absorption compares each member only against...
- [x] `9ed83bcd` test: a template bounded by a union with a scalar is a...
- [x] `34c2912d` perf: a type is a shared node carrying its hash
- [x] `af0808cd` refactor: a type's children are types themselves, not `Arc<Type>`
- [x] `a25d77c6` perf: types are interned
- [x] `ba7ffe83` perf: the lexer resolves a column from the nearest...
- [x] `0b97dadf` perf: a relation between single values (literals, true...
- [x] `607a43b3` feat: a union keeps up to 64 members before generalizing...
- [x] `291cb15f` perf: package ownership compares a file, normalized once...
- [x] `7b0fbcc0` perf: an include target is matched only against the files...
- [x] `5547d2ce` perf: package ownership looks a file's ancestors up among...
- [x] `1b7022ad` perf: a chain link's type is cached with the expression's...
- [x] `31d6c5a7` style: the composer crate's lookup maps are FxHashMaps, as...
- [x] `d4ae7678` perf: private property types are judged after the one
  checking pass...
- [x] `5400b17b` test: a dynamic member access on a nullable receiver
  expects the possibly non-object...
- [x] `96027afd` chore: the stub index regenerated - a float constant's
  value prints the way PHP shows...
- [x] `9cef98e6` fix: resolving the class names a type is written with
  leaves a template placeholder alone...
- [x] `3cea2a20` fix: a template binds an argument its bound holds once
  the bound's own templates are bound...

- [x] `9cfad246` fix: a loop widens an array shape that keeps its keys
  field by field... (sound on its own; `f0ad71f8` builds on it: F81)
- [x] `c0c51bca` fix: array_merge's open shape takes the other keys'
  types... (F86)
- [x] `2a31cc0c` fix: a template named in a sibling's bound or default
  carries its own bound... (F84; class-level and `@method` templates
  probed, no report either way)
- [x] `9b1c07d5` fix: a builtin's return rules apply to a polyfill of
  it too... (a namespaced function by a builtin's name probed: keeps
  its own return type)
- [x] `f0ad71f8` fix: a loop widens a union of array shapes as one
  shape... (F81)
- [x] `1c33a059` fix: what is known about an object's properties is
  forgotten after a call that may change them... (F82)
- [x] `310b2205` fix: a call forgets what is known about an object's
  properties only when it may write them... (F82)
- [x] `41e2198d` test: two open-shape array_merge fixtures expect the
  tail's key and value types (F86)
- [x] `9f3ba74e` fix: an element written at a variable key reads back
  as written...
- [x] `91666577` fix: a foreach over a variable leaves every element as
  the body leaves the element its key names... (F83; `break` and
  conditional writes probed right, and the subject no longer non-empty
  after a loop that may not run)
- [x] `69e25540` fix: an array read at a key whose type is several
  literal keys joins only those keys' values... (numeric-string keys
  probed right; a `bool` key still reads every field, which is sound)
- [x] `02454672` fix: a loop widens an array shape that opens to other
  keys as a shape... (F81)
- [x] `783727da` fix: a write at a non-literal key keeps a shape a
  required field keeps non-empty... (F85)

- [x] `cc4e6b5e` fix: a loop pass that is discarded leaves nothing
  behind... (F87, F88; a throw inside three nested loops probed: reported
  once, and a closure's return inside a loop typed right)

### Final state, module by module

Each module the fixes created or reshaped, read in its final state after
the commit pass. Every other module the window touched was read through
all of its diffs above, but not again whole.

- [x] Created: `syntax::stack`, `syntax::declarations`,
  `syntax::php_float` (checked against `var_export`), `types::numeric`
  (checked against PHP 8.5's `is_numeric` and `++`), `types::intern`,
  `registry::name_table`, and `checker::locals` (every writer bumps the
  version). Only the findings above.
- [x] `types::union`: F25 (a merged shape drops its index), F26, F37.
- [x] `types::narrowing`: F4 (the disjoint fallback), F37. Its
  `Traversable` arms compare the class name by case.
- [x] `types::offsets`: F25 (the stale index after a literal write, found
  here), F19 (a union of shapes).
- [x] `types::shape`: `NextArrayIndex::observe` exists for F25's write
  path.
- [x] `types::relation::super_type`: F11 (the NaN arms).
- [x] `types::relation::accepts`: F37.
- [x] `types::{lib, subtype, templates, parse, queries, arithmetic,
  coercion, display, intersection}`, read at `3cea2a20`: F39, F40, F41,
  F42, F43. `subtype`, `queries`, `coercion` and `intersection` only
  carry what is listed elsewhere.
- [x] The checker's `function_decl`, `assignment`, `stmt`,
  `control_flow`, `narrowing`, `member_resolution` and `builtin_calls`,
  read at `3cea2a20`: F44 to F51. `function_decl` carries none.
- [x] The checker's `expr`: F39's unary bullet, F50, F51, F52, F53,
  F54.
- [x] The registry's `class_info` and `purity`, `project` (with
  `use_sites` and `banned`), `lsp::navigation`, and
  `syntax::{ast, parser}`, read at `3cea2a20`: F7 (extended), F42,
  F55, F56, F57, F58, F59.

The items above read each named file, not the submodule files under it;
those, and two files read only in part, are the items below. Each is read
whole at `43f54302` (the same code as `2a31cc0c`), tests aside.

- [x] The checker's `expr` submodules: `array_ops`, `mysqli`,
  `preg_shapes`, `probe_checks`, `ranges`: F39, F45, F50, F61, F62, F64,
  F65, F70, F71, F80.
- [x] The checker's `stmt` submodules: `loops`, `switch`, `try_catch`,
  `new_exception_discarded`: F63, F67, F68, F69, F73.
  `new_exception_discarded` carries none.
- [x] The checker's `control_flow` submodules: `boolean_conditions`,
  `generators`: F79, F80. `boolean_conditions` carries none.
- [x] The checker's `narrowing` submodules: `assertions`, `predicates`
  (only excerpts reached from call sites were read before): F50, F67,
  F72.
- [x] The registry's `class_info` submodules: `attributes`,
  `doctrine_entity`, `tag_synthesis`, `virtual_property`: F79, F80.
  `tag_synthesis` still builds an `@method` tag's templates against
  unbounded siblings (`2a31cc0c` fixed `build_template_params` only), but
  no check reads those bounds, so nothing reproduces.
- [x] The `project` submodules: `architecture`, `class_shape`,
  `composer_hygiene`, `duplicate_class_sites`, `duplicate_classes`,
  `duplicate_fn_sites`, `duplicate_functions`, `mock_hygiene`, `session`,
  and `whole_program` with its six rules: F66, F74, F75, F76, F77, F78,
  F79, F80. Probed clean: an inherited method's types don't count as the
  subclass's architecture dependencies; `class.notFinal` and
  `method.notFinalInAbstractClass` see anonymous subclasses and
  overrides (and a private implementation of a trait's abstract
  protected method runs in PHP 8.5); a trait-using class doesn't repeat
  its parent's renamed parameters.
- [x] `syntax::visit` (its last mutable walkers were cut off) and
  `syntax::parser::tokens` (its comments were filtered out): F80. The
  mutable walk matches the shared one apart from names and type hints;
  `namespace\foo()` resolves.
- [x] `types::arithmetic`'s `div_int_range` and `sub_int_range_full`,
  checked exhaustively over small ranges against PHP: F39, F60 (the sweep
  covered every integer operator).
