"""Promote only the audited, rebuild-matched model evidence candidate."""
import datetime
import json
import os
from pathlib import Path
import subprocess
from devicehunt.rebuild import file_sha256

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'data'
OUT = DATA / 'exports'
review = json.loads((OUT / 'odm-model-review-2026-10-04.json').read_text())
comparison = json.loads((OUT / 'odm-model-rebuild-2026-10-04-comparison.json').read_text())
assert len(comparison) == 42
assert all(row['original'] == row['rebuilt'] and row['only_original'] == row['only_rebuilt'] == 0
           for row in comparison.values())
assert 'Ran 395 tests' in (OUT / 'tests-odm-models-2026-10-04.log').read_text()
assert (OUT / 'tests-odm-models-2026-10-04.log').read_text().rstrip().endswith('OK')
live = DATA / 'devices.sqlite'
candidate = DATA / 'odm-model-candidate-2026-10-04.sqlite'
assert file_sha256(live) == review['previous_sha256']
assert file_sha256(candidate) == review['database_sha256']
backup = DATA / 'devices-before-odm-models-2026-10-04.sqlite'
staging = DATA / '.devices-odm-models-promote.sqlite'
assert not backup.exists() and not staging.exists()
subprocess.run(['/bin/cp', '-cp', str(live), str(backup)], check=True)
subprocess.run(['/bin/cp', '-cp', str(candidate), str(staging)], check=True)
assert file_sha256(backup) == file_sha256(live) == review['previous_sha256']
assert file_sha256(staging) == review['database_sha256']
os.replace(staging, live)
result = dict(promoted_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
              previous_database=str(backup.relative_to(ROOT)), previous_sha256=review['previous_sha256'],
              database_sha256=file_sha256(live), candidate=str(candidate.relative_to(ROOT)),
              clean_rebuild='data/rebuilt-with-odm-models-2026-10-04.sqlite',
              matched_tables=42, tests=395, baseline=review['baseline'],
              old_rows_preserved=True, added_identities=2, added_records=3, added_pdfs=2)
(OUT / 'odm-model-promotion-2026-10-04.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result, indent=2))
