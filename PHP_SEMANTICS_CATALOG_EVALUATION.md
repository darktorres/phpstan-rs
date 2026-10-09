# PHP catalog evaluation results

Updated October 9, 2026. The extended comparative campaign has completed. The
production semantic/checker evaluation and Phase F remain incomplete.

The initial comparison is committed as `043f2c78`; the extended compiler and
framework evaluation is committed as `4e7d29a3`, and the CIR lowering follow-up
as `2b6afb95`.

Phase F implementation is now underway. A checker-independent
`crates/php-semantics` seed contains schema v6 graph/event/domain-coverage types,
per-domain dependency lists, separate precision and assumption fields, canonical SHA-256 graph identities,
explicit base64 JSON encoding with binary round trips, and an evaluator with
explicit exceptional continuations and host-supplied joins/widening. Fifty-eight
tests exercise
callback result propagation, write-before-callback order, callback throw
skipping later events, disjunctive normal/throw outcomes, pending-throw
cleanup/resumption, cleanup return overriding a pending throw, native unwind
separation, domain-masked unknown transfers with consistency checks, total
coverage/evidence validation, composed-child coverage propagation and rejection
of incomplete composed coverage, unresolved-return rejection, rejection of a
closed return refinement with an open throw dependency, state-sensitive
post-invocation predicates, conservative handling of unsupported predicates,
and loop convergence. A PHP-specific `SemanticHost` now runs through the shared
evaluator with PHP values and reference state. Its callback fixture maps
callback parameter writes through invoke operands and preserves separate
normal/throw states; an unknown callback havocs tracked state, emits normal,
PHP-throw, native-unwind, fatal, and suspension paths, and opens all effect
domains. Reads and unsupported operations also remain open. The evaluator rejects host join or widening results
that fail to subsume both incoming states/results; state-loss, result-loss,
and widening-loss fixtures cover that guard. A serializable certificate binds
the graph digest, initial state/arguments/receiver, solution, exits, and merged
coverage. Its separate one-step replayer checks every transfer from each
invariant frame, every observable exit, and that persisted coverage is no
narrower than replayed coverage. Round-trip
fixtures reject an omitted successor, omitted exit, dropped state/result/
pending outcome, and changed graph. Replay still trusts the supplied host's
semantic transfer functions; it does not establish that those functions match
PHP source behavior.
Domain projections now include transitive coverage
dependencies, validation rejects closed domains that depend on open domains or
on uncertified closed cycles, and evaluation propagates open child domains
through parent dependencies. These are foundation tests only: loop widening
has a small interval-like fixture, not a calibrated production domain, and the
replayer trusts the host's transfer semantics. The checker
bridge currently uses a narrow type-only host for `strlen()` and does not model
effect state, callback invocation, or exception integration. F00/F01 remain
OPEN. Composed host event outcomes now carry
per-domain coverage, precision, evidence, and assumptions; evaluation merges
those records so an open child opens the corresponding closed parent domain.
Schema major 4 added PHP value/reference flow instructions and regenerated the
embedded `strlen()` manifest; v3 manifests are rejected explicitly. Schema
major 5 adds keyed array get/set flow instructions, and the embedded artifact is
regenerated again for the current wire format. Schema major 6 adds explicit
array-element reference binding for callback alias tracking.
Canonical manifests now validate graph
and evidence digests and round-trip byte strings through explicit base64 JSON.
A shared target-set primitive distinguishes exact releases from minor ranges,
retains correlated scenarios, and keeps a Maybe-applicable graph alongside an
uncovered remainder. Catalog variants use it to preserve exact and partially
known target branches. The checker now has a thin adapter from its configured
PHP major/minor range to those scenarios; it leaves platform facts unknown and
preserves unknown release intervals before, between, or after known minors as
uncovered scenarios. Five adapter tests cover exact, unset, future-minor, and
patch-exact ranges. Exact patch releases are now parsed separately from the existing
major/minor language-gate projection, retained in project/registry settings,
included in canonical registry identity, and converted to exact catalog target
ranges while platform facts remain unknown. Config, registry, adapter, and CLI
fixtures exercise `8.5.4`; `cargo check -p phpstan-rs` passes. An unset version
maps to every language version recognized by the checker (5.3–8.5). This
verifies target propagation only; a separate exact-release `strlen()` slice is
now wired into the builtin call path as described below. The crate also
has an initial PHP value/reference state with array-copy, nested-array,
reference-cell, alias-rebinding, unset, and object-identity tests. The state now
backs the PHP-specific host. PhpFlow covers local assignment, unset, reference
binding, and literal-key array get/set. Graph fixtures verify copy isolation
and object identity through evaluation; another binds a reference before
callback writes on normal and throwing paths. Missing array keys remain open
transfers rather than modeled PHP diagnostics. These fixtures now satisfy the
Phase F minimum F04 evidence. Full key coercion, object properties, and
syntax-to-flow lowering remain C22 implementation work. An exact-entry catalog API validates
evidence, supports ASCII symbol matching, projection, and explanation lookup.
Catalog manifests also have a validated JSON ingestion API that checks
artifacts before installing runtime indexes; callback modeling remains bounded.

