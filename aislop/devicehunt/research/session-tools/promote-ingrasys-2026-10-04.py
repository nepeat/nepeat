"""Promote the preservation-audited, clean-rebuild-matched Ingrasys candidate."""
import datetime
import json
import os
import subprocess
from pathlib import Path
from devicehunt.rebuild import file_sha256

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'data'
OUT = DATA / 'exports'
review = json.loads((OUT / 'ingrasys-review-2026-10-04.json').read_text())
comp = json.loads((OUT / 'ingrasys-rebuild-2026-10-04-comparison.json').read_text())
assert len(comp) == 42 and all(r['original'] == r['rebuilt'] and r['only_original'] == r['only_rebuilt'] == 0 for r in comp.values())
tests = (OUT / 'tests-ingrasys-2026-10-04.log').read_text()
assert 'Ran 399 tests' in tests and tests.rstrip().endswith('OK')
ui = (OUT / 'tests-report-ingrasys-2026-10-04.log').read_text()
assert '# pass 6' in ui and '# fail 0' in ui
live = DATA / 'devices.sqlite'
candidate = DATA / 'ingrasys-candidate-2026-10-04.sqlite'
assert file_sha256(live) == review['previous_sha256']
assert file_sha256(candidate) == review['database_sha256']
backup = DATA / 'devices-before-ingrasys-2026-10-04.sqlite'
staging = DATA / '.devices-ingrasys-promote.sqlite'
assert not backup.exists() and not staging.exists()
subprocess.run(['/bin/cp', '-cp', str(live), str(backup)], check=True)
subprocess.run(['/bin/cp', '-cp', str(candidate), str(staging)], check=True)
assert file_sha256(backup) == file_sha256(live) == review['previous_sha256']
assert file_sha256(staging) == review['database_sha256']
os.replace(staging, live)
result = dict(promoted_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
    previous_database=str(backup.relative_to(ROOT)), previous_sha256=review['previous_sha256'],
    database_sha256=file_sha256(live), matched_tables=42, tests=399, report_script_tests=6,
    owned_baseline=review['baseline'], vendor_examples=review['vendor_examples'],
    old_rows_preserved=True, added_identities=8, added_records=17,
    added_pdf_derivatives=1, unchanged_manufacturer_counts=True,
    clean_rebuild='data/rebuilt-with-ingrasys-2026-10-04.sqlite')
(OUT / 'ingrasys-promotion-2026-10-04.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result, indent=2))
