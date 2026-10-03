# Callgrind drill-down of the 25.14 G Piwigo run, and what to change

Profile: `~/.cache/prs-profile/cl-final/cg.out-01..09` (all nine threads), binary `prs-final` built from `cff7bde1` (gate profile), command `check-project --config phpstan-rs.toml --format json` in `/home/torres/piwigo17-frozen`. Total **25,140,226,057 Ir**. Percentages below are of that total.

HEAD (`7a3f3f99`) differs from the profiled binary in two hot paths: `c848318b` (a per-file memo of `lookup_method_with_owner`, by class and method as `String`s) and `d6bd5c5a` (the end of a doc comment found with `memmem`). Everything else since `cff7bde1` is the lazy-vendor work and its revert, and docs. Each proposal below was checked against the code at HEAD; where one of those two commits already covers part of a cost, the item says so.

The proposals are in their most complete form (the request was maximum churn and properness): where a narrow fix and a structural one exist, the structural one is described, with the narrow one only as a fallback.

Part I (§0-§9) is the drill-down and the proposals. Part II (§10-§15) designs every proposal for implementation: data structures, signatures, the commits in order, the oracle each must pass, and the edge cases. Designing them corrected eight points of Part I, listed in §10.1 and fixed in place. An audit of the designs' first version (§10.3) then found about half short of the most proper form: a second implementation kept, today's behaviour kept where it departs from PHP, a proper design gated on a measurement, or a listed subset converted where the whole class of sites is the item. Those designs were replaced. A second audit (§10.4) found 15 more, one of them a correctness hole in a design (P7's memo key), and a third (§10.5), which checked each design's assumptions against the code, found nine more, including two correctness holes (C1's block rule and F2's cause). A fourth (§10.6), which checked the designs' claims about PHPStan against its vendored source and each list of readers against every reader in the code, found 12 more. Three of them were correctness holes (§14's dropped trees, C4's guard keys, C1's nested labels), and one was a limit that every earlier audit had kept on a false premise: `goto`, which PHPStan does model. A fifth (§10.7) applied the no-interim rule to every item and found five designs that improved separate walks the one traversal makes unnecessary. C1 now builds PHPStan's full statement result, and §12.10 gives every one of the checker's separate walks its fate. A sixth (§10.8) applied the same rule to the parse phase and found P12. Its post-parse walks (name resolution, references, declarations, the body index) had no design, though `resolve_names` alone is 376.3 M. A seventh (§10.9) removed mechanisms that duplicated what a design already had, notably C4's write history in favour of `OrdMap`'s structural diff. It also corrected P12's cost, which was overstated. Those were replaced too.

# Part I. Drill-down and proposals

## 0. Method

- Tools, in `~/.cache/prs-profile/drill-scripts/`: `parse.py <run dir> <out.pkl>` reads all nine thread files with `callgrind_annotate`'s rules (name compression, relative positions, `fi`/`fe` inline files) into one table of self costs, call edges (calls, inclusive cost) and per-line costs. Self costs sum to the profile total exactly (25,140,226,057). `q.py` queries it (`self`, `incl`, `callers`, `callees`, `tree`, `lines`), `charge.py` builds the charged table, `roots.py` attributes worker time, `sites.py` lists a function's call sites by source line, `selflines.py` its self cost by source line.
- "Inclusive" is the sum of the edges into a function from outside its own recursion family (`'2` suffixes folded), which is exact for non-recursive functions and an upper bound for the checker's and parser's recursive families.
- "Charged" is a function's self cost plus the cost of the library code it calls (allocator, `memcpy`, hash tables, `fmt`, iterator adapters, `imbl`), pushed up through library frames to the nearest workspace function in proportion to edge costs. Charged costs sum to 24.99 G of 25.14 G; they are the right unit for "what does this function's own code cost".
- Rayon helpers recurse into themselves, so worker time is attributed by the first workspace frame under any rayon frame. Callgrind runs threads one at a time, so per-thread splits say nothing about wall time; only totals and serial stretches matter.
- The gate binary has no line tables. A line-tables twin of the same commit (`cff7bde1`, `CARGO_PROFILE_GATE_DEBUG=line-tables-only`, same codegen) was built and profiled for the line-level sections (§9); its output is byte-identical to the baseline (md5 `18623d50…`).
- No source file in the checkout was changed; the build and the runs were in a throwaway worktree with its own target directory. Code claims are from reading HEAD at the cited lines.

## 1. Where the 25.14 G goes

| phase | Ir | % | root |
|---|---|---|---|
| Parse and per-file indexing (workers) | 10,853.9 M | 43.2 | `Session::read_and_parse_file`, 14,251 files |
| Checking (workers) | 10,338.1 M | 41.1 | `Project::run_checker`, 2,792 files |
| Finishing passes (one worker plus stolen `par_iter` work) | ≈2,640 M | 10.5 | `Project::run_finishing_passes` 1,516.7 M on its thread, the rest stolen |
| Main thread (serial) | 1,080.5 M | 4.3 | `prepare` 365.0, `Session::load`'s serial part ≈610, `render_json` 23.8 |
| Per analysed file after checking | 111.9 M | 0.4 | `check_files::{closure#2}` = the PHPStan-API walker |

Inside the parse phase: lexing 3,390.8 M, parsing 3,307.8 M, `annotate` 431.4 M, building declarations 2,232.9 M, reference collection 841.6 M, `FileBodies` ≈130 M, ignore comments 117.9 M, `Lexer::from_bytes` 131.2 M.

Inside checking: `check_function` 6,306.0 M (10,312 functions), top-level statements 3,402.5 M, per-file pre-walks and declaration checks ≈600 M. The checker's own big subsystems (inclusive, overlapping): inference 4,469.6 M (1.66 M cached lookups, 1.13 M computed), `always_terminates` 1,553.3 M, method-call return types 1,382.9 M, `infer_call_return_type` 1,076.2 M, `check_args` 647.2 M, narrowing 693.5 M, `merge_scopes` 319.7 M, `record_branch_implied_facts` 320.7 M.

The finishing passes, with the work other threads stole: property-write classification ≈470 M, `warm` (lazy stub decoding) ≈436 M, transitive purity ≈372 M, global variables ≈260 M, `closure_scope` 180.1 M, PHPDoc inheritance ≈158 M, member owners ≈159 M, `end_declaration_changes` 166.9 M, the rest under 100 M each.

Charged cost by area (crate and first module, from the charged table):

| area | charged |
|---|---|
| `phpstan_rs::checker` (all of it) | 4,572 M |
| `phpstan_rs::registry` | 4,060 M |
| `syntax::lexer` | 3,255 M |
| `syntax::parser` | 2,861 M |
| `syntax::name` (interner, `Name`) | 1,210 M |
| `syntax::visit` (walk machinery) | 1,108 M |
| `types` (`Type` methods 842 M, interner 832 M, relation 528 M, union 233 M, shape 148 M, parse 108 M, rest) | ≈2,920 M |
| `phpstan_rs::project` | 819 M |
| `phpdoc` (grammar 612 M, docblock 457 M) | 1,069 M |
| `phpstan_rs::deps` | 524 M |
| `syntax::name_resolution` | 429 M |
| `syntax::annotations` | 324 M |

Cross-cutting library cost (self): allocator ≈2.4 G (22.6 M allocations, 20.4 M frees, `finish_grow` 1,064 M inclusive), `memcpy` 784 M (27.1 M calls, 148 M of it `realloc` copying), `memcmp` 447 M (23.8 M calls), string formatting ≈1.0-1.2 G inclusive (`String::write_str` → `reserve` alone 434.8 M over 3.33 M growths).

## 2. Four themes that cut across the phases

Most of the large items below are instances of four patterns:

1. **Names travel as `String`s.** Class, function, method and variable names are re-lowercased, re-hashed byte by byte, re-interned and re-formatted at every hop: `NameTable::get::<str>` copies and lowercases the key before hashing it (≈1.2 G of table lookups by `&str` and by `Name` together), `Name` → `String` through `Display` 1.70 M times (288.9 M), `String`/`&str` → `Name` through the interner 1.34 M times (319.8 M), reference keys as fresh lowercase `String`s (deps, `register_references`, `closure_scope`), `(Option<String>, String)` keys in property writes, `String` keys in `Locals`, template bindings and the method memo at HEAD.
2. **Work done for files that never use it.** 80% of the parsed files (11,459 of 14,251, ≈68% of the 60 MB) are context files that are never checked, but each gets annotations, ignore comments, a comment list and code-line starts; and `annotate` is read by nothing at all.
3. **The same answer computed twice.** Termination re-infers every expression statement after it was checked; call return types re-infer every argument; `method_call_return_type` looks the method up three times; phpdoc types go AST → text → second parser; `map_class_names`/`resolve_conditional` walk types that contain nothing to map.
4. **Per-node overhead in the front end.** About 770 Ir of function levels per parsed operand before any real work; two line/column computations per token; UTF-8 re-validated per identifier.

## 3. Parse phase (10.85 G)

### P1. `annotate` is dead — 431.4 M (1.7%)

`syntax::parse` (`crates/syntax/src/parse.rs:123`) builds `Annotations` (parents, condition flags, catch scopes, goto targets, version checks) for every file, 14,252 times. Nothing outside `crates/syntax/src/annotations*` reads `ParsedFile::annotations` (grep over `crates/`, `xtask/`, `fuzz/`, at HEAD and at `cff7bde1`; `parse_with_ignores` drops it).

Do (§11.1): delete the `annotations` module altogether. The checker already tracks every fact it holds during its own traversal (statement position, `catch` types, `goto` targets, the leading `declare`), so it is a second, unused implementation. The one fact the checker finds by a separate walk, `goto` targets, is recorded by the parser. Exact.

### P2. Analysed-only products for context files — ≈0.25 G

For every file the front end also produces what only a reported file uses:
- ignore comments: `ignore_comments::scan_parsed` 117.9 M (its event sort 54.4 M);
- the comment list: every line comment's text copied into a `String` in `Lexer::lex` (47.4 M), every doc comment's text copied again in `parse` (`doc.text.to_string()`, 21.8 M), plus the sort;
- `code_line_starts`, a `Position` per code line (grows 14.0 M, plus its loop over every token inside `parse`'s 174.5 M self).

(Lexer warnings are not in this list: `Session::load` prints them for context files too, `session.rs:680`.)

Every reader of `ignores` skips files that are not analysed (`project.rs:686`, `project/banned.rs:467`, and the per-file diagnostics of analysed files), and the comment list and code-line starts exist only to build them. The scan, however, reads only comments containing `@phpstan-ignore` and, for each, whether code precedes it on its line and where the next code line starts. Do (designed in §11.2): the lexer records only those comments, with that neighbourhood resolved as it lexes, for every file. Then no role parameter is needed, and analysed files stop paying too: ≈0.25-0.3 G of the ≈0.37 G these products cost, leaving a `memmem` over comment bytes. Exact, by equality of the scan's output over every file. (An earlier version of this item had `parse_file` take the file's role and skip the products for context files; this is simpler and saves more.)

### P3. UTF-8 validated once — ≈0.22 G

- `next_token` turns every identifier and variable span into `&str` through `lossy` (`from_utf8`, falling back to lossy decoding): 128.5 M through `lossy` (1.09 M calls) plus 58.9 M inlined (692 K calls).
- `Session::read_and_parse_file` validates the whole file again with `String::from_utf8` to learn a length (`session.rs:592`, 29.8 M).
- `Lexer::from_bytes` runs `is_ascii` twice and `from_utf8` once.

`IDENT_CONTINUE` holds every byte ≥ 0x80 (`lexer.rs:1179`), so in a valid UTF-8 source an identifier span starts on a character boundary and ends before an ASCII byte or at the end: always valid. Do (§11.3): validate once, at the read. `String::from_utf8(bytes)` gives the `String` without a copy, or the bytes back for the rare invalid file. The lexer then takes a `&str` and validates nothing (`Lexer::new(&str)` stops re-validating too), with identifier, variable, heredoc-label and interpolation-name slices as `&text[span]`: a boundary check, no `unsafe`. Invalid files keep today's lossy path. Exact.

### P4. Byte-offset spans with a lazy line index — ≈0.7 G (and smaller AST nodes)

Per token, `run_lexed` computes two `Position`s (`lexer.rs:473-476`): `relative_line_col_at` 239.0 M (1.30 M calls, every new line), `columns` 185.8 M + 33.8 M (10.0 M calls, ≈21 Ir each, not inlined although an ASCII file needs `to - from`), plus the inlined cursor logic inside `run_lexed`'s 680.5 M self (≈277 M at line level, §9): ≈700 M of positions per run. `compute_line_starts` is 95.6 M per run and is only needed when a position is printed.

Do (the B3(c) design of the final plan, re-justified by these numbers): `Span { start: u32, end: u32 }` byte offsets in tokens and the AST; a per-file `LineIndex` (line starts, plus char offsets for non-ASCII lines) built on first use; `Span::start_position(&LineIndex)`-style accessors for diagnostics, LSP and the ~97 field reads in 32 files that use `.start.line`/`.col` today (50 `Position { .. }` constructions). Spans serialized in the stub cache change format (one bump). `Span` goes from 16 to 8 bytes, which shrinks `Expr` (96 B), `Stmt` (184 B), `Lexed::spans` (16 B per token, 6.9 M tokens) and the copies the parser does through every precedence level. Context files never build their line index unless a diagnostic or a go-to-definition needs it. Exact by construction (positions are derived from the same bytes); the test oracle is today's lexer positions over the fixtures, the vendor tree and the corpus. If P4 has to wait, two interim fixes take ≈0.27 G of it (§9): advance the cursor's line instead of binary-searching the line table, and inline `columns`.

### P5. Expression parsing by binding power — ≈0.9-1.0 G

Each operand descends ~15 frames before its primary is parsed: `parse_expression` → `parse_low_precedence_or` → `_xor` → `_and` → `parse_assignment` (stack guard) → `parse_ternary` → `parse_coalesce` (guard) → `parse_coalesce_inner` → `parse_logical_or` → `parse_binary(1)` → `parse_not` → `parse_instanceof` → `parse_unary` (guard) → `parse_unary_inner` → `parse_pow` → `parse_postfix_from_primary` → `parse_primary`, each moving a `Result<Expr, ParseError>` back up (`crates/syntax/src/parser/expr.rs`). Self costs of the levels alone (no postfix, no primary):

| level | self | per call |
|---|---|---|
| `parse_unary_inner` | 242.2 M | 144 |
| `parse_not` | 179.0 M | 107 |
| `parse_binary` | 155.0 M | 100 |
| `parse_ternary` | 145.8 M | 95 |
| `parse_coalesce_inner` | 125.3 M | 81 |
| `parse_low_precedence_and` | 120.6 M | 86 |
| `parse_low_precedence_or` | 110.7 M | 79 |
| `parse_low_precedence_xor` | 110.7 M | 79 |
| `parse_assignment_tail` | 97.0 M | 58 |
| total | **1,286 M** (1,314 M charged) | ≈770 per operand |

Binary operators already use precedence climbing (`parse_binary(min)`); the rest of the ladder does not. Do: one Pratt loop, `parse_expr_bp(min_bp)`: a prefix step (`!`, unary operators, casts, `@`, `new`, `clone`, `print`, `yield`, `include`, `throw`, `fn`/`function` and the primary with its postfix chain) and an infix loop over a binding-power table covering low `or`/`xor`/`and`, assignment (right, with the "assignment takes everything to its right" rule as the operand-position special case it is today), ternary, `??` (right), the binary table, `instanceof` (with `!` binding looser than it) and `**` (right, through unary). One stack guard per recursion into a sub-expression instead of three per operand. The loop follows PHP's own precedence table, not today's ladder, which departs from PHP in six places (§11.6): arrow-function bodies, `include`/`require` and `throw` bind too tightly; `instanceof` chains are rejected; chained comparisons and unparenthesized nested ternaries are accepted. Gate: a generated test over all operator pairs and prefixes against nikic/php-parser's trees and `php -l` (PHP 8.5); then a dual parse of every fixture, vendor and corpus file, whose differences must all be those six, listed by file.

### P6. Lexer per-token costs — ≈0.3 G

