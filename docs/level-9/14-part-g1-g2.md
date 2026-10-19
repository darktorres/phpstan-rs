# Part G. The designs of Part F 1 to 4 and 6

<!-- status:begin -->

## Status of this part's tasks

From the ledger at `b9de6dcf1` on 2026-10-19. Regenerate with `scripts/plan_status.py`; change the ledger, not this table. `also in` lists the other parts that name the task.

| task | status | ledger | also in |
|---|---|---|---|
| CHECK-6.3 | OPEN | OPEN | 05 06 07 12 13 23 |
| CHECK-6.18 | READY | READY | 05 06 10 12 16 23 |

<!-- status:end -->

Written from the code at `30aa2002` (the cross-platform plan's R7 is in; the stack's bookkeeping for it is not).
G1 and G2 finish item 9's checker side; G3 is the error-node model; G4 is the probe; G5 is item 0.
Where a design departs from a first-pass text it says so, and G6 lists the edits that follow.

## G1. How the checker holds a tree (Part F 1)

### G1 what is stored, and from where (verified)

Everything the checker keeps of a tree comes from
the tree it is checking (the first draft of this section put a tree lifetime on `Locals`; the workflow review
showed Stage D deletes it again, G-08, and G1.1 says why it is not there):

| holder | today | note |
|---|---|---|
| `Locals.condition_aliases` | `(Vec<LocalKey>, Rc<Expr>)` | `rhs.clone()` deep-copies the condition once per store (`narrowing.rs:1313`, `:1399`) |
| `Locals.query_builder_aliases` | `Rc<Expr>` | `doctrine/query_builder.rs:134` |
| `Checker.include_constant_initializers` | `FxHashMap<String, Expr>` | `includes::file_constant_initializers(program)` clones each constant's initializer out of the same file |
| `Checker.pending_or_disjunctions` | `Vec<(Expr, Expr)>` | owned clones |
| `Checker.statement_method_call` | `{ id: NodeId, owner, receiver }` | the id of a call expression |
| the two query-builder walks | `Cursor::{Borrowed(&Expr), Owned(Rc<Expr>)}`, `ChainCall.args: Vec<Arg>` | owned only because the alias is an `Rc` (the doc comment says so) |
| `try_throw_states`, `loop_break_states`, `loop_continue_states` | `Vec<.. Locals>` | per function |
| the infer cache | `Entry.addr: usize` plus a dense `NodeId` slot | `addr` tells apart trees that share ids |
| `var_tag_memo` | key `(doc.as_ptr(), len)` | a doc text's address (`var_tag_types.rs:290`) |
| LSP `Walker.current_function` (`navigation.rs`) and the second walker of `signature_help.rs` | `Option<FunctionDecl>` | deep-cloned for every function the walk enters; each also builds a `Resolved` with a `probe_scope()` and a class `String` for every site before it checks the anchor (F58) |
| LSP `ProbeScope` | `Function(Box<FunctionDecl>)`, `TopLevel(Vec<Stmt>)` | owned clones, so completion may truncate them |

Only two things come from another tree: the fabricated nodes (gone after step 0) and the probe's
receiver (G4). `Checker<'a>` borrows the registry, settings, bodies, SQL service and rules for `'a`,
and is built directly at three production sites (`project.rs:2513`, `lsp.rs:1518`, `:1546`) and
through `checker_for` at `inlay_hints.rs:26`; about 27 more constructions are in tests. `Locals`
has no lifetime, 415 mentions in 66 files (65 under `crates/phpstan-rs`), and ten `impl Checker<'_>` blocks
(`completion.rs:75`, `deprecated_usage.rs:241`, `expr.rs:6090`, `builtin_calls.rs:3624`, five in
`narrowing.rs`, `narrowing/preg.rs:7`) that this design leaves as they are.


### G1.1 No tree lifetime on any checker type

