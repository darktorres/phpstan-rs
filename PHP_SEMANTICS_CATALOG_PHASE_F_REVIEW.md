# Phase F task-by-task review

Reviewed October 9, 2026 against the task ledger, current contract/review, Rust implementation and tests, pinned PHP/compiler/runtime artifacts, comparative decision record, and the latest recorded optimized gate run. This is an author audit of evidence and scope, not an independent semantic acceptance. Status terms below retain the ledger meaning: **complete** is complete only for the explicitly narrow subtask; **partial/open** remains a gate limitation.

## F00 — Contract and semantic boundary

- **F00.1 — Partial.** `PHP_SEMANTICS_CATALOG_CONTRACT.md` now specifies observable projections, ordering, abstract order, assumption obligations, and return/exception refinement. `Evaluation` still returns projected exits/states/results and a replay certificate rather than an explicit trace set. Host implementations must preserve whatever ordered observations a closed projection needs. Independent review remains outstanding, so this is a useful draft, not accepted semantics.
- **F00.2 — Complete for recorded counterexamples.** The locked CBMC replay records 93 valid-contract properties and the expected invalid-composition counterexample among 94. This checks the listed algebraic counterexamples; it does not prove arbitrary host transfers or every concrete PHP trace.
- **F00.3 — Partial.** The `strlen` builder checks selected `zend_API.c` parser gates, the `__toString` dispatch source, LLVM data-flow, and positive runtime sentinels in locked profiles. Strict/weak fixtures cover the tested behavior. The evidence does not validate every host transfer, callback mutation, thrown outcome, release/destructor effect, or unsupported PHP behavior.
- **F00.4 — Complete as a specification only.** The contract now defines typed assumption records, canonical identity, evidence separation, acyclic prerequisites, and target/program applicability. `CatalogManifest` still has no assumption-record collection and currently resolves coverage assumptions against evidence IDs; `EvidenceRecord.assumptions` are not resolved. Implementation belongs to F01.1.
- **F00.5 — Open.** `PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md` is an author self-review. No independent reviewer findings or acceptance are recorded. This is a real sequencing dependency: F01 implementation and later checker/native work already exist, but their results remain provisional under the ledger's stated gate order.

## F01 — Graph schema, evaluator, and reference contracts

- **F01.1 — Partial.** Schema v7, canonical graph/evidence digests, coverage dependencies, evidence-reference checks, and JSON round trips have implementation and tests. The missing typed assumption records and applicability validation identified at F00.4 prevent closure. Deeper inspection found more gaps in the same validator: evidence dependencies are checked for existence but not cycles; catalog entry release predicates are not range-validated on add/load; and a closed domain may depend on a `NotApplicable` domain because dependency validation only rejects `Open`. The last case conflicts with the contract statement that a not-applicable child contributes no fact. The manifest also currently mixes assumption references into the evidence namespace.
- **F01.2 — Complete for the evaluator kernel.** Worklist evaluation, joins/widening, normal and exceptional continuations, PHP throw versus native unwind, fatal/suspended exits, cleanup/resumption, and masked unknown transfers have targeted fixtures. Tests reject join/widening losses and check loop convergence. Production-domain calibration and semantic-host soundness are separate open obligations.
- **F01.3 — Complete for graph replay validation.** Certificates bind a graph digest and replay transfer candidates, successors, exits, and coverage; tests reject dropped results/pending outcomes, missing successors/exits, graph changes, and falsely closed coverage. The certificate trusts the supplied host's transfer semantics. Conservative extra stored exits are allowed, so the validator establishes a post-fixpoint over that host, not PHP correctness or a native-IR post-fixpoint.
- **F01.4 — Partial.** Callback result/write order, result-selected second callback, state-sensitive predicates, branch joins, callback throws, cleanup, and normal/throw state separation have fixtures. These are substantial reference-contract examples, but not an independently reviewed or exhaustive composition matrix; F00.5 remains open.
- **F01.5 — Open.** A PHP-specific host and value/reference fixtures exist, including havoc for unknown callbacks and open coverage. It is still a narrow host, not a source-calibrated production semantic domain. In particular, the graph's `Stringable` invocation has open callback effects and exceptions.

## F02 — Locked native/compiler evidence

