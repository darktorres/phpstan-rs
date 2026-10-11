# Phase F00 contract review

The follow-up [adversarial review pass](#adversarial-review-pass-same-author-not-independent)
records additional issues. It is also author-led and does not close the
review requirement.

Status: **self-review complete for this pass; review outstanding**.
F00 remains open. This review compares the working contract with the shared
evaluator API, catalog validation, current semantic-host tests, and the pinned
PHP 8.5.4 `strlen` source/runtime evidence. It is an author review and
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
class declarations remain assumptions that require review.

## F00 status

F00 subtask statuses and remediation actions are maintained in the single
[Phase F task and remediation ledger](PHP_SEMANTICS_CATALOG_LEDGER.md).

## Review checklist for a reviewer

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
a reviewer accepts or amends the contract, dependent gate work
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
Review type: author-led adversarial pass; **not a review by another party**
Disposition: contract remains a working draft; F00.5 and R00.1 remain open.

This review examines the questions in the F00 checklist against
[the contract](PHP_SEMANTICS_CATALOG_CONTRACT.md),
[the prior contract self-review](#phase-f00-contract-review),
the plan's Phase F gate, the evaluator/catalog APIs, and the recorded CBMC
artifacts. It identifies issues for the reviewer and proposes
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

The reviewer should require an explicit trust rule for dependency
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

| Checklist question | Adversarial-pass disposition |
|---|---|
| Does abstract order cover values, state, pending outcomes, and observable projections? | Partly specified; the abstract-to-concrete representation/simulation relation is missing (ACR-1). |
| Are normal returns, exceptional exits, cleanup replacement, suspension, and nontermination prefixes composed without losing a concrete trace? | Core distinctions are specified, but cleanup combinations are incomplete and prefix preservation under widening lacks an explicit representation relation (ACR-1, ACR-4). |
| Can a closed claim be consumed with missing, inapplicable, or invalidated assumptions? | Intended behavior is specified, but typed records and context validation are not implemented; cross-namespace cycles and validator trust are unresolved (ACR-3). |
| Do unknown and partially modeled transfers conservatively widen every affected component? | This is a stated obligation with selected host tests; completeness of affected-domain/dependency declarations is not established (ACR-2, ACR-6). |
| Are PHP-host predicates and event transfers sound for the claimed source/build/profile? | Narrow `strlen`/Stringable evidence exists; it does not validate all host transfers or all callback outcomes (ACR-5, ACR-6; see F00-R3/R4 in the prior review). |

### Proposed dispositions and remaining acceptance

1. Amend the contract to resolve ACR-1 through ACR-4, or explicitly record why
   the reviewer accepts the current limits for the Phase F gate.
2. Keep the CBMC evidence scoped to the reduced C model and maintain its
   property-to-clause mapping; do not describe it as PHP semantic validation.
3. Keep F00.3/F01.5 open until source-derived host obligations and contradictory
   cases are reviewed for the actual closed slice.
4. Have a reviewer other than this author assess this report, the contract, the
   underlying implementation/evidence, and the proposed dispositions. Record
   that review and any amendments before closing R00.1.

No finding is marked accepted by a reviewer. R00.1 remains
**Blocked** in the remediation ledger until item 4 is completed; F00.5 remains
**Open** in the Phase F status ledger.

The fix sequence for each ACR finding is recorded in the
[authoritative Phase F and remediation ledger](PHP_SEMANTICS_CATALOG_LEDGER.md):
ACR-1/4 map to R00.2 and R01.5; ACR-2 to R00.3, R01.6, and R04.1; ACR-3 to
R00.2, R01.1, and R01.7; ACR-5 to R00.4; and ACR-6 to R00.3, R02.3, and
R04.1. Each row includes dependencies and acceptance evidence.

---

## Review record (2026-10-09)

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

**Remediation status (R01.5, 2026-10-10):** the current implementation rejects every
unsupported abrupt cleanup replacement in both evaluation and certificate replay. It models
only pending PHP throw followed by cleanup `Return`. An evaluator-level matrix checks all
four pending kinds against cleanup Return, PHP throw, fatal, and suspension; `Resume` is
tested separately for all four kinds. Callback throw through a maybe-branch, distinct-state
join, and resume is also covered. The original counterexamples now fail closed. This records
the author's remediation evidence, not independent acceptance; RV-1 remains pending a fresh
review of R01.5 and the updated contract.

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

**RV-10 — No missed-invalidation defect reproduced; spread callback dispatch remains unproven.** `resolve_call_args` skips spread arguments (calls.rs:956), so for `strlen(...$x)` the graph's argument 0 is `Type::mixed()` (builtin_calls.rs:2301). The raw formal slot is unresolved; `forget_catalog_string_arguments_may_change` then takes its conservative fallback, joins spread-element types, and applies generic `Operation::String` invalidation. The PHP probe's `rv10_spread` (`strlen(...$list)` with `$list: list<C>`) therefore invalidates `$c->p` through fallback, as does direct `rv10_direct`. This does not show graph-selected callback invocation or precise spread-element handling; the result is conservative invalidation. *Disposition:* the earlier claim of a specific missed invalidation is withdrawn. Keep spread catalog dispatch outside the accepted R05.2.1 scope until binding can identify the unpacked formal element.

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
  export, not in program behaviour. The inputs were under `<scratch root>`,
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
  marker, runs its own unasserted layout runs, writes its own marker, and hard-codes scratch
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

**R01.6 remediation status (2026-10-10):** the updated contract and evaluation report now
record why the locked `StringByteLength(Argument(0))` return has no required dependency.
`BehaviorGraph::validate` checks graph-derived dependencies for modeled reads/writes,
including supported PHP-throw cleanup returns, consumed invocation results/guards, and
invocation-derived exceptional projections; it continues to require each `Unknown` event's
affected domains to remain open. Mutation tests
remove required edges and confirm rejection. The locked builder was rerun against the pinned
PHP source, LLVM inventory, and runtime witness and reproduced the checked-in catalog byte for
byte. This is author evidence; fresh independent acceptance remains pending.

**R01.7 remediation status (2026-10-10):** a deployment-owned `ValidatorTrustPolicy`
now pins validator ID/version to an implementation digest under a configured root,
rejects ambiguous or unrooted pins, applies revisioned revocations, and supplies a
policy cache key. `Catalog::validate_assumption` checks context and support closure,
recursively validates prerequisites, and calls exactly one executor only after the
executor identity/digest matches the pin. Tests cover exact pins, stale executors,
revocation/cache invalidation, unknown program context, and transitive support.
Executor loading and root configuration remain deployment trust boundaries; the
deployment loader must verify implementation bytes. Context-free graph validation,
evaluation, and projection remain fail-closed. Catalog-backed projection and evaluation
now validate assumption-backed closed coverage for the selected entry/context, and
certificate replay reruns those checks under the current trust policy. The integration
is tracked as R01.7.a in the ledger. This is author evidence; independent acceptance is
pending.

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
unconditionally. `finish_campaign.py` waits on that marker and hard-codes scratch-root binaries.
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
- **S-5:** the amendment needs a fresh review before R00.2 closes.

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
`campaign.py` now pins each reviewed property-ID/status inventory and complete
normalized property-record digest, checks the pinned CBMC binary, C source
files, and reference transcripts, then validates property IDs, statuses,
descriptions, source locations, counterexample traces, summary, and process
exit. It rejects changed, missing, duplicate, or unreviewed properties. Ten
regression tests pass, including campaign dispatch and lock mutation checks.
All three commands also passed through the actual
`EnforcedRunner` with resource enforcement, producing the expected output
hashes and exit codes. The follow-up evidence, runner and test hashes, and
tool digest are recorded under `followup_cbmc_validation` in the evaluation
lock. This resolves only the campaign-accounting defect; it does not extend
the reduced C model's claims to PHP semantics.

## Author follow-up for R01.1 (2026-10-10)

Schema v8 now stores typed assumptions separately from evidence. Assumption
IDs are domain-separated and schema-bound; records carry a typed kind, claim,
target and optional loaded-program scope, prerequisite and supporting-evidence
IDs, and validator ID/version. Catalog and manifest validation keep the two
reference namespaces separate, propagate evidence assumptions to coverage,
reject unresolved references and malformed scope/record data, and round-trip
the new manifest field. Context matching is
explicitly not validator acceptance: graph validation rejects closed coverage
citing an assumption until trusted validation is implemented. R01.2, R01.7, and R01.8
remain open for cycles, validator trust, and unordered-vector canonicalization,
so IDs currently preserve input order for list-valued fields.
The semantics crate has 67 passing tests and six previously ignored findings;
`cargo check -p phpstan-rs` passes, and the generated PHP 8.5.4 catalog
reproduces byte-for-byte. A full `cargo test --workspace` run was started but
not completed: its cold build remained compiling the large checker test target
for more than ten minutes, so validation was narrowed to the changed crate and
its direct consumer.

## Author follow-up for R01.8 (2026-10-10)

The unordered-vector identity gap is addressed at both identity and validation
boundaries. Record IDs and graph/manifest digests normalize set-like vector
order, while graph, catalog, target-predicate, and manifest validation reject
noncanonical serialized order and duplicate members. Covered lists include
evidence and assumption references, domain coverage and its set-like fields,
open reasons, unknown-event domains, target-predicate extension/INI
constraints, and manifest record arrays. Existing identities for canonical
inputs remain unchanged, while ordered blocks and events remain
order-sensitive. Regression coverage exercises each collection family and
confirms reordered events still produce distinct graph digests. This is author
evidence; independent acceptance is pending.

## Author follow-up for R02.1 (2026-10-10)

`evaluate_strlen_svf_demand.py` now binds replay to the exact runtime fixture
path and SHA-256 recorded by the witness, and checks the recorded output hash
against both captured Hybrid and CALL output files. Regression tests prove
that a matching fixture is accepted, a one-byte change is rejected, and both
output artifacts still match their recorded hash. The checked-in fixture is
currently newer than the 13-sentinel witness (`94dc6509…` versus
`9b38a3ea…`), so replay fails before making a claim about the stale witness.
This closes the fail-open mechanism; it does not reconcile or replace the
historical witness. This is author evidence; independent acceptance is pending.

---

## Third review pass: commits 60c668a0..HEAD (2026-10-10)

Scope: the 24 commits after `60c668a0`, reviewed by reading their diffs and checking the
claims against the code, the tests and the stored artifacts. The uncommitted working tree
(about 50 modified files and several untracked witnesses) is not part of this pass. Where
the committed docs and the working tree differ, the committed text was judged.

### What was checked

- **Identity and canonical order** (`77ee2d87`, `4e0ef0db`, `a01299a0`, `dbb2182a`). Unordered
  vectors are normalized for identity, and validation rejects unsorted or duplicate input.
  Release ranges and predicate vectors are validated on add and on load. RV-5, RV-7 and RV-8
  are closed. Support-graph cycle detection covers self, multi-node and cross-namespace
  cycles, and accepts DAGs.
- **Typed assumptions and deployment-pinned trust** (`cb50f132`, `bdf3665a`, `c7efd837`).
  Applicability is separated from trust. Trust needs an exact validator version, a pinned
  implementation digest and an unrevoked root, and the policy has a cache key. Evaluation
  requires `Applicability::Yes`, and cycles through prerequisites and evidence are caught.
- **Not-applicable closure and transitive dependencies** (`b8cbd36e`). A closed domain
  reached through a not-applicable intermediate is rejected, and open propagation follows
  transitive edges. RV-2 is closed.
- **Cleanup replacement** (`69d936fb`). Only a return over a pending PHP throw is allowed.
  The other 19 cells fail closed. The unit matrix covers all 4 pending by 5 replacement
  cells. RV-1 is closed, and the three committed regression tests now pass with strict
  assertions: they accept only `UnsupportedCleanupReplacement`, and a normal or PHP-throw
  exit is never allowed through.
- **Required-domain dependencies** (`eca079b4`). Edges for `Return` are derived from events on
  normal-return paths, and missing edges fail with `MissingRequiredDomainDependency`. Seven
  mutation tests cover this. The strlen `Return` has no such edge, which matches the
  reasoning recorded for RV-3.
- **Composed coverage and certificates** (`eca079b4`, `bdf3665a`). Composed child coverage
  goes through the same required-edge check. Certificates can carry an assumption context
  that is re-validated against the catalog, the target, the loaded program and the policy.
- **CBMC accounting** (`39ccc5ad`, `04cd907a`, `dfa1c39b`, `119f0a27`, `d3375c16`). Each run
  checks the exit code, the property count, the inventory digest and the property-record
  digest against constants, and the reference transcript is read before the run. Failures
  must match the expected set exactly. I re-checked the lock, the CBMC binary hash and the
  three sources against the cache, and each transcript passes the validator against itself.
  This does not rerun CBMC, which is not installed here. The property mapping in the
  evaluation report matches the transcripts: the valid-run properties by exact ID, with
  `main.assertion.11` at `contracts.c:55` as the single expected failure, and the low-budget
  run's failure at `main.unwind.0` (`loop_contract.c:7`).
- **SVF replay pin** (`f16d46ee`). RV-11 is enforced: the replay checks the fixture digest,
  the recorded output digests and the sentinel before it runs anything.
- **Compiler correspondence** (`f307c252`, `7617797e`, `94bce99a`). All 15 recorded input
  artifacts match their hashes. The report states its own limits, including that it does not
  close `ZEND_STRLEN` opcode correspondence. The Clang cleanup finding (no destructor edge on
  the exceptional path of the AST CFG) matches what I re-derived from the recorded CFG.
- **Strlen artifact at HEAD** (`9f895cff`). The committed artifact is internally consistent
  with the committed checker: it refers only to the `strlen-weak-null-deprecation` and
  `strlen-weak-stringable-cast` predicates, which `HEAD` implements. Parser failure is
  an `Unknown` event in block 0, and the host's `Invoke` transfer exits both normally and
  as a PHP throw. Regeneration of this artifact from the `HEAD` builder has not been verified (see
  below).

### Findings

**RV-20 — Medium, status. R02.3 is overstated.** The committed ledger marks R02.3
"Implementation complete; independent acceptance pending". Its acceptance requires modeling
release, effects, exceptions, fatal and native unwind, and non-normal exits. The same ledger
lists R02.5 (release) and R02.7 (non-normal native and callback outcomes) as Open, and the
committed matrix says native unwind "has no evidence and stays Open". Recommendation:
downgrade R02.3 to Partial, and state which acceptance items it has met.

**RV-21 — Medium, test integrity. The required-edge tests are partly circular.** The `graph()`
helper in the `lib.rs` test module derives `Return` dependencies from the production
`event_domains` table. Tests built through it cannot detect an error in that table, and they
agree with the validator by construction. Recommendation: give at least the required-edge
mutation tests hand-written expected edges, and keep one test that checks `event_domains`
against a literal table.

**RV-22 — Low, coverage. The evaluator cleanup matrix omits native-unwind replacement.**
`evaluator_enforces_every_pending_completion_replacement_cell` tests four replacement kinds
and leaves out `NativeUnwind`. The unit matrix covers that cell at the decision-function
level only. Recommendation: add the evaluator-level case.

**RV-23 — Medium, process. Committed docs and working tree diverge.** About 50 files are
modified in the working tree and not committed. They include the parser-acceptance predicate
and its `Throw` branch, a larger artifact with 18 evidence records, and matrix and ledger text
claiming R02.4 and R02.6 are complete. None of that is in a commit. The committed ledger
correctly keeps R02.4 and R02.6 Open. Recommendation: commit or set aside the working-tree
changes before any status is claimed for them, and review them as commits.

**RV-24 — Medium, documentation. Stringable outcomes disable the catalog bridge for some
arguments.** `CatalogTypeHost` now returns a `PhpThrow` exit alongside the normal exit for
`__toString`. The strlen bridge returns `None` on any non-normal exit, so calls whose
Stringable branch is reachable (`mixed`, objects) fall back to the legacy path. That is
conservative. Probes at `HEAD` and in the working tree gave identical output for `int`,
`string`, `object`, `mixed`, `?string`, literal and spread `null` cases, so the fallback is
not visible on those inputs. The ledger does not state the fallback. Recommendation: record
it in the R05 and F05.4 rows.

**RV-25 — Low, design statement. Loading does not establish assumptions.** `add_entry`,
`from_manifest` and `evaluate` now use `validate_structure` or `validate_with_assumptions`.
The unestablished-assumption check runs only when an established list is supplied. Projection
without context refuses closed-with-assumption claims, and evaluation-time validation covers
the rest. This is consistent with the contract's rule that unestablished assumptions do not
establish anything, but the contract does not say that loading a manifest is not establishment.
Recommendation: add one sentence to the contract.

**RV-26 — Medium, evidence state. The SVF replay refuses to run.** RV-11 is enforced, but the
checked-in runtime witness still records the old `runtime.php` digest. The replay now stops
before doing any work, which is the intended behaviour. R02.1 is "implementation complete",
and the matrix's runtime evidence depends on that replay. Recommendation: regenerate the
witness against the current fixture, or pin the old fixture, and say which in the ledger.

### Corrections to earlier review records

- In an earlier pass I said the committed artifact and checker disagreed about the parser
  predicate. That was wrong: I had compared the working-tree artifact with the `HEAD` checker.
  The committed pair is consistent (see the strlen artifact entry above).
- The earlier claim that the regenerated artifact is byte-for-byte reproducible was checked
  against the working-tree builder and artifact only. It has not been checked at `HEAD`.
- RV-24 is retracted in its mechanism by the fourth pass (see its RV-24 entry there); the rest of its text stands.

### Not verified in this pass

- The full library and fixture suites at `HEAD`. A gate-profile test build in a worktree was
  still running when this record was written, and was competing with another session for CPU.
  The working-tree semantics suite passes (110 tests, none ignored), but that is not the
  committed state.
- Regeneration of `php-8.5.4-strlen.json` from the `HEAD` builder and inputs.
- The working-tree changes listed under RV-23.
- The remaining checker tests outside the catalog area, and the full optimized fixture run.

### Reviewer disposition

The committed commits close RV-1, RV-2, RV-4, RV-5, RV-6, RV-7 and RV-8 in their implemented
form, and RV-11 is enforced but its evidence is still stale (RV-26). They add the typed assumption, trust, cycle, cleanup-matrix and CBMC fail-closed machinery
the earlier records asked for. The open items for the next author pass are RV-20, RV-21,
RV-23, RV-24 and RV-26 (Medium or higher), and the verification gaps listed above.

---

## Fourth review pass: commits f307c252..9f895cff and c513c52e (2026-10-10)

Scope: the five commits that followed the third pass's last reviewed commit (`f307c252`
through `69da8569`, which are the builder, provenance, callback-throw and documentation
commits), and the third pass record itself (`c513c52e`). Each commit was read as a diff and
checked by running the code or regenerating the artifact at that commit. The uncommitted
working tree is not part of this pass.

### What was checked

- **Correspondence builder** (`f307c252`, `7617797e`, `94bce99a`). Each commit's builder,
  run on the same recorded inputs, reproduces that commit's committed
  `strlen-representation-correspondence.json` byte for byte. The `HEAD` builder reproduces
  the `HEAD` artifact. The Clang AST campaign records the scratch copy of the locked PHP
  source, and the builder requires that path. Passing the repository copy is refused, as
  intended. The `HEAD` rebuild that failed earlier was caused by this path choice.
- **Callback throw model** (`9f895cff`). At that commit, the semantics library tests pass
  (109), the catalog example tests pass (5), and `php-8.5.4-strlen.json` regenerates
  byte-for-byte from `strlen-llvm-operation-inventory.json` and
  `strlen-weak-scalars-8.5.4.json`. The checker's `catalog` unit tests pass (5, one
  pre-existing RV-9 ignore), including `stringable_invocation_keeps_observed_php_throw_path`.
  All 16 `strlen` fixtures pass. The `PhpThrow` coverage of the `__toString` graph is `Open`.
  No reference to the retired `callback-observation-v1` rule remains.