Lexing is ≈226 Ir of self per token (`next_token` 875.5 M + `run_lexed` 680.5 M over 6.89 M tokens), excluding the positions P4 removes and the validation P3 removes.
- **Identifier interning across files.** `Lexer::name` 375.7 M over 1.77 M identifiers and variables; 511 K miss the per-lexer table and take a shard lock in the interner (`name_hashed` 133.5 M, the per-lexer insert 68.3 M). A miss is a name new to the file, which is not the same as new to the thread. Do (§11.5(c)): a table of strong `Name`s for each worker job of a load, kept across the files that job lexes and dropped with the load, consulted before the interner; the per-lexer table stays as the first level. ≈0.15 G.
- **Keywords.** `Keyword::try_from_bytes` 98.4 M over 1.74 M identifiers (≈56 Ir): lowercases into a buffer, then a `match` on byte strings. Do: a generated perfect hash on length and two bytes, comparing case-insensitively in place (the final plan's B4/N6). ≈0.07 G.
- **Single-quoted strings.** `lex_single_quoted` (`lexer/strings.rs:150`) is 184.8 M charged over 335,704 literals (≈550 Ir each): it grows a `Vec<u8>` from empty (41.5 M of `reserve`), then `PhpString::from(Vec<u8>)` copies it into an `Arc<[u8]>` (a second allocation and copy). Do: scan to the closing quote first, noting whether an escape occurs; without one (the common case) the `Arc<[u8]>` is built from the source slice directly, with one allocation; with one, the buffer is sized from the scanned length. Exact.
- **No token vector at all** (§11.5(d)): dropping the parser's token vector is 73.5 M, building it is part of `run_lexed` (≈200 M of pushes and moves) and of `Parser::from_lexed` (59.6 M, with four doc-comment maps keyed by token index). The parser never rewinds and looks ahead a few tokens, so the lexer streams: each token carries its doc comments and keyword spelling, and a small lookahead ring replaces the vectors and the maps. Done together with P4 and P5.

### P7. Declarations: phpdoc types without the text round trip — ≈0.35-0.45 G

`registry::parse_type_node` is `types::parse_written_type(&node.to_source_string())` (`crates/phpstan-rs/src/registry/type_node.rs:8`): a phpdoc AST is printed to text and parsed again by the `types` crate's own parser. `to_source_string` costs 238.1 M inclusive (319 K calls; 72.5 M of `format_inner`, 71.6 M joining `Vec<String>`s), the `parse_written_type` memo 118.6 M (123 K lookups, each hashing the whole text). The same path serves `declared_type` (registry, 76.5 K parameters, 188.1 M) and the checker's `resolve_type_node` (34,853 statement `@var` tags, 118.3 M for 559 statements, each with about 62 tags, re-resolved per statement; `current_class_template_params` rebuilds a `HashMap<String, TemplateParam>` for each tag).

Do (designed in §11.7): a direct lowering `TypeNode → Type` in `phpdoc`, which may depend on `types` (`types` depends only on `syntax`, so no cycle), with the lowered type cached in a cell beside each type node of the memoized `DocBlock`; each distinct docblock's types are then lowered once per process, and the `types` crate's text grammar is retired in favour of the one phpdoc grammar; name resolution stays per use, and class template parameters are built once per class check, not per tag. Gate: the lowered type equals `parse_written_type(to_source_string(node))` for every type node of the fixtures, the vendor tree and the corpus (the round trip is the oracle), then the round trip is deleted. `mentions_mixed` (`type_node.rs:13`) reads the node instead of its text.

### P8. `types`: transforms that walk types with nothing to change, predicates asked of the relation every time — ≈0.4 G

- `Type::resolve_conditional` (`crates/types/src/conditional.rs:32`) runs for every call return type (333,718 calls) and walks the whole type through `map_children` (634 K calls, 122.6 M) even when it holds no conditional.
- `Type::map_class_names` (`transform.rs:76`) and `replace_objects` (`transform.rs:60`) rebuild and re-intern types that hold no class name (`map_class_names` 359.2 M inclusive over 610 K calls; `replace_objects`' `map_children` 185.9 M).

- `Type::is_null`, `is_object`, `is_array`, `is_string`, `is_bool` (`queries.rs:736-875`) are each `is_within(probe)`, i.e. `Relation::structural().is_super_type_of(probe, self)`: 651,785 + 221,491 + 119,373 + 82,666 + 58,892 calls, 137.4 M of relation work under them, plus `to_string_type`'s 167,674 (31.1 M).

Node flags already propagate bits up (`HAS_TEMPLATE_REF`, `HAS_LATE`, ...; 8 of 16 bits used, `lib.rs:107-118`). Do (designed in §12.3): add `HAS_CONDITIONAL`, set by every `Conditional`, since `resolve_conditional` also turns an unresolved template-subject conditional into the union of its branches (`conditional.rs:58`), and `HAS_OBJECT`, set by `Object`, the only node `map_class_names` and `replace_objects` rewrite. Pass both up like the other bits, and return `self.clone()` at the top of each transform when its bit is clear. Exact: with the bit clear there is nothing the transform changes, and `map_children` already returns the same node when nothing changed; the bits are computed where the other flags are.

For the predicates: the node already caches `is_refined_string`'s answers, two bits per kind in `refined: AtomicU32` (`lib.rs:101-104`, 20 of 32 bits used), filled by the relation on the first ask (`queries.rs:778`). The structural `is_*` questions are the same kind of question (context-free, about the node alone): give each two bits (widening to `AtomicU64` if needed, checking `size_of::<Node>()`), filled the same way. Exact by construction.

Union construction is already memoized (621 K `Type::union` calls, 38 K misses); a miss costs ≈11 K Ir (`union_types_impl` 431.9 M, of which `reduce_array_members` 202.7 M and `absorb_contained_members` 77.0 M). The final plan's E4 (one n-ary pass, absorption by an index) stands; no new evidence here beyond the per-miss cost.

### P9. Reference keys as folded names — ≈0.35 G (with C3)

`deps::Collector::add` takes a `String` built by `dependency_key` (trim, then `to_ascii_lowercase`, one allocation per reference) and inserts it into per-file `FxHashSet<String>`s (`deps.rs:43`, `:115`); `visit_name` alone is 137.2 M charged over 291 K names, the whole collector 841.6 M. The sets then feed `register_references` (113.2 M serial, `HashMap<String, HashSet<FileId>>`) and `closure_scope` (180.1 M, `String` frontier and sets). Do: with C3's folded identity, a reference is a folded `Name` (no allocation for a name the file already interned, a pointer hash), the reverse indexes are keyed by it, and `closure_scope` walks names, not strings. The doc-comment token lists of `DocNames` become folded names too.

### P10. Smaller parse-phase items

- **Generated autoload file** (`composer::vendor::parse_generated_file`, `vendor.rs:158`): the full PHP parser (and `annotate`) on the main thread, 171.2 M serial, then `Resolver::path` 93.6 M rebuilding each path through `PhpString`'s `Display`. Do (§11.10): parse it with the one parser, on a worker while the rest of `prepare` runs (no second, hand-written grammar for Composer's shape). Build resolved paths from the string's bytes, and resolve directories with one `read_dir` each.
- **`body_contains_yield`** (`syntax/src/visit.rs:761`): 52.0 M of walks to learn whether a body yields, asked per function check and also per loop body (`stmt/loops.rs:76`, `:304`). The parser knows it when it parses a `yield` inside a function body: a flag on `FunctionDecl`/closure/arrow function/hook. The loop-body questions come from the loop body's statement result (C1). Exact (§11.10).
- **`FileFacts` for the per-file check walks**: the checker's pre-walks each walk the whole file: `Promotions` 80.2 M + 174 M of callbacks, `GuardArgs` 166.6 M inclusive, `global_constants::Collector` 110.8 M (magic constants inside closures), `Members` 78.7 M. `Promotions`, `Members` and the magic-constant walk go entirely: under the one traversal they are observers or traversal state (§12.10). `GuardArgs` is needed before checking, so it stays as a pass, gated on an identifier fact the lexer records when it first sees a name in the file (§11.10).

### P11. PHPDoc tags parsed once per distinct tag text — ≈0.2-0.3 G

`phpdoc::docblock::parse` is 784.1 M for 37,947 distinct docblocks (the whole-text memo answers 94,228 lookups, 60% hits), ≈20.7 K Ir each: `apply_tag_recording_errors` 518.2 M, of which the tag grammar (`parse_tag_rest_shared`) 312.3 M, plus a scratch `DocBlock` built per tag and merged (`drop_glue::<DocBlock>` 38.0 M over 75,612 drops, `HashMap<String, TypeNode>` extends 27.0 M, `Vec<TypeNode>` extends 14.3 M, `HashSet<String>` extends 13.1 M). The memo is by whole text, but the same tag (`@return void`, `@param string $name`, `@throws \InvalidArgumentException`) recurs across thousands of different docblocks. Do (the final plan's B8, declined then on effort, not on correctness): `apply_tag` becomes a pure `parse_tag(name, rest) -> TagEffect`, memoized by `(name, rest)` text, and a cheap `apply(effect, &mut DocBlock)`; the scratch block and its merge go away. Saving: the merge overhead (≈0.17 G measured earlier) plus the grammar work of every repeated tag. Gate: `DocBlock` equality against today's parse for every docblock of the fixtures, the vendor tree and the corpus. Most docblocks are in context files, so §7 removes the rest on warm runs.

### P12. Each file's products from the parse — ≈0.55-0.6 G

After parsing, every file is walked again for each product:
- `resolve_names` (376.3 M, a mutable walk that `parse_program` calls, `parser.rs:350`; ≈146 M of it is the walk, the rest the resolution itself);
- references (`deps::both_referenced_names`, 841.6 M: ≈198 M of walk, the rest P9's keys and doc reader);
- declarations (`for_each_declaration`, ≈160 M of walk around the builders, plus eight more callers that walk again);
- `FileBodies::of` (≈112 M of walk).

Five of these walks rebuild the import tables from the `use` statements (`add_uses`, 104.6 M). Nothing in Part I's first version designed this; the sixth audit found it (§10.8).

Do (§11.11): the parser resolves names as it builds nodes, in source order as PHP does, and records references and declarations as it resolves them. It publishes each import state once. What needs `phpstan-rs`'s knowledge (body summaries, global-variable sites, doc names in bodies) is built by one walk.

## 4. Check phase (10.34 G)

### C1. Termination as a by-product of checking — 1.49 G (5.9%)

`check_block` (`crates/phpstan-rs/src/checker/stmt.rs:60`) calls `always_terminates(stmt, locals)` after checking each statement; for an expression statement that is `self.infer_expr(e, locals)` (`control_flow.rs:100-103`) with no infer cache open, so the whole expression is inferred again from nothing, against the scope after the statement. A `Block`/`If`/`Try` recurses into its children, so a statement nested `d` deep is re-inferred by each enclosing level. Measured: `always_terminates::{closure#0}` 1,553.3 M, of which **1,493.8 M is 174,288 outermost re-inferences**. Callers: `check_block` 793.7 M, `check_function_impl` 317.1 M (whether a body falls off its end, `function_decl.rs:1044`, `:1558`), closure bodies 315.3 M (`expr.rs:3844`), `check_if` 89.9 M. `exits_function_or_loop` has the same shape (rare here).

The statement check already computes that type: `StmtKind::Expression(e) => { let ty = self.check_expr(e, locals, label); ...` (`stmt.rs:433-435`). Do: `check_stmt` returns how the statement completes (falls through or always terminates), built bottom-up during checking: an expression statement terminates when its checked type is `never` or it is a recorded terminating assertion; a block when any statement does; an `if` with `else` when every branch does (each judged in its own branch scope, as `check_if` already has them); `switch`, `try`, `while (true)`, `for (;;)` as `always_terminates` defines them. `check_block` accumulates it; `check_function_impl`, closures, `switch` and `try` read the result instead of calling `always_terminates`, which is deleted (or kept only for code that was never checked).

This is PHPStan's own design (`StatementResult::isAlwaysTerminating()`). It is also more correct: today the expression is re-evaluated against the scope after it ran (`$x = $x->next()` asks `next()` of the new `$x`), and an `if`'s branches are judged against the merged scope after the `if` rather than their own. So it can change diagnostics where the two scopes disagree; gate: corpus and fixture diff, each difference read.

### C2. Call return types without template work for template-free signatures — ≈0.6-0.8 G

`infer_call_return_type` (`checker/expr/array_ops.rs:455`) runs for every call (333,718), 1,076.2 M inclusive. Its doc says "a signature with no templates at all is untouched (the common case, cheap)", but `call_template_bindings` (`:501`) always: allocates a `HashMap<String, Type>` sized to the parameters (52.9 M plus 77.1 M of aligned allocation), infers every argument again (483.3 M, 374 K inferences), stringifies every parameter name (`Name` `Display` 82.9 M over 502 K), inserts (53.6 M), promotes class-string literals (35.9 M), infers template bindings (75.9 M), and drops the map (49.2 M); then `substitute_templates_resolving_to_bounds` (88.5 M) and `resolve_conditional` (93.5 M) run over the return type. At most 71 K of the 333 K return types hold a template reference (`substitute_with`'s closure runs 71,069 times).

Do: when the return type has neither `HAS_TEMPLATE_REF` nor P8's `HAS_CONDITIONAL`, return it after `resolve_deferred_value_of_now` without computing any binding (the bindings are read only by those two transforms). For the rest: `param_bindings` keyed by parameter index (or the parameter's `Name`), a small inline vector, built only when the return type holds a parameter-subject conditional; template bindings keyed by the template's `Name`. Exact. Counter to add first: share of calls taking the fast path (expected ≈79% from the `substitute_with` count).

### C3. Names as interned, folded identities — ≈1.0-1.3 G (the largest checker theme)

Evidence:
- `NameTable<ClassReflection>::get::<str>` 354.6 M (1.40 M lookups, ≈250 Ir each), `NameTable<ClassOwners>::get::<str>` 335.9 M (1.32 M), `contains_key::<str>` 149.4 M charged (574 K), `get_with_name::<str>` 122.5 M, and lookups by `Name` (`with_key`) 247.4 M (1.84 M). A `&str` key with a capital is copied into an `ArrayString` and lowercased (`with_function_key`, `name_table.rs:50`; 16.2 M + 15.1 M of `memcpy` from those two `get`s alone), then hashed byte by byte, then compared with `memcmp`; a `Name` key skips the copy but still hashes its lowercase bytes.
- `Name` → `String` through `Display` 288.9 M (1.70 M): `call_template_bindings` 82.9, `narrowing_key` 41.9, `resolve_static_class_name` 26.1, `statement_assigned_variable_names` 17.8, property writes 16.4, `declared_method_name` 13.8 ...; each a `Formatter` round trip and a growing `String`.
- `String`/`&str` → `Name` through the interner (`Interner::name` 319.8 M over 1.34 M; `Name::from(&str)` 209.7 M over 839 K): `Type::bind_self` re-interns the current class 103 K times (23.8 M), `resolve_name_expr` 130 K (34.5 M), `all_ancestor_names` 72 K, stub decoding ...
- `Registry::class_template_bindings_rec` (165 K calls, 166.0 M) keeps its visited classes in a `HashSet<String>` (49.4 M of inserts) and looks classes up by `&str` (43.4 M); `own_owner_names` (finishing passes, 15,656 classes) inserts 326,655 member names into three `HashMap<Name, Name>` per class, 78.7 M, each insert hashing the name's text.
- The checker keeps the current class and many class names as `String` (`current_class`, `declared_method_name` returning `String`), the method memo at HEAD is `FxHashMap<String, FxHashMap<String, ..>>` (two string hashes per hit), property writes are keyed by `(Option<String>, String)` (31.5 M of hashing in `resolve_transitive_purity`).

Do:
1. `NameData` gains its folded hash (FxHash of the lowercase bytes, computed once in `Name::new`) and a pointer to its folded `Name` (itself when already lowercase), so "the same name, ignoring case" is a pointer comparison between two folded names of one interner.
2. `NameTable` becomes a `hashbrown::HashTable` of entries hashed by the folded hash: a lookup by `Name` is one load, one probe, a pointer compare; a lookup by `&str` hashes the lowercase bytes on the fly (no copy into a buffer) and compares case-insensitively against the stored folded text. The base table read in place from the stub cache (`Base`, with its own index, `name_table.rs:205`) is indexed by the same folded hash, so a `Name` lookup into it skips hashing too.
3. Callers hold `Name`s: the checker's current class, resolved class names, `declared_method_name`, method memo keys (`(folded class, folded method)` pointers), template binding keys, property-write keys, reference keys (P9). `bind_self`/`bind_static` take a `Name`. Nothing formats a `Name` to compare or to key a map.
4. Interner identity: a `Project` owns its interner while deserialization and `From<&str>` go to the global one (`name.rs:13-16`), so two `Name`s of the same text can be different allocations. Folded-pointer equality is only valid within one interner; either the project uses the global interner (one per process; the weak table already frees what nothing holds), or equality keeps today's byte-compare fallback after the pointer test. The first is simpler and makes the fast path exact.

Gate: `NameTable` differential test against the current table on random spellings (the test that exists for `with_function_key` extended to lookups); then the callers move file by file. HEAD's method memo is replaced by a per-class method table keyed by folded method name. That table changes one thing on purpose. Today a lookup tries the exact spelling first, so a call written `doStuff()` finds a parent's `doStuff` even when the child overrides it as `DoStuff`; PHP calls the child's, and so does a folded table. That makes this a semantic change in that one case, with its corpus differences listed (§12.4).

### C4. `Locals` with interned keys, a dependency index and structural diffs — ≈0.7-0.9 G of a 1.1 G family

The scope (`checker/locals.rs:29`) keeps types, natives, provenance and a dozen fact maps in `imbl` hash maps keyed by `String`. Real variables and synthetic narrowing keys (`a->b`, `m[$k]`, `truthy(...)`) share that string space and are built by `format!` (`narrowing_key` 166.2 M inclusive, 338 K calls; `locals.get(&format!("{base_key}->{prop}"))` in `chain_link_type_uncached`, `expr.rs:3058`).

Measured, charged: the locals family (`checker::locals`, `merge_scopes`, `record_branch_implied_facts`, `narrowing_key`, `purge_stale_narrowing_for`, `key_depends_on`, implied-fact helpers) 1,015.2 M, plus 141.1 M of `imbl` calls from the expression code; `imbl` self is 534.1 M, of which copy-on-write node copies (`make_mut`) are 139.0 M because a node copy clones every `String` key in it. Two scans dominate: `record_branch_implied_facts` (`narrowing.rs:426`) iterates every local of the branch scope three times per condition (890 K iterator steps over 24,810 calls, 320.7 M), and `merge_scopes` (`control_flow.rs:576`) visits every key of both scopes per join (22,497 joins, 319.7 M), although only keys written since the fork can differ (earlier measurement: 94.2% of visited keys unchanged).

Do (the final plan's E6+E7, in full):
1. `LocalKey`: an interned key per checked function: a root variable `Name` plus a path of property and index steps (literal key or subscript variable), with a dense `u32` id. A real variable is a key with an empty path. `narrowing_key` returns a `LocalKey`, never a string; the ~164 `locals.*` call sites and ~30 producers change in one commit.
2. The persistent maps are keyed by key id (`u32` keys), so a node copy is a copy of ids and `Type` pointers.
3. A dependency index: per root and per subscript variable, the synthetic keys that depend on it; a purge visits only those (replacing `is_real_variable_key` tests and the scans of `synthetic`).
4. What changed between two forks of a scope: `merge_scopes` starts from the then-scope and recomputes only the keys that differ; `record_branch_implied_facts` looks only at keys the branch changed, which is all it can use (its three loops require `before.get(name) != after`). §12.5 takes these from `imbl::OrdMap::diff`, which skips shared nodes, rather than from a log kept beside the maps.

Gate: the string-keyed `Locals` kept as an oracle in a differential test over the fixtures (every scope at every statement compared by rendered keys), then removed.

### C5. Diagnostic text built only for diagnostics, and statement rules that re-resolve the call — ≈0.45 G

The whole run builds 32,581 diagnostics (`DiagnosticBuilder::build`), but formats hundreds of thousands of texts that only a diagnostic would read. Sites, from the line-tables twin (call sites inside inlined code resolved with `addr2line -i` on the call instructions):
- `check_expr_impl`, every method/static/property access: `describe: &format!("method {owner}::{method}()")` (`expr.rs:1371`, 43.8 M over 49,509), `callee_label: &format!("{class}::{method}")` (`:1417`, 36.2 M), the static-call twins (`:2691` 28.4 M, `:2731` 24.8 M over 34,228), the property `describe` (`:3031`, 27.1 M over 31,364), the constructor label (`:2475`, 6.3 M): ≈167 M. `describe` is read only inside `check_member_visibility`'s report (`visibility.rs:125`); `callee_label` only in the boolean-trap and swapped-argument messages (`calls.rs:517`, `:590`), both behind opt-in settings.
- `check_no_discard_statement` (`checker/calls/misc_rules.rs:30-80`), for every call statement: formats the callee (`format!("{owner}::{method}()")` and its function and static twins: 104,781 times, 49.6 M) before it tests `sig.no_discard()`; and for a method call it first re-infers the receiver and looks the method owner up again (`infer_expr` 41,089 times, 60.7 M; `find_method_owner` 40,522 times, 29.6 M), although the expression check resolved that same callee a moment before.
- `check_function_impl`, every function: `label`, `display_name` and `subject` (`function_decl.rs:291`, `:396`, `:1145`; 33,184 formats, 20.4 M); `check_program` 11,314 (11.0 M).

Do: (1) message parts become small `Copy` values implementing `Display` (`MemberLabel { kind, owner, name }`, `CalleeLabel::Method(class, method)`, `FunctionLabel`), formatted inside the report; (2) `check_no_discard_statement` tests the attribute first, and takes the callee from the expression check (the call arms record the resolved signature and owner of the call node, which the statement rules read) instead of resolving it again; the same record serves `check_pure_call_without_effect` and `check_new_exception_discarded`. The string machinery behind all of these (`String::write_str` → `reserve` 434.8 M over 3.33 M buffer growths) shrinks with C5, C3 and C4 together. Exact.

### C6. Cheap tests first, precomputed facts, exact gates — ≈0.5 G

- `check_node_connecting_attribute` (`checker/phpstan_api.rs:417`), called for every method call (50,201), computes `class_implements_phpstan_interface(current_class)` — `all_ancestor_names`, a BFS with a fresh `HashSet` and `Name::from` (191.8 M) — before it tests `method == "getAttribute"`. Do: test the method name first, and compute the calling class's answer once per class the checker enters. It cannot be read from the precomputed `ancestor_names` table (`registry/hierarchy.rs:122`): that table holds `extends` and `implements` only, while `class_implements_phpstan_interface` (`:244`) also walks `uses` (`hierarchy.rs:288-293`). 208.8 M, exact (§12.7).
- The PHPStan-API walker (`check_phpstan_api_usage`, `project.rs:2119`) walks every analysed file (107.8 M) behind `settings.phpstan_api` only. `FileState::mentions_phpstan` alone is not an exact gate: the runtime-reflection and old-PhpParser rules fire inside any class implementing a `PHPStan\` interface through a project interface, which the file need not name. Exact gate: `mentions_phpstan || the file declares a class whose precomputed ancestors include a PHPStan\ name`. Under the one traversal (§12.10) its rules are an observer family registered under that gate, and the walk goes. ≈0.1 G.
- `method_call_return_type` (`checker/expr/member_resolution.rs:524`) asks `classes.contains_key(class)`, then `member_confirmed_missing` (`contains_key` again, `lookup_method(c, method)`, `lookup_method(c, "__call")`: 144.3 M over 144 K calls), then `declared_method_return_type` looks the method up again with `lookup_method_with_owner`. Do: one lookup with owner; `None` and no `__call` is the error type. ≈0.15 G before HEAD's memo; whatever the memo leaves.
- `bind_arguments` (`checker/calls/parameters.rs:421`) builds a `FxHashMap<&str, &ParameterReflection>` of every parameter for every call (22.9 M of inserts, 25.8 M of growth over 144 K calls) though it is read only for a named argument. Do: no map; a named argument is matched by scanning the parameter slice (a handful of entries) with `Name` identity. ≈0.05 G, exact.

### C7. The infer cache as per-node slots — ≈0.2 G

`cached` (`checker/expr.rs:4486`) costs 151.0 M of self plus 131.8 M of `HashMap<InferKey, Type>` inserts (1.35 M) for 1.66 M lookups and a 32% hit rate: ≈170 Ir per lookup for a 32-byte key. Node ids are dense per file (annotations index vectors by `id.index()`). Do (§12.8): a per-file vector indexed by node id, each node holding a short list of `(expression address, scope version, question, Type)` entries for the current generation. Opening an outermost inference bumps the generation instead of clearing a map. The key set is exactly the map's, so the hits are too, with no hashing. (A first version kept only the latest version per node and needed a hit-rate comparison; that trade-off is gone.)

### C8. Interning without building first, and scope-free literal types — ≈0.2 G

`types::intern::insert` is 605.1 M inclusive over 2.15 M calls (≈281 Ir): the caller builds a `Repr` (allocating its `Vec`s and `Arc`s), `insert` hashes it, probes the thread-local front cache (205.4 M) and, on a hit, drops the `Repr` it was given (`drop_glue::<Repr>` 104.8 M). The busiest constructor is `Type::string_literal`: 386,555 calls, 145.4 M, 204,794 of them from `infer_expr_unguarded` re-inferring the same literal nodes and 111,989 from `check_expr_impl`. Do (§12.9): (a) every constructor interns from borrowed parts (`ReprRef`, a borrowed mirror of `Repr` with one `exact_hash` definition) and builds the owned `Repr` only on a miss; (b) a literal's type does not depend on the scope, so C7's entry for a literal node could be kept for the whole file check. That part is dropped (§10.8): the 204,794 calls are re-inference, which §12.10 removes, and after its step 4 a literal is typed once per node per pass through (a). Exact.

### C9. Loop convergence

`foreach` bodies run 1.55 passes on average here (2,177 loops, 3,382 passes, 1,365.8 M of body checking), `for` 4.1 (180 loops, 744 passes, 346.3 M), so roughly 0.6-0.7 G of this run is passes after the first. Each pass is a full check whose diagnostics are kept only if it is the last. One alternative is to iterate with rules off (a scope-only mode) until the entry state is stable, then run one full pass. Counted in passes, independently of any corpus: a loop stable after its first pass costs 1 full pass today and 1 scope-only + 1 full that way; stable after its second, 2 full against 2 scope-only + 1 full (or 1 full + 1 scope-only + 1 full with a full first pass); from the third pass on the scope-only design wins by (passes - 2) x (full - scope-only). So it pays only if a scope-only pass is much cheaper than a full one and loops needing three or more passes are common. The proper form is C10's architecture (§12.10). There, rules observe the traversal and cannot change the scope, so a convergence pass can record what the rules would see instead of running them, and only the fixpoint pass's record is replayed through the rules. That is PHPStan 2.2's own scheme. It runs the rules once per loop and walks the body only as often as convergence needs, so it beats every schedule above at every pass count from two up, with nothing to choose or measure (§12.10).

### C10. Two traversals, check and infer (architecture note)

`check_expr_impl` re-infers sub-expressions it has just checked (94,506 inferences, 399.3 M) and the checker mutates `locals` between steps, so the version-keyed cache misses; `infer_expr_unguarded` runs 1.13 M times against 609 K `check_expr_impl` calls. A single traversal that returns the type and the new scope from each node (PHPStan's `NodeScopeResolver`), with rules as observers of it, removes the second pass by construction. It is designed in §12.10: an `At` context, a rule registry, and migration of the 335 `check_*` entry points family by family. C1, C2 and C7 come first and are its building blocks.

## 5. Finishing passes (≈2.64 G)

- **F1 Stub classes decoded member by member.** `warm` reads the stub entries in the passes' scope in parallel, which decodes them: `load_base` initialises 6,320 class entries (379.6 M) and 9,023 function entries (39.8 M); 3,949 of the class entries decode a `ClassReflection` (`stub_cache::decode`, 376.1 M, ≈95 K Ir each). Of a class decode the methods map is 291.2 M (77%), properties 36.5 M, constants 19.9 M. Do (§13.2): methods stored as separately decodable records with an offset index, decoded on first lookup (a `OnceLock` per member). The finishing passes that iterate every method of a stub class decode them; the checker decodes only what it asks for. It is built after F2 settles which stub classes the passes should see, not gated on a count.
- **F2 Why stub entries are in the scope at all.** The first full run takes the incremental path, because the stub base counts as resolved, and `closure_scope` adds every stub file that references a changed name, transitively. Stub files reference only core names, so project names reach them through polyfills: Piwigo's vendor tree has `symfony/polyfill-php80`, `-php82`, `-php84` and `-php85`, which declare core classes inside `PHP_VERSION_ID` guards. Its `composer.json` requires PHP ^8.5, so those guards can never hold, yet `insert_built_decl` merges the polyfill into the stub class and marks the name changed (`registry/index.rs:108-178`). Do (§13.1): evaluate declaration guards against the configured version range and the stubs' availability, so a declaration that cannot exist is not one; and record a name as changed only when its entry changed.
- **F3 No raw copies in a one-shot run.** `end_declaration_changes` with `keep_raw` clones every class and function declaration to keep the as-built form for later incremental runs (`reset_scoped`, `registry.rs:116`: 137.6 M over 15,626 classes, 17.5 M over 6,730 functions). A CLI run never reads them. Do (§13.3): structural sharing at the entry, member-map and member level (`Arc` with copy-on-write at each), so a raw copy is a pointer copy and a finishing pass that changes one method copies that method only. This also removes `ClassReflection::clone` 53.0 M and `FunctionReflection::clone` 35.9 M elsewhere. There is no run-mode flag: one path serves one-shot and incremental runs. With sharing, `changed_since`'s field-by-field comparison of every snapshot entry (68.7 M, serial) compares pointers first, and fields only for the entries a pass copied.
- **F4 Property writes keyed by names.** `property_writes` keys are `(Option<String>, String)`; `resolve_transitive_purity` hashes 398 K of them (31.5 M) and moves them through `HashMap`s and rayon collects; the classification itself is `property_writes::classify` ≈470 M including stolen work. With C3 the key is a pair of folded `Name`s.
- **F5** `closure_scope` (180.1 M, two calls, serial): a breadth-first search over dense `FileId`s with a bitset and P9's `RefKey`s, the two calls side by side (§13.4).

## 6. Main thread (1.08 G, serial)

- `prepare` 365.0 M: Composer discovery 318.9 M, of which the generated autoload file parse 171.2 M and path resolution 93.6 M (P10).
- `insert_parsed_file` 390.5 M over 14,251 files: `register_references` 113.2 M (P9), `PackageIndex::owner` 62.5 M, `insert_built_decl` 61.4 M, `absolute_path` 51.6 M (`lexically_normalized` 48.9 M), `FileTable::id_for` 50.1 M (a `PathBuf` hash map). Do: the path work (`absolute_path`, owner, the file id's path hashing) moves into the parse worker closure, which already knows the path, so the serial loop only merges (the final plan's pipeline item, its C3).
- `sort_paths` 108.7 M: four sorts, already `sort_by_cached_key`, whose `path_order::key` builds a `Vec<u8>` from `Path::components()` per path (73.6 M over 34 K paths). Also `collect_php_files` 68.2 M and the size sort 40.8 M. Do: a directory walk that sorts each directory's entries before recursing, so its output is already in order and needs no key at all (§13.5).
- `render_json` 23.8 M.

## 7. Architecture: a persistent per-file cache (warm runs)

Context files are 11,459 of 14,251 files and about 41 of the 60 MB lexed; by bytes they are ≈68% of lexing and parsing, and a larger share of declaration building. Their whole parse-phase product is a pure function of the file's bytes, its path and the build: an upper bound of ≈7.5 G (30% of the run) of work that a warm run repeats unchanged. The lazy-vendor experiment was rejected because what is not loaded is not seen; a cache loses nothing, since every file is still loaded, only from a stored form.

What each file needs after the parse phase, and how it would be stored:
- declarations (`built_decls`: `ClassReflection`, `FunctionReflection`), names, duplicate sites, facts: serializable as the stub cache already serializes reflections (postcard);
- references and declaration references (P9's folded names);
- the AST, read later by `FileBodies` (purity and property writes read vendor bodies) and by `infer_global_variables_of`'s `walk_top_level` (227.5 M over every file), and by LSP go-to-definition. Those consumers move to per-file summaries built on the parse worker, for every file and every run, not only for the cache (§14): each body's purity events before the fixpoint, its property-write events, each file's global-variable sites (`FileSites`). Context files then drop their AST after loading, and the cache stores no AST.

Key: the file's content hash, path and the build's source hash. Gate: a run from cache gives byte-identical output to a cold run over the corpus and the vendor tree. Open questions, as in the final plan's "resolved vendor base": the per-body summaries' dependence on other classes (property writes follow typed receivers), and the LSP's need for ASTs of files it navigates into.

## 8. Ranked proposals

Savings are upper bounds read from this profile, in Ir of 25.14 G; items overlap where noted. "Exact" means output cannot change; "semantic" means it can and the corpus diff decides.

| # | item | saving | kind | churn |
|---|---|---|---|---|
| C1 | termination as a by-product of checking, exit points by level, `goto` modelled | 1.49 G | semantic (more correct) | medium |
| C3 | names as interned folded identities; callers hold `Name`s | 1.0-1.3 G | exact, except mixed-spelling method resolution (semantic, as PHP) | very high |
| P5 | Pratt expression parser by PHP's precedence table; parse errors reported | 0.9-1.0 G | exact where today's parse matches PHP; semantic in six listed cases | high |
| C4 | `Locals` by interned key, dependency index, structural diffs of ordered maps | 0.7-0.9 G | exact (oracle) | very high |
| C2 | template-free call return types skip bindings | 0.6-0.8 G | exact | low |
| P4 | byte-offset spans, lazy line index | ≈0.7 G | exact | high |
| P1 | delete the dead `annotations` module; `goto` labels from the parser | 0.43 G | exact | low |
| P7 | one phpdoc grammar; `TypeNode` → `Type` lowered once per docblock; resolution memoized per resolution context | 0.35-0.45 G | exact (round-trip and resolution oracles) | high |
| P9 | reference keys as folded names (with C3) | ≈0.35 G | exact | medium |
| C5 | diagnostic text only for diagnostics; statement rules read the call's resolution | ≈0.45 G | exact | medium |
| C6 | cheap tests first, per-class PHPStan ancestry, one method lookup, no `by_name` map; the API walker and the private-member walk become §12.10 observer families | ≈0.55 G | exact, except member usage by receiver type (semantic) | low |
| P6 | streaming lexer (no token vector), byte-class dispatch, cross-file name cache, perfect-hash keywords, one-allocation string literals | ≈0.3 G, plus ≈0.33 G of token vectors | exact | high |
| P3 | UTF-8 validated once | ≈0.22 G | exact | low |
| P2 | ignore marks recorded by the lexer for every file; no comment list or line starts | ≈0.25-0.3 G | exact | medium |
| P8 | `HAS_CONDITIONAL`/`HAS_CLASS_NAME` early exits, cached `is_*` predicates | ≈0.4 G | exact | low |
| C7 | infer cache as per-node entry lists | ≤0.2 G | exact | low |
| C8 | interning from borrowed parts (`ReprRef`) | ≈0.15 G, the literal re-inference counted under §12.10 | exact | high |
| §12.10 | one traversal, rules as observers; loops by record and replay (C9, C10) | ≈0.4 G of re-inference, plus part of 0.6-0.7 G of later loop passes | semantic (rules see the scope at their node), per family | very high |
| §12.11 | n-ary union over a member index | part of 0.43 G of union misses | exact (fold oracle, differences classified) | high |
| F1 | stub members decoded on demand (after F2) | ≤0.3 G | exact | high |
| F3 | declarations shared structurally (entry, member map, member) | ≈0.15-0.25 G | exact | high |
| P10 | autoload parse off the main thread; parser generator flags; the remaining `FileFacts` gate | ≈0.4 G | exact | medium |
| P11 | phpdoc tags parsed once per distinct tag, no scratch block | ≈0.2-0.3 G | exact (oracle) | medium |
| P12 | names resolved, references and declarations recorded by the parser; one walk for the summaries | ≈0.55-0.6 G | exact (oracle) | high |
| §7 | body summaries for every file (context ASTs dropped), then a persistent per-file cache | finishing-pass body walks on every run; ≤7.5 G on warm runs | exact (summary and cache oracles) | very high |

Sum of the rows above §7: ≈10.7 G at the middle of each range; the overlaps (C3 with P9, C2, C5 and F4; C2 with P8; P4 with P6) are ≈1.2 G, so ≈9.5 G net, the run going from 25.1 G to about 15.5-16 G before the cache. What Part II added is left out of that sum: the streaming lexer's ≈0.33 G in P6's row, the §12.10 and §12.11 rows, which overlap with C1, C2, C7 and P8, and the cold-run part of the §7 row.

Order, by dependency (the full commit sequence with checkpoints is §15):
1. P1, C6, C2 (with P8), C5, P3, C7, C8 first: independent and small.
2. C3, which P9, F4 and the method memo build on; then C4.
3. P2, then P4 with P6's token rework and P5: all three change the front end's hot loop, so one oracle serves them.
4. P7 and P11, under one docblock oracle; P10.
5. C1, with its own corpus diff.
6. F1-F3, after F2's measurement.
7. §7 last, after P9 and C3 have made the per-file products small and serializable.

## 9. Line-level detail (line-tables twin of `cff7bde1`)

Twin: `~/.cache/prs-profile/lt/cg.out-01..09`, binary `~/.cache/prs-profile/lt/prs-cff7bde1-linetables` (built with `CARGO_PROFILE_GATE_DEBUG=line-tables-only` in a throwaway worktree at `cff7bde1`, since removed), run by `lt/run.sh`. Total 25,193,022,601 Ir (+0.2% against `cl-final`: per-thread memos and work stealing differ from run to run), output byte-identical to the baseline in both its plain and its callgrind run. Line numbers are `cff7bde1`'s, which match HEAD in every file cited here except `lexer.rs` below `lex_doc_comment`.

**Lexer, `run_lexed` (680.5 M self).** The inlined position code is ≈277 M of it: the cursor test `lexer.rs:291` 75.5 M, `:292` 25.0 M, `:297` (the `columns` call) 34.6 M, `:299` 17.3 M, `:290` 13.8 M, `in_file` `:310`/`:317` 45.1 M, and the saturating adds (`uint_macros.rs:2526` 41.4 M plus 15.4 M unattributed). Pushing the token and its span and moving the 32-byte `Token` is ≈200 M (`ptr::write` 61.8 M, `Vec::push` 75.4 M, `:489` 27.2 M, `raw_vec` 20.5 M), `pending_keyword_raw.take()` 27.6 M. With `relative_line_col_at` (216.6 M self) and `columns` (202.9 M self), positions cost ≈700 M per run: P4's estimate stands, and is if anything low.

**Lexer, interim fixes if P4 waits.** `relative_line_col_at` spends ≈120 M in `binary_search` (`slice/mod.rs:2996-3018`) for each new line, although tokens arrive in order and the next line is almost always the cursor's line plus one: advancing from the cursor's line is O(1). `columns` (202.9 M) is mostly call overhead: its prologue (`:372`) 30.9 M and epilogue (`:384`) 41.2 M for what is `to - from` in an ASCII file; `#[inline]` with the ASCII test hoisted into `line_col_at` removes ≈150 M.

**Lexer, `next_token` (875.5 M self).** Byte-at-a-time scanning (`slice/iter/macros.rs:379`, `:385`, `:180` and unattributed, ≈180 M) is the whitespace run (`:749`) and the identifier run (`:838-840`); the function's prologue (`:544`) is 76.0 M over 6.9 M calls, a large function's frame; bounds-checked `peek()` (`:511`, 63.9 M) and slice indexing (`slice/index.rs:184`, 38.5 M) ≈100 M; the dispatch is a chain of `if c == ...` tests (`:561` 33.0 M, `:615` 30.8 M, `:644` 29.0 M, `:623`, `:625`) rather than one jump. Do, with P4/P6: a 256-entry byte-class table and one `match` on the class, the rare forms (`?>`, `#[`, `b'`, heredoc, doc comment) outlined into cold functions so the hot path's frame is small, and class-table scanning of whitespace and identifier runs eight bytes at a time.

**Parser levels.** Each precedence level's self cost is mostly plumbing: the `?` on `Result<Expr, ParseError>` (`result.rs:2176-2177`: 48.9 M in `parse_unary_inner`, 25.4 M in `parse_not`, 21.7 M in `parse_binary`, 23.5 M in `parse_ternary`, 21.0 M in `parse_low_precedence_or`), the move of the result out (`expr.rs:460` 37.0 M, `:389` 35.1 M, `:354` 23.5 M, `:282` 32.3 M, `:28` 25.2 M), bounds-checked token reads (`slice/index.rs:184`, 12-34 M per level), the prologue (12-17 M per level) and the stack guard's thread-local read (`thread_local/native/mod.rs:75`, ≈6-7 M per guarded level). Operator tests are a small part. A Pratt loop keeps one level's worth of this per operand (P5).

**`format!` sites.** Resolved in C5 (call instructions of `check_stmt_impl` disassembled and their inline chains read with `addr2line -i`: the per-statement one is `misc_rules.rs:40`/`:51`/`:67`/`:78`, inlined through `stmt.rs:444`).

**`NameTable<ClassReflection>::get::<str>` (298.5 M self).** ≈124 M (42%) is preparing the key before any hashing: `to_ascii_lowercase` per byte (`num/mod.rs:863`, 45.4 M), `make_ascii_lowercase` (`slice/ascii.rs:210`, 27.1 M + 10.1 M), the copy into `ArrayString` (`name_table.rs:54`, 25.2 M), the capitals scan (`:41`, 11.3 M), `is_ascii_uppercase` (`num/mod.rs:715`, 7.2 M); then FxHash (`uint_macros.rs:2758`, 10.6 M) and the probe (`raw.rs:2452`, 11.2 M). C3's precomputed folded hash removes the first part for `Name` keys entirely and makes it one pass without a copy for `&str` keys.

---

# Part II. Designs

Every proposal of Part I, designed to the point where it can be implemented: the code as it stands at HEAD (`7a3f3f99`), the data structures and signatures that replace it, the commits in order, what each must preserve and how that is checked, and the edge cases. The designs take the complete form throughout (the request was maximum churn and properness); where Part I offered a narrow fix as a fallback, the fallback is not designed.

## 10. Common ground

### 10.1 Corrections to Part I found while designing

Designing each item meant reading its code again in full, which turned up eight places where Part I was wrong or incomplete. Part I has been corrected in place; the list is here so the changes are visible.

1. **P2.** The ignore-comment products need not depend on the file's role. The scan reads only comments that contain `@phpstan-ignore` and, for each, two facts about the code next to it. The lexer can record exactly that for every file at almost no cost (§11.2). No role parameter is needed, and the saving covers analysed files too: ≈0.25-0.3 G instead of ≈0.25 G.
2. **P7.** `phpdoc` does not depend on `types` today, but it can: `types` depends only on `syntax`, so adding the dependency creates no cycle. The lowered `Type` can then live in the `DocBlock` itself (one cell per type node) instead of in a side table in `phpstan-rs` (§11.7).
3. **P8.** `resolve_conditional` rewrites every `Conditional`, template subjects included: an unresolved one becomes the union of its branches (`conditional.rs:58`). The new flag must therefore be set by every `Conditional`, not only parameter-subject ones. `map_class_names` and `replace_objects` rename `Object` nodes only (an `EnumCase` is a leaf that `map_children` leaves as it is), so the second flag is `HAS_OBJECT`, set by `Object`.
4. **C3.** Today a method lookup tries the exact spelling first (`hierarchy.rs:604`), and owner maps keep one key per spelling. So if a child declares `DoStuff` and its parent `doStuff`, a call written `doStuff()` resolves to the parent's method; PHP resolves case-insensitively to the child's override. A table keyed by the folded name resolves as PHP does. This is a semantic change, settled by the corpus diff (§12.4).
5. **C6.** `class_implements_phpstan_interface` walks `uses` (traits) as well as `extends`/`implements` (`hierarchy.rs:288-293`), but the precomputed `ancestor_names` holds only the latter (`class_extends_implements_closure`). Reading the table would therefore not be exact. The design keeps one answer per class in the checker instead (§12.7).
6. **P10.** `body_contains_yield` is also called on loop bodies (`stmt/loops.rs:76`, `:304`), not only on function bodies, so the parser flag alone does not cover every call. The loop calls are gated on the enclosing function's flag (§11.10).
7. **P10.** `Promotions` (`function_like_declarations.rs:56`) is not a fact-gathering walk: it runs the declaration checks of every function-like node. Only its descent into expressions can be gated, on a fact that the file has a closure, arrow function or anonymous class.
8. **§6.** `sort_paths` already uses `sort_by_cached_key` (`path_order.rs:32`). The 73.6 M is building one `Path::components()` key per path. The fix is a traversal that produces paths already in order (§13.5), not a cheaper comparison.

### 10.2 Oracles and checkpoints shared by the designs

- **O1, front end.** A dump of what the front end produced, kept stable across the front-end items (P2-P6, P10): for every token, its kind, payload and position as line and column; every `LexWarning`; the `{:?}` of the statement list (the AST derives `Debug` but not `PartialEq`, `ast.rs:46`, `:1372`), with spans printed as line and column through the file's `LineIndex` once P4 lands; `ParseError` messages and positions; `FileFacts`; the ignore marks. The test-only binary `xtask front-end-dump <dirs>` writes one file per source. The oracle passes when the dumps before and after a commit are byte-identical over the fixtures, `vendor/` and the Piwigo tree, both analysed and context files.
- **O2, output.** `check-project --format json` on the scoped Piwigo run and on the fixture projects, byte-identical after `LC_ALL=C sort`, saved under `.corpus-runs/<scope>/<hash>.txt` so later commits diff against the saved file rather than a rebuild. Checker items also compare `--dump-types` output, which prints every tracked variable's type after every statement; it is the oracle for C4 (scope contents), C7 (cache) and C2 (return types). Items marked semantic (C1, C3's method resolution, C5's receiver timing) instead read every line of the diff.
- **O3, cost.** A callgrind run of the scoped Piwigo command (nine thread files summed, as in §0) at each group checkpoint (§15), plus `hyperfine -N` against the previous checkpoint's binary for items whose instruction count and wall time can differ (allocator and memory-bound changes). Each commit message states its measured delta.
- **Commit discipline.** Each step below is one commit, run through `cargo fmt`, `cargo check` and `cargo clippy` before committing. An item's oracle runs when its last step lands. The full `cargo test --workspace`, `--test fixtures` and O2 run at each group checkpoint, not per commit.

### 10.3 Properness audit of the first version of these designs

Each design was re-read item by item against one question: is this the most proper form, or was something chosen because it is less work or less churn? About half of them fell short: they kept a second implementation, kept today's behaviour where it departs from PHP, gated a proper design on a measurement, or converted a listed subset where the whole class of sites is the item. The table records each verdict and what replaced it. The sections below are the corrected designs.

| item | first version | what fell short of proper | corrected design |
|---|---|---|---|
| P1 | narrowed | kept `annotate` "for a future consumer": an unused second implementation of facts the checker already tracks during its traversal | module deleted; the one fact the checker finds by a separate walk (`goto` targets) recorded by the parser (§11.1) |
| P2 | proper | — | — |
| P3 | proper | — | — |
| P4 | narrowed (minor) | a re-export shim for the old module path | moved, every import updated |
| P5 | not proper | reproduced today's grammar, including six departures from PHP's: arrow-function bodies, `include`/`require` and `throw` bind too tightly; `instanceof` chains are rejected; chained comparisons and unparenthesized nested ternaries are accepted; `?:` chains associate the wrong way | PHP's own precedence table, checked against nikic/php-parser and `php -l`; a file that fails to parse is reported, not only counted (§11.6) |
| P6(c) | gated | built only if a corpus counter showed it pays | built; O3 measures it |
| P7 | narrowed | lazy cells inside the docblock; names resolved again at every use; statement `@var` tags resolved at every visit | eager lowering; resolution memoized per (type, name context); statement tags once per file check (§11.7) |
| P9 | narrowed | kept `deps`' raw-text reader of PHPDoc types, a third grammar | doc names from the one grammar, after an audit of every doc-text consumer (§11.9) |
| P10 | narrowed / not proper | a hand-written second grammar for Composer's generated files; generator flag on functions while loop bodies are still walked | one parser, off the main thread, with the path-resolution fixes; loop flags from the parser (§11.10) |
| P11 | proper | — | — |
| C1 | narrowed | today's termination rules kept with their imprecisions: any `break` anywhere disables `while (true)`; `finally`, `do`-`while`, unreachable tails and conditions known true by type are ignored | exit points tracked by level, as PHPStan's `StatementResult` does (§12.1) |
| C2 | narrowed | parameter bindings still a name-keyed map; one choice left open ("a bit, or a walk") | conditional subjects resolved to parameter positions when the signature is built; bindings by position (§12.2) |
| P8 | proper | — | — |
| C3 | narrowed | `Name` kept its text hash, text equality and `Borrow<str>` so existing maps compile unchanged; only a listed set of call sites converted | identity equality and the precomputed hash; `Borrow<str>` removed; every `String` key classified and converted (§12.4) |
| C4 | not proper in one place | folded keys for property and constant facts, which PHP compares case-sensitively | each fact keyed by the identity PHP gives it (§12.5) |
| C5 | proper | labels over `&str` | labels over `&Name` once C3 lands |
| C6 | narrowed | the PHPStan-API walker kept as a walk of its own; the private-member check, which walks every method body twice for every class with or without private members, not covered | walker run inside the per-file check; private-member check returns at once without private members and walks once (§12.7) |
| C7 | gated trade-off | slots holding only the latest version, a hit-rate measurement deciding | per-node entry lists over exactly the map's keys: the same hits, no trade-off (§12.8) |
| C8 | narrowed | probe-before-build for a list of hot constructors | borrowed-form interning (`ReprRef`) for every constructor (§12.9) |
| C9, C10 | not designed | a measurement plan and a "direction" | one traversal with rules as observers; the loop schedule becomes a parameter of it (§12.10) |
| union (P8 note) | not designed | "unchanged here" | an n-ary union over a member index, with incremental work on already-normalized operands (§12.11) |
| F1 | gated | built only if a counter showed waste | built, after F2; the counter stays as a check |
| F2 | proper | diagnosis first: the cause is unknown, and the two possible fixes differ | — |
| F3 | not proper | `Arc` only at entry level, so the first mutation copies whole classes; plus a run-mode flag | structural sharing at entry, member-map and member level; no flag (§13.3) |
| main thread | narrowed | `PackageIndex::owner`'s per-ancestor hashing, the size sort's per-comparison map lookups and `render_json`'s in-memory document left out | designed (§13.5) |
| §14 | proper | — | — |

### 10.4 Second audit

A second item-by-item read of the corrected designs, against the same question, found 15 more places short of the most proper form. One of them, P7's memo key, was a correctness hole in the design itself. The sections below have been corrected.

| item | what still fell short | corrected design |
|---|---|---|
| P2 | resolved marks from the token-span vector, which the streaming lexer removes, so it would have been written twice | resolved inside the lexer as it streams, one offset comparison per token while a mark waits |
| P3 | validated twice when the source is kept, and `Lexer::new(&str)` re-validates text that is already a `str` | validation once, at the read (`String::from_utf8`, bytes back on failure); `Lexer::from_str` validates nothing |
| P6(d) | kept a token vector (compacted) and the four doc-comment maps keyed by token index, though the parser never rewinds and looks ahead a few tokens | a streaming lexer: tokens carry their doc group and keyword spelling, a small lookahead ring, no vectors, no index-keyed maps |
| P5 | kept the hand-made list of 30 tokens that decides whether `yield` has an operand, which misses `&`, `and`, `or`, `xor`, `instanceof` and `\|>` (so `yield and $x`, valid PHP, fails today) | `!can_begin_expression()`, PHP's own rule |
| P7 | the resolution memo was keyed by namespace and imports only, but resolution also reads the template names, type aliases and `self` in scope: the memo would have returned wrong types | keyed by an interned resolution context holding all of them; the oracle compares every answer with today's resolution |
| P11 | stale wording; memo limits to be measured | the docblock memo's own policy, which bounds it |
| P10 | the rebind scan only gated, its two whole-program walks kept | gated and fused into one walk |
| C3 | identity equality while `Interner::new()` stayed public, so names from a second interner could compare unequal with equal text | `Interner::new` removed; the global interner is the only one |
| C4 | static-property keys by the class as written (`self::$p`, `Foo::$p` and `foo::$p` three keys) and index keys by literal text (`$a['k][j']` collides with `$a['k']['j']`) | keys by resolved class and PHP-canonical array key: two semantic corrections |
| C5 | labels over `&str`, scheduled before C3 and so rewritten after it | labels over `&Name`, after C3 |
| C6 | the PHPStan-ancestry answer computed in two places (a checker memo and a registry query for the walker gate); a per-call map for named arguments, merely built lazily | one registry fact per class, computed in the finishing pass; named arguments by scanning the parameter slice |
| union | n-ary unions memoized only with two members | memoized by member list for any length |
| F5 | "re-measure" instead of a design | `closure_scope` as a breadth-first search over dense ids with a bitset, both calls side by side |
| §14 | summaries only for cached files, and only sketched; every file kept its AST and bodies | concrete summary types for purity, property writes and global-variable sites; they replace stored bodies for every file and run; context ASTs dropped after loading; the cache then stores no AST |
| §15 | C5 before C3 | reordered |

Checked again and still the most proper form: P1, P4, P6(a)-(c), (e), P8, P9, C1, C2, C7, C8, §12.10, F1-F4, the main-thread items. One limit was stated rather than designed away: C1 treated `goto` as neither terminating nor jumping, on the grounds that PHPStan does not model it either. The fourth audit found that premise false (§10.6); C1 now models `goto`.

### 10.5 Third audit

A third item-by-item read, which this time checked the assumptions under each design against the code, found nine more. Two were correctness holes: C1's block rule and F2's cause, which had been left to a measurement although the code shows it. The sections below are corrected.

| item | what still fell short | corrected design |
|---|---|---|
| F2 | left as "measure first", though the code shows the cause. `insert_built_decl` merges a version-guarded polyfill into the built-in stub class and marks the name changed, even when the configured PHP range can never take the guard. `closure_scope` then pulls in every stub file referencing it, transitively. Piwigo ships four symfony polyfill packages and requires PHP ^8.5 | guards evaluated where declarations are collected (`Never`/`Always`/`Maybe` against the version range and the stubs' availability); a declaration that cannot exist is not one; a name is changed only when its entry changed (§13.1) |
| C1 | "statements after the first terminating one are unreachable" ignored `goto` labels, so `return; target: foo();` would have counted as never falling through. Today's `always_terminates` has the same bug (`control_flow.rs:114`) | a targeted label restarts reachability; the block's result is its last reachable region's |
| P11 / P4 | the phpdoc tag lexer copies each input into a `Vec<char>`, tracks line and column per character, and tokenizes into a vector; its positions are never read for reporting | a byte lexer pulled by the parser, with byte-offset spans relative to the doc |
| C3 | the identity sweep covered map keys, not struct fields, parameters and return values holding identifiers; interning a mixed-case name could take a second shard lock while holding one | every identifier-holding value classified; the folded spelling is interned first, outside the lock |
| C4 | the new `Locals` listed 13 of its 15 maps (`query_builder_aliases` and `clone_reinitialized` were missing) | all 15, each keyed by its identity |
| C5 | an interim `StatementCall` record that the observer API would replace | the statement rules become §12.10's first observer family, right after C3 |
| §12.10 | "hypothetical questions" kept a second inference path | one engine: `process_expr` on a cloned scope, with no observers |
| P6(d) | did not say how the four consumers of token lists get them | the lexer is an `Iterator`; they collect |
| P5 | the oracle's list of allowed differences did not include the `yield` rule | included |

The remaining designs were re-read, with their assumptions checked against the code, and stand as written.

### 10.6 Fourth audit

A fourth read checked two things the earlier ones had taken on trust: every claim a design makes about PHPStan, against the vendored PHPStan 2.2.16, and every list of readers a design changes, against every reader in the code rather than the ones it names. It found 12 more.
- Three were correctness holes in the designs: §14 dropped context trees that five more readers walk, C4 keyed the member guards by a class where the code keys them by a variable, and C1's label rule missed labels inside blocks.
- One limit, kept through every earlier audit, rested on a false premise: PHPStan does model `goto`.

| item | what still fell short | corrected design |
|---|---|---|
| C1 | `goto` was left unmodelled because "PHPStan does not have it either". PHPStan 2.2.16 does: `GotoHandler` returns an always-terminating result with an exit point, `mergeForwardGotoExitPoints` merges it at the label, `resolveBackwardGotoScope` runs backward jumps to a fixpoint, and `GotoLabelVisitor` marks the labels. Separately, the label rule looked only at labels in the block itself, though PHP allows a `goto` into an `if`, `else`, `try` or plain block (checked with `php`) | `goto` terminates and records its scope for its label. At a label, the scope is the merge of the fall-through and the pending entries. Backward jumps make a region that runs to a fixpoint. Labels at any depth restart reachability, since unreachable code is still traversed. P1 marks backward labels (§12.1, §11.1) |
| §12.10 | the loop schedule was a parameter to be chosen by measuring. PHPStan 2.2.16 records what the rules would see during convergence passes and replays the fixpoint pass's record (`RecordingNodeCallback`, `replayRecording`), so nothing needs choosing | record and replay: the rules run once per loop, on the fixpoint pass. Nested loops and `goto` regions compose through the active observer |
| §14 | the readers of context trees listed five consumers, but five more walk them: the uninitialized-property dataflow (other files' method bodies, through `Checker::bodies`), the PHPStan-base switch and the incremental re-declaration (both rebuild declarations from trees), `PestConfig::build`, and `check_renamed_named_arguments`. Dropping the trees would have broken them without a word | declarations kept per file and re-inserted; an `InitFlow` summary for the dataflow; Pest facts on the worker; the renamed-arguments pass from declared names; a test-build assertion that nothing else asks for a context tree; `is_stub` in the cache key |
| C4 | the `method_exists`/`property_exists` guards were typed by class, but the code keys them by the receiver variable (`narrowing.rs:729-744`). Container-field keys were `Name`s, though they are array keys, and the discriminant subject a `Name`, though it is a variable. A rebind leaves the member guards in place today | guards by `(LocalKey, …)`, field keys by `ArrayKey`, subjects by `LocalKey`. Every fact about a variable is purged by id when the variable is rebound, a semantic correction |
| P4 | the LSP kept character columns. The server answers `utf-8` when offered but sends characters, which the spec defines as bytes, and approximates `utf-16` | positions in the negotiated encoding, from byte offsets: the byte column for `utf-8`, the line's UTF-16 units for `utf-16` |
| F2 | guards were to be recorded by the parser and evaluated during collection, though the declarations walker already tracks conditionality and the answer depends on configuration. The polyfills' own shape, an early `return` at the top of the file, was not covered, nor `!defined()` | the walker records a symbolic guard, including earlier top-level early returns for nested declarations (top-level ones are hoisted, checked with `php`). `insert_built_decl` evaluates it. `!defined()` is included |
| P6(c) | a `thread_local` table, with a global epoch and a size cap to keep it from pinning names | a table per load, given to each rayon job by `map_init` and dropped with the load |
| P9 | `RefKey` kept today's single key space for classes, functions and constants, with constants folded | keys by kind, constants by `ConstName`. A reference of open kind is recorded under each kind it can name |
| P10 | one `readlink` per directory, which fails on every directory that is not a link; the fact table's names kept in `syntax` | `readlink` only where the parent's listing shows a link. The table is `phpstan-rs`'s and is passed to the lexer |
| P7 | `syntax/name_resolution.rs` listed as a caller of the text grammar (its three hits are comments); types written in the Rust source still parsed on every call | callers listed correctly; those texts parsed once into static tables |
| C2 | name-keyed template bindings justified by "name and scope identify a template", which argues the opposite | the reason stated correctly: a template reference is its name (`Repr::Template(Name)`) |
| C6 | single-pass member usage attributed "after the walk", though aliases are per method | attributed at the end of each method's walk |

Re-read against the code and PHPStan and still the most proper form: P2, P3, P5, P6(a), (b), (d), (e), P8, P11, C3, C5, C7, C8, §12.11, F1, F3, F4, F5, the main-thread items.

### 10.7 Fifth audit

The fifth read applied the rule the earlier audits used against C5's interim record (nothing is built that a later item replaces) to every item. It also checked each design against all the code doing the same job, not only the code it names. It found five more. All five are the same shortfall: improvements to separate walks that the one traversal (§12.10) and a full statement result make unnecessary.

| item | what still fell short | corrected design |
|---|---|---|
| C1 | `Completion` carried termination only, while PHPStan's statement result also carries exit points, throw points, impure points and yields. Separate syntactic walks recompute each of those, more narrowly: `collect_exit_point_statements`, `stmt_contains_break`, `stmt_contains_escaping_exit`, `is_switch_break_equivalent`, `collect_throws`, `body_contains_explicit_throw`, `expr_contains_explicit_throw`, `expr_side_effects`, and a third termination function in the uninitialized-property dataflow. Meanwhile the traversal already accumulates throw and return points for each function | `StmtResult`: completion, plus exit and throw points as tails of two per-function vectors (each construct removes what it settles), effects and `has_yield`. The walks are deleted. The completion rules become shared functions that the `InitFlow` builder also uses (§12.1) |
| §12.10 | migrated the 335 `check_*` entry points but said nothing of the checker's 18 `Visitor`s and the per-file passes in `check_program` | each walk is given its fate: part of the statement result; an observer family (keywords and declares, `Promotions`, the PHPStan-API walker, member and variable usage, risky tests, the rebind scan); traversal state (magic constants in closures); or a pass that must stay (includes, assigned variables, Pest). Member usage is attributed by receiver type, as PHPStan's `UnusedPrivateMethodRule` does |
| C6 | moved the PHPStan-API walker into the per-file check and rewrote the private-member walk as a single pass, both to be replaced by §12.10 later | both become observer families directly. C6 keeps the ancestry fact, the one method lookup and named arguments by scan |
| P10 | gated `Promotions`, fused the rebind walk, had the parser record magic-constant spans and loop yield flags: mechanisms for walks and questions that the traversal and the statement result remove. It also counted 8 yield calls where there are 13 | the parser records the function-level generator flag only, and the lexer's facts gate only `GuardArgs`, the one pass that stays |
| §14 | rewriting the uninitialized-property dataflow over `InitFlow` would have carried over its own termination function and its two stated simplifications: a single pass for loops, and `switch` arms as alternatives | `InitFlow` nodes carry completions from C1's shared rules. Loops run to a fixpoint, and `switch` passes state through a fall-through |

The other designs were read again under the same rule, and nothing in them is built to be replaced: P5's dual parser, C1's assertion stage and F2's count are test-build oracles, and §12.10's first step is the migration path itself.

### 10.8 Sixth audit

The sixth read applied the fifth's rule (one traversal, no second walk) to the parse phase, where no earlier audit had looked. It also checked that every hotspot in Part I's tables has a design, and that the designs agree with each other after five rounds of changes. It found five more.

| item | what still fell short | corrected design |
|---|---|---|
| P12 (new) | no design for the walks after parsing. `resolve_names` (376.3 M, ≈146 M of it walk) is a mutable walk of every tree, listed in §1's table (`syntax::name_resolution`) but never proposed. References (≈198 M of walk) and declarations (≈160 M of walk, and eight other callers walking again) are walks of their own, and `FileBodies::of` (≈112 M) another. Five walks rebuild the import tables (`add_uses`, 104.6 M), and §14 would have added four more walks per body | the parser resolves names in source order (PHP's order, checked with `php`), records references and the declaration list as it resolves, and publishes each import state once. One walk builds what needs `phpstan-rs`'s knowledge (§11.11) |
| F2 | the guard recording moved in the fourth audit to the declarations walker, which P12 removes | guards recorded in P12's declaration list, from the parser's statement stack |
| P9, P7, §14 | references from `deps::Collector`'s walk, contexts built per file from scratch, a builder walk per summary | references from the parser; resolution contexts start from P12's `ImportScope`; summaries from P12's one walk |
| C8(b) | a special case in C7 keeping literal types across generations, against re-inference that §12.10 removes. It would have been deleted at §12.10's step 4, like C6's interim walker | not built; after step 4 a literal is typed once per node per pass, through C8's borrowed interning |
| C7 / §12.10 | record and replay kept a per-pass type table beside C7's entry lists: two tables of node types | one structure: at §12.10's step 4, C7's lists become `NodeTypes`, one entry per node per pass, which record and replay keeps |

**Considered and not designed: finishing only what the analysed files reach.** On a first run the finishing passes compute facts for every loaded class, vendor ones included. Restricting them to the forward closure of the analysed files' references would not be exact. A property's write facts are the union of every body that writes it, and a global's type the union of every file that writes it. The code that reads them does not name those writers, so a forward closure misses them, and the run would lose writes without saying so. That is the failure the lazy-vendor revert (`250bcbee`) removed. Doing it exactly would need facts computed on demand over reverse dependencies, which is a redesign of those analyses rather than a scope.

Part I's other hotspots were checked against the designs: `check_args` (its re-inference, §12.10; its `by_name` map, C6), narrowing and merges (C4), and declaration building (P7, P11, C3, F3). Each has its design.

### 10.9 Seventh audit

The seventh read looked for mechanisms that duplicate information a design already has. It checked the newest design (P12) line by line against the code it replaces, and the serial parts of the finishing passes against the designs. It found five more, one of them my own arithmetic.

| item | what still fell short | corrected design |
|---|---|---|
| C4 | a write-history list on every scope, maintained on every write, to find what two forks changed. The persistent maps already know that: `imbl::OrdMap::diff` skips the nodes two forks share (`advance_skipping_shared`), while `imbl::HashMap` has no such diff. The history was a second record kept to make up for choosing the hash map | id-keyed `OrdMap`s, whose `diff` gives exactly the changed keys at the cost of the changed paths. The history, its lengths and its fallback go |
| C1 | `goto` scopes in a `pending_gotos` table beside the exit vector, which holds the gotos already. Nothing said that a discarded loop pass drops the points it pushed | a `goto` is an exit point carrying its scope, and a label settles the ones aimed at it from the function's vector. A discarded pass truncates both vectors |
| F3 | `changed_since` compares every snapshot entry with its resolved form field by field: 68.7 M, serial, no design | `Arc::ptr_eq` first, fields only for entries a pass copied, as a parallel filter |
| P9 | said the doc names come from the grammar, but not that they are resolved. The memoized `referenced_names` are names as written, so the keys would have been wrong and an edit would not have marked the dependent files stale | each doc name is resolved through the resolution context where its docblock sits, which drops templates and aliases, as the collector does today |
| P12 | listed resolved names as the only references. The collector also records `use` import paths, `global` variables, both fallback candidates and unresolved written names, and counts signatures as outside a body. And the cost figures were wrong: the 0.47 G for the declaration walk and the 0.9-1.0 G total came from inclusive costs that recursion counts twice | all those sources recorded by the parser, and resolved names interned through the lexer's tables. Recomputed from self costs: walks ≈146 + 160 + 198 + 112 M plus ≈75 M of import tables, less the one walk, ≈0.55-0.6 G |

Considered and kept: P4's character-column surplus table now serves only the CLI's character columns, since the LSP's `utf-8` columns are bytes. Diagnostics keep line and column, which costs 32,581 resolutions in this run.

## 11. Parse phase

### 11.1 P1. Delete `Annotations`; the parser records `goto` targets

**Now.** `parse` calls `annotate(&stmts)` on every file (`parse.rs:123`) and stores the result in `ParsedFile::annotations` (`parse.rs:76`), which nothing reads. Every fact `Annotations` computes, the checker already keeps its own way:
- whether an expression is a statement of its own: `in_bare_statement_position` (`stmt.rs:434`);
- the enclosing `catch` types: `current_catch_types` (`stmt/try_catch.rs:77-88`);
- which labels a `goto` jumps to: `collect_goto_targets` (`control_flow.rs:929`), a statement walk per function-like body at five call sites (`function_decl.rs:930`, `expr.rs:3742`, `calls/closure_context.rs:301`, `oop/property_hooks.rs:248`, `:543`);
- the leading `declare(strict_types=1)` (`keywords.rs:221`);
- whether an expression is a condition, which `check_if` and the loop checks know when they check one.

So `Annotations` is a second, unused implementation of facts the checker tracks during its traversal at no extra cost.

**Design.**
- Delete `syntax::annotations`: the module, `Annotations`, `annotate`, `VersionCheck` and its tests, together with the `ParsedFile` field. A rule that wants a node's context gets it from the traversal (C10's `At`), not from a side table built by a second walk.
- The one fact the checker finds by a walk of its own comes from the parser. Its function-scope stack, shared with P10's generator flags, collects the labels of the `goto` statements of each function-like body, and of the file's top level. Each label is marked when a `goto` after it in source order targets it: a backward jump, whose region C1 runs to a fixpoint. They are stored as `goto_labels: Box<[GotoLabel { name: Name, backward: bool }]>` (empty for nearly every function) on `FunctionDecl`, `ClosureExpr`, property hooks and `ParsedFile`. The five calls read the field, and `collect_goto_targets` is deleted.

**Steps.**
1. Delete the module and the call.
2. Have the parser record `goto` labels. A test checks the names equal `collect_goto_targets` for every function-like node of the fixtures, vendor tree and corpus, and the backward marks a source-order walk's. Then delete the walk.

**Oracle.** That test, O1 and O2.

### 11.2 P2. Ignore marks recorded by the lexer, for every file

**Now.** The lexer pushes the byte range of every non-doc comment (`lexer.rs:771-788`). `Lexer::lex` turns each into a `(Position, String)` (`lexer.rs:438-443`), which resolves a position for each comment after the token loop has moved the cursor to the end of the file. `parse` copies every doc comment's text again (`parse.rs:95-99`), builds `code_line_starts` by walking every token (`parse.rs:100-111`) and sorts the comments (`parse.rs:112`). `ignore_comments::scan_parsed` then merges the two lists into one event list, sorts it (`ignore_comments.rs:55-68`) and runs a state machine over it. The machine looks only at comments containing `@phpstan-ignore`: any other comment hits `continue` (`:102`) before it touches any state. For each marked comment it needs:
- whether code precedes it on its first line (`previous_code_line == Some(line)`, `:141`);
- the next code event after it, i.e. the next line's first token, which either settles a pending comment (`:76-94`) or leaves it to the end of the file (`:153-158`);
- whether such an event comes before the next marked comment (a later marked comment replaces the pending one, `:151`).

**Design.**
```rust
// syntax::lexer
pub struct IgnoreMarkRange { pub start: u32, pub end: u32, pub is_doc: bool }
// Lexer: `marked: Vec<IgnoreMarkRange>` replaces `comments: Vec<(usize, usize)>`

// syntax::parse
pub struct IgnoreMark {
    pub text: Box<str>,                 // the comment, copied: marks are rare
    pub line: u32,                      // its first line
    pub is_doc: bool,
    pub code_before_on_line: bool,      // a token before it starts on its first line
    pub next_code: Option<(u32, u32)>,  // (line, byte offset) of the next line-starting token
}
pub struct ParsedFile { pub stmts, pub ignore_marks: Vec<IgnoreMark>, pub warnings, pub facts, pub line_index }
```
- The lexer runs `memmem` for `@phpstan-ignore` over each comment's bytes. It already finds each comment's end with `memchr2`/`memmem`, so this is a second pass over bytes that are in cache. Only matching comments are recorded, doc comments included (`lex_doc_comment`).
- The lexer resolves each mark itself as it goes, so nothing depends on a token vector (P6(d) removes it). It keeps the start of the last code token it emitted (P). At a marked comment, `code_before_on_line` is whether P starts on the comment's first line: one line lookup, for the mark only. The mark then waits for the first code token that starts at or after the beginning of the line after P's, an offset read once from the line table. That token is exactly the next code-line start as `code_line_starts` defines it. While a mark waits, each token costs one offset comparison; with no P (a comment before any code), the first code token settles it. A file without marks does none of this.
- `scan_parsed` walks the marks in order. Before processing mark *k*, a pending mark *k-1* whose `next_code` lies before mark *k*'s start is settled with that line, exactly as the `Event::Code` arm does today; at the end a pending mark is settled by its own `next_code`, or by the end-of-file rule when there is none.
- `ParsedFile::comments`, `code_line_starts` and `Comment` are deleted. `demo.rs:63` prints the number of marks instead.

**Saving.** Every file stops paying for comment texts, line starts, the comment sort and the event sort: ≈0.25-0.3 G, analysed files included. What remains is the `memmem` over comment bytes, about 20 MB in the run.

**Steps.** (1) Add marks beside the old lists, plus a test-only comparison of `IgnoreComments` from both scans; (2) switch `scan_parsed` to the marks and delete the old lists.

**Oracle.** `IgnoreComments` equality over every file of the fixtures, vendor tree and corpus; the 20 existing scan tests (`ignore_comments.rs:389-589`); O2.

**Edge cases.** A comment after the last token (no T): `next_code` is `None`, as today. A marked comment that shares a line with the code on both sides (`a(); /* @phpstan-ignore x */ b();`): P is on its line, so `code_before_on_line` holds, and the next code event is the next line's first token, as today. Comments inside `?>`/`<?php` boundaries are not lexed as comments, as today.

### 11.3 P3. UTF-8 validated once

**Now.** `Lexer::from_bytes` calls `bytes.is_ascii()` twice and `str::from_utf8` once (`lexer.rs:217-218`). Every identifier and variable is turned into text through `lossy` (`lexer.rs:640`, `:588` via `lex_ident_raw`, `:636` for keyword spellings), and doc comments too (`lexer.rs:947`). `read_and_parse_file` validates the whole file again to learn its length (`session.rs:592-606`).

**Design.**
- **Validation at the read, once.** `read_and_parse_file` converts the file's bytes with `String::from_utf8(bytes)`. That either gives a `String`, without a copy, or gives the bytes back (`FromUtf8Error::into_bytes`).
- **Two lexer constructors.**
  - `Lexer::from_str(&str)` validates nothing. It also serves `Lexer::new(&str)` (the LSP, tests, interpolation fragments), which today re-validates text that is already a `str`, through `from_bytes`.
  - `Lexer::from_invalid_bytes(&[u8])`, for the rare file that is not UTF-8, keeps today's lossy slicing.
- **No second validation.** Identifier, variable, keyword-spelling and doc-comment slices are `&text[range]`: a boundary check, no validation, no `unsafe`. It never fails in a valid file, because `IDENT_START`/`IDENT_CONTINUE` include every byte ≥ 0x80 (`lexer.rs:1178-1192`), so an identifier run starts and ends on character boundaries; a debug assertion holds the invariant. The `String` from the read is the one `retain_source` keeps, and the length the size sort needs is its `len()`. `is_ascii` survives only until P4 removes the column code.

**Steps.** One commit. **Oracle.** O1 over the corpus, plus the existing `lossy` property test (`lexer.rs:1258`) extended with a whole-file test on invalid samples.

### 11.4 P4. Byte-offset spans and a per-file `LineIndex`

**Now.** `Span { start: Position, end: Position }` (16 B, `token.rs:30`) sits in every token, node, diagnostic, reflection provenance, `FileBodies` key and stub-cache record. The lexer computes two positions per token (`lexer.rs:474-477` at HEAD) with a cursor, a line table, a per-line marks list for out-of-order positions (`line_col_from_marks`, `:386`) and character counting (`columns`, `:372`). The `Span` field is read in 97 places across 32 non-test files (`.start.line`/`.start.col`/`.end.*`); there are 50 `Position { .. }` and 94 `Span { .. }` constructions outside tests, 39 of them in the parser and lexer. `diagnostics::fix` converts line/column back to byte offsets to apply a fix (`fix.rs:8-33`).

**Design.**
```rust
// syntax::span (new module; every import of the old `token` paths is updated, no re-exports)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug, Serialize, Deserialize)]
pub struct Span { pub start: u32, pub end: u32 }         // byte offsets in the file
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug, Serialize, Deserialize, PartialOrd, Ord)]
pub struct LineCol { pub line: u32, pub col: u32 }       // today's `Position`, 1-based, col in characters
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug, Serialize, Deserialize)]
pub struct Range { pub start: LineCol, pub end: LineCol } // today's `Span`

pub struct LineIndex {
    line_starts: Box<[u32]>,
    // For a file with non-ASCII bytes: after each multi-byte character
    // (or lossy replacement run of an invalid file), its offset and the
    // running total of bytes beyond one per character up to there.
    surplus: Box<[(u32, u32)]>,
}
impl LineIndex {
    pub fn new(src: &[u8], utf8: bool) -> LineIndex;  // memchr lines; surplus only if !is_ascii
    pub fn line_col(&self, offset: u32) -> LineCol;  // line by binary search; col = 1 + bytes - surplus delta
    pub fn range(&self, span: Span) -> Range;
    pub fn offset(&self, at: LineCol) -> Option<u32>; // LSP requests
    pub fn line(&self, offset: u32) -> u32;          // the common "which line" question, no column
}
```
- **Who owns the index.** The lexer builds it in its constructor (the `compute_line_starts` pass it already makes) and hands it out with the tokens. `ParsedFile::line_index: Arc<LineIndex>` is stored in `FileState`, for context files too: about 4 B per line, ≈8 MB for the 2 M lines of the run. Diagnostics about other files and go-to-definition into context files need it without the source. Built-in stubs get theirs on demand from the embedded stub sources (`stub-sources`), cached in a `OnceLock` per stub file.
- **Tokens and AST.** `Token` spans and every AST `span` are `Span`. The parser's `current_pos()` returns `u32`; `stmt()`/`expr()` build `Span { start, end: previous_end }`; the hand-built spans of the parser, interpolation and string lexer (94 constructions) become offset arithmetic, which they already compute: `strings.rs:374`'s `off` closure is already a byte offset.
- **Fragments.** `Lexer::new_in_php_at(src, base: Position)` becomes `Lexer::fragment(src, base: u32, map: Option<&[u32]>)`, with the heredoc indentation map `strings.rs` already builds (`offsets`) mapping a fragment offset back to the file. Spans inside a multi-line `{$...}` of an indented heredoc thereby become exact, where today a column on a fragment's second line ignores the removed indentation.
- **Diagnostics.** `Diagnostic` and `DiagnosticBuilder` keep a `Range`, since output, sorting (`diagnostic_order`), ignore lines and baselines are all line and column. The checker resolves at report time through `self.line_index.range(span)`: the run builds 32,581 diagnostics, against millions of token positions today. `DiagnosticFix` carries a byte `Span`, so `fix.rs`'s `byte_offset` and `position_at` go away and fixes edit the file's bytes, which is also right for a file that is not UTF-8.
- **Reflections.** `Provenance` keeps `(Origin, Span)`; its readers (`class_declaration`, `method_declaration` and the rest, `provenance.rs:236-277`) resolve through the project's index of the origin file. `ClassReflection::redeclared_methods: FxHashMap<(u32, u32), _>` (keyed by line and column, `class_info.rs:373`) is keyed by start offset. `FileBodies` (`bodies.rs:32`) and the LSP's provenance sets keep `Span` with an 8-byte key. The stub cache stores offsets: one format bump.
- **Lexer.** `cursor`, `line_marks`, `base`, `columns`, `char_count`, `line_col_at`, `relative_line_col_at` and `line_col_from_marks` are deleted; a token's span is `(last_token_start, pos)`. `LexWarning` and `ParseError` carry `Span`; `Session::load` prints lexer warnings through the file's index (`session.rs:679-688`).
- **LSP.** Positions are converted from byte offsets, in the encoding negotiated at `initialize`.
  - With `utf-8`, the LSP character is the byte column, `offset − line_start`: exact, and without the surplus table.
  - Otherwise (`utf-16`, the spec's default) it is the number of UTF-16 code units in the line's text up to the offset.
  - Today the server answers `utf-8` when the client offers it but sends character counts (`lsp.rs:2358-2373`; `protocol.rs:277-293`, `:349-354`). The spec defines `utf-8` columns as bytes, so every column after a non-ASCII character on its line is wrong. With `utf-16` it accepts characters as an approximation (`lsp.rs:16-20`).
  - Requests go back through the same conversion, and nodes are found by offset containment. `LineIndex` gains `line_start(line)`.
- **PHPDoc positions.** The phpdoc grammar uses `syntax::token::{Position, Span}` for positions within a doc text, which its lexer tracks per character (`grammar/lexer.rs:73-90`). Its spans become byte offsets relative to the doc text, the doc's own file offset placing them in the file when needed. No checker rule reads them today: invalid tags are reported at the declaration's span (`checker/oop.rs:758`). The lexer change is in §11.8.
- **Columns of invalid UTF-8.** Today's column is a lossy character count of the bytes from the nearest resolved position, so the sum over two slices can differ from one count over the line when a slice boundary splits an invalid sequence. The index defines the column as the lossy count from the line start. Where this differs, the new value is the consistent one; O1 lists the differences.

**Steps.**
1. `LineIndex` in `syntax`, built by the lexer, carried to `FileState`. Test: `line_col(offset) == Lexer::line_col_at(offset)` for every token boundary of every file of the fixtures, vendor tree and corpus.
2. Pure rename, `Position` → `LineCol` and `Span` → `Range`, across the workspace.
3. `Span` (bytes) in tokens, AST, warnings and parse errors; consumers resolve through the index (the checker holds `&LineIndex`, the project an index per file). Stub cache bump. This is the large commit, driven by the compiler.
4. Delete the lexer's position machinery.
5. `DiagnosticFix` by byte span, `fix.rs` simplified.
6. LSP conversions in the negotiated encoding, with tests over lines holding multi-byte and astral-plane characters.

**Oracle.** O1 with spans printed through the index equals the dump from before step 3; O2; the LSP tests.

**Saving.** The ≈700 M of positions in the run, minus building the surplus table for the non-ASCII files (a byte scan of those files only), plus the moves saved by a `Span` 8 bytes smaller in every `Expr`, `Stmt`, token and `Result<Expr, ParseError>` the parser passes up.

### 11.5 P6. Lexer per-token work

**(a) Keywords by perfect hash.** `Keyword::try_from_bytes` (`token.rs:121`) lowercases into a buffer and matches byte strings. Design: a `const` table of 128 slots indexed by `(len * 31 + (first | 0x20) * 7 + (last | 0x20)) & 127`, whose collisions a test proves absent for the 62 keywords (if two collide, the hash takes a third byte). The probe compares the candidate's length and its bytes `eq_ignore_ascii_case` against the slot's spelling, without a copy. The keyword's raw spelling is recorded only when it differs from the canonical one, as today (`lexer.rs:625-637`). **Oracle:** every keyword in all case mixes, plus every distinct identifier of the corpus, against the old function.

**(b) Single-quoted strings in one allocation.** `lex_single_quoted` (`strings.rs:150-178`) grows a `Vec` from empty and then copies it into the `Arc<[u8]>` of `PhpString`. Design: `memchr2(b'\'', b'\\', rest)` loops to the closing quote; with no `\\` before it (most literals) the token is `PhpString(Arc::from(&src[start..end]))`, a single allocation and copy. With an escape, the length is known after the scan, so `Vec::with_capacity(end - start)` is filled and converted, with no growth. **Oracle:** O1 (payload bytes).

**(c) Names across files.** `Lexer::name` (`lexer.rs:252-264`) interns on a miss in the per-lexer table; 511 K misses reach the interner's shard lock. Design: identifiers recur across a project's files by nature (class, method, property and variable names), so a second-level table is the right shape whatever the share; O3 measures it, it does not decide it.
  - The table lives as long as one parallel load. `Session::load` parses with `par_iter().map(..)` (`session.rs:657-664`), which becomes `map_init(ThreadNames::default, ..)`, so each rayon job gets a table that is dropped when the load ends.
  - It holds strong `Name`s keyed by the text hash the lexer already computes, and the lexer probes it between its own table and the interner.
  - No `thread_local`, epoch or cap is needed to keep a long LSP session from pinning names: the table goes with the load, and the names it holds are held by the load's trees in the meantime.
  - Single-file parses (an LSP edit, an incremental change) pass no table.

  **Oracle:** O1; names are interned identically, which the interner's identity test covers.

**(d) A streaming lexer: no token vector.**
- **Now.** `run_lexed` lexes the whole file into three vectors (`tokens`, `spans`, `keyword_raw`) plus a doc list (`lexer.rs:456-500`). `Parser::from_lexed` (`parser.rs:126-177`) then builds four `FxHashMap`s keyed by token index (`doc_comments`, `doc_comment_lines`, `doc_comments_all`, `doc_comment_starts`). The parser moves tokens out with `take_token`, and the vectors are dropped at the end. That costs ≈200 M of pushes and moves in `run_lexed`, 59.6 M in `from_lexed` and 73.5 M of drops, with 48 B per token over 6.9 M tokens.
- **The vector is not needed.** The parser never rewinds: the only assignment to `pos` jumps to the end for `__halt_compiler` (`parser/stmt.rs:395`). Its lookahead reaches a few tokens (`peek_at(1..3)`, plus the modifier scans of `looks_like_class_decl` and `at_anonymous_class_start`, which stop at the first non-modifier). Its look-back is the previous token's end (`previous_end`) and, for a member's removal span, the member's first token and the one before it (`member_removal_span`, `parser.rs:205-235`).
- **Design.**
  ```rust
  pub struct Tok {
      pub kind: TokenKind,                  // field-less kinds plus Keyword(Keyword)
      pub span: Span,                       // P4's byte offsets
      pub payload: Payload,                 // None | Name | Int | Float | Str(PhpString) | Parts(Vec<StringPart>) | Html(Arc<str>)
      pub docs: Option<Box<DocGroup>>,      // the doc comments right before this token
      pub raw_keyword: Option<Box<str>>,    // a keyword's spelling when not lowercase
  }
  pub struct DocGroup { pub last: Arc<str>, pub first_start: u32, pub folded: Option<Arc<str>> }
  impl Lexer<'_> { pub fn next(&mut self) -> Tok }
  // Parser: `ahead: VecDeque<Tok>`, filled on demand by `peek_at(n)`; `last_end: u32`
  ```
  - The lexer attaches each group of doc comments to the token that follows it, folding several into one as `from_lexed` does today. The four maps and `keyword_raw`'s index table disappear: `take_doc_comment` takes the current token's group.
  - `previous_end` is a field updated on each consumed token. `MemberStart` records the member's first start (its doc group's, if any) and the end of the token before it, so `member_removal_span` needs no index.
  - Interpolation fragments are parsed from a fragment lexer, the same way. `Parser::new(Vec<(Token, Span, Option<String>)>)` and `tokenize_with_spans`, which tests use, become `Parser::new(Lexer::from_str(src))`.
  - The ignore marks (P2) and `FileFacts` (P10) are lexer state already. The four places that want a token list collect one from the lexer, which implements `Iterator`: `stub_attributes.rs:124`, `output/codeframe.rs:50` (syntax-highlighted code frames, with comments), `lsp/completion.rs:88` and `:173`.
- **Oracle.** O1, which dumps the token stream and the AST, unchanged.

**(e) Byte-class dispatch.** `next_token` (`lexer.rs:544-680`) is a chain of `if c == ...` tests. Design: a `static CLASS: [ByteClass; 256]` and one `match CLASS[c]` with arms `Space`, `IdentStart`, `Digit`, `Dollar`, `Quote1`, `Quote2`, `Backtick`, `Lt`, `Slash`, `Hash`, `Question`, `Op`. The rare forms (`?>`, `#[`, `b'`, heredoc, doc comment, inline HTML) move into `#[cold]` functions so the hot frame stays small. Whitespace and identifier runs are scanned eight bytes at a time against the class table: a `u64` load, then the table on each byte until one leaves the class. **Oracle:** O1.

### 11.6 P5. Expressions by binding power, by PHP's own precedence table

**Now.** `parser/expr.rs:8-462` is a ladder: `parse_expression` → low `or`/`xor`/`and` → `parse_assignment` (guard) → `parse_ternary` → `parse_coalesce` (guard) → `parse_binary(1..12)` → `parse_not` → `parse_instanceof` → `parse_unary` (guard) → `parse_pow` → `parse_postfix_from_primary`. `parse_assignment_tail` is applied to every operand (`:62`, `:446-447`). Entry points used elsewhere:
- `parse_assignment`: `print` (`primary.rs:373`), `throw` (`:378`), `yield from` (`:386`), `include`/`require` (`:469`), arrow-function bodies (`closures_and_match.rs:82`);
- `parse_unary`: `clone` (`primary.rs:368`);
- `parse_ternary`: `yield` (`primary.rs:445`, `:447`).

Node ids are allocated when a node is built (`parser.rs:294`), so children get ids before their parent.

Checked against PHP 8.5.4 (`php -l`) and against nikic/php-parser 5, the parser PHPStan uses (from the Piwigo tree's `vendor/`, AST dumped by a small script), the ladder departs from PHP's grammar in six places:

| input | PHP | today |
|---|---|---|
| `fn() => $a or $b` | `fn() => ($a or $b)`: an arrow function's body extends over `and`/`or`/`xor` | `(fn() => $a) or $b` |
| `include $f or exit` (and `require`, `_once`) | `include ($f or exit)` | `(include $f) or exit` |
| `throw $a or $b` | `throw ($a or $b)` | `(throw $a) or $b` |
| `$a instanceof B instanceof C` | valid: `($a instanceof B) instanceof C` | a parse error: `parse_instanceof` takes one, then the statement fails |
| `1 < 2 < 3`, `$a == $b == $c`, `$a <=> $b <=> $c` | a parse error: the equality and relational levels are non-associative (mixing levels is fine: `$a === $b < $c`) | accepted, left-associative |
| `$a ? $b : $c ? $d : $e`, `$a ?: $b ? $c : $d`, `$a ? $b : $c ?: $d` | a compile error ("Unparenthesized `a ? b : c ? d : e` is not supported"); an all-short chain is valid and left-associative: `$a ?: $b ?: $c` is `($a ?: $b) ?: $c` | all accepted, right-associative |

The first three change what code means. In `$f = fn($x) => $x->a() or $x->b();` PHP's closure returns the `or`; today's tree assigns a closure returning `$x->a()` and ors the assignment. `require $path or die()` is a real-world bug pattern: PHP requires the file named `"1"`. The checker should see what PHP runs.

**Design.** One Pratt loop over PHP's table (`Zend/zend_language_parser.y`), loosest first:

| level | operators | associativity |
|---|---|---|
| prefix, operand to the end of the expression | `throw`, arrow-function body, `include`/`require`(`_once`) | — |
| low logical | `or`, `xor`, `and` | left |
| prefix, operand above the low logical ones | `print`, `yield`, `yield from` (as today) | — |
| assignment | `=` and compound forms, in operand position (`parse_assignment_tail`, as today) | right |
| ternary | `? :`, `?:`, with PHP's nesting rule | left |
| coalesce | `??` | right |
| binary | `\|\|`, `&&`, `\|`, `^`, `&` | left |
| equality | `==`, `!=`, `===`, `!==`, `<>`, `<=>` | none |
| relational | `<`, `<=`, `>`, `>=` | none |
| pipe | `\|>` | left |
| concatenation | `.` | left |
| shift | `<<`, `>>` | left |
| additive | `+`, `-` | left |
| multiplicative | `*`, `/`, `%` | left |
| logical not | `!` (operand at `instanceof` level) | prefix |
| `instanceof` | `instanceof` | left |
| unary | `+`, `-`, `~`, casts, `@`, `++`, `--` | prefix |
| power | `**` (right side through unary) | right |
| — | `clone`, `new`, primaries and their postfix chains | — |

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Bp(u8);
#[derive(Clone, Copy)] enum Assoc { Left, Right, None }
fn parse_expr_bp(&mut self, min: Bp) -> Result<Expr, ParseError>  // the one stack guard
fn parse_prefix(&mut self, start: u32) -> Result<Expr, ParseError> // prefix operators, then the operand
fn parse_operand(&mut self, start: u32) -> Result<Expr, ParseError> // postfix-from-primary, assignment tail, `**`
fn infix(&self) -> Option<(Bp, Assoc, Infix)>                       // one match on the next token
```
- **Non-associative levels.** After an operator of level L, an operator of the same level in the loop is `syntax error, unexpected token "<op>"`, as PHP words it. Operators of the other comparison level mix freely.
- **Ternary.** Left-associative at its level. The middle operand is a whole expression (`$a ? $b ? $c : $d : $e` is valid, as in PHP); the else operand is parsed above the ternary level. When a ternary's condition is itself a ternary not wrapped in `Grouping`, it is the error PHP raises, unless both are short. The parser already raises PHP's compile-time errors as `ParseError`s ("Cannot use empty list", "Cannot mix keyed and unkeyed array entries", `parser/expr.rs:192-236`), and PHP refuses to compile such a file.
- **`instanceof`.** Left-associative; its right side is read as `parse_instanceof` reads it today.
- **`throw`, `include`/`require`, arrow-function bodies** parse their operand with `min = LOWEST`. `print` and `yield` keep today's operand level, which matches PHP.
- **`yield` without an operand.** Today a hand-kept list of 30 tokens decides that `yield` has no operand (`primary.rs:391-443`). The list misses `&`, `and`, `or`, `xor`, `instanceof` and `|>`, after each of which PHP 8.5 accepts a bare `yield` (`php -l`), so `$r = yield and $y;` fails to parse today. PHP's rule is that `yield` stands alone when the next token cannot begin an expression. The list becomes `!self.can_begin_expression()`, one predicate over token kinds, which the generated test covers.
- **Assignment** stays the operand-position rule: PHP's grammar has `variable '=' expr` wherever an operand may stand. php-parser gives `$x = $a ?? $b = 1` as `$x = ($a ?? ($b = 1))`, `(int) $v = 5` as `(int) ($v = 5)` and `!$a = $b` as `!($a = $b)`, as today.
- **Spans and ids.** Every node of a chain uses the start captured before its first operand, and is built after its operands, so post-order ids come out as today wherever the tree is the same.
- **Entry points.** `parse_expression` = `parse_expr_bp(LOWEST)`; `parse_assignment` and `parse_ternary` = `parse_expr_bp(TERNARY)`; `parse_unary` = `parse_prefix` under a guard.

**A file that fails to parse is reported.** Today an analysed file with a parse error is skipped, and the summary line says only "N file(s) skipped: parse error" (`main/check_project.rs:291-293`), without the file or the error. Phase 188 (no silent drops) does not cover it. Since this item makes PHP-invalid files fail where they passed before, it includes the reporting: an analysed file's `ParseError` becomes a diagnostic at its position, with its message and an identifier of its own, non-ignorable. A context file's parse error is listed by path in the verbose summary.

**Steps.**
1. Report the parse errors of analysed files (O2 changes only for files that already fail).
2. Add the Pratt functions beside the ladder, with a `#[cfg(test)]` switch that routes the entry points to the old functions.
3. Switch the default, run the oracles, and delete the ladder.

**Oracle.**
- **Grammar.** A generated test covers every ordered pair of the table's operators and prefix forms, nested two deep, with and without parentheses. It writes each expression to a file and compares acceptance with `php -l` and the tree's grouping with php-parser's AST, through a dump in a common notation (the script used above). Each expression must match PHP.
- **Corpus.** Parse every file of the fixtures, vendor tree and corpus both ways and compare the `{:?}` dumps, spans and ids included. Every difference must be one of the six rows above or the `yield` rule, and each is listed with its file. O2 is then read for the semantic consequences of rows 1-3.

### 11.7 P7. PHPDoc types: one grammar, lowered once, resolved once per name context

**Now.** `registry::parse_type_node` is `types::parse_written_type(&node.to_source_string())` (`type_node.rs:8-10`), and `mentions_mixed` likewise prints the node first (`:13-15`). The `types` crate keeps a text grammar of its own (`parse.rs`, 1,458 lines; `parse_display.rs`, 790). Besides this path, it is used directly by `registry/stubs.rs` (16 calls), `checker/predefined_constants.rs` (10), `checker/expr/mysqli.rs` (3) and others, and by its own tests (74 + 63 + 17 + ...). (`syntax/name_resolution.rs`'s three mentions are comments: `syntax` cannot call `types`.) `phpdoc` depends only on `syntax`.

Two costs repeat per use:
- Names are resolved through `map_class_names` on every use: 359.2 M inclusive in the run, mostly while building declarations.
- A statement's `@var` tags are re-read from the doc text at four sites (`stmt.rs:155`, `:371`, `phpdoc_type_compat.rs:597`, `var_tag_types.rs:287`) and resolved again on every visit of the statement (`statement_var_tags`, `var_tag_types.rs:286`), loop passes included.

**Design.**
- **One grammar.** `phpdoc` takes a dependency on `types`, which creates no cycle. `phpdoc::lower::lower(node: &TypeNode) -> Type` is a structural lowering over entry points that `types` exposes from code its text parser already has, taking lowered children instead of argument strings: `types::written::{atom, generic, int_range, array_of, offset_access, shape, callable, conditional, const_fetch, literal_*}`. `parse_type_keyword_or_class` (`parse.rs:1038`) and `parse_type_atom_by_form` (`:681`) are split so the keyword table is one function. Every other caller of the text grammar moves to `phpdoc::parse_type(text) = lower(grammar::parse_type(text))`. Those that parse a type written in the Rust source on every call (`predefined_constant_type`, for each constant fetch and PHP version; `mysqli.rs`) parse each text once into a `LazyLock` table; `stubs.rs` parses its texts once per stub build already. The comments that name the text grammar are updated. The text grammar is then deleted; `native_type`, `keyword_type`, `is_reserved_type_word` and `canonical_int_key`, which are not PHPDoc parsing, stay. Its tests move to `phpdoc`, since `types` cannot dev-depend on `phpdoc` without two copies of `types` in the test build.
- **Lowered eagerly, once.** `DocBlock`'s type fields hold `DocType { node: TypeNode, ty: Type }`, filled by `docblock::parse`. That function is memoized per doc text, so each distinct docblock's types are lowered once per process. It is a plain value: no cell, and `Eq`/`Hash` cover both fields. Declaration building reads nearly every type of every docblock, so eagerness costs little over laziness and keeps `DocBlock` free of interior mutability.
- **Resolved once per resolution context.** Resolving a written type reads more than the namespace and the `use` imports (`NameContext`): it also marks template references and type aliases (`type_node.rs`'s "names still unresolved, template and alias references not yet marked"). So its context is the namespace, the imports, the template names in scope (class and method), the type aliases in scope (`@phpstan-type`, `@phpstan-import-type`) and the class `self` refers to. The first two are P12's `ImportScope`, which the parser publishes. Each file interns its distinct resolution contexts (`Arc<ResolutionContext>`), each an `ImportScope` plus the rest, and `resolve(ty, ctx)` is memoized in a per-file table keyed by `(type node address, context address)`. Types are interned, so `Foo`, `?Foo` and `array<int, Foo>` are shared nodes across docblocks, and a file resolves each distinct type once per context instead of once per tag. A type without `HAS_OBJECT` (P8) is returned as it is, without a probe.
- **Statement `@var` tags once per file check.** The four `var_tag_nodes` sites and `statement_var_tags` read one per-file memo, keyed by the docblock's `Arc` address and the resolution context in force (the same interned context, method templates included), filled on first use. Loop passes and the four sites share one resolution.
- `mentions_mixed` walks the node. The class template parameters the statement path needs (`current_class_template_params`) are built once per class check and kept on the checker.

**Steps.**
1. Add the `types::written` entry points and rewire the text parser onto them (a refactor; O2 unchanged).
2. Add `phpdoc::lower`, with the oracle test.
3. Add `DocType` and switch the registry and checker readers.
4. Add the per-file resolution memo and the statement-tag memo.
5. Move the other text-grammar callers.
6. Delete the text grammar and move its tests.

**Oracle.**
- For every type node of every docblock of the fixtures, vendor tree and corpus, `lower(node) == parse_written_type(node.to_source_string())`. `Type` equality is node identity, since types are interned. The texts the two grammars read differently are listed before step 6 removes one.
- The resolution memo's answers equal today's resolution on every probe in a test build, which also catches any input missing from the context key.
- O2.

### 11.8 P11. Tags parsed once per distinct tag

**Now.** `apply_tag_recording_errors` (`docblock.rs:449-488`) builds a scratch `DocBlock` per tag, applies the tag to it (`apply_tag`, `:778`, about 40 arms), then merges it into the block (`merge_from`, `:499`, which destructures every field). Docblocks are memoized by whole text, and the same tag lines recur across thousands of distinct docblocks.

**Design.**
```rust
pub(crate) struct TagEffect {
    // the fields one tag can set, as the scratch block held them,
    // with `TypeNode`s behind `Arc` so applying is a pointer copy
    pub(crate) fields: DocBlockDelta,
    pub(crate) errors: Vec<ParseError>, pub(crate) invalid: Option<InvalidTagValue>,
}
fn parse_tag(written: &str, tag: &str, rest: &str) -> Arc<TagEffect>   // memoized by (written, rest)
fn apply(block: &mut DocBlock, effect: &TagEffect)                     // `merge_from`'s rules, per field
```
- `DocBlockDelta` is `DocBlock` with each type behind `Arc` (`Arc<DocType>`, so the type P7 lowers is shared too); `DocBlock`'s own fields become `Arc<DocType>`, which `Deref` keeps source-compatible for readers.
- The memo is `syntax::memo::Memo<TagEffect>` keyed by the tag line's text, with the same policy as the docblock memo it feeds. The distinct tag lines are bounded by the tag lines of the distinct docblocks that memo holds.
- The non-PHPStan path (`apply_tag` straight into the block, `:476-479`) goes through the same `parse_tag`, since applying an effect is what applying directly did.
- **The tag grammar's lexer works on bytes.** `phpdoc::grammar::Lexer::new` copies its input into a `Vec<char>` (`grammar/lexer.rs:47`, `:57`) and advances line and column per character (`:80-90`). The parser tokenizes a whole tag value into a `Vec` before parsing it (`grammar/parser.rs:42`, `:1202`, `:1216`, `:1231`). The lexer instead scans the `&str`'s bytes, PHPDoc's delimiters being ASCII and identifier bytes ≥ 0x80 as in PHP, records byte-offset spans (P4), and is pulled by the parser like the PHP lexer (§11.5(d)), with no token vector and no char vector.

**Steps.** (1) `TagEffect` and `apply`, with `merge_from` re-expressed through them (no memo yet); (2) memoize; (3) `Arc` fields.

**Oracle.** `DocBlock` equality, old against new, for every docblock of the fixtures, vendor tree and corpus; the docblock tests.

### 11.9 P9. Reference keys as folded names, doc names from the one grammar (needs C3, P7)

**Now.**
- `deps::Collector::add(String)` (`deps.rs:115`) is fed `dependency_key(name)` (trim and `to_ascii_lowercase`, `:43`), inserting into two `FxHashSet<String>`s.
- `Project::dependents: FxHashMap<String, FxHashSet<FileId>>` (`project.rs:158`); `register_references` and `closure_scope` move `String`s.
- `deps` also has its own reader of PHPDoc types: `doc_tags`, `tag_type_tokens` and `type_head` (`deps.rs:197-349`) split raw doc text into type tokens, cached as `DocNames { tokens: Vec<String> }` (`:65-79`). That is a third grammar beside the phpdoc grammar and the `types` text grammar. The final plan rejected reading the names from the parsed docblock because the raw reader collects more ("over-collects"), but whether that excess is needed was never settled.

**Design.**
```rust
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum RefKey {
    Class(FoldedName),      // class-likes: case-insensitive
    Function(FoldedName),   // functions: case-insensitive
    Constant(ConstName),    // namespace folded, name exact (C3)
    GlobalVar(Name),        // `$name` keys today: `global_variable_key`
}
```
- **Keys by kind.** Today every key is one lowercased string (`dependency_key`, `deps.rs:43`). So a class `Foo` and a function `foo` share a key, and so do constants `FOO` and `foo`, and a change to either makes stale every file that mentions the other.
  - `ChangedNames` already records changes by kind (`classes`, `functions`, `constants`, `registry.rs:106-111`), so keys by kind make the dependents exact.
  - A reference whose kind the code leaves open, such as a callable string (a function, or a class and its method), is recorded under each kind it can name.
- `ParsedFile::{references, declaration_references}` (recorded by the parser as it resolves names, P12, where `deps::Collector` walks today), `dependents` and `signature_dependents` are keyed by `RefKey`, hashed by the precomputed hash of its name (folded for classes and functions, C3). A resolved name is a `Name` the resolver interned, so no `String` is built per reference. `closure_scope`'s frontier and visited sets are `RefKey`s.
- **Doc names from the grammar.** A file depends on a doc name only if some reader of that docblock resolves it.
  - Step 1 audits every consumer of doc text. After P7 there is one grammar, but a few readers may still look at raw text (`contains("var")` prefilters, `@phpstan-ignore` scanning, `doc_names` itself).
  - When every reader that resolves names goes through the grammar, the names a docblock can make a file depend on are exactly the class names in its parsed `TypeNode`s and in the class-bearing tags (`@mixin`, `@method`/`@property` signatures, `@throws`, `@extends`/`@implements`/`@use`, `@see` when it resolves). `docblock::parse` collects them once, memoized with the block (`DocBlock::referenced_names() -> &[Name]`). The raw tokenizer and `DocNames` are deleted.
  - These are names as written, since the docblock memo is shared by every place the same text appears. Each one is resolved through the resolution context where the docblock sits (P7's interned context: namespace, imports, template names, aliases), by P7's memo, and becomes a `RefKey::Class`.
  - A name that is a template or an alias in that context drops out, as the collector's template stack does today (`deps.rs:403-445`), and a relative name resolves against the namespace.
  - Without this step the keys would be the written names, and a file would not be marked stale when the class its docblock actually names changes.
  - A reader found to resolve names from raw text is converted to the grammar first, as part of this item.

**Steps.**
1. The consumer audit, with conversions.
2. `RefKey` everywhere.
3. Doc names from `DocBlock`.

**Oracle.**
- The affected-file sets of the incremental tests, and O2.
- For the names the raw reader collects and the grammar does not, over the fixtures and corpus: a test that changes the declaration of each such name in turn, for a sample of files that mention it, and checks the files' diagnostics do not change. Such names cannot make a file stale.

### 11.10 P10. Smaller parse-phase items

**Generated autoload files: one parser, off the main thread.**
- **Now.** Discovery parses `vendor/composer/autoload_*.php` with the full parser on the main thread (`vendor.rs:158-163`, 171.2 M serial). `Resolver::path` rebuilds each path through `PhpString`'s `Display` (93.6 M). Canonical directories cost 44 K `statx` and 21 K failing `readlink` calls in a 4 s run (final plan, C1 correction).
- **Design.**
  - **No second grammar.** A scanner for Composer's fixed shape with the parser as fallback would be two readers of the same input, one of them rarely exercised. The general parser stays the only reader. After P1 (no `annotate`) and P4-P6, its cost per byte falls with every other file's.
  - **Off the critical path.** The parse runs on a scoped thread that discovery spawns as soon as the generated files' paths are known. It overlaps with collecting the analysed files, which does not depend on discovery, and with loading the stub cache.
  - **Paths from bytes.** `Resolver::path` joins the string's bytes (`OsStr::from_bytes`) instead of formatting it.
  - **Fewer system calls.** Each distinct directory's entries come from one `read_dir`, which gives existence and file type from the dirents.
    - A directory's canonical form is its parent's cached canonical form joined with its name.
    - Only when the parent's listing marks the entry as a symbolic link does one `readlink` resolve it. Today `canonicalize` (`realpath`, `vendor.rs:235-258`) issues a `readlink` for each component, and it fails on every one that is not a link.
    - The shared `__DIR__ . '/..'` prefix is evaluated once.
- **Oracle.** The class, file and directory lists equal today's for the Composer fixtures and every `vendor/composer` directory available (the Piwigo tree, the repository's vendor tree).

**Generators known from the parser.** The parser keeps a stack of open function-like scopes.
- A `yield` or `yield from` sets `is_generator` on the innermost function-like node: `FunctionDecl` (methods included, through `MethodDecl::function`), `ClosureExpr`, `ArrowFnExpr` and property-hook bodies. The checker needs this before it checks the body, since a generator's `return` means something else.
- The 11 function-level calls of `body_contains_yield`/`expr_contains_yield` read the field (`function_decl.rs:919`; `property_hooks.rs:241`, `:245`, `:536`, `:540`; `closure_context.rs:292`; `expr.rs:3726`, `:4012`, `:4115`, `:5838`, `:5904`).
- The two loop-body calls (`stmt/loops.rs:76`, `:304`) serve rules that run after the body has been checked, so they read `has_yield` from the body's statement result (C1). A per-loop flag on the AST is not needed.
- The walks are deleted.
- **Oracle:** each flag equals the walk's answer for every function-like node of the fixtures, vendor tree and corpus.

**`FileFacts` from the lexer; the walks that remain gated by them.**
- `FileFacts::of_tokens` walks every token (`parse.rs:41-60`). Instead, the lexer evaluates a fact table when an identifier spelling is first seen in the file, i.e. on a miss in its per-lexer name table, which is far rarer than tokens. The table maps a lowercase spelling to a fact bit:
  - the three stub attributes, read while declarations are collected (`project.rs:2575`);
  - the `GUARD_FUNCTIONS` names, `set_include_path` and `ini_set`: include guards.

  The table belongs to `phpstan-rs` and is handed to the lexer (`Lexer::watching(&'static WatchList)`), with each bit declared beside the walk it gates. `syntax` returns a plain `mentions: u32` in place of today's `FileFacts` fields that name closure rebinds and stub attributes (`parse.rs:23-60`). The syntax crate then holds no checker knowledge, and a new gate does not touch the lexer.
- **Gate.** `GuardArgs` (`includes.rs:356`) runs only with the include-guard bit. It compares callee names case-insensitively, without the `to_ascii_lowercase` allocation per call (`:381`). It stays a pass, because checking an `include` needs its answer first (§12.10).
- **No gates for walks the traversal removes.** `Promotions`, the closure-rebind scan, the private-member walk and the magic-constant walk become observers or traversal state (§12.10), so they get no gate, no fused walk and no parser-recorded spans. The rebind bit, the `has_closure` fact and the `__function__`/`__method__` bit go with them. The first versions of this item designed all four and would have built them only for §12.10 to delete them.
- **Oracle:** `GuardArgs` gated equals ungated over the fixtures, vendor tree and corpus; O2.

### 11.11 P12. Each file's products from the parse, and one walk for the rest

**Now.** After `parse_program` builds a file's tree, every file gets walked again and again. Not counting P1's `annotate`, the walks are:
- `resolve_names`, a mutable walk over the whole tree, called by `parse_program` itself (`parser.rs:350`): 376.3 M over 14,252 files. ≈146 M of that is the walk; the rest is the resolution, which moves into the parser.
- `deps::both_referenced_names`, the reference walk with its raw doc reader: 841.6 M, of which ≈198 M is the walk and the rest P9's keys and doc reader.
- `for_each_declaration` for declaration collection (`syntax/declarations.rs`). The walk around the builders costs ≈160 M, and eight other callers walk again for their own lists: the registry rebuilds (`registry/index.rs:603`), duplicate class and function sites, `architecture.rs`, `composer_hygiene.rs`, `renamed_named_arguments.rs` and two in `project.rs` (`:2820`, `:2838`).
- `FileBodies::of`, ≈112 M of walk.
- `name_anonymous_classes`, gated, and the anonymous-class name list.

Each walk that needs names in context rebuilds the import tables from the `use` statements: `NameContext::add_uses`, 223 K calls and 104.6 M, from five walks (the resolver, `deps`, declarations, `check_program` and the PHPStan-API walker). §14 would add four more walks per body (purity, property writes, `InitFlow`, `FileSites`).

**Design.** What the parser knows when it builds a node, it records then. What needs `phpstan-rs`'s knowledge is built by one walk.
- **Names are resolved by the parser.** The parser keeps the `NameContext` (the namespace and the three import tables) and updates it at each `namespace` and `use` statement in source order. That is PHP's own order: checked with `php`, an import applies only to code after it, and a conflicting later import is a compile error. It resolves each name as it builds the node:
  - class references: `new`, static calls and fetches, `instanceof`, `catch`, type hints, `extends`/`implements`/trait `use`, attributes;
  - function calls and constants, with the global fallback.

  `resolve_names` is deleted, along with its call from `stub_attributes.rs:173`; the attributes pass resolves the few types it adds as it adds them. Each distinct import state is published once as an interned `ImportScope`. P7's resolution contexts and the checker start from it, so no walk rebuilds import tables.
- **References are recorded as names are resolved.** Each name is pushed into the file's references as P9's `RefKey` of its kind, matching what `deps::Collector` collects today (`deps.rs:372-490`):
  - an exactly resolved name;
  - both candidates of a name with the global fallback;
  - the written form of a name that stays unresolved, unless it is `self`, `parent` or `static`;
  - each `use` import's path, when the `use` statement is parsed;
  - a `GlobalVar` key for each variable of a `global` statement.

  A reference also goes into `declaration_references` when it lies outside every function-like body. Signatures (parameters, return types, attributes) count as outside, as the collector's body depth does today. The `deps::Collector` walk is deleted. Doc names come from docblocks (P9): those of declarations from declaration building, the others (statement docblocks, top-level or in bodies) from the one walk below.
- **Resolved names are interned through the lexer's tables** (the per-file table and P6(c)'s per-load one), so resolving does not take the interner's lock more often than lexing does.
- **Declarations are listed by the parser.** Each class-like, function and constant, and each `define()` call (by its resolved callee), is appended to the file's declaration list. Each entry carries its `ImportScope` and its guard (F2). The guard is the enclosing conditions, which are on the parser's statement stack, and the earlier top-level early returns. `for_each_declaration` iterates that list for all nine callers instead of walking the tree.
  - An entry is owned data, so it outlives the tree: the name, its span, the declaration kind (and class kind), the span, the guard, the `ImportScope`, and the node's index in the tree.
  - Declaration building reaches the node while the tree exists.
  - The callers that read only these (the duplicate class and function sites, `composer_hygiene.rs:765`) read the entry, so they keep working for a context file after §14 drops its tree. `architecture.rs:33` reads attributes from the tree, as an opt-in rule over analysed files, through `ast_of` for a context file.
- **Anonymous classes are numbered by the parser.** Their names are formed from the path and the number, with no walk.
- **One walk for the rest.** What needs `phpstan-rs`'s knowledge is built by one `Visitor`, on the worker, that runs every builder at each node:
  - the §14 body summaries (purity, property writes, `InitFlow`), keyed in `FileBodies` by the body's span;
  - `FileSites`;
  - the names in statement docblocks, top-level or in bodies;
  - the LSP's use sites, when indexed.

  It replaces `FileBodies::of` and the four walks §14 would add.

**What stays exact.** Resolution happens at the same point in source order as in the walk it replaces, so every resolved name is the same. References and declarations are the same sets, built at their nodes.

**Saving.** The four walks (≈146 M, ≈160 M, ≈198 M, ≈112 M) and the repeated import tables (≈75 M of the 104.6 M), less the one walk that replaces `FileBodies::of` (≈110 M): ≈0.55-0.6 G. The resolution itself (≈230 M) moves into the parser.

**Steps.**
1. Resolution in the parser, `ImportScope`s; `resolve_names` deleted.
2. References from the parser; the `deps` walk deleted (after P9's doc names, row 14).
3. The declaration list; the nine `for_each_declaration` walks become iterations.
4. The fused summary walk, with §14.

**Oracle.**
- O1 with resolved names printed: identical to the dump from before.
- For every file of the fixtures, vendor tree and corpus, the reference sets, the declaration lists (with contexts) and the anonymous-class names equal the walks' output.
- O2.

## 12. Check phase

### 12.1 C1. The statement result, built while checking: completion by level, exit and throw points, effects

**Now.**
- `check_block` checks a statement and then asks `always_terminates(stmt, locals)` (`stmt.rs:57-60`). `always_terminates` (`control_flow.rs:93-198`) re-infers expression statements (`:100-103`) against the scope *after* them, and recurses through blocks, `if`, `switch`, `try` and loops. `exits_function_or_loop` (`:219-271`) has the same shape.
- Callers:
  - `check_block` (`stmt.rs:60`);
  - `check_if` (`stmt.rs:1240-1245`), against each branch's final scope;
  - function bodies (`function_decl.rs:1044`, `:1558`) and closures (`expr.rs:3844`);
  - switch cases (`stmt/switch.rs:104`, `:111`);
  - try and catch (`stmt/try_catch.rs:106`, `:270`, `:321`);
  - property hooks (`oop/property_hooks.rs:278`).
- The rules are imprecise in five places:
  - `while (true)`/`for (;;)` counts as never ending only if its body has no `break` at all, even one that targets a nested loop or switch (`control_flow.rs:187`, `:195`);
  - `do { ... } while (...)` is never terminating, even when its body always returns;
  - a `try` whose `finally` always terminates is not;
  - a block's "exits" answer is its last statement's, even when an earlier statement already returned;
  - a condition known by its type to be always true or false is treated as unknown;
  - `goto` is ignored: a label gets only the scope that falls through to it, and the goto targets serve only to keep a targeted label from being reported unreachable (`stmt.rs:66`).
- `check_stmt` returns nothing (`stmt.rs:83`). There are 9 `check_stmt` and 10 `check_block` call sites.
- The rest of what PHPStan's statement result carries (exit points, throw points, impure points, yields) is recomputed by separate syntactic walks, each narrower than the traversal:
  - **Exit points.**
    - `collect_exit_point_statements`, for the finally-override rule (`try_catch.rs:119`, `:273`, `:323`), looks only at a block's last statement and at `if` branches.
    - `stmt_contains_break` and `stmt_contains_escaping_exit`, for the loops' constant-condition rules (`loops.rs:74-75`, `:293`), count a `break` at any depth, which is the imprecision above again.
    - `is_switch_break_equivalent` (`control_flow.rs:43`).
    - The `Returns` visitor walks a closure's body again to infer its return type (`expr.rs:5845`).
  - **Throw points.** `collect_throws` (`try_catch.rs:402`, `:459`), and `body_contains_explicit_throw` and `expr_contains_explicit_throw` (`function_decl.rs:756`, `property_hooks.rs:180`, `:357`). Meanwhile the traversal already accumulates `current_function_throw_points` and `current_function_explicit_throw_points` for the function as a whole (`checker.rs:429-446`), as it does `current_function_return_points` (`:456`).
  - **Effects.** `noop::expr_side_effects` (`noop.rs:41`, `:174`).
  - **Termination, once more.** `stmt_definitely_terminates` in the uninitialized-property dataflow (`uninitialized_dataflow.rs:338`) misses `exit`/`die`, `switch`, `try` and labels.

**Design.** PHPStan's statement result (`InternalStatementResult`: scope, `hasYield`, `isAlwaysTerminating`, exit points, throw points, impure points), in this codebase's terms:
```rust
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct Completion {
    /// No path through the statement reaches the statement after it.
    pub(crate) terminates: bool,
    /// Every path that leaves goes out of the function (`return`, `throw`,
    /// a `never` expression).
    pub(crate) leaves_function: bool,
    /// The `break` (bit 2k) and `continue` (bit 2k+1) jumps that leave this
    /// statement, by how many enclosing loops or switches beyond it they go:
    /// k = 0 for the innermost enclosing one, saturating at 15.
    pub(crate) jumps: u32,
}
pub(crate) struct StmtResult {
    pub(crate) completion: Completion,
    /// Where this statement's exit and throw points start in the function's vectors:
    /// its points are the tail pushed while it was checked, less what it settled.
    pub(crate) exits_from: u32,
    pub(crate) throws_from: u32,
    /// The strongest effect while checking it (`Effects`, as `noop.rs` classifies them today).
    pub(crate) effects: Effects,
    pub(crate) has_yield: bool,
}
struct ExitPoint { kind: ExitKind /* Return(Type) | Break(u8) | Continue(u8) | Goto(Name, Box<Locals>) | Throw */, span: Span }
struct ThrowPoint { ty: Type, explicit: bool, span: Span }
pub(crate) fn check_stmt(&mut self, ..) -> StmtResult
pub(crate) fn check_block(&mut self, ..) -> StmtResult
```
- **Points as tails of two vectors.** Each function-like body has `exits: Vec<ExitPoint>` and `throws: Vec<ThrowPoint>`; today's three per-function accumulators become them. A statement's points are what was pushed while it was checked. A construct that settles some of its children's points removes them from its tail when it completes:
  - a loop, its k = 0 `break`s and `continue`s; a `switch`, its k = 0 `break`s;
  - a `catch`, the throw points its types catch;
  - a label, the `goto`s to it.

  What remains leaves the construct. There is no allocation per statement, and the function's rules read the body's whole vectors as they read the accumulators today. A loop pass that is discarded truncates both vectors to where it started, as `rewind_to` does with its diagnostics.
- **The readers take the result, and the side walks are deleted:**
  - the finally-override rule reads the exit tails of the `try` body and of the `finally`;
  - the loop constant-condition rules read "a k = 0 `break` in the body" from the completion, and escaping exits and `has_yield` from the result;
  - a closure's return type is the union of its body's `return` exit types, with no second walk. The `Returns` visitor serves `infer_expr`'s read-only view, which does not walk bodies and so misses their narrowing (`expr.rs:5818-5830`). It goes with §12.10's step 4, which folds inference into the traversal;
  - the `try`/`catch` and throws rules read throw tails;
  - the noop rules read `effects`, which the traversal raises node by node with today's classification.
  
  `collect_exit_point_statements`, `stmt_contains_break`, `stmt_contains_escaping_exit`, `is_switch_break_equivalent`, `collect_throws`, `body_contains_explicit_throw`, `expr_contains_explicit_throw` and `expr_side_effects`' walk are all deleted.
- **One set of completion rules.** The rules below are pure functions over child completions (`Completion::block`, `::if_chain`, `::loop_`, `::switch`, `::try_`), which take the facts about conditions as arguments. The traversal passes what it knows by type. §14's `InitFlow` builder passes what is known syntactically (a literal `true` condition, `exit`/`die`, a `throw`). So `stmt_definitely_terminates` goes too.

**Rules**, computed bottom-up from the results of checking the children:
- **`return`, `throw`, or an expression statement whose checked type is `never`** (`exit`, `die` and `throw` expressions included): terminates and leaves the function. The `assert` arm (`stmt.rs:511`) produces the same result directly, and the `terminating_assertions` span set and its save and restore (`function_decl.rs:932`, `:1231`) are deleted.
- **`break n`, `continue n`**: terminates, with jump bit k = n − 1.
- **Block**: statements after the first one that terminates are unreachable until a label that a `goto` targets (P1's parser-recorded set), which makes the code after it reachable again.
  - The label may sit at any depth. PHP allows a `goto` into an `if`, `else`, `try` or plain block, but not into a loop, a `switch` or a `finally` (all checked with `php`; the last three are compile errors).
  - Unreachable statements are still checked, as today, so the traversal reaches every label. Reachability restarts at a targeted label wherever it is. The completions of the statements around the label, computed bottom-up, then reflect it: an `if` whose then branch is entered at a label falls through when that branch's last region does.
  - The jumps of unreachable statements cannot happen.
  - The block's result is that of its last reachable region: that region's first terminating statement (its `leaves_function`), with the union of the jumps of every reachable statement. If the last region never terminates, the block falls through.
  - Today `always_terminates` says a block terminates when any statement does (`control_flow.rs:114`), so `return; target: foo();` reached by a `goto` is wrongly taken as never falling through.
- **`if`/`elseif`/`else`**: if the checked condition is always true by type, the result is the then branch's; if always false, the rest of the chain's. Otherwise it terminates when an `else` exists and every branch terminates; jumps are the union, and `leaves_function` holds when every branch's does.
- **Loops**:
  - A body jump at k = 0 belongs to the loop: a `break` lets the loop end, a `continue` goes to the next pass. Deeper jumps leave with k − 1.
  - `while`/`for` terminates when its condition is always true by type (a missing `for` condition counts as true) and the body has no k = 0 `break`.
  - `do`-`while` terminates when its body terminates with no k = 0 jump, or when its condition is always true and the body has no k = 0 `break`.
  - `foreach` terminates when the iterated type is non-empty by type (`isIterableAtLeastOnce` in PHPStan) and its body terminates with no k = 0 jump.
- **`switch`**: a case body's k = 0 `break` or `continue` belongs to the switch, and deeper jumps leave with k − 1. The switch terminates when it has a `default` and every case path reaching its end terminates without a k = 0 jump (fall-through as today's rule).
- **`try`**: terminates when the body and every `catch` terminate, or when `finally` terminates. Jumps are the union of all three.
- **`goto L`** terminates, since nothing after it is reached by falling through. It neither leaves the function nor jumps a loop level, and it records its scope as an entry of `L`. PHPStan 2.2.16 does the same: `GotoHandler` returns an always-terminating result with an exit point, and `NodeScopeResolver::mergeForwardGotoExitPoints` merges it at the label.
  - **Forward jumps.** A `goto L` pushes an exit point `Goto(L, scope)` into the function's exit vector (above). At label `L`, the scope is the merge of the scope that falls through (when the label is reachable) with the scope of every `Goto(L)` in the vector, which the label removes. The vector belongs to the function, not to the statement list, so a label inside a block sees the gotos outside it. PHPStan's merge works per statement list and does not. There is no separate table of pending gotos.
  - **Backward jumps.** A `goto L` that comes after `L` in source order (P1 marks such labels) turns a region into a loop. The region runs from the statement holding `L` through the statement holding the `goto`, within their innermost common statement list. It runs through `converge_loop_body`'s fixpoint, merging the goto's scope into `L`'s entry on each pass, as PHPStan's `resolveBackwardGotoScope` does, and its passes are recorded and replayed like any loop's (§12.10).

The callers read results. `exits_function_or_loop`'s one use (`stmt/switch.rs:111`) becomes "terminates without a k = 0 jump". Loop and closure convergence keep the completion of the pass they keep. `always_terminates` and `exits_function_or_loop` are deleted.

**What changes, and why it is right.**
- An expression is judged in the scope it ran in, not the one after it, and a branch by its own statements' results. This is PHPStan's model.
- The precision rules make more code terminating, and therefore more code after it unreachable. Every one of them is something PHP itself guarantees.
- At a label, a variable's type is what every path reaching it gives. Today a `goto` that carries another type to the label is not seen, which is unsound.

**Steps.**
1. `check_stmt_impl`'s arms return a `Completion` under today's rules exactly, while `always_terminates` stays. A debug assertion at each old call site compares the two answers.
2. The callers switch; the old functions and the assertion are deleted.
3. One commit per precision rule: jump levels, the unreachable tail, conditions known by type, `do`-`while`, `foreach` over non-empty types, `finally`.
4. `goto`: labels at any depth, forward entries, then backward regions.
5. Exit and throw tails, effects and loop yields in the result. Each side walk is deleted after a test build has compared its answer with the result's over the fixtures and corpus.
6. The completion rules as shared functions, which §14's `InitFlow` uses (step 4 there).

**Oracle.**
- Step 1's assertion over the fixtures and corpus lists every place the two answers differ (the scope-after effect), and each one is explained.
- After every commit, O2's diff is read line by line.
- Fixtures for each precision rule. The `goto` fixtures cover forward jumps into blocks, backward jumps, jumps out of loops and nested regions; each fixture's paths are confirmed by running it with `php`.

### 12.2 C2. Call return types: no binding work for template-free signatures, parameters bound by position

**Now.**
- `bound_call_return_type` (`array_ops.rs:468-481`) always calls `call_template_bindings` (`:501-534`), which:
  - builds a `FxHashMap<String, Type>` sized to the parameters;
  - re-infers every argument;
  - stringifies every parameter name;
  - infers template bindings.
- The bindings are read only by `substitute_call_templates` and `resolve_conditional` on the return type.
- `substitute_with` already returns early on a type without `HAS_TEMPLATE_REF | HAS_LATE | HAS_DEFERRED` (`templates.rs:441`); `resolve_conditional` has no early exit. It finds a parameter subject's binding by name (`conditional.rs:45`, `param_bindings.get(param.as_str())`).

**Design.**
- **Fast path.** With P8's flags, `if !ret.needs_call_binding() { return resolve_deferred_value_of_now(ret.clone()) }` before computing any binding. `Type::needs_call_binding()` tests `HAS_TEMPLATE_REF | HAS_LATE | HAS_DEFERRED | HAS_CONDITIONAL`.
- **Parameter subjects by position.** A conditional's parameter subject names a parameter of the signature whose return type holds it, so the position is known when the signature is built:
  - `ConditionalSubject::Parameter(Name)` becomes `Parameter { name: Name, index: Option<u16> }`.
  - `FunctionReflection::from_decl` and the stub decoder fill `index` from `params`, using the variadic parameter's slot for its name. A name no parameter has stays `None` and binds nothing, as today it finds no binding.
  - `resolve_conditional(&self, args: &[Option<Type>], relation)` reads `args[index]`. Each slot holds the argument's type, or for an omitted argument its default's type, or `None`; these are the slots `call_template_bindings` fills today.
  - The argument slice is built only when the return type has `HAS_PARAM_CONDITIONAL`, a second bit set by a parameter-subject `Conditional`. There is no map, no parameter name formatted, and no re-inference of arguments for other calls.
  - Equality and hashing of `Conditional` include the index. That is correct: the same text in two signatures with different parameter orders denotes different types. Display is unchanged.
- **Template bindings stay keyed by name**, `TemplateBindings = FxHashMap<Name, Type>` (`checker.rs:231`), with C3's precomputed hash and reserved from the template count (the final plan's N14). Templates are not local to one signature as parameters are: class and method templates meet in one binding map, and inference finds them inside nested types, where a reference is only its name (`Repr::Template(Name)`, `types/src/lib.rs:434`). So the map is keyed as the references are, by name.
- `narrowing/assertions.rs:402`, the other caller of `call_template_bindings`, needs the bindings and keeps the full path.

**Exactness.** With the four bits clear, substitution returns the same node, and `resolve_conditional` returns `self` (P8). The skipped argument inferences write only caches and deduplicating sets, which the arguments' own checks fill. Binding by position gives the same slot the name lookup found.

**Steps.**
1. The fast path.
2. Positional parameter subjects, with a stub cache format bump.
3. Name-keyed template bindings.

**Oracle.** O2 including `--dump-types`, and the types crate's conditional tests.

### 12.3 P8. Type flags and cached predicates

**Design.**
- `HAS_CONDITIONAL: u16 = 256` is set by every `Repr::Conditional`, `HAS_OBJECT: u16 = 512` by `Repr::Object`, and `HAS_PARAM_CONDITIONAL: u16 = 1024` by a `Conditional` with a parameter subject (C2), both in `reference_flags` (`lib.rs:145`) and passed up like the others. Of the 16 bits, 8 are used today.
- `resolve_conditional` returns `self.clone()` when `HAS_CONDITIONAL` is clear; `map_class_names` and `replace_objects` when `HAS_OBJECT` is clear.
- The structural predicates `is_null`, `is_object`, `is_array`, `is_string`, `is_bool`, `is_int`, `is_float`, `is_scalar` and `is_iterable` (`queries.rs:736-875`), plus `to_string_type`'s acceptance test, get two bits each in `refined`, filled on first ask through the same `is_within` path. Ten kinds already use 20 bits; the nine new ones need 18 more, so `refined` becomes `AtomicU64`. `size_of::<Node>()` is asserted in a test before and after: `Node` is `Repr` plus two `u64` hashes, `u16` flags and the atomic, and the `u16` to `u32` padding is checked.
- Union construction: §12.11.

**Oracle.**
- A test-only switch `types::testing::disable_shortcuts()` makes each early exit and each cached bit fall through to the computation.
- A test runs the types crate's suite and a corpus-derived type list (every type the corpus run interns, dumped once) with the shortcuts on and off, and compares results.
- O2.

### 12.4 C3. Names as interned identities, case-sensitive and folded

**Now.**
- `NameData { text, lower: Option<Box<str>> }` (`name.rs:8-11`). `Name`'s `PartialEq` compares pointers and then text, `Hash` hashes the text, and `Name: Borrow<str>`, so every `HashMap<Name, _>` rehashes the bytes on each insert and lookup and can be asked with a `&str`.
- `Project` owns an interner (`project.rs:157`, `:524`), while deserialization and `From<&str>` use the global one.
- There are 147 `HashMap`/`HashSet`s keyed by `Name` in 23 files, and 211 keyed by `String` in `phpstan-rs`, `types` and `phpdoc`, most of the latter holding PHP identifiers.
- `NameTable` is a `FxHashMap<Folded, V>` with a `hidden` set and a stored `Base` searched by `hash_key` over the folded key bytes (`name_table.rs:233`, `:423`). A `&str` key with a capital is copied into an `ArrayString` and lowercased (`with_function_key`, `:50`).
- A method lookup goes `method_owners(class)` → `owners.get(method)` (exact spelling, rehashing the text) → `classes.get(owner)` → `methods.get(method)` (rehashing again), with a linear case-insensitive scan as the fallback (`hierarchy.rs:598-614`). HEAD memoizes it in `FxHashMap<String, FxHashMap<String, ..>>` (`checker.rs:277`).

**Design.**
```rust
// syntax::name
struct NameData {
    text: Box<str>,
    hash: u64,            // text_hash(text), which the interner and the lexer already compute
    folded_hash: u64,     // folded_hash(text): FxHash of the ASCII-lowercased bytes, a word at a time
    folded: Option<Name>, // the lowercase spelling's Name, when it differs
}
impl PartialEq for Name { fn eq(&self, o: &Name) -> bool { Arc::ptr_eq(&self.0, &o.0) } }
impl Hash for Name { fn hash<H: Hasher>(&self, h: &mut H) { h.write_u64(self.0.hash) } }
// No `Borrow<str>`. `PartialEq<str>` and `Ord` (by text) stay.
impl Name {
    pub fn folded(&self) -> &Name;                   // itself when already lowercase
    pub fn lower(&self) -> &str;                     // self.folded().as_str()
    pub fn folded_hash(&self) -> u64;
}
pub fn folded_hash(text: &[u8]) -> u64;              // the same function, for probes by text, no copy

#[derive(Clone, PartialEq, Eq, Hash)] pub struct FoldedName(Name); // always a folded Name: identity of
                                                                  // a case-insensitive PHP identifier
pub type NameHasher = BuildHasherDefault<PassThroughHasher>;      // write_u64 stores, finish returns it

/// Maps whose lookups sometimes start from text, not from a Name.
pub struct NameMap<V>   { table: hashbrown::HashTable<(Name, V)> }  // get(&Name) | get_str(&str)
pub struct FoldedMap<V> { table: hashbrown::HashTable<(Name, V)> }  // keyed by the declared spelling;
                                                                     // get(&Name) | get_str(&str), ignoring case
```
1. **One interner.** `Project`'s interner, `with_interner` and the field go; everything interns into `Interner::global()`, whose weak table already frees what nothing holds. `Interner::new` is removed too: `gate.rs` and the tests that build their own interner use the global one. Identity equality is only sound when a second interner cannot exist. Interning a mixed-case text interns its folded spelling first, outside the shard lock, then the text with `folded` set. The two may live in the same shard, and creation must not hold one shard's lock while taking another's. The interner returns the live allocation for a text when one exists (`WeakTable::get_or_insert` upgrades before it creates; a test holds concurrent interning to one allocation). So, with one interner, equal text means the same allocation, and `Name`'s equality and hashing can be its identity: a pointer comparison, and a hash read from the node.
2. **`Borrow<str>` removed.** Each `HashMap<Name, _>`/`HashSet<Name>` lookup by `&str` either gets a `Name` from its caller (the point of this item: callers hold names), or, where the lookup starts from text (a class named in a string literal, a configuration entry), the map becomes a `NameMap` with `get_str`. Compile-driven, module by module.
3. **Every `String` key classified.** Each of the 211 `String`-keyed maps and sets gets the identity PHP gives its key:
   - case-insensitive identifiers (class-likes, functions, methods): `FoldedName`;
   - case-sensitive identifiers (properties, class constants, variables, parameters, templates): `Name`;
   - namespaced global constants (namespace part case-insensitive, name exact): a `ConstName { ns: FoldedName, name: Name }`;
   - everything that is not an identifier (paths, messages, SQL, configuration) stays `String`.
   The classification is a table in each module's commit. It covers every value that holds a PHP identifier, not only map keys: struct fields (`ParsedFile::{functions, classes, constants, anonymous_classes}`, `ChangedNames`, `DeclaredNames`, reflections' `extends`/`implements`/`uses` and the like), function parameters and return types (`&str` that names a class, function, method, property or constant), and the `Option<String>`/`Vec<String>` the checker carries (`current_class`, `inline_var` names and so on).
4. **`NameTable<V>`** is built on `FoldedMap<V>` for `entries` and on a `FoldedName` set for `hidden`. `TableKey` becomes `Into<Probe>` (`&Name` → the stored folded hash, `&str` → `folded_hash` on the fly), so no key is ever copied or lowercased. The stored `Base` keeps a `folded_hash: u64` per record, probed by it (stub cache format bump). A base probe by `Name` compares bytes folded and then caches the slot's `Name` as today (`Base::name`).
5. **Member tables.**
   - `ClassReflection::methods` (`class_info.rs:369`) becomes `FoldedMap<Arc<FunctionReflection>>`: PHP rejects two methods of one class differing only in case, so one key per folded name loses nothing.
   - Properties and constants keep `Name` keys, being case-sensitive in PHP.
   - The owners pass (`own_owner_names`, `hierarchy.rs:355`) builds per class a `FoldedMap<MethodEntry { declared: Name, owner: Name, sig: Arc<FunctionReflection> }>`, holding the most-derived declaration per folded name.
   - A lookup is the class's owners entry plus one probe, with no second class lookup and no fallback scan:
     ```rust
     pub fn lookup_method_with_owner(&self, class: impl Into<Probe<'_>>, method: impl Into<Probe<'_>>) -> Option<(&Name, &FunctionReflection)>
     ```
   - `lookup_method_canonical_name` is `entry.declared`. `find_case_mismatched_ancestor_method` reads the declarations along the ancestry, which the owners pass records per class.
6. **Callers hold names:**
   - the checker's `current_class: Option<String>` becomes `Option<Name>`;
   - `declared_method_name` returns `&Name`, and `resolve_static_class_name` returns `Option<Name>`;
   - `class_template_bindings_rec`'s visited set holds `FoldedName`s;
   - `TemplateBindings` is keyed by `Name`, and property-write keys are `(Option<FoldedName>, Name)` (F4);
   - C4's keys, P9's references, and `bind_self`/`bind_static` taking `&Name`.

   Nothing formats a `Name` to compare it or to key a map.
7. **HEAD's method memo** (`c848318b`) is deleted. A lookup is now two probes by stored hash, and keeping the memo would add a third; O3 confirms.

**Semantics.**
- `Name` equality keeps its meaning (same text, case-sensitive); only its implementation changes. A debug assertion inside `PartialEq` checks that two different allocations differ in text, across the test suites and the corpus run.
- The one intended change is method resolution under mixed spellings (§10.1, item 4): the folded table resolves to the most-derived declaration, as PHP does. A debug assertion comparing the old and new `lookup_method_with_owner` lists each difference over the fixtures and corpus.
- The `ConstName` keys make `defined('Ns\FOO')` guard `ns\FOO` too, as PHP does.

**Steps.**
1. `NameData` fields, `folded_hash`, `FoldedName`, `NameMap` and `FoldedMap`, with differential tests: each map against a `HashMap<String, _>` keyed by the text (or its `to_ascii_lowercase()`), on random spellings, ASCII and not.
2. One interner.
3. Identity equality and hashing, and removal of `Borrow<str>` (compile-driven).
4. `NameTable` on `FoldedMap`, with the stub cache bump.
5. Member tables and `lookup_method_with_owner`; the memo is removed.
6. The `String` keys, by module: `checker::expr`, `checker::calls`, `checker::narrowing`, `registry::hierarchy`, `registry::purity`, `registry::property_writes`, `project`, `types`, `phpdoc`.

**Oracle.**
- The differential map tests.
- The `NameTable` lookups against the old table, over every name the corpus run asks for (recorded once).
- O2, with the method-resolution differences listed.

### 12.5 C4. `Locals` by interned key, with a dependency index and structural diffs

**Now.**
- `Locals` (`locals.rs`) holds 15 `imbl` maps and sets keyed by `String`: types, natives and provenance per variable; the synthetic-key set; guarded functions, constants, classes, methods and properties; condition aliases; branch-implied facts by `Discriminant { subject: String, value }`; query-builder aliases; container-field facts `String → Rc<FxHashMap<String, Type>>`; discriminant-array facts `(String, String, bool)` (`checker.rs:24-231`).
- Synthetic keys are formatted strings: `a->b`, `a[k]`, `a[$k]`, `C::$p`, `truthy(..)`, `isset(..)` (`narrowing/predicates.rs:64-151`, `narrowing.rs:206`, `locals.rs:246`).
- A purge scans the synthetic set with string prefix tests (`purge_stale_narrowing_for`, `predicates.rs:197`).
- `merge_scopes` visits every key of both scopes (`control_flow.rs:576`), and `record_branch_implied_facts` iterates the branch's locals three times (`narrowing.rs:426`).
- There are 179 `locals.get/insert/remove/declare/contains_key/insert_with_provenance` calls and 65 `narrowing_key` calls in `checker/`.

**Design.**
```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub(crate) struct LocalKey(u32);

pub(crate) enum KeyData {
    Var(Name),
    Prop { base: LocalKey, prop: Name },
    Index { base: LocalKey, key: ArrayKey },        // int or string, canonical as PHP stores array keys
    VarIndex { base: LocalKey, subscript: LocalKey },
    StaticProp { class: ClassRef, prop: Name },     // ClassRef: Resolved(FoldedName) for a name, self or parent; LateStatic for static
    Truthy(LocalKey),
    Isset(LocalKey),
}

/// One file check's keys: interned once, never removed.
pub(crate) struct KeyTable {
    data: Vec<KeyData>,
    index: hashbrown::HashTable<LocalKey>,           // by KeyData
    dependents: Vec<SmallVec<[LocalKey; 2]>>,        // key → keys built on it (base, subscript, truthy, isset)
}
impl KeyTable {
    pub(crate) fn var(&mut self, name: &Name) -> LocalKey;
    pub(crate) fn key_of(&mut self, e: &Expr) -> Option<LocalKey>;   // today's `narrowing_key`
    pub(crate) fn find(&self, e: &Expr) -> Option<LocalKey>;         // read side: no insertion
    pub(crate) fn is_real_variable(&self, k: LocalKey) -> bool;
    pub(crate) fn render(&self, k: LocalKey) -> String;             // the old string, for messages and dumps
}

type IdMap<V> = imbl::OrdMap<LocalKey, V>;   // ordered by id: `diff` skips the nodes two forks share
pub(crate) struct Locals {
    types: IdMap<Type>, native: IdMap<Type>, provenance: IdMap<ProvenanceSet>,
    synthetic: imbl::OrdSet<LocalKey>,
    guarded_functions/classes: imbl sets of FoldedName,         // case-insensitive in PHP
    guarded_constants: imbl set of ConstName,                    // namespace folded, name exact (C3)
    guarded_methods: imbl set of (LocalKey, FoldedName),         // receiver variable; method names fold
    guarded_properties: imbl set of (LocalKey, Name),            // receiver variable; property names are exact
    condition_aliases: IdMap<(SmallVec<[LocalKey; 2]>, Rc<Expr>)>,
    branch_implied_facts: imbl map of Discriminant { subject: LocalKey, value: Type } → Rc<Vec<(LocalKey, Type)>>,
    container_field_facts: IdMap<Rc<FxHashMap<ArrayKey, Type>>>, // the fields are array keys
    discriminant_array_facts: imbl map of (LocalKey, LocalKey, bool) → Type, // container, subject variable
    query_builder_aliases: IdMap<Rc<Expr>>,                     // variable keys
    clone_reinitialized: imbl set of Name,                       // property names, case-sensitive
    ... flags as today, version as today
}
```
- **Keys are PHP's identities.** Today a key is formatted text.
  - `C::$p` keeps the class as written (`narrowing/predicates.rs:87`), so inside `Foo`, `self::$p`, `Foo::$p` and `foo::$p` are three keys for one property.
  - An index key is the literal's text (`literal_string_key`, `assignment.rs:909`), so `$a['k][j']` and `$a['k']['j']` both format as `a[k][j]` and are one key for two different elements.
  - Structured keys fix both. The class is resolved (`self`, `parent` and names to the folded class name; `static` stays late-bound and distinct), and an index key is canonical as PHP stores array keys (`'1'` and `1` one key, `'01'` another; `canonical_int_key`). Both are semantic corrections, read in O2's diff.
  - The fact maps key by the same identities. The `method_exists`/`property_exists` guards are recorded per receiver variable, not per class (`(recv_name, member)`, `narrowing.rs:729-744`), so they hold a `LocalKey`. Container-field facts are written back as index keys (`{target}[{field}]`, `predicates.rs:273`), so their fields are `ArrayKey`s. A discriminant's subject is a variable. The checker's `active_discriminant: Vec<(String, bool)>` becomes `Vec<(LocalKey, bool)>`.
- **Key table.** One per `Checker` (one file), shared by every function, closure and scope of the file. `Locals` holds only ids; the methods that need a key's name take `&KeyTable` from the checker. `KeyTable::key_of` reproduces `narrowing_key`'s rules exactly: peeling, assignments, `?? null`. A key is created only by the write side (`insert`, `declare`, narrowing); a read uses `find`, which never inserts, so reads cannot grow the table.
- **Purge.** `purge_stale_narrowing_for(locals, keys, k)` removes `keys.dependents(k)` transitively, intersected with what the scope holds: it iterates the smaller of the dependents list and `synthetic`.
- **Every fact about a variable ends with it.** A rebind purges, by id, every fact keyed by the variable or by a key built on it: types, synthetic keys, condition aliases, branch-implied, container-field and discriminant facts, query-builder aliases, and the two member guards.
  - Today `declare` and `purge_stale_narrowing_for` leave the member guards (`locals.rs:170-193`, `predicates.rs:197-229`). So in `if (method_exists($o, 'm')) { $o = new Other(); $o->m(); }` the call still counts as guarded.
  - They also keep the discriminant facts recorded under a subject that has since been rebound.
  - Both are semantic corrections, read in O2's diff.
- **What changed between two scopes comes from the maps themselves.** The id-keyed maps are `imbl::OrdMap`s, whose `diff` walks two maps together and skips every node they share (`advance_skipping_shared`, `imbl` 7.0.2, `ord/map.rs:1938`). Two scopes forked from one, such as the branches of an `if` or a loop pass and its entry, share every node no write touched. So `diff` costs the changed paths, not the map. It yields exactly the keys whose values differ, with no log kept beside the maps. `imbl::HashMap` has no such diff, and the first version of this design added a write-history list to every scope to make up for it: a second record of what the persistent maps already hold, maintained on every write.
- **`merge_scopes(a, b)`** starts from `a`'s maps and recomputes only the keys in `a.diff(b)`. A map whose root is the same in both (`ptr_eq`) is taken as it is.
- **`record_branch_implied_facts(before, after)`** iterates only `before.diff(after)`, the only keys that can satisfy its `before.get(name) != after` tests.
- **Loop convergence's equality** is an empty `diff`.
- **Ordered by id.** Keys compare as their `u32`, so an `imbl` node copy copies ids and `Type` pointers, with no `String` clones. With at most a few hundred keys in a function, a B-tree of `imbl`'s node width is one or two levels deep, as shallow as the hash trie it replaces. Iteration is in key-creation order, so anything that lists locals gets an order that does not depend on a hasher; O2's diff is read for any message whose order changes.

**Steps.**
1. `KeyTable` and `LocalKey`, with `KeyTable::render(key_of(e)) == narrowing_key(e)` tested over every expression of the fixtures and corpus;
2. `Locals` maps by `LocalKey` and every call site (one compile-driven commit; `narrowing_key`'s 65 callers and the 179 `locals.*` calls);
3. dependency-indexed purge;
4. `OrdMap` diffs in `merge_scopes`, `record_branch_implied_facts` and convergence equality;
5. the fact maps' keys, and the purge of every fact about a rebound variable.

**Oracle.** `--dump-types` (O2) prints every tracked key's type after every statement through `render`; byte-identical over the fixtures, vendor tree and corpus after each of steps 2-4 except where a key correction applies (each such line listed), and O2's diff read after step 5. Also the LSP provenance tests, and a debug assertion in step 4 that the merge over the diff equals the full merge.

### 12.6 C5. Diagnostic text built only for diagnostics; statement rules read the call's resolution

**Labels.**
```rust
#[derive(Clone, Copy)]
pub(crate) enum MemberLabel<'a> {
    Method { owner: &'a Name, name: &'a Name },
    StaticMethod { owner: &'a Name, name: &'a Name },
    Property { owner: &'a Name, name: &'a Name },
    StaticProperty { owner: &'a Name, name: &'a Name },
    Constant { owner: &'a Name, name: &'a Name },
}
impl MemberLabel<'_> { fn identifier_prefix(&self) -> &'static str }  // "method", "staticMethod", ...
impl Display for MemberLabel<'_> { .. }                              // "method Foo::bar()", "property Foo::$bar", ...

#[derive(Clone, Copy)]
pub(crate) enum CalleeLabel<'a> { Function(&'a Name), Method { class: &'a Name, method: &'a Name }, Constructor(&'a Name), Fixed(&'static str) }
// The labels borrow the names the call arms hold after C3, which this item follows so the labels are written once.
impl Display for CalleeLabel<'_> { .. }
```
- `MemberVisibilityCheck { kind, describe, .. }` (`visibility.rs:107-131`) becomes `{ member: MemberLabel, .. }`: the label's variant gives the identifier prefix, so `kind` and `describe` can no longer disagree. Its construction sites are `expr.rs:1371`, `:2691`, `:3031`, `:4240`, `:4307`, `:6231`, `:6278`.
- `CheckArgsCall::callee_label: &str` (`calls.rs:19`) becomes `CalleeLabel`; its readers are the two opt-in message sites (`calls.rs:517`, `:590`). It is built at `expr.rs:871`, `:1420`, `:1821`, `:2475`, `:2742`, `calls.rs:751`, `:784`.
- `check_function_impl`'s `label`, `display_name` and `subject` (`function_decl.rs:291`, `:396`, `:1145`) become a `FunctionLabel { class: Option<&Name>, name: &Name, kind }` with `Display`.

**Statement rules.** `check_no_discard_statement` (`misc_rules.rs:30-80`), `check_pure_call_without_effect` (`noop.rs:240`) and `check_new_exception_discarded` re-infer the receiver after the call and look the method up again, for every call statement.
- They are the first rule family moved to §12.10's observers, whose steps 1 and 2 for this family come right after C3 (§15). As observers of the statement they read the receiver's type from `at.types`, computed once and in the scope the call ran in.
- They resolve the owner once with `find_method_owner`, the same function as today, and test `sig.no_discard() || (sig.must_use_result() && language_extensions)` before formatting anything, through `CalleeLabel`.
- An interim `StatementCall` record, which the first version of this design had, would have been written and then replaced by the observer API, so it is not built.

**What changes.** The three rules see the receiver's type before the call instead of after it. Only a call that changes its own receiver variable can tell them apart; O2's diff is read for any.

**Steps.** After C3: (1) labels; (2) the statement rules as §12.10's first observer family. **Oracle.** O2.

### 12.7 C6. Cheap tests first, answers per class, one lookup

- **`check_node_connecting_attribute`** (`phpstan_api.rs:417-450`) tests `method.eq_ignore_ascii_case("getAttribute")` first, then `is_subclass_of(recv, "PhpParser\\Node")`, then the calling class's PHPStan ancestry.
  - The ancestry is one registry fact per class: `phpstan_ancestry: bool`, whether extends, implements or uses, transitively (traits included), reach a `PHPStan\` name.
  - It is computed by the finishing pass that computes `ancestor_names`, extended over `uses` for this bit, and stored with the class.
  - The rule here and the walker's gate below both read it; `class_implements_phpstan_interface`'s per-call walk is deleted. Exact: the same closure today's walk computes, computed once.
- **The PHPStan-API walker** (`project.rs:2119`) is a walk of its own, run serially on the main thread inside `finish_diagnostics`. It becomes an observer family of §12.10, with no walk and no interim stage.
  - The family is registered for a file when `mentions_phpstan` is set or the file declares a class with the ancestry fact above.
  - Its diagnostics travel with the file's results as a list of their own. `finish_diagnostics` appends them where it does today, after the first sort and before the second (`project.rs:2117-2131`), so their order among equal positions is unchanged.
  - The rest of `finish_diagnostics` runs in parallel with an ordered collect (the final plan's C2).
  - Moving the walker into the per-file check first, as the first version of this design did, would have rewritten it once more before §12.10 replaced it.
  - **Oracle:** the old walker's output against the family's over the fixtures and corpus; O2.
- **Unused private members** (`unused.rs:138`). `check_unused_private_members` walks every method body of every class twice (`:172-173`), with or without private members, to fill `MemberUsage`. It becomes §12.10's member-usage observer family, with no walk of its own.
  - Accesses are attributed by the receiver's type.
  - Reporting happens at the end of the file, after the closure rebinds the traversal has seen.
  - A class without private constants, methods, `private(set)` properties, private properties or private promoted parameters (`:180-336`) registers nothing.
  - A single-pass rewrite of the separate walk, which the earlier version designed, is not built.
- **`method_call_return_type`** (`member_resolution.rs:524`) makes one `lookup_method_with_owner(class, method)`. When that misses, it makes one for `__call`; only when both miss does it ask `classes.contains_key(class)` to choose between the error type and an unknown class. This replaces `contains_key`, `member_confirmed_missing` (two lookups) and `declared_method_return_type`'s third lookup.
- **`bind_arguments`** (`parameters.rs:409`) builds no map. A named argument is matched by scanning the parameter slice with `Name` identity (C3): a signature has a handful of parameters, and a scan with pointer comparisons beats building a hash table per call, lazily or not.

**Steps.** Three commits here: the ancestry fact with `check_node_connecting_attribute`, `method_call_return_type`, and `bind_arguments`. The walker and the member-usage check move with their §12.10 families. **Oracle.** As listed, and O2.

### 12.8 C7. The infer cache as per-node entry lists

**Now.** `cached` (`expr.rs:4486-4510`) keys a `FxHashMap<InferKey, Type>` by `(expr address, id, scope version, question)`. The map is cleared per outermost expression check and bypassed for `NodeId::SYNTHETIC` and outside expression checks: 1.66 M lookups, 32% hits, ≈170 Ir each with the inserts.

**Design.** The same keys, without hashing:
```rust
struct InferSlots { generation: u32, nodes: Vec<NodeEntries> }   // indexed by node id; sized by the parser's node count
#[derive(Default)] struct NodeEntries { generation: u32, entries: SmallVec<[Entry; 2]> }
struct Entry { addr: usize, version: u64, question: InferQuestion, ty: Type }
```
- `begin_outermost` bumps `generation`. A node whose `generation` is stale is empty, and is cleared on its first touch.
- A lookup scans the node's entries for `(addr, version, question)`: one or two entries almost always. A miss appends.
- The key set is exactly the map's, so the hits are exactly the map's. There is no trade-off to measure: the old "latest version only" slot design is dropped.
- Nodes from another tree that share an id differ in address, as today.

**Later.** At §12.10's step 4 nothing re-infers a node within a pass, and the lists become `NodeTypes`, one entry per node per pass. The indexing by node id stays.

**Steps.** One commit. **Oracle.** O2 with `--dump-types`, plus a debug assertion comparing every answer with the map, which runs beside it in a test build.

### 12.9 C8. Interning from borrowed parts

**Now.**
- `types::intern::insert(repr)` (`intern.rs:172`) takes an owned `Repr`, so every constructor first builds one: it allocates its `Vec`s, clones its child `Arc`s, and on a hit drops it again (`drop_glue::<Repr>` 104.8 M).
- `exact_hash` (`intern.rs:395`) hashes the owned form.
- The busiest constructor, `Type::string_literal`, is called 204,794 times by the checker re-inferring the same literal nodes.

**Design.**
- **(a) Borrowed interning, for every constructor.**
  ```rust
  pub(crate) enum ReprRef<'a> {           // one variant per Repr variant, children borrowed
      StringLiteral(&'a PhpString), IntLiteral(i64), Object(&'a Name, &'a [Type]),
      Array(&'a Type, &'a Type), Union(&'a [Type]), Intersection(&'a [Type]), Shape(&'a Shape), ...
  }
  impl Repr { fn as_ref(&self) -> ReprRef<'_> }
  impl ReprRef<'_> { fn to_owned(&self) -> Repr }
  fn exact_hash(r: &ReprRef<'_>) -> u64;                  // the only definition
  fn exact_eq(stored: &Repr, r: &ReprRef<'_>) -> bool;
  fn intern(r: ReprRef<'_>) -> Arc<Node>;                 // front cache, then shard; `to_owned` only on a miss
  ```
  - Constructors take borrowed parts where callers hold them: `object(&Name, &[Type])`, `union_of(&[Type])`, `string_literal(&PhpString)`, `array(&Type, &Type)`, and so on.
  - `Type::new(repr)` remains for code that owns a `Repr` already, and calls `intern(repr.as_ref())`.
  - `exact_hash` exists once, over `ReprRef`, so the owned and borrowed paths cannot drift.
- **No literal special case.** The first version also kept C7's entry for an `ExprKind::Literal` node across generations, so that `infer_expr_unguarded`'s literal arm (`expr.rs:4514-4521`) ran once per node per file check. The repeats it saves are re-inference, which §12.10 removes. After §12.10's step 4 a literal is typed once per node per pass, through (a)'s borrowed interning. So it would be a special case in C7 that step 4 deletes, and it is not built.

**Steps.**
1. Add `ReprRef`, the single `exact_hash` and `intern`.
2. Convert the constructors, one module of `types` per commit.

**Oracle.** A property test: for random types, `intern(r.as_ref())` returns the same node as interning the owned `r`, and the hashes agree. Plus O2.

### 12.10 C9 and C10. One traversal; rules observe it

**Now.** `check_expr` and `check_stmt` interleave three things:
- computing types: `infer_expr`, pure over `&Locals`;
- moving the scope: `&mut Locals` for assignment, narrowing and invalidation;
- rules that report: 335 distinct `self.check_*` entry points, and 550 `report`/`report_builder` sites, in 59 K lines of checker code.

Two consequences follow:
- Rules re-infer what they need with `infer_expr(sub, locals_now)`, against the scope as it is when they run, which earlier steps have already moved. That is 94,506 re-inferences costing 399.3 M, missing the cache; `infer_expr_unguarded` runs 1.13 M times against 609 K `check_expr_impl` calls.
- Rules can touch checker state, so no pass can skip them. Every pass of a loop is a full pass: 1.55 passes per `foreach`, 4.1 per `for`, 0.6-0.7 G of passes after the first (§4 C9).

**Design.** PHPStan's `NodeScopeResolver` plus its rule registry, in this codebase's terms:
```rust
/// What a rule sees at a node: the scope the node ran in, and the types this
/// pass computed for the node and its operands.
pub(crate) struct At<'c, 'a> {
    pub(crate) scope: &'c Locals,
    pub(crate) types: &'c NodeTypes,          // C7's per-node entries for this pass
    pub(crate) ctx: &'c CheckContext<'a>,     // registry, settings, current class/function, keys (C4)
}
pub(crate) trait ExprRule { fn on_expr(&mut self, e: &Expr, at: &At<'_, '_>, out: &mut Report); }
pub(crate) trait StmtRule { fn on_stmt(&mut self, s: &Stmt, at: &At<'_, '_>, done: &StmtResult, out: &mut Report); }

fn process_expr(&mut self, e: &Expr, scope: &mut Locals) -> Type            // once per node per pass
fn process_stmt(&mut self, s: &Stmt, scope: &mut Locals) -> StmtResult      // C1
fn process_condition(&mut self, c: &Expr, scope: &Locals) -> (Locals, Locals) // truthy and falsy scopes
```
- **Types once.** `process_expr` computes each node's type in its evaluation scope, records it in `NodeTypes` and applies the node's effects to `scope`. What was `infer_expr(sub, locals)` for a visited operand is `at.types[sub]`. A hypothetical question ("what would `$x` be if this narrowed") runs the same `process_expr` on a cloned scope, which `imbl` makes cheap, with no observers and a scratch type table. There is one inference engine, and `infer_expr` as a second one disappears.
- **Rules cannot move the scope.** `At` hands out shared references only. A rule that needs a narrowing is not a rule; it belongs in `process_condition` (PHPStan's `TypeSpecifier`). The type system then enforces the split that C9 needs. Rules see the scope before their node's effects, as in PHPStan.
- **Rule registry.** Rules are values registered per node kind and setting. Opt-in rules are simply not registered when their setting is off, replacing today's `if self.settings...` tests at each call. External rules (`dispatch_stmt_rules`) are the same mechanism.
- **Loops (C9): record and replay.** A convergence pass runs no rules. Its observer is a recorder, which keeps for each node what the rules would be given: the node, the scope it ran in (an `imbl` clone, so a pointer copy) and its entries in the pass's own type table. When a pass's entry proves to be the fixpoint, its record is replayed through the registered rules. So the rules run once per loop, on the fixpoint pass, and the body is walked only as many times as convergence needs.
  - This is how PHPStan 2.2.16 checks loops: `ForeachHandler`, `WhileHandler` and `DoWhileHandler` record with `RecordingNodeCallback` during the passes, and `NodeScopeResolver::replayRecording` replays when the final entry equals the recorded pass's.
  - Costs, with *s* a walk, *r* the recording of one pass, *o* the rules over one pass and *n* passes: today *n*(*s* + *o*); record and replay *n*(*s* + *r*) + *o*. The saving is (*n* − 1)*o* − *n*·*r*. Recording is a few pointer copies per node, far below what the rules cost, so this is cheaper for every *n* ≥ 2 and costs *r* more at *n* = 1. There is no schedule to choose and nothing to measure first.
  - Nested loops and backward `goto` regions (C1) compose: an inner loop's replay goes to whichever observer is active, the rules or an outer recorder. PHPStan instead declines to replay a body holding a loop, a label or a class (`isReplayableConvergenceBody`), because its expression storage belongs to a walk. Here the record holds everything a rule reads, so no body is excluded.
  - Each pass writes its types into its own `NodeTypes` (step 4), and the kept pass's table is the one replayed.
- **Migration, family by family.** Each commit converts one family of rules to observers reading `At`, and its O2 diff is read. The families: calls and arguments, member access and visibility, operators, statements, phpdoc, strict and opt-in rules, the PHPStan-API rules (including C6's walker, which then needs no walk at all), dead code, purity, unused symbols. A rule that read the scope after its node now reads the scope at it. That is C1's and C5's change in general form, and it is how PHPStan's rules see code. When the last family has moved, `check_expr`/`infer_expr` fold into `process_expr`, and `check_stmt` into `process_stmt`.
- **The separate walks.** Besides its 335 entry points, the checker walks code on its own: 18 `Visitor` implementations, recursive helpers such as `stmt_contains_break`, and the per-file passes at the head of `check_program` (`checker.rs:1506-1540`). Under one traversal, each has one of three fates, decided by when its answer is needed:
  - **Part of the statement result (C1).** The exit, throw, effect and yield walks listed in §12.1, and the `Returns` visitor at step 4.
  - **Observers, because their answer is used only after the code they read.** Each moves with its family, and its walk is deleted:
    - `keywords::Scope` and `collect_declares`, and the passes `check_declare_directives`, `check_file_whitespace`, `check_statement_keywords`, `check_use_*_imports` and `check_used_names`;
    - `function_like_declarations::Promotions`;
    - `phpstan_api::Walker` (C6);
    - `unused/member_usage::Members` and its two passes (C6), and `unused/var_usage::Vars`;
    - `risky_tests::Assertions`;
    - the closure-rebind scan (`unused/rebind.rs`), whose only reader is the unused-private check (`unused.rs:153`). As an observer family, that check reports at the end of the file, after the traversal has seen every rebind.

    The member-usage family attributes an access by its receiver's type, as PHPStan's `UnusedPrivateMethodRule` does over `ClassMethodsNode` (`$classType->isSuperTypeOf($calledOnType)`). That replaces today's syntactic aliases (`new self()`, `instanceof self`, self-typed parameters) with what they approximate, a semantic change read in O2.
  - **Part of `InitFlow` (§14).** `uninitialized_dataflow::Finder` (`unsets_this_property`, `uninitialized_dataflow.rs:198`) reads the `unset`s that the summary records.
  - **Known to the traversal already.** `global_constants::Collector` finds which `__FUNCTION__`/`__METHOD__` sit inside a closure (`checker.rs:1513`, read at `global_constants.rs:155`), which the traversal knows from the function-like it is in.
  - **Passes that stay**, because checking needs their answer before it reaches the code that uses it:
    - the include initializers and guards (`includes.rs`, gated as in P10);
    - `strict_rules::assigned_variables` (whether a variable is assigned anywhere in the function);
    - the Pest file-local configuration.

**Steps.**
1. `At`, the registry and `Report`, with `process_expr` wrapping today's `check_expr` (no behaviour change). This lands right after C3, with the statement rules (C5) as the first family.
2. One commit per rule family, each deleting the separate walks of that family.
3. `process_condition` takes over the narrowing in `check_if`, loops, ternaries, `match` and `&&`/`||`.
4. Fold `infer_expr` into the traversal; a closure's return type comes from its body's result, and the `Returns` visitor goes. Nothing re-infers a node within a pass any more, so C7's entry lists (by address, version and question, cleared per outermost expression) become `NodeTypes`: one entry per node per pass, the table record and replay keeps. Hypothetical questions write to a scratch table.
5. Record and replay, for loops and backward `goto` regions.

**Oracle.**
- O2 after every commit, the diff read.
- `--dump-types` must stay identical through steps 1, 3 and 4, which change no rule.
- For step 5, a test build also runs an observed pass from each fixpoint entry and asserts that its reports equal the replay's.

### 12.11 Union construction: n-ary, over a member index

**Now.**
- `Type::union(a, b)` (`union.rs:17`) is the only union primitive, memoized by pair. Every n-ary union is a left fold of it: `Relation::union_of` (`:37`) and about 150 `reduce`/fold sites.
- `union_types_impl` (`:45-258`) flattens both operands, then runs a pipeline of passes, each a scan with `Vec::contains` or a freshly collected side vector:
  - `true|false`;
  - extension widening;
  - `void` under `null`;
  - benevolent members;
  - literal-under-base;
  - int ranges;
  - `reduce_array_members` (`:847`);
  - `absorb_contained_members` (`:332`), relation checks of each member against each container;
  - the member-cap folds.
- So appending one member to an already-normalized union of *n* members redoes O(*n*²) checks, and building it by a fold costs O(*n*³). In this run, 38 K memo misses cost ≈11 K Ir each (`union_types_impl` 431.9 M, of which `reduce_array_members` 202.7 M and `absorb_contained_members` 77.0 M).

**Design.**
```rust
pub fn union_of(members: &[Type]) -> Type;      // the primitive; `union(a, b)` = union_of(&[a, b])
struct MemberIndex {                            // built once per union
    by_kind: [SmallVec<[u16; 4]>; KINDS],       // member positions by repr kind (bool, int literal, int range,
                                                // string literal, string, float, array, shape, object, ...)
    present: hashbrown::HashTable<u16>,         // node identity -> position, for dedupe and "is X a member"
}
```
- **One normalization pass.** Flatten once, deduplicate by node identity through `present`, and keep members in first-occurrence order, which is the order the fold produces and the order messages print. Each rule reads the index instead of scanning:
  - `true|false` looks at two kinds;
  - literal-under-base asks `present` for the base;
  - `void` asks for `null`;
  - int ranges are merged within their kind;
  - array members are reduced within the array and shape kinds;
  - absorption checks a member only against containers that can contain it (`cannot_contain`'s families first, by kind), not against every member.
- **Normalized operands.** A `Union` node's members are already normalized, so `union_of` over several unions plus extra members needs only the cross checks: each new member against the others' containers and the reverse. Merging shapes can create a member that absorbs or is absorbed by others, so after any merge the pass re-runs absorption over the affected kinds only. The invariant is a flag on the `Union` node, set when the pass builds it.
- **Callers.** The fold sites call `union_of(&[..])` with all their members at once. The memo is keyed by the member list for any length: the node addresses are hashed, and the key keeps its members alive, as the pair memo's does. The binary memo is its two-member case.

**Exactness.** Union of a set should not depend on the order or grouping of the joins. Where the old left fold gives a different result from the single pass, either the order of the joins changed a cap-fold decision or a shape merge, which is an artefact, or a rule was not confluent, which is a bug.

**Steps.**
1. `MemberIndex` and the single pass, with `union(a, b)` defined through it.
2. The normalized-operand fast path.
3. The fold sites, by module.

**Oracle.**
- A differential test over random member lists, and over every union the corpus run builds (recorded once with its operands), comparing the n-ary result with the left fold of the old binary union. The differences are listed and each classified as one of the two above before step 1 lands.
- O2.

## 13. Finishing passes and the main thread

### 13.1 F2. Dead conditional declarations; a name is changed only when its entry changes

**Now.**
- The first full run takes the incremental path, because the stub base counts as resolved. `closure_scope` (`project.rs:1117-1186`) then starts from every changed name. For each name it adds the project files that reference it and, through `stubs::stub_dependent_files`, every stub file that references it. It then adds every stub file referencing what those declare, transitively, putting all their names in scope (`:1173-1182`).
- Stub files reference only core names, so a project name reaches them only when the project declares a core name, which is what a polyfill does. Piwigo's vendor tree has four: `symfony/polyfill-php80`, `-php82`, `-php84` and `-php85`. They declare `Stringable`, `Attribute`, `ValueError`, `PhpToken`, `UnhandledMatchError`, `AllowDynamicProperties`, `SensitiveParameter`, `Random\*`, `Pdo\*`, `RoundingMode`, `Deprecated`, `NoDiscard`, `Filter\*` and others, each inside a guard such as `if (\PHP_VERSION_ID < 80000)`.
- `insert_built_decl` (`registry/index.rs:105-200`) records every declared name as changed before it decides anything (`:108-111`). That includes a conditional declaration that `get_or_insert_with` then discards because the name exists, which changes nothing.
- A conditional class colliding with a built-in stub class is merged into the stub (`:119-178`), whatever the guard. For a project requiring PHP ≥ 8.0, `PHP_VERSION_ID < 80000` can never hold: the polyfill never exists at run time, yet its members are merged into the real class and its name is marked changed. So the closure pulls in every stub file referencing `Stringable`, `Attribute`, and so on, transitively. That would account for most of the 3,949 stub classes `warm` decodes (§5); step 4 confirms it.

**Design.**
- **A declaration carries its guard, and the registry evaluates it.**
  - Today the declarations walker marks every declaration inside an `if`, a `try` or a body as conditional (`syntax/src/declarations.rs:86-128`). With P12 the parser's declaration list records the condition that guards the declaration instead: the conjunction of the enclosing branches' conditions, negated on the `else` side, taken from the parser's statement stack.
  - It also records the negation of every earlier top-level `if (c) { return …; }` without an `else`. That is the shape of every symfony polyfill bootstrap: `if (\PHP_VERSION_ID >= 80000) { return; }`, then `if (!function_exists('fdiv')) { function fdiv(…) … }`.
  - An early return guards only nested declarations. A top-level function, class or interface after it is still declared, because PHP hoists it. Checked with `php`: after `if (PHP_VERSION_ID >= 80000) { return; }`, a top-level function, class, child class and interface all exist, and a function inside an `if` does not.
  - The guard is stored with the declaration in symbolic form (`GuardExpr`). `insert_built_decl` evaluates it into `Guard { Never, Always, Maybe }` against two things the registry knows:
    - the configured PHP version range (`Registry::php_versions`, from configuration or `composer.json`): `PHP_VERSION_ID <op> literal` in either order, `version_compare(PHP_VERSION, '…', op)`, and `&&`/`||`/`!` over these;
    - the built-in stubs' availability over that range: `!class_exists('X')`, `!interface_exists`, `!trait_exists`, `!enum_exists`, `!function_exists('f')` and `!defined('C')`, where the named stub declaration is available for the whole range (`Never`), for none of it (`Always`), or for part of it (`Maybe`).

    Anything else is `Maybe`, which is today's `conditional`.
  - So a change of the configured range re-evaluates the guards without parsing again, and the cache (§14) stores guards, not answers.
- **`Never` declarations are not declarations.** They are not inserted, not merged and not recorded as changed. They stay in the AST and are checked as today. **`Always`** declarations are inserted as unconditional. **`Maybe`** keeps today's behaviour, merge included.
- **A name is changed only when its table entry changed.** `record_changed_*` moves to the end of each insert arm and runs only when the arm modified the table. The `get_or_insert_with` no-op records nothing.
- `Annotations`' `VersionCheck`, deleted with P1, computed the first of these shapes for nothing; the evaluator replaces it where it is needed.

**What changes.** With an unknown version range (no configuration, no `composer.json` constraint), every guard is `Maybe` and nothing changes. With a known one, a polyfill that cannot exist no longer alters the real class it shadows, and no longer drags the stubs that reference it into the finishing passes. This is a semantic correction, read in O2's diff.

**Steps.**
1. Guards recorded in P12's declaration list, and the evaluator, with tests over the shapes above, the early-return and hoisting cases included.
2. `Never`/`Always` in `insert_built_decl`.
3. Changed names recorded only on change.
4. A debug-only count of stub keys in `scope`/`body_scope`, by the name that pulled each in, confirms what remains.

**Oracle.** O2 (diff read); the incremental tests' affected-file sets; the stub key count before and after.

### 13.2 F1. Stub members decoded on demand (after F2)

**Design.** Built, not gated on a count: decoding members that no one reads is waste by construction. Member-level laziness is the right shape for a cache of thousands of stub classes, as the class-level laziness already in place is. Whether the finishing passes then iterate, and so decode, every member of the stub classes in their scope is F2's question, so F1 lands after F2's fix.
- The stub cache stores each class's methods as a record list: names (folded hash, offset, length) plus separately encoded `FunctionReflection` bytes.
- `ClassReflection::methods` becomes `Members<FunctionReflection>`, `enum Members<V> { Built(FoldedMap<Arc<V>>), Stored { index: Arc<StoredIndex>, slots: Box<[OnceLock<Arc<V>>]> } }`. `get` decodes one slot; iteration decodes all.
- Access to `methods` is `pub(in crate::registry)` (`class_info.rs:369`), so the change stays inside `registry`.
- It shares one stub cache format bump with C3's.
- A test-only counter of members decoded against members read stays, to show what the laziness saves.

**Oracle.** O2; the stub cache round-trip tests.

### 13.3 F3. Declarations shared structurally

**Now.**
- `end_declaration_changes` copies every class and function declaration the first time, to keep the as-built form for later incremental runs (`registry.rs:526-533`).
- `reset_scoped` clones each raw entry back into the resolved table (`registry.rs:124-125`): 137.6 M over 15,626 classes and 17.5 M over 6,730 functions.
- Elsewhere, `ClassReflection::clone` costs 53.0 M and `FunctionReflection::clone` 35.9 M.

**Design.** Sharing at every level, so a copy is pointers and a change copies only what it changes:
- `NameTable<Arc<ClassReflection>>` and `NameTable<Arc<FunctionReflection>>`.
- Inside `ClassReflection`, the member maps sit behind `Arc`: `methods: Arc<FoldedMap<Arc<FunctionReflection>>>` (C3, F1), properties and constants likewise. Each member is itself an `Arc`.
- Writers use `Arc::make_mut` at the level they change. A finishing pass that rewrites one method's return type copies that method and the class's map of pointers, not the class.
- The as-built tables (`raw`) and the resolved ones share everything not yet changed, so `end_declaration_changes`' copy and `reset_scoped`'s clones become pointer copies, and so do the clones elsewhere.
- No run-mode flag: keeping the as-built form costs pointers, so a one-shot run and an incremental session take the same path.
- **Equality by pointer first.** `changed_since` (`registry.rs:610-643`) learns which declarations the finishing passes changed by comparing every class and function in the snapshot with its resolved form, field by field: 68.7 M, serial, on the finishing thread.
  - With sharing, an entry no pass wrote is the same `Arc` as its snapshot. So `PartialEq` for the shared levels tests `Arc::ptr_eq` first, and compares fields only for the entries a pass copied, where a copy may still hold equal content.
  - The comparison runs as a parallel filter over the snapshot.
  - It stays exact: a pointer-equal entry is equal.

**Steps.**
1. Member-level `Arc`s.
2. Map-level `Arc`s.
3. Entry-level `Arc` in `NameTable`.
4. Pointer-first equality, and `changed_since` as a parallel filter.

Each step is compile-driven inside `registry`, whose fields are module-private.

**Oracle.** O2; the incremental tests, which exercise raw resets.

### 13.4 F4, F5

- F4: property-write keys `(Option<FoldedName>, Name)` (class folded, property exact), after C3.
- F5: `closure_scope` (180.1 M, two calls, serial) becomes a breadth-first search over dense ids.
  - The frontier is a `Vec<FileId>`, and the visited files a bitset over `FileId`s: ids are dense and never reused (the final plan's A3, already in).
  - The expanded names are a set of `RefKey`s (P9). Each visited file contributes the declared names its `FileState` holds.
  - Nothing hashes a path or builds a `String`.
  - The two calls (over `signature_dependents` and over `dependents`) run side by side with `rayon::join`.
  - **Oracle:** the scopes equal today's in the incremental tests and on the corpus's first run.

### 13.5 Main thread

- **Path work on the workers.** `read_and_parse_file` returns three things: the absolute path; the `PackageIndex::owner` answer, since the index is read-only once `prepare` has built it; and the path's `FileId`. Ids are assigned by position in `combined_files` before the parallel map, so `FileTable::with_paths(&[PathBuf])` builds the path-to-id map once, in parallel with parsing. `insert_parsed_file` then only merges.
- **Package owner by directory.** `PackageIndex::owner` (62.5 M) hashes every ancestor path of every file. Replace that with a per-directory memo: `owner_of_dir(d)` is the deeper of `d`'s own entries and `owner_of_dir(parent(d))`, ties broken by index, computed once per distinct directory for the path as written and for its normalized form. It is exact: a maximum over all ancestors is the maximum of the per-ancestor maxima (the final plan's B9 correction). It is built on the workers.
- **Sorted traversal.**
  - `collect_php_files` sorts each directory's entries by name bytes before recursing, so its output is already in `path_order`'s component order. `sort_paths` and its per-path `Path::components()` keys (73.6 M) are no longer needed on it.
  - The explicit `scan_files` are sorted and merged with the walker's runs by a k-way merge.
  - A debug assertion checks that the result is sorted.
  - **Oracle:** the file lists before and after, for the corpus and fixtures.
- **Size order.** `analyze_files_by_size` sorts with a key that looks the size up in a `HashMap<PathBuf, usize>` on every comparison (40.8 M). The parse results already carry each length in file order. Sort `(Reverse(len), index)` pairs, which is stable as `sort_by_key` is, and map back. The map goes.
- **`render_json`** (`output.rs:126`, 23.8 M) builds a `BTreeMap` of per-file structures keyed by each path's display string, then serializes the whole document into a `String`. Instead, it sorts the display strings and streams each file's object into a `BufWriter` over the output with `serde_json`'s serializer. The order is the `BTreeMap`'s (display string order), so the output is byte-identical.
- **Autoload parse on a worker:** §11.10.

## 14. Body summaries for every file, then a persistent per-file cache

**Now.**
- Every file keeps its AST after loading: `FileState::ast`, plus `FileBodies` (`bodies.rs:32`), a map from each function-like body's span to its statements. Readers:
  - the purity pass (`registry/purity.rs`), which walks each body for syntactic disqualifiers and symbolic calls (`Disqualifier`, `:378-508`) and resolves the calls against the registry;
  - the property-write pass (`registry/property_writes.rs`), which walks each body for writes, for calls through `$this`, `$this` properties, never-reassigned parameters, `self`/`static`/`parent`, named static methods, `new` and functions (`Scan`, `:372`), and resolves receivers against declared types;
  - `infer_global_variables_of`'s `walk_top_level` (227.5 M over every file);
  - use sites for the LSP;
  - the opt-in project rules: `architecture.rs:267-286`, `composer_hygiene.rs:277-294`, `banned.rs:477`, `mock_hygiene.rs:374`, `class_shape.rs:103`;
  - the checker's uninitialized-property dataflow, which walks into the bodies of the methods a constructor calls, in whatever file they are declared, through `Checker::bodies` (`uninitialized_dataflow.rs:741-760`; the project hands it every file's bodies, `project.rs:2031`);
  - the two declaration rebuilds, both from every affected file's tree: switching to the PHPStan base rebuilds the declarations of every loaded file (`project.rs:1055-1080`), and an incremental edit rebuilds those of every file declaring a changed name (`:1377-1412`);
  - `PestConfig::build`, over every file named `Pest.php` (`project.rs:652-657`, `pest_config.rs:222`);
  - `check_renamed_named_arguments`, which walks every file's tree only to enumerate its classes (`project.rs:2475-2484`, `renamed_named_arguments.rs:63-78`).
- A warm run repeats the whole parse phase for the 11,459 context files, up to ≈7.5 G (§7).

**Design, part 1: summaries replace stored bodies, for every file and every run.** Each consumer gets the part of a body it reads, in symbolic form, resolved against the registry by its pass as today. All the summaries are built on the parse worker by P12's one walk, not one walk each:
```rust
pub struct BodySummary {
    /// `None` when a syntactic disqualifier occurs (echo, throw, global, static, superglobals,
    /// yield, print, exit, eval, include, shell exec, class constant or static property reads,
    /// property writes, dynamic calls); else the calls whose purity decides the body's.
    pub purity: Option<Box<[PurityCall]>>,          // Function(QualifiedName) | ThisMethod(Name) | Static(StaticTarget, Name) | New(QualifiedName)
    pub writes: PropertyWriteFacts,                 // own writes, plus calls by receiver kind:
                                                    // This(Name) | ThisProp(Name, Name) | Param(u16, Name) | SelfLike(kind, Name)
                                                    // | Static(QualifiedName, Name) | New(QualifiedName) | Function(QualifiedName) | Unknown
}
pub struct FileSites { /* `infer_global_variables_of`'s per-file reads and writes */ }
```
- `FileBodies` holds `Arc<BodySummary>` instead of `Arc<Vec<Stmt>>`. `classify_body`, `Scan` and `walk_top_level` become functions over summaries. Whatever they resolve against the registry (functions by name with the namespace fallback, `$this` methods, declared property and parameter types) they still resolve in the pass, in the same order of preference.
- **`InitFlow`, for the uninitialized-property dataflow.** It is the body's control structure (blocks, branches, loops, `switch`, `try`, `return`, `throw`) holding only the events that dataflow reads: reads, writes and `unset`s of `$this` properties, and calls on `$this`, `self`, `static` and `parent` with their method names, in evaluation order. `BodySource::body` returns `&InitFlow`, and `walk_stmts` walks it for every body, the checked class's own included, so there is one walker.
  - Each node carries its completion, from C1's shared rules over syntactic facts, which replaces `stmt_definitely_terminates`.
  - The rewrite does not carry over the walker's two stated simplifications.
    - A loop now runs to a fixpoint (`uninitialized_dataflow.rs:23-37`), where today the zero-iteration state is intersected with one pass. A domain of finite property sets reaches the fixpoint in at most one pass per tracked property.
    - A `switch` case without a `break` now passes its state into the next case, as PHP does, where today every arm is an alternative (`:38-42`). `match` arms remain alternatives.

    Both bring the findings closer to what PHP does, and O2's diff is read.
  - The value-level guard limit (`:43-54`) stays: it needs the values of properties, which this domain does not track.
- **Declarations are kept, not rebuilt.** Each `FileState` keeps its `BuiltDecl`s, shared with the registry through F3's `Arc`s. Both rebuild paths re-insert them in load order instead of building them again from a tree. The cache (part 2) stores them anyway.
- **Pest facts on the worker.** For a file named `Pest.php`, the parse worker extracts its `uses(...)->in(...)` bindings into `PestFacts`, which `PestConfig::build` reads.
- **The renamed-arguments pass** enumerates classes from each `FileState`'s declared class names and reads the rest from the registry, as it does today.
- **Context files drop their AST after loading.** The opt-in project rules and the LSP's use sites read analysed files' ASTs, which stay. Where they read a context file, `Project::ast_of(path)` re-parses it on demand with a small LRU, paying only when such a rule is enabled. A test build asserts that nothing else asks for a context file's tree.
- This speeds up every run, cold ones included (no body walks in the finishing passes), and cuts the memory of a large vendor tree to its declarations and summaries.

**Design, part 2: the cache.** With no AST to store, a file's whole parse-phase product is small and serializable:
- `built_decls` (the reflections, serialized as the stub cache already does);
- declared names, duplicate sites, `FileFacts`, ignore marks;
- references (P9's `RefKey`s, stored as text and interned on load);
- body summaries, `FileSites`;
- the line index.

Storage:
- Key: xxh3-128 of the file's bytes (`twox-hash`, already in the lock file), plus the path (declarations bake it into `Origin`), plus the parse options (`is_stub`, which `parse_file` takes), plus the build's source hash (the stub cache's key).
- Store: `~/.cache/phpstan-rs/files/<build>/<aa>/<hash>.bin` (postcard), written to a temporary file and renamed into place.
- Read: the file is read as today; the hash of its bytes picks the entry, and a hit skips lexing and parsing.
- Bound: a size cap with oldest-first eviction at the end of a run.
- Analysed files are always parsed, since checking needs their AST; their declarations can come from the cache like any other file's. The LSP bypasses the cache for files it is editing.

**Steps.**
1. Purity summaries, with `classify_body` over them.
2. Property-write summaries.
3. `FileSites`.
4. `InitFlow` with completions from C1's shared rules (C1 step 6), and the dataflow over it; then the loop fixpoint and switch fall-through, one commit each.
5. Declarations kept per file; both rebuild paths re-insert them.
6. Pest facts; the renamed-arguments pass from declared names.
7. Context ASTs dropped, `ast_of` added.
8. The cache.

**Oracle.**
- Each summary: the pass's verdicts over summaries equal its verdicts over ASTs, for every body of the fixtures, vendor tree and corpus (both run in a test build). For `InitFlow`, the dataflow's findings and absorbed states.
- Kept declarations: in a test build, both rebuild paths also build from the tree and compare.
- The cache: a warm run's output equals a cold run's, byte for byte, over the corpus, the fixtures and the vendor tree; corrupted and stale entries (truncated, other build) fall back to a cold parse.
- O2 after each step.

## 15. Commit sequence and checkpoints

Each row is one or more items, whose steps are the commits listed in their sections. "CP" marks a checkpoint: the full test suites, the fixtures, O2 saved by hash, and O3 (callgrind plus `hyperfine`). Each checkpoint's deltas go in its commit message and in `WORK_STACK.md`.

| # | items | needs | oracle |
|---|---|---|---|
| 1 | P1 (module deleted; `goto` labels from the parser, backward ones marked); C6 (the PHPStan-ancestry fact, one method lookup, named arguments by scan); P8 flags and predicates; C2 steps 1-3 | — | O2, goto-label equality |
| 2 | P3 (validation at the read); P6(a) keywords; P6(b) literals; P6(c) names across files | — | O1 |
| 3 | C7 entry lists; C8 `ReprRef` | — | O2 + dump-types |
| 4 | union, n-ary (§12.11) | C8 | fold differential, O2 |
| CP1 | | | |
| 5 | C1 steps 1-5: completion, `goto`, exit and throw tails, effects | row 1 (P1's labels) | assertion list, then O2 diff read per precision rule; `goto` fixtures confirmed with `php`; each side walk against the result |
| 6 | C3 steps 1-6 | — | differential maps + O2 (diff listed) |
| 7 | C5 labels over names; §12.10 step 1 (`At`, registry, `Report`) with the statement rules as its first observer family | C1, C3, C7 | O2 (diff read) |
| 8 | P9 steps 1-2 (consumer audit, `RefKey`); F4; F5 | C3 | incremental tests, scope equality, O2 |
| 9 | C4 steps 1-5 | C3 | dump-types, O2 diff read for the key corrections and the rebind purge |
| CP2 | | | |
| 10 | P2 ignore marks, resolved in the lexer | — | `IgnoreComments` equality |
| 11 | P4 steps 1-6 | — | O1 through `LineIndex`; LSP encoding tests |
| 12 | P6(e) dispatch; P6(d) the streaming lexer | P4 | O1 |
| 13 | P5: parse errors reported, then the Pratt parser by PHP's table, `yield` by predicate | P4, P6(d) | php-parser / `php -l` generated test, corpus dual parse with the differences listed |
| CP3 | | | |
| 14 | P7 steps 1-6; P11 steps 1-3 and the byte lexer for tags; P9 step 3 (doc names from the grammar) | P4, row 8 | lowering oracle, resolution-memo equality, `DocBlock` equality |
| 15 | P12: names resolved by the parser, `ImportScope`s, references and the declaration list from the parser; the fused summary walk lands with §14 | P1, P5, P6(d), P9 (row 8 and row 14) | O1 with resolved names, reference and declaration-list equality, O2 |
| 16 | P10: generator flags, `FileFacts` from the lexer and the `GuardArgs` gate, autoload parse off the main thread with resolver fixes | P1's scope stack, C1 (loop yields) | flag and gate equality, autoload lists |
| 17 | main thread: path work and owners by directory on the workers, sorted traversal, size order, streamed JSON | — | file lists, O2 |
| CP4 | | | |
| 18 | F2 (declaration guards, changed names only on change); F1; F3 steps 1-4 | C3, P12 (row 15) | O2 diff read, stub key count, incremental tests |
| CP5 | | | |
| 19 | §12.10 steps 2-4, the remaining rule families one per commit, each deleting its separate walks (the PHPStan-API walker, member and variable usage with the rebind scan, keyword and declare rules, `Promotions`, risky tests, the magic-constant walk) | C1, C7, C4, row 7 | O2 diff read per family; old walk's output against the family's; dump-types for steps 3, 4 |
| 20 | §12.10 step 5, record and replay for loops and `goto` regions | row 19, C1 | replay equals an observed pass from the fixpoint |
| 21 | C1 step 6 (shared completion rules); §14 part 1 (summaries, `InitFlow`, kept declarations, Pest facts, context ASTs dropped), then part 2 (the cache) | P9, C3, P12, rows 16 and 18 | summary verdict equality; rebuild equality; warm = cold |
| CP6 | | | |

**Dependencies that fix the order.**
- C3 before C5, P9, F4, F5, C4, F1 and F3, which key by its names. C5's labels hold `Name`s, so they are written once, after it.
- P4 before P6(d) and P5, so the parser's hot loop is reworked once, under one oracle.
- C8 before the n-ary union, which builds through `ReprRef`.
- C1 and C7 before the traversal (§12.10), which uses statement results and node entries; its first observer family lands right after C3.
- P1 before C1, whose backward `goto` regions need its marks.
- P12 after the parser rework (P5, P6(d)) and P9's keys, and before F2 (its guards ride on P12's declaration list) and §14 (its summaries come from P12's walk).
- F3 before §14, whose kept declarations share F3's `Arc`s with the registry.
- The summaries before the cache, which stores them.

Rows that share neither files nor oracles (2 and 3, for example) can be taken in either order.
