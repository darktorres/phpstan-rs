# The batch after the cross-platform fixes: the plan (index)

This directory holds the plan written on 2026-10-18 and 2026-10-19, split into one file per part, and one file per section
where a part was long. The text is the plan as committed in `4837b369` (four documents: `POST_LEVEL_9_PLAN.md`,
`FLAT_TREE_PLAN.md`, `ENGINE_PLAN.md` and `PLAN_REVIEW_LEDGER.md`), with the intro paragraphs of those documents replaced by
this index. Section numbers are kept, so a reference such as "Part E 40" or "I4.5" reads the same in every file. Where the
plan names its own documents (for example "call it `FLAT_TREE_PLAN.md`"), those are the plan's proposals, not the files here.

The plan was approved to start on 2026-10-19 ("commit and start the plan"), as the commit's intro recorded. Where a design
says it "waits for the user's word", the ruling of 2026-10-18 (Part E, M5) is the answer.

## Where each part is

| part | file |
|---|---|
| Overview | [`01-overview.md`](01-overview.md) |
| Part A (intro and table) | [`02-part-a-intro.md`](02-part-a-intro.md) |
| Part A, A1 | [`03-part-a1.md`](03-part-a1.md) |
| Part A, A2 to A8 | [`04-part-a2-to-a8.md`](04-part-a2-to-a8.md) |
| Part C | [`05-part-c.md`](05-part-c.md) |
| Part D | [`06-part-d.md`](06-part-d.md) |
| Part E | [`07-part-e.md`](07-part-e.md) |
| Part F (and G5) | [`08-part-f.md`](08-part-f.md) |
| Part I (I2 to I5) | [`09-part-i.md`](09-part-i.md) |
| Part K | [`10-part-k.md`](10-part-k.md) |
| Part L | [`11-part-l.md`](11-part-l.md) |
| Part N | [`12-part-n.md`](12-part-n.md) |
| Part B | [`13-part-b.md`](13-part-b.md) |
| Part G (G1, G2) | [`14-part-g1-g2.md`](14-part-g1-g2.md) |
| Part G, G3 | [`15-part-g3.md`](15-part-g3.md) |
| Part G (G4, G6) | [`16-part-g4-g6.md`](16-part-g4-g6.md) |
| Part I1 | [`17-part-i1.md`](17-part-i1.md) |
| Part H (H1) | [`18-part-h1.md`](18-part-h1.md) |
| Part H, H2 | [`19-part-h2.md`](19-part-h2.md) |
| Part H, H3 | [`20-part-h3.md`](20-part-h3.md) |
| Part H, H4 | [`21-part-h4.md`](21-part-h4.md) |
| Part H (H5, H6) | [`22-part-h5-h6.md`](22-part-h5-h6.md) |
| Part J | [`23-part-j.md`](23-part-j.md) |
| Part M (intro, M1, M2) | [`24-part-m-intro.md`](24-part-m-intro.md) |
| Part M (M4, M5) | [`31-m4-m5.md`](31-m4-m5.md) |

The ledger, `PROFILE_DRILL_PIWIGO_LEDGER.md`, is the only record of each task's state. The parts name the tasks they depend on; the tables that list tasks are written by hand.

Part B, G1 to G4, G6 and I1 came from the flat-tree document; Part H and J from the engine document; Part M from the ledger
document. The rest came from the batch document. The split is mechanical: every non-empty line of the four documents is in
one of these files, apart from the intros they had, which this index replaces.