- **Documentation** (`69da8569`). The `Unknown` event row matches the host, which returns a
  Normal exit with unchanged state (`builtin_calls.rs:523`). The R02.4 to R02.7 open rows
  match the committed matrix.

### Findings

**RV-27 — Medium, provenance. The builder does not bind input files to their recorded commands.**
Commits `7617797e` and `94bce99a` added checks that a campaign command names the pinned
source and exited successfully. None of them checks that an input file is the output of
that command. For the Clang AST summary, only the source path appears in the command. For
the GCC dumps, the only file check is the name suffix (`.007t.gimple`, `.016t.cfg`). For
the cleanup counterexample, the GIMPLE, AST and LLVM summaries are not compared with their
commands' outputs. Reproduced: with a fabricated `cleanup_gimple` file containing
`finally` and `Guard::~Guard (&guard)`, which no recorded command produced, the builder
exits 0 and reports `gcc_gimple_has_exceptional_finally_destructor: true`. The JSON
provenance note ("tied to the successful source-bound dump command") and the ledger's
R02.2 validation line ("a mutated Clang source digest was rejected", which exercises only
the source digest) both overstate the binding. *Required:* compare each input's path or
digest with the output argument of its recorded command, or record output digests in the
measurement records and check them. Add a mutation test for each bound input.

