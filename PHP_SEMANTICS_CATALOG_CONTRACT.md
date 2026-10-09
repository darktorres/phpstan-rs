# PHP behavior catalog semantic contract (working draft)

This draft makes the plan's correctness rule testable for the first slice. It
is not a formal proof and has not received independent review; F00 stays open
until reviewers accept the contract and its assumptions.

## Observable result

For one catalog entry, target scenario, and explicit assumption set, the
concrete behavior is the set of finite observable traces paired with their
exit states. A trace records observable invocations, reads and writes,
diagnostics, autoload, output, release/retention, normal return values, PHP
throws, fatal exits, and suspension. Trace order is significant. Native
unwind is an analysis boundary, not itself a PHP throw. A nonterminating
execution contributes every finite observable prefix; proving termination is
a separate claim.

The shared evaluator does not enumerate concrete traces. It executes graph
events in order and returns abstract values, states, exits, and coverage. A
host state must therefore retain every observable fact needed by a closed
domain or by a consumer's requested projection. If it cannot represent or
prove an observable, that domain remains open; a closed claim cannot be
recovered from provenance alone. In particular, open coverage is not a
substitute for a sound transfer: an unknown operation must conservatively
widen or havoc every affected part of the abstract state and invalidate any
dependent result facts. A consumer may project away observations only after
the remaining abstract state still soundly contains their influence on the
requested fact.

The abstract result is sound only when its represented projections contain
every concrete behavior relevant to each claimed-closed domain. A closed
domain can exclude behavior only under its listed assumptions and evidence.
Missing events, absent native bodies, unresolved dispatch, unsupported
transfers, or incomplete target applicability are not negative facts: they
leave the affected domain open and retain a conservative unknown transfer or
uncovered target remainder. Provenance identifies the basis of a fact; it
does not prove a transfer function correct.

Every invocation outcome carries complete per-domain coverage, precision,
evidence, and assumptions for the composed call; other events carry the same
metadata when they compose child work. The evaluator unions evidence and assumptions,
retains the least precise result, and opens a domain if either the enclosing
graph or a composed outcome is open. A child marked not applicable contributes
no semantic fact for that domain. This prevents a closed native wrapper from
concealing an unresolved PHP callback.

Coverage entries also name the domains required to justify each result. A
closed result cannot depend on an open domain, and catalog projections return
the transitive dependency closure so consumers can retain it in their query.
During evaluation, newly open child domains propagate through these dependencies
and widen dependent parent domains instead of leaving them closed.

## Composition rules

Sequential composition passes each normal state and result into the next
operation. A PHP throw, fatal exit, or suspension bypasses normal-only
operations. Effects that happened before the exit remain visible. A cleanup
continuation may run for a pending exit; an explicit cleanup return or throw
replaces the pending outcome according to PHP control flow. Native unwind must
not be silently converted into a PHP throw or a normal return.

Alternative paths retain their guard and outcome correlation until a sound
join is required. Joining can add possible values or events, but cannot remove
a behavior. A stronger predicate or return refinement may narrow only a state
on which its independent preconditions hold. It cannot erase prior effects or
exceptional outcomes. A query projection can omit irrelevant reported
properties only after retaining all operations needed to derive the requested
property.

`Return` describes values on normal exits only; it does not imply that every
execution returns normally. A consumer making a claim about whether a call
returns must also account for PHP throws, native unwind boundaries, fatal
exits, and suspension. A normal-return value must be defined on every normal
path represented by that result. If paths with and without a definition are
joined, the evaluator must preserve their distinction or reject the
evaluation. A closed return refinement must declare dependencies on any
effect or exceptional domain whose behavior can invalidate the refinement's
preconditions.

The host operations `join_value`, `widen_value`, `join_state`, and
`widen_state` are semantic obligations: each result must contain both inputs
in the host's abstract order. Predicate answers must be `Maybe` unless the
host proves the answer for every represented state. The evaluator checks
containment where the host supplies subsumption, but those checks do not prove
that the host's abstract order or transfer functions match PHP.

Assumptions are first-class dependencies: source/build/link identity, loaded
program and target profile, defined native behavior, engine invariants, model
contracts, and dispatch/load-order conditions. Each assumption ID resolves to
an immutable record with a typed kind, claim, applicability scope, prerequisite
assumption IDs, supporting evidence IDs, and a validator identifier/version.
The ID is the digest of the canonical record. Evidence references identify why
a fact is supported; assumption references identify conditions under which it
applies. They are distinct namespaces, and both dependency relations must
resolve and be acyclic.

Applicability is evaluated against the selected target and loaded-program
context. An assumption is established only when its scope matches, all
prerequisite assumptions are established, and its validator accepts the
referenced evidence for that context. Missing records, missing or unverified
evidence, unknown validators, and unknown context facts do not establish the
assumption. Conclusions that depend on an unestablished assumption become open
or are recomputed. Runtime observations can contradict a closed result and
force investigation; failure to observe an event cannot prove its absence.

The current schema does not yet meet this requirement: it has opaque digest
vectors but no assumption records or applicability validation. Until that
schema gap is fixed, no producer may rely on a non-empty assumption vector to
justify a closed fact.

## Counterexamples that composition must reject

