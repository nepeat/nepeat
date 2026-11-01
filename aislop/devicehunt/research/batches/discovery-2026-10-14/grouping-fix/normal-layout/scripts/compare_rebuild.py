import json,sqlite3
from pathlib import Path
from devicehunt import validation
import argparse
parser=argparse.ArgumentParser(description='Compare current catalog with a clean offline rebuild.')
parser.add_argument('rebuilt', nargs='?', default='data/rebuilt-with-inspur-2018-inventec-2026-10-04.sqlite')
parser.add_argument('--database', default='data/devices.sqlite', help='Reference catalog (read-only)')
parser.add_argument('--output-prefix', default='rebuild', help='Audit filenames under data/exports')
args=parser.parse_args()
root=Path.cwd();other=(root/args.rebuilt).resolve()
if not args.output_prefix or any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_' for c in args.output_prefix):
 parser.error('Output prefix must contain only letters, digits, hyphens or underscores')
db=sqlite3.connect((root/args.database).resolve().as_uri()+'?mode=ro',uri=True);db.row_factory=sqlite3.Row
db.execute('ATTACH DATABASE ? AS rebuilt',(other.as_uri()+'?mode=ro',))
tables='fcc_sitemap_pages fcc_sitemap_urls fcc_company_rows japan_equipment public_equipment_rows ghana_equipment nigeria_equipment ised_equipment energystar_datacenter energystar_server_archives energystar_servers sources devices records affiliations identifiers facts hyperscaler_evidence fcc_applications fcc_exhibits fcc_grantees equipment_register bahrain_equipment anatel_equipment ntra_equipment tec_equipment ieee_assignments pci_entries bsmi_certificates bsmi_product_certificates bsmi_voluntary_certificates bsmi_declarations rra_equipment ncc_equipment fcc_id_index documents document_pages fcc_detail_pages fcc_detail_links fcc_text_excerpts search_index document_search'.split()
result={}
tables.append('inspur_support_entries')
from devicehunt.lineage import TABLES
tables.extend(TABLES)
for table in tables:
 cols=[r['name'] for r in db.execute('PRAGMA main.table_info('+table+')') if r['name'] not in ('observed_at','fetched_at','cache_path')]
 if table in ('search_index','document_search'):cols.insert(0,'rowid')
 fields=','.join('"'+name+'"' for name in cols)
 # Group by all projected values and count each multiplicity: EXCEPT alone
 # would miss swapped duplicate multiplicities with the same total row count.
 left=f'SELECT {fields},count(*) AS multiplicity FROM main.{table} GROUP BY {fields}'
 right=f'SELECT {fields},count(*) AS multiplicity FROM rebuilt.{table} GROUP BY {fields}'
 info={'original':db.execute(f'SELECT count(*) FROM main.{table}').fetchone()[0], 'rebuilt':db.execute(f'SELECT count(*) FROM rebuilt.{table}').fetchone()[0], 'only_original':db.execute('SELECT count(*) FROM ('+left+' EXCEPT '+right+')').fetchone()[0], 'only_rebuilt':db.execute('SELECT count(*) FROM ('+right+' EXCEPT '+left+')').fetchone()[0]}
 assert info['original']==info['rebuilt'] and info['only_original']==info['only_rebuilt']==0,(table,info)
 result[table]=info
Path('data/exports/'+args.output_prefix+'-comparison.json').write_text(json.dumps(result,indent=2)+'\n')
base=validation.run(db,Path('validation/owned-hardware.json'))
with sqlite3.connect(other.as_uri()+'?mode=ro',uri=True) as other_db:
 other_db.row_factory=sqlite3.Row
 rebuilt=validation.run(other_db,Path('validation/owned-hardware.json'))
for key in ('summary','results','search_summary'):assert base[key]==rebuilt[key],key
Path('data/exports/'+args.output_prefix+'-validation.json').write_text(json.dumps({'database':str(other),'reference_database':str((root/args.database).resolve()),'matches_reference_baseline':True,'summary':base['summary']},indent=2)+'\n')
print('All',len(tables),'tables/indexes and baseline results match.')