**RV-28 — Medium, documentation. Committed rows say the checker does not consume throw-state effects.**
`69da8569` states, in R05.2, that "the checker still does not consume ... throw-state
effects", and the F05.4 row says "stateful ordered writes/throws are not consumed by the
checker." Since `9f895cff`, `catalog_php_throw_invocation_at` (`checker.rs:2440`) feeds
`expr.rs:248`, which records a throw state with `forget_catalog_string_arguments_may_change`
applied. The statement is therefore wrong for string-argument invalidation on the throw
path. It remains right for ordered callback writes, invalid returns and destructor effects.
*Required:* restate both rows to name what is consumed and what is not.

**RV-29 — Medium, coverage. The strlen throw path inside `try` has one fixture.** Before
`9f895cff`, `strlen` produced no `PhpThrow` exit, so the throw-state path in `expr.rs:248`
was inactive for it. The commit activates it for `strlen($stringable)` inside a `try`. The
fixture `strlen_catalog_stringable_conversion_preserves_callback_effects` covers only the
write-before-throw case (the write survives into the catch). It does not cover catch-side
narrowing or invalidation of string-argument facts on that path. The host unit test checks
the exit set, not the checker's behavior. *Required:* add a fixture that narrows a variable
inside a `catch` after a Stringable-throwing `strlen`, and one that checks the variable's
type after a non-throwing call on the same path.

