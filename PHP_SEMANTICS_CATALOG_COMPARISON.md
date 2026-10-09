# PHP catalog native representation and framework comparison

Measured on October 8, 2026. Decision record for §2.3 of `PHP_SEMANTICS_CATALOG_PLAN.md`.

**Extended evaluation record:** [the full-evaluation findings](PHP_SEMANTICS_CATALOG_EVALUATION.md)
adds the actual minimal CLI link, enforcing cgroup runs, an SVF layout adaptation,
and native target instrumentation. Both stock and adapted SVF fail the whole-link
10 GiB gate, so the successful slice timings below do not establish profile
feasibility. Full Phase F remains OPEN.

## Decision

Select **LLVM execution IR paired with Clang AST/type/layout metadata** for the first Clang-built profile. Clang AST/CFG remains the metadata and comparison surface. Its configured CFG failed a required C++ exceptional-cleanup case, so it cannot be the authoritative execution graph without additional lowering and validation.

**Do not select stock or layout-only adapted SVF as the whole-profile pointer prerequisite:** both were killed by the enforced 10 GiB cgroup gate on the actual configured PHP CLI link. The adaptation does reduce isolated array-layout costs, but it did not fix full-link feasibility or the nested/copy target misses. SVF remains a bounded-reduction research candidate only; F03/F06 must compare an auditable reduced reuse path against the regional candidate before any production solver is selected.

The regional Zend/PHP state, ordered behavior graphs, parameterized summaries, and their validator remain project-specific. Reusing a pointer solver does not implement those semantics. F03/F06 must compare the adapted reuse path against the regional candidate on actual closed summaries before final solver acceptance. This comparison selects the initial producer and narrows the solver work; **the full Phase F gate remains OPEN**.

A GCC-built CLI and full GIMPLE/CFG/EH command campaign were feasible under the resource cap, but 505 of 523 C units emitted GIMPLE/CFG/EH dumps; 18 successful invocations were data-only or empty and had no defined functions to dump. No GIMPLE importer or semantic analyzer was evaluated. GCC is a credible producer candidate, not yet a catalog profile. CIR-enabled Clang emitted output for only 44/523 PHP C units (8.4%); all four targeted fixtures failed, and 204 of the 479 failed PHP commands ended in compiler crashes. CIR is not a viable producer for this locked profile. Native compiler identity remains part of coverage; Clang replay of GCC source does not establish compiler equivalence.

## Locked workload and method

- PHP upstream tag `php-8.5.4`, commit `e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03`.
- Clang/LLVM **21.1.8**, Ubuntu build `6ubuntu1`; GCC **15.2.0**.
- SVF **3.4**, revision `e889a5139dbca132142b07d90192efab9a16c614`, built against that LLVM. Experimental Z3 dependency **4.13.3**; this does not change the plan's proposed production Z3 lock.
- Linux x86-64 NTS CLI configured with `--disable-all --disable-cgi --disable-phpdbg --without-pear --enable-cli --enable-session`, using Clang and `-O0 -g`.
- Six real translation units: `ext/standard/array.c`, `Zend/zend_builtin_functions.c`, `ext/session/session.c`, `ext/session/mod_user.c`, `Zend/zend_execute_API.c`, and `main/streams/streams.c`.
- Sixteen selected PHP/native entry functions cover array callbacks, count/object dispatch, scalar coercion, session retention, native invocation/exception handling, and stream operations. `zend_release_fcall_info_cache` was requested but not emitted in the invocation module; it is not counted as an imported entry.
- Reduced C fixtures cover conditional callbacks, aliased writes, cleanup ordering, handler slots, retained callbacks, copies, unknown external writes, TLS, GNU statement expressions/computed goto, atomics, and returns-twice/noreturn engine boundaries. A C++ fixture covers normal and exceptional destruction on Linux and an MSVC target.

The AST adapter uses Clang's actual `CFG::buildCFG`, enabling all statement elements, exception edges, initializers, implicit/temporary destructors, lifetime/scope elements, static initialization, allocator nodes, and rich constructors. Trivially false edges are retained. It does not use the old handwritten lowering.

