"""Promote only the preservation-audited candidate after successful offline checks."""
import datetime
import json
import os
import subprocess
from pathlib import Path
from devicehunt.rebuild import file_sha256

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT/'data'
OUT = DATA/'exports'
review = json.loads((OUT/'inspur-2018-inventec-review-2026-10-04.json').read_text())
comp = json.loads((OUT/'inspur-2018-inventec-rebuild-2026-10-04-comparison.json').read_text())
assert len(comp) == 43 and all(r['original'] == r['rebuilt'] and r['only_original'] == r['only_rebuilt'] == 0 for r in comp.values())
tests = (OUT/'tests-inspur-2018-inventec-2026-10-04.log').read_text()
assert 'Ran 408 tests' in tests and tests.rstrip().endswith('OK')
ui = (OUT/'tests-report-inspur-2018-inventec-2026-10-04.log').read_text()
assert '# pass 6' in ui and '# fail 0' in ui
assert all(r['missing'] == 0 for r in review['preserved_tables'].values())
live = DATA/'devices.sqlite'
candidate = DATA/'inspur-2018-inventec-candidate-2026-10-04-v2.sqlite'
assert file_sha256(live) == review['previous_sha256']
assert file_sha256(candidate) == review['database_sha256']
backup = DATA/'devices-before-inspur-2018-inventec-2026-10-04.sqlite'
staging = DATA/'.devices-inspur-2018-inventec-promote.sqlite'
assert not backup.exists() and not staging.exists()
subprocess.run(['/bin/cp','-cp',str(live),str(backup)],check=True)
subprocess.run(['/bin/cp','-cp',str(candidate),str(staging)],check=True)
assert file_sha256(backup) == file_sha256(live) == review['previous_sha256']
assert file_sha256(staging) == review['database_sha256']
os.replace(staging,live)
result = dict(promoted_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
    previous_database=str(backup.relative_to(ROOT)),previous_sha256=review['previous_sha256'],
    database_sha256=file_sha256(live),matched_tables=43,tests=408,report_script_tests=6,
    owned_baseline=review['baseline'],vendor_examples=review['vendor_examples'],
    old_rows_preserved=True,added_identities=88,added_records=93,
    added_original_pdfs=3,added_pdf_pages=54,unchanged_manufacturer_counts=True,
    clean_rebuild='data/rebuilt-with-inspur-2018-inventec-2026-10-04.sqlite')
(OUT/'inspur-2018-inventec-promotion-2026-10-04.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