**RV-30 — Low, documentation. R02.2 contains a contradictory sentence.** The F02 paragraph
in `f307c252` and carried by `7617797e` and `94bce99a` says "A systematic AST/CFG-to-LLVM
semantic correspondence adapter and broad transfer comparison are absent. Complete
systematic AST/CFG-to-LLVM comparison adapters and broader semantic correspondence." The
first sentence describes the state before R02.2; the second reads as unfinished work and
contradicts the R02.2 entry, which says the adapter is implemented. *Required:* delete the
pre-R02.2 sentence and keep the remaining-work statement, if any, under R02.7 or R02.5.

**RV-31 — Low, limitation. The GCC CFG reachability is an approximation.** `gcc_cfg_blocks`
(in `f307c252`) reconstructs successors from `goto <bb N>` and falls through to the next
block only after `if (` blocks, or after a block with no successor that is not
`__builtin_unreachable` or `return;`. The `gcc_path` result therefore depends on this
heuristic. The JSON and ledger describe the result as a structural correspondence, which
is the right scope, but the reconstruction is not a verified CFG. *Required:* none for
acceptance; note the heuristic next to the GCC path result in the JSON.

**RV-24 — retracted in its mechanism (correction to the third pass).** The third pass said
that the strlen bridge returns `None` on any non-normal exit, so Stringable calls fall back
to the legacy path. That is wrong. `catalog_builtin_return_type` (`builtin_calls.rs`,
around line 2766) returns `None` only for a `Normal` exit with no value, or when the Return
coverage is not `Closed`. `PhpThrow`, `NativeUnwind`, `Fatal` and `Suspended` exits keep
the normal return type. The `PhpThrow` observations feed
`php_throw_without_string_invocation` and `php_throw_invoked_string_arguments`, which the
checker's throw-state path consumes (RV-28, RV-29). The third pass's probe claim that
outputs were identical "under" the fallback was made under the wrong mechanism and is not
verified. The real issue is the new throw-state behavior, recorded as RV-28 and RV-29.

