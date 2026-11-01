"""Run only owned and affected suites, explicitly against the private overlay."""
import hashlib
import json
import os
from pathlib import Path
import time
import unittest
from unittest.mock import patch
import devicehunt
from devicehunt import specifications, __main__

OWN = Path(__file__).resolve().parent
ROOT = OWN.parents[3]
resolved = {module.__name__: str(Path(module.__file__).resolve()) for module in (devicehunt, specifications, __main__)}
assert all(Path(path).is_relative_to(OWN / 'overlay') for path in resolved.values()), resolved
assert Path(os.environ['PYTHONPATH']).resolve() == OWN / 'overlay'
assert Path.cwd().resolve() == OWN
hashes = json.loads((OWN / 'source-hashes.json').read_text())
def unchanged():
    return all(hashlib.sha256((ROOT / path).read_bytes()).hexdigest() == value for path, value in hashes.items())
assert unchanged(), 'frozen production changed before tests'
suite = unittest.TestSuite()
loader = unittest.TestLoader()
suite.addTests(loader.discover(str(OWN / 'tests'), pattern='test_specification_grouping.py'))
for name in ('test_specifications.py', 'test_specifications_source_facts.py', 'test_listing_specifications.py'):
    suite.addTests(unittest.TestLoader().discover(str(ROOT / 'tests'), pattern=name))
blocked = []
def reject(*args, **kwargs):
    blocked.append('external call')
    raise AssertionError('network/process unavailable in focused tests')
start = time.monotonic()
with patch('urllib.request.urlopen', reject), patch('socket.create_connection', reject), \
     patch('subprocess.run', reject), patch('subprocess.Popen', reject):
    result = unittest.TextTestRunner(verbosity=2).run(suite)
proof = {'module_paths': resolved, 'cwd': str(Path.cwd()), 'pythonpath': os.environ['PYTHONPATH'],
         'tests': result.testsRun, 'failures': len(result.failures), 'errors': len(result.errors),
         'skipped': len(result.skipped), 'blocked_calls': blocked, 'elapsed_seconds': time.monotonic()-start,
         'all_209_production_files_unchanged': unchanged()}
(OWN / 'focused-tests-result.json').write_text(json.dumps(proof, indent=2)+'\n')
assert unchanged(), 'production changed during focused tests'
raise SystemExit(not result.wasSuccessful())
