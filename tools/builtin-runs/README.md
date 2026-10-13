# Builtin runs

`record.php` calls every function and every public method of every class
the running PHP defines, with values at each parameter that record what
user code PHP runs through them: a callable (invoked, or kept without being
invoked), an object (which of its methods PHP calls), a URL or a stream of
a recording stream wrapper (which wrapper methods PHP calls), and, for a
method, an object made with such a value at a constructor parameter, or
given it by another method of the class called first (what of it the
method runs). Each call runs in a child process of its own, in an empty
directory, with an alarm, in a worker process that a sandbox holds.

The effects of calls (`crates/phpstan-rs/src/registry/effects.rs`) read
the table it prints: a builtin writes what the code it runs writes. A
builtin missing from the table, one this PHP does not define or could not
call (a class whose objects only PHP makes, such as `Generator`), may run
any code.

## Running it

    php tools/builtin-runs/record.php > table.tsv

Prints the table to standard output. With `--out=FILE` it writes the table
and a sidecar beside it (the table's name without `.tsv`, and `.meta.json`):
the SHA-256 of the table, the PHP and each extension it was recorded with
(version, a hash of the info text, and what each provides; Core and FFI
have no hash), the recorder's hash, the sandbox and skip mode, and each run
that wrote it. The regenerated table goes into
`crates/phpstan-rs/src/registry/data/builtin-runs-8.5.tsv` only in the
data step; until then the committed table is the recording of an earlier
run and the sidecar does not exist.

| flag | what it does | what it trades |
|---|---|---|
| `REGEX` (first argument) | records the builtins whose lowercased name it matches | a partial table: merge it with `--update` |
| `--ext=NAME,...` | records the functions and methods of those extensions; with a regex both must match | as above; the extension must be loaded |
| `--out=FILE` | writes the table and its sidecar, atomically | refused when a unit was lost |
| `--update=FILE` | replaces the rows of the selected builtins in FILE and keeps the rest, then writes FILE and its sidecar | with `--ext`, rows of a builtin this PHP no longer defines are kept; with a regex they are removed when it matches |
| `--modules=LIST` | the workers load the base modules (posix, ffi, tokenizer), the ones named, and no other module, with the ini files' settings for them; `--modules=` alone loads none | a worker of fewer modules forks faster; the rows of a module are recorded only where the module is loaded |
| `--jobs=N` | number of workers, one for each processor by default | more workers, more load, more timeouts under load (the quiet retry covers those) |
| `--php=BIN` | records the PHP BIN; the workers are its processes | the recorder's own PHP may define other names |
| `--sandbox=bwrap\|sudo-unshare\|none` | the sandbox of the workers (default `bwrap`) | `none` runs the probes with the host's files and network; `sudo-unshare` runs them with the host's files and root's rights for a moment, and is only chosen by name |
| `--skip=off\|dead\|released` | `dead` runs one survey child per held unit and skips its children when the object cannot be made; `released` also skips a child whose probe the object let go, for a method with a `probed` row | `off` is the full structure; the skips save forks, and the survey costs one fork per held unit |
| `--cache=DIR` | keeps each unit's rows under a key of everything that can change them (default `~/.cache/builtin-runs`) | a cache hit is not in `--stats` |
| `--no-cache` | ignores the cache | a full run |
| `--verify` | runs every unit twice without the cache and keeps what both runs agree on for `probed` and `releases`, and what either found for the others | twice the time; differing units are listed |
| `--coverage=FILE` | lists each builtin with no `probed` row and each stub declaration of `crates/stub-sources/index.tsv` with none, with the reason | a report of what the table lacks, not of what is wrong |
| `--zend-mm` | leaves the workers on PHP's own allocator | about twice as slow, the same output |
| `--stats=FILE` | a JSON line for each child, and a summary: wall histogram, slowest children, timeouts by class, children skipped by reason, each worker's peak memory | the file is written beside its name and renamed at the end |
| `--list` | prints the units and the children the selection would make, and stops | the count is the planned children, before any skip |

A line on standard error every five seconds gives the share of the planned
children done, the time, the rate and the timeouts and deaths so far.

## Regenerating by group

    php tools/builtin-runs/regenerate.php --list
    nice -n 19 php tools/builtin-runs/regenerate.php --out=FILE --skip=released

A worker loads every module the PHP does, and each child it forks pays for
them. `regenerate.php` records the built-in extensions in one group and each
loaded module in a group of its own, each with `--modules` so its workers
load only what the group needs (a module's needs, such as `pdo` for
`pdo_sqlite`, come first). The first group writes FILE and its sidecar, and
each later group updates them, so the sidecar's `runs` lists every group's
run. FILE must not exist. The driver stops at the first group that fails.

The grouping is only as good as the rows it gives: a table regenerated by
groups must equal one recorded by a single worker that loads every module,
which is what the regeneration is checked against.

## The sandbox

A worker under `bwrap` sees the host's files read-only, a private home
directory at `/home/worker` that is a tmpfs capped at 256 MiB (less when the
memory available cannot hold every worker's at twice that; the run refuses
below 256 MiB), where its directories are made; no network; and its own pid,
uts and ipc namespaces. Each child runs under
`RLIMIT_AS` 2 GiB, `RLIMIT_FSIZE` 64 MiB, `RLIMIT_NOFILE` 1024, `RLIMIT_CORE`
0 and `RLIMIT_CPU` 60 s, with `SIGXFSZ` ignored, so a write over the limit
fails rather than killing the child. Before a worker takes a unit it runs a
canary of user classes with known answers; a worker that answers differently
stops the run.

`bwrap` cannot start inside another `bwrap` (run the recorder with
`--sandbox=none` in that case, as `tests/compare.sh` does). A sandbox that
cannot start is an error, not a fallback.

## Units, workers and the quiet retry

A unit is a function, a method, or a method given a probe at one parameter
and followed by each other method of its class. Units that are not held run
first; a held unit is sent once they are all done, with the `probed` builtins
of its class, since the released rule reads them. A worker that dies has its
unit run again on another; a unit lost twice is reported and the run exits 1.

A unit with a child that timed out or died is run once more when the others
are done, on a quarter of the workers, and the rows of both runs are kept
(the second run's result is the one stored in the cache). The statistics count
the children of both runs.

## Tests

    php -d ffi.enable=1 tools/builtin-runs/tests/run.php

checks the table, the merge, the atomic write, the committed table against
`build.rs`'s grammar, the reconcile of `--verify`, the cache key, the
manifest, the selection, the stub index, the sidecar, a sandboxed worker,
the pinned rows in `tests/known.tsv`, and fixtures of user classes (kept and
released callables, a throwing constructor, a hanging method, an exiting one,
and a process that ignores the alarm). `cargo test -p phpstan-rs --test
builtin_runs_recorder` runs the same script.

`tests/compare.sh OLD.php NEW.php REGEX [JOBS] [-- NEW-ARGS...]` runs two
recorders under one sandbox and diffs their sorted output; it is the check
that a change keeps the table, with the differences named.
