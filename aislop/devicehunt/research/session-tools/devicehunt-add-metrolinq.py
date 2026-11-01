import json, hashlib, sqlite3
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt import importers, validation, vendor_inventory

root=Path.cwd(); out=root/'data/exports'
def save(path, value):
    path.write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n')
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()

scope=('Reviewed factual fields from web-indexed text of an original document hosted by FCC.report, '
       'not a direct FCC retrieval or locally archived PDF. Both PDF download attempts returned HTTP 403; '
       'no retry or alternative download was made. Page layout and full document bytes were not verified. '
       'Manufacturer denotes the explicit document claim, not a factory audit, current approval, '
       'hyperscaler use or an auction unit identity. No mapping of Nigeria NCC company abbreviations is implied.')
entries=[
    dict(id='metrolinq-ml1-report-fields',url='https://fcc.report/FCC-ID/HEDML16035/3913123.pdf',
         capture_method='reviewed factual search-result extract; not original PDF',page='1',
         report_number='FR7D2234-05AA',issue_date='2018-07-02',brand='Ignitenet',
         model_field='ML1-60-35/ML1-60-19',models=['ML1-60-35','ML1-60-19'],
         applicant='Accton Technology Corp',manufacturers=['Joy Technology (Shen Zhen) Co. Ltd','Accton Technology Corp'],
         fcc_id='HEDML16035',
         interpretation='Two complete slash-separated models are distinct labels, not aliases. Both named manufacturers are retained for both report-listed models; the report does not assign a particular unit to one factory. Test findings concern the tested sample.'),
    dict(id='metrolinq-ml10g-guide-fields',url='https://fcc.report/FCC-ID/HEDML10G360/3786294.pdf',
         capture_method='reviewed factual web-tool PDF-text extract; not original PDF',page='1, 5',
         document_version='E122017-CS-R01',brand='IgniteNet',model_field='ML-60-10G-360',models=['ML-60-10G-360'],
         manufacturers=['Accton Technology Corporation'],
         interpretation='Model/HVIN on page 1 and explicitly labeled manufacturer on page 5 of the same guide. FCC identifier in hosting URL is retained only as URL context, not a printed-ID assertion.')
]
curated={'reviewed_on':'2026-09-27','sources':[],'devices':[]}
for e in entries:
    e.update(reviewed_on='2026-09-27',scope=scope,pdf_download_status=403,pdf_archived=False)
    path=root/'data/raw'/(''+e['id']+'.json'); save(path,e)
    curated['sources'].append(dict(id=e['id'],url=e['url'],tier='primary_document_mirror',
        license='Factual model/company fields; original document rights retained',
        snapshot='raw/'+path.name,coverage=scope+' '+e['interpretation']))
    for model in e['models']:
        facts=[['printed_brand',e['brand']],['printed_model_field',e['model_field']],
               ['document_page',e['page']],['capture_method',e['capture_method']],
               ['evidence_scope',scope+' '+e['interpretation']]]
        for name in ['report_number','issue_date','document_version']:
            if name in e: facts.append([name,e[name]])
        affiliations=[[name,'manufacturer'] for name in e['manufacturers']]+[['IgniteNet','brand']]
        if e.get('applicant'): affiliations.append([e['applicant'],'regulatory_applicant'])
        curated['devices'].append(dict(source=e['id'],key=model,url=e['url'],brand='IgniteNet',
            model=model,title='IgniteNet '+model,kind='outdoor wireless radio',
            affiliations=affiliations,identifiers=[['fcc_id',e['fcc_id']]] if e.get('fcc_id') else [],facts=facts))
path=root/'data/metrolinq-reviewed.json';save(path,curated)
c=Catalog(root/'data/devices.sqlite')
before={t:c.db.execute('select count(*) from '+t).fetchone()[0] for t in ['devices','records','sources']}
assert before==dict(devices=3528,records=4643,sources=196),before
assert importers.curated(c,path)==3
after={t:c.db.execute('select count(*) from '+t).fetchone()[0] for t in before}
assert after==dict(devices=3530,records=4646,sources=198),after
rows=[dict(r) for r in c.db.execute('''SELECT d.brand,d.model,a.company,a.target,a.role,s.tier,s.sha256,r.source_id
  FROM records r JOIN devices d ON d.id=r.device_id JOIN sources s ON s.id=r.source_id
  JOIN affiliations a ON a.record_id=r.id WHERE r.source_id LIKE 'metrolinq-%' ORDER BY d.model,a.role,a.company''')]
assert len([r for r in rows if r['target']=='Accton' and r['role']=='manufacturer'])==3
assert len([r for r in rows if r['company']=='Joy Technology (Shen Zhen) Co. Ltd' and r['target'] is None])==2
assert not c.db.execute("select 1 from hyperscaler_evidence h join records r on h.record_id=r.id where r.source_id like 'metrolinq-%'").fetchone()
assert c.db.execute("select count(*) from identifiers i join records r on i.record_id=r.id where r.source_id like 'metrolinq-%'").fetchone()[0]==2
base=validation.run(c.db,root/'validation/owned-hardware.json');assert base['summary']==dict(total=10,hits=9,gaps=1)
inv=vendor_inventory.run(c.db,scope='manufacturer',limit=-1)
for model in ['ML1-60-35','ML1-60-19','ML-60-10G-360']:
    assert any(r['brand']=='IgniteNet' and r['model']==model and r['target']=='Accton' for r in inv['rows'])
manifest=root/'data/import-manifest.json';m=json.loads(manifest.read_text())
m['imports'].append(dict(source='curated',path='metrolinq-reviewed.json'))
for p in [path]+[root/'data/raw'/(e['id']+'.json') for e in entries]:m['files'][str(p.relative_to(root/'data'))]=sha(p)
m['expected']['devices']=after['devices'];m['expected']['records']=after['records']
for e in entries:m['expected']['sources'][e['id']]=len(e['models'])
save(manifest,m)
save(out/'metrolinq-import-audit.json',dict(before=before,after=after,claims=rows,owned_baseline=base['summary'],
    manufacturer_pairs=inv['identity_target_pairs'],manufacturer_identities=inv['unique_identities'],
    counts_by_target=inv['counts_by_target'],manifest_jobs=len(m['imports']),manifest_files=len(m['files']),
    database_sha256=sha(root/'data/devices.sqlite')))
c.db.close();print(json.dumps(dict(before=before,after=after,manufacturer_pairs=inv['identity_target_pairs'],baseline=base['summary'])))
