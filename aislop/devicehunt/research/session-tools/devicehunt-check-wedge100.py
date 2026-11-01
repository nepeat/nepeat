import json
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt import vendor_inventory, validation, specifications, listings
from devicehunt.rebuild import file_sha256

path=Path('data/devices.sqlite'); before=file_sha256(path)
c=Catalog(path,read_only=True); sid='meta-wedge100-2016'
rows=[dict(r) for r in c.db.execute('SELECT d.id,d.brand,d.model,r.id AS record_id,h.operator,h.relationship FROM records r JOIN devices d ON d.id=r.device_id JOIN hyperscaler_evidence h ON h.record_id=r.id WHERE r.source_id=? ORDER BY d.model',(sid,))]
assert len(rows)==2
assert {(r['brand'],r['model'],r['relationship']) for r in rows}=={
 ('Accton','Wedge 100','documented_platform_deployment'),
 ('Edge-Core','Wedge 100-32X','documented_design_lineage')}
assert c.db.execute('SELECT count(*) FROM identifiers i JOIN records r ON r.id=i.record_id WHERE r.source_id=?',(sid,)).fetchone()[0]==0
assert c.db.execute("SELECT count(*) FROM affiliations a JOIN records r ON r.id=a.record_id WHERE r.source_id=? AND a.target='Accton' AND a.role='manufacturer'",(sid,)).fetchone()[0]==2
assert c.db.execute("SELECT count(*) FROM affiliations a JOIN records r ON r.id=a.record_id WHERE r.source_id=? AND a.target='Edge-Core' AND a.role='manufacturer'",(sid,)).fetchone()[0]==0
assert c.db.execute("SELECT count(*) FROM devices WHERE brand='Edge-Core' AND lower(model) IN ('wedge100-32x','wedge 100-32x')").fetchone()[0]==2
matcher=listings.Matcher(c.db,include_specifications=True);examples=[]
for row in rows:
 model=row['model'];did=row['id']
 assert did in {d['id'] for d in c.search(model,target='Accton',hyperscaler=True,role='manufacturer',limit=-1)}
 spec=[r for r in specifications.search(c,model,exact_model=True,limit=-1) if r['source_id']==sid]
 expected={'physical_description','power_description'} if model=='Wedge 100' else {'software_installation_description'}
 assert {s['category'] for s in spec}==expected
 match=matcher.match(model+' empty chassis')
 scoped=[e for candidate in match['candidates'] for e in candidate['matches'] if e['source_id']==sid and e['record_id']==row['record_id']]
 assert scoped and all(e['match_type']=='model_mention' for e in scoped)
 assert {s['category'] for e in scoped for s in e['specifications']}==expected
 examples.append(dict(model=model,specifications=spec,listing=match))
 # Unknown suffixes remain leads and must never inherit specification statements.
 variant=matcher.match(model+'-UNPROVEN')
 scoped=[e for r in variant['candidates'] for e in r['matches'] if e['source_id']==sid]
 assert scoped and all(e['match_type']=='variant_lead' and not e['specifications'] for e in scoped)
 examples[-1]['unproven_suffix']=variant
baseline=validation.run(c.db,Path('validation/owned-hardware.json'))
assert baseline['summary']=={'total':10,'hits':9,'gaps':1}
assert baseline['search_summary']['queries_with_results']==10
c.db.close();assert file_sha256(path)==before
Path('data/exports/meta-wedge100-search-audit.json').write_text(json.dumps(dict(database_sha256=before,read_only=True,relationships=rows,examples=examples,owned_baseline=baseline['summary']),indent=2)+'\n')
print('Wedge models, roles, deployment scope, specifications and suffix isolation pass; baseline remains 9/10.')
