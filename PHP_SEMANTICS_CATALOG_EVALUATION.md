# PHP catalog evaluation results

Updated October 10, 2026. The extended comparative campaign has completed. The
production semantic/checker evaluation and Phase F remain incomplete.

R01.5 cleanup-completion remediation is implemented in the current worktree:
evaluation and certificate replay reject unsupported abrupt replacements with
`UnsupportedCleanupReplacement`. Cleanup `Return` over a pending PHP throw is
the only replacement currently modeled. Tests cover every pending kind against
abrupt cleanup terminators, preservation of all pending kinds through `Resume`,
and a callback-throw branch/join/resume composition. This implementation still
awaits independent acceptance under F00.5.

The initial comparison is committed as `043f2c78`; the extended compiler and
framework evaluation is committed as `4e7d29a3`, and the CIR lowering follow-up
as `2b6afb95`.

Phase F implementation is now underway. A checker-independent
`crates/php-semantics` seed now contains schema v8 graph/event/domain-coverage types,
per-domain dependency lists, separate precision and assumption fields, canonical SHA-256 graph identities,
explicit base64 JSON encoding with binary round trips, and an evaluator with
explicit exceptional continuations and host-supplied joins/widening. Sixty
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
array-element reference binding for callback alias tracking; schema major 7
adds ordered diagnostic events and regenerates the embedded artifact.
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
byte-identical at that checkpoint to the then-current schema-v6 artifact (SHA-256
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

### R00.3 source-to-host matrix: PHP 8.5.4 `strlen`

Scope: locked source worktree commit
`e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03` (PHP 8.5.4), the checked-in
`strlen` artifact and `CatalogTypeHost`, and the listed runtime/checker tests.
“Closed” below means only the stated narrow fact is supported; it does not
mean the whole PHP operation or its graph transfer is proven sound. Runtime
positive observations do not establish unobserved negative cases. The graph
is an author-encoded model, and its source evidence does not bind every PHP
opcode path.

| PHP behavior and primary source | Host/artifact transfer, affected components, and relevant catalog domains | Assumptions and evidence | Disposition |
| --- | --- | --- | --- |
| **Direct `ZEND_STRLEN` path.** `zend_compile_func_strlen` accepts one argument, folds constant strings, and emits `ZEND_STRLEN` for nonconstant values; `zend_try_compile_special_func_ex` selects it for the name `strlen` (`Zend/zend_compile.c:4062–4079, 4975–4979`). The VM handler reads string length directly and invokes weak parsing only when required (`Zend/zend_vm_def.h:8756–8816`). | The checked-in graph evidence and LLVM witness describe `zif_strlen`, not this compiler-emitted opcode path. `CatalogTypeHost::StringByteLength` computes the normal result; no host event models the compiler choice or the opcode handler's strictness/coercion/error branches. Components: result, diagnostic, exception, coercion/callback state. Required: `Return`, `Diagnostic`, `PhpThrow`, `Invocation`, `UserRead`, `UserWrite`, `ExternalRead`, `ExternalWrite`, `Autoload`, `Output`, `Release`, `Retention`, `Fatal`, `Suspension`; only narrow normal `Return` observations exist. | Source locations verified in the locked source tree. Existing direct runtime sentinels establish byte-length results for `""`, `"abc"`, and `"é"`; weak/strict fixtures cover selected values. No digest or builder validation for `zend_compile.c`/`zend_vm_def.h`, and no artifact edge binds their semantics to the graph. | **Open / evidence gap RV-16.** Positive result behavior is observed; source-to-artifact correspondence and negative/exception behavior for `ZEND_STRLEN` are not established. |
| **Native `zif_strlen` body.** `ZEND_PARSE_PARAMETERS_START(1,1)`, `Z_PARAM_STR(s)`, then `RETVAL_LONG(ZSTR_LEN(s))` (`Zend/zend_builtin_functions.c:377–385` in the locked tree). | Artifact `StringByteLength(Argument0)` produces a normal integer result; `CatalogTypeHost` uses exact byte lengths for eligible constant strings up to 16 bytes and `int<0,max>` otherwise. Component/domain: normal result, `Return` only. Every other domain listed above remains open. | Assumption: exact PHP 8.5.4 target, supported source/layout/LLVM witness chain, and normal completion with a successfully parsed string. Positive cases: empty, ASCII, and multibyte runtime sentinels; checker exact-constant fixture. Contradictory case: a possibly applicable minor range is not claimed covered and falls back. No negative result is claimed for a successfully parsed string. | **Narrowly supported.** It proves byte-length transfer for the native body under the stated assumptions, not that all calls reach this body or complete normally. |
| **Weak scalar parsing and null diagnostic.** `zend_parse_arg_str_weak` converts eligible scalar types; null uses the nullable diagnostic helper before conversion (`Zend/zend_API.c:758–779`). `ZEND_STRLEN` has its own null deprecation and weak-parse branches (`Zend/zend_vm_def.h:8770–8808`). | Host predicates can classify null as Yes/Maybe/No and produce a diagnostic event; scalar values can reach the length operand. Components/domains: result (`Return`), null diagnostic (`Diagnostic`), and failure (`PhpThrow`). Graph closes a PHP 8.1+ null diagnostic fact for the artifact slice, but not the full parser outcome; `Invocation`, `Release`, and state/write domains are also relevant to Stringable objects and remain open. | Assumptions: weak typing, supported exact target, conventional error handler that records rather than throws, and tested scalar domain. Positive runtime pair checks `[3,3,1,0,0]` for int/float/bool/bool/null and the null deprecation on locked TAILCALL/CALL profiles. Strict runtime rejects those values; fixtures cover weak/strict and PHP 8.0/8.1 boundary. A throwing/custom error handler is not covered. | **Partially supported.** The observed weak conversions and diagnostic are closed only in those tested conditions. Strict failures, custom handlers, parser edge cases, and exceptional state remain open. |
| **Strict scalar parsing.** The VM checks `EX_USES_STRICT_TYPES` before weak parsing; failed parsing raises `TypeError` and has no normal result (`Zend/zend_vm_def.h:8760–8817`). | Checker argument validation rejects scalar coercion in strict mode. Components/domains: result (`Return`) and exception (`PhpThrow`); `Invocation` is not expected for a rejected scalar. The artifact's normal-return result is not a proof that the call returns. | Positive contradiction to weak coercion: strict runtime companion rejects `int`, `float`, `bool`, and `null`; strict checker fixtures reject coercion. No whole-path graph evidence for the VM's TypeError construction/state. | **Behavioral rejection tested; graph exception transfer open.** |
| **Object cast and `Stringable::__toString`.** Weak parser calls `cast_object(...IS_STRING)` and releases the temporary object on success (`zend_API.c:758–779`). Standard cast dispatch calls `__toString`, releases the object, validates its result, and can throw (`zend_object_handlers.c:2468–2495`). Strict mode bypasses this weak cast (`zend_vm_def.h:8760–8808`). | Host predicate recognizes registered `Stringable` classes in weak mode; `Event::Invoke` records the argument and returns a string with only a Normal exit. It does not represent callback writes, throw, invalid return, re-entrancy, or release/destructor behavior. Components/domains: state (`Invocation`, `UserRead`, `UserWrite`, `ExternalRead`, `ExternalWrite`, `Release`), result (`Return`), diagnostic (`Diagnostic`), and exits (`PhpThrow`, `Fatal`, `Suspension`). | Positive runtime observation: Stringable callback runs in weak mode; strict companion rejects without invoking it. Rust fixtures assert selected property invalidation behavior but are checker tests, not independent PHP source/runtime evidence for all mutation paths. No negative case proves all non-Stringable or trait-provided implementations are classified correctly; RV-9 and RV-17 remain open. | **Partially modeled; effects and exits open (R05.2, RV-9, RV-17).** Callback mutation, callback throw, invalid return, and release are not closed by this graph. |
| **Object release and destructor consequences.** Parser/cast source releases temporary object references; standard cast also releases the object (`zend_API.c:758–779`; `zend_object_handlers.c:2468–2495`). | No `Release` event exists in this host graph. The separate checker effect invalidation is not a model of reference counts, destructor order, or exceptions. Components/domains: host state, `Release`, `UserRead`, `UserWrite`, `ExternalRead`, `ExternalWrite`, and reachable exit domains (`PhpThrow`, `Fatal`, `Suspension`). | No positive/negative/contradictory source-runtime matrix for destructor effects, exceptions, or release timing in the artifact witness. | **Open.** Do not infer no effects from the absence of graph events. |
| **Unknown event.** There is no corresponding PHP source operation: it is a graph abstraction event. | `CatalogTypeHost::Event::Unknown` returns Normal with unchanged state, while graph coverage leaves non-Return/non-Diagnostic domains open. This no-op is not a conservative PHP unknown-call transfer. Components/domains: all state, result and exits; all 14 catalog domains (`Invocation`, `UserRead`, `UserWrite`, `ExternalRead`, `ExternalWrite`, `Diagnostic`, `Autoload`, `Output`, `Release`, `Retention`, `Return`, `PhpThrow`, `Fatal`, `Suspension`) are required to make a complete unknown-operation claim. | No concrete PHP test can validate an unspecified abstract event. The contract requires unsupported behavior to stay open; current checker host's no-op is implementation behavior, not evidence that effects are absent. | **Open / host-model limitation.** Fail-closed handling or sound unknown transfer belongs to R01.6 and broader host calibration. |

The locked `Return` coverage intentionally has no domain dependencies. Its
checked expression is `StringByteLength(Argument(0))`; the `__toString` event's
result is unused, and the graph contains no explicit `Read`, `Write`, or
PHP-flow state operation on the normal path. Thus the narrow nonnegative
byte-length result does not depend on callback effects. R01.6 makes this
exception checkable: changing the graph to consume an invocation result or to
execute a modeled state operation requires the corresponding dependency edge;
unknown events keep each declared affected domain open, and declared dependent
projections widen through those open domains.

The matrix distinguishes source facts from what the graph and checker actually
consume. The native `zif_strlen` chain has pinned source/layout/LLVM evidence;
the usual direct opcode path does not. Weak scalar positives and selected
diagnostics have runtime evidence, but callback effects, throw/release behavior,
unknown behavior, and strict exceptional transfers remain open. In particular,
the checker’s separately applied `Operation::String` invalidation must not be
counted as graph evidence for ordered callback mutation or exceptional-state
retention. Follow-up implementation and calibration are tracked under R01.6,
R02.2–R02.3, R04.1, and R05.2; RV-16 remains an artifact/source-linkage gap.

R02.2 adds a reproducible bounded crosswalk for the PHP 8.5.4 `zif_strlen`
normal demand. The builder checks the source hash and confirms that Clang's
AST/CFG represents the `zend_string.len` read and zval result assignment,
Clang's emitted translation-unit LLVM loads field index 2 and stores that same
SSA value into the zval result, and GCC GIMPLE/CFG retain the parser gate and
length-to-result assignment. The [generated correspondence record](tools/php-catalog/experiments/comparison/results/evaluation/strlen-representation-correspondence.json)
records hashes for all input artifacts. These representations agree on that
selected path; this does not establish equivalence for all branches or effects.
The builder binds the Clang summaries to the source-hashed representation
manifest and successful source compile records, and checks the GCC dump command
record and source path before accepting the GIMPLE/CFG artifacts.
It also checks the locked PHP commit, verifies the cleanup fixture hash against
the campaign input record, and requires successful cleanup AST, GIMPLE, and
LLVM command records before reporting the exception counterexample.

The cleanup fixture supplies a counterexample to assuming CFG and LLVM
exception behavior coincide: GCC GIMPLE records a `finally` destructor, and
Clang LLVM has unwind cleanup blocks, while the Clang AST CFG output contains
no explicit `Guard` destructor edge. The PHP `zend_builtin_functions.c` CIR
campaign failed to emit this unit, so CIR agreement is unsupported. Clang
analyzer campaigns here concern C fixtures rather than PHP's `zif_strlen`.
The [correspondence builder](tools/php-catalog/experiments/comparison/build_strlen_representation_correspondence.py)
fails when the pinned source, required IR anchors, or cleanup evidence drift.

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
semantic tests. At that checkpoint the updated semantic suite had 58 tests; that run reran
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
binding. At that checkpoint the semantics suite passed 58 tests. For this schema-v6
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

The authoritative Phase F task statuses, remediation actions, dependencies, and acceptance criteria are in the [PHP catalog Phase F and remediation ledger](PHP_SEMANTICS_CATALOG_LEDGER.md). The evaluation evidence and audit basis are recorded below.

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

The current PTA cannot close that gap by itself. The analysis is private to the
`php-catalog` binary crate and returns a text report; `php-semantics` has no
dependency on it or machine-readable target/coverage interface. Internally,
each call site does retain `data` and `anything` fallback flags alongside
points-to targets, but the report counts those flags separately and lists known
function targets without a per-target completeness record. Reuse therefore
needs an explicit artifact boundary and a demand-oriented summary adapter,
including a rule that never treats known targets as exhaustive while either
fallback flag is set. No such adapter or native post-fixpoint validation is
implemented. This is an integration limitation, not a finding that the
existing PTA is unsound for its current reporting purpose.

An additional F05 implementation step in `master3` makes exact-8.5.4 weak-mode
null deprecation a typed graph event. Argument checking obtains both that
diagnostic and the return result from one evaluator run; later return inference
reuses the call memo. The artifact builder validates the paired runtime witness
and records it as diagnostic evidence. Fourteen focused `strlen_` checker
fixtures pass, including exact-patch weak and strict routing; all 60
`phpstan-rs-semantics` unit tests pass. Checker unit coverage verifies that
same-span narrowings keep separate memo entries, preserve distinct diagnostic
outcomes as well as returns, and clear on a file change. Its targeted gate
profile test passes. The artifact builder now verifies the referenced weak and
strict PHP fixture hashes, both captured output hashes and cross-VM equality,
the output JSON's equality with the declared expectations, and the referenced
TAILCALL/CALL profile identities and locked binary hashes before accepting the
runtime witness; it also pins the exact null-deprecation text. Builder
regressions confirm a changed fixture digest and an output/expectation mismatch
are rejected. A follow-up F01 fixture uses a first callback result to select a
branch before the second callback; the current semantic suite passes 60/60.
Other target ranges retain their legacy diagnostic fallback, and the remaining
effects and throws still do not share this graph evaluation, so F05 remains
open.
Re-running the catalog
builder with the locked PHP source, LLVM witness, and paired runtime evidence
produces byte-identical checked-in JSON. The embedded-artifact assertion now
requires the new closed Diagnostic evidence and preserves open status for every
other non-Return domain. The full `phpstan-rs` library suite passes (1,202/1,202).

On October 9, 2026, the source-to-artifact chain was replayed from the cached
PHP source at commit `e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03`, linked
`php-cli.bc`, the paired TAILCALL/CALL runtime witness, and the checked-in
manifest. LLVM witness extraction and `build_strlen_catalog` completed, and the
then-current manifest matched byte for byte (SHA-256
`928106923b27db0ed4ee7069d6f5e1dca86284545760189a3162dfcf20c9b8f2`). The
optimized fixture command `cargo test --profile gate -p phpstan-rs --test
fixtures -- strlen_` passed all 14 selected fixtures. This confirms the
replayable narrow Return/Diagnostic path; it does not close the other native
domains or validate a demand summary.

The linked `zif_strlen` witness now includes a complete opcode-family inventory
in [`strlen-llvm-operation-inventory.json`](tools/php-catalog/experiments/comparison/results/evaluation/strlen-llvm-operation-inventory.json): 241 instructions, including 134 storage operations, 9 layout accesses, 58 scalar operations, 7 calls, 32 branches, and one return. The extractor test suite verifies all supported instruction groups, direct versus indirect call accounting, rejection of an unknown opcode, and rejection of a missing return. This is accounting evidence only: the catalog graph still leaves effects and non-normal outcomes open, and it does not claim to interpret all 241 instructions.

The builder checks the inventory against the pinned opcode counts, grouped
counts, and direct call targets before generating the manifest. It records a
separate inventory evidence item and makes the LLVM Return transfer evidence
depend on that item, so the checker validates inventory provenance as part of
the artifact. Tests reject missing opcodes and invented call targets. The
updated manifest regenerates byte for byte with SHA-256
`0beb950a5f9c55a4f6e027aba86a8b7447708e283b56fae5af5849b6cbecd1ef`; all 60
semantic library tests, the then-current builder-example tests, the embedded checker
artifact test, and all 14 focused optimized checker fixtures pass. This closes
an evidence-accounting gap for the native Return slice, not the demand-summary
or semantic-operation gaps in F03.

The follow-up witness also derives a narrow Return demand: both parser output
destinations resolve to local slot `%17`; the demanded string length is loaded
from `_zend_string` field 2 into `%193`; that value is stored through `%195` in
the caller-provided return-zval slot `%16`. The builder validates that chain,
records a `strlen-native-return-demand` evidence item depending on pinned source,
layout, parser, and opcode-inventory records, and makes the linked LLVM
transfer depend on it. Builder tests reject a mismatched parser slot and any
attempt to mark effects closed. This remains a source-specialized demand slice,
not reusable native CFG/memory solving. The updated witness hash is
`cf1434af948728a6810a9a48195a075e1c3e3dc2edae5b0c633925b811995589`; the
updated manifest hash is
`277d23413c20fd5da6980f3d508c408689282844b92cce070ae86d47625fe631`. The
refreshed run passed all five builder-example tests, and the optimized checker
gate again passed all 14 `strlen_` fixtures.

A later optimized gate-profile run of `cargo test --profile gate -p phpstan-rs
-p phpstan-rs-types -p phpstan-rs-semantics` did not pass as a whole: 1,198 of
1,202 checker library tests passed, while four unrelated project/registry
tests failed. Each of those four failures reproduced when run alone in the
current tree. The deferred-registry failure also reproduced against the parent
commit `3129ece5`, before the typed diagnostic integration; the other three
have not yet been checked against that control. The optimized `strlen_` gate
fixtures (14/14), semantic library tests (58/58), the Debug-profile checker
library suite (1,202/1,202), crate checks, and deterministic artifact
regeneration pass. The unrelated optimized-profile failures remain open and
should be resolved or separately accepted before claiming a clean full gate
run.

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

#### R00.4 CBMC property accounting and enforcement

The three checked-in JSON UI transcripts were parsed property by property.
The 93 properties in `cbmc-contracts.out` all report `SUCCESS`, with
`cProverStatus=success` and `VERIFICATION SUCCESSFUL`. The invalid-composition
run retains those same 93 successful property IDs and adds only
`main.assertion.11` as `FAILURE`; its command exits 10, with
`cProverStatus=failure`. The `cbmc-low-budget` run has
`main.assertion.1=SUCCESS` and `main.unwind.0=FAILURE`; the latter is the
expected unwinding assertion from `loop_contract.c` at `--unwind 2`, and that
command also exits 10. This is a boundedness negative control, not a failed
semantic invariant.

| Recorded properties | Count and outcome | Contract mapping and evidence limit |
| --- | --- | --- |
| `main.assertion.1`–`.3` | 3, all successful | The loop computes `value`, callback count, and each result for a nondeterministic length constrained to at most 3 (`contracts.c:34–39`). These are C-only harness invariants; no distinct contract clause or PHP/Rust test asserts this bounded loop relation. |
| `main.assertion.4`–`.5` | 2, successful in the valid run | A throwing callback increments state, then the wrapper records release and does not append the later normal event (`contracts.c:41–42`). This illustrates the contract's callback-write-before-throw and no-normal-continuation counterexample row. The CBMC property is C-only; Rust has the analogous `callback_throw_preserves_prior_write_and_skips_later_events` regression, while PHP runtime coverage observes a narrower Stringable write/throw case. Neither derives this C assertion from PHP source. |
| `main.assertion.6`–`.7` | 2, successful | A normal callback path records callback, release, and normal-continuation order (`contracts.c:43–45`). The CBMC property is C-only; corresponding event order is separately exercised by Rust semantic fixtures and PHP runtime callback-order sentinels, but those do not prove this C harness property. |
| `main.assertion.8`–`.10` | 3, successful | Same-address alias writes (`:46`), a callback copied through an aggregate (`:47–48`), and a retained callback and pointer (`:49–50`). The contract has no corresponding aliasing, aggregate-copy, or retained-pointer clauses. These are C-only examples; the harness only reaches the two known callback functions, and the PHP/Rust evidence does not check these exact C properties. |
| `main.array_bounds.*` | 10, all successful | Generated CBMC array-bound checks across `main`, `increment`, `throwing`, `release_value`, and `invoke`. They establish harness memory safety for this bounded C model, not catalog semantics. |
| `*.overflow.*` | 7, all successful | Generated signed-overflow checks in those C functions. Harness-safety evidence only. |
| `*.pointer_dereference.*` | 45, all successful | Generated dereference checks across `main`, callbacks, `invoke`, alias writes, and the copied callback. Harness-safety evidence only; they do not prove unknown PHP callback behavior. |
| `memcpy.pointer*` | 16, all successful | Two generated `pointer` object-bound checks plus 14 pointer-arithmetic checks for the aggregate copy. Harness-safety evidence only. |
| `through_copy.precondition_instance.*` | 3, all successful | CBMC's `memcpy` overlap/readable/writable precondition instances. Harness-safety evidence only. |
| `main.unwind.0`–`.1` | 2, both successful at `--unwind 4` | Unwinding assertions for the bounded loop (`length <= 3`). These show that this run's chosen bound covers this loop; they are not unbounded proofs. |
| Invalid-run `main.assertion.11` | 1 expected failure, at `contracts.c:55` | Deliberately asserts that callback mutation and pending-throw state were erased. The counterexample rejects that invalid composition in the reduced C model. The same contract clause has an analogous Rust regression; this CBMC counterexample itself remains C-only and is not a PHP semantic proof. |
| Low-budget `main.assertion.1` | 1 success, at `loop_contract.c:8` | Asserts `count == length` on paths represented with `--unwind 2`. It is C-only and has no PHP contract clause. Since `main.unwind.0` fails in the same run, this local success does not establish the assertion for all allowed lengths. |
| Low-budget `main.unwind.0` | 1 expected failure, at `loop_contract.c:7` | With `length < 100` and `--unwind 2`, CBMC finds a possible path beyond the unwind bound. The failure correctly prevents treating the incomplete bounded run as proof. |

For an exact name-to-disposition crosswalk, the valid run's 93 successful IDs
are grouped below. These generated checks all map to C harness safety rather
than a PHP semantic clause; the explicit assertions' semantic mappings are in
the table above.

| Property family | Exact IDs in the valid run | Clause/evidence disposition |
| --- | --- | --- |
| Main assertions (10) | `main.assertion.1`–`.10` | `main.assertion.1`–`.3` and `.8`–`.10` are C-only harness invariants with no matching contract clause; `.4`–`.5` illustrate the callback-write/throw clause; `.6`–`.7` illustrate ordinary event order. Related Rust/PHP checks are described above and do not prove these C properties. |
| Array bounds (10) | `main.array_bounds.1`–`.2`; `increment.array_bounds.1`–`.2`; `throwing.array_bounds.1`–`.2`; `release_value.array_bounds.1`–`.2`; `invoke.array_bounds.1`–`.2` | Generated C-model safety checks; no PHP contract clause. |
| Overflow (7) | `main.overflow.1`; `increment.overflow.1`–`.2`; `throwing.overflow.1`–`.2`; `release_value.overflow.1`; `invoke.overflow.1` | Generated C-model safety checks; no PHP contract clause. |
| Pointer dereference (45) | `main.pointer_dereference.1`–`.7`; `increment.pointer_dereference.1`–`.6`; `throwing.pointer_dereference.1`–`.6`; `invoke.pointer_dereference.1`–`.7`; `alias_write.pointer_dereference.1`–`.12`; `through_copy.pointer_dereference.1`–`.7` | Generated C-model safety checks; no PHP contract clause. The harness restricts callback targets to `increment` and `throwing`. |
| `memcpy` object/pointer checks (16) | `memcpy.pointer.1`–`.2`; `memcpy.pointer_arithmetic.1`–`.14` | Generated C-model safety checks for the aggregate-copy example; no PHP contract clause. |
| `memcpy` preconditions (3) | `through_copy.precondition_instance.1`–`.3` | Generated C-model library precondition checks; no PHP contract clause. |
| Unwinding assertions (2) | `main.unwind.0`–`.1` | Bounded-loop completeness checks only; no PHP semantic clause. Both succeed in the `--unwind 4`, `length <= 3` run. |

The invalid-composition run adds exactly one property to that valid-run
inventory: `main.assertion.11=FAILURE`, mapped above. The low-budget run's
complete two-property inventory is `main.assertion.1=SUCCESS` and
`main.unwind.0=FAILURE`; both are C-only, and the failed unwind means the
successful assertion result is not a complete proof of the loop property.

All 93 valid-run properties are therefore accounted for: 10 explicit
assertions, 81 generated bounds/overflow/pointer/precondition checks, and 2
unwinding assertions. The invalid run has the same 93 plus the intentional
counterexample, for 94 total. Only assertions 4–7 and 11 connect to rows in the
PHP semantic contract, and only as reduced examples. All CBMC results are
C-model-only; selected contract clauses have analogous independent Rust or PHP
tests, as called out in the table, but those tests neither instantiate nor
prove the C properties. No CBMC property instantiates the PHP evaluator or
validates its host transfers.

`campaign.py` first checks the locked CBMC executable, the `contracts.c` and
`loop_contract.c` source digests, and the checked-in reference transcript
digests. It pins each run's sorted property-ID/status map and a digest of the
complete normalized property records, including descriptions, source
locations, and counterexample traces, then fails closed
unless the run has the exact expected records, summary, and process exit:
all-success/exit-zero for the valid run; only `main.assertion.11` failing/exit
10 for the invalid run; and only `main.unwind.0` failing/exit 10 for the
low-budget control. It rejects duplicate IDs and loads references before
writing run outputs. Ten Python regressions cover the recorded cases, missing
properties, changed outcomes/descriptions/source locations/traces, incorrect exits,
binary/source/reference digest drift, and unreviewed modes. All three runs
were invoked through the actual `EnforcedRunner`, including its systemd/cgroup
resource proof, and validated with the expected 93/94/2 property counts and
exit codes 0/10/10. Their output hashes, measured exits,
resource-enforcement results, and source hashes are in the evaluation lock.
The harness source and its `--unwind` settings remain C-model
evidence; this enforcement closes only the campaign's CBMC result-accounting
gap.

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


## Task-by-task audit and remediation record

The task-specific audit findings and remediation actions are folded into the
authoritative [Phase F and remediation ledger](PHP_SEMANTICS_CATALOG_LEDGER.md). The audit examined the contract, evaluator and certificate
replay, manifest/catalog and target selection, PHP value/reference host,
`strlen` builder/extractor, checker effect bridge, counterexample C model,
comparison records, and checked-in evidence. It traced current implementation
and tests against each task; this is an author audit, not independent semantic
acceptance.

The inspected work maps to these task histories: F00 (`599c2508`, `5df391b9`,
`e62c000e`); F01 (`9df6d5d3`, `80795f6e`, `d5f7598f`, `7cb0c627`, `d3c3fa63`,
`eee12e0f`, `a0b083a8`, `dab7e44f`); F02 (`043f2c78`, `4e7d29a3`, `2b6afb95`,
`47f5e62a`, `08499695`, `0a2f229e`, `8217e557`); F03 (`74229f12`, `fe1cab81`,
`9b384e8c`); F04 (`bd1b0272`, `68ec8f18`, `a0b083a8`, `017fe4c7`, `dab7e44f`);
F05 (`9b1c449e`, `bb274ed4`, `df437149`, `d6bcf2c1`, `93d950ee`, `ea4f4b50`,
`09acd24e`); F06 (`043f2c78`, `4e7d29a3`, `2b6afb95`).

Fresh verification recorded for the reviewed tree: semantic crate 60/60;
`build_strlen_catalog` example tests 5/5; focused optimized `strlen_` fixtures
16/16; LLVM inventory and SVF replay Python tests 5/5. The full optimized
checker library suite passed 1,198/1,202, and fixtures passed 3,954/3,962; the
four library and eight fixture failures remain separately tracked under F05.7.
The rebuilt LLVM witness/catalog matched the checked-in artifact
byte-for-byte (SHA-256 `b55221654574043a15d50dfd862377863a39e2272b5aab148c7da35b74d1e33b`);
the historical callback runtime replay matched the checked-in output
(`2bd72db611daeac4b383eed0b0352ac80581ab6c7e58e8cb13a8b505f9411c04`). The
multi-hour compiler/CIR and whole-link SVF campaigns were not repeated.

The six adversarial findings map into the same ledger: ACR-1 → F00.1/F01.4/F01.5;
ACR-2 → F00.3/F01.1/F01.5; ACR-3 → F00.1/F00.4/F01.1; ACR-4 → F00.1/F01.4;
ACR-5 → F00.2; ACR-6 → F00.3/F02.5/F01.5. Their remediation IDs appear in
the corresponding rows in the [authoritative ledger](PHP_SEMANTICS_CATALOG_LEDGER.md); the ledger is the single status source.

## Commit-by-commit Phase F review

The earlier task-history list named only 28 milestone commits. A complete path-scoped history query finds 108 commits touching Phase F implementation, tests, evaluation records, or comparison evidence. I reviewed those changes by implementation/evidence area against the current code and tests; the list below records the full commit inventory. This was an author review; it does not satisfy F00.5's independent-review requirement. `git show --check` passed for the implementation and documentation changes. It reported trailing whitespace in captured raw compiler/SVF logs in `043f2c78`, `4e7d29a3`, and blank EOF lines in `fe1cab81`; these are retained tool outputs, not source-code defects.

- **F00:** `599c2508` introduced the trace contract; `5df391b9` recorded the CBMC result; `e62c000e` revised the contract and added the self-review. The contract is still not independently accepted. At the time of the commit-by-commit review, the CBMC replay needed a fail-closed result checker and property-to-clause crosswalk; R00.4 has since completed that scoped accounting and enforcement work. The reduced C model remains separate from PHP semantic validation.
- **F01:** `9df6d5d3`, `80795f6e`, `d5f7598f`, `7cb0c627`, `d3c3fa63`, `eee12e0f`, `a0b083a8`, and `dab7e44f` add the graph, worklist, catalog/evidence boundary, PHP flow host, certificate replay, and array/reference fixtures. The focused semantic tests pass 60/60 in the reviewed historical state. The implementation remains a trusted-host post-fixpoint checker, not a proof of PHP semantics. R01.1 supplies typed assumptions and fail-closed reference validation; R01.2 rejects cycles across the combined evidence/assumption graph; R01.3 validates catalog-entry release ranges at insertion and manifest load; R01.4 checks N/A dependencies and propagates open dependencies transitively. Trusted validators and canonical-order enforcement remain under R01.7–R01.8.
- **F02:** `043f2c78`, `4e7d29a3`, `2b6afb95`, `47f5e62a`, `08499695`, `0a2f229e`, and `8217e557` add compiler/framework campaigns, CIR records, and the source-to-LLVM `strlen` witness. The witness is pinned to one linked module and demonstrates a narrow Return slice; it does not interpret general LLVM behavior or establish source/CFG semantic equivalence (R02.2–R02.3). R02.1 now rejects runtime fixture drift and validates the preserved Hybrid/CALL output digests. The historical 13-sentinel witness still does not match the later current fixture, so replay fails closed until the evidence is regenerated for that fixture.
- **F03:** `74229f12`, `fe1cab81`, and `9b384e8c` document pointer-service limits and add the empty-target regression. The capsule omits construction/handler initialization, so its empty set is incomplete. The replay records the supplied bitcode hash but labels it with a fixed PHP source commit without checking the association; it also does not reject callback fixture hash drift (R03.1, R03.4).
- **F04:** `bd1b0272`, `68ec8f18`, `017fe4c7`, plus shared changes in `a0b083a8` and `dab7e44f`, add the bounded value/reference host and conservative missing-read behavior. The fixtures establish the stated minimum cases only; broader PHP value semantics and syntax lowering remain deferred (R22.1).
- **F05:** `9b1c449e`, `bb274ed4`, `df437149`, `d6bcf2c1`, `93d950ee`, `ea4f4b50`, and `09acd24e` add patch-aware selection, the exact `strlen` artifact path, scalar/null coercion, memoization, and invocation invalidation. Targeted tests pass. A temporary `strlen(...[$value])` fixture confirmed that the callback effect widens a captured property as expected, so unpacked arguments are not recorded as a separate defect. Profile applicability, ordered callback writes/throws, and full-suite failures remain open (R05.1–R05.7). The optimized library and fixture suites each retain their independently tracked failures.
- **F06:** comparison evidence is introduced by `043f2c78`, `4e7d29a3`, and `2b6afb95`. It supports the measured feasibility and backend-limit findings, but no same-slice regional-versus-pointer comparison or calibrated solver cost comparison exists. Review also found a startup race in the serial campaign: it proceeds if the first worker has not created its proof file yet (R06.1–R06.4).

Complete path-scoped commit inventory (short hashes; sorted newest to oldest): 966a6551, 96a61797, e62c000e, e7c86279, 09acd24e, f11e2929, 65f882e9, 514ffa4b, 9b384e8c, fe1cab81, 8217e557, 0a2f229e, 6617138c, 74229f12, 5df391b9, 2d5cc024, 8b99cb1b, 950e7c62, eec59a76, 6a8bbb87, 75d401ad, 71f72f01, efb89e77, c29a0e7a, ea4f4b50, 3129ece5, 946923cf, 93d950ee, 7dceb947, d6bcf2c1, df437149, aa82db3a, abb60829, 2d1a892b, a67b022b, df638086, e67a7da1, 4ff2693f, 644350cf, c837fd62, 4d7cc348, 08499695, 9a367ab9, b32a7358, 4a27e40e, 5fcb5dab, 4e347e3a, 721d4069, 47f5e62a, 3fbd12ab, 1fdf7799, dab7e44f, 017fe4c7, e2c51043, a0b083a8, 2a07c705, 0e3bbb60, d3c3fa63, 7cb0c627, 68ec8f18, 7e74e69f, eee12e0f, 1ad343f3, 70423e3a, 1adfb365, 653cc29d, 42d85b64, 4384ad56, eec5048f, bb274ed4, 4be941db, 9f27e717, 25477020, 9b1c449e, 1fb4946b, 0208408a, 1d74e504, 71542c92, 10286cf2, 93d0225c, c1886086, 0c0a0096, 68e74282, c88d7209, 9d077079, b44de3b0, 2cb6b116, e058a8cf, 987a6d01, d5207a4e, 599c2508, db9bd8d4, 7e670a1a, c2f93b41, f4530908, 4bdd9302, 96c520c5, d5f7598f, 80795f6e, d938f622, bd1b0272, da2388ed, 5ec18631, 9df6d5d3, 130ff219, 2b6afb95, 4e7d29a3, 043f2c78.

Additional verification during this review: `cargo test -p phpstan-rs-semantics` passed 60/60, and the focused checker catalog tests passed 4/4. These targeted checks do not replace the recorded full-suite runs; the optimized full checker suite still has 4 library and 8 fixture failures.

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