### Commit verdicts

| Commit | Subject | Verdict | Basis |
|---|---|---|---|
| `f307c252` | Record bounded strlen compiler correspondence | Accept with notes | Reproduces its artifact. RV-30, RV-31. |
| `7617797e` | Bind compiler correspondence to campaign provenance | Needs fix | Reproduces its artifact. RV-27. |
| `94bce99a` | Verify cleanup evidence provenance | Needs fix | Reproduces its artifact. RV-27 reproduced. |
| `9f895cff` | Model observed strlen callback throws | Accept with notes | Tests and regeneration pass. RV-29. |
| `69da8569` | Track remaining strlen remediation gaps | Needs fix | Docs. RV-28, RV-30. |
| `c513c52e` | Record third review pass | Needs fix | RV-24 mechanism retracted here. |

### Not verified in this pass

- The full library and fixture suites at `HEAD`. The checker's catalog unit tests and the
  strlen fixtures were run at `9f895cff`, not at `HEAD`.
- The Python builder has no unit tests in the repository. Its behavior was checked by
  reproduction only.
- Other sources of input-artifact provenance were not traced beyond the cases in RV-27.
- The checker outputs for the spread and `object` probes cited in the third pass. They
  remain unverified until re-probed at `9f895cff`.

### Reviewer disposition

