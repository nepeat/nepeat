import json,hashlib
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt.listings import Matcher, flat_rows, SPECIFICATION_SCOPE
from devicehunt.specifications import CATEGORIES
from devicehunt import validation
from devicehunt.__main__ import write_rows
root=Path.cwd();path=root/'data/devices.sqlite';out=root/'data/exports';before=hashlib.sha256(path.read_bytes()).hexdigest()
c=Catalog(path,read_only=True);db=c.db
matcher=Matcher(db,vendor='Celestica',include_specifications=True)
checks=[]; examples=[]
for item in json.loads((out/'celestica-chassis-skus.json').read_text()):
 sku=item['chassis_sku'];result=matcher.match('Celestica '+sku+' empty chassis',limit=-1)
 evidence=[m for r in result['candidates'] for m in r['matches'] if m['source_id']=='celestica-ds4000-chassis-skus']
 exact=[m for m in evidence if m['match_type']=='identifier_mention']
 assert len(exact)==1,(sku,exact)
 assert exact[0]['identifier']==sku and exact[0]['record_part_numbers']==[sku]
 assert len(exact[0]['specifications'])==2
 assert all(item['configuration'] in s['statement'] for s in exact[0]['specifications'])
 assert all(not m['specifications'] for m in evidence if m['match_type']=='variant_lead')
 unknown=matcher.match('Celestica '+sku+'-UNPROVEN',limit=-1)
 assert all(not m['specifications'] for r in unknown['candidates'] for m in r['matches'])
 for row in result['candidates']:
  for m in row['matches']:
   if m['match_type']=='variant_lead':assert m['specifications']==[];continue
   expected=[{'category':f[0],'statement':f[1]} for f in db.execute('SELECT name,value FROM facts WHERE record_id=? AND name IN ('+','.join('?' for _ in CATEGORIES)+') ORDER BY name,value',[m['record_id'],*CATEGORIES])]
   assert m['specifications']==expected
 checks.append(dict(part_number=sku,exact_record=exact[0]['record_id'],statements=2,unknown_suffix_no_specifications=True))
 examples.append(result)
family=matcher.match('Celestica DS4000 chassis',limit=-1)
family_rows=[m for r in family['candidates'] for m in r['matches'] if m['source_id']=='celestica-ds4000-chassis-skus' and m['match_type']=='model_mention']
assert len(family_rows)==6 and len({m['record_part_numbers'][0] for m in family_rows})==6
for model in ['SC6100','SD4100','SD6200']:
 result=matcher.match('Celestica '+model+' empty chassis',limit=-1)
 evidence=[m for r in result['candidates'] for m in r['matches'] if m['source_id']=='celestica-storage-manufacturer-'+model.lower()]
 assert len(evidence)==1 and len(evidence[0]['specifications'])==1
 examples.append(result)
base=validation.run(db,root/'validation/owned-hardware.json');assert base['summary']=={'total':10,'hits':9,'gaps':1}
(out/'listing-specifications-examples.json').write_text(json.dumps({'input_scope':'Illustrative auction titles, not live listings or offers.','specification_scope':SPECIFICATION_SCOPE,'results':examples},ensure_ascii=False,indent=2)+'\n')
with (out/'listing-specifications-examples.csv').open('w',newline='') as stream:
 write_rows([row for n,result in enumerate(examples,1) for row in flat_rows(result,n)],'csv',stream)
c.db.close();assert before==hashlib.sha256(path.read_bytes()).hexdigest()
audit=dict(database_sha256=before,read_only=True,ordering_code_checks=checks,family_configurations_preserved=6,manufacturer_page_examples=3,baseline=base['summary'],search_baseline=base['search_summary'])
(out/'listing-specifications-audit.json').write_text(json.dumps(audit,indent=2)+'\n');print(json.dumps(audit,indent=2))
