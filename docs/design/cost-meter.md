# Design: the cost meter (`cargo xtask cost`)

The meter counts what a run of the shipped binary costs, in numbers that don't move between runs: instructions (callgrind's Ir) and heap (allocated blocks and bytes, peak, live at exit). It gates every push in CI and keeps a committed history. Wall time is not its job; that is measured at milestone ends (`PLAN.md`, Phase 2).

Inputs: LED-6, 9B-1, 9B-2, 9B-5, WS-2, HL-1, HL-2, OH-1, OFF-10, IT-4, P-7, LS-2 and PB-1 in `RESTART_FINDINGS.md` at `legacy-final`.

## Shape

```
cargo xtask cost [--workload <glob>] [--base <commit>] [--record]
```

Without `--base` or `--record` it prints the counts. `--base` builds that commit in a throwaway worktree, which it removes again.

1. **Build** the `cost` profile of `phpstan-rs`: release optimizations, debug info, frame pointers, no LTO. The profile never changes once measured (P-7, LS-2), because codegen settings move totals by more than most changes do.
2. **Run** each workload of `perf/workloads.toml` twice under valgrind, once with `--tool=callgrind` and once with `--tool=dhat`, at one worker thread (LED-6: Ir is deterministic only there).
3. **Read** from callgrind: the total Ir and the top 20 functions by self cost. From DHAT: total blocks and bytes, the peak (`t-gmax`) bytes, the bytes live at exit (`t-end`), and the top 20 allocation sites by bytes.
4. **Compare** with `--base`: the same workloads, measured the same way, on a build of that commit, in the same job.
5. **Record** with `--record`: one row per workload in `perf/history.tsv`, and the top-20 tables in `perf/top/<workload>.txt`.

The meter's output is data the caller prints; no measurement print goes into the binary (CLAUDE.md).

## Workloads

`perf/workloads.toml` holds one table per workload:

```toml
[parse-src-nsrt]
args = ["debug", "parser", "{src}"]
stdin = { files = ["src/**/*.php", "tests/PHPStan/Analyser/nsrt/*.php"] }
cache = "off"
since = "M1"
```

- `{src}` is `vendor/phpstan-src`. `stdin.files` globs are under it, listed in byte order, one per line, which is what the `debug` subcommands read.
- **A workload never changes once added;** a larger variant gets a new name (9B-2).
- **`cache` is part of its definition** (9B-2, HL-1). `off` gives the run a private, empty cache directory. `warm` fills it in a first run and checks it is byte-identical after the measured run; a run that rewrote its cache is invalid, not a data point. Series in different modes are never compared.
- **The environment is empty** apart from `PATH` and `HOME` (a private directory). Ir counts every environment variable and argument byte, so an inherited shell would make two machines' runs differ for no reason.
- **Nothing in the run may vary.** Three settings were needed, each found by a count that moved:
  - `setarch -R` turns address randomization off, and `--vgdb=no` drops a pipe file named after the PID. Rust's thread setup parses `/proc/self/maps`, so both reached the count.
  - `--fair-sched=yes` hands valgrind's lock out in FIFO order. The command runs on a thread of its own, and whether the main thread's join had to wait was a race worth 97 Ir (`__futex_abstimed_wait_cancelable64` and its callers); one full run in about seven took the other path. With the fair scheduler, 15 of 15 full runs gave the same counts.
- **Both sides run from the same path.** Each binary is copied to one place before it is measured, because its own path (`argv[0]`) is counted too. Two builds of one commit measured from their build directories differed by 76 Ir; from one path, they are equal.

The set grows with the port, as `PLAN.md` lists. The first two run before any port code exists:
- `startup`: `phpstan-rs --version`. It is the floor every other workload stands on.
- `parse-src-nsrt`: the M1 workload. Until the parser is ported it measures the unported answer, and its first real row lands with M1.

## The gate (CI, every push)

The `cost` job runs on `ubuntu-latest`:
- It builds the `cost` profile at the pushed head and at the push's `before` commit; for a pull request, at the merge base.
- It runs every workload on both, and fails if any workload's Ir rises by more than 1%, or its peak or live heap by more than 2% (WS-2: Ir alone accepted a +68% heap rise in 9B-1).
  - A rise must also pass an absolute floor, 10,000 Ir or 1 KiB, or it doesn't count. The workloads' counts are deterministic, so the floors aren't for noise: they keep a few bytes or instructions from failing a small workload.
- A rise is accepted when a commit in the pushed range carries a `cost:` line in its message, which says why.
- **One run per side is enough.** The counts are deterministic (spike S1), and both sides are measured on the same runner, so differences between runner CPUs and the local machine never reach the comparison (spike S3).

On failure the job prints, per workload, the top-20 self-cost and allocation-site diffs. It notes that function names move with inlining, quotes no inclusive cost and sums none (HL-2, OFF-10).

## History

`perf/history.tsv` is committed. Its columns are `commit workload ir blocks bytes peak_bytes live_bytes`.
- `cargo xtask cost --record` measures the checked-out commit, refusing a dirty tree, and replaces that commit's rows. The rows are committed with the next change, so each row names the commit it measured, never the commit that carries it.
- **Rows are recorded locally**, on this machine, so the series is comparable with itself. A milestone end records one; so does any commit whose message has a `cost:` line.
- `perf/top/<workload>.txt` holds the latest recorded top-20 tables, so their changes show in a diff.
- Reference rows for the legacy binary (`~/.cache/phpstan-rs-legacy/bin/phpstan-rs-legacy-final`) are recorded under the commit name `legacy-final`, for the workloads its CLI can drive (L-1, PB-1). M8's comparison reads them.

## Memo counters

Every memo has a hit counter (CLAUDE.md). The meter will print the counters from a third, unmeasured run of each workload, so they cost the measured runs nothing. That flag comes with the first memo; there are none yet.

## Spikes

- **S1: is a valgrind count deterministic here?**
  - Method: run `startup` and `parse-src-nsrt` five times each under callgrind and under DHAT, locally and in CI, with the empty environment.
  - Accept: within one machine, every run gives the same Ir, blocks, bytes, peak and live bytes.
  - Fallback: take the median of three runs and widen the thresholds to the spread seen.
  - **Result locally (2026-11-01): accepted, after three fixes.** Short repeated runs looked deterministic, but longer series showed two sources of variation, which the settings under "Workloads" remove. With them, 15 of 15 full meter runs gave identical tables. The CI half is read off the gate's first runs.
- **S2: is valgrind's DHAT fast enough for the per-push gate?** The legacy used a patched dhat-rs for whole-stack Piwigo profiles, because valgrind's DHAT was too slow there (OH-1).
  - Method: time both tools on `parse-src-nsrt` once M1 can parse.
  - Accept: the whole gate, both sides and both tools, under 10 minutes.
  - Fallback: run DHAT only on the smaller workloads per push, and on every workload at milestone ends.
- **S3: do local and CI counts differ for the same commit?**
  - Method: compare `--record`'s local rows with the gate's head numbers for the same commit.
  - Expected: they differ, through glibc and the memcpy variant chosen for the CPU, so the gate measures both sides itself.
  - If they are equal, the gate may compare against the committed rows instead of building the base.
