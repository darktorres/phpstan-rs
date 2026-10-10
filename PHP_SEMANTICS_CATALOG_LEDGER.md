# PHP catalog Phase F and remediation ledger

This is the authoritative status record for Phase F tasks and their remediation actions. Task evidence and evaluation methods are documented in [the evaluation report](PHP_SEMANTICS_CATALOG_EVALUATION.md). The semantic contract review is in [the contract review](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md).

## Phase F task and remediation ledger

Statuses: **complete** means the stated evidence exists and passed its recorded
check; **partial** means evidence covers only the listed narrow slice; **open**
means the required evidence has not been produced or independently reviewed;
**deferred** means the work belongs to a later plan phase.

### F00.1 — Partial: Specify observable traces, assumptions, abstract order, and return-refinement rules

**Reviewed evidence and current disposition:** Contract defines observable projections, trace order, host obligations, and return/exception distinctions. Review: `PHP_SEMANTICS_CATALOG_CONTRACT.md` now specifies observable projections, ordering, abstract order, assumption obligations, and return/exception refinement. `Evaluation` still returns projected exits/states/results and a replay certificate rather than an explicit trace set. Host implementations must preserve whatever ordered observations a closed projection needs. Independent review remains outstanding, so this is a useful draft, not accepted semantics. Independent review remains; see [F00 review](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md).

- **R00.2 — Open** (depends on: R00.1)
  Define the abstract-to-concrete representation/simulation relation or narrow the Phase F claim and evidence standard. Specify correlation through projection, joins, and widening; define cleanup completion and cross-namespace support/trust. Acceptance: independent review and tests cover initial states, transfers, joins/widening, projections, and exits.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F00.2 — Partial: Reject invalid trace compositions with counterexamples