The first embedded artifact exercises the production catalog call path for
`strlen()` when the configured target is exactly PHP 8.5.4. The checked-in
manifest is validated through `Catalog::from_json` at checker initialization;
its Return projection computes exact byte lengths for up to 16 constant
strings and otherwise produces `int<0, max>`, while all non-return domains
remain explicitly open. Its evidence records the SHA-256 of the pinned
`Zend/zend_builtin_functions.c` and the `ZSTR_LEN(s)` return site. That file
is from the locked `php-8.5.4` source worktree. The Rust generator and
the exact-release fixture cover artifact generation, selection, and call-site
inference: under exact 8.5.4, `strlen('')` is inferred as `0`; for the
8.5 minor range, the possibly applicable graph is left uncovered and the
checker retains the broad legacy result. This is a manually encoded
source-transfer rule, not an imported compiler graph. The generator now takes
the source file as an explicit input, computes its digest from those bytes,
rejects anything other than the locked translation unit, and checks that the
`strlen` body still contains the `RETVAL_LONG(ZSTR_LEN(s))` transfer before
emitting evidence. A Clang 21.1.8 witness extractor also checks linked
`zif_strlen` LLVM: its debug line maps the return load to source line 382, it
loads `_zend_string` field 2 as `i64`, and stores that value as the return zval
payload. The pinned Zend layout header has SHA-256
`601ed337205b2258ff0de217d40aaf38afd3a52f67a8edf0f91182414a6bcccd`; the
extractor confirms field 2 is `size_t len` and the linked target is Linux
x86-64. The generator records this source-layout dependency and checks that it
agrees with the compiler access before emitting the PHP `StringByteLength`
bridge. For the source `Z_PARAM_STR(s)` sequence, it traces both strictness-gate
destinations through their stored pointers to the local string slot consumed
by the linked length load. It pins `zend_API.c` and checks source plus linked
helper evidence for both strictness-gate returns before weak parsing, scalar
conversion, nullable diagnostics, the indirect object cast handler, and object
release. These parameter and implicit-operation
domains remain open in the graph; the witness records identity and data flow,
not PHP coercion, diagnostic, effect, or exception semantics. The embedded
domains cite the pinned function source, Zend layout, parser source, and linked
LLVM evidence; the LLVM record depends on all three source records. The witness is
[`strlen-llvm-witness.json`](tools/php-catalog/experiments/comparison/results/evaluation/strlen-llvm-witness.json);
its linked module SHA-256 is
`e3427ce81f98f9a04db3e04ce822c3e0f1c731137f0d63e0440f862b1e61c93e`. The report
is recreated by [extract_strlen_llvm_witness.py](tools/php-catalog/experiments/comparison/extract_strlen_llvm_witness.py) from the locked source and
linked `php-cli.bc`. Regeneration from the locked source and report is
byte-identical to
the checked-in schema-v6 artifact (SHA-256
`cdde75d14942525b75529b2c122f35b325d8e91bb54c67566bf462a30203d484`); an
unrelated source input is rejected. It does not execute effects, callbacks, or
exceptional state.
The checker uses it only when every configured target scenario is definitely
covered, and falls back on uncovered or Maybe-applicable ranges. This is
concrete F05 progress, but it does not satisfy the full C24/C25 architecture
or eliminate legacy effect handling. A direct sentinel on the normal PHP
8.5.4 runtime returns `0`, `3`, and `2` for `strlen("")`, `strlen("abc")`,
and `strlen("é")`, respectively, confirming byte-length behavior. This is
confirmed in the 13-sentinel Hybrid/CALL comparison below and in the separate
16-sentinel TAILCALL/CALL reference run.

