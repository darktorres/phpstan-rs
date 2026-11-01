# Design docs

Each milestone starts with one design doc here, at most about 300 lines. Its open questions are spikes, each stated as:
- the question;
- the method;
- an accept bound;
- a fallback.

When the milestone's work lands, the doc is deleted or folded into `PLAN.md`.

The table below gives each doc's inputs. The ids are findings in `RESTART_FINDINGS.md` at `legacy-final`. The § references are to `CRATE_SPLIT_PLAN.md` at `legacy-final`, read with `git show legacy-final:CRATE_SPLIT_PLAN.md`.

| doc | milestone | inputs |
|---|---|---|
| `scoreboard.md` | Phase 2 | C-1, FR-5, CA-4, CA-5, EV-3, SB-6, PL-3, F11; §4 gate 12 and the nsrt oracle section |
| `cost-meter.md` | Phase 2 | LED-6, 9B-2, 9B-5, HL-1, HL-2, OH-1, IT-4, P-7, OFF-10, WS-2 |
| `syntax.md` | M1 | §3.1; LED-3, L9-2, L9-3, L9-7, L9-8, D20-1, D20-2, D20-3, D25-2, D25-3, MG-5, MG-6, SB-1, SB-2, CP-6, C20-1, 20L-7, OFF-6, FR-9 |
| `phpdoc.md` | M2 | EV-1 (B5), L9-9, D25-4, OW-1, C20-1 |
| `types.md` | M3 | §3.2; FR-1 (M2/M3 list), FR-3, EV-1 (B4), MG-7, D25-4, D20-6, OFF-8, OFF-9, SB-3, WF-4, WF-7, C20-2 |
| `reflection.md` | M4 | §3.4–§3.5 (stub storage replaced by MG-4); FR-1 (M4 list), FR-2, FR-4, EV-2, MG-4, OFF-1 to OFF-4, D20-5, LED-2, LED-5, L9-5, L9-8, L9-10, L9-11, SC-1 to SC-5, PB-4, PO-1 |
| `analyser.md` | M5a | §3.8; FR-6, FR-7, EV-3, EV-4, D25-5, MG-8, MG-9, D20-10, SB-5, SBF-1, SBF-4, TR-2 |
| `builtins.md` | M5b | CA-1, CA-2, EV-5, UN-3, D20-10 |
| `rules.md` | M6 | §3.7, §3.9; FR-8, EV-5, CA-6, D25-6, OH-4, OFF-9, UN-1, UN-2, GS-3, D20-4 |
| `command.md` | M7 | §3.6 (key-path validation only), §3.10; FR-9, EV-2, F10, F13, MG-10, MG-11, MG-12, CP-1, CP-2, CR-1, CR-2, RV-1, PB-2, MO-2, WF-3, D20-7, OFF-5, OH-3, 20L-5, 20L-8, C20-7 |
