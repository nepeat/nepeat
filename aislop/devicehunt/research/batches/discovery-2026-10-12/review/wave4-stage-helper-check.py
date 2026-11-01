"""Inspect frozen unexecuted wave4 helper/candidates, no imports or future data writes."""
from pathlib import Path
import ast,collections,datetime,hashlib,json,sqlite3
from devicehunt.catalog import normalize
from devicehunt.importers import prepare_curated
from devicehunt import documents,reference_documents,rebuild
ROOT=Path.cwd();B=ROOT/'research/batches/discovery-2026-10-12';I=B/'integration-wave4';DATA=ROOT/'data';R=B/'review'
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1<<20),b''):h.update(b)
 return h.hexdigest()
script=I/'stage.py';tree=ast.parse(script.read_text());compile(tree,str(script),'exec')
assign={t.id:n.value for n in tree.body if isinstance(n,ast.Assign) for t in n.targets if isinstance(t,ast.Name)}
groups=ast.literal_eval(assign['GROUPS']);live=ast.literal_eval(assign['EXPECTED_LIVE']);assert len(groups)==2
for name,h in live.items():assert sha(DATA/name)==h
freeze=json.loads((B/'integration-wave3/code-under-test.json').read_text())['files'];assert len(freeze)==209
for name,h in freeze.items():assert sha(ROOT/name)==h
root,manifest=rebuild.read_manifest(DATA/'import-manifest.json');oldbundle=DATA/'raw/reference-documents-with-wave3-2026-10-12.json';old=json.loads(oldbundle.read_text());_,prepared=reference_documents.prepare(oldbundle);assert len(prepared)==57 and sum(len(p) for _,p in prepared)==3710
conn=sqlite3.connect((DATA/'devices.sqlite').as_uri()+'?mode=ro',uri=True);conn.row_factory=sqlite3.Row
counts={n:conn.execute('SELECT count(*) FROM '+n).fetchone()[0] for n in ['devices','records','facts','sources','documents','document_pages']}
plans=[];payloads=[];newids=set();existingids=set();sourceids=set();pages=0;newpdfs=[];roles=collections.Counter();hashes={str(script.relative_to(ROOT)):sha(script)}
for group,(relative,wanted,review) in groups.items():
 parent=B/relative;candidate=parent/'candidate.json';assert sha(candidate)==wanted
 assert wanted in (R/review).read_text() and 'accept' in (R/review).read_text().lower();hashes[str(candidate.relative_to(ROOT))]=wanted;hashes[str((R/review).relative_to(ROOT))]=sha(R/review)
 payload=json.loads(candidate.read_text());payloads.append(payload);pure=prepare_curated(candidate);assert len(pure)==len(payload['sources'])
 dest=DATA/'raw'/f'{group}-wave4-2026-10-12';plans.append({'source':str(candidate.relative_to(ROOT)),'dest':str((dest/'candidate.json').relative_to(ROOT)),'sha256':wanted})
 for s in payload['sources']:
  assert s['id'] not in sourceids;sourceids.add(s['id']);assert not conn.execute('SELECT 1 FROM sources WHERE id=?',(s['id'],)).fetchone()
  for field in ['snapshot','text_sidecar']:
   if field not in s:continue
   rel=Path(s[field]);assert not rel.is_absolute() and '..' not in rel.parts;source=(parent/rel).resolve();target=(dest/rel).resolve();assert source.is_relative_to(parent.resolve()) and target.is_relative_to(dest.resolve())
   h=sha(source);hashes[str(source.relative_to(ROOT))]=h
   if field=='snapshot':assert h==s['original_sha256']
   assert not target.exists() or sha(target)==h
   plans.append({'source':str(source.relative_to(ROOT)),'dest':str(target.relative_to(ROOT)),'sha256':h})
  if 'text_sidecar' in s:
   pdf=parent/s['snapshot'];text=parent/s['text_sidecar'];full,metadata=documents.read_capture(text.read_bytes(),sha(pdf));assert len(full)==s['extraction_metadata']['pdf_page_count'];assert full==json.loads(text.read_text())['pages']
   assert metadata['extraction_method']==s['extraction_metadata']['extraction_method'];pages+=len(full)
   for field,prefix in [('snapshot','pdf'),('text_sidecar','text')]:
    source=parent/s[field];target=DATA/'raw'/('wave4-2026-10-12-'+source.name);assert not target.exists() or sha(target)==sha(source);plans.append({'source':str(source.relative_to(ROOT)),'dest':str(target.relative_to(ROOT)),'sha256':sha(source)})
   newpdfs.append({'id':s['id'],'url':s['url'],'pdf':sha(pdf),'pages':len(full)})
 for d in payload['devices']:
  assert d['source'] in sourceids;did=hashlib.sha256(json.dumps([normalize(d['brand']),d['model'].casefold().strip(),'']).encode()).hexdigest()[:20]
  present=conn.execute('SELECT * FROM devices WHERE id=?',(did,)).fetchone()
  if present:existingids.add(did)
  else:newids.add(did)
  roles.update(tuple(a) for a in d['affiliations']);assert not d.get('hyperscaler')
  if group.startswith('as8000'):
   assert d['identifiers']==[] and all(a[1]=='documentation_vendor' for a in d['affiliations'])
  else:assert d['affiliations']==[['Inspur Corporation','brand']] and d['identifiers']==[['spec_system_name',d['key']]]