An October 9, 2026 comparison runs the same 13 positive PHP sentinels on
PHP 8.5.4 Hybrid and CALL runtimes. GCC 15.2.0 and `-O0 -g` match. The CALL
configuration disables GCC global-register variables because they select a
zero-argument handler ABI incompatible with CALL, and disables OPcache JIT to
avoid its separate tail-call helper incompatibility; both runtime commands use
`-n`, so OPcache is not loaded. The 13 names and comparison metadata are in
[`hybrid-call-8.5.4.json`](tools/php-catalog/experiments/comparison/results/runtime/hybrid-call-8.5.4.json),
and both captured output files are byte-identical. The CALL binary reports
`ZEND_VM_KIND_CALL`; its SHA-256 is recorded with the Hybrid binary digest.
Earlier builds without the VM-compatible global-register setting failed at
handler signatures, which identified the required configuration. This is
behavioral evidence for these positive cases, including direct/indirect
`strlen()`, callback order, throw-state retention, references, coercion, stream
and retained session callbacks. It does not prove equivalence for unobserved
behaviors or close the native source-to-graph linkage obligations.

A separate reference run exercises the current 16-case runtime fixture and a
strict-types companion on the hash-recorded PHP 8.5.4 TAILCALL and CALL profiles. Both outputs match and
include copied-array reference writes on normal callback return and during
`finally` before a pending throw resumes, plus a `Stringable::__toString()`
write followed by a thrown exception that remains visible to the catch block.
The strict companion confirms PHP throws `TypeError` without calling
`__toString()` under `strict_types=1`. The source, profile, binary, and output
hashes are recorded in
[`tailcall-call-reference-8.5.4.json`](tools/php-catalog/experiments/comparison/results/runtime/tailcall-call-reference-8.5.4.json);
the exact VM kinds and binary hashes are cross-checked against
[`profiles.json`](tools/php-catalog/experiments/comparison/results/runtime/profiles.json).

An earlier recorded full run on October 9, 2026 (`cargo test -p phpstan-rs -p
phpstan-rs-types -p phpstan-rs-semantics`) passed, including 1,201 checker
library tests, 3 checker binary tests, 45 CLI tests, 286 type tests, and 41
semantic tests. The current updated semantic suite has 58 tests; this turn reran
that suite and the targeted checker catalog test. The separate catalog fixture runs also pass for exact-patch
literal refinement and minor-range fallback. The run includes joined/widened
precision propagation, exact `8.5.4` config and CLI acceptance, registry target
conversion, and canonical identity separation across patches. Semantic
regressions reject inapplicable coverage for reachable and composed invocation
domains, and preserve fatal and suspension as distinct exits. This validates
the current checker build and the narrow `strlen()` call path; it does not
close the broader catalog execution or effect-integration obligations.

After the full run, a targeted CLI config round-trip also passes with
`php_version = "8.5.4"`, exercising config parsing through session creation and
the normal checker invocation.

After the replayable certificate and schema-v4 embedded artifact landed,
`cargo test -p phpstan-rs` passed on October 9, 2026: 1,201 library tests, 3
binary tests, 45 CLI tests, and the file-cache, scaling, stub-index, and version
integration suites. `cargo test -p phpstan-rs-semantics` passes all 54 tests,
including persisted certificate replay, omitted-successor/exit counterexamples,
and rejection of falsely closed replay coverage. The targeted embedded-catalog
checker test also passes. Schema v4 was necessary because adding a PHP flow event
changes the serialized graph format; regenerating the embedded manifest removed
a digest mismatch that the first full checker run caught. The subsequent schema
v5 added keyed array get/set instructions, and v6 added array-element reference
binding. The current semantics suite passes 58 tests. For this schema-v6
update, `cargo test -p phpstan-rs` also passes:
1,201 library tests, 3 binary tests, 45 CLI tests, plus file-cache, LSP-exit,
scaling, stub-index, and version integration suites. `cargo check -p phpstan-rs`,
edited-file rustfmt, and `git diff --check` pass.
Weak/strict `strlen()` binding now models scalar and `Stringable` coercions; its
null diagnostic is gated from PHP 8.1. All twelve `strlen_` gate fixtures pass,
including PHP 8.0/8.1, a range crossing the deprecation boundary, and strict-mode cases. The full checker library suite passes
all 1,201 tests, and the type library passes all 286 tests with the version feature.
TAILCALL and CALL runtime witnesses verify scalar lengths and the null deprecation.