| Invalid result | Why it is unsound | Current regression evidence |
|---|---|---|
| A callback writes and throws, but the enclosing graph reports the old state or runs a later normal event. | The callback's pre-throw write is observable; normal continuation is unreachable on that path. | `callback_throw_preserves_prior_write_and_skips_later_events` |
| A callback has normal and throw outcomes, but their result/state correlations are flattened into one certain result. | This invents a result on the throw path or loses a possible exit. | `callback_normal_and_throw_paths_keep_separate_results_and_states` |
| A branch after an invocation uses the pre-call state or treats an unsupported host predicate as false. | Callback mutations precede the branch; an undecidable predicate must retain both paths. | `host_predicate_observes_callback_state_after_invocation`; `unsupported_host_predicate_keeps_both_branches` |
| Distinct normal callback branches write different values and return different results, but a later join keeps one branch's exact value. | Both branch states and results are possible after the join; retaining either as exact excludes the other. | `php_flow_host_joins_distinct_normal_callback_branches_conservatively` |
| A state guard after two predecessor states join decides true or false from the pre-join state. | The join contains both predecessors, so a guard not proved across the joined state must retain both successors. | `state_guard_after_a_join_keeps_both_possible_branches` |
| A result is defined on only one predecessor path, then used after the paths join as if it were available on both. | A value from one path cannot replace an undefined result on another; the evaluator must retain the path distinction or return an incomplete result. | `joining_paths_with_a_partially_defined_result_is_incomplete` |
| A pending throw enters cleanup, then cleanup returns, while the evaluator still reports the original throw. | The cleanup return overrides the pending completion. | `pending_callback_throw_runs_cleanup_then_resumes`; `cleanup_return_overrides_pending_throw` |
| A native unwind is reported as a PHP throw. | It crosses a different language/ABI boundary and does not establish PHP exception state. | `native_unwind_remains_distinct_from_php_throw` |
| A fatal exit or suspension is collapsed into a PHP throw or normal return. | These are distinct observable completion modes and carry different continuation rules. | `fatal_and_suspension_remain_distinct_exits` |
| An unresolved return is treated as no return or as a closed empty result. | Incomplete evaluation cannot justify excluding a return behavior. | `unresolved_return_is_an_incomplete_evaluation_not_no_return` |
| A graph labels a domain closed while containing an unknown transfer that affects it, or supplies no coverage/evidence. | Its negative claims are unsupported by the actual transfer graph. | `unknown_transfer_cannot_be_marked_closed_or_reasonless`; `every_domain_requires_a_coverage_decision_and_evidence` |
| A return refinement drops a possible write or throw effect by declaring a dependency open. | A projected fact is closed only when every required semantic domain is justified, including effects and exceptional outcomes that can change its preconditions. | `graph_rejects_closed_return_coverage_with_open_domain_dependency`; `closed_return_refinement_cannot_drop_an_open_throw_path`; `projection_includes_transitive_domain_dependencies` |
| A child opens a domain required by the parent return while the parent return remains closed. | Uncertainty propagates through declared semantic dependencies. | `open_child_dependency_opens_the_parent_result_domain` |
| Two closed domains justify each other through a dependency cycle without a fixed-point certificate. | Circular dependency metadata alone provides no independent basis for either negative claim. | `graph_rejects_cyclic_closed_domain_dependencies_without_a_certificate` |
| A closed wrapper invokes an open child but retains its own closed return coverage, or omits child coverage entirely or by domain. | The wrapper's result may depend on child behavior, so child uncertainty must compose into the result and incomplete metadata must not default to closed. | `open_child_coverage_opens_a_closed_parent_domain`; `incomplete_child_coverage_rejects_the_evaluation`; `omitted_child_coverage_rejects_the_evaluation` |
| An invoked child marks its invocation domain not applicable while returning a complete coverage vector. | A composed call necessarily has invocation behavior; a full vector cannot make that fact inapplicable. | `invoked_child_cannot_mark_invocation_not_applicable` |
| A maybe-applicable target graph is selected as definitely applicable, or its uncovered remainder is dropped. | The target set includes configurations whose behavior was not established. | `target_selection_keeps_unknown_remainder_and_selects_exact_scenario`; target-module correlated-scenario tests |
| A loop joins or widens abstract state while the returned domain metadata remains `Exact`. | Consumers could treat a joined or extrapolated fact as exact and overstate its precision. | `cyclic_graph_reaches_a_stable_joined_exit`; `widening_is_reflected_in_domain_precision` |
| A host join or widening returns a state/result that does not contain both incoming possibilities. | The worklist can reach an empty queue after silently dropping a reachable value, so queue exhaustion alone is not a closure argument. | `evaluator_rejects_a_join_that_drops_a_reachable_state`; `evaluator_rejects_a_value_join_that_drops_a_reachable_result`; `evaluator_rejects_a_widening_that_drops_a_reachable_state` |

These tests check structural behavior in a small evaluator host. They do not
establish that the checker host, native transfers, or every PHP runtime obeys
the contract. The PHP value host has a regression that checks an unknown
callback havocs tracked state and leaves effect domains open, but this is one
host-specific check, not a general proof of unknown-transfer soundness.
Mismatched target scenarios are covered in the target module. This contract
revision records a self-review against the evaluator API and current fixtures;
it has not received independent review, so F00 remains open until reviewers
accept the contract and its assumptions.
