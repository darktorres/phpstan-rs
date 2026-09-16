# Phase dependency graph

Reinstated after being retired (see git history: "Retire
PHASE_DEPENDENCIES.md - no live blocking edges left") once a session-long
reevaluation of ROADMAP.md's "Permanent design boundaries"/"Won't-port"
lists produced real new phases (PHASE_BACKLOG.md's Phases 158-175) with
live dependency edges among them - unlike the rest of the backlog, which
has no cross-phase blockers today.

Extracted directly from every "needs Phase N" / "blocked on Phase N" /
"prerequisite for Phase N" statement made while filing Phases 158-175 -
not inferred. Every other phase in PHASE_BACKLOG.md (1-157) has no
dependency edge at all and isn't repeated here.

Regenerate by re-grepping `PHASE_BACKLOG.md`/`ROADMAP.md` for
`Phase [0-9]\+` after any substantial edit to this range - this file
drifts the same way corpus counts do, and isn't re-derived automatically.

## Graph

```mermaid
graph LR
    P174["174: Two-pass\ncollect-then-consume\nchecking architecture"]
    P166["166: Cross-file\ntrait-supplied\nproperty-assignment\ntracking"]
    P169["169: Same-branch\ndiscriminant-variable\ncorrelation"]
    P175["175: Closure capturing\nan outer branch's\nco-assignment fact"]
    P171["171: Equality-comparison\nleaves in the OR/AND-\nchain narrowing\nmechanism"]
    P172["172: Nullsafe-chain-\nproven-non-null implies\nits receiver is non-null"]
    XFN["(unnumbered) general\ncross-function array\ncorrelation - invariant\nsoundness, no known\nbounded design"]

    P174 -->|preferred path; unblocks| P166
    P174 -.->|necessary, not sufficient -\nsee XFN's own second layer| XFN
    P169 -->|blocks| P175
```

Solid arrows are hard blockers stated directly in ROADMAP.md/PHASE_BACKLOG.md.
The dotted arrow is a "necessary but not sufficient" note, not a hard
block: Phase 174 alone doesn't finish general cross-function array
correlation, since a fact persisted across a function boundary still
needs a separate, currently-unsolved invariant-maintenance proof to be
sound against arbitrary intervening mutation - that second layer has no
phase number because it has no known bounded design yet (see ROADMAP.md's
own boundary entry for the full reasoning), unlike every other node here.

Phases 171 and 172 are listed for context (both reevaluated in the same
session, alongside 169) but have **no** dependency edge on 169 or on each
other - each is a self-contained new leaf/match-arm addition to an
already-existing, already-working mechanism (`or_chain_narrowing_target`/
`and_chain_narrowing_target` for 171, `apply_narrowing`'s own direct-arm
dispatch for 172), not built on top of 169's own new correlation
mechanism. Listed here only so a reader scanning this file doesn't assume
a missing edge was an oversight.

## Edge list (with source)

| From | To | Relationship | Status |
|---|---|---|---|
| **174** Two-pass checking architecture | **166** Cross-file trait-property tracking | preferred path (alternative: 166's own option (a), an N-times-per-host re-check, independently buildable without 174) | live - 174 not yet built; 166 has a non-blocked fallback path, so isn't hard-blocked, just better served by 174 landing first |
| **174** Two-pass checking architecture | General cross-function array correlation (unnumbered - see ROADMAP.md's own boundary entry) | necessary, not sufficient | live - even once 174 is built, the invariant-soundness layer on top has no known design; this edge alone doesn't close the gap |
| **169** Same-branch discriminant-variable correlation | **175** Closure capturing an outer branch's co-assignment fact | hard blocker | live - 175 explicitly not independently assessable until 169's own stored-fact shape and replay mechanism are settled |

Three live edges today, all from this session's reevaluation work. None
are satisfied yet (unlike the prior version of this file, retired because
every edge it tracked had become satisfied) - this version starts fresh
and should be re-checked/updated whenever 166, 169, 174, or 175 change
status, and retired again once none of the edges above are live.
