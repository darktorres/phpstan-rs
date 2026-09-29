# Stage B fix review: findings

Findings from reviewing the work done on the Stage B review
(`STAGE_B_REVIEW.md`): every commit from its F23 fix (`041fcc46`) to
`31d6c5a7` and the three after it (`d4ae7678`, `5400b17b`, `96027afd`),
and every item it marks done. Judged on runtime correctness and design,
not parity with PHPStan. Each task names where the problem is,
what is wrong, and what done looks like.

The review reads every commit's diff in order, checks each done item's
claim against the code, and reads the modules the fixes reshaped in their
final state; its coverage is tracked at the end, and its findings are the F
tasks.

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

- [ ] **F1. A template bound in a namespace resolves twice.** Since
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
- [ ] **F2. A first-class callable on a generic receiver leaks the class's
  templates** (`b3412c22`). `first_class_callable_type` builds the closure
  from the method's declared signature and resolves only `self`/`static`,
  so for `Box<int> $b`, `$b->get(...)` is `Closure(): T` and `$g()` is `T`.
  `h($g())` with `h(int)` reports "expects int, T given". The pre-window
  binary gave a plain `Closure`, so there was no report. Done: the
  receiver's class template bindings are substituted into the signature
  (as a method call's return is) before `with_own_templates`, for static
  calls too.
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
- [ ] **F6. A rebound closure's `$this` is blessed as undefined**
  (`fda237d8`). The commit's own fixture expects `variable.undefined
  $this` (level 0) inside the two closures it rebinds with `->call(new
  Foo)` and `Closure::bind`. PHP runs both with `$this` bound. The rebind
  scan feeds only unused-member counting. Done: a closure rebound to an
  object has `$this` of that object's type (or `object`), and the fixture
  expects no `variable.undefined`.
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
    leaves `$b` a bare `array`.

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

## The Stage B review's done items

Checked against the commits each item names, the diffs read above, and
probes. A `[ ]` marks an item whose claim doesn't hold in full; the
finding names what is missing. V1 wasn't re-run here, since this review
builds nothing.

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
- [ ] **F53** (`408caaf4`). 29 roadmap sub-phase codes remain (F24).

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

### Final state, module by module

Each module the fixes created or reshaped, read in its final state after
the commit pass.
