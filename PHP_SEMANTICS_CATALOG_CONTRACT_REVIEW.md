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

**Reviewer:** a fresh Claude session, started at the maintainer's request to review the F00
contract and the ledger claims that depend on it. The maintainer reports that the ledger was
being worked by Codex, and the reviewer did not write the contract, either self-review, or the
ledger, so the reviewer is independent of that work. It is a model, not a human reviewer. Whether a human should sign
off on the contract's semantics before F00 closes is for the maintainer to decide.

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

**RV-9 — Active, Medium, reproduced. The Stringable predicate answers No for object types it does not recognise.** In `strlen-weak-stringable-cast` (builtin_calls.rs:111–126), a member is treated as "may cast" only if `as_object()` matches. `as_object` matches only `Repr::Object` and `EnumCase`. A bare `object` is `Repr::ObjectWithoutClass` (constructors.rs:141), so it falls into the `else` arm, `may_not_cast = true`, and the predicate returns No. The contract says a predicate answers `Maybe` unless the host proves the answer (lines 80–83). A plain `object` can be Stringable, so this answer is unproven.

The consequence is observable. A covered call re-computes the String operation's effect on the argument's type (`forget_what_an_operation_may_change`, call_effects.rs:530). For a bare `object` the predicate answers No, so the `__toString` branch is never taken and nothing is forgotten. PHP probe (`probe.php`, gate binary rebuilt at `09acd24e`), each function running `$c->p = 1; strlen($o); assertType('1', $c->p);`:

```
rv9_object_param(object $o, C $c)  -> no assertType mismatch: $c->p is still 1
rv9_class_param(C $o, C $c)        -> mismatch, actual type "int"
rv9_mixed_param(mixed $o, C $c)    -> mismatch, actual type "int"
```

The `mixed` control takes the Maybe branch, so the predicate's No is the cause. The `object` call also reports `argument.type` on the same line; that is a separate diagnostic. The ledger's F00-R4 wording ("keeps both branches for mixed or extensible unknown objects") is inaccurate for `object`. *Required:* return `Maybe` for `ObjectWithoutClass`, `ObjectShape`, object-bearing intersections, and object-bounded templates, with a fixture for each. *Ledger:* R05.8 (new).

**RV-10 — Mechanism confirmed; consequence not reproduced. Spread arguments.** `resolve_call_args` skips spread arguments (calls.rs:956), so for `strlen(...$x)` the graph's argument 0 is `Type::mixed()` (builtin_calls.rs:2301). The invalidation step (call_effects.rs:405–412) indexes the raw call arguments, so index 0 names the spread source expression, not the unpacked element. A PHP probe did not reproduce a missed invalidation: `rv10_spread` (`strlen(...$list)` with `$list: list<C>`) invalidated `$c->p`, exactly as the direct call `rv10_direct` does. I did not trace which path performs that invalidation, so the ledger's claim that a Stringable element's side effects can be missed is unproven. *Required:* identify the path that invalidates in the spread case before claiming either outcome. *Ledger:* F05.4 text corrected; R05.2 acceptance should include the spread case.

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
- **ACR-6:** accepted as stated. RV-9 is a reproduced example of a host-level gap; RV-10 is a mechanism without a demonstrated consequence.

### Not verified by this review

- F05.7: the 8 PHPDoc fixtures and the 4 library tests exist (checked by name), but I did not
  run the optimized suites or check the baseline attribution.
- F01.2, F01.3, F04.1: test names exist. I did not audit each assertion beyond the cleanup
  and fatal tests.
- F02.1, F03.2, F03.3, F06.1, F06.3–F06.5: not checked.
- `is_null()` on template parameters: not traced in code. A PHP probe shows the effect: `null_template` (`@template T of string|null`) gets no null deprecation, while `null_union` (`?string`) does. Both can be null, so the predicate treats the template as definitely non-null. The same probe reports `strlen($x)` for `T` as `expects string, T given` (`argument.type`). That is a separate diagnostic, and I did not investigate it.
- The consequence of RV-10: not reproduced; see RV-10.

