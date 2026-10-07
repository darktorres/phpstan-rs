# Pass 2: more optimizations for the scoped Piwigo run

Builds on `PIWIGO_OPTIMIZATION_PLAN.md` and `PIWIGO_OPTIMIZATION_VALIDATION.md`. New data for this pass (all from the same 45.13 G callgrind run of `target/gate/phpstan-rs`, which is HEAD with my earlier changes in it):

- Self cost of every function with no threshold, summed over the nine threads (the first drill cut the tail at 99 %). Files in `/tmp/p2/`.
- Caller trees for threads 01, 03 and 04 with no threshold (the largest main, check and parse threads); other threads are not in the caller sums, so caller figures are lower bounds.
- Corpus facts counted directly from `/home/torres/piwigo17-frozen`.

Status words as in the validation file: VERIFIED (source read or data settles it), DATA (a measured number but I did not read the code that causes it), HYPOTHESIS (plausible, needs a measurement before any change). Savings are upper bounds and overlap.

## 0. Two facts that reframe the run

**Context files are 80 % of the parse work.** `parse_file` runs 14,251 times; `check_program` runs 2,792 times. The other 11,459 files are context (vendor, loaded only so their declarations can be resolved), and parse + declaration indexing is 47.8 % of all instructions, so context files cost roughly 38 % of the run (about 17 G) if cost is proportional to file count. Two ways to stop paying it, both large designs:

