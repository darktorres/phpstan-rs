# phpstan-rs lib-suite optimization plan (69.19 G profile)

All savings are in callgrind instructions of the 69.19 G profile. They use the skeptics' corrected figures, and ranges are low–high. The profile is a debug (opt-level 1) test build, so release gains will be smaller. Source ids are written as area/id, for example purity/F2.

## 1. DO FIRST (low risk, clear)

**S1. Purity: candidate-list fixed point plus per-class lookup in classify_purity** (product). 3.1–3.7 G.
- Merges purity/F2, purity/F1, alloc/A1, lookups/F1, lookups/F2 and memhash/F11. Skeptics gave 2.1–2.4 G for the fixed point and 1.0–1.05 G for classify_purity, with alloc/A1 at 3.7–3.8 G for both together.
- Change in purity.rs:128-147 and 168-213:
  - classify_purity binds `let info = &registry.classes[class_name]` once and iterates `info.methods.iter()`, with the `is_stub` test before `is_constructor_name`.
  - classify_purity returns the (class, method, deps) and (function, deps) entries whose classify_body result is Some. run_purity_fixed_point loops only over those.
  - That removes 3,888,171 `NameTable<ClassReflection>::get` calls (2,278,619,445), 3,516,301 method-map gets (472 M), and the key-Vec clones (178.6 M plus 29.6 M drop).
  - It also removes 1,870,865 → 179,640 gets in classify_purity::{closure#1} (1,093 M).
- Risk: a scoped entry carrying a stale `Some(deps)`, or a method copied by trait flattening/alias with `is_transitively_pure` or constructor flags already set. A skeptic flagged this as a possible divergence from the old loop. Candidates must include such entries, or those flags must be cleared on copy.
- Verify: run the old and new fixed points side by side in a test build and assert identical flag sets over the fixtures, random_edits and edits_across_many_fixtures. Then delete the old loop. Also check the fixtures, the type snapshot and the corpus gates.
- Depends on nothing.

**S2. flatten_traits: read-phase hoist and vacancy filter** (product). 1.4–1.9 G.
- Merges traits/FT1, FT2, FT3 (residual), FT5, alloc/A2, memhash/F3 and lookups/F8. alloc/A2 gives 1.8–1.9 G. FT1 and memhash/F3 give 1.2–1.3 G for the filter alone.
- Change in resolve.rs:139-260:
  - Bind `let own = &registry.classes[class_name]` once, borrow `uses` and `trait_aliases` instead of cloning them, and return None before any clone.
  - Push `(name.clone(), sig.clone())` only when `!own.methods.contains_key(name)`. Apply the same filter to properties, consts and alias targets.
  - Replace `own_method_names` with `own.methods.contains_key`.
  - Keep the Entry::Vacant apply loop as the final arbiter. Keep the abstract/static set computation outside the filter.
- Profile basis: 1,312,143 clones (1,475 M) and 880,032 drops (390 M). 67% of clones are discarded, and the growth cost is `grow_one` 549 M.
- Risk: an added `contains_key` per trait method (about 1.3 M lookups), which the skeptics put at 80–260 M. Do not filter the abstract/static bookkeeping. Pre-sizing the Vec is moot once the filter is in.
- Verify: fixtures, trait fixtures, and the incremental tests.
- The scoped_class_names part of FT3 is counted in S11, not here.

**S3. Scope::from_keys in closure_scope** (product). 0.40–0.45 G.
- Merges scope/F2, memhash/F8 and the (a) part of alloc/A7. Scope::of costs 447,384,585 inclusive.
- Change: add `Scope::from_keys(FxHashSet<String>, FxHashSet<String>)` taking the already-lowercase sets by value, with a debug_assert. Keep Scope::of for the other caller.
- Risk: a non-lowercase seed silently misses in has_class. Verify with the differential tests.

**S4. Lazy tentative-return probe in flatten_phpdoc_inheritance_one_pass** (product). 0.90–0.92 G.
- Source: phpdoc/F1 (lookups/F3(c) is the same change).
- Change at resolve.rs:1213-1218: write `needs_return && (own_doc || (sig.return_type_native.is_mixed() && !ancestor.is_mixed() && built_in_stub_classes.contains_key(&owner)))`.
- Profile basis: `contains_key` 1,004,980x costs 935.9 M, and it is true 99.98% of the time. The cheap tests must come first. A skeptic warned that `needs_return` alone is not enough, because the `is_mixed` ordering is what removes the probes.
- Keep the constructor-branch probe (4,675x) as it is. No memo is needed.
- Risk: none for output.

**S5. Per-class hoist in the phpdoc pass** (product). 0.6–0.9 G.
- Merges phpdoc/F2, phpdoc/F3 and hierarchy/F1.
- Change at resolve.rs:1076-1090:
  - Fetch `ancestor_method_owner_by_class.get(class_name)` lazily, at the first method that passes the needs_param/needs_return gate.
  - Return early on a None or empty map.
  - Add `lookup_ancestor_method_in(owners, method)` with exact-key `.get`, and keep the old wrapper for the checker, purity and property_writes callers.
  - Keep a per-visit, per-task owner→&ClassReflection cache, with a cap.
- Profile basis: the table get is 903,997,853 over 1,499,915 calls, and `classes.get(owner)` is 478 M over 1,005,453 calls. There are only 179,017 class visits.
- Skeptic ranges: F2 0.6–0.78 G, F3 0.2–0.25 G, hierarchy/F1 0.55–0.9 G (this includes the owner-class part).
- Risk: an eager hoist adds gets for classes with no candidate methods. Do not cache across threads or passes. No case folding in the helper.
- Dependency: write it in the shape S13 needs (fetch each parent's own map once per class), to avoid a rewrite.

**S6. Keyword type-hint cache** (product). 0.53–0.55 G.
- Merges parse/P1, declbuild/D1 and the keyword part of types/T4. T4's lock-based memo loses to this table, because the table needs no locks.
- Change at parse.rs:804: `static [OnceLock<Type>; 17]` indexed by NativeType, with an exhaustive match and no wildcard arm, filled by `parse_type_string` of the canonical spelling.
- Profile basis: `parse_type_string` 548.7 M for 111,196 calls, plus 78.2 M for another 16,916 calls.
- Verify with a unit test that the cache equals `parse_type_string(written)` for all 17 kinds and for upper/mixed-case spellings.
- Risk: a synthetic Keyword whose `written` is not a spelling of its kind. The only one, the synthetic void at phpdoc_type_compat.rs:302, is consistent.
- Not counted: the doc-type path of T4 (437.7 M, hit rate unknown).

**S7. contains_deferred as an O(1) node flag HAS_DEFERRED** (product). 0.58–0.63 G.
- Merges typeres/TR2 and types/T2.
- Change: add bit 8 or 16 in `reference_flags` for ValueOf, KeyOf, OffsetAccess and ClassConstant, ORed with the children's flags. `contains_deferred` becomes a flag read.
- Profile basis: 692,310,988 inclusive over about 5.0 M calls.
- Verify with a debug_assert over the fixtures comparing the flag with the old recursive definition.
- The u8 has 3 bits in use. S7, S19 and T9(d,e) share the remaining budget.

**S8. check_unknown_phpstan_tags_on: parse once and guard** (product). 0.30–0.33 G.
- Merges checkclass/F1a, check/C3 and declbuild/D2. D2 is only the see-target part, 0.2 G.
- Change at oop.rs:726-770:
  - Call `docblock::parse(doc)` once, pass `&DocBlock` to the two helpers, and keep a `&str` wrapper for stmt.rs:134.
  - Return early if `!doc.contains('@')`.
  - Call unknown_phpstan_tags only if `doc.contains("@phpstan-")`.
  - Optionally add a names-only `grammar::tag_names` that copies grammar::parse's line rules exactly: first line pushed, empty name skipped, `//` cut before the `@` test.
- Profile basis: the three rows total 549 M (see-parse 203.4 M, invalid-values parse 208.7 M, unknown_phpstan_tags 130.5 M).
- Verify with a differential test of tag_names against grammar::parse names (corpus: `//`, `*`-only lines, CRLF, an empty `@`). Keep the report order: unknown tags, then invalid values, then see targets.

**S9. reconcile_with_native single-member path** (product). 0.12–0.17 G.
- Merges phpdoc/F5 and types/T6.
- Change at decide.rs:13-17: use `match self.repr() { Union(m) => m.clone(), _ => vec![self] }`. Replace `*native_ty == Type::void()` with a `matches!` on the Repr (decide.rs:207).
- Return `self` when a union keeps the same members, compared by ptr_eq after backfill. For a Union native, fall through to the general path.
- Profile basis: `into_repr` 43.1 M plus `Type::new` 116.5 M, over 268 K calls.
- Risk: low. The skeptics trimmed the claimed Vec savings.

**S10. Small safe batch** (product, plus one test-only item). 1.1–1.3 G in total (low 1.09, high 1.30).
- Lexer column counting skips UTF-8 validation when the source is ASCII (parse/P5): 0.09–0.095 G.
- Keyword hints do not intern a dropped Name (parse/P4): 0.045 G.
- Prefix tests without to_ascii_lowercase (memhash/F13): 0.14–0.15 G.
- `unwrap_or_else(Type::mixed)` in declared_type (declbuild/D11): 0.02–0.03 G.
- Constants served from static slots (types/T1): 0.20–0.26 G.
- Static probe types (types/T8 and relation/F7): 0.055–0.07 G.
- types/T9: 0.05–0.065 G.
- Cross-family and mixed/never early-outs in is_super_type_of (relation/F2, F3): 0.04–0.06 G and 0.03 G.
- relation/F9: 0.04–0.06 G.
- relation/F4: 0.02 G. Check ClassHierarchy for side effects before applying.
- resolve_pseudo_at skips `classes.get` when there is no `parent` (checkclass/F3): 0.05–0.065 G.
- purge_stale needle (check/C10 and checkclass/F7): 0.02–0.025 G.
- mentions_any (check/C9): 0.04–0.05 G.
- Lexer line marks (parse/P6): 0.04–0.045 G.
- Anonymous-class walk (declbuild/D12): 0.04–0.05 G.
- class_extends_implements_closure takes `&Name` (scope/F10): 0.04 G.
- affected_files moves its key instead of cloning (stubrebuild/F7): 0.03–0.038 G.
- LSP resolve_import without allocation (lsp/LSP-6): 0.015–0.02 G.
- Origin equality (memhash/F16): 0.03 G.
- response_result returns a reference (lsp/LSP-5): 0.055 G, test-only.
- Risk: each item is local. Verify with the types-crate unit tests, the fixtures and the type snapshot.
- The u8 flag budget is shared with S7 and S19. The statics must use an exhaustive match for AccessoryKind. Keep a debug_assert in the early-outs.

**S11. LSP-2: build the Response directly** (product). 0.159–0.16 G.
- Add an `ok_response(id, Value)` helper and use it in every handle_request arm that already holds a Value.
- Profile basis: `new_ok::<Value>` costs 158,963,324 for the 6 completion calls.
- Risk: none.

**S12. Test-only: load checker fixtures once per process** (alloc/T1, tests/T1). 0.58–0.60 G.
- Replace `fixture_sources` with `static LazyLock<Vec<String>>`. The selecting tests clone only the elements they use.
- Profile basis: 690.7 M for 8 calls, of which 7 are removed.
- Risk: none. There is no saving under a per-test-process runner such as nextest.

**S12b. Test-only: generated_stub_coverage tests use named lookups** (decode/D2). 0.5–0.55 G.
- This drops to about 0.19–0.25 G (the clone part) if S19 (decode sharing) is done first.
- Keep one test that decodes everything, so stored stub entries stay covered.
- The name sets are lowercased keys, so the assertions must be lowercased too.

## 2. DO NEXT (medium risk or churn)

**S13. Drop the two ancestor owner tables** (hierarchy/F2, owners/MO-1, MO-2). 1.6–1.8 G.
- hierarchy/F2 skeptics gave 1.6 G. MO-1 at 0.58 G plus MO-2 at 1.1–1.2 G gives 1.7–1.8 G.
- Change: delete `ancestor_method_owner_by_class` and `ancestor_property_owner_by_class` (registry.rs:323,327,524,526,693; stub_cache_round_trip.rs:95-99; hierarchy.rs:387-465).
- Answer the lookups as: first parent in `extends` (+ `implements` for methods) whose `method_owner_by_class` / `property_owner_by_class` map has the exact name.
- Profile basis:
  - The merge rows are 1,126,586,660 plus 70,974,167.
  - The share of the insert rows is 2/5 of 1,625,398,542.
  - The remove rows are 59.1 M plus 58.4 M.
- Needs S5 first or in the same commit. Without the hoist, k parent lookups per method can lose up to about 1 G.
- Risk:
  - The stored-registry layout changes, so the version needs a bump and the round-trip test needs an edit.
  - Out-of-scope staleness: today's tables could be stale for out-of-scope classes, while the derived lookup reads current parents.
  - Interface-heavy classes cost more reads.
  - Nothing may mutate extends/implements after precompute (grep shows none).
- Verify: the 3,807 fixtures, the type snapshot, the corpus, random_edits, edits_across_many_fixtures and property_overrides.

**S14. Snapshot by detach instead of deep clone** (bookkeeping/A1, alloc/A5). 2.0–3.7 G. Counted range is 2.0–3.7 G.
- bookkeeping/A1 skeptics gave 3.3–3.7 G. The 2.0 G floor is alloc/A5's high estimate, because A5 clones base-held entries.
- Change:
  - Add `NameTable::detach` returning Own(V) moved out, or Base(i) read in place through the snapshot's held Arc<Base>.
  - Build ResolvedSnapshot inside end_declaration_changes, after the swap-back, from values detached while resetting `scope`.
  - Keep cloning the body_scope-only keys (627) and keep the property_writes/global_variables clones.
  - Hold the keys without String::clone (29.9 M plus about 44 M memcpy).
- Profile basis:
  - `Option<&ClassReflection>::cloned` 2,949,506,713 and the function clones 101.6 M.
  - The snapshot's drop_glue 1,162,825,791.
- Risk:
  - The split of own-held versus base-held entries (forget drops 44,535 against 168,392 Some entries) is not read exactly. A debug counter is cheap and should come before the work.
  - end_declaration_changes gets a new signature (one caller).
  - Snapshot construction must stay after the swap-back and before the passes.
  - Keep scope a subset of body_scope.
  - Peak memory is slightly higher.
- Alternative: memhash/F7 (Arc inside NameTable, 2.0–3.0 G). It only moves the clone into the first `make_mut` of the mutating passes, and it touches about 62 mutation sites. A14 is the winner. They are not additive.

**S15. Compute the scoped class-name list once per run, passed explicitly** (phpdoc/F7, alloc/A3, scope/F8 part, traits). 1.4–1.55 G.
- Skeptic ranges: F7 1.4–1.55 G, A3 1.4–1.45 G, scope/F8 1.5 G.
- Change: scoped_class_names is built 12,874 times, about 11 per finish. Compute it once for scope and once for body_scope in run_finishing_passes, and pass the list to the passes as `&[Name]`. flatten_traits' `max_iterations` takes `.len()` from the same list.
- Profile basis: 1,938,646,636 inclusive. Callers: flatten_traits 505.5 M, purity 318.9 M, deferred_value_of 318.4 M, and five passes at 159 M each.
- Do not store a registry cache invalidated only in set_scope. The dropped lookups/F4 refutation shows two paths assign `self.scope` directly: hierarchy.rs:509-511 (precompute_member_owners_after_mixins, with a narrow scope) and property_writes.rs:144. Those would serve a stale or full-scope list.
- The nested mixin sub-pass must keep its own narrow-scope list.
- The list must be an owned Vec or Arc, because passes mutate classes. Class membership and spelling must not change between canonicalize_class_name_case and the end of the passes. Verify that resolve_deferred_mixins, inherit_property_hooks and apply_assertion_table add or remove no class.
- Not counted: scoped_removed_class_keys (278.9 M) and the skip of absent keys in scoped_classes (about 0.26 G, uncertain). Both depend on a version-counter design.

**S16. Typeres: HAS_CONSTANT_NAME node flag** (typeres/TR1, types/T3). 1.0–1.9 G.
- TR1 skeptics gave 1.8–1.9 G. Dropped types/T3 found only about 1.0 G, because the attributable self rows are about 1.1–1.3 G.
- Change:
  - Add a flag for Object/EnumCase names whose last segment is [A-Z_][A-Z0-9_]*, ORed with the children's flags.
  - Test it first in resolve_constant_names and fixed_sig.
  - Probe global_constants before lookup_class in constant_named_value.
- Profile basis: mentions_constant_name 2,271,846,752 inclusive, constant_named_value 1,155,205,811 over 3,056,709 nodes.
- The flag must mirror `looks_constant` bit for bit, including the EnumCase arm and an empty segment. All-caps class names (A, B, PDO) stay flagged and still take the exact path.
- Measure first: count the flagged share. About 865 M of constant_named_value's inclusive cost is not attributed to a callee in the data.
- Verify with a unit test: set for FTP_ASCII, `\Ns\FOO`, `_X` and EnumCase; unset for Foo and A_b.

**S17. Packed hash index** (scope/F3, memhash/F5). 1.0–1.2 G.
- Skeptic ranges: scope/F3 1.0–1.2 G, memhash/F5 1.2 G.
- Change: build an open-addressing index (places = next_pow2(2n)) lazily in memory with OnceLock, or store it in a section. The format is unchanged if it is lazy. Guard empty maps so there is no zero-place underflow, and verify the key bytes, not only the hash.
- Profile basis: `Packed::get` 1,951,139,192 inclusive, 1,768 per get over about 987 K production gets.
- Test: add a test that checks every key through the index and that misses terminate. This also shrinks the test-side share (162 M).
- Risk: all six Packed maps carry the index, so build it in warm_index. A duplicate-key file must keep the lowest position, to match the bisection.

**S18. Table key trait for Name and pre-folded keys** (lookups/F5). 1.0–1.5 G.
- Skeptic ranges: 1.2–1.5 G. memhash/F1 and F2 were dropped, because the Name::lower and memcmp rows are inside the probe and cannot be removed by a Name key.
- Change: make `get`, `get_with_name`, `contains_key`, `get_mut`, `take` and `remove` generic over a TableKey. A Name key uses `lower()` directly. Add `_folded` variants for scope keys with a debug_assert. Use a SWAR check for the uppercase scan on the remaining `&str` callers.
- Profile basis: self rows of about 7.9 G across the lookups. The figure is computed on the calls left after S1, S5 and S15, so it shrinks with other call removals.
- Risk: type inference for `&&Name`, `&String` and `&Cow`; a wrongly assumed lowercase key misses silently. Keep `get_mut` going through `own()`.

**S19. Base::find folded compare** (lookups/F6, memhash/F6 (a,b)). 0.5–0.6 G.
- Change: add `Keys::name_range(i)` that decodes only the name offset and length, hoist `self.len()`, and compare folded text eight bytes at a time.
- Profile basis: Base::find self 2,808,733,728 over 7,181,525 calls.
- Test: add a test of the chunk equality against `cmp_folded` for lengths 0–17, uppercase and bytes ≥ 0x80.
- Not counted: the single-hash change (memhash/F6 (c), 0.3–0.4 G), which needs the hashbrown `HashTable` and changes name_table.rs's Hash/Borrow arrangement.

**S20. hierarchy_levels and backfill apply** (phpdoc/F6). 0.20–0.25 G.
- Parts: reuse the fetched `info` on the stack entry (about 111 M), group backfills per class (about 70–105 M), and index the bindings OnceLock slots by position (small, with churn).
- Do (a) and (b) first. (c) is the churn item that makes hierarchy_levels return indices.
- Risk: keep `changed` set on any successful write. `get_mut` on a base entry still clones it once per class.

**S21. Own-owner BFS and ancestor walks** (hierarchy/F5, owners/MO-5, MO-6, alloc/A9, phpdoc/F10 part). 0.4–0.6 G.
- Skeptic ranges: hierarchy/F5 0.4–0.45 G, MO-5 0.2 G, MO-6 0.2 G, A9 0.2–0.3 G, scope/F10 0.04 G (counted in S10).
- Parts:
  - `result` as the visited set (class_extends_implements_closure, all_ancestor_names).
  - Pass `&Name` so there is no `Name::from(&str)` (47.4 M).
  - Replace the HashSet/VecDeque with a Vec and a linear `seen`, with a threshold fallback to a set.
  - Use `entry` instead of `contains_key` plus `insert`. The key clone cost on 1.63 M hits eats part of the saving.
- Risk:
  - Stack pop order and set iteration order change. Check consumers (`precompute_ancestor_names`, `check_require_tags`) for order dependence.
  - Passing the first node from scoped_classes() may move first-read decode into a sequential call.
- Owner-map presizing is NOT in this item (see section 3), because it changes iteration order.

**S22. precompute_ancestor_names: skip unchanged classes** (hierarchy/F6, scope/F6). 0.35–0.5 G.
- hierarchy/F6 wins over scope/F6 (0.9 G), because scope/F6's 97% "unchanged" figure is assumed. The profile shows only about 88 K of 179 K main-loop classes with an existing record (167,765 takes minus 79,766 removals). is_all runs have no records and skip nothing.
- Change: compare the stored declared spelling, the supertypes and the traits; `continue` when equal. Read the record by reference and `remove` instead of `take`. Compute supertypes in the parallel stage.
- Verify with random_edits and edits_across_many_fixtures. Check checker.rs:1338 and native_type_hints.rs:170/209 for order dependence on subtype_children and trait_users set order.

**S23. resolve_class_constant_references cleanup only for classes that had a deferred const** (typeres/TR4). 0.15–0.2 G.
- Record the class names in round 0 and run the cleanup loop over only those. With the S7 flag, the round-0 test is O(1). The 0.15 G figure avoids double counting with S15.

**S24. Empty-Vec clone guard in reflection structs** (alloc/A6, memhash/F4, declbuild/D8). 0.3–0.55 G after S2 and S14.
- Standalone this is 1.1–2.3 G: memhash/F4 1.1–1.4 G, A6 2.0 G, D8 2.3–2.6 G.
- Residual after S2 and S14, derived: FunctionReflection clones are 4,459,572, of which S2 removes about 880 K and S14 removes the class-clone share. The RawTable clone path is 3,045,003 clones. Roughly 22–25% of the clones remain, so apply that share to the standalone range.
- Change: a transparent `Seq<T>(Vec<T>)` with `#[serde(transparent)]` and an empty-Vec fast Clone, or a hand-written Clone with a destructuring test.
- The postcard layout must stay unchanged. Drop glue (2.47 G in FunctionReflection) is not addressed.
- Sequence this after S2 and S14, and measure.

**S25. Stub-dependents and declaration rebuild** (scope/F9, stubrebuild/F1; stubrebuild/F2, declbuild/D6, parse/P13). 0.25–0.28 G. The two groups total 0.17–0.18 G and 0.08–0.10 G.
- Resumable affected_files walk: run_finishing_passes returns the finished walk's affected set. It must not leak the global-variable keys into `stale`.
- rebuild_declarations keeps only the declarations in class_keys/function_keys/constant_keys, and skips anonymous-class decls for non-pending files.
- Risk: keep the exact lowercasing rule; the Define arm uses `define_constant`'s name; pending files keep everything.

**S26. Stub names are already keys (fallback)** (scope/F5, scope/F9). 0.25–0.28 G.
- Add a second `reached(key)` entry for stub names. It is subsumed by S32 and not additive with it.

**S27. Lazy diagnostic subjects and messages** (checkclass/F2, check/C7, check/C6). 0.12–0.29 G.
- C7 alone is about 0.12 G. checkclass/F2 skeptics gave 0.28–0.29 G, which includes C6's trait-use early-out (0.08 G). A10/F15 were dropped as misread.
- Change: Display structs for subjects and labels, or an early-out; build `location`/`label` only when `tag_types` is non-empty.
- Risk: the `label` signature change touches about 12 callees, and borrows must not span `report(&mut self)`.

**S28. Per-class ClassReflection reference in check_program** (check/C8, checkclass/F4). 0.11–0.15 G.
- Prefer passing `&'a ClassReflection` from the class arm over the single-entry RefCell memo. About 28 lookups at about 6.06 M per site are removed.
- Preserve the registry_matches gate for conditional duplicates and the early returns on a missing class.

**S29. Interner hit path** (types/T7). 0.11–0.12 G.
- Retain only on miss. Leave the +8 bytes per Name precomputed hash out unless measured; it is the weakest part (about 25 M).

**S30. Origin decoded as an index** (decode/D3). 0.13 G.
- Store the Builtin/Phpstan index into the embedded tables. Bump the container VERSION, bound-check the index, and add a uniqueness test of the path table.

## 3. NEEDS DESIGN OR MEASUREMENT FIRST (upside, not in the firm total)

**S31. Docblock parse memo (after S8)** (check/C1, C2, checkclass/F1b, declbuild/D3). 0.5–0.55 G.
- Skeptics' range across the findings is 0.5–0.85 G.
- Cache docblock::parse and grammar::parse by exact text, bounded. Count hits and misses first.
- A thread-local cache dies with each test thread, because libtest gives each test its own thread. A global sharded cache has more reuse but contention.
- Sites that move fields out must clone only the field, not the whole DocBlock.
- Keep the uncached `parse` for the grammar tests.

**S32. Stub-file reachability bitset** (scope/F4). 0.65–1.0 G marginal after S17 and S26.
- Standalone it is 1.6–2.0 G, and F3 and F4 together total 1.9–2.5 G. The marginal range is derived by subtracting S17 (1.0–1.2 G) and S26 (0.25–0.28 G).
- A lazily computed in-memory closure avoids a format bump.
- Risk: affected_files must still probe both `dependents` and `signature_dependents` for project-declared names. A stub name must not block a later full-path pop.

**S33. Decode sharing** (decode/D1). 0–0.75 G after S12b.
- Standalone it is 1.2–1.25 G (classes about 1.1 G plus functions about 0.37 G).
- Needs a process-wide content-keyed cache for ClassReflection/FunctionReflection, `Arc<V>` slots, and a memcmp on hit.
- Hash plus compare per hit is more than the proposal's 1 K, and the cache retains whole source buffers unless entries are copied.
- Test and product benefit differ: it shows up mostly where stub-data, raw and resolved tables are decoded in one process.

**S34. One owner table instead of three** (owners/MO-3, hierarchy/F3). 0.5–0.7 G.
- Only valid with S13 done. A merged value must stay lazily decodable per field, or the first read of a stored class decodes all three maps.

**S35. excluding_native keep-decision memo** (relation/F1, typeres/TR5, relation/F10). 0.3–0.5 G.
- Per-pass memo keyed by interned node pairs. The relation/F1 skeptics gave 0.3–0.4 G. TR5 at 0.2–0.28 G overlaps with it.
- Count distinct pairs per pass first.
- Key only on pairs without HAS_SELF/STATIC. is_null() replacement needs a debug_assert over the fixtures.
- Lazy `kept` (A11 and memhash/F14 were dropped): the profile shows only 1,356 `kept.push` calls, so it saves almost nothing.

**S36. map_children returns self when no child changed** (types/T5). 0.15–0.25 G.
- The unchanged fraction is unmeasured. Skeptics saw 0.15 G at 40% unchanged. Union rebuilding must stay. Do not add the HAS_TEMPLATE early-out (see section 4).

**S37. Test-only: oracle parse memo** (parse/P3, tests/T2). 1.0 G.
- Skeptics gave 1.0 G against the proposer's 1.3 G. The clone cost of ParsedFile and BuiltDecl is the one unmeasured input. It needs derive(Clone) under cfg(test) on ParsedFile, BuiltDecl and the site structs. Cache Err results too. Memoise per group, not process-wide. Prototype one group first.

**S38. Switch to the PHPStan base** (parse/P2 test-only, declbuild/D7 product). 0–0.8 G, not additive.
- The 741 M switch_to_phpstan_base rebuild closure is measured.
- It is contested which tests trigger it. P2 says 271 `switch_base` calls (192 plus 79) with about 27 files per switch. stubrebuild/F3 (dropped) says 79 switches at about 93 files each. Count which tests call it before touching the oracle.
- If it comes from the differential tests, P2 is a cheap test-only change using `expect_files`. Keep a dedicated per-file-load equivalence test.
- D7 (retaining BuiltDecls until finish, 0.3–0.6 G) adds clone cost on every pre-finish load.

**S39. Lazy ancestor_bindings** (hierarchy/F4). 0.25–0.3 G.
- Pass a lazy accessor. The skeptics note that method-scoped templates in ancestor types (common in stubs) still trigger the build. The avoidable share of the 818,977,829 is unmeasured.

**S40. Owner-map presizing** (alloc/A8, memhash/F10). 0–0.3 G.
- Changing capacity or clone-from-parent changes iteration order. `lookup_method` (hierarchy.rs:552-554) and `lookup_method_canonical_name` (563-570) pick the first case-insensitive match in iteration order, and completion order depends on it. This is the reason owners/MO-4 was dropped. Per-node `reserve` is mostly ineffective, because the BFS map grows to the whole chain. The merge clone disappears with S13.
- Do it only after a canonical-order decision, or with an exact count.

**S41. Low-confidence items:** plain-word fast path in the type parser (declbuild/D9, 0.07–0.08 G); docblock scratch-merge (declbuild/D10, 0.05–0.12 G); cached is_* probe results on the node (relation/F11, 0.05–0.06 G); stub-data in place (decode/D6, 0.06 G).

## 4. REJECTED, with reasons worth remembering

- **Skip substitution or settling when there is no template** (phpdoc/F4 and F8, check/C5, checkclass/F5). `replace_templates` and `substitute` are not the identity on template-free types: they end in `late::settle`, which rewrites KeyOf, OffsetAccess and ValueOf with no template present. Any "no template" flag must also cover those, and then the skip rate falls. `infer_expr` also records dependencies (`record_member_ref`), so skipping `call_template_bindings` can drop incremental records.
- **Arc<FunctionReflection>** (traits/FT6, alloc/A4, decode/D7, bookkeeping/A2). The deep copy moves to `Arc::make_mut` in the mutating passes (flatten_phpdoc_inheritance, purity, assertion pass), so the net is near zero or negative.
- **Early-cutoff reset in the closure** (bookkeeping/A2). Cycles in the purity, phpdoc and mixin fixed points need the whole strongly connected component reset. Cutting off at "own entry unchanged" leaves stale self-justifying values (example: mutually recursive f and g after f loses its `echo`).
- **Hash fingerprint instead of old value** (bookkeeping/A6). Equal ClassReflections can encode to different bytes, because the HashMap fields serialize in iteration order. Also the snapshot clone needs the decoded value anyway.
- **Hash index on the in-memory Base** (owners/MO-9). The 2.13 G in `Base::find` is the Stored base, which is already hash-indexed.
- **Reuse rather than re-derive owner maps** (MO-10, MO-11, MO-8, hierarchy/F7, hierarchy/F8). MO-11 misread a 685,375 instruction cost as a count. F8 would break the incremental indexes that `precompute_ancestor_names` writes eagerly, and the passes read stale tables at fixed points. MO-8 misread which forgets reach the base.
- **Dictionary-encoded Name/Type per file** (decode/D4). Nested `Lazy` values decode later, outside the table context, and would misread indices. **Compact wire form** (decode/D5): the bool decode is about 18 instructions each, not 100.
- **Diagnostic walk merging** (check/C12, checkclass/F6). The exists check runs on `checked` (native plus PHPDoc) and the trait check on the native type only, so merging changes output and report order. **AST pre-walk fusion** (check/C11, checkclass/F9): `check_statement_keywords` emits diagnostics and the walks scope differently.
- **Own-declaration sig cache** (check/C4, declbuild/D5, checkclass/F10). The cause of the 79% rebuild is unknown, `remove_declarations` removes by name (stale entries), and the likely cause is test registries that never indexed the file.
- **Skip `scope.is_all`-style or lazy ancestor closures, per-pass fusion, pre-filter lists** (typeres/TR6, TR7, scope/F7, F11, F12; bookkeeping/A3, A4, A5, A7). TR7's hooks predicate would miss trait-supplied properties. scope/F12 has no number and spans (MemberSpans) make "identical built form" rare. A5's borrowed keys do not compile, because `body_scope` is moved into `set_scope`.
- **Cached scoped-name list invalidated only in set_scope** (lookups/F4, typeres/TR3). Stale at hierarchy.rs:509-511 and property_writes.rs:144. The safe form is S15.
- **Mirror/shrink the differential tests** (tests/T3, T4, T6, owners/MO-12). They cut coverage, or the evidence for a saving is only a proportional guess.
- Dropped for a misread or non-existent profile row, with small or unsupported savings: owners/MO-4; types/T3 (folded into S16 at a lower range); declbuild/D4, D13, D14; parse/P7–P12, P14–P16; alloc/A7, A10–A13; lookups/F7, F10; memhash/F1, F2, F9, F12, F14, F15; stubrebuild/F3–F6; lsp/LSP-1, 3, 4, 7, 8, 9; relation/F5, F6, F8, F12, F13; checkclass/F8, F9.
- **Sound, but dropped on the number rather than on correctness** (not counted): scope/F1 (merge the body-scope walk with the signature walk; the skeptic's own correction was about 1.3 G); owners/MO-7 (about 0.25–0.28 G). Both could be re-proposed with corrected figures.

## 5. Totals

| Group | Saving (G) | Share of 69.19 G |
|---|---|---|
| Do first (S1–S12b) | 10.3–12.2 | 14.8–17.6% |
| Do next (S13–S30) | 10.9–15.6 | 15.7–22.5% |
| **Firm total** | **21.1–27.8** | **30.6–40.1%** |
| Needs design or measurement (S31–S41) | 3.8–7.1 | 5.4–10.2% |
| Firm plus upside | 24.9–34.8 | 36–50% |

- The firm total counts S1–S30 after de-duplication. Within it, the folded-lookup and Base::find items (S18, S19) are computed from call counts that other items shrink. Discounting them entirely gives 19.6–25.7 G (28–37%). The realistic expectation is the lower half of the firm range.
- The 50% upper end of the combined range is not a realistic target.
- Test-only savings are small: S12 (0.58–0.60 G), S12b (0.5–0.55 G) and LSP-5 (0.055 G) give about 1.1–1.2 G of the firm total. The upside adds 1.0 G (S37) plus up to 0.8 G (S38), so test-only totals about 2.1–3.0 G at most. Everything else is product code, but that code is measured on a suite dominated by repeated finishes: run_finishing_passes ran 1,146 times and roughly 675 of those are the every-fixture differential test. A single real project run gets a smaller absolute gain.
- Nothing kept shrinks the every-fixture differential test's finishing count. The only candidates that would (T3, T6, MO-12) cut coverage and were rejected.

## 6. Unexplained or unverified

- Class-clone overlap: the profile reports `Option<&ClassReflection>::cloned` 168,392x (2.95 G), `end_declaration_changes` clones 178,709x (2.55 G), and 200,596 `ClassReflection::clone` calls in total. These do not reconcile. S14 removes the snapshot clone only. `end_declaration_changes` still clones scope entries from raw. How much of S24's residual and of S14's saving overlaps is not quantified. Likewise, alloc/A5 saw 168,392 snapshot against 178,709 inserts.
- Own-versus-base split for S14 is unread. The own fraction determines whether the 3.3–3.7 G or the 2.0 G end holds.
- Hit rates are unmeasured for the docblock memo (S31), the excluding_native memo (S35), the types parse memo, the oracle parse memo's clone cost (S37), the map_children unchanged fraction (S36), the flagged-constant share (S16) and the lazy ancestor bindings (S39).
- The number of `switch_to_phpstan_base` switches is contradictory in the data (79 against 271) and so is which tests trigger them (S38).
- The share of constant_named_value's 1,155 M inclusive that is not attributed to a callee (about 865 M) is unexplained.
- Regions not covered by any kept finding:
  - finish_declarations has about 0.9 G of its 8.79 G unattributed.
  - check_program has about 1.24 G and check_function_impl about 1.0 G unitemised.
  - from_decl and from_def hold about 1.5 G in their own or inlined lines (declbuild/D4 was dropped).
  - The stub rebuild rows (generated_stub_classes_named 390 M, the five PHPStan-stub syncs 480 M) have no conclusion.
  - One LSP test costs 773 M; the LSP changes kept here are small.
  - Other callers of `is_super_type_of` hold about 490 M.
  - Project drop (1.74 G) and the proptest random_edits test (1.62 G) are unaddressed.
- Coverage gap: the every-fixture test's doc comment says every fixture takes part, but each group of 50 loads 25 and replaces with 4. About 1,567 of the 3,734 checker fixtures (42%) never take part. Covering them all with the current shape would cost +72% on that test. This is a test-design decision, not a saving.
- The flag budget on the node's u8 flags is shared by S7, S16, T9(d,e) and S10. They need coordinated bit assignments (3 of 8 bits in use today).
- Owner-map iteration order affects `lookup_method`, `lookup_method_canonical_name` and completion. Any change to map construction (S21, S40) needs a canonical-order check against the LSP completion tests.
- A debug build: absolute instruction savings overstate release savings, and instructions are not time. Parallel regions (rayon) may show smaller wall-clock gains.