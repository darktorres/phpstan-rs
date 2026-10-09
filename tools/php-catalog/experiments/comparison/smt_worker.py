#!/usr/bin/env python3
"""Pinned Z3 SMT-LIB child-process adapter, isolated from the Rust runtime."""
import sys
import z3
from z3.z3core import Z3_eval_smtlib2_string

if z3.get_version_string() != '5.1.0':
    raise RuntimeError('Required Z3 5.1.0 is missing')
context = z3.Context()
print(Z3_eval_smtlib2_string(context.ref(), sys.stdin.read()), end='')
