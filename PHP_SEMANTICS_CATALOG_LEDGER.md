# PHP catalog Phase F and remediation ledger

This is the authoritative status record for Phase F tasks and their remediation actions. Task evidence and evaluation methods are documented in [the evaluation report](PHP_SEMANTICS_CATALOG_EVALUATION.md). The semantic contract review is in [the contract review](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md).

## Phase F task and remediation ledger

Statuses: **complete** means the stated evidence exists and passed its recorded
check; **partial** means evidence covers only the listed narrow slice; **open**
means the required evidence has not been produced or independently reviewed;
**deferred** means the work belongs to a later plan phase.

### F00.1 — Partial: Specify observable traces, assumptions, abstract order, and return-refinement rules

**Reviewed evidence and current disposition:** Contract defines observable projections, trace order, host obligations, and return/exception distinctions. Review: `PHP_SEMANTICS_CATALOG_CONTRACT.md` now specifies observable projections, ordering, abstract order, assumption obligations, and return/exception refinement. `Evaluation` still returns projected exits/states/results and a replay certificate rather than an explicit trace set. Host implementations must preserve whatever ordered observations a closed projection needs. An independent adversarial review is recorded; its findings remain unresolved, so this is a working draft, not accepted semantics. See [F00 review](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md).

- **R00.2 — Complete (definition only; implementation conformance remains open)** (depends on: R00.1)
  The contract defines host concretization (`gammaV`/`gammaS`) and conditional simulation obligations for entry tuples, event transfers, predicates, joins/widening, exits, and projections; narrows the evaluator claim to a host-relative post-fixpoint; and disclaims a PHP-soundness theorem and full trace-prefix coverage. It defines absent result IDs as undefined and rejects joins where a result exists on only one path. Evidence is provenance, not trust; trusted validators and cross-namespace support need pinned roots and validation. Tests cover initial-state retention through transfer and exit, joins/widening, dependency projections, and completion kinds. `widening_preserves_the_transferred_lower_bound_and_reflects_precision` checks that the transferred lower bound survives widening; it explicitly does not claim upper-bound preservation. Claude's independent re-review confirmed the semantic argument and allowed R00.2 to close as a definition after this scope correction. Cleanup's fatal, suspension, and native-unwind paths still violate the normative rule (RV-1); they remain open under R01.5 and are outside this subtask's implementation-conformance claim.

### F00.2 — Partial: Reject invalid trace compositions with counterexamples