**Status note (independent review, 2026-10-09):** downgraded from Complete. The fatal and native-unwind counterexample row has no cleanup-path evidence, and the evaluator accepts those compositions (RV-1 in the [review record](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md#independent-review-record-2026-10-09)).

**Reviewed evidence and current disposition:** Locked CBMC replay passes 93 valid-contract properties; invalid composition yields the expected `main.assertion.11` counterexample among 94. Review: The locked CBMC replay records 93 valid-contract properties and the expected invalid-composition counterexample among 94. This checks the listed algebraic counterexamples; it does not prove arbitrary host transfers or every concrete PHP trace. Keep counterexamples synchronized with any contract changes.

- **R00.4 — Open** (depends on: None)
  Map each recorded CBMC property to a contract clause and classify it as reduced C-model-only or cross-checked by Rust/PHP. Account for all 93 valid and 94 invalid-run properties, expected main.assertion.11, --unwind 4 and loop bound; keep claims scoped to the C model. Make the campaign fail closed on unexpected CBMC exit/property results instead of only recording command output. `complementary()` in `campaign.py` does not assert the CBMC steps, and `run()` reports success only for exit 0, so expected CBMC failures are recorded and ignored. The `cbmc-low-budget` run (`loop_contract.c`, `--unwind 2`; one unwinding-assertion failure, `main.unwind.0`) is not classified anywhere in the ledger or evaluation (RV-8).
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F00.3 — Partial: Validate host transfer functions against PHP source behavior

**Reviewed evidence and current disposition:** Narrow weak `strlen` parser/cast source checks, runtime callback sentinel, strict/weak and Stringable fixtures exist. Review: The `strlen` builder checks selected `zend_API.c` parser gates, the `__toString` dispatch source, LLVM data-flow, and positive runtime sentinels in locked profiles. Strict/weak fixtures cover the tested behavior. The evidence does not validate every host transfer, callback mutation, thrown outcome, release/destructor effect, or unsupported PHP behavior. General source-to-host audit and callback effect/throw modeling remain open; see [F00 review](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md).

- **R00.3 — Open** (depends on: R00.1, R00.2)
  Build a source-to-host matrix for weak/strict strlen, casts, Stringable dispatch, callback mutation/throw, release, and unknown behavior. Include the `ZEND_STRLEN` opcode path (`zend_compile.c:4062–4079`), which is what a single plain-argument `strlen` compiles to, not only `zif_strlen`; the artifact models only the latter (RV-16). List affected state/result/exit components and required domains. Acceptance: every closed fact has source, assumptions, transfer, and positive/negative/contradictory cases; unsupported behavior stays open.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F00.4 — Partial: Specify distinct, validated assumption identities

**Status note (independent review, 2026-10-09):** downgraded from Complete. The contract specifies typed assumption records. The code has no assumption records and no applicability check, and it accepts non-empty assumption vectors (RV-4). The title's "validated" is not met.

**Reviewed evidence and current disposition:** The contract defines typed records, canonical IDs, distinct evidence references, acyclic dependencies, and context-dependent validation. Review: The contract now defines typed assumption records, canonical identity, evidence separation, acyclic prerequisites, and target/program applicability. `CatalogManifest` still has no assumption-record collection and currently resolves coverage assumptions against evidence IDs; `EvidenceRecord.assumptions` are not resolved. Implementation belongs to F01.1. Implement record validation under F01.1; current artifacts still contain opaque digests only. See [F00 review](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md).


### F00.5 — Open: Independently review the contract and assumptions

**Reviewed evidence and current disposition:** Author self-review is recorded. An independent review is now recorded in the [review record](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md#independent-review-record-2026-10-09) with findings still open, so this item stays Open until they are resolved or accepted. Review: `PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md` is an author self-review. No independent reviewer findings or acceptance are recorded. This is a real sequencing dependency: F01 implementation and later checker/native work already exist, but their results remain provisional under the ledger's stated gate order. Obtain and record reviewer findings before closing F00 or starting dependent F01 work.

- **R00.1 — Open** (depends on: none; findings open)
  Review recorded 2026-10-09 by a fresh reviewer session. It is a model, not a human reviewer; see the [review record](PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md#independent-review-record-2026-10-09). Findings RV-1 to RV-13 and C-1 to C-6 are open, and none is accepted. Close this row when each finding is resolved or the maintainer accepts it by name, and a human reviewer is recorded.
  Obtain and record review by someone other than this author, including reviewer role, findings, and disposition. Review traces, abstract order, host obligations, assumptions/applicability, exceptional outcomes, and counterexample scope; resolve or explicitly accept every finding.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F01.1 — Partial: Implement versioned graph, evidence, assumption, and domain-coverage schema

**Reviewed evidence and current disposition:** Canonical digests, explicit coverage/dependencies, evidence validation, and JSON round trips have tests. Review: Schema v7, canonical graph/evidence digests, coverage dependencies, evidence-reference checks, and JSON round trips have implementation and tests. The missing typed assumption records and applicability validation identified at F00.4 prevent closure. Deeper inspection found more gaps in the same validator: evidence dependencies are checked for existence but not cycles; catalog entry release predicates are not range-validated on add/load; and a closed domain may depend on a `NotApplicable` domain because dependency validation only rejects `Open`. The last case conflicts with the contract statement that a not-applicable child contributes no fact. The manifest also currently mixes assumption references into the evidence namespace. Review of the producing commit found that `EvidenceRecord::id` hashes raw dependency and assumption vector order, while semantically unordered vector permutations are accepted as different IDs; `CatalogManifest::canonical_digest` likewise hashes producer order without validating it. Add distinct assumption records and context validation (F00.4); reject evidence-dependency cycles, invalid entry release ranges, and closed dependencies on `NotApplicable` domains.

- **R01.1 — Open** (depends on: R00.1)
  Add typed assumption records separate from evidence; validate canonical IDs, evidence references, applicability context, prerequisites, and round trips. Reject missing/wrong-namespace references and invalid contexts. See R01.8 for canonical-order enforcement across evidence and manifest digests.
  Not yet evidenced as complete; see parent F task for the audited implementation state.
- **R01.2 — Open** (depends on: R01.1)
  Reject cycles in assumption prerequisites and in cross-namespace support (ACR-3). Evidence-dependency cycles cannot be formed, because IDs are SHA-256 digests of records that include their dependency IDs and are recomputed on validation (RV-6). Test self-cycles, multi-node cycles, and valid DAGs with useful diagnostics for the assumption graph.
  Not yet evidenced as complete; see parent F task for the audited implementation state.
- **R01.3 — Open** (depends on: None)
  Validate release ranges on catalog add and manifest load; reject inverted/malformed ranges while preserving exact, minor, and boundary cases. Inverted predicates currently never yield `Yes`, so this is a validation gap rather than a soundness gap (RV-7).
  Not yet evidenced as complete; see parent F task for the audited implementation state.
- **R01.4 — Open** (depends on: R00.1)
  Resolve NotApplicable dependency semantics. Ensure a closed domain cannot obtain a fact from an N/A dependency; test Closed/Open/N/A combinations. Also reject an Open domain reached through an N/A intermediate: dependency validation and open-propagation follow only Closed-to-Closed edges (RV-2; Return(Closed) → Diagnostic(N/A) → UserWrite(Open) is accepted).
  Not yet evidenced as complete; see parent F task for the audited implementation state.
- **R01.6 — Open** (depends on: R00.2, R00.3)
  Declare mandatory semantic-domain dependencies per reviewed transfer/projection and check them for the closed slice. Unknown event contracts stay open; mutation tests removing required edges must fail. The locked strlen `Return` is closed with no dependencies while Invocation, PhpThrow, and UserWrite are open: record a justification for that empty list (RV-3). The validator also accepts a closed `Return` on a path containing a closed `Write`, so required edges must be derived from events, not declared by hand (RV-3).
  Not yet evidenced as complete; see parent F task for the audited implementation state.
- **R01.7 — Open** (depends on: R00.2, R01.1)
  Validate cross-namespace evidence/assumption support as one graph, rejecting circular support; define trusted validator roots, version pinning, revocation/invalidation. Test cross-cycle and untrusted/stale validators.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

- **R01.8 — Open** (depends on: None)
  Canonicalize or reject noncanonical order for semantically unordered evidence, assumption, coverage, and manifest vectors. Acceptance: permuting an unordered list produces the same canonical ID/digest, or validation rejects it with a clear error; ordered graph blocks/events remain order-sensitive.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

### F01.2 — Partial: Implement evaluator control flow and exceptional continuations

**Status note (independent review, 2026-10-09):** downgraded from Complete. A cleanup `Return` or `Throw` replaces a pending fatal, suspension, or native unwind, which the contract forbids in part (RV-1, tracked in R01.5).

**Reviewed evidence and current disposition:** Worklist, joins/widening, PHP/native exceptional exits, cleanup/resumption, and masked unknown transfers are tested. Review: Worklist evaluation, joins/widening, normal and exceptional continuations, PHP throw versus native unwind, fatal/suspended exits, cleanup/resumption, and masked unknown transfers have targeted fixtures. Tests reject join/widening losses and check loop convergence. Production-domain calibration and semantic-host soundness are separate open obligations. Calibrate domains for production behavior.


### F01.3 — Complete: Validate post-fixpoint certificates and reject incomplete graph behavior

**Reviewed evidence and current disposition:** Graph-bound replay checks transfer candidates, exits, and coverage; partial result definitions and missing/duplicate invocation coverage fail closed. Review: Certificates bind a graph digest and replay transfer candidates, successors, exits, and coverage; tests reject dropped results/pending outcomes, missing successors/exits, graph changes, and falsely closed coverage. The certificate trusts the supplied host's transfer semantics. Conservative extra stored exits are allowed, so the validator establishes a post-fixpoint over that host, not PHP correctness or a native-IR post-fixpoint. None for the implemented validation contract.


### F01.4 — Partial: Validate reference contracts with broader branch/callback cases

**Reviewed evidence and current disposition:** Callback result/write/throw order, state-sensitive predicates, joins, cleanup, and second-callback branch selection are covered. Review: Callback result/write order, result-selected second callback, state-sensitive predicates, branch joins, callback throws, cleanup, and normal/throw state separation have fixtures. These are substantial reference-contract examples, but not an independently reviewed or exhaustive composition matrix; F00.5 remains open. Independent contract review remains F00-dependent; broaden combinations after review.

- **R01.5 — Open** (depends on: R00.2)
  Implement the reviewed pending-completion × cleanup-completion matrix and broaden branch/callback/join compositions. Test every supported cell; unsupported cells remain open or fail closed. Current behavior is wrong, not merely missing: a cleanup `Return` turns a pending fatal or native unwind into a normal exit, and a cleanup `Throw` turns a pending fatal into a PHP throw (RV-1). Add a failing test for each cell before fixing it.
  Not yet evidenced as complete; see parent F task for the audited implementation state.

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

**Status note (second review, 2026-10-09):** the cause was side-effecting `debug_assert!` calls in `registry/resolve.rs`, which dropped `@method` and `@mixin` members in release-profile builds (RV-14, R05.9, now Complete). With the fix, the gate-profile library suite has one failure left, a test that asserts a debug-only panic (RV-15, R05.6). The suites stay Open until that test is corrected and both suites are rerun.

**Reviewed evidence and current disposition:** The current focused `strlen` set passes 16 fixtures. A fresh full optimized fixture run passed 3,954/3,962; eight `@method`/`@mixin` PHPDoc fixtures failed. A separate fresh optimized library run passed 1,198/1,202; four project/registry tests failed. Each of those four library failures reproduced alone; only the deferred-registry case was compared with parent `3129ece5`. Review: The current focused `strlen_` set passes 16 fixtures. A fresh full optimized fixture run passed 3,954/3,962. Its eight failures are `a_method_tags_own_templates_are_inferred_and_validated`; `a_method_tag_enforces_its_own_argument_types`; `a_method_tag_enforces_its_own_return_type`; `a_method_tag_synthesizes_a_callable_virtual_method`; `a_method_with_no_return_type_defaults_to_mixed`; `a_mixin_makes_the_target_s_public_methods_callable`; and `a_method_tag_parameter_type_referencing_an_unknown_class_is_flagged` plus `a_method_tag_return_type_referencing_an_unknown_class_is_flagged`. A separate fresh optimized library run passed 1,198/1,202; its four failures are `deferred_registry_passes_all_run_through_the_real_project_path`, `switching_registry_bases_invalidates_the_previous_id_generation`, `trait_mixin_and_phpdoc_members_live_in_host_topology`, and `mixin_and_phpdoc_method_topology_survives_freeze_and_cache_round_trip`. All four library failures reproduced alone; only the deferred-registry case was checked against parent `3129ece5`. These are two different suites and both failure sets remain open. Do not claim a clean optimized gate until each set is resolved or baseline-established. Establish or resolve both failure sets independently. Compare the remaining three library failures against the parent; establish a baseline for the eight PHPDoc fixture failures. Do not combine the two suites' failure counts.

- **R05.5 — Open** (depends on: R05.9)
  Compare each of the eight PHPDoc fixture failures with parent/control; attribute and fix regressions or record an accepted baseline. Second-review result: all eight were caused by the side-effect defect in R05.9. With that fix all eight pass in the gate CLI.
  Not yet evidenced as complete; see parent F task for the audited implementation state.
- **R05.6 — Open** (depends on: R05.9)
  Compare the remaining three library failures with parent/control; attribute and fix regressions or record baseline dispositions. The deferred-registry case already has a control check. Second-review result: three of the four are the R05.9 defect; the fourth, `switching_registry_bases_invalidates_the_previous_id_generation`, asserts a debug-only panic and should accept `None` in release (RV-15).
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
