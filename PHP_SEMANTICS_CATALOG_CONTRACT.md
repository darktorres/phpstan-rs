# PHP behavior catalog semantic contract (working draft)

This draft makes the plan's correctness rule testable for the first slice. It
is not a formal proof. An independent adversarial review is recorded below;
its findings remain open, so F00 stays open until the contract and its
assumptions are resolved or explicitly accepted.

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

## Host-relative representation and soundness claim

For a selected graph, target scenario, assumptions, and concrete entry
configuration, let `B` be the set of concrete PHP executions admitted by those
assumptions. An execution consists of its ordered observable trace and its
completion (normal, PHP throw, native unwind, fatal, or suspension), including
the exit state and any value defined for that completion. A host supplies
abstract values and states. Their concretizations, written `gammaV(v)` and
`gammaS(s)`, are the concrete values and states each abstract object denotes.
Those functions are part of the host's semantic contract; they are not
currently represented or checked by the Rust API.

For the state/value/exit projections the API exposes, an abstract frame denotes
the product of its state's concretization, the concretizations of its defined
result values, and its pending completion. The set of frames at a block denotes
their union. This deliberately permits spurious combinations after
component-wise joins: adding behaviors is safe, while dropping a represented
state, result, pending completion, or correlation needed by a closed
projection is not. A host may preserve extra correlations in its state
representation; the evaluator does not infer those correlations from equal
block IDs or result IDs. The API does not return explicit trace sets. Any
ordered observation claimed closed must be represented in the host state or
by an explicit trace component in a future API; evidence and graph identity
alone cannot establish that an observation occurred or did not occur. The
current API makes no claim that it represents every finite observable trace
prefix.

The required simulation obligations are conditional on `gammaV` and `gammaS`:

1. For every concrete entry tuple admitted by the selected context, its
   arguments and receiver are in the concretizations of the supplied
   abstract arguments and receiver, and its state is in the concretization
   of the supplied initial abstract state.
2. For each represented input and each concrete transition allowed by an
   event, the host produces an abstract normal or exceptional outcome whose
   state, result, and completion contain that transition. Every observable
   effect must be reflected in the host state; there is no independent graph
   observation escape hatch. A transfer that cannot establish this must
   conservatively over-approximate it and leave affected domains open.
3. `Yes` and `No` predicates hold for every concrete input tuple represented
   by the frame (arguments, receiver, results, and state). `Maybe` retains
   every feasible successor. A path guard may refine a state only when that
   refinement satisfies the same containment obligation.
4. For values, `gammaV(x) union gammaV(y)` is a subset of both
   `gammaV(join_value(x,y))` and `gammaV(widen_value(x,y))`; the analogous
   state law uses `gammaS`, `join_state`, and `widen_state`. The host's
   subsumption answers must agree with these containment relations. These laws
   apply separately to normal results, each kind of pending completion, and
   exit states; different completion kinds must not be merged into one kind.
5. Every graph successor and every observable exit is represented by the
   evaluated frames and exits. A projection is sound only if it retains all
   trace/state components and dependency domains that can affect the queried
   fact. Removing a reported component is not permission to remove its
   influence from the represented state.

Together, obligations 1–5 and successor closure give a finite-path argument
for the state/value/completion projections only. The base case is obligation
1: every admitted entry configuration is covered by the initial frame. For
the induction step, assume a concrete configuration after `n` graph
transitions is covered by a frame. Obligation 2 covers each concrete event
transition from that configuration; obligation 3 keeps every successor whose
guard is not proved impossible; certificate successor closure requires every
resulting abstract successor to be covered by a solution frame. Therefore the
configuration after `n+1` transitions is covered. By induction, every finite
reachable graph configuration is covered. At a terminating configuration,
obligation 5 and replayed exit inclusion cover its completion and projected
exit state/value. This argument does not establish trace-prefix coverage:
the current API has no explicit trace component, so a trace-dependent claim
must either be encoded in the host state or remain unclaimed.

