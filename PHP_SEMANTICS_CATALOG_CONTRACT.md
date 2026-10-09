# PHP behavior catalog semantic contract (working draft)

This draft makes the plan's correctness rule testable for the first slice. It
is not a formal proof and has not received independent review; F00 stays open
until reviewers accept the contract and its assumptions.

## Observable result

For one catalog entry, target scenario, and explicit assumption set, the
concrete behavior is the set of finite observable traces plus their exit
states. A trace records observable invocations, reads and writes, diagnostics,
autoload, output, release/retention, return values, PHP throws, fatal exits,
and suspension. Trace order is significant. Native unwind is an analysis
boundary, not itself a PHP throw. A nonterminating execution contributes every
finite observable prefix; proving termination is a separate claim.

The abstract result is sound only when it contains every such concrete trace
and exit state. A closed domain can exclude behavior only under its listed
assumptions and evidence. Missing events, absent native bodies, unresolved
dispatch, unsupported transfers, or incomplete target applicability are not
negative facts: they leave the affected domain open and retain a conservative
unknown transfer or uncovered target remainder. Provenance identifies the
basis of a fact; it does not prove a transfer function correct.

Every invocation outcome carries complete per-domain coverage, precision,
evidence, and assumptions for the composed call; other events carry the same
metadata when they compose child work. The evaluator unions evidence and assumptions,
retains the least precise result, and opens a domain if either the enclosing
graph or a composed outcome is open. A child marked not applicable contributes
no semantic fact for that domain. This prevents a closed native wrapper from
concealing an unresolved PHP callback.

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

Assumptions are first-class dependencies: source/build/link identity, loaded
program and target profile, defined native behavior, engine invariants, model
contracts, and dispatch/load-order conditions. If an assumption is absent,
unverified, or invalidated, conclusions depending on it become open or are
recomputed. Runtime observations can contradict a closed result and force
investigation; failure to observe an event cannot prove its absence.

## Counterexamples that composition must reject

| Invalid result | Why it is unsound | Current regression evidence |
|---|---|---|
| A callback writes and throws, but the enclosing graph reports the old state or runs a later normal event. | The callback's pre-throw write is observable; normal continuation is unreachable on that path. | `callback_throw_preserves_prior_write_and_skips_later_events` |
| A callback has normal and throw outcomes, but their result/state correlations are flattened into one certain result. | This invents a result on the throw path or loses a possible exit. | `callback_normal_and_throw_paths_keep_separate_results_and_states` |
| A branch after an invocation uses the pre-call state or treats an unsupported host predicate as false. | Callback mutations precede the branch; an undecidable predicate must retain both paths. | `host_predicate_observes_callback_state_after_invocation`; unknown host predicates return `Maybe` in the reference host |
| A pending throw enters cleanup, then cleanup returns, while the evaluator still reports the original throw. | The cleanup return overrides the pending completion. | `pending_callback_throw_runs_cleanup_then_resumes`; `cleanup_return_overrides_pending_throw` |
| A native unwind is reported as a PHP throw. | It crosses a different language/ABI boundary and does not establish PHP exception state. | `native_unwind_remains_distinct_from_php_throw` |
| An unresolved return is treated as no return or as a closed empty result. | Incomplete evaluation cannot justify excluding a return behavior. | `unresolved_return_is_an_incomplete_evaluation_not_no_return` |
| A graph labels a domain closed while containing an unknown transfer that affects it, or supplies no coverage/evidence. | Its negative claims are unsupported by the actual transfer graph. | `unknown_transfer_cannot_be_marked_closed_or_reasonless`; `every_domain_requires_a_coverage_decision_and_evidence` |
| A closed wrapper invokes an open child but retains its own closed return coverage, or omits child coverage entirely or by domain. | The wrapper's result may depend on child behavior, so child uncertainty must compose into the result and incomplete metadata must not default to closed. | `open_child_coverage_opens_a_closed_parent_domain`; `incomplete_child_coverage_rejects_the_evaluation`; `omitted_child_coverage_rejects_the_evaluation` |
| A maybe-applicable target graph is selected as definitely applicable, or its uncovered remainder is dropped. | The target set includes configurations whose behavior was not established. | `target_selection_keeps_unknown_remainder_and_selects_exact_scenario`; target-module correlated-scenario tests |

These tests check structural behavior in a small evaluator host. They do not
establish that the checker host, native transfers, or every PHP runtime obeys
the contract. Before F00 closes, add composition tests for state guards across
joins, return refinement that drops effects/throws, dependency-incomplete
projections, and mismatched target scenarios; then obtain independent review.
