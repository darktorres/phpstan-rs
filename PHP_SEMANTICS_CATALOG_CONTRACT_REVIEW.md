# Phase F00 contract review

The follow-up [adversarial review pass](#adversarial-review-pass-same-author-not-independent)
records additional issues. It is also author-led and does not close the
independent-review requirement.

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

## F00 status

F00 subtask statuses and remediation actions are maintained in the single
[Phase F task and remediation ledger](PHP_SEMANTICS_CATALOG_LEDGER.md).

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

---

## Adversarial review pass (same author; not independent)

Review date: 2026-10-09
Review type: author-led adversarial pass; **not an independent review**
Disposition: contract remains a working draft; F00.5 and R00.1 remain open.

This review examines the questions in the F00 checklist against
[the contract](PHP_SEMANTICS_CATALOG_CONTRACT.md),
[the prior contract self-review](#phase-f00-contract-review),
the plan's Phase F gate, the evaluator/catalog APIs, and the recorded CBMC
artifacts. It identifies issues for the independent reviewer and proposes
dispositions; it does not amend the contract or claim that the same author can
independently accept it.

### Findings

#### ACR-1 — Abstract-to-concrete soundness is stated, but not defined precisely

The contract requires abstract results to contain all concrete behavior
relevant to closed domains and makes joins, widening, predicates, and transfers
semantic obligations. It does not define the relation by which an abstract
state, value, event sequence, or exit represents a set of concrete PHP states
and traces. As a result, “contains” and “sound transfer” remain obligations
without a shared mathematical or executable interpretation. The evaluator's
host-supplied `join_*`/`widen_*` checks can check the host's own containment
operation, but that alone cannot establish the meaning of containment in PHP.

The contract should either define a concrete-to-abstract representation
relation and the required simulation conditions (initial states, each event,
branch/join/widening, and exits), or explicitly scope the gate to validated
finite domains with independent differential evidence and state what that
evidence does not prove. It should also say how state/result/exit correlations
survive projection and joining. This is a blocking contract question, not a
request to add a full theorem prover.

#### ACR-2 — Declared domain dependencies can be incomplete without detection

The contract says every result names the domains needed to justify it, and
that open dependencies widen dependent results. The schema/evaluator can
validate the dependencies that were declared, but the correctness claim also
depends on the producer having listed every domain that can affect that result.
For example, a return may depend on callback writes or throws even if the
producer omits those dependencies. The validator cannot infer that omission
from the list itself; a self-consistent but incomplete dependency graph may
still validate.

The independent reviewer should require an explicit trust rule for dependency
completeness: for example, a reviewed transfer contract that declares the
dependencies for each operation kind, with exhaustive operation coverage and
negative tests that remove required edges. Until then, certificate replay
proves closure relative to the submitted graph and host, not completeness of
the graph's semantic dependency declarations.

#### ACR-3 — Separate acyclic graphs may still have a cross-namespace cycle

The contract requires evidence and assumption IDs to be distinct and says
both dependency relations must resolve and be acyclic. It also allows an
assumption to cite supporting evidence and an evidence record to cite
assumptions. Those two graphs can each be acyclic while the combined support
relation is circular: assumption A is justified by evidence E, while E is
declared applicable only under A. The current catalog implementation does not
yet have typed assumption records, so it cannot detect or reject this case.

Clarify whether cross-namespace support cycles are forbidden or have a
well-founded root rule, then validate the combined typed dependency graph. A
separate trust question also remains: a validator ID/version is part of an
assumption record, but the contract does not specify which validators are
trusted, how that trust is rooted, or how validator revocation/version changes
invalidate prior records.

#### ACR-4 — Cleanup replacement is specified for only some completion kinds

The contract distinguishes normal return, PHP throw, fatal exit, suspension,
and native unwind, and says cleanup can run with a pending exit. It explicitly
describes cleanup return/throw replacing the pending completion, but does not
state the composition rule for every modeled cleanup outcome against every
pending outcome. This leaves questions around cleanup fatal exit, suspension,
or native unwind, and around cleanup entered for a pending completion other
than PHP throw. The implementation may conservatively leave such cases open,
but the contract should state that fallback rule rather than imply a complete
completion algebra.

Add a pending-completion × cleanup-completion matrix for the modes represented
by the evaluator, with source/runtime support where the rule is PHP-specific.
Unmodeled cells must remain open and must not silently resume normal execution
or preserve a superseded completion.

#### ACR-5 — The CBMC result is useful reduced-model evidence, not PHP contract validation

I parsed the committed result artifacts: `cbmc-contracts.out` contains 93
successful properties; `cbmc-invalid-composition.out` contains 93 successful
properties and one expected failure at `main.assertion.11`. The failure checks
that a C callback write/throw is not erased by an invalid C wrapper assertion.
The harness itself labels these as independent reduced contracts, and models
callbacks, events, aliases, and a pending flag in C. It does not instantiate
the PHP evaluator, derive its transition rules from PHP source, or establish
the soundness of PHP host transfers. The run is bounded with `--unwind 4`,
though the harness constrains its example loop to at most three iterations.

Keep F00.2 complete only for the recorded reduced counterexamples. For a
reviewed contract claim, map each property to a contract clause and identify
which parts are C-only illustrations versus properties also checked by Rust
evaluator fixtures or PHP runtime evidence. The recorded results were
inspected, not freshly rerun here; `cbmc` is not installed on this worktree's
PATH.

#### ACR-6 — Counterexample coverage does not establish host-transfer soundness

The contract lists strong structural regressions for path correlation,
exceptional continuation, joins, widening, open coverage, and target
remainders. The tests show the current evaluator rejects those named bad
compositions. They do not show that arbitrary host operations preserve every
observable effect, or that each producer has supplied all relevant graph
events and dependency edges. The prior self-review already recognizes that its
unknown-callback havoc check is host-specific.

Retain the counterexamples as necessary regression evidence, but do not use
their count or CBMC's passing property count as a completeness argument. F00.3
and F01.5 need a source-to-host matrix for every fact that will be marked
closed, with positive, negative, and contradiction cases.

### Checklist disposition

| Independent-review question | Adversarial-pass disposition |
|---|---|
| Does abstract order cover values, state, pending outcomes, and observable projections? | Partly specified; the abstract-to-concrete representation/simulation relation is missing (ACR-1). |
| Are normal returns, exceptional exits, cleanup replacement, suspension, and nontermination prefixes composed without losing a concrete trace? | Core distinctions are specified, but cleanup combinations are incomplete and prefix preservation under widening lacks an explicit representation relation (ACR-1, ACR-4). |
| Can a closed claim be consumed with missing, inapplicable, or invalidated assumptions? | Intended behavior is specified, but typed records and context validation are not implemented; cross-namespace cycles and validator trust are unresolved (ACR-3). |
| Do unknown and partially modeled transfers conservatively widen every affected component? | This is a stated obligation with selected host tests; completeness of affected-domain/dependency declarations is not established (ACR-2, ACR-6). |
| Are PHP-host predicates and event transfers sound for the claimed source/build/profile? | Narrow `strlen`/Stringable evidence exists; it does not validate all host transfers or all callback outcomes (ACR-5, ACR-6; see F00-R3/R4 in the prior review). |

### Proposed dispositions and remaining acceptance

1. Amend the contract to resolve ACR-1 through ACR-4, or explicitly record why
   the independent reviewer accepts the current limits for the Phase F gate.
2. Keep the CBMC evidence scoped to the reduced C model and maintain its
   property-to-clause mapping; do not describe it as PHP semantic validation.
3. Keep F00.3/F01.5 open until source-derived host obligations and contradictory
   cases are reviewed for the actual closed slice.
4. Have a reviewer other than this author assess this report, the contract, the
   underlying implementation/evidence, and the proposed dispositions. Record
   that review and any amendments before closing R00.1.

No finding is marked accepted by an independent reviewer. R00.1 remains
**Blocked** in the remediation ledger until item 4 is completed; F00.5 remains
**Open** in the Phase F status ledger.

The fix sequence for each ACR finding is recorded in the
[authoritative Phase F and remediation ledger](PHP_SEMANTICS_CATALOG_LEDGER.md):
ACR-1/4 map to R00.2 and R01.5; ACR-2 to R00.3, R01.6, and R04.1; ACR-3 to
R00.2, R01.1, and R01.7; ACR-5 to R00.4; and ACR-6 to R00.3, R02.3, and
R04.1. Each row includes dependencies and acceptance evidence.

---

## Independent review record (2026-10-09)

**Reviewer:** a fresh Claude session (Haiku 5.5), started at the maintainer's request to
review the F00 contract and the ledger claims that depend on it. It did not write the
contract, either self-review, or the ledger. It is a model, not a human reviewer, and it
shares a model family with the authoring work. Whether this satisfies R00.1's "someone
other than this author" requirement is for the maintainer to decide. A human review is
still needed before F00 closes.

**Scope:** the contract, the two self-reviews, and every ledger claim that touches F00,
F01.1–F01.4, F02.2, F02.4, F03.1, F05.2, F05.4–F05.6, F06.2, and R02.1–R06.4. Code read:
`crates/php-semantics/src/{lib,catalog,target}.rs`, the strlen bridge in
`crates/phpstan-rs/src/checker/{expr/builtin_calls,call_effects}.rs`, `calls.rs`, and the
evaluation tools (`campaign.py`, `run.py`, `evaluate_strlen_svf_demand.py`). Artifacts
read: `php-8.5.4-strlen.json`, the CBMC `.out` files, the strlen LLVM inventory, and the
runtime witness.

**Method:** no repository code was changed and nothing was committed. `cargo test -p
phpstan-rs-semantics` (debug) gives 60 passed, matching the author's count. Behavior
claims were checked with a scratch crate outside the repository, which depends on
`phpstan-rs-semantics` by path and calls only the public API (`reviewer-probe`, in the
session scratchpad). Probe outputs are quoted below.

### Confirmed as stated

- All 29 counterexample tests named in the contract table exist. I checked existence and
  read the cleanup and fatal tests; I did not audit every assertion.
- The locked artifact hash `b5522165…` matches the checked-in JSON. Its only entry is
  release-only (8.5.4), so F05.6 is accurate.
- The artifact has no coverage or evidence assumptions, so the contract's "no producer may
  rely on a non-empty assumption vector" rule is not violated by the locked data.
- The strlen LLVM inventory has 241 instructions, and its group counts sum to 241.
- CBMC: `cbmc-contracts.out` has 93 successful properties and no failures;
  `cbmc-invalid-composition.out` has 93 successes plus one failure, `main.assertion.11`.
  The ledger's "93" and "94" are correct.
- The runtime witness has 13 sentinels, and its Hybrid/CALL output hash is `2bd72db6…`.
- The cgroup record shows the 10 GiB limit (F03.1, F06.2).
- The merge of coverage in `merge_coverage` (lib.rs:889) matches the contract's rule: open
  wins, the least precise result is kept, and a not-applicable child adds no fact. The gap
  is in dependency traversal (RV-2), not in the merge itself.

### Findings

Severity: **active** means the finding affects the locked artifact, current checker output,
or evidence integrity today. **Latent** means it is reachable only by graphs or hosts not
yet produced.

**RV-1 — Latent, High. Cleanup `Return` and `Throw` replace a pending fatal, suspension, or
native unwind.** In the evaluator, `Terminator::Return` (lib.rs:1734) pushes a normal exit
without looking at `pending`. `Terminator::Throw` (lib.rs:1744) sets the pending outcome to
`PhpThrow` regardless of what was pending. Probe results, using `evaluate` with a minimal
host:

```
pending Fatal -> cleanup Return : Ok([Normal { value: Some("cleanup-return"), .. }])
pending Fatal -> cleanup Throw  : Ok([PhpThrow { value: "cleanup-throw", .. }])
pending NativeUnwind -> cleanup Return : Ok([Normal { value: Some("cleanup-return"), .. }])
```

The contract says native unwind "must not be silently converted into a PHP throw or a
normal return" (lines 58–60). Its table row at line 123 says a fatal exit must not be
collapsed into a PHP throw or normal return. The cited regression test,
`fatal_and_suspension_remain_distinct_exits`, uses no pending state. No test covers a
cleanup block with a pending fatal, suspension, or native unwind. ACR-4 says these cells
are "incomplete". They are actively wrong.
*Required:* state the rule for each pending-kind × cleanup-terminator cell. PHP cannot
return over a fatal error. Suspension is unspecified in the contract. Native unwind may not
become a normal return or PHP throw. Fail closed on every cell that is not modeled, and add
a test per cell, before any cell is claimed. *Ledger:* R01.5; F01.2 and F00.2 downgraded.

**RV-2 — Latent, High. The closed-dependency closure is bypassed through a `NotApplicable`
intermediate.** `validate_domain_dependencies` (lib.rs:532–592) and
`propagate_open_dependencies` (lib.rs:837) follow only edges from a closed domain to another
closed domain. Probe P1, Return(Closed) → Diagnostic(N/A) → UserWrite(Open), is accepted.
`project(Return)` then reports `coverage=Closed` with `dependencies=[UserWrite, Diagnostic]`,
so a closed result carries an open dependency. Contract lines 47–51 ("a closed result
cannot depend on an open domain"; propagation "through these dependencies") are violated
transitively. Probe P2, a closed domain depending on a N/A domain with nothing open, is also
accepted; the contract says nothing about that case (C-2).
*Required:* traverse every dependency edge in both validation and propagation. Decide,
and write down, whether a closed domain may depend on N/A at all. If it can, the N/A node's
own dependencies must be checked too. *Ledger:* R01.4.

**RV-3 — Latent, Medium. Closed domains can declare no dependencies with no justification
recorded; the locked `Return` does this.** Probe P3 shows that a path containing a closed
`Write` is accepted with `Return` closed and `deps=[]`, so the validator does not require
declared edges to match the events. In the locked artifact every one of the 14 coverage
entries has empty `dependencies`. `Return` is closed while `Invocation`, `PhpThrow`,
`Fatal`, `Suspension`, and `UserWrite` are open. Reading the graph: `Return` is
`StringByteLength(0)` with a nonnegative range, and the `Invoke` result (`result: 0`) is not
what it returns. On that reading, a normal strlen return is a nonnegative int whatever the
callback does, so an empty list may be defensible. But nothing in the artifact, contract, or
ledger records that argument, and the contract requires a closed return refinement to declare
such dependencies (lines 74–78).
*Required:* a recorded justification for each closed domain with an empty dependency list,
and the trust rule ACR-2 asks for, so that required edges are derived from events. *Ledger:*
R00.3, R01.6.

**RV-4 — Latent, Medium. The assumption-record rules are not enforced, and the ledger
overstates F00.4.** `CatalogManifest::validate` checks `coverage.assumptions` against the
evidence namespace (catalog.rs:179), and `add_variant` does the same (catalog.rs:266–272).
Probe P4a accepts an evidence ID as a coverage assumption. Probe P5 accepts a manifest whose
`EvidenceRecord.assumptions` names an unknown digest, because those are never resolved.
The contract says "no producer may rely on a non-empty assumption vector" (lines 106–109),
but `add_entry` accepts non-empty vectors. F00.4 is titled "validated" and its own text says
the implementation belongs to F01.1, so "Complete" overstates it.
*Required:* reject non-empty `coverage.assumptions` and `EvidenceRecord.assumptions` now,
until F01.1 provides typed records. *Ledger:* F00.4 downgraded to Partial; R01.1.

**RV-5 — Latent, Low. Ordering: unordered vectors change identity.** Probe P6 gives two
different `EvidenceRecord` IDs for `[a,b]` and `[b,a]`. `CatalogManifest::validate` does not
check order (catalog.rs:136–194), and `canonical_digest` hashes producer order (catalog.rs:128).
*Ledger:* R01.8 confirmed as written.

**RV-6 — Low (no soundness effect). Evidence-dependency cycles cannot be formed.** An
`EvidenceRecord`'s ID is the SHA-256 of its encoding, which includes its dependency IDs, and
`validate` recomputes every ID (catalog.rs:169–176). A cycle would need a SHA-256 fixed
point. The existence check (catalog.rs:186–192) is therefore sufficient for evidence. The
real cycle risks are assumption prerequisites and cross-namespace support (ACR-3), which
exist only once assumption records exist. *Ledger:* R01.2 narrowed.

**RV-7 — Low. Inverted release ranges are accepted and silently treated as non-applicable.**
Probe P7 accepts an inverted `TargetPredicate.release`. `TargetPredicate::test` answers No
or Maybe for it, never Yes, so there is no false applicability. Range validity is checked
only for `TargetScenario` (target.rs:67), never for predicates. *Ledger:* R01.3 confirmed as
written; it is a validation gap, not a soundness gap.

**RV-8 — Active, Medium. The CBMC campaign does not fail closed, and one run is unclassified.**
`complementary()` (campaign.py:46–56) calls `runner.run` for the three CBMC steps without
checking the result. `run()` returns `exit_code == 0` (run.py:95), and CBMC exits nonzero on
expected failures, so these steps can only record output. The layout steps do assert. The
`cbmc-low-budget` run (`loop_contract.c`, `--unwind 2`) has one pass and one failure,
`main.unwind.0`, an unwinding assertion. It is a plausible negative control, but the ledger
and evaluation never mention it. *Required:* assert each step's expected status, and map
`cbmc-low-budget` in R00.4. *Ledger:* R00.4.

**RV-9 — Active, Medium. The Stringable predicate answers No for object types it does not
recognise.** In `strlen-weak-stringable-cast` (builtin_calls.rs:111–126), a member is treated
as "may cast" only if `as_object()` matches. `as_object` matches only `Repr::Object` and
`EnumCase`. A bare `object` is `Repr::ObjectWithoutClass` (constructors.rs:141), so it falls
into the `else` arm, `may_not_cast = true`, and the predicate returns No. The contract says
a predicate answers `Maybe` unless the host proves the answer (lines 80–83). A plain `object`
can be Stringable, so this answer is unproven. The consumer's effect is small today: a
covered call only forgets the String operation on its argument expression
(call_effects.rs:405–415). So the impact is latent, but the host violates its own rule. The
ledger's F00-R4 wording ("keeps both branches for mixed or extensible unknown objects") is
inaccurate for `object`. This was established by reading the code, not by a PHP run.
*Required:* return `Maybe` for `ObjectWithoutClass`, `ObjectShape`, object-bearing
intersections, and object-bounded templates, with a fixture for each. *Ledger:* R05.8 (new).

**RV-10 — Active, Medium. Spread arguments: the mechanism differs from the ledger text.**
`resolve_call_args` skips spread arguments (calls.rs:956), so for `strlen(...$x)` the graph's
argument 0 is `Type::mixed()` (builtin_calls.rs:2301). The invalidation step
(call_effects.rs:405–412) indexes the raw call arguments, so index 0 invalidates the spread
source expression, not the unpacked element. A Stringable element's callback side effects
are therefore never attributed. The ledger's description is right about the outcome but
wrong about the path. Not run in a PHP probe. *Ledger:* F05.4 text corrected; R05.2
acceptance should include the spread case.

**RV-11 — Active, Medium. The runtime witness no longer describes the checked-in fixture.**
`results/runtime/hybrid-call-8.5.4.json` records the `runtime.php` digest `9b38a3ea…`, from
commit `043f2c78`. The checked-in `runtime.php` was changed in `644350cf` and hashes to
`94dc6509…`. `evaluate_strlen_svf_demand.py` checks only that the sentinel string is present
(lines 103–108), and records both digests without comparing them (lines 173–174). F02.4 says
the witness was "independently replayed against the historical fixture bytes". That is
accurate, but the witness does not describe the fixture in the tree. *Ledger:* R02.1 (instance
added); F02.4 note.

**RV-12 — Active, Medium. The SVF demand replay hard-codes the PHP source commit.**
`evaluate_strlen_svf_demand.py` sets `source_commit = "e5e09d06…"` (line 103) and writes it
as `php_source_commit` (line 157). It does not check that the supplied bitcode came from
that source. *Ledger:* R03.4 confirmed as written.

**RV-13 — Latent, Low. The pool-admission wait can be skipped.** `campaign.py` waits only
while the report file exists and has no `exit_code` (lines 95–98). The loop also breaks when
the cgroup path disappears without an `exit_code`, which admits the next job. *Ledger:* R06.4
(second hole added).

### Contract points

- **C-1 (ACR-1).** "Contains", "sound", and "projection" have no defined relation to
  concrete PHP traces. Agreed, and blocking for any "sound" claim. Either define the
  representation relation or restate the claim as a scoped invariant.
- **C-2.** The contract is silent on dependencies to N/A domains (RV-2).
- **C-3.** The table row at line 128 ("without a fixed-point certificate") suggests a
  certificate could justify a closed cycle. The code rejects every closed cycle
  (lib.rs:586–590, and the cyclic test). The text should say so.
- **C-4.** "According to PHP control flow" (line 58) is undefined for fatal, suspension, and
  native unwind (RV-1).
- **C-5.** The closed-return dependency rule (lines 74–78) cannot be checked until ACR-2's
  trust rule exists (RV-3).
- **C-6.** The assumption-applicability rule (lines 97–104) is a stated rule with no code
  behind it (RV-4).

### Disposition of ACR-1 to ACR-6

- **ACR-1:** confirmed; blocking for "sound" wording (C-1).
- **ACR-2:** confirmed and strengthened by RV-2 and RV-3.
- **ACR-3:** confirmed as forward-looking; no current code path can form the cycle. Accepted
  as F01.7 work, not a present defect.
- **ACR-4:** confirmed, and worse than described. The evaluator performs the conversions the
  contract forbids (RV-1).
- **ACR-5:** accepted as stated. The `cbmc-low-budget` mapping is added (RV-8).
- **ACR-6:** accepted as stated. RV-9 and RV-10 are concrete examples of host-level gaps.

### Not verified by this review

- F05.7: the 8 PHPDoc fixtures and the 4 library tests exist (checked by name), but I did not
  run the optimized suites or check the baseline attribution.
- F01.2, F01.3, F04.1: test names exist. I did not audit each assertion beyond the cleanup
  and fatal tests.
- F02.1, F03.2, F03.3, F06.1, F06.3–F06.5: not checked.
- `is_null()` on template parameters, which bears on the null predicate's treatment of them.
- The observable PHP-level impact of RV-9 and RV-10: no PHP run was made.

### Reviewer disposition

No finding is accepted. Each one needs an author amendment or an explicit acceptance by the
maintainer. The ledger has been updated to match: R00.1 moves from Blocked to Open (review
recorded, findings open). F00.2, F00.4, and F01.2 move from Complete to Partial, for RV-1 and
RV-4. Rows R01.2, R01.3, R01.4, R01.5, R01.6, R02.1, R00.4, R06.4, R05.8 (new), and F05.4
carry the findings. F00 stays Open.
