# Part M. Ledger of the workflow review's findings (2026-10-18)

<!-- status:begin -->

## Status of this part's tasks

From the ledger at `13ff6c1e1` on 2026-10-19. Regenerate with `scripts/plan_status.py`; change the ledger, not this table. `also in` lists the other parts that name the task.

| task | status | ledger | also in |
|---|---|---|---|
| REG-15 | OPEN | OPEN | 06 |
| REG-15.1 | OPEN | OPEN | 06 23 |
| REG-17.2 | OPEN | OPEN | 10 11 20 |
| STUB-11 | OPEN | OPEN | 05 06 07 10 12 21 23 |
| CHECK-9 | READY | READY | 05 06 07 10 11 12 23 31 |

<!-- status:end -->

A workflow (run `wf_cd83ade2-42f`) of 13 independent, read-only reviewers read the whole plan, one part or aspect each, and
returned **267 findings: 1 blocker, 84 major, 163 minor, 19 nit**. By basis, 182 cite lines the reviewer read (q), 72 are
inferences (i) and 13 rest on knowledge from outside the repository (x). The user asked for no verification inside the
workflow, so the rows began as one reviewer's claim each. **All 267 were then checked one by one in the main thread, in
ledger order, each against the source it cites (265 hold, one holds in part, one could not be checked here), and the
confirmed findings were folded into the plan.** The ones that depart from a ruled decision were ruled on by the user on 2026-10-18 (M5). Once every row was applied,
the ledger rows were deleted; this part keeps the summary.

- **Data.** `~/.claude/projects/-home-torres-projects-phpstan-rs/0fdc40b5-accc-4267-853a-f5ef8f30c07c/subagents/workflows/wf_cd83ade2-42f/journal.jsonl`:
  one `result` line per reviewer (`aspect`, `read`, `checked_ok`, `findings`, `not_reviewed`); a finding has `id`, `where`,
  `severity`, `kind`, `claim`, `problem`, `evidence`, `basis`, `fix`. The journal is the only full record of each finding
  now that the ledger rows are gone.

| key | reviewer's range | BLK | maj | min | nit | total |
|---|---|---|---|---|---|---|
| A | Part A and the conventions | 0 | 7 | 8 | 2 | 17 |
| B | Part B, the flat tree, and I1 | 0 | 4 | 11 | 1 | 16 |
| G | G1-G4: lifetimes, aliases, error nodes and recovery, the probe | 0 | 9 | 14 | 1 | 24 |
| T | I2 tokens, I3 line kernel | 0 | 6 | 15 | 1 | 22 |
| P | I4 type-grammar lowering, I5 message type | 0 | 6 | 11 | 2 | 19 |
| N | H1 identity and names | 0 | 6 | 10 | 2 | 18 |
| E1 | H2, H3: the engine and the levels | 0 | 6 | 11 | 1 | 18 |
| E2 | H4-H6: pipeline, persistence, the spikes | 1 | 12 | 10 | 1 | 24 |
| W | Part D and Part J: the wall | 0 | 9 | 12 | 1 | 22 |
| S | the sequence, its dependencies, interim work | 0 | 6 | 13 | 3 | 22 |
| C | consistency, staleness, drift from the code | 0 | 4 | 14 | 2 | 20 |
| X | cross-platform, robustness, dependencies | 0 | 4 | 18 | 2 | 24 |
| V | verification, measurement, process | 0 | 5 | 16 | 0 | 21 |
| | | 1 | 84 | 163 | 19 | 267 |

## M1. How the checks were made, and what they found

Every row was checked in the main thread, one at a time, in ledger order (A, B, G, T, P, N, E1, E2, W, S, C, X, V), by
reading the source the finding cites, with a one-line announcement of each before its reads; no row was sampled or
taken from another's check. The tools were the repository's own files and history (`git`), `php` 8.5.4 for runtime
truth (`php -l`, `token_get_all`, short scripts), the sources of salsa 0.28.5, rayon, stacker, wide, memchr and
triomphe 0.1.14 in the cargo registry cache, a toy `rustc` probe on the pinned nightly for the macro-lint question
(E1-16, X-07), and counts by `git grep` at HEAD.

