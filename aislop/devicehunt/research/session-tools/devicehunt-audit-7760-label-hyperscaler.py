import json
from pathlib import Path
from devicehunt.catalog import Catalog, TARGETS
from devicehunt import hyperscaler_inventory as h, validation
from devicehunt.__main__ import write_rows
from devicehunt.rebuild import file_sha256

root=Path.cwd(); path=root/'data/devices.sqlite'; out=root/'data/exports'
before=file_sha256(path);c=Catalog(path,read_only=True)
results={
 'hyperscaler-inventory':h.run(c.db,limit=-1),
 'hyperscaler-inventory-all':h.run(c.db,include_unverified=True,limit=-1),
 'hyperscaler-deployed-manufacturer':h.run(c.db,role='manufacturer',relationship='documented_platform_deployment',limit=-1),
}
for target in TARGETS:
 results['hyperscaler-inventory-'+target.lower().replace(' ','-')]=h.run(c.db,vendor=target,limit=-1)
assert results['hyperscaler-inventory']['assertions']==19
assert results['hyperscaler-inventory-all']['assertions']==21
assert {r['model'] for r in results['hyperscaler-deployed-manufacturer']['rows']}=={'Wedge 100','Wedge 400','Wedge 400C'}
raw=list(c.db.execute('SELECT record_id,operator,relationship,detail FROM hyperscaler_evidence'))
assert {(r['record_id'],r['operator'],r['relationship'],r['detail']) for r in results['hyperscaler-inventory-all']['rows']}=={tuple(r) for r in raw}
assert not any(r['model']=='Minipack2' for r in results['hyperscaler-inventory']['rows'])
for result in results.values():
 for r in result['rows']:
  actual=[dict(a) for a in c.db.execute('SELECT company,target,role FROM affiliations WHERE record_id=? ORDER BY company,role',(r['record_id'],))]
  assert r['affiliations']==actual
  if result['vendor']:
   assert r['matched_affiliations'] and all(a['target']==result['vendor'] for a in r['matched_affiliations'])
for name,result in results.items():
 (out/(name+'.json')).write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
 with (out/(name+'.csv')).open('w',newline='') as stream:write_rows(h.flat_rows(result),'csv',stream)

# Readable saved shortlist retains each relationship and source, including dates
# and all printed company roles. No ranking or new operator aliases are inferred.
lines=['# Hyperscaler hardware evidence shortlist','',
 'Collected source assertions, not a complete inventory or proof of an auction unit’s previous owner.',
 'Company roles belong to the same source record as each relationship. Historical dates are retained.', '',
 '| Model / namespace | Operator | Relationship | Company roles | Source date | Source |',
 '|---|---|---|---|---|---|']
labels={'documented_brand':'Brand documented','documented_fabric_role':'Reported fabric role','documented_platform_deployment':'Deployment reported','documented_design_lineage':'Design lineage','announced_platform_deployment':'Deployment announced','documented_product_family':'Product-family link','documented_product':'Documented product'}
def escape(value):return str(value).replace('|','\\|').replace('\n',' ')
for r in results['hyperscaler-inventory']['rows']:
 roles='; '.join(a['company']+' ('+a['role']+')' for a in r['affiliations'])
 dates='; '.join(d['value'] for d in r['source_dates'] if d['field']=='published_date') or 'not recorded'
 lines.append('| '+' | '.join(map(escape,[r['model']+' / '+r['brand'],r['operator'],labels.get(r['relationship'],r['relationship']),roles,dates]))+' | ['+r['source_id']+']('+r['source_url']+') |')
lines+=['','Two additional uncached claims are available in hyperscaler-inventory-all.json with their eligibility status.',
        'No marketplace availability, physical configuration, software entitlement or factory location is inferred.','']
(out/'hyperscaler-shortlist.md').write_text('\n'.join(lines))
baseline=validation.run(c.db,root/'validation/owned-hardware.json')
assert baseline['summary']==dict(total=10,hits=10,gaps=0)
assert baseline['search_summary']['queries_with_results']==10
c.db.close();assert file_sha256(path)==before
audit=dict(database_sha256=before,database_unchanged=True,counts={k:{key:r[key] for key in ['assertions','unique_identities','counts_by_relationship']} for k,r in results.items()},all_relationship_records_preserved=True,owned_baseline=baseline['summary'])
(out/'7760-label-hyperscaler-audit.json').write_text(json.dumps(audit,indent=2)+'\n')
print(json.dumps(audit,indent=2))
