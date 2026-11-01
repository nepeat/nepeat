"""Extract complete PDFs with the reviewed project extractor; additions only."""
from pathlib import Path
import importlib.util,json,hashlib,shutil,sys,tempfile
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
sys.path.insert(0,str(ROOT))
from devicehunt.documents import read_capture
from devicehunt.reference_documents import prepare


def sha(path):
 with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()

def save(name,obj):
 with (HERE/name).open('x') as f:json.dump(obj,f,indent=2,ensure_ascii=False);f.write('\n')


def main():
 extractor=ROOT/'scripts/extract_pdf_pages.py'
 spec=importlib.util.spec_from_file_location('reviewed_pdf_extractor',extractor)
 module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
 tools=Path('/nix/store/wb8486c5l68f1shsbc217b7m4hbgrw3s-poppler-utils-26.06.0/bin')
 results=json.loads((HERE/'collection-results.json').read_text())
 entries=[];rows=[]
 for result in results['results']:
  if result['outcome']!='new_pdf':continue
  pdf=HERE/result['pdf'];sidecar=HERE/(result['slug']+'-pages.json')
  assert sha(pdf)==result['sha256']
  capture=module.extract(pdf,sidecar,tools)
  pages,metadata=read_capture(sidecar.read_bytes(),result['sha256'])
  assert len(pages)==capture['pdf_page_count'] and sha(pdf)==result['sha256']
  entry=dict(id='edgecore-'+result['slug'],url=result['response_url'],title=result['printed_document_heading'],category='datasheet',tier='vendor',scope='Complete original vendor-linked datasheet preserved for offline document search. Embedded text only, no OCR or whole-document visual review. Printed names, revisions, grouped options and conflicting statements remain source-scoped text. No identities, aliases, manufacturer, compatibility, deployment, current support or auction-unit assertions are created.',pdf=dict(path=pdf.name,sha256=sha(pdf)),text=dict(path=sidecar.name,sha256=sha(sidecar)),page_count=len(pages))
  entries.append(entry)
  rows.append(dict(selection_number=result['selection_number'],pdf=pdf.name,pdf_sha256=sha(pdf),sidecar=sidecar.name,sidecar_sha256=sha(sidecar),page_count=len(pages),pages_with_text=sum(bool(p['text'].strip()) for p in pages),text_characters=sum(len(p['text']) for p in pages),extraction_method=metadata['extraction_method'],extractor_version=metadata.get('extractor_version'),source_role='documentation_publisher_only',structured_device_facts=0))
  print(pdf.name+': '+str(len(pages))+' complete pages',flush=True)
 # Validate full standard entry shapes and strict sibling dependencies in an
 # ephemeral private bundle; never persist/import a partial replacement bundle.
 with tempfile.TemporaryDirectory(prefix='entry-validation-',dir=HERE) as temp:
  root=Path(temp)
  for entry in entries:
   for kind in ['pdf','text']:shutil.copyfile(HERE/entry[kind]['path'],root/entry[kind]['path'])
  bundle=root/'validation-only.json'
  bundle.write_text(json.dumps(dict(schema_version=1,collection='public-device-references-v1',documents=entries)))
  _,ready=prepare(bundle)
  assert len(ready)==len(entries)
 save('candidate-reference-entries.json',dict(schema_version=1,artifact_kind='reference-document-additions-only',integration_warning='Do not pass this file to import_bundle. The integrator must preserve all existing61 complete-reference entries, append only independently accepted new entries, and validate the complete combined collection with sibling inputs.',documents=entries))
 save('extraction-report.json',dict(outcome='complete',extractor=dict(path=str(extractor.relative_to(ROOT)),sha256=sha(extractor)),tool_directory=str(tools),tools={name:dict(path=str(tools/name),sha256=sha(tools/name)) for name in ['pdfinfo','pdftotext']},pdfs=len(entries),pages=sum(r['page_count'] for r in rows),pages_with_text=sum(r['pages_with_text'] for r in rows),complete_sidecars=len(rows),standard_reference_prepare='all entries validated together using temporary byte-identical sibling files; no partial bundle persisted or imported',ocr=False,visual_review=False,structured_device_facts=0,results=rows))
 print('prepared '+str(len(entries))+' additions-only entries',flush=True)


if __name__=='__main__':main()
