"""Stage reviewed hardware batch; require old-data preservation before manifest update."""
import json
import subprocess
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt.rebuild import file_sha256, read_manifest
from devicehunt import validation, vendor_inventory, specifications
ROOT=Path(__file__).resolve().parents[2]
DATA=ROOT/'data'; OUT=DATA/'exports'; LIVE=DATA/'devices.sqlite'
BEFORE='79a824face1fea6949b930d81a1500a8bebae4a1fe734e84b8c892ad8684994f'
assert file_sha256(LIVE)==BEFORE
_,manifest=read_manifest(DATA/'import-manifest.json')
candidate=DATA/'inspur-2018-inventec-candidate-2026-10-04-v2.sqlite'
bundle=DATA/'raw/reference-documents-with-inspur-2018-inventec-2026-10-04-v2.json'
facts=DATA/'inspur-2018-inventec-facts-2026-10-04.json'
assert not candidate.exists()
subprocess.run(['/bin/cp','-cp',str(LIVE),str(candidate)],check=True)
c=Catalog(candidate)
for source,path in [('reference-documents',bundle),('curated',facts)]:
    print(source,import_source(c,source,path),flush=True)
c.db.close()
c=Catalog(candidate,read_only=True); db=c.db
db.execute('ATTACH DATABASE ? AS old',(LIVE.as_uri()+'?mode=ro',))
tables=json.loads((OUT/'inspur-rail-rebuild-2026-10-04-comparison.json').read_text())
audit={}
for table in tables:
    if table in ('sources','search_index','document_search'): continue
    cols=[r['name'] for r in db.execute('PRAGMA main.table_info('+table+')') if r['name'] not in ('observed_at','fetched_at','cache_path')]
    fields=','.join('"'+n+'"' for n in cols)
    missing=db.execute(f'SELECT count(*) FROM (SELECT {fields} FROM old.{table} EXCEPT SELECT {fields} FROM main.{table})').fetchone()[0]
    assert missing==0,(table,missing)
    audit[table]=dict(previous=db.execute('SELECT count(*) FROM old.'+table).fetchone()[0],current=db.execute('SELECT count(*) FROM main.'+table).fetchone()[0],missing=missing)
for old in db.execute('SELECT * FROM old.sources'):
    new=db.execute('SELECT * FROM main.sources WHERE id=?',(old['id'],)).fetchone()
    assert new is not None
    for column in old.keys():
        if column in ('fetched_at','cache_path'): continue
        if column=='sha256' and old['id'].startswith('reference-pdf-'): assert new[column]==file_sha256(bundle)
        else: assert new[column]==old[column],(old['id'],column)
assert db.execute('PRAGMA user_version').fetchone()[0]==28
assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
assert not db.execute('PRAGMA foreign_key_check').fetchall()
base=validation.run(db,ROOT/'validation/owned-hardware.json')
examples=validation.run(db,ROOT/'validation/vendor-examples.json')
assert base['summary']==dict(total=10,hits=10,gaps=0)
assert base['evidence_summary']==dict(document_snapshot_hits=9,label_photo_only_hits=1,unresolved_identity_gaps=0)
assert examples['summary']==dict(total=2,hits=0,gaps=2)
inventories={scope:vendor_inventory.run(db,scope=scope,limit=-1) for scope in ('manufacturer','factory','all')}
previous=json.loads((OUT/'latest-export-audit.json').read_text())['inventories']
for scope in ('manufacturer','factory'):
    for key in ('identity_target_pairs','unique_identities','counts_by_target'):
        assert inventories[scope][key]==previous[scope][key],(scope,key)
