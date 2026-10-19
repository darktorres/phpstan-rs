#!/usr/bin/env python3
"""Give each part of the level 9 plan a table of the status of its tasks.

A task is a row of `PROFILE_DRILL_PIWIGO_LEDGER.md` (`| ID | task | item | state | needs |`).
A part names a task in a heading, or in the task column of a table: the column whose
header is `row`, `rows`, `task` or `ledger`, or the first two columns when no header
matches. The status is the first word of the ledger's state cell. The table sits
between two markers under the part's first heading; running the script again replaces it.
"""

import datetime
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
LEDGER = ROOT / "PROFILE_DRILL_PIWIGO_LEDGER.md"
PLAN = ROOT / "docs" / "level-9"
BEGIN = "<!-- status:begin -->"
END = "<!-- status:end -->"
# Not tracked: the index.
EXCLUDED = {"README.md"}
TASK_COLUMNS = {"row", "rows", "task", "ledger"}
ID = re.compile(r"\b[A-Z][A-Z0-9]*-\d+(?:\.\d+[a-z]?)*\b")
CELL = re.compile(r"(?<!\\)\|")
STATE = re.compile(r"^\s*([A-Z]+)\b")


def ledger_rows():
    """Each ledger row with an ID: the ID's state cell and the row's text."""
    rows = {}
    for line in LEDGER.read_text().splitlines():
        if not line.startswith("| "):
            continue
        cells = CELL.split(line)
        if len(cells) != 7:
            continue
        ident = cells[1].strip()
        if ID.fullmatch(ident) and ident not in rows:
            rows[ident] = cells[4].strip()
    return rows


def task_ids(text, known):
    found = []

    def add(ident):
        if ident in known and ident not in found:
            found.append(ident)

    columns = None
    for line in text.splitlines():
        if line.startswith("#"):
            columns = None
            for ident in ID.findall(line):
                add(ident)
        elif line.startswith("|"):
            cells = [c.strip() for c in CELL.split(line)[1:-1]]
            if not cells or set(cells[0]) <= set("-: "):
                continue
            if columns is None:
                header = [c.lower() for c in cells]
                columns = [i for i, c in enumerate(header) if c in TASK_COLUMNS] or [0, 1]
                continue
            for i in columns:
                if i < len(cells):
                    for ident in ID.findall(cells[i]):
                        add(ident)
        else:
            columns = None
    return found


def status_of(state):
    match = STATE.match(state)
    return match.group(1) if match else "SEE LEDGER"


def shorten(state, limit=120):
    text = " ".join(state.split()).replace("|", "\\|")
    if len(text) <= limit:
        return text
    return text[:limit].rsplit(" ", 1)[0] + " …"


def table(ids, rows, head, today):
    lines = [
        BEGIN,
        "",
        "## Status of this part's tasks",
        "",
        f"From the ledger at `{head}` on {today}. Regenerate with "
        "`scripts/plan_status.py`; change the ledger, not this table.",
        "",
        "| task | status | ledger |",
        "|---|---|---|",
    ]
    if not ids:
        lines.append("| — | — | no ledger row named in this part's headings or tables |")
    for ident in ids:
        lines.append(f"| {ident} | {status_of(rows[ident])} | {shorten(rows[ident])} |")
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


def main():
    rows = ledger_rows()
    head = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout.strip()
    today = datetime.date.today().isoformat()
    for path in sorted(PLAN.glob("*.md")):
        if path.name in EXCLUDED:
            continue
        text = path.read_text()
        ids = task_ids(text, rows)
        path.write_text(place(text, table(ids, rows, head, today)))
        print(f"{len(ids):4d} {path.name}")


if __name__ == "__main__":
    main()
