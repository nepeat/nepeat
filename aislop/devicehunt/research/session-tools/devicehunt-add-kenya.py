import hashlib,json,re
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt import importers,reference_documents

root=Path.cwd();raw=root/'data/raw';capture=raw/'kenya-ca'
for old,new in [('approved-2026-08.pdf','kenya-approved-2026-08.pdf'),('approved-2026-08-text.json','kenya-approved-2026-08-text.json')]:
    assert not (raw/new).exists()
    (capture/old).rename(raw/new)
url='https://www.ca.go.ke/sites/default/files/2026-09/List%20of%20Type%20Approved%20Equipment%20August%202026.pdf'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
pdf=raw/'kenya-approved-2026-08.pdf';txt=raw/'kenya-approved-2026-08-text.json'
pages=json.loads(txt.read_text())['pages'];assert len(pages)==203
assert 'List%20of%20Type%20Approved%20Equipment%20August%202026.pdf' in (capture/'index.html').read_text() or 'List of Type Approved Equipment August 2026.pdf' in (capture/'index.html').read_text()
tables=json.loads((capture/'reviewed-page-tables.json').read_text())
norm=lambda s:' '.join(s.split())
scope=('Selected manufacturer/brand/model cells visually reviewed in the original Kenya CA August 2026 register. '
       'All 203 pages screened for target company names; seven candidate rows on five pages, six imported. '
       'The CAMILLO / GOLGI two-line cell remains unresolved in the page archive, not split or aliased. '
       'Printed categories may be inconsistent; retained as source facts, not inferred device capabilities. '
       'Manufacturer namespace is a registry claim, not a factory audit, current approval verification or unit provenance. '
       'The official index explicitly says the register is nonexhaustive. Physical PDF page and printed page are distinct.')
selected=[(9,0,13,9,'Amazon Data Services','Amazon Data Services, Inc.','MOBILE PHONES AND TABLETS'),
          (23,0,11,23,'Edge-Core','EDGECORE NETWORKS','MOBILE PHONES AND TABLETS'),
          (180,0,22,183,'Edge-Core','EDGECORE NETWORKS','ROUTERS'),
          (186,3,2,189,'Amazon Data Services','AMAZON DATA SERVICES INC','SERVERS'),
          (192,0,9,195,'Edge-Core','EDGECORE NETWORKS CORPORATION','SWITCHES'),
          (192,0,10,195,'Edge-Core','EDGECORE','SWITCHES')]
devices=[]
for page,ti,ri,printed,target,company,category in selected:
    row=tables[str(page)][ti][ri]
    assert tables[str(page)][ti][1]==['NO','MANUFACTURER','BRAND','MODEL']
    number,manufacturer,brand,model=map(norm,row)
    assert model!='CAMILLO GOLGI'
    # No guessed part-number extraction or prefix propagation: preserve whole cells.
    facts=[('registry_manufacturer',manufacturer),('registry_brand',brand),('registry_model_cell',row[3]),
           ('registry_category',category),('document_page',str(page)),('printed_page',str(printed)),
           ('registry_row_number',number),('evidence_scope',scope),
           ('manufacturer_mapping','This reviewed row maps the printed company and location to '+target+'; no global alias rule added.')]
    if page==180:
        facts.append(('source_quality_note','Adjacent row 22 prints the same model ECS5550-30X under EXTREME NETWORKS, INC.; the conflicting source text is retained in the full PDF archive.'))
    if page==23:
        facts.append(('source_quality_note','Registry prints EDGECORE NETWORKS for ECS TABLET PC (TR10RS1). Preserved as a regulator claim; not independently confirmed as the physical manufacturer.'))
    devices.append({'source':'kenya-ca-reviewed','key':f'pdf-{page}-table-{ti+1}-row-{number}',
                    'url':url+'#page='+str(page),'brand':target,'model':model,'title':target+' '+model,
                    'kind':'registered equipment','affiliations':[[company,'registry_manufacturer'],[brand,'brand']],
                    'facts':facts,'reviewed_cells':row,'table_index':ti,'row_index':ri})
data={'reviewed_on':'2026-09-27','sources':[{'id':'kenya-ca-reviewed','url':url,'tier':'regulatory',
      'license':'Kenya CA public register; factual extraction, source rights retained',
      'snapshot':'raw/kenya-approved-2026-08.pdf','coverage':scope}],
      'devices':devices,'unresolved_rows':[{'physical_pdf_page':186,'printed_page':189,'table_index':3,'row_index':3,
      'cells':tables['186'][3][3],'reason':'Two-line model cell could be a combined label or multiple names; no split or alias to existing Camillo_Golgi inferred.'}]}
curated=root/'data/kenya-approved-reviewed.json';curated.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
bundle=raw/'reference-documents-bundle.json';refs=json.loads(bundle.read_text())
assert not any(d['id']=='kenya-2026-08' for d in refs['documents'])
refs['documents'].append({'id':'kenya-2026-08','pdf':{'path':pdf.name,'sha256':sha(pdf)},
    'text':{'path':txt.name,'sha256':sha(txt)},'url':url,'title':'Kenya CA type approved equipment August 2026',
    'category':'equipment register','tier':'regulatory','scope':scope+' Complete embedded page text archived; not a structured all-row register.',
    'page_count':203})
bundle.write_text(json.dumps(refs,ensure_ascii=False,indent=2)+'\n')
reference_documents.prepare(bundle)
c=Catalog(root/'data/devices.sqlite')
assert not c.db.execute("SELECT 1 FROM sources WHERE id='kenya-ca-reviewed'").fetchone()
before={table:c.db.execute('SELECT count(*) FROM '+table).fetchone()[0] for table in ('devices','records','sources','documents','document_pages')}
assert importers.curated(c,curated)==6
archive=reference_documents.import_bundle(c,bundle)
after={table:c.db.execute('SELECT count(*) FROM '+table).fetchone()[0] for table in before}
manifest=root/'data/import-manifest.json';m=json.loads(manifest.read_text())
m['imports'].append({'source':'curated','path':str(curated.relative_to(root/'data'))})
for p in [curated,pdf,txt,bundle,capture/'index.html',capture/'reviewed-page-tables.json']:
    m['files'][str(p.relative_to(root/'data'))]=sha(p)
for key in m['expected']:
    if key=='sources':m['expected'][key]={r[0]:r[1] for r in c.db.execute('SELECT id,records_seen FROM sources')}
    else:m['expected'][key]=c.db.execute('SELECT count(*) FROM '+('fcc_grantees' if key=='grantees' else key)).fetchone()[0]
manifest.write_text(json.dumps(m,ensure_ascii=False,indent=2)+'\n')
c.db.close()
proof={'before':before,'after':after,'delta':{k:after[k]-before[k] for k in before},'archive':archive,
       'selected_rows':6,'unresolved_rows':1,'screened_pdf_pages':203,'visually_reviewed_pdf_pages':[9,23,180,186,192],
       'source_pdf_sha256':sha(pdf),'new_snapshot_files':5,'manifest_jobs':len(m['imports']),'manifest_files':len(m['files'])}
(root/'data/exports/kenya-import-audit.json').write_text(json.dumps(proof,indent=2)+'\n')
print(json.dumps(proof,indent=2))
