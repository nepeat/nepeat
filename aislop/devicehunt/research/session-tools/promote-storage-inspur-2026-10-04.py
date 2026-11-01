"""Promote the preservation-audited storage/Inspur candidate after offline rebuild."""
import datetime
import json
import os
import subprocess
from pathlib import Path
from devicehunt.rebuild import file_sha256

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'data'
OUT = DATA / 'exports'
review = json.loads((OUT / 'storage-inspur-review-2026-10-04.json').read_text())
comp = json.loads((OUT / 'storage-inspur-rebuild-2026-10-04-comparison.json').read_text())
assert len(comp) == 43 and all(r['original'] == r['rebuilt'] and r['only_original'] == r['only_rebuilt'] == 0 for r in comp.values())
tests = (OUT / 'tests-storage-inspur-2026-10-04.log').read_text()
assert 'Ran 408 tests' in tests and tests.rstrip().endswith('OK')
ui = (OUT / 'tests-report-storage-inspur-2026-10-04.log').read_text()
assert '# pass 6' in ui and '# fail 0' in ui
assert all(r['missing'] == 0 for r in review['preserved_tables'].values())
live = DATA / 'devices.sqlite'
candidate = DATA / 'storage-inspur-candidate-2026-10-04.sqlite'
assert file_sha256(live) == review['previous_sha256']
assert file_sha256(candidate) == review['database_sha256']
backup = DATA / 'devices-before-storage-inspur-2026-10-04.sqlite'
staging = DATA / '.devices-storage-inspur-promote.sqlite'
assert not backup.exists() and not staging.exists()
subprocess.run(['/bin/cp', '-cp', str(live), str(backup)], check=True)
subprocess.run(['/bin/cp', '-cp', str(candidate), str(staging)], check=True)
assert file_sha256(backup) == file_sha256(live) == review['previous_sha256']
assert file_sha256(staging) == review['database_sha256']
os.replace(staging, live)
result = dict(promoted_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
    previous_database=str(backup.relative_to(ROOT)), previous_sha256=review['previous_sha256'],
    database_sha256=file_sha256(live), matched_tables=43, tests=408, report_script_tests=6,
    owned_baseline=review['baseline'], vendor_examples=review['vendor_examples'],
    old_rows_preserved=True, added_identities=1, added_records=3,
    added_original_pdfs=2, added_support_rows=1064, unchanged_manufacturer_counts=True,
    clean_rebuild='data/rebuilt-with-storage-inspur-2026-10-04.sqlite')
(OUT / 'storage-inspur-promotion-2026-10-04.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result, indent=2))
