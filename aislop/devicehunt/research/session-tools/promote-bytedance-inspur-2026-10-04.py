"""Promote the reviewed thirteen-target candidate after clean comparison."""
import datetime,json,os,subprocess
from pathlib import Path
from devicehunt.rebuild import file_sha256
ROOT=Path(__file__).resolve().parents[2];DATA=ROOT/'data';OUT=DATA/'exports'
review=json.loads((OUT/'bytedance-inspur-review-2026-10-04.json').read_text())
comp=json.loads((OUT/'bytedance-inspur-rebuild-2026-10-04-comparison.json').read_text())
assert len(comp)==42 and all(r['original']==r['rebuilt'] and r['only_original']==r['only_rebuilt']==0 for r in comp.values())
testlog=(OUT/'tests-bytedance-inspur-2026-10-04.log').read_text()
assert 'Ran 399 tests' in testlog and testlog.rstrip().endswith('OK')
ui=(OUT/'tests-report-bytedance-inspur-2026-10-04.log').read_text()
assert '# pass 6' in ui and '# fail 0' in ui
live=DATA/'devices.sqlite';candidate=DATA/'bytedance-inspur-candidate-2026-10-04.sqlite'
assert file_sha256(live)==review['previous_sha256']
assert file_sha256(candidate)==review['database_sha256']
backup=DATA/'devices-before-bytedance-inspur-2026-10-04.sqlite';staging=DATA/'.devices-bytedance-inspur-promote.sqlite'
assert not backup.exists() and not staging.exists()
subprocess.run(['/bin/cp','-cp',str(live),str(backup)],check=True)
subprocess.run(['/bin/cp','-cp',str(candidate),str(staging)],check=True)
assert file_sha256(backup)==file_sha256(live)==review['previous_sha256']
assert file_sha256(staging)==review['database_sha256']
os.replace(staging,live)
result=dict(promoted_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),previous_database=str(backup.relative_to(ROOT)),
    previous_sha256=review['previous_sha256'],database_sha256=file_sha256(live),matched_tables=42,tests=399,report_script_tests=6,
    owned_baseline=review['owned_baseline'],vendor_examples=review['vendor_examples'],
    original_source_assertions_and_identity_keys_preserved=True,display_metadata_changes=len(review['display_metadata_changes']),
    added_identities=53,added_records=60,added_pdfs=2,clean_rebuild='data/rebuilt-with-bytedance-inspur-2026-10-04.sqlite')
(OUT/'bytedance-inspur-promotion-2026-10-04.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