assert db.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0]==22
rows=json.loads(facts.read_text())['devices']
assert len(rows)==93
new_sources=[s['id'] for s in json.loads(facts.read_text())['sources']]
added_ids=[dict(r) for r in db.execute('SELECT i.kind,i.value,d.model FROM identifiers i JOIN records r ON r.id=i.record_id JOIN devices d ON d.id=r.device_id WHERE r.source_id IN (?,?,?)',new_sources)]
assert added_ids==[dict(kind='alias',value='AGX-2',model='NF5288M5')],added_ids
for a,b in [('i48','NS5482JD'),('I8020','NX5440M5'),('i24','NS5162M5'),('1010S','1010S-M5'),('NF5280M5','NF5280M5-V')]:
    identities=db.execute("SELECT id FROM devices WHERE brand='Inspur' AND model IN (?,?)",(a,b)).fetchall()
    assert len(identities)==2 and identities[0]['id']!=identities[1]['id'],(a,b)
# Scope negative checks: full-code expansions and software never become assertions from this source.
assert not db.execute("SELECT 1 FROM records r JOIN devices d ON d.id=r.device_id WHERE r.source_id=? AND d.model IN ('FS6600','AS13000-M3','ClusterEngine','DPS-M1','BCP')",(new_sources[0],)).fetchall()
checks={}
for model,term in [('OB5162M5','Open19 power shelf'),('ON5263M5','bus-bar'),('SN3116J4','12V'),('FS5900','85-264V'),('AGX-2','3000W'),('1020E','200-240V'),('Artemis II Rack','L2L')]:
    result=specifications.search(c,model,target='Inventec' if model=='Artemis II Rack' else 'Inspur',exact_model=True,limit=-1)
    assert term in json.dumps(result),model
    checks[model]=result
counts={key:db.execute('SELECT count(*) FROM '+('fcc_grantees' if key=='grantees' else key)).fetchone()[0] for key in manifest['expected'] if key!='sources'}
counts['sources']=dict(db.execute('SELECT id,records_seen FROM sources'))
assert (counts['records'],len(counts['sources']),counts['documents'],counts['document_pages'])==(5273,263,50,3641)
report=dict(previous_sha256=BEFORE,schema=28,baseline=base['summary'],evidence_summary=base['evidence_summary'],
    vendor_examples=examples['summary'],preserved_tables=audit,expected_counts=counts,
    added_identities=counts['devices']-3976,added_records=93,added_original_pdfs=3,added_pdf_pages=54,
    source_scoped_aliases=added_ids,unchanged_manufacturer_counts=True,exact_spec_queries=checks,
    inventories={scope:{key:value[key] for key in ('identity_target_pairs','unique_identities','counts_by_target')} for scope,value in inventories.items()})
db.close()
report['database_sha256']=file_sha256(candidate)
assert file_sha256(LIVE)==BEFORE
(OUT/'inspur-2018-inventec-review-2026-10-04.json').write_text(json.dumps(report,indent=2,ensure_ascii=False)+'\n')
backup=DATA/'import-manifest-before-inspur-2018-inventec-2026-10-04.json'
with backup.open('xb') as stream: stream.write((DATA/'import-manifest.json').read_bytes())
jobs=[j for j in manifest['imports'] if j['source']=='reference-documents']
assert len(jobs)==1
jobs[0]['path']=str(bundle.relative_to(DATA))
manifest['imports'].append(dict(source='curated',path=facts.name))
archive_files = [RAW for stem in ('inspur-products-2018','inventec-artemis-ii-rack','inventec-x680m') for RAW in (DATA/'raw').glob(stem+'-2026-10-04.*')]
for path in [*list((DATA/'raw/inspur-2018-inventec-2026-10-04').iterdir()),*archive_files,bundle,facts]:
    assert path.is_file()
    manifest['files'][str(path.relative_to(DATA))]=file_sha256(path)
manifest['expected']=counts
(DATA/'import-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps(dict(devices=counts['devices'],records=counts['records'],sources=len(counts['sources']),documents=counts['documents'],pages=counts['document_pages'],jobs=len(manifest['imports']),inputs=len(manifest['files']),added_identities=report['added_identities'],inventories=report['inventories']),indent=2))