- **S1. Lazy context loading (HYPOTHESIS).** Parse a context file only when something looks up a name it declares (composer's classmap and PSR-4 maps say which file). The analysed files and the stubs would be loaded eagerly; vendor classes on demand. Needs: (a) a measurement of how many context classes are ever looked up by a Piwigo run (count registry hits on context-origin classes; the drill gives no such number), (b) proof that existence checks (`class.notFound`) and hierarchy walks give the same answers when a missing name falls back to the map, (c) deterministic emission order (lazy loading must not change which declaration wins a collision). If (a) says well under 20 % of context classes are touched, this is the largest single win available.
- **S2. A persistent cache of parsed context declarations (HYPOTHESIS).** The stub cache already stores a resolved registry in one container file keyed by a source hash. The same container could hold, per vendor file, its built declarations and dependency sets, keyed by (path, size, mtime or content hash), so a warm run reads them instead of lexing and parsing. Needs the stored format for `BuiltDecl`, `references`, duplicate-site tables and ignore comments, and an invalidation rule; it helps repeated CLI runs, not the first one.

S1 and S2 stack with every item below in the parse phase; they make B-items worth less for context files and no less for analysed files.

**The AST is walked many times per file.** Self cost of the `walk_expr`/`walk_stmt`-family functions alone, summed over visitors: **2.30 G** (5.1 % of the run), across about 25 visitors. In the parse phase, per file: `Declarations` x5 (declared_names, declared_constants, DuplicateClassSites, DuplicateFunctionSites, `build_declarations`) at 131-136 M each, plus a sixth `Declarations` in `class_shape::collect_classes` on the main thread (36.6 M self, 526 K call expressions seen), `deps::Collector` (173 M, one entry covering both runs), `anonymous_class::Lines` (130 M), `anonymous_class::Found` (130 M), `name_resolution::Resolver` (129 M), `mentions_stub_attributes::Find` (120 M), `FileBodies::of::Collect` (120 M), `annotations::Builder` (119 M). In the finishing and check phases: `global_vars::Scope` 119 M, `property_writes::Reassigned` 100 M, `Scan` 43 M, `strict_rules::Assigned` 43 M, `ClosureVariables` 41 M, `member_usage::Members` 36 M, `global_constants::Collector` 36 M, `phpstan_api::Walker` 34 M, `GuardArgs` 34 M, `Rebinds` 34 M, and smaller ones. These numbers are walk overhead only (the callbacks cost more on top, e.g. `define_call` 161 M, `add_uses` 110 M).

## 1. New items

**N1. Skip expression traversal in the declaration walks when the file cannot need it (VERIFIED by reading, DATA for size).**
- `Declarations::visit_expr` (`syntax/src/declarations.rs:126`) calls `define_call(e)` on every expression and then descends into it, so the five parse-phase walks (and the sixth on main) visit every expression of every function body to find only `define('NAME', ...)` calls and any declaration nested in an expression (a class or function declared inside a closure). `define_call` alone is 160.8 M self; 387,758 + 138,686 calls on main and 95,650 per walk per worker thread.
- Change: the parser records, per file, whether it saw a call to `define` (any case, with or without a leading `\`) and whether it saw a class or function declaration while inside a closure or arrow-function body. When neither, the `Declarations` visitor does not descend into expressions at all (statements only). Expected: most of the 5 x 131 M of expression traversal, 161 M of `define_call`, and the matching part of `collect_classes`.
- Hazards: the flag must be exact or conservative (a missed `define` loses a constant: the differential test is declared constants and names over every fixture plus the whole vendor tree, flag on versus a full walk). `define` called through a variable (`$f = 'define'`) is not a declaration today either.

**N2. Fuse the remaining walks that run over the same tree (DATA, VERIFIED for the two collectors).**
- `referenced_names` and `declaration_referenced_names` are one `Collector` with `bodies` true and false (verified in the validation pass). One walk, two sets.
- `anonymous_class::Lines` and `Found` (260 M together) only look for `new class`. The parser constructs those nodes, so it can set a per-file flag; `name_anonymous_classes` and `anonymous_class_decls` return at once without it. (This is the "bit from the parse stage" that the earlier D12 note postponed; the measurement now justifies it.)
- `mentions_stub_attributes::Find` (120 M): the parser can set a flag when it parses an attribute whose name is one of the stub attributes (the set is small and fixed).
- `FileBodies::of::Collect` (120 M): collects function bodies by span; the parser can hand the bodies over as it builds them.
- A general mechanism instead of many flags: a `Facts` struct the parser fills while building nodes (has_define_call, saw_anonymous_class, saw_stub_attribute, nested_declaration_in_closure, has_global_stmt, has_yield, ...), stored on the parse result. Each visitor then starts with `if !facts.x { return }`.
- Expected: about 1.2-1.5 G of walk overhead (parse phase), more with the callbacks; the finishing and check phase walks (`Scope`, `Reassigned`, `Scan`) can use the same facts (`has_global_stmt`, `has_property_write`).

**N3. `NameContext::add_uses` is rebuilt by every walk (DATA).** 109.6 M self, 359,926 allocations (16.4 M alloc cost, `RawTable<((UseKind, String), String)>::reserve_rehash` 5.6 M more), and `SplitInternal<char>::next_back` (rsplit on `\`) 35.6 M from the same function. Every visitor with a `NameContext` re-adds the file's `use` imports to its own table. Fusion (N2) removes most of it; otherwise compute per statement the resolved context once (`for_each_in_context` results) and share it.

**N4. `purge_stale_narrowing_for` scans every local on every assignment (DATA; fix VERIFIED sound by reasoning).**
- In thread 03: 122.1 M inclusive over 11,082 calls (11 K Ir per call), of which the persistent-map iteration `hamt::Iter::next` is 37 M and the closure 84 M self.
- Change: `Locals` keeps two monotone sets, the roots (text before the first `->` or `[`) of every non-real-variable key ever inserted, and the variables used as `[$var]` subscripts. A purge for `key` returns at once when `key` is in neither set (nothing can depend on it). The sets only grow (like the existing flag), so they are supersets and the answer is exact.
- Also: put the `'['` byte test before the substring search (the omission in my earlier change; 170 M `is_contained_in` self total).

**N5. Lexer positions on non-ASCII files (DATA).** 9.0 % of the PHP files and 22.1 % of the bytes (counted) contain non-ASCII bytes, so they take the `char_count` path of `Lexer::columns`, which validates UTF-8 per call (`from_utf8` 19.8 M from `columns`). Change: validate the file once; when valid, count characters as the bytes that are not UTF-8 continuation bytes (`(b & 0xC0) != 0x80`), which vectorizes and has no validation; keep the old path only for files that are not valid UTF-8 (identical results for valid files).

**N6. `Keyword::try_from_bytes` converts to `&str` (DATA).** It calls `from_utf8` (24.4 M) for every identifier; match on the bytes directly (lowercase compare of at most 15 bytes).

**N7. `Type::object_class_names` returns a `Vec<&Name>` (DATA).** 162 K allocations (6.8 M) in the three threads; return an iterator or a stack vector for the common one-class case.

**N8. The union memo allocates per miss (DATA).** `intern::memoized` stores a `Vec<Type>` of clones to keep the argument nodes alive: 37.7 M (`Type::union`) + 18.7 M (`without`) + 3.1 M (`intersect_types`) in the `LocalKey::with` closure, plus the key. Use a fixed `[Type; 2]` for the binary operations (`union`, `without`), a `Vec` only for the n-ary ones.

**N9. `is_plain_word` costs 81.9 M self (DATA, my own change).** It runs at the head of `parse_written_type` and `parse_type_atom`. The saving it was meant to buy (skipping `split_top_level` and the trial of shapes, numbers and quotes) is real, but the check itself is now a visible leaf. Cheaper: test the first byte and scan with a 256-entry table, or fold the check into `split_top_level`, which already walks the string. Re-measure net benefit before keeping it as is.

**N10. `looks_like_constant_name` in `reference_flags` (DATA, my own change).** 28,764 calls in thread 03 take 6.9 M in `rsplit` (and `rsplit` is the `memrchr` leaf, 149 M in all callers). A constant-like last segment must end in an upper-case letter, a digit or `_`: test the last byte first and skip `rsplit` for the usual class name ending in a lower-case letter. Exact.

**N11. Parallelize `check_class_shapes` (DATA, VERIFIED).** It is gated off unless `closed_classes` or parent-call rules are set, and ran in this corpus: 91.8 M on main, serial, walking every analysed file for classes (`collect_classes`). The per-file result is independent and sorted by path afterwards, so `par_iter` over files with an ordered collect is exact. Also eliminated by N1/N2 if `collect_classes` reuses `declared_names`'s class list.

**N12. Pass 1's A3 grows (VERIFIED by data).** `Path::to_path_buf` is the second largest allocator in these threads: 434,202 allocations (27.9 M), 404 K of them from `register_references`; `String::clone` another 1.24 M allocations (56 M) of which 404 K are the key clone already listed as A3d. File ids remove both.

**N13. Vector growth is the largest single allocation source (DATA).** `RawVecInner::finish_grow` is 1.14 M allocations (79.9 M) and 1.08 M reallocations (191 M inclusive) in the three threads. The identified owners from the first report are the AST `Vec<Stmt>` and the token vector (`grow_one` 110 M, `Vec<Type>` 614 K growths). Reserve capacity where the final length is known or cheap to estimate (`parse_program` statements from token count / 12, `ArrayItem`, call arguments).

**N14. `check_args` 84 M, `resolve_call_args` 50 M, `call_template_bindings` 46 M, `class_argument_return` 23 M, `infer_call_return_type` 23 M (DATA).** Per-call work in the checker's call path with no leaf above 1 % each. Not actionable without a caller-side count; listed so they are not mistaken for omissions.

**N15. `InferKey` cache insert + rehash 130 M (DATA).** `HashMap<InferKey, Type>::insert` 74.6 M plus `reserve_rehash` 55.9 M: the infer cache grows by doubling for each function (701,761 inserts). Reserve from the previous function's size, or reuse the map's allocation across functions (`clear()` instead of creating a new map). HYPOTHESIS on how the cache is created per function; read `cached` before changing.

## 2. Where pass 1 needs correcting after this data

- The two largest AST leaves are not in the check phase: `Declarations` x5 + `Lines` + `Found` + `FileBodies` + `Find` + `Resolver` + `Builder` = about 1.27 G of walk overhead in the parse phase alone, so N1/N2 are worth more than pass 1's B1 (about 2 G including callbacks) suggested only if the callbacks go too; the flag approach removes both.
- `Components::next` 184 M + `parse_next_component_back` 91 M + `compare_components` 181 M are more than the sort: `lexically_normalized` 31.5 M (inside `PackageIndex::owner`), `Path::_strip_prefix` 20 M and `_starts_with` 16 M (`absolute_path`) are on main too, so B9 (owner cache) should also remove the repeated `strip_prefix`/normalize per file by working on the directory.
- `Packed::get` is 113.8 M self at HEAD: the hash index I added is in use; it is not a regression, but it shows the stub-name lookups are still a visible leaf.
- The name-table lookups (323 M `ClassReflection::get`, 320 M `ClassOwners::get`, 204 M `with_key`, 180 M `get_with_name`, 158 M `Folded` `contains_key`, 84 M `contains_key`) total 1.27 G self and have no caller breakdown. A breakdown is the first step: run `callgrind_annotate --tree=caller --threshold=100` on threads 02, 05..09 as well and attribute by caller; until then the only safe change is passing `&Name` keys so the lowercase scan is skipped (about 137 M in `Name::lower` + the scan inside `with_key`).

## 3. What I would drop from pass 1

B10 (sort order), C1/C3 (prepare overlap), D4, F4 stay rejected. B3(b) stays rejected. E10 and E9 stay UNVERIFIED and should not be started before the caller breakdown above.

## 4. Order of work if it were up to me

1. Cheap and exact, no design needed: A1, A2, A3d (get_mut before clone), N10, N6, N7, N4's `'['` test, E7 (flag first), B9 (owner by directory), N11.
2. Parse-phase facts (N1, N2, N3): one design (a `Facts` struct filled by the parser), then each walk switches over; differential tests on declarations over the fixtures and the vendor tree.
3. The relation and type work (E1/E2, E4, N8, N9).
4. Measure before touching: S1's lookup count, S2's format, the name-table caller breakdown, N15, E8.