### Reviewer disposition

No finding is accepted. Each one needs an author amendment or an explicit acceptance by the
maintainer. The ledger has been updated to match: R00.1 moves from Blocked to Open (review
recorded, findings open). F00.2, F00.4, and F01.2 move from Complete to Partial, for RV-1 and
RV-4. Rows R01.2, R01.3, R01.4, R01.5, R01.6, R02.1, R00.4, R06.4, R05.8 (new), and F05.4
carry the findings. F00 stays Open.

---

## Second review pass: unchecked items (2026-10-09)

This pass works through the items the first record listed as not verified. Each item
states what was read or run. Claims that were only checked by name are marked as such.
Scratch probes and reruns live in the session scratchpad, not the repository. Nothing
here was committed.

### Results by item

- **F02.1 (was Complete, now Partial).** Source commit `e5e09d06` matches both PHP trees.
  clang 21.1.8 and gcc 15.2.0 match the lock. The linked bitcode hash `e3427ce8…` matches
  `whole-clang/php-cli.bc`. Of the lock's eleven `source_sha256` entries, two no longer
  match: `runtime.php` (already RV-11) and `run.py`, which changed in `4e7d29a3` after the
  lock was written in `043f2c78`. See RV-19.
- **F01.3 (Complete stands).** Each claimed category has a test. Dropped transfer candidate:
  `lib.rs` `post_fixpoint_validator_rejects_a_dropped_transfer_candidate`. Dropped result
  or pending throw: `…_dropped_result_or_pending_throw`. Missing successor or exit: the
  `omitted` and `missing_exit` cases in `published_post_fixpoint_certificate_is_revalidatable_and_graph_bound`.
  Graph change: the `CertificateGraphMismatch` case in that test. Falsely closed coverage:
  `lib.rs` around line 2843, in a PhpFlow test.
- **F04.1 (Complete stands).** The `php_value.rs` tests (lines 675–803) assert concrete
  values for copy isolation, nested copies, identity, rebinding and unset. Callback
  mutation across cleanup is covered by `php_flow_array_reference_copy_tracks_callback_writes_across_cleanup`,
  and across normal and throw states by `php_flow_host_composes_callback_writes_with_normal_and_throw_states`.
- **F02.3 (Partial stands).** Every figure in the report matches its source. GCC: 523
  commands exit 0, 505 with dump sets, 18 without, 13 sentinels (`gcc-profile.json`). CIR:
  523 objects, 44 emitted, 479 failed, 8.41%, 204 frontend crashes, StaticAssert 246,
  pointer casts 89, function decay 34, ptrdiff 31, 11.8 MB, and 36 of 44 lowered and
  verified (`cir-campaign.json`, `cir-lowering.json`). Clang CFG: I re-ran the bypass check
  from `checks.py` on `cleanup-ast-0.out`. The `may_throw` exceptional edge reaches
  `event(4)` without passing the `Guard` destructor block. The LLVM export does not show
  this, and the native trace `[1,2,4]` runs the destructor. So the loss is in the Clang CFG
  export, not in program behaviour. The inputs are under `/tmp/php-catalog-comparison`,
  outside the repository.
- **F03.2 (Partial stands).** In `strlen-native-demand-svf.json`, the Stringable target at
  `zend_API.c:769` is empty in the default, blackhole and expanded runs with no
  out-of-budget query. In the low-budget run it is empty with one out-of-budget query.
  `finding.empty_target_is_not_closed` is recorded.
- **F03.3 (Open stands).** No Rust crate reads `query_points_to` or `out_of_budget_query_count`.
  The Python tests pass (5 of 5). The `php-catalog` tool has no Rust tests.
- **F05.3 (Partial stands).** The argument check (`checker/calls/parameters.rs:307`) and
  return inference both call `catalog_builtin_return_type`, so they share its memo. The
  null diagnostic comes from the same evaluation.
- **F05.5 (Partial stands).** The covered-call early return (`call_effects.rs:405–415`)
  skips the legacy builtin run, as the ledger says. The cross-consumer audit is still open.