LLVM receives the same frontend flags recovered from each actual Makefile recipe. It emits unoptimized IR with debug locations and names; disabling the `optnone` marker permits the separately recorded `mem2reg` experiment. GCC produces GIMPLE, CFG, and EH dumps using these Clang-configured headers; this is not a separately configured GCC PHP build. These are bounded exporters and framework probes, not production NativeIR importers.

Each command records stdout/stderr, elapsed time, GNU time peak RSS, and sampled process-group RSS. Heavy commands have a 10 GiB per-process address-space limit and a polled group RSS stop. There is **no kernel-enforced aggregate cgroup limit**, so this is measured feasibility evidence, not the plan's hard-limit acceptance result. Import timings use three sequential repetitions with a warm filesystem; other applications were running. No cold-cache or incremental-summary speedup is claimed.

Inputs, actual frontend recipes, read-header hashes, layout changes, binary/profile hashes, measurements, and counterexamples are retained in [the results directory](tools/php-catalog/experiments/comparison/results/). [Reproduction instructions](tools/php-catalog/experiments/comparison/README.md) and the experiment sources are checked in alongside them.

## Representation results

All six 8.5.4 translation units imported successfully through Clang AST/CFG, LLVM, and GCC GIMPLE. Two preserved PHP 8.4.26 units also imported through both Clang candidates after relocating include paths and isolating exported system headers. This historical replay is a compatibility check, not a completed regression analysis or coverage of PHP 7.x.

Median import/export wall time, with maximum GNU time peak RSS across the three runs:

| Translation unit | AST/CFG seconds / MiB | LLVM emit + read seconds / MiB | GIMPLE seconds / MiB |
|---|---:|---:|---:|
| Array functions | 0.952 / 145.1 | 1.859 / 135.9 | 1.967 / 128.6 |
| Scalar builtins | 0.498 / 100.6 | 0.921 / 114.8 | 0.896 / 84.2 |
| Session registration | 0.553 / 128.1 | 1.204 / 148.0 | 0.918 / 85.7 |
| Session user callbacks | 0.397 / 102.7 | 0.499 / 107.2 | 0.364 / 57.1 |
| Zend invocation | 0.462 / 110.4 | 0.676 / 109.4 | 0.501 / 65.2 |
| Streams | 0.465 / 106.4 | 0.696 / 111.7 | 0.626 / 67.3 |

LLVM times exclude a second metadata extraction, which the selected producer requires. They are not complete hybrid-pipeline timings. GIMPLE had one repetition and no normalized exporter or semantic analyzer, so its lower costs cannot establish a generator win. AST statement counts and LLVM instruction counts measure different things and are not precision scores.

Source CFG retains field names, source callback types, union structure, and macro-expanded conditions directly. LLVM retains concrete operations and layouts but needs the metadata bridge to reconstruct Zend meaning. Compiler lowering can also expand source helpers considerably: `zif_session_set_save_handler` has 313 source CFG blocks versus 1,149 LLVM blocks in this build. A source-level analyzer would need to traverse the helper bodies too; the source root's smaller graph does not establish cheaper complete analysis.

### The deciding C++ cleanup counterexample

The fixture constructs a guard, emits event 1, calls a potentially throwing function, emits event 3 on success, and emits event 4 in the catch handler. Guard destruction emits event 2. Clang and GCC native executions agree:

| Outcome | Observed order |
|---|---|
| Normal | `1, 3, 2` |
| Throw | `1, 2, 4` |

Despite enabled `AddEHEdges` and `AddImplicitDtors`, Clang 21's CFG has one normal destructor node and an exceptional edge from the call directly to the catch dispatch. The path reaches event 4 without destruction. The same gap occurs in the MSVC-target CFG. This is a concrete limitation of this configured producer, not a claim that every possible Clang extension must fail.

LLVM includes destruction on the Linux landing-pad path and on the Windows cleanup-pad path before catch entry. GIMPLE's EH dump also includes exceptional destruction. The automated graph checks preserve these counterexamples in [findings.json](tools/php-catalog/experiments/comparison/results/findings.json).

Source CFG could be repaired using additional exception/lifetime lowering, but that would recreate behavior already emitted by the compiler and add another correctness obligation. Maximum churn favors the compiler's execution graph plus source metadata here.

