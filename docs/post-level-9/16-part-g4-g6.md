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