- **F02.1 — Complete for the experimental profile.** PHP 8.5.4 source commit, compiler/tool versions, build and link manifests, binary identities, and Hybrid/CALL runtime profiles are recorded. This does not cover other platforms, SAPIs, builds, or extensions.
- **F02.2 — Complete as inventory and Return-demand evidence.** The linked `zif_strlen` inventory accounts for 241 instructions (storage/layout/scalar/call/branch/return families), checks unknown opcodes and missing return, and traces the selected string-length field to the return payload. It is accounting plus a narrow demand witness; it does not interpret all instructions.
- **F02.3 — Partial.** Source/layout/helper evidence and LLVM extraction support the narrow `strlen` Return/Diagnostic paths. The AST/CFG, LLVM, GCC GIMPLE, and CIR campaigns expose real representation behavior and counterexamples, including Clang CFG exceptional cleanup loss and poor CIR coverage. A systematic AST/CFG-to-LLVM semantic correspondence adapter and broad transfer comparison are absent.
- **F02.4 — Partial.** Weak/strict scalar and Stringable source/runtime witnesses, paired VM comparisons, and 13 runtime sentinels support positive tested paths. I replayed the historical runtime fixture with recorded source hash `9b38a3ea…`; both cached Hybrid/CALL binaries produced the checked-in output byte for byte (`2bd72db6…`). The builder's callback-specific validation does not compare that recorded fixture hash with source bytes; it checks only that the current fixture still contains the sentinel, so future source drift can pass regeneration. Matching sentinels also do not establish equivalence over unobserved inputs; callback writes/throws and other implicit operations are not all connected to modeled graph behavior.
- **F02.5 — Open.** The graph closes only its narrow return and exact null-diagnostic observations. Scalar conversion, complete callback behavior, object release, effects, exceptions, fatal/unwind behavior, and other non-normal outcomes remain open. The opcode inventory intentionally fails closed instead of implying coverage.

## F03 — Demand summaries, pointer reuse, and fallback

- **F03.1 — Complete as a negative feasibility result.** Stock SVF and the compact-layout adaptation each hit the enforced 10 GiB cgroup limit on the linked minimal PHP CLI workload. This rules out those configurations as mandatory whole-link prerequisites. Each candidate had one enforcing attempt; neither result evaluates an alternative solver.
- **F03.2 — Partial.** Reduced probes cover Zend layout/dispatch, absent external writes, copy/initializer cases, and the empty Stringable-target fallback counterexample. The capsule lacks object construction/handler initialization, so an empty target is explicitly incomplete evidence. Broader source-derived demand summaries are not implemented.
- **F03.3 — Open.** Existing PTA internals track `data` and `anything` fallback flags, and the regression preserves a dangerous empty-target example. The analysis remains private to `php-catalog` and emits text; there is no machine-readable catalog adapter, per-target completeness contract, or production integration. A known target set cannot be treated as exhaustive when fallback is present.
- **F03.4 — Open.** The semantic graph certificate replays the graph evaluator, not imported native IR summaries. Native summary validation and missing-write/growth regressions at that boundary do not exist.

## F04 — PHP value/reference flow

- **F04.1 — Complete for the minimum Phase F fixture set.** The value store and PhpFlow tests cover array-copy isolation, nested references, object identity, alias rebinding/unset, reference cells, and callback mutation across normal/throw/cleanup paths. These fixtures meet the stated minimum gate; they do not constitute full PHP value semantics.
- **F04.2 — Deferred to C22.** Broader key coercion, object properties, and syntax-to-flow lowering remain later-plan work. The current ledger correctly keeps this separate from the minimum Phase F fixture result.

## F05 — Checker integration and target selection

