# PHP catalog evaluation results

Updated October 9, 2026. The extended comparative campaign has completed. The
production semantic/checker evaluation and Phase F remain incomplete.

The initial comparison is committed as `043f2c78`; the extended compiler and
framework evaluation is committed as `4e7d29a3`, and the CIR lowering follow-up
as `2b6afb95`.

Phase F implementation is now underway. A checker-independent
`crates/php-semantics` seed contains schema v1 graph/event/domain-coverage types,
separate precision and assumption fields, canonical SHA-256 graph identities,
explicit base64 JSON encoding with binary round trips, and an evaluator with
explicit exceptional continuations and host-supplied joins/widening. Twenty-four
tests exercise
callback result propagation, write-before-callback order, callback throw
skipping later events, disjunctive normal/throw outcomes, pending-throw
cleanup/resumption, cleanup return overriding a pending throw, native unwind
separation, domain-masked unknown transfers, total coverage/evidence
validation, unresolved-return rejection, and loop convergence. These are
foundation tests only: loop widening is generic and has only a small fixture,
with no calibrated abstract domains; the evaluator also has no catalog
selection, target semantics, real checker host, native producer, or publication
certificate, so F00/F01 remain OPEN. A shared target-set primitive distinguishes
exact releases from minor ranges, retains correlated scenarios, and keeps a
Maybe-applicable graph alongside an uncovered remainder; it is not yet wired
into catalog variants or the checker, so F05 remains OPEN. The crate also has
an initial PHP value/reference state with array-copy, nested-array,
reference-cell, and object
identity tests. An exact-entry catalog API now validates evidence, supports
ASCII symbol matching, projection, and explanation lookup. Target/scenario
selection and checker execution remain absent, so F04/F05 remain OPEN.
`cargo test -p phpstan-rs-semantics` and
`cargo check -p phpstan-rs` pass with the pinned `vendor/phpstan-src` submodule
initialized.

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
  invalid responses retain paths. The process-failure entry is a reference policy
  record; a real failed solver-process test remains to implement.
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
  contracts/runtime witnesses exist, but the production graph evaluator, native
  Zend bridge, recursive post-fixpoint validation, PhpFlowIR product domain,
  target selection, and real checker integration remain unimplemented.

There is **no accepted production solver** and no generated closed PHP
BehaviorGraph. None of F00–F06 should be marked DONE from this checkpoint.

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
