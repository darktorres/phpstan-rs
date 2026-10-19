# The flat tree: Part B (item 9), Part G's G1 to G4 and G6, and I1

Document two of four; see `POST_LEVEL_9_PLAN.md` for the table of where each part of the plan went, and for the sequence.
Item 9 is SYN-1 (the flat tree). Its steps are in Part B7; the batch that starts it is in `POST_LEVEL_9_PLAN.md`, Part C.

---

# Part B. Item 9: the flat tree (SYN-1.2 to 1.10)

## B1. What exists (code facts verified at `0fb1a4de` and re-counted at `ffa7cccb`; figures marked with their commit)

- **The tree** (`syntax/src/ast.rs`, 1,691 lines at `ffa7cccb` since R18 moved `is_truthy_literal` in, 58 public
  types): `Stmt` has 26 kinds and `Expr`
  47. Children are `Box`, `Vec`, `Arc<str>` (doc comments, inline HTML, namespaced names),
  `Name` (an `Arc`, one pointer) and `PhpString` (`Arc<[u8]>`). Sizes from dhat (20 G §8, M2, at `5fb858c9`):
  `Expr` 80 B, `Stmt` 160 B (24 B of it `doc_comment`/`doc_line` that few statements use), `Arg`
  104 B, `ArrayItem` 168 B, `Param` 200 B. The analysed files' trees are 173 MB for 19 MB of
  source, in 1.10 M allocations (20 G M2, at `5fb858c9`; the ledger's MEAS-2 measured 151.1 MB at exit for
  `791d29cb`'s code, which is the current figure); 53 MB of that is `Vec` capacity no element uses.
- **Ids.** Every `Stmt`, `Expr` and `ElseIf` has a dense `NodeId` numbered in build order
  (children before parents). Only two readers use it: the infer cache (`checker/infer_cache.rs`,
  keyed with the node's *address* too, "since trees of other files share node ids") and
  `StatementMethodCall.id` comparisons.
- **The parser** (5,241 lines in 15 files, `parser.rs` and the 14 under `parser/` with their tests): about 80 `self.expr(` and 37 `self.stmt(` constructor
  calls and 96 `Box::new`. Every function returns `Result<Expr, ParseError>` by value (the
  Pratt loop's `?` is 55.5 M and its moves 28.8 M). It also reads back what it built:
  `as_pattern` turns an `ArrayLiteral` into a `Destructure` keeping id and span;
  `parse_ternary_rest` and `parse_assignment_tail` inspect `lhs.kind`; `pattern_only` and
  `long_arrays` are keyed by `NodeId`; a sub-parser parses each interpolation fragment and
  continues the parent's numbering. Names are resolved while parsing (`NameContext`).
- **Mutation after the parse**, three `VisitorMut` users: `stub_attributes::Pass` (drops
  functions, methods and parameters, sets type hints, appends `@return`/`@param` lines to doc text,
  strips attributes), `registry::anonymous_class::Namer` (`AnonymousClass::name`) and
  `name_resolution::Resolver` (re-resolves names after the stub pass). They run on a file just
  parsed (`project.rs:parse_with_ignores`, `registry/stubs.rs`).
- **Readers.** 134 files and 2,452 lines name `ExprKind::`/`StmtKind::` (the ledger counted 2,612
  sites in 135 files earlier); 486 mentions of `&Expr`/`&Stmt`/`&[Expr]`/`&[Stmt]` (mostly
  parameter types) outside `syntax` in 103 files (28 inside); 51 `Visitor` impls (43 outside
  tests, counted as `impl .. Visitor<..> for`; 52 by a looser pattern) and 3 `VisitorMut` users; about 40 sites that take an argument or item list apart with a
  slice pattern (`let [a, b, ..] = args.as_slice()`); 3,829 lines of parser tests that match the
  old nodes; `composer/vendor.rs` (the generated autoload files), `xtask/stub_index.rs`, `fuzz/`
  and the benches also use it.
- **Holders of trees.** `FileState.kept_ast: Option<Vec<Stmt>>`, `AstRef`, `AstCache` (32 trees
  parsed again), the project's per-file `ParsedFile.ast`; the checker's scopes (`ConditionAliasMap`
  and `QueryBuilderAliasMap` hold `Rc<Expr>` clones), `includes::Initializers` (cloned `Expr`s),
  the Doctrine and SQL query-builder walks (`Owned(Rc<Expr>)`); the LSP's `ProbeScope` and
  `current_function: Option<FunctionDecl>`. Registry bodies keep trees only in debug builds: a
  skimmed body is text, parsed on demand by `parse_function_body` into a short-lived tree that a
  summary and a flow are made from.
- **Nodes fabricated outside the parser** (six sites in five groups; B6 lists each with the step that removes it): `checker/expr.rs` `static_call` and
  `constant_fetch` (4 calls) for computed member names; `var_tag_types.rs:536` a variable read;
  `phpdoc_type_compat.rs:268-301` a `FunctionDecl` with a `Param` and a `TypeHint` built to feed
  `FunctionReflection::from_decl`; `calls.rs:777` a `Vec<Arg>` from array items;
  `lsp/completion.rs:309-360` the probe statements, a `FunctionBody` and an `Arc::make_mut` on
  a cloned body, and `parse_standalone_expr`. **Identity by address or pointer** (6): `calls.rs:87`,
  `whitespace.rs:64-65`, the infer cache, `var_tag_types.rs:290` (the `@var`
  memo, keyed by the address and length of a statement's doc text), and the `Rc::ptr_eq` merge
  tests on the stored condition and query-builder aliases (`control_flow.rs:446`, `:478`), whose
  meaning is "both branches inherited this very store": an instance identity that a node handle's
  equality would change. (`parameters.rs:485` compares registry `ParameterReflection` references from
  `sig.params()`, not tree nodes, so item 9 does not touch it.)
- **What the ledger decided (decision 4, §2.4):** a file's `Ast` owns its source, its names and
  one typed vector per node kind; a child is a `u32` id and a list a range; strings are spans with a
  side buffer where they need decoding; large variants go in side tables; `Send + Sync` with no
  `unsafe` and an `Arc<Ast>` shared by the batch run and the server; immutable after the parse;
  every parse yields a tree (error nodes); an id lives only in products owned with its tree, with a
  debug tag as the backstop; SYN-2's token arrays are designed with it, and "whether nodes keep spans or token
  indices is settled by a measurement" (decision 4's last sentence and SYN-1.2's text; ruled on 2026-10-18: it
  stands, and spike S-12 is the measurement, made before step 2). The costs it names: the
  match sites read through the tree, and each access has a bounds check. (The workspace denies
  `unsafe_code`, which is why the 20 G plan's `bumpalo` arena, not `Sync`, was dropped.)

## B2. Decisions to rule on before step 1 (first-pass leans; Part E 1 to 9 are the current answers, D1 and D9 changed, and the user ruled on D3 and D4 on 2026-10-18)

| # | question | recommendation | why | the alternative |
|---|---|---|---|---|
| D1 | what consumers hold | **`Copy` handles and plain-field views over the stored ids** (`Expr<'a>`, `Stmt<'a>`, `ExprKind<'a>`, `Param<'a>`, list views), carried through the checker's walks, with an expression that must be stored held as a `Rooted<Expr>` (an `Arc<Ast>` and a raw id), so no checker type carries a tree lifetime and a bare id is only a cache key (G1) | the 486 signatures and 2,452 sites change mechanically (`&Expr` → `Expr<'_>`, `.kind` → `.kind()`, `Box` and `clone` vanish); struct destructuring (`let ClosureExpr { params, .. } = …`, 16 sites) survives; a handle cannot outlive its tree where it is borrowed, and a stored one is owned with its tree, which is the decision's "ids live only with their tree" in the type | ids + `Index<ExprId>` on `Ast` (la-arena style): fewer types, but every free function gains an `ast` parameter, and nothing stops an id leaking into a product |
| D2 | how to migrate | **staged**: step 1 moves every reader to the handle API over the *existing* boxed tree, step 2 swaps the storage behind it | each commit stays green; step 1 is a behaviour-preserving refactor, step 2 is local to `syntax`. **What the split buys is two oracles:** step 1's is the compiler and the readers' tests over about 130 files, step 2's is the canonical dump compared with the baseline taken at the end of step 1 (the boxed tree's), where one change set would have neither for either half. **What is end state and what is scaffolding:** step 1's API (handles, views, list views, `Visitor<'a>`, the dump, the `Ast` container) is the final one and survives; thrown away are the adapter that implements the views over the boxed nodes (step 2 writes the same signatures over the store), the old `Visitor` trait (deleted at the end of step 1, so it lives one step), the legacy-node bridges (B4) and the serial field the legacy nodes carry. The throwaway part is estimated at 1,000 to 1,500 lines, from the sizes of `walk_expr` (181 lines) and `walk_stmt` (120), the 47 + 26 arms of the two `kind()` mappings, about 25 view constructors and 15 list views (not the 400 of the first estimate). The rule that refuses interim work (Part E 35 and 41) is met by the end-state argument: nothing here is a structure the engine deletes, and it is the way the readers reach the final API with an oracle at each half | one change set that flips the type and all 134 files together: no intermediate commit compiles, and the dump baseline would have to be written once against the old nodes and once against the store |
| D3 | what a node records of position | **decision 4's last sentence stands: a measurement settles it** (spike S-12, before step 2; ruled 2026-10-18). The expectation, and the working assumption of B3 to B5 and I1, is **byte spans** (`Span`, 8 B), as now | token indices would tie every kept tree to its token stream, which the workers reuse as scratch, so a kept tree would keep its own tokens (about 5 B a token, kind plus start offset: about 24 MB for Piwigo's analysed files at ≈4 source bytes a token). Break-even is about 20 B saved per node (≈4 tokens a node on the plan's dhat counts), more than an expression node can shed (32 B against a Zig-style 16 B). Those are estimates, which is why S-12 builds both layouts for the corpus and measures exact bytes and the readers' hot paths. No reader needs trivia between tokens | token indices (a node reads its identifier, string, number or operator from its token, as Zig's do); chosen only if S-12 shows it wins, and then B3's `span` fields become token indices and a kept tree keeps its token arrays |
| D4 | where payload bytes live | **the ledger's layout, ruled 2026-10-18 with no amendment: the tree owns its source, and a `Text { start, len }` is a range of that source or of a side buffer** (two bits of `len` choose). A plain string, a doc comment and inline HTML are spans of the source: the parser already reads `&src[span]` when it takes a token, so no payload is copied. The side buffers hold the bytes no source has: a `String` for UTF-8 text (a folded doc group, the text the passes and the probe append, the lossy decode of a payload of a source that is not valid UTF-8) and a `Vec<u8>` for string-literal bytes that needed decoding (escapes, heredoc indentation). A source valid as UTF-8 (checked once at load) is held as a `str` where the buffer allows it, so a `&str` view of a doc comment or inline HTML is a boundary-checked slice; where the builder holds the worker's byte buffer instead, a view costs one small-range validation, which the census prices | no payload copies at all (72 K docs and 352 K single-quoted literals allocate today); the tree owns the bytes its spans index, so a span always resolves; a context tree needs no copy of the source, since the builder takes the worker's read buffer by swap and hands it back on `clear`; the lexer's `PhpString`/`Arc<str>` payloads go with SYN-2 | the tree's own text arenas, a copy of every payload in a `String` and a `Vec<u8>` (the plan's first answer): kept trees about 5.6 MB smaller at Piwigo scale, at the price of a copy of every payload and a second kind of text; rejected on 2026-10-18 |
| D5 | names in nodes | **`NameId` into the tree's name table** in step 2 | one refcount increment per *distinct* name per file instead of one per occurrence (≈2 M identifier tokens); no shared-cache-line ping-pong between the 8 workers on common names; nodes lose their drop glue | `Name` in nodes until SYN-2: smaller diff, keeps the atomics |
| D6 | how the three mutating passes run | **on the builder, before `finish`**: `Ast::into_builder()` gives the vectors back, the pass edits them (compacting a list, replacing a type hint, appending doc text), `finish()` freezes again; `Ast` itself has no `&mut` API. Deleted with STUB-7.3/PROD-5.1 (SYN-1.10) | keeps the interim honest and small; they are the only writers. They are interim under the no-interim rule, since STUB-7.3 needs STUB-7.1 and so cannot precede item 9; B6 sizes and lists them (about 500 lines, and the `Resolver` is not ported) | port the stub-attribute logic into the parser: wasted when STUB-7.3 re-specifies it |
| D7 | nodes fabricated in the checker | **remove them first** (the sites B6 puts in step 0 become calls on parts; `calls.rs:777` follows in step 1 and the probe in step 2) | an immutable tree cannot host them, and an overlay id space would put a branch on every access | an overlay arena in the checker |
| D8 | rows | **regroup** onto the steps: SYN-1.3 (readers read through the tree) = step 1; SYN-1.2, 1.4, 1.5, 1.7, 1.8 = step 2 (they are facets of one storage design, each checked by an assertion: size, `!needs_drop`, exact capacity); new SYN-1.11 (step 0) and SYN-1.12 (step 1's API); SYN-1.9 = step 4; SYN-1.10 = step 5. The ledger's `needs` change with it: SYN-1.3 needs SYN-1.12 (not SYN-1.2), and SYN-1.2, 1.4, 1.5, 1.7 and 1.8 need SYN-1.3 (B11) | the ledger orders them as if separable | keep eight commits that each leave a half-flat tree |
| D9 | error nodes | not in step 1: the model is G3, designed before step 2; the `Error` arms arrive with SYN-1.9 | a variant nothing produces has no tests and no defined meaning, and a `_ =>` arm decides it by accident | add them in step 1 so readers are touched once |

## B3. The stored tree (step 2)

```rust
pub struct Ast { store: Store, root: StmtList, goto_labels: List<GotoLabel>, … }   // immutable
struct Store {
    serial: u32,                 // keys per-tree caches; the debug tag of public ids; issued as the paragraph below says
    names: Vec<Name>,            // the only Arcs: one per distinct name in the file
    source: Source,              // the file's text, owned: `Utf8(Box<str>)` when it is valid UTF-8 (checked once at load), else `Bytes(Box<[u8]>)`; spans index it
    side_text: String,           // UTF-8 text no source has: folded doc groups, text the passes append, the lossy decode of a payload of a source that is not UTF-8
    side_bytes: Vec<u8>,         // string-literal bytes that needed decoding; two bits of `Text.len` choose source, side_text or side_bytes
    exprs: Vec<ExprNode>,        // children before parents, as NodeId numbers them today
    stmts: Vec<StmtNode>,
    expr_ids: Vec<RawExpr>, stmt_ids: Vec<RawStmt>,      // backing of every list of nodes
    qname_ids: Vec<RawQName>, hint_ids: Vec<RawHint>,    // backing of the lists of qualified names (parents, traits, catch types, `insteadof`) and of union and intersection members
    args: Vec<ArgNode>, items: Vec<ItemNode>, arms: Vec<ArmNode>, params: Vec<ParamNode>,
    attrs: Vec<AttrNode>, qnames: Vec<QNameNode>, hints: Vec<HintNode>, …
    functions: Vec<FunctionNode>, classes: Vec<ClassNode>, methods: …, properties: …,
    closures: …, arrow_fns: …,                           // large variants: one id from the node (an anonymous class is a `ClassNode` with no name: I1.4)
    docs: Vec<DocEntry>,                                 // sparse, sorted by owner at `finish`: binary search (I1.5)
}
struct RawExpr(NonZeroU32);      // what nodes and lists store: 4 B in every profile, so Option<RawExpr> is 4 B
pub struct ExprId { raw: RawExpr, #[cfg(debug_assertions)] tree: u32 }   // the public id: the debug tag lives here
struct List<T> { start: u32, len: u32 }                  // exact size, typed by its element
enum ExprRepr { Binary(BinOp, RawExpr, RawExpr), Call(RawExpr, List<ArgNode>), Variable(NameId), … }
struct ExprNode { span: Span, repr: ExprRepr }           // asserted == 32 B (I1.7), in debug and release alike
```

- **The serial** comes from a process-wide atomic `fetch_add` in `AstBuilder::new` and again in `clear` (step 3's
  scratch builders reuse their store, and `(serial, id)` of one file must not alias the next file's in any cache that
  outlives a file); `finish` and `from_builder` copy it unchanged; `Clone` (the project's test-only `Clone` of a
  parsed file) never copies it, a clone takes a new one; a skimmed body's tree takes one per read; the counter's wrap
  is checked (unreachable in practice, but stated).
- **`NameId` is a `NonZeroU32`** (table index plus one), like the raw node ids, so `Option<NameId>` is 4 B and the
  record sizes of I1.4 (12, 24, 32) hold.

- **Records vs identified nodes.** A record that is only ever read through its list (`ArgNode`,
  `ItemNode`, `ArmNode`, `ParamNode`, `AttrNode`, class members) is stored by value in its vector and
  has no id; a node that something refers to or that is large (`Expr`, `Stmt`, class-likes,
  functions, closures, qualified names, type hints) has a typed id. Class members of one class are
  gathered in the parser's scratch and appended as one block, so a class's lists are contiguous even
  when an anonymous class inside a method appends its own meanwhile.
- **Declaration nodes reserve their id on entry and fill it on exit**, so `functions` and `classes`
  are in source order of their start, nested ones included, which is what PROD-5.1 asks of "the
  tree's own vectors" and costs nothing now.
- **What a statement carries.** No doc fields: the leading doc is found in `docs` (few
  statements have one; I1.5 lists every owner kind). `ElseIf`, `SwitchCase`, `CatchClause` and `MatchArm` are records.
- **Strings.** `Literal::String` is a `Text` (bytes, as `PhpString` is now); `Interpolated` parts
  are ordinary literal and variable nodes. A string that needs no decoding is a span of the source and
  costs no copy; one that does (heredoc indentation, `\u{..}` and the other escapes) is written decoded
  into `side_bytes`. A doc comment and inline HTML are spans of the source (a boundary-checked `&str`
  slice of a `Utf8` source), or, when the source is not valid UTF-8, the lossy decode in `side_text`.
  Until SYN-2 the lexer still makes a `PhpString` per literal: the builder keeps the span when the
  payload equals the source at the span (a `memcmp`) and appends only a decoded payload, so this step
  removes the payload the tree holds and adds no allocation for a plain string.
- **Invariants checked in tests and by `const` assertions:** `size_of::<ExprNode>() == 32` and
  `StmtNode` == 40 (I1.7); no node vector's element `needs_drop`; `Ast: Send + Sync`; every list is exact
  (`len == capacity` after `finish`). **Stored ids are raw and the tag is on the public id.** A node reaches
  another only through a handle that knows its tree, so a stored child needs no tag; the debug tag guards the
  one place an id travels without its tree, the public `ExprId` that `Expr::id()` returns and `Ast::expr(id)`
  takes, whose access asserts the serial. Were the tag stored in the nodes, the node sizes would differ between
  the debug build the tests run in and the release build, and the size assertions above could hold in only one.
  **A public id has no `Serialize`**, because the cache key hashes the sources and the target, not the build
  profile (`build.rs`), so a debug and a release build read each other's entries, and the public id's size
  differs between them. Where B5, B6 and Part I write `ExprId`, `StmtId` and the like for what the builder
  returns or a node stores, they mean the raw ids.
- **Size model (to be replaced by the census in step 0).** The 2,792 analysed files (2,793 in the 20 G report's M2)
  hold ≈1.1 M allocations at `5fb858c9`: mostly `Box<Expr>`/`Box<Stmt>` at 80/160 B plus lists. At 28-32 B an
  expression and ≈40 B a statement, the nodes take roughly a third to a quarter of today's bytes before the
  exact lists save the 53 MB, so I expect the 173 MB of `5fb858c9`'s figures (151 MB in MEAS-2, which scales the
  estimate by 0.87) to fall to somewhere around 35-60 MB, with
  1.1 M allocations becoming ≈35 vectors per kept file (≈0.1 M). The tree also owns its source
  (D4): about 19 MB for Piwigo's analysed files, 5.6 MB more than the payload copies an arena would
  hold, which the census prices. That is an estimate; the claim comes from dhat after step 2.

## B4. The read API (step 1 builds it over the boxed tree, step 2 over the store)

**Handles and views.** `Expr<'a>` and `Stmt<'a>` are `Copy` handles with `id()`, `span()`,
`kind()` (and `Stmt::doc_comment()`, which returns a `DocText<'a>`: the `Text` of B3 with its tree,
whose `key()` is (tree serial, start, len); `doc_line()`; `Expr::peeled()`). Handles compare equal
when their tree and id do. **In step 2** a handle is a reference to the store and a raw id (16 B). **In step 1**
a handle wraps a reference to the legacy boxed node, and each legacy node carries its tree's serial (a `u32`
field beside its `NodeId`, set by the parser from the tree being built, thrown away with the legacy nodes in step 2),
so `Expr::from_legacy(&'a legacy::Expr)` and `Expr::legacy(self) -> &'a legacy::Expr` are both free and need no
container: these two functions are the whole of "bridges", a module converts when it likes and in any order (callees
first leaves the fewest bridge calls), and no free function (`print_expr`, `literal_string_value`, `is_truthy_literal`,
`peeled`, …) has a second entry point. The two bridge functions and the serial field are deleted in step 2. `kind()` returns a view enum built on demand: `ExprKind<'a>`
and `StmtKind<'a>` keep today's variant names and shapes, with `Box<Expr>` → `Expr<'a>`,
`Vec<Expr>` → `Exprs<'a>`, `Vec<Arg>` → `Args<'a>`, `Name` → `&'a Name`, `Arc<str>` → `&'a str`,
`PhpString` → `PhpStr<'a>` (`as_bytes`, `as_str`, `to_str_lossy`), `QualifiedName` → `QName<'a>`
(the owned `QualifiedName` stays for the summaries and caches that store it and serialize it).
Every other AST type (`Param`, `Arg`, `ArrayItem`, `MatchArm`, `Attribute`, `ClassDef`,
`FunctionDecl`, `MethodDecl`, `PropertyDecl`, `ClassConst`, `Closure`, `ArrowFn`, `TypeHint`, …) is
a plain-field `Copy` struct of handles and borrowed data, built when a list is iterated or a kind
viewed, so `p.default`, `arg.value`, `closure.params` and destructuring keep their syntax. Lists
(`Exprs`, `Stmts`, `Args`, `Params`, …) are `(tree, slice)` pairs: double-ended `Iterator`,
`ExactSizeIterator`, `len`, `is_empty`, `get(i)`, `first`, `last`, `only()`, `split_first()`,
`split_last()`, `ids()`. A slice pattern cannot match a view, and about 40 sites use one on an
argument list, so the list views give the same shapes as values: `let [a, b] = args.as_slice()`
becomes `let Some([a, b]) = args.exact() else …`, `[a, b, ..]` becomes `args.prefix::<2>()`
(first two, whatever follows), `[first, rest @ ..]` becomes `args.split_first()` and a slice pattern whose rest is
read (`[needle, haystack, rest @ ..]`) becomes `args.split_prefix::<2>()`, which returns the array and the rest.
The same holds for the struct patterns that name an opaque handle (`Some(Expr { kind: ExprKind::Variable(name), .. })`,
about 20 sites, 7 to 10 of them outside parser tests by the pattern used, in `function_decl.rs`,
`property_type_too_wide.rs`, `stmt/loops.rs`, `strict_idioms.rs` and others): each becomes a `.kind()` match, and
step 1's table counts them. `Args<'a>` is an
enum with a materialized variant (a `Vec<Arg<'a>>` view), so the one synthesized argument list (`calls.rs:777`) is an
`Args` like any other and `check_args` and its callees need no second argument-list type.

The two kind enums and the ~25 view structs are written once in `syntax` (the mapping from the
stored form is one exhaustive `match` per enum, so a new variant is a compile error in both). A
small macro over a `Child` trait with a generic associated view type can generate struct pairs
from one declaration; handwriting the first few and deciding is fine.

**Before and after, three real sites:**

```rust
// checker/expr.rs ExprKind::Call
ExprKind::Call(callee, args) => {                      // callee: Expr<'a>, args: Args<'a>
    if let ExprKind::Name(name) = callee.kind() { … }  // was `&callee.kind`
    for arg in args { self.infer_expr(arg.value, locals); }
}
// checker/narrowing.rs: a condition kept for later replay
locals.set_condition_alias(alias, target_keys, rhs);        // rhs: Expr<'a>, stored as a handle (G1); was Rc::new(rhs.clone())
// checker/expr.rs: a closure
let Closure { params, uses, body, attrs, .. } = closure;    // was `= &**closure` of ClosureExpr
```

**Where a tree-derived value may be stored (G1).** Nowhere as a bare handle: a walk holds handles as
parameters and locals, and a value that must outlive the walk (the two alias maps, three checker fields,
the probe's tail) is a `Rooted<T>` (an `Arc<Ast>` and an id, built only from a handle of that `Arc`'s tree),
so no checker type takes a tree lifetime; never in a product that outlives the tree (summaries, flows,
reflections copy what they keep, as now). A bare id is named only in
`syntax`, in the infer cache (whose key becomes `(tree serial, id)` in place of `(address, id)`) and
in the module that defines `ProbeScope`.

**Traversal.** `Visitor<'a>` takes handles (`visit_expr(&mut self, e: Expr<'a>)`, …) with the same
hooks, order and `walk_*`/`any_*_in_body` helpers (class members by kind: constants, cases,
properties, methods). During step 1 the old and new traits coexist so the 51 implementors move one
module at a time; the old one is deleted at the end of step 1. `VisitorMut` is not migrated (D6).

**Rooted and owned forms** for the few products that must hold a tree part: `Arc<Ast>` (a file's
tree, a parsed body), and a rooted `(Arc<Ast>, id)` pair where a single expression is held
(`parse_standalone_expr`, the LSP's current function).

**Debug.** `Debug` for handles prints the canonical dump (B8); no `NodeId(n)` text. During step 1 the
container `ParsedFile` keeps a hand-written `Debug` that prints the legacy shape (`stmts: [...]`, the same fields), so
the existing `front-end-dump` digests stay comparable until the baseline is taken at the end of step 1; after that
the dump command is `syntax::dump`, and the old line cannot be recomputed (B8).

## B5. Construction (step 2)

`AstBuilder` owns the `Store` plus the parser's scratch stacks. A list is built by `mark = scratch.len()`,
pushing ids, `finish_list(mark) -> List<T>` (copies the exact slice into the backing vector and
truncates the scratch), so no list grows inside the tree. Nodes are `push`ed with their span;
`b.expr(kind, start)` replaces `self.expr(…)`. The parse functions return `Result<ExprId, ParseError>`
(fail-fast as now; step 4 replaces the failure with error nodes).

- **Reading back what was built**: `as_pattern` rewrites the node at `target.id` in the builder's
  vector (allowed until `finish`); its `ArrayItem` range is left as dead space in `items` (patterns
  are rare). `long_arrays` is the long-syntax arm of I1.2 (not a flag bit), `pattern_only` stays a short
  `Vec<(ExprId, ParseError)>`. The parser reads back and patches built nodes at more places, and each is a builder
  operation: `pop_last` for the Name expression that `Foo::x`, `Foo::class`, `new Foo` and `instanceof Foo` build
  and then discard for a copy of its `QualifiedName` (else every static access leaves a dead 32-byte node and an
  unused id; `tree-stats` counts live nodes); declaration `fill(id, attrs, doc, …)` taking what the caller parsed
  before the node's entry (a function's `attrs` and `doc_comment`, a class's `def.attrs`, a closure's `attrs`,
  declared names in `Parser::stmt`); `set_span_end` (a double-quoted literal without interpolation, a switch case's
  end, `removal_span` in `finish_member`); flag setters on built params (`promotion.readonly`); and `isset`/`unset`,
  which parse an argument list and keep only the values.
- **Limits are errors, not assertions.** A table that would pass 2^30 entries (ids are `NonZeroU32` below 2^30,
  I1.1) makes the builder return a `ParseError`; the loader refuses a source of 2^28 bytes (256 MiB) or more before it
  reads it (from the listing's length, and again after the read for a file that grew) and reports "file too large" as
  a run-level error like an unreadable file, and a `didOpen` buffer over it is refused with a message. A source under
  that limit has fewer than 2^28 tokens and the nodes are a few per token at most (the recovery's zero-width error
  nodes are at most one per consumed token, G3.4), so no id limit can fire on it, and `parse` assumes the bound (its
  checks are debug-only restatements). Today nothing checks: `LineIndex` casts offsets with `as u32` and the file is
  read whole (the largest corpus file, `autoload_static.php`, is 1.8 MB).
- **Sub-parsers** for interpolation fragments take the parent's builder by `mem::take` and return it,
  so ids keep running.
- **Names and payloads**: `b.name(Name) -> NameId` looks up by `Name::text_hash()` with identity
  equality (≈25 Ir) until SYN-2's lexer hands out ids itself; `b.text_span(Span) -> Text` takes a range of
  the source with no copy, and `b.text_decoded(&[u8]) -> Text` (or `text_str` for UTF-8 text) appends to a side
  buffer, for the bytes that no source has.
- **Skimmed bodies and fragments** (`parse_function_body`, `Lexer::fragment_bytes`) build their own
  small `Ast` over the body's text, which is its source; ids are local to it, as `NodeId`s are today ("the
  numbers differ when a body is parsed on its own").
- **Scratch reuse (LOAD-3.5)**: the builder's vectors are cleared between files on a worker and
  copied out exact-size for a file whose tree is kept (`Ast::from_builder`); a file whose tree is
  not kept never leaves the worker. The builder takes the worker's read buffer as its source by swap and
  hands it back on `clear`, so a context tree copies no source; `from_builder` copies it, exact-size, for
  a tree that is kept. This is step 3 and needs no other change, because handles are
  over `&Store` for both.

## B6. Special cases

| case | design | step |
|---|---|---|
| `stub_attributes::Pass`, `registry::anonymous_class::Namer` | builder passes via `Ast::into_builder()` (D6) with these mutators: `retain` on a statement list (function declarations, also inside `Block` and `Declare` bodies), on a class's methods, on a parameter list and on an attribute list (attributes are stripped on seven owner kinds), `set_type_hint`, `append_doc` (six owners: function, statement, parameter, class, property, constant) and `set_anonymous_name`; every hint the pass creates has the attribute's span; `parse_native_type` parses a hint with a `Parser` over the pass's own builder. **A dropped declaration leaves PROD-5.1's vectors by a `dropped` bit** in its node's flags word, never by renumbering (a method's `FunctionNode` lives in `functions`, so removing an entry would renumber the `FunctionId`s that statements and methods store): `retain` rebuilds the owning list's range without the dropped ids, so no list view names it, and the declaration vectors that `finish` makes for the products (`decl_names`, `decls`, `sites`) skip a node with the bit. About 500 lines written in step 2 and deleted with STUB-7.3 and PROD-5.1 (SYN-1.10): interim under the no-interim rule, since STUB-7.3 needs STUB-7.1 and cannot precede item 9. **The scaffolding of step 1 and 2 is one list with its deleting row:** the adapter that implements the views over the boxed nodes and the old `Visitor` trait (step 2), the two bridge functions and the legacy `Rooted` (step 2), `Ast::legacy_mut()` (step 2), `into_builder` with the mutators above (SYN-1.10); an `xtask arch` check reads the ledger and fails once a deleting row is DONE while its scaffold's name still appears in the source, so a scaffold cannot outlive its row | 2 |
| `syntax::name_resolution::Resolver` (`resolve_names`) | **not ported:** it is a whole-tree `VisitorMut` that re-resolves names and `namespaced_name` only because the stub pass adds hints after the parser resolved each name; the pass resolves a hint's names where `set_type_hint` creates them, from the `NameContext` it carries while it walks the statements in source order, and `resolve_names` (which only the pass calls) goes in step 2 with its `VisitorMut` use | 2 |
| `checker/expr.rs` `static_call`/`constant_fetch` | the `StaticCall`/`ClassConstFetch` arms' bodies become methods taking `(target, name, args, span)`; the computed-name paths call them; synthetic nodes also skipped the infer cache, so behaviour is unchanged | 0 |
| `var_tag_types.rs:536` | `native_variable_type(name, locals)` | 0 |
| `phpdoc_type_compat.rs:268-301` | `FunctionReflection::from_decl` and `check_phpdoc_types_against_native` take a borrowed `FunctionParts` that `from_decl` fills from the declaration and this site from parts | 0 |
| `calls.rs:777` | `Arg<'a>` is a `Copy` view, built from existing `Expr<'a>` values, so a synthesized argument list is a `Vec<Arg<'a>>` | 1 |
| address identity (`calls.rs:87`, `whitespace.rs:64`) | handle equality (`Expr::id()`, `Stmt::id()`); `parameters.rs:485` compares registry parameter reflections, not tree nodes, and needs no change | 1 |
| the `@var` memo's key (`var_tag_types.rs:290`) | the doc's `Text` range with the tree serial | 1 |
| stored-alias identity (`Rc::ptr_eq` on condition and query-builder aliases, `control_flow.rs:446`, `:478`) | G2: a `Stored` stamp taken when the alias value is made and compared by `same_store` keeps today's meaning; the node's id would change it, so it is not used | 1 |
| LSP probes (`completion.rs`) | G4a: a rooted `ProbeScope` and a checker entry that walks a statement prefix and infers a receiver handle from another tree; no synthetic statements, sentinel spans or import | 2 |
| `types::Type::string_literal_ref(&PhpString)` | add `string_literal_bytes(&[u8])` that interns without allocating when the type exists; `Literal::String` views give bytes | 2 |
| `composer/vendor.rs`, `xtask/stub_index.rs`, `fuzz/`, benches | readers of the API; migrated with their module | 1 |
| per-file cache, stub cache | hold no tree; ids are not serializable | – |
| `syntax::ParsedFile` (`syntax/src/parse.rs`) | `stmts: Vec<Stmt>` becomes `ast: Ast` (`ParsedFile::stmts()` returns the list view); a hand-written `Debug` keeps the legacy shape until the step 1 baseline (B4) | 1 (container), 2 |
| the project's per-file parse (`Parsed` and `ParsedFile.ast` in `project.rs`) | `ast: Option<Vec<Stmt>>` becomes `Option<Arc<Ast>>` | 1 |
| `FileState.kept_ast`, `AstRef`, `AstCache`, `Project::ast` | `Option<Arc<Ast>>`, `Arc<Ast>` | 1 |

After step 2 no node is fabricated outside the builder: every row above that builds one has been removed or replaced
by a call on parts, a handle or a rooted pair.

## B7. Steps, commits, and what checks them

| step | ledger | content | commits | checked by |
|---|---|---|---|---|
| 0 | new SYN-1.11 | prerequisites on the old tree: (a) `xtask tree-stats` (node counts by kind, live nodes only, token counts and the tokens-per-node ratio per file, which spike S-12 reads, list-length histograms, payload bytes, over Piwigo, the vendor projects and the stubs; the callgrind and dhat before-figures come from G5 tool 8's profile and the item-0 baseline, not a second run); (b) the fabrication sites B6 puts in step 0 (`static_call` and `constant_fetch`, `var_tag_types`, `phpdoc_type_compat`) | 4 | per commit: `cargo check`, clippy, the touched modules' tests |
| 1 | SYN-1.3, new SYN-1.12 | the read API (handles, views, lists, `Visitor` over handles, dump, `Ast` container, the two bridge functions of B4); then every *reader* module by module, in any order; the old node types stay reachable only through `Ast::legacy_mut()`, for the two builder passes, until step 2; the baseline dump is produced at the end | ≈30 (an estimate: one per module group of the list below) | per module: `cargo check`, clippy, that module's tests; at the end of the step the baseline dumps are produced and stored (B8), no suite runs |
| 2 | SYN-1.2, 1.4, 1.5, 1.7, 1.8 | spike S-12 first (byte spans or token indices, from step 0's census; the layout below assumes spans); then the store, the builder, the parser rewrite, views over the store, passes on the builder, the types change, the probe's tail (G4a); then the shrink (side tables, exact lists) | 4-6 (an estimate; the parser rewrite and the store with its views are one atomic pair, the passes and the probe tail follow) | per commit: check, clippy, the module's tests; the canonical dump equals the step 1 baseline over the corpus and the mutation corpus (B8), compared at the item's end |
| 3 | LOAD-3.5 | one builder per worker, cleared between files | 1-2 | per commit as above |
| 4 | SYN-1.9 | `parse` returns tree and errors; error nodes with spans; recovery with Q-10 | later | G3.7's matrix, golden dumps and mutation properties |
| 5 | SYN-1.10 | passes and `into_builder` go (needs PROD-5.1 and STUB-7.3) | later | `registry-dump` before and after equal for the stub attributes |

**The item's validation, once, at the end of step 3** (Part E 24; the user's cadence, no suite per step): `cargo test
--workspace` and the fixtures; `xtask dumps-compare` against the item-0 baseline for the expression types (the whole
checker moved to handles), the registry dump (unchanged by the item) and O2, and for the commits that touch `lsp`
(the probe tail, the `Arc<Ast>` holders) the server session and the edit replay; the step 2 dump equality above; the
Piwigo oracle off and warm; one thread = default threads; callgrind off and warm; dhat (trees at exit, allocation
count); wall by `abab` pairs (B8); peak RSS.

Step 1's module order (any order works, because the bridge functions are free; dependencies first leaves the fewest bridge calls): `syntax` internals (`printer`, `declarations`,
`name_resolution` readers, and `completion`, the statement-completion rules that level 9's R18 moves into `syntax`) → registry (`summaries`, `init_flow`, `purity`, `property_writes`,
`global_vars`, `guard`, `bodies`, `function_sig`, attributes) → project passes (`banned`,
`mock_hygiene`, `architecture`, `class_shape`, `composer_hygiene`, `use_sites`, `deps`,
`file_declarations`) → checker (`expr.rs` alone in several commits; `narrowing`, `stmt`,
`assignment`, `noop`, the rest) → `lsp` → `composer/vendor.rs`, `xtask`, `fuzz`, benches → parser
tests. During step 1 `VisitorMut` and the two builder passes (`stub_attributes::Pass`, `Namer`; the `Resolver` is
not ported, B6) keep the old node types through a `legacy_mut()` on `Ast`. A codemod for the two commonest rewrites
(`.kind` → `.kind()`, `&Expr` → `Expr<'_>`) is used, by span-based text edits and never by reprinting a file
(Part E 27); it is throwaway.

## B8. The dump, and verification

- **The canonical dump** (`syntax::dump`, step 1, written against the handle API only) prints everything a parse
  returns, per role (analysed, skimmed context, stub) and after the passes: an S-expression per node with kind,
  operators, names (written and resolved), payload bytes, doc text, spans, list structure, flags (anonymous-class
  names included); the token list as (kind name, span, payload text), independent of the `Token` type, so item 10 gets
  its baseline from this one; the non-doc comments and the lexer warnings; the facts with every watch bit and the
  other `FileFacts` fields (`has_anonymous_class`, `has_declaration_in_body`, `has_skimmed_bodies`, `mentions`); the
  references, sorted; the file's `goto` labels; the ignore marks; a line-index summary (the line count and, at every
  token boundary, the line, the UTF-8 and UTF-16 columns and the `offset_in` round trip); and the parse error's text
  and span. No ids, addresses or interner state; every view struct is destructured without `..` and every enum
  matched without `_`, so a field cannot be dropped unseen. `Debug` for handles prints the tree part. The dump is
  two parts, the tree and the rest (facts, references, labels, marks, warnings, line summary, error), so step 2 has a
  byte-comparable rest that does not depend on the node representation. This is what `front-end-dump` hashes today
  (`xtask/src/front_end.rs`: every token with its span, every non-doc comment, every lexer warning and the whole
  `ParsedFile` or the parse error), so the new dump covers no less than the tool it replaces.
- **Baseline** at the end of step 1 (still the boxed tree): the dump of every file of Piwigo, each vendor project, the
  two corpora the stub build embeds (`phpstorm-stubs` and `vendor/phpstan-src/src`, as `xtask`'s `stub_corpora`
  lists them) and the fixtures (digest per file plus a compressed full dump), next to the existing `front-end-dump`
  line per file (comparable because of step 1's legacy `Debug`), under `.corpus-runs/dumps/<commit>/` with G5's
  other dump sets. It also covers a **seeded mutation corpus**: G3.7.3's operations (delete, duplicate or swap a token,
  truncate at a random byte, insert a keyword) applied to corpus files, dumped by the step 1 binary and later by the
  step 2 binary, so the parser rewrite is compared on malformed input and the first parse error as well, where the
  fuzz targets only look for crashes. Step 2 must reproduce it byte for byte; anything else is read. The baseline
  is kept until item 10 (SYN-2) closes, since its token dump is item 10's baseline; the item-0 baselines are kept
  until VAL-1.13 (G5).
- **Tests as dumps.** The 3,829 lines of parser tests match nodes one at a time. Where a test
  asserts a shape, the assertion becomes an expected dump (short, and independent of the
  representation); tests of spans, ids or other semantics keep their form. Either way they move in
  step 1.
- **Equalities needing no baseline** (the 20 G plan's §11 list): one thread = default threads; stub cache
  cold = warm; per-file cache off = written = read; each edit-replay result = a fresh load.
- **Measurements** (taken at step 0 from G5 tool 8's profile and the item-0 baseline, compared at step 2): off
  callgrind of the parse job (`read_and_parse_file`, `parse_function_body`), `drop_glue` of the nodes, the main
  thread; dhat heap at exit by holder (trees 173.1 MB, ≈1.10 M blocks at `5fb858c9`; MEAS-2's 151.1 MB is the
  current figure), peak; wall time by `scripts/abab.py` pairs with the ledger's rule (the pair count and the Wilcoxon
  test), on Piwigo and on one vendor project through `check-project` and `incremental-bench`, since `abab.py` has
  Piwigo's path and digest as constants today and is parametrised by corpus and digest for this; `nice -n 19`,
  loadavg logged.
- **What the 20 G plan estimated** for item 9 (≈0.8-1.2 G of its 20.57 G) predates PROD-3
  (skimmed bodies), R11 (context trees dropped on workers) and the analysed trees being leaked at
  exit in the batch run, so the instructions it can still save are the parse's allocation and
  moves and the context trees' remaining drop. Memory (the analysed trees) and what the tree
  unlocks are the surer gains; the plan claims nothing before step 0's numbers.

## B9. What it unlocks and what it must leave room for

- **SYN-2 (item 10)**: the parser takes names and payloads at take time (`b.name`, `b.text_span`; I2): the lexer
  keeps only kinds and spans, and no `PhpString` or `Arc<str>` payload is made per token.
- **PROD-5.1** (declarations as the tree's own vectors): `functions`/`classes` already in source
  order with reserved ids; guards and name-context columns are added beside them.
- **DOC-2.9** (doc blocks per file found by offset): the tree keeps each doc as `Text` with the comment's line,
  and the parser does not carry the comment's source offset today (`take_doc_comment` returns only the text; the
  lexer's `DocGroup` has `first_start`). SYN-3.4 (batch 2) turns the line into the offset, which is what DOC-2.9
  (batch 6) finds a block by, so the `docs` entry needs no change then; SYN-3.4 also keeps `first_start` through
  the attribute re-key (`parse_leading_attributes` puts the doc into a fresh group with none today, so an
  attribute-prefixed declaration has a doc and no line), and `DocEntry.line` has an explicit "none" until it does.
- **CHECK-6.3** (variable slots): `ExprRepr::Variable(NameId)` leaves 4 B for a slot.
- **LOAD-3.5, LOAD-5.x, REG-16.4**: `Arc<Ast>` and the scratch builder.
- **SYN-1.9 and Q-10**: the error-node model is G3; its arms fit the size budgets of B3 and arrive
  with SYN-1.9.

## B10. Risks

1. **Stored expressions are rooted, not borrowed.** The first draft put the check's lifetime on `Locals` (415
   lines in 66 files); Stage D removes the tree from `Locals` again, so G1 keeps `Locals` and `Checker` free of a
   tree lifetime and roots the few stored expressions instead: an `Arc` count increment per stored alias
   where the deep clone was, and in step 1 a legacy form of `Rooted` that holds the cloned node (deleted
   with the bridges in step 2).
2. **The adapter is throwaway** (1,000 to 1,500 lines, D2): accepted because step 1 and step 2 each get an oracle
   (the compiler and the readers' tests, then the dump baseline), and because its API is the final one.
3. **`kind()` cost in hot loops.** A view is a match and a few copies (an estimated 10-20 Ir). Mitigation:
   `#[inline]`, call it once per node, helpers for the commonest projections (`variable_name()`,
   `string_literal()`, `is_null_literal()`); callgrind at step 2 decides.
4. **Owned string values.** The checker builds `Type::string_literal` from literals 13 times outside tests (18 with
   tests; 46 is the count of `Literal::String` mentions, which the first draft read as builds);
   `string_literal_bytes` must intern without allocating or the checker pays an allocation where
   it paid a refcount. A `&str` view of a doc comment or inline HTML is a slice of the tree's source (B3, D4), or of
   its `side_text` when the source is not valid UTF-8; a string literal is bytes, as `PhpString` is now.
5. **Tests and slice patterns.** 3,829 lines of parser tests and ~300 of visitor tests are
   rewritten (dumps shrink them, but the first conversion is manual), and about 40 slice-pattern
   sites on argument lists change shape (`exact`, `prefix`, `split_first`, `split_prefix`), besides the
   struct patterns on handles (B4).
6. **Hidden mutation.** The survey found only the three `VisitorMut` users (`stub_attributes`,
   `anonymous_class`, `name_resolution`) writing to a tree; any other would fail to compile
   against immutable views in step 1. Two of them are ported to the builder and the third (`Resolver`) is not (B6).
7. **Id overflow / giant files**: ids are `NonZeroU32` below 2^30; exceeding a limit is a `ParseError` from the
   builder, the loader refuses a source of 2^28 bytes or more (B5), and `parse` assumes the bound; the largest file in
   the corpus (`autoload_static.php`, 1.8 MB) is far below it.
8. **The stub build and the cache.** The stub pass and `resolve_names` run for every embedded stub
   file at build time; step 2's builder passes must not slow the cold stub build (measure it).

## B11. Ledger and stack edits when this is approved

- Ledger SYN: SYN-1.2/1.4/1.5/1.7/1.8 get one shared design note pointing at B3-B5; SYN-1.3's text
  becomes "readers read through the handle API (step 1)"; new SYN-1.11 and SYN-1.12; VAL-1.5 notes
  that the front-end tree dump is delivered by step 1. **No ruled wording of decision 4 changes** (the user ruled on
  2026-10-18 that both stand): the layout bullet ("a file's `Ast` owns its source ... a string is a span, plus a side
  buffer where it needs decoding"), SYN-1.2's "owns its source" and SYN-1.4's "payloads are spans" are what B3 builds,
  and the last sentence ("whether nodes keep spans or token indices is settled by a measurement", SYN-1.2's "settled
  first, by a measurement made with SYN-2's design") is spike S-12, run before step 2. SYN-3.4's anonymous-class
  bullet stays: the name is still positional, as types and messages show it, and only the identity is the
  ordinal (H1.4).
- The ledger's `needs` change with D8: SYN-1.3 needs SYN-1.12 and not SYN-1.2 (ledger SYN-1.3, 835), and SYN-1.2, 1.4,
  1.5, 1.7 and 1.8 need SYN-1.3 (SYN-1.8 already needs it); Part N lists every ledger and stack edit the plan implies.
- `WORK_STACK.md` level 8, item 9: "SYN-1.11, then 1.12 (+1.3), then 1.2/1.4/1.5/1.7/1.8; this design".
- Write Part B, I1 and the item-9 parts of G1 to G4 (G1's lifetimes, G2's stamp, G3.3's layout, G4a's probe tail) into
  the repository as `FLAT_TREE_PLAN.md` (the item is too large for the plan's item text) when plan mode ends; Part N
  says where every other part goes.

---


# Part G. The designs of Part F 1 to 4 and 6

Written from the code at `30aa2002` (level 9's R7 is in; the stack's bookkeeping for it is not).
G1 and G2 finish item 9's checker side; G3 is the error-node model; G4 is the probe; G5 is item 0.
Where a design departs from a first-pass text it says so, and G6 lists the edits that follow.

## G1. How the checker holds a tree (Part F 1)

**What is stored, and from where (verified).** Everything the checker keeps of a tree comes from
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

**Design.**

1. **No tree lifetime on any checker type.** Walks take and pass handles (`Expr<'_>`, `Stmt<'_>`,
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
2. **Rooted values in `Locals`.** The alias values become
   `ConditionAlias { targets: Rc<[LocalKey]>, condition: Rooted<Expr>, stored: Stored }` and
   `QueryBuilderAlias { chain: Rooted<Expr>, stored: Stored }` (`Stored` is G2). The deep clone of each
   stored condition goes, and so does the `Rc<Expr>`; a store costs one `Arc` count increment on the
   file's tree, and a replay clones the alias (as `locals.condition_aliases().get(&key).cloned()` does
   today) and reads `get()` from the clone, so the borrow of `Locals` is released before the replay
   narrows it.
3. **The other holders** are rooted the same way: `include_constant_initializers: FxHashMap<String,
   Rooted<Expr>>` (the visitor in `includes.rs` returns handles; the map roots them),
   `pending_or_disjunctions: Vec<(Rooted<Expr>, Rooted<Expr>)>`, and `StatementMethodCall { call:
   Rooted<Expr>, owner, receiver }`, whose id comparison becomes `Rooted::is(handle)` (same tree, same
   id).
4. **Identity is handle equality.** `Expr<'a>: Eq` compares (tree serial, id). That replaces
   `std::ptr::eq` at `calls.rs:87` and `whitespace.rs:64-65`, and the `NodeId` comparison of
   `statement_method_call`. (`calls/parameters.rs:485` compares registry `ParameterReflection`
   references, not tree nodes, and is not touched.)
5. **The infer cache** keeps its dense slots indexed by `id.index()` and replaces `Entry.addr` with
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
6. **The query-builder walks lose their ownership machinery.** `Cursor` becomes a transient `Expr<'_>` (a
   walk that starts from a stored alias works on `get()` of its own clone), and `ChainCall` holds
   `Args<'_>` under the walk's own lifetime; both modules' `Cursor` enums and their `Deref` impls go.
7. **Where a check starts.** `AstRef<'a>` becomes `Kept(&'a Arc<Ast>) | Parsed(Arc<Ast>)` with
   `Deref<Target = Ast>` and `arc() -> &Arc<Ast>`. `check_program(&mut self, ast: &Arc<Ast>)` keeps a
   clone as the tree being checked, which is what `Rooted::new` roots a handle in; the checker borrows
   nothing from it, so `checker_for_file` can go on returning the checker and the `AstRef`, and a test
   that builds a checker before it parses keeps its order. A kept tree is an `Arc` in step 1 already
   (Part B6).
8. **The rooted form** for what must outlive a borrow: `Rooted<T> { ast: Arc<Ast>, node: T::Id }`
   with `get(&self) -> T<'_>`, `is(&self, handle) -> bool` and `Rooted::new(&arc, handle)`, which always
   asserts that the handle's serial is the `Arc`'s. Its users are the stored expressions above,
   `ProbeScope` and `ProbeTail` (G4) and the LSP's `Resolved` values. **In step 1** the boxed tree has no
   index from an id to its node, so a step-1 `Rooted` also holds the node, as the clone the aliases hold
   today (an `Rc` of the legacy node), `get()` returns a handle over that clone with the tree's serial, and
   the field goes with the bridges in step 2, when the id alone resolves.
9. **The LSP walkers** (`navigation.rs` and `signature_help.rs`, which has its own) are request-local
   visitors: each holds `current_function: Option<FunctionDecl<'a>>` (a copy) and `top_level: Stmts<'a>`
   under the walk's own lifetime, and a request clones one `Arc<Ast>` instead of deep-cloning each function
   it walks past. F58's other half is done with it: `record_var_self`, `record_var_read` and `instance` take
   a closure and build the `Resolved` (with its rooted `ProbeScope` and class name) only when the anchor
   matches, with a test that one construction happens per request.
10. **One checker per worker (CHECK-6.18).** With no tree lifetime a `Checker` can outlive a file, so the
    ledger's "one checker per worker, reset for each file" is buildable; the reset is E7's, because E7
    enumerates the per-file, per-function and per-pass field sets it clears, and batch 7 reuses only the
    handle-free scratch (C2 note 7). A table that holds a rooted expression is cleared at each file, so that
    it does not keep the previous file's tree alive.

**Order.** (a) Step 1's API with the tree serial (a step-1 handle is 16 bytes: the node and the
container) and `Rooted` in its legacy form (item 8). (b) The checker's modules follow Part B7's order, each
taking handles for its walks; the alias maps and the three fields convert in the commit that makes their
producers handle-typed (`record_condition_alias`, `record_query_builder_alias`, the flag alias,
`file_constant_initializers`, the or-disjunction collector, the statement-method-call recorder), since those
are called with the expression being visited and cannot hold a rooted one before they take a handle;
the infer cache and the two query-builder walks have commits of their own. (c) `AstRef`, `checker_for` and
the LSP last.

**Checks.** Behaviour: the merge tests of G2, the checker's own tests, O2, and the expression-types dump of
G5 (the whole checker moves to handles, so every expression's type is compared before and after). The
compiler proves no handle outlives its tree where one is borrowed, and cannot prove it is the same tree
(one lifetime says nothing of that), so the proof of tree identity is the debug assertions: `Rooted::new`
asserts that a handle's tree is the `Arc`'s, and a stored rooted expression is read only through the
`Checker` whose tree it is rooted in, or a tree this check was given (the probe's fragment, G4a).

**Not here.** Replacing alias replay by conditional holders created at assignment (Appendix D of the
crate split plan) is Stage D's; it deletes the two alias maps, the only tree references in `Locals`, and
this design costs nothing else to remove.

## G2. Stored-alias identity (Part F 2)

**What the code means.** Three sites create an alias: `record_condition_alias`
(`narrowing.rs:1313`), `record_query_builder_alias` (`doctrine/query_builder.rs:134`), and the flag
alias (`narrowing.rs:1399-1401`), which builds one `Rc` and puts it in both the then and the else
scope so that the merge after the `if` keeps it.
`merge_scopes` keeps an alias only when `Rc::ptr_eq` holds for both sides (`control_flow.rs:446`,
`:478`): "both branches inherited this very store". `Locals::declare` purges an alias when its name
or any target is rebound, which is what makes keeping it sound.

**Design.**

1. **A store has an explicit identity.** `Stored(NonZeroU64)` is taken from the counter `Locals`
   already has for versions (`NEXT_VERSION`, an `AtomicU64`, unique across threads), once when the
   alias value is made. `ConditionAlias::new(targets, condition)` takes it; the flag-alias site makes
   one alias and sets clones of it in both scopes, so both carry the same `Stored`. A merge keeps an
   alias when `a.stored == b.stored`. This is exactly `Rc::ptr_eq`'s meaning, without relying on an
   allocation, and step 1 stays byte-identical.
2. **No equality is derived.** `ConditionAlias` offers `same_store(&self, &Self)`, so no code can
   compare two aliases by node by accident.
3. **Node identity is not adopted.** Two stores from the same node on different forks could be kept
   soundly (the purge discipline covers rebinds), and a pass of a loop does store the same node
   again, but the entry scope of a pass has no alias for it, so the filter drops it either way. That
   is an argument that the two agree in practice, not a proof; the stamp costs nothing, so it stays.
4. **The `@var` memo** (`var_tag_types.rs:290`) is keyed `(tree serial, doc start, doc len)`:
   `statement_var_tags` takes a `DocText<'a>` (Part B4's view of a statement's leading doc), whose
   `key()` is that triple. The class and method parts of the key are unchanged.

**Tests.** The existing merge tests; a test that an alias stored from the same node by two separate
events on two forks is dropped by the merge (pins the stamp's meaning); the flag-alias test (one
store in two scopes survives); the memo returns the same tags for the same statement in a loop's
second pass and different ones for another statement with an identical text.

## G3. The error-node model and parse recovery (Part F 3, 9)

**Why now.** Decision 4 says every parse yields a tree; Q-10 and decision 15 say what a failing file
keeps and how its version range is read; F77 says the failure is reported. Step 2 fixes the stored
layout and the builder's API, and batch 3 builds the recovery on them, so the model comes first.

**What exists (verified).**
- `parse` returns `Result<ParsedFile, ParseError>`; `ParseError { message, span }` is built at 28
  sites (31 lines name `ParseError {`; the type and its two impls are the other three), and the `expect*`
  helpers in `parser/tokens.rs` (`expect`, `expect_semicolon`, `expect_ident`, `expect_variable`) are the
  common path (`expect_variable` takes the offending token before it fails, so it peeks first once
  recovery needs "an `expect` that fails consumes nothing"); the parser uses `?` throughout and has no
  speculative parse-and-retry (a search for rewinds finds only the body skim's token pushback), so
  recovery needs no node rollback.
- The lexer has `LexWarning { span, message, is_error }` for what it skipped or what PHP refuses; only
  three sites set `is_error` today (a numeric literal, an octal digit, a `\u{}` escape). An unterminated
  string, comment or heredoc records nothing (an unterminated heredoc takes the rest of the file as its
  body; an unterminated single-quoted string ends one byte past the source), and a NUL byte in code is
  lexed as `Token::Eof`, which drops the rest of the file silently where PHP 8.5.4 reports "unexpected
  character 0x00". Those are new lexer errors, not existing ones.
- A load that fails to parse is `Read::Unparsed(ParseFailure { line, col, message })` and the file
  contributes nothing (F77). A body error in a context file is invisible because bodies are skimmed.
- `types::php_version` has `PhpVersion`, `PhpVersions { min, max }`, `PhpFeature` (45 variants) with
  `is_available(v)` (including features removed at a version), `KNOWN_PHP_VERSIONS` (5.3 to 8.5) and
  `known_php_versions_where` (which returns the hull `min..max`, not a set); the registry has
  `php_versions` and its methods `php_supports_everywhere`, `php_supports_somewhere` and
  `php_version_consensus` (`registry.rs`), which the checker calls.
- `docker` is available, and `tools/php-comparison-matrix` is the precedent: a PHP script run on
  official images whose JSON is committed as test data (`crates/types/src/tests/data/`).

**G3.1 Contract.** `syntax::parse(bytes) -> Parsed` is infallible. `Parsed` holds the `Ast`, whose
`errors() -> &[SyntaxError]` and `marks() -> &[Mark]` are sorted by position, and the file facts
with `has_errors`. For every byte string: it returns within a work bound proportional to the input and
within a memory bound (tested); it does not panic; every span is inside the file and nested in its
parent's; the tokens plus the spans in `errors()` cover the file (coverage is defined over those spans, not
over nodes, since a failed parameter, `use` item, catch type or class member is an entry in `errors()` and
not a node). **The memory bound** is a `NestingTooDeep` error kind at a fixed depth near PHP's (php 8.5.4
evaluates 9,000 nested `[` and fails at 10,000 and 12,000 with "memory exhausted", as at 40,000 parentheses):
the guard (`syntax::stack`) turns a stack overflow into heap growth in 16 MiB segments without limit, so
without a bound a file of tens of megabytes of `(` would cost tens of gigabytes; a bound near PHP's is both
runtime truth and the resource bound, tested at the boundary. A source over 2^28 bytes is refused by the loader
before `parse` (B5), so the id and offset limits cannot fire inside it. `ParseError` becomes `SyntaxError {
span, kind: ErrorKind, detail: Text }`; the message is built from the kind when shown, in PHP's wording
(`syntax error, unexpected token ";", expecting "]"`), so no per-error string lives in the tree.

**G3.2 Three kinds of failure, and one thing that is not a failure.**
1. *Missing.* A required token or operand is absent and the next token cannot continue the
   construct. The parser records the error at the position and builds the node as if the token were
   there: a call without `)`, a block without `}`, a statement without `;`, an operator without its
   right operand (a zero-width `ExprKind::Error`). The tree keeps its regular shape.
2. *Unexpected.* Tokens that cannot belong to the construct are consumed into an error node covering
   them: `StmtKind::Error`, `ExprKind::Error`, a `TypeHint::Error`. An error node is a leaf with an
   `ErrorId` (an index into `errors()`); it keeps no children. What the parser finished before failing is
   already a node of the enclosing construct (item 1), so nothing needs salvaging. **A class has no member
   enum** (its members are five per-kind lists, I1.4), so a failed member, and likewise a failed parameter,
   `use` item or catch type (records, not expression slots), is an entry in `errors()` found by span, and
   the owner's `incomplete` bit (in the flags word of `ClassNode`, `FunctionNode`, `ClosureNode`,
   `ArrowFnNode` and `PropertyNode`, which exist) says the declaration lost something; no field is added
   and no asserted size moves.
3. *Lexical.* Unterminated strings, comments and heredocs, `08`, an invalid escape PHP refuses, a heredoc
   body line indented less than its closing marker or mixing tabs and spaces there (PHP's "Invalid body
   indentation level"; `strip_heredoc_indent` cuts silently today), and, from PHP 8.0, a NUL byte or a stray
   control byte (item 4: a validity mark, not a `SyntaxError`). The lexer records a `SyntaxError` and yields a token the parser
   treats as a literal operand, so the parse continues; a NUL becomes an `Error` token like any stray
   control byte (the lexer returns `Eof` only at the real end), and an unterminated string's span is clamped
   to the source. Of these only the numeric literals, the octal digit and the `\u{}` escape are
   `LexWarning`s with `is_error` today; the others are new lexer errors (G3.8). The warnings that are not
   errors (skipped bytes before 8.0) stay warnings.
4. *Not a failure: syntax some version accepts.* The parser accepts every version's syntax and
   records a `Mark` (G3.5). Only syntax that no version accepts is an error (SYN-6): `(void)` as an
   expression and `function exit` in user code. **A stray control byte, a NUL byte included, is a validity mark,**
   not an error node: valid through 7.4 with a warning and a skip (the ledger's SYN-6 and Q-2; the syntax matrix
   confirms NUL itself on the 7.x images, which no binary here can), refused from 8.0 (php 8.5.4: "unexpected
   character 0x00"), which bounds a file from above at 8.0 (decision 15) and keeps the oracle's rule of G3.7.1
   without an exception.

**G3.3 Layout, for step 2.** Three enum arms, each with a payload of one `ErrorId` (4 bytes, inside
the 32-byte node budget), added with SYN-1.9: `ExprRepr::Error`, `StmtRepr::Error` and `HintRepr::Error`
(no `MemberRepr`: item 2 of G3.2 says where a class records its errors). `Ast` gains `errors:
Vec<SyntaxError>` and `marks: Vec<Mark>`. Step 2 does not build them: the parse functions keep
`Result` and the first failure ends the parse, so that step 2 is checked by dump equality; batch 3
replaces each `?` with the recovery below. What step 2 must guarantee is only that the arms fit: an enum
is as wide as its largest arm, and the largest arms of `ExprRepr` and `StmtRepr` are 20 and 28 bytes
(I1.2, I1.3), so a 4-byte payload relays out nothing; `HintNode` has room in its 20 bytes.

**G3.4 The recovery (batch 3).** A parse function returns its node, never a `Result`.
- *Progress, as a mechanism.* Every recovery loop records its position when it starts an iteration and,
  if the position has not moved when it comes round again, consumes one token as unexpected; the `expect*`
  helpers peek before they fail (so "an `expect` that fails consumes nothing" is true of all four); an
  error node consumes at least one token unless the token ends an enclosing construct (then it is
  zero-width). A step budget proportional to the token count ends the parse by turning the rest into one
  `Error` statement, so termination does not rest on an argument.
- *Suppression.* After an error at position p, another *missing* error is not recorded until a token
  past p is consumed, so one mistake gives one report; unexpected-token errors always record. At a
  bound (200 to start with) the parser stops recording, takes the rest of the file as one
  `StmtKind::Error` and ends; ordinary damage never reaches it (a test, G3.7.3).
- *Delimiters.* A stack of open delimiters (`(`, `[`, `{`, `${`, `#[`, alternative-syntax openers).
  Skipping never passes the closer of an enclosing construct; **a closer with no opener** (a stray `}` at
  the top level included) **is one unexpected token, consumed as a one-token error statement,** so it can
  never be a statement that consumes nothing; a delimiter left open at EOF is reported once, at its
  opener ("unclosed `{`"), and closed implicitly.
- *Statement level.* On a statement that cannot start or continue: consume tokens into a
  `StmtKind::Error` until a `;` at relative depth 0 (consumed), a `}` that closes an enclosing construct
  (not consumed), `?>` or an inline-HTML token, or a token that can only start a statement at depth 0
  and is not preceded by `->`, `?->`, `::` or `new` and is not inside a `function (...)` header
  (`if`, `while`, `for`, `foreach`, `switch`, `try`, `return`, `echo`, `class`, `interface`, `trait`,
  `const`, `declare`, `global`, `unset`, `goto`, `break`, `continue`, `abstract`, `final`, and `use`
  only at the start of a statement). Not stoppers: `function`, `fn`, `static`, `new`, `clone`, `print`,
  `include`, `yield`, `throw`, `match`, which can begin an expression, and `namespace`, `enum` and `#[`,
  which can too (`namespace\foo();`, `enum();`, `#[A] fn() => 1;`) and stop only when the tokens after them
  complete a declaration, as `stmt.rs` decides. Every keyword above is valid as a member name after `->` or
  `::` (`$o->use`, `$o->if()`, `Foo::while()`) and `class` follows `::` and `new`, hence the exclusions; one
  golden dump per excluded token.
- *Expression level.* A primary that cannot start: if the token is one that ends an expression
  (`)` `]` `}` `;` `,` `=>` `:` `?>` EOF) the error node is zero-width and nothing is consumed;
  otherwise it consumes that token. A binary operator with no right operand gets a zero-width error
  operand. (The rule is by the token's class alone; the statement and delimiter rules above are what keep a
  zero-width node from being repeated.)
- *Lists* (arguments, array items, parameters, `use` lists, `catch` types, match arms): after an
  item expect `,` or the closer. If neither: a token that can only begin a statement ends the list with one
  missing-closer error, and in an argument, parameter or `use` list so does a token that begins a new line
  after a complete item (`foo(1, 2` followed by `$y = 3;` is one error, as in PHP, and `$y = 3` is the next
  statement, not a third argument); in an array or `match`, whose items often sit one to a line, a token that
  can start an item is a missing `,` and the item is parsed; anything else is skipped as unexpected until
  `,`, the closer, or a statement-level stopper at depth 0. The golden dumps and the quality oracle of
  G3.7.3 pin the choice.
- *Class bodies.* A member that fails is an entry in `errors()` covering the tokens to the next member
  start (`public`, `protected`, `private`, `static`, `abstract`, `final`, `readonly`, `const`,
  `function`, `use`, `case`, `var`, `#[`), a `;` (consumed) or the closing `}` (not consumed), and sets the
  class's `incomplete` bit (G3.2).
- *Headers.* A function or class whose name is missing keeps the declaration with an empty
  zero-width name and the error, then skips to its `(`, `{` or `;`; every reader treats such a name as
  unknown (G3.6). A declaration that parsed its name is always a declaration, so references to it bind.
- *Everything else* (strings with interpolation, heredocs, alternative syntax, `match`, closures)
  follows the same four rules; each construct's sync set is a table next to its parse function, and
  the tests of G3.7 cover each row.

**G3.5 Marks: what a file used that a version lacks (decision 15).** `Mark { feature:
SyntaxFeature, span: Span, detail: MarkDetail }`, recorded by the parser where the grammar uses a
feature, deduplicated by span, sorted. `SyntaxFeature` is the grammar-only subset of `types::PhpFeature`'s
45 variants plus those below, defined in a leaf crate both `syntax` and `types` depend on (`php-version`:
`PhpVersion`, `PhpVersions`, `VersionSet`, `Availability`, `SyntaxFeature` with its one threshold table,
`KNOWN_PHP_VERSIONS`; Part E 32), because the parser is below `types` in the graph; `PhpFeature` in `types`
delegates to it for those variants and keeps the library gates that are not the parser's
(`PrintfHhSpecifier`, `FilterThrowOnFailureConstant`, `TentativeReturnTypes`, …). A file's validity is the
`VersionSet` of the known versions every mark accepts (`known_php_versions_where` returns a hull, not a set),
intersected with the declared range; an analysed file is reported at each mark a version of its range
refuses, a context file is not (decision 15), **and neither is a file that every load reaches only through
an include a guard covers when no autoloader maps it**, which also does not narrow the range (decisions 15
and 18). The guards are evaluated over the range being computed, so `effective_range` (J1.2) computes the
range with such files left out and then checks the guards against the result; when the analysed files share
no version the whole range is read and the conflict is reported. The mark count (every use of a feature
from 7.1 on is a mark in every `Ast`) is added to step 0's census, and marks are stored per feature with
a first span and a count if the census says the volume matters.

A mark is not only a feature with a threshold, because some syntax changes meaning instead of
becoming invalid, and some is deprecated and still valid. `MarkDetail` carries what the answer needs, and
`accepts(&self, v)` takes it. **There are two kinds of mark:** *validity marks* bound a file's range, and
*deprecation marks* never do (they are read by the existing deprecation rules).
- *Attributes* (`#[`): before 8.0 `#` starts a comment to the end of the line. A group is
  `comment_safe` when it lies on one line and nothing but whitespace follows it; only an unsafe group
  bounds validity from below. (`symfony/polyfill-*` and others rely on single-line groups.)
- *Reserved words as names* (`match`, `enum` in some positions, `readonly`, `fn`, `mixed`, `never`,
  `static`…): valid as an identifier below the version that reserves them, invalid from it, so such a
  use bounds validity from above.
- *Removed syntax* (`(real)`, `(unset)`, `$s{0}`, a stray control byte from 8.0): bounds from
  above; `(void)` bounds a statement use to 8.5 and an expression use to nothing (an error).
- *Deprecated, still valid* (the backtick operator from 8.5, which php 8.5.4 compiles and runs with a
  notice; `"${var}"` from 8.2): deprecation marks, read by `PhpFeature::DeprecatedBacktickOperator` and the
  like. Decision 15 bounds a file from above only by syntax a version removed, so these never bound it.

The inventory starts from `PhpFeature`'s syntax variants (non-capturing catch, throw expression,
promoted properties, the `(unset)` cast, final constants, readonly
classes and properties, constants in traits, typed class constants, named arguments, union types,
pure intersection types, first-class callables, property hooks, asymmetric visibility, dynamic class
constant fetch, standalone `null`/`false`/`true`, DNF types, `(void)`, attributes on global
constants, final promoted properties; array unpacking with string keys is not here, since its validity
depends on the unpacked array's keys and not on syntax) and adds the 7.x set: nullable types and `void` (7.1),
`iterable` (7.1), class constant visibility (7.1), short `list` syntax with keys and references
(7.1, 7.3), multi-catch (7.1), `object` (7.2), trailing comma in calls (7.3), flexible heredoc
(7.3), typed properties (7.4), arrow functions (7.4), `??=` (7.4), array spread (7.4), numeric
separators (7.4), `static`/`mixed` types and `?->` (8.0), `match` (8.0), trailing comma in
parameters (8.0), enums and `never` and `0o` octals (8.1), the pipe operator (8.5). It is a draft
from the grammar's history: **the matrix of G3.7 is its authority**, and a row is not final until the
images agree with it.

**G3.6 What readers do with an error (the contract).**
- *Checker.* `ExprKind::Error` infers the error type (F21's `Mixed(Error)`, which lands in batch 0: every "is this unknown" guard already covers it, and it records that a report was made), narrows nothing, reports nothing. **What a statement error sets** is stated, because the flag the first draft named does not silence anything: `any_variable_may_exist`
  (set by `extract()`, `eval` and `include`) is read today only at `builtin_calls.rs:826`, and the diagnostics
  `extract()` and `eval` discard are dropped by `saw_dynamic_var_construct`, which drops every diagnostic of
  the enclosing body (a loss for an editor). So a statement error sets `any_variable_may_exist`, and
  `variable.undefined` honours it: a read of a variable the swallowed region may have assigned reports
  nothing, which also changes `include` (it sets the flag and still reports such a read today), a delta for
  VAL-2.10 with its fixture; completion falls through. This is PHPStan's own rule (`canAnyVariableExist()` is true
  at global scope and after an include or `extract()`: `afterExtractCall`), and the stored Piwigo baselines hold no
  `variable.undefined`, so the oracle should not move. **`extract()` is made precise in the same commit (ruled
  2026-10-18):** every variable in scope becomes `mixed` and any may exist, and nothing else is dropped, where
  `saw_dynamic_var_construct` truncates every diagnostic of the body (`function_decl.rs`), the ones before the call
  and the ones unrelated to variables included; the two detections also disagree (`\extract(...)` and
  `EXTRACT(...)` set the flag but not the truncation), so one test on the resolved name serves both. `eval` keeps
  the truncation, since evaluated code can also declare functions and classes. **Exact include modelling is
  CHECK-12 (Stage D, on the scope model of D1 and D2):** the included file's top-level assignments in the
  includer's scope, and its return value as the type of the include (`mixed` today); `any_variable_may_exist` is
  its fallback for an include that cannot be resolved, so the flag is not interim. **A missing name** (the commonest missing token in an
  editor is a member name) is a zero-width name that every reader treats as unknown: no `property.notFound`,
  `method.notFound` or `class.notFound` for it, and the probe hit is still recorded, with a test per access
  kind. **The `Error` arms are added to the view enums and to a reader audit:** an `xtask arch` pass lists
  the matches that name `ExprKind::` or `StmtKind::` and end in a wildcard (`infer_expr_unguarded` ends
  `_ => Type::mixed()`, which would give implicit mixed, not `Mixed(Error)`), each classified once in the
  commit that adds the variants. An error node is a leaf, so nothing is checked inside it; a node built around
  a missing token is checked as written. For a file with errors the opt-in dead-code, unused and
  whole-program rules are not run (they would read a hole as absence).
- *Declarations.* A class whose `incomplete` bit is set (a failed member, G3.2) or whose header failed keeps
  the members that parsed: a lookup of a member it does not have answers *unknown*, as for a class with an
  unresolved ancestor, so `method.notFound` and its family stay silent. A function whose parameter or
  return type failed keeps the rest, and the failed part is `mixed`; a body with an error is a
  skimmed body (impure, throws anything). After a `use` that failed, a name in that file that nothing
  else binds is *unknown*, not `class.notFound`, since the import it might have been is lost.
- *Registry and cache.* The file's declarations enter the registry as for any file, with the
  `incomplete` flags; the per-file cache stores them with the flags and the file's error list (`CachedFile`
  stores `lex_warnings` today and gains `errors()`, so a warm run reports as a cold run does: B8's equality
  of cache off, written and read) and is keyed by the build, so a parser change never reads an old entry.
- *Fixes.* `--fix` and code actions skip a file that has errors.
- *Reporting.* An analysed file's errors are `phpstanRs.parseError` (level 0, non-ignorable, at the
  error's span, PHP's message) and fail the run. A context file's errors are recorded in the dump and the
  per-file cache and, as ruled on 2026-10-18, reported as a run-level notice, `phpstanRs.contextParseError`: its
  path, line and column and the first error, printed where today's "N file(s) skipped: parse error" count is, with
  the exit status unchanged and an opt-in switch that makes it fail the run. F77 designed a failing run, which would
  fail a run for a vendor file the parser refuses; decisions 9 and 15 ("nothing in a context file is reported,
  since its author is not the user") keep their text and gain one sentence for the notice, which is not a finding
  about the file's code but the report that part of the analysis context is missing (a class it declares may show
  as `class.notFound` elsewhere). A parser panic stays `phpstanRs.internalError`. **The version
  marks are the data the existing rules read:** `check_void_cast` (`cast.voidNotSupported`, `cast.void`) and the
  about 25 other syntax features reported through the registry's `php_supports_*` (property hooks, promoted
  properties, union types, named arguments, first-class callables, asymmetric visibility, typed class
  constants, …) each iterate `ast.marks()` and keep their identifier, message and level, so no construct is
  reported twice and no identifier that users have in `ignoreErrors` or a baseline changes; the commit that
  adds the marks lists which `php_supports_*` sites become mark readers, with a test that one `(void)`
  expression gives exactly one diagnostic and that a baseline keyed by those identifiers still matches.
- *Editor.* The live buffer's tree is a request-local parse of the request's text (one parse per completion,
  with its own `LineIndex`): the project's tree lags the buffer by the debounce window and the last-good tree
  stops being a notion (G4b).

**G3.7 Oracles and tests.**
1. *The syntax matrix.* `tools/php-syntax-matrix/`: a probe set (one file per feature and per edge,
   including the contexts above) run under the official images 7.0 to 8.5 with `php -l` on each probe
   file, writing `{probe: {version: ok | parse error: message | fatal: message}}` into
   `crates/syntax/src/tests/data/syntax-<version>.json` as the comparison matrix does. **`php -l` only
   says accepted or refused, and the marks of G3.5 exist for syntax that an older version accepts and
   reads differently** (a one-line attribute group is a comment before 8.0; `object`, `mixed`, `never`,
   `void` and `iterable` are class names before the version that reserves them; the flexible heredoc
   marker of 7.3), so each probe also records a *meaning* column per version: the token-name sequence
   from `token_get_all` (an identical API on 7.0 to 8.5) and, for type-name probes, a short reflection
   script (declare, then `isBuiltin()` and `getName()`). "Accepted" means accepted with the same meaning;
   the probes expected to differ are listed, and are the data `MarkDetail`'s `comment_safe` and keyword
   cases are checked against. Deprecated-but-valid probes (the backtick operator, `"${var}"`) are in
   the matrix with the result "ok with a notice", and the probe files that hold a CR or CR LF are marked
   `-text` in `.gitattributes` (R2's `* text=auto eol=lf` would store them as LF) or built from Rust
   strings. The test parses each probe and checks: no error nodes if and only if some version accepts
   it with a meaning (a known gap is listed, never silent), and the validity range from the marks equals
   the set of versions that accepted it. This is the authority for G3.5.
2. *Corpus agreement.* Over Piwigo, the vendor projects, phpstorm-stubs and the fixtures: no error
   nodes in a file PHP 8.5 accepts (Q-1's refused list stays the known exceptions); an error in each
   file PHP refuses.
3. *Mutation properties* (`proptest` and the `fuzz` targets `lex_parse` and `token_stream_parse`,
   extended): from corpus files, delete, duplicate or swap a token, truncate at a random byte, insert
   a keyword, a NUL byte or a lone control byte. Invariants: it returns without panic within a work
   and a memory bound; spans nested and in range; tokens plus the spans in `errors()` cover the
   file; the declarations that end before the first error equal those of the undamaged file. **A
   quality oracle, because those invariants are met by a parser that gives up at the first error:** over
   the corpus, delete or insert one closing delimiter or token and require at most the number of errors
   PHP reports (1 for a missing closer) and at least a stated share of the declarations after the damage
   recovered, with per-rule thresholds in the golden dumps; ordinary damage never reaches the 200-error
   stop. The fuzz targets also run 10^6 openers, stray closers at every level, and an unterminated
   everything. **The bar for checking an analysed file with parse errors (Part E 31, ruled 2026-10-18) is stated
   here, before the check is built:** over the same corpus, for damage to a single token, the diagnostics that are
   new, meaning absent from the undamaged file's own and not parse errors, are none at the 95th percentile of
   damaged files, measured per damage class; a class that misses falls back to parse errors only for the files it
   damages.
4. *Golden dumps* of about 150 hand-written broken snippets, one row per recovery rule of G3.4, in the
   canonical dump of Part B8.
5. *Editor tests*: completion, hover and symbols on buffers broken in each way (G4b).

**G3.8 Staging.** (a) Step 2 as G3.3 says. (b) Batch 3, in this order: the lexer's errors and tokens
(the new ones: unterminated strings, comments and heredocs, the NUL byte as an `Error` token, the clamped
span); `SyntaxError`, `errors()` and the infallible signatures with the first-error behaviour unchanged for
valid input (dump equality); the statement, expression, list, class-body and header rules, each with
its golden dumps; the `php-version` crate and the marks; the matrix; the readers (G3.6); reporting
and the editor. (c) SYN-6's rows are marks and error kinds in that table, not a separate pass.

## G4. The probe (Part F 4)

**What exists (verified).** Hover, signature help and completion share `ProbeScope` (`navigation.rs`:
`Function(Box<FunctionDecl>)` or `TopLevel(Vec<Stmt>)`), built by the navigation `Walker`, which
deep-clones every function it enters. `Server::probe_receiver_type` and `probe_variable_provenance`
build a throwaway `Checker`, set `probe_span`, run `check_function` or `check_program` over the scope
and read `probe_hit` or `probe_var_provenance`. Hover and provenance need nothing else: the span they
ask about is in the tree. Completion cannot do that, because the receiver of `$x->` is not in any
tree. `completion.rs` therefore: re-lexes the live buffer; scans backwards for the receiver's start
and the enclosing function's name (`receiver_start_idx`, `header_start`, `function_name_before`,
`class_name_before`, `find_enclosing_names`); finds that function by name in the *last-good* tree
(`find_function_by_name`); parses the receiver text alone (`parse_standalone_expr`, spans local to
the fragment); clones the function, drops the statements ending on or after the cursor's line
(`index.line(s.span.end) < cut_line`), and appends two synthetic statements (`$probe = <receiver>;`
and a read of `$probe`, with sentinel spans `u32::MAX - 3` and `- 1` and `NodeId::SYNTHETIC`). Its
doc says the uniform `$probe` read is what makes `->` and `::` alike.

**G4a. The entry point (item 9, step 2).** Step 2 makes the tree immutable, so the synthetic
statements cannot be built. The replacement builds nothing. **It is interim, on purpose,** and the
alternatives were weighed: recovery (G3) cannot come before step 2 because it is built on the layout step 2
fixes (G3.3), and a window with no completion between step 2 and batch 3 is worse than a small tail (about a
hundred and fifty lines), so the tail is built to be deleted whole by G4b and nothing of it survives as a
fallback:
1. **`ProbeScope` is rooted:** `ProbeScope { ast: Arc<Ast>, at: ProbeAt }` with `ProbeAt::Function {
   decl: FunctionId, class: Option<ClassId>, is_static: bool }` or `ProbeAt::TopLevel`. The walker
   builds it from one `Arc` clone; nothing is cloned per function, and `Resolved` stays an owned,
   lifetime-free value (built only when the anchor matches: G1.9).
2. **A tail instead of two statements.** `Checker` gets `probe_tail: Option<ProbeTail>`,
   `ProbeTail { cut: Cut, receiver: Rooted<Expr> }`, where `Cut::BeforeLine(u32, &LineIndex)` selects the
   statements of the body whose end line is before the cut, as today, as a prefix view of the body's
   `Stmts` (`prefix(k)` / `take_while`). `check_function_impl` and `check_program` walk
   `body.prefix(cut)` when a tail is set and, after the block, infer the receiver in the resulting
   scope and store the type in `probe_hit` directly; the result is not found by matching a span, so
   no sentinel spans are needed and a fragment-local span cannot collide with a real one. In
   `check_function_impl` the reads of `f.body_stmts()` (the walk through `check_block`,
   `enter_assigned_variables`, the emptiness test) go through one `probe_body(f)` helper; in
   `check_program` the same for its statement list.
3. **The receiver** is parsed by `parse_standalone_expr` into a small `Ast` that the request holds, in an
   `Arc`, and rooted there for the tail. While the checker infers the receiver it keeps the fragment's
   `Arc<Ast>` as the tree it is checking (G1.7), so the aliases that inference may store are rooted in the
   fragment and die with the probe.
4. **Deleted:** `probe_statements`, `sentinel_span`, `PROBE_VAR`, `NodeId::SYNTHETIC` (its four uses
   in `completion.rs`, and four in the checker: the three fabrication sites that step 0 removes and the infer
   cache's own test at `expr.rs:4523`, which changes with the cache key), `build_probe_scope`'s clone and
   truncate, the walker's per-function clone, and **the `declared_names` union in bare completion**, which
   is redundant today (`Project::update_from` parses before it unindexes, so a failed parse leaves the
   file's registry entries as they were), with `completion.rs`'s stale module doc (lines 7 to 13)
   corrected in the same commit.
5. **Kept until G4b:** the token scans (`classify`, `tokenize`) and the name lookup in the last-good tree;
   the `::` path for a bare class name; hover, provenance and signature help, which only change
   `ProbeScope`'s shape.
6. **The limits copied from today's probe, on purpose, and pinned by tests.** The cut slices only the
   function's top-level statements, by the live buffer's line against the last-good tree's line numbers, so
   the statement that contains the cursor (a `foreach`, `if`, `try`, closure or arrow-function statement) is
   dropped with everything it binds (for `foreach ($rows as $row) { $row->` the receiver `$row` is unknown;
   likewise a closure's parameters and an assignment earlier in the same block), top-level code is the same
   through `check_program`, a static method is fine (`is_static` is passed through), and lines inserted above
   the edit retain statements below the cursor. Tests pin each (nested block, closure, arrow function,
   top-level, lines inserted above), so batch 3 has a before and after; G4b removes the limits.

**G4b. The live tree (batch 3, with SYN-1.9).** With recovery the live buffer always parses, but not always
with a missing name: in PHP an arrow typed in front of an existing statement takes the next token as the
member name (`$x->` newline `$y = 5;` is `$x->$y = 5;`, `$x->` newline `foo();` a method call, `Foo::`
newline `bar();` a static call; php 8.5.4 accepts all of them), and when the next token cannot be a name
(`return 1;`) the error appears at that later token. So **the trigger stays the `->`, `?->` or `::` token
before the cursor** (`classify` and `tokenize` are kept, today's behaviour: it reads the token before the
cursor and ignores what follows), and the receiver is the object or class operand of the innermost access
whose operator token is that anchor, whether the name is missing, partial or the first token of the next
statement; nodes store no operator span, so the query finds the operator by scanning the source between the
operand's end and the name's start. Completion becomes a node-at-position query on **a request-local parse of
the request's text** (one parse per completion, with its own `LineIndex`; the project's tree lags the buffer
by the debounce window, so it would be stale exactly when completion is asked): the checker runs over the
enclosing function of that tree with `probe_span` set to the operand's span, as hover does. That deletes
`receiver_start_idx`, `header_start`, `function_name_before`, `class_name_before`, `find_enclosing_names`,
`find_function_by_name`, `parse_standalone_expr`, the tail of G4a and the last-good tree (the file's own
declarations no longer vanish from the registry while the buffer is broken, because recovery keeps them).
Statements after the cursor can be skipped with a `stop_after(span)` option on the checker, the same hook an
observer-based design (E6, F5) would use. **No fallback is kept:** a buffer whose enclosing function is
lost entirely (G3.4 keeps a declaration whose name parsed, and one with a zero-width name, but a header
with no usable `function` keyword leaves the cursor at the top level or in an error statement) gets bare
completion, stated as a limit with a test. The seven unit tests of `completion/tests.rs` that call the
deleted helpers directly (`receiver_start_idx_*`, `find_enclosing_names_*`) are listed in
`gates/test-renames` as `REMOVED` with the node-at-position tests that replace them, and the integration
tests stay unchanged. New cases (G3.7.5): an arrow before a statement that starts with a variable, with an
identifier, with a keyword and with `{`, a receiver inside a `foreach` body, a closure and an arrow function
(limits that G4a pinned and this removes), and completion on each kind of broken buffer.


## G6. What these designs change elsewhere

- **Part B.** B1's identity list is already corrected. B4's "Where an id may be stored" is replaced
  by G1 (handles in walks, `Rooted` for a stored expression, ids only in the infer cache and in `syntax`; a
  step-1 handle is 16 bytes), and B4 gains the `DocText<'a>` view of a statement's leading doc (the `Text` of
  B3 with its tree) that G2 uses. B6's LSP-probe row is replaced by G4a, and its `import` is dropped. B7 step 1
  loses "`Error` variants" and step 2 loses `import` and gains "the probe's tail (G4a)". B9's last
  bullet and B10 risk 1 follow. D9 is G3.3.
- **Part F** rows 1 to 4 and 6 are designed here; row 9 (parse recovery as a whole) is G3.4 and G3.8.
- **Part C** batch 3 (parse recovery) is G3.8(b); the sequence's batch 0 is G5 with F10's tests and F20 (the
  parser findings, whose designs are in the review) before item 9's baseline.
- **Part D** W7 gains nothing; G1's alias maps are what Stage D1/D3 delete.
- **What else the designs change** (each is a commit of the row that needs it): the public extension API in
  `rule.rs` (`pub trait Rule` takes `&Expr` and `&ExprKind`, and `pub struct Scope<'s, 'r>` pairs a
  `&Checker<'r>` with a `&Locals`; `set_external_rules` is embedder-facing), whose signatures change to
  handles with item 9 (D6 later redefines the trait); `Project::update_file*`, which return
  `Result<_, ParseError>`, and `lsp.rs`'s two `Err` arms, which assume a failed parse (G3.1 makes `parse`
  infallible); `CachedFile`, which gains the error list (G3.6); the ledger rows CHECK-6.18 (G1.10), SYN-1.9,
  Q-10 and SYN-6, and `CRATE_SPLIT_PLAN.md:6883-6891` ("a file with parse errors is still not analysed"), which Part E
  31 supersedes (ruled on 2026-10-18); and Part E 26, whose "observers replace it later" is now G4b in batch 3.

---


## I1. Node layout per variant (Part F 5)

Written from `syntax/src/ast.rs` (1,691 lines at `ffa7cccb`, 1,680 when it was read whole), against B3's budgets: `ExprNode` 32 bytes,
`StmtNode` 40, no element that needs a drop. The census of step 0 confirms the distributions; it can move a
field between inline and a side table (I1.7) but not change the shape.

### I1.1 Primitives

- **Ids** (raw, `NonZeroU32`, so `Option<Id>` is 4 bytes in every profile; B3): `RawExpr`, `RawStmt`, `RawFunction`,
  `RawClass`, `RawClosure`, `RawArrowFn`, `RawHint`, `RawQName`, `RawHooks`, which is what nodes and lists store.
  The public `ExprId`, `StmtId` and the like wrap one with the debug tree tag. **One id limit, 2^30:** the top two
  bits of an id or offset word are free for flags (`ItemNode.value` keeps `spread` and `by_ref` there,
  `QNameNode.resolved` its kind, `IdentAt`'s name word its `dollar` and `explicit_end`), and `ArgNode.name_at`
  keeps `spread` in bit 31, which is free because the loader refuses a source of 2^28 bytes or more (B5), so
  offsets are below 2^28. A table that would pass 2^30 entries is a `ParseError` raised by the builder, never an
  assertion (B5). Where this part writes `ExprId` in a payload, it means the raw id.
- **`NameId`:** an index plus one into the tree's name table (`NonZeroU32`, so `Option<NameId>` is 4 bytes and
  the record sizes below hold), one `Name` per distinct name, so equal ids are equal names.
- **`Text { start, len }`:** a range of the tree's source (a string, a doc comment or inline HTML as lexed) or of
  one of its two side buffers, `side_text` (UTF-8 text no source has) and `side_bytes` (decoded string-literal
  bytes); the top two bits of `len` choose (B3, D4). **`List<T> { start, len }`:** a range of `T`'s vector;
  `OptList` is a `List` whose `start` is `u32::MAX` when absent (a `finally` block, a `declare` body and a bare
  `exit` are absent, not empty).
- **`IdentAt { name: NameId, at: u32 }`:** an identifier and where it starts. Its span is `(at, at + name.len())`
  in the common case. **The encoding is total, not asserted:** the top two bits of the name word are `dollar`
  (the span is one longer for a `$`-prefixed name: a static property, a parameter, a catch variable) and
  `explicit_end` (the span's end is in the `member_ends` side table, found by `at`), and the builder sets
  `explicit_end` whenever the real span differs from the name's length (plus one for a `$`), so no assertion can
  fire on any input the parser accepts. The cases the rule alone would miss are real: an identifier read lossily
  from non-UTF-8 bytes (the name is shorter than the source), a `use` path with spaces (`use Foo \ Bar;` is
  refused by PHP 8.5 and accepted by this parser until SYN-4.8, and its span runs from the first token to the
  previous end), and the fuzz stream of `token_stream_parse`, whose spans are all zero. Tests: a non-UTF-8
  identifier, a spaced `use` and that stream; the dump oracle stays a check and is not how the rule is
  discovered.
- **`Target(u32)`:** a `StaticTarget` or an `instanceof` target: a `QNameId`, or an `ExprId` with bit 31 set.
- **`Wide { lo, hi }`:** an `i64` or the bits of an `f64`, so no enum needs 8-byte alignment.
- Every field is at most 4-byte aligned, so an enum is its tag word (the tag and a few flag bytes) plus its
  payload: `ExprRepr` is 4 + 20 = 24 bytes, `ExprNode { span, repr }` 32; `StmtRepr` 4 + 28, `StmtNode` 40.
- **Small enums fold into the tag word or into arms.** `ObjectOperator`, `DestructureSyntax`, `ExitKind`,
  `long_arrays`, `dollar`, `by_ref`, `spread` do not take payload bytes. A reader never sees the arms: the view
  of B4 folds them back into today's `ExprKind` shapes, so the repr is free to be what is smallest.

### I1.2 Expressions (47 kinds, 58 stored arms)

| payload | arms |
|---|---|
| none | `Null`, `True`, `False` |
| 4 | `Variable(NameId)`, `DynamicVariable(e)`, `ErrorSuppress(e)`, `Grouping(e)`, `Empty(e)`, `Print(e)`, `Eval(e)`, `Throw(e)`, `YieldFrom(e)`, `FirstClassCallable(e)`, `ShellExec(e)`, `ClassNameFetch(Target)`, `Closure(ClosureId)`, `ArrowFn(ArrowFnId)`, `NewAnonymousClass(ClassId)`, `Name(QNameId)` (a qualified name as an expression: the `qnames` record holds the written and resolved names and its own span, which is not the node's for `clone(...)` and `exit(...)`: the callee name there is the bare word's span and the node's extends over `(...)`; both forms are in the dump tests) |
| 5 to 9 | `Unary(op, e)`, `Cast(spelling, e)`, `Include(kind, e)`, `Binary(op, l, r)`, `CompoundAssign(op, l, r)`, `Int(Wide)`, `Float(Wide)`, `Str(Text)`, `Exit(kind, OptList)` |
| 8 | `Assign(l, r)`, `AssignRef(l, r)`, `Index(base, Option<e>)`, `Clone(e, Option<e>)`, `Yield(Option<e>, Option<e>)`, `Instanceof(e, Target)`, `DynamicPropertyAccess(obj, e)` and its nullsafe arm, `DynamicStaticPropertyFetch(Target, e)`, `DynamicClassConstFetch(Target, e)`, `Array(List<ItemNode>)` and its long-syntax arm, `Destructure(List<DestructureNode>)` and its `list()` arm, `Isset(List<ExprId>)`, `Interpolated(List<ExprId>)` |
| 12 | `Ternary(c, Option<t>, e)`, `Call(callee, List<ArgNode>)`, `New(class, List<ArgNode>)`, `PropertyAccess(obj, IdentAt)` and its nullsafe arm, `ClassConstFetch(Target, IdentAt)`, `StaticPropertyFetch(Target, IdentAt)`, `Match { subject, List<ArmNode> }` |
| 16 | `DynamicMethodCall(obj, e, List<ArgNode>)` and its nullsafe arm, `DynamicStaticCall(Target, e, List<ArgNode>)` |
| 20 | `MethodCall(obj, IdentAt, List<ArgNode>)` and its nullsafe arm, `StaticCall(Target, IdentAt, List<ArgNode>)` |

`MethodCall` and `StaticCall` fill the payload exactly. A field added to either spills it to a side table, not the
node to 40 bytes; the assertions below catch the day that happens. The old per-expression `NodeId` is the
`ExprId`, and `Drop for Expr` with its stack guard is gone: no node owns anything.

### I1.3 Statements (26 kinds)

| payload | arms |
|---|---|
| 4 | `Expression(e)`, `Return(Option<e>)`, `Throw(e)`, `FunctionDecl(FunctionId)`, `ClassDecl(ClassId)`, `Namespace(NameId)` |
| 8 | `InlineHtml(Text)`, `Echo(List<ExprId>)`, `Block(List<StmtId>)`, `Unset(List<ExprId>)`, `Global(List<GlobalNode>)`, `Use(List<UseNode>)`, `GlobalConst(List<ConstNode>)`, `StaticVar(List<StaticVarNode>)`, `While { cond, body }`, `DoWhile { body, cond }`, `Goto(IdentAt)`, `Label(IdentAt)` |
| 12 | `Switch { subject, List<CaseNode> }`, `Break(Option<Wide>)`, `Continue(Option<Wide>)` (a `Wide` has no niche, so the option is 12 bytes, not 9) |
| 16 | `Declare { List<DirectiveNode>, OptList<StmtId> }`, `Foreach { subject, Option<key>, value, body }` with `by_ref` in the tag word |
| 20 | `If { cond, then, List<ElseIfNode>, Option<else> }` |
| 24 | `Try { List<StmtId>, List<CatchNode>, OptList<StmtId> }` |
| 28 | `For { List<ExprId>, List<ExprId>, List<ExprId>, body }` |

A statement carries no doc fields (`doc_comment` and `doc_line` go to the doc table, I1.5) and no `Box`. `If`'s
`else if` is an `else` whose statement is another `If`, as now. `ElseIf` has no id: `ElseIf.id` is built by the
parser and read by nothing.

### I1.4 Records and side tables

A record is stored by value in its vector and has a position, not an id, when it is reached only through a list or
a single owning field. A node that others refer to by id, or that is large, has its own vector and a typed id.

| vector | element | bytes | notes |
|---|---|---|---|
| `args` | `ArgNode { value, name: Option<NameId>, name_at \| spread }` | 12 | `spread` is bit 31 of `name_at`; offsets are below 2^28 (the loader refuses a larger source, B5) |
| `items` | `ItemNode { key: Option<e>, value \| flags }` | 8 | `spread` and `by_ref` in the top two bits of `value` |
| `destructure` | `DestructureNode { key: Option<e>, target \| by_ref }` | 8 | `target` absent for a skipped slot |
| `arms` | `ArmNode { conditions: List<ExprId>, body, span }` | 20 | an empty list is `default` (the parser never builds `Some(vec![])`) |
| `elseifs` | `ElseIfNode { cond, body, span }` | 16 | |
| `cases` | `CaseNode { test: Option<e>, body: List<StmtId>, span }` | 20 | |
| `catches` | `CatchNode { types: List<QNameId>, var: Option<IdentAt>, body, span }` | 32 | |
| `directives`, `globals`, `static_vars` | `{ name, at, value or init }` | 12 | |
| `uses` | `UseNode { path, written: (NameId, Span), alias: Option<IdentAt>, kind }` | 28 | `written` is a full `Span`, not an `IdentAt`: a `use` path's span runs from the first token to the previous end and can hold spaces (I1.1) |
| `qnames` | `QNameNode { written, kind \| resolved, span }` | 16 | `kind` (unresolved, exact, with global fallback) in the top bits of the resolved `NameId` |
| `hints` | `HintNode { span, Keyword(native, case_mask) \| Name(QNameId) \| Nullable(HintId) \| Union(List) \| Intersection(List) }` | 20 | a keyword's written spelling is its canonical word plus a 16-bit case mask (`INT`), not a copy |
| `attrs` | `AttrNode { name: QNameId, args: List, span, removal_span }` | 28 | |
| `goto_labels` | `{ name, backward }` | 8 | |
| `params` | `ParamNode { name, name_at, type_hint, default, hooks, attrs: List, span, flags \| promotion }` | 40 | the promotion (two visibilities, `readonly`, `final`) packs into 16 bits |
| `functions` | `FunctionNode { name, at, namespaced: Option<NameId>, params: List, return_type, body: BodyNode, attrs, span, flags }` | 80 | `BodyNode { open, stmts: List<StmtId>, goto_labels: List, flags }` is inline; a bodiless declaration clears `has_body`, a skimmed one sets `skimmed` and keeps no statements |
| `closures` | `ClosureNode { params, uses: List<ClosureUseNode>, body: List<StmtId>, goto_labels, return_type, attrs, flags }` | 48 | `ClosureUseNode { name, at, by_ref }` 12 |
| `arrow_fns` | `ArrowFnNode { params, body: ExprId, return_type, attrs, flags }` | 28 | |
| `classes` | `ClassNode { name: Option<NameId>, at, namespaced, kind \| flags, backing_type, extends, implements, properties, methods, consts, trait_uses, enum_cases, attrs, args: List<ArgNode>, span }` | 112 | one vector for a declaration and an anonymous class (no name, with `args`), as `ClassDef` is shared today; the member lists are contiguous ranges (B3) |
| `properties` | `PropertyNode { name, at, flags, type_hint, default, hooks, attrs, span, removal_span }` | 56 | |
| `methods` | `MethodNode { function: FunctionId, flags, span, removal_span }` | 28 | the method's `FunctionNode` lives in `functions`, as `MethodDecl.function` does |
| `consts` | `ConstNode { name, at, namespaced, value, flags, type_hint, attrs, span, removal_span }` | 52 | a class constant and a global `const` share it |
| `trait_uses`, `aliases`, `precedences` | `TraitUseNode { traits, aliases, precedences, span }` 32, `AliasNode` 16, `PrecedenceNode` 16 | | |
| `enum_cases` | `EnumCaseNode { name, at, value, attrs, span }` | 28 | |
| `hooks`, `hook_bodies` | `HooksNode { get, set: Option<HookId> }`; `HookNode { body: Arrow(e) \| Block(List) \| Abstract, goto_labels, param: List<ParamNode> (0 or 1), attrs, flags }` | 8, 40 | breaks `Param` / `PropertyHooks` recursion by ids, not `Box` |

Declaration nodes reserve their id on entry and fill it on exit (B3), so `functions` and `classes` are in source
order of their start, nested ones included.

### I1.5 Sparse data

- **`docs: Vec<DocEntry { owner, text: Text, line: u32 }>`** (20 bytes), sorted by `owner = (kind, index)` and found
  by binary search. It replaces eight fields: `Stmt.doc_comment` and `doc_line`, and the `doc_comment` of a
  function, class, property, constant, trait use, parameter and hook. About 72 thousand doc comments are
  allocated over a run (72,171: the 20 G report's M10) against millions of nodes; the table is a fraction of the
  `Option<Arc<str>>` slots it removes. Its owner
  kinds also include the closure and the arrow function, which have no doc today: the Stage B review's C2 (a
  closure's docblock is ignored, so its `@param`, `@return` and `@template` are not read) is then a parser change
  that attaches the doc, and not a relayout. SYN-3.4 turns `line` into the comment's offset, as DOC-2.9 finds a doc
  block by it; the sizes are equal. `line` has an explicit "none" value until SYN-3.4 keeps `first_start` through
  the attribute re-key (an attribute-prefixed declaration has a doc and no line today). Each owner kind's tag or
  flags word carries a `has_doc` bit, so the binary search runs only for an owner that has a doc: the checker
  starts four statement-level checks with `let Some(doc) = stmt.doc_comment.as_deref() else { return }`, which
  is a field read today and would otherwise be a search per statement.
- `names: Vec<Name>`, the `source` and its two side buffers (B3), `goto_labels`, and the parse's `errors` and `marks` (G3).

### I1.6 What a reader sees

B4's views: `Expr<'a>::kind()` folds the arms back (`PropertyAccess` and its nullsafe arm are one `ExprKind`
variant with an `ObjectOperator`), builds `Ident<'a>` from an `IdentAt`, and returns the list views. Struct views
(`Param<'a>`, `Arg<'a>`, `ClassConst<'a>`, `TypeHint<'a>`) are built from the records when a list is iterated.
`Arg<'a>` carries `name: Option<Ident<'a>>` and `spread`; `ArrayItem<'a>` carries `key`, `value`, `spread`,
`by_ref`. None of the packed bits is visible outside `syntax`.

### I1.7 Assertions, and what the census can change

- `const` assertions: `size_of::<ExprNode>() == 32`, `StmtNode == 40`, every record at or below the size in I1.4,
  `Option<Id>` 4 bytes, no vector element `needs_drop`.
- **The census (`xtask tree-stats`, step 0) answers six questions:** (1) are named arguments rare enough that
  `name_at` belongs in a side table (an `ArgNode` of 8 bytes); (2) is `ParamNode`'s empty `attrs` list (8 bytes in
  nearly every parameter) better as a flag and a side table; (3) is `QNameNode` worth inlining for the commonest
  parents (`ClassConstFetch`, `New` of a name); (4) the doc table's density against a per-kind sparse field;
  (5) the distribution of list lengths, which sizes nothing here (`List` is 8 bytes) but sizes the scratch stacks;
  (6) the tokens-per-node ratio and the kept tree's bytes under the ruled layout (the source it owns is priced
  here), which spike S-12 needs to decide spans against token indices.
- A change is accepted when the asserted sizes fall (or stay) and step 2's dump stays byte-equal. It is never
  accepted by weakening an assertion.