Commits `f307c252` and `9f895cff` are accepted with notes. `7617797e`, `94bce99a`,
`69da8569` and `c513c52e` need fixes. The open items for the next author pass are RV-27,
RV-28 and RV-29 (Medium), with RV-30 and RV-31 (Low). The third pass's RV-24 is retracted
in its mechanism, and the rest of its text stands.

## Author remediation pass (2026-10-10)

This pass implements remediations for the fourth review's remaining findings and
the earlier open Stringable, linked-module, and campaign-integrity findings.
Implementation, artifact regeneration, and validation are complete for the
claims listed below. Independent acceptance remains open; the actual SVF replay
and full resource campaign were unavailable in this pass.

- **RV-9 / RV-17 / nullable-template gap:** the Stringable predicate now keeps a
  `Maybe` branch for object types without an exact class identity and recognizes
  registered trait-composed `__toString` methods in weak-argument and call
  transfer checks. Nullable templates preserve the possible null deprecation.
  Regressions cover bare objects, object shapes, intersections, object-bounded
  templates, trait methods, and nullable templates. All 1,234 PHPStan library
  tests pass, including these regressions; independent acceptance remains open.
- **RV-21 / RV-22:** the dependency-test graph helper now uses an independent
  literal event mapping, paired with a literal regression for the production
  `event_domains` table. The evaluator completion matrix now includes native
  unwind as a cleanup replacement for every pending completion kind. All 112
  semantics tests pass.
