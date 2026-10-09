# Phase F00 contract review

Status: **self-review complete for this pass; independent review outstanding**.
F00 remains open. This review compares the working contract with the shared
evaluator API, catalog validation, current semantic-host tests, and the pinned
PHP 8.5.4 `strlen` source/runtime evidence. It is not an independent review and
does not certify the implementation.

## Findings

### F00-R1 — Assumptions do not have a validated identity or record

The contract and plan require assumptions to be first-class identities whose
scope and validity can be checked against the selected target and loaded
program. The current manifest has `Digest` vectors on `DomainCoverage` and
`EvidenceRecord`, but no separate assumption-record table. In
[`catalog.rs`](crates/php-semantics/src/catalog.rs),
`CatalogManifest::validate` looks up domain `assumptions` in the evidence ID
list, conflating assumptions with evidence. `EvidenceRecord.assumptions` are
not resolved there at all. An opaque digest therefore does not establish
what was assumed, where it applies, or whether the active target satisfies it.

The contract now specifies the intended record: typed kind, claim, scope,
prerequisite assumptions, supporting evidence, validator identity/version,
and a digest over canonical encoding. Applicability is derived for the active
target/program context; unknown facts or validators cannot establish it. F00
has specified this obligation. F01 must implement the record, distinct
reference namespaces, acyclic dependency checks, and context validation before
schema work is complete.

### F00-R2 — The evaluator returns projections, not an explicit trace set

The contract now states this limitation directly. `Evaluation` returns exits,
abstract states/results, coverage, and a replay certificate; it does not
materialize ordered observation traces. A host must retain the observable
facts needed by every closed domain and query projection. The contract has
been clarified to require that responsibility and to forbid using open
coverage as a replacement for a sound state transfer.

The PHP-specific unknown-callback fixture verifies havoc of a tracked local,
five distinct exit kinds, and open effect coverage. It does not establish a
generic guarantee for arbitrary hosts. Any host-specific closed claim still
needs a transfer/order fixture and evidence for its observation projection.

### F00-R3 — The `strlen` source/runtime evidence supports invocation, not full callback semantics

The builder pins `zend_API.c` parser gates and checks that
`zend_std_cast_object_tostring` invokes `ce->__toString`; the runtime witness
confirms the callback sentinel in both Hybrid and CALL profiles. This supports
the existence and order of a positive callback path for the tested profile.
The callback witness is not evidence that all callbacks return normally,
avoid writes, or avoid throwing. The graph correctly keeps effect and
exceptional domains open, but checker consumers must preserve that limitation.

The checker host's `Invoke(__toString)` transfer returns `string` on its
modeled normal path. Its open throw/effect coverage means that result must not
be read as a no-throw or no-effect claim. The call invalidation bridge currently
uses the recorded invocation to run the existing string-operation transfer;
it does not consume general open effect state. That is an F05 integration
obligation, not evidence that callback behavior is closed.

### F00-R4 — The host's `Stringable` predicate is conservative only under its registry invariants

The checker host takes the invocation branch for registered `Stringable`
subtypes, rejects a known final non-Stringable class, and keeps both branches
for mixed or extensible unknown objects. PHP 8+ class registration adds the
implicit `Stringable` interface for classes declaring `__toString`, and
fixtures cover explicit and implicit Stringable cases plus strict mode and a
non-Stringable negative control. This establishes the current checker model's
intended partition for those cases; registry soundness and invalid/incomplete
class declarations remain assumptions that require independent review.

## F00 subtask disposition

| ID | Status | Disposition |
|---|---|---|
| F00.1 | Partial | Observable-trace, order, return, host, and assumption-identity semantics are specified. Independent acceptance remains outstanding. |
| F00.2 | Complete | The contract's listed invalid-composition counterexamples map to passing evaluator/target tests; the locked CBMC replay is recorded in the evaluation ledger. |
| F00.3 | Partial | Narrow `strlen` source/runtime alignment is checked. General host/source transfer validation and the callback effect/throw boundary remain open. |
| F00.4 | Complete | Contract specification defines typed assumption records, canonical IDs, distinct evidence references, acyclic dependencies, and context-dependent validation; implementation remains F01.1. |
| F00.5 | Open | Independent review of the contract, assumptions, abstract order, and return-refinement rules has not occurred. |

## Review checklist for an independent reviewer

- Does the abstract-order language cover values, state, pending outcomes, and
  all observable projections a closed domain may expose?
- Are normal returns, exceptional exits, cleanup replacement, suspension, and
  nontermination prefixes composed without losing a concrete trace?
- Can any `Coverage::Closed` claim be consumed when its assumptions are
  missing, inapplicable, or invalidated?
- Do unknown and partially modeled transfers conservatively widen every
  affected state/result component, including across joins and certificates?
- Are the current PHP-host `strlen` predicates and event transfers sound for
  the exact source/build/profile facts they claim, including strictness,
  implicit `Stringable`, callback writes, and callback throws?

The review cannot be marked complete by the author of this self-review. Until
an independent reviewer accepts or amends the contract, dependent gate work
must not be treated as closed.

## Verification performed for this review

- `cargo test --profile gate -p phpstan-rs-semantics`: 60 passed.
- `cargo test --profile gate -p phpstan-rs-semantics --example build_strlen_catalog`: 5 passed.
- Rebuilt the locked `strlen` artifact from pinned PHP source, LLVM inventory,
  and runtime evidence; output matched the checked-in JSON byte for byte
  (SHA-256 `b55221654574043a15d50dfd862377863a39e2272b5aab148c7da35b74d1e33b`).
- The earlier full checker-fixture run and its remaining failures are recorded
  in the evaluation ledger; they are not evidence that F00's semantic
  assumptions have been independently accepted.
