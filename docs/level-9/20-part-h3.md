## H3. REG-7 and REG-8 on the engine (Part F 14)

<!-- status:begin -->

## Status of this part's tasks

From the ledger at `54abfd547` on 2026-10-19. Regenerate with `scripts/plan_status.py`; change the ledger, not this table. `also in` lists the other parts that name the task.

| task | status | ledger | also in |
|---|---|---|---|
| PROD-5.1 | OPEN | OPEN | 05 07 10 11 12 13 18 22 |
| REG-1 | DONE | DONE `c0dd2788` | 06 19 |
| REG-4.2 | READY | READY; a delta, approved in the plan, fixtured | 05 06 10 18 |
| REG-4.11 | OPEN | OPEN; the comments can be corrected now | 06 10 12 18 22 |
| REG-4.12 | READY | READY | 06 08 10 11 12 18 19 |
| REG-4.15 | OPEN | OPEN | 05 06 07 10 11 12 18 22 |
| REG-7 | OPEN | OPEN | 05 06 08 12 18 19 21 23 |
| REG-7.1 | OPEN | OPEN | 06 10 23 |
| REG-7.2 | OPEN | OPEN | 04 05 06 10 11 |
| REG-7.3 | OPEN | OPEN | 06 10 23 |
| REG-7.4 | OPEN | OPEN | 05 06 10 23 |
| REG-7.5 | OPEN | OPEN | 01 02 04 05 06 07 10 12 22 |
| REG-7.6 | OPEN | OPEN; a delta, approved (VAL-2.5) | 06 10 |
| REG-7.7 | OPEN | OPEN | 06 10 |
| REG-7.8 | OPEN | OPEN | 06 10 |
| REG-7.9 | OPEN | OPEN | 06 08 10 19 |
| REG-7.10 | OPEN | OPEN | 06 10 19 |
| REG-7.11 | OPEN | OPEN; it lands with the snapshots | 06 10 |
| REG-7.12 | OPEN | OPEN | 06 10 19 23 |
| REG-8 | OPEN | OPEN | 06 08 18 22 |
| REG-8.1 | OPEN | OPEN; a delta: no declaration's members merge into another's, and the server and the batch run bind a name alike … | 06 10 |
| REG-8.2 | SUPERSEDED | SUPERSEDED by REG-16.3 (decision 3): the read sets are the dependencies the engine records, absent names included | 06 10 |
| REG-8.3 | OPEN | OPEN | 02 05 06 10 |
| REG-8.4 | OPEN | OPEN | 06 10 |
| REG-8.5 | OPEN | OPEN; a delta, approved and corrected (decision 14; nsrt `property-hooks.php`) | 06 10 |
| REG-8.6 | OPEN | OPEN; overlaps NAME-1 and NAME-2 | 05 06 07 08 10 12 18 22 |
| REG-8.7 | OPEN | OPEN | 06 10 |
| REG-9 | OPEN | OPEN; deltas, approved in the plan | 06 |
| REG-10 | OPEN | OPEN | 06 |
| REG-11 | OPEN | OPEN; each a delta, approved in the plan | 06 10 |
| REG-12 | OPEN | OPEN | 06 10 19 22 |
| REG-16.4 | OPEN | OPEN | 02 04 05 06 07 08 10 11 12 13 21 22 23 31 |
| REG-16.5 | OPEN | OPEN | 06 10 12 21 22 |
| REG-17 | OPEN | OPEN | 05 06 08 10 12 19 21 |
| REG-17.1 | OPEN | OPEN | 10 23 |
| REG-17.2 | OPEN | OPEN | 10 11 24 |
| REG-17.3 | OPEN | OPEN | 10 |
| REG-17.4 | OPEN | OPEN | 10 |
| REG-18.2 | READY | READY | 06 10 |
| REG-18.3 | READY | READY | 06 10 23 |
| REG-18.6 | READY | READY | 03 06 10 11 12 |
| VAL-2.9 | DONE | DONE: approved (decision 14), STUB-7.2 and REG-8.5 corrected; each is read at VAL-1 as it lands | 06 07 12 |
| VAL-2.10 | OPEN | OPEN: read at VAL-1, as they land | 05 06 07 12 15 |

