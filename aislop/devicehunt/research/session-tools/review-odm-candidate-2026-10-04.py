import json,sqlite3
from pathlib import Path
from devicehunt.catalog import Catalog,target_for,TARGETS
from devicehunt import vendor_inventory,validation
from devicehunt.rebuild import file_sha256
root=Path('/Users/nep/nocloud/git/nepeat/aislop/fcc_shenanigans');path=root/'data/odm-expanded-candidate-2026-10-04.sqlite';db=sqlite3.connect(path.as_uri()+'?mode=ro',uri=True);db.row_factory=sqlite3.Row
db.execute('ATTACH DATABASE ? AS old',((root/'data/devices.sqlite').as_uri()+'?mode=ro',))
# Original assertions and raw data must survive; new target names can annotate only existing exact companies.
tables='devices records identifiers facts hyperscaler_evidence fcc_applications fcc_exhibits fcc_grantees equipment_register bahrain_equipment anatel_equipment ntra_equipment tec_equipment ieee_assignments pci_entries bsmi_certificates bsmi_product_certificates bsmi_voluntary_certificates bsmi_declarations rra_equipment ncc_equipment fcc_id_index documents document_pages fcc_detail_pages fcc_detail_links fcc_text_excerpts energystar_datacenter energystar_server_archives energystar_servers ised_equipment nigeria_equipment ghana_equipment public_equipment_rows japan_equipment fcc_company_rows fcc_sitemap_pages fcc_sitemap_urls'.split()
audit={}
for table in tables:
 cols=[r['name'] for r in db.execute('PRAGMA main.table_info('+table+')') if r['name'] not in ('observed_at','fetched_at','cache_path')]
 fields=','.join('"'+x+'"' for x in cols)
 missing=db.execute(f'SELECT count(*) FROM (SELECT {fields} FROM old.{table} EXCEPT SELECT {fields} FROM main.{table})').fetchone()[0]
 counts=dict(previous=db.execute('SELECT count(*) FROM old.'+table).fetchone()[0],candidate=db.execute('SELECT count(*) FROM main.'+table).fetchone()[0],previous_rows_missing=missing)
 assert missing==0,(table,counts);audit[table]=counts
oldaff=[dict(r) for r in db.execute('SELECT * FROM old.affiliations')];newaff={(r['record_id'],r['company'],r['role']):dict(r) for r in db.execute('SELECT * FROM main.affiliations')};remapped=[]
for old in oldaff:
 key=(old['record_id'],old['company'],old['role']);assert key in newaff,key;new=newaff[key]
 assert new['target']==(old['target'] or target_for(old['company'])),(old,new)
 if old['target']!=new['target']:remapped.append(dict(previous=old,candidate=new))
raw_sources={r['id']:dict(r) for r in db.execute('SELECT * FROM main.sources')}
for old in db.execute('SELECT * FROM old.sources'):
 new=raw_sources[old['id']]
 for col in old.keys():
  if col in ('fetched_at','cache_path','records_seen') or col=='coverage' and old['id']=='onl':continue
  assert old[col]==new[col],(old['id'],col)
assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
assert not db.execute('PRAGMA foreign_key_check').fetchall()
base=validation.run(db,root/'validation/owned-hardware.json');assert base['summary']==dict(total=10,hits=10,gaps=0)
assert base['evidence_summary']['document_snapshot_hits']==9
inventories={scope:vendor_inventory.run(db,scope=scope,limit=-1) for scope in ('manufacturer','factory','all')}
# Same-record separation: a parent affiliation must never inherit Hyve manufacturer/platform roles.
assert not vendor_inventory.run(db,'TD SYNNEX',scope='manufacturer',limit=-1)['rows']
parent=vendor_inventory.run(db,'TD SYNNEX',scope='all',limit=-1);assert any(r['model']=='Hercules 9120' for r in parent['rows'])
assert not db.execute("SELECT 1 FROM affiliations WHERE company='Synnex Technology International Corp' AND target='TD SYNNEX'").fetchone()
wiwynn=[r[0] for r in db.execute("SELECT model FROM devices WHERE id IN (SELECT device_id FROM records WHERE source_id='wiwynn-product-index')")];assert len(wiwynn)==12 and 'EP100G2' in wiwynn and 'EP200G2' not in wiwynn
report=dict(database_sha256=file_sha256(path),tables=audit,affiliation_target_changes=remapped,baseline=base['summary'],evidence_summary=base['evidence_summary'],inventories={s:{k:r[k] for k in ('identity_target_pairs','unique_identities','counts_by_target')} for s,r in inventories.items()},source_count=len(raw_sources),source_record_counts={k:v['records_seen'] for k,v in raw_sources.items()})
(root/'data/exports/odm-candidate-review-2026-10-04.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if k not in ('tables','source_record_counts')},indent=2))
# Counts become expectations only after the preservation and source-role audit above.
m=json.loads((root/'data/import-manifest-before-odm-2026-10-04.json').read_text());m['imports'].append(dict(source='curated',path='odm-products-2026-10-04.json'))
for p in sorted((root/'data/raw/odm-primary-2026-10-04').iterdir()):
 if p.is_file():m['files'][str(p.relative_to(root/'data'))]=file_sha256(p)
p=root/'data/odm-products-2026-10-04.json';m['files'][str(p.relative_to(root/'data'))]=file_sha256(p)
for key in list(m['expected']):
 if key=='sources':m['expected'][key]=report['source_record_counts']
 else:m['expected'][key]=db.execute('SELECT count(*) FROM '+('fcc_grantees' if key=='grantees' else key)).fetchone()[0]
(root/'data/import-manifest.json').write_text(json.dumps(m,indent=2)+'\n');db.close()
print('Reviewed manifest:',len(m['imports']),'jobs;',len(m['files']),'hashed inputs')
