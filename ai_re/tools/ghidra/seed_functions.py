"""Seed entry-point functions in a raw-binary Ghidra project, then re-analyse.

Raw (BinaryLoader) imports have no entry points, so auto-analysis finds only
code reachable from call targets it happens to discover. Give it the reset
path explicitly.

Run with:  ghidra-py tools/ghidra/seed_functions.py <project.gpr> <program-name> NAME=ADDR [NAME=ADDR ...]
e.g.       ghidra-py tools/ghidra/seed_functions.py \
              devices/delta-tn48m/ghidra/tn48m.gpr ap806-bootrom.bin \
              reset_vector=0xFFFF0000 bootrom_init=0xFFFF040C
"""
import sys
from pathlib import Path

import pyghidra

pyghidra.start()

gpr = Path(sys.argv[1]).resolve()
prog_name = sys.argv[2]
seeds = [a.split("=", 1) for a in sys.argv[3:]]

with pyghidra.open_project(gpr.parent, gpr.stem) as project:
    with pyghidra.program_context(project, "/" + prog_name) as program:
        from ghidra.program.flatapi import FlatProgramAPI
        from ghidra.program.model.symbol import SourceType

        flat = FlatProgramAPI(program)
        tx = program.startTransaction("seed entry functions")
        try:
            for name, addr_s in seeds:
                addr = program.getAddressFactory().getAddress(addr_s)
                flat.disassemble(addr)
                fn = flat.getFunctionAt(addr) or flat.createFunction(addr, name)
                if fn is None:
                    print(f"FAILED to create function {name} @ {addr}")
                    continue
                fn.setName(name, SourceType.USER_DEFINED)
                print(f"function {fn.getName()} @ {addr}")
        finally:
            program.endTransaction(tx, True)
        pyghidra.analyze(program)
        from ghidra.util.task import TaskMonitor
        program.getDomainFile().save(TaskMonitor.DUMMY)
        print("functions now:", program.getFunctionManager().getFunctionCount())
