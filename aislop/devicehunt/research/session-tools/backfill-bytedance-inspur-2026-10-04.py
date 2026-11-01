"""Fresh expanded-target candidate; leave live database and manifest unchanged."""
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt.rebuild import read_manifest,file_sha256
ROOT=Path(__file__).resolve().parents[2];DATA=ROOT/'data'
live=DATA/'devices.sqlite';before=file_sha256(live)
assert before=='f329ebd945e58327cdfc1f0a8127363b69457478535dd6b1587bdb7c1c3d4c7d'
_,manifest=read_manifest(DATA/'import-manifest.json')
candidate=DATA/'bytedance-inspur-candidate-2026-10-04.sqlite';assert not candidate.exists()
c=Catalog(candidate)
try:
    for n,job in enumerate(manifest['imports'],1):
        path=DATA/job['path']
        if job['source']=='reference-documents':path=DATA/'raw/reference-documents-with-bytedance-inspur-2026-10-04.json'
        print(n,job['source'],import_source(c,job['source'],path),flush=True)
    print('NEW FACTS',import_source(c,'curated',DATA/'bytedance-inspur-facts-2026-10-04.json'),flush=True)
    assert c.db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
    assert not c.db.execute('PRAGMA foreign_key_check').fetchall()
finally:c.db.close()
assert file_sha256(live)==before
print('Candidate built; live database unchanged.',flush=True)