Result definitions are partial maps from result IDs to abstract values. An
ID absent from a frame is undefined on every concrete configuration denoted
by that frame; absence does not mean an arbitrary or unknown value. Joining
frames with different result-ID sets would make that meaning ambiguous, so
the evaluator rejects a result defined on only one predecessor path. A host
that needs to continue such paths must preserve them as separate frames until
the value is defined on both, or model an explicit defined/undefined sum in a
future schema.

Cleanup is part of the same simulation obligation, not an exception to it. A
normal cleanup continuation that resumes a pending completion preserves that
completion and its accumulated state. A cleanup return or throw may replace a
pending PHP throw only where that PHP behavior is modeled and tested. The
replacement rules for pending fatal, suspension, and native-unwind completions
are currently unspecified: such combinations must be rejected or left open
until a rule and regression evidence exist. In particular, they may not be
silently converted to a normal return or PHP throw.

This cleanup requirement is normative, but the evaluator currently violates
it for cleanup `Return`/`Throw` with pending fatal, suspension, and
native-unwind outcomes;
those executions are accepted and converted. The rule is defined here, while
conformance remains open under RV-1/R01.5. R00.2 closes only the definition,
not the evaluator's implementation.

The evaluator and post-fixpoint certificate validator check a narrower,
host-relative invariant: under the supplied host's operations and
`*_subsumes` answers, the recorded initial frame is covered, every replayed
successor is covered, and every replayed exit is included. They do not
establish the host's concretization, predicate correctness, event-transfer
soundness, or the compiler/runtime's correspondence to the graph. Therefore
Phase F may claim a structurally validated post-fixpoint for a named host and
artifact, but may claim PHP semantic soundness only for a slice whose host
simulation obligations have separate source/runtime evidence and review. The
current generic API alone proves no PHP-soundness theorem.

Evidence records identify artifacts or observations; they do not make those
artifacts authoritative. Assumptions state the conditions under which a claim
is intended to apply. A trusted validator must be identified by immutable
implementation/version, its trust root and revocation policy must be explicit,
and its supporting evidence must be validated for the selected target and
loaded program. Evidence and assumption support form one dependency graph for
trust analysis, even though their identifiers remain separate namespaces.
Until the schema validates that graph and the trusted roots, a manifest can
report provenance but cannot establish semantic trust.

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
continuation may run for a pending exit. Resuming cleanup preserves the pending
completion. The only replacement rule currently modeled and tested is a
cleanup return or throw replacing a pending PHP throw. The replacement rules
for pending fatal, suspension, and native-unwind outcomes remain unspecified;
the evaluator must reject those combinations or keep the affected result open.
Native unwind must not be silently converted into a PHP throw or a normal
return.

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

Schema v8 stores typed assumption records separately from evidence. Each record
has a schema-bound content ID, claim kind and text, target/program scope,
prerequisite assumption IDs, supporting evidence IDs, and validator ID/version.
Manifest validation resolves assumption references only in the assumption
namespace, requires evidence assumptions to propagate into coverage, and
rejects malformed scopes or records. Context matching alone does
not establish an assumption: until trusted validator execution and
cross-namespace cycle checks are implemented, graph validation rejects a
closed fact that cites an assumption. This applies even when a graph bypasses
the catalog loader. Open facts may retain assumptions as provenance without
treating them as established. See R01.2 and R01.7 for the remaining graph and
trust rules.

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
| A fatal exit or suspension is collapsed into a PHP throw or normal return, including through cleanup. | These are distinct observable completion modes and carry different continuation rules. The no-cleanup cases pass; pending-fatal cleanup cases are currently accepted incorrectly and covered by ignored regressions. | `fatal_and_suspension_remain_distinct_exits`; ignored `cleanup_return_does_not_replace_a_pending_fatal`, `cleanup_throw_does_not_replace_a_pending_fatal`, and `cleanup_return_does_not_replace_a_pending_native_unwind` |
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
