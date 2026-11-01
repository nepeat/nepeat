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
 info={'original':db.execute(f'SELECT count(*) FROM main.{table}').fetchone()[0], 'rebuilt':db.execute(f'SELECT count(*) FROM rebuilt.{table}').fetchone()[0], 'only_original':db.execute('SELECT count(*) FROM ('+left+' EXCEPT '+right+')').fetchone()[0], 'only_rebuilt':db.execute('SELECT count(*) FROM ('+right+' EXCEPT '+left+')').fetchone()[0]}
 assert info['original']==info['rebuilt'] and info['only_original']==info['only_rebuilt']==0,(table,info)
 result[table]=info
Path('data/exports/rebuild-comparison.json').write_text(json.dumps(result,indent=2)+'\n')
base=validation.run(db,Path('validation/owned-hardware.json'))
with sqlite3.connect(other.as_uri()+'?mode=ro',uri=True) as other_db:
 other_db.row_factory=sqlite3.Row
 rebuilt=validation.run(other_db,Path('validation/owned-hardware.json'))
for key in ('summary','results','search_summary'):assert base[key]==rebuilt[key],key
Path('data/exports/rebuild-validation.json').write_text(json.dumps({'database':str(other),'matches_live_baseline':True,'summary':base['summary']},indent=2)+'\n')
print('All',len(tables),'tables/indexes and baseline results match.')