- **F06.1, F06.3, F06.5 (Open or Partial, consistent).** The plan sections the ledger cites
  exist (§2.3, §6, §7). No code under `crates/` or `tools/` implements the regional
  candidate or a same-slice comparison. F06.4: the measurement files exist, and no same-slice
  cost report does.
- **R03.1, R03.3 (Open stands).** No native-summary validator exists in the crates. The
  capsule lacks object construction, as the F03.2 record shows.
- **F00.3 source claims (partly confirmed; two gaps found).** I checked the PHP 8.5.4 source
  (`e5e09d06`):
  - `zend_parse_arg_str_weak` (`Zend/zend_API.c:758–781`) casts objects through
    `cast_object`. `zend_std_cast_object_tostring` (`Zend/zend_object_handlers.c:2468`)
    dispatches `__toString`. A non-string return is a thrown error, which the graph leaves
    open.
  - The null-deprecation message matches the runtime fixture.
  - Strict mode rejects before any conversion, as the strict runtime fixture shows.
  - Implicit `Stringable` is added at compile time for a non-trait `__toString`
    (`zend_compile.c:8226–8239, 8319`). A trait supplies it at inheritance time
    (`zend_inheritance.c:3723`).
  - Gap 1 (RV-16): a single plain-argument `strlen` compiles to `ZEND_STRLEN`
    (`zend_compile.c:4062–4079`, reached via `zend_try_compile_special_func`). Only spread
    or named-argument calls use `zif_strlen`. The artifact models `zif_strlen`. The
    runtime witnesses exercise the opcode for scalars, null and direct Stringable, but no
    source-matrix row covers the opcode handler.
  - Gap 2 (RV-17): the checker reports a false `argument.type` for a trait-supplied
    `__toString`. PHP accepts it: `strlen(new Traited())` returns 1 and `instanceof
    Stringable` is true.
- **CBMC mapping (R00.4).** All 93 valid-run properties are CBMC's, not the contract's.
  Ten are explicit checks at `contracts.c:37–50` (`main.assertion.1`–`10`). Two are
  unwinding checks (`main.unwind.0`, `.1`). The other 81 are generated memory, overflow and
  pointer checks of the harness. The invalid run adds `main.assertion.11` at line 55, which
  is the expected failure. Mapping: assertions 4–5 (a throw keeps the prior write and skips
  later events) correspond to contract row 1. Assertions 6–7 (normal event order)
  correspond to the ordinary sequential rows. Assertion 11 corresponds to the
  invalid-composition row. Assertions 1–3 and 8–10 (aliasing, a callback pointer copied in
  a struct, a retained callback) have no clause in the contract. The harness restricts
  dereferenced callbacks to two functions, so an unknown callback is not modeled at all. The
  low-budget run's expected failure is `main.unwind.0`, an unwinding assertion, which is a
  valid negative control. Every property is C-model-only.
- **campaign.py (RV-18).** Pointer-query runs at lines 74, 81 and 82 are unasserted, and
  line 103 writes `completed.json` unconditionally. `finish_campaign.py` waits for that
  marker, runs its own unasserted layout runs, writes its own marker, and hard-codes `/tmp`
  paths for the PHP binary and stock `dvf`.
- **Test quality (semantics crate).** All 68 tests contain an assertion. Scanning for
  presence-only checks flagged one test, my `manifest_rejects_unresolved_evidence_assumptions`,
  which I then tightened to `MissingEvidence`. The checker's 1200-plus tests and the fixtures
  were not audited.
- **F05.7 root cause (RV-14, RV-15).** Four library tests fail in the gate profile and pass
  in debug. The eight PHPDoc fixtures fail in the gate CLI and pass once the cause below is
  removed. The gate CLI reproduces `call to undefined method` for every `@method` tag, and
  `@mixin` members are missing too. The cause is side-effecting `debug_assert!` calls in
  `registry/resolve.rs`. Hoisting the side effects out of the macros, in a build I reverted
  afterwards, makes three library tests and all eight fixtures pass. The fourth library test
  is a test-design issue, not a release bug.