In the initial LLVM/Clang build without CIR enabled, CIR flags were recognized but each import reported `action unknown not compiled in`. The extended evaluation then built the combined CIR-enabled toolchain: the actual campaign emitted CIR for only 44/523 C units and failed all four cleanup/dispatch fixtures. Of those 44 outputs, 36 lowered to LLVM IR that passed `opt-21` verification; eight hit CIR-to-LLVM backend assertions. Thus only 36/523 units (6.9%) reached verified LLVM IR, still inadequate for the locked profile, and verifier success does not establish semantic equivalence. A different CIR revision would need its own feature and exception/ABI comparison. [CIR build requirements](https://llvm.github.io/clangir/GettingStarted/build-install.html)

## Framework results

### Clang components

The pinned FlowSensitive API rejects every native C function with `Can only analyze C++`. Recompiling the reduced fixture as C++ allows its builtin transfer probe to complete, and a one-block visit budget produces an explicit incomplete error. Compiling php-src as C++ is not an equivalent producer. This framework therefore requires substantial C support work before reuse as the PHP native analyzer.

The Static Analyzer supports C, but its stock diagnostic interface is not a catalog-summary interface. With tiny exploration budgets, the experiment reports unfinished worklists while returning exit code zero. Empty diagnostics or successful process exit cannot certify exhaustive effects. Its inlining configuration also requires an explicit cutoff/unknown audit. [Clang interprocedural analysis](https://clang.llvm.org/docs/analyzer/developer-docs/IPA.html)

Clang AST/layout extraction remains useful independently of those solver interfaces. These findings do not reject using its compiler primitives.

### SVF demand-driven pointer queries

SVF was built and actually queried; it was not rejected from documentation alone. External process measurements include parsing, graph construction, Andersen initialization, value-flow setup, query solving, and reporting:

| Input | Function-pointer queries | Whole-process seconds | Peak MiB |
|---|---:|---:|---:|
| Reduced dispatch/copy fixture | 5 | 0.056 | 29.7 |
| Unknown external write | 1 | 0.058 | 27.2 |
| PHP array translation unit | 34 | 1.825 | 208.3 |
| Zend invocation translation unit | 9 | 0.313 | 61.8 |

The real PHP modules are incomplete link units with unseeded external/PHP inputs. These timings are pointer-prerequisite measurements, not complete native-program summaries. In particular, the Zend module solves no indirect call edges; its empty targets cannot establish no invocation.

The source confirms that DDA initialization constructs Andersen analysis and a pointer-only value-flow graph. A query is demand-driven after that setup; it does not make the setup itself query-local. [Pinned initialization](https://github.com/SVF-tools/SVF/blob/e889a5139dbca132142b07d90192efab9a16c614/svf/include/DDA/DDAVFSolver.h)

At a one-step budget, all tested queries fall back to Andersen: 5/5, 1/1, 34/34, and 9/9. The fallback produces results and exit zero. The adapter must preserve its abstraction, model dependencies, and precision-loss reason. [Pinned fallback](https://github.com/SVF-tools/SVF/blob/e889a5139dbca132142b07d90192efab9a16c614/svf/lib/DDA/FlowDDA.cpp)

### Unknown external writes require a coverage gate

The reduced caller initializes a callback slot to `increment`, passes its address to `unknown_write`, then invokes the slot. A separately compiled implementation replaces it with `decrement`.

| Inputs to SVF | Reported target | Result |
|---|---|---|
| Caller module; external write body absent | `increment` | Misses the native witness |
| Same module with `-blk` | `increment` | Blackhole option does not repair the absent write |
| Caller plus actual external implementation | `decrement` | Includes the native witness precisely |

Native Clang and GCC executables both pass the assertion that `decrement` executed. This demonstrates a boundary-model obligation, rather than an inherent inability of SVF to propagate writes. The external-model implementation supports `STORE_TOP` annotations, which are worth adapting; they are not automatically applied to an unknown declaration.

A missing body/model must keep affected pointer and effect domains open and invalidate aliased storage. Do not take the stock target set as an exhaustive universe, even when DDA reports no budget exhaustion. Cross-language Zend callbacks need the same rule.

### Byte-array initialization still scales with physical extent

Increasing a payload's byte array leaves the reported graph constant: 42 objects, 193 pointers, 139 SVF statements, and 204 value-flow nodes. The copied callback field remains represented. However, peak memory grows substantially:

| Payload bytes | SVF DDA peak MiB | AST exporter peak MiB | LLVM emitter peak MiB |
|---:|---:|---:|---:|
| 64 | 28.7 | 83.1 | 90.3 |
| 65,536 | 29.6 | 83.1 | 90.4 |
| 16,777,216 | 537.5 | 83.1 | 90.4 |
| 67,108,864 | 2,073.7 | 83.1 | 90.0 |

This is reproducible with array-sensitive modeling disabled, its default. `collectArrayInfo` eagerly creates per-element indices/types, and struct layout collection copies flattened element types. The measured growth is approximately 32 bytes of metadata per payload byte for this fixture. The catalog's symbolic region design must avoid equivalent layout expansion as well as byte-copy edge expansion. [Pinned type-layout initialization](https://github.com/SVF-tools/SVF/blob/e889a5139dbca132142b07d90192efab9a16c614/svf-llvm/lib/LLVMModule.cpp)

This is a reduced, static array case, not a reproduction of the complete former 439-million-edge workload. It justifies a sparse layout adaptation experiment before accepting stock SVF. It does not prove that all real PHP workloads exceed the memory budget or that a new custom solver would fare better.

## Runtime, invalidation, and coverage

Two source-built 8.5.4 runtimes give identical traces on 13 sentinels: empty/null/callback-result `array_map`, write-before-throw, `Countable`, by-reference `array_walk`, scalar return, array copy, shared reference cells, string-coercion callback, stream result/dispatch, and retained session callbacks.

The normal Clang build actually selects **TAILCALL**. Generating with `--with-vm-kind=CALL` alone still lets the compiler capability select TAILCALL; the first generated build failed with incompatible tail-call conventions. The verified CALL profile configures `php_cv_preserve_none=no` before generation. Preprocessor checks confirm distinct TAILCALL/CALL kinds, and both generated headers/configurations/binaries are hashed. Use actual VM selection and generated identities in the production manifest. Matching traces on these inputs are positive compatibility evidence, not full VM equivalence.

All three sequential exports for each representation are byte-identical. A header-only change moves a callback field while the translation-unit text remains identical, and the AST layout changes accordingly. The experiment inventories read project/system headers; no production summary cache or SCC invalidation implementation is claimed.

The Windows exception fixture imports, but no Windows runtime/profile is verified. Historical 8.4 import success does not establish patch/minor coverage. Native bailout/returns-twice constructs import, but their semantic transfer remains a boundary-model obligation. SMT solving, binary lifting, CBMC, full dependency libraries, instrumented PHP indirect-target traces, and checker integration were not benchmarked in this comparison.

## Changes to implementation order

1. Retain LLVM plus metadata as the selected initial producer. Complete actual link capture and the Zend abstraction bridge before treating source imports as semantic coverage.
2. Audit closed native capsules and unknown boundaries first. An incomplete PHP link unit or unmodeled native write must never supply closed negative facts.
3. Restrict SVF array/layout and boundary-modeling work to reduced, discriminating fixtures until a reduced configuration passes resource and target-coverage gates. Both stock and compact-layout variants already fail the actual whole-link 10 GiB gate. Measure initialization as well as query work; preserve fallback evidence. A maintained adaptation and its dependency/license terms are part of the reuse cost.
4. Keep one regional ordered semantic engine. Pointer results are a subordinate service; they cannot independently narrow returns, erase exceptions, or override observable transfers.
5. Compare adapted SVF-assisted summaries against the regional candidate on actual PHP graphs, including callback results, mutation, pending-exception cleanup, retention, post-fixpoints, and whole-profile resource limits. Select the production solver only after that gate.

The measured comparison is complete at the representation/framework-prerequisite level. None of the 13 runtime witnesses has yet been matched against a generated closed PHP BehaviorGraph. F00–F05 and the remaining semantic/resource obligations of F06 remain OPEN.
