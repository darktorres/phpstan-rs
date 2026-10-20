# Part C. What follows item 9

## Tasks this part owns

Each task is listed in the C1 batch that builds it. The state is the ledger's, as of `a3931d49d`.

| task | state | batch |
|---|---|---|
| SYN-2 (the token table, I2) | OPEN | 1 |
| SYN-4.1 | OPEN | 1 |
| SYN-4.8 | READY | 1 |
| SYN-4.9 | READY | 1 |
| SYN-3.1 | READY | 2 |
| SYN-3.2 | READY | 2 |
| SYN-3.3 | READY | 2 |
| SYN-3.4 | OPEN | 2 |
| SYN-4.2 | READY | 2 |
| SYN-4.3 | OPEN | 2 |
| SYN-4.4 | READY | 2 |
| SYN-4.6 | READY | 2 |
| SYN-4.7 | OPEN | 2 |
| SYN-4.10 | READY | 2 |
| SYN-1.9 | OPEN | 3 |
| Q-10 | OPEN | 3 |
| SYN-6 | OPEN | 3 |
| NAME-2.1 | READY | 4 |
| NAME-2.2 | READY | 4 |
| NAME-2.3 | READY | 4 |
| NAME-1.1 | READY | 4 |
| NAME-1.2 | READY | 4 |
| NAME-1.3 | OPEN | 4 |
| NAME-1.4 | OPEN | 4 |
| NAME-1.5 | READY | 4 |
| NAME-1.6 | READY | 4 |
| SYN-4.5 | READY | 4 |
| REG-18.4 | READY | 4b |
| CHECK-4.2 | READY | 4b |
| PROD-5.8 | READY | 5 |
| PROD-5.2 | READY | 5 |
| PROD-5.1 | OPEN | 5 |
| PROD-5.3 | READY | 5 |
| PROD-5.5 | READY | 5 |
| PROD-5.6 | READY | 5 |
| REG-14.1 | OPEN | 5 |
| CHECK-13 | OPEN | 5 |
| DOC-2.1 | READY (deferred in the ledger; #14 undefers it) | 6 |
| DOC-2.2 | OPEN | 6 |
| DOC-2.3 | READY | 6 |
| DOC-2.4 | READY | 6 |
| DOC-2.5 | OPEN | 6 |
| DOC-2.6 | OPEN | 6 |
| DOC-2.7 | READY | 6 |
| DOC-2.8 | READY | 6 |
| DOC-2.9 | OPEN | 6 |
| REG-4.10 | READY | 6 (C1 folds it into DOC-2.9; the ledger keeps it READY and overlapping DOC-2.6, to reconcile) |
| TYPE-5 | READY | 6 |
| CHECK-6.4 | READY | 7 |
| CHECK-6.5 | READY | 7 |
| CHECK-6.6 | READY | 7 |
| CHECK-6.8 | READY | 7 |
| CHECK-6.9 | READY | 7 |
| CHECK-6.10 | READY | 7 |
| CHECK-6.11 | READY | 7 |
| CHECK-6.12 | READY | 7 |
| CHECK-6.16 | READY | 7 |
| CHECK-6.17 | READY | 7 |
| CHECK-6.18 | READY | 7 |

Batch 0 is the README's, item 9 is Part B's and Part I's, and the batch 8 rows are other parts' (LOAD-5.4 is Part A's; CACHE-13.1, CACHE-8, LOAD-5.1, LOAD-5.2 and LOAD-3.1 are W3's). They are named in the text, not listed here.


Read from the 20 G plan's items 10 to 19 (its §10), the ledger's rows, needs and notes
(`PROFILE_DRILL_PIWIGO_LEDGER.md` §2 to §13) and the code at `0fb1a4de`. The savings are the 20 G
report's estimates for the off run (the ledger's "now" is 20.37 G at `8a1ca1e0`; 20.57 G is the 20 G profile's
`cl-20g`, file cache off with a warm stub cache); PROD-1 to PROD-3 and DOC-1 overtook parts of
items 13 and 14, so none is a claim. Validation is at the end of each item.

## C1. The path, in order (the order of the README's "The sequence")

1. The cross-platform fixes (finished at `a83b1675`); then Part A's rows
   (A1 to A3, A5, A5a, A7 and A8).
