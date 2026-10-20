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

Batch 0 is the README's, item 9 is Part B's, and the batch 8 rows are other parts' (LOAD-5.4 is Part A's; CACHE-13.1, CACHE-8, LOAD-5.1, LOAD-5.2 and LOAD-3.1 are W3's). They are named in the text, not listed here.

The designs in this part own no rows of their own; each serves rows listed above or owned elsewhere. Level 3's placement and G5 (item 0's tooling) serve batch 0's rows and the review's findings. I2 serves batch 1 (SYN-2 and SYN-4.1 are above) and I3 batch 2 (SYN-3.2). G3 serves batch 3: SYN-1.9, Q-10 and SYN-6 are in C1's table, CHECK-12 is W7's, and CHECK-13 is batch 5's with PROD-5.1 (decided in H1.4). H1 serves batch 4 (NAME-1.1 to 1.6, NAME-2.1 to 2.3 and SYN-4.5, all in batch 4). I4 serves batch 6 (DOC-2.1 and F41), and I5 batch 7 (CHECK-6.10 and CHECK-4.1). The designs below are written from the code at `8a1ca1e0`, and the sections follow the order of the batches they serve.


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

## Level 3's placement

The level-3 count overstates the work. `STAGE_B_FIX_REVIEW.md` has 88 open findings, a validation and 16 pointer lines
(the earlier reviews' items that a finding re-opens, listed again under "Full review"); `STAGE_B_REVIEW.md` has 15 and a
validation; `STAGE_C_REVIEW.md` has 14 and a validation, of which O1 and F5 are fixed (`e06f470c`, `1cdfe80d`; the fix
review says so) and two are the same bug as a fix-review finding (Stage C's F4 is F55, its F8 is F30). That is about 115
findings. Whether each still reproduces is settled by running its probe on the gate binary in batch 0, not by reading.

**The rule for when each is fixed:** a finding in code that a later task rewrites gets its fixture now and its fix inside
that task; one that changes what the baseline records is fixed before the baseline; the rest go in the review's own order
between heavy runs.

| finding | meets | placed |
|---|---|---|
| F10 | the item 0 baseline (a skipped file has no tree dump) | its symptom is fixed; tests and fixture before the baseline, the exhaustive predicate in batch 1 |
| F21 | the canonical printers (`Repr::Mixed` changes) | before them |
| F20 | the cross-platform plan's R10; item 9 | after R10 and before item 9, so the lint checks the recursion item 9 writes |
| F77 | the item 0 baseline (a refused file must show in O2) and SYN-1.9 | batch 0, for analysed files (built: the analysed-file half and the context-file notice are in `4278ec2a`); the dump and the per-file cache record a context file's failure in batch 0 (needs VAL-1.8); batch 3 feeds it from `errors()` |
| the seven fixes the crate split's F1 puts first (debounce stack, `latest_text`, `didClose`, `--generate-baseline`, `--fix`, the closed pipe, rename) | none: they are on today's code and independent | batch 0, each with its test (built: SRV-1 to SRV-7) |
| F22 (closure `use` list), F8 and F7 (the use-site walk) | item 9 step 1 (`Visitor` over handles) | in step 1 |
| F14, F41, F19 (the PHPDoc bullets) | DOC-2.1, TYPE-5 | batch 6 |
| F69, T2 | CHECK-9, REG-17.2 | F69's fixtures in batch 0; F69 and CHECK-9 in D2, since they sit on the recorders D2 replaces; T2 with REG-17.2. T2 asks "decide whether callee throw points count" and is a decision row: they do, as PHPStan's `MissingCheckedExceptionInMethodThrowsRule` reads the throw points of calls, and F69's throw points for implicit calls and REG-17.2's inferred throws are what make it complete, so T2 is built with them and the rule reports both a direct call and `$o[…]` on an `ArrayAccess` object |
| F5 (definedness certainty), F6 (closure-literal alias), F45 (reference groups), F61 (unset properties) | D1 and D5 (holder storage): each design adds per-variable state to `Locals`, and the fix review's own first step is one `ExpressionHolder` per narrowing key | fixture now, fix inside D1 and D5 (J3); F6's design stores an `Rc<Expr>`, which G1 and G2 replace by a rooted alias that D1 deletes |
| F18 | the `&&`/`\|\|` and nested-`match` causes are Stage D's one inference; the trait chain is REG-7.2's; the `.` rope and the `elseif` suffix are local | fixture now (the scale scripts), local ones now, the rest in D3 and W2 |
| F35, F50, F55 (Stage C F4), F75, F78, Stage B D1 | REG-4.2, REG-5, REG-11.4 and 11.5, REG-8.3, REG-7.5 | fixture now, fix in W1 and W2 |
| F76 | REG-7.10 (namespace cycles) | W2 |
| F58 | G1.9 and G4a (no per-function clone; the `Resolved` is built only when the anchor matches, in both walkers) | resolved by the design |
| F33 (anonymous class names, infer cache assumption) | #34, G1.5, H1.4 | the name is the shown path and the line, as its design states (`File.shown`, H2.2); the infer cache's context epoch and the env-gated recompute-on-hit assertion are G1.5's |
| F30 (Stage C F8) | REG-4.15's builder | fixture now |
| Stage C F1 | decision 11, REG-8.1 | W2 |
| Stage C F5, F6, F7 | REG-11.10 (F5, fixed), REG-18 and J1 (F6, F7) | W1, W4 |
| Stage C F2, F3 | LOAD-7.3 and 7.4, STUB-7.3 | W1, W4 |
| Stage C F9, F10, F12 | LOAD-7.6, LOAD-7.7 | W1 |
| Stage C F11 | VAL-1.3, REG-8.5 | batch 0, W2 |
| F74 | the composer follow-ups of #40 | with them |
| Stage B "Typed purity", D2 | REG-17.4 and SUM-3, REG-11.6 and EXT-2 | W2, W5 |
| the comments (F24, F34, F37, F43, F51, F59, F80, F88, Stage C F13) and the fixtures (F23) | none | any gap; F23 before the baseline |
| everything else (F2 to F6, F9, F11 to F13, F15 to F17, F25 to F32, F36, F38 to F40, F42 to F49, F52 to F56, F60 to F73, F79, F81 to F87, F89; Stage B W1, C2, M1, CMP-1, DF-1 to DF-7) | none | in the review's own order, F4 first |

## G5. Item 0's tooling plan

The 20 G plan's item 0 (written at `5fb858c9`) is the design; this refreshes it against HEAD, builds
on what the gates already have, and says in what order and with what acceptance.

**What exists (verified).** `cargo xtask gates | corpus | compare | type-drift | baseline | gatetime |
modgraph | arch | front-end-dump`, and `tools/gate-runner`, which runs one analysis through the same
library calls `check-project` makes and writes, per configuration, the report lines
(`path, line, col, level, identifier, message, tip`), the emission-order view, the baseline run and
`--dump-types` (every tracked variable's type and native type after each statement, as display text).
Missing against item 0: the diagnostic lines drop the end position, `non_ignorable`, the fix (span,
replacement, description, `within`) and all but a joined tip; `Display` for types drops `mixed`'s
explicit flag and a template's bound, variance, scope and default; `front-end-dump` hashes `Debug`
text; there is no registry dump, no per-expression type dump, no PHPDoc record, no server session, no
edit replay, and no `oracle` profile.

**Tools, in build order.** Each is a commit or two; each ends with its own acceptance. The oracle run comes after the
session and the replay because VAL-1.2 includes the replay, and the oracles go (VAL-1.3) only after that run.

| # | tool | content | acceptance |
|---|---|---|---|
| 1 | VAL-1.1: read the starting state | HEAD against `1892b477` (the last validated state) over the vendor trees, all-on and unscoped reports; the six listed behaviour changes (Pratt rejections, `__halt_compiler`, kind-exact keys, hyphenated words, declarations ruled out by version, the server's UTF-16 columns) read; anything wrong fixed in its own commit | each difference named and either approved or fixed |
| 2 | canonical printers (VAL-1.4), after F21 (the error type as a kind of `mixed`, which changes `Repr::Mixed`'s fields) | `types::canon` over every field of `Repr` (explicit-`mixed` flag and subtraction; template name, scope, bound, variance, default; benevolent unions, call-site variance, accessories; literal bytes and float bits; shape optionality and sealedness; conditional subjects), no interning, addresses or hash order; the same for `Diagnostic` | every printer destructures without `..` and matches without `_`, held by an `xtask arch` rule that the `canon` and `dump` modules contain no `..` in a struct pattern and no `_ =>` arm (a test that must fail to compile cannot be an in-tree test: `Repr` is `pub(crate)`, and a copy of a type with an extra field is not the type the printer destructures), plus a one-time mutation check recorded in the commit message, as tool 4's; two runs and one thread against many give identical bytes |
| 3 | full-field O2 (VAL-1.8) | the gate-runner lines gain end line and column, UTF-16 columns, `non_ignorable`, the fix and every tip; run over Piwigo's three configurations, each vendor project with every rule set, and the fixtures; stub cache cold and warm; per-file cache off, written and read | all states give one output; `xtask compare` reads it |
| 4 | `registry-dump` (VAL-1.6) | taken after the finishing passes, before checking: every project declaration and every record of both stub bases (at the configured range and its ends once ranges exist), with reflection, guard and guard value, purity dependencies and verdict, property-write classification, ancestors, owners, the ancestry fact; the constants, global-variable facts, both reverse indexes, the use-site index, duplicate-site tables, the file table (path, role, load order, absolute path, Composer owner, `mentions_phpstan`). **Defined over an enumeration interface** (all declarations as reflection records, from a registry value), implemented for `Registry` now and for `Provider` at W2 step 1, so that after a level flips to the engine (H3.6 step 3) the dump prints the engine's answers and not the old maps' on both sides; the `Registry` field destructuring below is the pre-W2 completeness check and goes with the struct | sorted by kind, folded name and load order; lazy records decoded, deferred values printed as stored; the dump destructures `Registry` (31 fields at `registry.rs:262`) without `..`, so a field added later is a compile error, and **all 31 are classified**: scratch with its reason (`raw`, `raw_active`, `changed`, `scope`, `scoped_names`, `demand`, and the history-dependent `names_with_assert_tags` ("only ever added, never removed on `remove_declarations`") and `stub_provided_functions` ("not yet overwritten"), which an edited registry and a fresh one hold differently, so printing them would fail the replay's equality by construction) or products that are printed (`doctrine_entity_index`, `trait_users`, `subtype_children`, `recorded_parents`, `as_built`, the two configured assertion tables, and the rest); the printers of `ClassReflection` (about 45 fields), `FunctionReflection` (about 24), the member records and `Provenance` destructure exhaustively too, held by the same `xtask arch` rule as tool 2. The printers never print a version, an id from a process-global counter, an address, an absolute path or a temp-directory name, and a difference between two runs is a finding to triage (engine or dump); mutation test: changing one member's flag changes the dump |
| 5 | `--dump-expr-types` and the PHPDoc record (VAL-1.7) | every expression's inferred and native type in the final pass, keyed by file, span and host class (a trait method is checked once per host); every PHPDoc resolution keyed by file, docblock span, tag ordinal, host class and function | off by default: one `Option` test at the end of inference, nothing when off; the dump is identical across `PHPSTAN_RS_THREADS=1` and the default |
| 6 | the server session (VAL-1.9) | an in-process session over Piwigo and two vendor projects, driven by **a public in-memory session API of the `lsp` crate** (start, send, wait until quiet, run a closure over the `Project`, and a dump request), which `lsp`'s own tests, `tools/gate-runner` and the replay all use. The harness of `lsp/tests/helpers.rs` (`encode`, `decode_all`, `run_rooted`, `run_across_debounce`) is `pub(super)` and test-only today and drives the server's private `run_loop` and `Server`, so it cannot live in `testing` (the module graph gives `testing` no `lsp` edge, `testing` depends on nothing above `project`, and a dev-dependency from `lsp` back on it would be a cycle that links two copies of `lsp`): `testing` keeps the fixture runner only. `run_rooted` pumps all input and exit and returns only the decoded messages, and `run_loop` builds the `Server` inside and drops it, so stepping needs `run_across_debounce`'s thread-and-sentinel pattern (a `didChange` is applied on the debounce worker, so the end of a step is not the end of a message) and a hook into the server's `Project`. The requests: files and roles, `registry-dump`, diagnostics with UTF-16 ranges, code actions, definition, hover and references, and completion (after `->` and `::` at a seeded sample of receivers), signature help, document symbols and semantic tokens, each over a sample chosen by a hash of each path, since item 9's step 1 migrates every `lsp` reader (navigation, completion, signature help, document symbols) and step 2 rewrites the probe. `TempDir` and `TMPDIR` are under `~/.cache` (not the RAM-backed `/tmp`) and removed afterwards | the first dumps equal the CLI's (LOAD-1's equality) |
| 7 | the edit replay (VAL-1.10) | a fixed script over a seeded sample, **on a copy of the corpus** (a hard-link or reflink copy under `~/.cache`, removed afterwards: the Piwigo corpus is frozen and `vendor/*` are submodules whose changes `require_clean_tree` rejects), over Piwigo and two vendor projects: save one unchanged, delete a method, rename a class, change a parameter type, add a file declaring a class named but declared nowhere, delete a file; after each step the project dump, `registry-dump` and every file's diagnostics; at the end a fresh load of the edited tree | the end state equals the fresh load; a failing equality leads to commits of its own, as tool 9's |
| 8 | memory (VAL-1.11) | the dhat profile by holder with untrimmed backtraces, at this commit; the conversion and holder scripts move into `scripts/` first (they live outside the repository today, in `phpstan-rs-kept/prs-profile/dhat-heap-modes/analysis/`, with a hand-curated `OVERRIDES` table) | no allocation left unplaced (the 10,987 of M10), and the holder table at this commit is the new baseline, explained against MEAS-2's (which measured `791d29cb`) by the commits since, LOAD-3.3's -214 MiB among them; no heap noise band exists, so none is claimed |
| 9 | the `oracle` profile and its one run (VAL-1.2) | `gate` plus `debug-assertions` and `overflow-checks`, with `PHPSTAN_RS_NO_FILE_CACHE=1`, over the three Piwigo configurations, each vendor project with every rule set, the server session and the edit replay (the incremental paths hold oracle sites that only they reach); a temporary uncommitted counter shows every oracle site ran | every failure fixed in its own commit |
| 10 | retire the debug oracles (VAL-1.3) | `FileBodies` stops storing statements; `BodySource::body` and the debug-only re-exports go; the test oracles whose second implementation exists only for them go; assertions about one implementation stay (the file cache's round trip) | tests and fixtures pass; item 9's reader set is smaller by these |
| 11 | the baseline (VAL-1.12) | every dump above from the commit that adds the last tool, with the `gate` build at one thread and at the default count (byte-identical) and with the `oracle` build (no assertion); the ordered file lists with roles | stored with the binaries |

VAL-1.5 (the front-end dump) is item 9's: its canonical dump over handles replaces `front-end-dump`
and is baselined at the end of step 1.

**Where things live.** The writers sit with the data: `types::canon`, `registry::dump`,
`checker::dump` (types, PHPDoc), `syntax::dump` (item 9). The gate-runner gains `--dump registry |
expr-types | phpdoc | diagnostics`; `xtask dumps` records the set at HEAD under
`.corpus-runs/dumps/<commit>/`, and `xtask dumps-compare <a> <b>` diffs two sets and classifies every
difference by dump and file. The baseline's binaries are copied to `.corpus-runs/gate/bin/<commit>/<profile>/`
and its profiles to `~/.cache/prs-profile/<run>/` as soon as they exist, and nothing else is kept:
the dumps a comparison reads, and no per-commit hoard.

**What each dump protects.** Item 9: the expression-types dump (the whole checker moves to handles),
the registry dump (unchanged by it), O2, its own dump. Batches 1 to 3: its dump. Batch 4 (names):
registry, expression types, O2 (container-field keys are VAL-2.7's delta). Batch 5: registry, O2.
Batch 6: the PHPDoc record, expression types, registry. Batch 7: expression types, O2 (CHECK-6.4
bumps a format). The wall: `registry-dump` across REG-7, the edit replay across REG-8, REG-16.4 and
CACHE-13.2.

**Wall-time rule.** A wall-time accept in this plan (item 9's, the spikes') is a paired, alternating
comparison by `scripts/abab.py` with the ledger's rule: the pair count stated with the accept, every output
checked against the oracle, a Wilcoxon test at a stated p-value, and an Ir count where Ir decides (the VM has
no performance counters, and plain off runs vary by hundreds of milliseconds: 2,310, 2,296 and 2,878 ms).
`abab.py` has Piwigo's path and digest as constants today and is parametrised by corpus and digest, and item 9's
wall, Ir and heap comparisons measure a second corpus (a vendor project through `check-project` and
`incremental-bench`) beside Piwigo, since the target is general PHP.

**Risks.** The registry dump's field list moves with the registry (REG-4.x already changed it), so
the printers destructure everything: a new field is a compile error in the dump, not a silent gap. A
dump that costs minutes on Piwigo is acceptable; one that differs between runs is a finding to triage
(engine or dump). Nothing here gates on Piwigo alone: each dump runs over the vendor projects and the
fixtures too.

## I2. Token payloads under decision 4

**The problem.** 20 G item 10 gives `TokenStream` a `payloads: Vec<Payload<'a>>` holding `&'a Name`, `&'a [u8]`
and `&'a str` into an arena. Decision 4 dropped the arena (a `bumpalo` arena is not `Sync`), and the tree's name
table and side buffers (B3) grow while the parse runs, so no reference into them can be held across a push. The
stream cannot hold ids either without the lexer calling the builder for every token.

**The design: a token is a kind and a span, and its text is read at the span when the parser takes it.** The
parser holds the source it parses (it does, the worker's read buffer); the builder interns a name, takes a span of
the source or appends decoded bytes to a side buffer at the moment a node needs it. Nothing is materialized at lex
time that a node may not want.

```rust
pub struct TokenStream {
    kinds: Vec<TokenKind>,     // one byte: the 64 punctuation and structural kinds, the 62 keywords, and the new classes below (Cast, QualifiedName, Number, the string kinds, Error)
    spans: Vec<Span>,
    docs: Vec<DocMark>,        // each doc comment: the index of the token it precedes, and its span
}
```

A template or heredoc carries only its kind and its span: its parts are built when the literal is taken, so a skimmed
body pays for none.

| class | the lexer records | the parser's `take` does |
|---|---|---|
| identifier, keyword used as a name | kind (`Identifier` or the keyword's); the `WatchList` bits are set in the file facts from the spelling, by the perfect-hash table of SYN-4.1 (`keyword_slot`, an existing `const fn`, is the precedent for it) | `b.name(&src[span])`: hash once, probe the tree's name table, intern into the global interner on a miss |
| variable | `Variable`, span including the `$` | the name is `&src[span][1..]` |
| qualified name written without spaces | one `QualifiedName` token (SYN-4.8) | `b.name` of the slice, split at the last `\` by the header |
| number | `Number` (the lexer validates the lexeme and records an error for `08`, `1__0`) | parse the digits skipping `_` (SYN-4.9); an integer past `i64` becomes a float, as now |
| single-quoted string | `StringPlain` or `StringEscaped` (does it hold a `\\` or `\'`) | a span of the source for `StringPlain`, with no copy; decoded into `side_bytes` for `StringEscaped` |
| double-quoted string without interpolation | the same two kinds | a span when it holds no escape; otherwise decoded into `side_bytes` (PHP's escapes) |
| double-quoted string, backtick string, heredoc, nowdoc | `Template`, `ShellTemplate`, `Heredoc` or `Nowdoc`, the span of the whole literal, and a flag per token of the string family: has-escape, interpolates, **has-braced-part** (the skim reads this bit) | scan the span with the code that is `scan_interpolated` now and build the parts: a part is `Literal(span, needs_decode)`, `SimpleVar(span)`, `Property { var, prop, nullsafe }`, `Index { var, key }`, `NameIndex { name, key }` or `Complex(span)`, all spans of the source |
| heredoc indentation | nothing | the closing indent is read back from the closing line at the end of the span, and after each LF inside a literal part up to that many space or tab bytes are dropped before the part is decoded by flavour (double-quoted and heredoc decode `\"` differently, backtick decodes `` \` ``, nowdoc decodes nothing); a part whose first literal starts after the first line's indent keeps its span there. Today `strip_heredoc_indent` runs on the whole body before `scan_interpolated`, and a pinned test has `Literal "!\n  "` over `(3,17)-(4,7)`; PHP refuses an under-indented body line ("Invalid body indentation level"), which today is silently cut |
| inline HTML | `InlineHtml` (text only: before `<?=` or the end of the file) or `InlineHtmlOpen` (the text and the `<?php` tag that ends it: the span includes the five bytes of the tag, as `lex_inline_html` advances past them before it returns, which `checker/whitespace.rs` and the statement's span depend on) | a span of the source (less the tag for `InlineHtmlOpen`) |
| `<?=` | the `Echo` keyword with the span of `<?=`, as now | nothing |
| `?>` | `CloseTag`, the span of `?>` and the optional newline it absorbs (today `Token::Semicolon` with that span); the parser treats it as a statement end, and G3.4's stoppers can tell it from `;` | nothing |
| a cast `(int)` | `Cast`, span including the parentheses. **The lexer rule, exactly:** the parentheses hold a case-insensitive cast name between optional spaces or tabs and nothing else (php 8.5.4: `( int )`, `(INT)` and tab-padded forms are casts; a newline or a comment inside is not) | the spelling is `&src[span]` between them, spaces and tabs trimmed (`CastSpelling::from_name`) |
| everything else | the kind | nothing |

Two deltas follow from the cast rule, each with a fixture and approved with the others at batch 1: today
the parser recognises a cast by a three-token lookahead that allows comments and newlines between the tokens, which PHP
does not; and `foo(string);` lexes as a cast token and is a parse error, as PHP reports it, where it is a call with a
constant argument today. The oracle excludes `Cast` from its "exact" claims.

- **No payload array.** The 20 G design's `payloads` is replaced by the span every token already has.
  `Payload`, `Token`'s `Arc`, `String` and `Name` fields, `Tok`, `DocGroup`, `Supply`, `next_raw`, `next_tok`,
  `fill`, `pop`, the `VecDeque` and `raw_keyword` go; a keyword's spelling is the source at its span.
- **Fragments.** An interpolation fragment (`{$a['k']}`) is lexed, when the literal is taken, into a scratch stream
  taken from a small stack of them, over the same source with absolute spans, because the file's own stream is
  still in use. The parser's sub-parser reads that stream, as `build_interpolated` does now; the
  `ComplexExpr(String, u32)` text copy goes, being a slice of the source, **except where it was never a slice**:
  `${name[expr]}` is rewritten today to `$name[expr]` (the braces dropped), `${expr}` is re-spelled, and a bare
  `$a[key]` whose key fits none of the three simple shapes is rebuilt with quotes added (a pinned test has
  `"$arr[1a]"` as an index with the key `1a`, which php 8.5.4 refuses, as it refuses `"$arr[foo bar]"` and
  `"$arr[ 0 ]"`). So: `${name[expr]}` is a part of its own (`NameIndex`: the name span and the bracketed expression
  span, lexed as a fragment and built as `Index(Variable, expr)`); `${expr}` is the slice from the dollar to the
  closing brace; and the bare fallback follows PHP: a lexical error (G3.2 item 3), with the pinned test replaced by
  an error test, an approved delta with its `gates/test-renames` entry.
- **Doc comments** are `DocMark { before: u32, span: Span }`. The doc before the current token is the run of
  marks at its index (a cursor beside the token cursor); a single doc is a span of the source, and several are
  folded when a declaration or statement asks, the folded text written once into `side_text` for the group,
  however many statements share it. The rules
  `DocGroup` carries today are kept: a declaration takes the last mark, a statement the folded text (or the last);
  a doc before an attribute list wins and carries no `first_start` (`parse_leading_attributes` re-keys it into a
  fresh group), so the parser holds it as pending state across the attributes.
- **Lookahead at any distance** is `kinds[pos + n]`. **Skimming a body is the three-bit state machine it is today,
  over kinds:** brace depth opens on both `{` and `${`; the bits are *after a member operator* (`::`, `->`, `?->`),
  *after `function`* and *after `PHPStan\`*; and the triggers (the file reads the body at load) are `Eof`, inline
  HTML, a template with the has-braced-part bit, `function` followed by a name, `class`, `interface`, `trait`,
  `enum`, `global`, `include`, `include_once`, `require`, `require_once`, `yield` and `goto` (none of them after
  a member operator), `define`, and the `PHPStan\` prefix, the last two read from the identifier's source only
  for identifiers that begin with `d` or `p`. The oracle is `parse.rs`'s six skim tests, with `${$x}` and
  `QualifiedName` cases added. A skimmed region costs no interning and no string decoding, where today every
  token of it is lexed to a value and dropped.
- **Buffers** are per worker thread, cleared between files and emptied when the load ends; `reserve` starts from
  the source length divided by a constant the census sets (about 4 bytes per token at the project's average).
  **A retained-capacity cap** applies to the token arrays and the builder's vectors, as `KEPT_READ_CAPACITY`
  (1 MiB) does for the read buffer: capacity above it is released after each file, so one 100 MB file does not
  hold nine bytes a token on a worker until the load ends, and a 100 MB file is in the peak-RSS checks of step 3
  and S-7.
- **Errors** (G3): the lexer records a `SyntaxError` with the token's span and kind `Error` where PHP refuses the
  lexeme, and the parser treats that token as a literal operand. A NUL byte is an `Error` token and not `Eof` (the
  stream ends at the real end only), and an unterminated string's span is clamped to the source (G3.2).
- **The stream ends at `__halt_compiler`.** The lexer ends the stream at `__halt_compiler`, `(`, `)`, then `;` or
  `?>` in statement position (not after `->`, `?->`, `::`, `function`, `const` or a backslash), gives the final
  `Eof` the file-end span (the `Block` statement's span reaches the end today), records the data offset (the
  checker already knows `__COMPILER_HALT_OFFSET__`, `checker/global_constants.rs`) and lexes nothing after it:
  today the parser calls `skip_to_eof` (`lexer.skip_to_end()` sets `pos` to the end of the source without
  lexing), and a whole-file lex would otherwise lex binary PHAR data and report errors in it. A test whose payload
  holds `08`, `1__0`, a lone quote, `<<<EOT`, NUL and control bytes gives no warning, no error and the same
  tree.
- **The lexer's result** is `Lexed { stream: TokenStream, ignore_marks, warnings and errors, facts, line_index,
  comments (optional) }`, which replaces `Lexed`, `LexOutput` and `LexEnd`; the entry points that survive are the
  whole file, a fragment and in-PHP (the `tokenize_with_spans` and `with_comments` of `lsp/completion.rs`,
  `stub_attributes.rs` and `output/codeframe.rs`), and the shared name cache `parse_with_names` takes today
  (`NameCache`) is the builder's `b.name` path. **Ignore marks and file facts** are recorded in the lexing loop as
  now.

**What it changes in Part B.** B5's "until SYN-2 the lexer still makes a `PhpString` per literal and the builder
copies it" ends: after SYN-2 the lexer never touches the builder, and the builder's `name` and `text_span` are
called only by the parser. B3's side buffers are filled at take time and a span costs nothing, so a skimmed body
and an unclaimed doc add nothing to them. The token dump of B8 prints (kind, span, the text at the span, and for a string kind the decoded bytes through
the builder's own decode). **It is not "by construction" the same dump as the old tokens':** the new stream
differs from the old in an explicit list, and each difference is a delta with its fixture, or is mapped by the dump's
kind-name function to the old name: `Cast` replaces three tokens (and the two deltas above), `QualifiedName`
several, `Number` merges `Integer` and `Float`, six string kinds replace three, `CloseTag` was `Semicolon`,
`InlineHtml` and `InlineHtmlOpen` keep the old span, a NUL is an `Error` token and not `Eof`, and the stream ends at
`__halt_compiler`. The B8 baseline and this dump also cover a CRLF, a lone-CR and a BOM-prefixed copy of every
corpus file, made in memory (the corpora are LF-only, and the lexer's newline-sensitive sites, a `//` or `#`
comment's end, the newline `?>` absorbs and a heredoc's closing label, take the one terminator function of I3).

**Readers of token lists outside `syntax`:** `stub_attributes.rs` (`parse_native_type`), `output/codeframe.rs`,
`lsp/completion.rs`, `xtask/src/front_end.rs`, the `lex_parse` bench and the test helpers, and the fuzz targets
`lex_parse` and `token_stream_parse`, read `kinds`, `spans` and the source they hold; `Parser::new(&TokenStream,
src)` replaces the test constructors. **`token_stream_parse` is rebuilt,** because it feeds `Parser::new` an
arbitrary `Vec<Token>` with every span default today: it generates arbitrary kinds, prints each as a canonical
lexeme into a source and lexes that, and a `TokenStream` validator (spans inside the source and ordered, a flag
byte on every string-family kind) runs in `Parser::new` in debug builds and is satisfied by the generator.

**Oracle.** O1, O2 and the token dump; the property test that lexing a file and parsing it equals the old
streaming parse on every corpus file (the tree dump); a fuzz target that compares the old and new lexer's
(kind, span) pairs until the old one is deleted (one of the policy's listed exceptions, #13). **Saving:**
the 20 G figure, about 1.7 G, from the plumbing alone (257 Ir per token, 1.77 G), and what a skimmed region no
longer interns or decodes, which no one has measured. **Measure, with a threshold:** callgrind of
`parse_with_ignores` and the existing criterion bench "lex phpstorm-stubs", off and warm, against the base
commit; whole-file lexing adds work the streaming parse did not do (a failed parse lexes the rest, a skim lexes
everything), so the accept is that lex plus parse Ir together fall by at least half of the 1.77 G plumbing
estimate (on the stubs and Piwigo), and if that work eats the saving the design is reconsidered (a bounded-batch
lexer that the parser can stop is the alternative); a measurement that does not show the saving is a finding, not
a claim.

## I3. SYN-3.2's kernel

Read from `syntax/src/span.rs` (`LineIndex::new`: `memchr_iter` for `\n`, then `Utf8Chunks` for the columns of a
non-ASCII file) and `wide` 0.7.33's source (`~/.cargo/registry/src/*/wide-0.7.33`), which #10 chose on merit
(it is also already in the build through `imbl`).

**What `wide` gives, and what it does not.** `u8x16` has `cmp_eq`, `max`, `min`, saturating add and subtract,
`move_mask` (the high bit of each byte, one bit per lane) and `any`/`all`/`none`. It compiles to SSE2 where the
target has it (baseline x86-64), to NEON on aarch64 and to a scalar `[u8; 16]` elsewhere, chosen at compile time
(`pick!` over `target_feature`). It has **no unsigned greater-than** on `u8x16`, and `u8x32` is real AVX2 only if
the build enables it, which the baseline build does not. So the kernel is built from equality masks and the sign
bit, in 16-byte blocks, with no run-time dispatch (#10's answer).

**The kernel.** One pass over `src` produces the line starts and whether the file is ASCII:

```rust
pub(crate) fn scan_lines(src: &[u8], rule: Terminators) -> (Vec<u32>, bool)   // starts, ascii
```
per 16-byte block: `v = u8x16::new(block)`; `lf = v.cmp_eq(LF).move_mask()`; the vectors are OR-ed (`BitOr` exists
on `u8x16`) and one `move_mask` after the loop decides `ascii` (a `move_mask` per block for the high bits would
cost two or three per block on NEON); and, for the rule that counts `\r`, `cr = v.cmp_eq(CR).move_mask()`. The
block's line ends are `lf` for `Terminators::Lf`
and, for `Terminators::Php`, `lf | (cr & !(lf >> 1))` (a `\r` not followed by `\n` ends a line; `\r\n` ends it at
the `\n`), with one bit of carry across blocks: a `\r` in a block's last lane is a line end only if the next
block's first byte is not `\n`. Each set bit `i` gives the start `base + i + 1`, taken by `trailing_zeros` and
`mask &= mask - 1`. The tail (under 16 bytes) is copied into a zero-padded block and its masks cut to its length.
A start is a `u32` that cannot wrap, since the loader refuses a source of 2^28 bytes or more (B5).

- **One rule, applied everywhere a newline matters.** PHP ends a line at `\n`, `\r\n` or a lone `\r`, and the
  lexer applies it in more places than the line index (php 8.5.4 was run: a `//` or `#` comment ends at a lone CR,
  `?>` absorbs a lone CR, and a heredoc's closing label is found after a lone CR). Today the lexer ends a line
  comment at `\n` or `?>`, absorbs only `\r\n` or `\n` after `?>`, searches heredoc lines by `\n`, and the splitters
  (`codeframe.rs`, the source-line helpers) split on `\n`. So `Terminators` is one function used by the line
  index, the lexer's comment, close-tag and heredoc/nowdoc sites, the shebang skip and the splitters, and SYN-3.2
  ships `Php` in batch 2 with the lone-CR fixtures and their approved delta (a lone-CR file's comment no longer
  swallows the code after it, and its heredocs close): shipping `Lf` and leaving `Php` to F1's text, which
  changes "the lexer's line starts and the line index" only, would leave line numbers following CR while the
  comment swallowed the code after it, and would carry an unused enum arm that `clippy -D warnings` must be told
  about. `CRATE_SPLIT_PLAN.md`'s F1 text is edited to say so. The oracle is a scalar loop; for the old rule also
  the `memchr` version it replaces.
- **Types (SYN-3.1).** `LineStarts { starts: Box<[u32]>, len, ascii }` for every file, from the scan, one
  allocation (`into_boxed_slice` after the exact fill). `LineIndex` is `LineStarts` plus the column tables, built
  for analysed files and the server's open documents only; it is built from the starts and the source, and the
  tables are skipped when `ascii` is true.
- **The column tables** are built from `first_non_ascii(bytes, from)` positions (`move_mask` and
  `trailing_zeros` over the same blocks) with a decoder that looks only at a window of at most four bytes at
  each non-ASCII byte (the maximal ill-formed subpart is one character: `Utf8Chunks`' rule) and, for a source the
  load already knows is valid UTF-8 (`simdutf8` in `source::decode_text`; the lexer holds `text: Option<&str>`),
  takes the width from the lead byte alone. **Keeping `Utf8Chunks` and replacing only its position search would not
  deliver the saving:** `Utf8Chunks::next` is itself a byte-at-a-time scalar decoder over every byte, ASCII
  included (a copy of std's loop is in `bumpalo`'s `lossy.rs`; std's own current source was not readable here),
  so for a valid file one `next()` walks the whole file and the position search then walks it again, and the
  validation walk is most of the 83.4 M the column tables cost. The current `Utf8Chunks` version stays as the
  oracle on random invalid UTF-8, and what the column tables save is restated after that.
- **SYN-4.3's blank runs** use the same blocks: a mask of the bytes in the lexer's blank set (`cmp_eq` for each
  member, or-ed), and the run's length is the number of trailing ones (`(!mask).trailing_zeros()`), crossing to the
  next block while the whole block is blank.
- **Dependency:** `wide` becomes a direct dependency of `syntax`, pinned as the workspace pins its others
  (`=0.7.33`; Zlib or Apache-2.0 or MIT; it uses `safe_arch` and `bytemuck`, and holds `unsafe` of its own too:
  the `transmute` in `u8x16::new`, the NEON intrinsics, `unsafe impl Pod`). No `unsafe` of ours.

**Tests.** The scan against the scalar oracle on: every string of length at most 6 over `{a, LF, CR, 0x80}`
placed at each offset 0 to 33 inside filler of `a`, CR and LF (so the content, not the memory alignment, moves
across the 16-byte block grid: blocks are cut from the slice start); lengths 15, 16, 17, 31, 32 and 33 ending in
CR; a CR in lane 15 followed by a non-LF, by LF and by CR LF; a block of only CR LF pairs; CR at the end of the file
with a length that is a multiple of 16; the ascii flag with 0x80 in lane 0 and in lane 15; 100,000 strings of
length 0 to 200 over `{a, LF, CR, 0x80, 0xff, 0x00}` from a fixed-seed generator; files that end in `\r`, `\r\n` or
`\n`; and the corpus. The suite runs on the aarch64 and Windows legs and says so (`wide`'s scalar branch is never
forced on a leg; only the leg's own target is covered). `LineIndex::line_col` and `offset` on the multi-byte and
astral lines the existing tests use, built through the new path, equal the old. The blank-run and
`first_non_ascii` helpers against their scalar definitions the same way.

**Measure.** Off callgrind of `LineIndex::new` before and after, and the main thread's share: 270.4 M over 19,211
builds in the 20 G profile (63.5 M for 1.68 M `memchr` calls, 83.4 M for the column tables, 116.3 M of its own
and 7.2 M unattributed); the ledger's later 363.4 M is over 25,753 builds, stub files included, with no
breakdown; the baseline is re-measured before the change. The estimate that the scan costs about one
instruction per byte and under ten per line is a model made for SSE2; no saving is claimed before the run.
**Accept and kill:** the kernel beats the `memchr` plus `is_ascii` version by at least 20% (proposed) in Ir and in a
criterion bench of `scan_lines` on x86-64, and is not slower on the aarch64 runners (macOS and Linux) or on
Windows (R16's method: a criterion bench on each leg, since instruction counts are not time), where `move_mask` is four
NEON instructions and a horizontal add and `memchr` itself avoids it. Otherwise `memchr` stays and `wide` is
dropped, or the same API is written over `u64` words (eight bytes at a time, the classic zero-byte test). There is
no scalar-fallback trigger: `wide` takes its scalar branch only where neither `sse2` nor `neon` is a target
feature, which x86-64 and aarch64 never are.

## G3. The error-node model and parse recovery

G3 is the design of batch 3's recovery: the error-node model (G3.1 to G3.5), the contract for readers (G3.6) and the oracles (G3.7).

### G3 why now

Decision 4 says every parse yields a tree; Q-10 and decision 15 say what a failing file
keeps and how its version range is read; F77 says the failure is reported. Step 2 fixes the stored
layout and the builder's API, and batch 3 builds the recovery on them, so the model comes first.

### G3 what exists (verified)

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

### G3.1 Contract

`syntax::parse(bytes) -> Parsed` is infallible. `Parsed` holds the `Ast`, whose
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

### G3.2 Three kinds of failure, and one thing that is not a failure

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

### G3.3 Layout, for step 2

Three enum arms, each with a payload of one `ErrorId` (4 bytes, inside
the 32-byte node budget), added with SYN-1.9: `ExprRepr::Error`, `StmtRepr::Error` and `HintRepr::Error`
(no `MemberRepr`: item 2 of G3.2 says where a class records its errors). `Ast` gains `errors:
Vec<SyntaxError>` and `marks: Vec<Mark>`. Step 2 does not build them: the parse functions keep
`Result` and the first failure ends the parse, so that step 2 is checked by dump equality; batch 3
replaces each `?` with the recovery below. What step 2 must guarantee is only that the arms fit: an enum
is as wide as its largest arm, and the largest arms of `ExprRepr` and `StmtRepr` are 20 and 28 bytes
(I1.2, I1.3), so a 4-byte payload relays out nothing; `HintNode` has room in its 20 bytes.

### G3.4 The recovery (batch 3)

A parse function returns its node, never a `Result`.
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

### G3.5 Marks: what a file used that a version lacks (decision 15)

`Mark { feature:
SyntaxFeature, span: Span, detail: MarkDetail }`, recorded by the parser where the grammar uses a
feature, deduplicated by span, sorted. `SyntaxFeature` is the grammar-only subset of `types::PhpFeature`'s
45 variants plus those below, defined in a leaf crate both `syntax` and `types` depend on (`php-version`:
`PhpVersion`, `PhpVersions`, `VersionSet`, `Availability`, `SyntaxFeature` with its one threshold table,
`KNOWN_PHP_VERSIONS`; #32), because the parser is below `types` in the graph; `PhpFeature` in `types`
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
parameters (8.0), enums and `never` and `0o` octals (8.1), the pipe operator (8.5). The list is provisional:
**the matrix of G3.7 is its authority**, and a row is not final until the
images agree with it.

### G3.6 What readers do with an error (the contract)

- *Checker.* `ExprKind::Error` infers the error type (F21's `Mixed(Error)`, which lands in batch 0: every "is this unknown" guard already covers it, and it records that a report was made), narrows nothing, reports nothing. **What a statement error sets:** `any_variable_may_exist`
  (set by `extract()`, `eval` and `include`) is read today only at `builtin_calls.rs:826`, and the diagnostics
  `extract()` and `eval` discard are dropped by `saw_dynamic_var_construct`, which drops every diagnostic of
  the enclosing body (a loss for an editor). So a statement error sets `any_variable_may_exist`, and
  `variable.undefined` honours it: a read of a variable the swallowed region may have assigned reports
  nothing, which also changes `include` (it sets the flag and still reports such a read today), a delta for
  VAL-2.10 with its fixture; completion falls through. This is PHPStan's own rule (`canAnyVariableExist()` is true
  at global scope and after an include or `extract()`: `afterExtractCall`), and the stored Piwigo baselines hold no
  `variable.undefined`, so the oracle should not move. **`extract()` is made precise in the same commit:** every variable in scope becomes `mixed` and any may exist, and nothing else is dropped, where
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
  per-file cache and reported as a run-level notice, `phpstanRs.contextParseError`: its
  path, line and column and the first error, printed where today's "N file(s) skipped: parse error" count is, with
  the exit status unchanged and an opt-in switch that makes it fail the run. A failing run would fail on a vendor file the parser refuses. Decisions 9 and 15 ("nothing in a context file is reported,
  since its author is not the user") are read with this notice, which is not a finding
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

### G3.7 Oracles and tests

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
   everything. **The bar for checking an analysed file with parse errors (#31) is stated
   here:** over the same corpus, for damage to a single token, the diagnostics that are
   new, meaning absent from the undamaged file's own and not parse errors, are none at the 95th percentile of
   damaged files, measured per damage class; a class that misses falls back to parse errors only for the files it
   damages.
4. *Golden dumps* of about 150 hand-written broken snippets, one row per recovery rule of G3.4, in the
   canonical dump of Part B8.
5. *Editor tests*: completion, hover and symbols on buffers broken in each way (G4b).

### G3.8 Staging

(a) Step 2 as G3.3 says. (b) Batch 3, in this order: the lexer's errors and tokens
(the new ones: unterminated strings, comments and heredocs, the NUL byte as an `Error` token, the clamped
span); `SyntaxError`, `errors()` and the infallible signatures with the first-error behaviour unchanged for
valid input (dump equality); the statement, expression, list, class-body and header rules, each with
its golden dumps; the `php-version` crate and the marks; the matrix; the readers (G3.6); reporting
and the editor. (c) SYN-6's rows are marks and error kinds in that table, not a separate pass.

## H1. Identity

H1 is what batch 4 is built to. Its build (H1.7) takes rows that C1's table already schedules: NAME-1.1 to 1.6, NAME-2.1 to 2.3 and SYN-4.5, all in batch 4. CHECK-13 is decided in H1.4 (anonymous classes reach the class-shape parent-call rules) and built in batch 5 with PROD-5.1, whose file-local anonymous-class identity its rule needs.

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

## I4. The single grammar's lowering

F41 (`STAGE_B_FIX_REVIEW.md`), DOC-2.1 (the ledger) and C7's grammar item are one piece of work. (Finding numbers
are ambiguous across the two review files: F40, F41, F51, F61 and F63 exist in both with different content; this part
cites `STAGE_B_REVIEW` F40, the PHPDoc parsing one, and `STAGE_B_FIX_REVIEW` F41.) Sources: `types/src/parse.rs`
(1,429 lines, outline and the keyword table), `phpdoc/src/grammar/ast.rs` (`TypeNode`, 19 variants),
`phpdoc/src/doc_type.rs` and `registry/type_node.rs`, and the call sites (58 mentions of `parse_written_type`
in 3 crates, 219 of `parse_type_string`, 156 of them in `types`' own tests).

**Today.** A PHPDoc type is parsed twice. `phpdoc::grammar` lexes and parses it into a `TypeNode` (and reports
`phpDoc.parseError`); then `DocType::ty` renders the node back to text (`to_source_string`) and
`types::parse_written_type` parses that text again with a second, string-splitting parser, memoized by the text
(2,048 entries in each of 64 shards: 131,072 entries and 64 MiB of text at most). Each of F41's bullets is a place
where one of the two gets the language wrong. `types::parse_display`
is a third reader, of a different language (a type's `Display`, which `assertType()` and the type dumps are written
in); it stays, being `Display`'s inverse.

### I4.1 Where it lives

- **The lowering is `phpdoc::lower`.** `phpdoc` depends on `types` and `syntax`; `types` depends on `syntax` only.
  F41 considered moving the grammar into a crate below `types` so `types` could keep a text entry point. It does
  not need one: outside its own tests `types` calls its parser only from `parse.rs` itself. So `types` loses its
  parser, as the crate split's §3.3 says ("`types::parse_type_string` deleted"), and the grammar stays where
  DOC-2.1 put it. No new crate.
- **The type knowledge stays in `types`:** the keyword table (`parse_type_keyword_or_class`, about 380 lines of
  `int-mask`, `class-string`, `non-empty-lowercase-string`, `key-of`, ...) becomes `types::phpdoc_keyword(word, args:
  &[Type]) -> Option<Type>`, taking lowered arguments where it takes argument text now, and the builders
  the string parser's `try_parse_array_shape`, `try_parse_object_shape` and `try_parse_callable_shape` use become
  public constructors (`Shape::from_fields`, `CallableSig::from_parts` and the like) that `lower` calls. The string
  splitters (`split_top_level`, `top_level_find`, `matching_close`, `try_parse_conditional_type`,
  `parse_type_atom`) are deleted. **The extraction also completes the table** against `TypeNodeResolver`'s
  identifier and generic case lists, with a fixture per keyword: the string parser lacks `number`,
  `empty-scalar`, `non-empty-scalar`, `non-empty-literal-string`, `associative-array`, `static-closure`,
  `static-pure-closure`, `non-empty-mixed`, `callable-object`, `callable-array`, `empty`, the `__string*` pair,
  and the generics `__benevolent`, `new` and `template-type` (checked in the vendored `phpstan-src` at 2.2.16,
  and again at 2.3.1, the latest release, where every one is still in `TypeNodeResolver`), so each reads as a class today, is namespace-qualified and
  reaches `class.notFound`. `number`, like `scalar` and `numeric`, yields to a class or a `use` alias of that
  name (PHPStan's `tryResolvePseudoTypeClassType`), the one scope-dependent part, which C7 does; the rest of
  C7's "PHPDoc keyword types resolve before namespace qualification" is done here, and leaves C7's and RES-1's
  lists.
- **Entry points** in `phpdoc` (one name across #14, F41 and here): `lower(&TypeNode) -> Type` (names as
  written, a leading `\` kept: what `parse_written_type` returns), `parse_phpdoc_type(text) -> (Type,
  Vec<ParseError>)` (lex, parse, lower) and `parse_global_type(text)` (the old `parse_type_string`: leading `\`
  stripped). The text entry points return the errors, because a string a user supplied must not turn into the
  error type, which accepts and is accepted by everything, with no report: `checker/constant_resolver.rs`,
  `restricted_symbol_usage.rs` and `registry/assertion_table.rs` parse user-supplied texts today with no error
  channel, and those callers report. Fixed texts (`predefined_constants.rs` parses nine per call) are statics
  (a `LazyLock` table, as 25 G says), and the text memo goes unless a dynamic caller measures a need for it. An
  error node lowers to the error type; a tag with an error contributes nothing when PHPStan parses that tag
  family (a `phpstan-` prefixed tag, or one in `PLAIN_TAGS_PHPSTAN_PARSES`), and the others apply as they are
  (`docblock.rs`). A *missing-token* error whose recovery is unambiguous (a `callable` with no return type, I4.2)
  is not an error node: the grammar reports it and the lowering keeps what the author wrote, so the tag keeps the
  recovered type.
- **`lower` is a walk generic over a scope** (the identity scope gives `lower`), living in `phpdoc`, so that C7
  adds an implementation (names, templates, aliases and `self` resolved in the walk) and not a second walk. It is
  written against the node accessors that DOC-2.7 keeps (children by iterator), so DOC-2.5 and 2.7 change the
  node's storage and not `lower`; after C7 `DocType` keeps the node, and the resolved type is DOC-2.2's memo value.

### I4.2 The lowering, by `TypeNode` variant

| node | lowers to |
|---|---|
| `Identifier(w)` | `phpdoc_keyword(w, [])`, else the class of that name as written; `$this`, `self`, `static`, `parent` as now |
| `Nullable(t)`, `Union`, `Intersection`, `Paren` | the normalizing constructors (`Type::union_of`, `intersect_types`); `?T` is the normalizing union of `T` and `null` (the string parser builds a raw nested union, a delta: flattened), and `?mixed` is `mixed` (tested by `is_mixed()`); **the written width is bounded:** `union_types_many` flattens every operand with `push_member`, a linear scan per member, and runs its passes before `MAX_UNION_MEMBERS` applies, so a written union of n distinct members costs at least n²/2 comparisons, and `eager` `DocType` runs the lowering for every docblock read; the lowering caps the written width at a multiple of the cap and generalizes as the cap does (or dedupes through a hash set before the passes), with a 50,000-member union, a 100,000-deep nest and colliding-name inputs (`FxHasher` does not resist collisions) in I4.5 step 3's inputs and S-1's |
| `Generic(w, args)` | `phpdoc_keyword(w, lowered args)`, else a generic class reference; `int<a, b>` is a range only for the exact word `int` (the string parser tests `ends_with("int")`, so `Point<1, 2>` becomes a range today, a delta); an argument of a keyword generic is lowered as the type it is, and a wildcard or a variance marker there is accepted as PHPStan does: phpdoc-parser turns `*` into `mixed` with bivariant variance and PHPStan's `array`, `list` and the other keyword generics ignore variances, so `array<*, int>` is `array<mixed, int>`; only the class arm used them (`parse_generic_arg`), and today a keyword argument that holds one fails, a delta |
| `Wildcard`, `VarianceArg` | for a class generic as `parse_generic_arg` does: any type; call-site variance |
| `ArrayOf(t)`, `OffsetAccess(b, o)` | `array<t>` (list-ness as the string parser gives it today), the offset-access type with any type as the offset (the string parser read it as a shape key by name, a delta) |
| `ArrayShape { keyword, fields, sealed, unsealed_type }` | the shape constructors with `list`, `non-empty` and `object` refinements kept (`STAGE_B_REVIEW` F40) |
| `Callable { keyword, templates, params, return_type }` | `CallableSig` with its own templates marked; **without a return type** (`callable(int)`) the grammar records a PHPDoc syntax error for the missing return type, as phpdoc-parser and PHPStan do (at 2.3.6 `parseCallable` consumes the colon unconditionally, so it is mandatory there; the port's `None` branch carries no note that its acceptance is deliberate), and the node is lowered as written, `callable(int): mixed`, so the tag keeps the parameters the author wrote, where the string parser's fallback, a bare `callable`, discarded them (the missing-token recovery of G3.2 item 1, applied to PHPDoc); `Closure(...)` is the intersection of `\Closure` and the callable (`parse.rs`), not the bare class |
| `IntRange`, `IntLiteral`, `FloatLiteral`, `StringLiteral` | the literal types; a float only from a `FloatLiteral` token |
| `ConstFetch { class, const_name }` | the constant-fetch type, a trailing `*` kept; `Foo::class` is `class-string<Foo>` (`Type::class_string`, as the string parser has it) |
| `Conditional { subject, negated, target, if_true, if_false }` | `ConditionalSubject` and the three lowered branches (each branch is the node the grammar built) |
| `Error` | the error type |

The explicit `mixed` of a node is the kind F21 gives (`Mixed(Explicit)`), set at the keyword by `phpdoc_keyword`, as
the string parser already does at `Type::explicit_mixed()`; the text search for explicit `mixed` that C7 mentions is
gone. Template and alias references are marked by the same later passes as now, until C7's resolver
takes the name scope.

### I4.3 What changes in behaviour (F41, with F19 and F40's grammar items)

`lex_number` follows phpdoc-parser's lexemes (`Lexer.php`, read at 2.3.5 and unchanged at 2.3.6, the latest release; its patterns are matched with the `i` modifier): an
optional sign, `0b`, `0o` and `0x` integers and plain decimal digit runs, all with `_` separators and
case-insensitive (`0X1F`, `1E5`, `0B1`), and floats with a fraction or exponent, including a trailing dot (`1.`).
There is no leading-`0` octal in phpdoc-parser (`017` is a plain digit run). The values follow PHP's source
reading and deliberately differ from PHPStan's `(int)` and `(float)` casts of the lexeme (php 8.5.4: `0x1F` reads
31, `017` reads 15 and `0b11` reads 3, where `(int)"0x1F"` is 0 and `(int)"017"` is 17); an integer past `i64` is a
float, as PHP reads such a literal, but `-9223372036854775808` is the int `PHP_INT_MIN`: the
sign is part of the PHPDoc lexeme, where in PHP source the literal overflows before the minus applies (php 8.5.4:
the source literal is `float(-9.2233720368547758E+18)` and `(int)"-9223372036854775808"` is
`int(-9223372036854775808)`), and a float would make `@return -9223372036854775808` reject `PHP_INT_MIN`.
Consequences, each with a fixture: a class named `Infinity`, `Nan` or `inf` is a class
(it was `INF` or `NAN`); `@param 1_000 $u` and `0x1F` parse (they gave `phpDoc.parseError`); `1e25` is `1.0E+25`; a
conditional's branches may hold `\Closure(): int`, `self::A` and `?T` (the first-`:` split gave `class.notFound`).
The string parser's other known errors: `value-of` and `key-of` of a list (parsing saw only a bare array or
shape; the lowering lowers the argument as a type, so `value-of<list<int>>` is `int`), and the differences listed
in I4.2 (each a delta with a fixture, approved with the PHPDoc-lowering deltas; the consequences above are
plain bug fixes). `numeric` and `non-decimal-int-string` already have their right types in
`parse.rs` at this commit, and a bare `class-string` stays a plain `string` by design (the comment there says it
has nothing to preserve); what needs the name scope is C7's. `TYPE-5` is not part of this: it is its own commit
after step 5 (below).

### I4.4 The consumers

- **`DocType`** holds the node and the lowered `Type` made when the docblock is read (eager, DOC-2.1): the `OnceLock`
  and the text memo go, and `WrittenType` for a bare `TypeNode` lowers it.
- **DOC-2.2's memo key** needs no node identity. Written types are hash-consed, so the lowered `Type`'s node is the
  identity of the written type (two docblocks that write `array<int, Foo>` share it), and the resolution context is
  interned (PROD-5.2's `ImportScope`, the template names, the aliases, the `self` names). The key is the pair
  (the interned `Type` itself, which the entry holds so its address cannot be reused, and the interned context's
  id): equality is pointer identity on exact forms and costs 8 bytes, and the value is the whole resolved type.
  A key made from a 128-bit fingerprint of the node's `exact` hash would be 64 bits of `FxHasher` from the
  node, so a collision would hand back another type's resolution with no equality check, and `Node.exact` is
  `pub(crate)`. If DOC-2.8's fingerprint is wanted it is a 128-bit hash over the node's structure behind a public
  `Type` API. A test: `\StdClass` and `\stdClass`, `a|b` and `b|a`, and explicit and implicit `mixed` get
  different keys.
- **The 58 `parse_written_type` and the Rust-side `parse_type_string` sites** (about 20 files of `phpstan-rs`) become
  `phpdoc::parse_phpdoc_type` / `parse_global_type` where the type is written as text for a reason (a builtin's
  return rule, stub synthesis; `predefined_constants.rs` takes statics), and constructors where it is a literal
  `Type::int()` in disguise.
- **`types`' 156 test mentions:** a test that reads a type from text moves to `phpdoc/tests`, where it can use the
  grammar and `Type`'s equality; a test of the relation, a union or a shape that only needs a type uses constructors
  (the crate split's "about 150"). The relation tests keep their text form in the crate that has a grammar.

### I4.5 Order and checks

1. The lexemes: `lex_number` and the quoted-literal lexeme, with their grammar tests, one for each lexeme. The
   string lexeme is a step-1 change and not C7's, since it needs no name scope: a backslash escapes any following
   character inside the token (phpdoc-parser's lexer pairs a backslash with any non-newline character; ours
   unescapes only backslash-plus-the-quote, so a literal ending in a backslash pair swallows its closing quote),
   the unescape rules are phpdoc-parser's (`\\` and `\'` single-quoted; `\\ n r t f v e`, `\xHH`, octal and
   `\u{..}` double-quoted), and `TypeNode::StringLiteral` and the shape key carry bytes (`PhpString`), not `String`.
   Fixtures: the one-backslash literal in a union, an escaped quote, a double-quoted escape, a shape key with an
   escape.
2. `types::phpdoc_keyword` and the public shape and callable constructors, extracted with the string parser still
   calling them (and the keyword table completed: I4.1).
3. `phpdoc::lower`. Its baseline is VAL-1.7's PHPDoc record (batch 0), compared through VAL-1.4's canonical
   printer, so the comparison is exact and the string parser need not stay alive; the per-`TypeNode` comparison
   with `parse_written_type(node.to_source_string())` over the stubs, Piwigo, the vendor projects and the fixtures
   runs for one stage only if that record cannot see what it finds (one of the policy's listed exceptions, #13). Either way the differences of I4.2 and I4.3 are the list the comparison starts from, and each new
   difference is read and added to it with a fixture.
4. `DocType` eager; the callers converted module by module; DOC-2.2 on the memo key.
5. The string parser, its memo and `to_source_string`'s round-trip use deleted; `types::parse` is gone but for
   `parse_display`. `registry/type_node.rs` (`parse_type_node`) becomes `phpdoc::lower`.
6. TYPE-5, **its own commit after step 5** (batch 6), once template and alias references are marked: it is the
   ledger's rule (the array member takes `K` as `array<K, V>` does, and a float or object key leaves only the empty
   array), with fixtures for a template key, an alias key and a class key, because `K` is a template, an alias or
   a class name when it is lowered and only the resolution step knows. F14's design text, which restricts `K` in
   the iterable arm and drops the array member when the intersection with `int` or `string` is never, is
   superseded by that rule; never inside `phpdoc_keyword`, which stays behaviour-exact.

Oracle: O2 and `--dump-types` over the corpus, with I4.2's and I4.3's differences read. Exact otherwise.

**C7 later** adds the name scope: `TypeNodeResolver` is an implementation of the scope that `lower` is generic over
(I4.1), calling the same `phpdoc_keyword` and shape builders with the resolution of names, templates, aliases and
`self` done in the walk, and `phpdoc::lower` is its context-free case; no second walk is written. C7 also keeps
the pieces of the crate split's C7 text that no row names: `TypeNodeResolverExtension` (the extension point,
registered through the `Extensions` input of J2) and the assert targets (`$param->prop`, `$param->method()` and
`$this->method()` are kept), `@throws` and assert types getting alias substitution, and a written `Traversable`
resolving like any name (J2's RES rows).

## I5. The rule message type

CHECK-6.10 (messages formatted only when reported, 37 rules), CHECK-4.1 (a message kept unformatted past the
report) and the crate split's D6 (`RuleError`, `RuleErrorBuilder`) need one type. Read from `Checker::report`
(`checker.rs:1507`), `report_builder`, `rewind_to` (`checker.rs:1295`), `reports_are_dropped` and
`diagnostics::DiagnosticBuilder`.

**Today.** `report(message: String, span, level, identifier)` has about 440 call sites in `checker/` (and 8 in
`project/mock_hygiene.rs`) and `report_builder(DiagnosticBuilder)` 113; a message is `format!`ed at the call, so a
type is displayed even when the report is dropped by `rewind_to` (a
loop's second pass, 30 of the 55 `offsetAccess.mightNotExist` messages, about 409 K Ir each), by
`reports_are_dropped()` (after the string is built), or by the level filter (22 more, which E5 removes by not
running a rule above the level). Several rules format a subject for every element they check, reported or not
(`phpdoc_type_compat.rs:359`, 21,919 of them). `report` builds the `Diagnostic` at once, converting the span to a
line and column with the file's `LineIndex`, and pushes it; `rewind_to` truncates the vector. **Not everything
pushed is a `report` call, and not every drop is before the check ends:** `check_trait_methods` checks a trait
once per host, then sorts its own slice of the diagnostics by (span, message), dedups by span and message and
retains by identifier and span (`function_decl.rs`, all before the check ends); the rules' `check_stmt` and
`check_expr` return `Vec<Diagnostic>` that the checker extends (`stmt.rs`, `expr.rs`);
`check_too_wide_property_types` and eleven `self.diagnostics.push(DiagnosticBuilder::new(..).build(self.line_index))`
in `phpstan_api.rs` push ready-made `Diagnostic`s; and `sort_diagnostics` (in `diagnostics`) breaks ties by the
rendered message.

**The type.** One `Message`, with `Own` in `syntax` (the impl for `Name`), the impl for `Type` in `types`, and
`Message`, `message!` and `RuleError` in `diagnostics`, which depends on `syntax` alone and so can name `Own` and
`Name`; a `Message` is a boxed closure and names neither `Type` nor `Name`, only the `Own` impls do. That keeps
`RuleError` where the crate split's 3.7 puts it (`diagnostics`).

```rust
pub struct Message(Repr);
enum Repr { Text(String), Deferred(Box<dyn FnOnce() -> String>) }
impl Message {
    pub fn from_static(s: &'static str) -> Message;  pub fn text(s: String) -> Message;   // the only constructors
    pub fn render(self) -> String;  pub fn rendered(&mut self) -> &str                   // rendered: caches into Text, for the dedup and the sort's tie-break
}
// interim, steps (1) to (3) only: impl From<String> for Message {}   (deleted in step 4)
trait Own { type Owned: Display + Debug + 'static; fn own(self) -> Self::Owned }       // Type and Name by clone, &str by Arc<str>, numbers by copy, a Display newtype
message!("Parameter #{} ${} of {} expects {}, {} given.", i, &name, &func, &expected, &given)   // deferred; each argument owned once
message!("Offset {dim} might not exist on {container}.")                              // captures too
```

- `message!` is a proc-macro in a small `phpstan-rs-macros` crate (a new crate: #32's file set), because a
  `macro_rules!` cannot number its arguments and 464 of the checker's 849 `format!` lines use inline captures
  (`{dim}`, `{key:?}`). It parses the literal, owns every positional and captured argument through `Own` (once,
  at the call: `Type` and `Name` clones are refcount increments), accepts `{x:?}` and width specs (so
  `Own::Owned: Debug`), and defers the `format!`; its dependencies (`syn`, `quote`, `proc-macro2`) are judged on
  merit like the others, and the build-time gate is checked. A message built from text stays `Text`: an
  external rule's `RuleErrorBuilder::message("...")` costs what it does now.
- **Pending reports.** The checker's `diagnostics` vector holds `Entry::Pending(Pending { message, tips:
  Vec<Message>, span, level, identifier, non_ignorable, fix })` or `Entry::Done(Diagnostic)` (the ready-made
  ones above, until D6 retires the external splices); `report` pushes a `Pending`; `rewind_to` truncates, so a
  dropped report is never formatted and never converted to a line and column; `into_diagnostics()` renders the
  survivors and builds their ranges, then sorts. So "displayed" means "the file's check ended", which is after
  every drop the checker makes **except the trait path**: `check_trait_methods` renders its slice's messages
  (`rendered`) before it dedups, because the dedup needs message equality, so a trait method costs hosts ×
  findings renders. `Message` has no `PartialEq`; `rendered` is the only way to compare.
- **Tips and fixes** are `Message`s too (a tip is a message; a fix's description is). A fix's replacement text,
  which is code and not prose, stays a `String`.
- **Identifiers** are `&'static str` (CHECK-6.10's "a closed set"): the sites that build one with `format!` are
  about 52 (43 `report(` calls and 11 `DiagnosticBuilder::new(` calls, 21 files, e.g. `duplicates.rs`'s
  `"{kind}.duplicateEnumCase"`), and they become members of the families the batch builds: a const table, kind ×
  suffix, debug-asserted against the closed set, in step (3), so the sites are static now and D6 adopts it;
  `Identifier` keeps its `Cow` for rules outside the package.
- **`RuleError`** (D6) is a `Pending` without the file: `RuleErrorBuilder::message(impl Into<RuleMessage>)`,
  `.identifier(...)`, `.tip(...)`, `.non_ignorable()`, `.fix(...)`, where `RuleMessage: From<String> +
  From<Message>`, so an extension's string still works while the checker's own methods take `Message` only. The
  dispatcher turns each into a `Pending` of the checker it runs in (or straight into a `Diagnostic` where no
  checker is), attaching the file and the trait file.
- **The engine boundary.** `FileCheck.diagnostics` holds rendered `Diagnostic`s, exact values that compare and
  persist (H1.5b, H4.6); a `Message` never crosses it. A warm run that reads a persisted check renders nothing.

**Order.** (1) `Message`, `Own`, `message!`, `Entry`, `report` taking `impl Into<Message>`, `into_diagnostics`
rendering; the interim is `report`'s `impl Into<Message>` signature and `impl From<String> for Message`, so every
existing call site compiles; the trait path's `rendered` and the `Entry::Done` pushes come here too. (2)
`report_missing_offset` (CHECK-4.1's measured site). (3) CHECK-6.10's 37 rules, one commit each family, in two
kinds: a site whose element is rarely reported carries borrowed parts or `Name`/`Type` newtypes (a lazy `Display`
subject, as 20 G designs it) to the report and formats there, because `message!` allocates a `Box` and an
`Arc<str>` on the kept path, and only a site that reports and then drops uses `message!`; kept-path allocations
are measured before and after in the check phase's callgrind, as the plan already promises for the scalar-only
sites. The identifier families (a const table) land here too. (4) Then every other site, about 440 of `report`
and 113 of `report_builder`: the checker's own methods stop accepting a `String` (they take `Message`, and the
interim `From<String>` is deleted), so the compiler names each remaining site and none is left to be converted
someday. A literal becomes `Message::from_static`, anything with arguments `message!`, and `Message::text` is the
explicit eager form: it is in `disallowed-methods` (a per-package list in `crates/phpstan-rs/clippy.toml`, so
package-wide until the split makes `checker/` a crate, and the 20 or more existing
`allow(clippy::disallowed_methods)` sites show how far that list already reaches), and each use carries
`#[expect(clippy::disallowed_methods, reason = "...")]` that an `xtask` counts, so the count ignores the older
allows. The end state is measured (callgrind of the check phase, O2): a net regression on the reports that are
kept would send the scalar-only sites back to `Message::text` by that explicit, counted `expect`. D6 builds
`RuleError` on the same `Message` when it is reached; the rule API takes `RuleMessage`, so an extension's string
costs what it does now.

**Checks.** `message!` renders as `format!` for each `Own` impl (integers, `&str`, `String`, `Type`, `Name`, a
`Display` newtype, a Debug capture); a dropped deferred message is never rendered (a test-only counter in the
closure); `rewind_to` then `into_diagnostics` gives the diagnostics of the kept passes only; a test of the trait
path (hosts × findings, the dedup by `rendered`); a targeted test per family between conversion commits, and O2
(every message and tip byte-identical) once, at the end of batch 7 (validation at the end of
the item, never per commit). **Estimate, unmeasured:** CHECK-4.1's 22.5 M is for all 55 `offsetAccess` formats; the
22 the level filter drops are dropped in the finalizer, after the checker returned, and E5 removes them by not
running the rule, so this step saves about 13.5 M (33 messages, labelled an estimate) and the other 9 M waits for
E5; the 37 rules' temporaries (21,919 and 46,005 allocations are two of them); nothing is claimed before a run.

*Left as it is:* the items of I4.3 that wait for C7's name scope, which the lowering does not have.

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

### #13 oracle tools

**Item 0 first.** Between Part A and item 9, in G5's order: read HEAD against the last validated state (VAL-1.1), build the canonical type printer (1.4, after F21), the full-field O2 (1.8), `registry-dump` (1.6), the expression-types dump and the PHPDoc record (1.7), the server session (1.9) and the edit replay (1.10) and the memory profile (1.11); run the `oracle` profile once over all of them (1.2, which includes the replay, so it comes after it); retire the debug oracles (1.3, which also shrinks item 9's reader set); take the baseline (1.12). VAL-1.5 is item 9's dump. The 20 G plan designed it so (oracles are dumps), Piwigo's 581 diagnostics are a weak oracle for exact refactors, and the rule is not to gate on Piwigo. **The policy on old implementations:** no row keeps one alive as a standing test; a comparison against the implementation being replaced is allowed for one stage where a dump cannot see the difference (every `Provider` answer for every name, H3.6 steps 2 and 3; I4.5 step 3 only if the PHPDoc record cannot see what a per-`TypeNode` comparison finds; I2's comparison of the old and new lexer's (kind, span) pairs), each is deleted in the commit after its stage (the overview's convention), and A2 keeps none

*Needed before:* batch 0

### #14 DOC-2.1

**Undefer it, as F41.** F41, the ledger's DOC-2.1 and C7's grammar item are one piece of work: one grammar, a `TypeNode` to `Type` lowering, the string parser deleted, internal strings through `parse_phpdoc_type`. It fixes real bugs (a class named `Infinity`, `Nan` or `inf` read as a float; `1_000` and `0x1F` rejected; conditionals split at the first `:`), so "the saving is gone" does not apply. First in batch 6, before DOC-2.2; C7 later builds the resolver on it

*Needed before:* batch 6

### #17 CHECK-9 and TYPE-5

**Neither in batch 7.** CHECK-9 only after F69 gives implicit calls (`__get`, `offsetGet`, `__toString`, `include`, `Traversable` iteration) throw points, or it reports live catches as dead. F69 records them on `record_call_throw_point` and `current_function_throw_points`, the recorders that D2's `StatementResult` replaces, so by the crate split's Appendix F ("a bug in code a later phase rewrites gets a fixture now and its fix as that phase's behaviour commit") F69's fixtures go in batch 0 and F69 and CHECK-9 are built in D2, CHECK-9 after F69. The same reasoning puts CHECK-7's checker half in D2 and CHECK-10's operator sites in D3 (J3). TYPE-5 is F14, which follows F41, so it comes right after #14's work

*Needed before:* batch 6 (TYPE-5); D2 (F69, CHECK-9; fixtures in batch 0)

### #18 when to reevaluate

**Now, for the engine's identity model.** Design REG-16.2's ids, REG-8.6's interner ownership and NAME-2's weak tables as one lifecycle before batch 4: names, products and memo keys are built on it. The rest at batch 8

*Needed before:* before batch 4

### #22 level 3 and the Q-11 follow-ups

**A correctness backlog, not fillers.** Triage the findings first (about 115 are distinct; F57, Stage C's O1 and F5 are
built), by running each probe on the gate binary. F10's symptom is fixed (`can_begin_expression`'s list, `3b7e4d95`), so
its parser tests and fixture go before item 9's baseline, which must not encode a parser bug, and its exhaustive predicate
waits for SYN-2's `TokenKind`. F20 goes before item 9 (after the cross-platform plan's R10), so its lint checks the
recursion that item 9 writes. F21 goes before the canonical printers. F41 goes in batch 6. F77 goes in batch 0, before
item 9's baseline: an analysed file's diagnostic and exit status are the final ones, so a parser rewrite that newly
refuses a file shows in O2, and batch 3 feeds it from `errors()`.

**F77's context-file half** (`phpstanRs.contextParseError`, run-level, failing the run) is user-visible behaviour that
decisions 9 and 15 ("nothing in a context file is reported") do not give. Those two decisions keep their text and gain one
sentence: a context file that fails to parse is reported as a run-level notice (its path, line and column, and the first
error; `phpstanRs.contextParseError`), printed where the "N file(s) skipped: parse error" count is, and it leaves the exit
status as it is; an opt-in switch makes it fail the run. Batch 0 builds the notice and records each failure (the count and
the first error per file) in the dump. F69's fixtures are batch 0's; F69 and CHECK-9 are D2's (#17).

The seven fixes the crate split's F1 puts first (the debounce thread's stack size and the scan's parses on the pool,
`queue_change` writing `latest_text`, `didClose` restoring the disk's content, `--generate-baseline` keeping the entries of
files not analysed, `--fix` skipping a file changed since analysis, a closed output pipe ending the run quietly, rename
refusing qualified names, imports and aliases) end the server, lose data or corrupt files, and are independent of
everything else here. They are batch 0's, each with its test (built: SRV-1 to SRV-7 in the README's batch-0 table), and
rows of the ledger and `WORK_STACK.md`, not part of W3.

Where each finding sits is a place in the sequence: every finding or follow-up that changes a dump goes before the
baseline, and the rest go in the review's own order (F4 first), fixture first where a later phase rewrites the code (the
table under "Level 3's placement" names which). A later commit that changes behaviour runs `dumps-compare`, lists its
differences as its delta, and rolls the baseline forward.

*Needed before:* batch 0

### #29 NAME-2.1's thin name

**`triomphe::ThinArc<Header, u8>`, after a spike that settles how the text is read back as a `&str`.** A header plus one slice fits (the text, then the folded form), and the interner frees names without `Weak` (triomphe 0.1.14 has no weak counts) by a unique-count sweep, which REG-8.6 already plans. Its thin pointer exists only for slices (a header plus a `str` is a fat pointer), so without `unsafe` of ours a `&str` costs a UTF-8 validation on every `as_str`, `lower` and `Deref`, unless a crate supplies a thin header-plus-`str` type (`slice-dst`'s `StrWithHeader` behind `erasable::Thin` is one to evaluate, on merit). S-1 therefore measures the read path and accepts the thin name only if the corpus total of those reads is not above today's; the fallback is one `Arc<NameData>` whose single `Box<str>` holds the text and the lowercase form (sliced at `len`): two allocations against today's three. No hand-rolled `unsafe`

*Needed before:* batch 4

### #31 An analysed file with parse errors: the parseable parts are checked and reported as usual, or only its errors are reported (PHPStan's behaviour)

**Checked (G3.6).** Q-10 asks for one rule for the batch run and the server, and decisions 4 and 15 keep what parses and make the rest unknown (decision 6 says nothing of parse failures); the conservative reads (error nodes infer the error type of F21, a statement error sets `any_variable_may_exist` and `variable.undefined` honours it, a missing name is a zero-width name every reader treats as unknown, nothing reported inside an error node, whole-file opt-in rules off) are what keep it quiet. The editor is where files are broken most, and recovery exists for it (#11). No ruled decision says an analysed file's own findings are reported, and this departs from `CRATE_SPLIT_PLAN.md:6883-6891` ("a file with parse errors is still not analysed (its findings would be noise)"), which #20 supersedes. **The bar is stated before the check is built (G3.7.3):** on the mutation corpus, for damage to a single token, the diagnostics that are new, meaning absent from the undamaged file's own and not parse errors, are none at the 95th percentile of damaged files. A damage class that misses the bar falls back, for the files it damages, to parse errors only, with the registry and navigation keeping the recovered declarations; an unclosed brace that reshapes everything after it is the likely class to miss

*Needed before:* batch 3

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
- The 20 G plan's §9 makes the oracles dumps.
  `FileBodies` still keeps statements in debug builds for the old oracles.
