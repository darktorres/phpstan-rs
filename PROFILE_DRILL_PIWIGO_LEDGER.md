# Piwigo performance ledger

Every optimization task for the Piwigo profile on this branch (`callgrind-20g`), with its state and what blocks it. State as of 2026-10-15, at `0f32829b`.

It replaces `PROFILE_DRILL_PIWIGO_OFF_BUILD_LEDGER.md` (folded in, Appendix A) and the status text of `PROFILE_DRILL_PIWIGO_20G.md`. The 20 G report stays as the plan's design text. Its status claims were written at `5fb858c9`, before the off drill's rows changed the code under them, so each of its twenty items is re-evaluated here against the code and the latest profile.

**Status words.** DONE (hash), OPEN, READY (open, nothing blocks it), BLOCKED (on what), SKIP (reason), SUPERSEDED (an earlier row of this ledger did the work), PART (what is left).

**Ids.** `OFF-x` is a row of the off drill (Appendix A has its full text under the id `x`). `20G-n` is item n of the 20 G plan (`PROFILE_DRILL_PIWIGO_20G.md`, Part II), `20G-n.s` its step s. `DEC`, `CRATE` and `VAL` rows are blocking tasks that are not optimizations.

**Rules.**

- One commit per row, made as soon as the row's own check passes, with a pathspec (`git commit -m … -- <paths>`).
- A row that changes behaviour or layout keeps the Piwigo output byte-identical to the base's (`LC_ALL=C sort` both before diffing), unless the row says why it cannot.
- A saving is claimed only from a measurement: callgrind `Ir` summed over threads against the previous row's commit, off mode, `dhat` profile with frame pointers; wall time by `hyperfine -N`, alternating A B A B, `nice -n 19`, `/proc/loadavg` noted. A "target" in this ledger is the cost of the code a row would replace, read from a profile. It is not a saving.
- Builds, profiling runs and `check-project` probes need the user's go-ahead. Heavy jobs run one at a time.

## 1. Where the branch stands

| | |
|---|---|
| base | `5fb858c9`'s code (the 20 G plan's), off mode: 25,625,190,065 Ir |
| now | `791d29cb` (P8b): 20,928,053,254 Ir, −4.70 G (−18.3%); output byte-identical at every row |
| profile | `~/.cache/prs-profile/p8b/cg` (off mode: stub cache and file cache off, so the 3.4 G stub build is in it) |

Where the 20.93 G sits (threads summed): the parse job on the workers 8.66 G (`read_and_parse_file`, 14,251 files); the check 6.42 G (`check_files`, 2,792 files); the stub build 3.18 G on its pool; the finishing passes 0.97 G on their thread plus ≈0.7 G stolen by other threads; the main thread 0.71 G; the autoload thread 0.32 G. Of the parse job's `parse_file` cost 78% is the project's files, 20% the stub files, 2% the autoload files.

None of the 20 G plan's twenty items is built. Four of the off drill's rows changed what some of them would save (P7 and item 14; P8 and items 3 and 13; P9 and item 4; E5-E7 and items 7 and 16); §3B says how.

## 2. Blocking tasks

These are not optimizations. Each gates optimization rows. Each is split into sub-tasks below, with its own state and the commit or result that closed it; the state of a task is the state of its sub-tasks, so progress is written on a sub-task row and not in this table.

| id | task | blocks | state |
|---|---|---|---|
| DEC-1 | `OFF-D3`: how a binary carries the stub data (compile time or not). §2.1 | `OFF-P6` | BLOCKED (CRATE-1.C5) |
| CRATE-1 | The crate split from Stage C on: C4 (snapshots and lazy resolution), C5 (locator chain and stub layer) and C10 (the `reflection` crate). It reshapes the stub layer, so it decides the form of `OFF-P6`, `OFF-E8`, `OFF-E11`, `20G-6` and `20G-17`, and it removes the eager stub build that `OFF-P1`-`P4` speed. §2.2 | `OFF-P6`; the form of `20G-6`, `20G-17`, `OFF-E8`, `OFF-E11` | PART (C4 started 2026-10-15) |
| DEC-2 | The output-changing rows. §2.3 | nothing now | DONE |
| VAL-1 | The plan's end validation: dumps, an `oracle` profile, an edit replay, the server session. §2.4 | the plan's final validation | OPEN |
| MEAS-1 | Warm-mode costs at this branch's code. §2a | targets of `20G-6`, `20G-17`, `OFF-E8`, `OFF-E11` | DONE |
| MEAS-2 | Memory at this branch's code. §2b | the memory claims of `20G-3`, `4`, `9`, `14`, `16`, `19` | DONE |

### 2.1 DEC-1: how a binary carries the stub data

| id | sub-task | state | blocked by |
|---|---|---|---|
| DEC-1.1 | The stub layer's version key over a `PhpVersions` range, chosen and written in the crate-split plan's Appendix F (`CRATE-1.C5.1`). Its form should keep the gates as data, one parse per file, or the data stops being a function of the source | BLOCKED | `CRATE-1.C5.1` |
| DEC-1.2 | What the stub layer leaves to carry at compile time, measured on a cold run: after C4 and C5 only each stub file's declarations are left, and a run reaches 6-43% of the stub bytes. Estimate ≈0.2-1.5 G per cold run (C8) against 5.05 G in the old code; measured at the layer, not estimated | BLOCKED | `CRATE-1.C4`, `CRATE-1.C5` |
| DEC-1.3 | The decision: embed the data at compile time or not. Needs `DEC-1.1` and `DEC-1.2`. If yes: the build script must run the code the runtime runs (the `reflection` crate, `CRATE-1.C10`), `.rodata` grows from 18.2 to ≈50 MB per binary, and checked-in binary data is out | BLOCKED, yours to decide | `DEC-1.1`, `DEC-1.2` |

### 2.2 CRATE-1: the crate split from Stage C

The plan is `CRATE_SPLIT_PLAN.md`, stage C. Its states on this branch were audited against the code on 2026-10-15: the module graph's `finished` line is `A0 A1 A2 B1-B9 C1 C2 C3`. Each commit of a sub-task below is made with check, clippy and the touched module's tests; the fixture suite and the corpus run at the stage's checkpoints. A sub-task whose output changes says so on its row.

**C4. Snapshots and lazy resolution**