## Phase F gate-by-gate status

The current [working semantic contract](PHP_SEMANTICS_CATALOG_CONTRACT.md)
turns the design rule into observable trace and composition requirements and
maps current tests to concrete invalid compositions. It is explicitly a draft,
so it does not close F00 or substitute for independent review.

| Gate | Evidence established | Remaining evaluation before closure |
|---|---|---|
| F00 — semantic contract | Ordered callback/write/throw/cleanup behavior is exercised in graph and CBMC reference fixtures; graph assumptions, coverage, precision, and open-domain reasons are explicit. A working contract draft maps invalid compositions to tests, including state-sensitive predicates across joins, partial result definitions, effect/throw refinement, and mismatched targets. A serialized graph-bound certificate replays each invariant frame and validates successor/exit completeness plus coverage non-narrowing. | Obtain independent review of observable traces, abstract order, boundary assumptions, and return refinement; validate that the supplied host transfer functions match PHP-source behavior. |
| F01 — graph/evaluator | Versioned graph/evidence schema, canonical digests, domain coverage/dependency checks, rejection of not-applicable reachable or composed invocation domains, explicit PHP/native exceptional outcomes, cleanup continuations, worklist convergence with join/widen precision propagation, child-coverage propagation, closed dependency-cycle rejection, graph-bound candidate containment plus persisted transfer/exit/coverage replay, and fail-closed handling of result IDs defined on only some predecessor paths are implemented. Missing, partial, or duplicate invocation coverage is rejected. The production checker now replays the evaluator certificate before consuming the catalog result. A PHP-host regression sends distinct normal callback results and writes through a CFG join and verifies both become unknown. | Review the reference contract fixtures against the written contract; add broader branch/callback combinations; replace the toy host with calibrated domains and validate host transfer behavior against source-derived fixtures. |
| F02 — compiler/source slice | Pinned PHP 8.5.4 source, LLVM import/link/inventory, Clang/GCC/CIR campaigns, indirect-call witnesses, and a reproducible 13-sentinel Hybrid-vs-CALL comparison are recorded. The comparison uses matched GCC/`-O0 -g` settings; CALL disables GCC global-register variables for its handler ABI and disables an unused JIT build feature. The embedded `strlen()` Return domain cites pinned `zend_builtin_functions.c`, `_zend_string` layout, `zend_API.c`, and a validated Clang 21.1.8 module witness: both `Z_PARAM_STR` helper destinations are traced to the local string slot consumed by the length load. A separate exact-8.5.4 Diagnostic rule now combines the parser's nullable diagnostic call path with the paired weak/strict runtime witness. Scalar string conversion, indirect object cast callback, and object release remain open operations. Paired weak/strict runtime witnesses show scalar conversion and `Stringable` conversion in weak mode, with strict mode rejecting before the cast callback. Int/float/bool/null values are accepted by weak `strlen`; null emits the recorded deprecation. The [PHP RFC](https://wiki.php.net/rfc/deprecate_null_to_scalar_internal_arg) sets that deprecation at PHP 8.1. TAILCALL and CALL outputs match for both modes in [`strlen-weak-scalars-8.5.4.json`](tools/php-catalog/experiments/comparison/results/runtime/strlen-weak-scalars-8.5.4.json). | These remain manually translated, narrow Return and Diagnostic slices. The evidence establishes identities and data flow, not general PHP coercion semantics, effect/exception behavior, nor broader graph correspondence to LLVM. |
| F03 — native summaries/pointer reuse | Full-link stock and compact SVF resource failures are measured; reduced SVF copy/initializer counterexamples and native runtime witnesses exist. The `strlen` bridge now connects pinned Zend layout and parser source through linked LLVM pointer flow to the graph transfer. It does not solve native memory or interprocedural summaries. | Implement demand summaries, coverage-aware pointer consumption/fallback tracking, and validate source-derived closed native graphs with an independent post-fixpoint checker. No native demand summary has passed this gate. |
| F04 — PhpFlowIR/value domain | **Phase F minimum fixture set complete.** PHP values distinguish unknown/null, copy arrays, preserve nested reference cells, preserve object identity, cover alias rebinding/unset, and execute array copy/mutation/read plus object assignment through graph instructions. A copied array containing a reference observes callback writes on normal return and cleanup-resumed throw paths (58 semantic tests); matching PHP 8.5.4 TAILCALL/CALL reference sentinels confirm both traces. A runtime sentinel also verifies that a `Stringable::__toString()` state write remains visible when conversion throws through `strlen()`. Missing array keys retain open normal/throw/fatal outcomes. Unknown callbacks retain five exit kinds, havoc tracked state, and open every effect domain. | No remaining Phase F fixture is identified. Broader key coercion, object properties, and syntax-to-flow lowering remain C22 phase work. |
| F05 — checker/targets | Catalog selection handles exact and partially known correlated target scenarios; manifests can be validated and installed from JSON; exact patch ranges flow through project settings. An embedded exact-8.5.4 `strlen()` graph is loaded at argument checking and return inference, and its post-fixpoint certificate is replayed before the result is consumed. Its Return and exact-target null Diagnostic outcomes share one memoized evaluation. Gate-profile fixtures pass exact literal refinement, nonliteral nonnegative-range inference, conservative minor-range fallback, and an exact-patch null-deprecation path. The checker reads file `strict_types` mode for this slice: weak-mode calls accept int/float/bool/null, explicit `Stringable`, a class with `__toString()`, and the tested unions, while strict mode and non-Stringable objects remain rejected. The catalog reports weak-mode null deprecation at exact PHP 8.5.4; configured ranges still use the legacy PHP-version-gated diagnostic fallback, including suppression for PHP 8.0 and strict calls. Other non-return domains remain open, and target selection requires definite full coverage. | Generalize source-derived coercion/diagnostic rules beyond this `strlen` case; execute effects, invocation, and exception behavior through a stateful `SemanticHost`; apply ordered callback/write/throw outcomes once across checker consumers; include platform/profile configuration and verify uncovered behavior end to end. |
| F06 — producer/solver decision | Comparative producer/compiler results and full-profile SVF resource failures rule out stock/layout-only SVF as mandatory whole-profile prerequisites; LLVM plus source metadata remains the initial producer. | Implement useful closed summaries, compare regional and audited pointer-service alternatives on the same slice, measure graph growth/RSS/runtime/checker cost, calibrate defaults, and record a final solver decision. No production solver is accepted. |

