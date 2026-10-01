# Where the work stands

`WORK_STACK.md` lists what is paused, in what order, and where to resume. When work is paused for a fix, add the fix there instead of starting a new tracking file.

# Profiling

Find hotspots with a sampling profiler, not with `hotpath`: `hotpath` times
only the functions annotated `#[hotpath::measure]`, so it shows what was
already suspected. `perf` is installed; `samply` and `inferno`
(`inferno-collapse-perf`) are installed under `~/.cargo/bin`.

## Build

Use the `dhat` profile (release optimizations, debug info, no LTO,
incremental) with frame pointers, in a target directory of its own so the
normal build isn't disturbed:

```sh
RUSTFLAGS="-C force-frame-pointers=yes" CARGO_TARGET_DIR=<dir>/fp \
  nice -n 19 cargo build --profile dhat -p phpstan-rs --bin phpstan-rs
```

The first build compiles every dependency again with the flag (minutes);
later ones are incremental.

## Record

```sh
perf record -o <file> -F 499 -g <dir>/fp/dhat/phpstan-rs <command>
```

- **Frame pointers, not `--call-graph dwarf`.** DWARF unwinding is slow and
  its stacks stop short, so the callers of a hotspot vanish. 499 Hz is plenty
  for runs of seconds; a 0.05 s run needs 1999 Hz and still gives only
  ~100 samples.
- **Run the command once before recording.** The stub cache
  (`~/.cache/phpstan-rs`) is keyed by the build's source hash, so the first
  run after any rebuild is cold and builds and stores it. Check that a
  warm run does not rewrite those files.

## Read

- **Always `--no-inline`.** Without it `perf` resolves inlined frames through
  `addr2line` for every sample, which takes minutes on a debug-info binary.
  It applies to `perf script` too.
- Inclusive cost by function:
  `perf report -i <file> --children --stdio -g none --no-inline --percent-limit 3`
- Self time by symbol:
  `perf report -i <file> --no-children --stdio -g none --no-inline -s sym`
- Stacks as text: `perf script -i <file> --no-inline | inferno-collapse-perf`,
  then aggregate the folded lines (frame names, deepest `phpstan_rs::` frames
  after `main`). A flamegraph SVG can't be read in a terminal.
- **Serial stretches (the usual cause of a slow parallel run):** bucket
  `perf script -i <file> -F tid,time,ip,sym -G --no-inline` into 0.5 s
  windows and count samples / (frequency x 0.5) = cores busy. A window under
  about 1.6 cores is serial. Fold the stacks of just that window with
  `perf script --no-inline --time <t0+a>,<t0+b>`; times are absolute, `t0`
  is the first sample's. Run these in the background: they take minutes.
- **Scaling curve:** `PHPSTAN_RS_THREADS=1,2,4,8` sizes the worker pool.
  Fit wall = serial + parallel / threads; the serial part is what is left to
  find.

## Short runs: count instructions

A run of a few tens of milliseconds gives a sampling profile a few hundred
samples and a report step that takes minutes. For those, `valgrind` is
installed: the instruction counts are exact, and the run takes about a
minute under it.

```sh
valgrind --tool=callgrind --callgrind-out-file=<dir>/cg.out \
  --separate-threads=yes <dir>/fp/dhat/<binary> <command>
callgrind_annotate --inclusive=yes --tree=none <dir>/cg.out-<thread>
```

- One output file per thread (`cg.out-01` is `main`); read the largest ones
  first, and note which run in series (a test thread starts after `main`
  has listed its tests) and which beside each other.
- `--tree=calling` lists a function's callees and `--tree=caller` its
  callers; a callee that is inlined shows only as part of its caller.
- Instruction counts are not time: memory-bound work weighs more in wall
  time than here, and system calls are not counted. `strace -f -c` counts
  those.

## Workloads

| what it shows | command |
|---|---|
| startup: stub cache, project build | one file in a directory: `check-project --no-composer .` |
| load and an edit on a real workspace | `phpstan-rs incremental-bench <dir>` (e.g. `vendor/symfony/src/Symfony/Component/Console`) |
| a large corpus, parallel stages | in `/home/torres/piwigo17-frozen`: `check-project --config phpstan-rs.toml --format json` |

## Timing

- `hyperfine -N --warmup <n> --runs <n>` on `gate`-profile binaries; compare
  against a binary built from the base commit in a throwaway
  `git worktree` (copy `vendor/phpstan-src` into it rather than
  initialising submodules), alternating A, B, A, B.
- Run under `nice -n 19` and note `/proc/loadavg`; say so when a number was
  taken under load. Don't wait for the machine to go quiet.
- A timing run before and after a change must both be warm-cache.

## Traps

- `pkill -f "perf script"` (or any pattern naming the command that runs it)
  kills the shell that issued it; kill by pid.
- Don't wrap commands in `timeout`; long ones are backgrounded by the harness.
- Don't leave measurement `eprintln!`s in a commit.
- One heavy job at a time (builds, profiles, test suites): they skew each
  other.