<!-- status:end -->

### H3.1 The five levels as queries

Each level is a tracked function keyed by `DeclKey`, per declaration (decision 11: no merge). A level reads the
declarations, a lower level of any class, and its own level only along an ancestor edge; that rule is what
keeps the queries acyclic.

| level | query | result | reads |
|---|---|---|---|
| (declaration) | `class_decl` | `Arc<ClassDecl>`, the entry of `file_decls(file)`: parents as written, `use`d traits, `@mixin` and `@method` tags, members as declared, doc types as written names and `TypeNode`s, templates, the file's `ImportScope` | `file_decls(file)` (a change anywhere in the file re-runs it; it is a projection and backdates) |
| 1 | `class_ancestors` | the `extends`/`implements` closure in resolution order, the `use` closure in composition order, `@mixin` targets, unresolved edges (a name no declaration answers to, with the class declaring it), the folded ancestor set that `is_subclass_of` reads, the constants composed with the traits' | `class_decl`; `declarations_named` of each parent; `class_ancestors` of each candidate parent; `cycle_edges_of` |
| 2 | `class_own` | own members composed with the `use` closure's declared members, `@method` tags, every type resolved in the class's context (imported aliases, `value-of`, class-constant types, constant-named types), the interim assertion step | `class_decl`, `class_ancestors`, the traits' `class_decl`, and the exporters' `class_decl` and the target's `class_ancestors` for an imported alias; `class_constant_value` and, for a constant-named type, `global_constant` and `declarations_named` (`constant_named_value` reads a constant and a class's existence) |
| 3 | `class_inherited` | PHPDoc inheritance and property hooks | `class_own`, the ancestors' `class_inherited` |
| 4 | `class_members` | each `@mixin` target's public members, added where the class has none of that name; then the member owners | `class_inherited`, the mixin targets' `class_inherited` |
| 5 | `class_final` | decision 8's combination of native and PHPDoc types, the relation reading other classes' hierarchy and members | `class_members`, other classes' `class_ancestors` and `class_members` through the relation |

A level stores what it changes and shares the rest with the level below (member maps behind `Arc`), so the
five memos cost about the final reflection plus deltas (H2.9 measures). `function_final` is REG-7.12's resolver
(`finalize_standalone_sig` today); `global_constant` is the constant case.

**Where the logic lives.** The crate split's L3 `reflection` holds the resolution logic and L4 `analyser` the
checker, and both sit below the database. So the levels are written as **pure functions over a reader trait**
(`reflection::levels::own(reader: &dyn LevelReader, decl: &ClassDecl) -> OwnLevel`, with
`LevelReader::{declarations_named, class_decl, ancestors, cycle_edges_of, own, inherited, members,
constant_value, global_constant}`; its keys are reflection-level declaration references that the engine
interns, so no engine id crosses the trait), and the engine module wraps each as a tracked function whose
reader calls the other tracked functions, so every call records its read. Tests of `reflection` use a reader
over plain maps, with no database; the checker reads through the checker-facing trait of H2.4, which `Provider`
implements. This is what keeps salsa in one module, `engine`, which sits between `analyser` and `project` in the
crate split's layers (L6, the first half of `project`'s role: the result cache and the dependency graph are what
it is), and what keeps the contract of H2.1 implementable by another engine.

### H3.2 The finishing passes, by level

| today (`run_finishing_passes`, `project.rs:1533`) | becomes |
|---|---|
| `canonicalize_class_name_case` | deleted (REG-4.2: folded keys) |
| `flatten_traits`, `resolve_method_tags`, `apply_assertion_table` (interim until C6) | level 2 |
| `resolve_deferred_type_imports`, `resolve_deferred_value_of`, `resolve_constant_named_types` | level 2, on demand, so `class_has_deferred_type`, `function_has_deferred_type` and the eager pass go (REG-7.2) |
| `flatten_phpdoc_inheritance`, `inherit_property_hooks` | level 3 |
| `resolve_deferred_mixins`, `precompute_member_owners` (and `_after_mixins`) | level 4 |
| `precompute_ancestor_names` | level 1 |
| `drop_doc_members_excluded_by_native` | level 5 |
| `reset_demand`, `changed_since`, `walk_affected`, `closure_scope`, `finishing_scopes`, `Scope` | deleted: recorded reads and cutoff |
| `infer_global_variables_of` | `global_sites(File)`, `global_scope_files`, `global_site_index` and `global_variable(GlobalKey)` (H3.7) |
| `rebuild_pest_config` | an ordinary query over the Pest files |

