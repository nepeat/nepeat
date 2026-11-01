"""Private runner for the applied patch in a normal production package layout."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import time
import unittest
from unittest.mock import patch

import devicehunt
from devicehunt import specifications, __main__

OWN = Path(__file__).resolve().parent
NORMAL = OWN / 'normal-layout'
assert Path.cwd() == NORMAL
assert Path(os.environ['PYTHONPATH']).resolve() == NORMAL
resolved = {m.__name__: str(Path(m.__file__).resolve()) for m in (devicehunt, specifications, __main__)}
assert all(Path(path).is_relative_to(NORMAL / 'devicehunt') for path in resolved.values())
assert not (NORMAL / 'overlay').exists() and not (NORMAL / 'originals').exists()
assert not (NORMAL / 'data').exists()
suite = unittest.TestSuite()
for name in ('test_specification_grouping.py', 'test_specifications.py',
             'test_specifications_source_facts.py', 'test_listing_specifications.py'):
    suite.addTests(unittest.TestLoader().discover(str(NORMAL / 'tests'), pattern=name))
blocked = []
def reject(*args, **kwargs):
    blocked.append('external call')
    raise AssertionError('external call forbidden in tests')
start = time.monotonic()
with patch('urllib.request.urlopen', reject), patch('socket.create_connection', reject), \
     patch('subprocess.Popen', reject), patch('subprocess.run', reject):
    result = unittest.TextTestRunner(verbosity=2).run(suite)
expected = json.loads((OWN / 'patch-files.json').read_text())['production_patch_files']
assert all(hashlib.sha256((NORMAL / name).read_bytes()).hexdigest() == data['after_sha256']
           for name, data in expected.items())
assert not (NORMAL / 'data').exists()
proof = {'module_paths': resolved, 'cwd': str(Path.cwd()), 'pythonpath': os.environ['PYTHONPATH'],
         'private_overlay_and_originals_absent': True, 'default_data_absent': True,
         'tests': result.testsRun, 'failures': len(result.failures), 'errors': len(result.errors),
         'skipped': len(result.skipped), 'blocked_calls': blocked,
         'elapsed_seconds': time.monotonic() - start, 'applied_patch_files_match_reviewed_bytes': True}
(OWN / 'normal-layout-tests-result.json').write_text(json.dumps(proof, indent=2) + '\n')
raise SystemExit(not result.wasSuccessful())
