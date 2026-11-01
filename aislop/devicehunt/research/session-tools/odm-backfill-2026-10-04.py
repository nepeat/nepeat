import json,sqlite3,shutil
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt.rebuild import read_manifest,file_sha256
root=Path('/Users/nep/nocloud/git/nepeat/aislop/fcc_shenanigans');manifest=root/'data/import-manifest.json';_,data=read_manifest(manifest)
live=root/'data/devices.sqlite';before=file_sha256(live);assert before=='57ecfa75fb3bd48fe2b9a47318a73635645c96b2084fe94719aa966572a0d678'
# Keep live catalog unchanged while reviewing the full expanded import result.
trial=root/'data/odm-expanded-candidate-2026-10-04.sqlite';assert not trial.exists()
c=Catalog(trial)
try:
 for n,job in enumerate(data['imports'],1):
  result=import_source(c,job['source'],manifest.parent/job['path']);print(n,job['source'],result,flush=True)
 print('NEW PRODUCTS',import_source(c,'curated',root/'data/odm-products-2026-10-04.json'),flush=True)
 assert c.db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
 assert not c.db.execute('PRAGMA foreign_key_check').fetchall()
finally:c.db.close()
assert file_sha256(live)==before
print('Expanded candidate ready; live database unchanged.',flush=True)
