"""One-shot c1 acceptance runner; prevent network in parent and spawned workers."""
from contextlib import ExitStack
import json
import os
from pathlib import Path
import re
import sys
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[4]
OUTPUT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))
LABEL = os.environ.get("FCC_C1_ACCEPTANCE_LABEL", "final")
if not re.fullmatch(r"[a-z0-9_-]+", LABEL):
    raise ValueError("safe acceptance label required")
TRACE = OUTPUT / (LABEL + "-network-attempts.jsonl")
RESULT = OUTPUT / (LABEL + "-suite-result.json")
GUARDS = ExitStack()


def blocked(name):
    def fail(*args, **kwargs):
        # No request arguments or credentials enter the diagnostic.
        line = json.dumps({"entrypoint": name, "pid": os.getpid()}) + "\n"
        fd = os.open(TRACE, os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o600)
        try:
            os.write(fd, line.encode())
        finally:
            os.close(fd)
        raise AssertionError("offline acceptance forbids " + name)
    return fail


# Executed on import in multiprocessing spawn children too. Fixture-level mocks
# can replace these with fake transports; no real connection is permitted.
for entrypoint in (
    "socket.socket.connect", "socket.socket.connect_ex",
    "socket.create_connection", "socket.getaddrinfo", "socket.gethostbyname",
    "socket.gethostbyname_ex", "socket.gethostbyaddr", "urllib.request.urlopen",
    "urllib.request.OpenerDirector.open", "subprocess.run", "subprocess.Popen",
):
    GUARDS.enter_context(patch(entrypoint, side_effect=blocked(entrypoint)))


if __name__ == "__main__":
    if TRACE.exists() or RESULT.exists():
        raise SystemExit("use a fresh acceptance output name; prior proof is retained")
    os.chdir(ROOT)
    started = time.monotonic()
    suite = unittest.defaultTestLoader.discover(str(ROOT / "tests"))
    result = unittest.TextTestRunner(verbosity=1).run(suite)
    attempts = TRACE.read_text().splitlines() if TRACE.exists() else []
    proof = {
        "tests": result.testsRun,
        "failures": len(result.failures),
        "errors": len(result.errors),
        "skipped": len(result.skipped),
        "seconds": round(time.monotonic() - started, 3),
        "blocked_network_or_external_process_calls": len(attempts),
        "passed": result.wasSuccessful() and not attempts,
        "scope": "full suite with network/external subprocess entrypoints guarded",
    }
    with RESULT.open("x") as stream:
        json.dump(proof, stream, indent=2)
        stream.write("\n")
    GUARDS.close()
    print(json.dumps(proof))
    raise SystemExit(0 if proof["passed"] else 1)