| outcome | rows |
|---|---|
| holds | 265 (the claim is true as the finding states it; where the finding's count, line or detail was off, the row's note says what is right) |
| part | N-10 (PHP 8.5's constant rules reproduced; the PHP 7 case-insensitive form could not be run) |
| unver | W-07 (needs PHP 7.x reflection: no older PHP is installed) |
| open | 0 |

Findings the checks changed the shape of: the blocker E2-01 (salsa flattens onto base-input leaves, not onto persisted
queries: H4.6 persists the whole chain), E1-16 and X-07 (the unsafe exception of Part E 33 is probably unnecessary),
G-08 (the lifetime on `Locals` is built in item 9 and deleted in Stage D: G1 roots the stored expressions instead),
A-04 and C-11 (counts), G-04 (the backtick is deprecated and still valid), and N-02 (triomphe's thin pointer exists only
for slices).

## M2. The same issue found by more than one reviewer (each group was checked once, under its first id)

`=` the same issue, `~` related. The others close with the first id of their group.

| issue | findings |
|---|---|
| `vendor_dir` finds only a vendor directory that is a direct child of the manifest's directory (`vendor-dir: libs/vendor` regresses against the name test) | A-01 = X-03 |
| Composer's `vendor-dir` forms the plan does not specify (`COMPOSER_VENDOR_DIR`, `{$name}` references, `~`, `$VAR`, the absolute-path test) | A-07 ~ E2-24 ~ X-03 |
| A2 calls A1's `vendor_dir`, so it cannot be built first | A-11 = C-06 ~ S-10 |
| no item-0 baseline exists when A6 validates (Part A comes before batch 0) | S-17 = V-08 |
| the CI gates job runs xtask steps one by one and never `cargo xtask gates` (checked: holds) | A-15 = X-08 = V-09 |
| the start condition is stale (R18 built, R6 landed) and Parts E and G name the other plan's commits | A-17 = S-21; C-01 ~ both |
| no validation exercises the degrade walk; real vendor trees would | A-06 ~ V-07 |
| class-body errors have no slot in `ClassNode` (there is no member enum) | G-03 = C-03 |
| G4b's fallback is built from what it deletes, and G4a is interim | C-02 = G-11 = S-13 ~ V-21 |
| F69 and CHECK-9 are placed three ways (batch 7, D2, with REG-17.2) | S-03 = W-03 |
| OVL-2 needs STUB-11's build, STUB-11 needs REG-15, REG-15.1 needs OVL | S-06 = W-02 |
| file numbers differ between a restored and a cold database; order by path rank | E2-06 = X-21 ~ E1-10 |
| the Ir figures for `LineIndex::new` disagree and their parts do not add up (63.5 + 83.4 + 116.3 = 263.2 M) | T-16 = C-09 = V-13 |
| counts that drifted: Visitor impls 49/51, `Locals` mentions 414/415, `TypeNode` variants 19 | C-05 = C-10 = G-24; C-11 ~ P-18 |
| the front-end digest hashes more than B8's dump lists | B-03 = V-04 |
| the plan departs from ruled wording of decision 4 (spans against an arena; a measurement settles spans against token indices) without saying so | B-08 = T-17 = S-11 |
| decisions 9 and 15 disagree on reporting a context file's findings | C-14 = S-12 ~ G-14 |
| a live SQL reflector breaks the engine's rule that a query reads only inputs | E1-01 = W-09 (in part) |
| salsa's generation check is a `debug_assert`, so a stale id reads its slot's new occupant in release | N-06 = C-18 ~ E1-15 |
| PHP's CR rule belongs to the lexer too (comments, heredoc labels, `?>`), not only to the line index | T-05 = X-10 |
| the anonymous-class name splits ambiguously when a path ends in `:digits` or holds a colon | N-09 = X-24 |
| persisting path bytes: `path_bytes` has no such API and Windows' bytes are unspecified | E2-17 ~ X-13 |
| `REG-8.8` does not exist | S-20 = C-08 |
| per-thread database handles: `for_each_init` makes one per rayon job, and the plan's snippet does not compile as written | E1-08 ~ X-23 |
| step 1's adapter, bridges and `legacy_mut()` are interim machinery the plan's own rule would reject | C-17 ~ S-08 ~ B-14 |

