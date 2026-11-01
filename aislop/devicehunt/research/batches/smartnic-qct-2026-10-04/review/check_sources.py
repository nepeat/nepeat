from pathlib import Path
import hashlib,json,shutil
from devicehunt.reference_documents import prepare
from devicehunt.documents import read_pages
from pypdf import PdfReader
base=Path('data/raw/smartnic-qct-2026-10-04');out=Path('research/batches/smartnic-qct-2026-10-04/review');proof={'proposals':[],'candidates':[],'hashes':{}}
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
for rel in ['smartnic/reference-pdf-proposal.json','smartnic/english-reference-pdf-proposal.json','t42s/reference-pdf-proposal.json']:
 p=base/rel;obj=json.loads(p.read_text());root=out/(p.parent.name+'-'+p.name.removesuffix('.json'));root.mkdir(exist_ok=True)
 for e in obj['documents']:
  for k in ['pdf','text']:
   dep=p.parent/e[k]['path'];assert sha(dep)==e[k]['sha256'];shutil.copyfile(dep,root/dep.name);proof['hashes'][str(dep)]=sha(dep)
  pages=read_pages((p.parent/e['text']['path']).read_bytes(),e['pdf']['sha256']);assert len(PdfReader(p.parent/e['pdf']['path']).pages)==len(pages)==e['page_count']
 q=root/p.name;shutil.copyfile(p,q);_,ready=prepare(q);proof['proposals'].append({'path':str(p),'documents':len(ready),'pages':sum(len(pgs) for _,pgs in ready),'isolated_prepare':True});proof['hashes'][str(p)]=sha(p)
for rel in ['smartnic/candidate-curated.json','smartnic/english-candidate-curated.json','t42s/candidate-curated.json']:
 p=base/rel;obj=json.loads(p.read_text());proof['hashes'][str(p)]=sha(p)
 for s in obj['sources']:
  snap=p.parent/s['snapshot'];assert snap.is_file();proof['hashes'][str(snap.resolve().relative_to(Path.cwd()))]=sha(snap)
 for d in obj['devices']:
  s=next(s for s in obj['sources'] if s['id']==d['source']);snap=p.parent/s['snapshot'];env=snap.with_suffix('.pages.json');pages=read_pages(env.read_bytes(),sha(snap))
  for e in d['evidence']:
   assert e['capture_sha256']==sha(snap);text=pages[e['pdf_page']-1]['text'];quote=e['literal_quote']
   if 'extracted_line_start' in e:
    assert quote=='\n'.join(text.splitlines()[e['extracted_line_start']-1:e['extracted_line_end']]),(rel,e['fact_name'])
   else:
    # QCT text extraction spatial order differs; visible transcription separately inspected.
    for fragment in quote.split('\n'):
     norm=lambda t:''.join(t.replace('®','').split())
     assert norm(fragment) in norm(text),(rel,e['fact_name'],fragment)
 proof['candidates'].append({'path':str(p),'records':len(obj['devices']),'facts':sum(len(d['facts']) for d in obj['devices']),'models':[d['model'] for d in obj['devices']]})
proof['totals']={'records':sum(c['records'] for c in proof['candidates']),'facts':sum(c['facts'] for c in proof['candidates']),'pdfs':sum(p['documents'] for p in proof['proposals']),'pages':sum(p['pages'] for p in proof['proposals'])}
proof['live_sha256']=sha(Path('data/devices.sqlite'));(out/'source-checks.json').write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps(proof['totals']))