### H3.3 Cycles: three, each with its own rule

1. **Hierarchy (REG-7.1).** One query, `hierarchy_cycles`, runs Tarjan's algorithm over the declared
   `extends`, `implements` and `use` edges of the project's and context's declarations (candidates expanded; the
   builtin corpus is acyclic and left out), over a canonical node order, and returns the edges between two members
   of one component, a self-edge included. `cycle_edges_of(DeclKey)` slices it per declaration, so a cycle
   appearing or going away re-runs only the classes whose own slice changed; `class_ancestors` drops those edges.
   "The same whichever class resolves first" holds by construction, since no class resolves before the pass. No
   salsa cycle is ever entered, so the default strategy (panic) stays for these queries and a hierarchy cycle that
   reached it would be the bug REG-7.9 says it is. Cost: one pass over the declared edges of the project's and
   context's classes (12,546 classes in the 20 G profile's load), run again when declarations change; S-6
   measures it.
2. **Constants (REG-7.6, decision 1).** `class_constant_value` and `global_constant` use salsa's
   `cycle_result` (`FallbackImmediate`): every query on the cycle gets the fallback, which is the rule, "every read
   is `never`, and the throw is its throw point". Its fallback function receives the query's own id
   (`tests/cycle_fallback_immediate.rs`), so the PHPDoc case, a constant whose PHPDoc reaches a cycle, can fall
   back per constant to *its type without that PHPDoc*, and the PHPDoc is reported unresolvable. **The
   value-only query is `class_constant_value_only`** (H2.3: key (`DeclKey`, name), `cycle_result` = `never`
   plus the definite throw, and a flag in its result for "PHPDoc dropped"; no PHPDoc edge enters it). The
   fallback is: ask it; if it is itself on a value cycle the answer is `never` plus the definite throw, else its
   type and the flag. salsa's tests show a query can be called from a cycle initial function
   (`tests/cycle_initial_call_query.rs`) and that re-entering the cycle panics
   (`cycle_initial_call_back_into_cycle.rs`), and `cycle_initial` and `cycle_result` receive the query's own id
   and its arguments (the macro passes them; only `cycle_fn` gets the `Cycle`), which is what S-6 confirms. "Reported once"
   is produced by a report query over the value-only SCC, at the earliest member in declaration order. A
   constant that merely reads a cycle's member computes from the fallback, as PHP's read of it would throw.
3. **Bodies.** `purity` and `property_writes` use `cycle_result` with the impure / `Anything` answer: REG-1's
   rule is that a declaration in or reaching a cycle never leaves it, which is what an immediate fallback
   on every member gives. `inferred_return` and `inferred_throws` (REG-17) use `Fixpoint`: `cycle_initial` is
   `never`. **Termination is shown, not asserted (ruled 2026-10-18).** A recursive body that wraps its own result
   (`return $n ? [f($n - 1)] : 1;`, a generic or object argument, a closure type) grows one level of nesting per
   iteration while no literal changes; the `types` crate caps union width (64) and shape fields (256) and has no
   depth cap; and salsa does not stop at 200 iterations (`cycle.rs:58`): it panics, in release too, and re-runs
   every member of the cycle each time. Decision 17's "a literal that an iteration changed" is also a function of
   the iteration history, so a cycle entered at another member could end at another type, against H2.1 7. So
   `cycle_fn` is the capped operator, a function of the value alone: the iterate is the least fixpoint of
   `cap ∘ f` from `never`, which does not depend on the member the iteration is entered from. `cap` is the
   `types` crate's own generalization (a union past `MAX_UNION_MEMBERS` generalizes by the chain in `union.rs`,
   and the result is a fixed point), the shape-field cap, and two caps that do not exist yet, a nesting depth and
   a generic-argument depth. A literal that stays the same across iterations is kept (`'done'` stays `'done'`),
   which the first draft's "widen every literal in a recursive position after the first iteration" lost; one that
   keeps changing (`$n + 1`) is bounded by the union cap and generalizes to its kind there. A legitimate chain
   therefore takes up to about 66 iterations (64 members, then the generalization), so the safety net, when
   `cycle.iteration()` reaches K, returns the position's top type and is an internal error in a debug build, sits
   just under salsa's cap of 200: it is never expected to fire, and salsa's own cap is never relied on. Decision
   17's sentence becomes "the iterate is the least fixpoint of the capped operator", with the same intent (an
   inferred return keeps the literals its body returns). S-6 adds the nested-wrapper fixture, a mutual-recursion
   fixture entered from each member, and a `$n + 1` fixture whose iteration count it records: if the cost of
   about 66 iterations per member is too high, a lower per-kind literal threshold inside the cycle operator is the
   fallback, which is a function of the value as well.

