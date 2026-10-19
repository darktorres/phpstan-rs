# Piwigo performance ledger

Every optimization task for the Piwigo profile on this branch (`callgrind-20g`), grouped by the part of the program it changes, with its state and what blocks it. State as of 2026-10-17; the code is at `8a1ca1e0`.

This is the one list of the branch's tasks. The documents the tasks come from stay as their design texts. Their status claims are not kept up; this ledger's are.

- **The off drill.** `PROFILE_DRILL_PIWIGO_OFF.md` holds its rows' designs, and `PROFILE_DRILL_PIWIGO_OFF_HEAP.md` holds its heap row, H1. The retired `PROFILE_DRILL_PIWIGO_OFF_BUILD_LEDGER.md` (deleted in `d08c9a76`) tracked those rows. Their text is kept whole in this ledger's records, because it holds the evidence for what is done and what was skipped. One status in it was corrected, D7's: the proposals it recommended are built. Its rows, by the off report's sections:
  - decisions that gate the build, D1-D7;
  - defects in the code, E1 and E14-E16 (§10.14);
  - proposals, P1-P11 (§10.1-§10.11);
  - cuts the investigations found, E2-E13 (§10.13);
  - questions left unmeasured or untraced, U1-U9 (§12, §10.8, §10.11).
- **The 20 G plan.** `PROFILE_DRILL_PIWIGO_20G.md`, Part II: twenty items (0-19), written at `5fb858c9`, before the off drill's rows changed the code under them. Each is re-evaluated here against the code and the latest profile.
- **The 25 G report's unbuilt designs:** the designs of `PROFILE_DRILL_PIWIGO_25G.md` that the 20 G report's §2 lists as not built, or built otherwise. Each is carried by a 20 G item; Appendix B maps them.
- **The crate split.** `CRATE_SPLIT_PLAN.md`, Stage C from C4 on (C4, C5 and C10), worked on this branch since 2026-10-15. Its states were audited against the code on 2026-10-15: the module graph's `finished` line is `A0 A1 A2 B1-B9 C1 C2 C3`.
- **The items 1 to 3 fixes.** `ITEMS_1_TO_3_FIXES_PLAN.md` (2026-10-17) fixes the costs that §1's attribution of the 20 G plan's items 1 to 3 found, and the serial stretches of the load that MEAS-4 measured. It holds each row's design, numbers and test; its rows are LOAD-8, LOAD-3.3 and CACHE-12.

## Conventions

**Areas.** Every task sits in the area of the program it changes.

| area | what it covers | section |
|---|---|---|
| LOAD | the session, the walk, the load's merge, how long a tree lives | §3 |
| SYN | the lexer, the parser and the tree | §4 |
| PROD | a file's products: declarations, summaries, flows, references | §5 |
| DOC | PHPDoc | §6 |
| NAME | names and interned values | §7 |
| TYPE | type nodes and the type relation | §8 |
| REG | the registry: declarations, resolution, snapshots, and the query engine | §9 |
| STUB | the stub build, the stub layer and the stub data | §10 |
| CACHE | the contents and formats of the stub cache and the per-file cache | §11 |
| CHECK | the check phase | §12 |
| VAL | validation | §13 |
| MEAS | the measurements that set targets | §14 |
| Q | not optimizations: open questions and unbuilt policies | §15 |

Each area has:

- a table of its tasks;
- a table of targets for its open tasks;
- notes on how its tasks overlap one another and those of other areas;
- records: the evidence for each of its tasks that is done or skipped.

**Ids.**

- `AREA-n` is a task, and `AREA-n.m` a sub-task of it.
- A task's state is the state of its sub-tasks, so progress is written on the sub-task's row.
- A need written on a task applies to each of its sub-tasks.
- Appendix A maps every id the earlier ledgers used to its id here.

**From.** Each row names its source:

- `20 G n` is item n of the 20 G plan, and `20 G n.s` is its design step s. Items 0, 6, 9, 11-14, 17 and 18 number their steps; the task text names the parts of the other items.
- `off X` is row X of the off drill, and `heap H1` is the heap drill's row.
- `25 G X` is a design of the 25 G report.
- `split C4 <part>` is a part of the crate split's C4, named by the plan's own heading for it: gates, gatetime, data model, written results, harness, memo, snapshots, staleness, tests, behaviour, gate. `split C5` and `split C10` work the same way.
- `decision n` is decision n of §2.4: a row that decision made, or rewrote.
- `fixes Rn` is row Rn of `ITEMS_1_TO_3_FIXES_PLAN.md`. That plan's decisions are cited with its name, since `decision n` is §2.4's.

**Ids from the sources are always cited with their document's name**, because the documents reuse ids:

- The off report's C1-C8 are investigations, the 25 G report's C1-C8 are designs, and the crate split's C1-C10 are stages.
- E4, P6, P7, P11 and F1 each name different things in two or three of the documents.
- For example, "the off report's P11" (CHECK-4) formatted messages on display and was measured and reverted, while "the 25 G report's P11 step 1" (DOC-2.3) is `TagEffect`, which was never built.

Inside a record, ids and section numbers are those of the record's source, as written there. In the off drill's records, "item n" is an item of the 20 G plan.

**States.**

| state | meaning |
|---|---|
| DONE (hash) | built and committed |
| READY | open, every need met |
| OPEN | not started, with a need not yet met |
| BLOCKED | waiting on a decision or a rule, named on the row |
| PART | partly done; the row says what is left |
| SUPERSEDED | another row did the work, or a decision gave the work to another row; the row names it |
| SKIP | not done; the row says why |

**Needs** are read from the source's text: Part II of the 20 G plan, and the crate split's plan. Some sources fix a build order: the 20 G plan numbers its items in the order it builds them, each closed by a tests-only gate, and the crate split orders C4's commits. Such an order is a process rule, not a need.

**Numbers.**

- A "target" is the inclusive cost, in millions of instructions, of the code a row would replace (calls in brackets). It is read from the P8b off-mode profile, or from the warm run where the row says so (MEAS-1). It is the most the row could remove, not what it saves.
- Inclusive figures of recursive functions count nested calls more than once, so they are not quoted.
- A "report's estimate" is its source's. For the 20 G items, it comes from the plan's §12 table, measured on the 20.57 G profile.
- Rows about memory or allocation counts say so.

**Rules.**

- One commit per row, made as soon as the row's own check passes, with a pathspec (`git commit -m … -- <paths>`).
- A row that changes behaviour or layout keeps the Piwigo output byte-identical to the base's (`LC_ALL=C sort` both before diffing), unless the row says why it cannot.
- A saving is claimed only from a measurement:
  - callgrind `Ir` summed over threads, against the previous row's commit, in off mode, on the `dhat` profile with frame pointers;
  - a run's total also holds work that follows the threads' schedule and not the code: mimalloc's cross-thread reclaim and the per-thread caches moved by −6.4 M and then +22.4 M across the runs of P7, LOAD-6 and CHECK-4, none of which touches them (CHECK-4's record). So a change of that size is judged by the inclusive `Ir` of the functions it changes, in its own profile against the base's, with the total beside it. MEAS-3 measured the spread: three runs of one binary at the default thread count differ by up to 16.9 M, and two at one thread (`PHPSTAN_RS_THREADS=1`) by 1.1 M. So a change under ≈20 M is also measured at one thread, where the total resolves about 1 M;
  - wall time by `scripts/abab.py` on `gate` binaries, under `nice -n 19`, with `/proc/loadavg` noted: pairs of runs whose order alternates (A B, B A, …), every output checked against the Piwigo oracle, judged by the paired difference with a Wilcoxon signed-rank test. It replaced `hyperfine -N`'s rounds on 2026-10-17, when the second command of each round ran slower than the first, whichever binary it was (`ITEMS_1_TO_3_FIXES_PLAN.md`'s Verification).
- Builds, profiling runs and `check-project` probes need no go-ahead since 2026-10-17 (the user's standing approval). Heavy jobs run one at a time.

## 1. Where the branch stands

| | |
|---|---|
| base | `5fb858c9`'s code (the 20 G plan's), off mode: 25,625,190,065 Ir |
| now | `8a1ca1e0`: 20,373,086,199 Ir, off mode, measured 2026-10-17; −5.25 G (−20.50%) against the base, and the lowest measured so far (LOAD-8.8's `f676049c` was 20,499,751,098, LOAD-8.13's `4a996352` 20,512,402,375, `ee56851c` 20,909,227,687 and `791d29cb` (PROD-3) 20,928,053,254); LOAD-8.10, a wall row, adds 69.9 M, most of it doc comments the check parses again (its row); LOAD-8.9 (`098af486`) changes only the editor's path, which no batch run reaches. Piwigo output not compared since PROD-3 (VAL-1 is deferred) |
| profile | `~/.cache/prs-profile/p8b/cg` (off mode: the stub cache and the file cache are off, so the 3.4 G stub build is in it) |

Where the 20.93 G sits (threads summed):

| part | Ir |
|---|---:|
| the parse job on the workers (`read_and_parse_file`, 14,251 files) | 8.66 G |
| the check (`check_files`, 2,792 files) | 6.42 G |
| the stub build, on its own pool | 3.18 G |
| the finishing passes, on their thread | 0.97 G, plus ≈0.7 G that other threads steal |
| the main thread | 0.71 G |
| the autoload thread | 0.32 G |

Of the parse job's `parse_file` cost, 78% is the project's files, 20% the stub files and 2% the autoload files.

After `791d29cb` the branch has the crate split's first C4 commits:

- REG-3's `58cfc5d2`, an `xtask` command;
- REG-4's `8d35c99d`, `7a61603e`, `5401afc9`, `e7b2521f` and `0f32829b`.

They change the registry's data model, and `e7b2521f` and `0f32829b` change the reports of the missing-type rules. No Ir was measured at them.

Since `791d29cb` the 20 G plan's items 1 to 5 have landed in part: LOAD-1 (item 1), LOAD-2 except its last deletion, LOAD-3.2, 3.4 and 3.6, and LOAD-4.3 to 4.5 (their rows in §3). Their instruction counts and heap were measured on 2026-10-16 and 17 (the rows below the table), and the wall time at `aa896697` on 2026-10-17 (MEAS-4). The Piwigo gate has not run. The items 1 to 3 fixes (`ITEMS_1_TO_3_FIXES_PLAN.md`, LOAD-8) started on 2026-10-17: LOAD-8.1 (`6016499f`), LOAD-3.3's load part (`a5a4d85f`), CACHE-12 (`6705eedd`), LOAD-8.15 (`d6ca3442`), LOAD-8.2 (`68af1c72`, `276ed5ad`), LOAD-8.3 (`75b628f8`), LOAD-8.4 (`1ba57cc0`), LOAD-8.6 (`59bf55bc`), LOAD-8.14 (`1f351379`, `6e49d4e7`), LOAD-8.13 (`4a996352`), LOAD-8.10 (`9513479b`), LOAD-8.5 (`0fb1a4de`), LOAD-8.8 (`f676049c`), LOAD-8.9 (`098af486`, the editor's) and LOAD-8.7 (`30aa2002`, `8a1ca1e0`) are in, each measured against the row before it: every row of the plan.

The rows that moved the Ir, in order (callgrind, off mode). Each row's output was byte-identical to the base's; each row's record has its checks.

| row | from | commit | Ir after | change |
|---|---|---|---|---|
| base | | `5fb858c9`'s code | 25,625,190,065 | |
| STUB-1 (with STUB-2, STUB-3) | off E1 (with P1, P5) | `3d5434b3` | 25,029,730,463 | −595.5 M |
| CACHE-1, CACHE-2, CACHE-3 | off P2, D5, E13 | `c2cd1586` | 24,498,907,030 | −530.8 M |
| STUB-4 | off P3 | `a13f4ed2` | 24,155,076,222 | −343.8 M |
| STUB-5 | off P4 | `0ce52479` | 23,866,820,730 | −288.3 M |
| CHECK-1 | off E2 | `60b0cd33` | 23,812,168,909 | −54.7 M |
| CHECK-2 | off E3 | `07a0f111` | 23,785,065,626 | −27.1 M |
| TYPE-1 | off E5 | `5ec70c1e` | 23,725,134,207 | −59.9 M |
| TYPE-2 | off E6 | `f2f65cbb` | 23,701,951,690 | −23.2 M |
| TYPE-3 | off E7 | `60e85393` | 23,672,079,898 | −29.9 M |
| CHECK-3 | off E10 | `e081dbe9` | 23,619,596,541 | −52.5 M |
| DOC-1 | off P7 | `a4035361` | 23,216,039,715 | −403.6 M |
| REG-1 | off P9 | `c0dd2788` | 22,362,774,991 | −853.3 M |
| PROD-1 | off P8a | `7726dacf` | 22,014,267,117 | −348.5 M |
| PROD-2 | off P8b1 | `11837a04` | 21,775,661,606 | −238.6 M |
| PROD-3 | off P8b | `791d29cb` | 20,928,053,254 | −847.6 M |
| item 1, LOAD-1 | | `428ed943` → `99b1c2e4` | 21,114,767,711 | +226.2 M: path keys, their hashing and component walks (`aab67287`, `7bef998a`; about +128 M of self cost); `walk::resolve`'s `canonicalize` of symlinks (+24.6 M, `aab67287`); the content hash of every file (+25 M, `3334ebc9`, LOAD-1.4) |
| the eight fixes (range; `fixes8`, one run) | | `99b1c2e4` → `ee56851c` | 20,909,227,687 | −205.5 M |
| items 2 and 3, 28 commits | | `ee56851c` → `d870ddcb` | 21,289,129,115 | +379.9 M: `845efaab` (LOAD-3.4) +394 M, the other 27 commits −14 M |
| item 5, LOAD-4.3 to 4.5 (range, unattributed) | | `d870ddcb` → `aa896697` | 21,331,572,685 | +42.4 M |
| LOAD-8.1 | fixes R1 | `6016499f` | 20,948,523,065 | −381.2 M against `aa896697`'s code built again and measured beside it (21,329,725,495); warm −136.9 M |
| LOAD-3.3, the load | fixes R11 | `a5a4d85f` | 20,809,876,336 | −138.6 M: mimalloc's cross-thread frees; the main thread −265.0 M |
| CACHE-12 | off E11; fixes R12 | `6705eedd` | 20,806,948,614 | −2.9 M, inside the noise: a warm row. Warm −31.8 M (13,209,699,103 → 13,177,851,490); the main thread −394.7 M warm |
| LOAD-8.15 | fixes R15 | `d6ca3442` | 20,803,639,377 | −3.3 M, inside the noise: a wall row. Off −21.6 ms (60 pairs, p = 0.031) for +18.9 MiB of peak RSS; the 745 purges (105.5 MiB) are gone |
| LOAD-8.2 | fixes R2 | `68af1c72`, `276ed5ad` | 20,785,126,006 | −18.5 M: the walk's tree finds a file's directory by the hash its path carries (`Walk::merge` 50.4 M → 29.8 M), and the listings take the carried hash (`DirListings::listing` 31.9 M → 19.6 M) |
| LOAD-8.3 | fixes R3 | `75b628f8` | 20,704,757,165 | −80.4 M: a directory's reach is made from its parent's, where every directory scanned its ancestors (`Starts::reaching` 74.8 M → 18.3 M inclusive, `Walk::prunes` 14.5 M → 0, for `Starts::below`'s 7.2 M); `Path::hash` −57.3 M of self cost |
| LOAD-8.4 | fixes R4 | `1ba57cc0` | 20,657,737,639 | −47.0 M: the hand-over finds each file's parse by the hash its path carries (the main thread −27.8 M, its `Path::hash` 53.4 M → 26.1 M); the other −19.2 M is spread over the pool, inside MEAS-3's spread |
| LOAD-8.6 | fixes R6 | `59bf55bc` | 20,533,513,526 | −124.2 M: vendor discovery places a path by its directory and reads kinds from the listings (its thread 391.0 M → 262.0 M: the drop and deduplication 119.2 M → 23.9 M, `Resolver::path` 97.8 M → 63.4 M). Warm −130.6 M, and the warm vendor wait −38.3 ms (p = 0.011) |
| LOAD-8.14 | fixes R14 | `1f351379`, `6e49d4e7` | 20,522,048,368 | −11.5 M: the walk takes the vendor paths that discovery resolved (the main thread −7.9 M, `Walk::add_file` 6.5 M → 0), and its `readlink`s fall from 7,478 to 3,055 and its `statx` calls from 2,231 to 238. The main thread's stretch before the walk, 16 to 20 ms → 0 to 4 ms; warm −65.6 ms (p = 0.033), more than that stretch can give |
| LOAD-8.13 | fixes R13 | `4a996352` | 20,512,402,375 | −9.6 M: a warm row; off, the discovery thread −5.7 M, as it builds each path in one buffer. Warm −181.3 M (12,894,418,032 → 12,713,085,018), all of it the discovery thread's: a warm load reads the generated files' entries from the per-file cache, and `parse_generated_file`'s 163.0 M is gone. The warm vendor wait fell from about 85 to 38 ms in two recordings each, with no wall change beyond the noise (p = 0.709) |
| LOAD-8.10 | fixes R10 | `9513479b` | 20,582,303,714 | +69.9 M: a wall row. The walk's own code +0.9 M; the check parses 1,899 more doc comments (+54 M), since the memo they are read from is bounded and the context files, parsed last now, fill it. Warm −65.4 ms (p = 0.017): the parse starts at once, beside vendor discovery, where the pool waited 42 to 48 ms for it |
| LOAD-8.5 | fixes R5 | `0fb1a4de` | 20,535,375,165 | −46.9 M: the use sites are kept by the file's number, so the merge routes a file's sites by it, and `judge_property_types` tests a set of numbers (`Path::hash` 50.4 M → 21.0 M: `file_tables::bucket`'s 13.65 M and the judgement's 13.65 M are gone); no wall change beyond the noise (p = 0.601) |
| LOAD-8.8 | fixes R8 | `f676049c` | 20,499,751,098 | −35.6 M: a project's diagnostics are kept by the file's number until the report, which names each file from the table once (`Path::hash` 21.0 M → 1.3 M, the report's own map; `path_hash` 5.4 M → 0, as the check hands `checker_for` the number it holds); no wall change beyond the noise, off (p = 0.940) or warm (p = 0.247) |
| LOAD-8.7 | fixes R7 | `30aa2002`, `8a1ca1e0` | 20,373,086,199 | −126.7 M: a path's name, its directory, a prefix and what is left after it are read from its bytes (`composer::path_bytes`), where `Path` walked its components, and the walk carries what it holds (each directory's place among the packages' directories, the file roots of one directory as one job); std's path code 117.4 M → 11.5 M of self cost, its entries by caller 140.9 M → 16.2 M, for 14.2 M of byte reads. The second commit −2.9 M, inside MEAS-3's spread (its path code −12 M by function). No wall change beyond the noise, off (p = 0.313) or warm (p = 0.627) |

The fixes' rows are measured against the row before them, both built in a worktree of their own (`~/.cache/prs-profile/fixes/`): there `aa896697`'s code measured 21,329,725,495, 1.8 M under `regress-item5`'s run, inside MEAS-3's spread. The four range rows are measurements of commit ranges, not rows. Item 1's and items 2 and 3's are attributed; the eight fixes' and item 5's are not. Each is one off-mode callgrind run per commit with default threads, on the dhat profile, from `~/.cache/prs-profile/`: `item1-regress` (base and head), `fixes8` and `regress-item5`. MEAS-3 puts the noise at up to 16.9 M. The +380 M of items 2 and 3 is attributed (the 28-commit row): `845efaab` (LOAD-3.4) adds a lossy UTF-8 decode of every file's text, for its length, in `read_and_parse_file` (`session.rs:1224`), which is 394 M of `Utf8Chunks::next`'s callers in the off run. The other 27 commits net −14 M. The function diff came from `callers.py` and `cgfn.py` on the two runs' callgrind files, with no new build.

Warm runs (`regress-item5`): `d870ddcb` 13,382,535,920, `aa896697` 13,346,910,728 (−35.6 M). Heap at one thread (`regress-item5`): `d870ddcb` 2,913,109,581 bytes allocated, peak 807,381,174, exit 647,439,034; `aa896697` +23.7 M allocated (+0.82%), peak +2.2 M, exit −5.8 M. Item 1's heap was measured at default threads, so its deltas (allocated +2.7 M, peak +7.7 M, exit +5.7 M) are not comparable with item 5's.

Wall time was first measured on 2026-10-17, at `aa896697` (MEAS-4): an off run takes about 2.3 s and a warm run about 1.6 s on this machine, a VM with 8 vCPUs, and leaves 0.75 s and 0.85 s of the 8 cores unused. At `a5a4d85f` the off run's median is 1.796 s, from LOAD-3.3's 20 pairs, and at `6705eedd` the warm run's is 1.322 s, from CACHE-12's. The Piwigo output is the same at `d870ddcb` and `aa896697` (171,439 bytes, sha256 `8ff144c3…`, off and warm), so item 5 changed no diagnostic; it has not been compared with PROD-3's.

Defects fixed along the way:

- SYN-5 (off E14), `6dd06124`: `instanceof` with a static property as the class.
- CACHE-4 (off E15), `efe621ad`: the stub cache key left out `vendor/phpstan-src/stubs`.
- CACHE-5 (off E16), `480107e3`: maps and sets of names wrote in table order.

The off drill's decisions: D6 is decided (skim unchecked, 2026-10-15; PROD-4.3), and so are D1, D2 and D5 (PROD-4.1, PROD-4.2, CACHE-2). D3 was decided again on 2026-10-15 as §2.4's decision 7 (STUB-10): the analyser's builtin knowledge is built at compile time, and PHPStan's own classes are read from the PHPStan the project installs.

Five of the off drill's rows are open:
- CACHE-8 (READY);
- CHECK-4, re-opened: CHECK-4.2 is READY, and CHECK-4.1 is built with CHECK-6.10;
- STUB-10, whose measurements (STUB-10.1) are left, and STUB-11, which waits on them and on the reflection crate;
- CACHE-11, which waits on CACHE-9 and CACHE-10.

E9's cut is CHECK-6.16's (READY). The off drill's questions (U1-U9) and D6's unbuilt policy are §15's Q-1 to Q-10: Q-1 to Q-7 and Q-9 are done (Q-2's fix is SYN-6, and Q-4's regression test is in the tree as `55d9ea1f`), Q-8 is superseded by STUB-10.1 and STUB-11, and Q-10 is open. The rest of its rows are done or skipped.

None of the 20 G items is built whole. Four of the off drill's rows changed what some of them would save:

| off drill row | 20 G item | its task here |
|---|---|---|
| DOC-1 (off P7) | item 14 | DOC-2 |
| PROD-1 to PROD-3 (off P8) | items 3 and 13 | LOAD-3, PROD-5 |
| REG-1 (off P9) | item 4 | REG-13 |
| TYPE-1 to TYPE-3 (off E5-E7) | items 7 and 16 | TYPE-4, NAME-2 |

One part of an item is built, by the crate split: item 8's explicit-mixed flags (REG-14.2, delivered by REG-4.8).

On 2026-10-15 the eighteen decisions of §2.4 were taken. They:

- retire item 4 (REG-13), whose wanted parts move to REG-4.15, REG-7.4 and REG-8.3;
- retire SYN-1.6's `unsafe impl Sync`;
- retire the merging of declarations (REG-11.14), and F2's count (STUB-9) for the engine's query trace (REG-16.5);
- add:
  - one incremental query engine (REG-16), whose persistence is CACHE-13.2;
  - the record of the names a file writes (PROD-5.8);
  - parsed doc blocks as a per-file product (DOC-2.9);
  - the places a constant's throws land (CHECK-7);
  - the sources of the builtin knowledge (STUB-12);
  - PHPStan's own classes read from the installed PHPStan (STUB-13);
  - two parts of the flat tree (SYN-1.9, SYN-1.10);
  - `iterable<K, V>`'s array keys (TYPE-5);
  - what a body shows, inferred on demand (REG-17);
  - the reports of a kind a PHPDoc leaves out (CHECK-8, opt-in) and of a dead catch (CHECK-9);
  - the engine read over a version range (CHECK-10);
  - the output changes they bring (VAL-2.10);
- re-specify how a declaration's native and PHPDoc types combine (REG-7.5, REG-7.12), how a builtin's sources combine (STUB-12), how a name binds to its declarations (REG-8.1, STUB-7.4 to 7.6, STUB-8), when a file's `stat` stands in for its bytes (CACHE-13.1), and which versions a file and the builtin knowledge are read over (Q-10, STUB-7.1, STUB-12.2);
- approve the crate split's output changes that had no approval, correcting STUB-7.2 and REG-8.5 (VAL-2.9);
- unblock SYN-1, PROD-5.1, TYPE-4, REG-4.11, REG-7.6, REG-7.7, REG-11.1, REG-14.1, REG-14.3, STUB-7.1, STUB-10 and STUB-11.

Four questions that a reading of the whole ledger found the decisions had left open, §15's Q-12 to Q-15, were then decided as decisions 15 to 18: a file's syntax against a version range, a builtin's result before PHP 8.0 when an argument fails its parameter parsing, an inferred return's literals, and the oldest version the builtin knowledge covers.

## 2. At a glance

### 2.1 Ready now

These rows are open with every need met:

- **CACHE-8** (heap H1), a memory row. It and CHECK-4.2 (off P11, under CHECK below) are the two off-drill rows nothing blocks.
- **LOAD:** LOAD-2.2's last part (the name floor's replacement, decided on 2026-10-17) and LOAD-5.4; LOAD-9. The fixes plan's rows (LOAD-8, LOAD-3.3, CACHE-12) were built since the user's second go (2026-10-17), in the plan's order (its decision 4), and all are in: LOAD-8.1, LOAD-3.3's load part, CACHE-12, LOAD-8.15, LOAD-8.2, LOAD-8.3, LOAD-8.4, LOAD-8.6, LOAD-8.14, LOAD-8.13, LOAD-8.10, LOAD-8.5, LOAD-8.8, LOAD-8.9 and LOAD-8.7. The plan's end-of-batch checks passed the same day (LOAD-8's record).
- **SYN:**
  - SYN-1.1 (`peeled` becomes a loop) and SYN-1.2 (the flat tree, decision 4);
  - SYN-3.1, 3.2 and 3.3;
  - SYN-4.2, 4.4, 4.5, 4.6, 4.8, 4.9 and 4.10.
- **PROD:** PROD-5.2, 5.3, 5.5 and 5.6; PROD-5.8 (the record of written names, decision 2).
- **DOC:** DOC-2.3, 2.4, 2.7 and 2.8. DOC-2.1 is ready but deferred (2026-10-16).
- **NAME:** NAME-1.1, 1.2, 1.5 and 1.6; NAME-2.1, 2.2 and 2.3.
- **TYPE:** TYPE-5 (`iterable<K, V>`'s array keys, decision 8).
- **REG:**
  - the crate split's C4, in its order: REG-2; the open parts of REG-4 (4.2, 4.5, 4.7, 4.9, 4.10 and 4.12 to 4.14; 4.11 waits on REG-8.1, REG-8.5 and REG-10, and 4.15 on 4.2 and 4.12); what is left of REG-5; REG-6; then REG-7 from REG-7.1, once REG-4.2 and the engine (REG-16.3) are in;
  - REG-16.2, the engine's interned identity (decision 3), now that REG-16.1 has measured the engine;
  - REG-14.4 and 14.5;
  - the crate split's C1-C3 follow-ups left (Q-11): LOAD-7.1 and 7.3 to 7.7, REG-18.1 to 18.6, and CHECK-11.
- **CHECK:** CHECK-4.2; CHECK-6.1 (approved in principle), 6.2, 6.4, 6.5, 6.6, 6.8 to 6.12, and 6.16 to 6.18; CHECK-9 (the dead catch, decision 9).

### 2.2 Chains

- **The load (the 20 G plan's items 1, 2, 3 and 19):** LOAD-1 → LOAD-2 → LOAD-3 → LOAD-5. Roles become final before the parse, then trees are dropped by role, then each is freed after its last reader.
- **The items 1 to 3 fixes (LOAD-8):** LOAD-8.2 → 8.3, 8.4, 8.6 and 8.14; LOAD-8.6 → 8.14; LOAD-8.3 → 8.10; LOAD-8.5 → 8.8; LOAD-8.2 to 8.6 and 8.8 → 8.7.
- **The crate split:**
  - REG-4.2 and the engine (REG-16.3) → REG-7 → REG-8 → REG-9 and REG-10 → REG-11 → REG-12;
  - REG-4.2 and REG-4.12 → REG-4.15;
  - REG-18.2 and REG-18.3 → REG-7.6; REG-18.6 → REG-8.4; REG-18.2 → REG-18.7;
  - REG-8.1, REG-8.5 and REG-10 → REG-4.11;
  - then STUB-7 (C5) → STUB-10.1 → STUB-11;
  - REG-15 (C10) → STUB-11.
- **What follows REG-7 and STUB-7:**
  - REG-16.3 → TYPE-4 → NAME-2.4;
  - REG-7.6 → CHECK-7;
  - REG-7.5 → CHECK-8;
  - REG-16.4 → REG-17.1 → REG-17.2, 17.3 and 17.4;
  - STUB-7.6 and REG-16.1 → CACHE-9 and CACHE-10 → CACHE-11;
  - STUB-7 → STUB-8;
  - STUB-7.1 → STUB-12 and CHECK-10; STUB-7.4 → STUB-13;
  - STUB-7.3 → PROD-5.4.
- **The engine:** REG-16.1 (the measurement) → REG-16.2 → REG-16.3 → REG-16.4 → CACHE-13.2 (the persistence). REG-16.3 → REG-16.5 (the query trace). REG-16.4 also needs SYN-1.2 and LOAD-1.4; it replaces LOAD-1.6 and LOAD-5.3, and LOAD-3.1 follows it.
- **The tree:** SYN-1.2 → the rest of SYN-1 → SYN-2 → SYN-4.1. SYN-1 also leads to PROD-5.1 → REG-11.1, to LOAD-3.5, to REG-16.4, and to CHECK-6.3 → CHECK-6.13 and 6.14. PROD-5.1 and STUB-7.3 → SYN-1.10.
- **Trees freed:** PROD-5 and CHECK-6.1 → LOAD-5.1 → LOAD-5.2.
- **Names and PHPDoc:**
  - NAME-2.1 → NAME-1.4;
  - NAME-1 → DOC-2.5;
  - PROD-5.2 → DOC-2.2 and PROD-5.7;
  - PROD-5.8 → PROD-5.7 and REG-14.1;
  - REG-4.10 → DOC-2.9.

### 2.3 The biggest targets

Measured by what they would replace, the biggest are:

| task | what it replaces | estimate |
|---|---|---|
| the check phase (CHECK-6) | ≈6.4 G of work | ≈1.6-2.0 G |
| the token plumbing (SYN-2) | 2.9 G of functions | ≈1.7 G |
| the per-file declarations and references (PROD-5) | 1.7 G of reflection building, plus 0.27 G of references | ≈0.45 G |
| the lexer's own work (SYN-4) | identifiers, keywords and operators (§4's targets) | ≈0.35 G |
| name hashing (NAME-1) | ≈0.63 G of folding | ≈0.6 G |

The estimates are the 20 G report's. REG-13's estimate no longer held, and REG-13 is superseded (decision 3).

### 2.4 Decisions

Merging the sources into one list found six places where two of them disagree, or where a design cannot be built as written. A seventh decision waited from before the merge. All seven were decided on 2026-10-15, on what PHP does at run time and on what is most proper; churn and PHPStan's output were given no weight. Eleven more were decided the same day on the same grounds: the eighth to the tenth (REG-7.5's combination of native and PHPDoc types, what a body shows inferred on demand, how the sources of a builtin's types combine), and the eleventh to the eighteenth, which settle what a reading of the whole ledger found open (how a name binds to its declarations, STUB-9's count, when a `stat` stands in for a file's bytes, the crate split's output changes that had no approval, and Q-12 to Q-15). Rows cite them as "decision n".

1. **What evaluating a constant yields.**
   - *The question was* how a constant on a reference cycle types. REG-7.6 typed it as `mixed`, an approved delta of the crate split; TYPE-4.2 read each member as `never` and reported it, approved as VAL-2.5.
   - *The outcome.* The resolver's result for a constant is its value type plus the throws its evaluation can raise, each definite or possible.
     - Definite: a cycle over value edges ("Cannot declare self-referencing constant"), or arithmetic that fails on literal operands (`1 % 0`, a `DivisionByZeroError`). Every read is `never`, and the throw is its throw point.
     - Possible: a constant or a class that no known declaration names. The value stays unknown and the throw possible, because code the analysis has not seen may declare it.
   - *Where it lands (CHECK-7).* Every place PHP evaluates the constant inherits the outcome:
     - a read, and `defined()`;
     - a parameter's default, where the argument is omitted;
     - a static variable's initializer, and a global `const`;
     - an attribute's arguments, where they are read;
     - every operation that initializes the class's constants:
       - `new` of the class, of a subclass, of an implementer of an interface that holds the cause, or of a user of a trait that does;
       - `newInstanceWithoutConstructor`, and a static property read;
       - `getConstants`, and `getConstant` of any name;
       - a backed enum's case access, `from()` and `tryFrom()`, which evaluate every constant of the enum, and its `cases()`, which evaluates the case values.
     - The full list is read from php-src when CHECK-7 is built.
   - *Reporting.* Each cause is reported once, at the constant that has it.
   - *PHPDoc.* PHP never evaluates a PHPDoc edge (`@var`, a type alias, a `self::X_*` pattern). A constant whose PHPDoc reaches a cycle keeps the type it has without that PHPDoc, and the PHPDoc is reported as unresolvable. So REG-7.6 finds cycles over value edges only, with no `*` pattern expanded.
   - *Cycles* are found by the query engine (decision 3).
   - *Evidence:* PHP 8.5.4, one process per case (`~/.cache/prs-ledger/`: `cyc.php`, `cyc2.php`, `cyc3.php`, `thr.php`, `tryfrom.php`, `tryfrom2.php`).
     - These throw:
       - every operation listed above, including `getConstant('Z')` of a constant off the cycle;
       - `const U = NOPE`, with `Error: Undefined constant "NOPE"`;
       - `const D = 1 % 0`, with `DivisionByZeroError: Modulo by zero`;
       - `const C = Missing::X`, with `Error: Class "Missing" not found`;
       - `new A`, for its first failing constant.
     - These do not:
       - a direct read of a constant off the cycle, and `defined()` of one;
       - a static method call, `A::class`, `instanceof` and `class_exists`;
       - a call that passes the argument;
       - a pure enum's case access and `cases()`;
       - a backed enum's `cases()` when its case values are off the cycle;
       - `ReflectionEnum::getCases()`.
   - *Rows:*
     - the crate split's `mixed` delta is withdrawn, and REG-7.6 carries the rule;
     - TYPE-4.2 keeps only its memo;
     - VAL-2.5 stands, extended;
     - CHECK-7 is new.
2. **No case references: one record per file of the names it writes.**
   - *The question was* where case references live. REG-14.1 moved the `*case_refs` fields to the checker, computed from what is written at each position; REG-7.7 kept them as build scratch beside the merged declaration.
   - *The record (PROD-5.8).* Each file has one record of every name it writes that resolves without types: class-likes, functions and constants, in code and in PHPDoc. An entry holds:
     - its span, from which the source gives its spelling;
     - its resolved key, or its candidates where a fallback to the global name is possible;
     - where it appears.
   - *Who makes it.* The parser and the PHPDoc parser record it, extending today's `Recorder` (`syntax/src/references.rs:184`). PROD-5.7's dependency keys are derived from it.
   - *Who reads it.*
     - The case rule, the `@internal` rule (`checker/internal_usage.rs:337`) and every other rule that judges a written name.
     - A member name (`$x->foo()`) needs types to resolve, so its check stays in the checker.
   - *Doc blocks (DOC-2.9).* Parsed doc blocks are a per-file product, found by the comment's offset.
     - Each is parsed once, and the declaration builder and the checker share the parse.
     - It is dropped with the file's other products.
     - The ≈25 checker sites that parse a doc comment again read the analysed file's doc blocks.
   - *Rows:*
     - The `*case_refs` fields leave the declarations, with a format bump (REG-14.1, rewritten; it delivers VAL-2.6). These are the six item 8 names (a property's `case_refs` and `set_case_refs`, a class's `type_alias_case_refs` and `mixin_case_refs`, a parameter's `case_refs`, a function's `return_case_refs`) and an assertion's (`registry/function_sig/asserts.rs:60`).
     - REG-7.7 keeps neither the case references nor the doc blocks.
     - PROD-5.7 derives its keys from the record.
     - PROD-5.8 and DOC-2.9 are new.
3. **Item 4 is retired; REG-7 and REG-8 become one incremental query engine over the whole pipeline, with persistence.**
   - *The question was* what of the 20 G plan's item 4 is still wanted after REG-7.
   - *REG-13 is superseded.* What of it is still wanted moves:
     - `FileDecls` (REG-13.2) is the layout of REG-8.3's per-file declaration sets, and a stub file's record decodes into it.
     - The member list (REG-13.3: one entry per member, its traits' and their aliases included, in declaration order, with no copies) is the layout of REG-7.4's result. VAL-2.3 stands.
     - Building a declaration straight into its final form (REG-13.9: one member list, one folded index) is REG-4.15.
   - *Identity.*
     - A declaration's id is interned from (file, kind, folded name, occurrence in the file) and is never reused.
     - A member is (class id, kind, folded name).
     - A slot is a position within one revision, never an identity: a `(FileId, slot)` pair shifts when its file changes, and a stale one names another declaration.
   - *The engine (REG-16).*
     - Queries are keyed by interned ids.
     - The accessors record what each query reads, absent names included.
     - Every result has early cutoff: a query whose new value equals its old one stops the change there.
     - The builtin knowledge (decision 7) is a durable input.
     - Cycles are the engine's to find (REG-7.1's hierarchy cycles, REG-7.6's constant cycles).
     - Stale work is cancelled when an edit arrives.
   - *Its scope is the whole pipeline.*
     - The file texts are the inputs. The parse, a file's products, its declarations, resolution (REG-7's levels) and the check are queries.
     - The batch run and the server run the same queries.
     - It replaces:
       - the keeping and re-parsing of LOAD-1.6 and LOAD-5.3;
       - the dependents indexes (`dependents` and `signature_dependents`, `project.rs:250`, `:253`), and the walks over them (`walk_affected`, `closure_scope`);
       - the hand-made cancellation.
   - *Persistence (CACHE-13.2).*
     - Results are saved with the dependencies they recorded, as rustc's incremental compilation saves its query results and its dependency graph.
     - A warm run parses and checks again only the files whose recorded reads changed.
     - The resolved context base and a check-results cache are this persistence, not stores of their own. Today an analysed file is always parsed and checked (`project/file_cache.rs:1-8`).
   - *salsa* (0.28.5) is the default engine. rust-analyzer and Astral's `ty` are built on it, and it has a `persistence` feature. A measurement comes first (REG-16.1): coarse queries must keep the hot lookups plain table reads.
   - *`redeclared_methods` goes* (`registry/class_info.rs:382`, read at `checker/function_decl.rs:65` and `:245`). The checker checks each body against its own declaration's entry, REG-8.5's principle.
   - *Retired with item 4:*
     - the generational ids in three ranges;
     - the result tables, and the parallel passes (the global-variable pass moves with the crate split's C9);
     - the bitset scope;
     - change detection by id.
   - *Rows:*
     - REG-13 and its sub-tasks are SUPERSEDED. REG-4.5 loses its flag; REG-4.15 is new; REG-14.3 needs REG-8.3.
     - LOAD-1.6, LOAD-4.1, LOAD-4.2, LOAD-4.6 and LOAD-5.3 are SUPERSEDED.
     - REG-7's levels, REG-7.9's cycles, REG-8's snapshots and REG-8.2's read sets are the engine's.
     - TYPE-4.1's generation is the engine's revision.
     - CACHE-13.2 is the persistence.
     - REG-16 is new.
4. **A flat, index-based, immutable tree.**
   - *The question was* SYN-1.6's `unsafe impl Sync`. The 20 G plan's arena, a `bumpalo::Bump`, is not `Sync`, and step 6 made a finished arena readable from other threads with one `unsafe impl Sync`, which the workspace denies (`unsafe_code = "deny"`, `Cargo.toml:42`).
   - *The layout.*
     - A file's `Ast` owns its source, its names and one typed vector per node kind.
     - A child is a `u32` id; a list is a range.
     - A string is a span, plus a side buffer where it needs decoding. Large variants go in side tables.
   - *Sharing.* A tree is `Send + Sync` without `unsafe`, and an `Arc<Ast>` serves the batch run and the server alike.
   - *Immutable after the parse (SYN-1.10).* `resolve_names` and the stub-attribute rewrite go with decision 6, and `name_anonymous_classes` with decision 5.
   - *Every parse yields a tree (SYN-1.9).* A part that fails to parse is an error node with its span, which Q-10's recovery fills.
   - *Ids.* An id lives only in products owned with its tree; a debug build tags ids with their tree, as the backstop.
   - *SYN-2's token arrays* are designed together with the tree. Whether nodes keep spans or token indices is settled by a measurement.
   - *The cost:* the 2,612 `ExprKind::`/`StmtKind::` match sites in 135 files read through the tree, and each access has a bounds check.
   - *Prior art:* the Zig and Carbon compilers keep their trees this way; rust-analyzer's `la-arena` is the same kind of index arena.
   - *Rows:*
     - SYN-1.6 is SUPERSEDED;
     - SYN-1.2 to 1.8 are rewritten, SYN-1.1 is unchanged, and SYN-1.9 and 1.10 are new;
     - LOAD-3.5's and CHECK-6.3's wording follows.
5. **Declarations are the tree's own vectors.**
   - *The question was* who lists a file's declarations. PROD-5.1 had the parser list them; REG-11.1 (the crate split's `.10a`) indexed declarations below the top level through one Visitor walk, which also replaced `declared_names`, `collect_anonymous_classes` and the stub-index generator's walk.
   - *The vectors.*
     - The parser fills the tree's vectors of class-likes and functions in source order, nested ones included.
     - Beside them, as columns, are each one's guard (the condition node and its polarity, or an early-return guard) and the names in effect.
     - `const` statements and `define()` calls get index lists.
     - An anonymous class is a class node there too, named from its position.
   - *Guards.* `registry/guard.rs` still interprets them.
   - *Readers.* REG-11.1's behaviour, and the readers its walk would have served (`declared_names`, the anonymous-class walks, the stub-index generator in `xtask/src/stub_index.rs`), read these vectors. A walk that compares them with the tree lives only in the tests.
   - *Rows:* PROD-5.1 and REG-11.1 are rewritten, and no longer conflict.
6. **Gates as data, one parse per file.**
   - *The question was* STUB-7.1's version key. The crate split's text had the stub layer parse once per version, because member and parameter gates are inside the files; the off report recommended the gates as data, with one parse per file (this was DEC-1.1).
   - *What the parse records,* for each declaration and parameter:
     - its availability, as version ranges;
     - its type readings, by version range;
     - the extension it belongs to.
   - *Queries take a version range:*
     - availability is always, never or in part, and in part is reported unless a guard covers it;
     - a return type is the union of the readings in the range;
     - an argument is checked against each reading;
     - a `PHP_VERSION_ID` branch narrows the range. Today `checker/predefined_constants.rs` reads `PHP_VERSION_ID` as a range, and `registry/guard.rs` reads its guards.
   - *The range* comes from the configuration, else Composer's `config.platform.php` or `require.php` (`crates/composer/src/lib.rs:114`), else every version known: 7.0 to the newest, narrowed to the versions the project's own files compile on (decision 18). No cache key holds a version.
   - *Sources (STUB-12):*
     - for PHP 8.0 and later, php-src's own stub files, as PHPStan's `phpstan/php-8-stubs` packages them (`Php8SignatureMapProvider` reads them by version id);
     - for earlier versions, reflection dumps of real PHP binaries, made by a generator;
     - for the rest (the PECL extensions), phpstorm-stubs;
     - a check that reports where the sources disagree.
   - *What goes:*
     - The functionMap deltas become readings of its claims by version range (decision 10); today they are read at the newest revision (`registry/stubs.rs:253`).
     - STUB-7.9, STUB-7.10 and STUB-8 become range queries.
     - The stub-attribute rewrite (`stub_attributes.rs`, which applies the newest version and then removes the attributes) goes, and with it `resolve_names` (`syntax/src/name_resolution.rs:515`), which only it calls.
   - *Rows:*
     - STUB-7.1 is decided, and STUB-7.3 is re-specified;
     - STUB-7.6, STUB-7.9, STUB-7.10 and STUB-8 read the ranges;
     - STUB-7.12's check compares the records, ranges included, where the plan's text checked them "for the unset version key";
     - PROD-5.4 waits on STUB-7.3;
     - STUB-12 is new.
7. **Embed the builtin knowledge; read PHPStan's classes from the project.**
   - *The question was* STUB-10.2: whether to embed the stub data at compile time. It waited for the user from before the merge.
   - *Embedded:*
     - per-file records of PHP's declarations, PHPStan's function map and PHPStan's `.stub` files;
     - built at compile time by the code the runtime runs;
     - a read-only layer: no run builds them, and the stub disk cache goes.
   - *`**` is folded deterministically,* by a pure-Rust `pow` (`types/src/arithmetic.rs:715-717` calls `powf` today), so that the host's build and the target's run agree. A test asserts that the embedded records equal a runtime build.
   - *The data crate* is linked only into the final binaries.
   - *Measured, not assumed (STUB-10.1):* the records' size, whether the stub sources can leave the binary, and the build time. The off report's §10.6 has the figures for today's form:
     - four files of 31.35 MB;
     - `.rodata` growing from 18.2 to ≈50 MB;
     - ≈2.7-3.6 min for the release legs to compile reflection's dependencies a second time;
     - up to ≈20 s of lost pipelining, unless only the final binaries take the data crate.
   - *PHPStan's own classes are not embedded (STUB-13).*
     - They are read from the PHPStan the project installs, its phar or its source, as a context package cached per file.
     - An embedded copy is right only for the pinned 2.2.16. Today they are 15.33 of the 31.35 MB (`stub-base-phpstan`).
     - The installed `phpstan.phar` holds 7,127 entries, 1,959 of them under `src/`, none compressed, with their namespaces intact. `crates/stub-sources/build.rs` says the phar ships no loose source, which is wrong.
   - *Rows:*
     - STUB-10.2 is decided (yes), and STUB-10.1 is the measurements;
     - STUB-11 is re-specified;
     - STUB-13 is new, its phar locator in STUB-7.4's chain;
     - Q-8 is superseded: B9's timings and the build script's cost are STUB-10.1's, and B10's backstop is STUB-11's equality test;
     - the off drill's D3 is decided again (§1).
8. **A PHPDoc type refines the kinds its native type admits; it never removes one.**
   - *The question was* REG-7.5. The crate split's text takes PHPStan's `TypehintHelper::decideType` (2.2.16; the latest, 2.3.1, adds one rule) whole, and nothing had checked that against PHP.
   - *What PHP does* (PHP 8.5.4; the probes are in `~/.cache/prs-ledger/reg75/probes/`):
     - a native type is enforced at each call, return and typed-property write, after coercion in coercive mode; `null` to a non-nullable `int` is a `TypeError` in that mode too;
     - a PHPDoc type is never enforced;
     - `iterable` admits what `Traversable|array` admits (a `Generator`, an `ArrayIterator`, an array), though reflection names it `iterable`;
     - a resource passed to `object` is a `TypeError`;
     - `Foo $x = null` declares `?Foo` (deprecated since 8.4);
     - a `void` function gives its caller `null`, and a `never` function that returns is a `TypeError`;
     - a variadic collects named arguments under string keys: `f(1, 2, a: 3)` gives `[0 => 1, 1 => 2, 'a' => 3]`;
     - an array stores a decimal-int string key as an int; a generator's keys are not converted.
   - *So:*
     - A value can have any kind its native type names, whatever its PHPDoc says.
     - The PHPDoc is checked only where both ends are analysed: an analysed caller for a parameter, an analysed write for a property, an analysed body for a return. Callers through `call_user_func`, a framework or a library's users are not checked, nor are hydrating writes, nor the bodies of vendor and stub code.
     - The PHPDoc in use was written under PHPStan's rule, which ignores a kind the PHPDoc leaves out (`@return Foo` on `?Foo` is `Foo|null` there), so no author was told of one.
     - So one rule serves parameters, returns, properties and typed constants alike. A combination that depended on whether a body is analysed would type a declaration differently once its file is opened.
   - *The rule:*
     - The native type's kinds are the members of its union form: `?T` is `T|null`, `iterable` is `array|Traversable`, and an intersection (`A&B`) is one kind. `mixed`, written or not, names no kind: the PHPDoc type is then the declared type, as today.
     - A PHPDoc member that shares no value with any kind is dropped, since no run can produce it; `removeNull` is one case. At the final level the relation reads the class hierarchy, so a named class, `object`, a template and a callable are judged like any other member. Today they are kept unjudged, because the combination runs while the registry is built (`types/src/decide.rs:211`).
     - A PHPDoc member that shares values with a kind is intersected with it:
       - `callable(int): string` on `Closure` is `Closure(int): string`; today, and in PHPStan, it loses `Closure`;
       - `T` on `Foo` is `T&Foo`, so `T` is bound from the argument and the body sees `Foo`'s members. Today it is `T` alone; PHPStan drops `T` unless its bound is within `Foo`;
       - an inherited `@return Baz` on `: Bar`, `Baz` a subclass, is `Baz` (the plan's change).
     - A kind that no PHPDoc member shares values with is kept as the native type has it:
       - `@param string[]` on `string|array|A` is `array<string>|string|A`; today `A` is lost, since no class is added back (`native_member_addable`, `types/src/decide.rs:120`);
       - `@return Foo` on `?Foo` is `Foo|null`, as today.
     - An array PHPDoc member other than a shape (`T[]`, `array<K, V>`, `list<V>`, `non-empty-array<V>`) also gives a native `Traversable` kind its key and value types:
       - on `iterable` it describes the iteration, which an array argument and a `Traversable` argument share, and it is checked no less for one than for the other;
       - `@param array<int> $x` on `iterable` is `array<int>|Traversable<mixed, int>`, where today it is `array<int>`;
       - a shape describes an array only;
       - PHPStan widens only `T[]` and `array<V>`, and for any other array member it keeps no `Traversable` kind at all.
     - When no PHPDoc member is kept, the native type applies, and `parameter.phpDocType`, `return.phpDocType` or `property.phpDocType` reports the PHPDoc.
       - This is decided member by member: `@return non-empty-string|false` on `string` is `non-empty-string`, as today.
       - `decideType` instead drops the whole PHPDoc type when any member is outside the native type. REG-7.5's earlier text asked for both.
     - A written `never` is kept over any native type: it is within every kind, and the body is checked against it (`return.never`, `checker/function_decl.rs:1029`). A PHPDoc type that has no value without writing `never` (`int&string`) is contradictory: the native type applies, and the tag is reported as unresolvable (`parameter.unresolvableType`, `return.unresolvableType`; PHPStan's `UnresolvableTypeHelper` counts such a `never` the same way).
     - A conditional type is judged branch by branch. A kind is kept only when no branch shares values with it, so `($c is true ? void : never)` on `: void` keeps both branches (the plan's `bug-10337.php`). Today the add-back is skipped for any conditional, and under `void` only `void` and `never` survive the filter.
     - `void` admits only `void` and `never`, so `@return null` on `: void` stays `void` (the fixture REG-7's commit adds).
     - A member that resolves only later carries the native type and is combined by this rule where it resolves: `static::FOO_*` for the called class, and `value-of`, `key-of` and `T[K]` on a template for its binding. This is 2.3.1's `withNativeType`; today `resolve_deferred_value_of_with_native_fallback` does it for `value-of` alone.
     - The PHPDoc is read with its aliases resolved (the plan's change). Today `substitute_type_aliases` runs after the combination (`registry/function_sig.rs:734`).
     - A variadic parameter with no native type and an array PHPDoc type takes the array's value type for each argument (the plan's change).
       - The runtime cannot tell which reading an author meant.
       - The PHPDoc in use follows this one: PHPStan applies it, and our PHPDoc check applies it when the native type is not an array (`checker/phpdoc_type_compat.rs:99`).
       - In the body the parameter is `array<int|string, V>`, as it already is (`checker/function_decl.rs:501`).
     - PHPStan returns a benevolent native type whole. That rule has no case here, since no written native type is a benevolent union. For a builtin, decision 10 gives the bound and the claims that this rule combines.
   - *What goes:*
     - `reconcile_doc_with_native`'s exclusion of `object` (`registry/function_sig/type_resolution.rs:153`). Both cases in its comment have `mixed` natives (the fixture `a_mixed_native_hint_does_not_override_a_docblocks_resource_member.php`), and a resource under `object` is a `TypeError`.
     - Its separate `?T` null union (`:113`); the kept-kind rule covers it.
     - `native_member_addable`, and the add-back's skips for templates, conditionals and deferred members (`types/src/decide.rs:66`). The template, branch and deferred rules replace them.
     - `member_possible_given_native`'s list of members kept unjudged.
   - *A new check, opt-in (CHECK-8):* a PHPDoc type that leaves out a kind its native type names (`@param int` on `int|string`, `@param Foo` on `Foo $x = null`) is reported at the tag. It is correct, since the PHPDoc misstates what the declaration admits. It is off by default because every earlier tool accepted such PHPDoc.
   - *Rows:*
     - REG-7.5 and REG-7.12 are re-specified;
     - VAL-2.9 and VAL-2.10 list the output changes;
     - CHECK-8 is new;
     - TYPE-5 is new: `iterable<K, V>`'s array member ignores `K`, found while this was judged.
9. **What a body shows is inferred on demand; nothing in a vendor body is reported.**
   - *The question was* whether checking vendor files would improve the tool.
   - *Today:*
     - A context file's declarations are indexed, and its bodies are kept as text, parsed when a pass reads them (`project/session.rs:666`).
     - Purity and property writes are derived from bodies on demand (`registry/demand.rs`), from their syntax alone.
     - No body outside the analysed files is type-checked or reported.
   - *What is wanted:* the facts a body shows, where its declaration says nothing, or says something the body refutes (REG-17):
     - a return type and the throws, where none are declared;
     - a context declaration's refinement that its own code refutes, widened: a return's by its body, a property's by its default and the writes in its class's bodies;
     - receiver types in the body summaries, for purity and property writes.
   - *Bounds:*
     - A body speaks for a call only when PHP binds the call to that body. That is a function; a private or final method, or a method of a final class (`registry/purity.rs:664`); a `self::`, `parent::` or `Name::` call, which PHP binds lexically (probed: from a `B` that overrides `m`, `self::m()` and `A::m()` in `A` run `A`'s `m`, and `static::m()` runs `B`'s). Any other call of an overridable method keeps what is declared.
     - A body's fact never overrides a declared type. The installed body is one version of the code, and its declaration is the contract the next version is held to: decision 8's kept kinds stay, and a fact fills only what is undeclared or refuted.
     - Only what a query reaches is inferred, as `registry/demand.rs` does today.
     - Dynamic code (`eval`, `extract`, a variable call, `__call`) leaves a fact unknown, which is what a declaration that says nothing gives today: `mixed`, or throws anything.
     - The configured version range (decision 6) bounds the branches a body is read with.
     - Nothing in a context file is reported, since its author is not the user.
     - A property's type is never inferred from its writes. Hydrators write a property outside its class's code, and only a native type holds them (PHP 8.5.4, `~/.cache/prs-ledger/reg173/probes/`):
       - `ReflectionProperty::setValue`, `unserialize` and `Closure::bind` wrote a string, an array and `null` into an untyped property;
       - reflection and `unserialize` raised a `TypeError` for an `int` property;
       - no write checks a PHPDoc: a closure wrote -5, and `unserialize` -7, into an `int` property whose `@var` is `positive-int`.
   - *Rows:*
     - REG-17 is new. It needs REG-16.4: each inference is a query, and inferring one body asks for others;
     - REG-7.9 expects one more cycle, a recursive body's;
     - CHECK-9 is new: the never-thrown half of the dead catch check, which can be built now and decides more catches with REG-17.2;
     - VAL-2.10 lists the output changes.
10. **A builtin's types: what php-src verifies bounds them, and the other sources' claims are intersected within that bound.**
   - *The question was* how decision 6's sources combine for one declaration. Decision 6 made the function map's deltas readings, and decision 8 combined only the `.stub` PHPDoc with a builtin's native type. Neither said whether the function map is a native reading or a PHPDoc, nor which source applies where two disagree; STUB-12.5 only reported it.
   - *What php-src verifies* (php-src 8.5.4; the sources read are in `~/.cache/prs-ledger/php-src-8.5.4/`):
     - A builtin's argument parsing enforces its parameters on every call, and a debug build asserts that the native parameter types agree with it (`zend_internal_call_should_throw`, `Zend/zend_execute.c:1298`).
     - A debug build asserts each builtin's return value against its native return type, on every call (`Zend/zend_vm_def.h:4134`). php-src's CI runs its tests on debug builds.
     - A function's PHPDoc return type in php-src's stubs becomes its func info: its kinds, and an array's key and value kinds. Methods and aliases get none (`build/gen_stub.php:1579`). With `ZEND_VERIFY_FUNC_INFO` set, a debug build asserts each return against it, reading at most 16 elements of an array (`Zend/zend_execute.c:1337`). The flag is off by default (`:1333`); php-src's nightly `_VARIATION` job sets it, on a debug build (`.github/matrix.php:105`, `.github/workflows/test-suite.yml:166`).
     - Nothing checks the rest against a run: php-src's other PHPDoc, PHPStan's function map and `.stub` files, and phpstorm-stubs' PHPDoc.
   - *The rule:*
     - A type position's bound is its native reading for the version range (decision 6's sources). For a php-src function's return, it is that reading narrowed by the function's PHPDoc return type, at func info's precision. Below 8.0, a call whose argument may fail the builtin's parameter parsing also gives the function's failure value (decision 16).
     - Every other source's type for the position, an assertion's and a `@param-out`'s included, is a claim that the value lies within it. The claims are intersected, since a value that each claim describes lies in all of them.
     - Decision 8's rule combines the intersection with the bound: no claim removes a kind the bound names, a member the bound excludes is dropped, and a kind within which the claims share no value is kept as the bound has it.
     - The function map is a claim, not a native reading. Its deltas give its claims by version range.
     - `@throws` and purity name what a call can do, so the sources' lists are united: an exception or a side effect that any source names is kept.
     - Templates and generic ancestors (`@template`, `@extends`, `@implements`) model the declaration for the analyser and say nothing of a run. They are read from PHPStan's `.stub` file where it writes them, since it is written for this type system, and otherwise from the source that does.
     - php-src's stubs write none of these tags: gen_stub reads `@param`, `@return`, `@refcount`, `@prefer-ref`, the aliases, `@frameless-function` and its build flags (`build/gen_stub.php:4690-4733`). PHPStan's `.stub` files write them for five PECL extensions that phpstorm-stubs also covers (`Redis.stub`, `Memcached.stub`, `ImagickPixel.stub`, `ext-ds.stub`, `ibm_db2.stub`: 38 `@template`, 22 `@throws`), and the vendored phpstorm-stubs writes 255 `@throws` for redis, 403 for imagick, and 42 `@template` and 29 `@throws` for ds.
   - *Reported when the data is built (STUB-12.5):* native readings that differ between sources for one version; a claim's member that the bound excludes; a claim that leaves out a kind the bound names; claims that share no value within a kind; templates or generic ancestors that two sources write differently. The rule gives each case its result; the report is for correcting the sources.
   - *PHPStan* nests the layers instead, each passed through `decideType` with the result below it as the native type: php-src's PHPDoc over the native type, the function map over that (`Php8SignatureMapProvider::mergeSignatures`, `:327`), then the `.stub` PHPDoc over the result (`NativeFunctionReflectionProvider.php:163`, `:179`). This is 2.3.1, unchanged from 2.2.16 (`~/.cache/prs-ledger/phpstan-src-2.3.1/`). A layer that leaves the one below it is dropped whole, so each unverified layer bounds the next: a `.stub` type wider than the function map's is lost, and php-src's unverified PHPDoc bounds the function map.
   - *Rows:*
     - decision 6's function map readings are claims, and decision 8's builtin case follows this rule;
     - STUB-12, STUB-12.1, STUB-12.4 and STUB-12.5 are re-specified;
     - VAL-2.10 lists the output changes.

11. **A name binds to the declarations PHP may bind it to; no declaration merges into another, so each keeps its own location.**
   - *The question was* REG-4.11's rule for the location of a merged declaration, which provenance needs before it can be the only location record. The registry merges two declarations of one name in five places, and the merged entry keeps one side's provenance:
     - `merge_stub_classes` (`registry/index.rs:589`) merges a built-in class with a declaration of its name indexed before it: the built-in's provenance is kept, and the other's members win;
     - `insert_built_decl` (`:144`) merges a guarded class onto a built-in (`:244`), keeping the guarded one's provenance while the built-in's members win. It merges a second unguarded class onto the first, a built-in (`:329`) or a context declaration (`:344`), keeping the first's provenance while the second's members win; an analysed file's declaration replaces a context one instead;
     - it merges a second unguarded function onto the first (`:436`): `FunctionReflection::merge` keeps the first's provenance (`registry/function_sig.rs:951`), `is_stub` is either side's (`:1019`), and `function_decl_sites` records no declaration (`registry/index.rs:434`).
   - *What PHP does* (PHP 8.5.4; the probes are in `~/.cache/prs-ledger/reg411/probes/`):
     - a name is bound to one declaration at a time, and members never merge: when an autoloader loads one of two files declaring `Dup`, `Dup` has that file's methods and none of the other's, and a class declared in both branches of an `if` has one branch's methods;
     - redeclaring a built-in is fatal ("Cannot redeclare class ArrayIterator", "Cannot redeclare function strlen()"), and so is a second unguarded declaration when its file loads ("Cannot redeclare class Dup (previously declared in a.php:2)");
     - a guarded polyfill is skipped where its built-in exists: `str_contains` and `Stringable` are the built-ins in 8.5;
     - Composer's loader maps a class to one file: its classmap, else the longest PSR-4 prefix and the first of its directories that holds the file, else PSR-0 (`ClassLoader::findFile`, `vendor/composer/src/Composer/Autoload/ClassLoader.php:442`, `:492`). A class that two classmap files declare takes the first, with a warning (`AutoloadGenerator.php:374-380`).
   - *The rule:*
     - Every declaration is its own entry in its file's declaration set (REG-8.3), with its own provenance. No entry merges two declarations, so a location is always one declaration's, and `is_stub` becomes a question about a declaration's provenance or a file's role.
     - A name binds to the declarations PHP may bind it to:
       - a built-in where it is available in the version range (decision 6), and an unguarded redeclaration of it only where it is not;
       - a guarded declaration where its guard holds (STUB-8, which decides a guard on run-time evidence only);
       - an autoloaded class in the file Composer's loader maps it to (STUB-7.5);
       - a declaration that no loader maps, where its file is loaded: in that file, in a file that includes it, and wherever no other declaration of the name can bind.
     - When one declaration binds, it is the name's entry. When the analysis can't tell which of several binds, each is a candidate, and a query combines them as decision 6 combines a built-in's readings in a range: an argument is checked against each candidate, a return or a property's type is their union, and a member that only some candidates have is available in part, reported unless a guard covers it. The relation reads such a class as each candidate: an ancestor that every candidate has is certain, and one that only some have is possible.
     - Each candidate keeps its location: a definition lists every candidate, and a report cites the declaration it is about.
     - One declaration stays one. A built-in's readings from its sources (decision 10) are located at its native reading's source, with the extension it belongs to. The forms that one source writes for one built-in are its readings: phpstorm-stubs declares `strtr` twice (`standard/standard_1.php:690`, `:700`), and a pair under version attributes is readings by range (decision 6). A stub file the project names (`stub_paths`) is an overlay of claims on the declaration it refines, as decision 10's claims are, and never relocates it; it declares only a name that nothing else declares.
     - PHP refuses a declaration that loads beside another: a redeclared built-in where the built-in is available, or a second unguarded declaration. The analysed one is reported, as `function.duplicate` and the opt-in `class.duplicate` family do today; a context file's is not (decision 9).
   - *PHPStan* (2.3.1) binds a name to the first source locator that finds it (`AggregateSourceLocator::locateIdentifier`). Its chain puts the analysed files before PHP's internals (`BetterReflectionSourceLocatorFactory::create`, `:146-214`), so an analysed file's `class ArrayIterator` is found before the built-in. Its stub files supply PHPDoc only (`StubPhpDocProvider`). The phar read is in `~/.cache/prs-ledger/phpstan-2.3.1-phar/`. Merging members is ours alone, and so is the question of a merged declaration's location.
   - *What goes:*
     - the five merges, and the collision state they keep: `built_in_stub_classes` and `stub_provided_functions`, which STUB-7.7's provenance queries replace, and `unconditionally_declared_classes` and `unconditionally_declared_functions`;
     - `function_decl_sites`, for REG-8.5's checks of each body against its own declaration;
     - `Origin::Unknown`, with `Registry::build` (REG-10).
   - *Rows:*
     - REG-4.11 is unblocked and re-specified; it needs REG-8.1, REG-8.5 and REG-10;
     - REG-8.1's collision policy is this binding, so no order picks a winner;
     - REG-8.5, REG-9 and STUB-7.6 lose their merges, and REG-11.14 (one operation per merge case) is superseded;
     - STUB-7.4's chain finds a name's declarations, and no locator's place in it makes one win; STUB-7.5 maps a class as Composer's loader does; STUB-8's undecided guards keep both declarations as candidates;
     - VAL-2.10 lists the output changes.

12. **What F2's count explained is the engine's to report: STUB-9 gives way to a query trace.**
   - *The question was* STUB-9: F2's count of the stub keys in the finishing passes' scope, by the changed name that pulled each in. The 20 G plan has it "explain whatever remains" and gate nothing (`PROFILE_DRILL_PIWIGO_20G.md:1104`); the 25 G report has it confirm what remains (`PROFILE_DRILL_PIWIGO_25G.md:1520`). REG-7 replaces the scope it counts, so its form, if any, waited on REG-7 and the engine.
   - *What is left to explain.* On the engine a stub class is resolved only by a query that reads it, and REG-12's test pins that checking one file resolves no class the file doesn't reach. That test says that something was resolved, not why. And the question is not the stubs' alone: any query can be reached by more than it needs, a project class's as much as a stub's.
   - *The rule:* when a runtime switch asks for it, in any build (as TYPE-4.4's switch works), the engine reports every query a run executes, with the query that first asked for it, and counts the stub layer's queries apart. It gates nothing; REG-12's test is the gate.
   - *Rows:* STUB-9 is superseded by REG-16.5, which is new and needs REG-16.3.

13. **A file's recorded hash stands in for its bytes only where a change time sees every write.**
   - *The question was* CACHE-13.1's soundness concern. A per-project index of `(size, mtime_ns, inode, device, content hash)` takes a file's hash without reading it when those fields match, and a same-size rewrite that keeps its mtime matches too (`rsync -t`, `tar`).
   - *What a rewrite leaves* (Linux 7.0, ext4; `~/.cache/prs-ledger/cache131/`). Each case put back a file of the same size with the old mtime, to the nanosecond:
     - an in-place write followed by `touch -d`, `cp -p`, and `rsync -c --inplace` left size, mtime and inode unchanged;
     - `rsync -c` wrote a new file, which took an inode number an earlier file had used, and `mv` gave another inode;
     - the change time moved every time. The kernel sets it to now on every write and every change of the times, and no call sets it to a chosen value;
     - plain `rsync -t` copied nothing: its own quick check compares size and mtime, so it has the same blind spot, and the file kept its old bytes.
   - *Where the change time can't be trusted:*
     - FAT and exFAT store none, and Linux fills it from the modification time when it reads an inode (`fat_fill_inode`, `fs/fat/inode.c:568`; `exfat_fill_inode`, `fs/exfat/inode.c:652`; Linux 7.0, `~/.cache/prs-ledger/linux-7.0/`), so a restored mtime restores it once the inode is read again.
     - On Windows a program can set it, or ask that a write leave it (`FILE_BASIC_INFORMATION`, Microsoft's driver documentation), and `std` exposes it only behind an unstable feature (`MetadataExt::change_time`, `windows_change_time`; Rust 1.98's `library/std/src/os/windows/fs.rs:613`).
   - *Git* compares mtime, ctime, inode, owner and size, and the device only when built with `USE_STDEV`, since it is unstable on network file systems (`match_stat_data`, `statinfo.c`; `Documentation/technical/racy-git.txt`; git 2.47.0, `~/.cache/prs-ledger/git-2.47.0/`). An entry whose mtime is not older than the index file's is checked by its content ("racily clean"), and its recorded size is zeroed when the index is written again.
   - *The rule:*
     - The index records `(size, mtime_ns, ctime_ns, inode, device, content hash)`, from a stat taken before the bytes are read, so a write during the read shows next time.
     - A file takes its recorded hash without being read only when all five stat fields match and both recorded times are older than the index's own write time: git's rule for racily clean entries, applied to the change time too.
     - That holds only on a file system known to keep a change time that no program sets (ext4, xfs, btrfs, APFS and the like). On any other, and on Windows, every file is read and hashed.
     - It covers analysed files as well as context files. Since decision 3 an unchanged analysed file is neither parsed nor checked (CACHE-13.2), so it need not be read either.
   - *Rows:* CACHE-13.1 is re-specified, and its soundness concern is settled.

14. **The crate split's output changes that the ledger had left unapproved are approved on what PHP does, and two are corrected.**
   - *The question was* the rows VAL-2.9 lists with a delta and no approval: REG-4.14, REG-5's rest, REG-6, REG-8.1, REG-8.5, STUB-7.1, 7.2, 7.3, 7.5, 7.9 and 7.10. VAL-2.1 to 2.8 were decided one by one; these were left to be read as they land.
   - *Approved in the plan already* (`CRATE_SPLIT_PLAN.md`), which the rows had dropped: REG-4.14's Doctrine pick and REG-5's assert lookup by folded name ("an approved delta, fixtured"), and STUB-7.5's lazy composer locator, STUB-7.9's version-based skip-polyfill and STUB-7.10's version-dependent signature selection ("approved delta"). Each matches PHP: a method name is case-insensitive (`(new A)->ASSERTFOO()` runs `assertFoo`), an autoloader runs when a class is first used (decision 11's probes), and decision 6 has the readings by range.
   - *Approved now:*
     - REG-4.14's ordering: no result depends on the class map's order, so the server and the batch run agree (decision 11). Its Doctrine pick by name order holds until REG-11.16 resolves a table that two entities map to no entity.
     - REG-6's test expectations: the harness resolves against the stubs as a project does, since PHP's built-in classes exist in every run.
     - STUB-7.1's and STUB-7.3's, which decision 6 designed. A function removed before the analysed range is unknown: `mysql_connect()` is an undefined function in PHP 8.5.4. The attributes read as data give a property's and a class constant's types, every entry `newest_type` reads, and a deprecation in its own overload only.
     - STUB-7.2's `PS_UNRESERVE_PREFIX_` names, which are the reserved words they spell: `Generator::throw()` exists.
     - REG-8.1's, which decision 11 states.
   - *Corrected:*
     - **STUB-7.2.** `exit` and `die` are functions from PHP 8.4: php-src 8.4.0 declares `function exit(string|int $status = 0): never` (`Zend/zend_builtin_functions.stub.php:10`, `die` its alias at `:13`), where 8.3.0 has neither, and in 8.5.4 `function_exists('exit')` is true and `exit(...)` is a closure. So they stay in the index, available from 8.4 (decision 6). Only `array`, `list`, `empty`, `isset`, `unset`, `eval`, `__halt_compiler` and the `___PHPSTORM_HELPERS` classes leave it; none of them is a function in 8.5.4.
     - **REG-8.5.** PHP types a set hook's parameter this way (PHP 8.5.4, `~/.cache/prs-ledger/reg85/probes/`): without a type it has the property's native type, a wider type receives values the property doesn't hold (`set(int|string $value)` on an `int` property gets `'42'`), and a narrower one is a compile error. So a parameter whose native type is the property's, written or omitted, takes the property's PHPDoc refinement by decision 8's rule (`set(array $val)` on an `@var array<string>` property sees `array<string>`, nsrt `property-hooks.php`), and analysed writes are checked against it. A parameter with a wider native type keeps its own type, since its hook takes what the property can't hold and converts it. The plan's "where it narrows the native one" would narrow a wider parameter too.
   - *Still read at VAL-1:* CHECK-6.1's families (VAL-2.8), whose diffs exist only as each family lands.
   - *Rows:* the eleven rows say their deltas are approved; STUB-7.2 and REG-8.5 are corrected; VAL-2.9 records the approvals.

15. **A file's syntax bounds the versions it runs on: what it declares exists only where its grammar holds.**
   - *The question was* Q-12. D6 has a file's validity follow the configured version, and decision 6 made that version a range. Nothing said what a file whose syntax holds in only part of the range declares, or what is reported.
   - *What PHP does* (PHP 8.5.4, and 7.4 in its official image; `~/.cache/prs-ledger/q12/probes/`):
     - a file is compiled whole when it loads, before any of it runs: `(real)` inside `if (PHP_VERSION_ID < 80000)` stops PHP 8.5 from compiling the file, so a version check inside a file guards none of its syntax;
     - an include under a version check that is false never compiles the file, and on 7.4 the same include loads it and declares its function;
     - an include on a version that refuses the file throws `ParseError`, and nothing the file declares exists afterwards;
     - so a polyfill puts a version's syntax in a file of its own and guards the include: `symfony/polyfill-mbstring`'s `bootstrap.php:12` requires `bootstrap80.php` only on PHP 8.
   - *The rule:*
     - A file's validity range is the versions whose grammar accepts every construct it uses, read from the parser's marks (Q-10). Syntax a version added bounds it from below, and syntax a version removed bounds it from above.
     - Everything the file declares is available only within that range, intersected with its own availability (decision 6), and a name binds to it only there (decision 11).
     - Loading the file outside its range fails. So an include of it, and a use of a class it declares that the autoloader would load from it, are uses whose availability is the file's range, and decision 6's rule reports them unless a guard covers them. `bootstrap.php`'s version check covers its include of `bootstrap80.php`.
     - An analysed file is reported at each construct that a version of the range refuses, since PHP refuses the whole file there. It isn't when every load of it is an include that a guard covers and no autoloader maps it. A context file is not reported (decision 9).
   - *Rows:* Q-12 is decided; Q-10 builds the marks and this rule; VAL-2.10 lists the output changes.

16. **Before PHP 8.0, a builtin whose parameter parsing refuses an argument returns its own failure value, which the generator reads from a real binary.**
   - *The question was* Q-13. Decision 10's bound and claims describe a call whose arguments parse. Before 8.0, outside `strict_types`, a call whose argument fails a builtin's parameter parsing warns and goes on.
   - *What PHP does.* In php-src 7.4.33 the result starts as `null` (`ZEND_DO_ICALL`, `Zend/zend_vm_def.h:3993`); `ZEND_PARSE_PARAMETERS_END` reports through `zend_internal_type_error`, which throws only under `strict_types` (`Zend/zend.c:1577`), and returns. A survey on the official PHP 7.3.33 and 7.4.33 images (`~/.cache/prs-ledger/q13/probes/`) called every internal function whose required parameters are by value, with an object for each, and recorded the result where PHP reported a parsing failure. Of 7.4's 793:
     - 503 returned `null`;
     - 186 returned `false`: `strtotime`, `date`, `preg_match`, the `gz*` family and others; 7.3 gave the same 186;
     - 4 went on with a default: `chr` returned a string;
     - 100 threw anyway: `TypeError` 66, `Error` 31, and others.

     PHP 8.0.30 threw for all of its 819.
   - *The rule:*
     - Below 8.0, a call from a file without `strict_types=1` whose argument may fail a builtin's parameter parsing gives the function's failure value as well as its return type: the warning doesn't stop the program, and the call returns that value. Where the argument fails for every value it can have, the call gives the failure value alone.
     - In a `strict_types=1` file, and from 8.0, such a call throws instead (`TypeError`, `ArgumentCountError`), as the functions that throw on 7.x do in every file.
     - The argument is reported as in every version (`argument.type`). The failure value is what the code after the call sees in the versions where it goes on.
     - The failure value is the builtin's own, by version. STUB-12.2's generator calls each function on the real binary with an argument each parameter's parsing refuses, as the survey did, and records what it returns or throws. A function that no argument makes fail has none.
     - For such a call, decision 10's bound is the reading's return type joined with the failure value. The claims describe a call that succeeds, so they don't remove it.
   - *Rows:* Q-13 is decided; STUB-12.2's generator records the failure values; decision 10's bound takes them; VAL-2.10 lists the output change.

17. **A return inferred where none is declared keeps the values the body returns.**
   - *The question was* Q-14: whether REG-17.1's inferred return keeps its literals (`return '1.2'` as `'1.2'` or `string`). Decision 9 lets a body fill only what is undeclared, since the installed body is one version of the code, and a literal is what the next version is likeliest to change.
   - *What the analysis already does* (HEAD's gate build): the analysis is of the installed code throughout. A constant's type is its value: `Lib::VERSION === '1.2'` is reported always true. A closure's inferred return keeps its literals: `$f() === 'x'` is reported always true (`closure_inferred_return_type`, `checker/expr.rs:6625`). A method exists or not, and a body is pure or not, as the installed version has it.
   - *The rule:*
     - An inferred return is the union of what the body's returns give, literals included: `return '1.2'` is `'1.2'`, and `return [1]` is `array{1}`. That is what the installed body returns, and a declared constant is read the same way. Such a union is capped as every union is (`MAX_UNION_MEMBERS`, `types/src/union.rs:12`).
     - A recursive body is inferred to a fixed point (REG-7.9's cycle). An iteration that changes a literal widens it to its kind (`'a'` to `string`, `1` to `int`), so the inference ends.
     - A project that must hold to more than its installed dependencies declares what it relies on, in a stub file it names (decision 11's overlay), and a declared return is never inferred.
   - *Rows:* Q-14 is decided; REG-17.1 follows; VAL-2.10 lists the output change.

18. **The builtin knowledge goes back to PHP 7.0; a project that names no range is read over the versions its own files compile on; and the engine is read version by version.**
   - *The question was* Q-15. STUB-12.2 named no oldest version, and decision 6's default range, every version known, starts there. So it set how much decision 6's in-part availability reports in a project that names no version.
   - *Evidence:*
     - PHP 7 made errors exceptions (`Error`, `TypeError`, `Throwable`), which the checker and decisions 8, 10 and 16 are built on. On the official PHP 5.6.40 image neither `Throwable` nor `TypeError` exists, and a call with an argument of the wrong type is a "Catchable fatal error" that ends the program; 7.4 throws a `TypeError` there (`~/.cache/prs-ledger/q15/probes/`).
     - Official images, real binaries, exist for every version from 7.0.
     - Of the 371 packages installed across the vendor trees, 31 (8%) allow PHP below 7.0, 30 from 7.0 or 7.1, 61 from 7.2 or 7.3, 42 from 7.4, 24 from 8.0 and 183 (49%) from 8.1. These are context, read over the project's range.
     - PHPStan 2.3.1 analyses from 7.1, and raises a lower `phpVersion` to it (`PhpVersionFactory::MIN_PHP_VERSION`).
     - The engine changed at 8.0 (PHP 7.4 against 8.5.4). `0 == 'a'` is true, then false. `1 / 0` is `INF` with a warning, then a `DivisionByZeroError`. `"abc" * 1` is `0` with a warning, then a `TypeError`.
     - The checker reads one version today: at `--php-version 7.4`, `0 == 'a'` is reported always true, and at 8.5 always false.
   - *The rule:*
     - The builtin knowledge covers PHP 7.0 to the newest version: php-src's stubs from 8.0 (STUB-12.1), and STUB-12.2's dumps of real binaries for 7.0 to 7.4.
     - A range that reaches below 7.0 is read from 7.0, and a notice says so once: below 7.0 a failed call is a fatal error, not a throw, and nothing in the analysis models that.
     - When neither the configuration nor Composer names a range, it is 7.0 to the newest version, narrowed to the versions on which the analysed files compile (decision 15). The program can't run where its own files don't compile. A file loaded only through guarded includes doesn't narrow the range. When the files share no version, the whole range is read and the conflict is reported.
     - Over a range, the engine is read version by version, as decision 6 reads a builtin (CHECK-10). An expression's type is the union of its types at each version, and what only some versions do is possible in part of the range. So over 7.4 to 8.5, `0 == 'a'` is `bool`, and `1 / 0` warns on 7.4 and throws from 8.0.
   - *Rows:* Q-15 is decided; decision 6's default range, STUB-7.1 and STUB-12.2 follow; CHECK-10 is new; VAL-2.10 lists the output changes.

## 3. LOAD: the session, the walk, the load's merge, how long a tree lives

### Tasks

| id | task | from | state | needs |
|---|---|---|---|---|
| LOAD-1 | One session for the CLI, the server and the tests: one identity, one configuration, one load, one analysis | 20 G 1 | DONE (LOAD-1.6 SUPERSEDED by REG-16.4) | — |
| LOAD-1.1 | **One identity.** A file is its resolved path, as PHP's `include_once` and `__FILE__` see it. Listed paths are normalized lexically. Each link is resolved once, by `realpath`: a target under a walked root is skipped, and a target outside every root is claimed in a set keyed by its resolved path. A link cycle ends where a directory's `(device, inode)` equals an ancestor's (on Windows, the volume serial and file index). Directories that are not links are never merged by identity, and hard links stay two files | 20 G 1 | DONE `aab67287` | — |
| LOAD-1.2 | **What the identity decides.** A file's spelling, `__FILE__`, `__DIR__` and Composer owner come from its resolved path. Its role is that of the strongest configured root containing it. `Session::classify(path) -> (FileId, role, is_stub)` serves the load and every edit entry point, with an alias table from path to `FileId` | 20 G 1 | DONE `7bef998a` | LOAD-1.1 |
| LOAD-1.3 | **One configuration.** `Session::resolve` is the server's resolution too, with one error policy as a parameter (fail, or degrade and report). `workspace_config::resolve` is deleted. A reload applies the difference in file sets | 20 G 1 | DONE `c9f6a85c` | — |
| LOAD-1.4 | **One load.** `Session::load` reads through a file source with an in-memory overlay: the server's open documents, the tests' files. `load_file`, `load_analyzed_file`, `Project::load_files` and the server's walk are deleted. `update_file` builds through the load's `parse_file`. The read hashes every file's bytes, and `FileState` keeps the hash | 20 G 1 | DONE `3334ebc9` | — |
| LOAD-1.5 | **One analysis.** `finalize` takes the results by value. The server's first publish is `Session::analyse` plus `finalize`. An edit rechecks through the same per-file check. The whole-program passes run again on save, debounced. `defer_inline_ignores` goes | 20 G 1 | DONE `5061565f` | — |
| LOAD-1.6 | **What the server keeps.** `built_once` splits: `keeps_as_built` decides only the snapshot, and the file's role decides whether a tree is kept. A context file's tree comes from `ast_of`'s re-parse cache, its text read on demand and checked against the kept hash. Only open documents' text is kept. The use-site index becomes a per-file product, stored in the per-file cache | 20 G 1 | SUPERSEDED by REG-16.4 (decision 3). The engine keeps and evicts trees as parse results and reads a file's text on demand. The use-site index is a per-file product query, which CACHE-13.2 persists | — |
| LOAD-1.7 | **Folders.** Each workspace folder has its own session and project, and a document belongs to the deepest folder that contains it | 20 G 1 | DONE `11e820f8`, `b97bdbd2` | LOAD-1.3, LOAD-1.4 |
| LOAD-1.8 | **Off the message loop.** The load runs on the pool and reports through `$/progress`; edits that arrive meanwhile are queued as overlays | 20 G 1 | DONE `99b1c2e4` | LOAD-1.4 |
| LOAD-2 | A parallel walk: one listing per directory, path data from the walk, each path stored once | 20 G 2 | OPEN | — |
| LOAD-2.1 | **One listing per directory per load.** The vendor resolver's listings become the walk's directory table. The vendor context lists each file once: a classmap or `files` entry under one of its PSR-4 directories is dropped at discovery, by a trie of the resolved directories. The stub directories are walked once, as stubs | 20 G 2 | DONE `3d2dc7c3`, `08877e58`, `5cca45c1`; the drop is a guard on each file's directories (`walked_by_a_directory`), not a trie, which is not exact for a directory that cannot be read | — |
| LOAD-2.2 | **One task per directory, on the pool.** Entries are sorted by name bytes, and each entry's kind comes from the dirent. Links follow LOAD-1.1's rules, with cycles found against the task's ancestors. The subtrees of stronger roots and of promoted packages are pruned, so each file is listed once and its role is final before its parse. `collect_php_files_excluding`, `path_under_excluded_dir` and `DEFAULT_SCAN_EXCLUDED_DIR_NAMES` are deleted | 20 G 2 | Walk DONE in `85a4ecad`. READY: the name floor's replacement, decided on 2026-10-17 (`ITEMS_1_TO_3_FIXES_PLAN.md`'s decision 7). The floor is `path_under_excluded_dir` and `DEFAULT_SCAN_EXCLUDED_DIR_NAMES`, which the editor still uses for its degrade root (no `paths`, `session.rs:695`), for a file under no configured root (`unlisted_role`, `session.rs:369`), and for a never-seen path (`update_file_cancellable`, `project.rs:1922`). Those three skip Composer's vendor directory by its resolved path, so a `vendor/` outside Composer is walked, and keep the `.git` and `node_modules` names until `exclude_paths` (LOAD-7.8) exists; then the function and the constant are deleted. The plan's "after item 1 no walk uses a list of directory names" is not true until then. A promoted package is an analysed root, so the rule that leaves a context walk under a stronger root with no names unwalked covers it. A bind mount that makes a directory its own ancestor inside an analysed root is a divergence: the old walk listed that directory's files again under the context path, the new walk does not. A link cycle's tree is listed once, a delta, approved (VAL-2.2) | LOAD-1.1, LOAD-1.2 |
| LOAD-2.3 | **Spellings built, not respelled.** A task carries its directory's configured spelling, resolved path and path hash; a child's are its parent's plus its name | 20 G 2 | DONE `07595369`. The configured spelling is not carried: the walk builds every path from its resolved parent, so no respelling runs. A reported file's relative path is still taken in `finalize_results` | — |
| LOAD-2.4 | **Parsing starts as directories are listed.** Each listed directory's PHP files go to the pool as one task, parsed with the worker thread's name cache, which the load owns and clears at its end | 20 G 2 | DONE `df804239`. The walk hands each directory's files to the sink as it reads the listing, when one walk alone reaches the directory, so their roles are final and the session parses them on the pool while the walk goes on. The files not handed over are parsed after the walk, as before. **Deviation:** the design's premise, that each file is listed once by the walk of its own role, fails: a stub or analysed root nested under an analysed root with exclusions lists its files twice. So the rule counts the walks that reach a directory. A walk reaches it when its root holds it, no directory from the root down is one that walk excludes, and a context walk does not count a directory a covering root holds (a stronger root that excludes no names), since it lists nothing there (`prunes`). A file two walks reach, a file only a link target reaches, and an explicit file a directory walk also lists are handed over by no one. Each result is placed by its listing slot, so the ids and the output do not change. The name caches are one per pool thread for each load (`Names`), where HEAD made one per split of the parallel iterator (`map_init`). Measured on the debug binary with composer on: 972 of 2284 files on vendor/doctrine-collections and 6816 of 8066 on vendor/monolog are parsed during the walk (319 and 414 before the covering rule was counted). The output matches HEAD on both projects, with composer on and with `--no-composer`; the two vendor/monolog files that hit the debug assertion at `crates/types/src/union.rs:302` do so at HEAD too. Wall time and the Piwigo gate are not measured yet (they need approval) | LOAD-2.2's walk (`85a4ecad`), not its deletion |
| LOAD-2.5 | **Order without comparisons.** Each result is keyed by its root's index and its entries' index path. Roots are sorted in component order, and nested roots are dropped. An explicit file under no root is placed by one binary search, and ids follow the placement | 20 G 2 | DONE `8590ff55`, with two deviations from the design above. Nested roots are not dropped, and the roots are not sorted: a nested root keeps its own role and exclusions (a context root under an analysed root that excludes names, and a stub root under an analysed one, which keeps the stub mark), so dropping it would lose files or marks. The walk instead places each file in the slot of its entry in its directory's listing. The directories with listed files form a tree, with a node for each and for each directory above them up to the top, one slot per entry; a depth-first walk lists the files in path order, and the slot is the index path, so no key is made. A file listed by two roots takes one slot, with the strongest role and the stub mark of both. Ids are the same as before: positions in the listing, context files first. A file whose directory cannot be listed has no slot; it is merged in by a sort, and only a walk with such a file sorts. The sorts in `load` and `load_listed` (now a partition) and in the vendor `dedup` (now `keep_first`) are gone, and `sort_paths` has no caller, so it is deleted. The overlay listing and the test helper still sort, since they are not walks. Tests in `walk_identity.rs`: nested roots, a link into a root, a dangling link, reversed root order and the unplaceable fallback, all in path order. 157 project, 291 LSP, 14 PHP API and 51 composer tests pass; clippy is clean. The Piwigo gate and wall time are not measured yet; the fixtures and the gate run at item 2's end | LOAD-2.2's walk (`85a4ecad`), not its deletion |
| LOAD-2.6 | **Path data from the walk, all resolved.** A root's absolute path comes from one `realpath`, and a child's is its parent's plus its name. The package index stores each package directory once, so `owner_of_dir` is one trie lookup per directory, and the server's single-file lookup uses the same implementation | 20 G 2 | DONE `c1db8372`, `c7ff651c`: the index keeps resolved directories only, and `owner_of_dir` is one trie lookup of a resolved path. The walk finds each directory's package once and each listed file carries it, so the parse takes it from the listing (`Project::note_owner` is gone); the single-file lookup, `owner_of_file`, calls the same `owner_of_dir`. A root is resolved once, and a child is its parent's plus its name (LOAD-2.3). `absolute_path` still normalizes the paths a caller gives the project (`update_file`, `checker_for`). That is off the load path, and it stays because the project's API takes spelled paths. A path spelled through a vendor link now falls under the root package, while the walk lists the link's target under the target's own package. 154 project, 291 LSP and 14 PHP API tests pass. Output and wall time are not measured yet | — |
| LOAD-2.7 | **A path stored once.** `FileTable`'s map becomes a `hashbrown::HashTable<FileId>` that hashes the slot's path with the walk's hash | 20 G 2 | DONE `2cd13c8e`: the map is a `hashbrown::HashTable<FileId>`, and each slot holds its path and hash. `with_paths` takes the walk's hash, and a debug assertion checks that each hash is `path_hash` of its path. 153 project and 291 LSP tests pass | — |
| LOAD-2.8 | **The walk's other allocations.** `installed.json` and the Composer manifests are deserialized straight into their typed form, `eval_concat` builds into one buffer, and `ignore_errors::filter` matches the text the walk built | 20 G 2 | Parts 1 and 2 DONE: `27c3b093` (the manifests and `installed.json` read member by member; a repeated key keeps its last value, a wrong-shape member is skipped, and the documents that `Value` rejected for an out-of-range number or for nesting past serde_json's recursion limit are read as `json_decode` reads them, with nesting capped at its 511), `53e6f766` (`eval_concat`). Instructions and wall time are not measured yet (needs approval). Part 3 DONE `47adfd19`: `ignore_errors::filter` borrows the path's text, which is what `display` writes, instead of a text the walk builds. The walk would build it for every listed file, and the filter reads only the files with diagnostics | — |
| LOAD-3 | Context trees dropped on the worker that parsed them | 20 G 3 | PART: LOAD-3.2, 3.4, 3.6 and 3.3's load part are in; its premise was halved by PROD-3 | — |
| LOAD-3.1 | Whether a tree is kept is one function of (role, open), in the load and in `update_file` alike; today `update_file` keeps every tree. The insert's assertion that an analysed file has its tree stays | 20 G 3 | OPEN | LOAD-2.2 (roles final before the parse), REG-16.4 (the parse as a query whose eviction this is; it took LOAD-1.6's `built_once` split) |
| LOAD-3.2 | A context file's state has the cached shape: the worker builds it, so a parsed and a cached context file give the same state. It keeps no ignore comments | 20 G 3 | DONE `001bd4f2`. The parse keeps a file's ignore comments only when the file is analysed: `read_and_parse_file` takes the role from the listing, and `update_file_as` reads the file's flag before its parse, so an edit keeps the role it had. `ast_of` and the two test parses that read only a tree pass `false`, and `UnindexedFile` no longer holds the flag, which nothing read. The CLI output cannot change: `apply_inline_ignores` and `check_banned` read the comments of analysed files only. The one route that showed a context file's malformed-comment report was `check_file` for an open, unchanged, non-analysed document (`lsp.rs:1152`); it shows none now, which matches PHPStan, which reports nothing for a file it does not analyse. Measured: `check-project vendor/monolog` gives the same sorted stdout as HEAD with the file cache off, and cold and warm with it on. Tests: three in `inline_ignore_comments` (a context file keeps none, an analysed file keeps its own, an edit keeps the role); lib 1151 and fixtures 3832 pass; clippy is clean. The tree is still kept, which LOAD-3.1 changes | — |
| LOAD-3.3 | The drop runs in parallel, on the thread that allocated the tree | 20 G 3; fixes R11 | PART. **The load: DONE `a5a4d85f`.** In a project built once `keeps_tree` is a function of the role, which LOAD-2.2's walk makes final before the parse, so `read_indexed` drops a context tree on its worker once its cache entry is stored, and `insert_numbered_file` asserts that a file arrives with its tree exactly when the project keeps it. **Measured** (off, against LOAD-8.1's `6016499f`): callgrind 20,809,876,336 Ir, −138.6 M, since the frees became thread-local (mimalloc's cross-thread paths: `mi_free_try_collect_mt` −37.0 M, `mi_abandoned_page_try_reclaim` −34.0 M, `_mi_page_free_collect_partly` −28.7 M); the main thread fell from 534.6 M to 269.6 M. Perf, one recording each: `insert_numbered_file` 56 main-thread samples (112 ms, 51 of them freeing trees) → 3 (6 ms); the kernel's page zeroing (`__clear_pages_unrolled`) 491 → 200 samples, since the parse reuses the freed trees' memory instead of touching new pages; the recording 2.316 → 1.812 s. Wall, 20 pairs (`scripts/abab.py`, load 1.6 to 5.6): medians 1.983 → 1.796 s, paired median −217.5 ms (mean −259.1 ms), 18 of 20 pairs faster, Wilcoxon p < 0.001; peak RSS medians 1,121.8 → 903.3 MiB (paired median −213.8 MiB); CPU time medians 10.83 → 10.14 s (paired median −0.87 s). The Piwigo output was the oracle's in every run. Test: `load_text.rs`'s `a_project_built_once_keeps_the_tree_of_an_analysed_file_and_of_no_context_file`; the lib suite passes (1194). **`update_file`: OPEN,** since it keeps every tree until LOAD-3.1 | LOAD-3.1 (for `update_file` only) |
| LOAD-3.4 | A context file is read into the worker's reusable read buffer; only a file whose tree is kept gets a source of its own | 20 G 3 | DONE `845efaab`. Every file is read into its worker's buffer, one for each pool thread beside its name cache (`Workers`), not only context files: an analysed file's text is copied out as before. A buffer over 1 MiB is freed after its file. A file's text is copied out only when the file keeps its tree (`Project::keeps_source`), so a context file in a project built once keeps no text; its tree was dropped in the insert until LOAD-3.3's load part moved the drop to the reader (`a5a4d85f`). **Deviations:** the reader indexes a file's use sites before the insert, because only the reader holds the text, and `insert_numbered_file` no longer computes them from text (a debug assertion checks that a parse with a tree arrives indexed); `source_len` is now the text's length for every file (it was 0 for a batch file, which kept no text), and the batch check's order is unchanged, since `analyze_files_by_size` already lists the files largest first and `check_files` re-sorts them by the same lengths; `load_cache`'s test asserted the text a context file keeps under retention in a project built once, which the rule drops (no product path combines the two), so it now asserts the rule. Verified: the five tests in `load_text.rs` and the two changed tests pass; the `project::` and `lsp::` module tests pass (461); clippy is clean. Output identity and the full suites are not run: they wait for §11. Measured since (§1): this commit's lossy decode of every file's text, to take its length, costs 394 M Ir in the off run, which is the whole +380 M that items 2 and 3 added. Fixed by LOAD-8.1 (`6016499f`), with no ruling needed on the length: the bytes are checked first and read lossily only when the check fails, so a file's length, text and parse are what they were, for valid and invalid files alike | — |
| LOAD-3.5 | A file whose tree is not kept is parsed into the worker's scratch `Ast`, whose vectors are cleared, not freed, once its products are copied out | 20 G 3; decision 4 | OPEN | SYN-1 (1.2) |
| LOAD-3.6 | `ast_of` reads the overlay before the disk, so an open file with unsaved edits is parsed from its editor text | 20 G 3 | DONE `11f03ec9`. The overlay read was already in (`FileSource::read`, which `read_bytes` uses), so the row needed its test. The test found a gap the rule covers: `ast_of` kept each parse in a cache by path and never forgot one, so after an edit a file with no kept tree was parsed from its old text. The cache entry is now forgotten on every change to a file's bytes or state (`set_document`, `unset_document`, the insert, the update and the removal, which is `unindex_file`), and `set_source` empties the cache. Today the editor keeps every tree, so the cache is not reached there; LOAD-3.1 makes context trees go, and from then on it serves. Tests: two in `ast_of_overlay.rs` (the second fails without the forgetting); the `project::` and `lsp::` module tests pass (463); clippy is clean. Output identity is not run: it waits for §11 | — |
| LOAD-4 | The load's merge in parallel; files by id | 20 G 5 | OPEN | — |
| LOAD-4.1 | **Ids before the merge, deterministic.** Each file's declarations take a contiguous range, given by a prefix sum of the per-file counts in load order | 20 G 5 | SUPERSEDED by REG-16.2 (decision 3): an id is interned from (file, kind, folded name, occurrence in the file), so no file takes a range and no load order is needed for it | — |
| LOAD-4.2 | **Stable ids on the edit path.** A file parsed again keeps its declarations' ids, matched by kind, folded name and position. `changed_since` compares a kept id's old and new values | 20 G 5 | SUPERSEDED by REG-16.2 and 16.3 (decision 3): an interned id survives the parse that finds the same declaration, and the engine's early cutoff replaces `changed_since` | — |
| LOAD-4.3 | **The merge sharded** over every table the insert writes, by each key's own hash (`RefKey::hash`). Shards merge in parallel, each over the files in load order, so context files come first and an analysed file's declaration wins a collision. Tables with another key are filled in a second parallel step | 20 G 5 | DONE `6bda466c`, `0e55b8ab`, `773da0c8` and `cf37f8d9`, after `24d89f88` (each map and set drains) and `ac2fdd05` (the insert over borrowed tables). The declaration tables (`registry/declaration_merge.rs`) split into `SHARDS` parts by the declared name's folded hash, the key they hold. Each part applies its declarations in file order, in parallel, and joins back: the classes, functions, global constants, the unconditional sets, the stub-provided functions, the declaration sites, the assert-tag names, the PHPStan class files and the changed names. `merge_by_shard` (`shard.rs`) is the two-step merge both steps use. The use sites by target and by file, the duplicate sites and the reverse indexes of references are one batch per load, `Project::insert_file_tables` (`project/file_tables.rs`), sharded by their own keys. **Deviations:** the declarations route by the folded hash of their name, not by `RefKey::hash`, which routes the reference tables in the second step; the assert-tag set joins the first step, since its keys are names; `insert_numbered_file` stays serial on the main thread, one file at a time. Verified: the declaration merge is checked against the serial inserts on 1, 2 and 8 threads (`registry::tests::declaration_merge`), and the batch likewise (`project::tests::file_tables`); clippy is clean. The output check is in the note after this table | — |
| LOAD-4.4 | **Whole-program merges by path rank.** `judge_property_types` and the named-argument merge sort integers. Named-argument facts are recorded and merged only when `check_renamed_named_arguments` is on | 20 G 5 | DONE `412fafbc`. `FileTable` keeps each slot's place among all paths (`rank_of`, read through `Project::path_rank`, which gives a path with no number the last place). A load seeds the places from the path-ordered listing it reads (`set_path_order`, `ids_in_listed_order`), so it sorts nothing. A new number clears them, and the next use sorts them for. `judge_property_types` sorts its files and its sources by place, `check_whole_program` its programs, and `check_project` the named-argument calls. The rule is off by default, so a default run records no named-argument calls. **Deviations:** the plan computes the ranks at load and again before each whole-program pass. `judge_property_types` runs inside `check_files`, which `check_file(&self)` reaches, so the table fills lazily under `&self` (a `OnceLock`). It is cleared on numbering only: a path keeps its slot when its file is removed or replaced, so its place holds. An unnumbered path holds no file, and its check records no facts, so its last place cannot change a merge. The other path sorts (architecture, composer hygiene, class shape, duplicate classes and functions, output) are outside this row and are not converted. Verified: `cargo fmt` and `cargo clippy --all-targets` are clean; the `project::` tests pass (181, three new in `project/tests/path_places.rs`: an overlay load and a batch load set the places from their listing, and a project numbered before its load sorts them on first use); the `file_table` tests cover the seed, an order that leaves a path out or repeats one, and the debug check on the order; `lsp::` (291) and the `renamed_named_arguments` and `property_assign_facts` fixtures passed on this code before the path-places test was added. Output identity and the full suites wait for §11 | — |
| LOAD-4.5 | **Keys stay keys, and files stay `FileId`s,** in `walk_affected` and the scope bookkeeping, in the duplicate sites (a `PathBuf` per site today) and in the declarations' origins (an `Arc<Path>` per file today) | 20 G 5 | DONE `e9b253b4` (the duplicate sites hold their file's id) and `4e443227` (the declarations' origins are `Origin::File(FileId)`, and the address map `by_origin` is gone). The part on `walk_affected` and the scope bookkeeping goes with REG-16.4, as the note below says. **Deviations:** the design numbers a file before its parse; the walk parses files as it finds them (`StreamParse::take`), so a parse builds its declarations `Origin::PendingFile`, and `ParsedFile::bind_file` gives them their file once it has one (in each load, after `number_files`, and in an edit, after its parse). `insert_declarations` asserts in debug builds that nothing pending reaches a registry. The file cache stores no file, so a declaration read from it is pending until its file is numbered. `FileId` is public as an opaque type, because `Origin` is public; `Origin` is not weakened. Verified: `cargo check` and clippy are clean; the `project::`, `lsp::`, `registry::` and `checker::` lib tests pass (185, 291, 288 and 192) | — |
| LOAD-4.6 | **The frontier starts from ids.** `closure_scope`'s frontier starts from REG-13's changed ids, the stub dependency index holds `RefKey`s and stub base ids, and the scope is REG-13.6's bitset | 20 G 5 | SUPERSEDED by REG-7 and REG-16.4 (decision 3). `closure_scope` computes the finishing passes' scopes (`finishing_scopes`, `project.rs:1359`), which REG-7 replaces. The engine runs again only the queries whose recorded reads changed, so no frontier or scope is left | — |
| LOAD-4.7 | **The indexes hold posting lists, not sets.** A reverse index maps a key to a sorted `FileId` list, with one id held inline. A file's own references are slices, built once at parse, in content order (kind, `RefKey::hash`, folded bytes), deduplicated, and stored that way in the per-file cache | 20 G 5 | OPEN, blocked. Its file-references part is one change with PROD-5.7 (OPEN), which needs PROD-5.8 and PROD-5.2, both READY and not started (checked 2026-10-17). Its reverse-index part (`dependents`, `signature_dependents`, `project.rs:250`, `:253`) goes with REG-16.4 (OPEN, decision 3), as the note below says. Nothing of it is built | — |
| LOAD-5 | Each tree kept until its last reader; the project-wide passes read facts, not trees. Memory only | 20 G 19 | OPEN | — |
| LOAD-5.1 | The project-wide passes read per-file facts: architecture, banned constructs, class shapes, Composer hygiene, mock hygiene, unmatched ignores and the use-site index. Each pass's per-file part becomes a family of CHECK-6.1's traversal, or of PROD-5.3's walk when it needs no types | 20 G 19 | OPEN | CHECK-6.1 or PROD-5.3 (per pass); PROD-5.1 (class shapes and unmatched ignores read the declaration list); REG-16.4 (the use-site index as a per-file product, which it took from LOAD-1.6) |
| LOAD-5.2 | A batch run frees each analysed tree when its file's check ends: the check owns the tree for its job, and files are checked largest first. `judge_property_types` reads the recorded writes, and parses again only the files outside the batch | 20 G 19 | OPEN | LOAD-5.1, LOAD-3.1 |
| LOAD-5.3 | The server keeps the trees of open documents plus an LRU of recently checked files. Any other tree is parsed again on demand through `ast_of`, from text checked against the kept hash | 20 G 19 | SUPERSEDED by REG-16.4 (decision 3): the server and the batch run keep trees alike, as parse results the engine evicts and parses again on demand | — |
| LOAD-5.4 | No reader gets an empty file: `FileState::ast` goes, and every reader asks `ast_of` | 20 G 19 | READY | — |
| LOAD-6 | Check UTF-8 with SIMD (`simdutf8`) | off P10 | SKIP: built, measured and reverted: −0.47 M against ≈ −22 M estimated. Under the workspace's no-`unsafe` rule, the checked bytes can't become a `String` without being checked again. LOAD-8.1 (`6016499f`) takes `simdutf8` with the checked text borrowed, so that copy does not arise (its record) | — |
| LOAD-7 | **The configuration the crate split's C1 left:** its follow-ups not done on this branch (Q-11's audit). Its one settings assembly is LOAD-1.3, its one walk per file LOAD-1.1 and LOAD-2.2, and its PHPStan configs ROADMAP Phase 183's | split C1 | OPEN | — |
| LOAD-7.1 | **Nested validation.** `deny_unknown_fields` on `IgnoreErrorEntry`, `RestrictedSymbolEntry`, `ParamCondition`, `ParamRef`, `ParamFlagsCondition`, `DoctrineCustomFunctionEntry` and `sql::config::SqlReflectionConfig`, none of which has it. `[[ignore_errors]]` takes PHPStan's whole shape and semantics: `raw_message`, the plural `messages`, `raw_messages`, `identifiers` and `paths`, the string form, a per-entry `report_unmatched`, entries merged with their counts summed, and `count` enforced on single-path entries; it is "parsed but not enforced yet" today (`ignore_errors.rs`). Messages compile once, at load, through PCRE2, and a malformed one fails the load, as `IgnoredRegexValidator`'s checks do; today a message compiles with `regex` at each match, and a malformed one never matches. A config error already names its dotted key | split C1 | READY | — |
| LOAD-7.2 | **Activation reaches library typing.** `Extension` impls for mockery, nette-tester, codeception, type-perfect, disallowed-calls and sql (13 extensions have one, these none); `test.risky` under an activation of its own (ext-extras or an explicit key) instead of `ext.phpunit.check_risky_tests`, and Pest's risky tests under `[ext.pest]`, which has only `enabled`; strict-rules activation sets the core settings that phpstan-strict-rules' `rules.neon` sets, as each exists (LOAD-7.8). The plan also gates PHPUnit's assertion narrowing and mock types on ext-phpunit's activation, which nothing reads today (`checker/narrowing/phpunit.rs`). A PHPUnit assertion throws whenever PHPUnit runs it, so its narrowing is a fact of the program, and that half needs a ruling before it is gated | split C1 | OPEN; the PHPUnit half needs a ruling | — |
| LOAD-7.3 | **Paths and matching.** `ignore_errors.path` resolves against the config file's directory and compares as a normalized path, by one matcher with PHPStan's `FileExcluder` semantics (a literal file, a literal directory as a prefix, or an fnmatch pattern), shared with `exclude_paths` (LOAD-7.8); `glob_matches` goes (`ignore_errors.rs:96`). `--set` paths resolve against the working directory. `psr3_context_key_pattern` is a PCRE pattern matched through PCRE2, where it is a `regex::Regex` today (`checker/settings.rs:95`) | split C1 | READY | — |
| LOAD-7.4 | **Entries validate at load.** A `restricted_symbols` member entry without `class` is a load error, and a user entry wins over a preset's for the same symbol; `restricted_symbols.rs` validates nothing today. `[[assertions]]` and `[[class_argument_returns]]` type strings parse at load, and an entry naming a missing function, method or parameter is a load error; their entries are checked for their shape only today (`assertion_tables.rs:87`) | split C1 | READY | — |
| LOAD-7.5 | **Config discovery.** `check-project` without `--config` reads the first of `.phpstan-rs.toml`, `phpstan-rs.toml`, `.phpstan-rs.dist.toml` and `phpstan-rs.dist.toml` in the working directory, by one lookup that the LSP shares; it reads none today, and the LSP reads only `phpstan-rs.toml` (`lsp/workspace_config.rs:66`). The LSP watches the config files | split C1 | READY | — |
| LOAD-7.6 | **Keys.** Grouped keys name the upstream parameters they cover; one casing; no key repeats its section's name, as `ext.psr3.check_psr3_logging` and `ext.psr3.check_psr3_exception_context` do today; a PHPStan parameter's own name where one exists. PHP versions and extension activation read every discovered Composer root's manifest | split C1 | READY; a delta in the config's keys | — |
| LOAD-7.7 | **Tests and docs.** Config tests for nested keys, `--set` of an unknown key and error key paths; a config reference generated from the section doc comments; `check-project`'s help corrected: an unset `--level` caps at `DEFAULT_REPORT_LEVEL` 6 (`project/session.rs:40`, `:347`) where the help says it reports everything, and `--stub-path` and `--format` stop citing the deleted `check_duplicate_class_declarations` flag and `config.rs` (`main/check_project.rs:71`, `:91`). `declared_php_versions` has its doc comment already | split C1 | READY | — |
| LOAD-7.8 | **The missing settings,** each with the phase the plan names for it: the PHPStan parameters that change analysis (mixin exclude classes, universal object crates, global type aliases, `treatPhpDocTypesAsCertain`, `polluteScopeWith*`, early-terminating calls, the exception model, additional constructors, `exclude_paths`, `file_extensions`, the `report*` and `check*` parameters, `featureToggles`, `skipCheckGenericClasses` and the rest). One of about 40 exists (`report_possibly_nonexistent_general_array_offset`). The version range is C5's (STUB-7.1); the others' phases (C7, C8, D2, D5, D6, E3, F3) are not tracked here | split C1 | OPEN | each its phase |
| LOAD-8 | **The fixes for the costs items 1 to 3 added,** and for the load's serial stretches that MEAS-4 measured: `ITEMS_1_TO_3_FIXES_PLAN.md`, which holds each row's design, numbers and test. LOAD-8.n is the plan's Rn. One commit each, in the plan's order (its decision 4), since the user's second go (2026-10-17): 8.1; LOAD-3.3, CACHE-12, 8.15; 8.2, 8.3, 8.4, 8.6, 8.14, 8.13; 8.10; 8.5, 8.8; 8.9; 8.7 | fixes | DONE (2026-10-17): every row is in (8.1 to 8.10 and 8.13 to 8.15; 8.11 and 8.12 are LOAD-3.3 and CACHE-12), and the plan's end-of-batch checks passed on `8a1ca1e0`: the lib suite (1202), the fixtures (3832) and the integration tests (cli 44, file_cache 5, lsp_exit_code 2, scaling 1, stub_base_reentry 1, stub_index 1, version 2); `check-project vendor/monolog` wrote the same stdout and stderr at `aa896697` and at `8a1ca1e0`, byte for byte (25,951 lines), with the file cache off, cold and warm, and the warm runs wrote no cache file; and the Piwigo output was the oracle's in every run of LOAD-8.7's code, off and warm. Off, the rows took the run from `aa896697`'s 21,329,725,495 Ir, measured again beside them, to 20,373,086,199 (−956.6 M) | — |
| LOAD-8.1 | **UTF-8 validated once, with the fast check.** `read_and_parse_file`'s `String::from_utf8_lossy` (`session.rs:1224`), 394 M on the pool, becomes `simdutf8::basic::from_utf8`, borrowed, with the lossy decode only for a file that fails it. One helper serves the three other lossy decodes (`session.rs:1213`, `project.rs:2049`, `lsp.rs:1290`). A file's length, text and parse stay the same for valid and invalid files, so LOAD-3.4's note and LOAD-6's record are corrected with it | fixes R1 | DONE `6016499f`. `source::decode_text` checks the bytes with `simdutf8` 0.1.5 and reads them lossily only when the check fails; the file read, a cache hit's kept source, `update_file_as` and the language server's read of a file use it, and `ast_of` parses from the checked text. **Measured** (callgrind, off, against `aa896697`'s code built again, 21,329,725,495): 20,948,523,065 Ir, −381.2 M (−1.79%). `Utf8Chunks::next` falls from 532.5 M to 138.7 M of self cost, and `simdutf8`'s check adds 10.2 M; `read_indexed` falls from 8,950.7 M to 8,567.3 M inclusive. Warm: 13,346,625,329 → 13,209,699,103 (−136.9 M; `read_indexed` 3,889.5 M → 3,760.7 M). Wall, off, 20 pairs (`scripts/abab.py`, load 3.2 to 4.6): medians 1.975 and 1.998 s, paired median +8.0 ms (mean +48.7 ms), 10 of 20 pairs slower, Wilcoxon p = 0.370, so no change beyond the noise. The Piwigo output was the oracle's in every run. **Tests:** `load_text.rs` gains the valid counterpart of the non-UTF-8 test (a multibyte file is weighed by its byte length, keeps its text, and declares its function) and a differential test of the check and the decode against `std` on every cut of a multibyte text and every kind of bad sequence at every place in one; the lib suite passes (1194), clippy is clean. `cargo deny` reports four rustls advisories (RUSTSEC-2026-0346 to 0349) that predate the row; licenses, bans and sources pass | — |
| LOAD-8.2 | **A path that carries its hash.** `PathHash`, `path_hash` and `child_hash` move into the `composer` crate, and `HashedPath { path, hash }` keys `Listed`, `Found::File`, `DirListings` and the walk's tables. A hash is made by `HashedPath::new` and extended by `child`, so a recomputation is a call a review finds. `Tree::node` takes its directory's hash from the listing | fixes R2 | DONE `68af1c72`, `276ed5ad`. `composer::hashed_path` holds `HashedPath`, `path_hash` (for a lookup of a path that is not held) and `hashed_ancestors`. A `HashedPath` carries its parent's hash too, so a listed file names its directory's hash, and `Found::File` carries only the file's path, where the row had it carry the directory's `HashedPath` (a copy of the directory's path per file). The walk's jobs, listed files, found files and tree nodes are `HashedPath`s, and `DirListings::listing` takes one. A new directory's nodes are found by `hashed_ancestors`: one fold of the hashes, then the ancestors from the parent up to the nearest one with a node. The first commit made every ancestor of every new directory (1,899 calls, 17,930 ancestors, 11.0 M); the second makes them as it goes (3.0 M). **Measured** (off, against LOAD-8.15's `d6ca3442`): callgrind 20,785,126,006 Ir, −18.5 M. `Walk::merge` 50.4 M → 29.8 M inclusive; `DirListings::listing` 31.9 M → 19.6 M; the walk's per-file `path_hash` (21.1 M self) is gone, `Path::hash` falls by 11.9 M and `Components::next` by 11.5 M of self cost, and composer's `path_hash` costs 10.8 M (the editor's lookups by path and `HashedPath::new` at the walk's entry points). Wall, 20 pairs (`scripts/abab.py`, load 1.2 to 5.8): paired median +8.8 ms (mean −20.0 ms), p = 0.627, so no change beyond the noise; peak RSS unchanged (p = 0.48). A first set of pairs, at `68af1c72`, ran beside another session's job (load up to 12.6, σ 1.85 s) and is not used. The Piwigo output was the oracle's in every run. **Tests:** composer's `hashed_path.rs` (a path's and its parent's hashes are their components', an entry made by `child` is the path made whole, each ancestor's hash, the order of paths); the lib suite passes (1197: the walk's path-hash test moved to composer, which passes 56) | — |
| LOAD-8.3 | **A directory's reach carried down the walk.** Each directory task receives the walks that reach it and whether it is covered, where `Starts::reaching` scans every ancestor (quadratic in depth) and `prunes` does so per child. `Starts`' tables become `HashTable`s keyed by `PathHash`. The old `reaching` stays as a test oracle | fixes R3 | DONE `75b628f8`. A directory task gets a `Reaching`: the directory walks that reach it, each with its role and excluded names, and whether a covering root holds it. `Starts::below` makes a subdirectory's from its directory's: the walks that do not exclude its name, and the walks that start at it, found by its carried hash. A root's walk folds `below` from the top over its directory's ancestors (`hashed_ancestors`), where the row had one ancestor scan of the old `reaching`; a link target's walk carries none, as before. `prunes` reads the carried `covered`, and a sole-reach directory's file is checked against the file roots by its carried hash. `Starts`' directory and file tables and the covering roots are `HashTable`s keyed by the carried hash. The old scan stays as `reach_by_ancestors`, under `cfg(test)`, and `walk_dir` asserts that the carried state equals it in every walk the unit tests make. **Measured** (off, against LOAD-8.2's `276ed5ad`): callgrind 20,704,757,165 Ir, −80.4 M. `Starts::reaching` 74.8 M → 18.3 M inclusive and `Walk::prunes` 14.5 M → 0, for `Starts::below`'s 7.2 M; `Starts::of` 4.9 M → 0.8 M; `hashed_ancestors` 3.0 M → 6.3 M. Self cost: `Path::hash` −57.3 M, `parse_next_component_back` −15.7 M, `next_back` −8.2 M. Of the 18.3 M left, 16.1 M is the file roots' (`file_alone`, inlined into `Walk::run`: one fold per explicit file, 1,744 vendor files on Piwigo, mostly `Path::parent` 6.4 M and `below` 6.2 M), and 2.2 M the directory roots'; LOAD-8.7 lists it. Wall, 20 pairs (`scripts/abab.py`, load 1.1 to 4.1): medians 1.662 and 1.637 s, paired median +4.5 ms (mean +6.5 ms), 10 of 20 pairs slower, p = 0.940, so no change beyond the noise; peak RSS (p = 0.455) and CPU time (p = 0.940) unchanged. The Piwigo output was the oracle's in every run. **Tests:** the oracle's assertion, which fails in 4 `walk_identity.rs` tests when `below` keeps the walks that exclude a child's name; the lib suite passes (1197); clippy is clean | — |
| LOAD-8.4 | **The walk's hand-over keyed by the carried hash:** `into_parsed`'s map and `Session::load`'s removal per file (28,502 path hashes, on the main thread) become a `HashTable` keyed by `Listed`'s hash | fixes R4 | DONE `1ba57cc0`. The streamed parses keep each file's `HashedPath`, and `into_parsed` puts them in a `HashTable` keyed by the hash the path carries, with path equality; `Session::load` takes each listed file's parse out of it by the listed file's carried hash. **Measured** (off, against LOAD-8.3's `75b628f8`): callgrind 20,657,737,639 Ir, −47.0 M. The main thread 266.2 M → 238.4 M (−27.8 M): `Session::load` 151.8 M → 123.9 M inclusive and its `Path::hash` 53.4 M → 26.1 M, so the hand-over's 27.3 M is gone; the other −19.2 M is spread over the pool, inside MEAS-3's spread. The main thread's `Path::hash` left is the `checked` set's 13.65 M (LOAD-8.5), the diagnostics map's 8.4 M and `check_files`' 2.2 M (LOAD-8.8), and `add_file`'s 1.5 M (LOAD-8.14). Wall (`scripts/abab.py`, off, load 1.5 to 6.1): three sets of 20 pairs, since the first gave +55.5 ms (p = 0.145) for a change of 2 to 8 ms; the next two gave −2.1 ms (p = 0.852) and +12.9 ms (p = 0.575), and the 60 together +11.9 ms (mean +16.5 ms), 34 of 60 slower, p = 0.296, so no change beyond the noise; CPU time unchanged (p = 0.227). Peak RSS: −1.0 MiB in the first and third sets (p = 0.55, 0.74), +10.5 MiB in the second (p = 0.002), which did not reproduce; the hand-over holds 16 bytes more per file, about 0.5 MiB, so that shift is not its data. The Piwigo output was the oracle's in every run. **Tests:** no behaviour changes; the lib suite passes (1197), clippy is clean | — |
| LOAD-8.5 | **Per-file tables keyed by `FileId`:** `UseSiteIndex.by_file` and `file_tables::bucket`'s routing, and `judge_property_types`' `checked` set (13.65 M of path hashes each) | fixes R5 | DONE `0fb1a4de`. Both maps of `UseSiteIndex` hold the file's number where they held its path, so `file_tables::bucket` routes a file's sites by it, and the editor's references and rename read each site's path from the table. `check_files` finds each path's number once and carries it through `judge_property_types`, whose `checked` set is a `FileIdSet` and whose sources sort by `rank_of`, and through `finish_diagnostics`, which looked the number up by the path again. `number_files` takes the hash the numbering table keeps for a path (`id_for_numbered`), where it hashed the path again; that runs only on the editor's path. Where the row differs: `by_target`'s sites hold the number too, not only `by_file`, so no use site holds a path; and `check_files` finds the numbers once for the size, the judgement and the finish. **Measured** (off, against LOAD-8.10's `9513479b`, whose code `c8505326` is): callgrind 20,535,375,165 Ir, −46.9 M. `Path::hash` 50.4 M → 21.0 M of self cost: `file_tables::bucket`'s 13.65 M, the judgement's filter over every file 13.65 M and `check_files`' `checked` inserts 2.2 M are gone; `path_hash` 10.8 M → 5.4 M (the judgement's `path_rank` and `finish_diagnostics`' lookup, 2,792 each) and `Components::next` 51.6 M → 41.8 M (those lookups' path equality). `check_files`' main-thread cost, the judgement inlined into it, 39.7 M → 9.1 M inclusive. The `Path::hash` left is LOAD-8.8's: `run_finishing_passes`' result set 12.6 M, and the diagnostics map's 8.4 M (its inserts, its rehash and `apply_inline_ignores`' lookups). Wall (`scripts/abab.py`, off, 20 pairs, load 1.7 to 6.5): medians 1.937 and 1.908 s, paired median −74.7 ms (mean −13.3 ms), 8 of 20 pairs slower, Wilcoxon p = 0.601, so no change beyond the noise; peak RSS (p = 0.881) and CPU time (p = 0.601) unchanged. The Piwigo output was the oracle's in every run. **Tests:** no behaviour changes; the `project::` tests (among them `file_tables`, `use_site_index` and `property_assign_facts`) and the language server's references, rename and watched-files tests pass (206), clippy is clean | — |
| LOAD-8.6 | **Vendor discovery stops hashing paths and stat-ing files.** `DirListings.listed` is keyed by `PathHash`. `drop_files_under_dirs` groups the files by parent, folds each directory's components once and reads a file's kind from its parent's listing. `Resolver::path` decides containment once per listed directory and makes the as-written fallback's base once. The `composer` crate gains `hashbrown` | fixes R6 | DONE `59bf55bc`. **The drop:** `Walks` finds, once per file's directory, the nearest of the walks' directories at or above it, by `hashed_ancestors` against a `HashTable` of the walks' directories keyed by the hashes their paths carry. It reads each directory's kind below that one from its parent's listing, and the file's kind from its directory's, where every ancestor of every file was hashed and every file stat-ed. A walk's own directory is checked once (a directory and not a link, by `symlink_metadata`, and listed). **`Resolver::path`:** each listed directory carries its `Place` against the vendor directory: in it, its parent, or out of it. So an entry that is not a link lies where its directory does, by its name, and only a link is canonicalized and compared. The as-written fallback's base (`vendor_as_written`) is made once in `Resolver::new`, and a directory's as-written form and its places are kept beside its listing, so a fallback entry is that form joined with its name. The resolved paths are `HashedPath`s made by `child` from their directory's, so the deduplication hashes none again. Where the row differs: `DirListings` was keyed by the carried hash already (LOAD-8.2), and `hashbrown` was a composer dependency already. The drop and the deduplication run after the silenced panic hook is restored, so a panic in them is reported, and the old per-file check stays under `cfg(test)` as the oracle, asserted for every file. **Measured** (against LOAD-8.4's `1ba57cc0`): callgrind off 20,533,513,526 Ir, −124.2 M. The discovery thread 391.0 M → 262.0 M (−129.0 M); the other threads +4.7 M, inside MEAS-3's spread. On the discovery thread: the drop and deduplication 119.2 M → 23.9 M (the retain closure 114.0 M → 23.8 M, `keep_first` 4.6 M → 0.2 M); `Resolver::path` 97.8 M → 63.4 M (`starts_with` 25.2 M → 0.6 M, `lexically_normalized` 11.5 M → 0.8 M, `entry_canonical` 8.4 M → `HashedPath::child` 5.0 M; `listed`, the directory reads, 27.3 M → 28.0 M); self cost `Path::hash` 45.5 M → 0 and `Components::next` 39.6 M → 4.9 M; `symlink_metadata` 3.8 M → 0.1 M. `parse_generated_file`, 163.0 M, is LOAD-8.13's. Warm: 13,028,394,758 → 12,897,745,859 (−130.6 M), discovery 391.3 M → 262.1 M. Perf, warm, one recording each (load 1.1): the discovery thread 50 → 31 samples (0.023 to 0.121 s, then 0.022 to 0.085 s); under it the drop and deduplication 18 → 1 (12 of the drop's 16 were `statx`), `Resolver::path` 21 → 19 (`listed` 13 → 16). The main thread waits in `vendor.get()` 102 ms → 69 ms, and the pool's first sample moves from 0.138 to 0.104 s. Wall (`scripts/abab.py`, 20 pairs each): warm (load 4.0 to 5.3) medians 1.333 and 1.267 s, paired median −38.3 ms (mean −61.0 ms), 6 of 20 pairs slower, Wilcoxon p = 0.011; off (load 1.2 to 4.0) medians 1.620 and 1.619 s, paired median +1.5 ms, p = 1.000, since the stub build keeps the pool busy through the off run's wait. Peak RSS (p = 0.145 warm, 0.263 off) and CPU time (p = 0.218, 0.391) unchanged. The warm runs wrote no cache file, and the Piwigo output was the oracle's in every run. **Tests:** composer's `vendor_context_discovery.rs` gains a vendor tree with linked path-repository packages, a linked file, a directory behind a link, a file beside a walked directory, a lone file and a file that is not PHP, from which only the files a directory's walk lists are dropped; and `Place`'s property, that it places each entry of a directory against a base as `Path::starts_with` does. The oracle's assertion fails there when the drop takes a linked directory as listed. The composer suite passes (58) and the lib suite (1197); clippy is clean | — |
| LOAD-8.7 | **The remaining component walks on the load path,** enumerated by caller from a fresh profile, and each replaced with what the walk holds (a listed path's directory, name and root-relative form) or shown to be another row's. `walk::path_hash`'s one fold per path stays. A known site, measured at `75b628f8`: the file roots' reach, 16.1 M, folded from the top once per explicit file where the files share far fewer directories (LOAD-8.3's record) | fixes R7 | DONE `30aa2002`, `8a1ca1e0`. Read from the fresh off profile of LOAD-8.8's `f676049c`, after the rows that took their parts of the 266 M: there std's path code was 117.4 M of self cost, and its entries 140.9 M over 62 callers (each call into it from a function outside it, charged to the caller). A new module, `composer::path_bytes`, answers `Path`'s queries (`file_name`, `parent`, both from one reading, `starts_with`, `strip_prefix`, whether a path is absolute and normal, the components with the ancestors they end) from the path's bytes where its spelling gives the same answer (no empty or `.` component, no trailing separator) and asks `Path` otherwise, so every answer is `Path`'s own; differential tests check it against `Path` on generated paths of every shape. The callers replaced: the walk's tree, which reads a file's directory and name once; vendor discovery's resolver, its drop and its canonical directories; the PHP name checks; the Pest checks, where a `Pest.php` file is known by the path's last bytes; the report's root-relative paths; and the declarations' merge, which makes a file's root-relative path only for a PHPStan class it declares. What the walk holds is carried: a listed file is read by its own path, with no join to the root; a checked file whose path is absolute and normal is its own absolute path; a directory's entry is made in one allocation; the file roots of one directory and role share one job, so what reaches the directory and its owner are found once; each directory's place among the packages' directories is carried down to its subdirectories (`OwnerAt`); a path's ancestors come with their hashes and names from one pass; and what reaches a directory and the tree's new directories take the name their caller holds. Where the row differs: the name and the directory are read from the bytes, not carried in each path. A carried field would save the scan of the last component (7.1 M, for 44,643 paths), for a field in every `HashedPath` whose meaning depends on how its directory is spelled: state to keep consistent, which LOAD-8.4 rejected a dense index for. **Measured** (off, against `f676049c`, on x86_64 Linux): callgrind 20,373,086,199 Ir, −126.7 M: `30aa2002` −123.8 M, and `8a1ca1e0` −2.9 M, inside MEAS-3's spread (its path code −12 M by function, the allocator +5 M). Std's path code 117.4 M → 11.5 M of self cost, its entries 140.9 M → 16.2 M over 57 callers, and `path_bytes`' own reads 14.2 M: the last component's scan 7.1 M, the prefixes 4.7 M, and whether a checked file is absolute and normal 1.7 M. What stays: each entry's path, made once (`PathBuf::push` 3.4 M); the probes' path equality, about 5.7 M; the report's map, keyed by the root-relative path (1.4 M); the table's copy of each path, made beside the parse (1.5 M); and the package index's walks, once per root and per package (0.7 M). The ancestors' byte pass costs what the components' fold and the scans back cost (+5.1 M against −4.8 M); one that checks the spelling as it splits would take about 2 M more. Wall (`scripts/abab.py`, 20 pairs each, against `f676049c`): `8a1ca1e0` off, load 1.9 to 4.8, medians 1.823 and 1.815 s, paired median −39.2 ms (mean −32.5 ms), 8 of 20 pairs slower, p = 0.313; warm, load 4.8 to 5.1, medians 1.358 and 1.348 s, paired median −7.4 ms (mean +20.2 ms), 9 of 20 slower, p = 0.627; `30aa2002` gave p = 0.093 off and 0.370 warm. So no change beyond the noise, with peak RSS (p = 0.455, 0.881) and CPU time (p = 0.108, 0.881) unchanged. The Piwigo output was the oracle's in every run, off and warm, and in both callgrind runs. **Tests:** no diagnostic changes; the composer tests (67, among them the differential tests and the owner's carried place) and the lib suite (1202) pass; clippy is clean **Platforms:** the −126.7 M is x86_64 Linux's; see the note "LOAD-8.7 on each platform". | — |
| LOAD-8.8 | **Identity is a `FileId` inside the scope that owns the table, and the resolved path where the scope spans tables.** Diagnostics are keyed by `FileId` until `finalize_results`, which converts each once. The `composer.json` manifests are numbered at load and merged into the rank seed. The language server keys its per-project results by `FileId` and its server-wide state by path | fixes R8 | DONE `f676049c`. Before `finalize_results` a project's diagnostics are keyed by the file's number: the per-file checks, where `check_files` hands `checker_for` the number it holds, so its `id_of` goes; the parse failures, whose files are listed and so numbered; `apply_inline_ignores`, which walks the files with their numbers; the whole-program passes, which held the number already; and `run_finishing_passes`' affected files, which it returned as a set of paths. `finalize_results` names each file once, by its root-relative path from the table, and the report's maps stay keyed by that text. The project-wide checks (`class_shape`, `banned`, `mock_hygiene`, `architecture`, `composer_hygiene`) walk the files with their numbers and sort them by place (`rank_of`), where `class_shape` sorted by path. Both loads number the `composer.json` manifests (`number_manifests`) and place them among the listed files by path (`ids_in_path_order`), so the rank seed covers every number; a `path_places.rs` test asserts it. The language server keys its per-project results by number and converts them when it publishes; its server-wide state stays keyed by path. Where the row differs: Project's public result maps are keyed by number too (`check_paths`, `check_all`, `update_file` and its variants, `remove_file`, `remove_files`, and the editor's unchecked files), with `path_of` to name a key, so no caller converts a key to a path and back; `remove_files` numbers a path it has not seen, so every path it is given is a key of its result; `check_files` takes numbers, and `Loaded` carries the load's numbers of the analysed files, the largest first; `line_in` and `ast_of` take a number, and the named-argument facts carry it; `path_rank` is for tests only. **Measured** (off, against LOAD-8.5's `0fb1a4de`): callgrind 20,499,751,098 Ir, −35.6 M. `Path::hash` 21.0 M → 1.3 M of self cost: the finishing passes' result set 12.6 M, the check's diagnostics map's inserts 2.2 M and its rehash 2.7 M, and `apply_inline_ignores`' lookups 2.2 M are gone; the 1.3 M left is the report's map, keyed by the root-relative path that the ignore and baseline filters match. `path_hash` 5.4 M → 0 and `Components::next` 41.8 M → 31.0 M (the lookups' path equality 9.8 M, `class_shape`'s path sort 1.0 M). Inclusive: `checker_for` 29.8 M → 20.7 M, `check_project` 22.9 M → 10.5 M, `finalize_results` 11.4 M → 8.8 M, `check_class_shapes` 7.7 M → 5.3 M, `apply_inline_ignores` 3.1 M → 0.6 M; the main thread 228.5 M → 213.5 M. Wall (`scripts/abab.py`, 20 pairs each): off, load 1.3 to 5.1, medians 2.017 and 2.030 s, paired median +35.9 ms (mean −71.9 ms), 12 of 20 pairs slower, p = 0.940; warm, load 5.1 to 6.0, medians 1.337 and 1.304 s, paired median −26.9 ms (mean −28.6 ms), 7 of 20 slower, p = 0.247; so no change beyond the noise in either, and peak RSS (p = 0.156, 0.627) and CPU time (p = 0.940, 0.232) unchanged. The Piwigo output was the oracle's in every run, off and warm, and in the callgrind run: the oracle check the plan runs right after this row. **Tests:** no diagnostic changes; the lib suite (1201), the fixtures (3832) and the integration tests (cli 44, file_cache 5, lsp_exit_code 2, stub_base_reentry 1) pass; clippy is clean | — |
| LOAD-8.9 | **The bytes hashed once in the editor:** a document's digest is taken once per edit and handed to every project that holds the file, where opening or changing it hashed it once per such project, to find it unchanged, and each update hashed it again for the digest it keeps. The batch run is unchanged | fixes R9 | DONE `098af486`. The row as first written had the per-file cache key hash the content hash with the path and flags, on the reading that the editor hashes the bytes for both. Read in the code, at `aa896697` as now, no load takes both: only a project built once reads the cache (`load_listed` passes `built_once && role != Analysed`, and the batch load's project is always built once), the editor's projects never are, and the batch run's retains no source. So the key would only have moved the batch run's hash into a digest and added a second `finish_128` per file. `incremental-bench` takes neither hash: its project retains no source and is not built once. What the editor does hash twice is a document it is given: `handle_open_or_change` took the text's digest once for each project that holds the file, to compare it with the one the file was loaded with, and `update_file_as` took it again to keep it; the debounced change, the open documents' sync and the watched files' change took it once per project. `source::Digested` holds a document's bytes with their digest, made in one place, so the two cannot disagree; the server makes one per edit, and `Project::update_digested_as` keeps its digest, where `update_file_as` still takes its own. **Measured** by the digests taken in `did_open_with_a_workspace_loaded_publishes_diagnostics_for_dependents_too` (two files loaded, one opened with new text), under callgrind on the lib test binary of `f676049c` and of this row: 4 → 3, the open's comparison and update now sharing one; each is one XXH3 pass over the document, microseconds, so no time is claimed. **Tests:** an edit given its digest keeps it, as an edit given the bytes keeps theirs; the `project::` and language server tests pass (483); clippy is clean | — |
| LOAD-8.10 | **The walk does not wait for vendor discovery.** The analysed and stub walks start before `vendor.get()`, and the context walks when discovery ends. A walk hands a directory's files over when it is the only walk of the strongest role that reaches the directory. `finish` runs its jobs in `in_place_scope`, and each job keeps its slot in the merge. It undoes the wait that LOAD-1.2 (`7bef998a`) added | fixes R10 | DONE `9513479b`. **The rule:** a walk hands a directory's files over when it is the one walk of their strongest role that reaches the directory, and a file root's walk its file when it is the one walk of the file's strongest role, counting the directory walks that reach its directory and the file roots that name it. A walk hands a file over with the stub mark the listing gives it: from every stub walk that reaches the directory and every stub file root that names the file (`Reach`), since `is_stub` changes the parse and the cache key. So the walks of the roles above context are decided by their own roots (`Walk::starts`) and start at once, and the context walks by every root's (`every_start`), made once the context roots are known. `finish_with` runs the walks in `in_place_scope`: it spawns the stronger walks, runs the caller's closure on the calling thread, which waits in `vendor.get()` and gives the context roots (`ContextRoots`), then spawns the context walks. `walk_listing` gives its stub roots before the closure, and the stale comment at `session.rs:1081` is replaced. Where the row differs: the stub mark is carried, which the row's rule left out, so a file a weaker stub walk marks would have been handed over unmarked. The results are merged stronger walks first, then context walks, not in slots of the old order: the listing does not depend on the order, since each file's role is the strongest of its listings, its stub mark their union, and the files are listed in path order, which the tests check. `Walk::add_file`, `add_resolved_dir` and `add_resolved_file` are for tests only, since a load gives its context roots through `ContextRoots`. **Measured** (against LOAD-8.13's `4a996352`): callgrind off 20,582,303,714 Ir, +69.9 M. The walk's own code is +0.9 M of self cost (`Starts::with_file_walks` 1.0 M). The check parses 1,899 more doc comments (`phpdoc::docblock::parse` 64,229 → 66,128 calls for 191,182 and 191,184 lookups of the shared memo), +54 M in the check's callers (`check_stmt_impl` 5.6 M → 38.8 M, `check_unknown_phpstan_tags_on` 19.0 M → 30.6 M, `check_class_generic_bounds` 14.9 M → 22.1 M): the memo is bounded and empties a full shard, so what it holds when the check starts follows the order the files were parsed in, and the context files are parsed after the analysed ones now. The main thread +29.1 M is `Walk::merge` (29.7 M), which moved to it from a pool thread. The rest, about 15 M, is inside MEAS-3's spread. Perf, warm, two recordings each (load 1.0 to 1.6): the pool's first sample moves from 0.073 and 0.064 s to 0.025 and 0.022 s, so the parse runs beside vendor discovery, and the unused capacity of the first 0.15 s falls from 0.081 and 0.070 s to 0.038 and 0.034 s. Beside the parse the discovery thread takes 49 and 40 samples, where it took 24 and 19 alone, and the parse 1,246 and 1,154, where it took 1,074 and 980, for the same Ir. Off, the first 0.15 s are as busy as before (unused 0.029 and 0.024 s → 0.033 and 0.030 s), since the stub build keeps the pool busy through the wait. Wall (`scripts/abab.py`, 20 pairs each): warm (load 2.5 to 3.7) medians 1.304 and 1.260 s, paired median −65.4 ms (mean −56.3 ms), 5 of 20 pairs slower, Wilcoxon p = 0.017; off (load 3.7 to 6.1) medians 1.699 and 1.686 s, paired median −61.6 ms (mean −33.6 ms), 8 of 20 pairs slower, p = 0.232. The warm pairs give more than the stretch the row shortens can (the pool starts 42 to 48 ms earlier), so that stretch is the row's saving and the pairs show its direction. Peak RSS (p = 0.794 warm, 0.823 off) and CPU time (p = 0.247, 0.108) unchanged. The Piwigo output was the oracle's in every run, off and warm (the 80 timed runs, the 8 recordings and the callgrind run), and the warm runs wrote no cache file: the oracle check the fixes plan asks for right after this row. **The merge** now runs after the parse ends, on the main thread (2 to 4 samples, 4 to 8 ms), where it overlapped the parse's last 10 ms before. `8f93dd07` placed the walks' results inside the scope, but the walks themselves end as the last handed-over files are parsed (their samples run to the parse's end in every version recorded), so the placing came after the parse there too, and 20 pairs each gave +12.3 ms warm (p = 0.502) and +31.8 ms off (p = 0.218); `c8505326` reverted it. **Tests:** `walk_identity.rs`: the analysed walk hands over the files of a context root and of a stub root nested in it, the latter with the stub mark; a file two stub walks list is handed over by neither; a file walk of a stronger role than the directory walk hands its file over with its mark, and two context file walks of one file neither; and context roots given in `finish_with`'s closure, beside the analysed and stub roots given first, give the same hand-overs, listing and roots as every root given before the walk with the stub root last, where a vendor file root in an analysed directory and a context root in the analysed root are handed over by the analysed walk. In tests every walk asserts that no file is handed over twice and that each is handed over as it is listed, and the carried reach against the oracle. The lib suite passes (1200), and the `file_cache`, `cli`, `scaling` and `stub_base_reentry` tests; clippy is clean | — |
| LOAD-8.11 | The plan's R11, the context trees dropped on their workers | fixes R11 | SUPERSEDED: it is LOAD-3.3's load part | — |
| LOAD-8.12 | The plan's R12, a file's number bound without decoding its signatures | fixes R12 | SUPERSEDED: it is CACHE-12 | — |
| LOAD-8.13 | **The autoload files' parse not repeated:** the entries `Resolver` reads from the generated autoload files are stored in the per-file cache under the file's content key, so a warm load does not parse `autoload_static.php` (`parse_generated_file`, 163.0 M) | fixes R13 | DONE `4a996352`. **What is kept:** discovery reads each generated file's paths from its tree into a spelled form (`GeneratedEntries`: for the files and for the directories, the literals' text and the directories they name, `__DIR__`, `$vendorDir` and `$baseDir`). That form depends on the file's bytes alone, so it is kept in the per-file cache (the `GeneratedCache` the session passes, `file_cache::GENERATED`) by the file's bytes and how it is read (`Reading`: the static class, a files array or a directories array). A warm load resolves the paths from that form without parsing the file. The paths are resolved on every run, since resolving reads the file system, and the off run, which keeps nothing, spells and resolves them as it evaluated them. Where the row differs: the key is the file's bytes and its reading, not the path and the parsed files' content key, since the spelled form depends on neither the path nor the file system. The entries are stored as `{hash}.autoload.bin` beside the files' entries, with their checksum and their write. A file that does not parse, or is not of the shape its reading expects, keeps nothing, so it is read again on the next run. **Measured** (against LOAD-8.14's `6e49d4e7`): callgrind off 20,512,402,375 Ir, −9.6 M. The discovery thread 262.2 M → 256.5 M (−5.7 M), mostly reallocations, since each path was built by growing a buffer of its own (`realloc`, `finish_grow` and `reserve` −4.4 M). Warm 12,894,418,032 → 12,713,085,018 (−181.3 M), all of it the discovery thread's, 262.3 M → 78.9 M: `parse_generated_file` 163.0 M → 0; the entries read from the cache 3.1 M (`read_generated`, of which the decode 2.1 M); `Resolver::collect` 51.1 M, where `collect_files` and `collect_dirs`, which evaluated each entry (`append_concat`), were 64.6 M. Perf, warm, two recordings each (load 1.1 to 2.6): the discovery thread 44 and 42 samples (0.026 to 0.111 s, 0.026 to 0.108 s) → 20 and 20 (0.021 to 0.059 s, 0.022 to 0.060 s); under it the parse 10 and 13 → 0, and the entries' resolution 29 and 25 → 17 and 17. The pool's first sample moves from 0.115 and 0.113 s to 0.063 and 0.065 s. Wall (`scripts/abab.py`, 20 pairs, warm, load 2.6 to 4.0): medians 1.351 and 1.347 s, paired median −29.3 ms (mean −9.0 ms), 9 of 20 pairs slower, Wilcoxon p = 0.709, so no change beyond the noise, where the recordings' wait fell by about 50 ms. Peak RSS (p = 0.100) and CPU time (p = 0.296) unchanged. The warm runs wrote no cache file, and the Piwigo output was the oracle's in every run. **Tests:** composer's `vendor_context_discovery.rs` counts a cache's loads and stores: a second discovery finds the kept entries; the kept entries are what is resolved, so emptied ones give an empty context; the paths are resolved each time, so a file removed since is not listed; a file of other bytes is read again; and the separate generated files are kept each by its own bytes and reading. `tests/file_cache.rs`: a warm run reads a generated file's entries back, with the same output and no entry rewritten, and a truncated entry is rewritten whole. The composer suite passes (60), the lib suite (1198) and `file_cache` (5); clippy is clean. `scripts/probe-vendor` builds and runs, with no cache: 63 to 79 ms over 7 runs, 1,744 files and 249 directories | — |
| LOAD-8.14 | **The walk takes discovery's resolved paths.** `VendorContext` keeps resolved and as-written paths apart, and a `Resolved` path enters the walk through an entry point that does not resolve it again, with its listing entry's kind. It completes LOAD-1.1's "each link is resolved once" | fixes R14 | DONE `1f351379`, `6e49d4e7`. **Discovery** gives each directory and file it found as an entry of a listed directory that is not a link as a `Resolved` path, with the kind its listing names (`VendorPath::Resolved`); a path kept as written, or one resolved through a link, whose own kind the listing does not give, stays `VendorPath::Unresolved`. Composer's `ClassLoader.php` is located by the resolver like the autoload entries, where it was the project directory joined with `vendor/composer` after an `is_file` stat. The vendor directory and its parent, spelled as discovery spells them, are canonical already, so the resolver does not list the directories above them to resolve them (`/home/torres` holds 142 entries). **The walk:** `add_resolved_dir` and `add_resolved_file` take a resolved path as a root without resolving it again or asking the system its kind; the as-written paths and the configured roots go through `add` and `add_file` as before. A `Resolved` is made only where a path is known to be its own resolution: by `Resolved::resolve`, and by the composer crate's `Resolved::known`, which asserts it in debug builds. Where the row differs: `VendorContext` keeps one list of files and one of directories, each of `VendorPath`s, where the row split each into resolved and as-written. The first commit compared each directory asked of `canonical_dir` with the vendor directory and its parent as paths, component by component from the end (the discovery thread's `Components::eq` 1.5 M → 3.6 M inclusive); the second, `6e49d4e7`, compares their bytes. **Measured** (against LOAD-8.6's `59bf55bc`): callgrind off at `6e49d4e7` 20,522,048,368 Ir, −11.5 M. The main thread 238.3 M → 230.4 M: `Session::load` 123.8 M → 115.6 M, `Walk::add_file` 6.5 M → 0, `Walk::add` 1.3 M → 0.3 M, `canonicalize` 1.8 M → 0.8 M, `Path::hash` 26.1 M → 24.2 M. The discovery thread 262.0 M → 264.7 M at `1f351379` and 262.2 M at `6e49d4e7` (`Resolver::listed` 28.0 M, 30.5 M, 28.0 M). System calls (`strace -ff`, off, one run each, at `1f351379`): the main thread's `readlink` 7,478 → 3,055 and `statx` 2,231 → 238; all threads' `readlink` 7,493 → 3,070 and `statx` 18,973 → 16,979. Perf, two recordings each at `1f351379` (load 1.5 to 3.8): off, the main thread's samples from the vendor wait's end to the walk's start 9 and 8 (18 and 16 ms) → 0 and 1; warm, the stretch from the discovery thread's end to the pool's first sample 17 and 20 ms → 4 and 3 ms. Wall (`scripts/abab.py`, 20 pairs each, at `1f351379`): warm (load 6.7 to 5.6) medians 1.466 and 1.382 s, paired median −65.6 ms (mean −77.2 ms), 7 of 20 pairs slower, Wilcoxon p = 0.033; off (load 3.6 to 6.7) medians 1.771 and 1.804 s, paired median +30.3 ms (mean +23.9 ms), 11 of 20 pairs slower, p = 0.433. The warm pairs give more than the stretch the row shortens can (about 15 ms), so that stretch is the row's saving, and the pairs show only its direction. Peak RSS (p = 0.332 warm, 0.411 off) and CPU time (p = 0.204, 0.765) unchanged. The warm runs wrote no cache file, and the Piwigo output was the oracle's in every run. Left on the main thread: 3,055 `readlink`s, 2,654 of them from 409 resolutions of the installed packages' directories (LOAD-9). **Tests:** walk_identity's `a_path_that_discovery_resolved_is_walked_as_when_the_walk_resolves_it`: a vendor tree with a linked path-repository package and a linked file gives the same roots, files, roles and hand-overs through the resolved entry points as through `add` and `add_file`. The composer tests check that discovery gives a listed entry and the class loader as resolved, and a path-repository package's directory as written. The composer suite passes (58) and the lib suite (1198); clippy is clean | — |
| LOAD-8.15 | **A batch run does not purge.** `check`, `check-project`, `demo` and `convert-baseline` set mimalloc's `purge_delay` to −1 (`mi_option_set`, option 15) at the start of `main`'s dispatch, through one safe function in `allocator.rs`; `lsp` and `incremental-bench` keep the default | fixes R15 | DONE `d6ca3442`. **The function is `unsafe`,** where the row said safe: `mi_option_set` writes the option with no synchronization and the allocator reads it as it purges, so `phpstan_rs::keep_freed_memory` states that no other thread may be running, and `main` calls it before any thread starts. A `hotpath` build keeps the default, since its guard starts threads before `main`'s body. The binding declares the options API under its `extended` feature, which adds the `cty` crate and changes nothing in the C build. **Measured** (off, against `6705eedd`): mimalloc's exit statistics, one run each: 745 purges (105.5 MiB) → none. Perf, one recording each: stacks with a TLB-shootdown or IPI frame 34 → 3; kernel leaves 1,135 → 1,020 samples (22% → 20%); the page zeroing 298 → 306, the same. Wall (`scripts/abab.py`): 20 pairs (load 2.8 to 6.1), paired median −3.9 ms, p = 0.313; then 40 pairs (load 1.1 to 8.3), −31.2 ms, p = 0.064; the 60 together, median −21.6 ms (mean −63.1 ms), 36 of 60 faster, Wilcoxon p = 0.031 (pooled after the first 20 were inconclusive). Peak RSS +18.9 MiB (the 40 pairs' paired median, p < 0.001; +23.0 MiB in the 20); CPU time unchanged (p = 0.66 over the 60). So after LOAD-3.3 purging costs about 1% of the off run, not MEAS-4's 5.4% (at `aa896697` there were 987 purges, 154.8 MiB), and it lowers the peak RSS by 19 MiB, which MEAS-4's statistics (1.0 GiB in both) could not resolve. The Piwigo output was the oracle's in every run. **Test:** the option's index reads 1000, its default (skipped when the environment sets it); the call itself is not tested, since a test binary runs other threads, and the statistics show its effect. The lib suite passes (1198); clippy is clean; a `hotpath` and a `system-allocator` build check clean; `cargo deny` passes, and `cargo machete` reports only `memchr`, which predates the row | — |
| LOAD-9 | **The installed packages resolved once, beside discovery.** `PackageContext::load` (`crates/composer/src/lib.rs:111-114`) reads the manifest before it spawns discovery, so `ComposerManifest::load` runs on the main thread ahead of the discovery thread, which needs nothing from it. Under it `read_installed` (`manifest.rs:294`, `:299`) canonicalizes each installed package's `vendor/<name>` and then its install path, which Composer writes from `vendor/composer` as `../<name>` for a package installed in place, so the same directory: one `readlink` per component each time. The change: discovery is spawned first; a package's directory is resolved once when its install path names the same directory, and through the vendor directory's listings (`DirListings`, which discovery shares), so a directory that is not a link takes no `readlink` | LOAD-8.14's measurement | READY | — |

### Targets

| task | target now | report's estimate |
|---|---|---|
| LOAD-1 | server memory | the server's ≈420 MB of context trees |
| LOAD-2 | `collect_php_files` 75.3 M serial, `sort_paths` 112.3 M (4 calls, on the main and vendor threads), `strip_prefix` 38.1 M (29,756), `PackageIndex::owner` 69.3 M, `absolute_path` 54.1 M, `FileTable::id_for` 33.7 M, `Components::next` 139.4 M across the run; vendor discovery 321.3 M on its own thread | ≈0.3 G and ≈0.15 s of serial start |
| LOAD-3 | the load's drop is in (LOAD-3.3, `a5a4d85f`: 265.0 M off the main thread, −138.6 M in all, peak RSS −214 MiB); left are the trees `update_file` keeps (LOAD-3.1, 3.3) and the scratch tree (LOAD-3.5) | ≈0.3 G, and 0.58 G off the serial path; peak −≤209 MB |
| LOAD-4 | `insert_numbered_file` 413.9 M serial (its `drop`, 267.8 M, is LOAD-3's; `register_references` 77.2 M over 28,502, which fills the dependents indexes REG-16.4 replaces; `insert_built_decl` 34.8 M); the two path sorts, 20.7 M and 20.6 M | ≈0.25 G |
| LOAD-5 | none in instructions | the analysed trees, ≈173 MB, freed during the check |
| LOAD-8 | at `aa896697` (§1's `regress-item5` runs; MEAS-4 for time): `Path::hash`, 194.3 M of self cost, and about 266 M of component walks, not all on the load path (8.2 to 8.7); 27.3 M of path hashes in the walk's hand-over (8.4) and 27.3 M in the per-file tables (8.5); about 34 M of identity hashing (8.8); a document's digest, taken once per project that holds it and again by each update when the editor opens or changes it (8.9; the cache key's XXH3, 24.1 M warm, is the batch run's only hash of the bytes); the warm run's 120 ms wait for `vendor.get()`, with the pool idle (8.10); `parse_generated_file`, 163.0 M on the discovery thread (8.13); 14 to 16 ms of serial root resolution (8.14); purging, 5.4% of the off run's wall time (8.15) | none: the plan claims a saving only from a measurement |
| LOAD-9 | at `1f351379`: 409 resolutions of Piwigo's 205 installed packages' directories, 2,654 of the main thread's 3,055 `readlink`s (`strace -ff`, off); `ComposerManifest::load` 4 to 7 perf samples (8 to 14 ms) on the main thread in each of four recordings (two off, two warm), before discovery starts, about half of them system calls | — |

### Notes

- **LOAD-1 is mostly semantic.** Its output changes are approved (VAL-2.1) and are read at VAL-1. Its performance part: the server stops keeping context trees and the sources of unopened files. Since decision 3 that part is the engine's (REG-16.4), and LOAD-1.6 is superseded.
  - P8b (PROD-3) added a third use of `built_once`: it decides whether skipped bodies' references are completed. REG-16.4, which took LOAD-1.6's split, must keep that.
  - LOAD-1.3 is also the crate split's C1 follow-up ("one settings assembly").
  - The crate split's Stage F (the `Session` and the language server; not tracked here) covers LOAD-1.3 to 1.5 too. Until its F1, `Project::finish_loading` stays the batch entry point (REG-7.8).
- **The off drill did not change LOAD-2.**
- **LOAD-3's premise was halved by P8b:** skimmed bodies are not trees, so the drop fell from 575.5 M to 267.8 M. The 11,459 context trees were dropped on the main thread, inside `insert_numbered_file`, until LOAD-3.3's load part (`a5a4d85f`) moved the drop to the worker that parsed each.
  - Decision 4's flat tree frees a tree as a few vectors, not node by node. LOAD-3.3 was to wait for it and a new measurement, but `ITEMS_1_TO_3_FIXES_PLAN.md` builds LOAD-3.3's load part first (its R11, kept by its decision 6): MEAS-4 put the drop on the critical path (72 ms of the main thread, off), and dropping each tree on its worker also lowers the peak, which the flat tree changes only in size. Built (`a5a4d85f`), it saved 0.22 s of the off run's 1.98 s, three times the drop's time on the main thread: most of the gain is the kernel's page zeroing, since the parse reuses the freed trees' memory.
- **Decision 3's engine takes over LOAD-4's bookkeeping.**
  - LOAD-4.1, 4.2 and 4.6 are superseded.
  - The engine replaces the dependents indexes, `walk_affected` and the finishing passes' scopes (REG-16.4), so the parts of LOAD-4.5 and 4.7 that work on them go when it lands. Their other parts stand.
- **LOAD-4.3 overlaps REG-8.1:** the crate split's `apply` builds each file's declarations in parallel and recomputes a name's entry by the collision policy, in canonical order. On the engine that entry is a query over the declaring files.
- **LOAD-4.7 and PROD-5.7 are one change,** seen from the index and from the recording.
- **Item 5's validation (2026-10-17, after `4e443227`):** `cargo test -p phpstan-rs --lib` (debug) passes, 1191 tests, and `--test fixtures` passes, 3832. `check-project vendor/monolog` with the default options writes the same stdout bytes at `d870ddcb`, the item's base, and at HEAD, with the file cache off and then cold and warm with it on. Stderr is empty with the cache off. With it on, both binaries print the `union.rs:302` debug assertion for two files, as they did before item 5. The base is a worktree of `d870ddcb`, built in debug, with `vendor/phpstan-src` copied from the main checkout at its pin, `08e3736e`. Run after this note was written, at `aa896697` against `d870ddcb`: callgrind (off and warm) and dhat, in §1. Not run: the Piwigo gate, timing and hyperfine, which need approval. The dump tooling (VAL-1.5 to 1.10) is open, so LOAD-4.3's oracle is the tests above and this identity check.
- **LOAD-5.4 makes LOAD-5.2's frees safe** for any reader left. LOAD-5.3's are the engine's since decision 3.
- **Commits that no row cites (2026-10-16 and 17):** ten code commits from the walk, path and registry work. Each is listed with the row it reads as; that reading is this ledger's and is to be confirmed when the row is worked.
  - `8fe88b90` and `27b978a5`: a listed file's path is hashed once, and the session's listing keeps the walk's index (LOAD-2.7).
  - `ab01515a`, `ee56851c` and `cf5a8ae2`: an existing path resolves without taking its prefix off first, an explicit file resolves its directory once per walk, and the report names each file relative to the root once (LOAD-2.6).
  - `40a83f19`: a file's display path is built only where its anonymous classes or its cache key need it (LOAD-2.3).
  - `8ffd68d9`: a file's digest is computed only when the project retains its source (LOAD-3.4).
  - `6b5bb0b3`: tests that an explicitly listed link is listed under its resolved path (LOAD-1.1).
  - `a5d61611`: the corpus gate runner follows item 1's `prepare` and `finalize` changes (LOAD-1.4 and LOAD-1.5).
  - `4bd62127`: a class's display path is built only for a `PHPStan\*` name. It is in the registry, and no LOAD row fits it.
- **Memory (MEAS-2):** the analysed trees are 151.1 MB at exit, and at the peak of the warm run.
- **LOAD-8 holds `ITEMS_1_TO_3_FIXES_PLAN.md`'s rows,** but for R11 (LOAD-3.3) and R12 (CACHE-12). The plan's decisions were ruled on 2026-10-17: the finishing passes' unused cores are in REG-7's notes (its decision 1), the stub-base wait in STUB's notes (its decision 5), and the name floor in LOAD-2.2 (its decision 7). R8 and R11 to R15 are older costs than items 1 to 3, kept by its decision 6.

- **LOAD-8.7 on each platform** (the cross-platform plan's R16, `9c555663`).
  - The −126.7 M is x86_64 Linux's, by callgrind. macOS reads the same Unix paths with the same code on aarch64 and has not been measured. Windows asked `Path` for every query until R16 gave it a reader of its own (a drive and a root, then names separated by `\`; anything else is left to `Path`). No Windows machine runs the load here, so there is no load-level figure for it and none is claimed.
  - `crates/path-bytes/benches/path_bytes.rs` times each query over 1,000 paths shaped as a load's (absolute, below a project's root, a few directories deep, a PHP file) and, for the last three rows, over paths the bytes leave to `Path` (a `.` component or a doubled separator), which costs what looking first costs. Medians of 100 samples, ns per call, `Path` then the bytes, one run each. Linux ran natively (load average 3.4 to 2.6). Windows ran as `x86_64-pc-windows-gnu` under wine with MinGW (load average 1.2 to 4.0), so its `std::path` is Windows' own but the build is not the MSVC leg's.

    | query | Linux | Windows, under wine |
    |---|---|---|
    | `file_name` | 29.1 → 17.2 | 43.0 → 38.0 |
    | `parent` | 47.9 → 19.6 | 74.8 → 48.3 |
    | `split` (both) | 71.1 → 19.4 | 115.2 → 49.1 |
    | `has_file_name` | 27.8 → 2.9 | 49.7 → 4.1 |
    | `starts_with` | 120.6 → 19.9 | 206.8 → 31.3 |
    | `strip_prefix` | 149.9 → 84.7 | 217.2 → 131.1 |
    | `is_normal_absolute` | 123.5 → 78.7 | 176.5 → 95.4 |
    | components and ancestors | 627.4 → 141.2 | 974.7 → 299.7 |
    | unusual `parent` | 58.3 → 65.2 | 90.7 → 114.9 |
    | unusual `strip_prefix` | 148.0 → 233.6 | 221.9 → 327.0 |
    | unusual components and ancestors | 592.8 → 679.7 | 1,049 → 1,079 |

  - On Windows the name read (`file_name`) is level with `Path`: another run of the wine bench gave 39.1 and 41.4, the other way round, so no saving is claimed for it. The other reads save between a third and about nine tenths of a call; `is_normal_absolute` is one pass over the bytes. A path the bytes leave to `Path` pays for the look first, 12 to 58 % more for `parent` and `strip_prefix`, which a load does not meet: every path it carries is spelled as its components are.

### Records

**LOAD-6** (off P10): SKIP

- *Status, as recorded:* SKIP (not kept: built, measured and reverted, nothing committed. `simdutf8::basic::from_utf8` in the file read, the text then made with `to_owned` because the workspace denies `unsafe` and so cannot turn the checked bytes into a `String` without checking them again: off run 23,215,574,656 Ir against 23,216,039,715 at P7's commit, −0.47 M against the estimated ≈ −22 M, Piwigo output byte-identical. The copy costs what the faster check saves. Wall time not measured; Ir shows no gain to look for)
- *Task:* Check UTF-8 with SIMD (`simdutf8`).
- *Estimated saving:* ≈ −22 M (estimated)
- *Needs:* —
- *Verify:* The check stays; the `LineIndex` `Utf8Chunks` is item 11's.
- *Since:* LOAD-8.1 (`6016499f`, 2026-10-17) checks each file's bytes with `simdutf8::basic::from_utf8` and keeps the checked `&str` borrowed: the parse and the file's length use it in place, and a file copies it only where it keeps its source. So the copy that made this row a SKIP does not arise. It replaced the lossy decode of every file that LOAD-3.4 had added (394 M), for −381.2 M off.

## 4. SYN: the lexer, the parser and the tree

### Tasks

| id | task | from | state | needs |
|---|---|---|---|---|
| SYN-1 | A flat tree: one vector per node kind, nodes as ids, immutable after the parse | 20 G 9; decision 4 | OPEN | — |
| SYN-1.1 | `peeled` becomes a loop, with no guard. The plan builds this first, on its own | 20 G 9.1 | READY | — |
| SYN-1.2 | **The tree.** A file's `Ast` owns its source, its names and one typed vector per node kind. A child is a `u32` id into its kind's vector, and a list is a range of one. The tree is `Send + Sync` without `unsafe`, and an `Arc<Ast>` serves the batch run and the server alike. A tree that is not kept is built in the worker's scratch `Ast`, whose vectors are cleared, not freed. Whether nodes keep spans or token indices is settled first, by a measurement made with SYN-2's design | 20 G 9.2; decision 4 | READY | — |
| SYN-1.3 | **Nodes as ids.** `Expr` and `Stmt` become ids, and the 2,612 `ExprKind::`/`StmtKind::` match sites in 135 files read through the tree. An id lives only in products owned with its tree; a debug build tags each id with its tree, as the backstop | 20 G 9.3; decision 4 | OPEN | SYN-1.2 |
| SYN-1.4 | **Payloads are spans:** doc comments, unescaped literals, inline HTML, names, bodies and scope aliases. A string that needs decoding (heredoc indentation, `\u{..}` escapes) is a span plus a range of a side buffer, and a large variant's payload goes in a side table | 20 G 9.4; decision 4 | OPEN | SYN-1.2 |
| SYN-1.5 | **Nothing in a tree needs a drop of its own:** no `Arc` lives in a node, names live in the file's name table, and lists are ranges. Dropping a tree frees its vectors | 20 G 9.5; decision 4 | OPEN | SYN-1.2 |
| SYN-1.6 | **Reads from any thread.** The finished arena moves into a `FrozenArena` with one `unsafe impl Sync` | 20 G 9.6 | SUPERSEDED by SYN-1.2 (decision 4): a tree of vectors is `Send + Sync` with no `unsafe`, and the workspace denies `unsafe_code` (`Cargo.toml:42`) | — |
| SYN-1.7 | **No growth inside the tree.** Children are gathered in the parser's reusable scratch stacks and appended once, at their exact size, as a range. The same goes for `parse_type_hint`'s `Vec<Vec<TypeHint>>`, interpolation's fragments and parts, and `use` imports | 20 G 9.7; decision 4 | OPEN | SYN-1.2 |
| SYN-1.8 | **Nodes the size of their variant.** A node is as large as the small variants of its kind; a large variant's payload goes in a side table. A statement carries no doc fields, and neither does a parameter: the doc is found in the file's doc sites, by the start offset (DOC-2.9) | 20 G 9.8; decision 4 | OPEN | SYN-1.3 |
| SYN-1.9 | **Every parse yields a tree.** `parse` (`syntax/src/parse.rs:82`) returns a tree and its errors, where it returns a `Result` today. A part that fails to parse is an error node with its span. Q-10's recovery fills the error nodes, so a file that fails to parse keeps what parses; today it is skipped (`project.rs:1141`) | decision 4 | OPEN | SYN-1.2 |
| SYN-1.10 | **Nothing rewrites a tree.** `name_anonymous_classes` (`registry/anonymous_class.rs:45`) gives way to names taken from the class nodes' positions (PROD-5.1). The stub-attribute rewrite gives way to the attributes read as data (STUB-7.3), and with it goes `resolve_names` (`syntax/src/name_resolution.rs:515`), which only it calls. Then `Ast` has no `&mut` access | decision 4 | OPEN | PROD-5.1, STUB-7.3 |
| SYN-2 | **Tokens in flat arrays.** The lexer lexes the whole file into a `TokenStream` (kinds, spans, payloads, docs), and the parser keeps three cursors over it. The buffers are per worker thread, and fragments come from a small stack of scratch streams. `Token`, `Tok`, `Supply`, `next_raw`/`next_tok`, the `VecDeque`, `fill`/`pop`, `raw_keyword` and `LexEnd`'s second pass go. Its arrays are designed with SYN-1.2's tree (decision 4) | 20 G 10 | OPEN; the target grew | SYN-1 (the file's names live in its tree) |
| SYN-3 | Line starts for every file, columns only where read, ignore marks and member spans as offsets | 20 G 11 | OPEN | — |
| SYN-3.1 | **Two types.** `LineStarts` serves every file, built by one pass over newline bitmasks that also says whether the file is ASCII. `LineIndex` adds the column tables, for analysed files only. `FileState` holds `enum Lines { Starts(LineStarts), Full(LineIndex) }` | 20 G 11.1 | READY | — |
| SYN-3.2 | **Blocks.** SSE2's 16-byte blocks, with AVX2's 32-byte blocks behind one `is_x86_feature_detected!` dispatch per process; eight-byte words on other targets | 20 G 11.2 | READY, with SYN-3.1 | — |
| SYN-3.3 | The server indexes context files on demand, but only while the hash of the bytes the project loaded still matches the file | 20 G 11.3 | READY (its need, LOAD-1.4, is in: `3334ebc9`) | — |
| SYN-3.4 | **Nothing in the front end reads lines.** Ignore marks, member removal spans and statement docs record offsets. Anonymous-class names take their line from `LineStarts` on the worker. The texts of ignore marks and ignore comments become offsets and slices | 20 G 11.4 | OPEN | SYN-3.1 |
| SYN-4 | Lexer details | 20 G 12 | OPEN | — |
| SYN-4.1 | **The identifier lookup** is inlined into the identifier arm, its table is SYN-2's scratch, and a probe compares the length first. The watch list becomes a compile-time perfect-hash table | 20 G 12.1 | OPEN | SYN-2 (the name table) |
| SYN-4.2 | **Keywords by word.** The slot's spelling and the identifier are loaded as `u64` pairs and folded by OR with the slot's case mask, so one comparison answers both questions. `lex_word`'s second comparison goes | 20 G 12.2 | READY | — |
| SYN-4.3 | **Blank runs by bitmask,** with SYN-3.2's blocks and dispatch | 20 G 12.3 | OPEN | SYN-3.2 |
| SYN-4.4 | **Operators without a call.** A byte that is a whole operator takes its kind from the byte-class table (`ByteClass::Single`); a longer operator goes to a four-byte matcher built from a generated table | 20 G 12.4 | READY | — |
| SYN-4.5 | `RefKey::Constant` keeps namespace and name apart | 20 G 12.5 | READY | — |
| SYN-4.6 | `UseMap::get` stops re-validating UTF-8 | 20 G 12.6 | READY | — |
| SYN-4.7 | Native type names are matched case-insensitively by SYN-4.2's word comparison, with no lowercase copy | 20 G 12.7 | OPEN | SYN-4.2 |
| SYN-4.8 | **A qualified name written without spaces is one token** (`T_NAME_QUALIFIED`), interned from its source slice; the spaced form of PHP 7 keeps the parser's path. This is NAME-1's round trip for qualified names | 20 G 12.8 | READY | — |
| SYN-4.9 | Numbers with `_` separators are parsed skipping them | 20 G 12.9 | READY | — |
| SYN-4.10 | `strip_leading_backslash` returns a slice | 20 G 12.10 | READY | — |
| SYN-5 | Read `instanceof`'s right side with `parse_new_class_reference` (defect) | off E14 | DONE `6dd06124` | — |
| SYN-6 | **The parser gaps Q-2 probed,** under D6's policy (PROD-4.3) and Q-10's marks: a construct that some version of the range accepts is accepted, and the configured version is applied after the parse. `$s{0}` gets its postfix arm (`syntax/src/parser/postfix.rs:16-240`). `(real)` and `(unset)` stay accepted and are marks for the versions that refuse them. `(void)` as an expression is an error node (SYN-1.9), and as a statement a mark for PHP 8.5 only. A stray control byte from PHP 8.0 on is an error node, and below 8.0 it stays a warning and a skip. `function exit` is an error node in user code and stays accepted in a stub file (phpstorm-stubs' `Core/Core.php:1193`). Evidence: `~/.cache/prs-ledger/q2/php-matrix.txt` | off U2 (Q-2) | OPEN | SYN-1.9, Q-10 |

### Targets

| task | target now | report's estimate |
|---|---|---|
| SYN-1 | `peeled` 61.2 M (2.54 M calls); `drop_glue` of `ExprKind` 486.9 M (1.87 M) and of `StmtKind` 700.7 M (538,692), the two overlapping; 267.8 M of the `Stmt` drop is on the main thread | ≈0.8-1.2 G; ≥53 MB of the analysed trees |
| SYN-2 | the six plumbing functions, 2,868 M self (`next_token` 837.9, `next_tok` 582.3, `fill` 567.0, `next_raw` 490.5, `skip` 231.4, `take_token` 159.7), over 8.44 M tokens, 78% of it in the project's parse; `drop_glue::<Token>` 131 M | ≈1.7 G |
| SYN-3 | `LineIndex::new` 363.4 M (25,753 builds; ≈280 M of it the project's) | ≈0.2 G |
| SYN-4 | `name_and_bits` 631.4 M (2.41 M calls), `lex_ident_span` 223.7 M, `lex_operator` 203.4 M (4.50 M), `Keyword::try_from_bytes` 165.9 M (2.23 M), `parse_qualified_name` 765.9 M incl. (499,347; recursive), `UseMap::get` 111.8 M (442,690), `NativeType::from_name` 29.7 M, `strip_digit_separators` 18.8 M | ≈0.35 G |

### Notes

- **SYN-1:** since P8b, context bodies are no longer trees, so the tree's gain shifts to the analysed files and to declarations.
  - Decision 4 replaced the plan's bump arena with a flat tree of vectors, shared across threads without `unsafe`, so SYN-1.6 went.
  - Its cost is the match sites rewritten to read through the tree, and a bounds check per access.
  - SYN-1.1 stands alone, as the plan has it.
- **SYN-2's target grew:** skimming still pops a `Tok` for every skimmed token (`drop_glue::<Tok>`, 2.16 M calls inside `parse_function_body`).
- **SYN-4.5:** a `RefKey::Constant` splits its text on every hash and comparison today.

### Records

**SYN-5** (off E14): DONE `6dd06124`

- *Status, as recorded:* DONE (6dd06124; unit test over `static::$p`, `self::$p`, `$this->p`, a parenthesized expression and a plain name; the syntax lib's 269 tests pass, clippy clean; the two vendor files are not added as fixtures, the test holds their two forms)
- *Task:* Read `instanceof`'s right side with `parse_new_class_reference`: a plain name stays `InstanceofTarget::Name`, anything else becomes `InstanceofTarget::Expr`.
- *Verify:* A parser test per form; the two files (`AbstractConfigurationProvider.php:41`, `BigInteger.php:177`) as fixtures.

## 5. PROD: a file's products

### Tasks

| id | task | from | state | needs |
|---|---|---|---|---|
| PROD-1 | Bodies of context files made lazy at the summary and the flow | off P8a | DONE `7726dacf` | — |
| PROD-2 | The load's walks over context bodies skipped by file facts | off P8b1 | DONE `11837a04` | — |
| PROD-3 | The parser skims context-file bodies to the matching `}` | off P8b | DONE `791d29cb` | — |
| PROD-4 | The skim's decisions | off D1, D2, D6 | DONE | — |
| PROD-4.1 | D1: every skimmed body is checked, and each file's verdict cached | off D1 | SUPERSEDED by PROD-4.3 (D6), 2026-10-15 | — |
| PROD-4.2 | D2: a skimmed body's text is copied at load, from the bytes the load reads | off D2 | DONE (decided 2026-10-13; built in PROD-1, `7726dacf`) | — |
| PROD-4.3 | D6: a context file that fails to parse keeps what it declares, and a skimmed body is not checked | off D6 | DONE (decided 2026-10-15); what it leaves unbuilt is Q-10 | — |
| PROD-5 | A file's products: the parser fills the declarations, one walk makes the summaries, flows and global facts, one record holds the names the file writes, and the old collector goes | 20 G 13; decision 2 | OPEN; its premise was mostly delivered by PROD-1 to PROD-3 | — |
| PROD-5.1 | **Declarations are the tree's own vectors.** The parser fills the tree's vectors of class-likes and functions in source order, nested ones included. Beside them, as columns, are each one's guard (the condition node and its polarity, or an early-return guard) and the names in effect (its `ImportScope`). `const` statements and `define()` calls get index lists. An anonymous class is a class node there too, named from its position. `registry/guard.rs` still interprets the guards. These read the vectors: declaration building, the duplicate sites, the incremental path, the opt-in rules, `declared_names`, the anonymous-class walks and the stub-index generator (`xtask/src/stub_index.rs`). The declarations walk, the declared-classes collection and `name_anonymous_classes` go; a walk that compares the vectors with the tree lives only in the tests | 20 G 13.1; decision 5 | OPEN | SYN-1.2 |
| PROD-5.2 | `ImportScope`s are published once, at each `namespace` and `use`, and readers start from them | 20 G 13.2 | READY | — |
| PROD-5.3 | **One walk for the rest,** with every builder at each node: `BodySummary`, `InitFlow` and `FileSites` (`GlobalFacts`) | 20 G 13.3 | READY | — |
| PROD-5.4 | **The old collector (`deps::Collector`) goes.** It runs today only where the stub-attribute pass rewrote a tree: a project file that mentions the attributes (`parse_with_ignores`, then `file_references`, `project.rs:2924`), and every stub file (`index_stub_file`, `registry/stubs.rs:1486`). The plan had that pass record the references of the types it adds. Since decision 6 the parse reads the attributes as data (STUB-7.3) and no pass rewrites a tree, so the parser's record is every tree's | 20 G 13.4; decision 6 | OPEN | STUB-7.3 |
| PROD-5.5 | **A file's bodies are one sorted slice** of `(span, summary, flow)`, and a body's lists are ranges into vectors of the file | 20 G 13.5 | READY | — |
| PROD-5.6 | Builders gather in scratch and emit exact-size slices | 20 G 13.6 | READY | — |
| PROD-5.7 | **References are one vector per file, sorted once,** derived from the record of written names (PROD-5.8). A doc site holds the interned `ImportScope`, `recorded_references` reads the sites in their recorded order, and a declaration's name is its `Name` | 20 G 13.7; decision 2 | OPEN; one change with LOAD-4.7 | PROD-5.2, PROD-5.8 |
| PROD-5.8 | **One record of the names a file writes.** It holds every name that resolves without types (class-likes, functions, constants), in code and in PHPDoc. Each entry has its span, from which the source gives its spelling; its resolved key, or its candidates where a fallback to the global name is possible; and where it appears. The parser and the PHPDoc parser record it, extending `Recorder` (`syntax/src/references.rs:184`). The case rule, the `@internal` rule and the other rules that judge a written name read it (REG-14.1). A member name needs types, so its check stays in the checker | decision 2 | READY | — |

### Targets

| task | target now | report's estimate |
|---|---|---|
| PROD-5 | `file_declarations::collect` 1,685.6 M (reflection building is most of it: `from_decl` 1,019.1 M, `from_def`), `recorded_references` 265.6 M, `DocNames` 170.5 M, `add_uses` 148.3 M (289,484) | ≈0.45 G |

### Notes

- **P8a, P8b1 and P8b delivered most of PROD-5's premise.** Context bodies are no longer summarized, flowed or walked at load:
  - `BodySummary::of` costs 83.6 M over 10,863 bodies, where it cost 344 M over 59,926;
  - `InitFlow::of` costs 50.8 M;
  - `GlobalFacts` costs 74 M.
- **PROD-5.1 and REG-11.1 (decision 5).** The parser fills the tree's declaration vectors (PROD-5.1). REG-11.1's indexing of the declarations below the top level reads them, where the crate split's `.10a` had one Visitor walk.
- **PROD-5.4 and the stub build.** The old collector still runs in release builds, for a tree the stub-attribute pass changed: a project file's (`file_references`, `project.rs:2924`) and every stub file's (`index_stub_file`, `registry/stubs.rs:1486`).
  - Since decision 6 that pass goes (STUB-7.3), and since decision 7 no run builds the stub records (STUB-11). So PROD-5.4 deletes what is left, and the plan's way of feeding the pass's references to the parser's record is not built.
- **PROD-5.8 and PROD-5.7 (decision 2).** The record holds what is written where, for the rules. PROD-5.7's sorted keys are the dependency view of the same record.
- **Memory (MEAS-2):** a file's products are up 25 MB, because P8a keeps the skimmed bodies' text.

### Records

**PROD-1** (off P8a): DONE `7726dacf`

- *Status, as recorded:* DONE (7726dacf; measured: off run 22,014,267,117 Ir against 22,362,774,991 at P9's commit, −348.5 M, Piwigo output byte-identical in the optimized build, in a debug build with the oracle on (cache off) and in two warm-cache debug runs. A context file with no anonymous class, no nested declaration and no declaration as a statement of a body (`FileFacts::has_declaration_in_body`, new, set by the parser) keeps each function and method body as its bytes (copied at load from the bytes the load reads, D2) with the index of the name context in effect where it is declared; `FileBodies` makes the summary and flow in a `OnceLock` when a pass first reads the body, by `syntax::parse_function_body` over `Lexer::fragment_bytes(text, base)` with that context, so spans stay absolute. The load no longer walks those bodies for their summaries, flows or nested declarations. The per-file cache stores a skimmed body as its range and context (`NameContext::to_stored`/`from_stored`, a fresh version on decode) and `CachedFile::into_parsed` takes the bytes and copies the text again, whether or not the body was read. Only context files skim (`parse_file_with_names`'s `skim_bodies` is the loader's `cacheable`); analysed files and `ast_of` are unchanged. A debug build reads every skimmed body at load and asserts its summary and flow equal the tree's. Tests: `registry::tests::skimmed_bodies` (6: tree equality across namespaces, `use function`, goto labels, names of the declaring context, cache round trip with re-encode equality, stored the same read or not, a file that is not UTF-8, files with a declaration in a body keep every body read); three mutations (context dropped, base offset 0, the `has_declaration_in_body` condition removed) fail them; syntax 270, lib 1069, fixtures 3830 pass. Not run: workspace tests other than those, clippy. Not measured: peak memory (the kept text is ≈18 MB on Piwigo, D2).)
- *Task:* The first slice of P8: bodies of context files made lazy at the summary and flow, the parse itself still done in full at load.
- *Estimated saving:* −348.5 M measured (below the whole-P8 estimate because the parse, the lexer's token layer and the drops are still paid)
- *Needs:* P9
- *Verify:* Debug build asserts lazy = eager for every skimmed body.

**PROD-2** (off P8b1): DONE `11837a04`

- *Status, as recorded:* DONE (11837a04; measured: off run 21,775,661,606 Ir against 22,014,267,117 at P8a's commit, −238.6 M, Piwigo output byte-identical in the optimized build and in a debug build with the oracles on (cache off). The parser records two more file facts: `has_global_or_include` (a `global` statement or an `include`/`require` expression, also in a `{$...}` fragment, whose flags are now merged into the file's) and uses `has_declaration_in_body` (P8a). `GlobalFacts::of_file` walks no function, method or closure body when the file has no `global` and no include, because such a body adds no site; `file_declarations::collect` enters no body unless the file mentions `define`, has a nested declaration or has a declaration as a statement of a body (`for_each_declaration_with`). A debug build walks the bodies too and asserts the same global facts, and the existing count of declarations of a full walk still guards the declarations. Tests: two fact tests in `parse`, mutation (bodies never walked) fails 2 global-variable tests; syntax 272, lib 1069, fixtures 3830 pass. Not run: workspace tests other than those, clippy.)
- *Task:* The load's walks over context bodies (`GlobalFacts`, declarations) skipped by file facts.
- *Estimated saving:* −238.6 M measured
- *Needs:* P8a
- *Verify:* Debug build walks the bodies too and compares.

**PROD-3** (off P8b): DONE `791d29cb`

- *Status, as recorded:* DONE (791d29cb; measured: off run 20,928,053,254 Ir against 21,775,661,606 at P8b1's commit, −847.6 M, Piwigo output byte-identical in the optimized build and in a debug build with the oracles on (cache off and stub cache off). The parser skims a context file's function bodies that the load keeps as text (`parse_skimming`): the tokens of a body that opens at `{` are counted to the matching `}` and not parsed. A doc comment, `Eof`, inline HTML, a string with a complex part, a class/interface/trait/enum/global/include/require/yield/goto keyword, a named `function`, `define`, a name that can be `phpstan\...` rewinds the tape to a full parse of that body. Lexer errors and unbalanced braces are still found; a syntax error in a skimmed body is not (D6). Skimmed bodies' references are registered by `complete_skipped_references`, which parses every skimmed body once, so it runs only where a lookup needs it: incremental updates and removals, not a project built once. The first measurement of this commit, with the completion running in the batch, was 23,567,209,391 Ir (+1.79 G): the completion parsed 46,413 bodies for 2.36 G Ir. In a project built once the only `dependents` lookup is the one finishing pass, and every name its files declare is among those that changed, so what a skipped body refers to adds no declared name to its scopes; a debug build completes the references anyway and asserts both scopes equal (it ran once on Piwigo, with and without the stub cache). Tests: 6 parse tests (skipped bodies parse to the whole-parse content, each trigger, member-operator keywords and closures, PHPStan contexts, syntax error not found but lexer and brace errors found, references union), 3 project tests, mutation (`define` never triggers) fails the oracle at `file_cache.rs`; syntax 278, lib 1072, fixtures 3830 pass. Not run: workspace tests other than those, clippy; peak memory not measured.)
- *Task:* The rest of P8: the parser skims context-file bodies to the matching `}` and rewinds to a full parse on a trigger.
- *Estimated saving:* −847.6 M measured
- *Needs:* P8a; D6
- *Verify:* A debug build parses every skimmed body eagerly and asserts equal summaries, flows, declarations, global facts and references (`file_cache::assert_same_but_bodies`); a debug build of a built-once project asserts the scopes equal with the references completed.

**PROD-4.1 to PROD-4.3** (off D1, D2, D6)

- *D6, status as recorded:* DONE (decided 2026-10-15, answered in the session: adopt D6, skim unchecked)
- *D6, decision:* A context file that fails to parse keeps what it declares: what parses is recovered, what failed is unknown. A skimmed body is not checked: a syntax error in one is not found at load, and a body that does not parse when read is unknown. There is no check pass and no verdict cache, so D1's check requirement does not apply to P8b. Validity following the configured PHP version and one rule for the batch run and the language server stay as written in §10.8.
- *D6, gates:* P8, parser gaps
- *D1, D2 and D5, status as recorded:* DONE (decided 2026-10-13, report)
- *D1, D2 and D5, decision:* D1: every skimmed body is checked and each file's verdict cached. D2: a skimmed body's text is copied at load from the bytes the load reads. D5: each cache checks once, at its first store, whether it can write.
- *D1, D2 and D5, gates:* P8, P2, E13
- D1 was replaced by D6 on 2026-10-15. D5 is CACHE-2.

## 6. DOC: PHPDoc

### Tasks

| id | task | from | state | needs |
|---|---|---|---|---|
| DOC-1 | Tokenize a PHPDoc tag's description only when it is read | off P7 | DONE `a4035361` | — |
| DOC-2 | PHPDoc finished: tag effects, eager lowering, two memos, nodes in one vector, doc blocks as a per-file product | 20 G 14; decision 2 | OPEN | — |
| DOC-2.1 | **Lowering replaces the lazy round trip.** `DocType`'s type comes from `phpdoc::lower(&TypeNode)`, eagerly in `docblock::parse`. The text grammar's other callers move to `phpdoc::parse_type`, and the text grammar is deleted. The statement `@var` memo goes in | 20 G 14.1; 25 G P7 | READY, deferred 2026-10-16 (see WORK_STACK level 7) | — |
| DOC-2.2 | **Resolution memoized per file,** keyed by the type node and an interned resolution context (PROD-5.2's `ImportScope`, the template names, the type aliases, the class `self` names). The memo holds the whole resolved type | 20 G 14.2 | OPEN | PROD-5.2 |
| DOC-2.3 | **Tag effects.** `parse_tag -> Arc<TagEffect>` and `apply(&mut DocBlock, &TagEffect)`, with no scratch block and no `merge_from` | 20 G 14.3; 25 G P11 step 1 | READY | — |
| DOC-2.4 | The tag memo is keyed in place, by the borrowed pair `(tag, rest)` in a `hashbrown::HashTable`, not by a `format!` | 20 G 14.4 | READY | — |
| DOC-2.5 | **Tag tokens borrow.** A `PhpDocToken`'s text is a `&'t str` into the tag, and `TypeNode`'s identifiers are `Name`s. A docblock's tag lines are slices of the doc text. Identifiers and tag values are matched case-insensitively, without copies | 20 G 14.5 | OPEN. DOC-1 rewrote the tag parser to pull tokens; the borrowed token text is not built | NAME-1 |
| DOC-2.6 | **One product per doc text, one per tag line.** The docblock memo holds the grammar node, the block, its referenced names as `Name`s and its `@var` winners; `DocNames` and its memo go. The tag memo holds `TagEffect`s. The written-type memo goes with the text grammar | 20 G 14.6 | OPEN; overlaps REG-4.10 | DOC-2.1, DOC-2.3 |
| DOC-2.7 | **A docblock's nodes are one vector.** A `TypeNode`'s children are indices into it. The grammar node, the tag effects and the per-tag maps are vectors of the docblock too | 20 G 14.7 | READY | — |
| DOC-2.8 | **Keys by fingerprint, bounds by weight.** A memo is keyed by a 128-bit fingerprint, bounded by a byte budget that counts keys and values, and evicts by CLOCK | 20 G 14.8 | READY | — |
| DOC-2.9 | **Parsed doc blocks are a per-file product,** found by the comment's offset. Each doc comment is parsed once, the declaration builder and the checker share the parse, and it is dropped with the file's other products. The ≈25 checker sites that parse a doc comment again read the analysed file's doc blocks, where REG-7.7 had the declaration keep them | decision 2 | OPEN; overlaps DOC-2.6 | REG-4.10 |

### Targets

| task | target now | report's estimate |
|---|---|---|
| DOC-2 | `parse_shared` 1,394.7 M (195,201 lookups, stub files included), `apply_tag_recording_errors` 702.0 M, `merge_from` 101.0 M, `parse_written_type` 151.5 M, `to_source_string` 96.8 M, `resolve_type_node` 94.3 M (34,821), `DocNames` 170.5 M | ≈0.35 G; ≈50 MB |

### Notes

- **DOC-1 (off P7) built the plan's lazy tag description,** and with it the `[optional]` prefix test that read from the first tokens. That covers the allocation item 14 lists for `apply_tag`'s copy of every description. The rest of item 14 is DOC-2.
- **The earlier ledger said here that "P11 was measured and not kept".** That was the off report's P11 (messages, CHECK-4). The 25 G report's P11 step 1 (`TagEffect`, DOC-2.3) has never been built or measured.
- **DOC-2.6 overlaps REG-4.10 and DOC-2.9.**
  - In the crate split, the builder parses each doc comment once (REG-4.10), and the declaration kept its parsed doc blocks for the checker sites that parse a doc comment again.
  - Since decision 2 the parsed blocks are the file's product (DOC-2.9), which those sites read; no declaration keeps them.
  - DOC-2.6's memo is keyed by the doc text, across files. DOC-2.9's product is owned by its file.
- **Memory (MEAS-2):** PHPDoc holds 94.9 MB at exit in the off run and 38.3 MB in the warm run, against item 14's ≈50 MB claim. P7 saves allocations, not retained bytes.

### Records

**DOC-1** (off P7): DONE `a4035361`

- *Status, as recorded:* DONE (a4035361; measured: off run 23,216,039,715 Ir against 23,619,596,541 at E10's commit, −403.6 M against the ceiling of ≈ −0.37 G, Piwigo output byte-identical; the tag parser pulls tokens from the lexer (`Lexer::next_token`) into a buffer and fills it only as far as a lookahead or rewind asks, and a tag's description is `Description`, the source from the first token the parser did not take, which `text()` lexes again and joins with single spaces and `starts_with_optional_marker()` answers from the first tokens, on the same rule as the old test (the joined text without spaces starts with `[optional]`, so `[opt ional]` still matches); a debug build compares every description's text with the tokens the parser goes on to read, which fails with the offset moved to the token's end (23 tests); five new tests (text for `//`, string escapes, floats, overflowing integers, `<b>`; the offset after rewinds; the marker against the old rule on 14 inputs; a serde round trip; no allocation without a description); the warm figure was not measured; the workspace tests, clippy of the other crates and the fixtures not run)
- *Task:* Tokenize a PHPDoc tag's description only when it is read: the parser pulls tokens, records the description's offset, and re-lexes on request; `[optional]` becomes a three-token prefix test.
- *Estimated saving:* ≈ −0.37 G off, ≈ −0.08 G warm (ceilings); ≈ −0.04 G after P6 and item 14
- *Needs:* —
- *Verify:* Re-lex, never slice (`//`, string escapes, floats, overflowing integers, single-space join); every phpdoc test; the memo keys unchanged.

## 7. NAME: names and interned values

### Tasks

| id | task | from | state | needs |
|---|---|---|---|---|
| NAME-1 | Names held as names | 20 G 15 | OPEN | — |
| NAME-1.1 | The checker's method memo (`checker.rs:271`, keyed by `String`) and `class_memo` are deleted | 20 G 15; 25 G C3 step 7 | READY | — |
| NAME-1.2 | **Every `String`-keyed map, and every value holding a PHP identifier, gets PHP's identity** (`FoldedName`, `Name`, `ConstName`, `ArrayKey`), in the profile's order of cost: the registry's `&str` entry points, `TemplateBindings`, the scope's guarded sets and container fields (keyed as PHP's array keys), `closure_scope`'s frontier sets, the identifiers copied as text, the round trips through the interner, then by module | 20 G 15; 25 G C3 step 6, 25 G C4 step 5 | READY; the container-field keys are a delta, approved (VAL-2.7) | — |
| NAME-1.3 | `TableKey for str` and `FoldKey for str` are deleted from the registry; text from outside is interned once, where it enters | 20 G 15 | OPEN | NAME-1.2 |
| NAME-1.4 | A short name is a slice of the name, at its last segment's offset, which NAME-2.1 stores in the name's header | 20 G 15 | OPEN | NAME-2.1 |
| NAME-1.5 | **Name resolution hashes the parts as one text** and compares a candidate part by part (`qualify`, `resolve_as`), so only a new name allocates | 20 G 15 | READY | — |
| NAME-1.6 | A call's callee is lowercased once: its resolved `Name` already holds its folded form | 20 G 15 | READY | — |
| NAME-2 | Interned values: one allocation each, tables without per-entry allocations | 20 G 16 | OPEN | — |
| NAME-2.1 | **A name is one allocation:** a header (text hash, folded hash, length, the last segment's offset, where the lowercase form starts), then the text and the lowercase form. It is a thin dynamically sized value, as `triomphe::ThinArc` is. `Name` stays one pointer wide, and its equality stays identity | 20 G 16 | READY | — |
| NAME-2.2 | The weak tables hold `(hash, Weak)` pairs in a `hashbrown::HashTable`, so the type interner loses its per-hash `Vec`s | 20 G 16 | READY | — |
| NAME-2.3 | Shape string keys are interned in a weak table of byte strings | 20 G 16 | READY | — |
| NAME-2.4 | The type memos are cleared with the relation memo's generation | 20 G 16 | OPEN | TYPE-4.1 |

### Targets

| task | target now | report's estimate |
|---|---|---|
| NAME-1 | `<str>::fold_hash` 275.7 M and `is_folded_eq` 358.2 M (4.67 M and 3.81 M calls); `Interner::name` 260.8 M (1.01 M) and `name_hashed` 354.8 M (1.44 M); `resolve_as` 202.2 M; `substitute_class_templates` 193.8 M; `class_template_bindings_rec` 102.1 M | ≈0.6 G |
| NAME-2 | `types::intern::insert` 439.5 M (1.33 M calls), 151.3 M self; `Interner::str` 53.6 M | ≈0.1 G; ≈15 MB |

### Notes

- **The purity keys the plan assigned to item 4 are gone,** with P9 (REG-1).
- **NAME-1 and the crate split:**
  - NAME-1.3 removes the `&str` lookups that REG-4.1 made allocation-free.
  - NAME-1.2's registry entry points meet REG-4.2's exact-spelling accessors and folded method keys.
  - REG-8.6 routes about 100 `syntax::intern::intern` calls through the project's interner.
- **SYN-4.8 is NAME-1's round trip** for qualified names.
- **TYPE-3 (off E7) keeps a class's plain object type,** which item 16 overlaps.
- **Which of the two needs which.** The earlier ledger had item 16 needing item 15. It is the other way round: NAME-1.4 (item 15's short names) needs NAME-2.1 (item 16's header).
- **Memory (MEAS-2):** the interned values hold 32.0-32.5 MB, against item 16's ≈15 MB claim.

## 8. TYPE: type nodes and the type relation

### Tasks

| id | task | from | state | needs |
|---|---|---|---|---|
| TYPE-1 | A kind code in the type node's free flag bits | off E5 | DONE `5ec70c1e` | — |
| TYPE-2 | One node against itself is answered first in `is_super_type_of` | off E6 | DONE `f2f65cbb` | — |
| TYPE-3 | Plain object types kept with their class, two statics, a table of small integers | off E7 | DONE `60e85393` | — |
| TYPE-4 | One relation memo, keyed by the engine's revision, replaces `structural_memo` and `KEEP_DECISIONS` | 20 G 7; decision 3 | OPEN | REG-16.3 |
| TYPE-4.1 | **The memo's generation is the engine's revision.** No input changes within a revision, so an answer found in it holds until the next. Any change of an input makes a new revision and clears the memo: an edit, the configuration, or the PHPStan the project installs (STUB-13). The plan's `ClassHierarchy::generation() -> Option<u64>`, with its `None` while declarations are inserted, is not needed | 20 G 7; decision 3 | OPEN | REG-16.3 |
| TYPE-4.2 | **One memo, keyed `(sup node, sub node)` under the revision, storing only final answers.** An answer found while the engine is resolving a cycle is provisional and is not stored. The relation's code is first read for any input beyond the key. The plan's rule for constants on a cycle is REG-7.6's since decision 1 | 20 G 7; decision 1 | OPEN | TYPE-4.1 |
| TYPE-4.3 | **Bounded within a generation:** at most 2^17 entries per thread, in two segments; the older is dropped when the current one fills | 20 G 7 | OPEN | TYPE-4.2 |
| TYPE-4.4 | **A runtime switch,** `PHPSTAN_RS_NO_RELATION_MEMO`, turns the memo off in any build, for the oracle | 20 G 7 | OPEN | TYPE-4.2 |
| TYPE-5 | **`iterable<K, V>` is `array<K, V>\|Traversable<K, V>`:** its array member takes `K` as `array<K, V>` does, so a key no array can have (`float`, an object) leaves only the empty array. Today the array member's key is `array-key` whatever `K` (`types/src/parse.rs:1348`): an `iterable<int, V>` accepts an `array<string, V>`, and `foreach` over `iterable<string, int>` gives `int\|string` keys, which the fixture `foreach_over_a_generic_iterable_or_traversable_infers_its_key_and_value.php` pins and which becomes `string`: an array that has string keys has no decimal-int string among them, since PHP stores such a key as an int, and a `Traversable` keeps its keys as they are | decision 8 | READY | — |

### Targets

| task | target now | report's estimate |
|---|---|---|
| TYPE-4 | `is_super_type_of` 422.5 M (1,447,876 calls), 109.0 M self | up to ≈0.3 G |

### Notes

- **TYPE-1 and TYPE-2 put one-word shortcuts and a pointer test in front of `is_super_type_of`.**
- **TYPE-4's needs.** The earlier ledger had TYPE-4 needing item 4 (its on-demand cycle guards) and item 6 (its cycle rule). In the crate split's design both were REG-7's: REG-7.9's reentrancy guard, and REG-7.6's constant cycles.
  - Since decision 3 the generation and the cycle guards are the engine's (REG-16.3).
  - Since decision 1 the constant rule is REG-7.6's alone; TYPE-4.2 keeps the memo.
- **Memory:** `KEEP_DECISIONS` holds 1.3 MB at exit.

### Records

**TYPE-1** (off E5): DONE `5ec70c1e`

- *Status, as recorded:* DONE (5ec70c1e; measured: off run 23,725,134,207 Ir against 23,785,065,626 at E3's commit, −59.9 M against the estimated ≈ −30 M, Piwigo output byte-identical; the kind (a family code 1-8, plain `mixed`, `never`) and the scalar-value bit are set in `reference_flags` and masked out of what a node passes up to its parent; `scalar_family` and `is_scalar_value` read them, so `union.rs` and the other readers got cheaper too; a unit test checks the kind against the form on the samples, NAN, class-kind strings and nested forms, and the types crate's shortcut tests pass; clippy clean on the types crate, the workspace tests and fixtures not run)
- *Task:* A kind code in the type node's free flag bits.
- *Estimated saving:* ≈ −30 M
- *Needs:* —

**TYPE-2** (off E6): DONE `f2f65cbb`

- *Status, as recorded:* DONE (f2f65cbb; measured: off run 23,701,951,690 Ir against 23,725,134,207 at E5's commit, −23.2 M against the estimated ≈ −10 M net, Piwigo output byte-identical; the test is `Arc::ptr_eq` on the two nodes, first in `is_super_type_of`, and answers Yes; a subtracted `mixed~S` against itself, which the form-only answer gave Maybe, is now Yes, so the two existing tests that compared with the form-only answer expect Yes for equal types, and `a_type_is_a_super_type_of_itself` is new; the types crate's 275 lib tests pass; the workspace tests, clippy and fixtures not run)
- *Task:* One node against itself answered first in `is_super_type_of`.
- *Estimated saving:* ≈ −10 M net
- *Needs:* —

**TYPE-3** (off E7): DONE `60e85393`

- *Status, as recorded:* DONE (60e85393; measured: off run 23,672,079,898 Ir against 23,701,951,690 at E6's commit, −29.9 M against the estimated ≈ −55 M, Piwigo output byte-identical; item 4's index is not built, so the class's type is kept in a `OnceLock` inside `ClassReflection` (left out of its comparison, copy and stored form, and checked against the declared name on each read); a class the registry does not know is built each time as before; `Throwable` is a static and the fixed class lists of `instance_of_any` come from a per-thread table keyed by the literal, which needed the callers' class names to be `'static`; `int_literal` answers −1..=255 from a table; unit tests for the table and for the class's kept type, the types crate's integer-literal test and the three registry tests pass; the workspace tests, clippy and fixtures not run)
- *Task:* Plain object types kept with their class, two statics, a table of small integers.
- *Estimated saving:* ≈ −55 M
- *Needs:* item 4's slot; item 16 overlaps

## 9. REG: the registry

The crate split's Stage C is worked on this branch from C4 on. Decision 3 puts its memo and snapshots (REG-7, REG-8) on one incremental query engine (REG-16).

- Each commit of a sub-task is made with check, clippy and the touched module's tests; the fixture suite and the corpus run at the stage's checkpoints.
- A sub-task whose output changes says so on its row.
- The keys, engine, memo and snapshot commits (REG-4.2, REG-16, REG-7, REG-8) each run the full gate tier, because they change every lookup's hashing, the resolution path, and the LSP's per-edit invalidation.

### Tasks

| id | task | from | state | needs |
|---|---|---|---|---|
| REG-1 | Purity and property writes on demand | off P9 | DONE `c0dd2788` | — |
| REG-2 | The gates that G's extraction relies on, each with its first inventory: gate 2's `impl` placement and each module's path in its crate; gate 3's names qualified by package and target | split C4 gates | READY. `xtask modgraph --inventory` lists the crossing edges; the `impl` and name inventories are not made | — |
| REG-3 | `xtask gatetime`: `--record` and `--since <hash>`, medians of three runs on prebuilt binaries | split C4 gatetime | DONE `58cfc5d2` | — |
| REG-4 | Keys and data model | split C4 data model | PART | — |
| REG-4.1 | Class and function tables keyed by the folded name; a lookup that allocates nothing; `FunctionTable` on the class table's type | split C4 data model | DONE (already on this branch) | — |
| REG-4.2 | The exact-spelling accessors, then deleting `canonicalize_class_name_case` and the declared-names side map; method keys fold (`method_access` included). Two class-like declarations, or two methods of one class, whose names differ only in case become one name | split C4 data model | READY; a delta, approved in the plan, fixtured | — |
| REG-4.3 | A class's templates as one list of parameters, each with bound, variance, default and scope, where five parallel maps and sets were rebuilt into parameters in four places that disagreed | split C4 data model | DONE `5401afc9` | — |
| REG-4.4 | Per-member flags: a property's finality on the property | split C4 data model | DONE `8d35c99d` | — |
| REG-4.5 | Per-member flags for the method sets (`abstract_methods`, `final_methods`, `static_methods` and the `_by_phpdoc` forms), as flag bits on the member's entry | split C4 data model; 20 G 4 | READY. The plan names only the property case. Item 4 also made `redeclared_methods` a flag; since decision 3 it goes instead (REG-8.5) | — |
| REG-4.6 | A property's native type as an `Option` | split C4 data model | DONE `7a61603e` | — |
| REG-4.7 | A parameter's and a return's native type as an `Option`, with one encoding for no native type | split C4 data model | READY | — |
| REG-4.8 | The explicit-`mixed` flags (`explicit_mixed_hint`, `return_explicit_mixed_hint`) read from the types | split C4 data model | DONE `e7b2521f`, corrected `0f32829b` | — |
| REG-4.9 | Accessors return `Option<&T>`/`&[T]` | split C4 data model | READY | — |
| REG-4.10 | The builder parses each doc comment once; today a method's is parsed per use | split C4 data model | READY; overlaps DOC-2.6 | — |
| REG-4.11 | **Provenance as the only location record.** No declaration merges into another (decision 11), so each keeps its own provenance and a location is always one declaration's. `is_stub` (63 sites) becomes a question about a declaration's provenance or a file's role; `function_decl_sites` (11) goes with REG-8.5's checks of each body against its own declaration; `Origin::Unknown` (29, 22 of them in tests) goes with `Registry::build` (REG-10). The comments citing `project/decl_locations.rs` are corrected | split C4 data model; decision 11 | OPEN; the comments can be corrected now | REG-8.1 (no merge), REG-8.5, REG-10 |
| REG-4.12 | **Query accessors** replace the storage-returning ones and the `pub` maps: about 280 sites outside `registry/`, 23 of them in tests. Item 4's first step is the same change; it counted 149 sites | split C4 data model; 20 G 4 | READY | — |
| REG-4.13 | The builders read the AST's parse-time `QualifiedName::resolved` instead of resolving again through a `NameContext`; the anonymous-class builder is the exception | split C4 data model | READY | — |
| REG-4.14 | **No result depends on the class map's iteration order:** `classes_using_trait` is sorted, and `doctrine_entity_for_table` picks by name order | split C4 data model | READY; a delta, approved (decision 14), fixtured | — |
| REG-4.15 | **A declaration is built straight into its final form:** the ordered member list is filled once, with one folded index, where `from_def` builds three hash tables per class; a member's span is a field of its entry | 20 G 4 (its REG-13.9); decision 3 | OPEN | REG-4.2 (folded method keys), REG-4.12 (no reader holds the maps) |
| REG-5 | **Written results leave the signatures.** Purity results go to a registry table keyed by function and by `(class, method)`. `names_with_assert_tags` is seeded from the assertion entries, and checked by folded name | split C4 written results | PART: the purity part was done by REG-1; what is left is a delta, approved (decision 14): a call spelled in another case than its method reaches the assert lookup | — |
| REG-6 | The checker test harness builds like a project: it merges the stubs before resolving | split C4 harness | READY; a test-expectation delta, approved (decision 14) | — |
| REG-7 | Per-class resolution replacing the passes, in five levels, each a query of the engine | split C4 memo; decision 3 | OPEN | REG-16.3 |
| REG-7.1 | **Ancestors level:** the `extends`/`implements` closure in resolution order, the `use` closure in composition order, the `@mixin` targets, the unresolved edges, and the constant declarations composed with the traits'. A cycle through `extends`, `implements` or `use` is detected here and broken: every edge between two classes of the same cycle is dropped, whichever class resolves first | split C4 memo | OPEN | REG-4.2 |
| REG-7.2 | **Own level:** own members composed with the `use` closure's declared members, `@method` tags, every type resolved in the class's context (imported aliases, `value-of` and class-constant types, constant-named types), and the interim assertion step. It includes item 6's step 1, deferred types on demand: `class_has_deferred_type`, `function_has_deferred_type` and the eager pass go, and the stub cache asserts that no stub record holds a deferred type. The scan's decodes go only with the passes that read every method in scope (`flatten_phpdoc_inheritance`, `resolve_constant_named_types`, `drop_doc_members_excluded_by_native`), since each would decode them otherwise | split C4 memo; 20 G 6.1 | OPEN | REG-7.1 |
| REG-7.3 | **Inherited level:** PHPDoc inheritance and property hooks, from the ancestors' inherited level | split C4 memo | OPEN | REG-7.2 |
| REG-7.4 | **Members level:** each `@mixin` target's public methods and properties, then the member owners | split C4 memo | OPEN | REG-7.3 |
| REG-7.5 | **Final level:** each declared type is its native and PHPDoc types combined by decision 8's rule, the relation reading the class hierarchy. A PHPDoc member that shares no value with the native type is dropped; one that shares values with a native kind is intersected with it; a native kind that no member shares values with is kept; a member that resolves later carries the native type until it does. The PHPDoc is read with its aliases resolved. The crate split's text took PHPStan's `TypehintHelper::decideType` whole: decision 8 keeps its added-back union members, its variadic reading and its `iterable` widening (generalized), and replaces the rest. The plan's Appendix H causes for C4 stay here | split C4 memo; decision 8 | OPEN | REG-7.4 |
| REG-7.6 | **Class constants through the `ConstantResolver`** (the crate split's C2 follow-up lands first), reading the ancestor level's declarations with the imported aliases substituted. An enum's case values are constants to the resolver too. A constant's result is its value type plus the throws its evaluation can raise, each definite or possible. A cycle over value edges, or arithmetic that fails on literal operands, is definite: every read is `never`. An unknown constant or class is possible: the value stays unknown. Each cause is reported at the constant that has it. A PHPDoc edge is not evaluated: a constant whose PHPDoc reaches a cycle keeps its type without that PHPDoc, which is reported as unresolvable. The engine finds the cycles, over value edges only; no `*` pattern is expanded. The plan's `mixed` for a cycle is withdrawn | split C4 memo; decision 1 | OPEN; a delta, approved (VAL-2.5) | REG-7.1, REG-18.2 (the resolver in `reflection`), REG-18.3 (the evaluator) |
| REG-7.7 | **Build scratch leaves `ClassReflection`** for the merged declaration kept beside each level: `method_tags`, `trait_aliases`, `resolved_trait_conflicts` and `type_imports`. The rules reading that scratch query the declaration. Since decisions 2 and 3, `redeclared_methods` goes (REG-8.5), the `*case_refs` leave for the record of written names (REG-14.1), and the parsed doc blocks are the file's product (DOC-2.9), not the declaration's | split C4 memo; decisions 2, 3 | OPEN | REG-7.1 |
| REG-7.8 | **`Project::finish_loading` keeps only the whole-program passes,** and stays the batch entry point (69 call sites) until the crate split's F1. About 100 comments naming a deleted pass are rewritten. `index_and_flatten` is deleted. `Registry::build` indexes and runs the whole-program passes until its callers move onto `apply` (REG-10). The stub pass resolves each stub file's classes within that file, until STUB-7.6 | split C4 memo | OPEN | REG-7.1 to REG-7.5 |
| REG-7.9 | **No query waits on itself.** A query that reaches itself is a cycle, which the engine finds. A hierarchy cycle, a constant cycle and a recursive body's inference are resolved by their rules (REG-7.1, REG-7.6, REG-17.1); any other is a bug, reported as one. A panic inside a query leaves it retryable, so `check-project`'s per-file `catch_unwind` resumes cleanly. A panic-injection test and a parallel-resolution test run over cyclic fixtures | split C4 memo; decision 3 | OPEN | REG-7.1 |
| REG-7.10 | **A whole-registry iteration decides before resolving a class,** from the declarations, provenance and the ancestor level, until STUB-7.8 replaces it. This covers the two `native_type_hints.rs` scans, the concrete-implementor scan in `checker.rs`, the whole-program passes (dead abstract classes, dead enum cases, dead members, namespace cycles), `class_shape`, `classes_using_trait`, `doctrine_entity_for_table`, `repository_entity`, `Project::run_reflection_rules`, LSP workspace symbols and completion. `demo` resolves everything it prints | split C4 memo | OPEN | REG-7.1 |
| REG-7.11 | **Trait and mixin members no longer outlive a change to their source.** `removing_a_files_trait_does_not_retroactively_unbake_its_members_from_existing_users` inverts, renamed through `gates/test-renames`, and `remove_files`' known-limitation comment goes | split C4 memo | OPEN; it lands with the snapshots | REG-8 |
| REG-7.12 | **A function resolves lazily the same way:** value types, constant-named types, decision 8's combination of native and PHPDoc types, the interim assertion step. `finalize_standalone_sig` (`registry/resolve.rs:2119`) is that resolver | split C4 memo; decision 8 | OPEN | REG-7.2 |
| REG-8 | `ReflectionBuilder::apply` snapshots, as the engine's revisions | split C4 snapshots; decision 3 | OPEN | REG-7 |
| REG-8.1 | **`FileDelta`** carries a file's role and its new declarations, or its removal; the initial load is one apply, built per file in parallel. On the engine a delta is an input change, and a revision what follows it. A name's entry is a query over its declaring files: the declarations PHP may bind the name to (decision 11), each its own entry and never a merge, so no order picks a winner and no file wins by arriving last. Declarations are shared between revisions by `Arc`, and what a change does not reach keeps its value | split C4 snapshots; decisions 3, 11 | OPEN; a delta: no declaration's members merge into another's, and the server and the batch run bind a name alike (VAL-2.10) | — |
| REG-8.2 | **Read sets.** Each memo records every folded class and constant name it looked up, absent ones included. A delta invalidates the transitive readers; a change to any other reflection input invalidates everything. Purity and global-variable inference run again per snapshot, until the crate split's C9 | split C4 snapshots | SUPERSEDED by REG-16.3 (decision 3): the read sets are the dependencies the engine records, absent names included | — |
| REG-8.3 | **The per-file declaration sets** answer what `DuplicateClassSites` and `DuplicateFunctionSites` index; those two, with their own declaration walks, are deleted for a query that groups by exact spelling. `FileState.functions`/`classes` become the file's set. Its layout is item 4's `FileDecls`: one vector per kind, a class's members and a function's parameters as ranges, and one folded index per file. A stub file's record decodes into the same layout | split C4 snapshots; 20 G 4 (its REG-13.2); decision 3 | OPEN | REG-8.1 |
| REG-8.4 | **`Registry` splits** into `ReflectionBuilder` (the per-file declaration sets, `apply`) and `ReflectionProvider` (the snapshot, with the query methods); its 22 fields go. The provider is made only with the builtin knowledge, so a registry without it can't be represented (STUB-1's S4, a runtime check until then): `Registry.corpus: Option<Arc<Corpus>>` and the `expect`s of `corpus()` and `shared_corpus()` (`registry.rs:856`, `:865`) go, and so do `sync_phpstan_stubs`' branch for a project with no base (`project.rs:1186`) and `stubs_of`' panic (`:1211`). A project still loading holds only the builder | split C4 snapshots; off E1 (S4) | OPEN | REG-8.1 to REG-8.3, REG-18.6 (`phpstan_class_files` gone) |
| REG-8.5 | **The checker stops rebuilding reflection from the AST.** The rebuild-and-compare sites (`ClassReflection::from_decl` and `from_enum_decl` in `oop.rs`, `from_decl` for a trait in `function_decl.rs`) and a duplicate function's rebuilt signature (`own_declaration_sig`, `checker/function_decl.rs:206`) read the file's own declaration. Property-hook signatures are built with their property. A set hook's parameter whose native type is the property's, written or omitted, takes the property's PHPDoc refinement by decision 8's rule, and one with a wider native type keeps its own (decision 14). A redeclared method's body is checked against its own declaration's entry, so `redeclared_methods` goes (`registry/class_info.rs:382`, read at `checker/function_decl.rs:65` and `:245`) | split C4 snapshots; decisions 3, 11, 14 | OPEN; a delta, approved and corrected (decision 14; nsrt `property-hooks.php`) | REG-8.1 |
| REG-8.6 | **Snapshots own their names.** The registry's and the checker's `syntax::intern::intern` calls (about 100) go through the project's interner; the stub layer's names stay global; `apply` sweeps the session interner's entries that nothing else holds | split C4 snapshots | OPEN; overlaps NAME-1 and NAME-2 | REG-8.1 |
| REG-8.7 | **Closure gates.** A test applies a sequence of deltas and compares every query with a fresh build of the final file set: a property test over the fixture files, and one test per staleness case (REG-9) | split C4 snapshots | OPEN | REG-8.1 |
| REG-9 | **Staleness fixes,** each with a closure gate: anonymous classes are removed with their file version; a file's global constants are removed with it; removing a declaration of a built-in's name keeps the built-in; removing one of two same-named declarations keeps the other; `stub_provided_functions`, `names_with_assert_tags` and the dead-code escape sets follow their files | split C4 staleness; decision 11 | OPEN; deltas, approved in the plan | REG-8 |
| REG-10 | **The 188 registry tests** and the other `Registry::build` callers move onto `apply` and the accessors; `Registry::build` and `Origin::Unknown` are deleted | split C4 tests | OPEN | REG-8 |
| REG-11 | Behaviour fixes, one commit each, after the structural ones | split C4 behaviour | OPEN; each a delta, approved in the plan | REG-7 to REG-10 |
| REG-11.1 | Declarations below the top level are indexed (inside function bodies, `try`, `switch`), conditional, from the tree's declaration vectors (PROD-5.1). The plan's one Visitor walk is not built: `declared_names`, the anonymous-class walks and the stub-index generator read the same vectors | split C4 behaviour (`.10a`); decision 5 | OPEN | PROD-5.1 |
| REG-11.2 | Class names resolve case-insensitively where a type's or a literal's name is looked up, and a type takes the declared spelling; a written union collapses members that name one class; the exact class accessors are deleted | split C4 behaviour (`.10b`) | OPEN | — |
| REG-11.3 | A conditional or duplicate class-like declaration is checked against its own hierarchy | split C4 behaviour (`.10c`) | OPEN | — |
| REG-11.4 | Duplicate class-like and function declarations compare names case-insensitively | split C4 behaviour (`.10d`) | OPEN | — |
| REG-11.5 | Member names fold case everywhere; the exact method accessors are deleted | split C4 behaviour (`.10e`) | OPEN | — |
| REG-11.6 | Member resolution follows PHP: the class chain first, then interfaces; enum natives implement an interface declaring them; a `@property` tag over a private property | split C4 behaviour (`.10f`) | OPEN | — |
| REG-11.7 | Trait composition: `insteadof`, visibility-changing `as`, `final`, an abstract method implemented by another trait, `@use` bindings, aliases resolved in the trait's scope, a shadowed trait method | split C4 behaviour (`.10g`) | OPEN | — |
| REG-11.8 | Each `@extends`/`@implements` tag binds only the parent it names | split C4 behaviour (`.10h`) | OPEN | — |
| REG-11.9 | PHPDoc inheritance carries asserts, `@param-out`, `@phpstan-self-out`, `@throws`, purity and method templates; a property redeclared without `@var` keeps its parent's; inheritance searches every parent interface | split C4 behaviour (`.10i`) | OPEN | — |
| REG-11.10 | `@phpstan-type` aliases apply in `@property`/`@method`/assert tags and on enums, chained to any depth; an alias on an import cycle stays unresolved | split C4 behaviour (`.10j`) | OPEN | — |
| REG-11.11 | Enums read `@deprecated` on cases, `@var` on constants, and `@phpstan-require-*` | split C4 behaviour (`.10k`) | OPEN | — |
| REG-11.12 | Promoted properties carry asymmetric visibility and their own `@deprecated`/`@internal` | split C4 behaviour (`.10l`) | OPEN | — |
| REG-11.13 | Anonymous classes resolve names per namespace block, and are named `class@anonymous` | split C4 behaviour (`.10m`) | OPEN | — |
| REG-11.14 | `merge` splits into one operation per case | split C4 behaviour (`.10n`) | SUPERSEDED by decision 11: no declaration merges into another, and what is left, a built-in's readings and forms, is decision 10's and STUB-7.3's | — |
| REG-11.15 | LSP navigation uses the folded keys | split C4 behaviour (`.10o`) | OPEN | — |
| REG-11.16 | A table that two entities map resolves to no entity | split C4 behaviour (`.10p`) | OPEN | — |
| REG-12 | **C4's gate.** The gated steps' medians are lower than the phase start's (`xtask gatetime --since 58cfc5d2`); no stub class resolves unless something reaches it; peak RSS is at or below the phase start | split C4 gate | OPEN | REG-2 to REG-11 |
| REG-13 | The remaining parts of item 4, the registry by declaration ids | 20 G 4 | SUPERSEDED (decision 3): what is still wanted moved to REG-4.15, REG-7.4, REG-8.3 and REG-16; each sub-task names where | — |
| REG-13.1 | **Generational ids.** `ClassId`/`FnId` carry a generation, in three ranges: a stub base record's index, a context base record's index, then project ids. The registry's id space has a generation of its own, which a switch to the PHPStan base bumps | 20 G 4 | SUPERSEDED by REG-16.2: interned ids, never reused, in no ranges and with no generation | — |
| REG-13.2 | **`FileDecls`.** A file's declarations are stored together, one vector per kind, with a class's members and a function's parameters as ranges, and one folded index per file. A stub base record decodes into the same layout | 20 G 4 | SUPERSEDED by REG-8.3, whose per-file sets take this layout | — |
| REG-13.3 | **The member list.** A class has one entry for every member it has, its traits' and their aliases included, in declaration order, each pointing at the declaring `FunctionReflection` without a copy; `MethodRef` is (class, slot) | 20 G 4 | SUPERSEDED by REG-7.4, whose result takes this layout; a member's identity is (class id, kind, folded name) (REG-16.2), and its slot is only a position. VAL-2.3 stands | — |
| REG-13.4 | **Results as tables by id:** purity and property-write verdicts by `MethodRef`, ancestors and owners, the PHPStan-ancestry fact, inherited doc types, trait members, constant values. `raw`, `reset_scoped` and the per-member copies go | 20 G 4 | SUPERSEDED by REG-1 and REG-7: the results are query results of the engine, keyed by interned ids (REG-16.3) | — |
| REG-13.5 | **Passes as parallel maps,** level by level over the hierarchy; references between classes on demand, with a memo per id; the global-variable pass over the include graph; fixed points as worklists | 20 G 4 | SUPERSEDED by REG-1 and REG-7, which replace the passes; the global-variable pass moves with the crate split's C9 | — |
| REG-13.6 | **The scope is a bitset over ids,** which `closure_scope` yields | 20 G 4 | SUPERSEDED by REG-16.4: the engine runs again only the queries whose recorded reads changed, so no scope is left | — |
| REG-13.7 | **Snapshots and change detection by id:** `changed_since` becomes a parallel filter | 20 G 4 | SUPERSEDED by REG-8 and REG-16.3: snapshots are the engine's revisions, and early cutoff replaces `changed_since` | — |
| REG-13.8 | **Queries read the tables** (F6b, 149,360 allocations). The hierarchy walks iterate the ancestor slice; `visible_class_consts` reads the owner table; the PHPDoc-inheritance backfill runs level by level; attributes are returned as a slice | 20 G 4 | SUPERSEDED: the ancestor slice and the owners are REG-7.1's and REG-7.4's levels, the backfill is REG-7.3's level, and slices are REG-4.9's | — |
| REG-13.9 | **A declaration is built straight into its final form:** the ordered member list is filled once, with one folded index, where `from_def` builds three hash tables per class; a member's span is a field of its entry | 20 G 4 | SUPERSEDED by REG-4.15 | — |
| REG-14 | Declarations carry only what every reader needs | 20 G 8 | PART | — |
| REG-14.1 | **Case references leave the declarations.** The case rule, the `@internal` rule (`checker/internal_usage.rs:337`) and the other rules that judge a written name read the file's record of written names (PROD-5.8). The six fields item 8 names leave, with a format bump: a property's `case_refs` and `set_case_refs` (`registry/class_info.rs:163`, `:166`), a class's `type_alias_case_refs` (`:518`) and `mixin_case_refs` (`:565`), a parameter's `case_refs` (`registry/function_sig.rs:65`) and a function's `return_case_refs` (`:260`). So do an assertion's (`registry/function_sig/asserts.rs:60`) and `collect_class_name_case_refs` (`registry/name_context.rs:55`). A member name's check needs types and stays in the checker | 20 G 8; decision 2 | OPEN; a delta, approved (VAL-2.6) | PROD-5.8 |
| REG-14.2 | Fields read only while their own declaration is checked leave it: `explicit_mixed_hint`, `return_explicit_mixed_hint` | 20 G 8 | DONE by REG-4.8 (`e7b2521f`, `0f32829b`); no such field is left | — |
| REG-14.3 | **Duplicate parameters** are resolved as a function's parameters are pushed into its file's parameter vector (the last occurrence, at the first one's place); `dedup_versioned_params`' map and order vector go | 20 G 8; decision 3 | OPEN | REG-8.3 (the parameter vector, item 4's `FileDecls`) |
| REG-14.4 | `from_decl` passes the parameters on, not a `Vec<&str>` of their names | 20 G 8 | READY | — |
| REG-14.5 | `reconcile_with_native` refines a lone member and returns it, with no `Vec` | 20 G 8 | READY | — |
| REG-15 | Extract `reflection` | split C10 | OPEN | — |
| REG-15.1 | No module-graph edge is left to the extracted modules. The graph's last two `registry` edges, to `registry::global_vars` and `registry::purity`, are the crate split's C9's, and its C6-C9 remove the others | split C10 | OPEN | REG-12, STUB-7, the crate split's C6-C9 (not tracked here) |
| REG-15.2 | **The extraction.** `cargo machete` checks the plan's Appendix E rows, and gate 10's engine-I/O rule covers `crates/reflection` | split C10 | OPEN | REG-15.1 |
| REG-16 | **One incremental query engine over the whole pipeline,** with its results persisted (CACHE-13.2) | decision 3 | OPEN | — |
| REG-16.1 | **The measurement first.** salsa (0.28.5) is the default engine: rust-analyzer and Astral's `ty` are built on it, and it has a `persistence` feature. On a prototype, coarse queries (a class's members, a name's declarations) must keep the check's hot lookups plain table reads; the check's lookups are timed through the engine against today's tables. The result decides between salsa and an engine of our own with the same contract | decision 3 | DONE (2026-10-15): salsa, with each name asked once per file (its record) | — |
| REG-16.2 | **Interned identity.** A declaration's id is interned from (file, kind, folded name, occurrence in the file) and is never reused; a member is (class id, kind, folded name). A slot is a position within one revision, never an identity: a `(FileId, slot)` pair shifts when its file changes, and a stale one names another declaration | decision 3 | READY | — |
| REG-16.3 | **The engine.** Queries are keyed by interned ids. The accessors record what each query reads, absent names included, so a declaration added later reaches the queries that looked for its name (REG-8.2's read sets). Every result has early cutoff. The builtin knowledge (decision 7) is a durable input. Cycles are the engine's to find (REG-7.1's, REG-7.6's). Stale work is cancelled when an edit arrives. REG-7's levels are its queries and REG-8's snapshots its revisions. Purity (REG-1) and global-variable inference run again per revision, until the crate split's C9. The check asks the engine once per file for each name it reads, and reads the answers from a table of its own after that: a query per lookup costs 374 instructions, where today's tables cost 73-300 (REG-16.1) | decision 3 | OPEN | REG-16.2 |
| REG-16.4 | **The whole pipeline as queries.** The file texts are the inputs, read through LOAD-1.4's file source and its overlay. The parse, a file's products, its declarations, resolution and the check are queries, and the batch run and the server run the same ones. It replaces: the keeping and re-parsing of LOAD-1.6 and LOAD-5.3, since a tree is a parse result the engine evicts and parses again on demand; the dependents indexes (`dependents`, `signature_dependents`, `project.rs:250`, `:253`), `walk_affected` and `closure_scope`; and the hand-made cancellation | decision 3 | OPEN | REG-16.3, SYN-1.2 (a tree shared as `Arc<Ast>`), LOAD-1.4 (the file source) |
| REG-16.5 | **The query trace.** When a runtime switch asks for it, in any build, the engine reports every query a run executes, with the query that first asked for it, and counts the stub layer's queries apart. It explains what a run resolves and gates nothing: REG-12's test is the gate | decision 12 | OPEN | REG-16.3 |
| REG-17 | **What a body shows, inferred on demand** (decision 9): only the declarations a query reaches, project and vendor alike, each inference a query of the engine. A body speaks for a call only when PHP binds the call to it, and its facts fill only what is undeclared or refuted | decision 9 | OPEN | REG-16.4 |
| REG-17.1 | **A return inferred where none is declared.** A declaration with no native return type and no `@return`, own or inherited (REG-7.3), returns what its body's returns infer, for the calls PHP binds to that body. Today it returns `mixed` (`registry/function_sig/type_resolution.rs:101`), and only closures and arrow functions infer theirs (`checker/expr.rs:3977`). A generator returns `Generator` with its yields' types. A recursive body is inferred to a fixed point, a cycle REG-7.9 expects. The inferred type keeps the literals the body returns, and an iteration of the fixed point that changes a literal widens it to its kind (decision 17) | decision 9; decision 17 | OPEN | REG-16.4 |
| REG-17.2 | **Throws inferred where none are declared.** A declaration with no `@throws`, own or inherited, throws what its body can: its `throw`s and its calls' throws (declared, or inferred the same way), less what its `try` blocks catch. A call that PHP does not bind to one body, or that resolves to nothing, throws anything. Today a resolved call with no `@throws` throws nothing for `throws.unusedType` (`checker/throws.rs:43`) and anything for the catches around it (`call_throw_type`, `:105`); both read the inferred throws. A caller's `@throws FooException` for an undocumented function that throws it is then no longer `throws.unusedType` | decision 9 | OPEN | REG-17.1 |
| REG-17.3 | **A context declaration's refinement that its own code refutes is widened.** A PHPDoc type of a return or a property, whether it narrows a native kind (`positive-int` on `int`, `non-empty-string` on `string`, a shape on `array`, a generic's arguments) or is all that is declared, is refuted by a counterexample in the declaration's own code. For a return, that is a return its body can reach. For a property, it is its default (a promoted parameter's default included) or a write to it in its declaring class's bodies, its traits' included. A counterexample is a value whose inferred type shares no value with the declared type, decision 8's combination. Only such a definite one counts, since the inference knows less than the author. A refutation holds for the whole declaration: every call may reach the refuting body, and every read may see the refuting write. A refuted refinement is widened to the native kinds it narrows. With no native type nothing stays declared: a return is then inferred as an undeclared one is (REG-17.1) for the calls PHP binds to the body, and is `mixed` for the others; a property is `mixed`, since hydrators write it outside its class's code (decision 9). A report the widening causes names the refuting line in a tip. An analysed declaration is not widened: its refutation is reported where it happens, by `return.type` at the return, `assign.propertyType` at the write and `property.defaultValue` at the default | decision 9 | OPEN | REG-17.1 |
| REG-17.4 | **Receiver types in the body summaries.** A method call on a receiver a summary cannot name today (a local, `$this->a->b`, a call's result; `Receiver`, `registry/summaries.rs:47`) takes the type the body's inference gives. Today such a call makes the body impure (`:494`) and its writes anything (`:504`). So a call to that body forgets what is known of its receiver's and its object arguments' properties (`forget_members_a_call_may_change`, `checker/expr.rs:178`), and the purity checks (`function.resultUnused`'s family and `impureMethod.pure`, `checker/noop.rs`) never prove it pure. Purity follows such a call where PHP binds it to one body, and writes follow it as they follow the receivers named today | decision 9 | OPEN | REG-17.1 |
| REG-18 | **The crate split's C2 and C3 follow-ups** not done on this branch (Q-11's audit). C2's predefined constants are done for one version (`PHP_VERSION_ID` is `int<80500, 80599>` at 8.5, `PHP_INT_MAX` `2147483647\|9223372036854775807`), and their range form is STUB-7.1's | split C2; split C3 | OPEN | — |
| REG-18.1 | **The settings gate:** an `xtask arch` rule that no `*Settings`, `CheckSettings` or `ProjectSettings` is named under `registry` or `stub_attributes`, so reflection reads no settings type but `ReflectionSettings`. `xtask/src/arch.rs` has none | split C2 | READY | — |
| REG-18.2 | **`ConstantResolver` in `reflection`,** with `dynamic_constant_names` a `ReflectionSettings` key; both are the checker's today (`checker/constant_resolver.rs:21`, `EngineSettings`). `resolve_constant_named_types` and class-constant initializer typing read constants through it, and the module graph assigns `checker::constant_resolver`, `checker::predefined_constants` and `global_constant_type` to `reflection`, so C10 doesn't extract it without them | split C2 | READY | — |
| REG-18.3 | **Initializers evaluated by one evaluator** (PHPStan's `InitializerExprTypeResolver`), for class and global constants, enum case values and parameter defaults. At HEAD `const A = 2 + 3`, `1 << 2` and `\PHP_INT_SIZE` are `mixed`, and a backed enum's `case A = self::X` or `case B = 2 + 3` has an `int` value. REG-7.6's value types and throws are this evaluator's results | split C2 | READY | — |
| REG-18.4 | **One folded constant key:** a constant's namespace prefix folds case at every lookup, `checker/imports.rs`'s `use const` check and `dynamic_constant_names` included. PHP 8.5.4 finds `App\A` as `\APP\A` and `\app\A`, and not as `\App\a`; at HEAD `\APP\A` is `constant.notFound`. `generalize` already keeps list-ness and the key type (`types/src/queries.rs:670`) | split C2 | READY; a delta | — |
| REG-18.5 | **`RuleLevelHelper` owns the rule:** the level thresholds, benevolent unions and strict mixed arrays leave `types::Relation::accepts`, which keeps `configured_level`, `strict_mixed_arrays` and `check_benevolent_union_types` today (`types/src/relation/accepts.rs:330`, `:502`, `:519`). Assertion tables are reapplied on reconfigure until C6 | split C2 | READY | — |
| REG-18.6 | **`is_phpstan_code` a provenance query** (an `Origin::Phpstan` class counts), and `phpstan_class_files` deleted. Today it reads `Registry::phpstan_class_files` and looks for a `vendor` path component (`checker/phpstan_api.rs:254`, `registry.rs:276`) | split C3 | READY | — |
| REG-18.7 | **Attribute arguments evaluate to types:** class constants through the `ConstantResolver`, with `self`, `parent` and `static` bound to the class, bare constant names namespace-resolved, and `new` and constant expressions (C8). At HEAD a global constant is typed (`#[Table(\PHP_INT_SIZE)]` is `argument.type`, "4\|8 given") and another class's private constant is `classConstant.private`; `#[Table(self::NUM)]` with `const NUM = 1` is not checked, and an undefined `self::BAR` is not reported | split C3 | OPEN | REG-18.2 |

### Targets

| task | target now | report's estimate |
|---|---|---|
| REG-7 | the passes it replaces: `flatten_phpdoc_inheritance` 162.1 M, `precompute_ancestor_names` 147.9 M, `precompute_member_owners` 121.5 M, `drop_doc_members_excluded_by_native` 86.1 M, `resolve_deferred_value_of` 69.1 M, `flatten_traits` 50.0 M (the finishing passes total 972 M over two runs, mostly parallel already). REG-7.2's part of item 6 is the warm run's 682.5 M of stub decodes (MEAS-1) | none in instructions: the crate split's gate is the test steps' wall time (REG-12); item 6's ≈0.35 G covers REG-7.2 with the stub scope |
| REG-14 | `collect_class_name_case_refs` 26.8 M; `declared_type` 456.3 M (240,689 calls, stub files included) | ≈0.1 G |
| REG-16 | the scope and change detection it replaces, which REG-13 counted: `closure_scope` 103.8 M, `reset_scoped` 43.8 M, `Members::eq` 4.8 M; the dependents indexes' `register_references` (77.2 M, in LOAD-4's target) | none: REG-16.1 measures what the engine costs first. Item 4's ≈0.9 G rested on the purity costs P9 removed |

### Notes

- **Item 4's premise is mostly gone.** P9 (REG-1) deleted the eager purity and property-write passes, which held its largest costs: string keys ≈0.5 G, `Members::eq` 96.9 M, 59,637 member copies. Now `Members::eq` costs 4.8 M and `reset_scoped` 43.8 M.
  - What was left is the other passes, which REG-7 replaces in the crate split's design.
  - Decision 3 superseded the rest of item 4 (REG-13). Its generational ids, result tables, bitset scope and change detection by id are retired, because the engine (REG-16) keys its queries by interned ids, records what each reads, and stops a change at an equal result.
- **Item 4's parts that are still wanted sit in C4's rows:**
  - its per-member flags in REG-4.5, and its private fields in REG-4.12;
  - since decision 3:
    - its `FileDecls` in REG-8.3;
    - its member list in REG-7.4;
    - its final-form build in REG-4.15.
- **REG-5:**
  - REG-1 (P9) did its purity part: `Registry::demand` answers purity and property writes, and `purity_dependencies` and `is_transitively_pure` left the build.
  - `names_with_assert_tags` is still filled by the insert (`registry/index.rs:453-481`) and by the assertion table (`registry/assertion_table.rs:142`).
  - Two comments still name the deleted `resolve_transitive_purity`: `checker/noop.rs:226` and `registry/function_sig.rs:1008`.
- **REG-7:**
  - A level stores what it changes and shares the rest with the level below (member maps behind `Arc`).
  - The plan has a change to any reflection input reset every memo within REG-7's commit (`doctrine_entity_table_index` included), until REG-8's read sets. On the engine (decision 3) a change reaches only the queries that read it, so that interim reset is not built. The collision policy reads declarations only.
  - The plan's approved deltas, each fixtured:
    - a method that two nested traits both supply resolves by `use` order, depth-first (REG-7.2);
    - a constant on a reference cycle types as `mixed` (REG-7.6). Decision 1 withdrew this one: REG-7.6 has the rule that replaced it, approved as VAL-2.5.
  - The final level's combination of native and PHPDoc types changes reports too, as decision 8 has them, each with a fixture (the plan's where it has one): for example, a PHPDoc conditional of `void` and `never` over a native `void` applies, and `@param string[]` on `string|array|A` keeps `A` (REG-7.5).
  - Purity moving after resolution changes nothing; a fixture pins `@return null` on `: void`.
  - The commit runs the full gate tier. Peak RSS stays at or below the phase start, and a test pins that loading a project and checking one file resolves no class the file doesn't reach.
  - `has_unresolvable_ancestor` reads the unresolved edges. The boolean `is_subclass_of` keeps counting an unresolved name as an ancestor until the crate split's C8.
  - The whole-program passes the plan keeps until its C9 are `resolve_transitive_purity` and `infer_global_variables`. On this branch the first is gone with REG-1, so only `infer_global_variables` (`registry/global_vars.rs:315`) is left.
  - **Wall time (MEAS-4; `ITEMS_1_TO_3_FIXES_PLAN.md`'s decision 1).** The finishing passes are the largest stretch of unused cores in a run at `aa896697`. They run from 1.16 to 1.52 s of the 2.02 s off run and from 0.75 to 1.19 s of the 1.70 s warm run, at 2 to 3 of the 8 cores, and leave about 0.26 s (off) and 0.28 s (warm) of the cores unused. The thread in `run_finishing_passes` is busy throughout (`is_folded_eq` and `fold_hash` lead its leaves), and the pool takes only part of the work, mostly `flatten_phpdoc_inheritance_one_pass`; 29% of the samples from 0.8 to 1.5 s of the off run are kernel time, mostly task switches. No row speeds the passes up: REG-7 replaces them, REG-4.2 deletes `canonicalize_class_name_case`, and `infer_global_variables_of` stays. This stretch is REG-7's wall-time baseline.
- **REG-17 adds work rather than removing it:** each inferred body is checked once per revision, and only the bodies a query reaches. Its cost is measured as it lands, against the check phase of the warm run (MEAS-1).
- **REG-8.4:** the plan counts `phpstan_class_files` as already gone (the crate split's C3 follow-up), but it is still a field (`registry.rs:276`). See Q-11.
- **REG-14.1 and REG-7.7 (decision 2).** No declaration holds case references. The rules read the file's record of written names (PROD-5.8), and REG-7.7's scratch is what is left.
- **REG-14:** item 8's one approved output change is a `@property` tag shadowed by a declared property, checked against the tag's own names (VAL-2.6).
- **Memory (MEAS-2):** item 4 holds 87.7 MB at exit in the off run (was 149.6 MB) and 46.7 MB in the warm run. Its ≈90 MB claim is stale by about half.

### Records

**REG-1** (off P9): DONE `c0dd2788`

- *Status, as recorded:* DONE (c0dd2788; measured: off run 22,362,774,991 Ir against 23,216,039,715 at P7's commit, −853.3 M, Piwigo output byte-identical; keyed by names, not by item 4's ids, so it did not wait for them. `Registry::function_is_pure`, `method_is_pure`, `function_property_writes` and `method_property_writes` take the `BodySource` the checker holds and search from the declaration asked about: an iterative depth-first search over the calls of its body, a call back into a declaration the search is inside of answering impure / `Anything`, which is the least fixed point because a declaration in or reaching a cycle never leaves it; every declaration the search settles is remembered in `Registry::demand` (two `RwLock`ed maps, nothing held while a search runs, a copy of a registry starts empty). The eager passes, `purity_dependencies`, `is_transitively_pure`, `clear_purity`, `property_writes` and `property_write_classes` are gone from the build. The incremental path takes the answers found since the last finish (`reset_demand`) and, once the declarations have settled, finds each again and marks the class or function whose answer differs (`changed_since`), so a file is re-checked on the same ground as before, for the answers something read. A debug build settles every declaration at once, as the build did, and each answer given on demand is asserted equal to it wherever the bodies at hand are the build's. Tests: `registry::tests::demand` (7: chain, cycle asked from any member, impure dependency, methods, `NoBodies`, property-write verdicts, order of askers), one new fixture (a cycle of methods with a return type), lib 1056 and fixtures 3829 passing in the test profile; both mutation checks (a stack hit answering pure; a stack hit answering `Nothing`) fail the oracle and the new tests. Not run: workspace tests other than the two, clippy.)
- *Task:* Purity and property writes on demand: a memoized depth-first search from the declaration asked about (least fixed point), the memo cleared on each edit; `check_declared_purity_tag` tests the `@phpstan-impure` / `@impure` tag first.
- *Estimated saving:* −853.3 M measured (estimated ≈ −0.3 to −0.4 G beyond item 4)
- *Needs:* —
- *Verify:* A debug build runs the eager pass and compares every answer given on demand; order of askers does not matter.

**REG-3**: DONE `58cfc5d2`. Phase start for `58cfc5d2`: lib tests 9.2 s, fixtures 1.3 s (`nice -n 19`, loadavg 2.71).

**REG-4.1**: DONE, already on this branch: `NameTable`, `with_function_key`.

**REG-4.3**: DONE `5401afc9`.

- The type relations read a class's slot variance from the same list, and the declaration check takes parameters.
- Before it, the checker's own rebuild dropped the default, and another rebuild read an unwritten bound as implicit `mixed`.
- An erased receiver's template with no default and no bound stays unbound: `0f32829b` restored this, after it had been changed to match PHPStan's output.

**REG-4.4**: DONE `8d35c99d`. A property records whether it is final, natively or by its `@final` tag, where the class held two sets of final names. A final property taken from a trait is the class's own (PHP's rule, a delta the plan did not list); one reached through a mixin is not.

**REG-4.6**: DONE `7a61603e`. A property's native type is `None` when it has no native hint; `has_native_type` is gone.

**REG-4.8**: DONE `e7b2521f`, corrected `0f32829b`.

- A written `mixed` excuses its own value type only.
- A union keeps a written `mixed` when it drops an equal member.
- A conditional return is read at its branches, not at the type its condition tests.
- Output changes on the missing-type rules only; Piwigo's output is identical.
- `e7b2521f` was checked against PHPStan's output on the vendored trees: the old reading differed from it on 216 reports where the new one does not, and the new one differs on 2.
- `0f32829b` restored two readings that `e7b2521f` had changed only to match PHPStan's output, which is no reason: a native iterable still admits an array, and a method overriding a built-in one that omits the return type is reported.
- This also delivers item 8's explicit-mixed part (REG-14.2).

**REG-16.1**: DONE (2026-10-15). salsa stays the engine, and the check asks it once per file for each name it reads.

- *What the check reads.* A probe build of HEAD counted the table lookups each file's check makes on Piwigo (`NameTable::get`, `get_with_name` and `contains_key`; `~/.cache/prs-ledger/reg161/`). There were 3,410,340 lookups over 2,792 files, of 107,068 names counted once per file (13,076 overall), so a file looks a name up 32 times on average. The median file makes 365 lookups of 21 names, and the largest 31,072 of 413. The output was unchanged.
- *What each path costs.* Those files' lookups were replayed against salsa 0.28.5, in a crate outside the workspace (`reg161/bench`), and counted under callgrind:

  | path | instructions per lookup |
  |---|---:|
  | a plain table read by id | 36 |
  | every lookup a salsa query | 374 |
  | one query per name per file, then a table of the file's own | 67 |
  | today's tables, by `Name` / by text (the P8b profile's calls) | 73-80 / 224-300 |

  The per-file table adds about 1,000 instructions per query to the plain reads, ≈0.11 G on Piwigo's check, which is the most an engine of our own could save there. In wall time, under a load average of 9.8, the first three cost 4-6, 47-62 and 12-14 ns per lookup.
- *Warm runs.* After an input that no file reads changes, the 2,792 files' queries verify in 5-10 ms; after one class changes, in 7-11 ms.
- *So* the engine's bookkeeping is affordable when a check asks it once per name, and not when every lookup is a query: Piwigo's check would then spend ≈1.3 G instructions on its lookups, against ≈0.23 G.
- *Not measured:* what salsa's `persistence` feature can hold, which is CACHE-13.2's question, and queries running on several threads at once.

## 10. STUB: the stub build, the stub layer and the stub data

### Tasks

| id | task | from | state | needs |
|---|---|---|---|---|
| STUB-1 | The stub state as a value owned by the entry point | off E1 | DONE `3d5434b3` | — |
| STUB-2 | Resolve only the stub base the files need | off P1 | DONE (rode on `3d5434b3`) | — |
| STUB-3 | Build the stub corpus beside the parse | off P5 | DONE (rode on `3d5434b3`) | — |
| STUB-4 | A hash index for stub bases built in memory | off P3 | DONE `a13f4ed2` | — |
| STUB-5 | Look each stub file's dependents up once in `build_closures` | off P4 | DONE `0ce52479` | — |
| STUB-6 | D7: build proposals 1-5 now, in the order of the build order | off D7 | DONE (moot: proposals 1-5 are all built) | — |
| STUB-7 | The crate split's C5: locator chain and stub layer | split C5 | OPEN | REG-12 (C4 done) |
| STUB-7.1 | **Gates as data, one parse per file,** over a `PhpVersions` range. The parse records, for each declaration and parameter, its availability as version ranges, its type readings by version range, and the extension it belongs to. Queries take the range: availability is always, never or in part, and in part is reported unless a guard covers it; a return type is the union of the readings in the range; an argument is checked against each reading; a `PHP_VERSION_ID` branch narrows the range. The range comes from the configuration, else Composer's `config.platform.php` or `require.php` (`crates/composer/src/lib.rs:114`), else 7.0 to the newest version, narrowed to the versions the analysed files compile on; a range reaching below 7.0 is read from 7.0, with a notice (decision 18). No cache key holds a version. `php_version` takes the range and patch forms in the same commit. A function removed before the analysed range is unknown | split C5; decisions 6, 18 | OPEN; a delta, approved (decision 14) | — |
| STUB-7.2 | phpstorm-stubs' `PS_UNRESERVE_PREFIX_` names are the reserved words they spell; the prefixed constructs (`array`, `list`, `empty`, `isset`, `unset`, `eval`, `__halt_compiler`) and the `___PHPSTORM_HELPERS` classes leave the index, and `exit` and `die` stay, as functions available from PHP 8.4 (decision 14) | split C5; decision 14 | OPEN; a delta, approved and corrected (decision 14) | — |
| STUB-7.3 | **The stub attributes read as data.** `LanguageLevelTypeAware` (or `PhpVersionAware`) gives the type readings by version range of a declaration, a parameter, a property or a class constant. `PhpStormStubsElementAvailable` gives an availability range, and a deprecation holds only in the range of the overload that has it. The rewrite that applies the newest version and then removes the attributes (`stub_attributes.rs`) goes. With it goes `resolve_names` (`syntax/src/name_resolution.rs:515`), which only it calls (`stub_attributes.rs:185`). The plan's three fixes are what reading every attribute as data gives: a property's or class constant's type, `newest_type` reading every entry, and a deprecation in one overload | split C5; decision 6 | OPEN; a delta, approved (decision 14) | STUB-7.1 |
| STUB-7.4 | **The locator chain finds a name's declarations.** No locator's place in the chain makes one declaration win (decision 11), so its order changes no result. The project's PHPStan, phar or source, is a locator in the chain (STUB-13.1) | split C5; decisions 7, 11 | OPEN | — |
| STUB-7.5 | **The lazy composer locator:** classes load on demand; functions, constants and `files` entries stay eager. A class is found in the file Composer's loader maps it to: its classmap, else the longest PSR-4 prefix and the first of its directories that holds the file, else PSR-0 (`ClassLoader::findFile`; decision 11) | split C5; decision 11 | OPEN; a delta, approved (decision 14) | — |
| STUB-7.6 | **The stub layer,** holding C4's memo levels for stub classes, with a per-file parse memo. No project declaration merges into a stub's (decision 11), so the plan's copy-on-merge goes; a stub memo whose reads name a project's declaration is that project's, as the plan has it. STUB-11's embedded records replace the memo. One layer serves every version range, since no key holds a version (decision 6). Whether the index build uses `rayon` is decided here | split C5; decisions 6, 7, 11 | OPEN | STUB-7.1 |
| STUB-7.7 | **Provenance queries** replace `built_in_stub_classes` and `stub_provided_functions`, each consumer naming its question | split C5 | OPEN | — |
| STUB-7.8 | **The enumeration APIs.** Every whole-registry iteration (REG-7.10's included) moves onto a scoped query, and `repository_entity` onto ext-doctrine's data (the crate split's E4). The index records `parent`/`interfaces` per declaration | split C5 | OPEN | — |
| STUB-7.9 | Version-based skip-polyfill: a polyfill is skipped where its built-in is available in the whole range, decision 6's availability query | split C5; decision 6 | OPEN; a delta, approved (decision 14) | STUB-7.1 |
| STUB-7.10 | Version-dependent signature selection, as decision 6's range query: a return type is the union of the readings in the range, and an argument is checked against each | split C5; decision 6 | OPEN; a delta, approved (decision 14) | STUB-7.1 |
| STUB-7.11 | A panic-injection test | split C5 | OPEN | — |
| STUB-7.12 | **C5's gate:** the wall times of the fixture suite, the lib tests and the cli tests are lower than at the phase start; the stub layer replaces the per-project stub clone and the per-process stub parse; the layer, made to resolve every name its index lists, gives exactly the records of the eager build it replaces (the plan's `generated_stub_data()`, today `Corpus::generated_stubs`, `generated_stub_classes` and `generated_stub_constants`, `registry/stubs.rs:385`, `:775`, `:1189`): the same names, and for each its availability ranges, its type readings and its extension (today the `function_extensions` and `class_extensions` maps, `:1247`). No key holds a version (decision 6), so records that are equal give equal answers for every version range, and the check pins none. It is made before the two delta commits | split C5; decision 6 | OPEN | STUB-7.1 to STUB-7.11 |
| STUB-8 | **Guards decided by the stubs' availability,** on run-time evidence only: built into PHP, tested by Composer's platform check, or named by the configuration. A guard is `Never` or `Always` only on that evidence; otherwise both declarations stay, as candidates (decision 11). Availability is decision 6's range query, and whether an extension is loaded is read from the extension each declaration records. New: a check that a polyfill matches the declaration it replaces | 20 G 6.2; 25 G F2; decisions 6, 11 | OPEN; a delta, approved, both parts (VAL-2.4) | STUB-7 (STUB-7.1's ranges and extensions); overlaps STUB-7.9 and 7.10 |
| STUB-9 | **F2's count as a diagnostic:** a debug-only report of the stub keys in the passes' scope, by the changed name that pulled each in | 20 G 6.4; 25 G F2 step 4 | SUPERSEDED by REG-16.5 (decision 12): the scope it counts goes with REG-7, and why a query ran is the engine's query trace to report | — |
| STUB-10 | How a binary carries the stub data: at compile time or not | off D3 | PART: decided (STUB-10.2); STUB-10.1's measurements are left | — |
| STUB-10.1 | **The measurements the embedding is built on,** at the layer, not estimated: the records' size; whether the stub sources can leave the binary; the build time, clean and after an edit to the reflection crate, with the build script's stub build and the data crate's compile and relink (B9's timings, which were read off the console and not kept); and what a cold run still pays. After C4 and C5 only each stub file's declarations are left, and a run reaches 6-43% of the stub bytes. The off report's C8 estimates ≈0.2-1.5 G per cold run, against 5.05 G in the old code | off D3; decision 7 | OPEN | REG-12, STUB-7 |
| STUB-10.2 | **The decision:** embed the data at compile time or not. If yes, the build script must run the code the runtime runs (the `reflection` crate, REG-15), `.rodata` grows from 18.2 to ≈50 MB per binary, and checked-in binary data is out | off D3 | DONE (decided 2026-10-15, §2.4 decision 7): yes, for PHP's declarations and PHPStan's function map and `.stub` files; PHPStan's own classes are read from the project (STUB-13) | — |
| STUB-11 | **The builtin knowledge built at compile time.** Per-file records of PHP's declarations, PHPStan's function map and PHPStan's `.stub` files are made by the build script with the code the runtime runs (the `reflection` crate, REG-15). They are a read-only layer: no run builds them, and the stub disk cache goes. `**` is folded deterministically, by a pure-Rust `pow` (`types/src/arithmetic.rs:715-717` calls `powf` today), and a test asserts that the embedded records equal a runtime build. The data crate is linked only into the final binaries | off P6; decision 7 | OPEN | STUB-10.1, REG-15 |
| STUB-12 | **The sources of the builtin knowledge,** read as decision 6's data and combined by decision 10's rule: what php-src verifies bounds a type, and the other sources' claims are intersected within that bound | decision 6; decision 10 | OPEN | STUB-7.1 |
| STUB-12.1 | **PHP 8.0 and later:** php-src's own stub files, as PHPStan's `phpstan/php-8-stubs` packages them (`Php8SignatureMapProvider` reads them by version id). Their native types, and a function's PHPDoc return type at func info's precision, are the bound; their other PHPDoc is a claim (decision 10) | decision 6; decision 10 | OPEN | — |
| STUB-12.2 | **Earlier versions, 7.0 to 7.4 (decision 18):** reflection dumps of real PHP binaries, made by a generator kept with the sources. The generator also calls each function with an argument each parameter's parsing refuses and records what it returns or throws, its failure value (decision 16) | decision 6; decisions 16, 18 | OPEN | — |
| STUB-12.3 | **The rest** (the PECL extensions) from phpstorm-stubs, its attributes read as data | decision 6 | OPEN | STUB-7.3 |
| STUB-12.4 | **PHPStan's function map by version:** its deltas (`functionMap_php*delta.php`) give its claims by version range. It is a claim, not a native reading (decision 10). Today the newest revision is read (`registry/stubs.rs:253`) | decision 6; decision 10 | OPEN | — |
| STUB-12.5 | **A check, when the data is built, that reports where the sources disagree** on a declaration they share (decision 10): native readings that differ between sources for one version; a claim's member that the bound excludes; a claim that leaves out a kind the bound names; claims that share no value within a kind; templates or generic ancestors that two sources write differently. The rule gives each case its result; the report is for correcting the sources | decision 6; decision 10 | OPEN | STUB-12.1 to STUB-12.4 |
| STUB-13 | **PHPStan's own classes read from the project's PHPStan,** not embedded. An embedded copy is right only for the pinned 2.2.16; today they are 15.33 of the 31.35 MB of stub data (`stub-base-phpstan`) | decision 7 | OPEN | — |
| STUB-13.1 | **A phar locator.** The installed `phpstan.phar` holds 7,127 entries, 1,959 of them under `src/`, none compressed, with their namespaces intact, so its classes are read in place. `crates/stub-sources/build.rs` says the phar ships no loose source, which is wrong | decision 7 | OPEN | STUB-7.4 |
| STUB-13.2 | **PHPStan's classes as a context package,** from the phar or the source the project installs, cached per file. The PHPStan base (`stub-base-phpstan`) and the switch to it go, and with them `expect_files`' choice of base. Once no run builds the stubs either (STUB-11), a project is made on its one base before its files parse, and `Project::loading` goes | decision 7 | OPEN | STUB-13.1 |

### Targets

| task | target now | report's estimate |
|---|---|---|
| item 6 as a whole (REG-7.2, STUB-8, STUB-9, CACHE-9, CACHE-10) | warm run (MEAS-1): 682.5 M of stub decodes, 414.7 M of them forced by `insert_built_decl` | ≈0.35 G, then more of `warm`; ≈40 MB of heap |
| STUB-11 | — | −5.05 G on the old code; ≈0.2-1.5 G per cold run after the crate split's C4 and C5; STUB-10.1 measures it |

### Notes

- **Item 6 is split across areas.** Its step 1 is REG-7.2, steps 2 and 4 are STUB-8 and STUB-9, and steps 3 and 5 are CACHE-9 and CACHE-10.
  - E1, P1 and P4 (STUB-1, STUB-2, STUB-5) changed how the stub base is built, but not what item 6 reads.
  - The warm run at this code (MEAS-1) decodes 87,146 stub function records.
- **STUB-7 removes the eager stub build** that STUB-2 to STUB-5 speed up. Until it lands, every cold run pays for that build.
- **What a cold run still pays, for STUB-10.1 (MEAS-4; `ITEMS_1_TO_3_FIXES_PLAN.md`'s decision 5).** In an off run at `aa896697` the stub build (`build_generated_stub_data`) takes the first 0.21 s at 4.7 to 4.8 cores, beside vendor discovery. Later the main thread is blocked 89 ms in `Stubs::base`, asked by `expect_files` (0.917 to 1.006 s of 2.02 s), while the stub-base thread runs `generated_stubs`, `merge_stubs` and `freeze` (copies and a sort of `(Name, Arc)` pairs) and the base's finishing passes run on the pool. A warm run reads the base from the stub cache and waits about 9 ms. No row speeds this up: STUB-11 removes the build, and STUB-13.2 removes the choice of base and the wait.
- **STUB-7.1's version key (decision 6).** The crate split's text says "member and parameter gates are inside the files, so the layer parses per version". Decision 6 took the off report's recommendation instead: the gates are data, with one parse per file and no version in any key.
- **STUB-8, STUB-7.9 and STUB-7.10 decide guards over the same declarations.** Since decision 6 all three are range queries over STUB-7.1's data. STUB-8 adds the extensions as evidence, and the polyfill check.
- **Decision 7 splits the stub data.**
  - The builtin knowledge is embedded (STUB-11): PHP's declarations, PHPStan's function map and its `.stub` files.
  - PHPStan's own classes are read from the project (STUB-13), so the plain and the PHPStan bases are no longer two builds.
  - STUB-12's sources feed STUB-11's build.
- **STUB-9 is superseded (decision 12).** After REG-7 there is no passes' scope left to count. REG-7's test pins that checking one file resolves no class the file doesn't reach, and why a query ran is the engine's query trace (REG-16.5).
- **Memory (MEAS-2):**
  - In the off run, the stub build (P6 in MEAS-2's item mapping) holds 94.3 / 94.3 MB now, against 132.8 / 111.8 then.
  - In the warm run, the stub records hold 109.2 MB, against item 6's ≈40 MB claim.

### Records

**STUB-1** (off E1): DONE `3d5434b3`

- *Status, as recorded:* DONE (3d5434b3; measured: Piwigo output byte-identical to the base in the off, cold and warm modes; Ir off 25,029,730,463 against 25,625,190,065 (−595.5 M), cold 25,238,288,982 against 25,683,423,164 (−445.1 M), warm 20,551,124,205 against 20,572,181,579 (−21.1 M); the cold run stored `stub-base-phpstan`, `stub-data` and `stub-index` and no plain base, and the warm run rewrote none of the three)
- *Task:* The stub state is a value owned by the entry point (`Stubs`, `registry/stub_set.rs`), built per layer on a thread of its own with the wait blocking in the OS (S2), on the pool the entry point hands in (S3), with the base chosen at `expect_files` (S4: `Project::loading`, then `expect_files(parsed, &Stubs)`; `finish_loading_with(&Stubs)` for a project that got none). Production code has no static for it; a `Registry` keeps an `Arc<Corpus>`. Not done: S4 is a runtime check (a registry with no corpus panics when asked), not a type; the regression test (U4) and `cargo test`/fixtures/clippy were not run, by instruction.
- *Verify:* The regression test with `PHPSTAN_RS_NO_STUB_CACHE=1`, a one-thread pool and two nested `Project::new()` calls, with a watchdog (run on 2026-10-16 as Q-4: it fails at `d8f524a8` and passes at `9c7c9555`; committed as `55d9ea1f`, in the form `crates/phpstan-rs/tests/stub_base_reentry.rs`); `cargo test --lib` cold; fixtures. Re-entry impossible by construction.
- *Since, 2026-10-15:*
  - *The runtime check let a defect through.* `phpstan-rs demo` built a registry of its own, which never got the corpus. So from `3d5434b3` the demo panicked on every file that calls one of its own functions, its built-in sample included: `checker/expr/builtin_calls.rs:39` asks the corpus whether a called function that is not a stub is a builtin's polyfill. The CLI test of the sample failed all along, unrun. `14dcddb2` loads and checks the demo's file in a project on the stubs. At that commit the CLI suite (44 tests) and the other integration tests (`file_cache`, `lsp_exit_code`, `scaling`, `stub_index`, `version`) pass.
  - *S4 by type is REG-8.4's.* One `Registry` type has three roles today: the registry a project checks with; the registry `index_and_flatten` builds while it extracts the stub declarations (`registry/stubs.rs:1491`), which makes the corpus and so can't hold it; and the tests' tables (23 `Registry::default()` calls, and the `Registry::build` callers of REG-10). Only the split into a builder and a provider makes a provider without the corpus unrepresentable. A `Project` typestate now would cover only the project's path and leave the checker's `expect`; REG-8.4 would reshape it, and STUB-11 and STUB-13.2 take away the waiting and the base choice `Project::loading` exists for.
  - *U4's regression test* is Q-4: run on 2026-10-16, and committed as `55d9ea1f`.

**STUB-2** (off P1): DONE, rode on `3d5434b3`

- *Status, as recorded:* DONE (rode on 3d5434b3; verified at 0ce52479: a two-file project that never mentions PHPStan and one that does give output byte-identical to the base binary's; the first stores only `stub-base`, the second only `stub-base-phpstan`, where the base binary stored both; a mention only inside a body is P8's and not built)
- *Task:* Resolve only the stub base the files need: the project starts with no base and takes one at `expect_files` (E1's S4).
- *Estimated saving:* −355.1 M serial; the plain base's finishing run ≈35-52 M (estimated)
- *Needs:* E1
- *Verify:* Output identical on a project that mentions PHPStan and on one that does not; a file that mentions PHPStan only inside a body (P8) still picks PHPStan's base.

**STUB-3** (off P5): DONE, rode on `3d5434b3`

- *Status, as recorded:* DONE (rode on 3d5434b3; paired wall time, Piwigo, file cache off, stub cache warm, `hyperfine -N` 6 runs after 1 warmup, alternating base/new twice, `nice -n 19`, loadavg 4-7 throughout: base binary mean 3.380 s and 3.286 s, the binary at 0ce52479 3.057 s and 3.206 s, medians 3.423/3.220 against 3.046/3.161. The new binary carries E1 to P4 together, so P5's share alone is not separated, and the difference is within one standard deviation of the base's runs; it is not slower. An earlier pair of runs overlapped another and was discarded.)
- *Task:* Build the stub corpus beside the parse: E1's S2 and S3; the corpus's `par_iter` moves into `worker_pool().install`; `check-project` makes the stubs before listing files.
- *Estimated saving:* wall time: ≈0.14 s alone, the whole ≈0.25 s stretch with P1 (a model); no Ir change
- *Needs:* E1
- *Verify:* Paired wall-time runs; `PHPSTAN_RS_THREADS` and the 64 MB stack apply to the corpus.

**STUB-4** (off P3): DONE `a13f4ed2`

- *Status, as recorded:* DONE (a13f4ed2; measured: off run 24,155,076,222 Ir against 24,498,907,030 at D5's commit, −343.8 M, output byte-identical; the test of the lookups against the ordering, `a_base_built_in_memory_finds_each_name_where_the_ordering_puts_it` (`registry/name_table.rs:1285`), was written and compiled then; run on 2026-10-15 at `f3ae5e86`, it passes)
- *Task:* A hash index for stub bases built in memory: `freeze` builds the index `store_entries` already builds.
- *Estimated saving:* ≈ −0.29 G
- *Needs:* —
- *Verify:* A built-then-frozen base answers every lookup as the bisection did. Checked by that test: it builds through `Base::built_shared`, the path `NameTable::freeze` takes (`:927`), and compares `find` and `find_by` with a bisection of the sorted keys, for a folding base and a plain one, over 3,000 names that collide in the index, and five absent keys.

**STUB-5** (off P4): DONE `0ce52479`

- *Status, as recorded:* DONE (0ce52479; measured: off run 23,866,820,730 Ir against 24,155,076,222 at P3's commit, −288.3 M, output byte-identical; the stub index a cold run stores is byte-identical to the one stored before, so the closures are equal file by file; the test it asked for is `4a8b7f05`)
- *Task:* Look each stub file's dependents up once in `build_closures`: per-file dependent lists, closures in strongly-connected-component order.
- *Estimated saving:* ≥ −0.2 G serial
- *Needs:* —
- *Verify:* The closures equal today's, file by file. Checked by `registry/stubs/tests/closures.rs` (`4a8b7f05`): every file of the corpus, where the test before it took every 37th, compared word for word with the key-by-key walk that `build_closures` made before `0ce52479`; hand-made files with the shapes the corpus need not have (a self-reference, a chain into a cycle, linked cycles, a diamond, files reaching a finished component from a later start or branch, a key two files declare, closures over several words, a 3,000-file chain); and random files under `proptest`. Breaking the merge of the components a component leads to, or merging only one member's edges, fails all three tests.

**STUB-6** (off D7): DONE

- *Status, as recorded:* DONE (moot: proposals 1-5 are all built, rows P1-P5; the build order below is complete)
- *Decision:* Build proposals 1-5 now, in the order of the build order below, since the crate split removes the eager build they speed and every cold run pays it until then. Alternative: only proposal 4 and E1's fix.
- *Gates:* P1-P5
- *The build order (D7):*
  1. **E1's fix**, which carries proposals 1 and 5.
  2. **D5's write check**, which carries proposal 2.
  3. **Proposal 3.**
  4. **Proposal 4.**

  Each step is measured with paired cold runs. B4 puts the ceiling at the 0.37 s the warm stub cache saves today. The cuts, proposals 7-11 and the defects E14 and E15 are independent of this order unless a row's "needs" says otherwise.

**STUB-10** (off D3)

- *Status, as recorded:* OPEN (reopened by C8, §10.6)
- *Decision:* How a binary carries the stub data. Decided again once the crate split's C5 sets its version key, on the numbers in §10.6 (≈0.2-1.5 G per cold run once C4 and C5 land, against 5.05 G today). Proposal 6 is not built in today's form.
- *Gates:* P6
- *Decided again, 2026-10-15 (§2.4, decision 7):* embed the builtin knowledge (PHP's declarations, PHPStan's function map and its `.stub` files) in C8's form, as per-file records the build script makes with the runtime's code. Read PHPStan's own classes from the PHPStan the project installs (STUB-13). The version key it waited on is decision 6's: none. The measurements are STUB-10.1's.

**STUB-11** (off P6)

- *Status, as recorded:* OPEN, blocked
- *Task:* Build the stub data at compile time into every binary. Not built in today's form.
- *Estimated saving:* −5.05 G today; ≈0.2-1.5 G per cold run after the crate split's C4 and C5
- *Needs:* D3; the reflection crate (C10)
- *Verify:* If built: B10's guard test, an embedded-equals-runtime-build test, the override equal to the profile (B9).
- *Since 2026-10-15:* D3 is decided (decision 7), so the row is OPEN and re-specified on its row above. It waits on STUB-10.1's measurements and on REG-15. B10's backstop is the equality test, run on each target the binaries are built for, with `**` folded by a pure-Rust `pow`.

## 11. CACHE: the stub cache's and the per-file cache's contents and formats

### Tasks

| id | task | from | state | needs |
|---|---|---|---|---|
| CACHE-1 | Serialize only what a store keeps | off P2 | DONE `c2cd1586` | — |
| CACHE-2 | D5: each cache checks once, at its first store, whether it can write | off D5 | DONE `c2cd1586` (decided 2026-10-13) | — |
| CACHE-3 | The per-file cache's shard directories made once, by D5's check; the process id read once | off E13 | DONE `c2cd1586` | — |
| CACHE-4 | Add `vendor/phpstan-src/stubs` to the inputs that the stub cache key hashes (defect) | off E15 | DONE `efe621ad` | — |
| CACHE-5 | A map or set of names writes its entries in the order of the names (defect) | off E16 | DONE `480107e3` | — |
| CACHE-6 | The analysed files stored in the per-file cache too | off E4 | SKIP: built, measured and reverted: warm −2% to −5%, cold +9% to +14% on two of ten trees | — |
| CACHE-7 | Each class's assert-tagged method names carried in its `BuiltDecl` | off E12 | SKIP: built, measured and reverted: the 420 M of method decoding moved to the workers, total −23 K | — |
| CACHE-8 | **Write the cache file from one buffer,** instead of building the file in a second `Vec` beside the body (`Writer::finish`, `container.rs`). −15.3 MB at the cold run's peak and −31.4 MB allocated per cold run, measured with the PHPStan base on the code of `5fb858c9`. A memory row; no instruction saving is claimed (`PROFILE_DRILL_PIWIGO_OFF_HEAP.md` §11) | heap H1 | READY | — |
| CACHE-9 | **Names stored once per cache file.** A cache file holds a table of its distinct names, and records refer to them by index; an entry is interned on first need (`OnceLock<Name>`). The per-file cache has one table per file. Format bump | 20 G 6.3 | BLOCKED: its form follows the stub layer and, since decisions 3 and 7, the persistence's format | STUB-7.6; the persistence's format (CACHE-13.2; REG-16.1 kept salsa) |
| CACHE-10 | **Records read, not mapped or copied.** The directory, the keys and the name table are read eagerly. A lazy record is its offset and length, beside a shared `Arc<File>`, read with `read_at` into a per-thread scratch buffer under a decode context. Each record has its own CRC, and failures are recoverable. No `unsafe` and no mapping; `u64` offsets; the stub index's keys are read with the eager sections. Same format bump as CACHE-9 | 20 G 6.5 | BLOCKED: as CACHE-9 | STUB-7.6; the persistence's format (CACHE-13.2; REG-16.1 kept salsa) |
| CACHE-11 | **A cache file's types stored once,** in a table its records index. Warm-cache runs only | off E8 | OPEN | CACHE-9, CACHE-10 (the name table by index, `read_at` records, a decode context: one format bump) |
| CACHE-12 | **A cached file's origins decoded as the `FileId` its file binds** (an origin is `Origin::File(FileId)` since LOAD-4.5, not an `Arc<Path>`). The file's number is kept in `Lazy` beside the encoded bytes: `bind_file` sets it without decoding, and the decode binds the value before any reader sees it. That number is the decode context, so CACHE-11's is not needed. A `Lazy` keeps its bytes until a write, so two values that still hold the same bytes and number are equal with no decode (`changed_since`'s comparison). No deferred-type fact: three later passes read every method in scope, so it would only move the scan's decodes (`ITEMS_1_TO_3_FIXES_PLAN.md`'s R12). The caches' formats stay | off E11; fixes R12 | DONE `6705eedd`. `Lazy` keeps the file's number beside its encoded bytes: `bind_file` sets it without decoding, and `get` binds the value it decodes. A `Lazy` keeps its bytes until `get_mut`, writes them when it is stored again, and is equal to another that holds the same bytes and number with no decode. `Members<FunctionReflection>::bind_file` binds each member so, and `ClassReflection::bind_file` calls it. **Measured** (warm, against `a5a4d85f`; the callgrind against `6016499f`'s warm run, since LOAD-3.3 changes nothing in a warm run): callgrind 13,177,851,490 Ir, −31.8 M; the main thread fell from 651.9 M to 257.1 M. The method decodes by caller, 656.0 M → 623.5 M: `bind_file`'s 393.1 M moved into the declaration merge's bucketing (`for_each_assert_tagged_name`, 398.0 M, on the pool in 32 chunks); `Members::eq` 87.9 → 48.1 M; the deferred-type scan's 177.9 M stays, REG-7's. Perf, one warm recording each: `bind_file` 39 → 4 samples, `merge_by_shard` 55 → 105, the main thread's `Session::load` 60 → 27; the serial stretch in `bind_file` (0.45 to 0.55 s, at 0.9 cores) is gone, and the merge after it runs at 4.65 cores. Wall, warm, 20 pairs (`scripts/abab.py`, load 1.2 to 3.8): medians 1.392 → 1.322 s, paired median −54.3 ms (mean −69.6 ms), 16 of 20 pairs faster, Wilcoxon p = 0.009; peak RSS and CPU time unchanged (p 0.94 and 0.97). Off: 20,806,948,614 Ir, −2.9 M, inside the noise. The Piwigo output was the oracle's in every run. **Tests** (`stub_cache_round_trip.rs`): a stored class's methods take their file's number without being decoded, and one read before the binding takes it too; members read back from the same bytes are equal with no decode, a member written to is compared by its value, and the same bytes bound to other files are not equal; a member read but not written is stored as the bytes it was read from. The lib suite passes (1197) | — |
| CACHE-13 | Warm runs: cache entries checked by `stat`, and the engine's results persisted | 20 G 17; decision 3 | OPEN | — |
| CACHE-13.1 | **Entries checked by `stat`.** A per-project index maps each path to `(size, mtime_ns, ctime_ns, inode, device, content hash)`, from a stat taken before the file's bytes are read. A file, analysed or context, takes the recorded hash without being read when its five stat fields match and both its recorded times are older than the index's own write time (git's rule for racily clean entries, applied to the change time too). That holds only on a file system known to keep a change time that no program sets (ext4, xfs, btrfs, APFS and the like); on any other, and on Windows, every file is read and hashed (decision 13) | 20 G 17.1; decision 13 | OPEN | LOAD-1.4 (the hash the read keeps); for analysed files, CACHE-13.2 |
| CACHE-13.2 | **The engine's results persisted with their dependencies.** Each query's result is saved with the dependencies it recorded, as rustc's incremental compilation saves its query results and its dependency graph. A warm run parses and checks again only the files whose recorded reads changed. The plan's resolved context base and a check-results cache are this persistence, not stores of their own; today an analysed file is always parsed and checked (`project/file_cache.rs:1-8`). It is keyed by the build. The configuration, the version range (decision 6) and each `(path, content hash)` are inputs, not keys. Its format is salsa's `persistence` feature or the cache's own container (CACHE-9, CACHE-10), as REG-16.1 decides | 20 G 17.2; decision 3 | OPEN | REG-16.4 |

### Targets

| task | target now | report's estimate |
|---|---|---|
| CACHE-8 | memory only | −15.3 MB at the cold run's peak, −31.4 MB allocated per cold run |
| CACHE-9, CACHE-10 | item 6's parts (STUB targets) | ≈40 MB of heap at exit, item 6's |
| CACHE-11 | types decoded, ≈436.5 M of the warm run (MEAS-1) | ≈ −65 M; ≈ −25 M after item 6 |
| CACHE-12 | done (`6705eedd`): `bind_file`'s 393.1 M of decodes, serial on the main thread, moved into the merge's bucketing on the pool, and `Members::eq`'s fell by 39.9 M; warm −31.8 M Ir and −54.3 ms of wall time (p = 0.009). The deferred-type scan's 177.9 M of decodes stays, REG-7's | ≈ −116 M, per-file cache warm |
| CACHE-13 | CACHE-13.1: reads, ≈60-70 M Ir; the per-file decode (≈1,260 M, MEAS-1) is not what a `stat` index removes. CACHE-13.2: on the warm run (MEAS-1), the parse of the analysed files (`parse_with_ignores` 1,967.9 M) and their check (`check_program` 6,414.6 M) where their recorded reads did not change, and the per-file decode | decoding and reads, ≈0.85 core-s (item 17's, before decision 3) |

### Notes

- **CACHE-13.1's soundness concern is settled (decision 13).** An mtime shortcut alone is unsound when a same-size rewrite keeps its mtime (`cp -p`, `rsync -c --inplace`); the change time sees every such write, where the file system keeps one that no program sets.
- **CACHE-13 and the per-file decode.** CACHE-13.2's persistence is what replaces the per-file decode of the context files, and the parse and check of unchanged analysed files; CACHE-13.1 does not touch them. CACHE-13.1's weight is syscalls and I/O, which Ir does not count.
- **CACHE-6 (off E4)** measured the idea nearest to CACHE-13 (the analysed files in the per-file cache) and found no gain worth its cold cost. A hit there still parsed the file and checked it. CACHE-13.2 keeps the check's results, so an unchanged file is neither parsed nor checked.
- **CACHE-9 and CACHE-10's form waits on the stub layer and on the persistence.**
  - The crate split reshapes the stub layer, so it decides their form, and with it CACHE-11's. CACHE-12 was built in today's format (`6705eedd`), since its decode context is the file's number.
  - Since decision 7 the stub disk cache goes with STUB-11.
  - Since decision 3 the per-file cache is CACHE-13.2's persistence, whose format REG-16.1 decides. So what is left for CACHE-9 to CACHE-11 is that format, if it is the cache's own container.
- **CACHE-7 (off E12) and REG-5 touch the same set,** `names_with_assert_tags`.

### Records

**CACHE-1** (off P2): DONE `c2cd1586`

- *Status, as recorded:* DONE (c2cd1586; measured: off run 24,498,907,030 Ir against 25,029,730,463 at E1's commit, −530.8 M, output byte-identical; less than the estimated −0.71 to −0.74 G; a read-only cache directory run prints one line per cache and gives the same output)
- *Task:* Serialize only what a store keeps: gate `stub_cache::store`, `StubIndex::store_cached`, `Registry::store_cached` and the stub-data store on D5's write check.
- *Estimated saving:* −1,005.3 M serial; ≈ −0.71 to −0.74 G after P1
- *Needs:* D5's check
- *Verify:* A cold run still checks `store_entries`' duplicate-name test; an unwritable cache directory pays no encoding.

**CACHE-2** (off D5): DONE `c2cd1586`. Decided 2026-10-13, with D1 and D2; its record is PROD-4's. It gates P2 and E13.

**CACHE-3** (off E13): DONE `c2cd1586`

- *Status, as recorded:* DONE (c2cd1586; measured: cold run with the per-file cache on, 26,149,980,863 Ir against 26,174,556,141, −24.6 M, same 11,459 entries in 256 shards, output identical; the mkdir and getpid syscalls are not counted by Ir and were not measured)
- *Task:* The per-file cache's shard directories made once, by D5's check; the process id read once.
- *Estimated saving:* ≈ −80 ms kernel CPU, ≈ −10 ms wall
- *Needs:* D5's check

**CACHE-4** (off E15): DONE `efe621ad`

- *Status, as recorded:* DONE (efe621ad; the hash covers `vendor/phpstan-src/stubs`; the test reads the `rerun-if-changed` paths of `stub-sources/build.rs` and asserts each is in the key's inputs)
- *Task:* Add `vendor/phpstan-src/stubs` to the inputs `crates/phpstan-rs/build.rs` hashes.
- *Verify:* A test that the hash's inputs cover every path `stub-sources`' build script reads.

**CACHE-5** (off E16): DONE `480107e3`

- *Status, as recorded:* DONE (480107e3; found by E4's debug oracle: `file_cache::store`'s debug check that a stored file re-encodes to its bytes fails on any class with several properties, because `NameMap`, `NameSet` and `FoldedMap` wrote their entries in table order, and a map rebuilt from them iterates in another; they write in name order now, the test fills one set in two orders and the mutation of removing the sort fails it; the syntax lib's 270 tests pass; the debug build then ran cold and warm over 215 files with no failure)
- *Task:* A map or set of names writes its entries in the order of the names.
- *Verify:* The unit test; a debug run with the file cache on over a tree with classes.

**CACHE-6** (off E4): SKIP

- *Status, as recorded:* SKIP (not kept: built once and reverted, nothing committed. Judged on ten general trees under `vendor/` with alternating repeats: the warm saving is 2-5% CPU, inside the ±10% spread of one run; a cold run costs +14% on composer and +9% on rector-src for the extra entries written; the cache grows by about 7 KB per file (137 MB on symfony). Not measured: a project whose analysed files change between runs, where hits are rarer and each changed file still pays the write, so it would be no better. The evidence is below; the E4 binary is `~/.cache/prs-profile/p4/e4-phpstan-rs`. Earlier text of this row: Piwigo output byte-identical in cold and warm; a hit for an analysed file reads the entry and parses the tree: warm 14,199,535,864 Ir against 14,223,265,225 without it (−23.7 M, where −250 to −300 M was estimated), cold 25,773,237,521 against 25,360,553,359 (+412.7 M for storing 2,792 more entries, 98 M to 116 M of cache); the decode of an analysed file's entry costs about what the steps it replaces do, so the saving on this one corpus is small. Not decided: how the cold store and the warm hit trade on a project whose analysed files change between runs. A debug oracle that a hit's products encode to the same bytes as a full parse's passed on 215 files.)
- *Task:* The analysed files stored in the per-file cache too.
- *Estimated saving:* ≈ −0.25-0.3 G net, per-file cache warm
- *Needs, and the evidence the row kept with it:* — (debug check that a hit's products encode to the entry's bytes) Ten other trees under `vendor/` (`check-project --no-composer .`, so every file is analysed, E4's best case; the pre-E4 and the E4 binaries, one cold run and the mean of three warm runs each, `nice -n 19`, loadavg not logged; wall time is too noisy to read, laravel's pre-E4 run took 22 s against 8 s cold with E4): output identical in all ten; warm CPU time (user + system) −4% to −10% on eight of them (guzzle, wordpress, psalm, league-commonmark, rector-src, drupal, laravel, symfony), +4% on composer and +14% on doctrine-orm; cold CPU time +0% to +26% (doctrine-orm) where it rose; the cache grew to 2-137 MB (about 7 KB per file). Single runs; `~/.cache/prs-profile/e4/corpora/`. doctrine-orm repeated: four alternating rounds (start order swapped each round), each a cold run and two warm runs per binary, loadavg 0.5-2.7, output identical every round: warm CPU 8.73 s without E4 against 8.56 s with it (−2%, round values 7.5-10.2 s, so inside the noise), cold CPU 9.17 s against 9.27 s (+1%), wall 1.55 s against 1.58 s; the +14% and +26% of the first run were noise; `~/.cache/prs-profile/e4/repeat/`. The other trees that rose, repeated the same way (four alternating rounds, output identical, loadavg in the range given): composer (0.7-1.9) warm CPU 3.88 s against 3.82 s (−2%), cold 3.83 s against 4.35 s (+14%, higher in 3 of 4 rounds); rector-src (1.8-4.7) warm 10.96 s against 10.78 s (−2%), cold 11.39 s against 12.43 s (+9%, higher in 3 of 4); wordpress (4.7-6.0) warm 7.88 s against 7.46 s (−5%), cold 8.08 s against 7.93 s (−2%); psalm (5.8-6.9) warm 23.33 s against 22.85 s (−2%), cold 23.45 s against 23.98 s (+2%, rounds 20.7-27.0 s). So the warm rises were noise and the warm saving is 2-5% on these trees, within the spread of single runs; the cold cost is real on composer and rector-src (the extra entries are written) and absent on wordpress and psalm; `~/.cache/prs-profile/e4/repeat/`.

**CACHE-7** (off E12): SKIP

- *Status, as recorded:* SKIP (not kept: built, measured and reverted, nothing committed; the patch is `~/.cache/prs-profile/e12/e12.patch`. Warm per-file cache, Piwigo, the binary at E10 against the same with E12: total 14,039,357,749 Ir against 14,039,334,519 (−23 K), output byte-identical in cold and warm. The main thread's `insert_built_decl` fell from 471.9 M to 52.1 M, but the 420 M of method decoding moved to the worker threads, because later passes read those methods anyway, so the report's estimate of ≈ −0.35-0.4 G assumed methods that nothing reads later. Wall time, `hyperfine -N`, 8 runs after 2 warmups, three alternating rounds, `nice -n 19`, loadavg 2.6-3.5 throughout: mean 1.806 s (rounds 1.840, 1.745, 1.834) against 1.796 s (1.801, 1.750, 1.838), no difference. Nothing is saved in instructions or in wall time, so by the rule that a saving is claimed only from a measurement the change is not kept)
- *Task:* Each class's assert-tagged method names carried in its `BuiltDecl`.
- *Estimated saving:* ≈ −0.35-0.4 G serial, per-file cache warm
- *Needs:* —

**CACHE-11** (off E8), as recorded: OPEN. A cache file's types stored once, in a table its records index. Estimated saving ≈ −65 M; ≈ −25 M after item 6. Needs item 6, steps 3 and 5.

**CACHE-12** (off E11), as recorded: OPEN. A cached file's origins decoded as the one `Arc<Path>` its file binds. Estimated saving ≈ −116 M, per-file cache warm. Needs E8's decode context.

- *Since:* an origin names its file by number (LOAD-4.5), so the decode context is that number, kept beside the encoded bytes, and E8's was not needed. Built as `6705eedd` (2026-10-17, the fixes plan's R12); the task row has its measurements. The decodes `bind_file` made moved to the first reader that reads every method, the declaration merge's bucketing, which runs on the pool, so the saving is the warm run's serial stretch (−54.3 ms), not its Ir (−31.8 M).

## 12. CHECK: the check phase

### Tasks

| id | task | from | state | needs |
|---|---|---|---|---|
| CHECK-1 | A statement's assigned and defined names walked only when it has `@var` tags | off E2 | DONE `60b0cd33` | — |
| CHECK-2 | A method or static call probed for a PHPUnit assertion only when its written name starts with `assert` | off E3 | DONE `07a0f111` | — |
| CHECK-3 | Nothing reported while an enclosing body has called `extract()` or `eval()` | off E10 | DONE `e081dbe9` | — |
| CHECK-4 | (a) Messages formatted when displayed; (b) class-constant keys built only when dynamic constants are set | off P11 | OPEN: re-opened 2026-10-15. It was built once and reverted on the run's total (−0.65 M); the build's own profile shows the saving, 22.5 M for (a) and 5.1 M for (b), hidden by the schedule's share of the total (its record) | — |
| CHECK-4.1 | **(a) A reported message formatted when it is displayed.** The message is CHECK-6.10's value that displays itself, kept past the report, so a report that is dropped is never formatted: on Piwigo, the 55 `offsetAccess.mightNotExist` messages (`checker/assignment.rs:809`, ≈409 K `Ir` each), of which the level filter drops 22, `rewind_to` 30 and E10's check 3 | off P11 | OPEN | CHECK-6.10 (one message type for both) |
| CHECK-4.2 | **(b) Class-constant keys built only when dynamic constants are set.** `class_constant_type` returns the declared type at once while no dynamic constant is configured; a configured one is keyed by the folded class name and the constant's name, as `ConstantResolver::new` folds them, with no `String` per read | off P11 | READY | — |
| CHECK-5 | The regex scanner's concatenation | off E9 | SKIP: folded into CHECK-6.16 (item 18's F6f, `PROFILE_DRILL_PIWIGO_20G.md:1705`) | — |
| CHECK-6 | The check phase's remainder | 20 G 18 | OPEN | — |
| CHECK-6.1 | **§12.10 of the 25 G report, as designed: observers, and one inference per node.** It deletes the 25 G report's C5's interim `statement_method_call` record, because the statement rules become the first observer family. The PHPStan-API walker becomes its family. A literal's type is computed once per node per pass | 20 G 18.1; 25 G §12.10, 25 G C5 | READY; approved in principle (VAL-2.8) | — |
| CHECK-6.2 | **C1 step 5's tail.** Effects are carried in the statement result, so `noop::expr_side_effects`' walk goes; `has_yield` goes; `collect_throws` gives way to the throw tails C1 keeps | 20 G 18.2; 25 G C1 step 5 | READY | — |
| CHECK-6.3 | **Variables as slots, scopes as radix arrays.** The parser numbers each body's variables, and a variable's `LocalKey` is its slot. Variables that PHP can reach by a computed name or a shared scope keep the name lookup. The scope's maps become a persistent radix array: 32-way, `Arc`-shared, written in place when unshared, iterated in key order | 20 G 18.3 | OPEN | SYN-1 (a variable node holds its name's id and its `VarSlot`) |
| CHECK-6.4 | **C2 step 2:** conditional subjects bound by parameter position (`ConditionalSubject::Parameter { name, index }`), with a format bump | 20 G 18.4; 25 G C2 step 2 | READY | — |
| CHECK-6.5 | **§12.11's rest:** the `MemberIndex`, and the normalized-operand fast path | 20 G 18.5; 25 G §12.11 | READY | — |
| CHECK-6.6 | **C8 step 2's rest: type operations from borrowed parts.** Members are gathered in a per-thread scratch stack and interned from the borrowed slice; a traversal copies on the first change; memos are probed with the borrowed member list; shared values are borrowed, not cloned; `Display for Type` writes into the formatter | 20 G 18.6; 25 G C8 step 2 | READY | — |
| CHECK-6.7 | **A call's argument binding is made once,** as part of the call's record in the pass, and every rule borrows it | 20 G 18.7 | OPEN | CHECK-6.1 (one inference per node) |
| CHECK-6.8 | **Per-function state is reused:** `check_function_impl`'s tables and the expression-type cache are cleared, not freed, between functions | 20 G 18.8 | READY | — |
| CHECK-6.9 | **A generic call's signature is substituted type by type,** not copied whole | 20 G 18.9 | READY | — |
| CHECK-6.10 | **F1: messages formatted only when reported** (37 rules): a subject is a value that displays itself, and an identifier is a `&'static str` | 20 G 18.10 (F1) | READY | — |
| CHECK-6.11 | **F2: constant types built once,** as interned constants; a two-member union or intersection takes its two operands with no `Vec` | 20 G 18.10 (F2) | READY | — |
| CHECK-6.12 | **F6a: rules borrow, iterate, or use scratch** | 20 G 18.10 (F6a) | READY | — |
| CHECK-6.13 | **F6c: throw, exit and loop states copy no scope.** With CHECK-6.3's persistent scopes a copy is a pointer copy, and the per-try and per-loop vectors are scratch stacks | 20 G 18.10 (F6c) | OPEN | CHECK-6.3 |
| CHECK-6.14 | **F6d:** the narrowing inserts and the `Rc<Vec<..>>` implied facts live in CHECK-6.3's radix arrays | 20 G 18.10 (F6d) | OPEN | CHECK-6.3 |
| CHECK-6.15 | **F6e:** the separate walks' vectors go with the walks, which become families of CHECK-6.1's traversal | 20 G 18.10 (F6e) | OPEN | CHECK-6.1 |
| CHECK-6.16 | **F6f: analyses that are a function of a text are made once per distinct text,** scanning bytes and keeping slices: regex groups, `preg` shapes, DQL, SQL query-builder rendering, format strings. It includes the off report's E9 | 20 G 18.10 (F6f); off E9 | READY | — |
| CHECK-6.17 | **F4: third-party parsers.** The SQL extension parses a query's text once per distinct text, and regex matches are read through one reused `Captures` | 20 G 18.10 (F4) | READY | — |
| CHECK-6.18 | **One checker per worker,** reset for each file | 20 G 18.10 | READY | — |
| CHECK-7 | **A constant's throws, where PHP evaluates it.** Each place inherits REG-7.6's outcome for the constant. A definite throw makes the code after it unreachable; a possible one is a throw point that the `try`/`catch` and `@throws` rules see. The places: a read and `defined()`; a parameter's default, where the argument is omitted; a static variable's initializer; a global `const`; an attribute's arguments, where they are read; and every operation that initializes the class's constants. Those are `new` of the class, of a subclass, of an implementer of an interface or of a user of a trait that holds the cause; `newInstanceWithoutConstructor`; a static property read; `getConstants`, and `getConstant` of any name; a backed enum's case access, `from()` and `tryFrom()` (every constant of the enum); and its `cases()` (the case values). The list is read from php-src when the row is built, not from the probes of §2.4 | decision 1 | OPEN; a delta, approved (VAL-2.5) | REG-7.6 |
| CHECK-8 | **A PHPDoc type that leaves out a kind its native type names, reported at the tag, off by default:** `@param int` on `int\|string`, `@param Foo` on `Foo $x = null`, `@return Foo` on `?Foo`. Under decision 8 the kind is kept, so nothing else reports the omission. The check compares the PHPDoc's members with the native kinds as REG-7.5 forms them, with an identifier of its own and a configuration switch | decision 8 | OPEN | REG-7.5 |
| CHECK-9 | **A dead catch: a caught type that no throw point of the `try` block can be** (`catch.neverThrown`, level 4, beside `catch.alreadyCaught`). The checker already decides it: a catch that no throw state reaches is not merged (`catch_live`, `checker/stmt/try_catch.rs:235`), but it is not reported. The comment at `:126` says this half needs a per-callee throw model, which the catches have (`call_throw_type`, `checker/throws.rs:105`): a user function's declared `@throws`, else anything, and a builtin anything, since it can run user code; `throw` its value; a division `DivisionByZeroError`. REG-17.2 then decides the catches around undocumented calls | decision 9 | READY | — |
| CHECK-10 | **The engine read over the version range.** Where PHP behaves differently across the range's versions (a loose comparison of a number and a non-numeric string, a division by zero, arithmetic on a non-numeric string), an expression's type is the union of its types at each version, and what only some versions do is possible in part of the range: over 7.4 to 8.5, `0 == 'a'` is `bool`, and `1 / 0` warns on 7.4 and throws `DivisionByZeroError` from 8.0. Today the checker reads one version (`--php-version`): `0 == 'a'` is always true at 7.4 and always false at 8.5 | decision 18 | OPEN | STUB-7.1 (the range) |
| CHECK-11 | **C3a's `property.unused` delta:** ORM-mapped properties count as used while ext-doctrine is active, and view objects passed to a template renderer aren't reported. At HEAD an entity's mapped private properties are `property.unused` with `ext.doctrine.enabled=true`. The approved gate file `gates/approved/c3-provenance-declarations` is reviewed again with it | split C3 | READY; a delta | — |

### Targets

| task | target now | report's estimate |
|---|---|---|
| CHECK-4.1 | 22.5 M: `report_missing_offset`'s 55 formats, gone in the build's profile | 22.5 M |
| CHECK-4.2 | 5.1 M: `class_constant_key`'s 6,857 formats, gone in the build's profile | 6.6 M |
| CHECK-6 | `check_program` 6,378.6 M; `infer_expr` 1,051.9 M (800 K calls), `check_args` 517.4 M, `method_call_return_type` 448.8 M, `apply_narrowing_unguarded` 467.4 M, `infer_binary` 246.6 M, `merge_scopes` 222.0 M, `call_bindings` 133.2 M, `find_var` 123.2 M, `OrdMap::get` 71.4 M, `Locals::clone` 33.5 M, `union_many` 263.9 M, `reduce_array_members` 146.8 M, `resolve_call_args` 63.4 M, `string_literal_ref` 91.3 M, `sqlparser` 88.0 M | ≈1.6-2.0 G (§12.10 alone ≈1.0-1.3 G, slots and radix scopes ≈0.15-0.25 G) |

### Notes

- **CHECK-1, CHECK-2 and CHECK-3 (off E2, E3, E10) cut small parts** of the check phase.
- **CHECK-6.10 and CHECK-4.1 are one message type.** CHECK-6.10 formats only what is reported; CHECK-4.1 keeps the same value unformatted past the report, until it is displayed. The build of CHECK-4 used a closure of its own (`report_lazy`); built with CHECK-6.10, the two are one change.
- **CHECK-6.1 is approved in principle (VAL-2.8).** Each family's output diff is read at VAL-1.
- **CHECK-7 is semantic,** not an optimization: decision 1's throws, at each place PHP evaluates a constant. Its output changes are VAL-2.5's.
- **Memory (MEAS-2):** the check phase allocates 232 MB in 2.16 M blocks in `checker`, and holds 0.6 MB of it at the end: the temporaries of item 18.

### Records

**CHECK-1** (off E2): DONE `60b0cd33`

- *Status, as recorded:* DONE (60b0cd33; measured: off run 23,812,168,909 Ir against 23,866,820,730 at P4's commit, −54.7 M against the estimated ≈ −50 M, Piwigo output byte-identical to the base's; the same binary also carries E14's parser change, which changes nothing on Piwigo; tests, clippy and fixtures not run)
- *Task:* A statement's assigned and defined names walked only when it has `@var` tags.
- *Estimated saving:* ≈ −50 M
- *Needs:* —

**CHECK-2** (off E3): DONE `07a0f111`

- *Status, as recorded:* DONE (07a0f111; measured: off run 23,785,065,626 Ir against 23,812,168,909 at E2's commit, −27.1 M against the estimated ≈ −35 M, Piwigo output byte-identical; tests, clippy and fixtures not run)
- *Task:* A method or static call probed for a PHPUnit assertion only when its written name starts with `assert`.
- *Estimated saving:* ≈ −35 M
- *Needs:* —

**CHECK-3** (off E10): DONE `e081dbe9`

- *Status, as recorded:* DONE (e081dbe9; measured: off run 23,619,596,541 Ir against 23,672,079,898 at E7's commit, −52.5 M against the estimated ≈ −35 M, Piwigo output byte-identical; `report` and `report_builder` return at once while the body's own flag or an enclosing body's is set (`enclosing_dynamic_bodies` counts the saved flags that are true), and `check_statement_var_tag_names` returns before formatting; the other pushes to `diagnostics` stay and are truncated as before; a debug build reports as before and records each report made while dropping, prunes the record at every truncation and rewind, and asserts at `into_diagnostics` that none is left, which fails when one prune is removed; seven new tests (extract, eval, a function declared in such a body before and after the call, a body after one, a script, a method before a top-level `extract`) and the six `extract` fixtures pass; the workspace tests and clippy not run)
- *Task:* Nothing reported while an enclosing body has called `extract()` or `eval()`.
- *Estimated saving:* ≈ −35 M on Piwigo
- *Needs:* — (debug assertion that the truncation drops exactly what the count skipped)

**CHECK-4** (off P11): OPEN, re-opened 2026-10-15

- *Status, as recorded:* SKIP (not kept: built, measured and reverted, nothing committed. Both parts built: (a) `Diagnostic.message` became a `Message` that dereferences to `str` and can be made by a closure on first read, with `report_lazy` used by the `offsetAccess.mightNotExist` report, and (b) `class_constant_type` returning the declared type while `dynamic` is empty. Off run 23,215,388,146 Ir against 23,216,039,715 at P7's commit, −0.65 M against the estimated 22.5 M + 6.6 M, Piwigo output byte-identical. The 22.5 M was 55 messages at ≈ 409 K Ir; the cause is not traced; the likely one is that E10 now drops, before formatting, the reports made inside bodies that call `extract()`, where B3 found most of the 55 were discarded. The one annotated `report_missing_offset` call line in the P7 profile costs ≈ 12 K Ir. What is left of (a) formats a message only for a rare setting (`reportPossiblyNonexistentGeneralArrayOffset`), at the price of a new message type touching 76 readers, so it is not kept without a measurement that shows it)
- *Task:* (a) A reported message formatted when displayed (a template and its operands in `Diagnostic.message`); (b) class-constant keys built only when a dynamic constant is configured.
- *Estimated saving:* 22.5 M (a), 6.6 M (b)
- *Needs:* —
- *Verify:* A message's text must not depend on state that changes after the report; the baseline and `ignore_errors` compare the displayed text; (b) folds the class name as `ConstantResolver::new` does.
- *Traced, 2026-10-15, from the saved profiles, with no new run* (`~/.cache/prs-profile/p7`, `p10`, `p11`; the scripts are in `~/.cache/prs-ledger/check4/`):
  - *The saving was made.* In the build's own profile, `report_missing_offset` falls from 56.2 M to 33.8 M inclusive over the same 7,957 calls: its 55 calls of `format_inner` (22.5 M) are gone. `class_constant_key`'s 6,857 formats (5.1 M) are gone too. Nothing before the build had removed them: at B3's base, at E10's commit and at P7's, `report_missing_offset` costs 56.9, 55.7 and 56.2 M over those 7,957 calls.
  - *What hid it.* Against P7's run, the program's own functions fall by 25.3 M and rise by 9.8 M, and the allocator's rise by 14.6 M. The rises are work that follows the threads' schedule: mimalloc's cross-thread reclaim (`mi_free_try_collect_mt` +5.8 M, `mi_abandoned_page_try_reclaim` +5.4 M, `_mi_page_free_collect_partly` +4.7 M) and the per-thread caches (`Interner::name_hashed`, `NameCache::insert`, the doc-block parse). The same three allocator functions moved −6.4 M from P7's run to LOAD-6's and +22.4 M from LOAD-6's to this one, and the name interner ±4 M; none of these changes touches them. The total of one off run doesn't resolve a change of this size.
  - *E10 doesn't cover these reports.* `report` checks `reports_are_dropped()` after the caller has formatted the message (`checker.rs:1500`). In the build, 3 of the 55 lazy messages are dropped by that check, inside `report_missing_offset`.
  - *The record's "a rare setting" is wrong.* The 55 are `MayBeAbsent` or `Absent` presences (`checker/assignment.rs:795-803`), formatted under the default settings; Piwigo's configuration doesn't set `reportPossiblyNonexistentGeneralArrayOffset`.
  - *Where the 55 go (Q-6).* The build's closures are dropped 33 times: 3 by E10's check, and 30 as diagnostics, all by `rewind_to`. It is the only path that drops a level-7 diagnostic before `finalize`, since the level filter runs before the ignore filters, and `finalize` filters a clone of the results, whose originals live until the process exits. The other 22 reach `finalize` and are dropped by the level filter, as B3 counted. So the 33 that B3 found discarded are 30 in a convergence pass's `rewind_to` and 3 in bodies that call `extract()`, and P11a's truncation saving is ≈12.3 M, not "well under 1 M".
  - *State.* The SKIP rested on the total, so the row is re-opened: CHECK-4.1 with CHECK-6.10, CHECK-4.2 ready.

**CHECK-5** (off E9), as recorded: SKIP (folded into the plan's F6f, `PROFILE_DRILL_PIWIGO_20G.md:1705`). The regex scanner's concatenation.

## 13. VAL: validation

### Tasks

| id | task | from | state | needs |
|---|---|---|---|---|
| VAL-1 | **The end validation:** dumps, an `oracle` profile, an edit replay, the server session | 20 G 0, 20 G §11 | OPEN | — |
| VAL-1.1 | **The starting state read:** the vendor-tree, all-on and unscoped reports at HEAD compared with the last validated state, and each difference read | 20 G 0.1 | OPEN | — |
| VAL-1.2 | **An `oracle` profile** (`gate` with `debug-assertions` and `overflow-checks`), run once over the gate's three configurations, each vendor project with every rule set, and the edit replay. A temporary counter shows that every oracle site ran; failures are fixed in their own commits | 20 G 0.2 | OPEN | — |
| VAL-1.3 | **The debug oracles, and the code only they use, are deleted:** `FileBodies` stops storing statements; the debug-only re-exports go, and so do the test oracles with a second implementation | 20 G 0.2 | OPEN | VAL-1.2 |
| VAL-1.4 | **One canonical type printer** in `types`, over all of `Repr`, with printers that cannot lose a field unseen | 20 G 0.3 | OPEN | — |
| VAL-1.5 | **O1:** `--dump-front-end`, per file and role, through the project's own `parse_file` | 20 G 0.3 | OPEN; `xtask front-end-dump` exists, and hashes only | — |
| VAL-1.6 | **`registry-dump`,** taken after the finishing passes | 20 G 0.3 | OPEN | VAL-1.4 |
| VAL-1.7 | **The checker's records:** every PHPDoc resolution, and every expression's inferred and native type (`--dump-expr-types`) | 20 G 0.3 | OPEN; `--dump-types` exists | VAL-1.4 |
| VAL-1.8 | **O2:** the gate's full artifact set, with a view of every field of every diagnostic, over Piwigo, each vendor project and the fixtures; with the stub cache cold and warm, and the per-file cache off, written and read | 20 G 0.3 | OPEN; `cargo xtask corpus --full` exists | — |
| VAL-1.9 | **The server:** an in-process session over Piwigo and two vendor projects (files and roles, `registry-dump`, diagnostics with UTF-16 ranges, code actions, definition, hover, references) | 20 G 0.3 | OPEN | — |
| VAL-1.10 | **The edit replay** for Piwigo and each vendor project, and a fresh load of the edited tree compared with it | 20 G 0.3 | OPEN | — |
| VAL-1.11 | **The dhat profile by holder,** once more with untrimmed backtraces | 20 G 0.3 | OPEN; MEAS-2 has the whole-stack profile at this code | — |
| VAL-1.12 | **The baseline,** taken from the commit that adds the printers and makes the deletions: the `gate` build at one thread and at the default count (byte-identical), and the `oracle` build | 20 G 0.4 | OPEN | VAL-1.3 to VAL-1.11 |
| VAL-1.13 | **After item 19:** `cargo test --workspace` and the fixtures (`--test fixtures`); item 0's dumps taken again with the `gate` build and the `oracle` build, which must finish without an assertion, and every difference from the baseline named as one of VAL-2's, listed with its file and read; O2 compared with the baseline's in output order (`xtask compare`), saved as `.corpus-runs/<scope>/<hash>.txt`, and its diff read line by line | 20 G §11 | OPEN; the last step of the 20 G plan | — |
| VAL-1.14 | **The equalities that need no baseline:** one thread against the default; the stub cache cold against warm; the per-file cache off against written and read; a warm run against a cold one (CACHE-13); the relation memo on against off (TYPE-4.4); the server's first dumps against the CLI's (LOAD-1); each edit replay against a fresh load of the edited tree | 20 G §11 | OPEN | — |
| VAL-1.15 | **O3, the measurements:** callgrind in `cl-20g`'s profile; a sampling profile of a cached run, for the serial windows; `hyperfine`, cold and warm, alternating with the starting commit's `gate` binary under `nice`, with the load average noted; the dhat heap profile in the 20 G plan's §8 build, compared by holder with §8's at the peak and at exit, and its allocation count; the server's heap on item 0's in-process session (VAL-1.9); the results in `WORK_STACK.md` | 20 G §11 | OPEN | — |
| VAL-2 | The output-changing rows | 20 G §11; split | PART: VAL-2.1 to 2.9 decided 2026-10-15 (VAL-2.5 extended by decision 1, VAL-2.9 by decision 14); VAL-2.10 is read as its rows land | — |
| VAL-2.1 | A file's identity is its resolved path; links, roles and the server read as the CLI does (LOAD-1) | 20 G 1 | DONE: approved | — |
| VAL-2.2 | A tree with a link cycle is listed once (LOAD-2.2) | 20 G 2 | DONE: approved | — |
| VAL-2.3 | The order in which code iterating a class's methods meets them: declaration order (REG-7.4's member list, REG-13.3's before decision 3) | 20 G 4 | DONE: approved | — |
| VAL-2.4 | Guards decided by the stubs' availability, on run-time evidence; a new polyfill-signature check (STUB-8) | 20 G 6.2 | DONE: approved, both parts | — |
| VAL-2.5 | Constants on a cycle read `never` and are reported; PHPDoc-only cycles keep every possibility. Extended by decision 1 (REG-7.6, CHECK-7): a constant's evaluation yields its value type and its throws, definite or possible. A cycle over value edges, or arithmetic that fails on literal operands, makes every read `never` and is reported at the constant; an unknown constant or class keeps the value unknown and the throw possible. A constant whose PHPDoc reaches a cycle keeps its type without it, and the PHPDoc is reported as unresolvable. Every place PHP evaluates the constant inherits the throws | 20 G 7; decision 1 | DONE: approved; decision 1 withdrew REG-7.6's `mixed` in its favour | — |
| VAL-2.6 | A `@property` tag shadowed by a declared property is checked against the tag's own names, which the record of written names holds at the tag (REG-14.1, PROD-5.8) | 20 G 8; decision 2 | DONE: approved | — |
| VAL-2.7 | Container-field keys canonical as PHP's array keys (NAME-1.2) | 20 G 15 | DONE: approved | — |
| VAL-2.8 | The rule families of §12.10: observers, one inference per node (CHECK-6.1) | 20 G 18 | DONE: approved in principle; each family's output diff is read at VAL-1, and anything that is not an improvement is fixed or reverted per family | — |
| VAL-2.9 | **The crate split's output changes, each on its own row:** REG-4.3, REG-4.4 and REG-4.8 (done; see their records); REG-4.2, REG-4.14, REG-5, REG-6, REG-7 (REG-7.2's order for a method two nested traits both supply, REG-7.5's native and PHPDoc combination where decision 8 keeps the plan's, VAL-2.10 having the rest; REG-7.6's constants are VAL-2.5's since decision 1), REG-8.1, REG-8.5, REG-9 and REG-11; STUB-7.1, 7.2, 7.3, 7.5, 7.9 and 7.10. The plan states each as a change of its design, most with a fixture, and lists some as approved deltas; this ledger lists them so that VAL-1.13 can name them | split | DONE: approved (decision 14), STUB-7.2 and REG-8.5 corrected; each is read at VAL-1 as it lands | — |
| VAL-2.10 | **The decisions' output changes** beyond VAL-2.5, 2.6 and 2.9: PHPStan's classes read from the PHPStan the project installs (STUB-13); the builtin knowledge from php-src's stubs and real binaries' dumps (STUB-12), combined by decision 10: the claims of the function map, the `.stub` files and the unverified PHPDoc intersected within what php-src verifies, where PHPStan nests them, a php-src function's PHPDoc return type narrowing its native one, and the `@throws` and impure marks of two sources united; a file that fails to parse keeping what parses (SYN-1.9 with Q-10); decision 8's combination where it departs from the plan's (REG-7.5, REG-7.12): a PHPDoc member intersected with its native kind (`Closure(int): string`, `T&Foo`), a class kind kept (`string\|array\|A`'s `A`), every array member but a shape giving `iterable`'s `Traversable` its key and value types, a kind no branch of a conditional covers kept, a template member judged by the relation, a resource under `object` dropped; `iterable<K, V>`'s array keys (TYPE-5); CHECK-8's reports where it is turned on; decision 9's: returns and throws inferred where none are declared (REG-17.1, 17.2), a context declaration's refuted return or property refinement widened (REG-17.3), receiver types in the body summaries (REG-17.4), and the dead catch (CHECK-9); decision 18's: the default range narrowed to the versions the analysed files compile on, a range below 7.0 read from 7.0 with a notice, and the engine read over the range (CHECK-10); decision 17's: an inferred return keeping the literals its body returns; decision 16's: below 8.0, a builtin's failure value after an argument its parameter parsing may refuse; decision 15's: a file's declarations available only in the versions its syntax holds on, an include or an autoloaded use outside them reported unless a guard covers it, and an analysed file reported at the constructs a version of the range refuses; decision 11's: no declaration's members merged into another's (a guarded polyfill's onto a built-in, a second unguarded declaration onto the first, an override onto a built-in unless it is in a stub file the project names), a name that several declarations may bind checked against each, and an unguarded redeclaration of a built-in bound only where the built-in is unavailable. Each is listed with its file and read at VAL-1 as it lands | decisions 4, 6, 7, 8, 9, 10, 11, 15, 16, 17, 18 | OPEN: read at VAL-1, as they land | — |

### Notes

- **VAL-1 is not a precondition for any row.** On this branch the rows were validated one at a time: output byte-identical, a debug oracle per row, the touched module's tests. VAL-1 is what the 20 G plan's own end validation needs.
- **The decisions of VAL-2.1 to 2.8 were made one at a time in the session, on 2026-10-15.** Each difference is listed with its file and read at VAL-1.
- **VAL-1.12's baseline.** The plan wrote that the baseline commit's release behaviour is `5fb858c9`'s. On this branch it is that of the commit the baseline is taken on: the off drill's rows (output identical) and the crate split's C4 commits (some output changes) lie under it.
- **VAL-1.13's differences** can also be the crate split's output changes (VAL-2.9) and the decisions' (VAL-2.10), if they land after the baseline.

## 14. MEAS: the measurements that set targets

MEAS-1, MEAS-2 and MEAS-3 measured `791d29cb`'s code, and MEAS-4 `aa896697`'s.

| id | measurement | state | used by |
|---|---|---|---|
| MEAS-1 | Warm-mode costs at this branch's code | DONE | the targets of item 6 (REG-7.2, STUB-8, STUB-9, CACHE-9, CACHE-10), CACHE-11, CACHE-12, CACHE-13 |
| MEAS-2 | Memory at this branch's code | DONE | the memory claims of LOAD-3, REG-13, SYN-1, DOC-2, NAME-2, LOAD-5 |
| MEAS-3 | **The spread of one off run's total:** two callgrind runs of one binary, and two at one thread (`PHPSTAN_RS_THREADS=1`), each split by function as CHECK-4's record splits P7's run from the build's | DONE (2026-10-15): up to 16.9 M at the default thread count, 1.1 M at one thread (its section below) | the rule on what a total can resolve (Conventions); the changes of §1's table that are tens of M: CHECK-1 (−54.7 M), CHECK-2 (−27.1 M), TYPE-1 (−59.9 M), TYPE-2 (−23.2 M), TYPE-3 (−29.9 M), CHECK-3 (−52.5 M) |
| MEAS-4 | **Wall time at `aa896697`:** the off and warm runs' timelines under perf, the cores they leave unused, the scaling curve and its CPU growth, instructions per CPU second, and the allocator's purging and huge pages (`ITEMS_1_TO_3_FIXES_PLAN.md`'s Step 0) | DONE (2026-10-17) | LOAD-8, LOAD-3.3 and CACHE-12; REG-7's and STUB's notes; the rule that a wall-time claim is a median of repeated runs |

### MEAS-1: the warm run, 13.18 G Ir

Threads summed. "Target" is inclusive cost in millions of instructions (calls in brackets): the most a row could remove.

| phase | Ir | notes |
|---|---:|---|
| `check_files` (`check_program`) | 6,414.6 M (2,792 files) | the same as off mode (6,378.6 M): the cache does not touch the check |
| `read_and_parse_file` | 3,802.1 M (14,251 files) | the 2,792 analysed files are parsed (`parse_with_ignores` 1,967.9 M); the 11,459 context files are decoded from the per-file cache |
| `run_finishing_passes` | 989.6 M | |
| main thread | 869.5 M | `insert_numbered_file` 15,266 calls; of its `insert_built_decl` (467.1 M), 414.7 M is stub decodes it forces |

Targets this run reads:

- **Per-file cache decode, 11,459 files: ≈1,260 M** (≈110 K per file).
  - The decode: `next_element::<FileBodies>` 502.0 M, `<Vec<BuiltDecl>>` 455.6 M, `<Vec<RefKey>>` 71.5 M (22,918) plus 25.7 M collecting them into sets, `Arc<LineIndex>` 51.0 M, `FileBodies::with_source` 37.6 M, `crc32fast::hash` 30.4 M, `DeclaredClass` 29.8 M, `DuplicateClassSites` 12.8 M.
  - Reading the files is cheap in Ir: `std::fs::read` 15.2 M (14,251), `File::open` 4.3 M, the two content hashes 30.4 M + 17.1 M.
- **Item 6's stub scope (REG-7.2, CACHE-9, CACHE-10): 87,146 stub function records decoded, 682.5 M** (of 107,571 inserted into the stub maps).
  - What forces them: 414.7 M (51,750) by `insert_built_decl` (15,266 declarations merged onto stub entries), 177.1 M (22,746) by `resolve_deferred_value_of`, 87.5 M (12,105) by `Members::eq`.
  - Deciding not to force these is what the item's step 1 (deferred types on demand) and the stub scope do.
  - The `Lazy` records' own deserialize is 27.3 M (87,580).
- **CACHE-11 (off E8), types decoded: ≈436.5 M,** attributed under `Repr`'s visitor: 1,177,481 `visit_enum` calls, 62.3 M self; `types::intern::insert` 35.4 M, `Vec<Type>` 29.4 M, `from_utf8` 27.8 M, `Type::new` 22.7 M, `name_hashed` 22.2 M, `exact_hash` 18.8 M.
  - `Type::deserialize` is entered 77,540 times at the top.
  - That is the pool the old estimate (≈ −65 M, ≈ −25 M after item 6) took a part of; no new saving is claimed.
- **CACHE-12 (off E11), origins decoded: 123.9 M,** for 144,820 `Provenance` records (`Origin::try_from` 4.1 M). The old estimate was −116 M.
- **CACHE-13 (item 17), reads by `stat`: the reads it would replace cost ≈60-70 M in Ir** (`fs::read` 15.2 M, `File::open` 4.3 M, `crc32` 30.4 M, `xxh3` 17.1 M).
  - The item's weight is syscalls and I/O, which Ir does not count.
  - The decode (≈1,260 M above) is not what a `stat` index removes.
  - Its soundness concern (the mtime shortcut) is settled on the row (decision 13).

### MEAS-3: the spread of one off run's total

The P8b binary (`791d29cb`), off mode, Piwigo, callgrind, one run after another under `nice -n 19` (load average 3.6-7.6, from other work on the machine). The runs and the split are in `~/.cache/prs-profile/meas3/`, and the scripts in `~/.cache/prs-ledger/meas3/`.

| run | threads | Ir |
|---|---|---:|
| P8b (`p8b/cg`) | default | 20,928,053,254 |
| t-a | default | 20,925,124,097 |
| t-b | default | 20,911,186,921 |
| one-a | 1 | 20,884,565,808 |
| one-b | 1 | 20,883,509,523 |

- **At the default thread count** the three totals span 16.9 M (sample standard deviation 11.4 M). What moves, split by function as CHECK-4's record does:
  - mimalloc, up to 20 M between two runs: its cross-thread frees and page reclaim (`mi_free_try_collect_mt`, `mi_abandoned_page_try_reclaim`) and its page and arena searches;
  - the per-thread caches: `Interner::name_hashed` ±3.7 M, and the work whose share depends on which thread meets an item first (`DocBlock::merge_from` ±1.5 M, the memo tables' rehashes ±0.8 M).
- **At one thread** the two totals differ by 1.1 M, all of it mimalloc's. Such a run is ≈40 M lower: no thread repeats another's cache fills (`name_hashed` −16.8 M, `NameCache::insert` −21 M), though mimalloc's own page searches cost ≈57 M more.
- **Every run gave the same output**, 581 diagnostics over 2,792 files.
- **What a total resolves.** All six rows of §1's table that are tens of M exceed the spread. CHECK-2 (−27.1 M) and TYPE-2 (−23.2 M) exceed it by less than twice, so each one-run total gives their sign but not their size; the functions each changed are what their records measured.

### MEAS-2: the heap

dhat counts requested bytes, not resident size. "Peak" is dhat's global peak (`t-gmax`); "exit" is what the leaked project holds after the check. The items are the 20 G plan's, mapped from the allocating function by `analysis/items.py`; `P6` is the stub build.

| run | allocated | blocks | peak | at exit |
|---|---:|---:|---:|---:|
| off, this code | 3,003.5 MB | 19.52 M | 808.7 MB | 655.0 MB |
| off, `5fb858c9`'s code (`dhat-heap-modes/off`) | 3,543.6 MB | 24.87 M | 996.5 MB | 714.1 MB |
| warm (both caches), this code | 1,694.8 MB | 12.86 M | 545.5 MB (at exit: the peak is the end of the run) | 544.1 MB |
| warm, `5fb858c9`'s code, per-file cache off (`dhat-heap-modes/warm`) | 2,669.4 MB | 19.44 M | 866.6 MB | 657.8 MB |

The two warm rows differ in the per-file cache as well as in the code, so only the off rows compare the code alone: −540 MB allocated, −188 MB at the peak, −59 MB at exit.

By item (MB; peak / exit), at this code in the off and warm runs, against the off run at `5fb858c9`:

| item | off now | off then | warm now | what changed |
|---|---|---|---|---|
| 9 (SYN-1), trees | 399.1 / 159.6 | 580.3 / 160.2 | 151.1 / 151.1 | P8 cut 181 MB from the off peak (context bodies are skimmed); the trees that stay are the analysed files' (≈151 MB) |
| 4 (REG-13), registry and passes | 47.4 / 87.7 | 45.6 / 149.6 | 46.7 / 46.7 | P9 removed 62 MB at exit in the off run |
| 14 (DOC-2), PHPDoc | 75.2 / 94.9 | 75.2 / 95.9 | 38.3 / 38.3 | unchanged (P7 saves allocations, not retained bytes) |
| 13 (PROD-5), a file's products | 106.7 / 105.0 | 81.7 / 79.8 | 42.6 / 42.6 | up 25 MB: P8a keeps the skimmed bodies' text |
| 16 (NAME-2), interned values | 14.4 / 32.5 | 14.4 / 32.7 | 31.9 / 32.0 | unchanged |
| 6, stub records | 1.2 / 2.2 | 0.0 / 1.0 | 109.2 / 109.2 | in the off run the stub build is `P6`: 94.3 / 94.3 now, 132.8 / 111.8 then |
| 10 (SYN-2), tokens | 13.0 / 12.8 | 13.8 / 13.7 | 3.6 / 3.6 | |
| 2 (LOAD-2) | 5.5 / 2.6 | 5.5 / 2.6 | 2.8 / 2.6 | |
| 3 (LOAD-3) | 9.7 / 0.0 | 9.0 / 0.0 | 0.0 / 0.0 | |
| 5 (LOAD-4) | 11.5 / 18.6 | 11.0 / 20.3 | 18.6 / 18.6 | |

What this does to the plan's memory claims:

- **LOAD-3 (item 3; peak −≤209 MB):** the off peak holds 399.1 MB of trees (was 580.3 MB), of which 151 MB are the analysed files' trees, as the warm run shows. The claim is a ceiling that fell by P8's 181 MB; it is not re-estimated here.
- **REG-13 (item 4; ≈90 MB at exit):** item 4 holds 87.7 MB at exit in the off run (was 149.6) and 46.7 MB in the warm run. The claim is stale by about half.
- **SYN-1 (item 9; ≥53 MB of the analysed trees) and LOAD-5 (item 19; ≈173 MB freed during the check):** the analysed trees are 151.1 MB at exit and at the peak of the warm run.
- **DOC-2 (item 14; ≈50 MB):** 94.9 MB at exit in the off run, 38.3 MB in the warm run.
- **NAME-2 (item 16; ≈15 MB):** 32.0-32.5 MB held.
- **Item 6 (≈40 MB of heap):** the stub records are 109.2 MB in the warm run.
- A warm run's peak is its end: nothing is freed during the check, which is what LOAD-5 (item 19) is about.
- The check phase allocates 232 MB in 2.16 M blocks in `checker`, and holds 0.6 MB of it at the end: the temporaries of CHECK-6 (item 18).

### MEAS-4: wall time at `aa896697`

`ITEMS_1_TO_3_FIXES_PLAN.md`'s Step 0 (2026-10-17) holds the tables, the commands and the scripts (`scripts/`); this section keeps what the rows read. Piwigo, `check-project --config phpstan-rs.toml --format json`; every kept output was the same, 171,439 bytes with sha256 `8ff144c3…`. The machine is a VM: 8 vCPUs on a 6-core, 12-thread i7-8700, so when all eight run, at least two cores run two each. It has no hardware performance counter and no cpufreq, so clock speed and cache or memory contention cannot be told apart. The 1-minute load was 1.5 to 2.5, with other sessions' processes present, so the figures bound what a row gains.

| | off | warm |
|---|---|---|
| plain runs (the frame-pointer `dhat` binary, no perf) | 2,310, 2,296, 2,878 ms | 1,769, 1,602, 1,568 ms |
| the run under `perf record -F 499 -g` | 2.02 s | 1.70 s |
| the main thread's CPU | 0.22 s | 0.21 s |
| the 8 cores' unused capacity | 0.75 s | 0.85 s |

**Where the main thread gates the run** (its own samples; the other stretches are the pool's):

- vendor discovery: blocked in `vendor.get()` 168 ms off, while the stub build keeps the pool at 5.5 cores, and 120 ms warm, with the pool idle (LOAD-8.10);
- adding the vendor roots after it: 14 ms off, 16 ms warm (LOAD-8.14);
- the hand-over after the parse: 8 ms in each (LOAD-8.4);
- `Stubs::base`: 89 ms off, with a cold stub cache; about 9 ms warm (STUB's notes);
- `insert_numbered_file`: 82 ms off, 72 ms of it freeing the context trees (LOAD-3.3); 8 ms warm;
- `bind_file`: 10 ms off; 110 ms warm, with the pool idle (CACHE-12).

The streaming parse runs at 6.0 to 6.9 cores off (0.22 to 0.90 s), and the check at 5.9 to 7.6 (1.52 to 2.02 s). The project's finishing passes run at 2 to 3 cores (1.16 to 1.52 s off, 0.75 to 1.19 s warm) and leave the most capacity unused, about 0.26 s and 0.28 s (REG-7's notes).

**Instructions are not time.** Leaf samples against callgrind's self Ir for the same functions: allocation, copies and the lexer's name work run at 1 to 5 G Ir per CPU second (`_mi_theap_malloc_zero` 1.0 G, `Lexer::next_token` 4.6 G), and the byte loops at 10 G or more (`Utf8Chunks::next` and `Components::next` 12 G). So LOAD-8.1 to 8.9 and 8.13 are instruction rows: they serve the Ir total and save a few ms of wall time each, and 8.9 none in a batch run. The wall rows are LOAD-3.3, CACHE-12, LOAD-8.10, 8.14 and 8.15.

**Scaling** (the `gate` binary, hyperfine, 5 runs per point, load 2.6 to 3.5). Medians at 1, 2, 4 and 8 threads: off 6,657, 3,769, 2,597 and 2,123 ms; warm 4,393, 2,530, 1,847 and 1,539 ms. The CPU time grows 70% off and 61% warm from one thread to eight, so a fit of serial + parallel / threads does not measure serial work. Two perf recordings of the off run, at one thread and at eight, split the growth (6.24 to 10.29 s of CPU): our code, 4.86 to 7.02 s; page zeroing, 0.08 to 0.82 s; task switches, 0.07 to 0.51 s; mimalloc, 0.57 to 0.93 s; rayon's idle spinning, 0.01 to 0.05 s. It is the VM's shared cores and memory, not spinning.

**The allocator** (mimalloc v3 with its defaults; off mode):

- Purging (`purge_delay` 1000 ms, each purge a `MADV_DONTNEED`) does 987 purges, 154.8 MiB, per run. With `MIMALLOC_PURGE_DELAY=-1` the medians of 20 runs each go from 2.228 to 2.108 s (−5.4%, Mann-Whitney p = 0.040), the TLB-shootdown IPIs go, and the peak stays at 1.0 GiB of RSS (LOAD-8.15).
- mimalloc's arenas are `MADV_HUGEPAGE`, and an off run takes about 550 transparent huge pages. With `MIMALLOC_ALLOW_THP=0` it takes 3.6 to 4.1 s, against 2.7 to 2.9 s with them, so they stay.
- With THP `defrag` at `madvise`, a huge-page fault compacts memory directly when no free 2 MiB page is at hand. One off recording took 6.35 s for that, so a wall-time claim is a median of repeated runs, with the compaction counters logged.
- Page zeroing is the first touch of the run's 1.1 GiB, nearly all of it huge pages, and the largest single leaf (7 to 9% of the samples). Only a smaller footprint lowers it, as LOAD-3.3 does.

## 15. Q: not optimizations

Kept for the record. None is built from this ledger.

| id | what | from | state | how it would be settled |
|---|---|---|---|---|
| Q-1 | The parser against the whole corpus: every file PHP accepts and the parser refuses. E14 came from two warnings only. | off U1 | DONE (2026-10-15): none. `check` over D6's 18 trees with HEAD's gate build (`7a449c44`) read 111,974 files: D6's 111,836, and 138 more reached through symlinked directories, which `collect_php_files` follows. It refused 10, and PHP 8.5.4 refuses all 10. The other way, PHP refuses 262 files that the parser accepts: 259 by compile errors, C7's kinds (Q-3), and 3 by parse errors, which join Q-2. The run gave one lexer warning, for the stray byte. Its outputs are in `~/.cache/prs-ledger/q1/` | — |
| Q-2 | Parser gaps already known (D6): `$s{0}` (valid in PHP 7; the postfix loop has no arm for `{`, `syntax/src/parser/postfix.rs:16-240`); `(real)` and `(unset)` accepted (`syntax/src/ast.rs:1308`, `:1316`) though PHP 8 refuses them (PHP 8.5.4: a parse error for `(real)` and `$s{0}`, a fatal error for `(unset)`). Q-1 found three more that the parser accepts and PHP refuses: `(void)` used as an expression (`$a = (void)$z;`, `phpstan-src/tests/PHPStan/Rules/Cast/data/void-cast.php:5`), where PHP 8.5 allows the cast only as a statement; a stray control byte (`0x01`, `phpstan-src/turbo-ext/tests/parser-fixtures/bad-character-token.php:3`), which PHP refuses as a parse error from 8.0 on and only warns about before, while the lexer skips it with a warning at every version; and `function exit(...)` (phpstorm-stubs' `Core/Core.php:1193`), which the stubs declare. | off U2 | DONE (2026-10-16): probed under PHP 5.6–8.5.4 in Docker (`~/.cache/prs-ledger/q2/php-matrix.txt`), and with `check` and `check-project` at 7.4, 8.0 and 8.5. `$s{0}` is accepted through 7.4, a fatal error on 8.0–8.3 and a parse error on 8.4 and 8.5.4. `(real)` and `(unset)` are accepted through 7.4 and refused from 8.0 (a parse error and a fatal error). `(void)` as an expression is refused by every version; as a statement, by every version but 8.5.4. The control byte warns and is skipped through 7.4, and is a parse error from 8.0. `function exit` is refused by every version. The checker reports `(real)`, `(unset)` and `(void)` at the configured version (`cast.real`, `cast.unset`, `cast.void`, `cast.voidNotSupported`). The parser work is SYN-6 (OPEN). | The fix is SYN-6, with tests. The policy is D6's (PROD-4.3; the off report's §10.8): the parser accepts every version's syntax and records what a file used, and the configured version is applied after the parse (Q-10). So `$s{0}` gets its arm, and `(real)` and `(unset)` stay accepted and are recorded as syntax that PHP 8 removed. Syntax that no version accepts, as Q-1's three, is an error node (SYN-1.9), except `function exit` in a stub file. |
| Q-3 | Which of C7's 238 compile-time errors the parser does not report the checker already reports. | off U3 | DONE (2026-10-15). The 247 trigger files that PHP 8.5.4 refuses at compile time and the parser accepted when C7 ran (`c7/verdicts_run.tsv`) were each run through `check-project --no-composer` in a directory of its own, with HEAD's gate build (`7a449c44`) and `--php-version 8.5`, once with the defaults and once with every opt-in rule on, and each was read against PHP's message. The checker reports 120 as PHP does, and 1 more only with an opt-in rule (`function.duplicate`). 10 flag the line for another reason (`class.notFound` for `\self` or `extends self`; one reads `parent::$x::get(...)` as a static property). 1 only warns in this configuration (`declare(encoding)` with `zend.multibyte` off). Nothing reports the other 115: constant expressions 25; writes, references, `$this` and `isset` 16; names, namespaces and calls 17; modifiers 13; property hooks and asymmetric visibility 11; arguments and attributes 10; magic methods and reserved functions 8; control flow and statements 8; types 4; the scanner 3. Q-9 corrected the set: the first count read C7's code-reading verdicts (`c7/verdicts.tsv`), so it ran the two trait-alias modifiers (`f as static g`, `f as abstract g`), which the parser already refused when C7 ran, and left out `namespace namespace;`, which the parser accepts and nothing reports. The verdicts are in `~/.cache/prs-ledger/q3/classify.tsv` | — |
| Q-4 | E1's deterministic reproduction. | off U4 | DONE (2026-10-16): run. At `d8f524a8`, the commit before STUB-1 (`3d5434b3`), two jobs of a one-thread pool that each make a `Project::new()` deadlock. The first job starts the plain base's build, and its `install` into the global pool (`project.rs:1398`) lets the worker steal the second job, which forces the same `LazyLock` on its own stack. No job finishes in 300 s, so the watchdog fails the test (`~/.cache/prs-ledger/q4/pre2.log`). The full stacks are in `pre-stack2.txt`: the second job blocks on the `Once` the first holds. At `9c7c9555`, the branch's HEAD when it ran, the same scenario, with the base forced through `load_files` and `expect_files`, passes in 1.2 s (`head.log`). The regression test is `crates/phpstan-rs/tests/stub_base_reentry.rs`, committed as `55d9ea1f`: the HEAD form of the probe, which passes at HEAD in about 1 s. The form that failed at `d8f524a8` is `~/.cache/prs-ledger/q4/pre-probe.rs` | — |
| Q-5 | E10: which facts that outlive an `extract()` body (property assignments, class-level sites) a dropped body still needs. | off U5 | DONE (2026-10-15): five, all made by the body's expression inference. Private property writes with their types (`checker/property_assignment.rs:121`), against which `judge_property_types` judges every file's private property types. Member references (`checker/expr.rs:2863`), enum escapes (`record_enum_escape`, `:2582`) and classes escaping through a dynamic member access (`record_member_class_escape`, `:2864`), which the whole-program dead-member and dead-enum-case rules read. Named-argument calls (`record_named_argument_call`, `:1418`), which the renamed-argument rule reads. A body whose check stopped at its `extract()` would lose them: a private property type would be judged too wide against writes made only there, members used only there would be reported dead, and renamed arguments passed there would be missed. Private-member usage for the unused rules is a syntax walk (`checker/unused/member_usage.rs`), and the purity and property-write summaries, the init flows and the property type sites (recorded at the class) don't depend on the body's check either. The inference is most of a body's check, and E10 already skips every report (`reports_are_dropped`, `checker.rs:1524`), so widening E10 would save only the rule checks that do nothing but report | — |
| Q-6 | Which truncation holds the 33 `offsetAccess.mightNotExist` reports that `finalize` does not drop (B3 counts them among the discarded reports but not by site). Decides whether P11a's truncation saving is "well under 1 M" or ≈13 M. | off U6 | DONE (2026-10-15): 30 in a convergence pass's `rewind_to`, 3 in bodies that call `extract()`, so ≈12.3 M. Settled from the counts in the CHECK-4 build's profile, with no probe build (CHECK-4's record) | — |
| Q-7 | Readers left out of d: the uninitialized-property rules on, or a tree with test classes whose built-in parent-call hook rules fire, add the ancestors' bodies to d. | off U7 | DONE (2026-10-15): they add nothing measurable. With the skim built, d is the share of a run's skimmed context bodies that it parses on demand. A probe build of HEAD counted `FileBodies::read`'s first reads, with both caches off (`~/.cache/prs-ledger/q7/`): Piwigo 314 of 49,377 (0.0064 by count, 0.0056 by bytes), as B1 found; Monolog 25 of 19,674 (0.0013, 0.0016) and Laravel 34 of 38,158 (0.0009, 0.0010), trees with PHPUnit test classes, where the parent-call rules gave 132 and 32 reports. With the uninitialized-property rules on, no count changed, though they reported 12 and 92 properties | — |
| Q-8 | B9's embed and edit timings were read off the console and not kept; the build script's cost under C8's form (every stub file parsed, no base) is not measured; nothing ran on aarch64 or macOS (B10's backstop is designed, not run). | off U8 | SUPERSEDED (decision 7, which revives P6 in C8's form): B9's timings and the build script's cost are measured at STUB-10.1, and B10's backstop is STUB-11's equality test, run on each target the binaries are built for | — |
| Q-9 | A third pass on the report's claims the review workflow could not check (102 items). | off U9 | DONE (2026-10-16). Each of the 102 items was checked against the code at `5fb858c9` (master's where the report reads master), the profiles and probe outputs on disk, the drill's scripts run again, and PHP 8.5.4: 54 confirmed, 18 settled by later work or by the report's later corrections, 6 superseded by built rows, 6 corrected in the report, 1 partly confirmed, 3 open (Q-4 twice, STUB-11) and 14 not claims. The corrections: the per-file cache hits' reads and joins hold ≈4 M of the analysed files'; item 18.10's parts are `write_str` 16.1 M, the regex crates 14.8 M and the SQL parser 13.7 M; `loops.rs` is `checker/stmt/loops.rs`; proposal 8's "two thirds" is of the tree drop's cost, not of the trees' size; the global-facts walk over context bodies is ≈0.12 G and the reference walk does not shrink with the skim (F ≈3.7 G, S ≈1.3-1.4 G, ≈11.1-11.5 G left without cached verdicts); `simdutf8` checks UTF-8 at a third of std's cost (LOAD-6), not a quarter. All 273 of the report's code cites were resolved and read. The pass also corrected Q-3's file set, which had come from C7's code reading instead of its run. The verdicts are in `~/.cache/prs-ledger/q9/verdicts.tsv` | — |
| Q-10 | D6's parse-failure policy beyond the skim is not built: a context file that fails to parse keeps what it declares (what parses is recovered, what failed is unknown), validity follows the configured PHP version, and one rule holds for the batch run and the language server (the off report's §10.8). | off D6 | OPEN, partly probed (2026-10-16, `~/.cache/prs-ledger/q10/`). The recovery is in place: a context file with a syntax error keeps its declarations, and a broken method keeps its signature (PHP refuses that file at 7.4 and 8.5.4, so it has no such class). The version is not: a context file that uses `(real)` gives the same declarations at 8.5 and at 7.4, though PHP refuses it at 8.5.4 and loads it at 7.4. The language server's rule is not probed: it builds its own context list (`lsp.rs:490-496`). | Built with the parser, on SYN-1.9's error nodes: recovery points, a registry entry of unknown shape for what failed, marks for what a file uses that a version lacks (syntax added later, and syntax removed, such as Q-2's `(real)` and `(unset)`), one rule for both entry points. Since decision 6 the configured version is a range; decision 15 judges a file against it: its declarations exist only where its syntax holds, and an analysed file is reported at the constructs a version of the range refuses. |
| Q-11 | The crate split's C1-C3 follow-ups were not audited on this branch. The plan counts `phpstan_class_files` as gone by C3's follow-up, and it is still a field (`registry.rs:276`). | split C3 | DONE (2026-10-15): of the 20 follow-ups, 1 is done for one version, 5 are partly done and 14 are not; their section below, with the rows that carry them (LOAD-7, REG-18, CHECK-11) | — |
| Q-12 | How a file's syntax is judged against a version range. D6 has validity follow the configured version, and decision 6 made that version a range, but nothing says what a file whose syntax holds in only part of the range declares. Decision 6's in-part rule judges declarations, not a file's syntax, and a guard can stand on the include: `symfony/polyfill-mbstring`'s `bootstrap.php:12` (as vendored under `vendor/league-commonmark/vendor/`) requires `bootstrap80.php`, which declares its functions with union types, only when `PHP_VERSION_ID >= 80000`. | decision 6; off D6 | DONE (2026-10-15): decision 15 | — |
| Q-13 | A builtin's result before PHP 8.0 when its arguments fail its parameter parsing. Outside `strict_types`, PHP 7 warns and the call yields what the function leaves, `null` unless it sets another value: the result starts as `null` (`ZEND_DO_ICALL`, `Zend/zend_vm_def.h:3993`), and `ZEND_PARSE_PARAMETERS_END` reports and returns (`Zend/zend_API.h:1185-1214`) through `zend_internal_type_error`, which throws a `TypeError` only under `strict_types` (`Zend/zend.c:1577`). A wrong argument count goes the same way (`zend_API.c:211`, `zend.c:1594`). These are php-src 7.4.33's sources (`~/.cache/prs-ledger/php-src-7.4.33/`); PHP 8 throws `TypeError` and `ArgumentCountError` (8.5.4: `strlen([])`, `strlen()`). Decision 10's bound and claims describe a call whose arguments parse, so for STUB-12.2's versions a reading misses that `null`. | decision 10 | DONE (2026-10-15): decision 16 | — |
| Q-14 | Whether a return that REG-17.1 infers keeps its literal values: `return 'x'` as `'x'` or `string`, `return [1]` as `array{1}` or `list<int>`. The installed body returns exactly those values, and a closure's inferred return keeps them today (`closure_inferred_return_type`, `checker/expr.rs:6625`, unions the return points as they are). But decision 9 lets a body's facts fill only what is undeclared because "the installed body is one version of the code", and a literal is the fact the next version is likeliest to change: a vendor function that returns `'1.2'` today would make a caller's `=== '1.2'` always true. | decision 9 | DONE (2026-10-15): decision 17 | — |
| Q-15 | The oldest PHP version the builtin knowledge covers. STUB-12.2's dumps of real binaries name none, and decision 6's default range, every version known (when neither the configuration nor Composer names a version), starts there. So it sets what decision 6's in-part availability reports in a project that names no version: every builtin that the oldest version lacks or a later one removed, wherever no guard covers its use. | decision 6 | DONE (2026-10-15): decision 18 | — |

### Q-11: the crate split's C1-C3 follow-ups, read against the code

Each follow-up of `CRATE_SPLIT_PLAN.md`'s C1, C2 and C3, read against HEAD's code and, where a probe settles it, HEAD's gate build (`7a449c44`; the probes are in `~/.cache/prs-ledger/q11/`).

| follow-up | state | evidence | carried by |
|---|---|---|---|
| C1, nested validation | partly | a config error names its dotted key; no `deny_unknown_fields` on the seven nested structs; `count` unenforced; messages compiled with `regex` at each match | LOAD-7.1 |
| C1, missing settings | not done | 1 of about 40 keys exists; `php_version` is one `major.minor` | LOAD-7.8, STUB-7.1 |
| C1, one settings assembly | not done | the LSP assembles its own settings (`lsp/workspace_config.rs:214`) | LOAD-1.3 |
| C1, activation reaches library typing | not done | no `Extension` impl for six extensions; PHPUnit narrowing reads no activation | LOAD-7.2 |
| C1, paths and matching | not done | `glob_matches` (`ignore_errors.rs:96`); the psr3 pattern a `regex::Regex` | LOAD-7.3 |
| C1, entries validate at load | partly | assertion entries checked for their shape only; restricted symbols not at all | LOAD-7.4 |
| C1, config discovery | not done | the CLI reads no config without `--config` | LOAD-7.5 |
| C1, one walk per file | not done | `collect_php_files` follows directory links with no dedup (Q-1 read 138 files through them) | LOAD-1.1, LOAD-2.2 |
| C1, keys | not done | `ext.psr3.check_psr3_logging` repeats its section | LOAD-7.6 |
| C1, PHPStan configs | not done | no `neon` crate, no `init` | ROADMAP Phase 183 |
| C1, tests and docs | partly | `declared_php_versions` documented; the help's `--level`, `--stub-path` and `--format` texts wrong | LOAD-7.7 |
| C2, the gate | not done | no settings rule in `xtask/src/arch.rs` | REG-18.1 |
| C2, predefined constants over a range | done for one version | `PHP_VERSION_ID`, `PHP_MAJOR_VERSION`, `PHP_INT_MAX` and `PHP_INT_SIZE` as the plan has them, at 8.5 | STUB-7.1 (the range) |
| C2, `ConstantResolver` in `reflection` | not done | `checker/constant_resolver.rs:21`; `dynamic_constant_names` an `EngineSettings` key | REG-18.2 |
| C2, initializers evaluated | not done | `2 + 3`, `1 << 2`, `\PHP_INT_SIZE` as `mixed`; enum case values `int` | REG-18.3 |
| C2, one folded constant key; `generalize` | partly | `generalize` done; `\APP\A` is `constant.notFound` | REG-18.4 |
| C2, `RuleLevelHelper` owns the rule | not done | `types::Relation` keeps the three settings | REG-18.5 |
| C3, C3a's `property.unused` delta | not done | mapped properties `property.unused` with ext-doctrine on | CHECK-11 |
| C3, `is_phpstan_code` by provenance | not done | `phpstan_class_files` read (`checker/phpstan_api.rs:254`) | REG-18.6 |
| C3, attribute arguments typed | partly | global constants typed and private ones reported; `self::` constants not | REG-18.7 |

## Appendix A. Where each earlier id went

Every id the earlier ledgers used, with its id here.

### A.1 The off drill's rows (`OFF-x`, and `x` in the earlier Appendix A)

| earlier | here | earlier | here |
|---|---|---|---|
| OFF-D1 | PROD-4.1 | OFF-P1 | STUB-2 |
| OFF-D2 | PROD-4.2 | OFF-P2 | CACHE-1 |
| OFF-D3 | STUB-10 | OFF-P3 | STUB-4 |
| OFF-D5 | CACHE-2 | OFF-P4 | STUB-5 |
| OFF-D6 | PROD-4.3 | OFF-P5 | STUB-3 |
| OFF-D7 | STUB-6 | OFF-P6 | STUB-11 |
| OFF-E1 | STUB-1 | OFF-P7 | DOC-1 |
| OFF-E2 | CHECK-1 | OFF-P8a | PROD-1 |
| OFF-E3 | CHECK-2 | OFF-P8b1 | PROD-2 |
| OFF-E4 | CACHE-6 | OFF-P8b | PROD-3 |
| OFF-E5 | TYPE-1 | OFF-P9 | REG-1 |
| OFF-E6 | TYPE-2 | OFF-P10 | LOAD-6 |
| OFF-E7 | TYPE-3 | OFF-P11 | CHECK-4 |
| OFF-E8 | CACHE-11 | OFF-H1 | CACHE-8 |
| OFF-E9 | CHECK-5 | U1 | Q-1 |
| OFF-E10 | CHECK-3 | U2 | Q-2 |
| OFF-E11 | CACHE-12 | U3 | Q-3 |
| OFF-E12 | CACHE-7 | U4 | Q-4 |
| OFF-E13 | CACHE-3 | U5 | Q-5 |
| OFF-E14 | SYN-5 | U6 | Q-6 |
| OFF-E15 | CACHE-4 | U7 | Q-7 |
| OFF-E16 | CACHE-5 | U8 | Q-8 |
| | | U9 | Q-9 |

### A.2 The 20 G plan's items

| earlier | here |
|---|---|
| 20G-0 | VAL-1 (merged with the earlier VAL-1, which was its sub-task list) |
| 20G-1 | LOAD-1 (its parts are LOAD-1.1 to 1.8) |
| 20G-2 | LOAD-2 (2.1 to 2.8) |
| 20G-3 | LOAD-3 (3.1 to 3.6) |
| 20G-4 | REG-13 (13.1 to 13.9); its per-member flags are REG-4.5, and its private fields REG-4.12. Superseded by decision 3, which moved what is still wanted to REG-4.15, REG-7.4, REG-8.3 and REG-16 |
| 20G-5 | LOAD-4 (4.1 to 4.7) |
| 20G-6 | step 1: REG-7.2; step 2: STUB-8; step 3: CACHE-9; step 4: STUB-9; step 5: CACHE-10 |
| 20G-7 | TYPE-4 (4.1 to 4.4) |
| 20G-8 | REG-14 (14.1 to 14.5) |
| 20G-9 | SYN-1; steps 1-8 are SYN-1.1 to 1.8. Decision 4 rewrote steps 2-8 for a flat tree, superseded step 6, and added SYN-1.9 and 1.10 |
| 20G-10 | SYN-2 |
| 20G-11 | SYN-3; steps 1-4 are SYN-3.1 to 3.4. The earlier ledger's "11.5" (the server's on-demand index) is step 3, SYN-3.3 |
| 20G-12 | SYN-4; steps 1-10 are SYN-4.1 to 4.10 |
| 20G-13 | PROD-5; steps 1-7 are PROD-5.1 to 5.7 |
| 20G-14 | DOC-2; steps 1-8 are DOC-2.1 to 2.8 |
| 20G-15 | NAME-1 (1.1 to 1.6) |
| 20G-16 | NAME-2 (2.1 to 2.4) |
| 20G-17 | CACHE-13; steps 1-2 are CACHE-13.1 and 13.2 |
| 20G-18 | CHECK-6. Steps 1-9 are CHECK-6.1 to 6.9 (step 1, which the earlier ledger called "18.1", is §12.10). Step 10's F1, F2, F6a, F6c, F6d, F6e, F6f, F4 and one checker per worker are CHECK-6.10 to 6.18 |
| 20G-19 | LOAD-5 (5.1 to 5.4) |

### A.3 The blocking tasks

| earlier | here | earlier | here |
|---|---|---|---|
| DEC-1 | STUB-10 | CRATE-1.C4.10a | REG-11.1 |
| DEC-1.1 | STUB-7.1 | CRATE-1.C4.10b | REG-11.2 |
| DEC-1.2 | STUB-10.1 | CRATE-1.C4.10c | REG-11.3 |
| DEC-1.3 | STUB-10.2 | CRATE-1.C4.10d | REG-11.4 |
| CRATE-1 | REG-2 to REG-12 (C4), STUB-7 (C5), REG-15 (C10) | CRATE-1.C4.10e | REG-11.5 |
| CRATE-1.C4.1 | REG-2 | CRATE-1.C4.10f | REG-11.6 |
| CRATE-1.C4.2 | REG-3 | CRATE-1.C4.10g | REG-11.7 |
| CRATE-1.C4.3 | REG-4 | CRATE-1.C4.10h | REG-11.8 |
| CRATE-1.C4.3a | REG-4.1 | CRATE-1.C4.10i | REG-11.9 |
| CRATE-1.C4.3b | REG-4.2 | CRATE-1.C4.10j | REG-11.10 |
| CRATE-1.C4.3c | REG-4.3 | CRATE-1.C4.10k | REG-11.11 |
| CRATE-1.C4.3d | REG-4.4 | CRATE-1.C4.10l | REG-11.12 |
| CRATE-1.C4.3e | REG-4.5 | CRATE-1.C4.10m | REG-11.13 |
| CRATE-1.C4.3f | REG-4.6 | CRATE-1.C4.10n | REG-11.14 |
| CRATE-1.C4.3g | REG-4.7 | CRATE-1.C4.10o | REG-11.15 |
| CRATE-1.C4.3h | REG-4.8 | CRATE-1.C4.10p | REG-11.16 |
| CRATE-1.C4.3i | REG-4.11 | CRATE-1.C4.gate | REG-12 |
| CRATE-1.C4.3j | REG-4.12 | CRATE-1.C5.1 | STUB-7.1 |
| CRATE-1.C4.3k | REG-4.13 | CRATE-1.C5.2 | STUB-7.2 |
| CRATE-1.C4.3l | REG-4.14 | CRATE-1.C5.3 | STUB-7.3 |
| CRATE-1.C4.4 | REG-5 | CRATE-1.C5.4 | STUB-7.4 |
| CRATE-1.C4.5 | REG-6 | CRATE-1.C5.5 | STUB-7.5 |
| CRATE-1.C4.6 | REG-7 | CRATE-1.C5.6 | STUB-7.6 |
| CRATE-1.C4.6a | REG-7.1 | CRATE-1.C5.7 | STUB-7.7 |
| CRATE-1.C4.6b | REG-7.2 | CRATE-1.C5.8 | STUB-7.8 |
| CRATE-1.C4.6c | REG-7.3 | CRATE-1.C5.9 | STUB-7.9 |
| CRATE-1.C4.6d | REG-7.4 | CRATE-1.C5.10 | STUB-7.10 |
| CRATE-1.C4.6e | REG-7.5 | CRATE-1.C5.11 | STUB-7.11 |
| CRATE-1.C4.6f | REG-7.6 (constants) and REG-7.9 (reentrancy) | CRATE-1.C5.gate | STUB-7.12 |
| CRATE-1.C4.7 | REG-8 | CRATE-1.C10.1 | REG-15.1 |
| CRATE-1.C4.7a | REG-8.1 | CRATE-1.C10.2 | REG-15.2 |
| CRATE-1.C4.7b | REG-8.2 | | |
| CRATE-1.C4.7c | REG-8.3 | | |
| CRATE-1.C4.7d | REG-8.4 | | |
| CRATE-1.C4.8 | REG-9 | | |
| CRATE-1.C4.9 | REG-10 | | |
| CRATE-1.C4.10 | REG-11 | | |

### A.4 Validation and measurements

| earlier | here |
|---|---|
| DEC-2 | VAL-2 |
| DEC-2.1 | VAL-2.1 |
| DEC-2.2 | VAL-2.2 |
| DEC-2.3 | VAL-2.3 |
| DEC-2.4 | VAL-2.4 |
| DEC-2.5 | VAL-2.5 |
| DEC-2.6 | VAL-2.6 |
| DEC-2.7 | VAL-2.7 |
| DEC-2.8 | VAL-2.8 |
| VAL-1, VAL-1.1 to VAL-1.13 | unchanged; 20G-0 is merged into VAL-1 |
| MEAS-1, MEAS-2 | unchanged |

## Appendix B. The 20 G report and the 25 G report's designs, re-evaluated

The 20 G report describes `5fb858c9`. For each finding and each design, these tables say what the code and the P8b profile show now.

### B.1 The 20 G report's findings (its Part I)

| finding | the report | now |
|---|---|---|
| R1, context trees dropped on the main thread (575.5 M) | item 3 (LOAD-3) | 267.8 M after P8b (skimmed bodies are not trees); still serial, 11,459 trees |
| R2, five functions per token (1.77 G) | item 10 (SYN-2) | 2,868 M in the six functions over 8.44 M tokens (the off profile also lexes the stub files; 78% is the project's); P8b's skim still pays it per skimmed token |
| R3, results written into shared declarations; string keys; `Members::eq` 96.9 M | item 4 (REG-13) | gone with P9 (REG-1): `Members::eq` 4.8 M, no eager purity pass |
| R4, a line index for every file (270 M) | item 11 (SYN-3) | 363.4 M over 25,753 builds (stub files included) |
| R5, stub members decoded to learn they have no deferred types (178 M) | item 6 (its step 1 is REG-7.2) | the warm run at this code (MEAS-1): 87,146 stub function decodes, 682.5 M. The scan's decodes, 177.8 M warm at `aa896697`, stay until REG-7: a fact carried in each entry would only move them to the passes that read every method in scope (`ITEMS_1_TO_3_FIXES_PLAN.md`'s R12) |
| R6, lookups by `&str` (701 M of folding) | item 15 (NAME-1) | 634 M (`fold_hash` 275.7 M, `is_folded_eq` 358.2 M) |
| R7, products for every body (`BodySummary` 344 M, `InitFlow` 218.5 M, `GlobalFacts` 218.5 M) | item 13 (PROD-5) | cut by P8a and P8b1 (PROD-1, PROD-2) to 83.6 M, 50.8 M and 74 M |
| R8, stack guards in hot paths | item 9 (SYN-1) | `peeled` 61.2 M (2.54 M calls) |
| R12, identifier names | item 12 (SYN-4) | `name_and_bits` 631.4 M, `Keyword::try_from_bytes` 165.9 M |
| M1-M11, memory | items 3, 4, 9, 14, 16, 19 (LOAD-3, REG-13, SYN-1, DOC-2, NAME-2, LOAD-5) | re-measured by MEAS-2 (§14): the baselines moved as MEAS-2 says |

### B.2 The 25 G report's unbuilt designs (the 20 G report's §2 lists them)

The ids in the first column are the 25 G report's.

| design | now |
|---|---|
| C3 steps 6-7: names as `Name` keys (≈0.6 G) | NAME-1.2 (step 6), NAME-1.1 (step 7) |
| F4: purity and property-write keys as folded strings (≈0.5 G) | gone: P9 (REG-1) removed the eager passes |
| F3: results inside shared declarations; a serial `changed_since` | the shared-declaration copies are gone with P9 (REG-1); `changed_since` goes for the engine's early cutoff (REG-16.3, decision 3; it was REG-13.7) |
| §14: trees dropped in the serial insert loop | LOAD-3, 267.8 M left |
| P7 and P11 step 1: PHPDoc lowering and tag effects | the off report's P7 (DOC-1) built the lazy description; DOC-2.1 (lowering) and DOC-2.3 (tag effects) have the rest |
| P6(d): the streaming lexer's plumbing | SYN-2 |
| P4: column tables for every file | SYN-3 |
| F1 and F2: the stubs' availability guards and their count | STUB-8 (the guards) and STUB-9 (the count), F2's other half; what F1 left decoding (R5) is item 6's step 1, in REG-7.2 |
| C1 step 5, C5, §12.10 | CHECK-6.2 (C1 step 5's tail), CHECK-6.1 (§12.10, which deletes C5's interim record) |
| C2 steps 2-3, C4 step 5's keys | CHECK-6.4 (C2 step 2); NAME-1.2 (C2 step 3's `TemplateBindings`, and C4 step 5's keys) |
| F5: a `FileId` bitset, string frontier sets | no scope is left to hold: the engine runs again only the queries whose recorded reads changed (REG-16.4, decision 3, which superseded LOAD-4.6 and REG-13.6) |
| P12 steps 3-4: declaration list, the collector goes | PROD-5.1, PROD-5.4 |
| §12.11: `MemberIndex` | CHECK-6.5 |
| the sorted traversal's k-way merge | LOAD-2.5 |

## Appendix C. How this ledger was made

- **Read:** when this ledger was first made, the off ledger and the 20 G report, whole. For the merge into one document, the earlier ledger, the 20 G plan's Part II and its §11-§13, and the crate split's C4, C5 and C10, whole; the off report and the heap report where a row cites them.
- **Profile:** `~/.cache/prs-profile/p8b/cg` (791d29cb, off mode, 20,928,053,254 Ir), indexed with `drill-scripts/parse.py` and read with `q.py` and `roots.py`.
  - Figures are inclusive, summed over threads.
  - The share of a shared function that belongs to the project's files was read from the call counts of `parse_file`'s callers: 6,316.8 M the project's, 1,578.4 M the stubs', 163.0 M the autoload files'.
- **Not measured:** wall time. MEAS-1 read the warm-mode costs and MEAS-2 the memory, after this appendix's profile. The estimates in the "report's estimate" columns are the reports', unchanged.
- **Dependencies** ("needs") are read from the sources' texts, not from code. The facts the rows cite from the code were checked against it on 2026-10-15:
  - `unsafe_code = "deny"` (`Cargo.toml:42`);
  - `phpstan_class_files` (`registry.rs:276`);
  - `finalize_standalone_sig` (`registry/resolve.rs:2119`);
  - `infer_global_variables` (`registry/global_vars.rs:315`);
  - the comments naming `resolve_transitive_purity`;
  - `names_with_assert_tags`' writers;
  - no `explicit_mixed_hint` left.

**What the merge into one document changed**, on 2026-10-15. Before it, the ledger listed the off drill's rows, the 20 G items, the 25 G designs and the crate split's sub-tasks in separate parts.

- **Every task is in the area of the program it changes, renumbered by area.** Appendix A maps the earlier ids.
- **Duplicates merged:**
  - item 0 and the earlier VAL-1 (now VAL-1);
  - the off report's D3 and DEC-1 (now STUB-10);
  - P6, E8 and E11, which appeared both in the open rows and in the earlier Appendix A (now STUB-11, CACHE-11 and CACHE-12).
- **The 20 G items' steps and parts are rows of their own,** with the needs their texts give. Several are ready, where their item as a whole was not.
- **Needs corrected:**
  - Item 16 was listed as needing item 15. It is NAME-1.4 (item 15's short name) that needs NAME-2.1 (item 16's header).
  - Item 17 was listed as needing item 5. Its ids by record index are item 4's (REG-13.1).
  - Four needs are not in the plan's text and are dropped: §12.10 on item 13, the radix scopes on item 16, item 13 on item 10, and item 14 on item 9 ("doc bytes"). Item 13 needs item 9 for its step 1 only (PROD-5.1).
  - The "11.5" the earlier ledger cited is item 11's step 3 (SYN-3.3).
- **Item 9 is no longer counted as ready.** Its arena is read from other threads only through step 6's `unsafe impl Sync`, which the workspace denies; only step 1 stands alone (decision 4). Decision 4 then replaced the arena with a flat tree, and SYN-1.2 is ready.
- **The 20G-14 row's "OFF-P11 was measured and not kept" is corrected.** That was the off report's P11 (CHECK-4); the 25 G report's P11 step 1 (DOC-2.3) was never built or measured.
- **The crate split's rows gained the parts of its plan that the earlier table left out:** REG-4.9, REG-4.10, REG-7.7, REG-7.8, REG-7.10, REG-7.11, REG-7.12, REG-8.5, REG-8.6, REG-8.7 and REG-11.1 to 11.16 as rows. The earlier C4.6f is split into REG-7.6 (constants) and REG-7.9 (reentrancy).
- **Parts of one design that two sources hold are one row:**
  - item 4's per-member flags with REG-4.5, and its private fields with REG-4.12 (the crate split counts ≈280 sites, item 4 counted 149);
  - item 6's step 1 with REG-7.2;
  - item 8's explicit-mixed part with REG-4.8, which is done (REG-14.2).
- **REG-5 is PART:** REG-1 (P9) did its purity part.
- **New rows:**
  - VAL-1.14 and VAL-1.15 (the 20 G plan's §11, which the earlier VAL-1 left out);
  - VAL-2.9 (the crate split's output changes);
  - Q-10 (D6's policy beyond the skim) and Q-11 (the crate split's C1-C3 follow-ups).
- **The six decisions in §2.4 were found by the merge,** and a seventh waited from before it. The merge stated each as its sources did and decided none.

**What the decisions changed**, on 2026-10-15. All eighteen were decided after the merge, the eleventh to the eighteenth on what a reading of the whole ledger found open; §2.4 has each, with its evidence and its rows.

- **Superseded:**
  - REG-13 and its nine sub-tasks;
  - SYN-1.6;
  - LOAD-1.6, LOAD-4.1, LOAD-4.2, LOAD-4.6 and LOAD-5.3;
  - REG-8.2;
  - REG-11.14 (decision 11) and STUB-9 (decision 12);
  - Q-8, whose parts are STUB-10.1's and STUB-11's.
- **Rewritten:**
  - SYN-1 to SYN-1.8 (SYN-1.6 aside), SYN-2's needs, LOAD-3.5 and CHECK-6.3;
  - PROD-5, PROD-5.1, PROD-5.4 and PROD-5.7;
  - TYPE-4 to TYPE-4.2;
  - REG-4.5, REG-4.11, REG-7, REG-7.5, REG-7.6, REG-7.7, REG-7.9, REG-7.12, REG-8, REG-8.1, REG-8.3, REG-8.5, REG-9, REG-11.1, REG-14.1 and REG-14.3;
  - STUB-7.1 to STUB-7.6, STUB-7.9, STUB-7.10, STUB-8, STUB-10 to STUB-11;
  - CACHE-9, CACHE-10, CACHE-13, CACHE-13.1 and CACHE-13.2;
  - VAL-2.3, VAL-2.5, VAL-2.6 and VAL-2.9;
  - STUB-10.1, which names B9's timings;
  - Q-10.
- **Approved:** the deltas of REG-4.14, REG-5, REG-6, REG-8.5, STUB-7.1 to STUB-7.3, STUB-7.5, STUB-7.9 and STUB-7.10 (decision 14).
- **New:**
  - SYN-1.9 and SYN-1.10;
  - PROD-5.8 and DOC-2.9;
  - TYPE-5;
  - REG-4.15, REG-16 (16.1 to 16.5) and REG-17 (17.1 to 17.4);
  - STUB-12 (12.1 to 12.5), which decisions 10, 16 and 18 re-specified, and STUB-13 (13.1, 13.2);
  - CHECK-7 to CHECK-10 and VAL-2.10.
- **The facts the decisions cite from the code** were checked against it on 2026-10-15:
  - the six declarations' `*case_refs` fields and the assertion's;
  - `redeclared_methods` (`registry/class_info.rs:382`) and its two readers;
  - `Recorder` (`syntax/src/references.rs:184`);
  - `parse`'s `Result` (`syntax/src/parse.rs:82`), and the skip of a file that fails to parse (`project.rs:1141`);
  - the dependents indexes (`project.rs:250`, `:253`) and `finishing_scopes` (`project.rs:1359`);
  - the old collector's two callers (`project.rs:2924`, `registry/stubs.rs:1486`);
  - `resolve_names` (`syntax/src/name_resolution.rs:515`) and its one caller (`stub_attributes.rs:185`);
  - `name_anonymous_classes` (`registry/anonymous_class.rs:45`);
  - the functionMap deltas read at the newest revision (`registry/stubs.rs:253`);
  - Composer's PHP constraint (`crates/composer/src/lib.rs:114`);
  - `pow`'s folding (`types/src/arithmetic.rs:715-717`);
  - the 2,612 `ExprKind::`/`StmtKind::` match sites in 135 files;
  - for decisions 8 and 9 and their rows, the lines they cite in `types/src/decide.rs`, `types/src/parse.rs`, `registry/function_sig/type_resolution.rs`, `registry/function_sig.rs`, `checker/phpdoc_type_compat.rs`, `checker/function_decl.rs`, `checker/throws.rs`, `checker/stmt/try_catch.rs`, `checker/expr.rs`, `registry/summaries.rs`, `registry/purity.rs` and `project/session.rs`;
  - for decision 11, the merges in `registry/index.rs` (`:144`, `:244`, `:329`, `:344`, `:434`, `:436`, `:589`) and `registry/function_sig.rs` (`:951`, `:1019`), and `own_declaration_sig` (`checker/function_decl.rs:206`).
- **PHP's behaviour** was read from PHP 8.5.4, not from the sources' texts (§2.4, decisions 1, 8 and 9). Decision 10 read php-src 8.5.4's sources and PHPStan 2.3.1's. Decisions 11 and 14 probed PHP 8.5.4 too. Decision 11 also read Composer's loader and PHPStan 2.3.1's phar; decision 13 probed ext4 on Linux 7.0 and read Linux's FAT and exFAT code, git 2.47.0's stat check and Rust 1.98's `std`; decision 14 read php-src 8.3.0's and 8.4.0's stubs. Decisions 15 to 18 probed PHP 8.5.4 and the official PHP 5.6, 7.3, 7.4 and 8.0 images, read PHPStan 2.3.1's phar and the installed packages' Composer requirements, and checked HEAD's gate build.

**What a reading after the decisions found**, on 2026-10-15, reading the whole ledger again:

- Q-2's policy question was decided with D6 (the off report's §10.8), so only the parser work is left.
- Q-8's parts are STUB-10.1's and STUB-11's, so it is superseded, and STUB-10.1 names B9's timings.
- Four questions the decisions leave open are new rows: Q-12 to Q-15. Q-13 read php-src 7.4.33's sources (`~/.cache/prs-ledger/php-src-7.4.33/`). Decisions 15 to 18 settled them.
- Four rulings it found open are decisions 11 to 14: REG-4.11's location rule, STUB-9's form, CACHE-13.1's soundness, and the crate split's output changes that had no approval.

**What the runs settled**, on 2026-10-15, one heavy job at a time under `nice -n 19`:

- Q-1, Q-3 and Q-7 ran HEAD's gate build (`7a449c44`, saved under `.corpus-runs/gate/bin/`), and Q-7 a probe build of it. MEAS-3 ran the P8b binary under callgrind. REG-16.1 ran a probe build of HEAD and a replay against salsa 0.28.5.
- Each probe build patched the checkout and was reverted after its binary was saved; the gate build made again from the clean tree is byte-identical to the saved one.
- The results are on the rows, with MEAS-3's in §14 and REG-16.1's in §9's records.
