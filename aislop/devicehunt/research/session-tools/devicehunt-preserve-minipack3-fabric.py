import sqlite3,json
from pathlib import Path
root=Path.cwd();db=sqlite3.connect('file:data/devices.sqlite?mode=ro',uri=True);db.row_factory=sqlite3.Row
old=root/'data/rebuilt-with-accton-joy.sqlite';db.execute('ATTACH DATABASE ? AS previous',(old.as_uri()+'?mode=ro',))
tables='japan_equipment public_equipment_rows ghana_equipment nigeria_equipment ised_equipment energystar_datacenter energystar_server_archives energystar_servers sources devices records affiliations identifiers facts hyperscaler_evidence fcc_applications fcc_exhibits fcc_grantees equipment_register bahrain_equipment anatel_equipment ntra_equipment tec_equipment ieee_assignments pci_entries bsmi_certificates bsmi_product_certificates bsmi_voluntary_certificates bsmi_declarations rra_equipment ncc_equipment fcc_id_index documents document_pages fcc_detail_pages fcc_detail_links fcc_text_excerpts search_index document_search'.split()
expected={'sources':(1,1),'records':(0,1),'affiliations':(0,1),'facts':(0,7),'hyperscaler_evidence':(0,1),'search_index':(1,1)}
result={}
for t in tables:
 cols=[r['name'] for r in db.execute('PRAGMA main.table_info('+t+')') if r['name'] not in ('observed_at','fetched_at','cache_path')]
 fields=','.join('"'+name+'"' for name in cols)
 before=f'SELECT {fields} FROM previous.{t}';after=f'SELECT {fields} FROM main.{t}'
 missing=db.execute('SELECT count(*) FROM ('+before+' EXCEPT '+after+')').fetchone()[0]
 added=db.execute('SELECT count(*) FROM ('+after+' EXCEPT '+before+')').fetchone()[0]
 assert (missing,added)==expected.get(t,(0,0)),(t,missing,added)
 result[t]=dict(previous_rows_missing_or_changed=missing,added_or_changed_rows=added)
changed=[dict(r) for r in db.execute('SELECT s.id,p.records_seen AS before,s.records_seen AS after FROM sources s JOIN previous.sources p ON p.id=s.id WHERE s.records_seen!=p.records_seen')]
assert changed==[dict(id='meta-ocp-2025',before=1,after=2)],changed
# The one rewritten search document must be the existing Celestica Minipack3.
ids=[r[0] for r in db.execute('SELECT device_id FROM previous.search_index EXCEPT SELECT device_id FROM main.search_index')]
assert not ids
old_text=db.execute('SELECT device_id,text FROM previous.search_index EXCEPT SELECT device_id,text FROM main.search_index').fetchall()
assert len(old_text)==1
model=db.execute('SELECT brand,model FROM devices WHERE id=?',(old_text[0][0],)).fetchone()
assert tuple(model)==('Celestica','Minipack3'),tuple(model)
result['source_metadata_change']=changed
result['search_index_note']='Only the existing Celestica Minipack3 document changes. All prior catalog assertions and raw source rows are preserved.'
(root/'data/exports/minipack3-fabric-role-preservation.json').write_text(json.dumps(result,indent=2)+'\n')
print('All 39 tables/indexes checked; one new fabric-role record and existing Minipack3 search document updated; all prior assertions preserved.')