Walks take and pass handles (`Expr<'_>`, `Stmt<'_>`,
`Args<'_>` and the rest of Part B's) as parameters and locals, each with its own anonymous lifetime;
`Checker<'a>` keeps its name and its `impl<'a>` blocks (its `'a` is the registry's, settings' and
rules' borrow, as now), and `Locals` stays lifetime-free. An expression that must outlive the walk that
produced it is a `Rooted<Expr>` (item 8). **Why not the check's one lifetime on `Locals`,** which the
first draft had: Stage D (D1 and D5 of the crate split, CHECK-6.3) replaces alias replay by conditional
holders created at assignment and "removes the tree from `Locals` altogether", so `Locals<'a>` (415
mentions in 66 files, the drop-order constraints on about 27 tests, `AstRef` and `checker_for` reshaped)
would be built in item 9 and unbuilt in Stage D, which the no-interim rule (Part E 35 and 41) refuses.
A rooted value is decision 4's "an id lives only in products owned with its tree", stated in the type;
Stage D's deletion then touches the two alias maps and three fields, nothing else.

### G1.2 Rooted values in `Locals`

The alias values become
`ConditionAlias { targets: Rc<[LocalKey]>, condition: Rooted<Expr>, stored: Stored }` and
`QueryBuilderAlias { chain: Rooted<Expr>, stored: Stored }` (`Stored` is G2). The deep clone of each
stored condition goes, and so does the `Rc<Expr>`; a store costs one `Arc` count increment on the
file's tree, and a replay clones the alias (as `locals.condition_aliases().get(&key).cloned()` does
today) and reads `get()` from the clone, so the borrow of `Locals` is released before the replay
narrows it.

### G1.3 The other holders

are rooted the same way: `include_constant_initializers: FxHashMap<String,
Rooted<Expr>>` (the visitor in `includes.rs` returns handles; the map roots them),
`pending_or_disjunctions: Vec<(Rooted<Expr>, Rooted<Expr>)>`, and `StatementMethodCall { call:
Rooted<Expr>, owner, receiver }`, whose id comparison becomes `Rooted::is(handle)` (same tree, same
id).

### G1.4 Identity is handle equality

`Expr<'a>: Eq` compares (tree serial, id). That replaces
`std::ptr::eq` at `calls.rs:87` and `whitespace.rs:64-65`, and the `NodeId` comparison of
`statement_method_call`. (`calls/parameters.rs:485` compares registry `ParameterReflection`
references, not tree nodes, and is not touched.)

### G1.5 The infer cache

keeps its dense slots indexed by `id.index()` and replaces `Entry.addr` with
`Entry.tree: u32` (the serial); the chain walk compares tree, epoch, version and question. **The key gains
F33's context epoch** (`STAGE_B_FIX_REVIEW.md` F33, first bullet): `Checker` bumps a counter whenever the
state `infer_expr` reads changes (the current class or function, the `$this` binding, the facts-only and
member-reference flags), so the cache no longer rests on "each outermost `check_expr` resets it", and the
cross-check F33 proposes comes with it (under `cfg(debug_assertions)` and `PHPSTAN_RS_CHECK_INFER_CACHE=1`,
off by default, a hit recomputes and `debug_assert_eq!`s; the fixtures run once with it on in the gate).
Item 9 rewrites this key anyway, and adding the epoch later would change it twice. The test-build shadow
map is keyed `(tree, epoch, id, version, question)`. Nothing else in the checker holds a bare id, and a
rule in `xtask arch` enforces it: `ExprId` and `StmtId` are named only in `syntax`, in
`checker/infer_cache.rs` and in the one module that defines `ProbeScope` (G4a), and `Rooted` is built
in `syntax` alone.

### G1.6 The query-builder walks lose their ownership machinery

`Cursor` becomes a transient `Expr<'_>` (a
walk that starts from a stored alias works on `get()` of its own clone), and `ChainCall` holds
`Args<'_>` under the walk's own lifetime; both modules' `Cursor` enums and their `Deref` impls go.

### G1.7 Where a check starts

`AstRef<'a>` becomes `Kept(&'a Arc<Ast>) | Parsed(Arc<Ast>)` with
`Deref<Target = Ast>` and `arc() -> &Arc<Ast>`. `check_program(&mut self, ast: &Arc<Ast>)` keeps a
clone as the tree being checked, which is what `Rooted::new` roots a handle in; the checker borrows
nothing from it, so `checker_for_file` can go on returning the checker and the `AstRef`, and a test
that builds a checker before it parses keeps its order. A kept tree is an `Arc` in step 1 already
(Part B6).

### G1.8 The rooted form

for what must outlive a borrow: `Rooted<T> { ast: Arc<Ast>, node: T::Id }`
with `get(&self) -> T<'_>`, `is(&self, handle) -> bool` and `Rooted::new(&arc, handle)`, which always
asserts that the handle's serial is the `Arc`'s. Its users are the stored expressions above,
`ProbeScope` and `ProbeTail` (G4) and the LSP's `Resolved` values. **In step 1** the boxed tree has no
index from an id to its node, so a step-1 `Rooted` also holds the node, as the clone the aliases hold
today (an `Rc` of the legacy node), `get()` returns a handle over that clone with the tree's serial, and
the field goes with the bridges in step 2, when the id alone resolves.

### G1.9 The LSP walkers

(`navigation.rs` and `signature_help.rs`, which has its own) are request-local
visitors: each holds `current_function: Option<FunctionDecl<'a>>` (a copy) and `top_level: Stmts<'a>`
under the walk's own lifetime, and a request clones one `Arc<Ast>` instead of deep-cloning each function
it walks past. F58's other half is done with it: `record_var_self`, `record_var_read` and `instance` take
a closure and build the `Resolved` (with its rooted `ProbeScope` and class name) only when the anchor
matches, with a test that one construction happens per request.

### G1.10 One checker per worker (CHECK-6.18)

With no tree lifetime a `Checker` can outlive a file, so the
 ledger's "one checker per worker, reset for each file" is buildable; the reset is E7's, because E7
 enumerates the per-file, per-function and per-pass field sets it clears, and batch 7 reuses only the
 handle-free scratch (C2 note 7). A table that holds a rooted expression is cleared at each file, so that
 it does not keep the previous file's tree alive.


### G1 order

(a) Step 1's API with the tree serial (a step-1 handle is 16 bytes: the node and the
container) and `Rooted` in its legacy form (item 8). (b) The checker's modules follow Part B7's order, each
taking handles for its walks; the alias maps and the three fields convert in the commit that makes their
producers handle-typed (`record_condition_alias`, `record_query_builder_alias`, the flag alias,
`file_constant_initializers`, the or-disjunction collector, the statement-method-call recorder), since those
are called with the expression being visited and cannot hold a rooted one before they take a handle;
the infer cache and the two query-builder walks have commits of their own. (c) `AstRef`, `checker_for` and
the LSP last.