- **RV-3 (decided).** The empty dependency list on the locked `Return` is defensible. `strlen`
  takes its argument by value, and PHP strings are immutable. The envelope is either the
  exact length of a constant-string argument, or a nonnegative range for anything else.
  No callback can change a length that is already computed, or the nonnegativity of a
  normal return. The justification must still be written into the contract or ledger, which
  the ledger update below does.

### New findings

**RV-14 — Active, High. Side-effecting `debug_assert!` calls drop registry members in release-profile builds.**
`registry/resolve.rs:459` runs `add_member_entry` inside `debug_assert_eq!`, and line 1602
runs `set_shared_method_result` inside `debug_assert!`. Under `debug_assertions = false`
(the gate profile) neither call runs, so `@method` and `@mixin` members never enter the
registry. A scan of every `debug_assert` under `crates/` found no other side effects, and
its one other hit is a read-only query. The sites date from commits `32fb0a32` ("Move composed
method topology into registry results") and `d28d22a1` ("Move remaining Piwigo results onto
declaration IDs"), both 2026-10-06. *Required:* hoist both calls out of the macros, then add a
release-profile regression test for an `@method` tag. The three library tests and eight
fixtures listed above are the regression set. *Ledger:* R05.9 (new), with R05.5 and R05.6
annotated.

**RV-15 — Low. A test asserts a debug-only panic. Fixed.** The test now accepts a panic in debug or `None` in release, and fails only if a stale id returns a live declaration.
`switching_registry_bases_invalidates_the_previous_id_generation` expects `class_by_id` on a
stale id to panic. The panic comes from `debug_check_id_generation`, which is compiled out
in release. In release the stale id returns `None` (`class_declarations` is keyed by the full
id, generation included), which is the safe answer. *Required:* assert `None` in release, or
gate the test with `cfg(debug_assertions)`. *Ledger:* R05.6.

**RV-16 — Latent, Medium. The `ZEND_STRLEN` opcode path has no source matrix row.**
See the F00.3 result above. The modeled semantics agree with the handler on the observables I
checked. The gap is evidence, not a known divergence. *Ledger:* R00.3 acceptance.

**RV-17 — Active, Medium. A trait-supplied `__toString` gives a false `argument.type`.**
The checker reports `expects string, Traited given` for a class that uses a trait with
`__toString`. The PHP oracle accepts it. Probe output (gate build, current HEAD):
`Parameter #1 $string of function strlen expects string, Traited given. (argument.type)`.
The `instanceof Stringable` answer for trait classes was not separately probed. *Ledger:* R05.8
(extended).

**RV-18 — Active, Medium. The comparison campaign fails open.**
`campaign.py` runs the pointer queries without checking results and writes `completed.json`
unconditionally. `finish_campaign.py` waits on that marker and hard-codes `/tmp` binaries.
A completed marker therefore means every step ran, not that every step succeeded.
*Ledger:* R06.4.

**RV-19 — Low. The lock file is stale.**
`results/lock.json` records a `run.py` hash that predates `4e7d29a3`, and a `runtime.php`
hash that predates `644350cf`. Compiler identities, the source commit, and the linked-bitcode
hash all verify. *Ledger:* F02.1 downgraded to Partial until the lock is regenerated.

### Reviewer disposition after the second pass

The first record's "Not verified" list is now answered as follows. Items confirmed as
stated: F01.3, F02.3, F03.2, F03.3, F04.1, F05.3, F05.5, and the F06 narrative items. New
findings: RV-14 through RV-19. Corrections: F02.1 downgraded to Partial, and the F05.7
failures are now explained rather than open. Still not verified: the checker's test suite
beyond the catalog area, the 1200-plus checker tests, the full optimized fixture run (I
checked only the eight named fixtures), R05.1 source portability, and the cross-consumer
audit in R05.3.

## Author follow-up for R00.2 (2026-10-09)

The contract now defines a host-relative representation using `gammaV` and
`gammaS`, states entry, transfer, predicate, join/widening, exit, and
projection obligations, and narrows the evaluator's claim to a structural
post-fixpoint conditional on those host laws. It explicitly says the generic
API does not prove PHP semantic soundness or return explicit trace sets. It
also separates evidence provenance from trust, requires a pinned trusted
validator root, and makes unsupported pending-fatal, suspension, and
native-unwind cleanup compositions fail closed. This addresses C-1 as a
contract amendment, not as reviewer acceptance; a fresh independent pass must
check the relation against the evaluator and current tests before R00.2 closes.

The regression
`published_post_fixpoint_certificate_is_revalidatable_and_graph_bound` now
starts from a non-empty entry state, verifies the certificate retains it at
the entry frame, and verifies the normal exit contains that state plus the
event's write. Existing tests cover event outcomes, branch joins, widening
containment, dependency projections, and completion-kind exits. The cleanup
cases identified by RV-1 remain failing/ignored and are assigned to R01.5;
the amended contract does not claim they pass.

## Follow-up review findings for R00.2 (2026-10-09)

The maintainer supplied a follow-up review from Claude against the uncommitted
R00.2 changes. Its verdict was **not accepted yet**. It accepted the
host-relative definition and cleanup caveat, and identified five remaining
points:

- **S-1:** state/value/completion coverage over finite paths needed an explicit
  induction argument from entry coverage, sound transfers and predicates,
  successor closure, and exit inclusion. Full trace-prefix coverage must not
  be claimed because the API does not represent trace sets.
- **S-2:** remove the unimplemented "separately justified graph observation"
  escape hatch for observable effects.
- **S-3:** define a missing result ID as undefined on all states represented by
  that frame, and explain why partial result definitions cannot be joined.
- **S-4:** add a regression where distinct concrete entry states remain
  represented through join and widening, not only a single-state write.
- **S-5:** the amendment needs a fresh independent review before R00.2 closes.

The author addressed S-1 through S-3 in the contract and added a widening
regression for S-4. A subsequent review found that the test's wording
overstated its upper-bound coverage, so it is narrowed to the lower-bound
property. See the following independent re-review disposition.

## Independent re-review disposition for R00.2 (2026-10-09)

The maintainer supplied Claude's re-review of the updated, uncommitted tree.
The reviewer confirmed the finite-path induction against the validator,
accepted removal of the graph-observation escape hatch, and accepted the
undefined-result denotation. It rejected the widening test's claim that it
preserved distinct entry states because widening deliberately drops the upper
bound. It also noted that the cleanup rule is normative while the evaluator
still violates it for pending fatal, suspension, and native-unwind outcomes.

The author narrowed the test to
`widening_preserves_the_transferred_lower_bound_and_reflects_precision` and
documented that it asserts the widened exit is `[1,*]` without testing
upper-bound preservation. The contract now
explicitly says its cleanup rule is not implemented and R00.2 closes as a
definition only; conformance remains under RV-1/R01.5. The reviewer stated
that these two corrections allow R00.2 to close as a definition, with R01.5
carrying cleanup conformance. This is the review disposition; it does not
claim that cleanup behavior is fixed.

## Author follow-up for R00.4 (2026-10-10)

The CBMC accounting and fail-open campaign finding RV-8 is resolved for the
three recorded runs. The evaluation report maps every property from the 93-
property valid run, 94-property invalid-composition run, and two-property
low-budget run, including the C-only scope and analogous Rust/PHP evidence.
`campaign.py` now pins each reviewed property-ID/status inventory by digest,
checks the expected CBMC summary and process exit, and rejects changed,
missing, duplicate, or unreviewed properties. Seven regression tests pass,
including a campaign-runner dispatch check. All three commands also passed
through the actual `EnforcedRunner` with resource enforcement, producing the
expected output hashes and exit codes. The follow-up evidence, runner and test
hashes, and tool digest are recorded under `followup_cbmc_validation` in the
evaluation lock. This resolves only the campaign-accounting defect; it does
not extend the reduced C model's claims to PHP semantics.
