#!/usr/bin/env python3
"""Lift actual binaries and record bounded symbolic-execution results."""
import argparse
import json
from pathlib import Path
import time

import angr
import claripy

parser = argparse.ArgumentParser(__doc__)
parser.add_argument('--fixture', type=Path, required=True)
parser.add_argument('--php', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
findings = []
for binary, names in [(args.fixture, ['first', 'second', 'nested_copy', 'overlap_copy', 'negative_index']),
                      (args.php, ['zif_array_map', 'zif_strlen', 'zend_call_function', '_php_stream_free'])]:
    start = time.monotonic()
    project = angr.Project(str(binary), auto_load_libs=False)
    for name in names:
        symbol = project.loader.find_symbol(name)
        if not symbol:
            findings.append(dict(binary=str(binary), name=name, status='missing-symbol'))
            continue
        block = project.factory.block(symbol.rebased_addr)
        record = dict(binary=str(binary), name=name, address=symbol.rebased_addr,
                      bytes=block.size, ir_statements=len(block.vex.statements),
                      jumpkind=block.vex.jumpkind, source_layout_recovered=False)
        if binary == args.fixture:
            argument = claripy.BVV(0, 32)
            state = project.factory.call_state(symbol.rebased_addr, *([argument] if name in ['first', 'second'] else []))
            manager = project.factory.simulation_manager(state)
            manager.run(n=1000)
            outcomes = [s.solver.eval(s.regs.eax) for s in manager.deadended]
            record.update(steps_limit=1000, active_states=len(manager.active),
                          errored_states=len(manager.errored), unconstrained_states=len(manager.unconstrained),
                          outcomes=outcomes, status='bounded-witness')
            assert not manager.active and not manager.errored and not manager.unconstrained
            expected = {'first': 1, 'second': 2, 'nested_copy': 4, 'overlap_copy': 2, 'negative_index': 1}[name]
            assert outcomes == [expected], record
        else:
            record['status'] = 'entry-block-lifted-no-PHP-summary'
        findings.append(record)
    findings.append(dict(binary=str(binary), load_and_probe_seconds=time.monotonic()-start,
                         missing_native_libraries='open', catalog_closure=False))
args.output.write_text(json.dumps(findings, indent=2) + '\n')
print(json.dumps(dict(records=len(findings), catalog_closure=False)))