This audit found additional evaluation work beyond the earlier compiler/framework
comparison. The minimum F04 value/reference fixture task is complete, but its
broader C22 lowering work remains. F00 needs independent contract review; F01
needs broader branch fixtures and a calibrated host; F02 has configured
implicit-operation/source/layout/call-flow evidence for one `strlen` slice but
still needs semantic comparison of the wider native/PHP behavior; F03 needs
native demand summaries and pointer reuse beyond the direct Zend string-field
bridge; F05 needs stateful checker integration across effects and exceptions;
and F06 needs same-slice precision, cost, and solver evidence. The next dependency is an
independently reviewed observable-trace contract, followed by one locked 8.5.4
builtin slice driven from native import through graph validation and a checker
call. Until that exists, producer/solver choice remains provisional.

An additional F05 implementation step in `master3` makes exact-8.5.4 weak-mode
null deprecation a typed graph event. Argument checking obtains both that
diagnostic and the return result from one evaluator run; later return inference
reuses the call memo. The artifact builder validates the paired runtime witness
and records it as diagnostic evidence. Fourteen focused `strlen_` checker
fixtures pass, including exact-patch weak and strict routing; all 58
`phpstan-rs-semantics` unit tests pass. Checker unit coverage verifies that
same-span narrowings keep separate memo entries and that a file change clears
them. Other target ranges
retain their legacy diagnostic fallback, and the remaining effects/throws still
do not share this graph evaluation, so F05 remains open. Re-running the catalog
builder with the locked PHP source, LLVM witness, and paired runtime evidence
produces byte-identical checked-in JSON. The embedded-artifact assertion now
requires the new closed Diagnostic evidence and preserves open status for every
other non-Return domain. The full `phpstan-rs` library suite passes (1,202/1,202).

The remaining **approach-comparison** work in §2.3 is also explicit: compare
regional and adapted pointer-service solutions on the same useful closed PHP
summary; measure graph/region growth, checker/runtime cost, contract volume,
invalidation behavior, and adaptation maintenance; and repeat representative
older PHP/platform compiler cases. The current report measures compiler import,
resource use, selected framework queries, and concrete failure witnesses, but
has no complete semantic engine to produce those same-slice solver metrics.
The 8.4 import replay and Windows EH fixture are narrow compatibility evidence,
not that missing representative historical/platform evaluation.

