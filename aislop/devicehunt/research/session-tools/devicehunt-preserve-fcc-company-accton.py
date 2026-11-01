import json,sqlite3
from pathlib import Path
from devicehunt import validation
root=Path.cwd();other=root/'data/rebuilt-with-cubscout-components.sqlite'
db=sqlite3.connect('file:data/devices.sqlite?mode=ro',uri=True);db.row_factory=sqlite3.Row
db.execute('ATTACH DATABASE ? AS rebuilt',(other.as_uri()+'?mode=ro',))
tables='japan_equipment public_equipment_rows ghana_equipment nigeria_equipment ised_equipment energystar_datacenter energystar_server_archives energystar_servers sources devices records affiliations identifiers facts hyperscaler_evidence fcc_applications fcc_exhibits fcc_grantees equipment_register bahrain_equipment anatel_equipment ntra_equipment tec_equipment ieee_assignments pci_entries bsmi_certificates bsmi_product_certificates bsmi_voluntary_certificates bsmi_declarations rra_equipment ncc_equipment fcc_id_index documents document_pages fcc_detail_pages fcc_detail_links fcc_text_excerpts search_index document_search'.split()
result={}
for table in tables:
 cols=[r['name'] for r in db.execute('PRAGMA main.table_info('+table+')') if r['name'] not in ('observed_at','fetched_at','cache_path')]
 if table in ('search_index','document_search'):cols.insert(0,'rowid')
 fields=','.join('"'+name+'"' for name in cols)
 left=f'SELECT {fields} FROM main.{table}';right=f'SELECT {fields} FROM rebuilt.{table}'
 if table=='sources':left+=" WHERE id <> 'fcc-company-accton'"
 if table=='fcc_id_index':left+=" WHERE source_id <> 'fcc-company-accton'"
 info={'original':db.execute('SELECT count(*) FROM ('+left+')').fetchone()[0], 'rebuilt':db.execute(f'SELECT count(*) FROM rebuilt.{table}').fetchone()[0], 'only_original':db.execute('SELECT count(*) FROM ('+left+' EXCEPT '+right+')').fetchone()[0], 'only_rebuilt':db.execute('SELECT count(*) FROM ('+right+' EXCEPT '+left+')').fetchone()[0]}
 assert info['original']==info['rebuilt'] and info['only_original']==info['only_rebuilt']==0,(table,info)
 result[table]=info

assert db.execute('SELECT count(*) FROM fcc_company_rows').fetchone()[0]==235
Path('data/exports/fcc-company-accton-preservation-audit.json').write_text(json.dumps(result,indent=2)+'\n')
print('All 39 prior tables/indexes preserved; new source rows are isolated.')
