# Heap drill-down of the off, cold and warm runs: every allocation, whole stacks

Profiles: `~/.cache/prs-profile/dhat-heap-modes/{off,cold,warm}/` (`dhat-heap.json`, converted to `prof/`), one run per mode.
- **Command.** `check-project --config phpstan-rs.toml --format json`, run in `/home/torres/piwigo17-frozen` with `PHPSTAN_RS_NO_FILE_CACHE=1` and `PHPSTAN_RS_DHAT_UNTRIMMED=1`. The three runs differ only in the stub cache:
  - off: `PHPSTAN_RS_NO_STUB_CACHE=1`;
  - cold: `PHPSTAN_RS_CACHE_DIR` set to an empty directory, which the run fills;
  - warm: the same directory, as the cold run left it.
- **Binary.** The `dhat` profile with `--features dhat-heap` and `-C force-frame-pointers=yes`, built at `d2d1e436` on branch `callgrind-20g`, with the patched dhat-rs that `ff347308` adds as `patches/dhat`. The code is that of `5fb858c9`, the 20 G report's heap profile: the commits between them change only documentation and the dhat environment variable.
- **Machine.** 8 cores; the default worker pool (8 threads); `nice -n 19`.

**Sources.** Every figure comes from the three profiles, from the 20 G report's heap profile (`dhat-20g/`, ten frames per stack) and from the code at `d2d1e436`. Nothing was built for this report beyond the profiled binary.

**Conventions.**
- "MB" is 10^6 bytes, and a "block" is one allocation. dhat-rs counts a `realloc` as a block of the new size, charged to the site that first allocated it.
- Bytes are the bytes requested, not the allocator's resident size.
- "At the peak" is what is live when the heap is largest (t-gmax), and "at exit" is what is live when the run ends. `check-project` leaks the project on purpose (`check_project.rs:159-160`), so "at exit" is what the project holds after checking.
- "Item n" is an item of the 20 G plan (`PROFILE_DRILL_PIWIGO_20G.md`, Part II). "Proposal n" (Pn) is one of the off report's (`PROFILE_DRILL_PIWIGO_OFF.md`, §10), and Hn is one of this report's (§11).
- M1-M11 are the 20 G report's memory findings (its §8).

## Summary

