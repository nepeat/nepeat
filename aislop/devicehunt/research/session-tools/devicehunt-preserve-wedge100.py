import sqlite3,json
from pathlib import Path
p=Path.cwd();db=sqlite3.connect('file:data/devices.sqlite?mode=ro',uri=True);db.row_factory=sqlite3.Row
db.execute('ATTACH DATABASE ? AS previous',((p/'data/rebuilt-with-storage-manufacturer.sqlite').as_uri()+'?mode=ro',))
tables='japan_equipment public_equipment_rows ghana_equipment nigeria_equipment ised_equipment energystar_datacenter energystar_server_archives energystar_servers sources devices records affiliations identifiers facts hyperscaler_evidence fcc_applications fcc_exhibits fcc_grantees equipment_register bahrain_equipment anatel_equipment ntra_equipment tec_equipment ieee_assignments pci_entries bsmi_certificates bsmi_product_certificates bsmi_voluntary_certificates bsmi_declarations rra_equipment ncc_equipment fcc_id_index documents document_pages fcc_detail_pages fcc_detail_links fcc_text_excerpts search_index document_search'.split()
result={}
for t in tables:
 cols=[r['name'] for r in db.execute('PRAGMA main.table_info('+t+')') if r['name'] not in ('observed_at','fetched_at','cache_path')]
 if t=='document_search':cols.insert(0,'rowid')
 fields=','.join('"'+s+'"' for s in cols);a=f'SELECT {fields} FROM previous.{t}';b=f'SELECT {fields} FROM main.{t}'
 missing=db.execute('SELECT count(*) FROM ('+a+' EXCEPT '+b+')').fetchone()[0]
 added=db.execute('SELECT count(*) FROM ('+b+' EXCEPT '+a+')').fetchone()[0]
 assert missing==0,(t,missing)
 expected={'sources':1,'devices':2,'records':2,'affiliations':4,'facts':12,'hyperscaler_evidence':2,'search_index':2}.get(t,0);assert added==expected,(t,added,expected)
 result[t]=dict(prior_rows_missing=missing,rows_added=added)
result['search_index_note']='Two new search documents; all prior device IDs and text preserved. FTS rowids can shift on reindex.'
Path('data/exports/meta-wedge100-preservation.json').write_text(json.dumps(result,indent=2)+'\n');print('All previous source evidence preserved; two search documents added.')