This completes the inventory of current **Phase F and approach-comparison
evidence**, not the full project-plan implementation. The plan's later phases (0–4) also retain their own open tasks
for reproducible build capture, separate normal/CALL runtimes, NativeIR import
and link resolution, recursive summary solving, demand summaries, whole-project
cache invalidation, and end-to-end resource/regression gates. They are gated on
the Phase F slice and cannot yet be meaningfully accepted by the current tests.
The workspace test suite passing does not close those plan items.

Plan-wide status check: Phase F is still open, and none of the Phase 0–9 phase
gates has acceptance evidence sufficient to mark it DONE. The outstanding rows
are C00–C62: baseline/schema durability (C00–C05); native capture/import/link
resolution (C06–C11); the analysis kernel and pointer/summary closure (C12–C19);
target configuration and the real checker slice (C20–C27); native API and
return-rule migration (C28–C33); PHP body summaries and shared consumers
(C34–C40); implicit language behavior (C41–C47); artifact/cache/operational
hardening (C48–C52); historical, platform, and extension profiles (C53–C58);
and final correctness/resource acceptance and removal of duplicate authorities
(C59–C62). Some have partial experiments or scaffolding, but each phase gate
still needs its own required artifact and validation. These phases are not an
additional compiler-choice campaign; they are the implementation and acceptance
work that follows the feasibility decision.

## Findings that change the decision

**Stock SVF and the compact-layout adaptation both fail the 10 GiB resource gate
on the full minimal PHP CLI link.** Each was run under a Linux cgroup with
`memory.max = 10737418240` and `memory.swap.max = 0`, with no virtual-address-space
limit or sampled-RSS stop. The systemd journal explicitly records a kernel OOM
kill and a 10 GiB memory peak. These are actual whole-link failures, unlike the
earlier successful array/invocation translation-unit measurements.

| Candidate | Full-link outcome | Approximate elapsed time |
|---|---|---:|
| Stock SVF DDA | Kernel OOM kill at 10 GiB | 101 seconds |
| Compact-layout SVF DDA | Kernel OOM kill at 10 GiB | 150 seconds |

There was one enforcing attempt for each candidate. The times do not establish
a comparative speed ranking: both analyses failed before producing their query
results. Earlier attempts stopped at a virtual-address-space limit or sampled
RSS threshold and are retained separately in the durable scratch directory.

**The compact-layout change fixes one real problem, but does not make the
whole-program prerequisite acceptable.** An eleven-line addition to SVF's array
layout collection avoids materializing physical array elements when
array-sensitive modeling is disabled. It reuses the same field abstraction as
that pointer configuration; array-sensitive mode retains the original layout.
The adapted DDA used approximately 30 MiB process peak across payload sizes of
64 bytes, 64 KiB, 16 MiB, 64 MiB, and **256 MiB**. The previously measured stock
64 MiB payload needed approximately 2 GiB. The patch compiled and linked using
unchanged objects from the pinned SVF build.

Stock and adapted pointer targets agreed on the dispatch, absent-external-write,
and new nested-layout fixtures. **Agreement does not establish correctness:**
both miss concrete targets in the new fixture. Native Clang and GCC executions
validate nested aggregate copies, overlapping `memmove`, and negative indexing.
With defaults, SVF reports empty target sets at all four indirect sites. Enabling
`model-arrays` repairs the overlapping-copy site's target set, but nested-copy
and negative-index sites remain empty; enabling `model-consts` does not repair
those gaps. These are configured-analysis counterexamples requiring reduction
and model/transfer investigation, not proof that every SVF configuration fails.

**Implementation consequence:** keep LLVM plus source metadata as the initial
producer. Do not adopt stock SVF or the layout-only adaptation as a mandatory
whole-profile pointer prerequisite. Reuse remains an experiment involving
audited native capsules, input/boundary coverage, copy/initializer transfers,
and explicit open alternatives. No custom regional solver has yet been
implemented or accepted; these results do not prove it would pass.

## Workload and completed measurements

The workload uses the same PHP 8.5.4 source commit
`e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03`, Clang/LLVM 21.1.8, and pinned SVF
revision `e889a5139dbca132142b07d90192efab9a16c614` as the first comparison.

The actual configured CLI link contains **525 objects**. All **523 C objects**
were replayed into LLVM bitcode and imported successfully, including bundled
dependencies. Two native assembly objects remain explicit boundaries. The C
modules linked and passed LLVM verification. This is the complete link inventory
of the configured `--disable-all --enable-session` NTS CLI, not every extension,
SAPI, historical release, or publication profile in the plan.

