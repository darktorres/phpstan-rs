# Part B. Item 9: the flat tree (SYN-1.2 to 1.10)

## Tasks this part owns

Each task is listed in the B7 step that builds it. The state is the ledger's, as of `a3931d49d`. SYN-1.11 and SYN-1.12 are not in the ledger yet: B11 adds them.

| id | task | state | step |
|---|---|---|---|
| SYN-1.11 | prerequisites on the old tree: `xtask tree-stats`, the fabrication sites | new (B11), not in the ledger | 0 |
| SYN-1.3 | nodes as ids: `Expr` and `Stmt` become ids, and the readers read through the tree | OPEN | 1 |
| SYN-1.12 | the read API: handles, views, lists, `Visitor`, dump, `Ast` container | new (B11), not in the ledger | 1 |
| SYN-1.2 | the tree: the `Ast` owns its source, one typed vector per node kind, a child is a `u32` id | READY | 2 |
| SYN-1.4 | payloads are spans (doc comments, unescaped literals, names, bodies) | OPEN | 2 |
| SYN-1.5 | nothing in a tree needs a drop of its own; names live in the file's name table | OPEN | 2 |
| SYN-1.7 | no growth inside the tree: children are gathered in scratch stacks, appended once at their exact size | OPEN | 2 |
| SYN-1.8 | nodes the size of their variant; a large variant's payload goes in a side table | OPEN | 2 |

