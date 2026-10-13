"""Classifies the positions of a closing scan that no probe reached, with the reason each one is not covered.

usage: gap-reasons.py SCAN.jsonl OUT.tsv

A position is covered when some probe of it returned or ran the sink. The reason of an uncovered position is the
category of the first message that a probe threw, or the signal that killed it. The refusal categories name the
positions where no input can reach the call's body: the method refuses every input, the class refuses every object
it has initialised, the driver refuses the attribute, or the call needs a display the sandbox does not have.
"""
import collections
import json
import re
import sys

rows = []
for line in open(sys.argv[1]):
    try:
        row = json.loads(line)
    except ValueError:
        continue
    if isinstance(row, list):
        rows.append(row)

seen = collections.defaultdict(list)
covered = set()
for row in rows:
    if len(row) < 4:
        continue
    key = (row[0], row[1])
    seen[key].append(row)
    if str(row[3]) == 'ok' or (len(row) > 4 and row[4]):
        covered.add(key)

# Order matters: the first pattern that matches the first message names the reason.
RULES = [
    ('crash', r'^killed:'),
    ('refused-by-design', r'Array access by key is not supported'),
    ('refused-by-class', r'Cannot modify readonly|are immutable|Cannot call constructor twice|Invalid serialization data for'),
    ('refused-by-driver', r'Driver does not support this function'),
    ('refused-by-environment', r'does not provide a content type'),
    ('needs-display', r'unable to open X server'),
    ('server', r'went away|getaddrinfo|not fully initialized|PostgreSQL|pg_|Redis|socket transport|mysqli|FTP|Connection|connection|Connect|No such file or directory.*ftp'),
    ('not-recordable', r'dynamically|must be called exactly once|readonly'),
    ('serialised', r'serialization data|Error at offset|Invalid serialization|Incomplete or ill-typed|unserialize'),
    ('session', r'Session is not active|SessionHandlerInterface'),
    ('state', r'No string was provided|Already initialised|unconstructed|must be opened|not imported|imported|No stream arrays|At least one array|brigade|userfilter|uninitiali|empty Imagick|Invalid or uninitialized|already|constructor twice|must be loaded|must be set|Index out of range|out of range|is an internal class|Invalid finfo|already initialized|Couldn.t fetch|Directory object|not assigned|not a valid process|Wrong Document|Not Found Error|Syntax Error|not in a class|no clip path'),
    ('file-or-font', r'does not exist|Failed to open stream|path does not exist|font|No such file'),
    ('class-name', r'must be a class|valid class name|Class ".*" does not exist|Extension ".*" does not exist|must be a class name|is not a valid class'),
    ('arity', r'expects at least|expects exactly|Too few arguments|Requires \d'),
    ('scope', r'no class scope'),
    ('value', r'.'),
]


def reason(message):
    for name, pattern in RULES:
        if re.search(pattern, message):
            return name
    return 'value'


gaps = [key for key in seen if key not in covered]
counts = collections.Counter()
with open(sys.argv[2], 'w') as out:
    out.write('builtin\tposition\treason\tfirst_message\n')
    for key in sorted(gaps):
        messages = [str(x[3]) for x in seen[key] if str(x[3]).startswith(('threw', 'killed'))]
        message = messages[0] if messages else str(seen[key][0][3])
        name = 'scope' if key[0] == 'forward_static_call' else reason(message)
        counts[name] += 1
        out.write(f'{key[0]}\t{key[1]}\t{name}\t{message[:160]}\n')
print('positions', len(seen), 'covered', len(covered), 'uncovered', len(gaps))
print(dict(counts))