- **RV-11 / RV-12 / RV-26:** the 13-sentinel Hybrid/CALL witness is paired with
  `runtime-hybrid-call-13.php`, restored from its source commit; the current
  16-sentinel `runtime.php` remains separately represented. SVF replay now
  validates the fixture selected by the witness, locked source digests, the
  link-manifest digest, and supplied linked-bitcode identity. The root lock is
  refreshed and its live source/manifest checks pass. Actual replay remains
  pending because the pinned linked PHP bitcode is unavailable here.
- **RV-27:** measurements bind preserved Clang stdout archives to captured
  producer bytes and GCC sidecar archives to paths derived from successful
  compile commands. The artifacts, measurements, and correspondence are
  regenerated; all 21 focused Python tests pass, including producer/path,
  digest, archive-mutation, malformed-run, and CIR-artifact cases.
- **RV-13 / RV-18:** campaign admission waits for completed resource proof and
  fails closed on missing completion data. Pointer/build steps are checked;
  completion manifests bind unique steps, expected exits, and measurement
  digests. CIR rejections are explicit unsupported outcomes. PHP and stock-DVF
  paths are supplied by the caller rather than fixed scratch paths. All 21
  focused Python tests pass; the resource-limited end-to-end campaign was not
  rerun.
- **RV-20 / RV-28 / RV-30 / RV-31:** the ledger scopes R02.3 as Partial,
  documents which throw-state effects the checker consumes, removes the
  contradictory R02.2 sentence, and the correspondence record identifies GCC
  CFG reachability as heuristic.
- **RV-23:** the earlier 61-file implementation commit exists. This remediation
  is reconciled against its tests and artifacts and committed as
  `Close PHP catalog review remediation gaps` in the current worktree history.

The review gate remains open. After implementation and validation,
the reviewer should recheck the changed transfer logic, campaign evidence
binding, regenerated artifacts, and amended statuses before F00.5 closes.

## Fifth review pass: author remediation pass and later catalog commits (2026-10-11)

Scope: the remediation commits after the fourth pass (`a00c6426` and the commits
between `c513c52e` and `8ad93491` (HEAD `8053863c` only deletes `MASTER_REIMPLEMENTATION_LEDGER.md`) that touch the catalog, its builder or its
checker routing), checked against the RV findings they claim to close. Each claim
was checked in the code, in a test run, or against the recorded measurements at
`8ad93491` (HEAD `8053863c` only deletes `MASTER_REIMPLEMENTATION_LEDGER.md`). This is not a line-by-line read of the 3,133-line transfer change in
`bc4b7524`.

### What was checked

- **Provenance (RV-27).** The builder's `validate_measurement_output` requires each
  sidecar input to match a successful command record's `output_artifacts` digest and
  the output path derived from that command's `-o`/`-c`. The cleanup GIMPLE and CFG
  dumps are bound this way (`results/main/measurements.json`, record `cleanup-gimple`).
  The fabricated-file reproduction from the fourth pass is therefore refused, since its
  digest is in no record. The Python suites (36 tests, including
  `test_gcc_sidecar_must_match_compiler_dump_path_and_digest` and
  `test_every_bound_output_requires_exact_path_and_digest`) pass.
- **Routing refactors (`4580fe5c`, `5e363624`).** `catalog_function` still requires a stub
  signature and a case-insensitive match against the constant name, so a user function of
  the same name is not catalogued. The named-callable resolver is only used for a string
  literal or a constant array callable. Ambiguous callables fall to the generic path.
  The 20 strlen fixtures pass at `8ad93491` (HEAD `8053863c` only deletes `MASTER_REIMPLEMENTATION_LEDGER.md`).
