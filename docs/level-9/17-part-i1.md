## I1. Node layout per variant (Part F 5)

<!-- status:begin -->

## Status of this part's tasks

From the ledger at `54abfd547` on 2026-10-19. Regenerate with `scripts/plan_status.py`; change the ledger, not this table. `also in` lists the other parts that name the task.

| task | status | ledger | also in |
|---|---|---|---|
| SYN-3.4 | OPEN | OPEN | 05 10 11 12 13 18 |
| SYN-4.8 | READY | READY | 05 07 09 10 11 12 |
| DOC-2.9 | OPEN | OPEN; overlaps DOC-2.6 | 05 06 07 10 11 12 13 19 |

<!-- status:end -->

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