Any other cycle is a bug. In a debug build it panics with the query stack; in a release build the unit
reports an internal error, as a panic does.

**A cycle is homogeneous.** salsa lets the query that is entered first decide: the cycle head's strategy
(`cycle.rs`, "the cycle head is responsible for managing iteration") either iterates the whole cycle to a fixed
point or gives every member with a fallback its fallback. Entered from another query, a cycle of mixed strategies
could resolve differently, which H2.1's determinism rule forbids. So every cycle has one strategy: the hierarchy has
none (the pass above), constants and purity are `cycle_result`, inference is `Fixpoint`, and an edge from one family
into another may not close a cycle. REG-17.4 is the row that threatens this (H3.8).

### H3.4 Candidates

`declarations_named` returns every declaration the name may bind to. The `Provider` turns that into a
`ClassView`: `One(&ClassFinal)` for the usual name, `Many` for a guarded polyfill or a duplicate. Every accessor on
`ClassView` has the `Many` semantic defined once, in the combinators decision 6 gives a builtin's readings:
an argument is checked against each candidate, a return or a property's type is their union, and a member that
only some candidates have is available in part (reported unless a guard covers it); the relation reads such a
class as each candidate, an ancestor every candidate has being certain and one that only some have possible.
`class_ancestors` carries that certainty. No level query handles `Many`; the levels are per declaration.

### H3.5 What the rows become

| row | becomes |
|---|---|
| REG-7.1 to 7.5, 7.12 | the queries above |
| REG-7.6 | `class_constant_value`, `global_constant` with `cycle_result` |
| REG-7.7 | nothing is built as scratch: `ClassDecl` carries `method_tags`, `trait_aliases`, `resolved_trait_conflicts` and `type_imports` as declared; the rules that read them read `class_decl` |
| REG-7.8 | `Session::load` demands every file's `file_decls` and `decl_index`; the whole-program passes are queries; `index_and_flatten` is deleted |
| REG-7.9 | H2.8 and H3.3 |
| REG-7.10 | `Provider`'s whole-registry iterations over `decl_index`, provenance and level 1 |
| REG-7.11 | by construction: a trait's members are read from its current declaration every revision |
| REG-8.1 | input writes: `FileSet`, `File.role`, and in W2 `file_decls` as an input (H3.6); collision policy is `declarations_named` plus candidates |
| REG-8.2 | superseded by the recorded reads (already so in the ledger) |
| REG-8.3 | `file_decls`; `DuplicateClassSites` and `DuplicateFunctionSites` become a query over `decl_index` grouping by exact spelling |
| REG-8.4 | `Provider`, which a database with no corpus cannot make; no `ReflectionBuilder` exists to be split off |
| REG-8.5 | the checker reads its file's declaration from `class_decl` and `file_decls`; `redeclared_methods` goes |
| REG-8.6 | H1.3 |
| REG-8.7 | the engine's closure gate: a random edit sequence applied through `apply` equals a database built fresh on the final file set, compared by `registry-dump` and by every `Provider` answer for every name; plus, during W2, the engine against the old passes (H3.6) |
| REG-9 | each staleness case is a test of the closure gate; the fix is the construction |
| REG-11 | behaviour fixes, each its own delta commit in the level or the declaration builder (`class_decl`) whose logic it changes, after step 3 of H3.6: 11.1 on PROD-5.1's vectors, 11.15 on the `Provider`'s folded keys, 11.16 on the entity-index queries (H3.7); 11.14 is superseded |
| REG-17.1 to 17.4 | the queries above the levels, H3.8 |
| REG-10 | a `TestProject` builder (inline files into a database with the corpus) replaces `Registry::build`'s 188 tests' setup; `Registry::build` and `Origin::Unknown` are deleted |
| REG-16.5 | salsa's event callback (`WillExecute`, `DidValidateMemoizedValue`, `WillBlockOn`; `event.rs`) counts executions and validations by query; the *asker* of a query is not in the event, so the provider records the first reader of each name itself (S-3) |