| id | sub-task | state | result |
|---|---|---|---|
| CRATE-1.C4.1 | The gates G's extraction relies on, each with its first inventory: gate 2's `impl` placement and each module's path in its crate, gate 3's names qualified by package and target | OPEN | `xtask modgraph --inventory` lists the crossing edges; the `impl` and name inventories are not made |
| CRATE-1.C4.2 | `xtask gatetime`: `--record` and `--since <hash>`, medians of three runs on prebuilt binaries | DONE `58cfc5d2` | phase start for `58cfc5d2`: lib tests 9.2 s, fixtures 1.3 s (`nice -n 19`, loadavg 2.71) |
| CRATE-1.C4.3 | Keys and data model, in the sub-tasks below | PART | |
| CRATE-1.C4.3a | Class and function tables keyed by the folded name, a lookup that allocates nothing, `FunctionTable` on the class table's type | DONE (already on this branch) | `NameTable`, `with_function_key` |
| CRATE-1.C4.3b | The exact-spelling accessors, then deleting `canonicalize_class_name_case` and the declared-names side map; method keys fold (`method_access` included) | OPEN | |
| CRATE-1.C4.3c | A class's templates as one list of parameters with bound, variance, default and scope, where five parallel maps and sets were rebuilt into parameters in four places that disagreed | DONE `5401afc9` | the type relations read slot variance from the same list; an erased receiver's template with no default and no bound stays unbound (`0f32829b`) |
| CRATE-1.C4.3d | Per-member flags: a property's finality on the property | DONE `8d35c99d` | a final property taken from a trait is the class's own (PHP's rule, a delta the plan did not list); one reached through a mixin is not |
| CRATE-1.C4.3e | Per-member flags for the method sets (`abstract_methods`, `final_methods`, `static_methods`) | OPEN | the plan names only the property case |
| CRATE-1.C4.3f | A property's native type as an `Option` | DONE `7a61603e` | `has_native_type` gone |
| CRATE-1.C4.3g | A parameter's and a return's native type as an `Option`, one no-native-type encoding | OPEN | |
| CRATE-1.C4.3h | The explicit-`mixed` flags (`explicit_mixed_hint`, `return_explicit_mixed_hint`) read from the types | DONE `e7b2521f`, corrected `0f32829b` | a written `mixed` excuses its own value type only; a union keeps a written `mixed` when it drops an equal member; a conditional return is read at its branches, not at its condition's type. Output changes on the missing-type rules only; Piwigo identical |
| CRATE-1.C4.3i | Provenance as the only location record: `is_stub` (63 sites), `function_decl_sites` (11), `Origin::Unknown` (29), the comments citing `project/decl_locations.rs` | OPEN | needs a rule for the location of a merged declaration |
| CRATE-1.C4.3j | Query accessors replacing the storage-returning ones and the `pub` maps (about 280 sites outside `registry/`, 23 in tests) | OPEN | |
| CRATE-1.C4.3k | The builders read the AST's parse-time `QualifiedName::resolved` instead of re-resolving through a `NameContext` (the anonymous-class builder excepted) | OPEN | |
| CRATE-1.C4.3l | No result depends on the class map's iteration order: `classes_using_trait` sorted, `doctrine_entity_for_table` by name order | OPEN | a delta, fixtured |
| CRATE-1.C4.4 | Written results leave the signatures: purity results to a registry table keyed by function and by `(class, method)`; `names_with_assert_tags` seeded from the assertion entries and checked by folded name | OPEN | a delta: a call spelled in another case than its method reaches the assert lookup |
| CRATE-1.C4.5 | The checker test harness builds like a project (merges the stubs before resolving) | OPEN | a test-expectation delta |
| CRATE-1.C4.6 | Per-class memoized resolution replacing the passes, five `OnceLock` levels | OPEN | |
| CRATE-1.C4.6a | Ancestors level: the `extends`/`implements` closure, the `use` closure, `@mixin` targets, unresolved edges, constant declarations | OPEN | needs `C4.3b` |
| CRATE-1.C4.6b | Own level: own members composed with the `use` closure's, `@method` tags, every type resolved in the class's context, the interim assertion step | OPEN | `C4.6a` |
| CRATE-1.C4.6c | Inherited level: PHPDoc inheritance and property hooks from the ancestors' inherited level | OPEN | `C4.6b` |
| CRATE-1.C4.6d | Members level: each `@mixin` target's public members, then the member owners | OPEN | `C4.6c` |
| CRATE-1.C4.6e | Final level: PHPDoc members the native type excludes dropped; the native and PHPDoc combination, whole | OPEN | `C4.6d` |
| CRATE-1.C4.6f | Class constants through the `ConstantResolver`, reference cycles detected on the ancestor level; a reentrant resolution panics and leaves the cell retryable | OPEN | `C4.6a`; a delta (a constant on a cycle types as `mixed`) |
| CRATE-1.C4.7 | `ReflectionBuilder::apply` snapshots | OPEN | |
| CRATE-1.C4.7a | `FileDelta` (role, new declarations or removal) and the collision policy in canonical order, not arrival order | OPEN | `C4.6`; a delta the server shows |
| CRATE-1.C4.7b | Read sets: each memo records every folded name it looked up, a delta invalidates the transitive readers | OPEN | `C4.6`, `C4.7a` |
| CRATE-1.C4.7c | The per-file declaration sets answer what `DuplicateClassSites` and `DuplicateFunctionSites` index; `FileState.functions`/`classes` become the file's set | OPEN | `C4.7a` |
| CRATE-1.C4.7d | `Registry` splits into `ReflectionBuilder` and `ReflectionProvider`; its 22 fields go | OPEN | `C4.7a`-`C4.7c` |
| CRATE-1.C4.8 | Staleness fixes, each with a closure gate: anonymous classes removed with their file version; a file's global constants removed with it; removing a declaration merged onto a built-in keeps the built-in; removing one of two same-named declarations keeps the other; the sets that follow their files | OPEN | `C4.7`; deltas |
| CRATE-1.C4.9 | The 188 registry tests and the other `Registry::build` callers move onto `apply` and the accessors; `Registry::build` and `Origin::Unknown` deleted | OPEN | `C4.7` |
| CRATE-1.C4.10 | Behaviour fixes, one commit each, after the structural ones (list below) | OPEN | `C4.6`-`C4.9` |
| CRATE-1.C4.gate | The gated steps' medians are lower than the phase start's (`xtask gatetime --since 58cfc5d2`); no stub class resolves unless something reaches it; peak RSS at or below the phase start | OPEN | |

The behaviour fixes of `CRATE-1.C4.10`, one commit each: declarations below the top level indexed (`.10a`); class names resolve case-insensitively and a type takes the declared spelling (`.10b`); a conditional or duplicate class-like declaration checked against its own hierarchy (`.10c`); duplicate declarations compare names case-insensitively (`.10d`); member names fold case everywhere (`.10e`); member resolution follows PHP: class chain first, then interfaces, enum natives, `@property` over a private property (`.10f`); trait composition: `insteadof`, visibility-changing `as`, `final`, abstract by another trait, `@use` bindings, aliases in the trait's scope, a shadowed trait method (`.10g`); each `@extends`/`@implements` tag binds only the parent it names (`.10h`); PHPDoc inheritance carries asserts, `@param-out`, `@throws`, purity and method templates, a property redeclared without `@var` keeps its parent's, inheritance searches every parent interface (`.10i`); `@phpstan-type` aliases in `@property`/`@method`/assert tags and on enums (`.10j`); enums read `@deprecated`, `@var` and `@phpstan-require-*` (`.10k`); promoted properties carry asymmetric visibility and their own `@deprecated`/`@internal` (`.10l`); anonymous classes resolve names per namespace block and are named `class@anonymous` (`.10m`); `merge` splits into one operation per case (`.10n`); LSP navigation uses the folded keys (`.10o`); a table two entities map resolves to no entity (`.10p`).

**C5. Locator chain and stub layer** (all BLOCKED on `CRATE-1.C4`)

| id | sub-task | state |
|---|---|---|
| CRATE-1.C5.1 | The stub layer's version key over a `PhpVersions` range, decided first and recorded in Appendix F; `php_version` takes the range and patch forms in the same commit; a function removed before the analysed range is unknown | OPEN |
| CRATE-1.C5.2 | phpstorm-stubs' `PS_UNRESERVE_PREFIX_` names are the reserved words they spell; the prefixed functions and the `___PHPSTORM_HELPERS` classes leave the index | OPEN |
| CRATE-1.C5.3 | `stub_attributes` fixes: a property's or constant's `LanguageLevelTypeAware` type becomes `@var`; `newest_type` reads every entry; a function deprecated in one version-split overload isn't deprecated in the others | OPEN |
| CRATE-1.C5.4 | Locator order | OPEN |
| CRATE-1.C5.5 | The lazy composer locator: classes load on demand; functions, constants and `files` entries stay eager (a delta) | OPEN |
| CRATE-1.C5.6 | The version-keyed stub layer holding C4's memo levels for stub classes, copy-on-merge, a per-file parse memo; `rayon` in the index build decided | OPEN |
| CRATE-1.C5.7 | Provenance queries replacing `built_in_stub_classes` and `stub_provided_functions`, each consumer naming its question | OPEN |
| CRATE-1.C5.8 | The enumeration APIs: every whole-registry iteration moved onto a scoped query; the index records `parent`/`interfaces` per declaration | OPEN |
| CRATE-1.C5.9 | Version-based skip-polyfill (a delta) | OPEN |
| CRATE-1.C5.10 | Version-dependent signature selection (a delta) | OPEN |
| CRATE-1.C5.11 | A panic-injection test | OPEN |
| CRATE-1.C5.gate | The wall times lower than the phase start's; the stub layer replaces the per-project stub clone and the per-process stub parse; the union of all lazily resolved symbols equals `generated_stub_data()` for the unset version key, checked before the two delta commits | OPEN |

**C10. Extract `reflection`**

| id | sub-task | state | blocked by |
|---|---|---|---|
| CRATE-1.C10.1 | No module-graph edge left to the extracted modules (the graph's last two `registry` edges, to `registry::global_vars` and `registry::purity`, are C9's, and the plan's C6-C9 remove the others) | BLOCKED | `CRATE-1.C4`, `CRATE-1.C5`, the plan's C6-C9 |
| CRATE-1.C10.2 | The extraction: `cargo machete` checks Appendix E's rows; gate 10's engine-I/O rule covers `crates/reflection` | BLOCKED | `CRATE-1.C10.1` |

### 2.3 DEC-2: the output-changing rows

Decided 2026-10-15, one at a time in the session. Each difference is listed with its file and read at `VAL-1`.

| id | plan item | change | decision |
|---|---|---|---|
| DEC-2.1 | `20G-1` | a file's identity is its resolved path; links, roles and the server read as the CLI does | approved |
| DEC-2.2 | `20G-2` | a tree with a link cycle is listed once | approved |
| DEC-2.3 | `20G-4` | the order in which code iterating a class's methods meets them (declaration order) | approved |
| DEC-2.4 | `20G-6` step 2 | guards decided by the stubs' availability on run-time evidence; a new polyfill-signature check | approved, both parts |
| DEC-2.5 | `20G-7` | constants on a cycle read `never` and are reported; PHPDoc-only cycles keep every possibility | approved |
| DEC-2.6 | `20G-8` | a `@property` tag shadowed by a declared property is checked against the tag's own names | approved |
| DEC-2.7 | `20G-15` | container-field keys canonical as PHP's array keys | approved |
| DEC-2.8 | `20G-18` | the rule families of §12.10 (observers, one inference per node) | approved in principle; each family's output diff read at `VAL-1`, and anything that is not an improvement fixed or reverted per family |

### 2.4 VAL-1: the plan's end validation (`20G-0`)

On this branch the rows were validated one at a time (output byte-identical, a debug oracle per row, the touched module's tests), so item 0 is not a precondition for a row; it is what the plan's own end validation needs.

| id | sub-task | state | note |
|---|---|---|---|
| VAL-1.1 | The starting state read: the vendor-tree, all-on and unscoped reports at HEAD compared with the last validated state, each difference read | OPEN | |
| VAL-1.2 | An `oracle` profile (`gate` with `debug-assertions` and `overflow-checks`), run once over the gate's three configurations, each vendor project with every rule set and the edit replay; a temporary counter shows every oracle site ran; failures fixed in their own commits | OPEN | |
| VAL-1.3 | The debug oracles and the code only they use deleted (`FileBodies` stops storing statements, the debug-only re-exports, the test oracles with a second implementation) | OPEN | `VAL-1.2` |
| VAL-1.4 | One canonical type printer in `types` over all of `Repr`, printers that cannot lose a field unseen | OPEN | |
| VAL-1.5 | O1: `--dump-front-end`, per file and role, through the project's own `parse_file` | OPEN | `xtask front-end-dump` exists and hashes only |
| VAL-1.6 | `registry-dump` after the finishing passes | OPEN | `VAL-1.4` |
| VAL-1.7 | The checker's records: every PHPDoc resolution, and every expression's inferred and native type (`--dump-expr-types`) | OPEN | `VAL-1.4`; `--dump-types` exists |
| VAL-1.8 | O2: the gate's full artifact set with a view of every field of every diagnostic, over Piwigo, each vendor project and the fixtures, with the stub cache cold and warm and the per-file cache off, written and read | OPEN | `cargo xtask corpus --full` exists |
| VAL-1.9 | The server: an in-process session over Piwigo and two vendor projects (files and roles, `registry-dump`, diagnostics with UTF-16 ranges, code actions, definition, hover, references) | OPEN | |
| VAL-1.10 | The edit replay for Piwigo and each vendor project, and a fresh load of the edited tree compared | OPEN | |
| VAL-1.11 | The dhat profile by holder, once more with untrimmed backtraces | OPEN | `MEAS-2` has the whole-stack profile at this code |
| VAL-1.12 | The baseline taken from the commit that adds the printers and makes the deletions, with the `gate` build at one thread and the default count (byte-identical) and with the `oracle` build | OPEN | `VAL-1.3`-`VAL-1.11` |
| VAL-1.13 | After item 19: the dumps taken again and every difference from the baseline named as one of `DEC-2`'s | OPEN | the last item of the 20 G plan |

### 2a. MEAS-1: the warm run, 13.18 G Ir

Threads summed. "Target" is inclusive cost in millions of instructions (calls in brackets), the most a row could remove.

| phase | Ir | notes |
|---|---:|---|
| `check_files` (`check_program`) | 6,414.6 M (2,792 files) | the same as off mode (6,378.6 M): the cache does not touch the check |
| `read_and_parse_file` | 3,802.1 M (14,251 files) | the 2,792 analysed files are parsed (`parse_with_ignores` 1,967.9 M); the 11,459 context files are decoded from the per-file cache |
| `run_finishing_passes` | 989.6 M | |
| main thread | 869.5 M | `insert_numbered_file` 15,266 calls; of its `insert_built_decl` (467.1 M) 414.7 M is stub decodes it forces |

Targets this run reads:

- **Per-file cache decode, 11,459 files: ≈1,260 M** (≈110 K per file). `next_element::<FileBodies>` 502.0 M, `<Vec<BuiltDecl>>` 455.6 M, `<Vec<RefKey>>` 71.5 M (22,918) plus 25.7 M collecting them into sets, `Arc<LineIndex>` 51.0 M, `FileBodies::with_source` 37.6 M, `crc32fast::hash` 30.4 M, `DeclaredClass` 29.8 M, `DuplicateClassSites` 12.8 M. Reading the files is cheap in Ir: `std::fs::read` 15.2 M (14,251), `File::open` 4.3 M, the two content hashes 30.4 M + 17.1 M.
- **`20G-6`, stub scope: 87,146 stub function records decoded, 682.5 M** (of 107,571 inserted into the stub maps). 414.7 M (51,750) are forced by `insert_built_decl` (15,266 declarations merged onto stub entries), 177.1 M (22,746) by `resolve_deferred_value_of`, 87.5 M (12,105) by `Members::eq`. Deciding not to force these is what the item's step 1 (deferred types on demand) and the stub scope do. The `Lazy` records' own deserialize is 27.3 M (87,580).
- **`OFF-E8`, types decoded: ≈436.5 M** attributed under `Repr`'s visitor (1,177,481 `visit_enum` calls, 62.3 M self; `types::intern::insert` 35.4 M, `Vec<Type>` 29.4 M, `from_utf8` 27.8 M, `Type::new` 22.7 M, `name_hashed` 22.2 M, `exact_hash` 18.8 M); `Type::deserialize` is entered 77,540 times at the top. That is the pool the old estimate (≈ −65 M, ≈ −25 M after `20G-6`) took a part of; no new saving is claimed.
- **`OFF-E11`, origins decoded: 123.9 M** for 144,820 `Provenance` records (`Origin::try_from` 4.1 M). The old estimate was −116 M.
- **`20G-17`, reads by `stat`: the reads it would replace cost ≈60-70 M in Ir** (`fs::read` 15.2 M, `File::open` 4.3 M, `crc32` 30.4 M, `xxh3` 17.1 M); the item's weight is syscalls and I/O, which Ir does not count. The decode (≈1,260 M above) is not what a `stat` index removes. Its soundness concern (mtime shortcut) is on the item.

### 2b. MEAS-2: the heap

dhat counts requested bytes, not resident size. "Peak" is dhat's global peak (`t-gmax`); "exit" is what the leaked project holds after the check. The items are the 20 G plan's, mapped by `analysis/items.py` from the allocating function; `P6` is the stub build.

| run | allocated | blocks | peak | at exit |
|---|---:|---:|---:|---:|
| off, this code | 3,003.5 MB | 19.52 M | 808.7 MB | 655.0 MB |
| off, `5fb858c9`'s code (`dhat-heap-modes/off`) | 3,543.6 MB | 24.87 M | 996.5 MB | 714.1 MB |
| warm (both caches), this code | 1,694.8 MB | 12.86 M | 545.5 MB (at exit: the peak is the end of the run) | 544.1 MB |
| warm, `5fb858c9`'s code, per-file cache off (`dhat-heap-modes/warm`) | 2,669.4 MB | 19.44 M | 866.6 MB | 657.8 MB |

The last two rows differ in the per-file cache as well as in the code, so only the off rows compare the code alone: −540 MB allocated, −188 MB at the peak, −59 MB at exit.

By item (MB; peak / exit), this code, off and warm, against the off run at `5fb858c9`:

| item | off now | off then | warm now | what changed |
|---|---|---|---|---|
| 9, trees | 399.1 / 159.6 | 580.3 / 160.2 | 151.1 / 151.1 | P8 cut 181 MB from the off peak (context bodies are skimmed); the trees that stay are the analysed files' (≈151 MB) |
| 4, registry and passes | 47.4 / 87.7 | 45.6 / 149.6 | 46.7 / 46.7 | P9 removed 62 MB at exit in the off run |
| 14, PHPDoc | 75.2 / 94.9 | 75.2 / 95.9 | 38.3 / 38.3 | unchanged (P7 saves allocations, not retained bytes) |
| 13, a file's products | 106.7 / 105.0 | 81.7 / 79.8 | 42.6 / 42.6 | up 25 MB: P8a keeps the skimmed bodies' text |
| 16, interned values | 14.4 / 32.5 | 14.4 / 32.7 | 31.9 / 32.0 | unchanged |
| 6, stub records | 1.2 / 2.2 | 0.0 / 1.0 | 109.2 / 109.2 | in the off run the stub build is `P6`: 94.3 / 94.3 now, 132.8 / 111.8 then |
| 10, tokens | 13.0 / 12.8 | 13.8 / 13.7 | 3.6 / 3.6 | |
| 2 | 5.5 / 2.6 | 5.5 / 2.6 | 2.8 / 2.6 | |
| 3 | 9.7 / 0.0 | 9.0 / 0.0 | 0.0 / 0.0 | |
| 5 | 11.5 / 18.6 | 11.0 / 20.3 | 18.6 / 18.6 | |

What this does to the plan's memory claims:

- **`20G-3`** (peak −≤209 MB): the off peak holds 399.1 MB of trees (was 580.3 MB); of that, 151 MB are the analysed files' trees, which the warm run shows. The claim is a ceiling that fell by P8's 181 MB; it is not re-estimated here.
- **`20G-4`** (≈90 MB at exit): items 4 holds 87.7 MB at exit in the off run (was 149.6) and 46.7 MB in the warm run. The claim is stale by about half.
- **`20G-9`** (≥53 MB of the analysed trees) and **`20G-19`** (≈173 MB freed during the check): the analysed trees are 151.1 MB at exit and at the peak of the warm run.
- **`20G-14`** (≈50 MB): 94.9 MB at exit in the off run, 38.3 MB in the warm run. **`20G-16`** (≈15 MB): 32.0-32.5 MB held. **`20G-6`** (≈40 MB of heap): the stub records are 109.2 MB in the warm run.
- A warm run's peak is its end: nothing is freed during the check, which is what `20G-19` is about.
- The check phase allocates 232 MB in 2.16 M blocks in `checker` and holds 0.6 MB of it at the end (the temporaries of `20G-18`).

## 3. Open optimization tasks

**How to read the numbers.** "Target" is the inclusive cost, in millions of instructions, of the code a row would replace, read from the P8b off-mode profile (calls in brackets). It is the most the row could remove, not what it saves. Inclusive figures of recursive functions count nested calls more than once and are not quoted. Rows about memory or allocation counts say so.

### 3A. The off drill's open rows

| row | task | state | blocked by |
|---|---|---|---|
| OFF-H1 | Write the cache file from one buffer instead of building the file in a second `Vec` beside the body (`Writer::finish`, `container.rs`). −15.3 MB at the cold run's peak and −31.4 MB allocated per cold run, measured with the PHPStan base on the code of `5fb858c9`; a memory row, no instruction saving is claimed (`PROFILE_DRILL_PIWIGO_OFF_HEAP.md` §11) | READY | — |
| OFF-P6 | Stub data built at compile time. −5.05 G on the old code; ≈0.2-1.5 G per cold run after the crate split's C4 and C5. Not built in today's form | BLOCKED | DEC-1, CRATE-1 |
| OFF-E8 | A cache file's types stored once, in a table its records index: ≈ −65 M, ≈ −25 M after `20G-6`. Warm-cache runs only | BLOCKED | `20G-6` steps 3 and 5 (the name table by index, `read_at` records, a decode context: one format bump). Target read by MEAS-1: types decoded ≈436.5 M of the warm run (§2a) |
| OFF-E11 | A cached file's origins decoded as the one `Arc<Path>` its file binds: ≈ −116 M, per-file cache warm | BLOCKED | `OFF-E8`'s decode context. Target read by MEAS-1: 123.9 M of the warm run (§2a) |

### 3B. The 20 G plan, item by item

"Needs" are the plan's own dependencies (its text, Part II). The items are numbered in the order the plan builds them, each closed by a tests-only gate; that order is a process rule, and a row with no "needs" is READY apart from it. Estimates are the report's (§12 table), from the 20.57 G profile; "target now" is read from the P8b profile.

| item | what the plan does | state and what the off drill changed | target now | report's estimate | blocked by |
|---|---|---|---|---|---|
| 20G-0 | Baseline dumps, canonical printers, an `oracle` profile; the debug oracles retired | OPEN. Only `xtask front-end-dump` (syntax crate, hashes) exists | none | none | VAL-1 |
| 20G-1 | One session for the CLI, the server and the tests: one identity, one configuration, one load, one analysis; `built_once` split | OPEN. Mostly semantic (DEC-2). P8b added a third use of `built_once` (it decides whether skipped bodies' references are completed), so the split must keep that. The perf part: the server stops keeping context trees and unopened sources | server memory | the server's ≈420 MB of context trees | — (output changes approved, DEC-2) |
| 20G-2 | A parallel walk, one listing per directory, path data from the walk, each path stored once | OPEN, unchanged by the off drill | `collect_php_files` 75.3 M serial, `sort_paths` 112.3 M (4 calls, the main and vendor threads), `strip_prefix` 38.1 M (29,756), `PackageIndex::owner` 69.3 M, `absolute_path` 54.1 M, `FileTable::id_for` 33.7 M, `Components::next` 139.4 M across the run; vendor discovery 321.3 M on its own thread | ≈0.3 G and ≈0.15 s serial start | `20G-1` (identity rules; roles known before the parse) |
| 20G-3 | Context trees dropped on the worker that parsed them | OPEN, premise halved by P8b: skimmed bodies are not trees, so the drop fell from 575.5 M to 267.8 M (11,459 context trees, still on the main thread inside `insert_numbered_file`) | 267.8 M serial | ≈0.3 G and 0.58 G off the serial path; peak −≤209 MB | `20G-2` (a file's role final before its parse), `20G-1` (`keeps_tree` in `update_file`) |
| 20G-4 | The registry by declaration ids; passes as parallel maps; scope as a bitset | OPEN, **premise mostly gone**: P9 deleted the eager purity and property-write passes, which held its largest costs (string keys ≈0.5 G, `Members::eq` 96.9 M, 59,637 member copies). Now `Members::eq` is 4.8 M and `reset_scoped` 43.8 M. What is left is the other passes: `flatten_phpdoc_inheritance` 162.1 M, `precompute_ancestor_names` 147.9 M, `precompute_member_owners` 121.5 M, `drop_doc_members_excluded_by_native` 86.1 M, `resolve_deferred_value_of` 69.1 M, `closure_scope` 103.8 M, `flatten_traits` 50.0 M (the finishing passes total 972 M over two runs) | those, 972 M in all, mostly parallel already | ≈0.9 G and ≈0.4 s serial; the estimate rests on the purity costs P9 removed, so it is stale | — (member order approved, DEC-2) |
| 20G-5 | The merge in parallel: ids by prefix sum, sharded tables, posting lists, files by id | OPEN | `insert_numbered_file` 413.9 M serial (`drop` 267.8 M is item 3's; `register_references` 77.2 M over 28,502; `insert_built_decl` 34.8 M); the two path sorts 20.7 M and 20.6 M | ≈0.25 G | `20G-4` (ids), `20G-2` (placement) |
| 20G-6 | The stub scope: deferred types on demand; guards on the stubs' availability; names once per cache file; records read with `read_at` and checked per record | OPEN. `OFF-E1`, `P1`, `P4` changed how the stub base is built but not what this item reads. The warm run at this code (MEAS-1, §2a) decodes 87,146 stub function records | 682.5 M of stub decodes, 414.7 M of them forced by `insert_built_decl` | ≈0.35 G, then more of `warm`; ≈40 MB of heap | `20G-4` (step 1); CRATE-1 (step 2's guards and the polyfill check are approved, DEC-2) |
| 20G-7 | One relation memo keyed by the registry's content generation, replacing `structural_memo` and `KEEP_DECISIONS` | OPEN. `OFF-E5` and `E6` put one-word shortcuts and a pointer test in front of `is_super_type_of` | `is_super_type_of` 422.5 M (1,447,876 calls), 109.0 M self | up to ≈0.3 G | `20G-4` (on-demand cycle guards), `20G-6` (cycle rule approved, DEC-2) |
| 20G-8 | Declarations carry only what every reader needs: the six case-reference fields move to the checker | OPEN | `collect_class_name_case_refs` 26.8 M; `declared_type` 456.3 M (240,689 calls, stub files included) | ≈0.1 G | `20G-4` (`FileDecls`) (the one `@property` case approved, DEC-2) |
| 20G-9 | Trees in an arena, nodes as handles; `peeled` a loop; source slices borrowed; nodes the size of their variant | OPEN. P8b means context bodies are no longer trees, so the arena's gain shifts to the analysed files and to declarations | `peeled` 61.2 M (2.54 M calls); `drop_glue` of `ExprKind` 486.9 M (1.87 M) and of `StmtKind` 700.7 M (538,692), the two overlapping; 267.8 M of the `Stmt` drop is on the main thread | ≈0.8-1.2 G; ≥53 MB of the analysed trees | — |
| 20G-10 | Tokens in flat arrays instead of five functions per token | OPEN, **target grew**: skimming still pops a `Tok` for every skimmed token (`drop_glue::<Tok>` 2.16 M calls inside `parse_function_body`) | the six plumbing functions 2,868 M self (`next_token` 837.9, `next_tok` 582.3, `fill` 567.0, `next_raw` 490.5, `skip` 231.4, `take_token` 159.7) over 8.44 M tokens, 78% of it in the project's parse; `drop_glue::<Token>` 131 M | ≈1.7 G | `20G-9` (the file's names in its arena) |
| 20G-11 | Line starts for every file, columns only where read, ignore marks and member spans as offsets | OPEN, unchanged | `LineIndex::new` 363.4 M (25,753 builds; ≈280 M the project's) | ≈0.2 G | — (11.5, the server's on-demand index, needs `20G-1`) |
| 20G-12 | Lexer details: identifier lookup, keywords by word, blank runs, operators without a call, qualified names as one token, `RefKey::Constant`, `UseMap::get`, native type names, digit separators | OPEN, unchanged | `name_and_bits` 631.4 M (2.41 M calls), `lex_ident_span` 223.7 M, `lex_operator` 203.4 M (4.50 M), `Keyword::try_from_bytes` 165.9 M (2.23 M), `parse_qualified_name` 765.9 M incl. (499,347; recursive), `UseMap::get` 111.8 M (442,690), `NativeType::from_name` 29.7 M, `strip_digit_separators` 18.8 M | ≈0.35 G | `20G-10` (name table), `20G-11` (blank runs by bitmask) |
| 20G-13 | A file's products: the parser lists declarations; one walk for summaries, flows and global facts; the old collector goes | OPEN, **premise mostly delivered by P8a, P8b1 and P8b**: context bodies are no longer summarized, flowed or walked at load (`BodySummary::of` 83.6 M over 10,863 bodies where 344 M over 59,926 was; `InitFlow::of` 50.8 M; `GlobalFacts` 74 M). Left: `file_declarations::collect` 1,685.6 M (reflection building is most of it: `from_decl` 1,019.1 M, `from_def`), `recorded_references` 265.6 M, `DocNames` 170.5 M, `add_uses` 148.3 M (289,484) | those | ≈0.45 G | `20G-9` (references into a finished tree), `20G-10` |
| 20G-14 | PHPDoc finished: tag effects, eager lowering, two memos, nodes in one vector | OPEN. `OFF-P7` built the plan's lazy tag description (and the P11 step 1 / `TagEffect` rows remain). `OFF-P11` was measured and not kept | `parse_shared` 1,394.7 M (195,201 lookups, stub files included), `apply_tag_recording_errors` 702.0 M, `merge_from` 101.0 M, `parse_written_type` 151.5 M, `to_source_string` 96.8 M, `resolve_type_node` 94.3 M (34,821), `DocNames` 170.5 M | ≈0.35 G; ≈50 MB | `20G-15` (names in the tag tokens), `20G-9` (doc bytes) |
| 20G-15 | Names held as names: `String` keys to `Name`; folded text lookups deleted; resolution by hashing parts | OPEN. The purity keys the plan assigned to item 4 are gone (P9) | `<str>::fold_hash` 275.7 M and `is_folded_eq` 358.2 M (4.67 M and 3.81 M calls); `Interner::name` 260.8 M (1.01 M) and `name_hashed` 354.8 M (1.44 M); `resolve_as` 202.2 M; `substitute_class_templates` 193.8 M; `class_template_bindings_rec` 102.1 M | ≈0.6 G | — (step 2's key change approved, DEC-2) |
| 20G-16 | Interned values: a name in one allocation; the type interner without per-hash vectors | OPEN. `OFF-E7` keeps a class's plain object type | `types::intern::insert` 439.5 M (1.33 M calls), 151.3 M self; `Interner::str` 53.6 M | ≈0.1 G; ≈15 MB | `20G-15` (short name in the header), `20G-7` (memo generation) |
| 20G-17 | Warm runs: cache entries checked by `stat`; a resolved context base | OPEN. `OFF-E4` measured the nearest idea (analysed files in the per-file cache) and found no gain worth the cold cost. The `stat` index conflicts with an exclusion the earlier work recorded: an mtime shortcut is unsound when a same-size rewrite keeps its mtime (`rsync -t`, `tar`) | reads ≈60-70 M Ir; the per-file decode ≈1,260 M is not what a `stat` index removes (§2a) | decoding and reads ≈0.85 core-s | `20G-1` (server), `20G-5` and `20G-6` (container, name table), CRATE-1 |
| 20G-18 | The check phase's remainder: observers and one inference per node (§12.10), slots, radix scopes, rule temporaries | OPEN. `OFF-E2`, `E3` and `E10` cut small parts | `check_program` 6,378.6 M; `infer_expr` 1,051.9 M (800 K calls), `check_args` 517.4 M, `method_call_return_type` 448.8 M, `apply_narrowing_unguarded` 467.4 M, `infer_binary` 246.6 M, `merge_scopes` 222.0 M, `call_bindings` 133.2 M, `find_var` 123.2 M, `OrdMap::get` 71.4 M, `Locals::clone` 33.5 M, `union_many` 263.9 M, `reduce_array_members` 146.8 M, `resolve_call_args` 63.4 M, `string_literal_ref` 91.3 M, `sqlparser` 88.0 M | ≈1.6-2.0 G | §12.10 (18.1): `20G-13` (the one walk); approved in principle, DEC-2; radix scopes: slots first, `20G-16` |
| 20G-19 | Each tree kept until its last reader; the project-wide passes read facts, not trees | OPEN, memory only | none in instructions | analysed trees ≈173 MB freed during the check | `20G-3`, `20G-13`, `20G-18`, `20G-1` |

**Ready now** (no open "needs", exact): `OFF-H1`, `20G-9`, `20G-11`, and `20G-15`. `20G-4` and `20G-1` have no technical need either, but `20G-4`'s estimate no longer holds and `20G-1`'s output changes are approved but are read at VAL-1. Everything else waits on an earlier item or on CRATE-1.

**Longest chains:** `20G-1` → `2` → `3` → `19`; `20G-4` → `5`, `6`, `7`, `8`; `20G-9` → `10` → `12` and `13` → `14`; `20G-13` → `18` → `19`; `20G-15` → `16`; `20G-5` + `6` → `17`.

## 4. Done

Each row's output was byte-identical to the base's; the rows' own text (Appendix A) has the checks. Callgrind `Ir`, off mode.

| row | commit | Ir after | change |
|---|---|---|---|
| base | `5fb858c9`'s code | 25,625,190,065 | |
| E1 (with P1, P5) | `3d5434b3` | 25,029,730,463 | −595.5 M |
| P2, D5, E13 | `c2cd1586` | 24,498,907,030 | −530.8 M |
| P3 | `a13f4ed2` | 24,155,076,222 | −343.8 M |
| P4 | `0ce52479` | 23,866,820,730 | −288.3 M |
| E2 | `60b0cd33` | 23,812,168,909 | −54.7 M |
| E3 | `07a0f111` | 23,785,065,626 | −27.1 M |
| E5 | `5ec70c1e` | 23,725,134,207 | −59.9 M |
| E6 | `f2f65cbb` | 23,701,951,690 | −23.2 M |
| E7 | `60e85393` | 23,672,079,898 | −29.9 M |
| E10 | `e081dbe9` | 23,619,596,541 | −52.5 M |
| P7 | `a4035361` | 23,216,039,715 | −403.6 M |
| P9 | `c0dd2788` | 22,362,774,991 | −853.3 M |
| P8a | `7726dacf` | 22,014,267,117 | −348.5 M |
| P8b1 | `11837a04` | 21,775,661,606 | −238.6 M |
| P8b | `791d29cb` | 20,928,053,254 | −847.6 M |

Defects fixed along the way: E14 `6dd06124` (`instanceof` with a static property as the class), E15 `efe621ad` (the stub cache key left out `vendor/phpstan-src/stubs`), E16 `480107e3` (maps and sets of names wrote in table order). D6 is decided (skim unchecked, 2026-10-15); D1, D2 and D5 are decided.

## 5. Skipped

| row | why |
|---|---|
| OFF-P10 (`simdutf8`) | built, measured, reverted: −0.47 M against ≈ −22 M estimated; the checked bytes can't become a `String` without checking again under the workspace's no-`unsafe` rule |
| OFF-P11 (messages formatted when displayed; class-constant keys when dynamic constants are set) | built, measured, reverted: −0.65 M against 29.1 M estimated |
| OFF-E4 | built, measured, reverted: warm −2% to −5%, cold +9% to +14% on two of ten trees |
| OFF-E9 | folded into the plan's regex-scan row (`20G-18`) |
| OFF-E12 | built, measured, reverted: the 420 M of method decoding moved to the workers, total −23 K |

## 6. Not optimizations

Kept for the record. None is built from this ledger: the questions in Appendix A, rows U1-U9 (the parser against the whole `vendor/` corpus, known parser gaps `$s{0}`, `(real)`, `(unset)`, compile-time errors the parser does not report, E1's deterministic reproduction, what an `extract()` body still needs, the truncation holding 33 `offsetAccess.mightNotExist` reports, readers left out of `d`, P6's build-script cost, a third check of the report's claims), and the plan's output-changing rows listed under DEC-2.

## 7. Where to resume

- Nothing in the off drill is waiting for an optimization except `OFF-H1` (READY) and the three blocked rows of §3A.
- The plan's items are mostly open and large. Measured by what they would replace, the biggest are the check phase (`20G-18`, ≈6.4 G of work, ≈1.6-2.0 G estimated), the token plumbing (`20G-10`, 2.9 G of functions, ≈1.7 G estimated), the per-file declarations and references (`20G-13`, 1.7 G of reflection building plus 0.27 G of references), the lexer's own work (`20G-12`) and name hashing (`20G-15`, ≈0.63 G of folding). `20G-4`'s estimate no longer holds. `20G-9`, `11` and `15` have no blocker.

## Appendix B. The 20 G report, re-evaluated

The report describes `5fb858c9`. This table says, for each finding and each item, what the code and the P8b profile show now.

### B1. Part I's findings

| finding | the report | now |
|---|---|---|
| R1, context trees dropped on the main thread (575.5 M) | item 3 | 267.8 M after P8b (skimmed bodies are not trees); still serial, 11,459 trees |
| R2, five functions per token (1.77 G) | item 10 | 2,868 M in the six functions over 8.44 M tokens (the off profile also lexes the stub files; 78% is the project's); P8b's skim still pays it per skimmed token |
| R3, results written into shared declarations; string keys; `Members::eq` 96.9 M | item 4 | gone with P9: `Members::eq` 4.8 M, no eager purity pass |
| R4, a line index for every file (270 M) | item 11 | 363.4 M over 25,753 builds (stub files included) |
| R5, stub members decoded to learn they have no deferred types (178 M) | item 6 | warm run at this code (MEAS-1): 87,146 stub function decodes, 682.5 M (§2a) |
| R6, lookups by `&str` (701 M of folding) | item 15 | 634 M (`fold_hash` 275.7 M, `is_folded_eq` 358.2 M) |
| R7, products for every body (`BodySummary` 344 M, `InitFlow` 218.5 M, `GlobalFacts` 218.5 M) | item 13 | cut by P8a and P8b1 to 83.6 M, 50.8 M and 74 M |
| R8, stack guards in hot paths | item 9 | `peeled` 61.2 M (2.54 M calls) |
| R12, identifier names | item 12 | `name_and_bits` 631.4 M, `Keyword::try_from_bytes` 165.9 M |
| M1-M11, memory | items 3, 4, 9, 14, 16, 19 | re-measured by MEAS-2 (§2b): the baselines moved as §2b says |

### B2. The 25 G report's unbuilt designs (the report's §2 lists)

| design | now |
|---|---|
| C3 steps 6-7: names as `Name` keys (≈0.6 G) | `20G-15` |
| F4: purity and property-write keys as folded strings (≈0.5 G) | gone: P9 removed the eager passes |
| F3: results inside shared declarations; a serial `changed_since` | the shared-declaration copies are gone with P9; `changed_since` is part of `20G-4` |
| §14: trees dropped in the serial insert loop | `20G-3`, 267.8 M left |
| P7 and P11 step 1: PHPDoc lowering and tag effects | `OFF-P7` built the lazy description; `20G-14` has the rest |
| P6(d): the streaming lexer's plumbing | `20G-10` |
| P4: column tables for every file | `20G-11` |
| F1 and F2: the stubs' availability guards and their count | `20G-6` |
| C1 step 5, C5, §12.10 | `20G-18` |
| C2 steps 2-3, C4 step 5's keys | `20G-18`, `20G-15` |
| F5: a `FileId` bitset, string frontier sets | `20G-5` |
| P12 steps 3-4: declaration list, the collector goes | `20G-13` |
| §12.11: `MemberIndex` | `20G-18` |
| the sorted traversal's k-way merge | `20G-2` |

## Appendix C. How this ledger was made

- **Read:** the off ledger and the 20 G report in this checkout, whole.
- **Profile:** `~/.cache/prs-profile/p8b/cg` (791d29cb, off mode, 20,928,053,254 Ir), indexed with `drill-scripts/parse.py` and read with `q.py` and `roots.py`. Figures are inclusive, summed over threads. Which share of a shared function belongs to the project's files was read from the call counts of `parse_file`'s callers (6,316.8 M the project's, 1,578.4 M the stubs', 163.0 M the autoload files').
- **Not measured:** wall time. Warm-mode costs were read by MEAS-1 (§2a) and memory by MEAS-2 (§2b) after this appendix's profile. The estimates in the "report's estimate" column are the report's, unchanged.
- **Dependencies** ("needs") are read from the report's Part II text, not from code.

## Appendix A. The off drill's rows

The rows of the retired `PROFILE_DRILL_PIWIGO_OFF_BUILD_LEDGER.md` (in git before this commit), kept whole because they hold the evidence behind §3 and §4. Their ids are unchanged; elsewhere in this file they appear as `OFF-<id>`. One status was corrected: D7 (the proposals it recommended are built). The report that holds each row's design is `PROFILE_DRILL_PIWIGO_OFF.md` (section numbers and the ids Dn, Bn, Cn, En, Nn are its); "item n" is an item of the 20 G plan. Row H1 comes from `PROFILE_DRILL_PIWIGO_OFF_HEAP.md` and was never in the retired ledger; it is in §3A.

### A1. Decisions that gate the build

| # | status | decision | gates |
|---|---|---|---|
| D7 | DONE (moot: proposals 1-5 are all built, rows P1-P5; the build order below is complete) | Build proposals 1-5 now, in the order of the build order below, since the crate split removes the eager build they speed and every cold run pays it until then. Alternative: only proposal 4 and E1's fix. | P1-P5 |
| D6 | DONE (decided 2026-10-15, answered in the session: adopt D6, skim unchecked) | A context file that fails to parse keeps what it declares: what parses is recovered, what failed is unknown. A skimmed body is not checked: a syntax error in one is not found at load, and a body that does not parse when read is unknown. There is no check pass and no verdict cache, so D1's check requirement does not apply to P8b. Validity following the configured PHP version and one rule for the batch run and the language server stay as written in §10.8. | P8, parser gaps |
| D3 | OPEN (reopened by C8, §10.6) | How a binary carries the stub data. Decided again once the crate split's C5 sets its version key, on the numbers in §10.6 (≈0.2-1.5 G per cold run once C4 and C5 land, against 5.05 G today). Proposal 6 is not built in today's form. | P6 |
| D1, D2, D5 | DONE (decided 2026-10-13, report) | D1: every skimmed body is checked and each file's verdict cached. D2: a skimmed body's text is copied at load from the bytes the load reads. D5: each cache checks once, at its first store, whether it can write. | P8, P2, E13 |

### A2. Build order (D7)

1. **E1's fix**, which carries proposals 1 and 5.
2. **D5's write check**, which carries proposal 2.
3. **Proposal 3.**
4. **Proposal 4.**

Each step is measured with paired cold runs. B4 puts the ceiling at the 0.37 s
the warm stub cache saves today. The cuts, proposals 7-11 and the defects E14
and E15 are independent of this order unless a row's "needs" says otherwise.

### A3. Defects in today's code (§10.14)

| id | status | task | verify |
|---|---|---|---|
| E1 | DONE (3d5434b3; measured: Piwigo output byte-identical to the base in the off, cold and warm modes; Ir off 25,029,730,463 against 25,625,190,065 (−595.5 M), cold 25,238,288,982 against 25,683,423,164 (−445.1 M), warm 20,551,124,205 against 20,572,181,579 (−21.1 M); the cold run stored `stub-base-phpstan`, `stub-data` and `stub-index` and no plain base, and the warm run rewrote none of the three) | The stub state is a value owned by the entry point (`Stubs`, `registry/stub_set.rs`), built per layer on a thread of its own with the wait blocking in the OS (S2), on the pool the entry point hands in (S3), with the base chosen at `expect_files` (S4: `Project::loading`, then `expect_files(parsed, &Stubs)`; `finish_loading_with(&Stubs)` for a project that got none). Production code has no static for it; a `Registry` keeps an `Arc<Corpus>`. Not done: S4 is a runtime check (a registry with no corpus panics when asked), not a type; the regression test (U4) and `cargo test`/fixtures/clippy were not run, by instruction. | The regression test with `PHPSTAN_RS_NO_STUB_CACHE=1`, a one-thread pool and two nested `Project::new()` calls, with a watchdog (designed, not run); `cargo test --lib` cold; fixtures. Re-entry impossible by construction. |
| E14 | DONE (6dd06124; unit test over `static::$p`, `self::$p`, `$this->p`, a parenthesized expression and a plain name; the syntax lib's 269 tests pass, clippy clean; the two vendor files are not added as fixtures, the test holds their two forms) | Read `instanceof`'s right side with `parse_new_class_reference`: a plain name stays `InstanceofTarget::Name`, anything else becomes `InstanceofTarget::Expr`. | A parser test per form; the two files (`AbstractConfigurationProvider.php:41`, `BigInteger.php:177`) as fixtures. |
| E15 | DONE (efe621ad; the hash covers `vendor/phpstan-src/stubs`; the test reads the `rerun-if-changed` paths of `stub-sources/build.rs` and asserts each is in the key's inputs) | Add `vendor/phpstan-src/stubs` to the inputs `crates/phpstan-rs/build.rs` hashes. | A test that the hash's inputs cover every path `stub-sources`' build script reads. |
| E16 | DONE (480107e3; found by E4's debug oracle: `file_cache::store`'s debug check that a stored file re-encodes to its bytes fails on any class with several properties, because `NameMap`, `NameSet` and `FoldedMap` wrote their entries in table order, and a map rebuilt from them iterates in another; they write in name order now, the test fills one set in two orders and the mutation of removing the sort fails it; the syntax lib's 270 tests pass; the debug build then ran cold and warm over 215 files with no failure) | A map or set of names writes its entries in the order of the names. | The unit test; a debug run with the file cache on over a tree with classes. |

### A4. Proposals (§10.1-§10.11)

| # | status | task | estimated saving | needs | verify |
|---|---|---|---|---|---|
| P1 | DONE (rode on 3d5434b3; verified at 0ce52479: a two-file project that never mentions PHPStan and one that does give output byte-identical to the base binary's; the first stores only `stub-base`, the second only `stub-base-phpstan`, where the base binary stored both; a mention only inside a body is P8's and not built) | Resolve only the stub base the files need: the project starts with no base and takes one at `expect_files` (E1's S4). | −355.1 M serial; the plain base's finishing run ≈35-52 M (estimated) | E1 | Output identical on a project that mentions PHPStan and on one that does not; a file that mentions PHPStan only inside a body (P8) still picks PHPStan's base. |
| P2 | DONE (c2cd1586; measured: off run 24,498,907,030 Ir against 25,029,730,463 at E1's commit, −530.8 M, output byte-identical; less than the estimated −0.71 to −0.74 G; a read-only cache directory run prints one line per cache and gives the same output) | Serialize only what a store keeps: gate `stub_cache::store`, `StubIndex::store_cached`, `Registry::store_cached` and the stub-data store on D5's write check. | −1,005.3 M serial; ≈ −0.71 to −0.74 G after P1 | D5's check | A cold run still checks `store_entries`' duplicate-name test; an unwritable cache directory pays no encoding. |
| P3 | DONE (a13f4ed2; measured: off run 24,155,076,222 Ir against 24,498,907,030 at D5's commit, −343.8 M, output byte-identical; the test of the lookups against the ordering is written and compiled, not run) | A hash index for stub bases built in memory: `freeze` builds the index `store_entries` already builds. | ≈ −0.29 G | — | A built-then-frozen base answers every lookup as the bisection did (to be checked by a test). |
| P4 | DONE (0ce52479; measured: off run 23,866,820,730 Ir against 24,155,076,222 at P3's commit, −288.3 M, output byte-identical; the stub index a cold run stores is byte-identical to the one stored before, so the closures are equal file by file) | Look each stub file's dependents up once in `build_closures`: per-file dependent lists, closures in strongly-connected-component order. | ≥ −0.2 G serial | — | The closures equal today's, file by file (to be checked by a test). |
| P5 | DONE (rode on 3d5434b3; paired wall time, Piwigo, file cache off, stub cache warm, `hyperfine -N` 6 runs after 1 warmup, alternating base/new twice, `nice -n 19`, loadavg 4-7 throughout: base binary mean 3.380 s and 3.286 s, the binary at 0ce52479 3.057 s and 3.206 s, medians 3.423/3.220 against 3.046/3.161. The new binary carries E1 to P4 together, so P5's share alone is not separated, and the difference is within one standard deviation of the base's runs; it is not slower. An earlier pair of runs overlapped another and was discarded.) | Build the stub corpus beside the parse: E1's S2 and S3; the corpus's `par_iter` moves into `worker_pool().install`; `check-project` makes the stubs before listing files. | wall time: ≈0.14 s alone, the whole ≈0.25 s stretch with P1 (a model); no Ir change | E1 | Paired wall-time runs; `PHPSTAN_RS_THREADS` and the 64 MB stack apply to the corpus. |
| P6 | OPEN, blocked | Build the stub data at compile time into every binary. Not built in today's form. | −5.05 G today; ≈0.2-1.5 G per cold run after the crate split's C4 and C5 | D3; the reflection crate (C10) | If built: B10's guard test, an embedded-equals-runtime-build test, the override equal to the profile (B9). |
| P7 | DONE (a4035361; measured: off run 23,216,039,715 Ir against 23,619,596,541 at E10's commit, −403.6 M against the ceiling of ≈ −0.37 G, Piwigo output byte-identical; the tag parser pulls tokens from the lexer (`Lexer::next_token`) into a buffer and fills it only as far as a lookahead or rewind asks, and a tag's description is `Description`, the source from the first token the parser did not take, which `text()` lexes again and joins with single spaces and `starts_with_optional_marker()` answers from the first tokens, on the same rule as the old test (the joined text without spaces starts with `[optional]`, so `[opt ional]` still matches); a debug build compares every description's text with the tokens the parser goes on to read, which fails with the offset moved to the token's end (23 tests); five new tests (text for `//`, string escapes, floats, overflowing integers, `<b>`; the offset after rewinds; the marker against the old rule on 14 inputs; a serde round trip; no allocation without a description); the warm figure was not measured; the workspace tests, clippy of the other crates and the fixtures not run) | Tokenize a PHPDoc tag's description only when it is read: the parser pulls tokens, records the description's offset, and re-lexes on request; `[optional]` becomes a three-token prefix test. | ≈ −0.37 G off, ≈ −0.08 G warm (ceilings); ≈ −0.04 G after P6 and item 14 | — | Re-lex, never slice (`//`, string escapes, floats, overflowing integers, single-space join); every phpdoc test; the memo keys unchanged. |
| P8a | DONE (7726dacf; measured: off run 22,014,267,117 Ir against 22,362,774,991 at P9's commit, −348.5 M, Piwigo output byte-identical in the optimized build, in a debug build with the oracle on (cache off) and in two warm-cache debug runs. A context file with no anonymous class, no nested declaration and no declaration as a statement of a body (`FileFacts::has_declaration_in_body`, new, set by the parser) keeps each function and method body as its bytes (copied at load from the bytes the load reads, D2) with the index of the name context in effect where it is declared; `FileBodies` makes the summary and flow in a `OnceLock` when a pass first reads the body, by `syntax::parse_function_body` over `Lexer::fragment_bytes(text, base)` with that context, so spans stay absolute. The load no longer walks those bodies for their summaries, flows or nested declarations. The per-file cache stores a skimmed body as its range and context (`NameContext::to_stored`/`from_stored`, a fresh version on decode) and `CachedFile::into_parsed` takes the bytes and copies the text again, whether or not the body was read. Only context files skim (`parse_file_with_names`'s `skim_bodies` is the loader's `cacheable`); analysed files and `ast_of` are unchanged. A debug build reads every skimmed body at load and asserts its summary and flow equal the tree's. Tests: `registry::tests::skimmed_bodies` (6: tree equality across namespaces, `use function`, goto labels, names of the declaring context, cache round trip with re-encode equality, stored the same read or not, a file that is not UTF-8, files with a declaration in a body keep every body read); three mutations (context dropped, base offset 0, the `has_declaration_in_body` condition removed) fail them; syntax 270, lib 1069, fixtures 3830 pass. Not run: workspace tests other than those, clippy. Not measured: peak memory (the kept text is ≈18 MB on Piwigo, D2).) | The first slice of P8: bodies of context files made lazy at the summary and flow, the parse itself still done in full at load. | −348.5 M measured (below the whole-P8 estimate because the parse, the lexer's token layer and the drops are still paid) | P9 | Debug build asserts lazy = eager for every skimmed body. |
| P8b1 | DONE (11837a04; measured: off run 21,775,661,606 Ir against 22,014,267,117 at P8a's commit, −238.6 M, Piwigo output byte-identical in the optimized build and in a debug build with the oracles on (cache off). The parser records two more file facts: `has_global_or_include` (a `global` statement or an `include`/`require` expression, also in a `{$...}` fragment, whose flags are now merged into the file's) and uses `has_declaration_in_body` (P8a). `GlobalFacts::of_file` walks no function, method or closure body when the file has no `global` and no include, because such a body adds no site; `file_declarations::collect` enters no body unless the file mentions `define`, has a nested declaration or has a declaration as a statement of a body (`for_each_declaration_with`). A debug build walks the bodies too and asserts the same global facts, and the existing count of declarations of a full walk still guards the declarations. Tests: two fact tests in `parse`, mutation (bodies never walked) fails 2 global-variable tests; syntax 272, lib 1069, fixtures 3830 pass. Not run: workspace tests other than those, clippy.) | The load's walks over context bodies (`GlobalFacts`, declarations) skipped by file facts. | −238.6 M measured | P8a | Debug build walks the bodies too and compares. |
| P8b | DONE (791d29cb; measured: off run 20,928,053,254 Ir against 21,775,661,606 at P8b1's commit, −847.6 M, Piwigo output byte-identical in the optimized build and in a debug build with the oracles on (cache off and stub cache off). The parser skims a context file's function bodies that the load keeps as text (`parse_skimming`): the tokens of a body that opens at `{` are counted to the matching `}` and not parsed. A doc comment, `Eof`, inline HTML, a string with a complex part, a class/interface/trait/enum/global/include/require/yield/goto keyword, a named `function`, `define`, a name that can be `phpstan\...` rewinds the tape to a full parse of that body. Lexer errors and unbalanced braces are still found; a syntax error in a skimmed body is not (D6). Skimmed bodies' references are registered by `complete_skipped_references`, which parses every skimmed body once, so it runs only where a lookup needs it: incremental updates and removals, not a project built once. The first measurement of this commit, with the completion running in the batch, was 23,567,209,391 Ir (+1.79 G): the completion parsed 46,413 bodies for 2.36 G Ir. In a project built once the only `dependents` lookup is the one finishing pass, and every name its files declare is among those that changed, so what a skipped body refers to adds no declared name to its scopes; a debug build completes the references anyway and asserts both scopes equal (it ran once on Piwigo, with and without the stub cache). Tests: 6 parse tests (skipped bodies parse to the whole-parse content, each trigger, member-operator keywords and closures, PHPStan contexts, syntax error not found but lexer and brace errors found, references union), 3 project tests, mutation (`define` never triggers) fails the oracle at `file_cache.rs`; syntax 278, lib 1072, fixtures 3830 pass. Not run: workspace tests other than those, clippy; peak memory not measured.) | The rest of P8: the parser skims context-file bodies to the matching `}` and rewinds to a full parse on a trigger. | −847.6 M measured | P8a; D6 | A debug build parses every skimmed body eagerly and asserts equal summaries, flows, declarations, global facts and references (`file_cache::assert_same_but_bodies`); a debug build of a built-once project asserts the scopes equal with the references completed. |
| P9 | DONE (c0dd2788; measured: off run 22,362,774,991 Ir against 23,216,039,715 at P7's commit, −853.3 M, Piwigo output byte-identical; keyed by names, not by item 4's ids, so it did not wait for them. `Registry::function_is_pure`, `method_is_pure`, `function_property_writes` and `method_property_writes` take the `BodySource` the checker holds and search from the declaration asked about: an iterative depth-first search over the calls of its body, a call back into a declaration the search is inside of answering impure / `Anything`, which is the least fixed point because a declaration in or reaching a cycle never leaves it; every declaration the search settles is remembered in `Registry::demand` (two `RwLock`ed maps, nothing held while a search runs, a copy of a registry starts empty). The eager passes, `purity_dependencies`, `is_transitively_pure`, `clear_purity`, `property_writes` and `property_write_classes` are gone from the build. The incremental path takes the answers found since the last finish (`reset_demand`) and, once the declarations have settled, finds each again and marks the class or function whose answer differs (`changed_since`), so a file is re-checked on the same ground as before, for the answers something read. A debug build settles every declaration at once, as the build did, and each answer given on demand is asserted equal to it wherever the bodies at hand are the build's. Tests: `registry::tests::demand` (7: chain, cycle asked from any member, impure dependency, methods, `NoBodies`, property-write verdicts, order of askers), one new fixture (a cycle of methods with a return type), lib 1056 and fixtures 3829 passing in the test profile; both mutation checks (a stack hit answering pure; a stack hit answering `Nothing`) fail the oracle and the new tests. Not run: workspace tests other than the two, clippy.) | Purity and property writes on demand: a memoized depth-first search from the declaration asked about (least fixed point), the memo cleared on each edit; `check_declared_purity_tag` tests the `@phpstan-impure` / `@impure` tag first. | −853.3 M measured (estimated ≈ −0.3 to −0.4 G beyond item 4) | — | A debug build runs the eager pass and compares every answer given on demand; order of askers does not matter. |
| P10 | SKIP (not kept: built, measured and reverted, nothing committed. `simdutf8::basic::from_utf8` in the file read, the text then made with `to_owned` because the workspace denies `unsafe` and so cannot turn the checked bytes into a `String` without checking them again: off run 23,215,574,656 Ir against 23,216,039,715 at P7's commit, −0.47 M against the estimated ≈ −22 M, Piwigo output byte-identical. The copy costs what the faster check saves. Wall time not measured; Ir shows no gain to look for) | Check UTF-8 with SIMD (`simdutf8`). | ≈ −22 M (estimated) | — | The check stays; the `LineIndex` `Utf8Chunks` is item 11's. |
| P11 | SKIP (not kept: built, measured and reverted, nothing committed. Both parts built: (a) `Diagnostic.message` became a `Message` that dereferences to `str` and can be made by a closure on first read, with `report_lazy` used by the `offsetAccess.mightNotExist` report, and (b) `class_constant_type` returning the declared type while `dynamic` is empty. Off run 23,215,388,146 Ir against 23,216,039,715 at P7's commit, −0.65 M against the estimated 22.5 M + 6.6 M, Piwigo output byte-identical. The 22.5 M was 55 messages at ≈ 409 K Ir; the cause is not traced; the likely one is that E10 now drops, before formatting, the reports made inside bodies that call `extract()`, where B3 found most of the 55 were discarded. The one annotated `report_missing_offset` call line in the P7 profile costs ≈ 12 K Ir. What is left of (a) formats a message only for a rare setting (`reportPossiblyNonexistentGeneralArrayOffset`), at the price of a new message type touching 76 readers, so it is not kept without a measurement that shows it) | (a) A reported message formatted when displayed (a template and its operands in `Diagnostic.message`); (b) class-constant keys built only when a dynamic constant is configured. | 22.5 M (a), 6.6 M (b) | — | A message's text must not depend on state that changes after the report; the baseline and `ignore_errors` compare the displayed text; (b) folds the class name as `ConstantResolver::new` does. |

### A5. Cuts the investigations found (§10.13)

| id | status | task | estimated saving | needs |
|---|---|---|---|---|
| E2 | DONE (60b0cd33; measured: off run 23,812,168,909 Ir against 23,866,820,730 at P4's commit, −54.7 M against the estimated ≈ −50 M, Piwigo output byte-identical to the base's; the same binary also carries E14's parser change, which changes nothing on Piwigo; tests, clippy and fixtures not run) | A statement's assigned and defined names walked only when it has `@var` tags. | ≈ −50 M | — |
| E3 | DONE (07a0f111; measured: off run 23,785,065,626 Ir against 23,812,168,909 at E2's commit, −27.1 M against the estimated ≈ −35 M, Piwigo output byte-identical; tests, clippy and fixtures not run) | A method or static call probed for a PHPUnit assertion only when its written name starts with `assert`. | ≈ −35 M | — |
| E4 | SKIP (not kept: built once and reverted, nothing committed. Judged on ten general trees under `vendor/` with alternating repeats: the warm saving is 2-5% CPU, inside the ±10% spread of one run; a cold run costs +14% on composer and +9% on rector-src for the extra entries written; the cache grows by about 7 KB per file (137 MB on symfony). Not measured: a project whose analysed files change between runs, where hits are rarer and each changed file still pays the write, so it would be no better. The evidence is below; the E4 binary is `~/.cache/prs-profile/p4/e4-phpstan-rs`. Earlier text of this row: Piwigo output byte-identical in cold and warm; a hit for an analysed file reads the entry and parses the tree: warm 14,199,535,864 Ir against 14,223,265,225 without it (−23.7 M, where −250 to −300 M was estimated), cold 25,773,237,521 against 25,360,553,359 (+412.7 M for storing 2,792 more entries, 98 M to 116 M of cache); the decode of an analysed file's entry costs about what the steps it replaces do, so the saving on this one corpus is small. Not decided: how the cold store and the warm hit trade on a project whose analysed files change between runs. A debug oracle that a hit's products encode to the same bytes as a full parse's passed on 215 files.) | The analysed files stored in the per-file cache too. | ≈ −0.25-0.3 G net, per-file cache warm | — (debug check that a hit's products encode to the entry's bytes) Ten other trees under `vendor/` (`check-project --no-composer .`, so every file is analysed, E4's best case; the pre-E4 and the E4 binaries, one cold run and the mean of three warm runs each, `nice -n 19`, loadavg not logged; wall time is too noisy to read, laravel's pre-E4 run took 22 s against 8 s cold with E4): output identical in all ten; warm CPU time (user + system) −4% to −10% on eight of them (guzzle, wordpress, psalm, league-commonmark, rector-src, drupal, laravel, symfony), +4% on composer and +14% on doctrine-orm; cold CPU time +0% to +26% (doctrine-orm) where it rose; the cache grew to 2-137 MB (about 7 KB per file). Single runs; `~/.cache/prs-profile/e4/corpora/`. doctrine-orm repeated: four alternating rounds (start order swapped each round), each a cold run and two warm runs per binary, loadavg 0.5-2.7, output identical every round: warm CPU 8.73 s without E4 against 8.56 s with it (−2%, round values 7.5-10.2 s, so inside the noise), cold CPU 9.17 s against 9.27 s (+1%), wall 1.55 s against 1.58 s; the +14% and +26% of the first run were noise; `~/.cache/prs-profile/e4/repeat/`. The other trees that rose, repeated the same way (four alternating rounds, output identical, loadavg in the range given): composer (0.7-1.9) warm CPU 3.88 s against 3.82 s (−2%), cold 3.83 s against 4.35 s (+14%, higher in 3 of 4 rounds); rector-src (1.8-4.7) warm 10.96 s against 10.78 s (−2%), cold 11.39 s against 12.43 s (+9%, higher in 3 of 4); wordpress (4.7-6.0) warm 7.88 s against 7.46 s (−5%), cold 8.08 s against 7.93 s (−2%); psalm (5.8-6.9) warm 23.33 s against 22.85 s (−2%), cold 23.45 s against 23.98 s (+2%, rounds 20.7-27.0 s). So the warm rises were noise and the warm saving is 2-5% on these trees, within the spread of single runs; the cold cost is real on composer and rector-src (the extra entries are written) and absent on wordpress and psalm; `~/.cache/prs-profile/e4/repeat/`. |
| E5 | DONE (5ec70c1e; measured: off run 23,725,134,207 Ir against 23,785,065,626 at E3's commit, −59.9 M against the estimated ≈ −30 M, Piwigo output byte-identical; the kind (a family code 1-8, plain `mixed`, `never`) and the scalar-value bit are set in `reference_flags` and masked out of what a node passes up to its parent; `scalar_family` and `is_scalar_value` read them, so `union.rs` and the other readers got cheaper too; a unit test checks the kind against the form on the samples, NAN, class-kind strings and nested forms, and the types crate's shortcut tests pass; clippy clean on the types crate, the workspace tests and fixtures not run) | A kind code in the type node's free flag bits. | ≈ −30 M | — |
| E6 | DONE (f2f65cbb; measured: off run 23,701,951,690 Ir against 23,725,134,207 at E5's commit, −23.2 M against the estimated ≈ −10 M net, Piwigo output byte-identical; the test is `Arc::ptr_eq` on the two nodes, first in `is_super_type_of`, and answers Yes; a subtracted `mixed~S` against itself, which the form-only answer gave Maybe, is now Yes, so the two existing tests that compared with the form-only answer expect Yes for equal types, and `a_type_is_a_super_type_of_itself` is new; the types crate's 275 lib tests pass; the workspace tests, clippy and fixtures not run) | One node against itself answered first in `is_super_type_of`. | ≈ −10 M net | — |
| E7 | DONE (60e85393; measured: off run 23,672,079,898 Ir against 23,701,951,690 at E6's commit, −29.9 M against the estimated ≈ −55 M, Piwigo output byte-identical; item 4's index is not built, so the class's type is kept in a `OnceLock` inside `ClassReflection` (left out of its comparison, copy and stored form, and checked against the declared name on each read); a class the registry does not know is built each time as before; `Throwable` is a static and the fixed class lists of `instance_of_any` come from a per-thread table keyed by the literal, which needed the callers' class names to be `'static`; `int_literal` answers −1..=255 from a table; unit tests for the table and for the class's kept type, the types crate's integer-literal test and the three registry tests pass; the workspace tests, clippy and fixtures not run) | Plain object types kept with their class, two statics, a table of small integers. | ≈ −55 M | item 4's slot; item 16 overlaps |
| E8 | OPEN | A cache file's types stored once, in a table its records index. | ≈ −65 M; ≈ −25 M after item 6 | item 6, steps 3 and 5 |
| E9 | SKIP (folded into the plan's F6f, `PROFILE_DRILL_PIWIGO_20G.md:1705`) | The regex scanner's concatenation. | — | — |
| E10 | DONE (e081dbe9; measured: off run 23,619,596,541 Ir against 23,672,079,898 at E7's commit, −52.5 M against the estimated ≈ −35 M, Piwigo output byte-identical; `report` and `report_builder` return at once while the body's own flag or an enclosing body's is set (`enclosing_dynamic_bodies` counts the saved flags that are true), and `check_statement_var_tag_names` returns before formatting; the other pushes to `diagnostics` stay and are truncated as before; a debug build reports as before and records each report made while dropping, prunes the record at every truncation and rewind, and asserts at `into_diagnostics` that none is left, which fails when one prune is removed; seven new tests (extract, eval, a function declared in such a body before and after the call, a body after one, a script, a method before a top-level `extract`) and the six `extract` fixtures pass; the workspace tests and clippy not run) | Nothing reported while an enclosing body has called `extract()` or `eval()`. | ≈ −35 M on Piwigo | — (debug assertion that the truncation drops exactly what the count skipped) |
| E11 | OPEN | A cached file's origins decoded as the one `Arc<Path>` its file binds. | ≈ −116 M, per-file cache warm | E8's decode context |
| E12 | SKIP (not kept: built, measured and reverted, nothing committed; the patch is `~/.cache/prs-profile/e12/e12.patch`. Warm per-file cache, Piwigo, the binary at E10 against the same with E12: total 14,039,357,749 Ir against 14,039,334,519 (−23 K), output byte-identical in cold and warm. The main thread's `insert_built_decl` fell from 471.9 M to 52.1 M, but the 420 M of method decoding moved to the worker threads, because later passes read those methods anyway, so the report's estimate of ≈ −0.35-0.4 G assumed methods that nothing reads later. Wall time, `hyperfine -N`, 8 runs after 2 warmups, three alternating rounds, `nice -n 19`, loadavg 2.6-3.5 throughout: mean 1.806 s (rounds 1.840, 1.745, 1.834) against 1.796 s (1.801, 1.750, 1.838), no difference. Nothing is saved in instructions or in wall time, so by the rule that a saving is claimed only from a measurement the change is not kept) | Each class's assert-tagged method names carried in its `BuiltDecl`. | ≈ −0.35-0.4 G serial, per-file cache warm | — |
| E13 | DONE (c2cd1586; measured: cold run with the per-file cache on, 26,149,980,863 Ir against 26,174,556,141, −24.6 M, same 11,459 entries in 256 shards, output identical; the mkdir and getpid syscalls are not counted by Ir and were not measured) | The per-file cache's shard directories made once, by D5's check; the process id read once. | ≈ −80 ms kernel CPU, ≈ −10 ms wall | D5's check |

### A6. Left unmeasured or untraced (§12, §10.8, §10.11)

| # | status | what | how it would be settled |
|---|---|---|---|
| U1 | OPEN | The parser against the whole corpus: every file PHP accepts and the parser refuses. E14 came from two warnings only. | One `phpstan-rs check` over the 111,836 files under `vendor/`, against D6's `php -l` verdicts (`~/.cache/prs-probe/d6/`). A run of the binary: needs the user's go-ahead. |
| U2 | OPEN | Parser gaps already known (D6): `$s{0}` (valid in PHP 7; the postfix loop has no arm for `{`, `syntax/src/parser/postfix.rs:16-241`); `(real)` and `(unset)` accepted (`syntax/src/ast.rs:1303`, `:1311`) though PHP 8 refuses them. | Fix with the parser, with tests; decide per gap whether the parser follows the configured PHP version (D6). |
| U3 | OPEN | Which of C7's 238 compile-time errors the parser does not report the checker already reports. | Count by running each of the 299 trigger files (`~/.cache/prs-probe/c7/cases/`) through `check`; the verdicts are in `c7/verdicts.tsv`. |
| U4 | OPEN | E1's deterministic reproduction. | Designed in §10.14, not run; becomes E1's regression test. |
| U5 | OPEN | E10: which facts that outlive an `extract()` body (property assignments, class-level sites) a dropped body still needs. | Trace in `function_decl.rs` and `checker.rs` before widening E10 past the report calls. |
| U6 | OPEN | Which truncation holds the 33 `offsetAccess.mightNotExist` reports that `finalize` does not drop (B3 counts them among the discarded reports but not by site). Decides whether P11a's truncation saving is "well under 1 M" or ≈13 M. | Record the truncation site in the B3 probe (`~/.cache/prs-profile/off-ledger-b/probe.patch`); needs a probe build. |
| U7 | OPEN | Readers left out of d: the uninitialized-property rules on, or a tree with test classes whose built-in parent-call hook rules fire, add the ancestors' bodies to d. | Re-run B1's probe on such a tree; needs a probe build. |
| U8 | OPEN | B9's embed and edit timings were read off the console and not kept; the build script's cost under C8's form (every stub file parsed, no base) is not measured; nothing ran on aarch64 or macOS (B10's backstop is designed, not run). | Only if P6 is revived (D3). |
| U9 | OPEN | A third pass on the report's claims the review workflow could not check (102 items). | Hand check against the code and the profiles, as the 104 confirmed and the 17 split findings were. |