The serial per-unit emit/import jobs total approximately 204 seconds. Linking
took 10.7 seconds and peaked at about 1,072 MiB of cgroup-charged memory; whole
verification, selected-root export, and `mem2reg` also completed below the cap.
These are warm-filesystem, single-campaign measurements, not a full hybrid
producer or semantic-analysis benchmark. Complete header dependency hashing
for all 523 C inputs is still missing; the earlier six-unit inventory exists.

The linked module contains **14,142 definitions and 3,727,445 instructions**.
The census includes integer/floating operations, vectors, phi nodes, atomics,
pointer conversions, switches, and **78 `callbr` operations**. Import success and
instruction inventory do not establish implemented semantic transfer coverage.

## Actual indirect-call witnesses

A new LLVM companion instruments every indirect-call site in the linked C
module with a stable ID within that locked module. The instrumented module
passes LLVM verification and links with the two original assembly objects.
The source-built instrumented runtime passes all **13 existing PHP sentinels**.

The module has **4,642 indirect sites**. Startup plus those sentinels observe
**529 sites and 307 distinct symbol targets**. The saved witnesses associate
site ID, enclosing native function, source position, and observed target symbol.
Unobserved sites remain open. These are positive native-target observations;
instrumentation of all observable PHP events and graph-path validation is still
unfinished. No full SVF result exists to compare against these witnesses.

A direct-call dependency audit of twelve selected roots also completed.
For example, `zif_strlen` conservatively reaches 996 native definitions,
291,347 instructions, 229 indirect sites, and 36 absent native bodies.
`zif_session_start` reaches 1,184 definitions, 334,484 instructions, 274 indirect
sites, and 53 absent bodies. The audit traverses all direct branches without
semantic pruning. It illustrates the importance of engine boundary contracts;
it is neither a demand summary nor a closure certificate.

## Complementary approaches

- **CBMC 6.6.0:** the reduced callback-result, write-before-throw, cleanup-order,
  aliased-write, aggregate-copy, and retained-callback contracts pass with
  bounds/pointer checks and unwinding assertions. A deliberately invalid
  composition is rejected. A loop whose possible bound exceeds its configured
  unwinding is rejected through the unwinding assertion. These are independent
  reference contracts, not full php-src verification or unbounded summaries.
- **Z3 5.1.0:** SMT-LIB child-process probes check contradictory guards,
  width-specific overflow, signed/unsigned comparison, extension casts, pointer
  identity, resource exhaustion, unsupported logic, and malformed input. Only
  valid `unsat` responses are classified as eligible to prune. Inconclusive and
  invalid responses retain paths. The harness now launches an actual child that
  exits 17 after consuming an SMT request; the recorded nonzero exit and stderr
  produce no pruning result, and the path is retained. This verifies the
  harness policy, not a production solver adapter.
- **Binary lifting/symbolic execution:** angr 9.3.4 lifts actual reduced binaries,
  and bounded symbolic execution reproduces five scalar/copy/index outcomes.
  Four PHP entry blocks also lift. Full PHP binary CFG recovery, high-level layout
  reconstruction, and PHP semantic summaries were not completed. The initial
  angr 10 installation exposed an API/dependency mismatch; it was replaced by a
  pinned compatible release, not treated as an approach failure.
- **GCC:** an independent GCC 15.2.0 PHP worktree built successfully under the
  enforced resource limit (peak 637 MiB, 136 seconds). Its CLI passes the same
  13 runtime sentinels. All 523 C compiler commands exited successfully. 505 units emitted CFG, EH,
  and GIMPLE dumps. The two generated Bison parser C files have hashes that
  differ from the Clang profile only in absolute `#line` source roots; they are
  byte-identical after normalizing those roots. The other 18 objects have no defined function symbols
  (some are data-only, others empty in this configuration), so GCC emitted no
  function dumps for them; their global data still needs explicit source/layout
  treatment. The checked-in profile summary verifies the symbol inventory.
  The full dumps are compiler output only: no normalizer, semantic importer,
  or PHP summary engine consumed them. The first comparison's six-unit GIMPLE
  imports used Clang-configured headers and remain a separate result.