### H3.6 The order inside W2, and what `file_decls` is until W3

W2 puts the registry on the engine before the pipeline is on it, so in W2 `file_decls(File)` is an **input**: the
existing loader parses a file and builds its declarations as today, and `apply(FileDelta)` sets them (REG-8.1's
`FileDelta` is exactly that). W3 turns it into a query over `parse`. Everything above it, `decl_index`, the levels,
the provider, does not change between the two. During W2 the old checker driver still decides what to recheck
(`walk_affected` and the dependents indexes stay until REG-16.4), reading the registry through the `Provider`
outside any query. In W3 `check_file` becomes a tracked query and the recording starts.

Within W2, each step has the same shape (a "closure gate" per step), **run with the cadence of your rule: the tests,
the fixtures and the in-binary comparison per commit, and the `registry-dump` and O2 comparisons once, at W2's stage
checkpoint** (the dumps are produced where they are cheap and compared there):
1. **REG-4.2 and REG-4.12** on the old storage: exact-spelling accessors, folded keys on `FoldedName`, the
   `Provider` facade over the old maps. `registry-dump` before and after is equal except REG-4.2's approved delta.
2. **The engine module, shadowing.** Inputs, `decl_index`, `declarations_named`; the `Provider` still answers from
   the old maps and, in a debug build, compares each answer with the engine's (a listed exception to Part E 13's
   policy, deleted with step 3's last level).
3. **One level at a time** in the order 1, 2 (with REG-7.6 and its needs, REG-18.2 and 18.3), 3, 4, 5, then
   REG-7.12 and REG-7.10: the `Provider` flips that level's answers to the engine, the old pass stays as the debug
   oracle for one stage, the in-binary comparison runs over Piwigo, the vendor projects and the fixtures in the
   tests, and the old pass is deleted in the next commit. Deltas (VAL-2.9, 2.10) are read as they land.
4. **REG-8's close:** `apply` for every edit path, the closure gate, REG-9's tests, REG-10's builder.
5. **The data-model rows that wait for the levels.** REG-4.15 (a declaration built straight into its final form,
   with an ordinal where it had a span: H1.5a) is level 2's construction, so it lands with step 3's level 2; REG-4.11
   (provenance as the only location record) lands after REG-8.5 and REG-10 have removed `is_stub`'s and
   `Origin::Unknown`'s readers.
6. **REG-11's behaviour fixes,** one commit each after the structural ones, each a delta approved in the plan and
   fixtured first.
7. **REG-12's gate:** the gated steps' medians, and the test that checking one file resolves no class the file does
   not reach (a count from the event callback, not a guess).

### H3.7 What is not a level

**Global variables.** `infer_global_variables_of` (`registry/global_vars.rs:323`) is not a fixed point, though it
is run again per stale file. A file's sites are a function of its `GlobalFacts` and the registry: a `StaticValue`
names a call, a static call, a class constant or a method call, never another global, and
`GlobalFacts::resolve` turns it into a type through the registry (run per file, in parallel). The merge then applies
the sites of the files that `resolve_global_scope_files` accepts (an include graph matched by path suffix) to one
accumulator per name. Nothing in a site reads another global, so **there is no cycle before REG-17.1**; once
REG-17.1 makes an undeclared return the body's inference, a site `$x = f();` resolves through
`effective_return(f)`, whose body can read a global (`function get() { global $cache; return $cache; }` with
`$cache = get();`), and `global_sites(F)` → `effective_return(f)` → `inferred_return(f)` →
`global_variable(cache)` → `global_sites(F)` closes. So the global queries and `effective_return` and
`effective_property` belong to the inference family (H3.8): `Fixpoint`, `cycle_initial` = `never`, the same
widening. Before REG-17.1 they keep the default strategy, since there is no cycle to enter. On the engine:

