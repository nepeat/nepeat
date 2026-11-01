import json,hashlib
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt import validation,lookup,vendor_inventory,specifications,listings
from devicehunt.__main__ import report,write_rows
root=Path.cwd();path=root/'data/devices.sqlite';out=root/'data/exports'
before=hashlib.sha256(path.read_bytes()).hexdigest();c=Catalog(path,read_only=True);db=c.db
base=validation.run(db,root/'validation/owned-hardware.json')
(out/'inventory-validation.json').write_text(json.dumps(base,ensure_ascii=False,indent=2)+'\n')
counts={}
for scope in ['manufacturer','factory','all']:
 result=vendor_inventory.run(db,scope=scope,limit=-1)
 (out/('vendor-inventory-'+scope+'.json')).write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
 with (out/('vendor-inventory-'+scope+'.csv')).open('w',newline='') as stream:write_rows(vendor_inventory.flat_rows(result),'csv',stream)
 counts[scope]={k:result[k] for k in ['identity_target_pairs','unique_identities','counts_by_target']}
rows=[dict(r) for r in db.execute('SELECT * FROM target_devices ORDER BY target,brand,model')]
for fmt in ['csv','json']:
 with (out/('targets.'+fmt)).open('w',newline='') as stream:write_rows(rows,fmt,stream)
report(c,out/'catalog.html')
matcher=listings.Matcher(db,vendor='Celestica');examples=[]
for model in ['SC6100','SD4100','SD6200']:
 sid='celestica-storage-manufacturer-'+model.lower()
 statements=specifications.search(c,model,target='Celestica',exact_model=True,limit=-1)
 assert len([r for r in statements if r['source_id']==sid])==1
 assert not any(r['source_id']==sid for r in specifications.search(c,model+'-UNPROVEN',target='Celestica',exact_model=True,limit=-1))
 match=matcher.match('Celestica '+model+' empty chassis')
 assert any(e['source_id']==sid and e['match_type']=='model_mention' for row in match['candidates'] for e in row['matches'])
 variant=matcher.match('Celestica '+model+'-UNPROVEN')
 assert any(e['source_id']==sid and e['match_type']=='variant_lead' for row in variant['candidates'] for e in row['matches'])
 assert not any(e['source_id']==sid and e['match_type']!='variant_lead' for row in variant['candidates'] for e in row['matches'])
 examples.append(dict(model=model,specifications=statements,listing=match,unproven_suffix=variant))
 (out/('lookup-storage-'+model.lower()+'.json')).write_text(json.dumps(lookup.run(db,model),ensure_ascii=False,indent=2)+'\n')
(out/'celestica-storage-manufacturer-search-audit.json').write_text(json.dumps(examples,ensure_ascii=False,indent=2)+'\n')
refreshed=0
for p in out.glob('lookup-*.json'):
 previous=json.loads(p.read_text())
 if 'query' in previous and 'limit_per_dataset' in previous:
  p.write_text(json.dumps(lookup.run(db,previous['query'],previous['limit_per_dataset']),ensure_ascii=False,indent=2)+'\n');refreshed+=1
c.db.close();assert before==hashlib.sha256(path.read_bytes()).hexdigest()
audit=dict(database_sha256=before,read_only_exports_unchanged_database=True,owned_baseline=base['summary'],inventories=counts,lookup_reports_refreshed=refreshed)
(out/'storage-manufacturer-export-audit.json').write_text(json.dumps(audit,indent=2)+'\n');print(json.dumps(audit,indent=2))
