import json,sqlite3
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt import documents,validation
from devicehunt.rebuild import file_sha256
root=Path.cwd();out=root/'data/exports';c=Catalog(root/'data/devices.sqlite',read_only=True);db=c.db
db.execute('ATTACH DATABASE ? AS previous',((root/'data/rebuilt-with-wedge100.sqlite').as_uri()+'?mode=ro',))
tables='japan_equipment public_equipment_rows ghana_equipment nigeria_equipment ised_equipment energystar_datacenter energystar_server_archives energystar_servers sources devices records affiliations identifiers facts hyperscaler_evidence fcc_applications fcc_exhibits fcc_grantees equipment_register bahrain_equipment anatel_equipment ntra_equipment tec_equipment ieee_assignments pci_entries bsmi_certificates bsmi_product_certificates bsmi_voluntary_certificates bsmi_declarations rra_equipment ncc_equipment fcc_id_index documents document_pages fcc_detail_pages fcc_detail_links fcc_text_excerpts search_index document_search'.split()
preserved={}
for t in tables:
 ignored={'observed_at','fetched_at','cache_path'} | ({'sha256'} if t=='sources' else set())
 cols=[r['name'] for r in db.execute('PRAGMA main.table_info('+t+')') if r['name'] not in ignored]
 if t=='search_index':cols.insert(0,'rowid')
 fields=','.join('"'+s+'"' for s in cols)
 old=f'SELECT {fields} FROM previous.{t}';new=f'SELECT {fields} FROM main.{t}'
 missing=db.execute('SELECT count(*) FROM ('+old+' EXCEPT '+new+')').fetchone()[0]
 added=db.execute('SELECT count(*) FROM ('+new+' EXCEPT '+old+')').fetchone()[0]
 assert missing==0,(t,missing)
 assert added=={'sources':1,'documents':1,'document_pages':32,'document_search':32}.get(t,0),(t,added)
 preserved[t]=dict(prior_rows_missing=missing,rows_added=added)
hash_changes=[dict(r) for r in db.execute('SELECT s.id,p.sha256 AS old,s.sha256 AS new FROM sources s JOIN previous.sources p ON p.id=s.id WHERE s.sha256<>p.sha256')]
bundle_sha=file_sha256(root/'data/raw/reference-documents-bundle.json')
assert len(hash_changes)==24 and all(r['id'].startswith('reference-pdf-') and r['new']==bundle_sha for r in hash_changes)
preserved['reference_bundle_hash_changes']=hash_changes
preserved['note']='Only reference archive source bundle hashes change; all prior individual PDF hashes, document pages and catalog assertions are identical. Document FTS rowids can shift; document keys/page/text are preserved.'
(out/'minipack2-reference-preservation.json').write_text(json.dumps(preserved,indent=2)+'\n')
sid='reference-pdf-meta-minipack2-spec'; checks={}
for query,page in [('277V',13),('asset tag',29),('deployed',9),('Supplier of Product',31)]:
 result=documents.search(db,query,source=sid,limit=-1)
 assert any(r['page_number']==page for r in result['rows']),query
 assert all(Path(r['local_pdf']).resolve()==root/'data/raw/meta-minipack2-spec.pdf' for r in result['rows'])
 checks[query]=result
all_pages=documents.search(db,source=sid,limit=-1)
assert all_pages['matches']==32 and len(all_pages['rows'])==32
assert documents.search(db,'Celestica',source=sid,limit=-1)['matches']==0
assert db.execute('SELECT count(*) FROM records WHERE source_id=?',(sid,)).fetchone()[0]==0
baseline=validation.run(db,root/'validation/owned-hardware.json')
assert baseline['summary']==dict(total=10,hits=9,gaps=1)
audit=dict(queries=checks,all_pages=all_pages['matches'],catalog_records_added=0,baseline=baseline['summary'])
(out/'minipack2-reference-search-audit.json').write_text(json.dumps(audit,indent=2)+'\n')
print('Prior data preserved; four targeted document queries and all 32 pages verified. Baseline unchanged.')
