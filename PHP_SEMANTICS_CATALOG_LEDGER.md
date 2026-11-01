# PHP catalog Phase F and remediation ledger

This is the authoritative status record for Phase F tasks and their remediation actions. Task evidence and evaluation methods are documented in [the evaluation report](PHP_SEMANTICS_CATALOG_EVALUATION.md). The semantic contract review is in [the contract review](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md).

## Phase F task and remediation ledger

Acceptance is by the author's review of the cited evidence; an independent review is not
required. Rows marked author-accepted were reviewed on 2026-10-12.

Statuses: **complete** means the stated evidence exists and passed its recorded
check; **partial** means evidence covers only the listed narrow slice; **open**
means the required evidence has not been produced or reviewed;
**deferred** means the work belongs to a later plan phase.

### F00.1 — Partial: Specify observable traces, assumptions, abstract order, and return-refinement rules
Acceptance: not accepted

**Reviewed evidence and current disposition:** Contract defines observable projections, trace order, host obligations, and return/exception distinctions. Review: `PHP_SEMANTICS_CATALOG_CONTRACT.md` now specifies observable projections, ordering, abstract order, assumption obligations, and return/exception refinement. `Evaluation` still returns projected exits/states/results and a replay certificate rather than an explicit trace set. Host implementations must preserve whatever ordered observations a closed projection needs. An independent adversarial review is recorded; its findings remain unresolved, so this is a working draft, not accepted semantics. See [F00 review](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md).

- **R00.2 — Complete (definition only; implementation conformance is tracked by R01.5)** (depends on: R00.1)
  Acceptance: accepted
  Scope: definition only
  Remainder: R01.5
  The contract defines host concretization (`gammaV`/`gammaS`) and conditional simulation obligations for entry tuples, event transfers, predicates, joins/widening, exits, and projections; narrows the evaluator claim to a host-relative post-fixpoint; and disclaims a PHP-soundness theorem and full trace-prefix coverage. It defines absent result IDs as undefined and rejects joins where a result exists on only one path. Evidence is provenance, not trust; trusted validators and cross-namespace support need pinned roots and validation. Tests cover initial-state retention through transfer and exit, joins/widening, dependency projections, and completion kinds. `widening_preserves_the_transferred_lower_bound_and_reflects_precision` checks that the transferred lower bound survives widening; it explicitly does not claim upper-bound preservation. Claude's independent re-review confirmed the semantic argument and allowed R00.2 to close as a definition after this scope correction. Cleanup's fatal, suspension, and native-unwind paths still violate the normative rule (RV-1); they remain open under R01.5 and are outside this subtask's implementation-conformance claim.

**Investigation, 2026-10-23 (status unchanged):** the cleanup rule in the contract (CONTRACT.md:124-137, 228-232, row 320) contradicts check_cleanup_replacement (lib.rs:412-423), which accepts a pending PhpThrow with Normal, PhpThrow, NativeUnwind, Fatal or Suspended replacements; the matrix test at lib.rs:5098 pins the wider set. The plan's return-refinement rules (PLAN.md:502: overlap, state token, guard) are absent from the contract: no hits for overlap, priority, state token, snapshot or disjoint. Closing needs an author amendment with PHP evidence for each cleanup cell (the oracle has no native-unwind cleanup case), the refinement rules or a Scope and Remainder line, and a reviewer other than the author (PLAN.md:890; EVALUATION.md:328). LEDGER:18 says an independent adversarial review is recorded, but CONTRACT_REVIEW.md:113-116 says that pass was same-author.

### F00.2 — Partial: Reject invalid trace compositions with counterexamples
Acceptance: not accepted

