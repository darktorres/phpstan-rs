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
| DOC-2.1 | READY (deferred) | 6 |
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

Batch 0 and item 9 are Part E's and Part I's, and the batch 8 rows are other parts' (LOAD-5.4 is Part A's; CACHE-13.1, CACHE-8, LOAD-5.1, LOAD-5.2 and LOAD-3.1 are W3's). They are named in the text, not listed here.


Read from the 20 G plan's items 10 to 19 (its §10), the ledger's rows, needs and notes
(`PROFILE_DRILL_PIWIGO_LEDGER.md` §2 to §13) and the code at `0fb1a4de`. The savings are the 20 G
report's estimates for the off run (the ledger's "now" is 20.37 G at `8a1ca1e0`; 20.57 G is the 20 G profile's
`cl-20g`, file cache off with a warm stub cache); PROD-1 to PROD-3 and DOC-1 overtook parts of
items 13 and 14, so none is a claim. Validation is at the end of each item.

## C1. The path, in order (the order of Part E's "The sequence this gives")

1. The cross-platform fixes (finished at `a83b1675`); then Part A's rows
   (A1 to A3, A5, A5a, A7 and A8).
2. Batch 0 (Part E 13 and 22: the oracle tools and the baseline, F10's tests, F20, F21, F77, the level-3 backlog that
   changes a dump, and the seven fixes the crate split's F1 puts first), then item 9: steps 0, 1, 2, then step 3
   (LOAD-3.5, the scratch builder, which is item 3's row). Steps 4 and 5 (SYN-1.9, SYN-1.10) come later (batch 3, and
   C4).
3. The batches below.

| # | batch | rows | estimate | why here |
|---|---|---|---|---|
| 1 | tokens | item 10: SYN-2 (I2), with the three rows its token table already uses: SYN-4.1 (the watch table), SYN-4.8 (`QualifiedName`) and SYN-4.9 (digit separators), which have no needs | ≈1.7 G | the largest single row; needs item 9; designed with the tree (the parser's `take` calls the builder's `name` and `text_span`); item 9's baseline serves it; the three rows go here so the lexer and the skim are not written over `Identifier Backslash Identifier` and again over one token |
| 2 | the lexer, second pass | items 11 and 12: SYN-3.1-3.4 (I3 for 3.2), SYN-4.2-4.10 less 4.5, 4.8 and 4.9 | ≈0.2 G + ≈0.35 G | same files as batch 1; SYN-4.1 shrank (the lexer interns nothing: I2) and is in batch 1; 4.3 needs SYN-3.2, 4.7 needs 4.2 |
| 3 | parse recovery (in: Part E 11; changes output) | SYN-1.9, Q-10's parser half, SYN-6 (G3); the `php-version` crate with `VersionSet` and `Availability` (Part E 32); F77's diagnostics now come from the tree's `errors()` | none (VAL-2.10 deltas) | after batch 1, so the parse functions are rewritten once; Q-10's version half needs STUB-7.1 (level 2) |
| 4 | names | S-1 first; item 16 (NAME-2.1-2.3), **then** item 15 (NAME-1.1-1.6), and SYN-4.5 (`ConstName`) | ≈0.1 G + ≈0.6 G; ≈15 MB | names first (Part E 12): products and PHPDoc work are born on `FoldedName` and `ConstName`; NAME-1.4 needs NAME-2.1, so the plan's number order has the needing item first |
| 4b | the constant lookup | REG-18.4 with CHECK-4.2 (needs `ConstName`; a delta) | CHECK-4.2 is 5.1 M Ir | one place for the two rows: after batch 4, before batch 5 |
| 5 | a file's products | item 13: PROD-5.8, 5.2, 5.1, 5.3, 5.5, 5.6 (PROD-5.7 is not built: Part E 35), REG-14.1, and CHECK-13 (its rule sees anonymous classes once PROD-5.1 gives them a file-local identity) | ≈0.45 G | three returns to earlier items open here (C3); PROD-5.4 is REG-16.4's to delete; PROD-5.8's record is keyed by `FoldedName` and `ConstName` (batch 4's types), and `RefKey` goes with the dependents machinery in W3 |
| 6 | PHPDoc | item 14: DOC-2.1 first (F41: Part E 14), then 2.2 to 2.9, with REG-4.10 in 2.9; TYPE-5 | ≈0.35 G; the memos' memory | 2.2 needs PROD-5.2 (batch 5), 2.5 needs NAME-1 (batch 4), 2.6 needs 2.1 and 2.3, 2.9 needs the comment's offset (SYN-3.4, batch 2); item 14 closes |
| 7 | the check's temporaries | item 18's temporaries (the `Message` of I5, CHECK-6.4 to 6.6, 6.9 to 6.12, 6.16, 6.17) and the scratch reuse of CHECK-6.8 and 6.18 (allocations that hold no tree handle) | the temporaries' share of ≈1.6-2.0 G | the inference core and the scopes group are Stage D's (Part E 16): their ≈1.0-1.3 G and ≈0.15-0.25 G arrive with it; F69 and CHECK-9 are D2's (Part E 17), CHECK-4.2 is batch 4b's, and the one-checker-per-worker form of CHECK-6.18 is E7's |
| 8 | nothing is left that is not the wall's | LOAD-5.4 moved to Part A (A5a); CACHE-13.1 is W3's, built once as `File.stamp` (Part E 15); CACHE-8 waits for S-4 (Part E 41); LOAD-5.1's six passes' per-file facts are W3's `file_products` fields, and E1 moves their collection onto D6's API (Part E 39); LOAD-5.2 and LOAD-3.1 are W3's (LOAD-5.2 needs both LOAD-5.1 and LOAD-3.1) | – | the decision point of C4 comes after batch 7 |
| 9 | decision point | – | – | level 8's READY list is empty; what is left is levels 1 to 7 (C4, and Part D) |