- **The stub build is 5.80 M of the off run's 24.87 M allocations.** The off run allocates 5.43 M blocks and 874 MB more than the warm run, its peak is 130 MB higher and it holds 56 MB more at exit (§4). The stub phases, by mechanism:

  | mechanism | blocks | MB | at the peak | at exit |
  |---|---:|---:|---:|---:|
  | the stub sources' parse and index | 4.27 M | 575 | 82.1 | 82.1 |
  | serialization that no store keeps (P2) | 1.26 M | 265 | 22.4 | 0 |
  | the stub corpus's merge | 0.23 M | 59 | 13.4 | 13.4 |
  | the stub bases' own work | 0.05 M | 49 | 15.2 | 16.2 |

  In the warm run, the same stub data costs 0.48 M allocations instead (item 6's decoding, §6).
- **The cold run is the off run plus the cache write.** It allocates 34 MB more and has 21 K fewer blocks. Its peak is 14.6 MB higher, because the cache file is built in a second buffer while the first is still live, at the peak (§5, H1).
- **The peak is the same event in all three runs.** All 14,251 trees are live (569 MB) while `expect_files` builds or loads the PHPStan stub base (`session.rs:791`, `project.rs:1203-1221`), between the parse and the inserts (§7):

  | run | peak | trees | the PHPStan base | the stub sources |
  |---|---:|---:|---:|---:|
  | warm | 866.6 MB | 569.1 MB | 17.1 MB, its cache file read whole | — |
  | off | 996.5 MB | 569.1 MB | 31.1 MB, with an 18.3 MB write buffer that nothing stores | 82.1 MB |
  | cold | 1,011.1 MB | 569.1 MB | 45.5 MB, with both write buffers | 82.0 MB |

  This confirms M1 with whole stacks.
- **What the run keeps** is 658 MB (warm) and 714 MB (off) (§8). The off run keeps the stub sources' declarations and PHPDoc (82 MB) where the warm run keeps the classes it decodes (§6). The process-wide memos keep the stub build's PHPDoc to exit: the tag memo holds 58.2 MB in the off run against 24.6 MB in the warm run.
- **Ten frames were enough for M10's map of allocation sites to items, and not for any question by phase.** With ten frames, 63% of the warm run's blocks have no phase root, and 18 blocks have none with whole stacks. Run on both profiles, this report's rules give the same item table within 16 K blocks per item (§10).
- **Every allocation site is mapped to the item or proposal that removes it**, in all three runs (§9). The 97 functions that the 20 G report did not read were read at their allocating lines, and each is mapped (Appendix A).

  | | warm | off |
  |---|---:|---:|
  | 20 G plan items | 19.44 M | 19.09 M |
  | the stub build (P6; P1, P2 and P7 without it) | — | 5.77 M |
  | made once per process | 1.5 K | 1.6 K |

  No block is unattributable.
- **One new proposal** (§11):

  | # | proposal | saving | runs |
  |---|---|---|---|
  | H1 | Write the cache file from one buffer | −15.3 MB at the peak, −31.4 MB allocated | cold |

  The report also measures, in heap terms, P1, P2, P6 and P7, and items 3, 4, 6 and 14 where the off and cold runs change them.

## 1. The runs

| run | allocated | blocks | at the peak | at the peak (blocks) | peak time, of the run | at exit | at exit (blocks) | program points |
|---|---:|---:|---:|---:|---|---:|---:|---:|
| off | 3,543.6 MB | 24,865,388 | **996.5 MB** | 5,254,824 | 39.8 s of 69.1 s | 714.1 MB | 4,461,708 | 3,370,746 |
| cold | 3,577.9 MB | 24,844,393 | **1,011.1 MB** | 5,255,665 | 44.8 s of 75.5 s | 715.2 MB | 4,457,795 | 3,623,142 |
| warm | 2,669.4 MB | 19,438,342 | **866.6 MB** | 4,573,499 | 29.9 s of 62.9 s | 657.8 MB | 4,111,448 | 3,146,608 |
| 20 G report (10 frames) | 2,668.6 MB | 19,440,892 | 866.1 MB | 4,574,578 | 425 s of 518 s | 657.1 MB | 4,113,459 | 212,513 |

- **The outputs.** All three runs exited 0, and their `out.json` files are byte-identical.
- **The cache.** The cold run wrote four files with key `7066d99f7b41a660`. The warm run read them and did not rewrite them.
- **The warm run is the 20 G report's profile again,** from the same code. It differs by 0.03% in bytes and 0.01% in blocks: per-thread memos fill differently on 8 threads.
- **Run time.** Each run took 3½-4 minutes of wall time under the patched dhat. The 20 G report's run, with upstream dhat-rs and ten frames, took 518 s. Whole stacks with upstream dhat-rs were on course for ≈9 h per run and were stopped. The time a run spends under the profiler still is not its time without it; the peak times above only place the peak among the phases.

## 2. Method

- **Whole stacks.** Every program point keeps its whole backtrace: 580-657 M frames per run. The patched dhat walks frame pointers outside its lock, keeps return addresses and resolves each address once at exit. Its stacks and counts were checked against DWARF unwinding, stack by stack, on a benchmark: the off run's stub build, run on one thread.
- **Phases.** Each stack belongs to the innermost frame that roots a phase:
  - the phase roots are those of the off report's callgrind attribution: `stub-sources`, `stub-corpus-merge`, `stub-corpus-store`, `stub-index`, `stub-data`, `stub-base-plain`, `stub-base-phpstan`, `vendor`, `parse`, `insert`, `finishing`, `check`, `session-load`, `session-run` and `main`;
  - a stolen rayon job that runs a `Registry` method or a finishing pass, with no other root, goes to `finishing`;
  - `finish_declarations` runs the passes inside `worker_pool().install` (`project.rs:1395-1399`), so they run on a worker thread whose stack holds no frame of the caller. A stub base's finishing run therefore counts as `finishing`, not as its base's phase. In the off and cold runs that is ≈0.15 M blocks, by difference (§4.2).
- **Holders.** A block's holder is the innermost frame that names one, with the 20 G report's categories: trees, declarations, the docblock memos, names and so on (`heap.py`, `HOLDERS`).
- **The allocating function.** This is the innermost frame of our code that is not a container, a conversion or a utility (`to_string`, `clone`, map inserts, `PhpString`'s constructors, serde), as M10 defined it.
- **Items.** Each program point gets an item by M10's rules, applied in order:
  - its regexes on the holder and the allocating function;
  - its tail mechanisms (F1-F7, M11);
  - then corrections from reading the allocating lines (`items.py`, `OVERRIDES`).
  
  In the stub phases of the off and cold runs, a block goes to P6, which removes all of them. §4 says which proposal removes it without P6.
- **Checks.**
  - The vectorized classifier was checked against per-stack classification on 30 K samples of each of the off profile and the 20 G profile, with no mismatch.
  - The item table was computed on the 20 G profile with the same rules (§10).
- **Scripts.** `dhat-heap-modes/analysis/`, with a README one level up (§13).

## 3. Where the allocations go, by phase

| phase | warm blocks | warm MB | off blocks | off MB | warm at the peak | off at the peak | warm at exit | off at exit |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| parse | 9,328,936 | 1,600.0 | 9,282,124 | 1,600.3 | 826.6 | 835.8 | 384.0 | 393.4 |
| check | 7,142,812 | 614.3 | 7,115,900 | 608.2 | 0 | 0 | 68.5 | 65.9 |
| stub-sources | — | — | 4,271,980 | 574.9 | — | 82.1 | — | 82.1 |
| finishing | 2,131,182 | 304.9 | 1,834,223 | 263.3 | 0 | 10.6 | 151.7 | 115.1 |
| stub-base-phpstan | 133 | 17.1 | 704,400 | 167.4 | 17.1 | 31.1 | 17.1 | 9.8 |
| stub-base-plain | 132 | 6.6 | 405,502 | 78.1 | 6.6 | 6.5 | 6.6 | 6.5 |
| vendor | 324,988 | 32.0 | 324,988 | 32.0 | 2.0 | 2.0 | 2.0 | 2.0 |
| session-load | 308,661 | 50.9 | 308,669 | 51.8 | 14.0 | 14.5 | 11.1 | 11.4 |
| stub-corpus-merge | — | — | 233,162 | 59.0 | — | 13.4 | — | 13.4 |
| stub-corpus-store | — | — | 189,736 | 67.9 | — | 0.1 | — | 0.1 |
| insert | 131,127 | 24.8 | 124,612 | 24.3 | 0 | 0 | 14.0 | 13.7 |
| main | 62,243 | 13.9 | 62,243 | 13.9 | 0.3 | 0.3 | 0.3 | 0.3 |
| session-run | 7,810 | 1.7 | 7,810 | 1.7 | 0 | 0 | 0.2 | 0.2 |
| stub-index | 299 | 2.4 | 1 | 0 | 0 | 0 | 2.2 | 0 |
| unrooted | 19 | 0.7 | 37 | 0.8 | 0 | 0.1 | 0 | 0.1 |

- **The parse** is 48% of the warm run's allocations, and all of its peak but 40 MB.
- **The check** allocates 7.1 M blocks, 37% of the warm run's, and keeps 68 MB. It has nothing live at the peak, because it starts after the peak.
- **The finishing passes** are the second largest holder at exit (152 MB warm). In the warm run that includes the stub classes they decode (§6).

**By holder (warm).**

| holder | blocks | MB | at the peak | at exit |
|---|---:|---:|---:|---:|
| trees | 4,981,158 | 853.2 | **569.1** | **150.8** |
| checker | 2,701,343 | 283.8 | 3.8 | 4.4 |
| types, other | 2,604,199 | 305.7 | 44.7 | 80.3 |
| names | 2,251,733 | 193.8 | 29.7 | 47.6 |
| phpdoc | 2,127,980 | 204.1 | 48.9 | 65.8 |
| declarations | 1,467,532 | 360.8 | 62.8 | **145.5** |
| lexer, other | 650,802 | 65.0 | 8.7 | 12.5 |
| purity and property-write tables | 603,939 | 48.8 | 0 | 28.2 |
| session and load | 550,728 | 134.1 | 14.5 | 2.6 |
| project, other | 365,042 | 25.1 | 4.3 | 4.4 |
| own, other | 295,436 | 54.7 | 6.2 | 17.5 |
| body summaries | 228,567 | 14.9 | 11.4 | 11.4 |
| file states and indexes | 190,994 | 43.2 | 24.8 | 26.4 |
| docblock memos | 163,508 | 24.6 | 14.8 | 17.0 |
| stub and base records | 111,778 | 35.6 | 20.6 | 32.0 |
| registry, other | 97,782 | 7.9 | 1.0 | 3.3 |
| type interner | 31,239 | 11.3 | 1.2 | 5.5 |
| other memos | 14,564 | 2.7 | 0.4 | 2.7 |
| unattributed | 18 | 0 | 0 | 0 |

## 4. What the off run adds: 5.43 M blocks, 874 MB, +130 MB at the peak, +56 MB at exit

| phase | blocks | MB | at the peak | at exit |
|---|---:|---:|---:|---:|
| stub-sources | +4,271,980 | +574.9 | +82.1 | +82.1 |
| stub-base-phpstan | +704,267 | +150.3 | +14.0 | −7.3 |
| stub-base-plain | +405,370 | +71.5 | −0.1 | −0.1 |
| stub-corpus-merge | +233,162 | +59.0 | +13.4 | +13.4 |
| stub-corpus-store | +189,736 | +67.9 | +0.1 | +0.1 |
| finishing | −296,959 | −41.6 | +10.6 | −36.6 |
| parse | −46,812 | +0.2 | +9.2 | +9.3 |
| check | −26,912 | −6.1 | 0 | −2.6 |
| insert, stub-index, the rest | −6,786 | −2.0 | +0.4 | −2.1 |

By mechanism, over the stub phases (`groups.py`):

| mechanism | blocks | MB | at the peak | at exit | removed without P6 by |
|---|---:|---:|---:|---:|---|
| serialization that no store keeps | 1,256,817 | 264.6 | 22.4 | 0 | P2 |
| the stub sources' PHPDoc | 2,190,487 | 217.6 | 56.1 | 56.1 | P7 and item 14 (churn); the memos' bound (exit, §8) |
| the stub sources' lexer and parser | 1,372,985 | 219.4 | 2.5 | 2.5 | items 9, 10, 12 and 15 on the stub parse |
| the stub sources' declarations and index | 706,107 | 137.9 | 23.5 | 23.5 | kept: the stub declarations |
| the stub corpus's merge | 233,162 | 59.0 | 13.4 | 13.4 | P4 (`build_closures`, 8.9 K blocks / 9.6 MB); the rest is kept |
| the stub bases' own work | 45,218 | 48.7 | 15.2 | 16.2 | kept; P1 drops the plain base's |

### 4.1 The stub sources: 4.27 M blocks, 575 MB

- **PHPDoc is half of it:** the `phpdoc` holder has 2.04 M blocks. The tag tokenizer alone is 1.20 M blocks in the stub sources, and 1.85 M over the off run:
  - `lex_identifier` (`grammar/lexer.rs:127`), 320 K;
  - `TagParser::advance` (`grammar/parser.rs:98`), 280 K;
  - `remaining_description` (`parser.rs:309`), 282 K + 53 K, 26.5 MB;
  - `token_text`, 133 K;
  - `tokenize`'s token vectors (`lexer.rs:286`), 67 K blocks of 52.6 MB.
  
  The rest is `docblock::parse` (`docblock.rs:337`, `:339`, 179 K), `tag_block`'s memo key (`:497`, 133 K) and `apply_tag` (`:915`, 41 K). P7 tokenizes a description only when it is read. Descriptions are 84% of the stubs' tag tokens, so P7 removes most of the tokenizer's 1.20 M. Item 14's borrowed tag tokens remove the per-token blocks first (§11).
- **The PHP lexer and parser** are 1.37 M blocks:
  - `name_and_bits` (`lexer.rs:439`), 161 K;
  - `parse_type_hint`'s nested vectors (`type_hints.rs:102`, `:135`), 160 K;
  - `parse_qualified_name`, 68 K;
  - `resolve_as` (`name_resolution.rs:382`), 58 K;
  - `NativeType::from_name`'s lowercase (`ast.rs:1068`), 48 K;
  - `parse_args` (`postfix.rs:277`), 46 K / 20 MB.
  
  These are the same sites as the analysed files' parse, and the same items remove them.
- **Kept to exit: 82.1 MB.** This is the stub declarations and what the memos keep:
  - `ClassReflection::from_def` (`class_info.rs:1616`), 8.7 MB;
  - `merge_stub_classes` (`index.rs:600`), 6.6 MB;
  - `stub_entries` (`stubs.rs:1068`), 4.7 + 1.9 MB;
  - `dedup_versioned_params`, 4.2 MB;
  - the tag memo's `DocBlock`s, 28.7 + 3.2 MB;
  - `parse_tag_rest_shared`'s memo, ≈8.6 MB;
  - `doc_names_of`, 5.1 MB.
  
  `stubsites-exit.txt` lists every site.

### 4.2 The stub bases and the finishing passes

- **The PHPStan base: 704 K blocks.** Of these, 470 K are `stub_cache::encode`:
  - `FunctionReflection`, 355 K blocks / 32.2 MB;
  - `ClassReflection`, 50 K / 40.5 MB;
  - `ClassOwners`, 48 K.
  
  `store_entries`' closures (`name_table.rs:1036`, `:1038`) add 163 K blocks, and `Writer::append` (`container.rs:74`) holds 36.6 MB over 13 reallocations. The base's declarations proper are small: `merge_stub_classes` 3.5 K blocks (4.6 MB kept), `insert_if_absent` 5.3 K, `clone_vec` 4.8 K.
- **The plain base: 406 K blocks.** Of these, 233 K are `encode` and 121 K are `store_entries`. Piwigo mentions PHPStan, so the plain base is built, kept to exit (6.5 MB) and never used past `expect_files`. That is P1's case. In the warm run the plain base is loaded and kept too: 6.6 MB at the peak and at exit (§6).
- **The finishing passes allocate 0.30 M blocks fewer in the off run.** The warm run's finishing passes decode stub classes (0.44 M blocks, §6), which the off run built in its stub phases. The off run's finishing phase also holds the stub bases' finishing runs (§2), which is what it has live at the peak (10.6 MB). By difference they are ≈0.15 M blocks: 1,834 K in the off run, against the warm run's 2,131 K less its 442 K of decoding.

### 4.3 Serialization that no store keeps: 1.26 M blocks, 264.6 MB, 22.4 MB at the peak

- The off run encodes every stub entry it would store, then drops the container: `encode` at `stub_cache.rs:23` (893 K blocks, 166 MB), `store_entries` (310 K blocks, 17.8 MB), `Writer::append`/`name`/`section` and `store_tables`.
- **Its buffer is live at the peak.** The PHPStan base is built at the peak (§7), so its `Writer`'s 18.3 MB body and 1.6 MB of names are part of the off run's peak.
- P2 gates the stores on a cache that can write, so this is the heap side of P2's 1,005 M Ir. It removes all 1.26 M blocks and 22.4 MB of the peak, on off runs.

### 4.4 The other phases

- **The parse is 47 K blocks lighter, and holds 9.2 MB more at the peak.** It is the same work: the PHPDoc memos are process-wide, and the stub build left entries in them that the analysed files hit. Against the warm run, the off run's parse and check allocate 12 K and 24 K fewer PHPDoc blocks, and the parse's PHPDoc holds 8.2 MB more at the peak.
- **The check** allocates 27 K fewer blocks, for the same reason (`check / phpdoc` −24 K).

## 5. What the cold run adds: the cache write

| phase | blocks | MB | at the peak | at exit |
|---|---:|---:|---:|---:|
| stub-base-phpstan | +40 | +15.3 | **+14.4** | 0 |
| stub-corpus-store | +53 | +10.8 | 0 | 0 |
| stub-base-plain | +39 | +5.3 | 0 | 0 |
| stub-index, stub-data | +302 | +2.3 | 0 | +2.2 |
| parse, check, finishing, the stub sources | −21,429 | +0.5 | +0.2 | −1.1 |
| **total** | **−20,995** | **+34.2** | **+14.6** | **+1.2** |

- **What the cold run does beyond the off run** is write the containers and read the stub index back (`stub-index`, 2.2 MB kept). The ≈21 K-block difference in the parse and the check is the memos again, run to run.
- **The writer** allocates its body by doubling: `Writer::append` (`container.rs:74`), 32 blocks of 70.8 MB over the four files. `Writer::finish` (`container.rs:129`) then copies the body into a second `Vec` with the header and the directory: 4 blocks, 31.4 MB. For the PHPStan base the body is 18.3 MB and the file is 15.3 MB. Both are live at once, at the peak.
- **H1 writes the file from the one buffer** (§11): −15.3 MB at the cold run's peak, −31.4 MB allocated.

## 6. The warm run's stub data: item 6

The warm run's stub data costs 475 K blocks and 90.7 MB, 23.6 MB at the peak and 68.7 MB at exit (item 6):

- **The cache files, read whole and kept** (`load_container`, `stub_cache.rs:53-54`): 22.6 MB in 3 blocks, 20.6 MB of them live at the peak. This is M5.
- **The finishing passes' scope, decoded in advance.** `Registry::warm` (`scope.rs:262-285`) decodes every class and function of the scope in parallel, before the passes read them (`project.rs:1413-1414`).
  - `decode<ClassReflection>` is 267 K blocks and 43.8 MB, and 27.5 MB stays to exit: 169 K + 23 K blocks from `scope.rs:271` and 60 K + 9 K from `:273`.
  - `decode<FunctionReflection>` is 139 K blocks and 7.8 MB, 6.3 MB of it kept.
  - Each decoded class copies its methods' encoded bytes into an `Arc<Vec<u8>>` (`name_table.rs:1208-1213`, 7.2 + 1.4 MB). A `Lazy` keeps room for the decoded method whether or not it is decoded (12.6 MB for 35,782): M5.
- **`Base::name`** (`name_table.rs:464`) makes 17.5 K blocks, and `load_base`'s tables 8.4 K blocks / 8.0 MB.

Item 6 reads records in place and decodes only what a pass reads. The off run builds the same declarations in its stub phases instead, as `from_def` and the stub sources' 82.1 MB kept (§4.1). P6, building the stub data at compile time, would make every run pay the warm run's 0.48 M blocks instead of the off run's 5.77 M.

## 7. The peak

| phase / holder | warm | off | cold |
|---|---:|---:|---:|
| parse / trees | 569.1 | 569.1 | 569.1 |
| parse, other holders | 257.5 | 266.7 | 267.0 |
| stub-sources | — | 82.1 | 82.0 |
| stub-base-phpstan | 17.1 | 31.1 | 45.5 |
| session-load | 14.0 | 14.5 | 14.5 |
| stub-corpus-merge | — | 13.4 | 13.4 |
| finishing (the stub bases' runs) | 0 | 10.6 | 10.6 |
| stub-base-plain | 6.6 | 6.5 | 6.5 |
| vendor, main, the rest | 2.3 | 2.5 | 2.5 |
| **total** | **866.6** | **996.5** | **1,011.1** |

- **When it falls.** `Session::load` parses every file into one `Vec<Read>` (`session.rs:747-783`), then calls `expect_files` on all of them (`:791`) before inserting any. `expect_files` (`project.rs:1211-1222`) builds the PHPStan stub base, or loads it from the cache, when a file mentions PHPStan. So the base is built while every tree is alive, and the stacks of the blocks live at the peak end in `expect_files` → `Project::phpstan_base`.
- **The trees are 569.1 MB** in all three runs: `parse / trees`, by this report's holders. The 20 G report's own categories gave 595.8 MB for "trees and their payloads"; this report's holders give 569.1 MB on its ten-frame profile too, so the difference is in the categories, not in the stacks. M1's mechanism stands as written: item 3 drops a context file's tree on the worker, which takes ≈420 MB of the trees off the peak.
- **What the PHPStan base adds to the peak:**
  - warm: 17.1 MB, its container read whole (§6);
  - off: 31.1 MB, of which 18.3 MB is the `Writer` body that nothing stores (§4.3) and 4.6 MB the merged classes;
  - cold: 45.5 MB, with `finish`'s 15.3 MB copy on top (§5).
- **What would move the peak.** Once item 3 has run, the trees at the peak are the analysed files' (≈150 MB). The stub build's 82 MB and the bases' buffers would then be a larger share of the peak. P2 and H1 remove the buffers, and P6 the stub build.

## 8. At exit

| phase / holder | warm | off |
|---|---:|---:|
| parse / trees | 150.3 | 150.3 |
| finishing / declarations | 81.9 | 59.4 |
| parse / declarations | 59.7 | 59.7 |
| parse / phpdoc | 48.9 | 57.0 |
| parse / types, other | 38.0 | 37.1 |
| check / types, other | 32.4 | 33.3 |
| finishing / purity and property-write tables | 28.2 | 28.2 |
| parse / names | 26.6 | 30.8 |
| finishing / names | 20.4 | 16.1 |
| check / phpdoc | 16.9 | 14.4 |
| stub-base-phpstan / stub and base records | 15.3 | 0 |
| parse / docblock memos | 14.8 | 12.6 |
| stub-sources (all holders) | — | 82.1 |
| the rest | 124.4 | 133.1 |
| **total** | **657.8** | **714.1** |

- **The largest kept sites (warm):**
  - `parse_args` (47.1 MB) and `parse_stmt_list_block` (29.7 MB): M2's lists that grow by `push`;
  - the decoded stub classes (27.5 MB, §6);
  - `ClassReflection::from_def` (27.0 MB);
  - `intern::insert_shared` (26.2 MB, M7);
  - the tag memo (24.6 MB, M6);
  - `load_container` (22.6 MB, M5);
  - `built_declaration` (20.3 MB);
  - `resolve_property_writes` (18.1 MB);
  - `Members::get_mut`'s `Arc::make_mut` copies (`members.rs:92`, 15.3 MB);
  - `NameTable::get_mut`'s (`name_table.rs:775`, 13.9 MB).
  
  The last two are M4's copies. Whole stacks put them at 31.9 MB together, against M4's 8.4 + 5.5 MB for the `ClassReflection` copies alone.
- **The off run keeps 56 MB more.**
  - The stub sources keep 82.1 MB: the stub declarations, built in memory rather than decoded, and the memos' stub entries.
  - The finishing passes keep 36.6 MB less, because they decode nothing.
  - **The process-wide PHPDoc memos are bounded by entries and key bytes, not by phase.** The stub build fills them before the analysed files are parsed. The tag memo holds 58.2 MB at exit in the off run against 24.6 MB in the warm run, and 34,128 of its entries are stub tags (28.7 MB of `DocBlock`s at 840 B each). Those entries also serve the analysed files: the off run's parse and check allocate 37 K fewer PHPDoc blocks than the warm run's (§4.4). Item 14's two memos, with fingerprint keys and values smaller than a whole `DocBlock`, shrink both sides of that.

## 9. Every allocation site, by the item that removes it

The rows of each column sum to the run's blocks.

| item | warm | off | cold | warm MB | warm at the peak | warm at exit | 20 G report (M10) |
|---|---:|---:|---:|---:|---:|---:|---:|
| P6, the stub build (off and cold only) | — | 5,773,115 | 5,770,977 | — | — | — | — |
| 9, trees in an arena | 4,326,042 | 4,324,281 | 4,324,784 | 818.0 | 580.0 | 159.6 | 4.24 M |
| 18, the check phase | 3,547,141 | 3,550,068 | 3,551,045 | 422.7 | 7.2 | 26.0 | 3.58 M |
| 15, names as names | 2,709,917 | 2,707,601 | 2,707,442 | 93.0 | 0 | 0.1 | 2.76 M |
| 14, PHPDoc | 2,659,363 | 2,615,362 | 2,590,310 | 262.2 | 69.1 | 93.3 | 2.66 M |
| 4, the registry by ids | 1,993,180 | 2,153,125 | 2,153,411 | 381.0 | 35.3 | 143.9 | 2.02 M |
| 13, a file's products | 1,310,136 | 1,310,120 | 1,309,121 | 188.3 | 81.3 | 79.3 | 1.31 M |
| 2, the walk and path data | 530,139 | 530,139 | 530,139 | 70.2 | 5.5 | 2.6 | 0.56 M |
| 10, flat token arrays | 520,620 | 498,602 | 504,520 | 112.0 | 13.0 | 10.6 | 0.52 M |
| 16, interned values | 497,672 | 468,365 | 468,048 | 54.0 | 12.5 | 34.2 | 0.47 M |
| 8, declarations | 482,691 | 502,406 | 502,392 | 57.0 | 12.0 | 12.0 | 0.48 M |
| 6, stub records | 475,194 | 40,797 | 40,797 | 90.7 | 23.6 | 68.7 | 0.45 M |
| 12, lexer details | 165,742 | 165,742 | 165,742 | 1.7 | 0 | 0 | 0.17 M |
| 5, merge and indexes | 110,459 | 115,573 | 115,573 | 28.9 | 10.9 | 20.1 | 0.11 M |
| 3, context files read into a worker's buffer | 59,799 | 59,799 | 59,799 | 66.5 | 9.0 | 0 | 0.06 M |
| 11, line starts and ignore marks | 48,724 | 48,724 | 48,724 | 20.9 | 7.1 | 7.1 | 0.05 M |
| made once per process | 1,523 | 1,569 | 1,569 | 2.5 | 0.1 | 0.2 | 0.001 M |
| unattributable | 0 | 0 | 0 | | | | 0.01 M |

- **Unattributable is empty.** The 20 G report's 11 K blocks whose stacks ended inside the allocator have whole stacks now.
- **The off run** moves 0.16 M blocks into item 4: the stub bases' finishing runs (§4.2). It moves 0.43 M blocks out of item 6, because nothing is decoded.
- **Where this table differs from M10** (≤0.09 M per item), it is from the rules below, not from the stacks (§10):
  - Item 9 gains 0.09 M blocks. `PhpString`'s constructors are no longer an allocating function, so a literal's string goes to the lexer site that makes it (`strings.rs:193`, `:196`, `:350`).
  - `ShapeKey::from_bytes` goes to item 16 wherever it allocates (182.6 K blocks).
  - In the finishing passes, the registry's own copies and rebuilt values go to item 4, as M4 has them:
    - `clone_vec` of parameters and of name pairs, 85 K;
    - `classify_purity`'s collect, 14 K;
    - `run_purity_fixed_point`;
    - the deferred-type resolutions;
    - `ClassReflection`'s whole-record clones, 16.6 K.
    
    The type operations those passes call stay in item 18.
  - Rayon's `LinkedList<Vec<…>>` collect nodes in finishing jobs that run on threads with no frame of ours go to item 4. Their element types are folded by identical code folding, so `lsp_types::…` in a name means nothing.

**The functions M10 did not read, now read** (Appendix A has every one, with its allocating lines and item):

| function | blocks | lines | mechanism | fix | item |
|---|---:|---|---|---|---|
| `RuleLevelHelper::failing_level` | 95.9 K | `rule_level_helper.rs:38` | a `Vec` and a `partition` into two more per call, to test the members | iterate the members twice, or keep the split in the checker's scratch | 18 |
| `Members::get_mut` | 63.1 K, 17.5 MB kept | `members.rs:90-92` | `Arc::make_mut` copies a method shared with the as-built snapshot, and the map is copied when shared | item 4's id tables, where a pass writes its own column | 4 |
| `NameTable::get_mut` | 12.1 K, 14.4 MB kept | `name_table.rs:775` | the same for whole classes | item 4 | 4 |
| `FunctionReflection::map_types` | 44.2 K | `function_sig.rs:372`, `:380` | the checker maps a whole signature, rebuilding the parameter list and the assert lists (`:366-380`), at each use | item 18's one binding per call, which every rule borrows | 18 |
| `Type::referenced_class_names` | 42.7 K | `queries.rs:227` | collects into a `Vec` to iterate, with a linear `contains` | a visitor, or a reusable buffer | 18 |
| rayon collect nodes in finishing jobs | 33.9 K + 33.9 K, the largest pair | the purity passes' dependency lists (`(Name, Name, Option<Vec<PurityDependency>>)`) | `par_iter().collect()` into a `LinkedList` of `Vec`s, on a thread with no frame of ours | item 4's per-id columns filled in place | 4 |
| `resolve_from_static_schema` | 29.7 K, 10.1 MB | `sql_reflection/resolve.rs:386` | `sqlparser` parses the same query's text at every call | parse once per distinct text (F4) | 18 |
| `mark_template_params` | 25.2 K | `generics.rs:142` | a `TemplateParam` clone for every occurrence | the parameter by id | 8 |
| `collect_class_name_case_refs` | 18.1 K | `name_context.rs:61-62` | two `Name`s per resolved class name, for `class.nameCase` | item 8's case references in the checker | 8 |
| `Base::name` | 17.5 K | `name_table.rs:464` | a stored entry's `Name`, built from its text at the first read and kept | item 6's records read in place | 6 |
| `check_phpstan_api_usage` | 17.1 K | `phpstan_api.rs:98` | `ctx.namespace().to_string()` for every statement | borrow the namespace (item 15's rule) | 15 |
| `PhpString::concat`, `repeat` | 13.1 K | `php_string.rs:60`, `:65-68` | constant folding of `.` and `str_repeat` | kept: these are values | 18 |
| `intersect_types` | 5.3 K | `intersection.rs:13` | on a memo miss, the members are cloned into `intersect` | pass them by value, since the memo needs only the slice | 18 |
| `Project::check_files` | 2.8 K | `project.rs:2159` | each result's path cloned | the file id (item 5) | 5 |
| `fold_doc_comments` | 1.9 K, 7.3 MB | `lexer.rs:204` | the folded comment's text | item 10's token arrays | 10 |
| `FileTable::with_paths` | 3 blocks, 8.4 MB kept | `file_table.rs:52-53` | the slot table | kept (item 5 sizes it) | 5 |

The off run's 90 unread functions outside the stub phases are the warm run's, plus `NameTable::forget`, `end_declaration_changes` and `with_function_key`: 43 blocks. Its 82 unread functions in the stub phases are `encode`, `store_entries`, the `Writer`, `stubs::*` and `packed::*`. They are mapped to P6, and §4 says which proposal removes them without it.

## 10. Whole stacks against ten frames

- **Totals** agree within run-to-run noise (§1).
- **Phases** need whole stacks. With ten frames, 12.25 M of the 19.44 M blocks (63%) reach no phase root, and 582 MB of the 866 MB peak. With whole stacks, 18 blocks do.
- **Holders** barely move:
  - declarations −33 MB and −231 K blocks, to the checker (+50 MB, +307 K): rule code that calls into the registry;
  - the 82,817 unattributed blocks go down to 18.
- **Items** barely move. The same rules on both profiles give:

  | item | ten frames | whole stacks | difference |
  |---|---:|---:|---:|
  | 13 | 1,294,350 | 1,310,136 | +15.8 K |
  | 18 | 3,561,714 | 3,547,141 | −14.6 K |
  | 2 | 523,517 | 530,139 | +6.6 K |
  | every other item | | | ≤1.7 K |

  Ten frames sufficed for M10's map, because an allocation's own site is near the top of its stack.
- **What M1-M11 gain from whole stacks:**
  - M1's peak, now placed in `expect_files` (§7);
  - M4's copies, now 31.9 MB of `make_mut` copies kept (§8);
  - M5's decoding, now split by its caller, `Registry::warm` (§6).
  
  None of their mechanisms changes.

## 11. Proposals

### H1. Write the cache file from one buffer

- **Now.** `Writer::finish` (`container.rs:120-142`) builds the file as a new `Vec`:
  1. it reserves the header, the body and the directory (`:129`);
  2. it copies the body in (`:136`);
  3. it checksums `file[HEADER_LEN..]` (`:138`) and patches the header.
  
  `store_container` writes that `Vec` (`stub_cache.rs:84`). The body and the file are both live, at the cold run's peak (§5): 18.3 + 15.3 MB for the PHPStan base.
- **Design.** `Writer::new` starts the body with `HEADER_LEN` zero bytes. Spans are offsets in the file already (`append`, `:69`), so they stay the same. `finish` appends the directory to the body, computes the checksum over `body[HEADER_LEN..]`, writes the header into the first bytes and returns the body. The directory is the only new allocation.
- **Saving.** −15.3 MB at the cold run's peak, and −31.4 MB and 4 blocks allocated in a cold run. Nothing changes on disk.
- **Checks.**
  - `body_len` (`:65`, tests only) would count the header. The test adjusts by `HEADER_LEN`.
  - `append`'s four-gigabyte checks already add `HEADER_LEN`, so they hold unchanged.
  - `Reader::new`'s layout check reads the same bytes.

### Measured here: P1, P2, P6, P7 in heap terms

| proposal | blocks | MB | at the peak | at exit | runs |
|---|---:|---:|---:|---:|---|
| P1, only the stub base the files need | −405 K | −78.1 | −6.5 | −6.5 | off and cold, when a file mentions PHPStan |
| P1, in the warm run | −132 | −6.6 | −6.6 | −6.6 | warm (the plain base's container is loaded and kept) |
| P2, serialize only what a store keeps | −1,257 K | −264.6 | −22.4 | 0 | off |
| P6, the stub data at compile time | −5,773 K, +475 K of item 6 | −944 + 91 | −133 + 24 | −112 + 69 | off and cold (§6) |
| P7, a tag's description tokenized when read | ≈ −1.38 M off, ≈ −0.37 M warm | ≈ −97 off, ≈ −25 warm | ≈0 | ≈0 | all; after item 14, the per-token blocks are gone first |

- **P1 in the warm run is new.**
  - The off report gave P1 to the off and cold runs only, because loading the plain base costs little Ir.
  - In memory, the warm run still loads and keeps the plain base: `Session::load` resolves it before the parse (`session.rs:733`), then `expect_files` switches to the PHPStan base. That keeps 6.6 MB from the peak to exit.
  - Deciding the base after the parse, as P1 does, removes it.
- **P2's gate** also takes the PHPStan base's 18.3 MB write buffer off the off run's peak.
- **P6** removes the off run's 5.77 M stub-phase blocks (944 MB; 133 MB at the peak, 112 MB at exit). Every run then pays the warm run's item-6 decoding instead: 475 K blocks, 91 MB, 24 MB at the peak and 69 MB at exit (§6). The off report's §10.6 records that P6 is not built in today's form, and the off report's C8 (§13) takes the decision again after the crate split's version key. Until then, P1, P2, P7 and H1 are what lower these runs.
- **P7's estimate.**
  - It applies the off report's description shares, by token, to the tag tokenizer's blocks: 84.4% of the stubs' 1.20 M and 56.5% of the analysed files' 0.65 M.
  - The 0.65 M is the same in the off and warm runs, because the analysed files' tokenizer allocates as much in both.
  - Blocks per token are taken as even, so this is an estimate, as P7's Ir figure is.

### What the items gain beyond M10

- **Item 4** also covers:
  - the rayon collect nodes of the finishing jobs (§9);
  - the `ClassReflection` clones;
  - the copies `make_mut` makes, measured at exit (31.9 MB).
- **Item 6** also covers `Registry::warm`'s decoding in advance: 406 K blocks, 33.8 MB kept. A pass reading records in place has nothing to decode in advance.
- **Item 14** also covers what the stub build leaves in the PHPDoc memos (§8).
- **Items 8, 15 and 18** take the new sites of §9's table, each by a design already in the item.

## 12. What this drill does not establish

- **Resident memory.** dhat counts requested bytes over the system allocator. mimalloc's resident size, its fragmentation and the tree's page behaviour are not in it.
- **Time.** The runs' times under the profiler only order the phases. Nothing here is a timing.
- **One run per mode, on 8 threads.** The per-thread memos and the order of stolen jobs differ run to run, by ≈0.1% of blocks (§1).
- **The per-file cache was off.** A run with it warm parses no context file, and this profile cannot show that run.
- **The language server.** M3's case, which keeps every tree and source, is not profiled.
- **P7's saving** is estimated from token shares, as in the off report. It needs a build to measure.
- **The split of the off run's finishing phase** between the project's run and the stub bases' runs is by difference (§4.2).

## 13. Reproduction

All paths are under `~/.cache/prs-profile/dhat-heap-modes/`. `README` there lists every file.

- **Runs.** `run.sh` (off, cold, warm in turn), with `fp/bin/phpstan-rs`. To build it from the repo:

  ```sh
  RUSTFLAGS="-C link-arg=-fuse-ld=mold -Z threads=8 -C force-frame-pointers=yes" \
    cargo build --profile dhat -p phpstan-rs --bin phpstan-rs --features dhat-heap
  ```
  
  `fp/bin/phpstan-rs.repo` is that build at `ff347308`.
- **Conversion.** `python3 analysis/convert.py <mode>/dhat-heap.json <mode>/prof` takes ≈5 min per run.
- **Keys and tables.** `python3 analysis/keys.py <mode>`, then `tables.py <mode>` and `items.py <mode>`, give `<mode>/keys.pkl`, `tables.txt`, `items.pkl` and `items.txt`. `ref20g/` is the 20 G profile, read through its `dhat.pkl`.
- **Differences.** `analysis/diff.py off warm` and `diff.py cold off` give `diff-off-warm.txt` and `diff-cold-off.txt`.
- **Stub mechanisms.** `analysis/groups.py off`, `phasesites.py off '^stub-' 160` (`off/stubsites.txt`) and `off/stubsites-exit.txt`.
- **Peak and exit.** `phasesites.py <mode> <phase> <n> gb|eb`, and `callers.py <mode> <site> <skip> …` for the outer callers (`Registry::warm`, `expect_files`).
- **The finishing split.** `stolen.py <mode>`.
- **Items.**
  - `phaseitems.py <mode> <phase> <item>` lists an item's functions in a phase.
  - `unread.py warm` and `unread.py off '^stub-'` list the functions the 20 G report did not read.
  - `vs20g.py <mode> <item>` compares a function's item against `dhat-20g/mem/mapped_code.txt`.
- **Output identity.** `cmp off/out.json warm/out.json` and `cmp off/out.json cold/out.json`.

## Appendix A. The functions read for this report

The 97 functions of the warm run that the 20 G report did not read, each with its blocks, bytes, MB at the peak and at exit, its item and its two largest allocating lines, are in `warm/unread.txt`. The off run's are in `off/unread-nonstub.txt` (outside the stub phases) and `off/unread.txt` (all).

| item | functions | blocks |
|---|---:|---:|
| 18 | 20 | 233,564 |
| 4 | 35 | 149,245 |
| 8 | 2 | 43,236 |
| 6 | 3 | 17,857 |
| 15 | 2 | 17,099 |
| 5 | 5 | 2,909 |
| 10 | 1 | 1,878 |
| once (argument parsing, configuration, the pool, lazy statics) | 27 | 560 |
| 13 | 1 | 218 |
| 15 and 18 (`preg_match_all_pattern_order_shape`) | 1 | 145 |
| **total** | **97** | **466,711** |

The functions of more than 1 K blocks are in §9's table. Below that:
- the remaining rayon collect nodes, `imbl` set growth and `NameEntry::or_insert_with` in finishing jobs (item 4);
- the Doctrine result row (`result_shape.rs:422-423`), `Shape::from_named`, `regex_automata`'s compiler for patterns built at run time, and `report_missing_typehint_findings` (item 18);
- `anonymous_class_decls` (item 13), `scoped_class_names`, `scoped_function_names` and `Scope::of` (item 5).