**Status note (review, 2026-10-09):** downgraded from Complete. The fatal and native-unwind counterexample row has no cleanup-path evidence, and the evaluator accepts those compositions (RV-1 in the [review record](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md#review-record-2026-10-09)).

**Reviewed evidence and current disposition:** Locked CBMC replay passes 93 valid-contract properties; invalid composition yields the expected `main.assertion.11` counterexample among 94. Review: The locked CBMC replay records 93 valid-contract properties and the expected invalid-composition counterexample among 94. This checks the listed algebraic counterexamples; it does not prove arbitrary host transfers or every concrete PHP trace. Keep counterexamples synchronized with any contract changes.

- **R00.4 — Complete (reduced C-model accounting and fail-closed CBMC runs)**
  Acceptance: accepted
  The [evaluation report's R00.4 accounting](PHP_SEMANTICS_CATALOG_EVALUATION.md#r004-cbmc-property-accounting-and-enforcement) maps all 93 valid-run properties, all 94 invalid-run properties, and both low-budget properties by exact ID, category, and contract relation; it marks every result C-model-only and classifies `main.assertion.11` and `main.unwind.0` as expected negative controls. `campaign.py` verifies the pinned CBMC binary, C source and reference digests, complete normalized property records (identity, status, description, source location, and trace), summary, and command exit; ten Python regressions pass. All three locked CBMC 6.6.0 commands passed through `EnforcedRunner` and yielded the expected 93/94/2 properties and exit codes 0/10/10 under resource enforcement. This closes only the accounting/enforcement task; it does not prove PHP or Rust evaluator semantics.

**Investigation, 2026-10-23 (status unchanged):** the RV-1 remediation (R01.5) has no reviewer record (CONTRACT_REVIEW.md:340-343). Contract line 330 cites widening_is_reflected_in_domain_precision, which does not exist; the investigator found the renamed test at lib.rs:5723. CBMC is not installed here, so the campaign transcripts were checked by recorded status counts only.

### F00.3 — Complete: Validate host transfer functions against PHP source behavior
Acceptance: accepted
Scope: the strlen host transfers, against PHP 8.5.4 source and the locked CLI
Remainder: R05.2

**Reviewed evidence and current disposition:** The `strlen` host transfers were checked against the locked PHP 8.5.4 CLI (42 programs in `results/evaluation/strlen-host-oracle-8.5.4.json`, pinned in `results/lock.json`) and against the locked source for the cited functions. The release transfer now models the destructor's throw, exit, fatal and suspension, and the evaluator keeps or replaces a pending PHP throw as PHP does. The strlen graph's only exceptional edge is the PHP-throw edge, so no other pending completion reaches cleanup. Found outside this scope and owned elsewhere: a legacy builtin call loses a `__toString` write (builtin-runs, `WORK_STACK.md`). The transfers and verdicts are in the [review record](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md#f00-3-host-transfer-validation-2026-10-12).

- **R00.3 — Complete (matrix/audit only; semantic validation is tracked by F00.3)** (depends on: R00.1, R00.2)
  Acceptance: accepted
  Scope: matrix and audit; the transfers and their unsupported outcomes
  Remainder: F00.3, R02.5, R05.2
  The [evaluation report's R00.3 matrix](PHP_SEMANTICS_CATALOG_EVALUATION.md#r003-source-to-host-matrix-php-854-strlen) covers weak/strict parsing, casts, Stringable dispatch, callback mutation/throw, release, unknown behavior, and the direct `ZEND_STRLEN` opcode path as well as `zif_strlen`. Each row distinguishes the PHP source fact, the actual host/artifact transfer, assumptions, observed positive/negative/contradictory cases, affected state/result/exit components, and what remains open. It closes the audit deliverable, not semantic conformance: R02.4 now supplies selected source-to-artifact evidence for the compiler-emitted opcode, while transfer semantics and unsupported outcomes remain open; callback effects/exits and release remain open under R05.2/R02.5; unsupported host behavior remains open under R04.1. R01.6 now validates the reviewed graph dependency rules and requires declared unknown-event domains to stay open.

### F00.4 — Partial: Specify distinct, validated assumption identities
Acceptance: not accepted

**Status note (review, 2026-10-09):** downgraded from Complete. The contract specifies typed assumption records. The code has no assumption records and no applicability check, and it accepts non-empty assumption vectors (RV-4). The title's "validated" is not met.

**Reviewed evidence and current disposition:** The contract defines typed records, canonical IDs, distinct evidence references, acyclic dependencies, and context-dependent validation. Schema v8 implements typed records (the current constant is SCHEMA_MAJOR 9, crates/php-semantics/src/lib.rs:33), a separate assumption namespace, canonical IDs, target/program context matching, manifest round trips, and combined evidence/assumption cycle rejection (R01.2). R01.7 implements deployment-pinned validator execution and R01.7.a threads freshly validated assumptions through catalog-backed projection, evaluation, and certificate replay. Context-free graph APIs still fail closed for closed coverage citing assumptions. Independent acceptance remains pending; see [F00 review](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md).

**Investigation, 2026-10-23 (status unchanged):** the RV-4 blocker is closed in code: typed, separately namespaced, validated assumption records exist and their tests pass. Acceptance is still not recorded, and the author must decide it under the 2026-10-12 policy. The shipped strlen artifact has no assumptions (the top-level list and every coverage item are empty) but carries 18 evidence records, so no shipped claim depends on a validated assumption. Stale text: the status note (line 53), 'independent acceptance remains pending' (line 55), and 'Schema v8' (the code uses SCHEMA_MAJOR 9). The investigator's git checks were refused, so its tests ran on a copy, not on 105b004f1.


### F00.5 — Complete: Review the contract and assumptions (author review, 2026-10-12)
Acceptance: accepted

**Reviewed evidence and current disposition:** The contract review is recorded in the [review record](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md#review-record-2026-10-09). Its findings are dispositioned in the review record's [2026-10-12 section](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md#dispositions-for-rv-25-and-contract-points-c-3-to-c-6-2026-10-12), which covers the points still open after the earlier passes. The author's review of 2026-10-12 accepts this item. The review occurred after F01 and later checker/native work had begun; those results are accepted under their own rows.

- **R00.1 — Complete** (depends on: none)
  Acceptance: accepted
  Review obtained and recorded 2026-10-09 by a reviewer: a Claude session. The maintainer reports that the ledger was being worked by Codex, so the reviewer is independent of that work. See the [review record](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md#review-record-2026-10-09). Its reviewer role, findings and dispositions are recorded there. The dispositions of these findings, and of RV-25 and contract points C-3 to C-6, are recorded in the review record's 2026-10-12 section. RV-3 is decided as defensible; RV-14 and RV-15 are fixed.
### F01.1 — Partial: Implement versioned graph, evidence, assumption, and domain-coverage schema
Acceptance: not accepted

**Reviewed evidence and current disposition:** Canonical digests, explicit coverage/dependencies, evidence and assumption validation, JSON round trips, combined evidence/assumption cycle rejection (R01.2), catalog-entry release-range validation (R01.3), dependency semantics for `NotApplicable` (R01.4), graph-derived mandatory dependencies for the reviewed transfer slice (R01.6), deployment-pinned assumption validation (R01.7), and noncanonical-order rejection (R01.8) have implementation and tests. Independent acceptance of R01.8 remains pending. Review of the producing commit found that `EvidenceRecord::id` hashed raw dependency and assumption vector order while semantically unordered permutations were accepted; `CatalogManifest::canonical_digest` also hashed producer order without validating it. R01.8 closes these by rejecting noncanonical vectors before catalog/manifest use.

- **R01.1 — Complete (typed assumption records and fail-closed references)** (depends on: R00.1)
  Acceptance: accepted
  Schema v8 adds separate typed assumption records with schema-bound content IDs, claim kind/text, target and optional loaded-program scope, prerequisite and supporting-evidence references, and validator identity/version. Manifest and catalog admission validate record shape, target-scope ranges/duplicates, evidence and assumption namespaces, required references, assumption propagation from evidence into coverage, and JSON round trips. R01.7 adds deployment-pinned contextual validator execution; R01.7.a provides catalog-backed projection/evaluation and replay while standalone graph validation remains fail-closed. Combined support-cycle checks are covered by R01.2; unordered-vector identity remains R01.8.
- **R01.2 — Complete** (depends on: R01.1)
  Acceptance: accepted
  Catalog admission checks one combined evidence/assumption support graph, covering assumption prerequisites, assumption-to-evidence support, evidence-to-assumption conditions, and evidence dependencies. Cycles return the closed path with each node's namespace and digest. Focused tests cover self-cycles, multi-node assumption cycles, cross-namespace cycles, and a valid DAG; a manifest-boundary test verifies malformed cyclic input receives the cycle diagnostic before its unavoidable content-ID mismatch. Valid content-addressed cycles cannot be constructed without a hash fixed point, but untrusted manifest structure is checked directly.
  Validation: all 3 focused cycle tests pass, including manifest-boundary rejection. The full semantics suite passes (70 passed, 6 existing ignored), `cargo check --workspace`, formatting, and diff checks pass.
- **R01.3 — Complete** (depends on: None)
  Acceptance: accepted
  `TargetPredicate::validate` now runs on catalog variant insertion and manifest validation/loading. Inverted ranges return `InvalidTargetPredicate(InvalidVersionRange)`; valid unbounded, exact, minor, and numeric boundary ranges remain accepted. Tests cover both insertion and manifest-load rejection, plus valid exact/minor and `u16` boundary values.
  Validation: focused release-range tests pass; the full semantics suite, workspace check, formatting, and diff checks pass.
- **R01.4 — Complete** (depends on: R00.1)
  Acceptance: accepted
  `NotApplicable` contributes no fact and may not appear anywhere in a `Closed` domain's transitive dependency closure. Every dependency edge is cycle-checked regardless of coverage. `Closed` claims are rejected when any transitive dependency is `Open` (including through an N/A node) or `NotApplicable`. During composition, open reasons, evidence, assumptions, and precision propagate across all dependency edges; N/A intermediates remain N/A while dependent claims widen. Tests cover all 3×3 direct `Closed`/`Open`/`NotApplicable` pairings, transitive closed-to-N/A rejection, the reported Return → Diagnostic → UserWrite path, propagation through that path during composition, and cycles through Open/N/A nodes.
  Validation: `cargo test -p phpstan-rs-semantics` passes (79 passed, 4 existing ignored); `cargo check --workspace`, `cargo fmt --check -p phpstan-rs-semantics`, and `git diff --check` pass.
- **R01.6 — Implementation complete; author-accepted (2026-10-12)** (depends on: R00.2, R00.3)
  Acceptance: accepted
  Graph validation derives required dependencies from reachable transfers and projection operands: normal `Return` requires `UserRead`/`UserWrite` for reachable read/write and PHP-flow state operations, including supported PHP-throw cleanup paths that return normally; output consuming an invocation result or an explicit result guard requires `Invocation`; closed PHP-throw/fatal/suspension projections in graphs with `Invoke` require `Invocation`. The same checks rerun after event coverage composition, so a child cannot make a previously `NotApplicable` output projection closed without the required edge. Unknown transfers still require each listed affected domain to be open. Mutation tests remove required edges and observe `MissingRequiredDomainDependency`; the graph test helper now uses a literal mapping independent of production `event_domains`, and a separate literal-table regression checks that production mapping (RV-21). An unrelated invocation does not contaminate a value-only return. The locked `strlen` builder records why its `StringByteLength(Argument(0))` return has no dependencies: the callback result is unused, there are no explicit state reads/writes, and the nonnegative result envelope is invariant under open callback effects. The builder regenerates the checked-in catalog byte-for-byte with pinned source, LLVM inventory, and runtime witness. RV-3's implementation gap is addressed; obtain a review before treating the finding as accepted.
  Validation: `cargo test -p phpstan-rs-semantics` passes (96 passed, 1 pre-existing ignored); `cargo check --workspace`, `cargo fmt --check -p phpstan-rs-semantics`, and `git diff --check` pass. The locked builder was rerun with the pinned PHP source, LLVM bitcode inventory, and runtime witness; `cmp` confirms byte-for-byte identity with `data/php-8.5.4-strlen.json`.
- **R01.7 — Implementation complete; author-accepted (2026-10-12)** (depends on: R00.2, R01.1)
  Acceptance: accepted
  Deployment-owned trust policy pins validator ID/version to an implementation digest and configured root; catalog data cannot add or authorize pins. Policy validation rejects duplicate/ambiguous pins, missing roots, and future revocations. `Catalog::validate_assumption` checks target/program context, requires exactly one matching executor and an exact implementation pin before execution, recursively validates prerequisites and transitive supporting evidence, and returns established only when the trusted executor accepts. Revocation takes effect at its policy revision; the canonical policy cache key changes on pin/root/revocation/revision changes. The deployment loader must verify the executor's loaded bytes against the declared digest and protect the configured roots. Cross-namespace support cycles are rejected under R01.2.
  - **R01.7.a — Complete (contextual projection, evaluation, and replay)**: catalog admission retains structurally valid assumption-backed graphs, while context-free graph validation/evaluation and projection remain fail-closed. The trusted projection/evaluation APIs require a catalog entry key applicable to the target, a loaded-program context, current trust policy, and matching executors. Evaluation validates every assumption used by closed coverage and stores the entry, target, program, policy key, and assumption IDs in its certificate. Certificate replay reruns validation under the current policy and rejects a changed key, target, program, entry, or assumption set. Tests cover context-free rejection, trusted projection/evaluation acceptance, serialized-certificate replay, revocation, and context substitution. Missing, stale, revoked, inapplicable, or indeterminate assumptions do not establish a claim.
  Validation: `cargo test -p phpstan-rs-semantics` passes (103 passed, 1 pre-existing ignored); `cargo check --workspace`, scoped formatting, and `git diff --check` pass.

- **R01.8 — Implementation complete; author-accepted (2026-10-12)** (depends on: None)
  Acceptance: accepted
  Identity methods sort semantically unordered vectors before hashing, so permutations produce the same record ID or digest. Validation also rejects noncanonical serialized ordering and duplicate members for evidence assumptions/dependencies, assumption prerequisites/supporting evidence, graph coverage and its domain/evidence/assumption vectors, open-reason and unknown-event-domain sets, target-predicate extension/INI constraints, and manifest entries/graphs/evidence/assumptions. Existing identities for canonical inputs remain unchanged, and ordered blocks/events remain order-sensitive. Regression tests cover each affected collection, verify duplicate rejection, and prove reordered events retain distinct graph digests.
  Validation: `cargo test -p phpstan-rs-semantics --all-targets` passes (109 library tests and 5 builder-example tests; 0 ignored); `cargo check --workspace`, `cargo fmt --check -p phpstan-rs-semantics`, and `git diff --check` pass.

**Investigation, 2026-10-23 (status unchanged):** the stated reason, pending independent acceptance of R01.8 (line 69), is superseded by the 2026-10-12 author-review policy. The failing condition is R01.7.a (line 93), which has no Acceptance line. The checkpoint validator regex (validate_checkpoint.py:136) matches only lines that start with `- **R<n>.<n> — `, so indented sub-rows and dotted ids such as R01.7.a are not read. The deployment-side digest and policy-root checks (contract:166-168) are not implemented anywhere in the repository.

### F01.2 — Partial: Implement evaluator control flow and exceptional continuations
Acceptance: not accepted

**Status note (review, 2026-10-09; remediation recorded below):** downgraded from Complete because cleanup `Return` or `Throw` replaced pending fatal, suspension, or native unwind (RV-1). R01.5 now rejects unsupported abrupt replacement cells and tests the completion matrix. F01.2 remains Partial until the independent acceptance gate in F00.5 is resolved.

**Reviewed evidence and current disposition:** Worklist, joins/widening, PHP/native exceptional exits, cleanup/resumption, and masked unknown transfers are tested. Review: Worklist evaluation, joins/widening, normal and exceptional continuations, PHP throw versus native unwind, fatal/suspended exits, cleanup/resumption, and masked unknown transfers have targeted fixtures. Tests reject join/widening losses and check loop convergence. Production-domain calibration and semantic-host soundness are separate open obligations. Calibrate domains for production behavior.

**Investigation, 2026-10-23 (status unchanged):** the status note (line 104) cites the F00.5 independent gate, which the 2026-10-12 policy supersedes. A Scope and Remainder line naming F01.5 and R05.2 is missing. R01.5 (line 122) and R05.2.4.3.2 (line 310) describe an older, narrower cleanup matrix; the code (lib.rs:402-430, 5098-5274) allows five pending-throw replacement kinds. The PhpThrow-to-NativeUnwind cell (lib.rs:5249-5257) has no oracle case, so it needs a fail-closed decision or evidence.


### F01.3 — Complete: Validate post-fixpoint certificates and reject incomplete graph behavior
Acceptance: accepted

**Reviewed evidence and current disposition:** Graph-bound replay checks transfer candidates, exits, and coverage; partial result definitions and missing/duplicate invocation coverage fail closed. Review: Certificates bind a graph digest and replay transfer candidates, successors, exits, and coverage; tests reject dropped results/pending outcomes, missing successors/exits, graph changes, and falsely closed coverage. The certificate trusts the supplied host's transfer semantics. Conservative extra stored exits are allowed, so the validator establishes a post-fixpoint over that host, not PHP correctness or a native-IR post-fixpoint. None for the implemented validation contract.


### F01.4 — Partial: Validate reference contracts with broader branch/callback cases
Acceptance: not accepted

**Reviewed evidence and current disposition:** Callback result/write/throw order, state-sensitive predicates, joins, cleanup, and second-callback branch selection are covered. Review: Callback result/write order, result-selected second callback, state-sensitive predicates, branch joins, callback throws, cleanup, and normal/throw state separation have fixtures. These are substantial reference-contract examples, but not a matrix reviewed by a party other than the author, nor exhaustive. F00.5 is recorded as accepted (2026-10-12), so the open part is the independent review that the plan requires (PLAN.md:890). Independent contract review remains F00-dependent; broaden combinations after review.

- **R01.5 — Implementation complete; author-accepted (2026-10-12)** (depends on: R00.2)
  Acceptance: accepted
  `GraphError::UnsupportedCleanupReplacement` now makes unsupported cleanup replacement fail closed in both evaluation and certificate replay. The only modeled replacement remains cleanup `Return` over pending PHP throw. A full evaluator matrix covers all four pending kinds against cleanup Return, PHP throw, native unwind, fatal, and suspension; it asserts the sole supported cell succeeds and every other cell returns the typed error. A certificate replay regression verifies the validator independently rejects a pending-fatal cleanup return. A separate matrix proves `Resume` preserves all four pending kinds. A callback throw also passes through a `Maybe` branch, distinct state writes, a join, and `Resume` without losing the pending throw. Existing six fatal/native-unwind/suspension Return/Throw counterexamples now fail closed. RV-1's implementation defect is addressed; obtain a review before treating the finding or parent F01.2 as accepted.
  Validation: `cargo test -p phpstan-rs-semantics` passes (90 passed, 1 pre-existing ignored); `cargo check --workspace`, `cargo fmt --check -p phpstan-rs-semantics`, and `git diff --check` pass. Workspace-wide `cargo fmt --all -- --check` reports pre-existing formatting differences in unrelated crates, so no unrelated formatting was retained.

**Investigation, 2026-10-23 (status unchanged):** the broader branch and callback matrix is not defined; 'broaden combinations' names no dimensions. The false arm of a branch predicate decided by a callback result has no fixture; only the true arm is exercised (lib.rs:3756-3813, 3904-3975). The gate text says F00.5 remains open, but F00.5 is Complete and accepted (lines 58-61), so the gate text needs correcting. No fixture was written.

### F01.5 — Open: Calibrate a production semantic host
Acceptance: not accepted

**Reviewed evidence and current disposition:** A narrow PHP host and PhpFlow/value fixtures exist. Review: A PHP-specific host and value/reference fixtures exist, including havoc for unknown callbacks and open coverage. It is still a narrow host, not a source-calibrated production semantic domain. In particular, the graph's `Stringable` invocation has open callback effects and exceptions. Replace toy/narrow transfers with calibrated, source-validated domains (depends on F00 and F02).

- **R04.1 — Implementation complete for the selected slice; author-accepted (2026-10-12)** (depends on: R00.3, R02.3)
  Acceptance: accepted
  Scope: the selected slice
  Remainder: F01.5
  The PHP host implements the exact `strlen` Return projection and weak-null Diagnostic projection against the source-linked graph, while parser rejection and callback throw/fatal/suspension remain explicit exits and effect/release domains remain Open. Source facts for `zif_strlen`, parser/cast behavior, Zend layout, and direct VM transfer are hash/fact checked by the builder; the evaluator emits a graph-bound post-fixpoint certificate and the embedded-artifact regression replays it before the checker consumes the projection. Weak scalar acceptance versus strict rejection and positive/negative argument paths are exercised by paired TAILCALL/CALL PHP fixtures; the direct/dynamic callback-order difference is retained in Open effect evidence instead of collapsed into the closed result claim.
  Validation: all 9 catalog-builder tests (including source/output/opcode/scope mutation checks), all 110 semantics tests, all 1,206 PHPStan library tests (one ignored), and the embedded-catalog certificate/evidence regression pass. The weak and strict runtime fixtures reproduce their checked-in artifacts byte-for-byte on both PHP profiles; outputs agree between profiles (SHA-256 `8c842c119b0e95205e77444bf1781ea8d7ecfa12edc487b8ad89a37e9803c8c0` weak; `fb700cd3f42668be8372b3cfc361d795167e25be2b5af673dd4e70f4a6b6856f` strict). The catalog artifact matches the builder output; formatting and diff checks pass. This closes only R04.1's bounded host-calibration deliverable; F01.5 remains open for general production-host calibration, and independent acceptance is pending.

**Investigation, 2026-10-23 (status unchanged):** production scope is undefined beyond strlen (CATALOG_FUNCTIONS = [strlen], semantics.rs:22). It depends on F00.1, F00.2, F00.4 and the F02 rows, none accepted, and on the R05.2.4.5.2 review. A maintainer must define the scope before any work starts.

### F02.1 — Partial: Lock PHP 8.5.4 source, compiler, build, and linked artifacts
Acceptance: not accepted

**Status note (review, 2026-10-09; remediation validated 2026-10-10):** downgraded from Complete after the review found stale `run.py` and `runtime.php` source hashes (RV-19). This remediation preserves the old runtime fixture under its historical filename and adds source/manifest digest enforcement to replay. The root lock now matches `run.py`, both runtime fixtures, and the checked link manifest; the source-lock and fixture/output regression tests pass. F02.1 remains Partial because the linked PHP bitcode is unavailable for an actual replay and the profile scope remains narrow.

**Reviewed evidence and current disposition:** Source commit, compiler identities, build/link manifests, and runtime profiles are recorded. Review: PHP 8.5.4 source commit, compiler/tool versions, build and link manifests, binary identities, and Hybrid/CALL runtime profiles are recorded. This does not cover other platforms, SAPIs, builds, or extensions. None for the locked experimental profile.

**Investigation, 2026-10-23 (status unchanged):** the replay exits with 'linked bitcode is unavailable' (evaluate_strlen_svf_demand.py:98). Missing: whole-clang/php-cli.bc (sha256 e3427ce8...), bison 3.8.2, re2c 4.4, SVF wpa and dvf at e889a513 with Z3 4.13.3, and the scratch root ~/.cache/php-catalog-evaluation. Restoring them needs network access this session did not have. The lock staleness (RV-19) is fixed: the source digests in results/lock.json match the live files.


### F02.2 — Complete: Inventory the linked `zif_strlen` LLVM instructions and trace the Return demand
Acceptance: accepted

**Reviewed evidence and current disposition:** All 241 instructions are accounted for; builder checks opcode counts/call targets and the field-to-return path. Review: The linked `zif_strlen` inventory accounts for 241 instructions (storage/layout/scalar/call/branch/return families), checks unknown opcodes and missing return, and traces the selected string-length field to the return payload. It is accounting plus a narrow demand witness; it does not interpret all instructions. Inventory is accounting evidence, not interpretation of all operations.


### F02.3 — Partial: Compare source, LLVM, and compiler representations
Acceptance: not accepted

**Reviewed evidence and current disposition:** Source/layout/helper and LLVM witness checks support the narrow Return/Diagnostic slices; Clang/GCC/CIR campaigns are recorded. Review: The AST/CFG, LLVM, GCC GIMPLE, and CIR campaigns expose representation behavior and counterexamples, including Clang CFG exceptional cleanup loss and poor CIR coverage. R02.2 implements a bounded structural correspondence for the selected `zif_strlen` normal path; it is not a general semantic adapter or broad transfer comparison. Broader native correspondence and useful closed summaries remain open.

- **R02.2 — Implementation and evidence regenerated; author-accepted (2026-10-12)** (depends on: None)
  Acceptance: accepted
  Added `build_strlen_representation_correspondence.py` and regenerated the hash-recorded [cross-compiler correspondence](tools/php-catalog/experiments/comparison/results/evaluation/strlen-representation-correspondence.json). For the selected `zif_strlen` slice, the builder checks source-to-Clang AST/CFG member access and result assignment, Clang translation-unit LLVM's `_zend_string.len` load and same-value zval result store, and matching GCC GIMPLE/CFG parser and result paths. It binds preserved Clang stdout archives to the captured producer output and GCC GIMPLE/CFG archives to the output paths derived from successful compile commands; producer/archive digests are checked before parsing (RV-27). The cleanup counterexample records GCC GIMPLE and Clang LLVM exceptional cleanup, while the Clang AST CFG omits an explicit destructor edge. CIR emission for `zend_builtin_functions.c` failed, so no CIR semantic correspondence is claimed. This is structural correspondence for the selected demand, not full transfer equivalence. Review remains required.
  Validation: the recorder regenerated archived Clang/GCC outputs and measurements from the preserved PHP 8.5.4 campaign; the builder regenerated the checked correspondence successfully and a second generation is byte-identical; all 21 focused campaign/provenance/replay tests pass, including path, digest, archive mutation, and malformed-run cases. The broader PHPStan and semantics suites also pass (1,234 and 112 tests respectively), along with formatting and diff checks.

**Investigation, 2026-10-23 (status unchanged):** correspondence covers only the zif_strlen normal parse-to-length-return path, structurally, not semantically. Missing: other transfers, path conditions, ABI effects and exceptional semantics. CIR covers 36 verified objects of 523 (the CIR frontend fails on zend_builtin_functions.c). The Clang AST CFG omits the Guard destructor edge that GIMPLE has (cleanup-ast-0.out). RecordedRootTests fails without PHP_CATALOG_SOURCE and needs a skip gate. The owner decides whether it closes on the narrow slice.

### F02.4 — Partial: Establish implicit-operation and normal/CALL behavior
Acceptance: not accepted

**Reviewed evidence and current disposition:** Paired weak/strict runtime witnesses and a 13-sentinel Hybrid/CALL comparison exist; Stringable invocation has pinned source/runtime evidence, independently replayed against the preserved historical fixture bytes. The 13-sentinel witness now names `runtime-hybrid-call-13.php`, restored from the locked PHP source commit, while `tailcall-call-reference-8.5.4.json` pins the current 16-sentinel `runtime.php`. The SVF evaluator checks the fixture selected by its witness and both paired outputs. Matching sentinels do not establish equivalence for unobserved inputs; callback writes/throws and other implicit operations remain only partly connected to modeled graph behavior. Expand beyond tested `strlen` scenarios and connect modeled operations to native behavior.

- **R02.1 — Implementation complete; author-accepted (2026-10-12)** (depends on: None)
  Acceptance: accepted
  `evaluate_strlen_svf_demand.py` requires exactly one witness record for its selected fixture, rejects a source digest mismatch before running, verifies both paired output files, and binds supplied linked bitcode to the digest in the PHP-commit link manifest. The 13-sentinel Hybrid/CALL witness is pinned to preserved historical `runtime-hybrid-call-13.php`; the newer 16-sentinel fixture remains separately pinned by `tailcall-call-reference-8.5.4.json`. Tests cover fixture mutation, output identity, bitcode mutation, source-commit disagreement, and an unavailable linked module. RV-11/RV-12/RV-26 mechanisms and root-lock hashes are addressed; actual SVF replay remains pending because the pinned linked bitcode is unavailable, and independent acceptance is open.
  Validation: all 21 focused campaign/provenance/replay Python tests pass; the Hybrid/CALL output artifacts match their locked digests. The live source and link-manifest locks match the checked files.

**Investigation, 2026-10-23 (status unchanged):** the checker does not carry ordered heap-state deltas or destructor and callback write effects (R05.2, R05.2.4, R02.7), and fatal, suspension and native-unwind cleanup replacement is not carried either. Scenarios are strlen only. The 13- and 16-sentinel hashes match the recorded files. The locked HYBRID and CALL binaries are not on this machine, so CALL was checked with the system PHP 8.5.4 CLI only. The owner decides whether it closes for the strlen slice.

### F02.5 — Open: Interpret the wider native `strlen` behavior
Acceptance: not accepted

**Reviewed evidence and current disposition:** Return and null diagnostic slices are represented; the 241-op inventory fails closed on unknown operations. R02.3 adds source-linked scalar/cast/release facts, an observed possible callback PHP throw, and distinct open `NativeUnwind` coverage. R02.4 now binds selected compiler/VM source facts to paired direct-opcode dumps and runtime cases, but does not model those VM transfers; independent acceptance is pending. The graph still leaves strict/parser error transfers, release/destructor consequences, and fatal/native-unwind/suspension outcomes open. Remaining acceptance work is split across R02.5–R02.7 below; callback effects and ordered exception consumption remain under R05.2.

- **R02.3 — Partial: selected source-linked evidence only; broader acceptance open** (depends on: R00.3, R02.2)
  Acceptance: not accepted
  The schema now represents `NativeUnwind` independently from `PhpThrow`. The strlen graph binds scalar parser/cast/release source facts, paired TAILCALL/CALL callback observations, and the full LLVM operation inventory. Its executable graph keeps incomplete coercion, callback effects, release/destructor consequences, parser failures, fatal, native unwind, and suspension open. The type host models selected normal and PHP-throw paths; bounded callback/destructor writes and PHP-throw cleanup are tracked under R05.2. General release/effect behavior, native unwind, non-normal exits, and operation-to-source completeness are not met. RV-20 downgraded R02.3 to Partial; independent acceptance is still pending.
  Validation: 109 semantics library tests and 5 builder tests pass; 1,203 checker library tests pass (1 ignored); all 16 focused `strlen_` fixtures pass; `cargo check --workspace` passes. Catalog regeneration validates against the locked PHP source and runtime witnesses and is byte-for-byte reproducible. Independent acceptance is still pending.

- **R02.4 — Implementation complete; author-accepted (2026-10-12)** (depends on: R02.2, R02.3)
  Acceptance: accepted
  The builder now locks and checks `zend_compile.c` and `zend_vm_def.h` hashes and selected source facts for constant folding, dynamic `ZEND_STRLEN` emission, opcode 121, string fast path, strictness, weak parsing, null diagnostic, TypeError, and exception-aware dispatch. A checked-in direct-opcode witness binds paired TAILCALL/CALL OPcache dumps to weak scalar success/diagnostic, strict rejection, and normal/throwing Stringable runtime cases; fixture, binary, dump, stdout, and source hashes are validated. Compiler, VM, and observation records now support the narrow Return and Diagnostic evidence. Mutation regressions reject changed dump and runtime output; the builder's locked-source checks reject source digest/fact/opcode drift. All unsupported state/effect/release/exit domains remain open. This closes the implementation deliverable for RV-16's direct opcode evidence gap; independent acceptance remains pending.

- **R02.5 — Evidence implemented; graph model remains open** (depends on: R02.3, R04.1, R05.2)
  Acceptance: not accepted
  The builder now hash-checks weak parser, standard Stringable cast, and direct `ZEND_STRLEN` VM release source facts. A PHP 8.5.4 probe runs on paired TAILCALL/CALL binaries and distinguishes scalar/no-cast behavior, a retained object with no destructor during `strlen`, destruction after caller `unset`, temporary destruction before the next statement, equivalent dynamic native invocation, callback writes/output, callback throw with destructor-before-catch ordering, and re-entrant destructor invocation. Fixture, source, binary, and output hashes are checked; mutation tests reject fixture and paired-output drift. Source/runtime evidence is attached only to open `Release`, `UserWrite`, `Output`, and `PhpThrow` coverage. It does not yet model ordered destructor state or exits in the host/checker; R04.1 and R05.2 remain open prerequisites for that integration. Acceptance remains open until source/runtime cases are consumed by a validated graph transfer; unsupported effects and exits stay open.
  Validation: 109 semantics library tests and 7 builder tests pass; the embedded-catalog evidence/dependency assertion passes; both locked PHP profiles produce the expected identical runtime output and five-opcode optimizer dump; generated catalog bytes match the embedded artifact.

- **R02.6 — Implementation complete; author-accepted (2026-10-12)** (depends on: R02.3, R02.4)
  Acceptance: accepted
  `CatalogTypeHost` now executes the graph's parser-acceptance predicate with weak/strict behavior, definite rejection for arrays and known non-Stringable final objects, and `Maybe` for mixed/unresolved cases. The graph's weak-null diagnostic transfer preserves its diagnostic state and both possible continuations: ordinary return or a PHP exception raised by a custom error handler. Definite parser rejection reaches `Throw(Unknown)`, so it cannot be interpreted as a normal result; the abstract throw value and `PhpThrow` domain stay explicitly open. Checked-in weak/strict PHP fixtures now exercise direct `ZEND_STRLEN` and dynamic native dispatch for throwing null handlers and array TypeErrors, plus strict null rejection without invoking the handler. The builder validates fixture, binary, output, and normalized opcode-dump identities and rejects mutations. The catalog was regenerated from a fresh LLVM inventory extraction using the locked PHP source tree and `php-cli.bc`; it no longer relies on reconstituted inventory fields. The embedded-catalog regression requires the new observation in diagnostic and open `PhpThrow` evidence.
  Validation: all 8 builder example tests pass, including parser fixture/output/opcode mutation rejection; the fresh extractor output is byte-identical to the checked-in full LLVM witness; the builder regenerated the embedded JSON byte-for-byte; all 4 weak/strict runtime fixtures match their locked outputs on both TAILCALL and CALL binaries; all 3 focused `CatalogTypeHost` tests pass; the extractor's 3 unit tests pass; the focused embedded-catalog evidence assertion passes; `cargo check -p phpstan-rs --lib`, `cargo fmt --all -- --check`, and `git diff --check` pass. `PhpThrow` remains Open for exact exception values/state and broader failure paths; resources and other untested PHP values are not claimed. Independent acceptance remains pending.

- **R02.7 — Implementation partial; cleanup/order integration remains open** (depends on: R02.3, R02.5, R05.2)
  Acceptance: not accepted
  A paired PHP 8.5.4 direct-opcode/dynamic-call fixture now observes callback PHP throw, fatal bailout, and Fiber suspension across TAILCALL/CALL profiles. It records direct-versus-dynamic destructor/catch/finally order, and confirms fatal bailout skips caller `finally`. The builder validates the locked Zend source hashes/facts for `trigger_error`, fatal error dispatch/bailout, Fiber suspension, and Stringable dispatch, along with fixture, binary, output, and opcode-dump digests. The `CatalogTypeHost` emits distinct possible `PhpThrow`, `Fatal`, and `Suspended` exits from Stringable invocation; the new observation is attached to open graph coverage for those domains. Graph-evaluation tests replay Fatal and Suspension as separate certified exits; existing cleanup matrix tests retain pending-completion kinds and reject unsupported cleanup replacements. The checker now keeps the normal result and invocation-effect marker when abrupt exits are also possible, then applies the existing string-operation effect transfer; a regression verifies inferred normal result and callback-write invalidation on normal and caught paths.
  Validation: all 9 catalog-builder tests pass, including mutation checks for source, output, opcode dump, and witness scope; all 110 semantics tests and all 1,206 PHPStan library tests pass (one ignored); focused `CatalogTypeHost` tests (3), the embedded-catalog evidence regression, the cleanup-resume regression, and the Stringable checker regression pass. Both PHP 8.5.4 profiles reproduce the checked-in abrupt-outcome output byte-for-byte and each other (SHA-256 `5f50b232d46dcfb958a85fb07221300b7c94357db3d76c2d4dc23c94c6ba1476`). The regenerated artifact matches the R02.7 builder output byte-for-byte; `cargo fmt --all -- --check` and `git diff --check` pass. A first embedded-catalog run exposed an assertion that incorrectly required release evidence on fatal/suspension; the assertion was corrected to reflect their separate abrupt-outcome evidence, and the test then passed.
  Native unwind has no source/runtime witness for this PHP C boundary and remains explicitly Open; no `NativeUnwind` exit is fabricated from fatal bailout. The checker still does not carry an ordered heap-state delta or destructor effects from the graph, nor model caller cleanup/resumption. Those integration gaps depend on R05.2 and remain open. Independent acceptance is pending.

**Investigation, 2026-10-23 (status unchanged):** no source or runtime witness exists for native unwind across the zif_strlen boundary; the search finds only 'not modeled' text, so NativeUnwind stays Open. R02.3 is Partial (RV-20) and R05.2 is not accepted; R05.2.4.5.2 needs a separate reviewer (line 319). The owner decides whether native unwind is inside the slice; if it is, a boundary witness is needed, otherwise a Scope and Remainder line.

### F03.1 — Complete: Measure whole-profile SVF resource behavior
Acceptance: accepted

**Reviewed evidence and current disposition:** Stock and compact full-link runs and resource measurements are recorded. Review: Stock SVF and the compact-layout adaptation each hit the enforced 10 GiB cgroup limit on the linked minimal PHP CLI workload. This rules out those configurations as mandatory whole-link prerequisites. Each candidate had one enforcing attempt; neither result evaluates an alternative solver. These measurements reject full-profile SVF as a mandatory prerequisite; they do not establish a replacement solver.


### F03.2 — Partial: Probe demand summaries, Zend layout/dispatch, and fallback cases
Acceptance: not accepted

**Reviewed evidence and current disposition:** Reduced SVF layout/dispatch probes, copy/initializer regressions, and the empty Stringable-target counterexample are recorded. Review: Reduced probes cover Zend layout/dispatch, absent external writes, copy/initializer cases, and the empty Stringable-target fallback counterexample. The capsule lacks object construction/handler initialization, so an empty target is explicitly incomplete evidence. Broader source-derived demand summaries are not implemented. Generalize beyond the reduced capsule and add demand-oriented source-derived summaries.

- **R03.1 — Open** (depends on: R02.2)
  Acceptance: not accepted
  Expand Zend layout/copy/initializer/dispatch probes through construction and handler initialization. Incomplete target discovery must remain incomplete, never a complete empty set.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

- **R03.4 — Implementation updated; identity checks validated; linked-module replay pending** (depends on: R02.2)
  Acceptance: not accepted
  The SVF replay derives the PHP commit from `results/lock.json`, requires the linked-module manifest to name the same commit, checks that manifest against a digest in the root lock, and rejects bitcode whose digest differs from the linked module bound by that manifest. It also verifies every source digest in the root lock and selects the exact runtime fixture named by its witness. Tests cover changed source, linked bitcode, manifest commit, unavailable bitcode, and fixture bytes; all 21 focused Python tests pass. Live source and link-manifest lock checks pass. The actual replay remains open because the pinned linked bitcode is not available in this environment; independent acceptance is also pending.

**Investigation, 2026-10-23 (status unchanged):** the reduced SVF capsule (strlen-native-demand-svf.json) omits construction, so zend_API.c:769 returns an empty target in every mode. The replay is blocked on the missing linked bitcode and dvf. No Rust consumer of query_points_to exists. R03.1 comes first, then restored inputs and summaries with a validating consumer.

### F03.3 — Open: Integrate coverage-aware pointer reuse and fallback tracking
Acceptance: not accepted

**Reviewed evidence and current disposition:** Existing PTA fallback flags and target-report limitations are documented; empty-target fallback regression is preserved. Review: Existing PTA internals track `data` and `anything` fallback flags, and the regression preserves a dangerous empty-target example. The analysis remains private to `php-catalog` and emits text; there is no machine-readable catalog adapter, per-target completeness contract, or production integration. A known target set cannot be treated as exhaustive when fallback is present. No machine-readable adapter, per-target completeness contract, or production integration exists.

- **R03.2 — Open** (depends on: R03.1)
  Acceptance: not accepted
  Add machine-readable pointer-result integration carrying provenance and data/anything fallback flags. Fallback or incomplete results cannot close per-target catalog domains.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

**Investigation, 2026-10-23 (status unchanged):** the PTA keeps its data and anything fallback flags internally (pta/mod.rs:476-502) and prints text only (main.rs:446). There is no machine-readable export, no per-target completeness contract and no consumer; no crate reads query_points_to or out_of_budget_query_count. R03.1 comes first. tools/php-catalog has no test items.

### F03.4 — Open: Validate native summary post-fixpoints
Acceptance: not accepted

**Reviewed evidence and current disposition:** Semantic graph certificates validate graph evaluation. Review: The semantic graph certificate replays the graph evaluator, not imported native IR summaries. Native summary validation and missing-write/growth regressions at that boundary do not exist. Add independent validation for native IR summaries and missing-write/growth regressions.

- **R03.3 — Open** (depends on: R02.3, R03.2)
  Acceptance: not accepted
  Independently validate imported native summaries bound to input/summary digests. Mutation tests reject omitted writes, exits, successors, stale summaries, and non-post-fixpoints.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

**Investigation, 2026-10-23 (status unchanged):** the subject does not exist. There is no native summary type, producer, digest binding or validator in crates or tools. The only post-fixpoint certificate (lib.rs:1125, validated at 2551-2564) replays the PHP graph evaluator, not a native import. The owner decides whether Phase F builds the producer now or waits for the F06.5 solver decision.

### F04.1 — Complete: Implement the minimum PHP value/reference product fixtures
Acceptance: accepted

**Reviewed evidence and current disposition:** Copy arrays, nested references, object identity, alias rebinding/unset, and normal/throw callback state are tested. Review: The value store and PhpFlow tests cover array-copy isolation, nested references, object identity, alias rebinding/unset, reference cells, and callback mutation across normal/throw/cleanup paths. These fixtures meet the stated minimum gate; they do not constitute full PHP value semantics. Minimum Phase F fixture requirement is met.


### F04.2 — Deferred: Expand PHP value semantics and syntax-to-flow lowering
Acceptance: not accepted

**Reviewed evidence and current disposition:** Current value state and PhpFlow operations provide a seed. Review: Broader key coercion, object properties, and syntax-to-flow lowering remain later-plan work. The current ledger correctly keeps this separate from the minimum Phase F fixture result. Broader key coercion, object properties, and syntax-to-flow lowering belong to C22.

- **R22.1 — Deferred** (depends on: Later C22 planning)
  Acceptance: not accepted
  Carry key coercion, object properties, and syntax-to-flow lowering into C22 unless the plan explicitly brings them into Phase F.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F05.1 — Partial: Select catalog behavior across exact and uncertain target scenarios
Acceptance: not accepted

**Reviewed evidence and current disposition:** Exact/partial target selection and exact/minor patch fixtures exist; uncertain remainders stay uncovered. Review: Exact patch selection, minor ranges, correlated scenarios, and uncovered/Maybe remainders are tested. The checker adapter leaves platform facts unknown; the selected profile carries SAPI, zts and debug (crates/phpstan-rs/src/semantics.rs:103-112); extension and INI facts are not set by the checker settings (settings.rs:60-61), so they stay unknown. Include platform, SAPI, build, extension, and configuration profile facts in applicability.

**Investigation, 2026-10-23 (status unchanged):** settings.rs:60-61 sets extensions and ini to None, and PhpTargetProfileSetting has no fields for them; no predicate test covers extension or INI facts. The ledger's claim that SAPI, zts and debug are missing is wrong: they are already in the profile (settings.rs:31-37, semantics.rs:103-112). Ownership is split: the ledger puts extension and INI applicability under F05.6 (lines 356, 362). A scope ruling comes first.


### F05.2 — Complete: Load and validate the exact 8.5.4 `strlen` artifact in the checker
Acceptance: accepted

**Reviewed evidence and current disposition:** Embedded JSON is validated; evaluator certificate is replayed before consuming Return/Diagnostic results. Review: The checker loads the embedded artifact through manifest validation, replays its certificate, and consumes its narrow Return/Diagnostic projection. Locked regeneration is byte-identical. This is not a general artifact/profile pipeline. None for this exact narrow slice.


### F05.3 — Partial: Route argument checks and null diagnostics through shared catalog evaluation
Acceptance: not accepted

**Reviewed evidence and current disposition:** Return and exact-target null Diagnostic share a memoized evaluation; weak/strict routing fixtures pass. Review: Argument checks, return inference, and the exact-target null deprecation share a memoized graph evaluation; weak/strict and version-boundary fixtures exercise it. Broader source-derived diagnostics and ranges outside exact coverage still use legacy behavior. Generalize source-derived rules; ranges outside exact catalog coverage still use legacy diagnostic behavior.

- **R05.4 — Open** (depends on: R05.2)
  Acceptance: not accepted
  Generalize source-backed argument, return, and diagnostic rules beyond exact strlen. Test boundaries and ensure uncovered/range cases retain legacy or conservative behavior.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

**Investigation, 2026-10-23 (status unchanged):** the catalog covers strlen only (semantics.rs:22). The catalog result feeds only the null diagnostic (parameters.rs:310-312, 372-385); argument acceptance is hand-written. PHP 8.5.4 emits a null-deprecation for str_repeat and strtoupper (checked with php -r). The checker emits it only for strlen: parameters.rs:203 gates it on `catalog_function == Some("strlen")`. Needs R05.4, which needs R05.2 accepted.

### F05.4 — Partial: Route implicit `Stringable` invocation/effect from the graph
Acceptance: not accepted

**Reviewed evidence and current disposition:** Graph contains a weak-mode predicate plus `Invoke(__toString, argument 0)` with pinned source/runtime evidence. The checker consumes graph invocation facts, bounded same-file callback property writes, literal callback-result validation, and PHP-throw string-argument invalidation; it also applies bounded temporary-release/destructor write and PHP-throw effects. The catalog bridge preserves the normal result when abrupt outcomes are possible. It still does not model general heap-state deltas, exact exception identity, destructor output/re-entry, or fatal, suspension, and native-unwind cleanup. R05.2.1 resolves formal-argument slots and covers named arguments after unpacking, one unpack, and multiple unpack sources. R05.2.2–R05.2.4 implement bounded transfers with conservative fallback; broader behavior and independent acceptance remain open.

- **R05.2 — Implementation partial; acceptance open** (depends on: R04.1, R05.1)
  Acceptance: not accepted
  R02.3 and R02.7 represent the source/runtime-observed possible PHP throw, fatal, and suspension as distinct open invocation exits. The checker retains normal returns and graph invocation markers alongside abrupt exits. R05.2.2 composes source-backed callback property writes by normal and PHP-throw outcome for a bounded same-file callback subset and falls back conservatively otherwise. R05.2.3 models bounded literal callback-result validity and the resulting TypeError path. R05.2.4 composes bounded temporary release, source-backed destructor writes, and destructor PHP-throw replacement; exact output/re-entry state and fatal, suspension, and native-unwind release replacement still need broader modeling. Full PHPStan library validation currently passes (1,227 passed, 1 ignored). R02.5 and R02.7 remain open for the uncovered exit/effect behavior.
  - **R05.2.1 — Complete: Bind invocation effects to the correct call argument**
    Graph operands identify formal parameter slots. The checker resolves those slots through the function signature before applying operation effects, and joins known element types from unpack sources when the formal slot is supplied by unpacking. When an unpacked formal cannot be resolved, the ordinary conservative `Operation::String` effect is applied to joined spread-element types; the PHP probe's observed invalidation comes from this generic fallback, not from a catalog-resolved Stringable element. Regression fixtures cover named arguments following unpacking, one unpack source, and multiple unpack sources; each proves property invalidation, but the spread cases do not establish catalog callback dispatch for unpacked elements. RV-10's unproven mechanism claim is corrected. Validation and the parent R05.2 acceptance remain tracked separately.
  - **R05.2.2 — Complete for bounded source-backed normal/PHP-throw callback outcomes**
    The evaluator preserves event state on normal and PHP-throw exits, with semantic-layer tests proving a write before a callback throw survives while a later event is skipped. The checker keeps normal-completion invocation facts separate from PHP-throw invocation facts and snapshots locals before applying each outcome's effect. Invoked and non-invoked `mixed` branches retain separate return blocks, preventing partial `Invoke` results. The catalog artifact was regenerated with `build_strlen_catalog` and byte-compared with its output. For a final, same-file callback, a bounded source interpreter handles literal property writes, branch conditions on locals or initialized hook-free `$this` properties, direct no-argument `RuntimeException` throws, and literal-string returns. It forks branches (up to 64 paths), joins only writes feasible for the selected outcome, and preserves uncertainty about callback/caller object aliasing by joining assigned values with the caller's prior value. It rejects writes that can fail native property type checks or target readonly/virtual properties, and rejects intermediate reads that are not definitely initialized non-null objects. Unknown argument types, cross-file bodies, arbitrary calls, unsupported control flow, and other throw constructors keep conservative invalidation.
    - **R05.2.2.1 — Complete: Carry bounded source-backed callback deltas by outcome**
      Normal completion and caught PHP-throw regressions verify distinct branch-specific property values, including a write after a conditional throw point and a write after a throw remaining unreachable. Mixed, cross-file, arbitrary-helper-call, alias, unsupported-loop, and potentially type-erroring assignment fixtures verify conservative fallback. The analyzer rejects unsupported shapes instead of partially summarizing them; fatal bailout, suspension, and native unwind remain open under R02.7/R05.2.4.
    Validation: all 11 catalog-object argument tests pass (1 existing ignored); full `cargo test -p phpstan-rs --lib` passes (1,221 passed, 1 ignored); `cargo fmt --all -- --check` and `git diff --check` pass. The prior semantic-layer suite (110 passed) and byte-for-byte catalog regeneration remain valid because this change only modifies checker-side outcome transfer. R05.2.2 and R05.2.2.1 are complete for the explicitly bounded summary and conservative fallback contract; broader exit kinds remain tracked separately.
  - **R05.2.3 — Complete for bounded source-backed literal callback results**
    The evaluator suppresses the normal invocation result only when the final same-file callback summary proves every path returns an invalid literal; it records `never` for calls with no normal exit and retains the PHP-throw outcome. The callback transfer preserves writes made before that TypeError. Conditional callbacks retain distinct valid and invalid branches. PHP 8.5.4 CLI probes confirm an untyped `__toString()` returning `null` writes before throwing `TypeError`, returning `1` is accepted through weak scalar conversion, and returning `1` in a strict file raises `TypeError`. The checker uses the callback file's `strict_types` mode when classifying scalar literal results. Regressions cover definite-invalid and `never`, valid weak scalar, strict scalar TypeError, and conditional string/null results with branch-specific state at the caught exception and after the join. Unsupported expressions, cross-file bodies, and result expressions that may invoke user code retain conservative normal/throw uncertainty; this bounded checker result does not close general callback outcome coverage or the fatal/suspension/native-unwind paths.
    Validation: the catalog-object argument group passes (16 passed, 1 existing ignored); full `cargo test -p phpstan-rs --lib` passes (1,226 passed, 1 ignored); `cargo fmt --all -- --check` and `git diff --check` pass.
  - **R05.2.4 — Open: Compose release and destructor effects in order**
    Consume source/runtime-backed release events and destructor writes, output, throw, and re-entry in their observed order. Cover retained objects, temporaries, direct and dynamic calls, and pending completion cleanup; keep unsupported native unwind open.
    - **R05.2.4.1 — Complete: Pin the release/destructor observation matrix**
      The builder validates the locked PHP source facts plus paired TAILCALL/CALL fixtures, binaries, opcode dumps, outputs, and hashes. The PHP 8.5.4 witness covers retained objects, a temporary, direct and dynamic dispatch, destructor output, callback throw with destruction before catch, and ordered re-entry. Mutation tests reject changed fixture/output/opcode evidence. This establishes the observed cases, not the host transfer.
    - **R05.2.4.2 — Complete: Represent cast-argument release on normal and PHP-throw paths**
      The generated graph now places `Release(Argument(0))` after `Invoke(__toString)` on normal completion and routes a callback PHP throw through a cleanup block that releases the argument before `Resume` restores the pending exception. The host records that event on both paths; a graph regression asserts each order. The remaining `Release` coverage is Open for refcount-dependent destruction and its effects, handled by R05.2.4.3; fatal, suspension, and native unwind still have no fabricated release edge.
      Validation: the locked catalog builder regenerated the artifact byte-for-byte; embedded graph/order and evaluator normal/throw regressions pass; `cargo check --workspace` passes; `cargo test -p phpstan-rs-semantics --lib` passes (110 passed); full `cargo test -p phpstan-rs --lib` passes (1,227 passed, 1 ignored); `cargo fmt --all -- --check` and `git diff --check` pass.
    - **R05.2.4.3 — Open: Transfer destructor effects at release**
      Consume source-backed destructor writes, output, PHP throw, and re-entry in observed order. Preserve the distinction between retained values and temporaries, and carry destructor effects separately from `__toString` callback effects.
      - **R05.2.4.3.1 — Complete for bounded final-class destructors: Transfer literal writes, with conservative fallback**
        For a direct `new` argument, the checker now interprets same-file final-class destructors whose bodies use only the supported literal property writes, simple branches, and direct no-argument `RuntimeException` throws. Normal and throwing destructor writes are applied after the corresponding `__toString` outcome; a destructor throw can replace a callback throw, and possible callback/destructor throw states are joined conservatively where the checker cannot retain exception identity. Retained caller variables do not receive destructor effects. Unsupported output, nested calls/re-entry, aliases, cross-file bodies, and other statement forms fall back to broad `Operation::Destroy` invalidation.
        Validation: regressions assert callback and destructor literal values on normal, callback-throw, and destructor-throw paths; retained values stay undestroyed; output/re-entry bodies retain broad fallback. Full suite counts are recorded under R05.2.4.5.1.
      - **R05.2.4.3.2 — Complete for conservative effects: Compose destructor output, throws, and re-entry in order**
        The graph separates callback invocation from release, then places an open destructor-effect transfer after `Release` on normal completion and callback-throw cleanup. Simple source-backed destructor writes are transferred in order; output, nested invocation/re-entry, and other unsupported destructor behavior remain explicit open domains and use conservative invalidation. A direct temporary release has a distinct possible PHP-throw exit. PHP 8.5.4 CLI evidence confirms a destructor can emit output and replace a pending callback exception while preserving it as `getPrevious()`; the release witness pins the `zend_objects.c` transfer that saves the old exception, calls `__destruct`, and links the old exception as previous. The evaluator permits only the evidenced PHP-throw-over-PHP-throw cleanup cell; native unwind, fatal, and suspension replacement remain fail-closed. Exact output bytes, nested re-entry results, aliases/refcounts beyond the direct temporary distinction, and exception values are not modeled.
        Validation: release witness validation and byte-for-byte catalog regeneration pass; PHP 8.5.4 probe observed `destructor;LogicException:destructor;previous=RuntimeException:callback`; graph ordering, temporary release PHP-throw, write transfer, and cleanup replacement regressions pass; `cargo test -p phpstan-rs-semantics --lib` passes (110 passed); `cargo test -p phpstan-rs --lib` passes (1,227 passed, 1 ignored); formatting and diff checks pass.
    - **R05.2.4.4 — Complete for direct calls and statically named dynamic `strlen` calls**
      The checker now sends a dynamic call through the catalog only when its callable expression has exactly the constant function name `strlen`; arbitrary callables and unions of names retain generic conservative effects. The same argument binding, literal destructor-write summary, and lifetime transfer then applies to direct temporaries, retained values, callback PHP throws, and destructor PHP throws. Regression cases cover direct and dynamic normal temporary release, retained dynamic objects, callback/destructor throws, complex output/re-entry fallback, and ambiguous dynamic callable fallback. This does not resolve dynamic callables whose target is known through a closure or a broader interprocedural result.
      Validation: focused catalog-object argument regression passes with all direct/dynamic cases and the ambiguous fallback; the embedded graph regression passes; full PHPStan library suite passes (1,227 passed, 1 ignored); semantic suite passes (110 passed).
    - **R05.2.4.5 — Open: Validate the ordered release transfer and fail-closed boundaries**
      - **R05.2.4.5.1 — Complete: Run the release and completion validation matrix**
        The pinned PHP 8.5.4 release witness and regenerated artifact pass byte-for-byte validation. Host/checker tests cover retained objects, direct and exact-name dynamic calls, source-backed destructor writes on normal and PHP-throw paths, destructor PHP-throw outcomes, complex output/re-entry fallback, and release-before-open-effect ordering. The semantic completion matrix verifies PHP-throw replacement is the only newly supported cleanup cell; native unwind, fatal, and suspension replacements remain rejected. Validation: semantic suite 110 passed; full PHPStan library suite 1,227 passed, 1 ignored; `cargo check --workspace`, `cargo fmt --all -- --check`, and `git diff --check` pass.
      - **R05.2.4.5.2 — Open: Accept by author review**
        The implementation and evidence have not yet received a review. The author audit checked: (1) the pinned PHP 8.5.4 source/runtime witness against the graph's normal and PHP-throw `Release` ordering; (2) the checker transfer ordering and its direct-temporary-only destructor summary; (3) direct/dynamic behavior and ambiguous-callable fallback; (4) the cleanup-replacement matrix; and (5) open coverage for output, re-entry, alias/refcount, exact exception values, fatal, suspension, and native unwind. It found and corrected stale claims in `PHP_SEMANTICS_CATALOG_EVALUATION.md` that release/destructor transfer was wholly unmodeled. Focused validation passed: cleanup-replacement matrix, callback-throw cleanup/resume, temporary destructor-PHP-throw host case, and release-witness drift rejection; the catalog-object argument group passed 18, 1 ignored. The LLVM operation inventory and catalog were regenerated from the locked local inputs; the artifact matches byte-for-byte. These checks are an author audit only. A separate reviewer must verify the six criteria above and record acceptance or findings here; keep R05.2.4 open until then.

- **R05.8 — Implementation and validation complete; author-accepted (2026-10-12)** (depends on: R05.2)
  Acceptance: accepted
  Correct the Stringable and nullable predicates conservatively, including bare `object`, object shapes, object-bearing intersections, object-bounded templates, trait-composed `__toString`, and nullable templates. Keep unknown forms on a `Maybe` path and retain conservative effects; do not infer a definite rejection from missing registry identity.
  - **R05.8.1 — Implement and regress unknown object representations**: bare `object`, object shapes, object-bearing intersections, and object-bounded templates must preserve the possible Stringable path (RV-9).
  - **R05.8.2 — Recognize trait-composed `__toString` for weak strlen acceptance and effects**: PHP's implicit Stringable behavior applies when the method comes from a used trait (RV-17).
  - **R05.8.3 — Preserve possible null diagnostics for nullable templates**: the `@template T of string|null` case must retain the same PHP deprecation possibility as an explicit nullable union.
  All child regressions pass in the full PHPStan library suite (1,234 passed, none ignored). The parent implementation is complete; independent acceptance remains open.

- **R05.9 — Complete** (depends on: None)
  Acceptance: accepted
  Done: the two side-effecting calls are hoisted out of their `debug_assert!` macros in `registry/resolve.rs`. Under the gate profile, the full library suite now has one failure (RV-15, R05.6). The eight PHPDoc fixtures pass in the gate CLI. The gate-profile library tests and the fixtures are the release-profile regression set, since the debug build cannot reproduce the defect.
  Original scope: Hoist the side-effecting `debug_assert!` calls in `registry/resolve.rs` (lines 459 and 1602, from commits `32fb0a32` and `d28d22a1`) so that the call runs in every profile and only the check is debug-only. Acceptance: the gate-profile library and fixture runs show no failures from this cause (RV-14).

**Investigation, 2026-10-23 (status unchanged):** no Scope and Remainder line exists for the bounded contract, and R05.2 and R05.2.4 are not accepted. The dynamic witness destroys after the catch, while the ledger text says destruction before the catch; the regression is a superset, so it is over-approximate, not unsound. The maintainer decides narrow scope or implementation first, and the ledger owner fixes the R05.2.4.1 and R05.2.4.4 wording.

### F05.5 — Complete for current catalog consumers: Prevent duplicate legacy application for covered calls
Acceptance: accepted
Scope: current catalog consumers
Remainder: F05.6

**Reviewed evidence and current disposition:** The checker currently has one live semantic-catalog dispatch: the `strlen` consumer in `catalog_builtin_return_type`. R05.3 audits its return inference, argument diagnostics, and call-effect application for direct and exact-name dynamic calls. Catalog invocation facts select covered callback/destructor operations; after applying those effects, `forget_what_a_call_may_change` returns without applying the legacy builtin-run effect a second time. Exact dynamic names use the built-in argument checker; ambiguous callables stay on the generic path, and uncovered profiles retain legacy effects. The direct/dynamic diagnostic, branch-effect, ambiguous-callable, and uncovered-target regressions pass. This closes F05.5 for every catalog consumer currently present; a future catalog consumer must receive the same ownership audit before being considered covered.

- **R05.3 — Complete for the audited `strlen` path; other consumers are tracked by F05.6** (depends on: R05.2)
  Acceptance: accepted
  Scope: the audited strlen path
  Remainder: F05.6
  Ownership is explicit for the audited exact `strlen` path: catalog return/diagnostic evaluation is memoized per call span and argument state; direct and exact-name dynamic calls route argument checking and invocation effects through the catalog; `forget_what_a_call_may_change` returns after applying covered graph-selected string/destructor operations, while uncovered targets and ambiguous callables continue through the ordinary builtin/call effect path. The end-to-end diagnostic regression confirms the direct and exact-name dynamic calls each emit exactly one matching `argument.type` diagnostic for `strlen([])`. This bounded consumer audit does not establish a global no-duplicate rule for other catalog consumers.
  - **R05.3.1 — Complete: Route direct and exact-name dynamic calls through the same catalog consumers**
    Direct function calls and a callable with exactly the constant name `strlen` share catalog return inference and call-effect ownership. Exact-name dynamic calls also use the built-in argument checker; unions and unknown callable values stay generic.
  - **R05.3.2 — Complete: Prove single branch transfer and preserve uncovered legacy effects**
    The regression uses distinct normal and throwing `__toString` writes to detect cross-outcome contamination or duplicate legacy transfer on direct and exact-name dynamic calls. An unconfigured target control proves the ordinary builtin-run effect still invalidates the property. Existing memo tests retain distinct argument-state entries and clear them on file changes. Validation before the diagnostic regression: full PHPStan library suite 1,228 passed, 1 ignored; workspace check passed. Current focused catalog-object argument group: 18 passed, 1 ignored; formatting and diff checks pass.
  - **R05.3.3 — Complete: Prove argument diagnostics survive exact-name dynamic catalog routing**
    Added `exact_dynamic_strlen_preserves_argument_diagnostics_once`: the direct-call control and exact-name dynamic call each produce exactly one `argument.type` diagnostic for `strlen([])`, and the two messages differ by design: the direct call reads `function strlen`, the exact-name dynamic call reads `callable 'strlen'` (`calls/parameters.rs:68` and `named_callable.rs:39`), and the test asserts the dynamic text and the count. The focused `catalog_object_argument` group also passes, including its ambiguous-callable conservative-fallback and uncovered-target controls: 18 passed, 1 ignored. `cargo fmt --all -- --check` and `git diff --check` pass.

### F05.6 — Open: Validate full target profile and uncovered behavior end to end
Acceptance: not accepted

**Reviewed evidence and current disposition:** The entry now requires PHP 8.5.4, Linux, x86-64, CLI, non-ZTS, and non-debug, matching the locked runtime witness. The checker accepts an exact configured profile; absent facts remain unknown, produce Maybe applicability, and retain an uncovered remainder. An architecture mismatch remains uncovered. Full build/compiler, extension, and INI applicability and broader covered/uncovered behavior have not been validated end to end; keep this parent task Open.

- **R05.1 — Implementation complete for the locked runtime slice; author-accepted (2026-10-12)** (depends on: R01.1, R01.3, R00.3)
  Acceptance: accepted
  Scope: the locked runtime slice
  Remainder: F05.6
  The `strlen` entry predicate now requires the locked Linux x86-64 CLI profile plus non-ZTS/non-debug PHP 8.5.4 facts. The builder validates the witness profile and rejects a mutated architecture; the embedded artifact test verifies exact applicability, unknown-profile Maybe plus uncovered remainder, and an incompatible architecture remaining uncovered. `php_target_profile` settings flow through session loading into registry target selection, and canonical registry identity includes the selected profile. Unconfigured platform facts remain unknown. Extension, INI, and broader build applicability remain open under F05.6; this closes only R05.1's bounded runtime-profile deliverable.
  Validation: all 9 catalog-builder tests pass; the session-load, canonical-identity, and embedded target-selection regressions pass; the full PHPStan library suite passes (1,211 passed, 1 ignored); regenerated catalog JSON matches the checked-in artifact byte-for-byte; formatting and diff checks pass.

**Investigation, 2026-10-23 (status unchanged):** the builder regeneration panics at build_strlen_catalog.rs:673 with the checked-in pin 3c51..., while the witness digest is e3427ce8 (strlen-llvm-witness.json:102). Unknown php_target_profile keys are dropped silently. A scratch patch was built and run in ~/.cache/php-catalog-phasef but not applied to any repository; the regenerated artifact matched after masking identifiers, not byte for byte. The owner decides the module digest first.

### F05.7 — Open: Resolve the full fixture-suite failures found during evaluation
Acceptance: not accepted

**Status note (second review, 2026-10-09):** the cause was side-effecting `debug_assert!` calls in `registry/resolve.rs`, which dropped `@method` and `@mixin` members in release-profile builds (RV-14, R05.9, now Complete). The gate-profile and debug library suites both pass (RV-15 fixed, R05.6). F05.7 stays Open until the full optimized fixture suite is rerun (R05.7).

**Reviewed evidence and current disposition:** The current focused `strlen` set passes 16 fixtures. A fresh full optimized fixture run passed 3,954/3,962; eight `@method`/`@mixin` PHPDoc fixtures failed. A separate fresh optimized library run passed 1,198/1,202; four project/registry tests failed. Each of those four library failures reproduced alone; only the deferred-registry case was compared with parent `3129ece5`. Review: The current focused `strlen_` set passes 16 fixtures. A fresh full optimized fixture run passed 3,954/3,962. Its eight failures are `a_method_tags_own_templates_are_inferred_and_validated`; `a_method_tag_enforces_its_own_argument_types`; `a_method_tag_enforces_its_own_return_type`; `a_method_tag_synthesizes_a_callable_virtual_method`; `a_method_with_no_return_type_defaults_to_mixed`; `a_mixin_makes_the_target_s_public_methods_callable`; and `a_method_tag_parameter_type_referencing_an_unknown_class_is_flagged` plus `a_method_tag_return_type_referencing_an_unknown_class_is_flagged`. A separate fresh optimized library run passed 1,198/1,202; its four failures are `deferred_registry_passes_all_run_through_the_real_project_path`, `switching_registry_bases_invalidates_the_previous_id_generation`, `trait_mixin_and_phpdoc_members_live_in_host_topology`, and `mixin_and_phpdoc_method_topology_survives_freeze_and_cache_round_trip`. All four library failures reproduced alone; only the deferred-registry case was checked against parent `3129ece5`. These are two different suites and both failure sets remain open. Do not claim a clean optimized gate until each set is resolved or baseline-established. Establish or resolve both failure sets independently. Compare the remaining three library failures against the parent; establish a baseline for the eight PHPDoc fixture failures. Do not combine the two suites' failure counts.

- **R05.5 — Complete (the eight PHPDoc fixtures pass in the gate profile at `8ad93491`)** (depends on: R05.9)
  Acceptance: accepted
  Scope: the eight PHPDoc fixtures at 8ad93491
  Remainder: R05.7
  Compare each of the eight PHPDoc fixture failures with parent/control; attribute and fix regressions or record an accepted baseline. Second-review result: all eight were caused by the side-effect defect in R05.9. Rerun at `8ad93491` with `cargo test -p phpstan-rs --profile gate --test fixtures`: all eight pass with no approved baseline needed. `own_templates` (generics_and_templates) and both `a_method_tag_*_referencing_an_unknown_class_is_flagged` (symbol_existence) plus the `method_and_mixin` group (12 passed) and the `a_method_tag` group (7 passed) are green. No failure remains to attribute.
- **R05.6 — Implementation complete; acceptance open** (depends on: R05.9)
  Acceptance: not accepted
  Done: all four remaining library failures are attributed. Three are the R05.9 defect, fixed in `bdb93af0f` (the commit the entry originally cited, 479a948b, is not reachable from any branch). The fourth, `switching_registry_bases_invalidates_the_previous_id_generation`, asserted a debug-only panic. It now accepts a panic in debug or `None` in release, and fails only on a live declaration (RV-15). Both profiles pass the full library suite.
  Not yet evidenced as complete; see parent F task for the audited implementation state.
- **R05.7 — Open** (depends on: R05.2, R05.3, R05.4, R05.5, R05.6)
  Acceptance: not accepted
  Rerun optimized fixture and library suites separately; require both to pass or every remaining failure to have an approved baseline disposition.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

**Investigation, 2026-10-23 (status unchanged, disputed):** the two reruns at base 105b004f1 pass, with fixtures at 4012 passed and library at 1304 passed in the gate profile. The investigator recommended complete, but the status-adversary validator rejected that: this entry keeps F05.7 Open until the R05.7 rerun, and R05.7 depends on R05.2 and R05.4, which are not accepted. The entry cites commit 479a948b, which the investigator found unreachable from any branch; the fix is at bdb93af0f, an ancestor of the base. Any later checker patch makes this evidence stale.

### F06.1 — Partial: Compare candidate producers and compiler routes
Acceptance: not accepted

**Reviewed evidence and current disposition:** Clang/GCC/CIR campaigns and prior comparative evidence are recorded; LLVM plus source metadata is the initial producer choice. Review: Comparative work evaluates LLVM plus Clang metadata, AST/CFG, GCC GIMPLE, CIR, Clang analysis interfaces, SVF, and bounded alternatives. LLVM plus metadata is a reasoned initial producer choice; systematic semantic coverage and evidence from useful closed summaries remain absent. Revisit with evidence from a useful closed production summary. Same-slice comparison is tracked under F06.3/R06.1.

**Investigation, 2026-10-23 (status unchanged):** no closed PHP behavior graph or summary exists for the locked 8.5.4 slice (COMPARISON.md:149). The GCC GIMPLE route has dump counts only (505 of 523 units). The entry gives no separate acceptance criterion and does not cite ALTERNATIVES.md (2026-10-15). Owner question: does the section 2.3 decision record alone close it, or does it also need a closed summary and the R06.1 comparison?


### F06.2 — Complete: Establish full-profile SVF resource feasibility
Acceptance: accepted

**Reviewed evidence and current disposition:** Stock/layout-only SVF resource failures are measured on the minimal CLI link. Review: The measured stock and compact-layout SVF whole-link attempts fail the configured 10 GiB gate on the minimal CLI profile. This rejects those configurations as mandatory whole-profile prerequisites; it does not reject every pointer-assistance use or establish a replacement. This rules out that configuration as a mandatory whole-profile prerequisite, not all pointer-service assistance.


### F06.3 — Open: Compare regional and audited pointer-service alternatives on the same slice
Acceptance: not accepted

**Reviewed evidence and current disposition:** Reduced SVF probe supplies a fallback counterexample. Review: Reduced SVF probes show fallback and target-coverage problems, but there is no same-slice comparison of adapted pointer reuse against the regional candidate on a useful closed PHP summary. Build an adapted, coverage-aware comparison against a regional candidate on identical input and demand.

- **R06.1 — Open** (depends on: R03.2, R03.3, R04.1)
  Acceptance: not accepted
  Compare adapted, coverage-aware pointer reuse against a regional candidate on identical input and demand using a useful closed PHP summary. Reduced fallback probes alone do not close this task.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

**Investigation, 2026-10-23 (status unchanged):** the only work item, R06.1, is Open. It needs R03.2 and R03.3 (both Open), a replay with the linked module (absent here), and a closed PHP summary. No regional candidate exists in the repository.

### F06.4 — Open: Measure graph growth, memory, runtime, and checker cost
Acceptance: not accepted

**Reviewed evidence and current disposition:** No calibrated same-slice cost report is recorded. Review: Compiler/import and pointer-prerequisite resource measurements exist. There is no calibrated same-slice report for graph/region growth, artifact size, checker evaluation/runtime, memory, and maintenance cost across candidate solvers. Measure reproducibly and compare alternatives; include artifact size and checker evaluation costs.

- **R06.2 — Open** (depends on: R06.1, R05.2)
  Acceptance: not accepted
  Measure graph/region growth, artifact size, evaluation time, memory, checker cost, and maintenance/rebuild cost on the same locked slice and candidates.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

- **R06.4 — Open** (depends on: None)
  Acceptance: not accepted
  The campaign now waits for a completed enforcing-worker record, fails closed if the cgroup disappears before its exit/resource proof is published, and has delayed-worker and failed-step regressions. Pointer queries and builds must succeed or match an explicit negative-control status; both stages publish completion manifests binding unique step names, expected exit codes, and the measurements digest. The finishing-stage marker is revalidated against its measurements and CIR outcome classification before it is accepted. CIR compiler rejections are recorded as unsupported outcomes with stderr digests; successful CIR imports bind the emitted artifact digest. Unexpected failures block completion. `finish_campaign.py` accepts explicit PHP source/binary and stock-DVF paths rather than assuming scratch-root locations. All 21 focused campaign/provenance/replay tests pass. The full resource campaign was not rerun; this task remains open for end-to-end execution and independent acceptance.

**Investigation, 2026-10-23 (status unchanged):** no same-slice cost report exists. The regional strlen artifact is 171708 bytes. The SVF demand replay peaked at 537168 KiB RSS, but that run is not a valid comparator. Blocked on R06.1 and R05.2.

### F06.5 — Open: Calibrate defaults and record final producer/solver decision
Acceptance: not accepted

**Reviewed evidence and current disposition:** Initial producer preference is documented; solver acceptance remains provisional. Review: The initial producer preference is documented, but final producer/solver acceptance and calibrated defaults depend on useful closed summaries and same-slice precision/cost evidence. The plan's §§2.3, 6–7 need a final evidence-based update after that comparison. Needs useful closed summaries, same-slice precision/cost evidence, and updated plan §§2.3, 6–7.

- **R06.3 — Open** (depends on: R06.1, R06.2, R05.7)
  Acceptance: not accepted
  Calibrate defaults and update producer/solver decision and plan §§2.3, 6, and 7 from same-slice precision, coverage, and cost evidence.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

**Investigation, 2026-10-23 (status unchanged):** the acceptance is carried by R06.3, which depends on R06.1, R06.2 and R05.7, all Open. The section 7 defaults (32, 64, 128, 32 and 100,000 Z3 units) have no calibration data. Blocked on its dependencies.

#### Gate roll-up

- **F00 — Open:** PHP-source validation of the host transfers (F00.3) remains.
- **F01 — Open:** core evaluator checks exist; broader reviewed contracts and a calibrated host remain.
- **F02 — Open:** compiler/source evidence is narrow and does not establish broad native semantic correspondence.
- **F03 — Open:** reduced probes exist, but production demand summaries, pointer reuse, and native post-fixpoint validation do not.
- **F04 — Minimum Phase F fixture set complete; broader semantics deferred to C22.**
- **F05 — Open:** the exact `strlen` slice is connected, while profile coverage and stateful effects/exceptions remain incomplete.
- **F06 — Open:** no final solver decision or calibrated same-slice cost comparison exists.

## Investigation log

### Phase F partial and open tasks, 2026-10-23

Source: workflow run wf_24657b30-df1 (23 investigators, 3 validators, report). The full report text is outside the repository at `~/.cache/php-catalog-findings/phasef-report.md`. No task is closable now, and every status above is unchanged. Validator verdicts are mostly "unverified" (not reproduced) except where stated.

**Validator disagreement.** F05.7: the investigator recommended complete. The status-adversary validator rejected it, because R05.7 (acceptance not recorded; depends on R05.2 and R05.4) keeps the remainder open. The entry stays Open.

**Shared environment blocker.** The missing evaluation cache blocks F02.1, F02.3 (replay), F03.2, F03.3, F03.4, F06.3 and F06.4: whole-clang/php-cli.bc (sha256 e3427ce8...), bison 3.8.2, re2c 4.4, SVF at e889a513 with Z3 4.13.3, and the scratch root `~/.cache/php-catalog-evaluation`. Restoring them needs network access.

**Dependency and conflict findings (validator 3).**
- F00.1 and F01.2 decide the same cleanup cells in opposite directions (contract to code, or code to contract). One owner must choose the cell list first.
- F05.6's digest change (pin 3c51 to e3427ce8) conflicts with F02.1, F03.2 and F03.4, which replay against the 3c51 module, and with F01.1's byte-for-byte claim.
- F05.1 and F05.6 both edit `PhpTargetProfileSetting`. Design risk, not present in the code: if F05.6 adds `deny_unknown_fields` (settings.rs has none today), it would reject F05.1's new keys if it lands first.
- F01.5, F05.3 and R05.4 all extend `CATALOG_FUNCTIONS` and builtin_calls.rs, so the production scope must be decided once.
- F03.3 and F03.4 define two new schemas for the same consumer and must share one record definition.
- F05.7's pass goes stale after any change to F01.4, F05.4, F05.1, F05.6 or the code changes in the order below.
- Order conflicts: F01.2 is in WORK_STACK step 2 but depends on F01.5 and R02.7. F03.4 waits on F06.5 in one reading, while WORK_STACK places it in step 4.
- Ownership conflict: the ledger assigns extension and INI applicability to F05.6 (lines 356, 362), while F05.1 claims it.
- R05.2.4.5.2 (a separate reviewer) gates R05.2, which gates R05.4, R05.7, F02.5, F05.4, F01.2, F06.4 and F06.5. Its rule conflicts with the 2026-10-12 author-review policy (line 319 against the review's lines 1327-1331).

**Suggested order (validator 3; the other two validators gave different orders).**
0. Text fixes that need no decision: the F00.2 citation and the F00.4 schema text (done); the F01.1 checkpoint regex and the R01.7.a line; the stale "independent acceptance pending" wording. Record the base commit 105b004f1.
1. Decisions: the cleanup cell list (F00.1 with F01.2); the canonical module digest (F05.6 against F02.1 and F03.2); the extension and INI owner (F05.1 against F05.6); the production builtin scope (F01.5, F05.3, R05.4).
2. Code, one patch at a time: the cleanup guard at lib.rs:402-427 with its test; extension and INI plumbing in settings.rs before unknown-key rejection; the callback false-arm fixture after the cleanup decision.
3. Evidence: restore the evaluation cache; run the SVF replay under the measurement lock (F02.1, F03.2); the R03.1 construction probe; R03.2 and R03.3 with one native summary schema.
4. Acceptance: F00.1, F00.2, F00.4, F01.1, F01.2; then R05.2 and R05.7; rerun F05.7 on the final base and record its commit.
5. F01.5 and F05.6, once F00, F02 and the scope are accepted; F02.3, F02.4 and F02.5 after R02.5, R02.7 and R05.2.
6. F06.1, then F06.3, F06.4 and F06.5.

**Checker gaps (for the checker owners, not ledger text).**
- F05.3: the null deprecation is emitted only for strlen. PHP 8.5.4 emits it for `str_repeat` and `strtoupper` (checked with php -r); the gate at parameters.rs:203 limits the checker to strlen.
- F05.6: unknown `php_target_profile` keys are ignored silently: the settings struct has no `deny_unknown_fields` (settings.rs:9-10 uses serde defaults only). A scratch patch was built and run in `~/.cache/php-catalog-phasef`, not applied to any repository.
- F05.1: settings.rs:60-61 sets extensions and ini to None.
- F02.3: `RecordedRootTests` fails without `PHP_CATALOG_SOURCE` and needs a skip gate.
- Checkpoint tool: `validate_checkpoint.py:136` skips indented R rows, so R01.7.a is invisible to it.

**Owner questions.**
- F00.4: accept under the 2026-10-12 policy now? Must "validated" cover the shipped strlen conclusions, which cite no assumptions?
- F01.2: stay Partial until F01.5 and R05.2 are in a Scope and Remainder line? Should the PhpThrow-to-NativeUnwind cell fail closed until it has oracle evidence?
- F01.4: define the matrix dimensions in the ledger before more fixture work?
- F02.1: restore the scratch root and the packages? This needs network access.
- F02.3: close on the narrow zif_strlen structural correspondence, or stay Partial?
- F02.4: close for the strlen slice with Scope and Remainder lines, or cover native behaviour beyond strlen?
- F02.5: is native unwind inside the strlen slice? If yes, who designs the boundary witness?
- F03.3: run R03.1 first, with the evaluation cache restored?
- F03.4: build the native summary producer and validator now, or wait for the F06.5 solver decision?
- F05.3: wait for R05.2 acceptance before R05.4, or proceed on the bounded contract?
- F05.4: narrow the scope, or implement R05.2.4 first? Which acceptance rule governs R05.2.4.5.2?
- F05.6: move the strlen module-digest pin to e3427ce8, with the artifact and its pins regenerated? Reject unknown profile keys, or accept them as extension, INI and compiler facts once those fields exist?
- F05.7: does R05.9's acceptance need the release-profile `@method` regression test that RV-14 asked for, or are the gate-profile fixtures enough?
- F06.1: close on the section 2.3 decision record alone, or also require a closed PHP summary and the R06.1 comparison?
- F06.5: which workstream owns the first closed PHP summary on the locked 8.5.4 builtin slice? R06.1 cannot start without it.

**Direct verification, 2026-10-23.** I checked these claims against the source, the php binary and the result files:
- Verified: the cleanup acceptance matrix (lib.rs:402-430); no overlap, priority, state-token, snapshot or disjoint text in the contract; SAPI, zts and debug in the selected profile (semantics.rs:103-112) and extensions and ini set to None (settings.rs:60-61); the F05.6 pin 3c51 in build_strlen_catalog.rs and the witness digest e3427ce8 (strlen-llvm-witness.json:102); the absence of the evaluation cache at ~/.cache/php-catalog-evaluation; no Rust consumer of query_points_to and no native-summary type in crates or tools; the F05.7 counts of 1304 library and 4012 fixture passes (the result files); PHP 8.5.4 null deprecations for str_repeat and strtoupper.
- Corrected in the notes above: the checkpoint regex wording (F01.1), the checker deprecation scope (F05.3), the unknown-key wording (F05.6), and the deny_unknown_fields item, which is a design risk rather than current code.
- Not verified: the F02.3 RecordedRootTests failure, the F05.4 destroy-after-catch witness, and the F00.2 CBMC run.

**Not run.**
- Environment-dependent replays: F02.1, the F02.3 replay, F03.2, F03.3, F03.4, F06.3 and F06.4.
- CBMC is not installed, so the F00.2 transcripts were checked by recorded status counts only.
- The locked HYBRID and CALL binaries are absent, so F02.4 was checked with the system PHP 8.5.4 CLI only.
- F01.5, F05.1, F05.3 and F05.4 were checked by reading tests and fixtures, not by running them.
- F05.7's two reruns ran under the measurement lock with about 50 minutes of waiting.
- Git was refused to many investigators, so their results come from scratch copies, not from base commit 105b004f1.

### Coverage workflow, 2026-10-23

Source: workflow run wf_b033d2c9-2d6 (26 agents: 18 group investigators, 3 enumerators, 3 validators, 1 report). The full report text is at `~/.cache/php-catalog-findings/coverage3-report.md`, and the per-item results at `~/.cache/php-catalog-findings/coverage3-finished.md`; both are outside the repository. Counts: 346 results; 188 marked closable by their group, 158 not; validator verdicts were mostly "unverified", so the closable marks are not confirmed.

**Review items at HEAD 105b004f1 (groups review-1, -3, -4).** Closed at HEAD: RV-1 (defect), RV-2, RV-4 to RV-8, RV-17 to RV-31 (RV-18 fixed in code, with stale R06.4 text). RV-3 is partly closed: the reported defect is closed, but a Diagnostic closed domain with an empty dependency list is a new gap, and a user error handler can emit a further diagnostic. RV-9 to RV-16 are not in any result file; they appear only in a draft, `review-2.draft.txt`, with statuses "run pending", so they are unverified. Stale text found: contract lines 124-131 say the cleanup throw and fatal, suspension and native-unwind replacements are rejected, which the code does not do (see F00.1 above); RV-20's "no longer appears" claim is wrong, because the phrase "has no evidence and stays Open" still appears at PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md:868; R02.7 line 199 wording is stale.

**Memo section 5 claims (96).** Per the groups: 53 hold, 8 are corrected, 35 stay unverified (several blocked on heavy.lock or missing extensions). Validators disagree on S5-53 (the memo's row and run figures do not match: 31/112, 27/104, 29/109 and 22/95 all appear in different readings), on S5-85 (in_array with a string haystack is a TypeError, not a __toString call), and on S5-1, which the D14 probe contradicts (A=5, A_reports 0, MISS).

**Plan steps (builtin-runs plan, `~/.claude/plans/1-in-this-plan-woolly-dragon.md`).** Steps 0 to 10 and the closing section are open. Step 0 has its substance done but is not placed correctly; step 1 is not closed (V2 and V4 open). Each step's first next step is in the group result file.

**Builtin-runs gap categories** (counted from `tools/builtin-runs/scan/coverage-gaps.tsv`). state 78, value 126, not-recordable 20, arity 22, class-name 21, file-or-font 19, serialised 19, session 7, scope 2, crash 1, server 1,048; 1,363 rows in all. Scope and crash need a recorded decision.

**Replay-miss functions with no owner.** About 51 functions, plus unserialize (one route). They need owners in these families: constructor-run for builtin-created objects (mkdir, opendir, stat, touch, unlink, rmdir, pdo::query, the pdostatement family, recursivearrayiterator::getchildren, xmlreader factories); the SQLite3 callback family; the lazy-initializer family; random\randomizer (nine rows; no script names "random"); the libxml entity loader and assert.callback; XSLT and ZipArchive registrars.

**Memo cross-checks.** E5 (rename row) and E7 (curl chain) have no owner: no decision script reference names them (checked directly). D16 is absent from the decisions script (checked directly); its ship order was decided by the group, but its baseline (step 0) was not run.

**Ledger notes.** Checkpoint: F00.3 is recorded Complete at line 37 and "Open ... F00.3 remains" at the roll-up (line 434). Several R-row texts are stale: R00.4's regression count (10 against 12 tests), R05.3's message claim, R05.5's check at HEAD rather than 8ad93491, and R06.4's text against RV-18.

**Owner questions (from the group report).**
1. D16: gate shipping on a recorded failing set, or on a green baseline? The memo's default is the recorded set.
2. Assign owners to E5 and E7.
3. Record decisions for the scope and crash categories.
4. Does the constructor-run family for builtin-created objects get a decision?
5. Does the SQLite3 callback family get a decision (D1 covers PDO only)?
6. Does the lazy-initializer family (H3) get a decision?
7. Who owns the random\randomizer rows?
8. Decide the libxml external entity loader and assert.callback owners.
9. Holding-group decision (S5-92, S5-93): needs an owner before any recorder change for holders.
10. RV-3: justify the closed Diagnostic domain with an empty dependency list.
11. RV-1: amend contract lines 124-131 to the code, or narrow the code to the contract (see F00.1).
12. RV-26: regenerate or remove the stale subfolder record.
13. F00.3: Complete (line 37) or Open (line 434)?
14. S5-53: which memo figure is intended?
15. Plan step numbering for class-name (step 3 or 4).
16. Is session_start in the recorder sandbox in scope?
17. RV-9 to RV-16: turn the draft into result files, or record them as unverified.

**Direct checks, 2026-10-23.** Verified: the gap-file counts above; the contract text at lines 120-135 (stale against the code); E5 and E7 have no owner; D16 is absent from the decisions script. Refuted: R00.1 does not cite 479a948b. The ledger's only mentions are the R05.9 row, now bdb93af0f, and this log. Not verified: the RV-9 to RV-16 status, and the validators' disagreements, which I did not re-run.

### Direct validation, 2026-10-23 (second pass)

Checked by reading the source and files, and by running tests, probes and php -r. Only the claims listed here were checked. Everything else in the finished workflows stays unverified.

**Verified:**
- F05.1 settings: the target-profile tests pass (5), and no predicate test covers extension or INI facts.
- F05.4: the 36 catalog-filtered library tests pass.
- F00.2: the CBMC property counts are 93, 94 and 2, with exit codes 0, 10 and 10 (campaign-completed.json).
- F00.4: the 118 semantics tests and 16 assumption-filtered tests pass. The artifact has no assumptions but carries 18 evidence records.
- F01.1: the checkpoint regex in validate_checkpoint.py:136 does not match R01.7.a, and matches only lines starting with "- **R<n>.<n> — ".
- F02.3: the correspondence regeneration is byte-identical with PHP_CATALOG_SOURCE set. RecordedRootTests errors without it (one error, one skipped regeneration test).
- F02.4: the 13-sentinel and 16-sentinel records pass. The 13-sentinel fixture's hash matches, and the system PHP 8.5.4 reproduces the recorded CALL output byte for byte.
- F02.1: whole-clang/php-cli.bc is absent from the home directory. bison, re2c, Z3 and CBMC are not installed. clang and llvm-link are installed.
- F03.2: the reduced SVF capsule sets empty_target_is_not_closed to true. The SVF replay peaked at 537168 KiB.
- F05.6: the builder pin 3c51... is at build_strlen_catalog.rs (the assert). The witness digest is e3427ce8 (strlen-llvm-witness.json:102). Unknown profile keys are ignored, because the settings struct has no deny_unknown_fields.
- F05.3: the checker emits the null deprecation only for strlen (parameters.rs:203). PHP 8.5.4 emits it for str_repeat and strtoupper, and no fixture expects it for them.
- F05.7: lens 2 was right. R05.7 is Open at ledger line 417, so F05.7 stays Open.
- F06.4 and F06.5: 537168 KiB and 171708 bytes were checked. The plan's section 7 defaults are 32, 64, 128 and 32 (line 454) and 100,000 (line 464).
- RV-3: the artifact's Diagnostic domain is Closed with an empty dependency list. Under set_error_handler, a handler that calls trigger_error produces a further diagnostic.
- RV-9 and RV-10: the draft marks the tests "run pending". The cited lines exist.
- RV-13: the campaign-admission tests pass, 11 of 11.
- RV-18: require_success is at lines 234, 328 and 336. The regression tests test_waits_for_a_late_enforcing_result and test_missing_exit_record_after_cgroup_disappears_fails_closed exist.
- RV-19: the lock hashes match, including runtime.php (94dc6509).
- RV-20: the phrase "has no evidence and stays Open" is at review line 868.
- RV-23: commit a00c6426 has 30 files and 36781 insertions.
- RV-26: the duplicate strlen-native-demand-svf.json exists in the results folder and in its subfolder.
- RV-29: fixtures-strlen-2 passes 8 tests.
- Owners: gc_collect_cycles is named in D10, and unserialize and ReflectionClass::newInstance are named in D14. The owner-none names (mkdir, stat, touch, pdostatement and getchildren) appear only in generic text in the scripts, so they have no owner.
- Plan gate lines: plan.php:145 and :196 and unit.php:192 hold the gates.
- Checkpoint: 40 open and 8 scoped entries.
- R00.4: the CBMC validation module has 12 tests, against the row's "ten".
- R05.9: the lib gate passes, 1304 tests.
- F00.3 and the roll-up disagree (lines 37 and 434).

**Refuted or corrected:**
- S5-1 (memo c07-3, "silent miss is sound"): refuted. The D14 probe row (A=5, A_reports 0) is a silent MISS. The stub has no #[Pure] on newInstance, so the stub effect is unknown, but silence is not sound.
- S5-53 (memo figure 31 rows and 112 runs): not reproduced under any filter tried. These do reproduce: 22 rows and 95 runs (callable-unplaced with "handler"), 27 and 104 (callable-unplaced with "handler" or "error"), and 29 and 109 (all statuses with "handler").
- S5-85: the in_array fact is verified. A string haystack raises a TypeError, so lens 1's disagreement was right.
- R00.1: refuted. The R00.1 entry does not cite 479a948b.
- R06.4 "text stale against RV-18": refuted. The text matches the code and its tests.
- D16 baseline: partly recorded. fixtures.txt completed with 4012 passing. lib.txt and runtime_oracle.txt contain only compile output, so no test run completed for either.
- F00.4 "no evidence records": corrected in the F00.4 note above.
- F02.3 "15 passed with the source set": the run has 12 tests, all passing with the source set. The investigator's count was wrong.

**Still unverified:**
- The memo section 5 claims beyond the ones above (about 90).
- The owner-none replay rows, about 50.
- The category first steps and the plan's first next steps.
- R05.5: no result file shows the check at 8ad93491, and the commit exists.
- R05.3.3: the message claim.
- The F05.4 strlen-fixture count.
- Phase F claims beyond the samples above.
- The lens-2 and lens-3 disagreements beyond those above.

### Direct validation, 2026-10-23 (third pass: memo section 5 refutations S5-78 to S5-95)

Each item below was checked with php 8.5.4 scripts under `~/.cache/php-catalog-validate3/`, or by reading the cited result rows. No status or acceptance line was changed.

**Verified:**
- S5-78 (SW-4): `fopen('w78://x', 'a')` calls the user wrapper's `stream_open`, `stream_seek` and `stream_tell`, so the refutation of SW-4 holds.
- S5-79 (c07-1): `unserialize` of a discarded object runs `__destruct` (twice in the probe), so the discard path is observable.
- S5-80: after `session_destroy()`, `isset($_SESSION['a'])` is `true`, so `$_SESSION` survives destruction.
- S5-81 (c08-08): with `session_set_save_handler`, `session_start()` runs `open` and `read`, and `session_write_close()` runs `write` and `close` (order `open,read,started,write,close`). Dropping `takes_callable` alone would therefore stop the registration-time run, and the session runs would be missing. The refutation holds.
- S5-82 (c09-05): an engine notice (undefined array key) and a builtin warning (`file_get_contents` failure) both reach a `set_error_handler` callback. The documented partial, which lists only `trigger_error` and `user_error`, is silent for both. The refutation holds; the choice between opt-in and documented gap stays with the owner.
- S5-83: a namespaced `strlen("abc")` returns 3, so the global fallback exists. Exact matching without the fallback drops it.
- S5-84: `strval` and `sprintf('%s')` call `__toString`; `print_r` and `array_push` do not. A type-only exclusion of the 38 is therefore unsound. The `array_push` reflection gives parameter 0 by reference and parameter 1 variadic, which matches the array-companion wording.
- S5-85: `in_array` with a string haystack raises `TypeError`; with an int haystack and an object needle it emits "Object of class T could not be converted to int" and does not call `__toString`. The gate depends on the other argument's value, not its declared type.
- S5-87: a stream from `tmpfile()` that has a user filter appended calls `onClose` at scope exit, both when created in the function and when returned from a helper. A release model limited to `fopen` or `stream_filter_append` misses it.
- S5-88 (C10-7): `include 'php://filter/read=uf88/resource=…'` runs the user filter's `onCreate` and `filter()`. Option B's resource-parameter rule does not reach `include`, so the refutation holds.
- S5-89 (C10-8): the floor names `fopen`, `file_get_contents`, `file` and `readfile`. `include` of a `php://filter` URL runs `filter()` and is not in the floor, so it is a silent miss. The refutation holds.
- S5-93 (c06): the PDO and DOMXPath rows in `results-base.tsv` (`pdo-query`, `pdo-exec`, `domxpath-evaluate`, `domxpath-query`) show `A_reports` 1 for the A case. The rows read FALSE-POS because `B_reports` is also 1. Removing the held runs would therefore silence the A cases (a MISS), so the refutation holds. The FALSE-POS verdict is a separate issue from the one the memo describes.
- S5-94 (C12-04): `(new C)->m()` with `__call` but no `m` dispatches to `__call`, and `(new D)->m()` dispatches to `D::m`. The scratch narrowing's lookup on the static class C would miss the runtime D, as the item says.
- S5-95 (c04-02): `foreach` over an `Iterator` calls `rewind` and `valid` with no named call in the source, so a callee-name index cannot see them. The refutation holds for method-name keys through `foreach`.
- S5-1 and S5-53: see the second-pass log above.

**Not verified here:**
- S5-86: the memo's own status is `needs-work` and the item's example needs correcting (the coverage-3 finding). The argument_runs trace was not run, so the cause stays a hypothesis.
- S5-90: the libxml loader paths were not exercised, so the list of entry points is unchecked.
- S5-91: the registrar branch for `Callee::New` was read in the findings, not in the code in this pass.
- S5-92: the rows checked are the PDO and DOMXPath rows only. The fiber-resume, appenditer-next and filteriter-rewind rows were not read in this pass.
- S5-96: no refutation to check.

**Still unverified after this pass:** the owner-none replay rows (about 50), the category first steps, R05.5 at 8ad93491, R05.3.3, the F05.4 strlen-fixture count, the remaining memo section 5 "Could not support" claims (mostly probe-dependent), and the lens-2 and lens-3 disagreements not listed above.

### Direct validation, 2026-10-23 (fourth pass: the remaining items from the third pass)

No new workflows were started. The gap and decision workflows still hold `heavy.lock` (about a dozen queued waiters at the time of this pass), so no fixture or corpus run was started here. Results come from files already on disk and from code reads.

**Verified:**
- R05.5 (the eight PHPDoc fixtures pass at `8ad93491`): the baseline full gate fixture run at `105b004f1` (`decisions-run2/baseline/fixtures.txt`, worktree at `105b004f1`) reports 4012 passed, 0 failed, with 58 PHPDoc-related test lines ok. The PHPDoc fixture directories (`phpdoc_native_compat`, `phpdoc_parse_errors`) have no changed files between `8ad93491` and `105b004f1`. The claim is supported at HEAD and by its unchanged inputs; the run at `8ad93491` itself was not repeated, and the entry's "eight" is not mapped to named fixtures.
- F05.4 "the 20 strlen fixtures pass at `8ad93491`" (cited in `PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md:1133`): the baseline run has 24 strlen-named fixture tests, all ok, 20 of them under `checker/builtin_range_narrowing` and 4 under `checker/closures_and_callables`. Seventeen fixture files in those two directories changed between `8ad93491` and HEAD, so the pass at `8ad93491` is not directly shown.
- R05.3.3: the test `exact_dynamic_strlen_preserves_argument_diagnostics_once` (`catalog_object_argument.rs:33`) asserts one `argument.type` diagnostic for each side and the dynamic message `Parameter #1 $string of callable 'strlen' expects string, array{} given.`. It does not compare the direct message with the dynamic one. The two differ, so the ledger sentence "their diagnostic messages match" was wrong; it is corrected in the R05.3.3 entry. The "18 passed, 1 ignored" count was not re-run.
- S5-91 (c03, Fiber registrar as written): `registration` in `callbacks-families.patch` (`~/.cache/php-catalog-continue/callbacks-families.patch`, lines 194-199) returns `true` (known, no target) whenever the callee is not `Callee::Function`. `new Fiber(closure)` is `Callee::New`, so the closure is not registered. The refutation holds for the patch text; the patch is not in the tree.
- S5-92 (c03, holding option b): the shipped rows show `A_reports` 1 for `fiber-resume` (15 rows), `filteriter-rewind` (11 rows) and `appenditer-next` (10 rows). The patched run `results-D1-r3-after.tsv` has one `appenditer-next` row with `A_reports` 0, a MISS. That is the silence the refutation predicts for option (b).

**Still unverified:**
- S5-86: the memo's example needs correcting (coverage-3 verdict `needs-work`); the argument_runs trace was not run.
- S5-90: the libxml loader entry points were not exercised.
- The owner-none replay rows (about 50) and the category first steps.
- R05.5 and the F05.4 count at `8ad93491` itself: they need a gate build and fixtures run at that commit, which waits on the lock.
- The remaining memo section 5 "could not support" claims (mostly probe-dependent) and the lens-2 and lens-3 disagreements.
- The decisions workflow (`wf_2b15efc5-124`) and gap workflow (`wf_bd518c9a-d5d`) are still running. Their findings are not validated and are not in the markdown.

### Direct validation, 2026-10-23 (fifth pass: S5-86, S5-90 and the owner-none rows)

The decisions and gap workflows were still running (26 queued `flock` waiters at the start of this pass), so no fixture or corpus run was started. Checks used php 8.5.4 scripts under `~/.cache/php-catalog-validate3/s90/` and code reads.

**S5-90 (libxml loader families), verified by runtime:** with `libxml_set_external_entity_loader` set to a counting closure, the loader is invoked once each for `DOMDocument::loadHTMLFile`, `DOMDocument::loadXML` (with `LIBXML_NOENT` and an external entity), `DOMDocument::load`, `DOMDocument::schemaValidate`, `DOMDocument::relaxNGValidate`, and `Dom\XMLDocument::createFromFile`. `DOMDocument::loadHTML` with a string makes no call. `XSLTProcessor::transformToUri` makes one loader call at transform time (a `document()` call), when the loader is installed after `importStylesheet`. Two corrections to the memo's list: `loadXML` with `LIBXML_NOENT` is an entry point the memo does not name, and `importStylesheet` also calls the loader while a loader is set (it returned false in my runs, so the loader's return value affects the import). Since the loader is user code, STATUS.md's "Silent, correct" for `DOMDocument::loadHTMLFile` holds only while the loader writes nothing.

**S5-86 (literal callable arrays with a call-result receiver), mechanism corrected by code reading; silence not established:** the memo's account is that a call-result receiver falls to `argument_runs` returning `None`. The code does not return `None` there. A literal array whose first element is not a string becomes `PairTarget::Object(receiver(...))` (`registry/summaries.rs:895`). `callable_targets` sends it to `call_targets`, and for `Callee::Method` with a method name the `Some(method)` arms return a list (`registry/effects.rs:3644-3720`: `implementation_ixs`, `exact_method` or `methods_named_ixs`), which can be empty but is not `None`. `None` comes only from `Holder::Unknown | Holder::Made(None)` with no method (a literal array always has one) and from `static_targets` when `named_classes` cannot resolve a class (`effects.rs:4341`). So a call-result receiver can produce an empty run list, which is silent in effect, and the claim that it is silent because of `None` is wrong. A trace of `argument_runs` (not run here) would settle which path is taken.

**Owner-none rows, found to be overstated:** `enum-unowned.json` (commit 105b004f1, 220 functions, 29 owned, 191 unowned) uses literal name matching only. At least 15 of the unowned names carry the class prefix of a D1 case group (`appenditerator::*`, `filteriterator::rewind`, `fiber::*`, `pdo::*`, `pdostatement::*`, `domxpath::*`), and about 27 more match names in D7, D10, D12, D13 and D14 scope (`session_start`, `session_write_close`, `session_encode`, `session_decode`, `session_gc`, `session_create_id`, `session_commit`, `unserialize`, `reflectionclass::newinstance`, `reflectionattribute::newinstance`, `gc_collect_cycles`, `curl_exec`, `curl_multi_exec`, `ob_start`, `spl_autoload`, `spl_autoload_call`, `stream_filter_append`, `stream_filter_remove`, `fopen`, `stream_get_contents`, `stream_copy_to_stream`, `stream_select`, `stream_socket_client`, `stream_socket_server`). Name matches are not proof of ownership; the 191 figure should not be used as an owner-none count until the match is redone with the decision case files.

**Still unverified:** the category first steps; the memo section 5 "could not support" claims (mostly probe-dependent); the lens-2 and lens-3 disagreements; S5-86's silence (needs the `argument_runs` trace); R05.5 and the F05.4 count at `8ad93491` (needs a gate build and fixtures run, which waits on the lock); the decisions and gap workflows, which are still running.

### Decisions workflow (memo D1-D19), finished 2026-10-24, and its validation

The probe-memo decisions workflow (`catalog-decisions-full`, run 3) finished. Its verdicts below are the workflow's own self-verdicts, not acceptance, which stays with the author. Everything in the patches lives in scratch worktrees and `~/.cache/php-catalog-continue/decisions-run2/`; nothing was applied to this repository and nothing was committed.

**Outcome as the workflow reported it:** 9 of 18 decisions returned a decision record (D1, D2, D3, D4, D6, D9, D14, D15 accept or accept-with-conditions; D7 reject; D5, D8, D10, D11, D13, D17, D18, D19 blocked; D12 returned nothing). The baseline was incomplete: no lib, fixture or corpus failure set was recorded before the changes. Integration applied D3, D6, D1, D2, D4 and D15, and skipped D9 (adversary counterexample), D14 (its condition depends on D9), and D7 (reject).

**Verified by this pass:**
- Integrated tests: `cargo test -p phpstan-rs --lib` 1307 passed, 0 failed (`integrated/lib.txt`); `--test fixtures` 4012 passed (`integrated/fixtures.txt`); `--test runtime_oracle` 4012 passed with the oracle line 384 executed, 3479 not executed, 149 skipped (`integrated/runtime_oracle.txt`). The base fixtures run at `105b004f1` is 4012 passed (`baseline/fixtures.txt`, gate profile). The integrated build is the dev profile, so the counts are comparable but not the same profile.
- Corpus (regression check only, not a basis for any decision): totals match at 2792 files and 614 diagnostics. Comparing line multisets of the sorted snapshots shows one lost `cast.useless` at `triggers_list.php:1083` (column 66) and one gained `property.notFound` at `Latte/TemplateCallSiteVisitor.php:49` (column 29). The warm run equals the cold run, and the base warm run equals its cold run.
- Probe (`integrated/results-integ-r2.tsv`): 213 rows, verdicts 58 ok, 38 FALSE-POS, 86 MISS, 31 invalid-case, as reported. Against the shipped base rows, no id goes to MISS that the base never had. The only id that loses its ok is `array-multisort-tostring` in its typed form (base `results-base-typed.tsv` ok; D15 after `results-D15-regress-after.tsv` FALSE-POS). That is a precision loss, not a miss, and D15 disclosed it.
- D1's six transitions (FALSE-POS to ok in the integrated run): `pdo-query`, `pdo-exec`, `domxpath-evaluate`, `domxpath-query`, `fiber-resume`, `fiber-throw`.
- D9 (rejected by the adversary lens): reproduced. GMP is not installed on this php 8.5.4 CLI. `new \GMP(1)` calls the registered autoloader, which writes `$b->value = 5` before the `Error: Class "GMP" not found`. A checker that exempts the stub-known class therefore misses this write. The integrator's skip is correct under the soundness rule.
- D13: the decision says its probes never ran, but `results-D13-base.tsv` and `results-D13-after1.tsv` exist (written 00:11 and 00:16). Both files are identical: every session case is MISS (`sess-session-start`, `sess-session-write-close`, `D13-sess-*`). The patch therefore has no measured effect on these probes. The decision record's "unverified" is stale, and lens 1 was right to flag it.
- D12 returned no record, so nothing about it can be validated.
- Lens-1 and lens-3 statements that D5, D8, D10, D18 and D19 have no patch file: confirmed by listing `decisions-run2/`. These have no patch to integrate. D11, D12, D13 and D17 do have patch files (`D11.patch`, `D12.patch`, `D13.patch`, `D17.patch`), but none of them was integrated, and their probe evidence is unmeasured or, for D13, has no effect (above).

**Not verified:**
- The workflow's baseline lib failure set is empty only because no baseline run completed. The "no new failures" claim is therefore against zero, not against a measured base.
- The D2 corpus-lost-findings count was not measured (the workflow lists it as a condition). The corpus swap above is the only corpus measurement, and it covers only the integrated tree.
- The D1 callable-group route and its soundness conditions were checked by the lens reviews, not by a new adversary run in this pass.
- The gap workflow (`wf_bd518c9a-d5d`) was still validating when this pass started (its three validators were launched at 06:08). Its findings are not included here.

### Gap workflow (R rows, F tasks and memo gaps), finished 2026-10-24, and its validation

The gap workflow (`php-catalog-gaps`) finished with 136 investigation records (126 not blocked, 88 needs-work, 38 ready-to-record, 10 blocked; 26 marked closable now) and three validator reviews. Its report says it was written from a recovered summary rather than from the investigation records, so the per-row status in the report is partial; the record data is the primary source here. Its verdicts are self-verdicts, not acceptance. Nothing was applied to this repository and nothing was committed. Scratch patches are under `~/.cache/php-catalog-gaps/` (`CLASSIFIER.patch`, `F01.3.patch`, `F04.1.patch`, `R05.1.patch`, `R05.8.probe.patch`).

**Verified by this pass:**
- **CLASSIFIER (replay keying).** The committed replay summary reproduces (`keys` 1183, `union_covered_keys` 555, `union_misses_outside_holes` 625, `union_misses_outside_holes_calls` 2740951). The fixed replay in `~/.cache/php-catalog-gaps/classifier/run-fixed` gives 560, 620 and 2740903. The diff of the miss lists removes exactly five keys: `clone ? other-method`, `appenditerator::rewind`, `multipleiterator::key`, `spldoublylinkedlist::serialize` and `splobjectstorage::serialize`. The table rows the report cites are present (`clone 0 method __clone`, `appenditerator::rewind by:append:0`, `multipleiterator::key by:attachiterator:0`, the two `__serialize` rows). The credit is a rule change, so it needs an owner decision. The tracer fix (record the operand as place 0) is the proper repair.
- **settype.** The table has only `settype  probed` (line 9715), with no place-0 rows. The memo's claim is false.
- **Recorder sidecar is stale.** `recorder_hash()` (`tools/builtin-runs/lib/cache.php`, run under php) gives `c1d438e9…`. The sidecar `crates/phpstan-rs/src/registry/data/builtin-runs-8.5.meta.json` records `47b6d59f…`. The table is therefore keyed to a recorder that no longer exists. This must be fixed by regenerating through the recorder, not by editing the sidecar or table by hand.
- **Module digest conflict (F02.2, F05.2, R01.6, R01.8, R02.2, R05.1).** The builder pins `0x3c, 0x51, 0xa5, 0x31, …` (`crates/php-semantics/examples/build_strlen_catalog.rs:676`), which is `3c51a531`. The witness JSON records `e3427ce8…` (`results/evaluation/strlen-llvm-witness.json:102`). No `.bc` module file exists under `tools/`. The owner must choose which digest is authoritative before any regeneration claim is made.
- **R02.6 formatting.** `rustfmt --check crates/phpstan-rs/src/registry/effects.rs` exits 1 at HEAD, and the file is clean in git, so the failure is already in HEAD.
- **R01.5 cleanup replacement.** `check_cleanup_replacement` accepts five kinds for a pending PHP throw (`lib.rs:402-430`), and the contract allows Return only. This matches the conflict already recorded in the third pass.
- **C04-05 (generator destroy).** PHP runs a generator's local `__destruct` when the generator is destroyed (`unset($g)` while suspended prints `[dtor]` before "after destroy"). A narrowing that treats generator destruction as running no local destructor is unsound, as the investigation says.
- **C12-06 (private method with `__call`).** From outside the class, `$p->m()` with a private `m` and a `__call` dispatches to `__call`. This is a confirmed miss for a checker that resolves the private method.
- **SW-1 (stream rename).** `st-rename` is MISS in both the shipped base rows and the integrated run, so the row is consistent with the investigation.

**Corrections to the workflow's record:**
- Lens 1 says "R05.1 digest pins not found in build_strlen_catalog.rs". The pin is at line 676, so that statement is wrong.
- The report says `git` was refused on the php-catalog checkout. In this session `git` works on it, so the refusal was the worktree environment, not the repository.
- The report says 136 records but the task text listed 127 ids. The workflow left this unresolved; the 136 records are what exist.

**Not verified:**
- C12-04's "DBG eprintln at patch line 305": the worktree that held the patch is no longer present, and no scratch patch under `C12-04/` contains it.
- G9's value counts (126 against 33) and the session-case count (7 against 5): lens 2 disputes them, and neither count was recomputed here.
- R02.2's ImplicitDtor grep and F03.1's `lock.json` lookup (lens 1): not re-run.
- The "about 40 uncovered functions" the report leaves unrecorded: the investigation records contain them, and they were not read one by one here.
- The 14 memo "no row" functions: the report says all 14 are absent from the table (`functions.tsv` shows 34 no-row functions in total, a different count), and this was not recomputed.
- The memo and `STATUS.md` are in the probe folder, outside the repository, and were not edited. The settype correction, the 1,363 gap-row figure and the 19 stale STATUS statements need the author.

### Owner-decision follow-up, 2026-10-24: digest, tracer, regeneration and cleanup

Work under the "most proper" plan, in the order the plan gave. Nothing is committed; the contract edit is in the working tree.

**Module digest (`3c51a531…` vs `e3427ce8…`): the pin is the rebuilt module, and the committed witness is stale.**
- History: `c5e1a4268` (2026-10-12, "regenerate the strlen catalog from the rebuilt linked module") moved the builder's pin from `e3427ce8…` to `3c51a531…`. Its message says the pin follows the rebuilt CLI, which `link-manifest.json` (commit `b00b5c883`, 2026-10-12, "the rebuilt, correctly configured CLI") binds to `linked_bitcode_sha256` `3c51a531…`.
- The witness (`strlen-llvm-witness.json`) and the inventory (`strlen-llvm-operation-inventory.json`) were last changed on 2026-10-09 and were not re-extracted in that commit. The extractor writes `linked_bitcode.sha256` as the SHA-256 of the linked-module file it was given, so the committed witness still records the misconfigured module, `e3427ce8…`.
- Running the builder on the committed inputs fails. With the witness, it stops at the missing `zif_strlen_operation_inventory` key. With the inventory file, it stops at the module-digest assertion (`build_strlen_catalog.rs:673`): witness-derived `e3427ce8…` against pinned `3c51a531…`. The builder cannot run from HEAD on the committed inputs.
- The commit message says "the lock pins its digest". `lock.json` contains no `3c51a531…`, so that claim is unsupported.
- The module itself is not on this machine (no `.bc` over 500 KB outside the incremental caches). Re-extraction needs a rebuild of the CLI with the configuration in the manifest. `bison` and `re2c` are not installed, and the PHP tree is a bare checkout with no generated parser or scanner files and no `configure`. `link-manifest.json` also records `all_link_objects_imported: false` for two native-assembly objects, which the rebuild must reproduce.
- Decision: `3c51a531…` is the authoritative module digest. The witness and inventory must be re-extracted from the rebuilt module, and the rebuild is blocked on installing `bison` and `re2c`. Nothing was changed for this item.

**Tracer (`clone` operand as place 0): investigated, not changed.** The `?` place is assigned by `compare.py` (`other-method` when no place is recorded). `cattrace.c` has no hook on the clone opcode, so the source object is never seen. The fix needs a hook that captures the source object. It is not designed or built. The tracer builds as an extension against the host PHP 8.5.4 (`phpize` and `php-config` are installed), so it is not blocked by the digest.

**Cleanup replacement: the code is right, and the contract was stale.**
- `check_cleanup_replacement` accepts five replacement kinds over a pending PHP throw. The oracle backs all five: `cleanup_destructor_returns`, `_throws`, `_exits`, `_fatal` and `_suspends` in `strlen-host-oracle-8.5.4.json`. The matrix test `cleanup_replacement_matrix_allows_evidenced_php_throw_replacements_only` and the other cleanup tests pass (`cargo test -p phpstan-rs-semantics --lib cleanup`, 15 passed).
- `PHP_SEMANTICS_CATALOG_CONTRACT.md` (cleanup paragraph and the composition rule) said the sole modelled cell was cleanup `Return`. I corrected it to the five cells, with their oracle cases, and kept the rule that replacement of a pending fatal, suspension or native-unwind completion is rejected. Two points are stated in the contract: the model records no previous-exception link, and `exit()` is modelled as the native-unwind completion (the checker's `user_code_exits` lists NativeUnwind, the exit kind; no separate test pins exit's mapping).
- The ledger's R01.5 entry ("contract allows Return only") is now out of date with the contract. Acceptance stays with the author.

**Regeneration:** not done. It depends on the digest decision and the tracer fix.

**Next actions, each needing your go:** install `bison` and `re2c` (a system package install, so it is yours to approve); then rebuild the CLI, check the digest against `3c51a531…`, re-extract the witness and inventory, and rerun the builder.

### Digest rebuild, 2026-10-24: the pinned `3c51a531…` is not reproduced

Installed `bison` 3.8.2 and `re2c` 4.4 with apt. Built the locked PHP 8.5.4 NTS CLI (`tools/php-catalog/experiments/comparison/build_php_pathfree.sh`, source `~/.cache/php-src-8.5.4` at `e5e09d06`, output `~/.cache/php-catalog-digest/cli/php-src/sapi/cli/php`, reports `PHP 8.5.4 (NTS)`). Ran `evaluation.py whole_link` into `~/.cache/php-catalog-digest/link/`: 523 of 525 objects imported (the two `.S` objects are the declared native-assembly boundary), `llvm-link-21` and `opt-21 -passes=verify` succeed.

- The rebuilt linked module has SHA-256 `37de322fa6b5a9b475…` (`link/link-manifest.json`, `linked_bitcode_sha256`). It is not `3c51a531…` (the committed manifest, `b00b5c883`) and not `e3427ce8…` (the witness, 2026-10-09).
- **What matches.** All 525 objects have the committed source digests. All 128 recorded source and header digests in `results/dependencies.json` match this tree, including `main/php_config.h` (`c009cfa8…`). The configure options match the recorded hybrid profile. The producer is the locked `clang version 21.1.8`. The bitcode is deterministic: two compiles of `Zend/zend_API.c` with the recorded command give identical digests, and that digest equals the one the new manifest records. Compile flags are identical for every object once output paths and prefix maps are normalised.
- **What does not match.** 501 objects have the same bitcode digest as the committed manifest. 24 do not, among them `Zend/zend_API.o`, `Zend/zend_execute.o`, `Zend/zend_operators.o`, `Zend/zend_alloc.o`, the Optimizer files, the PCRE2 JIT, and `ext/standard/{html,math,url}.c`. Their flags, sources and recorded headers match, and `main/php_config.h` is also used by matching objects. I did not find the input that differs.
- Consequence: the committed `3c51a531…` cannot be reproduced from the locked sources and recorded configuration on this machine, and the 12 October build inputs are not fully recorded. `37de322f…` is reproducible here.

**Owner decision needed (not changed):** choose the authoritative module digest. (a) Adopt the reproducible `37de322f…`: pin it, re-extract the witness and inventory from `link/php-cli.bc`, and re-issue `link-manifest.json`. Pros: anyone with the locked sources can verify it. Cons: it moves the pin from the reviewed 12 October value, and every evidence identifier that derives from the digest changes. (b) Keep `3c51a531…` and find the environment input behind the 24 objects before anything moves. Pros: no pin change. Cons: the input is still unknown after the checks above, so this may not converge.

Nothing was committed. The builder, witness, inventory, manifest and catalog data are unchanged.

### Digest adopted, 2026-10-24: the rebuilt module `37de322f…` is the pin

Owner decision (option 1): the pinned module digest is the reproducible rebuild `37de322fa6b5a9b475f55ded6e22b8f712288e225acfbd39896ee8fa46a2c350`. The 2026-10-12 value `3c51a531…` is no longer the pin. Nothing is committed.

**Changed in the working tree:**
- Pin, in three places: `crates/php-semantics/examples/build_strlen_catalog.rs` (the builder's assertion) and two `Digest` arrays in `crates/phpstan-rs/src/semantics.rs`.
- `results/evaluation/strlen-llvm-operation-inventory.json` and `strlen-llvm-witness.json`: both regenerated from `link/php-cli.bc` by `extract_strlen_llvm_witness.py` (241 instructions, `linked_bitcode.sha256` `37de322f…`). The witness file previously held the 2026-10-09 `e3427ce8…` module and no inventory key.
- `crates/php-semantics/data/php-8.5.4-strlen.json`: regenerated by `build_strlen_catalog` from the new inventory and the runtime witness (`cargo run --example`, output compared as in the README).
- `results/evaluation/link-manifest.json`: the rebuilt manifest, with build paths normalised to `<cache>` and `<repo>` as the committed manifest does.
- `PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md` (F02.1 result) and `PHP_SEMANTICS_CATALOG_EVALUATION.md` (witness digest): the current-state sentences now give `37de322f…` and record the earlier value as history. The earlier ledger entries in this log are unchanged.

**Verification:**
- Builder: the regenerated catalog is canonically equal to the committed one (digest arrays and identifiers removed, records sorted): the same records under the new identifiers, as the 2026-10-12 commit claimed for its own regeneration.
- `cargo test -p phpstan-rs-semantics --lib`: 118 passed. `--example build_strlen_catalog`: 9 passed. (The contract review's "5 passed" for this example is stale.)
- `cargo test -p phpstan-rs --lib`: 1304 passed, 0 failed, 1 ignored (the same count as before the change).
- `cargo test -p phpstan-rs --test fixtures`: 4012 passed, 0 failed. Both heavy runs used `flock -w 3600` on the heavy lock.
- Formatting: `rustfmt` on the two Rust files changed only the digest arrays (9 insertions, 9 deletions).

**Still stale, not regenerated:**
- `results/evaluation/strlen-native-demand-svf.json` (and its duplicate in the SVF subfolder) carry the `3c51a531…` source digest. They are outputs of the SVF tool (`dvf`), which is not installed. Regenerating them needs that tool.
- The F02.2 / F05.2 / R01.6 / R01.8 / R02.2 / R05.1 claims that cite the old pin are not re-checked here beyond the builder and tests above.
- The tracer fix (clone operand as place 0), the classifier change, and the recorder sidecar regeneration are not done. The sidecar's stale recorder hash (`47b6d59f…` vs the current `c1d438e9…`) is unchanged.
- Corpus check not run; the digest change does not touch PHP analysis code, but this has not been measured.

### Tracer re-recording and replay, 2026-10-24

The tracer's clone fix (`cattrace.c`: the operand of `clone` is place 0 of the internal call, so `__clone` on the new object is keyed by the object cloned) was run over the whole php-src 8.5.4 test suite. Nothing is committed.

**Run:** the scratch extension (`~/.cache/php-catalog-digest/tracer/modules/cattrace.so`) is built from the repo's `cattrace.c` (`diff` clean). The recording `php.ini` is the host's `php.ini` plus `conf.d` minus excimer, pcov, SPX and xhprof, plus the tracer, and loads `cattrace` (checked). Two runs under the heavy lock: `ext/standard` (3808 tests: 3299 passed, 500 skipped, 9 failed) and the rest, `Zend`, `tests`, `sapi` and every other `ext/*` (18020 tests: 14437 passed, 3395 skipped, 188 failed). The failures are the php-src tests' own failures, not tracer effects. The two runs total 21828 test runs against 21458 `.phpt` files that `find` counts; the 370 extra are not explained yet (no test is run twice by construction: `ext/standard` is excluded from the second run). The trace has 3026 aggregated lines, all with 8 fields.

**Scripts:** `compare.py` took its php-src prefix from a hard-coded path into the old recording folder. It now takes the tree root as an optional fourth argument.

**Results:** the derived-bound dump was regenerated (`writes_the_derived_bound`, 203076 rows); it has the same rows as the 2026-10-22 dump in a different order, so the bound is unchanged.

| | 2026-10-22 (old tracer) | 2026-10-24 (fixed clone place) |
|---|---|---|
| observed keys | 1183 | 1182 |
| missing from table | 217 | 223 |
| outside bound vocabulary | not recorded | 125 |
| union-covered keys | 555 | 556 |
| union misses outside holes | 625 | 623 |
| calls replayed | 4028427 | 4027793 |

The `__clone` keys leave `not-a-row/callable-unplaced` and `user-function` and reappear under `receives-argument`, which is the intended effect. Committed results in `tools/php-catalog/experiments/alternatives/tracer/results/` are replaced with the new `summary.json`, `missing-from-table.tsv`, `outside-bound-vocabulary.tsv`, `all-observed.tsv` and `replay/` outputs. None contains a machine path.

**Still open:** held-state keys (`appenditerator::rewind by:append:0`, `multipleiterator::key by:attachiterator:0`, `spldoublylinkedlist::serialize by:push:0`, `splobjectstorage::serialize by:attach:0`): the tracer has no mechanism for an object held by a builtin, so these remain `not-a-row`. The classifier's `?` place keys are unchanged.

**SVF:** a `dvf` binary now exists, built locally from the locked SVF commit (`e889a513…`) at `~/.cache/php-catalog-digest/svf-build/bin/dvf`, sha `a0001c8c…`. It does not match `svf_binary_sha256` (`80bcf3c4…`), and the local `wpa` (`9a660a8d…`) does not match `cbab6150…`. The earlier statement "`dvf` is not installed" is superseded. The regenerated `strlen-native-demand-svf.json` (from the local `dvf`) is not copied into the repo: it would record evidence from a binary the lock does not pin. Whether to repin or to reproduce the locked binary is an owner decision.

**Operational note:** the `ext/curl` tests start an `openssl s_server` that outlives its test. Run under `flock -c`, that process inherits the lock's file descriptor and keeps the heavy lock held after the run ends (here it blocked the next job for over an hour, until the orphan was killed). After a run-tests job, check for leftover `s_server` processes and the lock with `lslocks`.

### SVF pinned to the 3.3 release, 2026-10-24

Owner decision: the SVF pin is the prebuilt `SVF-3.3-ubuntu-24.04-x86_64.zip` from the GitHub release `SVF-3.3`, and the lock pins that release instead of the locally compiled master commit. Nothing is committed.

**What was pinned, and why the previous pin was replaced:**
- The previous pin, `e889a513…`, is an unreleased master commit 108 commits after the `SVF-3.3` tag. The binaries built from it (`dvf` `80bcf3c4…`, `wpa` `cbab6150…`) were never present on this machine; the local build from the same commit gave different hashes (`a0001c8c…`, `9a660a8d…`).
- The release tag `5c45081f…` is the commit the release is pinned to. Its `CMakeLists.txt` still declares `VERSION 3.4`, and the release libraries are named `libSvfCore.so.3.4`, so the "3.3" label is the release name, not the source version. The lock records both.
- Release digests: archive `1515c0c9…`; `bin/dvf` `4ca2135c…`; `bin/wpa` `5e965b06…`; `lib/extapi.bc` `5469f625…`. `dvf` and `wpa` link `libz3.so` from the system (4.13.3) and find `extapi.bc` through `$ORIGIN/../lib`; the release binaries embed `/home/runner/work/SVF/SVF/...` as source-location strings only.
- The relocatable patch (`svf-relocatable-extapi.patch`) is not applied to the release build. The lock records its old digest in `superseded_pins`; the patch file stays, because the local path-free build script still applies it.
- Superseded in `lock.json`: `svf_commit`, `svf_version`, `svf_binary_sha256`, `svf_relocatable_patch_sha256` (current key removed), and the old extapi digest (as `svf_extapi_sha256_master_build`).

**Code changed:**
- `restore_environment.py`: the SVF step now downloads the release archive, checks its digest, and extracts it into `svf-release/` (new `svf_release()`); it no longer rebuilds SVF from source. Tested on a scratch root: archive digest, all three pinned files, and a verify-only re-run pass. The full restore was not run.
- `evaluate_strlen_svf_demand.py`: `build_identity` records `release_asset_sha256` from the lock instead of hashing the relocatable patch, which the release does not use.

**Demand artefact:** `strlen-native-demand-svf.json` regenerated with the release `dvf` (`--svf-bin` takes the `bin` directory). It replaces the top-level artefact and the `strlen-native-demand-svf/` measurement folder (the new run also writes the `svf-dda*.cgroup.json` records, which are now untracked and need adding with the change).
- Query-level results and the finding are identical to the previously committed artefact. The capsule bitcode is byte-identical; only the paths in `extract_command` differ.
- The linked-module digest in the artefact is now `37de322f…` (the rebuilt module); the committed artefact previously carried `3c51a531…`, so the earlier artefact was already stale against the module digest.

**Still stale:**
- The whole-program campaign (`campaign_checkpoint_sha256`, the `campaign-*.json` and `finish-*.json` checkpoints, and the cgroup OOM outcomes that `resource_acceptance` cites) was measured with the path-free `e889a513` build. Those numbers describe a binary that is no longer pinned and have not been re-measured with the release. The campaign needs a rerun before any of its claims cite the 3.3 pin.
- `svf-dda` peaks on the release are about 527–560 MiB per query in the demand run; the `resource_acceptance` figure (10 GiB cap) is not re-verified.
- `restore_environment.py` has not been run end to end against the release.

### SVF-dependent remeasurement on the 3.3 pin, 2026-10-24

Partial remeasurement: the campaign's steps that run `dvf`, on the 3.3 pin. The rest of the campaign (CBMC, SMT, layout, binary, GCC and CIR stages) was not re-run; nothing in the lock's campaign checkpoints was replaced. Nothing is committed.

**Build of the compact SVF from the pinned tag:**
- `campaign.compact_build` could not run on the 3.3 tree as it stood: `svf-relocatable-extapi.patch` fails at `svf-llvm/CMakeLists.txt:144`, because 3.3 generates `extapi.bc` in a different command (`-fPIC`, `gen_extapi_ir`).
- New `svf-relocatable-extapi-3.3.patch` (sha `9b50a146…`): the same two edits as the master patch, ported to the 3.3 lines (`lib/extapi.c` relative to its directory; the build-directory candidate removed from `ExtAPI.cpp`). Verified to apply after `svf-compact.patch` on the clean 3.3 tree.
- `build_svf_pathfree.sh` takes the relocatable patch as an optional fourth argument (default: the master patch); `campaign.compact_build` passes the 3.3 port and records its digest.
- Compact `dvf` (sha `39aafb30…`), built from `5c45081f…` with `svf-compact.patch` and the 3.3 port, 6 min 43 s.
- Inputs: `root/SVF` a local clone at the tag; pinned z3 and bison packages extracted with `restore_environment.check_packages` (`libz3.so.4` sha `eff8f0f9…`, as pinned); promoted module `opt-21 -passes=mem2reg` of `link/php-cli.bc` (37de322f…).

**Results (cgroup `MemoryMax=10737418240`, `MemorySwapMax=0`, `OOMPolicy=continue`):**

| step | binary | exit | result |
|---|---|---|---|
| `dispatch`, `external`, `layout_cases` (stock and compact), `growth-*` (compact) | release / compact | 0 | all pass; results identical to each other once timing and memory lines are removed |
| `whole-compact-dda` | compact | 247 | kernel OOM kill, peak 10737418240 (the cap), 108 s; the campaign gate holds |
| `whole-svf-dda` | release (stock) | 250 | LLVM assertion `hasUseList()` in `Value.h:399` aborts `dvf` after 14 s, peak 790 MiB; not a resource outcome |

- The release binary has LLVM assertions enabled (the assertion path is from the release's `/home/runner/work/SVF/SVF/llvm-21.1.0.obj` headers). The compact build uses the distro LLVM 21.1.8, which has them off. So the release aborts on the whole-program module where the compact build reaches the cap.
- The previously recorded stock whole run (`e889a513…`, path-free, 101 s, exit 143) is not comparable: it was a different binary, and its exit was not the assertion.

**Not re-run and still stale:**
- The stock whole-program baseline on 3.3. The release cannot provide it (assertion abort); a stock build without the compact patch (no release assertions) would be a local build, not the pin.
- CBMC, SMT, layout, binary, and the GCC and CIR stages. `cbmc` is not installed on this machine and is not pinned, so the CBMC steps cannot run as recorded.
- `resource_acceptance` in `lock.json` still cites the `e889a513…` numbers; `results/evaluation/lock.json` still records `svf_source_commit: e889a513…`.
- The raw output is in `~/.cache/php-catalog-remeasure/campaign-svf33/`; nothing from it is copied into the repo.

**Stock baseline on 3.3 (local build), 2026-10-24:** the release `dvf` aborts on the whole-program module because its LLVM assertions are enabled (above). To measure the stock 3.3 source without that, a local stock build was made from the same tag with only the relocatable 3.3 port (`build_svf_pathfree.sh … svf-relocatable-extapi-3.3.patch`, no compact patch): `dvf` sha `f68c6dd9…`, 6 min 32 s. Under the same cgroup cap, `whole-svf-dda` exits 247 with a kernel OOM kill at the 10 GiB cap after 120 s, the same outcome as the compact build. This build is not the pin; it is a local measurement of the source. Raw output: `~/.cache/php-catalog-remeasure/campaign-svf33-stock/`.

### CBMC steps rerun with the pinned cbmc 6.6.0, 2026-10-24

`cbmc` 6.6.0 (Ubuntu package `6.6.0-4ubuntu2`) was installed with apt. Its binary sha256 `bf215bba…` matches `followup_cbmc_validation.cbmc_binary_sha256` in `results/evaluation/lock.json`, so the binary is the reviewed one.

`campaign.run_cbmc_campaign` ran the three CBMC steps under the enforcing runner (`MemoryMax` 10 GiB, `aggregate_hard_limit_enforced` true for each): `cbmc-contracts` exit 0, `cbmc-invalid-composition` exit 10, `cbmc-low-budget` exit 10. The campaign's own validation passed: the binary digest, the source digests, the reviewed reference transcripts, the property counts and the expected exit codes. Output is in `~/.cache/php-catalog-remeasure/campaign-cbmc/`.

The transcripts match the committed references once the checkout path is normalized. The reviewed runs were recorded from `/home/torres/projects/phpstan-rs-master3`, so their `file` and `workingDirectory` fields name that checkout; the rerun names `/home/torres/projects/php-catalog`. This is why the stdout digests recorded in `campaign-measurements.json` (`c201f70e…`, `2c8ef8d2…`, `ba01c9ce…`) differ from the new output's digests (`17ef3711…`, `11325bfc…`, `183d9979…`). The validation compares structure, not bytes, and the committed campaign records are unchanged.

### Complementary campaign steps rerun, 2026-10-24

`campaign.complementary` ran in full under the enforcing runner (every step `aggregate_hard_limit_enforced` true): the CBMC steps (above), `smt`, `layout-build-clang-21`, `layout-runtime-clang-21`, `layout-build-gcc`, `layout-runtime-gcc`, and `binary`. All exited as the campaign expects. Output is in `~/.cache/php-catalog-remeasure/campaign-complementary/`.

**Environments:** the venvs are at `~/.cache/php-catalog-remeasure/cache/smt-python` (`z3-solver==5.1.0.0`, from the lock's `smt_packages`; `z3` 5.1.0 reports) and `cache/python` (the 45 `binary_analysis_packages` in the lock; `angr` 9.3.4 loads, with the unicorn engine disabled by its own warning). The measured PHP CLI is the 2026-10-24 build, `23250df2…`, which matches `php_cli_sha256`.

**Comparison with the committed results:** `smt-results.json` and `binary-results.json` are identical to the committed copies once paths are normalized. The only other difference is the traceback text of the SMT file's deliberately invalid case, which records Python 3.14 here against 3.12 in the committed run.

**Still not done:** the campaign's `main()` writes one checkpoint (`campaign-completed.json`, `measurements.json`). It needs an admission report from an enforcing worker for the promoted module, and this remeasurement did not produce one. The steps were run one stage at a time instead, so the lock's `campaign_checkpoint_sha256` is unchanged.

### Corpus check and resource acceptance, 2026-10-24

**Corpus (`piwigo17-frozen`):** the `gate`-profile checker built from this tree (`cargo build --profile gate -p phpstan-rs --bin phpstan-rs`, 2 min 13 s, into `~/.cache/php-catalog-continue/target-decisions`) ran `check-project --config phpstan-rs.toml --format json` as prime, cold and warm passes, as the baseline job did. Results: 2792 files, 614 diagnostics, exit 0 in all three. The cold output is not byte-identical to the baseline `decisions-run2/baseline/corpus-cold.json`, but the two are identical after both are sorted with `LC_ALL=C` (the same 614 messages in a different order). The warm output is byte-identical to the cold one. Output is in `~/.cache/php-catalog-corpus-check/`.

**Lock:** `results/lock.json` `resource_acceptance` now describes the SVF-3.3 pin's measurements (whole-compact-dda and the local stock whole run reach the cap; the release `dvf` aborts on an LLVM assertion). The previous text is kept under `superseded_pins["resource_acceptance_path_free_master"]`.

### Builtin-runs sidecar: group regeneration, 2026-10-24 (not adopted)

The table was regenerated by the 49 groups of `regenerate.php` (`--skip=released`, sandbox `bwrap`), run one group per call with the driver's own command (`~/.cache/php-catalog-remeasure/sidecar/run-group.php`). The result is `~/.cache/php-catalog-remeasure/sidecar/builtin-runs-8.5.tsv` (9623 rows; sha `abbc13cd…`) and its sidecar. It is not written into the crate's data: it does not match the committed table.

**Against the committed table** (10725 rows, sha `dff5a4e3…`, recorded 2026-10-11):
- 1636 committed rows are missing here: 1552 `wrapper` rows (stream-wrapper methods such as `dir_readdir` and `directoryiterator` methods), 52 `probed`, 32 `method`.
- 534 rows are new here: 229 `method`, 181 `calls`, 69 `wrapper`, 54 `probed`, 1 `releases`.

**Why they differ, as far as known:**
- The committed table is one run over every unit (11234 units, 801562 forks) with `--verify`, which keeps a row either of two runs found. The sidecar's `runs` lists exactly that single run. The group regeneration runs without `--verify`, so a row that a single run misses (a timeout or a death) is lost.
- The recorder changed since the committed sidecar: its hash is `c1d438e9…` now against `47b6d59f…` recorded. The checked-in table therefore cannot be reproduced by the current recorder without a regeneration.
- The README's check is that a group regeneration equals a single worker that loads every module. That single-worker run, with `--verify`, has not been done.

**Decision needed:** adopt a regenerated table (after the single-worker `--verify` run, which takes about an hour and is in the background if it exceeds the foreground limit), or keep the committed table and record its recorder hash as stale. Replacing the table changes what the checker treats as known: the 1552 wrapper rows lost here would make those builtins unknown.

### Builtin-runs single-worker `--verify` run, 2026-10-24

One process over every unit (12906 units, 1140297 forks, 30 min 28 s, `--verify --skip=released`, sandbox `bwrap`): `~/.cache/php-catalog-remeasure/sidecar/single/`. Its table has the same sha as the group regeneration (`abbc13cd…`, 9623 rows) and `differing 0`. The README's check holds: the group regeneration equals a single worker.

**Against the committed table** (10725 rows, sha `dff5a4e3…`):
- The extension set and versions are identical (67 extensions; imagick 3.8.0, `info` `086da477…` in both).
- The committed table predates 77 recorder commits since 2026-10-10 (`git log -- tools/builtin-runs`), so the difference is expected and the committed table is stale against the current recorder.
- 1539 of the 1552 lost `wrapper` rows are `imagick` `stream_read` and `stream_eof`. In this run 10672 children died, 9472 of them imagick children, almost all in `compareimagelayers`, `identifyimage` and `readimages` (about 3100 receivers each), and 64 timed out.
- Likely cause, not verified: `9c4342006` (2026-10-12, "the Imagick receiver is sixteen pixels") changed the receiver after the committed table was recorded. The new receiver seems to make these methods crash the child, which loses the stream rows. That would be a recorder regression, not a change in what Imagick does.

**Not adopted.** Adopting the regenerated table would remove those 1539 imagick stream rows from the checker's knowledge. Options: (a) adopt it as is; (b) fix the imagick crash in the recorder first and regenerate; (c) keep the committed table and record its recorder hash as stale.

### Builtin-runs Imagick child deaths: analysis, 2026-10-24

The 9472 imagick children that died in the single-worker run (sidecar `single/`) come from three causes, and two are fixed in `tools/builtin-runs`.

**Fixed (in the working tree):**
- `compareImageLayers`: its only parameter is `int $metric` (the stub's name for the method). The probe gave 0, which ImageMagick's `CompareImagesLayers` asserts against (`layer.c:793`), and the child aborts. `imagick_value` now gives `LAYERMETHOD_COMPAREANY` for that function. Deaths for the method: 1566 → 0. The first version of this rule matched `method`, and `getName()` keeps the declared case, so the rule had to lowercase the name.
- `identifyImage` (and the same held pattern for other methods): under bwrap the host's `/tmp` is read-only, and the worker wrote nothing there. `worker_command` now gives the worker a private `/tmp` tmpfs with the home directory's cap. Deaths for `identifyImage`: 1537 → 0.

**Not fixed: `imagick::readimages`, 1568 segfaults (status 11) per run.**
- Outside the sandbox, with `LC_ALL=C`, the same run dies 1568 times. With `LC_ALL=C.UTF-8` it has 0 deaths. So the locale is a trigger outside bwrap.
- Under bwrap the locale does not help: `C.UTF-8` still gives 1578 deaths. The bwrap-only trigger is unidentified. The locale change was reverted, since it does not fix the default path.
- In the debug copy every death follows a held `similarityImage` or `subImageMatch` at argument 1 or 2, whose probe is an array (`RecordingObject`s). The segfault is not an assertion, so no message is printed.
- Ruled out by direct micro-cases, each with and without the sandbox: the held `subImageMatch` with an array offset, then `readImages` with the probe's `RecordingObject` array; the recorder's address-space cap (`Limits::apply`); a fork after ImageMagick has run in the parent; the `rec://` URL.
- Not usable: a no-sandbox run with the worker's cleared environment fails to start a worker. Core dumps are not written (the core pattern pipes to apport, and the child's core limit is 0). `gdb` followed the wrong fork.

**Next step for `readimages`:** get the crashing frame from a child inside bwrap, with core collection enabled in a debug copy and one run of `readimages`, or exclude `imagick::readimages` with a recorded reason. Neither is done.

### Builtin-runs `imagick::readimages` segfault: cause and fix, 2026-10-24

The crashing frame came from a debug copy of the recorder (`~/.cache/php-catalog-imagick/dbg`, the recorder with a per-child log and a `SIGSEGV` backtrace handler preloaded). Every readimages death is `zim_Imagick_readImages` reading address `0x11`, called from the plain child and from children after a held method on the same object.

- The probe for `readImages(array $filenames)` gives `[1, 2]` (`plain_value`). ImageMagick's PHP extension reads each element as a string, so the integer `1` is taken as a `zend_string *`; `ZSTR_LEN` at `0x1 + 0x10` is the address that faults.
- Micro-case `php -r '(new Imagick)->readImages([1, 2]);'` exits 139 under both `LC_ALL=C` and `LC_ALL=C.UTF-8`. Element kinds that do not crash: an object (`ImagickException`), a missing filename (`ImagickException`), and `'rec://probe'` (a warning).
- The locale explanation in the section above is superseded: the crash does not depend on the locale. Why the earlier `C.UTF-8` runs had no readimages deaths is not explained.
- Fix (working tree, `probes.php` `imagick_value`): `readImages`'s filenames parameter gives `['missing.png']`. Under bwrap, a readimages-only run gives 868 forks, 0 deaths, one timeout, and 22 rows identical to the committed table's readimages rows.

**Other Imagick deaths, same recorder, bwrap class run (earlier sandbox fixes in place):** `getimageartifacts` 34 died, `liquidrescaleimage` 6, `getimagechannelrange` 2, `getoptions` 2, and 4 `previewimages` timeouts. A debug-copy run of these five builtins in `none` mode attributes them to ImageMagick's own memory errors on objects in invalid states, not to the probes' callbacks:
- 32 children abort with `double free` / `double free or corruption`, and 2 with `SetPixelChannelMask: Assertion 'image != NULL'` (a null image reached by `getImageArtifacts`/`getImageChannelRange`).
- 13 `liquidrescaleimage` children segfault inside `liblqr-1.so.0`.
- The 638 `previewimages` timeouts in that `none` run do not reproduce: the same builtin under bwrap has 866 `ok` children and 0 timeouts, so they were a `none`-mode artifact.

These five builtins still lose rows when their units die twice, so a regeneration would exit 1 until they are excluded with a reason or the probe state that reaches them is changed. Not decided here.

### Builtin-runs remaining Imagick deaths: reachability from valid PHP, 2026-10-24

Each crashing state was run as plain PHP, one process per case, with core dumps off (`~/.cache/php-catalog-imagick/micro/`):

| valid PHP | result |
|---|---|
| `(new Imagick)->getImageArtifacts("x")` | segfault (139) |
| `(new Imagick)->getOptions("x")` | segfault (139) |
| `(new Imagick)->getImageArtifacts()` | segfault (139) |
| `(new Imagick)->getImageChannelRange(1)` | `SetPixelChannelMask: Assertion 'image != NULL' failed` (134) |
| `setImageArtifact("x","x")`, then `getImageArtifacts("x")` on a 1x1 image | `double free detected in tcache 2` (134) |
| `liquidRescaleImage(4, 4, 1.0, 0.0)` on a 1x1 image | segfault (139) |
| `liquidRescaleImage(2, 1, 1.0, 0.0)` on a 1x1 image | no crash |
| `liquidRescaleImage(4, 4, 1.0, 0.0)` on a 16x16 image | no crash |

Every crash is reached with arguments PHP accepts and an object made by PHP's own constructor or `newImage`. None needs an invalid probe, so the probes are not at fault: these are ImageMagick and liblqr bugs that the table cannot record as rows. The `extentImage` then `getImageArtifacts` abort recorded by the class run did not reproduce in isolation; its units are the same builtin and fall under the exclusion above.

**Correction to the reachability table above.** The empty-object and enlarge rows were labelled "valid PHP". They are not:
- An empty `new Imagick()` is invalid by the documented contract: `getImageWidth()` throws `ImagickException: Can not process empty Imagick object`. The `new:0` children of `getimageartifacts`, `getoptions` and `getimagechannelrange` get that state from the constructor probe, so the crash is a probe choice.
- `liquidRescaleImage(4, 4, …)` on a 1×1 image asks for an enlargement the operation does not define. The probe's plain `width = 4` produces it, so this is also a probe choice. `(1, 2)` and `(4, 1)` on the same image do not crash.
- `setImageArtifact("x", "x")` then `getImageArtifacts("x")` on a valid 16×16 image still aborts with `double free detected in tcache 2`. Valid image, valid arguments: this one is a genuine ImageMagick bug, not a probe choice. The class run's `by:setimageartifact:0` children are this sequence.
- Not yet classified: the class run's `getimageartifacts` children after `extentimage`, `deskewimage` and `houghlineimage` on the 1×1 maker. Their states are not checked.

### Re-checked on real images, 2026-10-25

The earlier reachability checks used 1×1 and 16×16 images made by `newImage`. The open cases were re-run on real files (`/usr/share/pixmaps/faces/tree.jpg`, a 512×512 TrueColor photo; `/usr/share/pixmaps/htop.png`, a 128×128 PaletteMatte image):

- `getImageArtifacts("*")` after `extentImage(640, 640, 0, 0)` on the photo: double free (134). After `setImageArtifact("x", "x")` on the photo: double free (134). Without a prior call: no crash. Confirmed: a genuine ImageMagick bug on a valid image and valid arguments.
- `ImagickDraw::setResolution(72, 72)` then `render()` with no image: assertion `image != NULL` (134). `setResolution` then `drawImage` on the photo: no crash. Confirmed: genuine, only on the image-less `render()`.
- `liquidRescaleImage(600, 600, 0.0, 0.0)` on the photo (enlarging): no crash. `(256, 256, …)` reducing: no crash. The earlier enlargement crash was specific to the 1×1 maker image. The probe rule that gives `width` and `height` 1 stays as a value choice matched to that maker, not as a claim that enlargement is invalid.
- `cycleColormapImage(4)`, `(0)` and `(1)` on the palette PNG: no crash; after `pingImageFile` on the same file, `cycleColormapImage(4)`: no crash. The recorder's SIGFPE in `CycleColormapImage` (after `pingImageFile` on the recorder's stream) is not reproduced by a real palette image, so it stays a `lost` entry until its state is found.
