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

Some designs depend on others, and they land in this order:

- F4 first. F53, F16's `never`, F44 and F39's `pow` rely on its
  implicit/explicit `never`.
- F35 before F55, and F22's `ImpurePoints` bullet.
- F41 before F14 and F19's `1e25`.
- F21 early. It removes hand-written `is_error()` checks that later
  fixes would otherwise copy.
- F82 before F62's guard removal.
- F9 with F32's and F22's pipe bullets (one resolver).
- F38 with F72's first bullet (one argument resolution).
- F13 with F70, F48 with F85, F42 with F43, F47 with F52 and F63, and
  F87 with F88.
- F7, F8 and F66 change the same use-site index, and land together.
- F69 before F82's implicit-call bullet (shared helpers).

Legend: `[x]` done (commit given), `[>]` moved to the crate-split plan
(where given), `[ ]` open.

## Validation

- [ ] **V1. The fixture suite and corpus** are the Stage B review's V1 and
  V2; a failure or delta traced to a commit below is judged here.

  Design: each fix commit runs its scoped checks (clippy, the touched
  module's unit tests, and the fixtures reaching the changed code, per
  F86). At the checkpoint after each group of the order below:

  - `cargo test --workspace` and `cargo test --test fixtures` run in full;
  - the corpus run is saved under `.corpus-runs/<scope>/<hash>.txt`;
  - it is diffed against the previous checkpoint's run, both sides sorted
    with `LC_ALL=C sort`.

  Every delta is traced to the finding whose fix caused it, and recorded
  in that finding's done note: a report gone for the right reason, or a
  new report that is right. A delta no finding explains is a regression
  of the checkpoint, fixed before the next group.

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
  silent. The probe of a name that is certainly undefined is itself
  dead code, so it is reported: `isset.variable`, `empty.variable` and
  `nullCoalesce.variable` ("Variable $m in isset() is never defined."),
  at level 1, from `check_pointless_null_probe`, which already owns the
  "always null" and "never null" verdicts for a bare variable. A `Maybe`
  name and any name under `any_variable_may_exist` (`extract`, `$$`,
  `include`, `eval`) are not reported. Fixtures: the five expressions
  above; `isset($m)` on a never-assigned `$m` reports
  `isset.variable`, and `isset($v)` after the conditional definition
  doesn't; `$m->foo()` inside `if
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
  (`calls/closure_context.rs`) is called for
  `Closure::bind(<literal>, …)` (`expr.rs` near 2620),
  `(<literal>)->bindTo(…)` (near 1140), `@param-closure-this` and Pest.
  Three gaps remain:

  - `(<literal>)->call($obj, …)` has no arm.
  - The two existing arms compare `method == "bind"` and
    `**method == *"bindTo"` by case, where `unused/rebind.rs` folds it.
  - A closure held in a variable and rebound later, the fixture's shape, is
    checked at its definition with no `$this`.

  The fix adds the `call` arm, with `$this` the first argument's type
  and the scope that object's class (PHP's `call` rebinds both), and
  folds both names' case. For the variable shape, the body is checked
  where the rebind happens, with the types that hold there, instead of
  guessing them at the definition:

  - `Locals` keeps a closure-literal alias: assigning a closure literal to a
    variable records the literal (an `Rc<Expr>`, like
    `query_builder_aliases`) and the captured `use` values at that point.
    The alias is dropped when the variable is reassigned and merged like the
    other aliases (kept only when both branches hold the same one).
  - A rebind site (`$v->call(E)`, `$v->bindTo(E, S)`,
    `Closure::bind($v, E, S)`, `$w = $v->bindTo(…)`) that finds the alias
    checks the literal's body there with
    `check_closure_literal_with_this_and_scope`: `$this` is `E`'s inferred
    type and the scope `S` (or `E`'s class for `call`), with the captures
    from the alias. So it takes the same path as the literal forms.
  - At the definition, a pre-scan of the enclosing body (the
    `ClosureVariables`/`Rebinds` walk of `unused/rebind.rs`, returning which
    variables are rebound later) tells whether the variable is rebound. If
    so, the definition-time check skips reporting on `$this` reads, since
    those are checked at the rebind sites, and checks the rest of the body
    as now. A variable bound at several sites is checked once per site, and
    a diagnostic repeated at the same span is kept once.

  A rebind to `null` (`bindTo(null)`) adds nothing, and a closure that
  is never rebound keeps reporting `$this` undefined, which is a runtime
  `Error`. The fixture then expects no `variable.undefined`. New
  fixtures: `$fn->call(new Foo())` reaching `Foo`'s private method with
  `$this` typed `Foo`, a literal `->call()`, and `BINDTO` in upper case.
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
    it yields to. The index keeps those in a second map,
    `function_fallbacks: FxHashMap<String, Vec<(PathBuf, Span,
    String)>>`, undone per file like `by_file`.
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

  Design: `syntax::visit::walk_attribute` hands the attribute's name to
  `visit_name` and walks its arguments. Neither `ClassFnRefWalker`
  (`project/use_sites.rs`) nor the navigation walker
  (`lsp/navigation.rs`) overrides `visit_attribute`, so the name reaches
  no recorder: the `Foo::class` inside `#[Route(Foo::class)]` is
  indexed, but `Route` is not. `ClassFnRefWalker` gains
  `visit_attribute`, which calls `self.class_at(&attr.name,
  attr.name.span)` and then `walk_attribute`, so the attribute is keyed
  like any class reference (lowercased per F7). The navigation walker
  gains the same override behind its `anchor.matches(attr.name.span)`
  check, calling `record_class`, so hover and go-to-definition work on
  an attribute name too. Attributes on parameters, promoted properties,
  closures, enum cases, constants and property hooks all reach
  `visit_attribute` through the shared walk, as the `for attr in …`
  loops in `visit.rs` show. The dead-code module doc's "attributes" gap
  then goes. Fixtures: `#[Marker] final class A {}` with
  `check_dead_code` on keeps `Marker` used, a use-site-index test finds
  the attribute site at its exact span (and a rename rewrites it), and a
  hover test resolves `#[Marker]`.
- [ ] **F9. A pipe into an impure callable is "does not do anything."**
  `$x |> 'var_dump';`, `$x |> 'printf';` and `$x |> fn($v) =>
  print($v);` as statements report `expr.resultUnused`. A pipe calls its
  right side. This was already the case before the fix window; the
  window's two pipe commits (`1cb1e25d`, `b3412c22`) didn't touch it.
  Done: a pipe statement is a no-op only when its callee is known pure.

  Design: a pipe is a call, so every walk that classifies calls has to
  see it as one. Today none does:

  - `noop.rs`'s `own_side_effects` has no `Binary(Pipe, ..)` arm, so the
    statement is effect-free unless its operands hold a call. `$x |>
    printf(...)` passes only because the first-class callable is an
    `ExprKind::Call` inside.
  - `registry/purity.rs` has the same gap, in `Disqualifier` and in
    `ImpurePoints`. It also stops at a closure literal ("doesn't run by
    being constructed"), which a pipe does run. Probed while designing
    this: `function show3(string $x): int { $x |> 'var_dump'; return 1; }`
    and `function show4(string $x): int { return $x |> fn($v) =>
    print($v); }` are both judged pure, so the bare statements `show3('a');`
    and `show4('a');` report `function.resultUnused`. That is a second
    false positive, at the callers.

  One resolver, `pipe_callee(rhs) -> PipeCallee`, sits beside
  `check_pipe_byref`, which resolves the same right side for the
  by-reference check. It answers:

  - `Function(name)` for a string literal or `f(...)`;
  - `Method(receiver, name)` or `Static(class, name)` for a method's
    first-class callable;
  - `Literal(body)` for a closure or arrow function;
  - `Unknown` otherwise, including a union of names (F32/F22's pipe
    bullet is the same resolver learning unions: each member is
    resolved).

  The walks then use it:

  - `own_side_effects` gives the pipe `Effects::Some`, and
    `check_pure_call_without_effect` gains a pipe arm that reports only
    when the callee resolves and is transitively pure (`'a' |>
    strtoupper(...)` is then `function.resultUnused`).
  - `Disqualifier` pushes a `PurityDependency` for a resolved function or
    a non-overridable method, walks a literal's body as if inline, and
    disqualifies on `Unknown`.
  - `ImpurePoints` pushes the matching point.
  - The call-forgetting path of F62/F82 counts a pipe as a call with that
    verdict.

  Fixtures: the three statements from the finding are clean, `show3` and
  `show4` are impure, and a pipe into a pure named function as a
  statement is reported.
- [ ] **F10. A bare `yield` followed by a binary-operator keyword is a
  parse error** (`ad2a5635` is incomplete). `(yield and $b)`,
  `or`/`xor`, `(yield & $b)`, `(yield |> $b)` and `(yield instanceof
  Foo)` all pass `php -l` but skip the file. The tokens that end a bare
  `yield` are a hand-written list. Done: a bare `yield` ends at any token
  that can't start an expression.

  Design: turn the list in `primary.rs`'s `Keyword::Yield` arm around.
  PHP's grammar gives a bare `yield` whenever the next token can't begin
  an expression, so the parser should ask that question rather than
  enumerate enders. The new predicate `Token::starts_expression(&self)
  -> bool` (`syntax/src/token.rs`) is true for:

  - `Variable`, `Integer`, `Float`, `StringLit`, `InterpString`,
    `ShellExec` and `Identifier` (which covers `exit`, `die`, `isset`,
    `empty`, `eval` and casts' contents);
  - `Backslash`, `LParen`, `LBracket`, `Bang`, `Tilde`, `Plus`, `Minus`,
    `Increment`, `Decrement`, `At`, `Dollar`, `DollarBrace` and
    `AttributeStart` (an attributed closure);
  - `Keyword(k)` where `k` is one of `Function`, `Fn`, `Static`, `New`,
    `Clone`, `Print`, `Throw`, `Yield`, `Include`, `IncludeOnce`,
    `Require`, `RequireOnce`, `Array`, `List`, `Match`, `True`, `False`,
    `Null` or `Namespace` (a `namespace\f()` name). This is an
    exhaustive `match` over `Keyword`, so a new keyword has to be
    classified.

  A `DocComment` token asks the token after it. The arm becomes `if
  !self.peek().starts_expression() { return Yield(None, None) }`. `Amp`,
  `Instanceof`, `And`/`Or`/`Xor`, `PipeGt`, `As` and the assignment
  operators then end a bare `yield` with no edit here, and the operator
  loop above takes them as for any operand. The comment about `+`/`-`
  stays true by construction. `yield -1` is still `yield` of `-1`,
  matching PHP (`php -l` and the 18:15 binary agree). A token-level unit
  test asserts `starts_expression` against a table, and a parser test
  parses each of `(yield and $b)`, `or`, `xor`, `(yield & $b)`, `(yield
  |> $b)`, `(yield instanceof Foo)` and `foreach ((yield) as $v)`. All
  of these pass `php -l`, as does `yield $k => $v = 3`, which already
  parses. A fixture checks that such a file is checked, not skipped.
- [ ] **F11. `NAN === NAN` is reported always true.** PHP gives `false`
  (and `NAN == NAN` is false, which is reported right). The NaN literal
  compares equal to itself, as `scalars_disjoint`'s "NAN is itself" says.
  This was already the case before the fix window; `89da4c25` and
  `d89ea9e5` kept it. Done: a NaN literal is never identical to anything.

  Design: the always-true verdict comes from
  `Relation::identical_result` (`types/src/relation/accepts.rs:810`).
  For two `is_finite_scalar_type` operands it answers `left == right`,
  and type equality (`lib.rs:388`) makes two NaN literals equal, as a
  type's identity must for interning. Value identity is a different
  question, so `identical_result` answers `Some(false)` first whenever
  either side is a NaN float literal: no value is identical to NaN, NaN
  included. The `is_finite_scalar_type` helper then becomes
  `Type::is_single_value` (`queries.rs:201`). That already excludes NaN
  and covers the empty array, plus every sealed shape of single values,
  where `===` is decided the same way (`[1] === [1]` is `true`). The
  separate helper and its doc go.

  Two more places read `=== NAN` as the value NaN:

  - The `===` true branch narrows `float $x` to `NAN` (probed), where it
    should be `never`. With F4's implicit `never`, the equality arm in
    `narrowing.rs` narrows to `never` when the other side is a NaN
    literal, and leaves the `!==` false branch untouched.
  - `match ($y) { NAN => 'nan', default => 'other' }` types the result
    `'nan'|'other'`, though a NaN arm never matches (PHP gives `2` for
    `match(NAN){NAN=>1, default=>2}`). The match arm-condition check, which
    goes through `identical_result`, then drops that arm and reports it
    always false.

  The type-level arms in `super_type.rs` ("NAN is itself") stay: they
  say the type `NAN` contains its value. Their comments should say that,
  so they aren't read as value identity. Fixtures: `NAN === NAN` reports
  `identical.alwaysFalse` and `NAN !== NAN` `notIdentical.alwaysTrue`,
  `$x === NAN` makes `$x` `*NEVER*`, and the `NAN` match arm is
  reported.
- [ ] **F12. An unresolvable property type is reported up to three times**
  (`22d4ff21`, F63's property half). `/** @var non-empty-foo */` on a
  property gives `class.notFound`, `property.unresolvableType`, and - new
  in the window - `assign.propertyType` "expects non-empty-foo, 2 given".
  `@var parent` gives the unresolvable report plus the assignment one.
  The Stage B ledger's F63 said the same goes for properties, and its
  done note omits them. Done: an unresolvable property type is ignored
  after its one report, as a parameter's is.

  Design: `22d4ff21` swapped in the native type at one consumer, the
  function body (`function_decl.rs`, `doc_unresolvable`). Every other
  consumer still reads the registry's merged PHPDoc type. A probe while
  designing shows how far that reaches:

  - A property typed `@var non-empty-foo` with no native type gets four
    reports: `class.notFound`, `property.unresolvableType`,
    `property.defaultValue` on its default `1`, and `assign.propertyType`
    at each write. Its reads are typed from the write, not declared.
  - A function with an unresolvable `@param` still reports
    `argument.type` ("expects non-empty-foo, 5 given") at every call.
  - With a native type, a parameter's name is also reported as
    `class.notFound` beside `parameter.unresolvableType`.

  So the decision moves to where the type is made. The node walk in
  `checker/phpdoc_unresolvable.rs` (`type_node_unresolvable` and its
  helpers) moves into the registry. It needs only the registry, the
  file's `NameContext` and the declaring class, which the resolve pass
  (`registry/resolve.rs`) has for every parameter, return, property,
  `@phpstan-assert`, `@phpstan-self-out` and inline `@var`. Each of
  `ParameterReflection`, the return, and `PropertyReflection` records
  `doc_unresolvable`. When it is set, the merged type is the native
  type, or `mixed` with none. That covers the body, the callers, other
  files, the property default, assignments and reads alike. The
  reporting sites in `phpdoc_type_compat.rs` read the flag instead of
  re-walking. The class-existence check skips a position whose PHPDoc is
  unresolvable, since its names are not class references. `22d4ff21`'s
  swap in `function_decl.rs` then goes, because the signature already
  carries the native type. Fixtures: the probe's file reports only the
  `*.unresolvableType` diagnostics (one each for `$p`, `$q` and `$x`);
  `@var parent` in a parentless class the same; `f(5)` is clean.

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

  Design: follow PHPStan's `nextAutoIndexes`, a set of possible next
  keys, and make it part of the type.

  - `Shape::next_index` becomes a normalized `NextIndex` value: a small
    sorted set of `NextArrayIndex` (capped at 8, past which it is
    `Unknown`), where the absent case means "derived from the int keys".
    A constructor normalizes a carried set equal to what the fields
    derive back to the derived case, so the same array built two ways is
    one type.
  - `PartialEq`/`Hash` for `Shape` compare it. The interner already does
    (`intern.rs:156`), and today the two disagree. The field doc's "not
    part of its type" goes.
  - `set_offset_value_type`'s literal-key arm calls `observe(n)` on every
    carried index for an int key `n`, which fixes `$c[7] = 3; $c[] = 4`
    giving 8. The several-literal-keys arm observes each int key into a
    copy of the set and unions the copies, since each key may be the one
    written. The open-key arm already clears it.
  - The append arm takes the set: one candidate appends as today. Several
    give one shape per candidate key, unioned (PHP puts the element at
    exactly one of them), and `Unknown` gives no precise append. It falls
    to the general arm, as an optional int key does now.
  - The union builder, where members compare equal but for the index, and
    `generalize_by_key_set` (`union.rs:388`) join index sets rather than
    keeping one side's. Their merged shape carries the union of their
    sets. `unset_offset` keeps the carried set, as it does now.
  - Display prints nothing new. Two members that differ only in index
    merge into one shape with a joined set before display, so no
    message shows the same shape twice.

  Fixtures: the `$c[8]` read and the `$b[] = 'y'` after `$b[10]` are
  clean; the finding's branch merge gives `array{0: 1, 1: 2, 2:
  9}|array{0: 1, 1: 2, 3: 9}` and `echo $b[2]` is no longer
  `offsetAccess.notFound` (one member has key 2); a types-crate test checks that
  two shapes built with and without a redundant carried index are equal.
- [ ] **F26. `mixed~S` joined with a value inside `S` stays `mixed~S`**
  (`67ed2a1d`). `union_with_mixed` takes each joined member out of the
  subtracted type with `without`, which can't take a literal or a subtype
  out of a general type (`string.without('x')` is `string`).
  `if (!is_string($z)) { $v = $z; } else { $v = 'x'; }` makes `$v`
  `mixed~string`, and `$v === 'x'` reports `identical.alwaysFalse`. The
  same happens with `mixed~int` and `5`, and `mixed~array` and `[1]`.
  Done: the subtracted part keeps only what no joined member overlaps,
  with fixtures for a literal and a refined join.

  Design: in `union_with_mixed` (`types/src/union.rs:885`), `without` is
  the wrong operation. `s.without(ty)` removes what `ty` wholly covers,
  while the join needs to remove from the subtracted part every member
  that `ty` may overlap at all, since a value of `ty` may be one of
  them. It can't represent "`string` but `'x'`", so the only sound
  result for an overlapping member is to stop subtracting it.

  The loop keeps each member `sm` of `subtracted` (the union's members,
  or the one type) only when `Relation::structural().disjoint(sm, ty)`
  holds for every non-mixed member `ty`, and rebuilds the rest with
  `union_of`. So:

  - `mixed~string | 'x'` is `mixed`;
  - `mixed~(string|int) | 'x'` is `mixed~int`;
  - `mixed~null | 'x'` stays `mixed~null`.

  The first loop's rule for two `mixed` members (keep the subtraction
  only when both carry the same one) becomes the intersection of their
  member sets: `mixed~(int|null) | mixed~null` is `mixed~null`, where
  today it is `mixed`. That is more precise and still sound, since a
  value excluded by both is excluded by the join. The `disjoint` used
  must be the structural relation the types crate already has, not one
  that needs the registry. Two classes' disjointness is then "maybe",
  which drops the member: sound. Fixtures: the three finding shapes
  (`'x'` with `mixed~string`, `5` with `mixed~int`, `[1]` with
  `mixed~array`) no longer report `identical.alwaysFalse`; a types-crate
  test covers the partial case (`mixed~(string|int)` with `'x'`) and the
  two-mixed intersection.
- [ ] **F27. `is_resource()` of a resource is "always true."**
  `while (is_resource($fp)) { fclose($fp); }` reports
  `function.alreadyNarrowedType`, but a closed resource is not
  `is_resource` (`gettype` gives "resource (closed)"). This was already
  the case before the fix window; since `c78732bc` the call is also typed
  `true`, so the loop reads as `while (true)`, and after an `fclose` an
  `if (is_resource($fp))` has a dead `else`. Done: `resource` answers
  `is_resource` maybe, or `fclose()` and the like leave a closed
  resource.

  Design: the type model already has a closed resource (`Repr::Resource
  { closed }`, PHPDoc `closed-resource`), but `parse.rs:1012` reads a
  written `resource` as `open-resource`, so a parameter or stub
  `resource` claims more than PHP does. PHPStan's `resource` covers both
  states, and so should this one:

  - `Repr::Resource` takes `state: ResourceState { Open, Closed, Either }`.
    `resource` parses to `Either`, and `open-resource`/`closed-resource`
    keep their states. Super-type, disjointness, intern and display
    (`resource` for `Either`) follow; `Either` is the supertype of the two
    others.
  - `is_resource`'s narrowing target (`predicates.rs:371`) is `Open`. On
    `Either` the call is "maybe", so `while (is_resource($fp))` and the
    call's type (`bool`) stop being constants. The false branch keeps
    `Closed`.
  - A small table of closing functions (`fclose`, `pclose`, `closedir`,
    `proc_close`, `gzclose` and `bzclose`, the ones that still take a
    resource on PHP 8.5) marks a narrowable argument `Closed` after the
    call. It sits beside the other by-reference effects of builtins, not
    in the stubs, which have no `@param-out`.
  - `gettype`'s return (`builtin_calls.rs:2708`) keeps answering `'resource
    (closed)'` for `Closed`, and gives both strings for `Either`.

  A resource a builtin returns is freshly opened, so a stub's `resource`
  in return position (`fopen`, `opendir`, `proc_open`, `popen`,
  `tmpfile`, …) means `Open`. `Either` is for parameters, properties and
  PHPDoc, where the value may have been closed already. So right after
  `$fp = fopen(…)` (and its `false` narrowed away), `is_resource($fp)` is
  `true`, and after `fclose($fp)` it is `false`, reported as
  `function.impossibleType`.
  Fixtures: `while (is_resource($fp)) { fclose($fp); }` is clean,
  `fclose($fp); if (is_resource($fp))` reports the impossible check, and
  the `operators.rs`/`strict_rules.rs` resource lists still build their
  unions.
- [ ] **F28. A guarded polyfill's body is checked against the stub's
  signature.** `if (!function_exists('array_key_first')) { function
  array_key_first(array $a) { … $a … } }` reports `undefined variable $a`,
  because the stub names the parameter `$array`, and
  `missingType.iterableValue` names `$array` too. This was already the
  case before the fix window; `f51447bd` relies on the stub winning.
  Done: a declaration's body is checked against its own parameters.

  Design: the mechanism already exists. `check_function` asks
  `own_declaration_sig` (`function_decl.rs:216`), which builds `f`'s own
  signature with `FunctionReflection::from_decl` and
  `finalize_standalone_sig` whenever the registry's slot for the name
  was built from another declaration. It reads the slot's site from
  `registry.function_decl_sites` with a `?`, so a name that has no entry
  counts as "the slot is `f`'s". A stub-provided builtin is exactly that
  case: `registry/index.rs` records a site only when a project
  declaration wins the slot (`Some(site)`) or merges (`None`), and when
  `better_conditional_function_declaration` keeps the stub, the
  polyfill's declaration leaves no entry. The fix is in the lookup:
  `match sites.get(key) { Some(Some(site)) if site == f's => None, _ =>
  build fresh }`, so an absent entry means "not this declaration". It is
  reinforced where the stub wins: that branch inserts the losing
  declaration's key as `None` if absent, so the map says "some other
  declaration holds this slot" rather than nothing. Callers keep the
  stub's signature, which is right, since on a PHP version with the
  builtin the guard skips the polyfill. Fixtures: the finding's polyfill
  reports no `variable.undefined` and names `$a` (not `$array`) in
  `missingType.iterableValue`; a polyfill with a different arity and a
  namespaced one behave the same; a project function with no stub is
  unaffected (a unit test that `own_declaration_sig` is `None` for it).
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

  Design: PHP 8.5's rule, checked with `php` for each shape:

  - Two used traits giving a method of the same name (compared
    case-insensitively: `Foo` and `foo` collide) is fatal unless the
    class declares the method or an `insteadof` excludes all but one.
  - An `as` alias doesn't help.
  - An abstract trait method never collides.
  - The same method reached from one trait through two paths (`T1` and
    `T2` both `use T0`) doesn't collide.
  - Properties collide between two traits, or between the class and a
    trait, unless visibility, `static`, `readonly`, type and default all
    match.

  `check_trait_conflicts` (`oop.rs:626`) is rebuilt on that rule:

  - The registry keeps a class's exclusions in place of
    `resolved_trait_conflicts` (a set of every mentioned name, which
    `ClassDef::resolved_trait_conflicts` in `ast.rs:595` builds from
    aliases and precedences alike). The new set holds `(trait, method)`
    pairs, lowercased, from each `TraitPrecedence`'s `insteadof` list.
  - For each lowercased method name, collect the directly used traits'
    non-abstract methods, dropping the excluded pairs. Deduplicate them by
    declaring trait, using `lookup_method_with_owner` on the used trait,
    so a shared `T0` counts once. Skip the name when the class's own
    `methods` holds it, folded. Two or more left report
    `trait.methodConflict` with PHP's wording ("Trait method B::foo has
    not been applied as C::foo, because of collision with A::foo"),
    whether or not their signatures agree. The old signature comparison
    then has nothing left to decide.
  - Properties compare the class's declared ones and each trait's in
    one map. A pair is compatible when visibility, static-ness,
    readonly-ness, the native type and the default's constant value are
    equal. Otherwise `trait.propertyConflict` with PHP's "A and B define
    the same property ($x) in the composition of C" wording, the class
    named first when it is one side. The `info.uses().len() < 2` early
    return goes, since one trait can conflict with the class.

  Fixtures: one per `php` line above (the three fatal method shapes
  reported, the five legal ones clean, and the three property shapes).

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

  Design: the sets are one symptom. A probe while designing shows that
  resolution itself is case-sensitive between a class and its parent.
  For `class P { function foo(): int }` and `final class C extends P {
  function FOO(): string }`, `(new C())->foo()` is typed `int`, P's
  method, where PHP runs C's (and rejects the pair as incompatible,
  which prs does report). `method_owner_by_class` is keyed by declared
  spelling, so `foo` and `FOO` are two entries. `lookup_method`
  (`hierarchy.rs:377`) finds the exact one first and falls back to a
  case-insensitive scan only when nothing matches.

  So a class's methods are keyed the way `registry::name_table` already
  keys classes and functions, ASCII-lowercased, with the declared
  spelling kept beside the value:

  - `ClassReflection::methods` becomes a `NameTable<FunctionReflection>`,
    whose `get`/`contains_key` fold and whose `iter` yields declared
    names.
  - `method_owner_by_class`'s inner maps fold, so a subclass's `FOO`
    replaces the parent's `foo` as PHP's override does, and
    `lookup_method`'s fallback scan goes.
  - `abstract_methods`, `final_methods`, `static_methods` and
    `final_methods_by_phpdoc` become a `FoldedNameSet` (a set of
    `name_table::Folded`), so `contains` folds.
  - Property and constant tables stay case-sensitive, as PHP's are.

  The 56 `methods().get`/`contains_key` call sites then fold with no
  edit. `method.nameCase` compares the call's text with the declared
  name the table returns (`lookup_method_canonical_name`), as it does
  now. F55's `__toString` lookup and `ImpurePoints`' `__construct` (F22)
  fold by the same change. Fixtures: the probe's `foo()` is typed
  `string`; `RUN()` implementing `abstract run()` reports only
  `method.nameCase`; `bar()` overriding `final Bar()` reports
  `method.parentMethodFinal`, and so does `final __Construct`.

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

  Design: two halves.

  At the call, `apply_assert_infos` (`narrowing/assertions.rs:385`)
  narrows with `info.ty` as declared. The bindings it needs are the ones
  `bound_call_return_type` (`expr/array_ops.rs:468`) already builds for
  the return type. That function's first half moves into
  `call_template_bindings(sig, resolved, locals) -> (TemplateBindings,
  param_bindings)`, which the return path and the assertion path share.
  `apply_assert_infos` resolves the arguments with `resolve_call_args`
  (so named arguments map by name, which is F72's first bullet by the
  same edit), computes the bindings, and substitutes `info.ty` with
  `substitute_templates_resolving_to_bounds`, using the same own-scope
  predicate the return uses, then `resolve_conditional`. Only after that
  does it `narrow` or `exclude`. For a method call, the six callers
  (`narrowing.rs:259/1225/1242`, `stmt.rs:1044/1055/1067`) pass the
  declared `sig`. The ones with a receiver first run
  `substitute_class_templates` with the receiver's type arguments, as a
  method call's return does, so `Collection<int>::assertHas(T $x)`
  narrows to `int`.

  At the declaration, `check_one_assert_tag` (`narrowing.rs:1531`)
  decides "does not narrow down" with `self.accepts(&info.ty,
  param.ty())`. Acceptance takes anything into a template
  (`accepted_though_no_subtype`'s `(_, TemplateParam)` arm), so
  `@phpstan-assert T $x` over `mixed $x` always looks already narrowed.
  The check is a subtyping question, so it uses
  `relation().is_super_type_of`. There a template placeholder as the
  supertype of anything but itself is `Maybe`, as in PHPStan, and
  neither verdict fires. The disjointness half compares the
  placeholder's bound.

  Fixtures: `assertInstance(Sub::class, $m)` makes `$m` `Sub` and `?Foo
  $f` `Sub`, and `assertSame(5, $n)` makes `$n` `5` so `needsInt($n)` is
  clean. `$m->nope` on the asserted `Sub` reports. Neither declaration
  reports `assert.alreadyNarrowedType`. A class-template assertion is
  narrowed through its receiver.

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

  Design: restructure `Type::binary_operation`
  (`types/src/arithmetic.rs:640`) around the operands' members rather
  than patch each arm.

  - Distribution. For `+`, `-`, `*`, `/`, `%` and `**`, the operands are
    first taken to numbers the way PHP does. That is the checker's
    `to_number`, moved into the types crate beside `numeric.rs`: a
    numeric-string literal becomes its value, `numeric-string` becomes
    `int|float`, `true`/`false`/`null` become `1`/`0`/`0`. Each side is then
    split into union members, the operation runs per pair, and the results
    are unioned. Past 16 pairs it falls back to a per-kind summary: `int`,
    `float`, or `int|float`, by whether each side may be an int or a float.
    A pair with a float member gives `float` (folded when both are
    constants), and a pair of ints runs the range math. So `int|float + 1`
    is `int|float`, `'1.5' + $i` is `float`, `$numericString + 1` is
    `int|float`, and the `either_float` shortcut and every
    `_ => Type::int()` fallback go.
  - Unary operators. `-x` is `x * -1` and `+x` is `x * 1`, which is how PHP
    compiles them. The two checker arms (`expr.rs:1766`, `4589`) then call
    `binary_operation(Mul, …)` instead of their string-to-`int` shortcut and
    `Type::negate`. Probed while designing, besides the finding's `-'1.5'`:
    `-(1|2)` is `1|2`, `-true` is `true` and `-null` is `null` today, where
    PHP gives `-1|-2`, `-1` and `0`. `Type::negate` stays only as the range
    helper `mul` uses.
  - `**`. `pow_int_range` becomes a candidate evaluation.
    - The base candidates are its bounds, plus `-1`, `0` and `1` when
      inside.
    - The exponent candidates are its bounds and their parity neighbours,
      both kept inside the exponent range.
    - The result's ends are the min and max of `checked_i64_pow` over the
      pairs.
    - A candidate that overflows adds `float`, as does any negative exponent
      (`1 ** -1` is `float(1)`, checked with `php`), and an unbounded
      exponent with `|base| >= 2` adds `float`.
    - A base range that spans zero gets `0` and the sign of each parity
      right: `int<-3, 2> ** 2` is `int<0, 9>`, `int<-5, max> ** 2` is
      `int<0, max>`, and `int<min, -3> ** int<1, 3>` is `int`: odd exponents
      run to `min` and even ones to `max`.
  - Overflow at unbounded ends. PHP turns an int result past `PHP_INT_MAX`
    or below `PHP_INT_MIN` into a float, whether or not the operand's range
    was written down. So the result adds `float` whenever the operands'
    bounds allow it, an unbounded end counting as able to reach the limit:
    `int<min, -3> + int<min, -3>` is `int<min, -6>|float`, and `int + 1` is
    `int|float`. `checked_bound_op` and `mul_int_range` answer "may
    overflow" from the corners with unbounded ends as infinities, instead of
    only when every bound is present. `++`/`--` (`increment_type`) follow,
    since `PHP_INT_MAX + 1` via `++` is a float too. The type is then true,
    and three changes keep it useful rather than noisy:
    - The overflow float is marked. `Repr::Float` gains an `overflow_only`
      flag (a `float` that exists only because an int overflowed), kept
      through unions and joins, lost as soon as any other float joins it.
      Display writes the member as `float` with a `(int overflow)` note in
      messages, and `assertType` prints `int|float`.
    - Rules tell it apart. When the only member of an argument, return,
      property or operand type that fails a check is an overflow-only float,
      the report goes under its own identifier (`argument.intOverflow`,
      `return.intOverflow`, `assign.intOverflow`, `binaryOp.intOverflow`),
      at level 9, with the message naming the overflow. A project can ignore
      that one identifier without losing real float mismatches, and the
      default reports it, since the value really can be a float.
    - Real bounds come in where the engine has them. `count()` and the
      array-size functions return `int<0, HT_MAX_SIZE>` (the hash table's
      size limit, read from `zend_types.h` when implementing), so
      `count($a) + 1` never overflows. A loop counter narrowed by its
      condition (`$i < count($a)`) is bounded the same way. `(int)`,
      `intdiv`, `is_int` narrowing and a comparison with an int bound all
      drop the overflow float, as they drop any float.

  The finding's last bullet is then fixed as written, not closed by a
  convention.

  F60's two range bugs are fixed in the same file (see there). Fixtures:
  each bullet of the finding, plus the three unary probes, `int + 1`
  typed `int|float`, `count($a) + 1` typed `int<1, HT_MAX_SIZE + 1>`,
  and `f($i + 1)` into an `int` parameter reporting
  `argument.intOverflow` and nothing else. A types-crate unit test runs
  F60's sweep over every operator, now including union and float
  operands, against a table generated once with `php`.
- [ ] **F40. `null == '0'` is taken as always true** (`loosely_equals`,
  read whole; also before the window). `null` against anything is
  compared as two booleans, but PHP compares `null` with a string as
  `''` with that string: `null == '0'` is `false`, and the check reports
  `equal.alwaysTrue` and treats the `else` branch as dead. Done: `null`
  against a string literal compares `''` with it; the other `null` and
  `bool` pairs stay as they are.

  Design: `Type::loosely_equals` (`types/src/arithmetic.rs:348`) opens
  with a bool-cast shortcut for any `true`/`false`/`null` side. PHP 8's
  table (checked with `php`: `null == "0"` false, `null == ""` true,
  `null == 0` and `null == []` true) casts to bool only when one side is
  a bool, or when `null` meets a non-string. `null` against a string
  compares `''` with the string, as strings. So the shortcut splits:

  - a `true`/`false` side keeps the bool cast;
  - `null` against a `StringLiteral` compares the literal with `''`: true
    only for `''`, and `'0'` is false, since `''` is not numeric;
  - `null` against anything else keeps the bool cast.

  While there, the float/non-numeric-string pair answers `None` today,
  as if unknown, though PHP 8 compares the float's string form with the
  string. `1.5 == '1.5abc'` is false and `INF == 'INF'` is true (both
  checked). That pair answers with `php_float_format`'s rendering when
  the version is 8 or later, as the int pair already does with
  `to_string`. The doc's "bool/null-vs-anything" sentence is corrected
  with it. Fixtures: `null == '0'` is `equal.alwaysFalse`, its `else`
  branch is live, `null == ''` is `equal.alwaysTrue`, and
  `1.5 == '1.5abc'` is `equal.alwaysFalse` on PHP 8.
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

  Design: a PHPDoc type is parsed twice today. `phpdoc::grammar` lexes
  and parses it into a `TypeNode`, reporting `phpDoc.parseError`. Then
  `registry/type_node.rs:8` renders the node back to text
  (`to_source_string`) and `types::parse_written_type` parses that text
  again with a second, string-splitting parser. Each bullet of the
  finding is a place where one of the two gets the language wrong:

  - the float fallback is in the string parser (`parse.rs:686`);
  - the `1_000`, `0x1F` and `1e25` (F19) errors are in the grammar's
    `lex_number` (`lexer.rs:140`), which reads only digits and one `.`;
  - the conditional's `:` split is `try_parse_conditional_type`
    (`parse.rs:311`), while the grammar already builds
    `TypeNode::Conditional` with its branches parsed.

  The fix is one parser:

  - `lex_number` follows phpdoc-parser's lexemes: decimal, `0x`, `0o` or
    leading-`0` octal, and `0b` integers with `_` separators, and floats
    with a fraction or exponent (`1e25`, `.5`, `1_000.5`). An integer past
    `i64` becomes a float, as PHP reads such a literal in code.
  - A new lowering `TypeNode -> Type` (in `types`, beside `parse.rs`) walks
    the node's variants: identifier, nullable, union, intersection, generic,
    wildcard, variance, `ArrayOf`, offset access, shapes, callables with
    their templates, int ranges, literals, const fetch and conditional. It
    reuses the string parser's keyword table (`int`, `non-empty-string`,
    `class-string<…>`, …), extracted into one `keyword_type(name, args)`
    function that both call.
  - `parse_type_node` becomes that lowering. A float literal can then come
    only from a `FloatLiteral` token, so `Infinity`, `NaN` and `inf` are
    class names, and each conditional branch is the node the grammar built,
    including `\Closure(): int`, `self::A` and `?T`.
  - `parse_written_type` stays for native hints and the few internal strings
    (`predefined_constants.rs`, stub synthesis). Its float fallback is gated
    on a first character that is a digit, a sign or a `.`, since native
    hints never hold a float.

  Fixtures: `@param Infinity $i` with a class `Infinity` is clean,
  `$n->v` on `Nan` resolves, `1_000`, `0x1F` and `1e25` read as `1000`,
  `31` and `1.0E+25`, and the conditional with a callable true branch
  resolves with no `class.notFound`. A grammar unit test covers each
  lexeme.

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

  Design: split the `random_int | rand | mt_rand` arm
  (`expr/builtin_calls.rs:132`) by what PHP does with crossed bounds
  (checked with `php`: `rand(10, 1)` returns a value, and
  `mt_rand(10, 1)` and `random_int(10, 1)` throw `ValueError`).

  - `rand` returns a value between `min($a, $b)` and `max($a, $b)`, so its
    range is `int<min(a_min, b_min), max(a_max, b_max)>`, with an unknown
    bound unbounded. When `$a` is always at most `$b` this is exactly
    today's `int<a_min, b_max>`, so nothing that works changes.
  - `mt_rand` and `random_int` keep `int<a_min, b_max>` when the bounds may
    be in order. When they are always crossed (`a_min > b_max`) the call
    always throws, so it is the explicit `never` of F4's split, since the
    statement after it really is unreachable. The call is also reported,
    since it can only throw: `argument.invalidRange` at level 0 ("Call to
    random_int() with minimum 10 greater than maximum 1 always throws
    ValueError."). When the bounds may cross but needn't, the call is
    reported at level 7 as possibly throwing, the level where the other
    "maybe" reports (a union member that fails) start. A `rand()` with
    crossed bounds is not reported, since it runs.

  The zero-argument forms (`rand()`, `mt_rand()`) are `int` today; they
  return `int<0, getrandmax()>` (`2147483647` on every platform PHP 8
  supports), which the same arm gives. Fixtures: `$r = rand(10, 1);` is
  `int<1, 10>` and the next statement is reachable; `rand($a, $b)` with
  `$a` in `int<5, 10>` and `$b` in `int<0, 3>` is `int<0, 10>`;
  `random_int(10, 1)` makes what follows unreachable.
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

  Design: `AssignRef` shares `Assign`'s arm (`expr.rs:2013`) and binds
  the left side's type, so the checker forgets, after the statement,
  that the two names are one variable. `Locals` gains reference groups:

  - a map from a narrowing key to a group id, and each group's members,
    bumped with the version;
  - `merge_scopes` keeps a pair only if both sides have it, and loop
    convergence goes through the same rule.

  Groups are formed:

  - `$a = &$b` puts `$a` into `$b`'s group, leaving its old one;
  - `foreach ($xs as &$v)` groups `$v` with the key `$xs[<current key>]`,
    dropped at the loop's end;
  - `global $x` and `static $x` join `$x` to a group of their own that
    nothing else writes locally;
  - `$r = &$arr['k']` and `$r = &$o->p` group `$r` with that key.

  A group is used:

  - A write to any member (`declare`/`insert` through
    `bind_assignment_target`, a compound assignment, `++`/`--`, a
    by-reference argument) writes the same type to every other member. For
    an offset or property member it is written as the write at that path, so
    `$arr`'s shape takes the new `k` value.
    `purge_branch_implied_facts_depending_on` runs for each member.
  - A narrowing of one member (`===`, `instanceof`, `is_*`) narrows every
    member, since they are one value.
  - `unset($a)` leaves the group. A plain `$a = …` doesn't: PHP writes
    through the reference.

  A closure's `use (&$x)` makes `$x` escape. Every call after the
  closure is created, and every call it is passed to, forgets `$x`'s
  narrowing the way F82's `Anything` verdict forgets properties, because
  any call may run the closure. The dead-branch fact replay and
  `check_pointless_null_probe` then read the propagated types and need
  no change. Fixtures: `$t = &$s; $t = null;` makes `$s` `null`;
  `$r = &$x; $r++;` gives `$x === 1` no report; the `??` probe after
  `$r = 'a'` is clean; `$r = &$arr['k']; $r = 5;` types `$arr['k']` `5`;
  a `foreach (… as &$v)` write shows in the array after the loop.
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

  Design: the alias records "the flag's truthiness is the condition's",
  which holds only when the body's literal is truthy and the flag's
  value before the `if` is falsy. After
  `$ok = true; if (cond) { $ok = false; }` the flag is truthy exactly
  when `cond` failed. So the alias carries a polarity:

  - `ConditionAliasMap`'s entry (`locals.rs`) gains `negated: bool`.
  - `record_if_body_flag_alias` (`narrowing.rs:1443`) reads the literal's
    truthiness and the prior type's (`else_locals.get(name)`, through
    `to_boolean`). It records the alias only when both are definite and
    differ, with `negated` set when the literal is the falsy one. A prior
    `bool` or a same-truthiness pair records nothing.
  - The plain `$ok = cond;` alias keeps `negated: false`.
  - The replay at `narrowing.rs:1336` calls
    `apply_narrowing(&alias, locals, branch_true != negated)`.
  - The nullsafe replay in `assertions.rs:235` bails on a negated entry,
    since it is written for the positive shape.
  - `merge_scopes` compares the flag too when it keeps an entry both sides
    share.

  Fixtures: the finding's `true`-then-`false` shape keeps `$x` a
  `string` inside `if ($ok)` and `null` inside `if (!$ok)`. The existing
  `false`-then-`true` shape is unchanged. A prior `bool $ok` records no
  alias, so `if ($ok)` narrows nothing.
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

  Design: replace `stmt_contains_break` (`control_flow.rs:825`) with
  `breaks_out_of(body)`, which answers whether some `break` leaves this
  loop, tracking depth as PHP counts it:

  - the walk starts at depth 0 in the loop's body;
  - a nested `while`, `do`, `for`, `foreach` or `switch` adds one level,
    since PHP counts a `switch` as a loop for `break`;
  - a function, closure or class body stops the walk;
  - a `break n` (default 1) at depth `d` leaves this loop when `n > d`;
  - a `goto` counts when its label is outside the loop body. Labels are
    in the AST, so the walk collects the body's own labels first, and a
    `goto` to one of them stays inside the loop.

  A `break 2` in the body leaves this loop and the one around it, so it
  counts. From the outer loop's walk it is a `break 2` at depth 1, which
  leaves that loop too, so each loop sees it once. `always_terminates`
  then:

  - keeps its `While` and `For` arms, using `breaks_out_of`;
  - gains a `DoWhile` arm. A truthy literal condition terminates when
    nothing breaks out. Any `do`-`while` terminates when its body does,
    since the body runs at least once, so `do { return 1; } while ($x)`
    counts too.

  The doc's "only ever causes a missed simplification" claim goes.
  The constant-condition rules in `stmt/loops.rs` (`while.alwaysTrue`
  at line 74, and `doWhile.alwaysFalse`/`alwaysTrue` at 293 and 302)
  call `breaks_out_of` too, so a nested loop's `break` no longer suppresses
  them. Fixtures: both `return.missing` shapes of the finding are clean;
  `while (true) { switch ($x) { case 1: break 2; } }` still falls
  through; a `break` inside a nested `foreach` doesn't suppress
  `while.alwaysTrue`.
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

  Design (F85 too): the per-key entries are a second copy of what the
  container's shape now says, and the copy is the one that goes wrong.
  In `bind_assignment_target` (`assignment.rs`, the dynamic-key block
  near 486), `purge_aliased_index_entries` has already removed every
  `row[…]` entry (the prefix for a non-literal index is `row[`). The
  loop over `literal_keys_from_type` therefore always takes its
  `None => ty.clone()` arm, and `$row['id']` reads back `null`. Since
  `783727da`, `set_offset_value_type`'s several-literal-keys arm
  (`types/src/offsets.rs`) joins each existing field with the written
  value and adds missing keys as optional. So the loop is removed rather
  than repaired: after the purge, a read of `$row['id']` falls through
  to the container's field, `1|null`, and container and element agree by
  construction. The "`foreach` over several literal keys, once per key"
  case the comment cites is covered by the shape, whose fields union
  across the passes. A general-array container (no shape) reads its
  value type, as it does for any other write through an unknown key.

  F85's second half is in the same arm: a key the shape lacks becomes
  `ShapeField::new(key, value, true)`. On an unsealed shape whose tail
  key type may hold that key, the field's type is the written value
  unioned with the tail's value type, since the key may already hold one
  of the tail's values. Fixtures: `$row[$key] = null` with
  `'id'|'name' $key` gives `$row['id'] === null` no report and
  `$row['id']` typed `1|null`; the open shape
  `array{a: 1, ...<string, int>}` written at `'a'|'b'` gains
  `b?: 'z'|int`; the corpus shape (a `foreach` over a literal key list
  writing `$row[$key]`) keeps each field's union.
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

  Design: one fix per arm in `expr/builtin_calls.rs`, each checked
  against `php` output.

  - `array_combine` (line 468). The keys come from the keys array's values,
    converted with `to_array_key`, and the values from the values array.
    When both are sealed shapes of the same length, the result zips them
    field by field. A repeated key is last-one-wins
    (`array_combine(['a', 'a'], [1, 2])` is `['a' => 2]`), which
    `Shape::set` already does. Shapes of different lengths throw
    `ValueError`, so the result is the explicit `never`. Otherwise it is
    `array<keys' value type as array key, values' value type>`, non-empty
    when the keys array is. List-ness never survives.
  - `array_change_key_case` (line 818). A literal mode (`CASE_LOWER`, the
    default, or `CASE_UPPER`) maps each string key of a shape through ASCII
    case folding, the fold PHP uses, merging fields that collide in key
    order, last one winning (`['Ab' => 1, 'aB' => 2]` is `['ab' => 2]`). Int
    keys stay. A general array keeps its value type with its string key type
    folded to `lowercase-string` for `CASE_LOWER` (`uppercase-string` for
    `CASE_UPPER`; both are `AccessoryKind`s already). An unknown mode joins
    both foldings.
  - `array_fill_keys` (line 1207). The key type is the keys array's value
    type through `to_array_key` (`'1'` is `1`, and `'x'` stays). A sealed
    keys shape gives a sealed result shape with one required field per
    distinct key, non-empty when the keys array is.
  - `get_parent_class` (line 1632). A final class, or an enum, answers its
    declared parent's name, or `false` without one. A non-final class with a
    parent answers `class-string`, since a subclass's parent may be any
    class under it; one without a parent answers `class-string|false`
    (`false` only when the value is exactly that class). The string-argument
    form reads the named class the same way.
  - `array_slice` (line 482). Non-emptiness survives only for a literal
    offset of `0`, or a negative literal whose magnitude is at most the
    shape's known minimum size, with no length or a positive literal length
    (`array_slice([1, 2], 5)` is `[]`). List-ness survives from a list
    input, since int keys are renumbered.
  - `explode` (line 1524). The result is `non-empty-list<string>` when there
    is no limit or a limit that is at least `0` (`0` counts as `1`), and
    `list<string>` for a negative or unknown limit. An empty literal
    separator throws `ValueError`, so the result is `never`.
  - `parse_sscanf_format` (line 3059). Rewritten as one lexer over PHP's
    format grammar:
    - an optional XPG `n$` position;
    - `*`, which suppresses the assignment, so the conversion counts for no
      slot;
    - a width, and the size modifiers `h`, `l` and `L`, which PHP ignores;
    - a conversion from `d i o u x X e E f F g G s c [ n`;
    - `%%`.

  `%X` is an int like `%x`, `%F`/`%g`/`%G` are floats, and `%n` is an
  always-set `int`, the characters read so far (`%[a-z]%n` on `"ab"`
  gives `[ 'ab', 2 ]`). `sscanf_field_type` and
  `check_scanf_argument_count` both read this lexer, so the count and
  the shape agree.

  Fixtures: one per arm, taking the finding's reports and the `php`
  results above as expectations.

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

  Design: in `closure_syntactic_return_type` (`expr.rs`, near 5820),
  `falls_off_end` becomes
  `!body.iter().any(|s| self.always_terminates(s, &scope))`. That is the
  question `always_terminates`' `Block` arm already answers for a
  statement list, asked against the scope the function has just built,
  whose parameters and captures are bound, so a call to a
  `never`-returning function in the body counts. With F47's `DoWhile`
  arm and the targeted `break`, the loop shapes come right in the same
  step. `check_expr`'s arm reads real return points from the converged
  body and needs no change. The two paths then differ only in precision,
  as the function's doc says, not in whether `null` joins. Fixtures: the
  finding's `if`/`else` closure passed to a `Closure(int): int`
  parameter is clean; a closure ending in a `switch` whose every case
  returns, one ending in `throw`, and one ending in a call to a `never`
  function all infer without `null`; a closure whose `if` has no `else`
  still adds `null`.
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

  Design: decide "the last arm must be true" the way PHPStan's type
  specifier does. The last arm is exhaustive when assuming it false
  leaves no possible value. `condition_is_provably_true`
  (`expr.rs:6325`) gains a narrowing test after its two existing ones:
  clone `accumulated_before_last_arm`, run
  `apply_narrowing(cond, &mut clone, false)` for each of the last arm's
  conditions (a multi-condition arm fails only when all of them do, so
  each is applied false in turn), and answer true when some key the
  narrowing touched is now `never` and wasn't before. That covers
  `instanceof`, the `is_*` predicates, `===` against a literal and
  `&&`/`||` compositions, since those are exactly the conditions
  `apply_narrowing` already understands, with no new per-shape logic. It
  relies on F4: today an impossible narrowing falls back to the checked
  type instead of `never`, so this test needs F4's `never` to see the
  empty branch. F4 lands first. The single-condition restriction at the
  call site (`[only_cond]`, `expr.rs:3415`) goes. The
  `UnhandledMatchError` throw state recorded when the arm isn't
  exhaustive follows the same answer. Fixtures: the finding's three
  matches (`instanceof A`/`B` on `A|B`, `is_int`/`is_string` on
  `int|string`, `=== true`/`=== false` on `bool`) report nothing and
  record no throw point; a match missing a class still reports; a
  two-condition last arm (`$x instanceof B, $x instanceof C =>`) on
  `A|B|C` is exhaustive.

- [ ] **F55. A class declaring `__tostring()` in lower case isn't
  `Stringable`** (`ClassReflection::from_def`; also before the window).
  The implicit interface is added on `methods.contains_key("__toString")`,
  an exact-case lookup, and PHP method names ignore case: for `final class
  A { public function __tostring(): string {...} }`, passing `new A()` to
  a `Stringable` parameter reports `argument.type` and
  `new A() instanceof Stringable` reports `instanceof.alwaysFalse`; PHP
  says `true`. Done: the lookup folds case, with a fixture.

  Design: the lookup is `methods.contains_key("__toString")` at
  `class_info.rs:1502`, on the map F35 turns into a folded `NameTable`,
  so F35's change fixes it with no edit here. Until then, or if F35
  lands in parts, the one-line fix is
  `methods.keys().any(|m| m.eq_ignore_ascii_case("__toString"))`. The
  other `"__toString"` sites go through
  `lookup_method`/`lookup_method_with_owner` (`checker.rs:1395`,
  `comparisons.rs:215`), which already fold, or compare with
  `eq_ignore_ascii_case` (`deprecated_usage.rs:91`). The implicit
  interface is added only for a class, not an interface, trait or enum,
  as now. Fixtures:
  `final class A { public function __tostring(): string {…} }` passed to
  a `Stringable` parameter is clean, `new A() instanceof Stringable`
  reports nothing, and `(string) new A()` is accepted.
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

  Design: two edits in `registry/purity.rs`, each mirrored in the
  impure-points walk, so the declared-purity report and the inferred
  purity agree.

  - `resolve_static_call` (line 534) splits `static` from `self`.
    `self::m()` and `parent::m()` are lexically bound and stay as they are.
    `static::m()` dispatches on the runtime class, so it adds the dependency
    only when `is_non_overridable(registry, class, &owner, method, sig)`
    holds, the rule `resolve_this_method_call` already applies to
    `$this->m()`, and disqualifies otherwise.
    `ImpurePoints::static_call_resolves` (line 696) takes the same split.
    While there, a static call through a class name (`Foo::bar()`, today
    `None`, so it disqualifies) resolves lexically through `lookup_class`
    and `lookup_method_with_owner`, since it too names one declaration.
  - `Disqualifier::visit_expr` (line 371) stops disqualifying
    `ClassConstFetch` and `ClassNameFetch`. A class constant, an enum case
    and `::class` are immutable reads whichever class `static::` names,
    which is also what `ImpurePoints` already says (line 745).
    `StaticPropertyFetch` and its dynamic form stay disqualifying. The
    comment above the arm is rewritten to say just that (F59's bullet).

  Fixtures: the finding's
  `final public function run(): int { return static::compute(); }` on a
  non-final class is impure, so `$b->run();` is clean. The same method
  on a `final` class, or with `compute()` private or final, stays pure
  and is reported. A getter returning `self::X` is pure, and a bare call
  to it reports `method.resultUnused`.
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

  Design: two parser edits, checked against `php -l`.

  - The arrow body. `parse_arrow_fn` (`closures_and_match.rs:82`) calls
    `parse_expression()` (the `or` tier, `expr.rs:8`) instead of
    `parse_assignment()`. PHP gives the arrow function the lowest
    precedence, so the body takes `and`/`or`/`xor` too. A comma still ends
    it, since commas are not operators (`[fn() => 1, 2]` is two items), and
    so do `)`, `]` and `;`. `$f = fn() => false or $b;` is then one
    assignment of a closure returning `false or $b`, and the
    `logicalOr.resultUnused` goes.
  - Nested ternaries. PHP's ternary is left-associative and rejects every
    unparenthesized nesting in the else position except a pure `?:` chain:
    - `a ? b : c ? d : e` fails, and so do `a ?: b ? c : d` and
      `a ? b : c ?: d`;
    - `a ?: b ?: c` and `a ? (b ? c : d) : e` parse.

  `parse_ternary` (`expr.rs:259`) parses the else branch as now, then
  refuses it when it is an `ExprKind::Ternary` (not wrapped in
  `Grouping`) unless both the outer and the inner are short ternaries.
  The `ParseError` quotes PHP's message ("Unparenthesized
  `a ? b : c ? d : e` is not supported"). The right-nested tree the
  parser builds for a pure `?:` chain evaluates the same as PHP's
  left-nested one, so it can stay.

  Such a file then fails to parse, which F77 makes a reported error
  rather than a silent skip. Parser tests cover the eight `php -l`
  cases, and a fixture checks `(fn() => false or true)()` as `true` with
  no `if.alwaysFalse`.

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

  Design: replace the transcriptions with identities that can't flip a
  sign.

  - Subtraction. `l - r` is `l + (-r)` on bounds. The negation maps
    `int<lo, hi>` to `int<-hi, -lo>`, an unbounded end staying unbounded on
    the other side, and `-PHP_INT_MIN` overflows, which adds `float` as
    `negate` already does. The addition then runs `checked_bound_op` per
    end. `sub_int_range_full` (`arithmetic.rs`) and the `Sub` arm's
    constant-right-operand shortcut both go, since the identity covers them.
    `c - int<lo, hi>` comes out `int<c - hi, c - lo>`: `-1 - int<min, -1>`
    is `int<0, max>`, and `3 - int<-1, max>` is `int<min, 4>`.
  - Division by a negative constant. `div_int_range(l, r)` with `r` entirely
    negative is `-(div_int_range(l, -r))`, the int half negated with the
    same helper and the `float` half kept. The tail that swaps the bounds
    when one is unbounded ("invert maximas on division with negative
    constants") goes, since it is what flips the unbounded case:
    `int<0, max> / -1` is `int<min, 0>|float`, and `int<min, -3> / -3` is
    `int<1, max>|float`. A divisor range spanning zero keeps the existing
    split into a negative and a positive half, and the negative half now
    goes through the identity.

  The sweep that found this (every operator over ranges with ends in
  `{min, -3, -1, 0, 1, 3, max}`, and exhaustively over `[-4, 4]`)
  becomes a types-crate unit test. The expected table is generated once
  with `php` into a checked-in data file, as F39's widened sweep does,
  so the test runs without PHP. The finding's `smaller.alwaysTrue` probe
  is a fixture.
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

  Design: `resolve_always_exists` (`expr/probe_checks.rs:180`) answers
  "always present" for a chain. Each link must then prove the value that
  the next link reads from can't be null and can't be missing:

  - After resolving `base_ty` in both the `Index` and the `PropertyAccess`
    arm, return `None` unless `base_ty.is_null().no()`. A `?Foo` receiver,
    or a nullable offset value under a deeper `['a']['b']`, makes the chain
    not always present. A nullsafe link is the same case.
  - The `PropertyAccess` arm requires every union member to be an object
    whose class declares the property, where today `find_property_owner`
    succeeds on the first member that has it. So `Foo|Bar` with `Bar`
    lacking `p` isn't always present either.
  - The property's type is the narrowed entry for the chain's narrowing key
    when `locals` has one, else the declared type.
    `$x->p = null; isset($x->p)` on an `int|null` property then reads
    `null`, not the declaration.
  - `unset($x->p)` makes the property uninitialized. `Locals` keeps a small
    set of unset property keys: `check_unset_target` adds the key, and any
    write to the key or its base removes it. The arm returns `None` for a
    key in the set.
  - The same set drives the read that PHP refuses: reading a typed
    property after `unset`, before any write, is an `Error` ("must not be
    accessed before initialization"). A plain read (not `isset`, `??` or
    `empty`) of a key in the set reports `property.uninitialized` at level
    0, the identifier the constructor-dataflow check
    (`uninitialized_dataflow.rs`) uses for the same runtime error. An
    untyped property reads `null` after `unset` with PHP's "Undefined
    property" warning (checked with `php`), reported as
    `property.notFound` at level 0, the identifier an undeclared property
    read already gets.

  Fixtures: with `?Foo $x` and `Foo::$p` an `int` with a default,
  `isset($x->p)` and `$x->p ?? 2` are clean.
  `unset($x->p); isset($x->p)` is clean. A non-null `Foo $x` still
  reports `isset.property`. A `Foo|Bar` receiver where only `Foo`
  declares `p` is clean.
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

  Design: `1c33a059` and `310b2205` did the forgetting half. Probed on
  the 21:15 binary: after `$this->x = null; $this->load();` the property
  is `string|null`, and the `(string) $this->x === ''` and
  `$this->x === ''` shapes after a `load()` report nothing. What is left
  is F82's gaps and then the guards. The guards are two syntactic
  walkers:

  - `expr_reads_a_property` (`expr/ranges.rs:24`), used by
    `check_constant_numeric_comparison` and the loose comparison
    (`probe_checks.rs:358`);
  - `expr_reads_a_mutable_property` (`probe_checks.rs:675`), used by
    `check_strict_comparison_of_different_types`.

  Both skip any comparison whose operand reads a property, and
  `expr_reads_a_property` also skips class constants and `::class`,
  which can't change. So `self::X === 1` and `count(self::LIST) > 0` are
  never checked, a missed report the guard causes on its own. Once F82
  makes every call, alias, static write and implicit call forget what it
  may change, both walkers and their three call sites go, and the
  comparisons run on the narrowed types like any other operand. Removing
  them before F82 would bring back the false positives F82's shapes
  describe, so the order is F82, then this. Fixtures: the finding's two
  shapes stay clean after the removal. `self::X === 1` with
  `const X = 1` reports `identical.alwaysTrue`.
  `$this->x = null; if ($this->x === null)` with no call between
  reports.
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

  Design: share the endless-loop exit.

  - The fold in `check_while`'s `is_truthy_literal(cond)` branch
    (`loops.rs:159`) moves into `endless_loop_exit(break_states, locals)`.
    After the loop comes the merge of the `break` states alone, and with
    none the code after is unreachable, keeping today's fallback of the
    pre-loop state.
  - `check_for` calls it both for the condition-less arm (`None`), which
    today starts from `body_locals` and so carries the body's end state (the
    one without `$y`), and for a condition list whose last expression is a
    truthy literal (`for (; true; )`, `for ($i = 0; 1; $i++)`). That case
    today goes through the `Some(c)` arm's "may run zero times" merge and
    the false-branch narrowing of `true`.
  - `check_do_while` does the same when its condition is a truthy literal.

  For `doWhile.alwaysFalse` (`loops.rs:292`), a `continue` that targets
  the loop leaves a `do { … } while (false)` just as a `break` does,
  since control goes to the condition, which is false. So the exemption
  asks F47's depth-aware walk for a `break` or a `continue` whose level
  targets this loop. A `continue` inside a nested loop doesn't exempt.
  Fixtures: `for (;;) { if ($c) { $y = 1; break; } } return $y;` is
  clean; the same with `for (; true; )` is clean;
  `do { if ($c) { continue; } … } while (false);` is clean; a `continue`
  inside a `foreach` nested in the `do` still reports.
- [ ] **F64. A `mysqli` query that doesn't start with its verb is typed
  `true`** (`returns_result_set`, `checker/expr/mysqli.rs`; also before
  the window). Only the first word after any `(` is read:
  `$db->query('/* report */ SELECT 1 AS a')`, `"-- c\nSELECT 1 AS a"`,
  `'CALL list_users()'` and `'CHECK TABLE t'` are `true`, so
  `$r->fetch_assoc()` reports `method.nonObject`. Done: leading comments
  are skipped, and `CALL`, `CHECK`, `ANALYZE`, `OPTIMIZE`, `REPAIR`,
  `HELP` or a verb the list doesn't know give `mysqli_result|true`, with a
  fixture.

  Design: `returns_result_set` (`expr/mysqli.rs:13`) becomes a three-way
  `statement_result(sql) -> ResultKind { ResultSet, NoResultSet, Either }`.
  It finds the verb with `sqlparser`'s tokenizer under `MySqlDialect`:
  the dependency is already there, and
  `sql_reflection/parameter_validation.rs` tokenizes the same way. Two
  details follow:

  - Whitespace tokens carry `--`, `#` and `/* */` comments, so skipping them
    and any leading `(` reaches the first `Word`.
  - A MySQL executable comment (`/*! … */`) is text the server runs, so it
    counts as unknown.

  The verbs split three ways:

  - a result set: `SELECT`, `WITH`, `TABLE`, `VALUES`, `SHOW`, `DESCRIBE`,
    `DESC`, `EXPLAIN`, `CHECK`, `CHECKSUM`, `ANALYZE`, `OPTIMIZE`, `REPAIR`,
    `HELP`, and an `INSERT`, `REPLACE` or `DELETE` with a top-level
    `RETURNING` (MariaDB);
  - no result set: `INSERT`, `UPDATE`, `DELETE`, `REPLACE`, `SET`, `CREATE`,
    `DROP`, `ALTER`, `TRUNCATE`, `RENAME`, `GRANT`, `REVOKE`, `USE`,
    `BEGIN`, `START`, `COMMIT`, `ROLLBACK`, `SAVEPOINT`, `RELEASE`, `LOCK`,
    `UNLOCK`, `LOAD`, `DO`, `FLUSH`, `KILL`, `RESET`, `PREPARE`,
    `DEALLOCATE`;
  - either: `CALL`, `EXECUTE`, `HANDLER`, and any verb the lists don't know.

  `mysqli_query_type` maps `Either` to `mysqli_result|true`, the same
  type an unfoldable string gets today. Only `ResultSet` reaches the
  row-shape resolution. Fixtures: the four queries of the finding
  (`/* report */ SELECT`, `-- c\nSELECT`, `CALL list_users()`,
  `CHECK TABLE t`) all allow `->fetch_assoc()` without
  `method.nonObject`; an `INSERT` is still `true`; an unknown verb is
  `mysqli_result|true`.
- [ ] **F65. A spread argument counts as one argument**
  (`checker/expr/array_ops.rs`; also before the window).
  `check_useless_return_value_call` reads `args.len()`, so `$x =
  var_export(...$args)` reports `function.uselessReturnValue` though
  `$args` may hold `true` for `$return`. `check_array_filter_no_op` and
  `check_array_values_no_op` do the same and judge `array_filter(...$args)`
  by the type of `$args` itself (read, not probed). Done: a spread
  argument makes each check bail, with a fixture.

  Design: the three checks are instances of a wider pattern. The builtin
  special cases (`infer_builtin_call_return_type`, 140
  `args.len()`/`args[i]` reads in `builtin_calls.rs`) and the call
  checks in `array_ops.rs` index the raw argument list. Its doc says
  named arguments are not worth handling. Only four arms (`min`/`max`,
  `compact`, and the `array_intersect`/`array_merge` spread unwrapping)
  look at `a.spread`. So a spread or a reordered named argument can
  reach an arm that reads position 1 as the second parameter. The fix is
  one normalization step ahead of all of them:

  - `positional_args(args, sig) -> Option<Cow<[Arg]>>` returns the list in
    parameter order. A named argument moves to its parameter's position. A
    gap, an omitted optional parameter before a later named one, gives
    `None`, since the arms assume a dense prefix. A spread whose value is
    a sealed list shape (`...[1, 2]`, or a variable typed
    `array{int, string}`) is expanded into one argument per element, a
    string-keyed sealed shape into named arguments, as PHP 8.1 unpacks
    them, so it resolves like the written-out call. Any other spread gives
    `None`, unless the function is in the small `SPREAD_AWARE` set (the
    arms that already unwrap a spread). The same expansion is shared with
    F72 and with `check_args`, whose argument-count and type checks gain
    the same precision.
  - `infer_builtin_call_return_type` calls it right after the polyfill
    check, falling back to `infer_call_return_type`'s generic path on
    `None`. `type_check_call_outcome` and every `check_*` taking `args` in
    `array_ops.rs` (`check_useless_return_value_call`,
    `check_array_filter_no_op`, `check_array_values_no_op`) and in
    `castable_arguments.rs` do the same, skipping the check on `None`.

  The doc's named-argument sentence is replaced by a pointer to the
  helper. Fixtures: `$x = var_export(...$args)` is clean;
  `array_filter(...$args)` isn't judged by `$args`'s type;
  `explode(limit: -1, separator: ',', string: $s)` is `list<string>`
  (with F49); `array_slice(array: $a, offset: 5)` keeps the generic
  return.
- [ ] **F66. Two live shapes the opt-in dead-code rules report**
  (`project::whole_program`; also before the window):
  - `class.unused` for a class named only in PHPDoc (`@param DocOnly
    $d`). `dead_code`'s module doc lists attributes, string callables and
    framework magic as known gaps, not PHPDoc.
  - `method.unused` for `A::mixedCase()` called as `$a->MIXEDCASE()`:
    member references are recorded under the name as written.

  Done: a PHPDoc type counts as a reference and member references fold
  case, with fixtures.

  Design: two index changes, each at the recording end.

  - PHPDoc class references. `ClassFnRefWalker` (`project/use_sites.rs`)
    also reads each doc comment the AST carries: a declaration's, a
    parameter's, a property's, and a statement's inline `@var`. It parses
    them with `phpdoc::docblock::parse` and walks every `TypeNode` in its
    tags (`@param`, `@return`, `@var`, `@throws`, `@method`, `@property*`,
    `@mixin`, `@extends`/`@implements`/`@use`, `@template … of`,
    `@phpstan-assert*`, `@phpstan-type`/`-import-type`, `@param-out`,
    `@param-closure-this` and `@phpstan-self-out`; not `@see` or `@uses`,
    which name code rather than type it). Each `Identifier`, `Generic` head,
    `ConstFetch` class and callable keyword that isn't a type keyword, a
    template in scope or a local type alias is resolved against the file's
    name context, and recorded as `RefTarget::Class`. The walker keeps that
    context as it walks the file's `namespace` and `use` statements. The
    site is the name's exact span within the comment when the source is
    retained, else the comment's span, the fallback
    `index_class_fn_refs_without_source` already uses.
  - Member case. `RefTarget::Member(owner, name)` splits by kind into
    `Method(owner, lowercased name)`, `Property(owner, name)`,
    `Constant(owner, name)` and `EnumCase(owner, name)`, since only method
    names fold in PHP. `record_member_ref` (`checker.rs:1226`) takes the
    kind, which each of its call sites knows (a call, a property fetch, a
    constant fetch), and the dead-member and dead-case queries
    (`dead_members.rs:216`, `dead_enum_cases.rs:62`) build the matching key.
    A method named in a string callable, which F7's function index doesn't
    cover, is unchanged.

  Fixtures: with `check_dead_code` on, `@param DocOnly $d` keeps
  `DocOnly` used, as do `@return list<DocOnly>`, `@var DocOnly $x`
  inline and `@extends Base<DocOnly>`; `$a->MIXEDCASE()` keeps
  `mixedCase()` used; a property read as `$a->Prop` doesn't count for
  `$prop`.

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

  Design: a widening operator has to reach a fixed point in a bounded
  number of passes whatever the body does. `widen_towards`
  (`types/src/queries.rs:328`) recurses field by field, so a field whose
  new value holds the old shape one level deeper recurses one level
  deeper on every pass. That is the classic unbounded chain, and the
  passes end before it does. The standard cure is a depth-sensitive
  step:

  - A helper `array_depth(ty)` gives the nesting depth of array shapes and
    general arrays in a type (the max over union members, 0 for a
    non-array).
  - In the combined-shape branch, a field is widened with `widen` only when
    `array_depth(new) <= array_depth(old)`. A field that gained a level
    becomes `new.generalize()` with every array nested inside collapsed to
    `array<generalized key, mixed>` (a new `Type::collapse_nested_arrays`).
    The field keeps "is an array" and its key kind, and gives up the values
    it could not bound. The next pass sees an unchanged depth and settles.
  - The general-array branch's `before.widen_towards(&after)` on values gets
    the same test, since a general array whose values nest deeper each pass
    has the same chain.

  After the finding's loop, `$a` is
  `array{x: int, y?: array<string, mixed>}`, and
  `($a['y']…['x'] ?? 1) === 1` has no constant verdict. The `prev`/`i`
  shape bottoms out the same way, at `prev?: array<string, mixed>`, with
  no `array{}` claim.

  `combined_shape`: when a key is missing from some member, the field is
  made optional. If that member is unsealed and its tail key type may be
  the key, the field's type also joins the tail's value type, since the
  key may be there with a tail value. That is the same rule F85 gives
  writes. Fixtures: both loops of the finding, a types-crate test that
  widening a self-nesting shape reaches the same type on two consecutive
  passes, and `combined_shape` of
  `array{a: 1}|array{b: 2, ...<string, int>}` giving `a?: 1|int`.

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

  Design: `infer_array_literal_type` (`expr/array_ops.rs:84`) stops
  maintaining `next_index` the moment it goes general. The
  `(key, general)` arm pushes a key without `observe`, and a positional
  item still asks the stale index. The index has to keep following the
  keys in general mode, where it can become uncertain:

  - `NextArrayIndex` (`types/src/shape.rs`) gains a range form: per version
    rule, `next` becomes `(min, Option<max>)`, with the exact index the case
    `min == max`.
  - `observe_type(key_ty)` observes each finite int value of a key type. For
    a key whose int part is a range `int<lo, hi>`, it raises the index to
    `max(next, lo + 1)` at the bottom and `hi + 1` (or unbounded) at the
    top. A key type with no int part (`'a'|'b'`) leaves the index alone.
  - `take()` returns the key type an append gets: an exact key, or
    `int<min, max>` for a range.
  - `AppendKey` grows a `Range(min, max)` case beside `Key(n)`.

  The general-mode arm then calls `observe` for an `Ok(Int(n))` key and
  `observe_type` for an `Err(ty)` key, and a positional item takes
  `take()`'s key or range. So `[$ab => 1, 5 => 2, 'y']` gives keys
  `'a'|'b'|5|6`, and `[$r => 1, 'y']` with `int<10, 20> $r` gives `'y'`
  the key `int<11, 21>`. The duplicate-key check's early `return` on
  `Err` stays, since a range can't prove a duplicate. F70's first bullet
  is the same fix. Fixtures: both finding shapes, and `[$i => 1, 'y']`
  with a plain `int $i`, whose positional key is `int`.
- [ ] **F14. `iterable<K, V>`'s array member drops `K`** (`321d388c`).
  The `"iterable"` arm in `parse.rs` builds
  `array(unknown_array_key, V)`, though it parsed `K`. For
  `iterable<string, int>`, the `foreach` key is `int|string` and
  `is_array($it)` gives `array<string>`. The commit's fixture asserts
  `int|string`. Done: `array<K, V>`.

  Design: in the `"iterable"` arm (`types/src/parse.rs:1129`; after F41
  it moves into `keyword_type`), the array member's key is the written
  `K` restricted to what an array key can be: `K` itself when it is
  within `int|string`, `unknown_array_key()` for `mixed`, and the
  intersection with `int|string` otherwise. When that intersection is
  `never` (`iterable<object, V>`, a generator-only key), the array
  member is dropped and the type is `Traversable<K, V>` alone, since no
  array can have those keys. The arm builds its union with
  `Type::union_of` rather than a raw `Repr::Union`, so it is normalized
  like every other union. `321d388c`'s fixture, which asserts
  `int|string` for the key, is corrected to `string`. Fixtures: for
  `iterable<string, int>`, the `foreach` key is `string` and
  `is_array($it)` narrows to `array<string, int>`;
  `iterable<int|object, V>` has an `array<int, V>` member;
  `iterable<object, V>` is `Traversable<object, V>`.
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

  Design: three changes in `types`.

  - The raw class is a supertype of its generic instances. In
    `super_type.rs`, the first `Object`/`Object` arm (line 201) answers
    `Yes` only when both argument lists are empty. An `instanceof` target
    always carries no arguments, so `Iterator` must be a supertype of
    `Iterator<int, string>` and of any subclass instance whatever its
    arguments. The arm becomes: a sup with no arguments, whose class
    declares templates or not, is `Yes` for a sub of the same class or a
    subclass (`hierarchy.is_subclass_of`). `without`, and so the negated
    `instanceof`, then removes the generic member, and `narrow_to_instance`
    keeps it whole in the true branch, both through the ordinary relation.
  - Object disjointness. `narrow_to_instance`'s intersection fallback asks a
    new `objects_disjoint(a, b)` first, and builds `never` when it holds. It
    holds when:
    - two classes (not interfaces) are unrelated, by single inheritance;
    - a final class or an enum isn't a subtype of the other;
    - the pair is PHP's mutually exclusive `Iterator`/`IteratorAggregate`
      (implementing both is a fatal error), or one of their subtypes each.

    `IteratorAggregate<int, string>` narrowed to `Iterator` is then `never`,
    and so is any `final class C` narrowed to an interface it lacks. With
    F4's `never`, the union case drops that member.
  - Carrying arguments down. When `narrow_to_instance` narrows a generic
    `current` (`Traversable<int, string>`) to a generic target named without
    arguments (`Iterator`), it binds the target's templates. The hierarchy's
    `ancestor_template_bindings(target, placeholders, current_class)` says
    how the target's placeholders map to the ancestor's arguments, and
    `infer_template_bindings` against `current`'s arguments solves them. The
    result is `Iterator<int, string>`, with an unsolved template left at its
    bound.

  Fixtures: the three shapes of the finding, and `final class C {}`
  narrowed to `Countable` being `*NEVER*` (with `instanceof.alwaysFalse`
  reported as today).
- [ ] **F16. Division by zero misses the divisors PHP converts to zero**
  (`b1dddb6c`). `$i / false`, `/ null`, `/ '0'`, `% 0.5`, `% false` and
  `% '0'` all throw `DivisionByZeroError` (checked with `php -r`), but
  `is_zero` compares only with int `0` and float `0.0`. Done: a divisor
  whose every value converts to zero is reported - to a number for `/`,
  to an int for `%`.

  Design: `check_division_by_zero` (`operators.rs:364`) compares each
  finite value of the divisor with `0` and `0.0` as written. PHP
  converts first, differently per operator (checked with `php`: every
  case of the finding throws, and so do `1 / '0.0'`, `1 % -0.9` and
  `1 % ' 0'`):

  - `/` converts to a number: `null` and `false` are `0`, `true` is `1`, and
    a numeric string is its value.
  - `%` converts to an int: a float or numeric string truncates toward zero,
    so any `|f| < 1` is `0`, and `null`/`false`/`true` are `0`/`0`/`1`.
  - A non-numeric string or an array is a `TypeError` ("Unsupported operand
    types"), which `binaryOp.invalid` owns, so it doesn't count as zero
    here.

  So the test becomes: every finite value, converted by the operator's
  rule, is zero. The conversions are the types-crate `to_number` from
  F39 and a sibling `to_int` for `%`. `-0.0` counts, and its literal
  already compares equal. F19's fourth bullet is the result side of the
  same fact. When the divisor always converts to zero, `infer_binary`
  gives the explicit `never` of F4's split rather than `int|float`,
  since the expression always throws. Fixtures: `$i / false`, `/ null`,
  `/ '0'`, `/ '0.0'`, `% 0.5`, `% '0.9'`, `% false` and `% null` each
  report, and `/ ''` reports only `binaryOp.invalid`.
  `$r = $i / 0; echo $r;` reports `division.byZero` and
  `deadCode.unreachable`.
- [ ] **F17. `new` with a non-name class expression isn't checked.**
  `check_expr`'s `New` arm (`expr.rs:2218`) runs `infer_expr` on the whole
  `new` and handles a `Name` class only. `new $undef()`,
  `new $undef->cls()` and `new $undef['k']()` report no undefined
  variable. This was already the case before the fix window; `3f04ac3f`
  widened the grammar there without checking it. Done: the class
  expression goes through `check_expr`.

  Design: both `New` arms treat a non-`Name` class expression as opaque.
  `check_expr`'s arm (`expr.rs:2352`) starts from `infer_expr(e)`, never
  checks `class_expr`, and so never reports on it. `infer_expr`'s arm
  (`expr.rs:4616`) answers `mixed` for it (`_ => Type::mixed()`). So:

  - `check_expr` first runs `self.check_expr(class_expr, locals, label)`
    when the class expression isn't a `Name`. The variable, property, offset
    or parenthesized-expression forms `3f04ac3f` added then report what they
    report elsewhere: undefined variable, undefined property, missing
    offset. It then checks what came back: a value that can't be a string or
    an object is PHP's `Error` "Class name must be a valid object or a
    string" (checked with `php` for an `int`), reported through
    `failing_level` like the other operand checks. The identifier is a new
    `new.invalidClass`.
  - `infer_expr` types the instance from that value:
    - a `class-string<T>` or a class-string literal gives `T`, with the
      constructor-argument template binding the `Name` path already does;
    - an object type gives that same type (`new $o()` makes another instance
      of `$o`'s runtime class, as `php` confirms for `ArrayObject`);
    - a plain `string` gives `object`, and anything else `mixed`.

  While there, the `self`/`parent`/`static` match compares `written` by
  case, though keywords fold (`new SELF()` is valid PHP), so it compares
  with `eq_ignore_ascii_case`. Fixtures: `new $undef()`,
  `new $undef->cls()` and `new $undef['k']()` report
  `variable.undefined`; `new $cls()` with `class-string<Foo> $cls` is
  `Foo` and checks its constructor's arguments; `new $i()` with `int $i`
  reports `new.invalidClass`.
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

  Design: each shape gets its cause confirmed with a
  `--features hotpath` build (the crate's `hotpath`/`hotpath-alloc`
  features) on the generator scripts this review timed with, and then
  its fix. The causes below are read from the code; the profile decides
  between them.

  - `&&`/`||` chains. `check_expr`'s `And`/`Or` arm (`expr.rs:1513`) runs
    `apply_narrowing(l, …)` on the whole left operand to build the right
    operand's scope. `apply_narrowing` of an `&&` recurses into both of its
    sides, so every level re-narrows the chain below it, O(n²).
    `check_boolean_and_or` and `infer_binary`'s `And` arm narrow it once
    more each. The fix computes a condition's branch scopes once per node.
    `check_expr` on `&&`/`||` returns (through a side table keyed like the
    infer cache, by node and scope version) the true and false scopes it
    already built for its right operand, and `apply_narrowing` of an
    `&&`/`||` node reuses a cached pair instead of recursing. This is
    PHPStan's `SpecifiedTypes` computed bottom-up.
  - An `||` of `instanceof` checks (705 s at 10,000). Each level unions one
    more class into the narrowed type with `Type::union`, and each union
    re-runs absorption over all members, O(n²) or worse. The false branch
    removes one class per level the same way. With the cached branch scopes
    above, the chain's true branch is built with one `union_of` over the
    leaves' targets, collected when the `||` node is visited, not once per
    level.
  - `.` chains of literals (51.8 s at 8,000). The candidate is per-level
    work proportional to the left operand. The profile decides between
    building the growing literal (`concat` of an ever-longer
    `StringLiteral`, rehashed at intern time) and a subtree walk in the
    per-level checks, which is removed as above. The literal is not
    capped or generalized, since that loses a value the code states. If
    building it is the cost, the literal's text becomes a shared rope
    whose hash is combined from its parts', so appending is O(1)
    amortized and interning never rehashes the prefix.
  - Nested `match` (71 s at 1,000). `narrow_match_arm_conditions` gives each
    arm a scope with a new version, so the infer cache misses when an outer
    arm's `infer_expr` walks the inner match again. The inner match's result
    type is recorded when `check_expr` computes it, keyed by node, so the
    outer walk reads it back.
  - The trait chain (cubic, 63 minutes at 3,000). A class's trait members
    are flattened by walking the used traits recursively, and each lookup
    through the chain repeats that walk. The registry builds every trait's
    flattened member table once, in dependency order, each from its used
    traits' finished tables, and lookups read the tables.
  - `elseif` chains. `check_if`'s `rest.iter().all(always_terminates)`
    (`stmt.rs:1233`) and `uninitialized_dataflow.rs:383` recompute a suffix
    answer at every `elseif`. They precompute it once, back to front, O(n).

  Done is the finding's: each shape measured linear up to 20,000 terms,
  with the timings recorded beside the scale scripts.
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

  Design, bullet by bullet, re-probed on the 21:15 binary:

  - The growing literal. `$s = 'a'; while ($c) { $s .= 'b'; }` is now
    `non-falsy-string`, so this bullet is fixed.
  - `40e086a6`'s intersection. In `types/src/intersection.rs`, the list and
    shape case checks a shape's keys against the list's. A required string
    key can't be in a list, sealed shape or unsealed, so the intersection is
    `never` whenever any required field's key is a string. An optional
    string field is dropped, and an unsealed tail keeps only its int keys.
  - `\int`. PHP refuses it at compile time ("Type declaration 'int' must be
    unqualified", checked with `php`). Where native hints resolve names
    (`bd7d7a89`'s type-hint node), a fully qualified name whose last segment
    is a reserved type keyword (`int`, `string`, `bool`, `float`, `array`,
    `callable`, `iterable`, `object`, `mixed`, `void`, `null`, `never`,
    `false`, `true`, `static`, `self`, `parent`) gets one non-ignorable
    report, `typeHint.qualifiedReserved` with PHP's message, and is then
    read as that keyword, so no `class.notFound` or argument report follows.
    Probed today: `function k(\int $x)` reports `class.notFound` for `int`.
  - `$i / 0`: F16's result side, the explicit `never`.
  - PHPDoc `1e25`: F41's lexer.
  - `array_pop`/`array_shift` by reference. Probed:
    - on `array{a: 1, b: 2}` both give `array`;
    - on `array<string, int>` they give `array`;
    - on `[1, 2, 3]` they give `list<1|2|3>`;
    - `non-empty-list` gives `list`, which is right.

    The by-reference effect becomes the array without that element, through
    a types-crate `without_last`/`without_first`:
    - a sealed shape loses its last or first field, and `shift` renumbers
      int keys as PHP does, so `[1, 2, 3]` popped is `array{1, 2}`;
    - a general array keeps its key and value types and loses only
      `non-empty`;
    - a list stays a list;
    - an unsealed shape turns general.
  - `$x['a']` on `array{a: 1}|array{b: 2}`. `offset_value_type` (in
    `types/src/offsets.rs`) gives `mixed` when one member lacks the key.
    Probed: still `mixed`. The read is the union of the members' values for
    that key, with `null` for a member that surely lacks it, since PHP reads
    a missing offset as `null` with a warning: `1|null` here. The read
    itself is also reported, since a member that lacks the key makes PHP
    warn "Undefined array key". `report_missing_offset` answers today only
    when every member lacks the key (`offsetAccess.notFound`). It gains the
    "some member" case: `offsetAccess.maybeNotFound` ("Offset 'a' might not
    exist on array{a: 1}|array{b: 2}.") at level 7, the maybe level, for a
    literal key some constant-array member surely lacks, or has only as
    optional. The quiet reads (`isset`, `??`, `empty`, `array_key_exists`
    guards) are exempt, as they are for the definite report, and so is a
    read the enclosing condition has narrowed (`if (isset($x['a']))`).

  Fixtures: one per bullet still open, with the probed types as the
  before state.

- [ ] **F30. A promoted property's set visibility is ignored.**
  `class_info.rs:1264` sets `write_visibility` from
  `promotion.visibility`, and nothing reads `promotion.set_visibility`
  (parsed, and carried into `Promotion` by `ac5240a9`). For a promoted
  `public private(set) int $x`, `$p->x = 2` from outside is unreported,
  where PHP gives "Cannot modify private(set) property"; a declared
  property's `private(set)` is reported. This was already the case
  before the fix window. Done: `set_visibility`, falling back to the
  visibility, as a declared property has it.

  Design: at `class_info.rs:1264` the promoted arm sets
  `write_visibility` from `promotion.visibility`. It becomes
  `promotion.set_visibility.unwrap_or(promotion.visibility)`, the
  declared arm's rule (line 1161,
  `p.write_visibility.unwrap_or(p.visibility)`).
  `implicit_protected_set` follows the declared arm too: readonly, no
  explicit set visibility, and public. Today the promoted arm leaves out
  the "no explicit set visibility" condition, so
  `public private(set) readonly` would also get the implicit
  `protected(set)`. A set visibility alone promotes
  (`protected(set) string $z` is a public property, as the `Promotion`
  doc says), and that stays. Checked with `php`:
  `public private(set) int $x` and `protected(set) string $z` written
  from the global scope fail with "Cannot modify private(set) property"
  and "Cannot modify protected(set) property". Fixtures: those two
  writes from outside report, as a declared `private(set)` does; a write
  from inside the class is clean; a subclass writing the
  `protected(set)` one is clean.
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

  Design: one class-check sequence for both kinds of class.

  - The body of the declared-class loop in `checker.rs` (from the
    `check_misplaced_var_tag` call near line 1502 to the methods loop and
    the private-member pass near 1640) moves into
    `check_class_def(fqn, def, name_span, anonymous: bool)`.
  - The declared-class loop calls it with `false`, and
    `check_anonymous_class` (`expr.rs:154`) with `true`, which replaces its
    hand-picked subset.
  - The few checks that mean nothing for an anonymous class skip on the
    flag: the class name's case, the misplaced class `@var`, `@sealed` and
    `@require-*`, which need a nameable class. So do the checks the
    anonymous path reaches another way, like `check_attributes`, which
    `NewAnonymousClass` already runs.

  Everything else then runs for an anonymous class: completeness (the
  `Countable` and abstract-parent fatals), class references (the unknown
  interface), generics and ancestor bounds, overrides, property and
  constant checks, trait conflicts, uninitialized properties, and the
  private-member pass. Messages name the class through the anonymous
  display name (see F33's bullet on it). Fixtures: the three shapes of
  the finding report,
  `new class extends Base { public function abstractOne() {} }` is
  clean, and an anonymous class using two colliding traits reports the
  collision (F29).
- [ ] **F32. Smaller gaps from commits 100 to 155:**
  - `b1dddb6c`'s companion `binaryOp.invalid` says `'abc' + 1` "results
    in an error" on `--php-version 7.4` too, where it is `1` with a
    warning; the version flag `1f4b45bf` added isn't consulted there.
  - `1cb1e25d`: a union of callable names isn't resolved for the pipe's
    by-reference check.

  Done: each is fixed or ledgered.

  Design:

  - The version. `binary_operand_validity` (`operators.rs:388`) decides a
    non-numeric string operand is invalid through the coercion that
    `1f4b45bf` made version-aware (`to_number` refusing a non-numeric string
    on PHP 8). The report in `check_binary_operand_validity` (line 201)
    doesn't consult the range. A non-numeric string is a `TypeError` from
    PHP 8.0, and a warning with the value `0` before. So the report fires
    when the failure happens on some version the code runs on: the
    configured maximum is 8.0 or later, or no version is configured. It is
    skipped when the maximum is below 8.0, since there the operation runs.
    The operands that fail on every version (an array with `-`, an object
    without number casting) are unaffected. The leading-numeric case
    (`'5 apples' + 1` is `6` with a warning on 8.5, checked with `php`)
    isn't an error on any version. F34 types it as its leading number
    and gives it its own warning report, `binaryOp.nonNumericValue`.
    Below 8.0 the same report covers a wholly non-numeric string too:
    there it is `0` with a warning rather than a `TypeError`, so when the
    version range lies below 8.0 it replaces `binaryOp.invalid` instead
    of being dropped.
  - The pipe's callable union. The same resolver as F9's `pipe_callee`,
    which resolves each member of a union of names and requires all of them
    to agree before the by-reference check reports. It is one change with F9
    and F22's identical bullet.

  Fixtures: `'abc' + 1` with `--php-version 7.4` reports only
  `binaryOp.nonNumericValue`, and with 8.0 or a 7.4 to 8.3 range reports
  `binaryOp.invalid`.

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

  Design: two parts, binding and checking.

  - Binding (`types/src/templates.rs`, the `TemplateParam` arm near line
    39). The bound's own templates are bound into `bindings` (line 48)
    before the bound check decides whether `T` binds at all, so a rejected
    argument still leaves `E` joined with `DateTime`. The arm binds into a
    trial copy: clone `bindings`, run the bound's inference into the clone,
    and test the substituted bound against the argument. Only when it holds
    are the clone's new bindings kept and `T` bound. On rejection `bindings`
    is untouched and the template is recorded in a `rejected` set that the
    call's inference returns beside the bindings.
  - Checking. The argument loop in `calls/parameters.rs` (near line 290)
    compares each argument with `param.ty()` as declared, and a template
    placeholder accepts anything (`accepted_though_no_subtype`'s
    `(_, TemplateParam)` arm). The loop computes the call's bindings first,
    through F38's shared `call_template_bindings`, which returns the
    `rejected` set too. It checks each argument against the parameter type
    with the call's own templates substituted: a bound template by its
    binding, and a rejected or unbound one by its bound. Then `cnt(5)` is
    checked against `Countable` and reports "expects Countable, 5 given", as
    PHPStan's resolved parameter type does. A bound-respecting argument is
    checked against its own binding, which holds, so nothing that passes
    today starts to fail.

  The return type already falls back to the bound for a rejected
  template, which stays. Fixtures: `cnt(5)` and `onlyInt('s')` report
  `argument.type`; the `Other::class` call reports and returns
  `Repo<stdClass>`, not `Repo<stdClass|DateTime>`; `3cea2a20`'s fixture
  stays clean.

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

  Design, per bullet:

  - `promote_class_string_literals` (`types/src/templates.rs:197`) walks the
    argument in parallel with the parameter. For an `Array` parameter it
    maps the argument's value types however the argument is built: a general
    array's value, each field and the tail of an `ArrayShape`, the array
    member of an `Intersection` (accessories kept), and each member of a
    union argument. A `list<class-string<T>>` parameter (an intersection)
    matches through its array member. `first(['Foo'])` then binds `T` to
    `Foo`.
  - `describe_accessory_intersection` (`types/src/display.rs`) takes its
    `Array` fast path only when every other member is the list or non-empty
    accessory. Any other accessory, `hasOffsetValue` among them, falls
    through to the general `&`-joined rendering, so messages and
    `assertType` show
    `non-empty-array<string, mixed>&hasOffsetValue('x', 1)`. The comment F43
    flags is corrected with it.
  - The `?iterable` message. The missing-typehint walk (`2b1ce22b`)
    describes the type by its first member, and the parser expands a native
    `iterable` into `array|Traversable`, so the array member names it. The
    walk describes a native hint from the hint node (`iterable`), as it did
    before `2b1ce22b`, and falls back to the parsed type only for PHPDoc
    types, where `iterable<…>` is written out anyway.
  - `argv`/`argc`. They leave `is_superglobal` (`syntax/src/ast.rs:1589`).
    The checker seeds them into the global scope's `Locals` only, typed
    `list<string>` and `int<1, max>` (the CLI shape; unset under a web SAPI,
    where reading them is as undefined as any variable), and
    `unassigned_variable_type`'s `argc` arm goes. Inside a function they are
    undefined unless `global $argv;` brings them in, which the `Global`
    statement already handles.

  Fixtures: one per bullet. `$argv` read inside a function reports
  `variable.undefined`, and at the top level it is `list<string>`.

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

  Design: one name resolver for every narrowing dispatch, and the tagged
  set folded. PHP resolves an unqualified function call in a namespace
  to the namespaced function if it exists and to the global one
  otherwise, and matches both case-insensitively. So `COUNT($x)` in
  `namespace App;` is the builtin, but not when an `App\count` exists.
  Today the arms compare the written text:
  `name.trim_start_matches('\\') == "count"` at `narrowing.rs:30` and
  `975`, and so on for `in_array` (1057), `array_key_exists` (1077), the
  `str_*` trio (1122), `is_a` (1160), `preg_match`
  (`predicates.rs:211`), `assert` (`stmt.rs:1329`) and `extract`
  (`expr.rs:472`). So a user's `App\count` narrows like the builtin, and
  `COUNT` narrows nothing.

  - A
    `Checker::builtin_called(name: &QualifiedName) -> Option<&'static str>`
    resolves through `registry.called_function_name`. It returns the
    lowercase builtin name only when the function reached is the stub
    (`is_stub()`), so a polyfill or namespaced override doesn't narrow as
    the builtin. Every arm above matches on its result. `extract`'s two
    branches, `name == "extract"` and the folded one, collapse into it.
  - `names_with_assert_tags` (built in `registry/index.rs`) stores
    lowercased resolved names, and `apply_bare_statement_assert_narrowing`
    looks up the lowercased resolved call, for functions and for
    `Class::method` keys alike (methods fold, F35).
  - `narrow_enum_from_call` (`expr/array_ops.rs`) compares `from` and
    `tryFrom` with `eq_ignore_ascii_case`. `or_chain_narrowing_target` looks
    predicates up through `builtin_called`. The PHPUnit assertion table
    (`c58bba2a`'s `assertIsX` family) matches method names folded.

  Fixtures: `COUNT($a) > 0`, `IN_ARRAY($x, ['a'], true)`,
  `Suit::FROM(1)`, `IS_INT($x) || is_string($x)`, `A::ISSTR($a)` with a
  tag, and `$this->ASSERTISINT($x)` all narrow; a
  `namespace App; function count(...)` does not narrow like the builtin.

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

  Design:

  - The short ternary, in both arms (`check_expr` at `expr.rs:2174`,
    `infer_expr` at `4871`). `then_` is `None` and the arm returns `mixed`.
    The value of `$c ?: $e` is `$c` itself when truthy, so the then-type is
    the condition's type in the truthy scope:
    `current_narrowed_type(cond, key, &then_locals)` when the condition has
    a narrowing key, else `cond_ty.without_falsy()`. The result unions it
    with the else type. `check_expr` already builds `then_locals` and
    `else_locals` and evaluates the condition once, which stays so.
  - The compound assignment. `check_expr`'s `CompoundAssign` arm (line 2108)
    computes the assigned `ty` and then returns `Type::mixed()`. It returns
    `ty` (cloned before `bind_assignment_target` consumes it), since PHP's
    value of `$n += 1` is the new `$n`. The `??=` case is `infer_binary`'s
    `Coalesce`, the same `widen_coalesce_result` rule as `??`. `infer_expr`
    gets the matching `CompoundAssign` arm, returning
    `infer_binary(op, lhs, rhs, locals)`, so both paths agree, as the
    infer-cache assumption (F33) needs.

  Fixtures: `$s ?: 'd'` with `?string $s` is `non-falsy-string|'d'`
  (`string` minus `''` and `'0'`, plus `'d'`); `$z = ($n += 1)` with
  `int<0, 5> $n` makes `$z` `int<1, 6>`; `$y = ($a ??= [])` is `$a`'s
  non-null type or `array{}`.

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

  Design: a loose-equality narrowing beside `apply_equality_narrowing`,
  used by `check_switch` (`stmt/switch.rs`) for every `case`, by
  `or_chain_narrowing_target`'s `BinOp::Eq` leaf
  (`narrowing/predicates.rs`), and by the plain `==`/`!=` narrowing if
  that shares the strict path too.
  `loose_equality_narrow(subject, other, branch) -> Option<Type>`
  answers only where PHP 8's rules pin the set down, and `None` (no
  narrowing) elsewhere:

  - Finite subject. For a union of literals (and `bool`), the true branch
    keeps each member whose `loosely_equals(other)` isn't `Some(false)`, and
    the false branch drops each member whose answer is `Some(true)`. With
    F40's null/string rule, this is exact.
  - `null` on the other side. `x == null` holds for `null`, `false`, `0`,
    `0.0`, `''` and `[]`, not for `'0'`. The true branch intersects the
    subject with that set: `?string == null` gives `''|null`. The false
    branch removes `null` and `false`, removes `0`/`0.0` from int and float
    ranges, turns `string` into `non-empty-string`, and removes `array{}`.
  - `true`/`false` on the other side is truthiness: `without_falsy` or the
    falsy part.
  - A number literal against an int or float subject is `===` on the
    number (`int|string`'s int part narrows to `1` for `== 1`).
  - A string subject against a number or a numeric-string literal narrows
    to `numeric-string` in the true branch: PHP 8 compares a numeric pair
    as numbers and anything else as strings, so only numeric strings can
    equal `1` (`'1'`, `' 1'`, `'01'`, `'1.0'`, `'1e0'` do, `'0x1'` and
    `'1abc'` don't, checked with `php`). Against a non-numeric string
    literal the comparison is byte equality, so it narrows to that
    literal, and the false branch removes it. Before PHP 8 a string
    compared with a number is converted to one (`'abc' == 0` and
    `'1abc' == 1` are true), so when the version range reaches below 8.0
    the string part isn't narrowed against a number, as F40's
    version-gated int/string pair already reasons.

  `switch` cases apply the true branch per `case` and the accumulated
  false branches to later cases and `default`, as they do now, only
  through this function. Fixtures: with `string $x`, `case 0:` gives
  `numeric-string` and `case 'a':` gives `'a'`; `case null:` on `?string`
  gives `''|null`; `case '1':` on `int|string` gives `1|numeric-string`;
  `$x == null || $x == 'a'` on `?string` narrows to `''|'a'|null`;
  `$x == 1 || $x == 2` on `int|string` gives `1|2|numeric-string`.
- [ ] **F68. A `break` inside an `if` in a `switch` case loses its path**
  (`check_switch`; also before the window). The frame pushed onto
  `loop_break_states` is popped and discarded, and only a case's last
  statement is read for a `break`: after `case 1: if ($c) { $a = 'one';
  break; } $a = 'two'; break; default: $a = 'd';`, `$a` is `'two'|'d'`.
  Done: the popped `break` states join the switch's exit states, with a
  fixture.

  Design: `check_switch` (`stmt/switch.rs`) already pushes a
  `loop_break_states` frame and a `loop_continue_states` frame, so that
  a `break` inside the switch is attributed to it. Every such `break`,
  nested in an `if` or not, records its scope into that frame. Then the
  frames are popped and dropped (line 121), and only a case's last
  statement is read for a `break` (`is_switch_break_equivalent`). The
  fix reads them:

  - The popped break frame's states join `exit_states`.
  - So does the popped continue frame's: a `continue` or `continue 1` aimed
    at the switch acts as `break`, which is why the frames stay
    depth-aligned.
  - The last-statement test for `break` goes, since a trailing `break;` is
    recorded in the frame like any other. A case's last statement still
    decides the fall-off-the-end exit of the last case and whether a case
    falls through to the next.

  The "no `default`" pre-switch state stays. Merging a state twice, once
  by fall-off and once by a `break` with the same scope, is idempotent.
  Fixtures: the finding's shape gives `$a` `'one'|'two'|'d'`; a `break`
  inside a nested `foreach` in a case doesn't leave the switch (it
  targets the loop's own frame); a `continue` directly in a case inside
  a `while` exits the switch with its state.
- [ ] **F69. A `catch` is judged unreachable when the `try` throws only
  through magic** (`check_try`, `checker/stmt/try_catch.rs`; also before
  the window). A clause is entered only from recorded throw points, and
  reading an `ArrayAccess` offset, converting an object to string,
  reading a property through `__get` and `include` record none: `$ok =
  true; try { $r = $bag['x']; } catch (RuntimeException $e) { $ok =
  false; }` leaves `$ok` `true`, as do `'a' . $s`, `$m->foo` and `include
  $f`. Done: each records a `Throwable` throw point (the offset read and
  the conversion only for an object with the method), with fixtures.

  Design: `thrown_by` (`expr.rs:364`) lists the expressions that throw.
  The code PHP runs implicitly is already recognized one file over:
  `noop.rs`'s `own_hidden_effects` and its helpers
  (`property_read_is_plain`, `offset_read_is_plain`,
  `may_be_stringable_object`) answer "does this run user code" for a
  property read, an offset read and a string conversion. Those helpers
  move to a shared `checker/implicit_calls.rs`, and `thrown_by` gains
  arms from them:

  - a `PropertyAccess` or `DynamicPropertyAccess` read that isn't plain
    (`__get`, or a `get` hook);
  - an `Index` read that isn't plain (`ArrayAccess::offsetGet`);
  - a string conversion of a possibly-object operand: `(string)`, `.`,
    interpolation, `print`, and `echo` (at the statement, through
    `record_throw_state`);
  - `include`/`require` and their `_once` forms: anything, since the file
    may throw;
  - the writes and probes that dispatch the same way: a property write
    through `__set` or a `set` hook, `isset`/`empty` on an `ArrayAccess`
    (`offsetExists`), `unset($o[k])` (`offsetUnset`), and `foreach` over a
    non-array `Traversable` (the `Iterator` methods).

  The thrown type is the magic method's own `@throws` when the
  receiver's class resolves one, through `call_throw_types`'s lookup for
  that method, and `Throwable` otherwise. F82's implicit-call forgetting
  reads the same shared helpers, so the two can't disagree about what
  runs code. Fixtures: the four shapes of the finding leave `$ok` `bool`
  after the `try`; a plain declared property read or array offset read
  keeps the catch dead; a `__get` with `@throws RuntimeException` makes
  a `catch (LogicException)` still dead.
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

  Design: the first bullet is F13's fix: general mode keeps observing
  keys, and the next index becomes a range. The rest is in
  `merge_shape_field` (`expr/array_ops.rs:283`), which both the spread
  literal and `array_merge` use, and in the spread path's list-ness.

  - An optional int field. `merge_shape_field` renumbers it to the next
    index as if present and advances the index by one, so every later
    renumbered key assumes it. Each optional int field doubles the possible
    layouts. The merge carries a small set of candidate shapes (at most 8,
    so three optional int fields) and forks the set at each optional int
    field, into a copy where it was present and a copy where it wasn't. The
    result is the union of the candidates. Past the cap it degrades to the
    general array as today. `[...$a, 'c']` with `array{0: 'a', 1?: 'b'}` is
    then `array{0: 'a', 1: 'b', 2: 'c'}|array{0: 'a', 1: 'c'}`.
  - An optional string field over an existing key. A later optional field
    may be absent, so the earlier value survives:
    - an existing required field stays required, with its type joined with
      the new one (`['x' => 1, ...$a]` with `x?: 2` gives `x: 1|2`);
    - an existing optional field stays optional, joined;
    - a required new field replaces as today.
  - List-ness and non-emptiness in `infer_spread_array_literal_type`. When
    the path degrades, the fields already merged into `shape` count too:
    `list_safe` is false if any merged key is a string, and
    `known_non_empty` is true if any merged field is required. Today only
    the sources after the degradation are consulted.
    `[...['a' => 1], ...$l]` is then `non-empty-array<'a'|int, 1|int>`, not
    `list<int>`.

  `array_merge`'s arm shares `merge_shape_field`, so it gets both field
  fixes. Fixtures: the three remaining shapes, each through a spread and
  through `array_merge`, and a four-optional-field merge that degrades.
- [ ] **F71. `preg_match_all`'s pattern-order groups never hold `''`**
  (`preg_match_all_pattern_order_shape`, `checker/expr/preg_shapes.rs`;
  also before the window). A group that doesn't take part in a match
  still gets a slot, `''` without `PREG_UNMATCHED_AS_NULL`, but the
  element type is the group's own: `preg_match_all('/(a)?b/', $s, $m)`
  types `$m[1]` `list<'a'>` (PHP gives `['', 'a']` for `'b ab'`), and
  `/(\d+)?b/` `list<numeric-string>`. The `PREG_SET_ORDER` result is
  `array<...>`, though it is a list. Done: an optional group's element
  adds `''`, and the set-order result is a list, with fixtures.

  Design:

  - Pattern order. In `preg_match_all_pattern_order_shape`
    (`expr/preg_shapes.rs:340`), `element_ty` adds `null` for an optional
    group only under `PREG_UNMATCHED_AS_NULL`, and nothing otherwise. PHP
    fills an unmatched group's slot in every match. With `php`,
    `preg_match_all('/(a)?b/', 'b ab', $m)` gives `$m[1] = ['', 'a']`, and
    with `PREG_OFFSET_CAPTURE` the unmatched slot is `['', -1]`. So an
    optional group's element joins its matched leaf with the unmatched leaf:
    `''`, or `null` under the flag, or under `PREG_OFFSET_CAPTURE` the pair
    `array{'', -1}` (`array{null, -1}` with both flags). `preg_leaf_type`
    gains that unmatched form so the two stay in one place.
  - Set order. The arm at `expr.rs:1061` builds
    `Type::array( unknown_array_key(), preg_match_matches_shape(…))`. The
    result is a list (`array_is_list` is `true`, checked), so it is
    `list<preg_match_matches_shape(…)>`. It is not non-empty, since zero
    matches give `[]`. Each element keeps `preg_match`'s shape, whose
    trailing unmatched groups are omitted, which is also what PHP does in
    set order (`$m[0]` for the `'b'` match is `['b']`, checked).

  Fixtures: `/(a)?b/` gives `$m[1]` `list<''|'a'>`; `/(\d+)?b/` gives
  `list<''|numeric-string>`; the offset-capture and unmatched-as-null
  variants; the set-order result is a `list` whose elements have group 1
  optional.
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

  Design:

  - The asserted parameter's argument. `assert_target_key_and_current`
    (`narrowing/assertions.rs:431`) finds the parameter's index and reads
    `args.get(idx)`, the positional slot. It resolves the arguments with
    `resolve_call_args(args, sig.params())`, which maps named arguments to
    their parameters, and reads the parameter's slot from that. A spread
    is expanded first, as F65's `positional_args` does for a sealed shape,
    and only a spread it can't expand, before or at the parameter's
    position, leaves no slot and no narrowing.
    F38's design runs the same resolution for its bindings, so it is
    computed once in `apply_assert_infos` and passed down. The
    `ThisSelf`/`ThisProp` targets are unaffected.
  - The not-found branch. `in_array_narrowing` (`assertions.rs`, near line
    77) builds the excluded set from every field of a constant haystack
    shape. An optional field's value may be absent, so it can't be ruled
    out. The found branch keeps every field's value, required or not. The
    excluded set is only the required fields' values, and `None` when that
    is empty. The literal-haystack path (`[...]` with no keys) has no
    optional fields and is unchanged.

  Fixtures: `checkA(b: $x, a: $y)` makes `$y` `int` and leaves `$x`
  `mixed`; `checkA(...$args)` narrows nothing; with `'a'|'b' $x` and
  `array{0: 'a', 1?: 'b'} $h`, `!in_array($x, $h, true)` makes `$x`
  `'b'`, and the found branch `'a'|'b'`.
- [ ] **F73. A `for` loop's in-range writes keep a list for a negative
  index** (`in_range_index_write_loop_vars`, `checker/stmt/loops.rs`;
  also before the window). `for ($i = -1; $i < count($x); $i++) { $x[$i]
  = 5; }` keeps `list<int> $x` a list, though key `-1` ends that. Done:
  the index must also be non-negative at the write, with a fixture.

  Design: `in_range_index_write_loop_vars` (`stmt/loops.rs:22`) matches
  the condition's shape (`$i < count($x)`), and the write site in
  `bind_assignment_target` (the `in_range` computation near
  `assignment.rs:425`) trusts it. The bound it proves is only the upper
  one. The write site also asks the index's type at the write, which is
  already narrowed by the condition and converged over the loop's
  passes: `in_range` holds only when that type is an int whose lower
  bound is at least 0 (`int_bounds().0 >= Some(0)`).
  `for ($i = -1; …; $i++)` converges to a range starting at `-1`, so the
  write drops list-ness. `for ($i = 0; …)` keeps it, as does any loop
  narrowed non-negative before the write
  (`if ($i >= 0) { $x[$i] = 5; }`). The index must be an int too, so a
  float index (truncated to a key) doesn't qualify. The syntactic
  matcher's doc gains the lower-bound condition. Fixtures: the finding's
  `-1` loop makes `$x` `array<int, int>`; a `0` loop keeps `list<int>`.
- [ ] **F74. A `config.platform.php` given as a full version is ignored**
  (`declared_php_versions`, `project/session.rs`; also before the
  window). Composer's `config.platform.php` is an exact version
  (`"8.1.12"`), and a release is kept only when its `.0` or `.99`
  satisfies the constraint, so none is: with `"8.1.12"` the run has no
  PHP version and `PHP_VERSION_ID >= 80200` goes unreported, where `"8.1"`
  reports it always false. Done: an exact version selects its release,
  and a release counts when any of its patches satisfies the constraint,
  with a fixture.

  Design: `declared_php_versions` (`project/session.rs:225`) samples
  each release at two patches, `.0` and `.99`, and keeps it when either
  satisfies the constraint. That misses every constraint that admits
  only patches in between. An exact `config.platform.php` such as
  `8.1.12` is the common case, and `>=8.1.5 <8.1.10` would miss too.
  `composer::constraint` already parses each condition into a version
  range (`range(min, max)`, `constraint.rs:62`). It gains
  `pub fn overlaps(constraint, lo, hi_exclusive) -> Option<bool>`, which
  is true when some `||` alternative's conditions intersect `[lo, hi)`.
  The alternative's comparison and wildcard conditions intersect as
  ranges. A `!=` only removes a point, so it can't empty a release's
  interval of patches. `declared_php_versions` asks
  `overlaps(constraint, "{v}.0", "{next minor}.0")` for each known
  release `v`, so a release counts when any patch satisfies, the rule
  its doc already states. The exact platform version is then just the
  one-point range, with no special case. Fixtures: `"8.1.12"` selects
  8.1 and `PHP_VERSION_ID >= 80200` reports always false, as `"8.1"`
  does; `">=8.1.5 <8.1.10"` selects 8.1; `"^8.1 != 8.1.3"` still selects
  8.1 through 8.x; unit tests for `overlaps` at the interval edges.
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

  Design:

  - Keys. `DuplicateFunctionSites::record` (`duplicate_fn_sites.rs:55`) and
    `DuplicateClassSites::record` (`duplicate_class_sites.rs:54`) key their
    maps by the lowercased FQN (`registry::name_table::Folded`), and each
    site keeps its declared spelling for the message. `remove_declarations`
    folds the names it is handed. PHP refuses `class foo` beside
    `class Foo`, and `function HELPER` beside `function helper`, so both are
    one name. The message names the first site's spelling.
  - Grouping. `check_duplicate_function_declarations`
    (`duplicate_functions.rs:40`) sorts by path and cuts a group at every
    signature change between neighbours, so equal signatures separated by a
    different one never meet. The signature doesn't decide a duplicate at
    all in project code: two unconditional declarations of one name are
    PHP's "Cannot redeclare" fatal whenever both files load, whatever
    their signatures. The signature exemption exists for phpstorm-stubs'
    version-split declarations (`strtr`'s two overloads), which describe
    alternatives, not code that runs. So stub files are not indexed as
    duplicate sites at all, as the class sites already pass `is_stub`,
    and every analysed name with two or more unconditional sites is one
    group listing all of them in path order. `sites_differ` and the
    signature fields on `FnDeclSite` go.

  Fixtures: `class Foo` and `class foo` in two files report
  `class.duplicate`, and `function helper`/`function HELPER` report too;
  `twice(int)` in `c.php` and `e.php` with `twice(string)` in `d.php`
  reports all three sites in one list; a stub file's overloads report
  nothing.
- [ ] **F76. A namespace cycle through a context file can go unreported**
  (`find_namespace_cycles`; also before the window). The cycle is
  reported at the alphabetically-first class contributing an edge, chosen
  among every registry class, and dropped when that class's file isn't
  analysed: `App\Foo extends Acme\Base` with `Acme\Plugin implements
  App\Contract` in a scan path reports nothing, and the same code with
  `Zed` for `Acme` reports `App, Zed`. Done: the representative is the
  first analysed class contributing an edge, with a fixture.

  Design: the representative is chosen before anything knows which files
  are analysed. `find_namespace_cycles`
  (`whole_program/circular_namespace_deps.rs`) keeps
  `edge_representative` as the alphabetically first class per
  namespace-pair edge, over every registry class, and the caller drops
  the diagnostic when that class's file isn't analysed. So the report
  depends on the alphabetical accident the finding shows, `Acme` against
  `Zed`.

  - The function takes `is_analysed: &dyn Fn(&str) -> bool`, from the
    caller's file set and the registry's class-to-file provenance.
  - Edges are built from every class, analysed or not, since a context
    class's `extends` is real structure.
  - `edge_representative` keeps the first analysed contributor per edge, in
    FQN order.
  - A cycle's representative is the minimum over its edges' analysed
    contributors. A cycle with none (every contributing class is in context
    files) isn't reported, since no analysed file can carry it, which is the
    only case where dropping stays right.
  - The `expect` on the minimum becomes that skip.

  Fixtures: `App\Foo extends Acme\Base` with
  `Acme\Plugin implements App\Contract` in a scan path reports
  `App, Acme` at `App\Foo`, the same as with `Zed`; a cycle entirely
  inside scan paths reports nothing.

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

  Design: `forget_members_a_call_may_change` (`expr.rs:190`) chooses
  what to forget by syntax: the receiver's narrowing key, and for
  `Anything` the argument keys. Facts are keyed by the expression that
  reached the object, while a call changes the object. So the choice
  moves from keys to objects:

  - May-alias by type. A property fact `$v->p` (or deeper) is stale after a
    call that may write objects of class `C` when `$v`'s type may be a `C`,
    meaning not disjoint by F15's `objects_disjoint`. This covers
    `$o = $this; $o->load()`, a second variable holding the object, and
    `$this->c` reached through a helper. `PropertyWrites` keeps its levels,
    and its `Receiver`/`Own` levels name the classes they may touch: the
    callee's declaring class, plus the classes of object arguments.
  - `Anything` forgets every `->` fact and every static-property fact. Its
    current receiver-and-arguments set is what makes the finding's second
    bullet (`$this->h->run()` writing `$this->c->x`) stay stale.
  - Static properties. `property_target_owner`
    (`registry/property_writes.rs`) classifies `self::$x = …`,
    `static::$x = …` and `Foo::$x = …` (and their compound forms) as writes
    of that class's static property. A method's `PropertyWrites` carries the
    static keys it writes, and a call forgets the matching `Foo::$x` facts,
    with `static::` resolved as `self::`, since the key is per class
    hierarchy.
  - Builtins that run object code. A builtin whose stub parameter is
    `mixed`, `object` or a union with either, given an argument that may be
    an object, may call that object's magic methods. Probed:
    `json_encode($j)` runs `jsonSerialize()`. By the stub rule the same
    holds for `var_dump`, `print_r`, `serialize`, `strval` and
    `sprintf('%s')`. Such a call forgets that object's facts by the alias
    rule. A builtin taking a `callable` runs user code, so it is `Anything`
    unless the callable resolves (F9's `pipe_callee` resolver) to a function
    whose `PropertyWrites` is known.
  - Calls without call syntax. The implicit-call helpers F69 shares
    (`__toString` for `echo`, `print`, `.`, interpolation and `(string)`;
    `__get`/`__set` for undeclared properties and hooks; `offsetGet` and its
    siblings for `ArrayAccess`; the `Iterator` methods for `foreach`) each
    forget like a method call on that object, with the magic method's
    `PropertyWrites` when it resolves, and `Anything` otherwise.

  F62's guards can go once these land. Fixtures: one per bullet of the
  finding, each ending with `$this->x === null` not reported, plus a
  negative one: a call on an unrelated final class keeps `$this->x`.
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

  Design: the override in `converge_loop_body` (`control_flow.rs`, the
  `LoopZeroIterations::Possible { subject_exempt, subject_key }` block)
  replaces every element's type with the exit entry for
  `$subject[$key]`. That stands for "every element ends as the body
  leaves the one its key names", which holds only if the body writes the
  subject nowhere else. `check_foreach` (`stmt/loops.rs`) sets
  `subject_key` only when the body passes a syntactic test,
  `writes_subject_only_at_key(body, subject, key)`. The test fails on:

  - a write target rooted at `$subject` whose first index isn't exactly the
    `$key` variable: `$items[$day + 1]`, `$items[] = …`, `$items['x']`;
  - `unset($subject[…])` at any other key;
  - the subject passed by reference: an argument slot the callee declares
    by-ref, or any argument to an unresolved call;
  - the subject reassigned whole;
  - `$key` itself reassigned, since then `$subject[$key]` no longer names
    the element being visited;
  - a nested function or closure capturing the subject by reference.

  On failure the loop falls back to the join the zero-iteration merge
  already does (`iterated`, with the entry's value type joined), which
  is the pre-`91666577` behaviour that F83 describes as sound but
  imprecise. The walk stops at nested function and class bodies, like
  the other scans. Fixtures: the finding's shape keeps `file?: 'x'|'y'`
  with `nb` optional; the plain `$items[$day]['file'] = 'y'` loop still
  types every element with `file`; a body that reassigns `$day` falls
  back.
- [ ] **F84. A template bound two siblings away still reads as `mixed`**
  (`build_template_params`; `2a31cc0c` substitutes one level). With
  `@template X of int`, `@template E of X`, `@template R of Holder<E>`
  and `Holder`'s `T of int`, `R` still reports `generics.notSubtype` on
  `@param` and `@return`: `E`'s substituted bound is `X`'s placeholder,
  whose bound is `mixed`. Done: siblings substitute to a fixed point
  (a cycle stopping at the placeholder), with a fixture.

  Design: `build_template_params` (`registry/function_sig/generics.rs`)
  substitutes each template's bound and default with the `built` map,
  whose entries still carry their own unsubstituted bounds. So `E of X`
  becomes `E of X(bound mixed)`, and `R of Holder<E>` sees an `E` whose
  bound's `X` is a bare placeholder. The substitution is resolved in
  dependency order:

  - Build the sibling graph from which templates each bound and default
    names.
  - Process the templates in a topological order. Each is substituted with
    the already-finished entries of the siblings it names, so `X` is
    finished before `E`, and `E` before `R`.
  - A template on a cycle (`@template A of B`, `@template B of A`) keeps the
    plain placeholder for the sibling that closes the cycle, which stops the
    recursion, as the finding's "a cycle stopping at the placeholder" asks.

  `tag_synthesis.rs`'s `@method` templates build the same way. The
  review noted they still use unbounded siblings, and nothing reads
  their bounds yet, so they move onto this function (or a shared helper)
  in the same change. Fixtures: `X of int`, `E of X`, `R of Holder<E>`
  with `Holder<T of int>` reports no `generics.notSubtype` on `@param`
  or `@return`; a two-template cycle parses without a hang or a report;
  a default naming a sibling (`@template B = A`) resolves through the
  same order.
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

  Design: with F48. The per-key entry loop in `bind_assignment_target`
  goes, so reads come from the shape `783727da` already fixes, and a key
  added to an unsealed shape takes the tail's value type too.

### Design

- [ ] **F20. Nothing enforces the stack guard** (`041fcc46`). It is a
  convention on about 200 hand-picked recursive functions, with no lint or
  test catching a new recursive walk without one. The types crate depends
  on `syntax` for `stack::guard` alone. Done: a test drives every
  `Visitor`/type traversal at depth, or a clippy-style check lists the
  recursive functions, and the guard lives in a crate both can use without
  the parser.

  Design:

  - Where the guard lives. `syntax::stack` (a `stacker::maybe_grow` wrapper,
    `syntax/src/stack.rs`) moves into its own crate, `stack`, which depends
    only on `stacker`. `syntax`, `types`, `phpdoc` and the main crate depend
    on it. `types` then drops its `syntax` dependency, if nothing else needs
    it; the crate-split plan's module graph (`gates/module-graph.txt`,
    regenerated by `xtask modgraph`) records the change.
  - Enforcement, two layers.
    - A lint, `xtask recursion`, in the style of `xtask arch`. It parses
      each crate's sources with `syn`, finds every function that calls
      itself, directly or through a `Visitor` or `walk_*` pair (a `visit_*`
      override calling `walk_*`, which calls `visit_*`), and fails when such
      a function's body doesn't start with `stack::guard(` or
      `crate::stack::guard(`. An allowlist file names the exceptions with a
      reason (Tarjan's recursion over namespaces, for instance, whose depth
      is the number of namespaces). CI and the gate run it.
    - A depth test in each crate's tests. It builds inputs 100,000 levels
      deep: a nested expression, a nested statement block, a nested type
      (`array<array<…>>`), a nested PHPDoc type, and a union chain. It runs
      every public traversal on them (`walk_stmts` with a no-op `Visitor`
      and `VisitorMut`, `Type::for_each_child`/`map_children`, `display`,
      `is_super_type_of`, `union`, interning, the parser and the PHPDoc
      parser) on a thread with a 256 KiB stack. An unguarded walk overflows
      there at once.

  The 195 current `guard` sites stay. The lint's first run lists the
  recursive functions without one, and each is guarded or allowlisted in
  the same change.
- [ ] **F21. The error type has no central rule** (`7ec336de`). Each
  "is this unknown" check has to learn `is_error()` by hand;
  `7ec336de` adds one more. Done: `is_error` folds into the one
  unknown-type predicate the checks already share, so a new check can't
  miss it.

  Design: make the error type a kind of `mixed`, so every "is this
  unknown" test sees it without learning it. Today `Repr::Error` is a
  variant of its own (`queries.rs:18`), and a check must list it by
  hand: two sites do (`global_constants.rs:254`, `operators.rs:223`) out
  of about 90 `is_mixed()` guards in the main crate. Those guards are
  exactly the "unknown, don't report" predicate a check shares, and each
  that lacks `is_error()` can report on an already-reported expression,
  the finding's risk.

  - `Repr::Mixed(bool, Option<Type>)` becomes
    `Repr::Mixed(MixedKind, Option<Type>)`, with `MixedKind` `Implicit`,
    `Explicit` or `Error`, and `Repr::Error` goes. `Type::error()` builds
    `Mixed(Error, None)`.
  - `is_mixed()` is true for all three, so every existing guard covers the
    error type. `is_error()` and `is_explicit_mixed()` read the kind. The
    relation code, whose `Mixed` arms accept and are accepted by everything,
    then gives the error type that behaviour with no arms of its own.
  - Display keeps `*ERROR*`. A union with an error member is the error type,
    as `mixed` absorbs: the error wins over the other kinds, since it
    records that a report was made. Equality, hashing and interning include
    the kind.
  - The places that must tell a real `mixed` from an error (the missing-type
    checks, `mixed`-specific messages, `assertType`) test `is_error()`
    explicitly. They are found by grepping for `Repr::Mixed(` and
    `is_explicit_mixed`, a short list, rather than by auditing every
    `is_mixed()`.

  `7ec336de`'s added `is_error()` becomes redundant and goes, as do the
  two hand-written ones above. Fixtures: an expression built on an
  undefined variable (`$undef->foo()->bar + 1`, `strlen($undef)`)
  reports only `variable.undefined`; `assertType('*ERROR*', …)` still
  matches.
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

  Design, one decision per bullet:

  - The closure `use` list. `walk_expr_in_scope` (`syntax/src/visit.rs:597`)
    returns without visiting anything of a closure. The `use (...)` list is
    read when the closure is created, in the enclosing scope, so the arm
    visits each `use` entry as a variable read (a new
    `Visitor::visit_closure_use`, defaulting to nothing, with the in-scope
    walks overriding it) and keeps skipping the body. By-reference uses
    count as a write too, since they create the variable. The
    unused-variable and definedness scans, which use `walk_expr_in_scope`,
    then see `function () use ($x)` as reading `$x`.
  - Union display order. Since `f81c5e70` a union compares as a set, but
    display prints members in construction order, so a message can depend on
    which evaluation built the type first. Display sorts members by a
    canonical key: PHPStan's `UnionTypeHelper::sortTypes` order (null last,
    then by kind, then by description). Messages and `assertType` are
    stable, and the fixtures' expected member orders are regenerated once.
  - Template scope. `build_template_params` gets a method's scope as
    `Class::method` (`class_info.rs:960` passes it) instead of the bare
    method name. `with_own_templates` compares with that, so a class named
    like the method no longer collides. It is the same edit as F2's.
  - `allowExceptParamFlags`. `param_flags_condition_matches`
    (`restricted_symbol_usage.rs:140`) requires every possible value to
    carry the flag, right for an allow gate and wrong for a disallow gate.
    It takes a mode: `Surely` for the allow gates (every value has the
    flag), and `Possibly` for the disallow gate (some value has it). The
    `allow_except_param_flags` sites pass `Possibly`. A non-constant
    argument's flags are unknown, so under `Possibly` it may carry the
    flag and the call is disallowed, with the message saying the flag
    "may be set". Under `Surely` it doesn't match, so an allow gate
    doesn't open for it. Both follow from what the argument can hold,
    not from the upstream extension's choice.
  - `ImpurePoints` and `new`. `resolve_new` (`purity.rs:563`) looks up
    `__construct` by exact case in the class's own methods and disqualifies
    a class without one. With F35's folded table the case goes. A class with
    no constructor anywhere in its ancestry runs no code on `new`, so it is
    pure. The constructor is looked up through ancestors
    (`lookup_method_with_owner`), and only an unknown class disqualifies.
  - The pipe's union of names: F9's `pipe_callee`.
  - Final sigma. `mb_strtolower`'s handling of a word-final `Σ` changed in
    PHP 8.3. The folding (`daa88e7c`) answers with the 8.3 result only when
    the configured minimum is 8.3 or later, and for a string holding `Σ` on
    a range reaching below 8.3 it answers the union of both results. A
    string without `Σ` stays certain on any version.

  Fixtures: one per bullet except the display order, which is covered by
  the regenerated expectations.
- [ ] **F23. Fixtures that lost what they tested.**
  - `15b5b494`: the arithmetic-operand fixture's string cases now see
    `int|float|string` (after `$s++`), so it no longer tests a string
    operand for post-decrement or `**=`. The `**=` report vanished for
    that reason.
  - `43758de2`: the default-value message shows the bound-resolved
    `array<(int|string), int>` rather than the declared `array<T>`.

  Done: the fixtures test what their names say.

  Design:

  - `strict_rules/arithmetic_operands__0.php` (and `__1`, its sibling
    config) run every case on shared parameters, so `$s++` on line 12 turns
    `$s` into `int|float|string` before `$s--` (line 13) and `$i **= $s`
    (line 16) read it. Each case gets its own fresh parameter: `string $s1`
    for `$s1++`, `string $s2` for `$s2--`, `string $s3` for and
    `$i **= $s3`. The expectations then read "post-decrement, string given",
    and the `**=` operand report comes back with `string` on the right side.
    They are regenerated and compared by hand with the pre-`15b5b494`
    expectations for those lines.
  - `43758de2`'s default-value message prints the bound-resolved type
    (`array<(int|string), int>`), because the check compares against the
    resolved type and prints what it compared. The comparison stays against
    the bound-resolved type, which is what makes the nested template
    checkable. The message prints the parameter's declared type
    (`param.ty()`, `array<T>`), which is what the reader wrote and what
    PHPStan prints. The call site in `function_decl.rs` passes both types.

  No code outside the one message changes. Both fixtures' expectations
  are updated in the same commit as the source they test.

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

  Design, one decision per bullet:

  - The infer cache (`2750baf2`/`1b7022ad`). The key gains a context epoch:
    `Checker` bumps a counter whenever the state `infer_expr` reads changes
    (the current class or function, the `$this` binding, the facts-only and
    member-reference flags), and the key includes it. The assumption stops
    depending on the "reset at each outermost `check_expr`" discipline. The
    cross-check the finding proposes is added too: under
    `cfg(debug_assertions)` and an environment switch
    (`PHPSTAN_RS_CHECK_INFER_CACHE=1`, off by default so debug test runs
    keep their speed), a hit recomputes and `debug_assert_eq!`s. The
    fixtures run once with it on in the gate.
  - The anonymous class name (`027811e4`).
    - The name embeds the path relative to the project root, which `Session`
      already knows. An LSP URI is made relative the same way. A message
      then doesn't depend on how the argument was spelled or on the LSP's
      absolute paths.
    - `anonymous_class_type`'s silent `mixed` fallback becomes a
      `debug_assert!`, and naming moves into the one parse entry point
      (`Session::load`/`read_and_parse_file`, and the LSP's reparse). No AST
      reaches the checker unnamed, so the fallback is unreachable.
  - The memo (`a25d77c6`, `intern.rs:436`). At `MEMO_CAPACITY`, `memoized`
    clears all 65,536 entries. It becomes two generations: a full current
    map is kept as the previous one, and the one before that is dropped. A
    lookup checks current, then previous, promoting a hit, so a hot working
    set survives the rollover. The interner's dead-weak buckets are swept
    incrementally: an insert that probes a bucket drops its dead entries,
    and every N-th insert sweeps one shard, so reclaiming no longer waits
    for the shard to double.
  - Lossy columns (`ba7ffe83`, `lexer.rs:189`). `relative_line_col_at`
    counts characters from the nearest mark. A lossy decoding counts an
    invalid sequence as one character only when read whole, so a mark inside
    one gives a different column than counting from the line start. A mark
    is inserted only at a boundary of the line's lossy decoding: the
    position is walked with `utf8_chunks` from the previous mark, which is
    always a boundary. A position strictly inside an invalid chunk gets the
    column of that chunk's replacement character and is not cached. A lexer
    test puts tokens after, between and inside invalid runs and compares
    every column with a count from the line start.

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

  Design: `Walker::record` (`lsp/navigation.rs:336`) takes a finished
  `Resolved`, so `record_var_read` (396), `instance` (357),
  `record_var_self` and `record_receiver_member` pay for `probe_scope()`
  (329) before the anchor test. That is a boxed clone of the
  `FunctionDecl`, or a clone of the whole top-level statement list.

  - `record(span, value: impl FnOnce(&Self) -> Resolved)`: the closure runs
    only when `self.anchor.matches(span)`. The four builders pass closures,
    and `record_class`'s existing early test becomes the same shape.
    `Anchor::At` then clones once per request.
  - `ProbeScope` holds an `Rc`: `Function(Rc<FunctionDecl>)` and
    `TopLevel(Rc<[Stmt]>)`. `Walker` keeps the current scope's `Rc`, made
    when it enters a function body (`visit_function_decl`,
    `visit_method_decl`, closures) and once for the top level. So under
    `Anchor::All` (`collect_all`) every entry in a scope shares one
    allocation, and `probe_scope()` is an `Rc::clone`. If the AST's
    `FunctionDecl` is already shared (`Arc` in the project's AST), the `Rc`
    wraps that handle instead of a deep clone.
  - `Resolved`'s consumers read the scope by reference, so only the
    construction sites change.

  Verification: a unit test counts `probe_scope` constructions for an
  `At` request on a file with many variable reads (one), and a generated
  20,000-line script's hover time is recorded before and after with the
  other scale timings (F18).

- [ ] **F77. A file that doesn't parse is skipped without a report**
  (`Session::load`; also before the window). `read_and_parse_file` gives
  `None` for a parse error and the file is only counted: `0 file(s)
  checked, 0 diagnostic(s) (2 file(s) skipped: parse error)`, with no
  location and exit status 0. Each commit in the window that makes more
  input a parse error (`474920e5`, `f4128dec`) turns a file that was
  checked into one skipped silently. Done: a parse error is a
  non-ignorable diagnostic at its span, and it fails the run.

  Design: `read_and_parse_file` (`project/session.rs:569`) turns three
  different failures into one `None`: an unreadable file, a
  `ParseError`, and a parser panic (`catch_unwind(...).ok()?`). `load`
  only counts them, and `check_project.rs:289` prints "N file(s)
  skipped: parse error".

  - The function returns `Result<(String, ParsedFile), LoadFailure>`, where
    `LoadFailure` is `Unreadable(io::Error)`, `Parse(ParseError)` (with its
    span and message) or `Panic(String)`.
  - For an analysed file, `load` records each failure as a diagnostic in
    that file's result: a parse error is `phpstanRs.parseError`, level 0,
    non-ignorable, at the error's span, with the parser's message; a panic
    is `phpstanRs.internalError` at the file's first line, naming it an
    internal error to report; an unreadable file is
    `phpstanRs.unreadableFile`. The file counts as checked, with those
    diagnostics, so the run's diagnostic count and exit status follow
    without new plumbing, and the "skipped" summary line goes.
  - A context (scan-path) file isn't analysed, but its declarations are
    missing from the registry, so every analysed file that uses them gets a
    wrong answer. Its failure is reported too, as a run-level diagnostic
    (`phpstanRs.contextParseError`, with the path, span and message) that
    the output formats print beside the file diagnostics and that fails the
    run, not as a summary line under `-v`.
  - The LSP reparse path publishes the same diagnostic for the open
    document, and a context file's failure for the workspace.

  Each commit that widens what is a parse error (`474920e5`, `f4128dec`,
  and F10/F57's) then shows up as a reported error. Fixtures: a file
  with any `php -l` failure reports `phpstanRs.parseError` at the
  offending token and the run exits non-zero; a broken scan-path file
  reports `phpstanRs.contextParseError`.
- [ ] **F78. The duplicate-declaration checks walk every name for every
  file** (`finish_diagnostics`, `project.rs`). With either opt-in on,
  each file's diagnostics loop over every recorded name in the project
  and keep the sites in that file: files times names. Done: the checks
  run once over the sites and hand each file its diagnostics.

  Design: `finish_diagnostics` (`project.rs:1474`) runs both checks for
  each file, and each check walks every recorded name to keep the sites
  in that file. The checks become one pass per run:
  `duplicate_declaration_diagnostics(&self) -> FxHashMap<PathBuf, Vec<Diagnostic>>`
  walks the site maps once, groups as F75 describes, and hands each
  group's diagnostics to the files its sites are in. `Project` caches
  the map behind a dirty flag that `index`/`remove_declarations`/`merge`
  on either site map set. `finish_diagnostics` reads its file's entry,
  recomputing the map first if it is dirty. So a batch run pays once,
  and an LSP edit pays once per edit rather than once per file per edit.
  The two opt-in gates stay where they are and decide whether the map is
  built at all. Fixture: the existing duplicate fixtures, unchanged. The
  scaling check is a generated project of 2,000 files and 20,000
  functions timed before and after, recorded with the other scale
  timings (F18).
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

  Design, per bullet:

  - Doctrine fields (`class_info/doctrine_entity.rs:199`).
    `compute_doctrine_entity` inserts every property into `fields` (line
    237) and only reads `#[ORM\Column]` for its name. Only a property
    carrying a mapping attribute (`Column`, `Id`, `ManyToOne`, `OneToOne`,
    `OneToMany`, `ManyToMany`, `Embedded`, or a `JoinColumn` with one of the
    association attributes) becomes a field, so a DQL path to a transient
    property reports as unknown. The name arguments are read positionally
    too: `string_arg` falls back to the first positional argument for
    `Column` (its `name` parameter is first) and for `Table`.
  - The OPcache probe (`composer_hygiene.rs`). The stub directory name goes
    through `composer_extension_name` (line 103), which maps `zend opcache`
    to `zend-opcache`. The probed name from `extension_loaded` (line 189) is
    only lowercased. The probe runs through the same function, so both sides
    use one normalization. Composer's rule is lowercase with spaces as `-`,
    so a general `replace(' ', "-")` replaces the one special case.
  - `yield from` (`control_flow/generators.rs:77`). Key and value use
    `rule_level_accepts`, and the send type uses `accepts` (line 167),
    strict at every level. The send check uses `rule_level_accepts` too,
    keeping its contravariant order (the inner generator's send type accepts
    the outer's), so all three follow the level's leniency.
  - `should_use_trait` (`project/architecture.rs:495`) compares against
    `traits`, the class's own `uses()`. The `uses_trait` selector (line 178)
    reads `all_ancestor_names`. A trait used by a parent is used by the
    child in PHP (its methods are there), so the rule reads the same
    ancestor set, and a class whose parent uses the trait satisfies
    `should_use_trait`.
  - The meta-attribute (`class_info/attributes.rs:50`).
    `compute_attribute_targets` takes any attribute whose last segment is
    `Attribute`. It compares the resolved name with the global `Attribute`
    (`resolved_class()`, no leading `\`), so a `#[App\Attribute]` doesn't
    define targets.
  - `mock.withWithoutExpects` (`project/mock_hygiene.rs`). The rule reads
    one statement's chain. The walker remembers, per function body, which
    variables were assigned a chain containing `->expects(…)`. A chain
    rooted at such a variable counts as having `expects`, and a reassignment
    of the variable forgets it.

  Fixtures: one per bullet, each with the finding's shape.

- [ ] **F86. `c0c51bca` landed with two fixtures failing.** Its
  `unseal_with` gives an open shape's tail its types, which changes the
  expected output of `an_open_shape_join_keeps_the_tail_type_for_a_one_sided_key`
  and `array_shape_writes_across_several_independent_branches_converge_instead_of_blowing_up`
  (both reach `array_merge`); `41e2198d` updated them two hours later.
  Done: nothing to change in the code; a fix commit's scoped test run
  includes the fixtures its function reaches.

  Design: nothing in the code. `41e2198d` fixed the two fixtures. The
  lesson is about scoped verification: a fix commit's scoped test run
  should include the fixtures that reach the function it changes. The
  fixtures are data files run by one test target (`--test fixtures`,
  which `cargo test --workspace` skips), so "the module's tests" never
  include them. The workflow change: a commit touching `types` or the
  checker runs `cargo test --test fixtures` filtered by the fixture
  directories whose names match the changed area (`array_merge`,
  `shapes`, …), or the whole fixture target when the change is in shared
  type code. `xtask` can offer that as `xtask fixtures-for <path>`,
  mapping a source file to the fixture directories that exercise it by
  the fixture names that mention its functions. Done is the workflow
  note in the contributor docs and the xtask helper.

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

  Design: complete `PassMark` (`checker.rs`) so a discarded pass leaves
  nothing, rather than reasoning per record about why a leftover is
  harmless.

  - `try_throw_states`. `record_throw_state` pushes into the innermost `try`
    frame. `PassMark` records that frame's length (one number per open
    frame, since a pass only pushes to frames open at the mark), and
    `rewind_to` truncates each.
  - `property_assign_facts`. It is a map, so `PassMark` records the keys a
    pass added or changed (a small undo log: key and prior value), and
    `rewind_to` restores them.
  - `named_argument_call_facts`. It becomes a length recorded in `PassMark`
    and truncated like the diagnostics, and the linear `contains` dedup
    added in `cc4e6b5e` goes.
  - Any other per-pass `Vec` found by listing the `Checker` fields a
    statement check pushes to joins `PassMark` the same way. A unit test
    asserts that checking a loop body twice with a rewind between leaves
    every such field as one check leaves it.

  `check_trait_methods` checks a trait's methods once per host. After
  the first host, it rewinds the LSP records (semantic tokens and inlay
  hints) the way it already deduplicates diagnostics, by taking a
  `PassMark` before each later host and rewinding those two fields after
  it. The trait's hover and highlight data then come from the first host
  only. Fixtures: an LSP test with a trait used by two classes gets one
  set of semantic tokens for the trait's methods; a `try` around a loop
  whose body throws keeps one throw state per distinct scope.

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

  Design, each checked still present at `3eaddbd1`:

  - `parser/tokens.rs:114-119`. `take_token`'s note becomes: "Sound because
    the parser reads tokens only at `pos` and `pos + k` for `k >= 0`, so a
    slot behind `pos` is never read again." The claim about the `elseif`
    backtrack goes, since it doesn't exist.
  - `parser.rs:26-27`. The field comment becomes: "Parallel to `tokens`
    (same length, same indices): `spans[i]` is `tokens[i]`'s span. Kept
    separate so the token accessors in `parser/tokens.rs` index one vector."
    The "there are many" claim goes.
  - The 29 `168a`-`168k` codes (listed by `grep -rn "168[a-k]"`). Each code
    is replaced by the component it names, from the commits that introduced
    them:
    - `168a`: the entity metadata (`registry::class_info::doctrine_entity`);
    - `168b`: the QueryBuilder chain resolver
      (`checker::doctrine::query_builder`);
    - `168c`: the DQL select-item parser (`checker::doctrine::dql`);
    - `168d`: the result-shape builder (`checker::doctrine::result_shape`);
    - `168e`: the `getQuery`/`getResult` wiring (`member_resolution`'s
      `doctrine_query_result`);
    - `168f`: the scalar hydration modes;
    - `168g`: `EntityManager::createQuery`;
    - `168h`: the custom DQL function config;
    - `168j`: entity inheritance mapping;
    - `168k`: scalar subqueries.

    Sentences that only narrate the history ("168b already resolved this")
    are rewritten to state the fact.
  - `union.rs:663-667`. The first two lines ("A shape as the general array
    it is…") move down to sit on `shape_as_general_array` (line 745),
    leaving `general_array_parts` with its own doc.
  - The `resource` message. This is not a comment but a display clash: a
    class named `resource` and the resource type print alike (the
    `a_native_resource_hint_names_a_class` fixture expects "expects
    resource, resource given"). Display prints an object type whose class
    name, case-folded, equals a type keyword (`resource`, and any other
    non-reserved keyword name PHP allows as a class) with a leading `\`.
    That fixture then reads "expects \resource, resource given".

  Verification: `grep -rn "168[a-k]" crates` is empty afterwards, and
  the fixture expectation is updated in the same commit as the display
  change.

- [ ] **F34. Comments from commits 100 to 155:**
  - `coercion.rs` `to_number`'s doc says a non-numeric literal like
    `'5 apples'` throws on PHP 8. PHP 8.5 gives `"5 apples" + 1` as `6`
    with a warning; only a wholly non-numeric `'abc'` throws. Reporting
    the leading-numeric case may stay as a choice; the doc's claim goes.

  Done: the doc says what PHP does.

  Design: the doc and the code share the claim. `to_number`
  (`types/src/coercion.rs:78`) returns `None` for any string literal
  that isn't wholly numeric when `non_numeric_throws`, and the doc names
  `'5 apples'` beside `'abc'`. Checked with `php` 8.5: `'abc' + 1`
  throws `TypeError` ("Unsupported operand types: string + int"), while
  `'5 apples' + 1` is `6` and `'1.5x' * 2` is `3.0`, each with the
  warning "A non-numeric value encountered". So a leading-numeric
  literal is a number with a warning, not an error. The fix goes beyond
  the doc, since `None` also makes the operation's type the error type:

  - `to_number` gives a leading-numeric literal its leading value
    (`'5 apples'` is `5`, `'1.5x'` is `1.5`, through `types::numeric`'s
    parse of the longest numeric prefix). `None` stays for a string with no
    numeric prefix at all.
  - The doc says just that: "`None` for an array, an object `to_float`
    refuses, or, when `non_numeric_throws` (PHP 8), a string with no leading
    number (`'abc'`), which throws `TypeError`; a leading-numeric string
    (`'5 apples'`) is its leading number, with a warning at runtime."
  - The warning is still worth a report, but not as `binaryOp.invalid`
    ("results in an error"). `check_binary_operand_validity` adds a separate
    level-2 report, `binaryOp.nonNumericValue`, with PHP's warning text,
    when an operand may be a leading-numeric, non-numeric string.

  Fixtures: `'5 apples' + 1` is typed `6`, reports
  `binaryOp.nonNumericValue` and not `binaryOp.invalid`; `'abc' + 1`
  still reports `binaryOp.invalid`.

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

  Design, each checked present at `3eaddbd1`. Every replacement states
  what the code does, in the file's register, without history.

  - `union.rs:174-197`, the empty-shape paragraph above `merge_int_ranges`.
    It moves onto `reduce_array_members`, which does the absorption, as "An
    empty `array{}` goes into any general array member: it is a subtype of
    every array type." The loop example and the `index_result_type` sentence
    go.
  - `union.rs:66-80`, the `mixed` absorption. It becomes "A `mixed` member
    of a flattened operand absorbs the union, as a bare `mixed` operand does
    above." The history and the `nullsafe.neverNull` story go.
  - `merge_has_offset_value_pair` (`union.rs:514`). The doc says what the
    merge keeps: a key tracked on both sides with its values joined, and a
    key tracked on one side dropped back to the base. It adds that this
    keeps a variable written in independent branches from growing a union
    member per branch. The `ActivityService` case, "observed behavior
    exactly", and the stale step name go. `write_literal_key_aware` becomes
    `types::offsets`' writer (F43's `lib.rs` bullet is the same rename).
  - `narrowing.rs`, `narrow_to_instance`. The fused first paragraph is
    `narrow`'s old doc and goes. `narrow`'s own doc (line 126) already
    exists. The remaining text loses "unconditionally trusts `target`",
    since `narrow` keeps a subtype `original`.
  - `narrowing.rs`, `narrow`'s fallback comment. The Piwigo case becomes one
    sentence: "An `original` already within `target` stays; `===` against a
    wider value proves nothing new about it." F4 rewrites this arm anyway,
    so the comment is written with F4's code.
  - `narrowing.rs`, `without`'s arms (lines 237, 257, 287). "Confirmed via
    `php -r`", "no case this could ever get wrong" and "doesn't attempt yet"
    become plain statements of each arm's rule. The PHP fact stays as a fact
    ("implementing both is a fatal error"), not as a report of having
    checked it.
  - `narrowing.rs`, `without_falsy`'s last arm: "the old hand-rolled arm"
    goes, and the comment says what the arm removes.
  - `relation/accepts.rs`, the generic-arguments arm (line 110 on).
    - The claim that `ClassReflection` keeps no class-level defaults is
      false, and the code built on it should change with the comment. A
      missing slot is padded with the class's declared default when it has
      one (`template_defaults`, the hierarchy trait gaining a
      `template_default(class, index)` query), and with `mixed` otherwise.
      The comment then says that.
    - The "no canonical member order" sentence (line 210) goes, since unions
      compare as sets since `f81c5e70`.
    - The "only ever binds a template used as a bare param type" sentence
      (line 179) goes, since since `b274f947` every position binds.
  - `accepts.rs:328`, the `Array(Mixed)` leniency. It becomes "An
    `array<mixed>` value may hold anything, and is accepted where a narrower
    array is declared unless `strict_mixed_arrays` is set." The claim that
    literals are never inferred precisely goes.
  - `accepts.rs`, the `Intersection`/`Intersection` arm. "Mirrors" an arm
    `69c76cef` removed, so it goes, and the comment states the arm's own
    rule.
  - `accepts.rs:447`, `class == "stdClass"`. Class names fold, so it
    compares with `eq_ignore_ascii_case("stdClass")`. A one-line code fix,
    with a fixture passing `new STDCLASS()` where `object{a: int}` is
    expected.

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

  Design:

  - `lib.rs:224`, `Repr::HasOffsetValue`. The construction site is
    `write_literal_key_aware` in `types/src/offsets.rs` (line 428, called
    from `set_offset_value_type`), not `checker::assignment`. The comment
    names it there. The "Confirmed directly against the vendored real
    `phpstan.phar`" narration becomes a statement of the rule: a literal
    write onto a general array adds a member, and a second literal write
    adds another. The same rename applies at `builtin_calls.rs:937` and in
    `union.rs:476`/`482`/`514`, found by `grep -rn write_literal_key_aware`.
    Every mention outside `offsets.rs` then names the module.
  - `lib.rs:204`, `Repr::IntRange`. "A `T` range" becomes "An int range".
  - `display.rs:152-160`, the `HasOffsetValue` arm's comment. It says no
    fast path of `describe_accessory_intersection` matches an intersection
    holding one. After F42's fix that is true, and the comment says it as
    the reason this arm is reached: the fast path takes only list and
    non-empty accessories. F42 and F43 land together.

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

  Design, each checked present at `3eaddbd1`:

  - `control_flow.rs:458-476`. `merge_scopes`' doc ("Joins two branch-local
    scopes…") and its `#[hotpath::measure]` move from above
    `add_implied_fact` (line 476) to above `merge_scopes` (line 563), and
    `add_implied_fact` gets a one-line doc of its own. This is a behaviour
    fix for profiling too: under `--features hotpath` the merge's time is
    currently measured as `add_implied_fact`'s.
  - `stmt.rs:1303-1340`. `statement_defined_variable_names`' doc moves from
    above `bare_assert_condition` (line 1321) to above its function (line
    1340). Its "Deliberately doesn't cover `foreach`" sentence goes, since
    the function has a `Foreach` arm. The post-dispatch comment in
    `check_stmt_impl` ("Deliberately does NOT extend this to a tag sitting
    directly on a `foreach` header") is rewritten to say that
    `check_foreach` takes the statement's inline `@var`s.
  - `member_resolution.rs`. `method_call_return_type`'s doc moves from above
    `class_argument_return` (line 408) to its own function (line 445). The
    `168e` citation at line 715 names the convention instead: the
    `getQuery`/`getResult` synthesized generic slot, per F24's mapping.
  - `builtin_calls.rs`:
    - line 159, "`array_pop`/`array_shift` are deliberately NOT
      special-cased", goes, since an `array_pop | array_shift` arm follows;
    - `get_defined_vars`' comment (line 683) moves from above the
      `mysqli_query` arm to its own arm (line 706);
    - `array_merge`'s comments name `types::offsets`' writer (F43) and lose
      "closing the gap this arm used to leave" (line 1019) and "Loosened
      from an exact `string` match" (line 961), each becoming the rule it
      states;
    - `sscanf_result_fields` (line 3135) says its fields are keyed by ints
      (`ShapeKey::Int`), as built. F49's lexer rewrite touches this function
      anyway.
  - `expr.rs`:
    - the `Index` arm (line 2317) says it calls `offset_value_type`;
    - `widen_coalesce_result`'s doc (line 5188) describes the general case
      as the code has it: the left side's non-null type joined with `r`, `r`
      alone when the left is `null` or `r` never returns, and the array
      exception. The "usually just `r`" and "not a blanket union" claims go;
    - the "A synthetic `target::method(args)` call" doc moves from above
      `unhandled_match_error` (line 5945) to `static_call` (line 5960), and
      `unhandled_match_error` gets its own;
    - the first-class-callable arm (line 4159) loses "Closes a real, live
      bypass … previously went unchecked" and states the check.

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

  Design, each checked present at `3eaddbd1`:

  - `project.rs`:
    - `update_file`'s doc ("Re-parses one already-loaded file…", line 787)
      moves from above `unindex_file` (line 800) to `update_file` (line
      928), and `unindex_file` gets its own one-line doc;
    - the plan references ("VSCode plan item 2.2" at 936, "(3.2)" at 1152,
      "this item's own open-risks note" at 876 and 1432, "1.1's own
      `phpstan-rs.toml` `paths`") become what they point at: the debounce
      worker, workspace folders, and the concurrency or ordering risk
      itself, stated in a sentence;
    - the narration ("`unindex_file` used to run first unconditionally" at
      954, "a real bug on an earlier pass of this exact method" at 1652)
      becomes the invariant it protects.
  - `lsp/navigation.rs:11`. The module doc names
    `Server::document_highlight` directly and drops the self-correction.
  - `project/use_sites.rs:230`. This is a comment and a bug. The arm records
    a bare `Name` expression as a function reference, but a first-class
    callable `foo(...)` is a `Call` (handled above), so a bare `Name` here
    is a constant (`FOO`, `PHP_EOL`). Constant reads are indexed as
    references to a function of that name, so LSP references on a function
    list its same-named constants' reads, and dead code counts them. The arm
    records a new `RefTarget::Constant`, keyed case-sensitively as PHP
    constants are, with an unqualified name in a namespace recorded under
    both candidates and resolved at query time the way F7 resolves
    functions. `define('FOO', …)` and `const FOO` declarations are the
    targets, and a literal argument to `constant()` or `defined()` is
    a reference too. LSP references and rename then work on global
    constants.
    Dead code gains `constant.unused` for a project `const`/`define` with
    no reference, beside `function.unused`. The comment says a bare name
    is a constant. Probed: in
    `namespace App;` with `const FOO`, `function FOO()` and
    `function bar()`, and only `echo FOO;`, dead code reports `bar` and not
    `FOO`. That is the fixture, with both reported after the fix.
  - `registry/purity.rs`: F56's rewrite of the "Any static property access
    at all" label.
  - `syntax/ast.rs`:
    - `PropertyDecl::hooks` (line 333): hook bodies are type-checked by
      `checker/oop/property_hooks.rs`;
    - `FunctionDecl::body` (line 260): bodies are found by provenance; the
      `body` field claim and the `src/intern.rs` path go;
    - the `PropertyAccess` comment (line 1426) describes `ObjectOperator`,
      not a trailing `bool`;
    - `StaticCall` (line 1462) drops the `"?"` sentinel history;
    - `LogicalAnd` (line 744) drops "the way this comment used to argue
      for";
    - `UseImport::kind` (line 427): the registry does keep global constants
      (`register_global_constant`), so the sentence is replaced with what
      `kind` distinguishes;
    - `Stmt::doc_comment` (line 54) cites `inline_var_annotations`.
  - `syntax/parser`. `functions.rs:181` names `parse_class_body` alone,
    since an enum body goes through it. `enums.rs:92` cites
    `ClassDef::attrs`.

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

  Design, each checked present at `3eaddbd1`:

  - `checker/expr/array_ops.rs`. The paragraph on `array_element_type` for
    literal shapes that heads `check_array_item_key_type`'s doc moves to
    `array_element_type`, or goes if that function's own doc already says
    it. The spread comment (line 185) loses "the original check" and
    "confirmed via `dumpType()`", and states the list-safety rule.
  - `checker/expr/ranges.rs:55`. `null_probe_verdict`'s doc says it serves a
    bare variable and the offset and property chains of
    `resolve_always_exists`, which call it through
    `check_pointless_offset_or_property_probe`. The "deeper chain is out of
    scope" clause goes.
  - `checker/control_flow/generators.rs:103`. This is a message, not a
    comment: `generator.void`'s text "it's return type is void" becomes "its
    return type is void". The fixtures expecting the old text are updated
    with it.
  - `registry/class_info/doctrine_entity.rs`. The module doc's Piwigo and
    "the real corpus this phase targets" sentences (line 8) go, and it
    states what is mapped (attributes; see F79 for which properties).
    `DoctrineEntity`'s doc says the discriminator map is modeled
    (`discriminator_map`, line 94), matching the module doc.
  - `project/duplicate_fn_sites.rs:25-27`. The sites are always indexed, and
    the check that reads them is opt-in
    (`check_duplicate_function_declarations`). The doc says both, and drops
    "always-on". `duplicate_functions.rs:22` either gives the `strtr` reason
    it points at (the stubs declare version-split signatures of one
    function) or drops the pointer. `duplicate_classes.rs` and
    `duplicate_class_sites.rs` state their rules without the corpus
    argument.
  - `project/whole_program/dead_members.rs`. The module doc's Piwigo
    measurement and past fixes (lines 17, 46, 59, 75: "used to have", "used
    to be a real gap here too", "Fixed at the parser level") go. The
    remaining text lists what counts as a reference and what is a known gap.
  - `project/mock_hygiene.rs:385`. "The members of a PHPDoc union as
    written…" moves down to `union_members` (line 408), leaving
    `native_members` its own doc.
  - `project/session.rs:219-225`. `composer_roots`' doc ("…per directory,
    falling back to `root`") moves from above `declared_php_versions` to
    `composer_roots` (line 236). F74 rewrites `declared_php_versions`' body,
    and its doc stays.
  - `syntax/parser/tokens.rs`. `take_name_segment`'s example (line 235) uses
    a neutral `App\Enum\Foo`. `parse_leading_attributes` (line 392) drops
    "before this phase" and says what is parsed.
  - `syntax/visit.rs:10`. The module doc says the shared walk visits type
    hints and names, and that `VisitorMut`'s defaults don't visit type hints
    or names (its `visit_type_hint` is a no-op), so a mutating visitor that
    needs them walks them itself.

- [ ] **F88. Comments that still describe the truncation `cc4e6b5e`
  replaced:** `converge_loop_body`'s doc calls discarding a pass "the
  existing `diagnostics.truncate` pattern", and `check_while`'s comment
  says every pass but the kept one "has its diagnostics discarded via
  `converge_loop_body`'s own `diagnostics.truncate`"; both now go
  through `rewind_to`, which drops more than diagnostics. Done: both
  name `rewind_to`.

  Design: `converge_loop_body`'s doc (`control_flow.rs`, near line 280)
  says a discarded pass "is rolled back with `rewind_to` to the pass's
  `PassMark`: its diagnostics, throw and return points, semantic tokens
  and inlay hints". `check_while`'s comment (`stmt/loops.rs`, near line
  73) says every pass but the kept one is rolled back the same way. If
  F87 lands first, both name the records it adds, or say "every per-pass
  record" and point at `PassMark`'s doc as the one list.

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
