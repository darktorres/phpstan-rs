#!/usr/bin/env python3
"""Give each part of the level 9 plan a table of the status of the ledger tasks it names.

A task is a row of `PROFILE_DRILL_PIWIGO_LEDGER.md` (`| ID | task | item | state | needs |`).
A part names a task anywhere in its text: in a heading, a table cell, or prose. A list
after a prefixed ID names each member of it: `CHECK-6.4 to 6.6, 6.9` names CHECK-6.4,
CHECK-6.5, CHECK-6.6 and CHECK-6.9; `SRV-1 to SRV-7` names SRV-1 to SRV-7, since a head with
no minor number takes top-level numbers. The status is the first known state word (OPEN, DONE,
READY, PART, SUPERSEDED, SKIP, BLOCKED) among the first words of the ledger's state cell.
A part's table lists the tasks it names and the other parts that name them. The table sits
between two markers under the part's first heading; running the script again replaces it.

The script exits with status 1 when a pending ledger task (OPEN, READY or PART) is named by
no part, or when the ledger has no state-table rows. A parent row (`CHECK-6`) counts as named
when one of its children (`CHECK-6.3`) is.
"""

import datetime
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
LEDGER = ROOT / "PROFILE_DRILL_PIWIGO_LEDGER.md"
PLAN = ROOT / "docs" / "level-9"
BEGIN = "<!-- status:begin -->"
END = "<!-- status:end -->"
# Not a part: the index.
EXCLUDED = {"README.md"}
PENDING = {"OPEN", "READY", "PART"}
KNOWN = ("OPEN", "READY", "PART", "DONE", "FOLDED", "SUPERSEDED", "SKIP", "BLOCKED")
ID = re.compile(r"\b([A-Z][A-Z0-9]*)-(\d+)((?:\.\d+[a-z]?)*)\b")
HEAD = re.compile(r"\b([A-Z][A-Z0-9]*)-(\d+)(?:\.(\d+))?[a-z]?\b")
CHAIN = re.compile(r"((?:\s*(?:,|and|or|to|through|–|-)\s*(?:[A-Z][A-Z0-9]*-)?\d+(?:\.\d+)?[a-z]?\b)+)")
TOKEN = re.compile(r"(?:([A-Z][A-Z0-9]*)-)?(\d+)(?:\.(\d+))?[a-z]?\b|(to|through|–|-)")
CELL = re.compile(r"(?<!\\)\|")


def ledger_rows():
    """Each ledger task: its state cell and its note, from the tables whose fourth column is the state.

    The ledger also keeps a history table per task (commit, Ir after, change), and a task can
    appear in both; only the state tables count.
    """
    rows = {}
    in_state_table = False
    for line in LEDGER.read_text(encoding="utf-8").splitlines():
        if not line.startswith("|"):
            in_state_table = False
            continue
        cells = [c.strip() for c in CELL.split(line)[1:-1]]
        if len(cells) == 5 and cells[0].lower() == "id":
            in_state_table = cells[3].lower() == "state"
            continue
        if not in_state_table or len(cells) != 5:
            continue
        if ID.fullmatch(cells[0]) and cells[0] not in rows:
            rows[cells[0]] = cells[3]
    return rows


def named(text, rows):
    """The ledger tasks a text names, lists included."""
    found = set()
    for m in HEAD.finditer(text):
        pre, major, minor = m.group(1), m.group(2), m.group(3)
        head = f"{pre}-{major}" + (f".{minor}" if minor else "")
        if head in rows:
            found.add(head)
        tail = CHAIN.match(text, m.end())
        if not tail:
            continue
        prefix, major_now, dotted = pre, major, minor is not None
        last, ranging = (int(minor) if dotted else int(major)), False
        for t in TOKEN.finditer(tail.group(1)):
            if t.group(4):
                ranging = True
                continue
            prefix = t.group(1) or prefix
            if t.group(3):
                major_now, dotted, nums = t.group(2), True, [int(t.group(3))]
            else:
                nums = [int(t.group(2))]
            if ranging and nums and nums[0] > last:
                nums = list(range(last + 1, nums[0] + 1))
            for n in nums:
                ident = f"{prefix}-{major_now}.{n}" if dotted else f"{prefix}-{n}"
                if ident in rows:
                    found.add(ident)
            last = nums[-1] if nums else last
            ranging = False
    return found


def status_of(state):
    words = re.findall(r"[A-Za-z]+", state)[:8]
    for word in words:
        if word in KNOWN:
            return word
    return "SEE LEDGER"


def shorten(state, limit=120):
    text = " ".join(state.split()).replace("|", "\\|")
    if len(text) <= limit:
        return text
    return text[:limit].rsplit(" ", 1)[0] + " …"


def part_text(path):
    text = path.read_text(encoding="utf-8")
    return re.sub(re.escape(BEGIN) + ".*?" + re.escape(END), "", text, flags=re.S)


def table(ids, rows, others, head, today):
    lines = [
        BEGIN,
        "",
        "## Status of this part's tasks",
        "",
        f"From the ledger at `{head}` on {today}. Regenerate with "
        "`scripts/plan_status.py`; change the ledger, not this table. "
        "`also in` lists the other parts that name the task.",
        "",
        "| task | status | ledger | also in |",
        "|---|---|---|---|",
    ]
    if not ids:
        lines.append("| — | — | no ledger task is named in this part | — |")
    for ident in ids:
        also = " ".join(sorted(others[ident])) or "—"
        lines.append(f"| {ident} | {status_of(rows[ident])} | {shorten(rows[ident])} | {also} |")
    lines += ["", END]
    return "\n".join(lines)


def place(text, block):
    if BEGIN in text:
        start = text.index(BEGIN)
        end = text.index(END) + len(END)
        return text[:start] + block + text[end:]
    lines = text.split("\n")
    heading = next((i for i, line in enumerate(lines) if line.startswith("#")), None)
    if heading is None:
        return block + "\n\n" + text
    return "\n".join(lines[: heading + 1] + ["", block] + lines[heading + 1 :])


def covered(ident, found):
    return ident in found or any(other.startswith(ident + ".") for other in found)


def main():
    rows = ledger_rows()
    if not rows:
        sys.exit("no ledger rows found in " + str(LEDGER))
    head = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout.strip()
    today = datetime.date.today().isoformat()
    parts = {p: part_text(p) for p in sorted(PLAN.glob("*.md")) if p.name not in EXCLUDED}
    names = {p: named(text, rows) for p, text in parts.items()}
    everywhere = set().union(*names.values())
    for path, found in names.items():
        others = {
            ident: {q.name[:2] for q, other in names.items() if q != path and ident in other}
            for ident in found
        }
        ordered = sorted(found, key=lambda k: list(rows).index(k))
        block = table(ordered, rows, others, head, today)
        path.write_text(place(path.read_text(encoding="utf-8"), block), encoding="utf-8")
        print(f"{len(found):4d} {path.name}")
    pending = [k for k, state in rows.items() if status_of(state) in PENDING]
    missing = [k for k in pending if not covered(k, everywhere)]
    for k in missing:
        print(f"not named by any part: {k} ({status_of(rows[k])})")
    if missing:
        sys.exit(1)


if __name__ == "__main__":
    main()