- **F05.1 — Partial.** Exact patch selection, minor ranges, correlated scenarios, and uncovered/Maybe remainders are tested. The checker adapter leaves platform facts unknown; SAPI, build, extension, and configuration applicability are not in the selected profile.
- **F05.2 — Complete for exact PHP 8.5.4 `strlen`.** The checker loads the embedded artifact through manifest validation, replays its certificate, and consumes its narrow Return/Diagnostic projection. Locked regeneration is byte-identical. This is not a general artifact/profile pipeline.
- **F05.3 — Partial.** Argument checks, return inference, and the exact-target null deprecation share a memoized graph evaluation; weak/strict and version-boundary fixtures exercise it. Broader source-derived diagnostics and ranges outside exact coverage still use legacy behavior.
- **F05.4 — Partial.** The weak-mode graph records `Invoke(__toString, argument 0)` based on pinned source/runtime evidence. The checker uses the invocation fact to run its existing string-operation transfer and invalidate call-site facts. The graph does not provide closed write/throw observations, so ordered stateful callback effects and exceptions are not consumed from the graph.
- **F05.5 — Partial.** Catalog-covered invocation avoids a second legacy builtin-run application for that exact path; uncatalogued calls retain legacy behavior. This does not establish a global no-duplicate rule across every consumer, and the underlying operation transfer is still existing checker logic.
- **F05.6 — Open.** Exact release selection is tested, but full platform/build/SAPI/extensions/configuration facts and covered/uncovered remainders have not been validated end to end. The checked-in entry predicate selects by PHP 8.5.4 alone. Its LLVM and runtime evidence identifies Linux x86-64 CLI, while the checker target adapter leaves those facts unknown and still accepts the release-only predicate. The PHP source-level transfer may be portable, but that applicability claim is not encoded as a validated assumption.
- **F05.7 — Open; ledger corrected.** The current focused `strlen_` set passes 16 fixtures. A fresh full optimized fixture run passed 3,954/3,962. Its eight failures are `a_method_tags_own_templates_are_inferred_and_validated`; `a_method_tag_enforces_its_own_argument_types`; `a_method_tag_enforces_its_own_return_type`; `a_method_tag_synthesizes_a_callable_virtual_method`; `a_method_with_no_return_type_defaults_to_mixed`; `a_mixin_makes_the_target_s_public_methods_callable`; and `a_method_tag_parameter_type_referencing_an_unknown_class_is_flagged` plus `a_method_tag_return_type_referencing_an_unknown_class_is_flagged`. A separate fresh optimized library run passed 1,198/1,202; its four failures are `deferred_registry_passes_all_run_through_the_real_project_path`, `switching_registry_bases_invalidates_the_previous_id_generation`, `trait_mixin_and_phpdoc_members_live_in_host_topology`, and `mixin_and_phpdoc_method_topology_survives_freeze_and_cache_round_trip`. All four library failures reproduced alone; only the deferred-registry case was checked against parent `3129ece5`. These are two different suites and both failure sets remain open. Do not claim a clean optimized gate until each set is resolved or baseline-established.

## F06 — Producer/solver decision and cost

- **F06.1 — Partial.** Comparative work evaluates LLVM plus Clang metadata, AST/CFG, GCC GIMPLE, CIR, Clang analysis interfaces, SVF, and bounded alternatives. LLVM plus metadata is a reasoned initial producer choice; systematic semantic coverage and evidence from useful closed summaries remain absent.
- **F06.2 — Complete as a narrow rejection.** The measured stock and compact-layout SVF whole-link attempts fail the configured 10 GiB gate on the minimal CLI profile. This rejects those configurations as mandatory whole-profile prerequisites; it does not reject every pointer-assistance use or establish a replacement.
- **F06.3 — Open.** Reduced SVF probes show fallback and target-coverage problems, but there is no same-slice comparison of adapted pointer reuse against the regional candidate on a useful closed PHP summary.
- **F06.4 — Open.** Compiler/import and pointer-prerequisite resource measurements exist. There is no calibrated same-slice report for graph/region growth, artifact size, checker evaluation/runtime, memory, and maintenance cost across candidate solvers.
- **F06.5 — Open.** The initial producer preference is documented, but final producer/solver acceptance and calibrated defaults depend on useful closed summaries and same-slice precision/cost evidence. The plan's §§2.3, 6–7 need a final evidence-based update after that comparison.

## Ordering and overall disposition

The ledger split into 33 explicit subtasks is useful and mostly tracks real evidence. Work has nevertheless crossed the stated gate dependency: F01 and F05 implementation proceeded while F00.5 independent review was still open. The result is not that the implementation should be discarded; it is that later task statuses describe implementation evidence, not accepted gate completion. Keep F00.5 as the next acceptance checkpoint, record any independent amendments, then revisit dependent claims in order. Do not represent F01–F06 as accepted merely because their narrow tests pass.

F04.1 is the only task group whose stated minimum Phase F fixture criterion is met, and F02.1/F02.2 plus F03.1/F06.2 have bounded, complete evidence for their explicitly narrow outcomes. F00, F01, F02, F03, F05, and F06 remain open at gate level. The central architectural gap remains a locked native-to-graph-to-checker slice with validated semantic closure, target applicability, effects/exceptions, and useful same-slice cost comparison.

