import sqlite3,json
from pathlib import Path
root=Path.cwd();db=sqlite3.connect('file:data/devices.sqlite?mode=ro',uri=True);db.row_factory=sqlite3.Row
old=root/'data/rebuilt-with-minipack3-fabric-role.sqlite';db.execute('ATTACH DATABASE ? AS previous',(old.as_uri()+'?mode=ro',))
tables='japan_equipment public_equipment_rows ghana_equipment nigeria_equipment ised_equipment energystar_datacenter energystar_server_archives energystar_servers sources devices records affiliations identifiers facts hyperscaler_evidence fcc_applications fcc_exhibits fcc_grantees equipment_register bahrain_equipment anatel_equipment ntra_equipment tec_equipment ieee_assignments pci_entries bsmi_certificates bsmi_product_certificates bsmi_voluntary_certificates bsmi_declarations rra_equipment ncc_equipment fcc_id_index documents document_pages fcc_detail_pages fcc_detail_links fcc_text_excerpts search_index document_search'.split()
expected={'sources':(1,1),'records':(1,1),'affiliations':(1,1),'facts':(0,4),'search_index':(1,1)}
result={}
for t in tables:
 cols=[r['name'] for r in db.execute('PRAGMA main.table_info('+t+')') if r['name'] not in ('observed_at','fetched_at','cache_path')]
 fields=','.join('"'+name+'"' for name in cols)
 before=f'SELECT {fields} FROM previous.{t}';after=f'SELECT {fields} FROM main.{t}'
 missing=db.execute('SELECT count(*) FROM ('+before+' EXCEPT '+after+')').fetchone()[0]
 added=db.execute('SELECT count(*) FROM ('+after+' EXCEPT '+before+')').fetchone()[0]
 assert (missing,added)==expected.get(t,(0,0)),(t,missing,added)
 result[t]=dict(previous_rows_missing_or_changed=missing,added_or_changed_rows=added)
changed=[dict(r) for r in db.execute('SELECT s.id,p.sha256 AS before,s.sha256 AS after FROM sources s JOIN previous.sources p ON p.id=s.id WHERE s.sha256 IS NOT p.sha256')]
assert len(changed)==1 and changed[0]['id']=='cubscout-report',changed
old_aff=db.execute('SELECT * FROM previous.affiliations EXCEPT SELECT * FROM main.affiliations').fetchall()
new_aff=db.execute('SELECT * FROM main.affiliations EXCEPT SELECT * FROM previous.affiliations').fetchall()
assert len(old_aff)==len(new_aff)==1
assert old_aff[0]['role']=='fcc_grantee' and new_aff[0]['role']=='regulatory_applicant'
assert old_aff[0]['record_id']==new_aff[0]['record_id'] and new_aff[0]['company']=='Amazon Data Services, Inc.'
# The one rewritten search document must be the existing Celestica Minipack3.
ids=[r[0] for r in db.execute('SELECT device_id FROM previous.search_index EXCEPT SELECT device_id FROM main.search_index')]
assert not ids
old_text=db.execute('SELECT device_id,text FROM previous.search_index EXCEPT SELECT device_id,text FROM main.search_index').fetchall()
assert len(old_text)==1
model=db.execute('SELECT brand,model FROM devices WHERE id=?',(old_text[0][0],)).fetchone()
assert tuple(model)==('Amazon Data Services','CUBSCOUT'),tuple(model)
result['source_metadata_change']=changed
result['search_index_note']='Only the existing ADS CUBSCOUT search document changes. One ADS role is corrected from grantee to applicant on the report record; its raw JSON gains configuration data. All other catalog claims and raw source rows are preserved.'
(root/'data/exports/cubscout-components-preservation.json').write_text(json.dumps(result,indent=2)+'\n')
print('All 39 tables/indexes checked; CubScout report metadata, four added facts, one applicant-role correction and one search document updated; other data preserved.')
