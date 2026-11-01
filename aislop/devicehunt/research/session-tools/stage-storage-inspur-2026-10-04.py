"""Stage schema 28 and reviewed sources, audit prior rows, and prepare offline manifest."""
import json
import subprocess
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt.rebuild import file_sha256, read_manifest
from devicehunt import validation, vendor_inventory

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT/'data'
OUT = DATA/'exports'
LIVE = DATA/'devices.sqlite'
BEFORE = '58f8eb5d0d273b2328a53e12a777796c149208943aca317aa4e368cd5e09c544'
assert file_sha256(LIVE) == BEFORE
_, manifest = read_manifest(DATA/'import-manifest.json')
candidate = DATA/'storage-inspur-candidate-2026-10-04.sqlite'
bundle = DATA/'raw/reference-documents-with-storage-inspur-2026-10-04.json'
facts = DATA/'storage-inspur-facts-2026-10-04.json'
support = DATA/'raw/storage-inspur-2026-10-04/inspur-support-bundle.json'
assert not candidate.exists()
subprocess.run(['/bin/cp','-cp',str(LIVE),str(candidate)],check=True)
c=Catalog(candidate)
for source,path in [('reference-documents',bundle),('inspur-support',support),('curated',facts)]:
    print(source,import_source(c,source,path),flush=True)
c.db.close()
c=Catalog(candidate,read_only=True)
db=c.db
db.execute('ATTACH DATABASE ? AS old',(LIVE.as_uri()+'?mode=ro',))
tables=json.loads((OUT/'inventec-qct-rebuild-2026-10-04-comparison.json').read_text())
audit={}
for table in tables:
    if table in ('sources','search_index','document_search'):continue
    cols=[r['name'] for r in db.execute('PRAGMA main.table_info('+table+')')
          if r['name'] not in ('observed_at','fetched_at','cache_path')]
    fields=','.join('"'+name+'"' for name in cols)
    missing=db.execute(f'SELECT count(*) FROM (SELECT {fields} FROM old.{table} EXCEPT SELECT {fields} FROM main.{table})').fetchone()[0]
    assert missing==0,(table,missing)
    audit[table]=dict(previous=db.execute('SELECT count(*) FROM old.'+table).fetchone()[0],
                      current=db.execute('SELECT count(*) FROM main.'+table).fetchone()[0],missing=missing)
for old in db.execute('SELECT * FROM old.sources'):
    new=db.execute('SELECT * FROM main.sources WHERE id=?',(old['id'],)).fetchone()
    assert new is not None
    for column in old.keys():
        if column in ('fetched_at','cache_path'):continue
        if column=='sha256' and old['id'].startswith('reference-pdf-'):
            assert new[column]==file_sha256(bundle)
        else:assert new[column]==old[column],(old['id'],column)
assert db.execute('PRAGMA user_version').fetchone()[0]==28
assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
assert not db.execute('PRAGMA foreign_key_check').fetchall()
base=validation.run(db,ROOT/'validation/owned-hardware.json')
examples=validation.run(db,ROOT/'validation/vendor-examples.json')
assert base['summary']==dict(total=10,hits=10,gaps=0)
assert base['evidence_summary']['document_snapshot_hits']==9
assert examples['summary']==dict(total=2,hits=0,gaps=2)
assert db.execute("SELECT count(*) FROM records WHERE source_id='inspur-support-models'").fetchone()[0]==0
raw=json.loads((support.parent/'inspur-product-names.json').read_text())
stored=[json.loads(r[0]) for r in db.execute('SELECT raw_json FROM inspur_support_entries ORDER BY row_number')]
assert stored==raw and len(stored)==1064
inventories={scope:vendor_inventory.run(db,scope=scope,limit=-1) for scope in ('manufacturer','factory','all')}
previous=json.loads((OUT/'latest-export-audit.json').read_text())['inventories']
for scope in ('manufacturer','factory'):
    for key in ('identity_target_pairs','unique_identities','counts_by_target'):
        assert inventories[scope][key]==previous[scope][key],(scope,key)
counts={key:db.execute('SELECT count(*) FROM '+('fcc_grantees' if key=='grantees' else key)).fetchone()[0]
        for key in manifest['expected'] if key!='sources'}
counts['sources']=dict(db.execute('SELECT id,records_seen FROM sources'))
assert (counts['devices'],counts['records'],len(counts['sources']),counts['documents'],counts['document_pages'],counts['inspur_support_entries'])==(3942,5143,253,46,3523,1064)
report=dict(previous_sha256=BEFORE,schema=28,baseline=base['summary'],evidence_summary=base['evidence_summary'],
    vendor_examples=examples['summary'],preserved_tables=audit,expected_counts=counts,
    added_identities=1,added_records=3,added_support_rows=1064,added_original_pdfs=2,
    inventories={scope:{key:value[key] for key in ('identity_target_pairs','unique_identities','counts_by_target')} for scope,value in inventories.items()})
db.close()
report['database_sha256']=file_sha256(candidate)
assert file_sha256(LIVE)==BEFORE
(OUT/'storage-inspur-review-2026-10-04.json').write_text(json.dumps(report,indent=2)+'\n')
backup=DATA/'import-manifest-before-storage-inspur-2026-10-04.json'
assert not backup.exists()
backup.write_bytes((DATA/'import-manifest.json').read_bytes())
jobs=[job for job in manifest['imports'] if job['source']=='reference-documents']
assert len(jobs)==1
jobs[0]['path']=str(bundle.relative_to(DATA))
manifest['imports'] += [dict(source='inspur-support',path=str(support.relative_to(DATA))),dict(source='curated',path=facts.name)]
inputs=list(support.parent.iterdir())+[bundle,facts]
for stem in ('inspur-nf5280m5-whitepaper','inventec-e850g4-brief'):
    inputs+=list((DATA/'raw').glob(stem+'-2026-10-04.*'))
for path in inputs:
    assert path.is_file()
    manifest['files'][str(path.relative_to(DATA))]=file_sha256(path)
manifest['expected']=counts
(DATA/'import-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps(dict(counts={k:v for k,v in counts.items() if k!='sources'},sources=len(counts['sources']),
    jobs=len(manifest['imports']),inputs=len(manifest['files'])),indent=2))
