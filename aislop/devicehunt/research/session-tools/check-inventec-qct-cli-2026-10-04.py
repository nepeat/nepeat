"""Verify user-facing search/specification/example commands on the live catalog."""
import json
import subprocess
import sys
from pathlib import Path
from devicehunt.rebuild import file_sha256

ROOT = Path(__file__).resolve().parents[2]
before = file_sha256(ROOT / 'data/devices.sqlite')
cases = [
    ['search', 'Horsea', '--vendor', 'Inventec', '--role', 'manufacturer'],
    ['specs', 'Horsea'],
    ['search', 'K905G6_G6P', '--vendor', 'Inventec'],
    ['search', 'QuantaPlex T41S-2U', '--vendor', 'Quanta'],
    ['validate-inventory', '--inventory', 'validation/vendor-examples.json'],
]
checks = []
for args in cases:
    process = subprocess.run([sys.executable, '-m', 'devicehunt', *args, '--format', 'json'],
                             cwd=ROOT, capture_output=True, text=True)
    expected_exit = 2 if args[0] == 'validate-inventory' else 0
    assert process.returncode == expected_exit, (args, process.stderr)
    data = json.loads(process.stdout)
    assert data
    if args[0] == 'validate-inventory':
        assert data['summary'] == dict(total=2, hits=0, gaps=2)
    elif args[0] == 'search':
        assert len(data) == 1 and data[0]['model'] == args[1], (args, data)
    checks.append(dict(arguments=args, exit_code=process.returncode, result=data))
assert file_sha256(ROOT / 'data/devices.sqlite') == before
(ROOT / 'data/exports/inventec-qct-cli-checks-2026-10-04.json').write_text(
    json.dumps(dict(database_sha256=before, read_only=True, checks=checks), indent=2) + '\n')
print('Verified four search/specification commands and two explicit example gaps.')
