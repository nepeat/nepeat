"""Audit additions and only then update the reproducible import manifest."""
import json
from pathlib import Path
from devicehunt.catalog import Catalog, target_for
from devicehunt.rebuild import file_sha256,read_manifest
from devicehunt import validation,vendor_inventory
ROOT=Path(__file__).resolve().parents[2];DATA=ROOT/'data';OUT=DATA/'exports'
live=DATA/'devices.sqlite';candidate=DATA/'bytedance-inspur-candidate-2026-10-04.sqlite'
before=file_sha256(live);assert before=='f329ebd945e58327cdfc1f0a8127363b69457478535dd6b1587bdb7c1c3d4c7d'
_,manifest=read_manifest(DATA/'import-manifest.json')
c=Catalog(candidate,read_only=True);db=c.db
db.execute('ATTACH DATABASE ? AS old',(live.as_uri()+'?mode=ro',))
tables=json.loads((OUT/'odm-model-rebuild-2026-10-04-comparison.json').read_text());audit={}
display_changes=[]
for old in db.execute('SELECT * FROM old.devices EXCEPT SELECT * FROM main.devices'):
    new=db.execute('SELECT * FROM main.devices WHERE id=?',(old['id'],)).fetchone();assert new
    assert all(old[key]==new[key] for key in ('id','brand','model','revision'))
    # Expanded NTRA screening is earlier in the manifest than these existing
    # Quanta sources. INSERT OR IGNORE now takes its display title/category;
    # every old source record/fact is checked independently below.
    assert old['brand']=='Quanta' and old['model'] in ('A5W','A56','A5QP','A5PE','A5TS','A5QB','A5WM')
    row=db.execute("SELECT * FROM records WHERE device_id=? AND source_id='ntra-2026-01'",(old['id'],)).fetchone();assert row
    assert not db.execute('SELECT 1 FROM old.records WHERE id=?',(row['id'],)).fetchone()
    cells=json.loads(row['raw_json'])['cells']
    assert new['title']==cells[2]+' '+cells[3] and new['kind']==cells[1].lower()
    display_changes.append(dict(previous=dict(old),current=dict(new),new_source_record=row['id']))
for table in tables:
    if table in ('sources','search_index','document_search','affiliations'):continue
    cols=[r['name'] for r in db.execute('PRAGMA main.table_info('+table+')') if r['name'] not in ('observed_at','fetched_at','cache_path')]
    if table=='devices':cols=['id','brand','model','revision']
    fields=','.join('"'+col+'"' for col in cols)
    missing=db.execute(f'SELECT count(*) FROM (SELECT {fields} FROM old.{table} EXCEPT SELECT {fields} FROM main.{table})').fetchone()[0]
    assert missing==0,(table,missing)
    audit[table]=dict(previous=db.execute('SELECT count(*) FROM old.'+table).fetchone()[0],current=db.execute('SELECT count(*) FROM main.'+table).fetchone()[0],missing=missing)
remapped=[]
for old in db.execute('SELECT * FROM old.affiliations'):
    new=db.execute('SELECT * FROM main.affiliations WHERE record_id=? AND company=? AND role=?',(old['record_id'],old['company'],old['role'])).fetchone()
    assert new is not None
    assert new['target']==(old['target'] or target_for(old['company']))
    if old['target']!=new['target']:remapped.append(dict(previous=dict(old),current=dict(new)))
bundle=DATA/'raw/reference-documents-with-bytedance-inspur-2026-10-04.json'
for old in db.execute('SELECT * FROM old.sources'):
    new=db.execute('SELECT * FROM main.sources WHERE id=?',(old['id'],)).fetchone();assert new
    for col in old.keys():
        if col in ('fetched_at','cache_path','records_seen'):continue
        if col=='sha256' and old['id'].startswith('reference-pdf-'):
            assert new[col]==file_sha256(bundle);continue
        assert old[col]==new[col],(old['id'],col)
assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
assert not db.execute('PRAGMA foreign_key_check').fetchall()
base=validation.run(db,ROOT/'validation/owned-hardware.json')
examples=validation.run(db,ROOT/'validation/vendor-examples.json')
assert base['summary']==dict(total=10,hits=10,gaps=0)
assert base['evidence_summary']['document_snapshot_hits']==9
assert examples['summary']==dict(total=2,hits=0,gaps=2)
b4010=next(row for row in examples['results'] if row['identifier']=='B4010')
assert any(row['source_id']=='accton-b4010-compliance' and row['reason']=='expected vendor not evidenced on this record' for row in b4010['leads'])
assert not db.execute("SELECT 1 FROM affiliations WHERE target='ByteDance'").fetchone()
assert not db.execute("SELECT 1 FROM affiliations WHERE target='Inspur' AND company LIKE '%LG%'").fetchone()
inventories={s:vendor_inventory.run(db,scope=s,limit=-1) for s in ('manufacturer','factory','all')}
for target in ('Accton','Edge-Core','Celestica','Oracle','Amazon Data Services'):
    oldcount=next(row['identities'] for row in json.loads((OUT/'odm-model-review-2026-10-04.json').read_text())['inventories']['manufacturer']['counts_by_target'] if row['target']==target)
    newcount=next(row['identities'] for row in inventories['manufacturer']['counts_by_target'] if row['target']==target)
    assert oldcount==newcount,(target,oldcount,newcount)
counts={key:db.execute('SELECT count(*) FROM '+('fcc_grantees' if key=='grantees' else key)).fetchone()[0] for key in manifest['expected'] if key!='sources'}
counts['sources']=dict(db.execute('SELECT id,records_seen FROM sources'))
report=dict(previous_sha256=before,database_sha256=file_sha256(candidate),preserved_tables=audit,
    affiliation_remappings=remapped,display_metadata_changes=display_changes,
    owned_baseline=base['summary'],owned_evidence=base['evidence_summary'],
    vendor_examples=examples['summary'],expected_counts=counts,
    inventories={s:{k:r[k] for k in ('identity_target_pairs','unique_identities','counts_by_target')} for s,r in inventories.items()})
(OUT/'bytedance-inspur-review-2026-10-04.json').write_text(json.dumps(report,indent=2)+'\n')
(OUT/'vendor-example-validation-candidate-2026-10-04.json').write_text(json.dumps(examples,indent=2)+'\n')
db.close();assert file_sha256(live)==before
backup=DATA/'import-manifest-before-bytedance-inspur-2026-10-04.json';assert not backup.exists()
backup.write_bytes((DATA/'import-manifest.json').read_bytes())
jobs=[job for job in manifest['imports'] if job['source']=='reference-documents'];assert len(jobs)==1
jobs[0]['path']=str(bundle.relative_to(DATA))
manifest['imports'].append(dict(source='curated',path='bytedance-inspur-facts-2026-10-04.json'))
inputs=list((DATA/'raw/bytedance-inspur-2026-10-04').iterdir())+[bundle,DATA/'bytedance-inspur-facts-2026-10-04.json']
for stem in ('accton-b4010-compliance','inspur-i24-whitepaper'):
    inputs+=list((DATA/'raw').glob(stem+'-2026-10-04.*'))
for path in inputs:
    assert path.is_file();manifest['files'][str(path.relative_to(DATA))]=file_sha256(path)
manifest['expected']=counts
(DATA/'import-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps(dict(counts={k:v for k,v in counts.items() if k in ('devices','records','documents','document_pages')},
    sources=len(counts['sources']),jobs=len(manifest['imports']),inputs=len(manifest['files']),
    owned=base['summary'],examples=examples['summary'],
    new_targets={s:[r for r in inv['counts_by_target'] if r['target'] in ('Inspur','ByteDance')] for s,inv in inventories.items()}),indent=2))
