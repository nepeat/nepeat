import sqlite3,json
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt import hyperscaler_inventory as h, vendor_inventory as v

root=Path.cwd();c=Catalog(root/'data/devices.sqlite',read_only=True);db=c.db
db.execute('ATTACH DATABASE ? AS previous',((root/'data/rebuilt-with-minipack2-reference.sqlite').as_uri()+'?mode=ro',))
tables='japan_equipment public_equipment_rows ghana_equipment nigeria_equipment ised_equipment energystar_datacenter energystar_server_archives energystar_servers sources devices records affiliations identifiers facts hyperscaler_evidence fcc_applications fcc_exhibits fcc_grantees equipment_register bahrain_equipment anatel_equipment ntra_equipment tec_equipment ieee_assignments pci_entries bsmi_certificates bsmi_product_certificates bsmi_voluntary_certificates bsmi_declarations rra_equipment ncc_equipment fcc_id_index documents document_pages fcc_detail_pages fcc_detail_links fcc_text_excerpts search_index document_search'.split()
result={}
for t in tables:
 ignore={'observed_at','fetched_at','cache_path'} | ({'sha256','coverage'} if t=='sources' else set())
 cols=[r['name'] for r in db.execute('PRAGMA main.table_info('+t+')') if r['name'] not in ignore]
 if t in ('search_index','document_search'):cols.insert(0,'rowid')
 fields=','.join('"'+s+'"' for s in cols);a=f'SELECT {fields} FROM previous.{t}';b=f'SELECT {fields} FROM main.{t}'
 missing=db.execute('SELECT count(*) FROM ('+a+' EXCEPT '+b+')').fetchone()[0]
 added=db.execute('SELECT count(*) FROM ('+b+' EXCEPT '+a+')').fetchone()[0]
 assert missing==(5 if t=='search_index' else 0),(t,missing)
 assert added==(5 if t in ('search_index','hyperscaler_evidence') else 0),(t,added)
 result[t]=dict(prior_rows_missing=missing,rows_added=added)
changes=[dict(r) for r in db.execute('SELECT s.id,p.sha256 AS old_sha256,s.sha256 AS new_sha256,p.coverage AS old_coverage,s.coverage AS new_coverage FROM sources s JOIN previous.sources p ON p.id=s.id WHERE s.sha256 IS NOT p.sha256 OR s.coverage<>p.coverage')]
assert {r['id'] for r in changes}=={'monitron-model','aws-monitron-sensor','aws-monitron-ex-sensor','aws-monitron-wifi-eu','aws-monitron-ethernet-eu'}
assert [r['id'] for r in changes if r['old_sha256']!=r['new_sha256']]==['monitron-model']
result['source_metadata_updates']=changes
result['search_index_note']='Only the five existing Monitron identity documents gain AWS product-relationship text. All other prior data and device IDs remain unchanged.'
models={'TE1A001','TE1A105','TE1A195','TG1A001','TG1A002'}
all_ads=h.run(db,vendor='Amazon Data Services',limit=-1)
assert {r['model'] for r in all_ads['rows']}==models and all_ads['assertions']==5
assert all(r['relationship']=='documented_product' for r in all_ads['rows'])
assert {r['model'] for r in h.run(db,vendor='Amazon Data Services',role='manufacturer')['rows']}=={'TE1A001','TE1A105','TE1A195'}
assert {r['model'] for r in h.run(db,vendor='Amazon Data Services',role='compliance_declarant')['rows']}=={'TG1A001','TG1A002'}
assert h.run(db,vendor='Amazon Data Services',relationship='documented_platform_deployment')['assertions']==0
old_db=sqlite3.connect((root/'data/rebuilt-with-minipack2-reference.sqlite').as_uri()+'?mode=ro',uri=True);old_db.row_factory=sqlite3.Row
for scope in ['manufacturer','factory','all']:
 before=v.run(old_db,scope=scope,limit=-1);after=v.run(db,scope=scope,limit=-1)
 assert {(r['target'],r['device_id']) for r in before['rows']}=={(r['target'],r['device_id']) for r in after['rows']}
old_db.close()
(root/'data/exports/monitron-product-links-preservation.json').write_text(json.dumps(result,indent=2)+'\n')
print('All prior assertions and inventories preserved; only five product links, five search documents and five scoped source metadata rows change.')