2. Batch 0 (#13 and 22: the oracle tools and the baseline, F10's tests, F20, F21, F77, the level-3 backlog that
   changes a dump, and the seven fixes the crate split's F1 puts first), then item 9: steps 0, 1, 2, then step 3
   (LOAD-3.5, the scratch builder, which is item 3's row). Steps 4 and 5 (SYN-1.9, SYN-1.10) come later (batch 3, and
   C4).
3. The batches below.

| # | batch | rows | estimate | why here |
|---|---|---|---|---|
| 1 | tokens | item 10: SYN-2 (I2), with the three rows its token table already uses: SYN-4.1 (the watch table), SYN-4.8 (`QualifiedName`) and SYN-4.9 (digit separators), which have no needs | ≈1.7 G | the largest single row; needs item 9; designed with the tree (the parser's `take` calls the builder's `name` and `text_span`); item 9's baseline serves it; the three rows go here so the lexer and the skim are not written over `Identifier Backslash Identifier` and again over one token |
| 2 | the lexer, second pass | items 11 and 12: SYN-3.1-3.4 (I3 for 3.2), SYN-4.2-4.10 less 4.5, 4.8 and 4.9 | ≈0.2 G + ≈0.35 G | same files as batch 1; SYN-4.1 shrank (the lexer interns nothing: I2) and is in batch 1; 4.3 needs SYN-3.2, 4.7 needs 4.2 |
| 3 | parse recovery (in: #11; changes output) | SYN-1.9, Q-10's parser half, SYN-6 (G3); the `php-version` crate with `VersionSet` and `Availability` (#32); F77's diagnostics now come from the tree's `errors()` | none (VAL-2.10 deltas) | after batch 1, so the parse functions are rewritten once; Q-10's version half needs STUB-7.1 (level 2) |
| 4 | names | S-1 first; item 16 (NAME-2.1-2.3), **then** item 15 (NAME-1.1-1.6), and SYN-4.5 (`ConstName`) | ≈0.1 G + ≈0.6 G; ≈15 MB | names first (#12): products and PHPDoc work are born on `FoldedName` and `ConstName`; NAME-1.4 needs NAME-2.1, so the plan's number order has the needing item first |
| 4b | the constant lookup | REG-18.4 with CHECK-4.2 (needs `ConstName`; a delta) | CHECK-4.2 is 5.1 M Ir | one place for the two rows: after batch 4, before batch 5 |
| 5 | a file's products | item 13: PROD-5.8, 5.2, 5.1, 5.3, 5.5, 5.6 (PROD-5.7 is not built: #35), REG-14.1, and CHECK-13 (its rule sees anonymous classes once PROD-5.1 gives them a file-local identity) | ≈0.45 G | three returns to earlier items open here (C3); PROD-5.4 is REG-16.4's to delete; PROD-5.8's record is keyed by `FoldedName` and `ConstName` (batch 4's types), and `RefKey` goes with the dependents machinery in W3; `Recorder::published` is the seed of PROD-5.2 |
| 6 | PHPDoc | item 14: DOC-2.1 first (F41: #14), then 2.2 to 2.9, with REG-4.10 in 2.9; TYPE-5 | ≈0.35 G; the memos' memory | 2.2 needs PROD-5.2 (batch 5), 2.5 needs NAME-1 (batch 4), 2.6 needs 2.1 and 2.3, 2.9 needs the comment's offset (SYN-3.4, batch 2); item 14 closes |
| 7 | the check's temporaries | item 18's temporaries (the `Message` of I5, CHECK-6.4 to 6.6, 6.9 to 6.12, 6.16, 6.17) and the scratch reuse of CHECK-6.8 and 6.18 (allocations that hold no tree handle) | the temporaries' share of ≈1.6-2.0 G | the inference core and the scopes group are Stage D's (#16): their ≈1.0-1.3 G and ≈0.15-0.25 G arrive with it; F69 and CHECK-9 are D2's (#17), CHECK-4.2 is batch 4b's, and the one-checker-per-worker form of CHECK-6.18 is E7's |
| 8 | nothing is left that is not the wall's | LOAD-5.4 moved to Part A (A5a); CACHE-13.1 is W3's, built once as `File.stamp` (#15); CACHE-8 waits for S-4 (#41); LOAD-5.1's six passes' per-file facts are W3's `file_products` fields, and E1 moves their collection onto D6's API (#39); LOAD-5.2 and LOAD-3.1 are W3's (LOAD-5.2 needs both LOAD-5.1 and LOAD-3.1) | – | the decision point of C4 comes after batch 7 |
| 9 | decision point | – | – | level 8's READY list is empty; what is left is levels 1 to 7 (C4, and Part D) |

The table's per-batch estimates add to about 3.75 G before batch 7's share (1.7 + 0.2 + 0.35 + 0.1 + 0.6 + 0.45 +
0.35); the 20 G plan's own sum for items 10 to 18, about 5.5 G (tokens and the check 3.5 G of it), counted the
inference core and PROD-5.7, which #16 and 35 moved or removed. Item 9's adds the rest of its 0.8-1.2 G,
itself in doubt (B8).

## C2. Notes by batch

**1. Tokens (item 10).** `TokenStream { kinds, spans, extras, docs, parts }` (I2: a token is a kind and a span, and
its text is read at the span), three cursors, per-worker buffers, interpolation fragments from a stack of scratch
streams; `Token`, `Tok`, `Supply`, `next_raw`/`next_tok`, the `VecDeque` and `fill`/`pop` go. Readers outside
`syntax`: `stub_attributes.rs` (`parse_native_type`), `output/codeframe.rs`, `lsp/completion.rs`, and the
`fuzz` targets `lex_parse` and `token_stream_parse`. Oracle: item 9's dump, which includes the
token list. Decision 4 has the token arrays designed with the tree; the only thing batch 1 needs
from item 9 is `b.name(&src[span])` and `b.text_span(span)`, which the parser calls at take time (the lexer never touches the
builder: I2) and B5 provides. The batch includes SYN-4.1's watch table, SYN-4.8 (`QualifiedName`) and SYN-4.9 (digit
separators), which I2's table already uses and no need holds back; the watch bits are set per segment of a qualified
name (`FileFacts::mentions` is set for "an identifier, a whole name or one segment of a qualified one"), and the body
skim has a stated reading for a `QualifiedName`, a leading backslash and a `namespace\` prefix, each with a test.

**2. Lexer, second pass (items 11 and 12).**
- Item 11's SYN-3.4 changes the *meaning* of two fields item 9 lays out (`doc_line` becomes the
  doc's offset, `removal_span` becomes the member's bounds); the sizes are equal, so item 9 needs
  no change and either order works.
- SYN-3.3 (the server indexes context files on demand while the loaded hash still matches) is
  READY and is LSP-only. The on-demand index is needed regardless of the engine: SYN-3.1 gives a context file no
  column table, so the server's navigation into one needs it. Its *hash check* is a stopgap: on the engine a context
  file keeps no text (H2.3), and the check becomes a comparison of `File.stamp` (the `stat` tuple on the file systems
  where a change time is trusted, the content hash elsewhere), replaced at W3; keep the check small.
- Item 12's rows are independent micro-rows except 4.3 and 4.7 (their needs are in the table); 4.1, 4.8 and 4.9 are in
  batch 1 (note 1). SYN-4.8 is NAME-1's round trip for qualified names. SYN-4.1 shrinks: its first half, the
  identifier lookup inlined into the lexer's arm with the scratch table, is gone, because the lexer interns nothing
  (I2: the parser's `take` interns); its perfect-hash watch list stays, built by a `const fn` in the caller from the
  lists it composes (the cross-platform plan's R18 makes `WATCHED` the join of three layers' lists, `stub_attributes`,
  `checker::mentions` and `file_declarations`, so `syntax` cannot see the entries when it compiles, and a build script
  there could not either). SYN-4.5 (`RefKey::Constant` keeps namespace and name apart) is `ConstName`, which NAME-1.2
  introduces, and it is the `Constant` kind of PROD-5.8's record (`RefKey` itself goes with the dependents machinery
  in W3), so it is built in batch 4 and not as a stand-in in batch 2.
- Oracle: item 9's dump (positions, fix spans, ignore marks, anonymous-class names), the line-index
  tests over multi-byte and astral lines, the LSP tests.

**3. Parse recovery (in: #11).** SYN-1.9 turns `parse` into "a tree and its errors" with error
nodes (their variants arrive here, not in item 9's step 1: #9, G3.3); Q-10 is half built (a context file that
fails to parse keeps what it declares) and its version half needs the range from STUB-7.1; SYN-6
then fixes the parser gaps Q-2 probed (`$s{0}`, `(real)`, `(unset)`, `(void)` as an expression, a
stray control byte, `function exit`). Its output changes are VAL-2.10's.

**4. Names (items 16, 15).** NAME-2.1 changes `Name`'s representation to one allocation (a thin
dynamically sized value, which a crate such as `triomphe` provides, judged on merit; our own code
stays without `unsafe`). NAME-1.2 is the largest churn of the batch: every `String`-keyed map and every
identifier held as text, by cost order, ending with `TableKey for str` and `FoldKey for str`
deleted (NAME-1.3) so the compiler lists the rest; its registry entry points meet REG-4.2's
accessors and REG-8.6 (level 2). Its container-field keys are an approved delta (VAL-2.7). The machinery that
W2 and W3 delete (`ChangedNames`, `reset_scoped`, `closure_scope`'s frontier sets, the dependents indexes) holds
`String` keys too. Its keys are converted here, because they are a type change and NAME-1.3 cannot delete
`FoldKey for str` while a caller holds text; what is not built is a new structure for it (#35). Oracle:
O2, `--dump-types`, `--dump-expr-types` and `registry-dump` (the last two are built in batch 0: #13).

**5. Products (item 13).** Order inside: PROD-5.8 (the record of written names, also what
REG-14.1 needs; it extends the parser's `Recorder` but is keyed by `FoldedName` (class-likes, functions) and
`ConstName` (constants), batch 4's types, and not by `RefKey`, which the engine deletes with the dependents machinery)
and PROD-5.2 (`ImportScope`s) first; PROD-5.1 (the declaration vectors; item 9
already reserves declaration ids on entry, B3); PROD-5.3, 5.5, 5.6 (one walk, bodies as a sorted
slice, scratch builders); PROD-5.7 is not built (#35). PROD-5.5's slice is keyed by the
body's ordinal (H1.4's `occurrence` and the member's ordinal), not by its span (H1.5a): an edit above a body then
leaves every other entry equal. PROD-1 to PROD-3 delivered most of this item's premise, so its Ir is
smaller than the report's; its memory and what it unlocks are the case for it. Oracle: item 9's
dump, `registry-dump` (built in batch 0), O2.

**6. PHPDoc (item 14).** DOC-2.1 comes first, as F41 (#14, I4): the grammar's lowering replaces the text
grammar, and `DocType` is eager. Then the memo work (DOC-2.2, 2.4, 2.8), tag effects (2.3), borrowed tokens (2.5),
one vector of nodes per docblock (2.7), one product per doc text (2.6) and doc blocks as a per-file product found
by the comment's offset (2.9). DOC-2.9 needs REG-4.10 (the builder parses each doc comment once), and the two are one
change: the builder and the checker read a doc comment's block from the file's doc blocks, which is how the builder
comes to parse it once. So REG-4.10 is built with DOC-2.9 and does not wait for level 2, and item 14 closes whole.
The statement `@var` memo overlaps DOC-2.2, so it is taken after 2.2. TYPE-5 (F14) follows F41 in this batch, as its
own commit after I4.5 step 5: it is the ledger's rule (the array member takes `K` as `array<K, V>` does), built once
templates and aliases are marked, and F14's design text, which restricts `K` and drops the array member, is
superseded (I4.5 step 6).
Before 2.9 the blocks live where `FileState` keeps a file's products; the offsets, the sharing and the checker's
reads are what survive into `file_products.doc_blocks` (H2.3).

**7. The check (item 18).** The *inference core* (CHECK-6.1 observers and one inference per node, 6.2, 6.7, 6.15) and
the *scopes group* (CHECK-6.3 variable slots and persistent radix arrays, then 6.13 and 6.14) are Stage D's
(#16, J3): they are the structure of the crate split's D1 to D4, and building them now would write the
traversal twice. What this batch builds is exact, local and survives either way: the *temporaries* 6.4 (which bumps
a format), 6.5, 6.6, 6.9, 6.10 with CHECK-4.1 (the `Message` of I5), 6.11, 6.12, 6.16 and 6.17, and the scratch reuse
of CHECK-6.8 and 6.18. CHECK-4.2 goes with REG-18.4 in batch 4b (#41). F69 and CHECK-9 are D2's: F69 records
implicit calls' throw points on the recorders (`record_call_throw_point`, `current_function_throw_points`) that D2's
`StatementResult` replaces, so by the crate split's Appendix F ("a bug in code a later phase rewrites gets a fixture
now and its fix as that phase's behaviour commit") and the same rule, they get their fixtures in batch 0
and are built in D2, and CHECK-9 follows F69 there; CHECK-7's checker half goes to D2 and CHECK-10's operator
sites to D3 for the same reason. TYPE-5 is batch 6's.
CHECK-6.8 and 6.18 reuse allocations that hold no tree reference (the infer cache's slots, scratch vectors of types
and keys, message buffers) across functions and files; a table that holds a rooted expression (G1's alias maps) is
cleared at each file and its allocation not kept, so that it does not keep the previous file's tree alive. That is
all of 6.8 and 6.18 that batch 7 builds and it is final: the ledger's "one checker per worker, reset for each file"
is buildable under G1 (a `Checker` takes no tree lifetime), but its reset clears the per-file, per-function and
per-pass field sets that E7 enumerates, so it is E7's (J3).

**8. Nothing is left that is not the wall's.** The batch kept item 19 and the first half of item 17, and the
pass over them found each already belongs elsewhere. LOAD-5.4 (no reader gets an empty file: `FileState::ast` goes)
is READY with no need and removes the empty tree that item 9's API would otherwise carry, so it is Part A's A5a.
LOAD-5.1 turns each project-wide pass into per-file facts (architecture, banned constructs, class shapes, Composer
hygiene, mock hygiene, unmatched ignores): each pass needs CHECK-6.1 or PROD-5.3, class shapes and unmatched ignores
need PROD-5.1, and the use-site index as a per-file product needs REG-16.4; W3 builds the six passes' per-file facts
as `file_products` fields (H2.3 and H4.4 already read them, and the persisted whole-program queries need them), and E1
moves their collection onto D6's Collector API and adds its semantic fixes over those fields, so the facts' data
is built once (#39). LOAD-5.2 (free each analysed tree after its check, the memory win) needs both LOAD-5.1
(the facts, so no pass reads a tree after its check) and LOAD-3.1, which needs REG-16.4 (#28), so it is W3's and
follows S-7's verdict (H2.9). CACHE-13.1 (entries checked by `stat`) is the source of `File.stamp`, and MEAS-1 puts what it
replaces at ≈60-70 M Ir of reads plus their system calls, with the per-file decode (≈1,260 M) untouched: a
standalone index file would be deleted the day the persisted database holds the stamps, so it is built once, as the
stamp, at W3 (H4.7 step 5, #15). CACHE-8 is READY and independent: `Writer::finish`
(`registry/container.rs`) builds the cache file in a second `Vec` beside the body, so the cold run holds both at
its peak (−15.3 MB there and −31.4 MB allocated, per the heap report's H1). The writer serves the stub caches
(`stub-base`, `stub-base-phpstan`, `stub-data`, `stub-index`), which go with STUB-11. A memory row: no instruction
saving is claimed, and nothing changes on disk. It is built once the persistence's format is chosen (S-4, #41): the writer that survives is the one worth making right.

## C3. Returns to earlier items (each is unblocked inside a later batch)

| row | item | opens when |
|---|---|---|
| LOAD-3.5 | 3 | SYN-1.2 (item 9, step 2) |
| LOAD-4.7, file-references half | 5 | PROD-5.7, which is not built (#35); its reverse-index half goes with REG-16.4 |
| REG-14.1 | 8 | PROD-5.8 (batch 5); 14.3 still needs REG-8.3 |
| DOC-2.2 | 14 | PROD-5.2 (batch 5) |
| DOC-2.5 | 14 | NAME-1 (batch 4) |
| NAME-1.4 | 15 | NAME-2.1 (batch 4, first half) |
| CHECK-4.1 | – | CHECK-6.10 (batch 7) |
| CHECK-6.7, 6.15 | 18 | CHECK-6.1 (Stage D) |
| CHECK-6.13, 6.14 | 18 | CHECK-6.3 (Stage D) |
| LOAD-5.1 | 19 | PROD-5.3 and PROD-5.1 per pass; W3 builds the facts, E1 the collection (#39) |
| SYN-6 | – | SYN-1.9 and Q-10's parser half (batch 3; its version half waits for STUB-7.1) |
| SYN-1.10 | 9 | PROD-5.1 and STUB-7.3 (level 2) |

## C4. The wall: what needs levels 1 to 7

After batches 1 to 8, the 20 G plan's remaining rows all sit behind the crate split's C4/C5 and the
engine. They are: LOAD-3.1 and 3.3's `update_file` half; LOAD-4.7's index half; LOAD-5.2 and the
use-site index; item 4 (REG-4.15, 7.4, 8.3); item 6 (STUB-8, CACHE-9/10, REG-7.2); item 7
(TYPE-4); item 17's CACHE-13.2; NAME-2.4; PROD-5.4; and SYN-1.10 (DOC-2.6 and 2.9 are batch 6's; item 14
closes whole). The final
validation (VAL-1) follows item 19 and is blocked by nothing but its own tooling (C5, 4). The
roots of that chain are READY today: **REG-4.2** (exact-spelling accessors; folded
method keys) and **REG-16.2** (interned identity), then REG-16.3 (the engine), which gates REG-7,
TYPE-4, REG-16.4 and so LOAD-3.1, CACHE-13.2, LOAD-5.2 and REG-17. Levels 1 to 7 are deferred "to be reevaluated together"; level 8's READY list runs out at batch 8, so that is when.
Design of REG-16.2 and REG-16.3 can run while batches 5 to 7 are built. Part D lays out what the wall holds and the proposed order.

## H1. Identity (Part F 10)

None. H1 is a design, and its batch-4 build (H1.7) lists rows that Part C's C1 table already schedules: NAME-1.1 to 1.6, NAME-2.1 to 2.3 and SYN-4.5, all in batch 4. CHECK-13 is decided in H1.4 (anonymous classes reach the class-shape parent-call rules), and it is built in batch 5 with PROD-5.1, whose file-local anonymous-class identity its rule needs (Part C's C1 table lists it there).

### H1.0 What the code and the crate showed

1. **Built already:** one process-wide interner, `Name` equality by pointer, no `Borrow<str>`, `FoldedMap`
   with a stored folded hash (`syntax/src/name.rs`; 25 G C3's steps 1 to 3). 25 G puts the interner in `Project`;
   it is process-wide. **Not built:** `FoldedName`, `ConstName`, the one-allocation name
   (NAME-2.1), `Name::short`.
2. **REG-8.6 cannot be built as written.** "Snapshots own their names ... the project's interner ... `apply`
   sweeps the session interner" needs a second interner, and a second interner makes two allocations of one
   text, which `Name`'s pointer equality forbids (the module's own doc says so). H1.3 re-specifies it.
3. **`ArrayKey` exists:** `types::ShapeKey` (`shape.rs:14`) is PHP's array key, `Int(i64)` or `Str(PhpString)`,
   a decimal-int string canonicalized by `canonical_int_key`; 229 uses in 38 files (242 lines in 42 files with `phpdoc`'s own `ShapeKey`). 25 G's new `ArrayKey`
   would be a second type for it.
4. **25 G C3 and NAME-2.1 disagree** on where a name's lowercase form lives: a second interned `Name`
   (`NameData.folded`), or inline in the one allocation. H1.2 takes NAME-2.1.
5. **A position inside a value defeats early cutoff.** `Provenance { origin, span, declaring_class }`
   (`registry/provenance.rs`) derives `PartialEq` and sits on every class, function and member, so a class is a
   different value after a line is inserted above it. Today that makes `changed_since` call it changed and
   recheck its readers; on the engine it would invalidate every reader of every class below an edit.
6. **`Type`'s `PartialEq` is semantic and its interner is exact.** `repr_eq` ignores a class name's case and the
   explicit-`mixed` flag (`types/src/lib.rs:601-640`), while "a shared node never changes how a type prints"
   (`types/src/intern.rs`). salsa backdates on the result's `PartialEq` by two paths and both go stale on the
   semantic equality: a tracked struct's field that compares equal **keeps the old value**
   (`update_fields`), so a reader gets the *old* node, which prints the old way; a tracked function's result
   that compares equal is stored **as the new value with the old memo's `changed_at`**
   (`function/backdate.rs`), so dependents that were verified and not re-run keep results computed from the old
   input while a re-run reader sees the new memo, and two readers in one revision can see different
   spellings. A warm run then differs from a cold one either way.
7. **salsa's interned structs are collected and their slots reused** with a new generation (`interned.rs:93`;
   `REVISIONS` defaults to 3; `revisions = usize::MAX` turns collection off). Only a value interned under a
   `LOW`-durability query is reusable (`:244-257`; a value interned outside any query, or under a query that
   read only higher-durability inputs, is never collected), and the query that *interns* it records an edge to
   it (`:263-290`: creation records the edge, reading does not), so a reuse invalidates that query. A reused
   slot is checked on a read only by `assert_validated`, which fails for a slot not re-interned this revision,
   and by a `debug_assert_eq!` on the id ("refers to a reused slot", `:932`): in a release build a stale id
   of a slot re-interned this revision reads the new occupant's fields and memos. What keeps a collectable id
   honest is H1.6's rule that none outlives its query or leaves the engine, not salsa; `DeclKey` is immortal
   (#38), so it needs no such discipline.
8. **salsa's macros emit `unsafe impl` into the using crate** (`salsa-macros/src/db.rs:94`), and the
   workspace has `unsafe_code = "deny"` with `allow`s for PCRE and the allocator only.
9. **A write needs `&mut` on the database,** cancels every other handle's running queries by unwinding
   (`Cancelled::PendingWrite`) and waits until those handles are dropped (`database.rs:30-80`). The database
   is `Send`, not `Sync`: a handle per thread, made by `clone`.
10. **Persistence is a whole-database serde** (`#[salsa::input(persist)]`, `interned(persist)`,
    `tracked(persist)`; `<dyn Database>::as_serialize` / `deserialize`, `database.rs:166`). It needs `&mut`
    (it cancels other workers), restores into a later revision (`tests/persistence_stale_interned.rs`), and **a
    non-persisted dependency is replaced by the base-input leaves under it** (`function.rs:602-634`, `:880-896`):
    the nested walk descends through every edge, persisted tracked functions included (it never inserts such an
    edge), inserts only input fields and persisted collectable interned structs, and drops tracked-field edges;
    it asserts that every input field a persisted memo depends on is itself `persist`
    (`input/input_field.rs:68-82`). `deserialize` changes the live database in place and is not atomic
    (`database.rs:186-193`, `:360-377`: it removes an ingredient from the database's `Vec`, decodes into it and
    puts it back only if the decode succeeded), and it expects a database with no inputs (a singleton that
    exists already panics, and input slots are re-allocated in the saved order under an assertion;
    `input/singleton.rs`, `input.rs:640-658`). An input, an interned struct and a tracked struct may name their
    own `serialize` and `deserialize` functions; a tracked function's memo is written by its result type's serde
    (`options.rs:318-333`, `tracked_fn.rs:66`), and each memo key is read as a borrowed `&str`
    (`function.rs:939`), which needs a format decoded from one whole in-memory buffer.

### H1.1 The identities

| identity | type | equal when | hash | made by | lives until | may be named by |
|---|---|---|---|---|---|---|
| a spelling | `Name` | same allocation: one per text while any holder lives | stored `hash` | `Interner::global().name`, the lexer | the last holder drops; the table sweeps | anything `'static` |
| a case-insensitive identifier (class-like, function, method, namespace) | `FoldedName(Name)` | `folded_hash` equal, then the same allocation, else the lowercase bytes equal | stored `folded_hash` | `Name::folded()`, a clone | as its name | as a `Name` |
| a global constant | `ConstName` | namespace equal ignoring case, last segment exact | stored | `ConstName::of(&Name)` | a value | as a `Name` |
| an array key | `ArrayKey` (`ShapeKey` renamed) | same variant and value; string bytes interned | stored | `ShapeKey::from_bytes` | weak table | as a `Name` |
| a file | `FileId(u32)` | number | itself | the project, at the path's first sight | the process; a persisted database restores it | anywhere, meaningful in its project |
| a tree and its nodes | serial, `Expr<'a>` handles (B4, G1) | (serial, id) | | `parse` | the `Arc<Ast>` | G1's list |
| a declaration | `DeclRef` (value) / `DeclKey` (engine) | file, kind, folded name, occurrence (an anonymous class: its ordinal in the file) | | the declaration index | a value / an immortal interned slot | `DeclRef` anywhere; `DeclKey` in the engine module |
| a global variable | `Name`, exact (PHP variable names are case-sensitive) | the pointer | stored `hash` | the lexer | as a name | as a `Name` |
| a member | `MemberRef` = (`DeclRef`, `MemberName`): a method by `FoldedName`, a property by `Name` (exact), a class constant by `Name` (exact; an enum case is a class constant, one namespace) | those | | a lookup | a value | anywhere |
| a type | `Type` (`Arc<Node>`, hash-consed) | exactly: the node; semantically: `PartialEq` | stored | the type interner | weak | anywhere; a result compares it by node (H1.5b) |
| a local | `LocalKey` | id | | one check's key table | the check | the check |
| a revision | salsa `Revision` | | | a write | | clears TYPE-4's memo |

### H1.2 The name types

```rust
// syntax::name. The header is NAME-2.1's, with the fields NAME-1.4 needs.
struct Header { hash: u64, folded_hash: u64, len: u32, last_segment: u32 }   // 24 B, no padding
pub struct Name(ThinArc<Header, u8>);        // the text, then its lowercase form when that differs (the slice is 2 * len long then)
pub struct FoldedName(Name);                 // equal ignoring ASCII case; offers no spelling
pub struct ConstName { name: Name, hash: u64 }
// ArrayKey: types::ShapeKey under its proper name. The rename takes its 229 uses in 38 files (242 lines in 42 files with `phpdoc`'s own `ShapeKey`); the old name is
// deleted, not aliased.
```

- **`Name`** keeps what it has: equality by pointer (with the debug assertion that two allocations differ in
  text), hash read from the header, `Deref<Target = str>`, serde as text re-interned into the one interner.
  New: `short() -> &str` (the last segment, from `last_segment`), `namespace() -> &str`, `lower()` from the
  slice (whether a lowercase form follows is the slice's length: `lower_at` would be `len` or 0, so it is not
  stored). One allocation replaces `Arc` + `Box<str>` and, for a name with capitals, an `Option<Box<str>>` (a
  lowercase name takes two allocations today and a temporary lowercase `String`). **How a `&str` is read back is
  S-1's:** triomphe's thin pointer exists only for `[T]`, so `Deref<Target = str>` over a `ThinArc<Header, u8>`
  costs a UTF-8 validation on every access unless a crate supplies a thin header-plus-`str` type; S-1 measures
  it, and H1.3's fallback has no such cost. Identity is `ThinArc::as_ptr()` and uniqueness is
  `strong_count(..) == 1` (triomphe has no `ptr_eq` or `is_unique`). **A PHP identifier is a byte string,** and
  the lexer reads identifier bytes lossily when the source is not UTF-8, so two classes that differ in a byte
  above 0x7f become one `Name` today and after: a known gap, which the lexer reports as a warning when it
  decodes an identifier lossily (a test); `ArrayKey` is the one place the bytes are kept.
- **`FoldedName`** is `Eq` when `folded_hash` is equal and either the allocations are the same or `lower()`
  is byte-equal; `Hash` is `folded_hash`; `Ord` is by lowercase text and agrees with `Eq` (a property test).
  `Name::folded()` is a clone, with no allocation. **Its spelling is unobtainable:** it offers `lower()` and a
  `Display` of the lowercase text, no `name()` and no `Deref` to a spelling. salsa keeps the fields of the first
  equal intern and the old value of a field that compares equal (H1.0 6 and 7), so after `class Foo` becomes
  `class FOO` a stored `FoldedName` would otherwise hand out `Foo`; its `Eq` is exact for everything it can
  show (H1.5b). A map that keeps and shows the declared spelling (`Members::iter` yields every member with its
  declared name and the LSP shows it as the symbol) is keyed by `Name` with folded probes, as `FoldedMap` is
  today, or stores the declared `Name` in its entry; a map that only answers "is there one" is keyed by
  `FoldedName`. Probes by `&Name` read the stored hash; probes from text keep `FoldKey` (the text hashed eight
  bytes at a time, `eq_lowercased` against the stored lowercase form).
- **Why the lowercase form stays inline** (NAME-2.1) and not as 25 G C3's second interned `Name`: a mixed-case
  name would then cost a second header and allocation, an interning order (the lowercase form first, outside
  the shard lock, since creating one must not hold one shard's lock while taking another's) and a strong edge
  between names; the one thing it buys is a pointer test where two spellings of one identifier meet. That is
  rare. A class is spelled one way almost everywhere, equal spellings are one allocation, so the pointer test
  already decides the common case, and the byte comparison runs only on a mismatch.
- **`ConstName`** is `of(&Name)`: one pass over the text (the folded hash of the namespace prefix mixed with the
  exact hash of the last segment). `Eq`: hash equal, then the prefix equal ignoring case and the last segment
  byte-equal. PHP folds a constant's namespace and not the constant (REG-18.4: `\APP\A` finds `App\A`, not
  `App\a`); a `define('NS\FOO')` name is split the same way. Three more rules decide identity (php 8.5.4 was
  run): `true`, `false` and `null` are case-insensitive in every spelling, even in a namespace that defines a
  constant of that name (`TRUE` is `bool(true)` in `namespace App` with `App\TRUE` defined, `defined('True')` is
  true, and `define('True', 1)` fails while `define('php_eol', 1)` succeeds), so a bare `true`, `false` or
  `null` folds; a `define()` literal is not trimmed of a leading backslash (`define('\\BAR\\X', 2)` succeeds and
  defines a constant no name reaches: `defined('\\BAR\\X')` is false), unlike a written `\BAR\X`, which
  resolution strips; and for a project whose range includes PHP 7, `define($n, $v, true)` registers a
  case-insensitive constant (the code models it: `PhpFeature::CaseInsensitiveConstantsRemoved`), a flag on the
  declaration gated by the version range.
- **`ArrayKey`** is `ShapeKey` under its proper name (PHP's array key, not a shape's). NAME-2.3 interns its string bytes in a weak table so `Str` equality is a pointer
  test, and **`ArrayKey::Str` becomes an opaque type built only by the interning constructor** (`ShapeKey::Str(PhpString)`
  is a public variant with 22 sites today, and `PhpString::from(&str)` builds un-interned bytes);
  `ArrayKey::from_name(&Name)` canonicalizes as PHP stores array keys (`'1'` and `1` are one key, `'01'` is
  another). The checker's `LocalKey::Index` and container-field facts hold it (25 G C4 step 5).
- **Serde:** each writes its spelling or bytes and re-interns. A persisted database therefore holds spellings,
  and equality after a restore is by the rules above.
- **Tests:** each map against a `HashMap<String, _>` keyed by the text or its `to_ascii_lowercase()`, on
  random spellings (bytes at or above 0x80 unchanged, as PHP folds ASCII only); one text interned from 16
  threads is one allocation; `FoldedName` equal implies hash equal and `Ord` agrees with `Eq` (property
  tests); a case-only edit leaves a `FoldedName`-valued product equal and changes no text a reader can print;
  `ConstName` finds `\APP\A` as `App\A` and not as `App\a`, folds a bare `true`, `false` and `null`, and keeps a
  `define()` literal's leading backslash; `ArrayKey::from_name` against a table generated from php 8.5 runs
  and checked in (PHP turns the string `-9223372036854775808` into the int `PHP_INT_MIN`, which
  `canonical_int_key` misses today because its digits overflow `i64::parse`; the fix is in the same commit, and
  the table has the overflow cases).

### H1.3 The interner: one, counted, swept. REG-8.6 re-specified

- **One interner, process-wide, as built.** Names cross every boundary (the stub layer, the engine's memos, a
  restored database, the LSP's tables), and `==` on them is a pointer test; two interners would make that
  false. The per-project interner REG-8.6 asked for is not built, and the ledger row is rewritten to what it
  was for: names do not accumulate in a long session.
- **Liveness by count.** `triomphe::ThinArc` has no weak counts, so the table holds its names strongly and a
  sweep drops those whose count is one (only the table holds them). Sweeps run when a shard has doubled since
  its last (as `WeakTable` does now) and at a revision boundary of a long-lived session, after the writes
  have cancelled other handles. A sweep runs under the shard lock, and every clone out of the table takes the
  same lock, so a name with count one cannot be cloned while it is dropped. A name a salsa memo or a persisted
  database holds is held, so it survives. **The ordering rule:** the clone returned to the caller is taken
  before the shard's sweep runs (a new entry has count one, and today's `WeakTable::get_or_insert` sweeps right
  after `insert_unique` while its local `Arc` is held; a counted table must not drop what it has just made).
  **The sweep's schedule and cost** are stated with S-1's numbers: the doubling trigger as now, and at a
  revision boundary of a long-lived session a sweep of at most one shard per write (a scan of 16 shards per
  keystroke would not do), its time per pass and the bytes of count-one names retained at the peak recorded.
  **Which tables are counted:** the name table; `Interner::str` (the weak `Arc<str>` of inline HTML, doc
  comments and index keys), NAME-2.3's byte-string table and the type interner stay on `std` `Arc` with
  `Weak`, as now. A strong table keeps dead names allocated until a sweep, where today the text is freed at
  the last drop, so S-1 has a retention criterion. **Tests:** a concurrent intern, drop and sweep stress with
  the pointer-equality debug assertion on.
- **The builtin knowledge's names** are held by the embedded data and are never swept.
- **Fallback:** if S-1 finds `ThinArc` unfit (its read path costs, or no crate gives a thin header-plus-`str`
  type), `Name` is one `Arc<NameData>` whose single `Box<str>` holds the text and then the lowercase form
  (sliced at `len`: safe, two allocations against today's three, and `Weak` counts stay, so the weak table and
  its sweep stay as they are), with the header's fields in `NameData` (`last_segment`). Nothing above
  `syntax::name` changes either way.

### H1.4 Declaration identity

```rust
// plain values: 'static, Eq + Hash + serde, named anywhere
pub enum DeclFile { Project(FileId), Builtin(u32) }   // Builtin: an index into the embedded file table (J1.2)
pub struct DeclRef { pub file: DeclFile, pub kind: DeclKind, pub name: FoldedName, pub occurrence: u32 }   // kind: Class (any class-like) or Function
pub struct ConstDeclRef { pub file: DeclFile, pub name: ConstName, pub occurrence: u32 }   // `const` and `define()`
pub enum MemberName { Method(FoldedName), Property(Name), Constant(Name) }    // PHP folds a method's name only; an enum case is a class constant, one namespace
pub struct MemberRef { pub decl: DeclRef, pub member: MemberName }
// in the engine only: salsa interned structs built from the values above, one key per PHP namespace
#[salsa::interned(persist, revisions = usize::MAX)] struct DeclKey<'db> { decl: DeclRef }   // immortal (#38): never reused, no creation edge
// the five probes below are collectable: made for names that do not exist too, held by nothing (#38)
#[salsa::interned(persist)] struct ClassKey<'db>    { name: FoldedName }   // class-likes: PHP keeps classes, interfaces, traits and enums in one namespace
#[salsa::interned(persist)] struct FunctionKey<'db> { name: FoldedName }   // functions: another namespace (`class A {}` and `function a() {}` coexist)
#[salsa::interned(persist)] struct ConstKey<'db>    { name: ConstName }
#[salsa::interned(persist)] struct GlobalKey<'db>   { name: Name }         // a global variable, exact: `$Foo` and `$foo` are two
#[salsa::interned(persist)] struct MemberKey<'db>   { decl: DeclRef, member: MemberName }
```

- `DeclKind` is the existing `registry::DeclKind` (`index.rs`), restricted to its `Class` and `Function`
  variants here (a class-like's own sort, class, interface, trait or enum, is data in `ClassDecl`, not part of
  its identity: two declarations of one folded name in different sorts are two candidates for a duplicate
  report); constants have `ConstDeclRef`. `declarations_named` is generic over the key and returns the matching
  ref type (`DeclRef` or `ConstDeclRef`), and `locate` takes `ClassKey` only: functions and constants cannot be
  autoloaded. `occurrence` is the ordinal among the file's declarations of the same kind and folded name, in
  source order, nested ones included (PROD-5.1's vectors give the order). It holds no position, so inserting a
  line moves nothing; inserting a duplicate above shifts the later duplicates, which are different declarations
  then.
- **Member identity refines decision 3** ("a member is (class id, kind, folded name)"): PHP
  folds a method's name and not a property's or a class constant's (php 8.5.4: `$a->DOSTUFF()` runs `doStuff`,
  `isset($a->P)` is false while `isset($a->p)` is true, `defined("A::c")` is false while `defined("A::C")` is
  true), so a member is (class's `DeclRef`, `MemberName`), folded for methods and exact for the rest; a
  `Provider` method read takes the `FoldedName` it was written as, so one method has one memo and one edge
  whatever spelling is asked. Putting a single `Name` in the key would make the set of keys depend on which
  spelling was interned first. **The kinds are PHP's namespaces:** an enum case is a class constant
  (`const A` beside `case A` is "Cannot redefine class constant E::A", and `ReflectionClassConstant::isEnumCase()`
  is true for a case), so constants and cases are one kind, and `Foo::BAR` is one read and not two; static and
  instance properties are one kind too ("Cannot redeclare C::$x"); a property and a method may share a name, so
  they stay apart. Only the member sentence of decision 3 changes.
- **Anonymous classes are file-local, and their identity is the ordinal.** They keep the name the ledger gives them, from their
  position: `class@anonymous/{shown path}:{line}[:{i}]` (decision 5, PROD-5.1, SYN-3.4; F33 makes the path the shown
  one, `File.shown`, so no spelling of the project argument reaches a name), and types and messages show it as they
  do today. That name carries a line, so it is a spelling and not an identity: a line inserted above one renames it, and
  an immortal `DeclKey` built on it would mint a key, and keep its memos, for every inserted newline. So the
  `DeclRef` of an anonymous class is (file, `Class`, the constant name `class@anonymous`, its ordinal among the
  file's anonymous classes), with no line in it, and the positional name is derived from the file's site table.
  They are therefore left out of `decl_names` and `decl_index` (nothing else can name one, since no
  source spells it, so no candidate is lost), and the provider reaches one through its name: a name with the
  `class@anonymous/` prefix has its trailing `:line[:i]` read, **every reading is tried against `FileSet`'s
  `shown` index, and the one whose file's `decls` hold exactly that name is taken** (a path can itself end in
  `:digits`, legal on Linux and macOS, so `…/a.php:12:2` is (`a.php`, 12, 2) or (`a.php:12`, 2); when both
  exist the name is an unknown class), as a recorded read of those `decls`, which is what a type that
  flowed out of the file (an inferred return) needs; a path no file has is an unknown class. The displayed-type
  parser (`anonymous_class_end`, which `try_parse_displayed` and the type-drift gate use) ends a name at a `:`
  not followed by a digit and at a space or a bracket, so it fails on `C:/x/a.php` and on a path with a space;
  it peels `:digits[:digits]` from the end of the token instead, with tests for `C:/x/a.php`, a path with a
  space and a file named `a.php:12`. A moved one changes its own file's products and nothing global.
  **The consumers that see an anonymous class are each decided from an enumeration of the code**:
  `find_dead_classes` skips it already (`!is_anonymous()`);
  `find_dead_members` iterates `registry.classes` unfiltered and reports the unused members of an anonymous class
  today (read from the code, not run), so it reads the file's `decls` to keep doing so, with a fixture for an unused
  member of an anonymous class; dead abstract classes and dead enum cases cannot meet one. **Namespace cycles keep
  it** (`circular_namespace_deps.rs`), attributed to its enclosing namespace, which the registry records where it
  registers the class (`anonymous_class_decls` has the `NameContext`): `namespace_of` gives `""` for its name today,
  so it is a global-namespace node, and `new class implements \App\Contracts\Logger {}` plus any `App\Contracts`
  class extending `\Exception` draws a false cycle between the two; its edges are real structural dependencies.
  **Workspace symbols** (`lsp/workspace_symbol.rs`) drop the class entry and keep its methods and properties, which
  have names and places. **Import completion** (`lsp/completion.rs`) drops it, since no source can spell it.
  **`main/demo.rs` keeps it**, because it dumps what the registry holds. **`registry/scope.rs` is not an iteration
  to decide:** it is the finishing-pass scope the engine deletes, and an anonymous class goes through the levels
  like any class, reached through its file's declarations. **The class-shape parent-call rules**
  (`project/class_shape.rs`) walk only named class statements (`collect_classes`) and include it: an anonymous
  subclass that overrides a method a configured rule says must call `parent::m()` is checked by nothing today (the
  closed-class rule does not apply, an anonymous class being closed by construction; the architecture rules count a
  class's references by span, so an anonymous class's references already go to its enclosing class). Each change is
  a VAL-2.x delta with a fixture. REG-11.13 (diagnostics name it `class@anonymous`) is a display change of its own.
- **`DeclKey` is immortal and the probes are collectable (#38).** `DeclKey` takes
  `revisions = usize::MAX`, which is decision 3's "is never reused": a stale id cannot name another
  declaration by construction (salsa's own check on a read is a `debug_assert`, H1.0 7, so a collected slot's stale
  id would read the new occupant silently in a release build), and an immortal key records no creation edge and no
  persisted interned edge. `ClassKey`, `FunctionKey`, `ConstKey`, `GlobalKey` and `MemberKey` are probes by an asked
  name, made for names that do not exist too (every half-typed identifier asks) and held by nothing, so immortal
  would only grow the table; they stay collectable, and H1.6's rule keeps them honest. Only values interned under a
  LOW-durability query are collected, and creating such a value records the edge. **The cost of the immortal key is
  what it retains:** a dead declaration's key keeps its memos until the database is rebuilt (an `lru` on the heavy
  queries drops values, not keys), so S-3 measures the retained bytes over an edit replay and H4.6 bounds them with
  an in-process compaction. If the bytes are too large, the fallback is a collectable `DeclKey` and decision 3's
  sentence amended to "a stale id never names another declaration".
- **No id leaves the engine.** What reflections, summaries and products hold are names (`Name`,
  `FoldedName`, `ConstName`) and `DeclRef`s. A reflection that holds an id could not be compared for cutoff,
  persisted by the stub cache's format, or shared with the stub layer, which has none. A parent is the
  `FoldedName` it was written as, resolved to its declarations when a level reads it (H2.4).
- **No result stores a member id.** A member is looked up as (class's `DeclRef`, `MemberName`) in the class's
  member table, and its slot there is a position in one level's result; the interned `MemberKey` of H2.4
  lives in the engine only.

### H1.5 Two rules for values the engine compares

**(a) Positionless.** A value that a query of another file reads holds no span, line or column of the file it
comes from (a file's own diagnostics and per-node results do: nothing but that file's check and the finalizer
read them, and their position is their meaning). "Where" is a `DeclRef` or `MemberRef`, and the position is read
from `file_sites(file)`, a separate query over the tree's declaration vectors (PROD-5.1). Why: when a new value
is equal the engine keeps the old one, and when it is not equal every reader of it runs again. With a position
in the value, an edit above a class invalidates the class and all its readers; with the position left out of
the equality instead, the memo keeps a stale position. Consequences:
- `Provenance` becomes `{ origin, site, declaring_class }`, where `site` is the declaration's ordinal data
  (the `DeclRef`'s occurrence, kind and name, and a member's ordinal in its class), and `declaring_class` stays
  (the declaring class, a trait for an imported member; the LSP reads it, `workspace_symbol.rs`);
  `declaration_span()` becomes a lookup, `Registry::span_of(&Provenance)`, through the file's site table.
  `class_declaration`, `method_declaration` and the other `Registry::*_declaration` accessors
  (`provenance.rs:263-312`; 25 uses elsewhere, in three files) become the same lookup.
- **Every span-bearing type reachable from a result type is listed and given an ordinal or body-relative form:**
  `Span`, `QualifiedName` (`{ written, resolved, span }`, with no `PartialEq` of its own, held by
  `BodySummary`'s `PurityCall::Function` and `Callee::{Function, Static}`) and `Ident` in the registry,
  `summaries`, `init_flow` (`Event::Read(Name, Span)` holds the span of a premature read), `purity` and
  `bodies`. An `xtask arch` rule says a result type reaches no `Span`, `QualifiedName` or `Ident`, as the rule
  of (b) does for `Type`.
- **REG-4.15's "a member's span is a field of its entry" is withdrawn:** the entry holds its ordinal. REG-4.11
  ("provenance as the only location record") is then over (`Origin`, `site`).
- A rule that quotes another declaration's line in a message (duplicate sites, `previously declared at`) reads
  that file's sites, and so depends on that file's positions: correct and minimal.
- `Origin::File(FileId)` stays in meaning, but its codec changes: `OriginData::File` stores no file number
  today and `Origin::File(_)` and `PendingFile` come back pending, so a persisted database could not restore
  its file table with it. The codec writes the `FileId` and `PendingFile` is deleted.

**(b) Total, exact equality.** The equality a result type implements compares every field a reader can
observe, exactly: `Name` by pointer, `Type` by node, floats by bits; never a semantic `PartialEq`, never "up to
case or order". Why: backdating keeps the old value when the new one compares equal, so anything observable that
the equality ignores goes stale (a union printed in its old order, a class in its old case), and an edit replay
would then differ from a fresh load (G5's tool 7). The two paths of H1.0 6 both apply: a tracked struct's
field keeps its old value, and a tracked function's result is stored new with the old `changed_at`, so the
exactness rule and its test cover both. Mechanism: salsa takes the result's `PartialEq` (the options
are that or `no_eq`; `options.rs`), so a result type implements `PartialEq` over fields whose own `PartialEq` is
exact. `types` gains `Type::same_node` and a newtype `Exact(Type)` (`PartialEq` and `Hash` by node) that
results hold in place of a bare `Type`. A test per result type changes each field in turn and asserts `!=`
(S-8 runs it once for a field and once for a function result, comparing a reader's output across a case-only
edit); an `xtask arch` rule rejects a result type that derives `PartialEq` over a bare `Type` (it parses one
file at a time with `syn`, so it sees a field written `Type` and not one reached through another struct, an
`Arc` or a container of another crate's struct such as `ClassReflection`'s `method_tags`: each result type's
own test covers those). Other leaf types are inexact on purpose too (`Shape::eq` ignores `next_index`,
`ObjectTypeCell::eq` is always true, and `FoldedName` is case-blind by design, with no spelling to show). `Type`'s
semantic `PartialEq` stays what the checker uses. **The stronger form S-8 evaluates:** if few `==` and
derived-`PartialEq` sites on `Type` rely on its semantic equality (43 `== Type::` sites and 268
`derive(PartialEq)` types today, which S-8 classifies), the semantic one is renamed `Type::equals` and `==`
means the node, compile-driven as `Borrow<str>` and `FoldKey for str` were, after which a derived `PartialEq`
over result types is exact by construction and `Exact` and the rule are belt and braces.

### H1.6 Where an engine id may be named

`salsa` is named only in the engine module (a crate from G1); so are `File` (the input), `DeclKey`,
`ClassKey`, `FunctionKey`, `ConstKey`, `GlobalKey` and `MemberKey`. The engine's public surface is `Provider`
(the accessors, H2.4), `Session` and plain values (`DeclRef`, `ConstDeclRef`, `MemberRef`, `FileId`, names).
An `xtask arch` rule enforces both, as G1's rule does for tree ids, and a release-build test says no engine id
appears in a result type. A reflection, summary, product, the checker and the LSP see no id. The
checker-facing reader is a trait in `reflection` (the accessors of H2.4) that `engine::Provider` implements,
so `analyser` depends on a trait it owns and not on its own consumer (H2.4).

### H1.7 What batch 4 builds, and what it must not

Builds: (1) the thin `Name` with its count-based sweep, which is NAME-2.1's and lands in the same commit
(a thin name has no `Weak`, so nothing frees a name until the sweep exists; gated by S-1, else the fallback
above); (2) `FoldedName`, `ConstName`, the `ArrayKey` rename, and `Type::same_node` and `Exact` (H1.5b); (3)
NAME-1.1, 1.5, 1.6 and SYN-4.5 (`ConstName`'s record kind); (4) NAME-1.2 over **values, not only maps**: the
table is of fields, parameters, returns and maps, with one census command and one count (the mentions of a
`HashMap<String` or `HashSet<String`, std or Fx, outside tests were 181 at `8a1ca1e0`; 187 with tests;
18 `BTreeMap<String`, 9 `BTreeSet<String` and one `HashMap<Box<str>` sit outside
that pattern, and `Vec<String>` (290) and `Option<String>` (220) hold identifiers among other text). It **leads
with the registry's `&str` entry points** (the ledger's cost order), and the compiler lists only those once
NAME-1.3 deletes `TableKey` and `FoldKey for str`; the value half has no compile-time forcing function, so its
completeness check is a reviewed table with a grep gate on identifier-named `String` fields. The `String` keys
of the machinery W2 and W3 delete (`ChangedNames`, `Scope`, `reset_scoped`, the guarded sets in `Locals`) are
converted because NAME-1.3 cannot delete `FoldKey for str` while a caller holds text, which is a type change an
end-state invariant needs (#35); (5) NAME-1.3, NAME-1.4, NAME-2.2, NAME-2.3. Must not: introduce an id, a
second interner, a value that holds a position, or a result type whose equality is semantic. NAME-1.2's
registry entry points are the ones REG-4.12 makes into the `Provider`'s accessors, so REG-4.2 and 4.12 are
written on `FoldedName`.

**Checks.** The conversions follow the ledger's order of cost. Oracle for them: `registry-dump`, `--dump-expr-types` and
O2 before and after (G5), with VAL-2.7's container-field delta read. The engine does not exist yet, so the
rules of H1.5 are tested where they can be in batch 4: `Exact`, the same-node tests and the case-only-edit test
of H1.2. The test that two `Provenance` values for one class at different lines are equal once the span is out
moves to the commit that removes the span (REG-4.15, W2 step 5): from batch 5 (PROD-5.1's site table) until then
declarations hold their spans and the site table at once.

## Decisions

### #10 SYN-3.2's SIMD

**A fused one-pass kernel (newline mask and non-ASCII mask) on a safe SIMD crate, judged on merit and not on `Cargo.lock`.** The candidates are `wide`, `core::simd` (no `#![feature]` exists in the workspace today, so it would be the first, and its API moves with each bump of the pinned nightly), `pulp`, and a `u64` word kernel (one path on every target, no dependency, about one instruction per byte). `wide` is chosen: stable API, and already in the build through `imbl`, with SSE2 or NEON as the baseline and no AVX2 dispatch; our own code holds no `unsafe`, though `wide` holds some of its own (the `transmute` in `u8x16::new`, the NEON intrinsics, `unsafe impl Pod`). **The accept rule:** the kernel beats the `memchr` plus `is_ascii` version by at least 20% (proposed) in Ir and in a criterion bench on x86-64 and is not slower on the aarch64 runners (macOS, Linux) or on Windows, where `move_mask` is four NEON instructions and a horizontal add, which `memchr` itself avoids there; otherwise `memchr` stays and `wide` is dropped, or the `u64` kernel replaces it. That version stays as the test oracle, and SYN-4.3 shares the kernel. The PHP line-terminator rule (a lone CR ends a line, as in the lexer's comments, close tag and heredocs) ships with it in one change (I3). If a measurement says AVX2 matters, a safe dispatching crate such as `pulp`, judged on merit

*Needed before:* batch 2

### #11 the parse-recovery batch

**In.** Decision 4 says every parse yields a tree, and an analyzer that stops at the first error is wrong for the editor. SYN-1.9, Q-10's parser half and SYN-6 are one design, built after SYN-2 because recovery synchronizes on tokens; the version half waits for STUB-7.1. F77's reporting (a parse failure is `phpstanRs.parseError`, non-ignorable, and fails the run) does not need recovery and is built in batch 0, so that item 9's parser rewrite cannot refuse a file unseen; batch 3 only changes where its errors come from

*Needed before:* batch 3 (F77: batch 0)

### #12 order of items 13, 16, 15, 14

**Names first: 16, 15, 13, 14.** `FoldedName` and `ConstName` do not exist yet (NAME-1.2 makes them), so products and PHPDoc work should be born on them, and DOC-2.5 and F41's lowering produce `Name`s.

*Needed before:* batch 4

### #14 DOC-2.1

**Undefer it, as F41.** F41, the ledger's DOC-2.1 and C7's grammar item are one piece of work: one grammar, a `TypeNode` to `Type` lowering, the string parser deleted, internal strings through `parse_phpdoc_type`. It fixes real bugs (a class named `Infinity`, `Nan` or `inf` read as a float; `1_000` and `0x1F` rejected; conditionals split at the first `:`), so "the saving is gone" does not apply. First in batch 6, before DOC-2.2; C7 later builds the resolver on it

*Needed before:* batch 6

### #17 CHECK-9 and TYPE-5

**Neither in batch 7.** CHECK-9 only after F69 gives implicit calls (`__get`, `offsetGet`, `__toString`, `include`, `Traversable` iteration) throw points, or it reports live catches as dead. F69 records them on `record_call_throw_point` and `current_function_throw_points`, the recorders that D2's `StatementResult` replaces, so by the crate split's Appendix F ("a bug in code a later phase rewrites gets a fixture now and its fix as that phase's behaviour commit") F69's fixtures go in batch 0 and F69 and CHECK-9 are built in D2, CHECK-9 after F69. The same reasoning puts CHECK-7's checker half in D2 and CHECK-10's operator sites in D3 (J3). TYPE-5 is F14, which follows F41, so it comes right after #14's work

*Needed before:* batch 6 (TYPE-5); D2 (F69, CHECK-9; fixtures in batch 0)

### #18 when to reevaluate

**Now, for the engine's identity model.** Design REG-16.2's ids, REG-8.6's interner ownership and NAME-2's weak tables as one lifecycle before batch 4: names, products and memo keys are built on it. The rest at batch 8

*Needed before:* before batch 4

### #29 NAME-2.1's thin name

**`triomphe::ThinArc<Header, u8>`, after a spike that settles how the text is read back as a `&str`.** A header plus one slice fits (the text, then the folded form), and the interner frees names without `Weak` (triomphe 0.1.14 has no weak counts) by a unique-count sweep, which REG-8.6 already plans. Its thin pointer exists only for slices (a header plus a `str` is a fat pointer), so without `unsafe` of ours a `&str` costs a UTF-8 validation on every `as_str`, `lower` and `Deref`, unless a crate supplies a thin header-plus-`str` type (`slice-dst`'s `StrWithHeader` behind `erasable::Thin` is one to evaluate, on merit). S-1 therefore measures the read path and accepts the thin name only if the corpus total of those reads is not above today's; the fallback is one `Arc<NameData>` whose single `Box<str>` holds the text and the lowercase form (sliced at `len`): two allocations against today's three. No hand-rolled `unsafe`

*Needed before:* batch 4

### #32 `PhpVersion`, `PhpVersions` and `PhpFeature` move out of `types` so the parser can record marks: a leaf crate (`php-version`), or a module of `syntax`, which `types` already depends on

**A leaf crate**, as the crate split makes every shared base (F20 proposes the same for the stack guard). It moves the code as it is, with `KNOWN_PHP_VERSIONS` starting at 5.3, and adds what the marks and `declarations_named` need before W4: `VersionSet` (bit i is `KNOWN_PHP_VERSIONS[i]`, append-only, so a new version is a new bit at the end) and `Availability` (J1.1), a file's validity being a `VersionSet`; `PhpVersions` gains exact version-id bounds (J1.1); STUB-7.1 only adds `Readings` to them. Decision 18's 7.0 floor, with the notice for a range that reaches below it, is a mask over the same bits and not a rebase of the list, and is STUB-7.1's change and a delta of its own, so batch 3 changes no project's range; until STUB-7.1, `declarations_named` reads a builtin as `Always` available. A narrowing by a patch-level `PHP_VERSION_ID` is conservative (a minor stays in the range if any patch takes the branch; `narrow_at_least(80103, false)` would drop 8.1 although 8.1.0 to 8.1.2 take it). **`SyntaxFeature` is the grammar-only subset** of today's 45 `PhpFeature` variants, in the leaf crate with the one threshold table, and `PhpFeature` in `types` delegates to it for those and keeps the rest (the library gates such as `PrintfHhSpecifier`, `FilterThrowOnFailureConstant` and `TentativeReturnTypes` are not the parser's); array unpacking with string keys depends on the unpacked array's keys, not the syntax, and stays out. A new crate is not one row: the last one (`27091848`) touched 19 files: its `crate` line in `gates/module-graph.txt`, `LIBRARY_CRATES` and the build-time scenario lists in `xtask`, `fuzz/Cargo.lock`, the dependents' manifests and `gates/test-renames/module-graph.txt` for the tests that move

*Needed before:* batch 3

### #34 positionless declarations: `Provenance` holds a declaration's ordinal data and the positions live in the file's site table; REG-4.15's "span a field of its entry" is withdrawn; an anonymous class, whose name carries its line, is a file-local entity outside the global index

**Yes.** On the engine an inserted line would otherwise invalidate every class below it and all their readers (today it already marks them changed and rechecks them), and ignoring the span in the equality instead would keep stale spans in a backdated memo. An anonymous class keeps the name the ledger and the review give it (`class@anonymous/{shown path}:{line}[:{i}]`: decision 5, PROD-5.1, F33), so types and messages display it as they do today, but **its identity is the ordinal**: its `DeclRef` is (file, `Class`, the constant name `class@anonymous`, its ordinal among the file's anonymous classes), with no line in it, so an inserted line mints no key and moves nothing, and the positional name is derived from the file's site table. Because no other file can spell it, it stays out of `decl_names` and `decl_index` and is found through the file its name carries (H1.4), so a moved one changes its own file's products and nothing global. The name's split is not parsed by guessing: every reading of the trailing `:digits[:digits]` is tried against the `shown` index and the one whose file's `decls` hold exactly that name is taken (both existing is an unknown class), so a file called `a.php:12` is no trap, and the displayed-type parser peels the same suffix. The consumers that see an anonymous class are each decided in H1.4 from an enumeration of the code: dead members keep it, read from the file's `decls`; workspace symbols drop the class and keep its methods and properties; import completion drops it, since no source can spell it; namespace cycles keep it, attributed to its enclosing namespace (`namespace_of` makes it a global-namespace node today, which draws false cycles); `demo` keeps it; `registry/scope.rs` is not an iteration (the engine deletes the scope, and an anonymous class goes through the levels like any class); and the class-shape parent-call rules, which walk only named class statements, include it. Each has a fixture, and each change a VAL-2.x delta

*Needed before:* before batch 5 (PROD-5.1 lays out the vectors)

### #35 PROD-5.7, LOAD-4.7's file-references half and PROD-5.4's rewrite work on `deps::Collector`, `recorded_references` and the dependents indexes, which REG-16.4 deletes whole: build them in batches 5 and 8, or leave them

**Leave them; keep PROD-5.8.** The savings (targets, upper bounds in the ledger's sense: ≈0.34 G of the off run, `recorded_references` 265.6 M and `register_references` 77.2 M) end when the wall's W3 deletes the machinery, and the 25 G audits' no-interim rule (§10.7) says not to build what a later traversal replaces. The line is between a new structure for a gain (PROD-5.7's one vector per file, a format bump: not built) and a type change that an end-state invariant needs (NAME-1.2 converts the same machinery's `String` keys, because NAME-1.3's deletion of `FoldKey for str` is a compile-time guarantee that a caller left holding text would break). It stands or falls with building the wall (#19, #21): if the wall is dropped, the three rows are READY again. Checked in the code: the readers of the reference sets besides the dependents index (`mentions_phpstan`, which loads PHPStan's classes while a file names a `PHPStan` name; the stub index at `registry/stubs.rs`; the body index at `registry/bodies.rs`) move to PROD-5.8's record, which is built, so only the dependents' feeders go; about 77 M of the 343 M is on the serial insert. **The rule is the 25 G report's own (§10.7).**

*Needed before:* batch 5

*Left as it is (#35):* the interim dependents-index work is not built, since REG-16.4 deletes what it feeds.

### #38 REG-16.2's "never reused": keep salsa's collection of interned slots (a stale id panics and invalidates what read it; memory stays bounded), or turn it off with `revisions = usize::MAX` (the literal "never reused", and a table that grows for the life of a server and of the persisted file)

**`DeclKey` is immortal, and the name-probe keys are collectable.** `DeclKey` takes `revisions = usize::MAX`, the literal "never reused" of decision 3, which stays as ruled for declaration ids: a stale id cannot name another declaration, by construction and not by discipline (salsa's own check on a read is a `debug_assert`, so with collection a stale id of a slot re-interned this revision would read the new occupant silently in a release build), and an immortal key records no creation edge. `ClassKey`, `FunctionKey`, `ConstKey`, `GlobalKey` and `MemberKey` are probes by an asked name, made for names that do not exist too (every half-typed identifier asks), and nothing holds them, so immortal would only grow the table; they stay collectable, and H1.6's rule (no id outlives its query or leaves `crates/engine`: an `xtask arch` rule and a release-build test that no engine id appears in a result type) keeps them honest. Only values interned under a LOW-durability query are collected, and creating such a value, not reading it, records the edge. **Two costs of an immortal `DeclKey` are stated and measured in S-3:** a dead declaration's key keeps its memos (the five levels, a full `ClassReflection` among them) until the database is rebuilt, and an anonymous class must not carry its line in the key (E34). **The bound is an in-process compaction (H4.6):** the persisted-file rule fires at the next start, which does nothing for a server open for days, so the server rebuilds its database at idle once the keys outnumber the live declarations several times over. If S-3 finds the retained bytes too large, the fallback is a collectable `DeclKey` and decision 3's sentence amended to "a stale id never names another declaration"

*Needed before:* W2

## Facts the decisions rest on

- DOC-2.1, F41 and C7's grammar item are one design. TYPE-5 is F14, which the review itself orders
  after F41. The iterable arm is in `types/src/parse.rs`.
- F69: implicit calls record no throw point, and CHECK-9 reads the throw points.

---
