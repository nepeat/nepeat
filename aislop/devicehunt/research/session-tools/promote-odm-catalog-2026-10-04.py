import json,os,subprocess
from pathlib import Path
from devicehunt.rebuild import file_sha256
root=Path('/Users/nep/nocloud/git/nepeat/aislop/fcc_shenanigans');out=root/'data/exports';data=root/'data'
comparison=json.loads((out/'odm-verified-rebuild-2026-10-04-comparison.json').read_text());assert len(comparison)==42 and all(x['original']==x['rebuilt'] and x['only_original']==x['only_rebuilt']==0 for x in comparison.values())
review=json.loads((out/'odm-candidate-review-2026-10-04.json').read_text());candidate=data/'odm-expanded-candidate-2026-10-04.sqlite';assert file_sha256(candidate)==review['database_sha256']
live=data/'devices.sqlite';before=file_sha256(live);assert before=='57ecfa75fb3bd48fe2b9a47318a73635645c96b2084fe94719aa966572a0d678'
backup=data/'devices-before-odm-2026-10-04.sqlite';assert not backup.exists();staging=data/'.devices-odm-promote.sqlite';assert not staging.exists()
subprocess.run(['/bin/cp','-cp',str(live),str(backup)],check=True);assert file_sha256(backup)==before
subprocess.run(['/bin/cp','-cp',str(candidate),str(staging)],check=True);assert file_sha256(staging)==review['database_sha256']
assert file_sha256(live)==before
os.replace(staging,live)
result=dict(before_sha256=before,after_sha256=file_sha256(live),previous_database=str(backup.relative_to(root)),promoted_from=str(candidate.relative_to(root)),matched_tables=42,owned_baseline=review['baseline'],original_rows_preserved=True)
(out/'odm-promotion-2026-10-04.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