- **Cleanup replacement (RV-22).** `replacement_allowed` (`php-semantics/src/lib.rs`
  around line 405) allows only a normal return or a PHP throw to replace a pending PHP
  throw. Every other pending kind fails with `UnsupportedCleanupReplacement`, and the
  matrix test asserts it. The 113 semantics tests pass.
- **Trait `__toString` (RV-17) and the Stringable predicate (RV-9).** The predicate in
  `checker/expr/builtin_calls.rs` keeps a `Maybe` branch for object types without an
  exact class identity. `trait_provided_tostring_is_accepted_by_strlen` and the
  object-shape/intersection/template regressions in `catalog_object_argument.rs` cover
  it. The 32 catalog unit tests pass at `8ad93491` (HEAD `8053863c` only deletes `MASTER_REIMPLEMENTATION_LEDGER.md`).
- **Documentation (RV-28, RV-30).** The F05.4 row now names the throw-state string
  invalidation the checker consumes, and the pre-R02.2 sentence in F02 is gone.
- **Heuristic note (RV-31).** The correspondence artifact labels the GCC CFG result as
  "structural match using heuristic CFG reconstruction".
- **R05.5 (ledger).** The eight ledger-named PHPDoc fixtures pass at `8ad93491` (HEAD `8053863c` only deletes `MASTER_REIMPLEMENTATION_LEDGER.md`) in the
  gate profile: `own_templates_are_inferred_and_validated`, the `a_method_tag_*` group,
  `a_method_with_no_return_type_defaults_to_mixed`, `a_mixin_makes_the_target_s_public_methods_callable`,
  and both `a_method_tag_*_referencing_an_unknown_class_is_flagged` fixtures. The row is
  updated to Complete.

### Findings

**RV-29 — still open, Medium, coverage.** The only `try`-wrapped `strlen` fixture,
`strlen_catalog_stringable_conversion_preserves_callback_effects`, covers only
write-before-throw. No fixture narrows a variable inside a `catch` after a throwing
`strlen`, and none checks a variable's type after a non-throwing call on the same path.
The remediation pass does not list RV-29 among its fixes. The fixture file
`builtin_range_narrowing/strlen_catalog_stringable_conversion_preserves_callback_effects.php`
is unchanged since `fb0b72bd`. The matching unit tests in `catalog_object_argument.rs`
(`stringable_throw_preserves_catch_narrowing_and_normal_path_narrowing`) cover
the catch narrowing at the host level, so the gap is the checker-level fixture the
fourth pass asked for. *Required:* add the two fixtures.

**RV-27 — accepted with a note.** The binding is now real for the cases the fourth pass
named. Two limits remain. The measurement records are trusted, so a forged record with a
matching digest would pass; that is a recorder-integrity question, not a builder one. And
the artifact could not be regenerated here: the cleanup dumps named by the records
(`<scratch root>/final/cleanup.cpp.007t.gimple` and
`.016t.cfg`) are absent, as is the `<scratch root>` tree, and the cache under
`~/.cache/php-catalog-evaluation` holds no cleanup dumps. The byte-for-byte regeneration
claimed for `strlen-representation-correspondence.json` is therefore not re-checked by
this pass.

### Commit verdicts

| Commit | Subject | Verdict | Basis |
|---|---|---|---|
| `a00c6426` | Close PHP catalog review remediation gaps | Accept with notes | RV-27, RV-28, RV-30, RV-31 and RV-22 verified in code. RV-29 not addressed. |
| `4580fe5c`, `5e363624` | Named-callable and catalog-function refactors | Accept | Read; no route change found; strlen fixtures pass. |
| uncommitted ledger update (R05.5) | Record R05.5 | Accept | Eight fixtures run and pass in the gate profile. |

### Not verified in this pass

- The full fixture suite and the full library suite at `HEAD` (R05.7 is still open; it
  depends on R05.4).
- The byte-for-byte regeneration of the correspondence and catalog artifacts (inputs
  absent, see RV-27).
- The line-by-line transfer logic of `bc4b7524`.

### Reviewer disposition

RV-27, RV-28, RV-30, RV-31 and RV-22 are accepted for the claims stated. RV-29 remains
open (Medium) and needs the two checker fixtures. The review gate for F00.5
still depends on the earlier findings' dispositions; this pass closes only the
fourth-pass items.
