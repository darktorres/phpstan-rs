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