Three of item 9's rows are scheduled by Part C, not here, so each has one owner. SYN-1.9 (step 4) is in Part C's C1 table, batch 3. SYN-1.10 (step 5) is in Part C's C3, batch 9, because it opens when PROD-5.1 and STUB-7.3 land. LOAD-3.5 (step 3) is in Part C's C3, because it opens when SYN-1.2 lands. SYN-1.6 is SUPERSEDED by SYN-1.2 (decision 4), so no part owns work for it.

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
- **Readers.** 134 files and 2,452 lines name `ExprKind::`/`StmtKind::` (the ledger counts 2,612
  sites in 135 files); 486 mentions of `&Expr`/`&Stmt`/`&[Expr]`/`&[Stmt]` (mostly
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
  indices is settled by a measurement" (decision 4's last sentence and SYN-1.2's text: it
  stands, and spike S-12 is the measurement, made before step 2). The costs it names: the
  match sites read through the tree, and each access has a bounds check. (The workspace denies
  `unsafe_code`, which is why the 20 G plan's `bumpalo` arena, not `Sync`, was dropped.)

## B2. Decisions before step 1 (the leans of this part; #1 to 9 are the current answers)

| # | question | recommendation | why | the alternative |
|---|---|---|---|---|
| D1 | what consumers hold | **`Copy` handles and plain-field views over the stored ids** (`Expr<'a>`, `Stmt<'a>`, `ExprKind<'a>`, `Param<'a>`, list views), carried through the checker's walks, with an expression that must be stored held as a `Rooted<Expr>` (an `Arc<Ast>` and a raw id), so no checker type carries a tree lifetime and a bare id is only a cache key (G1) | the 486 signatures and 2,452 sites change mechanically (`&Expr` → `Expr<'_>`, `.kind` → `.kind()`, `Box` and `clone` vanish); struct destructuring (`let ClosureExpr { params, .. } = …`, 16 sites) survives; a handle cannot outlive its tree where it is borrowed, and a stored one is owned with its tree, which is the decision's "ids live only with their tree" in the type | ids + `Index<ExprId>` on `Ast` (la-arena style): fewer types, but every free function gains an `ast` parameter, and nothing stops an id leaking into a product |
| D2 | how to migrate | **staged**: step 1 moves every reader to the handle API over the *existing* boxed tree, step 2 swaps the storage behind it | each commit stays green; step 1 is a behaviour-preserving refactor, step 2 is local to `syntax`. **What the split buys is two oracles:** step 1's is the compiler and the readers' tests over about 130 files, step 2's is the canonical dump compared with the baseline taken at the end of step 1 (the boxed tree's), where one change set would have neither for either half. **What is end state and what is scaffolding:** step 1's API (handles, views, list views, `Visitor<'a>`, the dump, the `Ast` container) is the final one and survives; thrown away are the adapter that implements the views over the boxed nodes (step 2 writes the same signatures over the store), the old `Visitor` trait (deleted at the end of step 1, so it lives one step), the legacy-node bridges (B4) and the serial field the legacy nodes carry. The throwaway part is estimated at 1,000 to 1,500 lines, from the sizes of `walk_expr` (181 lines) and `walk_stmt` (120), the 47 + 26 arms of the two `kind()` mappings, about 25 view constructors and 15 list views (not the 400 of the first estimate). The rule that refuses interim work (#35 and 41) is met by the end-state argument: nothing here is a structure the engine deletes, and it is the way the readers reach the final API with an oracle at each half | one change set that flips the type and all 134 files together: no intermediate commit compiles, and the dump baseline would have to be written once against the old nodes and once against the store |
| D3 | what a node records of position | **decision 4's last sentence stands: a measurement settles it** (spike S-12, before step 2). The expectation, and the working assumption of B3 to B5 and I1, is **byte spans** (`Span`, 8 B), as now | token indices would tie every kept tree to its token stream, which the workers reuse as scratch, so a kept tree would keep its own tokens (about 5 B a token, kind plus start offset: about 24 MB for Piwigo's analysed files at ≈4 source bytes a token). Break-even is about 20 B saved per node (≈4 tokens a node on the plan's dhat counts), more than an expression node can shed (32 B against a Zig-style 16 B). Those are estimates, which is why S-12 builds both layouts for the corpus and measures exact bytes and the readers' hot paths. No reader needs trivia between tokens | token indices (a node reads its identifier, string, number or operator from its token, as Zig's do); chosen only if S-12 shows it wins, and then B3's `span` fields become token indices and a kept tree keeps its token arrays |
| D4 | where payload bytes live | **the ledger's layout, with no amendment: the tree owns its source, and a `Text { start, len }` is a range of that source or of a side buffer** (two bits of `len` choose). A plain string, a doc comment and inline HTML are spans of the source: the parser already reads `&src[span]` when it takes a token, so no payload is copied. The side buffers hold the bytes no source has: a `String` for UTF-8 text (a folded doc group, the text the passes and the probe append, the lossy decode of a payload of a source that is not valid UTF-8) and a `Vec<u8>` for string-literal bytes that needed decoding (escapes, heredoc indentation). A source valid as UTF-8 (checked once at load) is held as a `str` where the buffer allows it, so a `&str` view of a doc comment or inline HTML is a boundary-checked slice; where the builder holds the worker's byte buffer instead, a view costs one small-range validation, which the census prices | no payload copies at all (72 K docs and 352 K single-quoted literals allocate today); the tree owns the bytes its spans index, so a span always resolves; a context tree needs no copy of the source, since the builder takes the worker's read buffer by swap and hands it back on `clear`; the lexer's `PhpString`/`Arc<str>` payloads go with SYN-2 | the tree's own text arenas, a copy of every payload in a `String` and a `Vec<u8>` (the plan's first answer): kept trees about 5.6 MB smaller at Piwigo scale, at the price of a copy of every payload and a second kind of text; rejected |
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
| `checker/expr.rs` `static_call`/`constant_fetch` | the `StaticCall`/`ClassConstFetch` arms' bodies are methods taking the target, the method or name, the args and the span (`check_static_call`, `infer_static_call`, `check_static_constant_fetch`, `infer_static_constant_fetch`); the computed-name paths call them, and the forget effect is `static_call_writes` with `forget_call_members`. Two behaviours change: external rules see only the source's `DynamicStaticCall` and `DynamicClassConstFetch` nodes, not a synthetic `StaticCall` or `ClassConstFetch`; and a computed static call throws what its method declares, so a `catch` that cannot receive that type is not reached from it | 0 |
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

**The item's validation, once, at the end of step 3** (#24; no suite per step): `cargo test
--workspace` and the fixtures; `xtask dumps-compare` against the item-0 baseline for the expression types (the whole
checker moved to handles), the registry dump (unchanged by the item) and O2, and for the commits that touch `lsp`
(the probe tail, the `Arc<Ast>` holders) the server session and the edit replay; the step 2 dump equality above; the
Piwigo oracle off and warm; one thread = default threads; callgrind off and warm; dhat (trees at exit, allocation
count); wall by `abab` pairs (B8); peak RSS.

Step 1's module order (any order works, because the bridge functions are free; dependencies first leaves the fewest bridge calls): `syntax` internals (`printer`, `declarations`,
`name_resolution` readers, and `completion`, the statement-completion rules that the cross-platform plan's R18 moves into `syntax`) → registry (`summaries`, `init_flow`, `purity`, `property_writes`,
`global_vars`, `guard`, `bodies`, `function_sig`, attributes) → project passes (`banned`,
`mock_hygiene`, `architecture`, `class_shape`, `composer_hygiene`, `use_sites`, `deps`,
`file_declarations`) → checker (`expr.rs` alone in several commits; `narrowing`, `stmt`,
`assignment`, `noop`, the rest) → `lsp` → `composer/vendor.rs`, `xtask`, `fuzz`, benches → parser
tests. During step 1 `VisitorMut` and the two builder passes (`stub_attributes::Pass`, `Namer`; the `Resolver` is
not ported, B6) keep the old node types through a `legacy_mut()` on `Ast`. A codemod for the two commonest rewrites
(`.kind` → `.kind()`, `&Expr` → `Expr<'_>`) is used, by span-based text edits and never by reprinting a file
(#27); it is throwaway.

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

1. **Stored expressions are rooted, not borrowed.** A check lifetime on `Locals` (415
   lines in 66 files) would be removed again by Stage D, so G1 keeps `Locals` and `Checker` free of a
   tree lifetime and roots the few stored expressions instead: an `Arc` count increment per stored alias
   where the deep clone was, and in step 1 a legacy form of `Rooted` that holds the cloned node (deleted
   with the bridges in step 2).
2. **The adapter is throwaway** (1,000 to 1,500 lines, D2): accepted because step 1 and step 2 each get an oracle
   (the compiler and the readers' tests, then the dump baseline), and because its API is the final one.
3. **`kind()` cost in hot loops.** A view is a match and a few copies (an estimated 10-20 Ir). Mitigation:
   `#[inline]`, call it once per node, helpers for the commonest projections (`variable_name()`,
   `string_literal()`, `is_null_literal()`); callgrind at step 2 decides.
4. **Owned string values.** The checker builds `Type::string_literal` from literals 13 times outside tests (18 with
   tests; 46 is the count of `Literal::String` mentions, which are not builds);
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
  that the front-end tree dump is delivered by step 1. **No ruled wording of decision 4 changes** (both stand): the layout bullet ("a file's `Ast` owns its source ... a string is a span, plus a side
  buffer where it needs decoding"), SYN-1.2's "owns its source" and SYN-1.4's "payloads are spans" are what B3 builds,
  and the last sentence ("whether nodes keep spans or token indices is settled by a measurement", SYN-1.2's "settled
  first, by a measurement made with SYN-2's design") is spike S-12, run before step 2. SYN-3.4's anonymous-class
  bullet stays: the name is still positional, as types and messages show it, and only the identity is the
  ordinal (H1.4).
- The ledger's `needs` change with D8: SYN-1.3 needs SYN-1.12 and not SYN-1.2 (ledger SYN-1.3, 835), and SYN-1.2, 1.4,
  1.5, 1.7 and 1.8 need SYN-1.3 (SYN-1.8 already needs it); Part N lists every ledger and stack edit the plan implies.
- `WORK_STACK.md` level 8, item 9: "SYN-1.11, then 1.12 (+1.3), then 1.2/1.4/1.5/1.7/1.8; this design".
- Write Part B, I1 and the item-9 parts of G1 to G4 (G1's lifetimes, G2's stamp, G3.3's layout, G4a's probe tail) into
  the repository as `FLAT_TREE_PLAN.md` (the item is too large for the plan's item text) when this plan is approved; Part N
  says where every other part goes.

## Decisions

### #1 D1: what consumers hold

**`Copy` handles and views for every walk; a stored expression is a rooted `(Arc<Ast>, id)`.** The tree's lifetime on `Locals` would be 415 mentions in 66 files, `Rc<Expr>` in 11 places. Stage D removes the tree from `Locals` altogether (conditional holders replace alias replay), so that lifetime would be built in item 9 and unbuilt then, which the no-interim rule (35, 41) refuses. So `Locals` and `Checker` take no tree lifetime: the two alias maps and three checker fields hold `Rooted<Expr>`, which is decision 4's "an id lives only in products owned with its tree" in the type, and a bare id is only ever a cache key (G1)

*Needed before:* item 9, step 0

### #2 D2: how to migrate

**Staged,** with the baseline dump taken at the end of step 1, so step 2 is checked byte for byte against the boxed tree's dump; the scaffolding it costs (1,000 to 1,500 lines, D2's row in B2) is argued against the no-interim rule there

*Needed before:* step 0

### #3 D3: position

**Decision 4's last sentence stands: a measurement settles it.** Spike S-12, before step 2, builds both layouts for the corpus: the exact bytes of a kept tree with byte spans against token indices, and the readers' hot paths. The expectation is **byte spans**, which B3 to B5 and I1 assume: a token-indexed tree must keep its tokens (about 5 B a token), which breaks even only at about 20 B saved per node, more than an expression node can shed

*Needed before:* step 0 (the census counts tokens); S-12 before step 2

### #4 D4: payload bytes

**The ledger's layout, with no amendment: the tree owns its source, a string is a span of it, and a side buffer holds the bytes no source has** (decoded strings, folded and synthesized text, the lossy decode of a source that is not valid UTF-8). A `Text` is a range of the source or of a side buffer, two bits of the length choosing. It copies no payload, a span always resolves because the tree owns what it indexes, and a context tree copies no source (the builder takes the worker's read buffer by swap). The arena design (a copy of every payload; kept trees 5.6 MB smaller at Piwigo scale) needed the side buffer too, for the bytes no source has, so it saved one thing and cost a copy of every payload and a second kind of text

*Needed before:* step 0

### #5 D5: names in nodes

**`NameId`** into a per-tree table: one refcount per distinct name, and nodes that are plain data

*Needed before:* step 2

### #6 D6: the mutating passes

**On the builder** until SYN-1.10 deletes the passes and `into_builder` with them; `Ast` never gets a `&mut`

*Needed before:* step 2

### #7 D7: fabricated nodes

**Remove them first:** a tree that cannot be built by hand makes the sites call functions on parts

*Needed before:* step 0

### #8 D8: rows

**Regroup** (SYN-1.3 is step 1; SYN-1.2, 1.4, 1.5, 1.7, 1.8 are step 2; new SYN-1.11 and 1.12)

*Needed before:* step 0

### #9 D9: error nodes

**Not in step 1.** Design the error-node model (what an error node keeps, such as salvaged children, and that it reads as unknown) with SYN-1.9, before step 2 fixes the layout, and add the variants there, so each has a producer, a meaning and tests. A `_ =>` arm for a variant nothing produces decides its meaning by accident

*Needed before:* step 2 (design), batch 3 (variants)

### #24 item 9's validation

**Once, at the end of the item (step 3), at the boundary the user names: the end of each top-level item.** The syntax dump baseline is produced at the end of step 1 (a cheap run that stores output, not a suite) and is compared with step 2's dump at the item's end; the item-0 baseline (expression types, registry, O2), taken from validated code before the item, covers step 1's readers and is compared then too. The suites, the fixtures, the Piwigo oracle and the measurements run once, then (B7).

*Needed before:* step 1

### #27 the step 1 codemod

**Use it, by span-based text edits;** never reprint a file (a `syn` printer drops comments), with `cargo check` as the oracle, one module at a time

*Needed before:* step 1

## Facts the decisions rest on

- `FoldedName` and `ConstName` are not in the code. `Locals` has 415 mentions in 66 files and
  `Rc<Expr>` sits in 11 places. `wide` comes in through `imbl`.

---