| query | key | result |
|---|---|---|
| `file_products.globals` | `File` | the file's `GlobalFacts` (already serializable), made by the products pass; the include sites are a field of their own, since they depend on no type |
| `global_sites` | `File` | `FileSites`: the facts resolved through the `Provider`, so a changed return type or class constant reruns exactly the files whose sites call it |
| `global_scope_files` | none | the files whose top level runs in global scope, from the include sites and the `File` paths |
| `global_site_index` | none | each name to the files that have a site for it |
| `global_variable` | `GlobalKey` | one name's accumulator `finish`ed: the sites of the files the index lists that the scope set accepts |

The `Provider` exposes `global_variable(&Name)`, so a file that reads one global depends on that global's entry
alone. The `stale` sets, `global_sites` as registry state and the staleness bookkeeping go. The crate split's C9
moves the code out of reflection and J2's SUM-2 changes what counts as a site; neither changes these queries.
The shape above is a fixture of S-6, entered from each of its queries, on one thread and eight.

**Queries with no state of their own.** The doctrine entity index, `names_with_assert_tags`, the PHPStan ancestry
fact and `phpstan_class_files` are queries over `decl_index` and provenance (REG-18.6 makes the last a provenance
query). The Pest configuration and the SQL context are inputs to the check, as `pest_config` and `sql` are
fields of `Project` today.

### H3.8 What REG-17 adds above the levels

REG-17.1 to 17.4 put body inference under the types a caller sees, and a reflection that read inference would make
the levels cyclic (`class_final(C)` reads the inference of `C::m`, which reads `class_final(C)`). So:

- **The levels never read inference.** `class_final` is a function of declarations, as H3.1 has it. Above it sit the
  *effective* queries: `effective_return(DeclKey, member)` (the declared return, else REG-17.1's inferred one, and
  for a context declaration REG-17.3's widening of a refinement its own code refutes) and `effective_property(DeclKey,
  name)`, and `inferred_throws` (REG-17.2). The `Provider`'s method and property views take the signature from
  `class_member` and the return and property type from these. A file that calls `C::m()` therefore depends on
  `m`'s body only when `m`'s return is undeclared or refinable, and a declared one backdates at once.
- **Families.** Inference (`infer_body`, `inferred_return`, `inferred_throws`, `effective_return`,
  `effective_property`, the global queries of H3.7, and the receiver types of REG-17.4) is one family with
  `Fixpoint`; `infer_body`, `effective_return` and `effective_property` are rows of H2.3's table. Purity and
  property writes are another with `cycle_result`. REG-17.4 has a summary read a receiver's type from the body's
  inference, and the inference of a call read the callee's purity (it forgets what the callee may change).
  `purity(f)` → `infer_body(f)` → `purity(f)` would be a mixed cycle, and so would `purity(f)` →
  `infer_body(f)` → `purity(g)` → `infer_body(g)` → `purity(f)` for mutual recursion: salsa records
  dependencies at query grain, so reading `infer_body(f)` "for receiver types alone" depends on its whole
  execution, including its reads of callees' purity. **So the staging is a query, not a hope:**
  `receiver_types(DeclKey, member)` runs the inference in a no-effects mode and may not read `purity` or
  `property_writes`; `purity` and `property_writes` read it; `infer_body` reads `purity`, `property_writes`
  and `receiver_types`: a DAG by construction. Which receivers stage is stated, because soundness needs it: for
  `$this->a->b->m()` the receiver's type depends on what earlier calls may have forgotten
  (`forget_members_a_call_may_change`), so an effect-free typing could follow a call to the wrong body and claim
  purity; only receivers no call can change (locals not taken by reference, parameters, `$this` itself) stage,
  and every other construct keeps the conservative answer it has today (impure, writes anything). S-6 tests it:
  the same results whichever query of a cyclic fixture is entered first, and on one thread and eight.
- **What it costs.** `effective_*` are per member and asked per call site; the effective return of a member with a
  fully declared return is one lookup of `class_member`'s return and no inference.