### G1 checks

Behaviour: the merge tests of G2, the checker's own tests, O2, and the expression-types dump of
G5 (the whole checker moves to handles, so every expression's type is compared before and after). The
compiler proves no handle outlives its tree where one is borrowed, and cannot prove it is the same tree
(one lifetime says nothing of that), so the proof of tree identity is the debug assertions: `Rooted::new`
asserts that a handle's tree is the `Arc`'s, and a stored rooted expression is read only through the
`Checker` whose tree it is rooted in, or a tree this check was given (the probe's fragment, G4a).

### G1 not here

Replacing alias replay by conditional holders created at assignment (Appendix D of the
crate split plan) is Stage D's; it deletes the two alias maps, the only tree references in `Locals`, and
this design costs nothing else to remove.

## G2. Stored-alias identity (Part F 2)

### G2 what the code means

Three sites create an alias: `record_condition_alias`
(`narrowing.rs:1313`), `record_query_builder_alias` (`doctrine/query_builder.rs:134`), and the flag
alias (`narrowing.rs:1399-1401`), which builds one `Rc` and puts it in both the then and the else
scope so that the merge after the `if` keeps it.
`merge_scopes` keeps an alias only when `Rc::ptr_eq` holds for both sides (`control_flow.rs:446`,
`:478`): "both branches inherited this very store". `Locals::declare` purges an alias when its name
or any target is rebound, which is what makes keeping it sound.


### G2.1 A store has an explicit identity

`Stored(NonZeroU64)` is taken from the counter `Locals`
already has for versions (`NEXT_VERSION`, an `AtomicU64`, unique across threads), once when the
alias value is made. `ConditionAlias::new(targets, condition)` takes it; the flag-alias site makes
one alias and sets clones of it in both scopes, so both carry the same `Stored`. A merge keeps an
alias when `a.stored == b.stored`. This is exactly `Rc::ptr_eq`'s meaning, without relying on an
allocation, and step 1 stays byte-identical.

### G2.2 No equality is derived

`ConditionAlias` offers `same_store(&self, &Self)`, so no code can
compare two aliases by node by accident.

### G2.3 Node identity is not adopted

Two stores from the same node on different forks could be kept
soundly (the purge discipline covers rebinds), and a pass of a loop does store the same node
again, but the entry scope of a pass has no alias for it, so the filter drops it either way. That
is an argument that the two agree in practice, not a proof; the stamp costs nothing, so it stays.

### G2.4 The `@var` memo

(`var_tag_types.rs:290`) is keyed `(tree serial, doc start, doc len)`:
`statement_var_tags` takes a `DocText<'a>` (Part B4's view of a statement's leading doc), whose
`key()` is that triple. The class and method parts of the key are unchanged.


### G2 tests

The existing merge tests; a test that an alias stored from the same node by two separate
events on two forks is dropped by the merge (pins the stamp's meaning); the flag-alias test (one
store in two scopes survives); the memo returns the same tags for the same statement in a loop's
second pass and different ones for another statement with an identical text.