**Status note (independent review, 2026-10-09):** downgraded from Complete. The fatal and native-unwind counterexample row has no cleanup-path evidence, and the evaluator accepts those compositions (RV-1 in the [review record](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md#independent-review-record-2026-10-09)).

**Reviewed evidence and current disposition:** Locked CBMC replay passes 93 valid-contract properties; invalid composition yields the expected `main.assertion.11` counterexample among 94. Review: The locked CBMC replay records 93 valid-contract properties and the expected invalid-composition counterexample among 94. This checks the listed algebraic counterexamples; it does not prove arbitrary host transfers or every concrete PHP trace. Keep counterexamples synchronized with any contract changes.

- **R00.4 — Complete (reduced C-model accounting and fail-closed CBMC runs)**
  The [evaluation report's R00.4 accounting](PHP_SEMANTICS_CATALOG_EVALUATION.md#r004-cbmc-property-accounting-and-enforcement) maps all 93 valid-run properties, all 94 invalid-run properties, and both low-budget properties by exact ID, category, and contract relation; it marks every result C-model-only and classifies `main.assertion.11` and `main.unwind.0` as expected negative controls. `campaign.py` verifies the pinned CBMC binary, C source and reference digests, complete normalized property records (identity, status, description, source location, and trace), summary, and command exit; ten Python regressions pass. All three locked CBMC 6.6.0 commands passed through `EnforcedRunner` and yielded the expected 93/94/2 properties and exit codes 0/10/10 under resource enforcement. This closes only the accounting/enforcement task; it does not prove PHP or Rust evaluator semantics.

### F00.3 — Partial: Validate host transfer functions against PHP source behavior

**Reviewed evidence and current disposition:** Narrow weak `strlen` parser/cast source checks, runtime callback sentinel, strict/weak and Stringable fixtures exist. Review: The `strlen` builder checks selected `zend_API.c` parser gates, the `__toString` dispatch source, LLVM data-flow, and positive runtime sentinels in locked profiles. Strict/weak fixtures cover the tested behavior. The evidence does not validate every host transfer, callback mutation, thrown outcome, release/destructor effect, or unsupported PHP behavior. General source-to-host audit and callback effect/throw modeling remain open; see [F00 review](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md).

- **R00.3 — Complete (matrix/audit only; semantic gaps remain open)** (depends on: R00.1, R00.2)
  The [evaluation report's R00.3 matrix](PHP_SEMANTICS_CATALOG_EVALUATION.md#r003-source-to-host-matrix-php-854-strlen) covers weak/strict parsing, casts, Stringable dispatch, callback mutation/throw, release, unknown behavior, and the direct `ZEND_STRLEN` opcode path as well as `zif_strlen`. Each row distinguishes the PHP source fact, the actual host/artifact transfer, assumptions, observed positive/negative/contradictory cases, affected state/result/exit components, and what remains open. It closes the audit deliverable, not semantic conformance: RV-16 remains open because artifact evidence binds `zif_strlen` but not the usual compiler-emitted opcode; callback effects/exits and release remain open under R05.2/R02.3; unsupported host behavior remains open under R04.1. R01.6 now validates the reviewed graph dependency rules and requires declared unknown-event domains to stay open.

### F00.4 — Partial: Specify distinct, validated assumption identities

**Status note (independent review, 2026-10-09):** downgraded from Complete. The contract specifies typed assumption records. The code has no assumption records and no applicability check, and it accepts non-empty assumption vectors (RV-4). The title's "validated" is not met.

**Reviewed evidence and current disposition:** The contract defines typed records, canonical IDs, distinct evidence references, acyclic dependencies, and context-dependent validation. Schema v8 now implements typed records, a separate assumption namespace, canonical IDs, target/program context matching, manifest round trips, and combined evidence/assumption cycle rejection (R01.2). Assumptions are not treated as established until trusted validator execution is implemented (R01.7); closed coverage citing an assumption therefore fails closed. See [F00 review](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md).


### F00.5 — Open: Independently review the contract and assumptions

**Reviewed evidence and current disposition:** An independent review is recorded in the [review record](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md#independent-review-record-2026-10-09); its findings remain unresolved or unaccepted, so this item stays Open. The review occurred after F01 and later checker/native work had begun, so those results remain provisional under the ledger's gate order until the findings receive dispositions.

- **R00.1 — Complete** (depends on: none)
  Review obtained and recorded 2026-10-09 by an independent reviewer: a Claude session. The maintainer reports that the ledger was being worked by Codex, so the reviewer is independent of that work. See the [review record](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md#independent-review-record-2026-10-09). Its reviewer role, findings and dispositions are recorded there. Findings still open pending disposition/acceptance: RV-1, RV-2, RV-4 to RV-13, RV-16 to RV-19, and contract points C-1 to C-6. RV-2's implementation remediation is recorded complete under R01.4 but remains awaiting independent acceptance. RV-3 is decided as defensible; RV-14 and RV-15 are fixed. Resolving or accepting these findings is tracked under F00 closure, not this row.
### F01.1 — Partial: Implement versioned graph, evidence, assumption, and domain-coverage schema

**Reviewed evidence and current disposition:** Canonical digests, explicit coverage/dependencies, evidence and assumption validation, JSON round trips, combined evidence/assumption cycle rejection (R01.2), catalog-entry release-range validation (R01.3), dependency semantics for `NotApplicable` (R01.4), and graph-derived mandatory dependencies for the reviewed transfer slice (R01.6) have implementation and tests. Remaining gaps are independent: trusted assumption validation and canonical-order enforcement (R01.7–R01.8). Review of the producing commit found that `EvidenceRecord::id` hashes raw dependency and assumption vector order while semantically unordered permutations are accepted; `CatalogManifest::canonical_digest` also hashes producer order without validating it. These order issues remain R01.8.

- **R01.1 — Complete (typed assumption records and fail-closed references)** (depends on: R00.1)
  Schema v8 adds separate typed assumption records with schema-bound content IDs, claim kind/text, target and optional loaded-program scope, prerequisite and supporting-evidence references, and validator identity/version. Manifest and catalog admission validate record shape, target-scope ranges/duplicates, evidence and assumption namespaces, required references, assumption propagation from evidence into coverage, and JSON round trips. Target/program matching is exposed but does not establish an assumption; graph validation rejects closed coverage that cites one, including graphs evaluated without the catalog loader. Combined support-cycle checks are covered by R01.2; trusted validator execution remains R01.7 and unordered-vector identity remains R01.8.
- **R01.2 — Complete** (depends on: R01.1)
  Catalog admission checks one combined evidence/assumption support graph, covering assumption prerequisites, assumption-to-evidence support, evidence-to-assumption conditions, and evidence dependencies. Cycles return the closed path with each node's namespace and digest. Focused tests cover self-cycles, multi-node assumption cycles, cross-namespace cycles, and a valid DAG; a manifest-boundary test verifies malformed cyclic input receives the cycle diagnostic before its unavoidable content-ID mismatch. Valid content-addressed cycles cannot be constructed without a hash fixed point, but untrusted manifest structure is checked directly. Validator trust, version pinning, and revocation/invalidation remain R01.7.
  Validation: all 3 focused cycle tests pass, including manifest-boundary rejection. The full semantics suite passes (70 passed, 6 existing ignored), `cargo check --workspace`, formatting, and diff checks pass.
- **R01.3 — Complete** (depends on: None)
  `TargetPredicate::validate` now runs on catalog variant insertion and manifest validation/loading. Inverted ranges return `InvalidTargetPredicate(InvalidVersionRange)`; valid unbounded, exact, minor, and numeric boundary ranges remain accepted. Tests cover both insertion and manifest-load rejection, plus valid exact/minor and `u16` boundary values.
  Validation: focused release-range tests pass; the full semantics suite, workspace check, formatting, and diff checks pass.
- **R01.4 — Complete** (depends on: R00.1)
  `NotApplicable` contributes no fact and may not appear anywhere in a `Closed` domain's transitive dependency closure. Every dependency edge is cycle-checked regardless of coverage. `Closed` claims are rejected when any transitive dependency is `Open` (including through an N/A node) or `NotApplicable`. During composition, open reasons, evidence, assumptions, and precision propagate across all dependency edges; N/A intermediates remain N/A while dependent claims widen. Tests cover all 3×3 direct `Closed`/`Open`/`NotApplicable` pairings, transitive closed-to-N/A rejection, the reported Return → Diagnostic → UserWrite path, propagation through that path during composition, and cycles through Open/N/A nodes.
  Validation: `cargo test -p phpstan-rs-semantics` passes (79 passed, 4 existing ignored); `cargo check --workspace`, `cargo fmt --check -p phpstan-rs-semantics`, and `git diff --check` pass.
- **R01.6 — Implementation complete; independent acceptance pending** (depends on: R00.2, R00.3)
  Graph validation derives required dependencies from reachable transfers and projection operands: normal `Return` requires `UserRead`/`UserWrite` for reachable read/write and PHP-flow state operations, including supported PHP-throw cleanup paths that return normally; output consuming an invocation result or an explicit result guard requires `Invocation`; closed PHP-throw/fatal/suspension projections in graphs with `Invoke` require `Invocation`. The same checks rerun after event coverage composition, so a child cannot make a previously `NotApplicable` output projection closed without the required edge. Unknown transfers still require each listed affected domain to be open. Mutation tests remove required edges and observe `MissingRequiredDomainDependency`; an unrelated invocation does not contaminate a value-only return. The locked `strlen` builder records why its `StringByteLength(Argument(0))` return has no dependencies: the callback result is unused, there are no explicit state reads/writes, and the nonnegative result envelope is invariant under open callback effects. The builder regenerates the checked-in catalog byte-for-byte with pinned source, LLVM inventory, and runtime witness. RV-3's implementation gap is addressed; obtain independent review before treating the finding as accepted.
  Validation: `cargo test -p phpstan-rs-semantics` passes (96 passed, 1 pre-existing ignored); `cargo check --workspace`, `cargo fmt --check -p phpstan-rs-semantics`, and `git diff --check` pass. The locked builder was rerun with the pinned PHP source, LLVM bitcode inventory, and runtime witness; `cmp` confirms byte-for-byte identity with `data/php-8.5.4-strlen.json`.
- **R01.7 — Open** (depends on: R00.2, R01.1)
  Define trusted validator roots, version pinning, revocation/invalidation, and test untrusted/stale validators. Cross-namespace support cycles are now rejected under R01.2.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

- **R01.8 — Open** (depends on: None)
  Canonicalize or reject noncanonical order for semantically unordered evidence, assumption, coverage, and manifest vectors. Acceptance: permuting an unordered list produces the same canonical ID/digest, or validation rejects it with a clear error; ordered graph blocks/events remain order-sensitive.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F01.2 — Partial: Implement evaluator control flow and exceptional continuations

**Status note (independent review, 2026-10-09; remediation recorded below):** downgraded from Complete because cleanup `Return` or `Throw` replaced pending fatal, suspension, or native unwind (RV-1). R01.5 now rejects unsupported abrupt replacement cells and tests the completion matrix. F01.2 remains Partial until the independent acceptance gate in F00.5 is resolved.

**Reviewed evidence and current disposition:** Worklist, joins/widening, PHP/native exceptional exits, cleanup/resumption, and masked unknown transfers are tested. Review: Worklist evaluation, joins/widening, normal and exceptional continuations, PHP throw versus native unwind, fatal/suspended exits, cleanup/resumption, and masked unknown transfers have targeted fixtures. Tests reject join/widening losses and check loop convergence. Production-domain calibration and semantic-host soundness are separate open obligations. Calibrate domains for production behavior.


### F01.3 — Complete: Validate post-fixpoint certificates and reject incomplete graph behavior

**Reviewed evidence and current disposition:** Graph-bound replay checks transfer candidates, exits, and coverage; partial result definitions and missing/duplicate invocation coverage fail closed. Review: Certificates bind a graph digest and replay transfer candidates, successors, exits, and coverage; tests reject dropped results/pending outcomes, missing successors/exits, graph changes, and falsely closed coverage. The certificate trusts the supplied host's transfer semantics. Conservative extra stored exits are allowed, so the validator establishes a post-fixpoint over that host, not PHP correctness or a native-IR post-fixpoint. None for the implemented validation contract.


### F01.4 — Partial: Validate reference contracts with broader branch/callback cases

**Reviewed evidence and current disposition:** Callback result/write/throw order, state-sensitive predicates, joins, cleanup, and second-callback branch selection are covered. Review: Callback result/write order, result-selected second callback, state-sensitive predicates, branch joins, callback throws, cleanup, and normal/throw state separation have fixtures. These are substantial reference-contract examples, but not an independently reviewed or exhaustive composition matrix; F00.5 remains open. Independent contract review remains F00-dependent; broaden combinations after review.

- **R01.5 — Implementation complete; independent acceptance pending** (depends on: R00.2)
  `GraphError::UnsupportedCleanupReplacement` now makes unsupported cleanup replacement fail closed in both evaluation and certificate replay. The only modeled replacement remains cleanup `Return` over pending PHP throw. A full evaluator matrix covers all four pending kinds against cleanup Return, PHP throw, fatal, and suspension; it asserts the sole supported cell succeeds and every other cell returns the typed error. A certificate replay regression verifies the validator independently rejects a pending-fatal cleanup return. A separate matrix proves `Resume` preserves all four pending kinds. A callback throw also passes through a `Maybe` branch, distinct state writes, a join, and `Resume` without losing the pending throw. Existing six fatal/native-unwind/suspension Return/Throw counterexamples now fail closed. RV-1's implementation defect is addressed; obtain an independent review before treating the finding or parent F01.2 as accepted.
  Validation: `cargo test -p phpstan-rs-semantics` passes (90 passed, 1 pre-existing ignored); `cargo check --workspace`, `cargo fmt --check -p phpstan-rs-semantics`, and `git diff --check` pass. Workspace-wide `cargo fmt --all -- --check` reports pre-existing formatting differences in unrelated crates, so no unrelated formatting was retained.

### F01.5 — Open: Calibrate a production semantic host

**Reviewed evidence and current disposition:** A narrow PHP host and PhpFlow/value fixtures exist. Review: A PHP-specific host and value/reference fixtures exist, including havoc for unknown callbacks and open coverage. It is still a narrow host, not a source-calibrated production semantic domain. In particular, the graph's `Stringable` invocation has open callback effects and exceptions. Replace toy/narrow transfers with calibrated, source-validated domains (depends on F00 and F02).

- **R04.1 — Open** (depends on: R00.3, R02.3)
  Calibrate a production host for the selected closed slice; retain havoc/open coverage elsewhere. Acceptance requires source checks, graph certificates, and positive, negative, contradictory runtime cases to agree.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F02.1 — Partial: Lock PHP 8.5.4 source, compiler, build, and linked artifacts

**Status note (second review, 2026-10-09):** downgraded from Complete. The lock file records source hashes for `run.py` and `runtime.php` that no longer match the tree (RV-19). The compiler identities, source commit and linked-bitcode hash do verify. See the [review record](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md#second-review-pass-unchecked-items-2026-10-09).

**Reviewed evidence and current disposition:** Source commit, compiler identities, build/link manifests, and runtime profiles are recorded. Review: PHP 8.5.4 source commit, compiler/tool versions, build and link manifests, binary identities, and Hybrid/CALL runtime profiles are recorded. This does not cover other platforms, SAPIs, builds, or extensions. None for the locked experimental profile.


### F02.2 — Complete: Inventory the linked `zif_strlen` LLVM instructions and trace the Return demand

**Reviewed evidence and current disposition:** All 241 instructions are accounted for; builder checks opcode counts/call targets and the field-to-return path. Review: The linked `zif_strlen` inventory accounts for 241 instructions (storage/layout/scalar/call/branch/return families), checks unknown opcodes and missing return, and traces the selected string-length field to the return payload. It is accounting plus a narrow demand witness; it does not interpret all instructions. Inventory is accounting evidence, not interpretation of all operations.


### F02.3 — Partial: Compare source, LLVM, and compiler representations

**Reviewed evidence and current disposition:** Source/layout/helper and LLVM witness checks support the narrow Return/Diagnostic slices; Clang/GCC/CIR campaigns are recorded. Review: Source/layout/helper evidence and LLVM extraction support the narrow `strlen` Return/Diagnostic paths. The AST/CFG, LLVM, GCC GIMPLE, and CIR campaigns expose real representation behavior and counterexamples, including Clang CFG exceptional cleanup loss and poor CIR coverage. A systematic AST/CFG-to-LLVM semantic correspondence adapter and broad transfer comparison are absent. Complete systematic AST/CFG-to-LLVM comparison adapters and broader semantic correspondence.

- **R02.2 — Open** (depends on: None)
  Build reproducible source/AST/CFG-to-LLVM correspondence for the selected demand; record where GCC GIMPLE, CIR, and Clang analysis agree or lose behavior. Include cleanup/exception counterexamples and unsupported coverage.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F02.4 — Partial: Establish implicit-operation and normal/CALL behavior

**Reviewed evidence and current disposition:** Paired weak/strict runtime witnesses and 13-sentinel Hybrid/CALL comparison exist; Stringable invocation now has pinned source and runtime evidence, independently replayed against the historical fixture bytes. Review: Weak/strict scalar and Stringable source/runtime witnesses, paired VM comparisons, and 13 runtime sentinels support positive tested paths. I replayed the historical runtime fixture with recorded source hash `9b38a3ea…`; both cached Hybrid/CALL binaries produced the checked-in output byte for byte (`2bd72db6…`). The builder's callback-specific validation does not compare that recorded fixture hash with source bytes; it checks only that the current fixture still contains the sentinel, so future source drift can pass regeneration. Drift has already happened: the checked-in `runtime.php`, changed in `644350cf`, no longer hashes to the witnessed digest (RV-11). Matching sentinels also do not establish equivalence over unobserved inputs; callback writes/throws and other implicit operations are not all connected to modeled graph behavior. Bind callback-specific runtime evidence to its recorded source fixture digest; expand beyond tested `strlen` scenarios and connect all modeled operations to native behavior.

- **R02.1 — Open** (depends on: None)
  Make runtime evidence regeneration verify the callback fixture recorded source digest. Reject a one-byte source change and preserve Hybrid/CALL output hashes. Current instance: the witness records the `runtime.php` digest `9b38a3ea…` (commit `043f2c78`), but the checked-in file, changed in `644350cf`, hashes to `94dc6509…`. The regeneration script checks only for the sentinel string (RV-11).
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F02.5 — Open: Interpret the wider native `strlen` behavior

**Reviewed evidence and current disposition:** Return and null diagnostic slices are represented; the 241-op inventory fails closed on unknown operations. Review: The graph closes only its narrow return and exact null-diagnostic observations. Scalar conversion, complete callback behavior, object release, effects, exceptions, fatal/unwind behavior, and other non-normal outcomes remain open. The opcode inventory intentionally fails closed instead of implying coverage. Model/validate scalar conversion, all callback behavior, object release, effects, and non-normal outcomes.

- **R02.3 — Open** (depends on: R00.3, R02.2)
  Extend strlen modeling to scalar conversion, callbacks, release, effects, exceptions, fatal/native unwind, and non-normal exits. Close only facts supported by source/compiler/runtime evidence and graph tests; contradictions fail closed.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F03.1 — Complete: Measure whole-profile SVF resource behavior

**Reviewed evidence and current disposition:** Stock and compact full-link runs and resource measurements are recorded. Review: Stock SVF and the compact-layout adaptation each hit the enforced 10 GiB cgroup limit on the linked minimal PHP CLI workload. This rules out those configurations as mandatory whole-link prerequisites. Each candidate had one enforcing attempt; neither result evaluates an alternative solver. These measurements reject full-profile SVF as a mandatory prerequisite; they do not establish a replacement solver.


### F03.2 — Partial: Probe demand summaries, Zend layout/dispatch, and fallback cases

**Reviewed evidence and current disposition:** Reduced SVF layout/dispatch probes, copy/initializer regressions, and the empty Stringable-target counterexample are recorded. Review: Reduced probes cover Zend layout/dispatch, absent external writes, copy/initializer cases, and the empty Stringable-target fallback counterexample. The capsule lacks object construction/handler initialization, so an empty target is explicitly incomplete evidence. Broader source-derived demand summaries are not implemented. Generalize beyond the reduced capsule and add demand-oriented source-derived summaries.

- **R03.1 — Open** (depends on: R02.2)
  Expand Zend layout/copy/initializer/dispatch probes through construction and handler initialization. Incomplete target discovery must remain incomplete, never a complete empty set.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

- **R03.4 — Open** (depends on: R02.2)
  Bind the SVF demand replay to the actual locked source and linked-bitcode identities. It currently writes a fixed PHP source commit into results without checking that the supplied bitcode came from that source; it records but does not enforce the callback fixture digest. Acceptance: reject mismatched bitcode/source/fixture inputs and test each mismatch.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F03.3 — Open: Integrate coverage-aware pointer reuse and fallback tracking

**Reviewed evidence and current disposition:** Existing PTA fallback flags and target-report limitations are documented; empty-target fallback regression is preserved. Review: Existing PTA internals track `data` and `anything` fallback flags, and the regression preserves a dangerous empty-target example. The analysis remains private to `php-catalog` and emits text; there is no machine-readable catalog adapter, per-target completeness contract, or production integration. A known target set cannot be treated as exhaustive when fallback is present. No machine-readable adapter, per-target completeness contract, or production integration exists.

- **R03.2 — Open** (depends on: R03.1)
  Add machine-readable pointer-result integration carrying provenance and data/anything fallback flags. Fallback or incomplete results cannot close per-target catalog domains.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F03.4 — Open: Validate native summary post-fixpoints

**Reviewed evidence and current disposition:** Semantic graph certificates validate graph evaluation. Review: The semantic graph certificate replays the graph evaluator, not imported native IR summaries. Native summary validation and missing-write/growth regressions at that boundary do not exist. Add independent validation for native IR summaries and missing-write/growth regressions.

- **R03.3 — Open** (depends on: R02.3, R03.2)
  Independently validate imported native summaries bound to input/summary digests. Mutation tests reject omitted writes, exits, successors, stale summaries, and non-post-fixpoints.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F04.1 — Complete: Implement the minimum PHP value/reference product fixtures

**Reviewed evidence and current disposition:** Copy arrays, nested references, object identity, alias rebinding/unset, and normal/throw callback state are tested. Review: The value store and PhpFlow tests cover array-copy isolation, nested references, object identity, alias rebinding/unset, reference cells, and callback mutation across normal/throw/cleanup paths. These fixtures meet the stated minimum gate; they do not constitute full PHP value semantics. Minimum Phase F fixture requirement is met.


### F04.2 — Deferred: Expand PHP value semantics and syntax-to-flow lowering

**Reviewed evidence and current disposition:** Current value state and PhpFlow operations provide a seed. Review: Broader key coercion, object properties, and syntax-to-flow lowering remain later-plan work. The current ledger correctly keeps this separate from the minimum Phase F fixture result. Broader key coercion, object properties, and syntax-to-flow lowering belong to C22.

- **R22.1 — Deferred** (depends on: Later C22 planning)
  Carry key coercion, object properties, and syntax-to-flow lowering into C22 unless the plan explicitly brings them into Phase F.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F05.1 — Partial: Select catalog behavior across exact and uncertain target scenarios

**Reviewed evidence and current disposition:** Exact/partial target selection and exact/minor patch fixtures exist; uncertain remainders stay uncovered. Review: Exact patch selection, minor ranges, correlated scenarios, and uncovered/Maybe remainders are tested. The checker adapter leaves platform facts unknown; SAPI, build, extension, and configuration applicability are not in the selected profile. Include platform, SAPI, build, extension, and configuration profile facts in applicability.


### F05.2 — Complete: Load and validate the exact 8.5.4 `strlen` artifact in the checker

**Reviewed evidence and current disposition:** Embedded JSON is validated; evaluator certificate is replayed before consuming Return/Diagnostic results. Review: The checker loads the embedded artifact through manifest validation, replays its certificate, and consumes its narrow Return/Diagnostic projection. Locked regeneration is byte-identical. This is not a general artifact/profile pipeline. None for this exact narrow slice.


### F05.3 — Partial: Route argument checks and null diagnostics through shared catalog evaluation

**Reviewed evidence and current disposition:** Return and exact-target null Diagnostic share a memoized evaluation; weak/strict routing fixtures pass. Review: Argument checks, return inference, and the exact-target null deprecation share a memoized graph evaluation; weak/strict and version-boundary fixtures exercise it. Broader source-derived diagnostics and ranges outside exact coverage still use legacy behavior. Generalize source-derived rules; ranges outside exact catalog coverage still use legacy diagnostic behavior.

- **R05.4 — Open** (depends on: R05.2)
  Generalize source-backed argument, return, and diagnostic rules beyond exact strlen. Test boundaries and ensure uncovered/range cases retain legacy or conservative behavior.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F05.4 — Partial: Route implicit `Stringable` invocation/effect from the graph

**Reviewed evidence and current disposition:** Graph contains weak-mode predicate plus `Invoke(__toString, argument 0)` with pinned source/runtime evidence; checker uses that fact for call-site invalidation. Review: The weak-mode graph records `Invoke(__toString, argument 0)` based on pinned source/runtime evidence. The checker uses the invocation fact to run its existing string-operation transfer and invalidate call-site facts. The graph does not provide closed write/throw observations, so ordered stateful callback effects and exceptions are not consumed from the graph. The graph leaves effect/exception domains open; stateful ordered writes/throws are not consumed by the checker. Review of the checker bridge found that for `strlen(...$spread)` the graph's argument 0 is `mixed`, because spread arguments are skipped by `resolve_call_args`. The invalidation step indexes the raw call arguments, so it invalidates the spread source expression, not the unpacked element, the mechanism is confirmed in code, but a PHP probe did not reproduce a missed invalidation (RV-10).

- **R05.2 — Open** (depends on: R04.1, R05.1)
  Close Stringable write/throw domains only with evidence and consume ordered graph effects/exceptions in checker evaluation. Test state before/after callbacks, normal/throw, diagnostics, and invalidation.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

- **R05.8 — Open** (depends on: R05.2)
  Return `Maybe`, not `No`, for any member that may be an object but is not `Repr::Object` or `EnumCase`: bare `object`, object shapes, object-bearing intersections, and object-bounded templates. The Stringable predicate answers No for them today, and a PHP probe reproduces the miss: a bare `object` argument leaves `$c->p` narrowed to `1` after `strlen($o)`, where a `mixed` argument invalidates it (RV-9). Trait-supplied `__toString` is also a false positive: the checker reports `expects string, Traited given` where PHP accepts it (RV-17). The null predicate shows the same family: `@template T of string|null` gets no null deprecation where `?string` does (not traced; see the review record). Acceptance: fixtures for each of those forms, with a `__toString` that writes state, and the predicate answering `Maybe`.

- **R05.9 — Complete** (depends on: None)
  Done: the two side-effecting calls are hoisted out of their `debug_assert!` macros in `registry/resolve.rs`. Under the gate profile, the full library suite now has one failure (RV-15, R05.6). The eight PHPDoc fixtures pass in the gate CLI. The gate-profile library tests and the fixtures are the release-profile regression set, since the debug build cannot reproduce the defect.
  Original scope: Hoist the side-effecting `debug_assert!` calls in `registry/resolve.rs` (lines 459 and 1602, from commits `32fb0a32` and `d28d22a1`) so that the call runs in every profile and only the check is debug-only. Acceptance: the gate-profile library and fixture runs show no failures from this cause (RV-14).

### F05.5 — Partial: Prevent duplicate legacy application for catalog-covered calls

**Reviewed evidence and current disposition:** Catalog invocation facts drive covered-call invalidation; legacy builtin-run behavior remains for calls without catalog evaluation. Review: Catalog-covered invocation avoids a second legacy builtin-run application for that exact path; uncatalogued calls retain legacy behavior. This does not establish a global no-duplicate rule across every consumer, and the underlying operation transfer is still existing checker logic. Verify this boundary across other consumers and scenarios; current operation transfer still comes from existing checker logic.

- **R05.3 — Open** (depends on: R05.2)
  Audit all consumers for duplicate/skipped legacy transfers and define ownership on covered/uncovered paths. Cross-consumer tests show exactly one application without losing unrelated effects/diagnostics.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F05.6 — Open: Validate full target profile and uncovered behavior end to end

**Reviewed evidence and current disposition:** Exact PHP release selection is tested; current entry predicate is release-only although its linked/runtime evidence is Linux x86-64 CLI. Review: Exact release selection is tested, but full platform/build/SAPI/extensions/configuration facts and covered/uncovered remainders have not been validated end to end. The checked-in entry predicate selects by PHP 8.5.4 alone. Its LLVM and runtime evidence identifies Linux x86-64 CLI, while the checker target adapter leaves those facts unknown and still accepts the release-only predicate. The PHP source-level transfer may be portable, but that applicability claim is not encoded as a validated assumption. Encode/validate the transfer's profile applicability (or establish source-level portability), lock platform/build/SAPI/extensions/configuration, and exercise covered and uncovered remainders.

- **R05.1 — Open** (depends on: R01.1, R01.3, R00.3)
  Validate locked Linux x86-64 CLI evidence against the entry predicate or establish source-level portability; coordinate shared profile applicability with F05.1.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F05.7 — Open: Resolve the full fixture-suite failures found during evaluation

**Status note (second review, 2026-10-09):** the cause was side-effecting `debug_assert!` calls in `registry/resolve.rs`, which dropped `@method` and `@mixin` members in release-profile builds (RV-14, R05.9, now Complete). The gate-profile and debug library suites both pass (RV-15 fixed, R05.6). F05.7 stays Open until the full optimized fixture suite is rerun (R05.7).

**Reviewed evidence and current disposition:** The current focused `strlen` set passes 16 fixtures. A fresh full optimized fixture run passed 3,954/3,962; eight `@method`/`@mixin` PHPDoc fixtures failed. A separate fresh optimized library run passed 1,198/1,202; four project/registry tests failed. Each of those four library failures reproduced alone; only the deferred-registry case was compared with parent `3129ece5`. Review: The current focused `strlen_` set passes 16 fixtures. A fresh full optimized fixture run passed 3,954/3,962. Its eight failures are `a_method_tags_own_templates_are_inferred_and_validated`; `a_method_tag_enforces_its_own_argument_types`; `a_method_tag_enforces_its_own_return_type`; `a_method_tag_synthesizes_a_callable_virtual_method`; `a_method_with_no_return_type_defaults_to_mixed`; `a_mixin_makes_the_target_s_public_methods_callable`; and `a_method_tag_parameter_type_referencing_an_unknown_class_is_flagged` plus `a_method_tag_return_type_referencing_an_unknown_class_is_flagged`. A separate fresh optimized library run passed 1,198/1,202; its four failures are `deferred_registry_passes_all_run_through_the_real_project_path`, `switching_registry_bases_invalidates_the_previous_id_generation`, `trait_mixin_and_phpdoc_members_live_in_host_topology`, and `mixin_and_phpdoc_method_topology_survives_freeze_and_cache_round_trip`. All four library failures reproduced alone; only the deferred-registry case was checked against parent `3129ece5`. These are two different suites and both failure sets remain open. Do not claim a clean optimized gate until each set is resolved or baseline-established. Establish or resolve both failure sets independently. Compare the remaining three library failures against the parent; establish a baseline for the eight PHPDoc fixture failures. Do not combine the two suites' failure counts.

- **R05.5 — Open** (depends on: R05.9)
  Compare each of the eight PHPDoc fixture failures with parent/control; attribute and fix regressions or record an accepted baseline. Second-review result: all eight were caused by the side-effect defect in R05.9. With that fix all eight pass in the gate CLI.
  Not yet evidenced as complete; see parent F task for the audited implementation state.
- **R05.6 — Complete** (depends on: R05.9)
  Done: all four remaining library failures are attributed. Three are the R05.9 defect, fixed in `479a948b`. The fourth, `switching_registry_bases_invalidates_the_previous_id_generation`, asserted a debug-only panic. It now accepts a panic in debug or `None` in release, and fails only on a live declaration (RV-15). Both profiles pass the full library suite.
  Not yet evidenced as complete; see parent F task for the audited implementation state.
- **R05.7 — Open** (depends on: R05.2, R05.3, R05.4, R05.5, R05.6)
  Rerun optimized fixture and library suites separately; require both to pass or every remaining failure to have an approved baseline disposition.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F06.1 — Partial: Compare candidate producers and compiler routes

**Reviewed evidence and current disposition:** Clang/GCC/CIR campaigns and prior comparative evidence are recorded; LLVM plus source metadata is the initial producer choice. Review: Comparative work evaluates LLVM plus Clang metadata, AST/CFG, GCC GIMPLE, CIR, Clang analysis interfaces, SVF, and bounded alternatives. LLVM plus metadata is a reasoned initial producer choice; systematic semantic coverage and evidence from useful closed summaries remain absent. Revisit with evidence from a useful closed production summary. Same-slice comparison is tracked under F06.3/R06.1.


### F06.2 — Complete: Establish full-profile SVF resource feasibility

**Reviewed evidence and current disposition:** Stock/layout-only SVF resource failures are measured on the minimal CLI link. Review: The measured stock and compact-layout SVF whole-link attempts fail the configured 10 GiB gate on the minimal CLI profile. This rejects those configurations as mandatory whole-profile prerequisites; it does not reject every pointer-assistance use or establish a replacement. This rules out that configuration as a mandatory whole-profile prerequisite, not all pointer-service assistance.


### F06.3 — Open: Compare regional and audited pointer-service alternatives on the same slice

**Reviewed evidence and current disposition:** Reduced SVF probe supplies a fallback counterexample. Review: Reduced SVF probes show fallback and target-coverage problems, but there is no same-slice comparison of adapted pointer reuse against the regional candidate on a useful closed PHP summary. Build an adapted, coverage-aware comparison against a regional candidate on identical input and demand.

- **R06.1 — Open** (depends on: R03.2, R03.3, R04.1)
  Compare adapted, coverage-aware pointer reuse against a regional candidate on identical input and demand using a useful closed PHP summary. Reduced fallback probes alone do not close this task.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F06.4 — Open: Measure graph growth, memory, runtime, and checker cost

**Reviewed evidence and current disposition:** No calibrated same-slice cost report is recorded. Review: Compiler/import and pointer-prerequisite resource measurements exist. There is no calibrated same-slice report for graph/region growth, artifact size, checker evaluation/runtime, memory, and maintenance cost across candidate solvers. Measure reproducibly and compare alternatives; include artifact size and checker evaluation costs.

- **R06.2 — Open** (depends on: R06.1, R05.2)
  Measure graph/region growth, artifact size, evaluation time, memory, checker cost, and maintenance/rebuild cost on the same locked slice and candidates.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

- **R06.4 — Open** (depends on: None)
  Make the serial campaign wait for the first enforcing worker to publish its proof and finish before admitting other memory-intensive jobs. `campaign.py` currently skips the wait when the proof file does not yet exist. Acceptance: a delayed-worker test demonstrates no second job starts before the first worker exits. The loop also breaks when the cgroup path disappears without an `exit_code` record, which admits the next job (RV-13). The pointer-query runs (`campaign.py` lines 74, 81–82) are unasserted, and `completed.json` is written regardless of results, so a completed marker means every step ran, not that each succeeded. `finish_campaign.py` also hard-codes `/tmp` paths for the PHP and stock `dvf` binaries (RV-18).
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F06.5 — Open: Calibrate defaults and record final producer/solver decision

**Reviewed evidence and current disposition:** Initial producer preference is documented; solver acceptance remains provisional. Review: The initial producer preference is documented, but final producer/solver acceptance and calibrated defaults depend on useful closed summaries and same-slice precision/cost evidence. The plan's §§2.3, 6–7 need a final evidence-based update after that comparison. Needs useful closed summaries, same-slice precision/cost evidence, and updated plan §§2.3, 6–7.

- **R06.3 — Open** (depends on: R06.1, R06.2, R05.7)
  Calibrate defaults and update producer/solver decision and plan §§2.3, 6, and 7 from same-slice precision, coverage, and cost evidence.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

#### Gate roll-up

- **F00 — Open:** independent semantic-contract review and PHP-source validation remain.
- **F01 — Open:** core evaluator checks exist; broader reviewed contracts and a calibrated host remain.
- **F02 — Open:** compiler/source evidence is narrow and does not establish broad native semantic correspondence.
- **F03 — Open:** reduced probes exist, but production demand summaries, pointer reuse, and native post-fixpoint validation do not.
- **F04 — Minimum Phase F fixture set complete; broader semantics deferred to C22.**
- **F05 — Open:** the exact `strlen` slice is connected, while profile coverage and stateful effects/exceptions remain incomplete.
- **F06 — Open:** no final solver decision or calibrated same-slice cost comparison exists.
