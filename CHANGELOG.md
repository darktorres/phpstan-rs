# Changelog

User-visible changes: output, formats, flags, configuration and exit codes.

## Unreleased

### Output
- Diagnostics come in one total order - position, then identifier, message
  and tip - in the CLI and the language server (`e87343f3`).
- A diagnostic no longer carries a symbolic location: text, checkstyle,
  JUnit, code-frame and editor output show the message alone, and JSON
  drops the `location` field (`bad4a754`).

### Baseline
- The baseline format is versioned: `{"version":2,...}`, its entries keyed
  by file, message and identifier with a count. A version-1 baseline still
  loads, its entries merged per new key (`bad4a754`).
- `convert-baseline <path>` rewrites a baseline in the current format
  (`bad4a754`).

### Performance
- `check-project` spends far less time on one thread: named-argument
  facts no longer need a second checker pass, property-assign facts are
  collected in parallel, a function lookup that misses no longer scans
  every function, and composer hygiene no longer calls `getcwd` or
  canonicalizes each file. The all-on Piwigo run went from about 71 s to
  about 30 s (`370f1d87`, `3c3c04e0`, `247d955d`, `9d3937f3`).
- `--dump-types` records the types the check itself inferred, in the
  same pass, instead of re-checking every file (`78047041`, `ee2eb7a4`).
- The language server's workspace check runs its files in parallel
  (`06a56b58`).
- Startup is faster: the embedded stubs are parsed and indexed in
  parallel, and PHPDoc inheritance no longer recomputes a class's
  ancestor bindings for each method (`5c0a1c49`, `2ce9c82c`).

### Language server
- `restricted_symbols` path zones (`allow_in`, `allow_except_in`) apply in
  the language server, with no ignore list or baseline configured, and its
  project resolves paths against the workspace root (`00a4b609`).
- `initialize` reports the binary's version (`4f1b3148`).

### Analysis
- Fixes to inference and to false positives, each listed in
  `CRATE_SPLIT_PLAN.md` under "Bugs found during A2" (delta
  `gates/approved/a2-known-bugs`); a duplicate method or property in a
  class body is now always reported.

### Project
- Licensed under MIT.