conn.close()
assert len(sourceids)==20 and len(newids)==5 and len(existingids)==9 and pages==14 and len(newpdfs)==2
records=sum(len(p['devices']) for p in payloads);facts=sum(len(d['facts']) for p in payloads for d in p['devices']);assert records==20 and facts==392
expected={**counts,'devices':counts['devices']+len(newids),'records':counts['records']+records,'facts':counts['facts']+facts,'sources':counts['sources']+len(sourceids)+len(newpdfs),'documents':counts['documents']+len(newpdfs),'document_pages':counts['document_pages']+pages}
assert expected=={'devices':5122,'records':7261,'facts':74602,'sources':2115,'documents':70,'document_pages':3798}
for key,newvalues,oldvalues in [('id',[p['id'] for p in newpdfs],[e['id'] for e in old['documents']]),('url',[p['url'] for p in newpdfs],[e['url'] for e in old['documents']]),('sha256',[p['pdf'] for p in newpdfs],[e['pdf']['sha256'] for e in old['documents']])]:assert len(set(newvalues))==2 and not set(newvalues)&set(oldvalues)
newpins={str(Path(p['dest']).relative_to('data')) for p in plans};newpins.add('raw/reference-documents-with-wave4-2026-10-12.json');assert len(newpins)==29 and not newpins&set(manifest['files']);assert len(manifest['files'])+len(newpins)==3970 and len(manifest['imports'])+2==150
for p in ['devices-candidate-wave4-2026-10-12.sqlite','import-manifest-candidate-wave4-2026-10-12.json','raw/reference-documents-with-wave4-2026-10-12.json']:assert not (DATA/p).exists()
assert not (I/'stage-report.json').exists();assert live=={name:sha(DATA/name) for name in live};assert sha(script)==hashes[str(script.relative_to(ROOT))]
report={'status':'PASS static helper and finite input review only; unexecuted','checked_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'helper_sha256':sha(script),'input_hashes':hashes,'copy_plan':plans,'accepted_live_pair_unchanged':live,'frozen_code_files_verified':209,'pure_prepare_groups':2,'sources':20,'records':records,'facts':facts,'new_identities':sorted(newids),'existing_identities':sorted(existingids),'role_counts':{str(k):v for k,v in roles.items()},'reference_bundle':{'old_entries':57,'new_entries':59,'old_pages':3710,'new_pages':3724},'expected_counts':expected,'manifest_expected_jobs':150,'manifest_expected_inputs':3970,'new_pins':sorted(newpins),'limitations':'No stage/helper/import execution, no candidate DB/bundle/manifest creation, no network or shared writes; stage results, clean rebuild and final promotion review remain required.'}
with (R/'wave4-stage-helper-check.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print('PASS static wave4 helper:392facts,5newIDs,59PDFs,150jobs3970pins; no execution')