## Deeper review and verification performed

I inspected the shared evaluator, certificate replay, manifest/catalog loader,
target-selection code, PHP value/reference host, `strlen` builder and LLVM
extractor, checker call path and effect bridge, and SVF fallback replay script.
I also checked the contract and counterexample C model, the current evaluation
and approach records, checked-in artifact summaries, and fixture expectations
touched by Stringable integration. For code and scripts, I traced how the
claimed status follows from implementation and negative cases rather than
treating a passing test count as semantic acceptance.

The inspected work maps to these task histories: F00 contract/CBMC/self-review
(`599c2508`, `5df391b9`, `e62c000e`); F01 graph, evaluator, target and PHP-flow
implementation (`9df6d5d3`, `80795f6e`, `d5f7598f`, `7cb0c627`, `d3c3fa63`,
`eee12e0f`, `a0b083a8`, `dab7e44f`); F02 locked compiler/source/runtime and
inventory work (`043f2c78`, `4e7d29a3`, `2b6afb95`, `47f5e62a`, `08499695`,
`0a2f229e`, `8217e557`); F03 pointer-boundary and fallback records
(`74229f12`, `fe1cab81`, `9b384e8c`); F04 value/reference and callback-flow work
(`bd1b0272`, `68ec8f18`, `a0b083a8`, `017fe4c7`, `dab7e44f`); F05 target,
checker, coercion, diagnostic, memo and Stringable integration
(`9b1c449e`, `bb274ed4`, `df437149`, `d6bcf2c1`, `93d950ee`, `ea4f4b50`,
`09acd24e`); F06 comparative decision and extended campaigns
(`043f2c78`, `4e7d29a3`, `2b6afb95`). I reviewed the resulting current files
and test paths, not just the commit subjects.

Fresh checks on the current tree produced:

- Semantic crate: 60/60 passed.
- `build_strlen_catalog` example tests: 5/5 passed.
- Focused optimized `strlen_` fixtures: 16/16 passed.
- LLVM inventory and SVF replay Python tests: 5/5 passed.
- Full optimized checker library tests: 1,198/1,202 passed; four project/registry failures are listed under F05.7.
- Full optimized checker fixtures: 3,954/3,962 passed; eight PHPDoc failures are listed under F05.7.
- Rebuilt the linked LLVM witness and catalog from cached locked source/bitcode; output matched the checked-in artifact byte for byte (SHA-256 `b55221654574043a15d50dfd862377863a39e2272b5aab148c7da35b74d1e33b`).
- Replayed the historical runtime fixture against cached Hybrid/CALL binaries; outputs matched each other and the checked-in capture hash (`2bd72db611daeac4b383eed0b0352ac80581ab6c7e58e8cb13a8b505f9411c04`).

The additional review findings are schema and applicability gaps: evidence-
dependency cycle validation is missing; entry release ranges are not
validated; closed-domain dependencies on `NotApplicable` are accepted; the
runtime callback builder does not verify the source fixture digest; and the
release-only catalog applicability is broader than the compiled/runtime
profile evidence unless source-transfer portability is accepted as an explicit
assumption. These keep the corresponding F00/F01/F02/F05 claims open.

## Review evidence consulted

- `PHP_SEMANTICS_CATALOG_EVALUATION.md` and its 33-row Phase F ledger, including the source/runtime, optimized test, and 10 GiB SVF results.
- `PHP_SEMANTICS_CATALOG_CONTRACT.md` and `PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md`.
- `PHP_SEMANTICS_CATALOG_PLAN.md` Phase F criteria and §2.3 decision obligations.
- `crates/php-semantics/src/lib.rs`, `catalog.rs`, and `php_value.rs`, plus checker catalog/call-effect integration.
- `tools/php-catalog/experiments/comparison/README.md`, the comparison decision record, checked-in witness/inventory/runtime/SVF artifacts, and their stated reproduction commands.

This review inspected current source and test logic and reran the semantic,
builder, focused checker, evidence-script, and full optimized checker suites.
It did not repeat the multi-hour compiler/CIR and whole-link SVF campaigns;
those remain reviewed from their locked scripts, outputs, and decision record.