- **CIR:** pinned LLVM 21.1.8 source was acquired. Its standalone Clang build
  explicitly rejects CIR; the combined LLVM/Clang/MLIR configuration succeeds.
  The CIR-enabled compiler built successfully in 4,845 seconds with eight jobs;
  its enforcing cgroup peaked at 6.34 GB of 10 GiB, with swap disabled and no
  memory-pressure or OOM events. CIR emission succeeded for 44/523 PHP C objects
  (8.41%); 479 failed, and all four targeted dispatch/platform/cleanup fixtures
  failed. Of the failed PHP commands, 204 ended in a frontend crash. Common
  unsupported forms include static assertions (246 first diagnostics), explicit
  pointer casts (89), function-to-pointer decay (34), and ptrdiff (31). The
  successful files total 11.8 MB, but were not passed through a separate CIR
  verifier or semantic analyzer during the emission campaign. A follow-up
  lowered all 44 emitted units through Clang's CIR-to-LLVM path: 36 produced
  LLVM IR and all 36 passed `opt-21 -passes=verify`; the other eight hit a
  CIR-to-LLVM backend assertion (LLVMFunctionType conversion), with no timeouts
  or resource kills. The largest unit (`ext/lexbor/lexbor/encoding/multi.c`)
  took about 245 CPU seconds and peaked at 302 MiB. This improves confidence in
  the subset that emits CIR, but 36/523 units (6.9%) is far short of profile
  coverage, and verifier success does not establish behavioral equivalence or
  CIR source-semantic completeness. CIR is not a viable full-profile producer
  in the tested configuration.
- **Clang CFG/dataflow:** the first comparison's exceptional-destructor omission,
  C rejection by FlowSensitive, and diagnostic-analyzer cutoff findings remain.
  No complete summary adapter was implemented during this extension.
- **Regional semantic engine, declarative graphs, and checker:** reference
  contracts/runtime witnesses exist. The shared graph evaluator, certificate
  replay, PHP reference-value fixtures, target selection, and a narrow
  catalog-backed `strlen` checker path are implemented. A native Zend summary
  producer, the full PhpFlowIR product domain, and stateful checker integration
  across return/effects/diagnostics remain unimplemented.

There is **no accepted production solver** and no generated closed PHP
BehaviorGraph. F04's minimum fixture task is satisfied, but F00, F01, F02, F03,
F05, and F06 remain open, so the Phase F gate as a whole is not complete.

## Evidence and resume instructions

Evidence is under
[`results/evaluation`](tools/php-catalog/experiments/comparison/results/evaluation/).
[The GCC profile summary](tools/php-catalog/experiments/comparison/results/evaluation/gcc-profile.json)
records all 523 unit commands and hashes for each emitted GIMPLE/CFG/EH dump.
[The CIR campaign summary](tools/php-catalog/experiments/comparison/results/evaluation/cir-campaign.json)
records every input, exact compiler command, exit status, diagnostic, resource
result, source hash, and emitted artifact hash.
[The CIR lowering summary](tools/php-catalog/experiments/comparison/results/evaluation/cir-lowering.json)
records the follow-up LLVM lowering and verifier result for each emitted unit;
the replay script is
[evaluate_cir_lowering.py](tools/php-catalog/experiments/comparison/evaluate_cir_lowering.py).
Raw CIR outputs/logs, GCC dumps, configured source trees, and binaries remain in
`/home/torres/.cache/php-catalog-evaluation`; the report names that location so
those files can be matched to the checked-in hashes.

Complete raw artifacts, linked bitcode, configured source trees, binaries, and
build caches are in **`/home/torres/.cache/php-catalog-evaluation`**. The original
Clang PHP/SVF build inputs still reside in `/tmp/php-catalog-comparison`; restore
them using the first experiment's README if that directory disappears. Exact
identities and package digests are in `results/evaluation/lock.json`.

Both extended compiler campaigns are complete. To reproduce them, run the GCC
and CIR stages separately using `finish_campaign.py --stage gcc` and
`finish_campaign.py --stage cir`, with the same cache path and source locks.
Each command uses serial enforcing cgroup jobs; do not run them concurrently.

Next work must also reduce the new SVF copy/initializer misses, implement and
validate coverage-aware pointer consumption, and build the actual Phase F
semantic/checker slice. Do not retry the failed full SVF runs after speculative
changes: test reductions first. Final acceptance still requires useful
source-derived closure, post-fixpoint certificates, target-scenario coverage,
runtime graph contradictions, and preserved-workload/resource gates.
