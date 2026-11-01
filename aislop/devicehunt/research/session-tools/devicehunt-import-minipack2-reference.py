import json
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt.reference_documents import import_bundle
from devicehunt.rebuild import file_sha256,read_manifest
root=Path.cwd();raw=root/'data/raw'; out=root/'data/exports'
pdf=raw/'meta-minipack2-spec.pdf'; text=raw/'meta-minipack2-spec.json'
pages=json.loads(text.read_text())['pages'];assert len(pages)==32
assert 'Celestica' not in '\n'.join(p['text'] for p in pages)
assert 'deployed' in pages[8]['text'] and 'Meta' in pages[8]['text']
assert '208V' in pages[12]['text'] and '277V' in pages[12]['text']
assert 'Company:' in pages[30]['text'] and 'Contact Info:' in pages[30]['text']
review=dict(pdf_sha256=file_sha256(pdf),reviewed_on='2026-09-27',
 document_revision='1.0',document_date='2021-09-16',
 rendered_and_reviewed_pages=[9,13,29,31],
 findings=[dict(page=9,scope='Historical Meta F16 platform deployment statement; not unit provenance.'),
           dict(page=13,scope='Generic system requirements with conditional power, cooling and port/module descriptions; not an installed-unit configuration.'),
           dict(page=29,scope='Label requirements describe the front pull-out model/serial tag and Meta asset tag; useful when inspecting listing photos.'),
           dict(page=31,scope='Supplier company and product fields are blank; this document supplies no Celestica manufacturer claim.')],
 extraction_limitations='pypdf emitted cross-reference warnings but extracted all 32 pages. Four rendered pages checked against embedded text. No OCR; graphics may contain unindexed text.',
 presentation_attempt='The separate rackcdn presentation URL failed the PDF signature/size validation; no PDF was saved. Error detail was not retained by the download script. No supplier assertion is based on that failed retrieval.')
(raw/'meta-minipack2-review.json').write_text(json.dumps(review,indent=2)+'\n')
bundle=raw/'reference-documents-bundle.json'; data=json.loads(bundle.read_text())
sid='reference-pdf-meta-minipack2-spec'
assert not any(e['id']=='meta-minipack2-spec' for e in data['documents'])
old_bundle_sha=file_sha256(bundle)
data['documents'].append(dict(id='meta-minipack2-spec',pdf=dict(path=pdf.name,sha256=file_sha256(pdf)),
 text=dict(path=text.name,sha256=file_sha256(text)),
 url='https://www.opencompute.org/documents/minipack2-generic-ocp-specification-rev1-0-pdf',
 title='Meta Minipack2 generic system specification Rev 1.0 (2021-09-16)',
 category='hardware specification',tier='vendor',page_count=32,
 scope='Original Meta-authored OCP generic specification. Historical platform design and deployment text, including configuration-dependent requirements. Supplier appendix is blank. This reference does not establish Celestica manufacturing, equivalence of suffixed commercial variants, installed-unit specifications or prior ownership.'))
bundle.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
c=Catalog(root/'data/devices.sqlite')
tables=('devices','records','sources','documents','document_pages','hyperscaler_evidence')
before={t:c.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in tables}
result=import_bundle(c,bundle)
after={t:c.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in tables}
assert {t:after[t]-before[t] for t in tables}==dict(devices=0,records=0,sources=1,documents=1,document_pages=32,hyperscaler_evidence=0)
assert c.db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
assert not c.db.execute('PRAGMA foreign_key_check').fetchall()
c.db.close()
manifest=root/'data/import-manifest.json'; m=json.loads(manifest.read_text())
for p in (bundle,pdf,text,raw/'meta-minipack2-fetch-report.json',raw/'meta-minipack2-review.json'):
 m['files'][str(p.relative_to(root/'data'))]=file_sha256(p)
m['expected']['sources'][sid]=1
m['expected']['documents']=after['documents'];m['expected']['document_pages']=after['document_pages']
manifest.write_text(json.dumps(m,ensure_ascii=False,indent=2)+'\n'); read_manifest(manifest)
audit=dict(before=before,after=after,reference_collection=result,
 old_reference_bundle_sha256=old_bundle_sha,new_reference_bundle_sha256=file_sha256(bundle),
 original_pdf_sha256=file_sha256(pdf),database_sha256=file_sha256(root/'data/devices.sqlite'),
 manifest_jobs=len(m['imports']),manifest_files=len(m['files']))
(out/'minipack2-reference-import-audit.json').write_text(json.dumps(audit,indent=2)+'\n');print(json.dumps(audit,indent=2))