The table's per-batch estimates add to about 3.75 G before batch 7's share (1.7 + 0.2 + 0.35 + 0.1 + 0.6 + 0.45 +
0.35); the 20 G plan's own sum for items 10 to 18, about 5.5 G (tokens and the check 3.5 G of it), counted the
inference core and PROD-5.7, which Part E 16 and 35 moved or removed. Item 9's adds the rest of its 0.8-1.2 G,
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

**3. Parse recovery (in: Part E 11).** SYN-1.9 turns `parse` into "a tree and its errors" with error
nodes (their variants arrive here, not in item 9's step 1: Part E 9, G3.3); Q-10 is half built (a context file that
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
`FoldKey for str` while a caller holds text; what is not built is a new structure for it (Part E 35). Oracle:
O2, `--dump-types`, `--dump-expr-types` and `registry-dump` (the last two are built in batch 0: Part E 13).

**5. Products (item 13).** Order inside: PROD-5.8 (the record of written names, also what
REG-14.1 needs; it extends the parser's `Recorder` but is keyed by `FoldedName` (class-likes, functions) and
`ConstName` (constants), batch 4's types, and not by `RefKey`, which the engine deletes with the dependents machinery)
and PROD-5.2 (`ImportScope`s) first; PROD-5.1 (the declaration vectors; item 9
already reserves declaration ids on entry, B3); PROD-5.3, 5.5, 5.6 (one walk, bodies as a sorted
slice, scratch builders); PROD-5.7 is not built (Part E 35). PROD-5.5's slice is keyed by the
body's ordinal (H1.4's `occurrence` and the member's ordinal), not by its span (H1.5a): an edit above a body then
leaves every other entry equal. PROD-1 to PROD-3 delivered most of this item's premise, so its Ir is
smaller than the report's; its memory and what it unlocks are the case for it. Oracle: item 9's
dump, `registry-dump` (built in batch 0), O2.

**6. PHPDoc (item 14).** DOC-2.1 comes first, as F41 (Part E 14, I4): the grammar's lowering replaces the text
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
(Part E 16, J3): they are the structure of the crate split's D1 to D4, and building them now would write the
traversal twice. What this batch builds is exact, local and survives either way: the *temporaries* 6.4 (which bumps
a format), 6.5, 6.6, 6.9, 6.10 with CHECK-4.1 (the `Message` of I5), 6.11, 6.12, 6.16 and 6.17, and the scratch reuse
of CHECK-6.8 and 6.18. CHECK-4.2 goes with REG-18.4 in batch 4b (Part E 41). F69 and CHECK-9 are D2's: F69 records
implicit calls' throw points on the recorders (`record_call_throw_point`, `current_function_throw_points`) that D2's
`StatementResult` replaces, so by the crate split's Appendix F ("a bug in code a later phase rewrites gets a fixture
now and its fix as that phase's behaviour commit") and this plan's own rule (K2) they get their fixtures in batch 0
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
is built once (Part E 39). LOAD-5.2 (free each analysed tree after its check, the memory win) needs both LOAD-5.1
(the facts, so no pass reads a tree after its check) and LOAD-3.1, which needs REG-16.4 (Part E 28), so it is W3's and
follows S-7's verdict (H2.9). CACHE-13.1 (entries checked by `stat`) is the source of `File.stamp`, and MEAS-1 puts what it
replaces at ≈60-70 M Ir of reads plus their system calls, with the per-file decode (≈1,260 M) untouched: a
standalone index file would be deleted the day the persisted database holds the stamps, so it is built once, as the
stamp, at W3 (H4.7 step 5, Part E 15). CACHE-8 is READY and independent: `Writer::finish`
(`registry/container.rs`) builds the cache file in a second `Vec` beside the body, so the cold run holds both at
its peak (−15.3 MB there and −31.4 MB allocated, per the heap report's H1). The writer serves the stub caches
(`stub-base`, `stub-base-phpstan`, `stub-data`, `stub-index`), which go with STUB-11. A memory row: no instruction
saving is claimed, and nothing changes on disk. It is built once the persistence's format is chosen (S-4, Part E
41): the writer that survives is the one worth making right.

## C3. Returns to earlier items (each is unblocked inside a later batch)

| row | item | opens when |
|---|---|---|
| LOAD-3.5 | 3 | SYN-1.2 (item 9, step 2) |
| LOAD-4.7, file-references half | 5 | PROD-5.7, which is not built (Part E 35); its reverse-index half goes with REG-16.4 |
| REG-14.1 | 8 | PROD-5.8 (batch 5); 14.3 still needs REG-8.3 |
| DOC-2.2 | 14 | PROD-5.2 (batch 5) |
| DOC-2.5 | 14 | NAME-1 (batch 4) |
| NAME-1.4 | 15 | NAME-2.1 (batch 4, first half) |
| CHECK-4.1 | – | CHECK-6.10 (batch 7) |
| CHECK-6.7, 6.15 | 18 | CHECK-6.1 (Stage D) |
| CHECK-6.13, 6.14 | 18 | CHECK-6.3 (Stage D) |
| LOAD-5.1 | 19 | PROD-5.3 and PROD-5.1 per pass; W3 builds the facts, E1 the collection (Part E 39) |
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

## C5. Rulings and risks (the questions; Part E has the current answers)

1. **SYN-3.2's SIMD** (batch 2). It specifies SSE2 16-byte blocks and AVX2 32-byte blocks behind
   `is_x86_feature_detected!`. The workspace denies `unsafe_code`, nothing in it uses `core::arch`,
   and `UNSAFE_PLAN.md` removes `unsafe` from everything but the PCRE FFI. Options: SSE2 through a
   safe crate such as `wide` (baseline x86-64, no dispatch) and a `u64` word loop elsewhere, no
   AVX2; or a small audited exemption like the PCRE one. The saving falls without AVX2 and has to
   be measured (`LineIndex::new` was 270.4 M over 19,211 lexers in the 20 G profile: 63.5 M of per-line `memchr`
   calls, 83.4 M of column tables, 116.3 M of its own, and 7.2 M unattributed; the ledger's later 363.4 M is over
   25,753 builds, stub files included).
2. **Item 14 closes whole** (Part E 14 undefers DOC-2.1 as F41, and REG-4.10 is built with DOC-2.9: note 6).
3. **Order inside batches 4 to 6** (13, 16, 15, 14 against 13, 14, 15, 16): the first leaves no item
   half done and needs no return to a finished item, at the price of leaving the plan's numbering.
4. **Oracles for items 13 to 16.** Their lines name `registry-dump`, `--dump-expr-types` and the
   PHPDoc resolution record, which are VAL-1.4, 1.6, 1.7 and do not exist (`--dump-types` and O2 do).
   VAL-1 "is not a precondition for any row", so the batch can go on Piwigo's output, O2 and
   `--dump-types`; building the three tools first (4 to 6 commits) makes "exact" checkable beyond
   what Piwigo exercises. (Answered in Part E 13: they are built in batch 0.)
5. **Item 18's groups**: confirm the split and that the inference core is validated by reading its
   diff by family. The inference core and the scopes group also meet the crate split's Stage D
   (Part D, "What bears on the order", 2), which decides whether they are built in batch 7.
6. **CACHE-13.1 for context files early** (batch 8, or sooner): a warm-run wall lever, independent of
   the engine. (Answered in Part E 15: it is built once, as `File.stamp`, at W3.)
7. **When to reevaluate levels 1 to 7** (C4), and whether the engine's roots (REG-4.2, REG-16.2)
   start as design work before then. Part D has the proposed order and its rulings.

---
