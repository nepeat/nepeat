"""Read-only unexecuted stage helper / accepted input plan verification."""
from pathlib import Path
import ast,collections,datetime,hashlib,json,sqlite3
from devicehunt.catalog import normalize
from devicehunt.importers import prepare_curated
from devicehunt import documents,reference_documents,rebuild
ROOT=Path.cwd();B=ROOT/'research/batches/discovery-2026-10-12';I=B/'integration-wave5';DATA=ROOT/'data';R=B/'review'
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for block in iter(lambda:f.read(1<<20),b''):h.update(block)
 return h.hexdigest()
script=I/'stage.py';frozen=sha(script);assert frozen=='ec9ebc10d4f7bebab2875467ff7fdc70ce9fbb94ca02d3f56c6f09734007418e';tree=ast.parse(script.read_text());compile(tree,str(script),'exec')
assign={t.id:n.value for n in tree.body if isinstance(n,ast.Assign) for t in n.targets if isinstance(t,ast.Name)}
groups=ast.literal_eval(assign['GROUPS']);reviews=ast.literal_eval(assign['REVIEW_HASHES']);live=ast.literal_eval(assign['EXPECTED_LIVE']);assert len(groups)==len(reviews)==2 and all(reviews.values())
for name,h in reviews.items():assert sha(R/name)==h
for name,h in live.items():assert sha(DATA/name)==h
freeze=json.loads((B/'integration-wave3/code-under-test.json').read_text())['files'];assert len(freeze)==209
for name,h in freeze.items():assert sha(ROOT/name)==h
root,manifest=rebuild.read_manifest(DATA/'import-manifest.json');oldpath=DATA/'raw/reference-documents-with-wave4-2026-10-12.json';old=json.loads(oldpath.read_text());_,prepared=reference_documents.prepare(oldpath);assert len(prepared)==59 and sum(len(p) for _,p in prepared)==3724
con=sqlite3.connect((DATA/'devices.sqlite').as_uri()+'?mode=ro',uri=True);con.row_factory=sqlite3.Row
counts={n:con.execute('SELECT count(*) FROM '+n).fetchone()[0] for n in ['devices','records','facts','sources','documents','document_pages']}
plans=[];payloads=[];newids=set();existingids=set();sourceids=set();newpdfs=[];roles=collections.Counter();deps=[];pages=0;hashes={str(script.relative_to(ROOT)):frozen}
for group,(relative,wanted,review) in groups.items():
 parent=B/relative;candidate=parent/'candidate.json';assert sha(candidate)==wanted and wanted in (R/review).read_text() and 'accept' in (R/review).read_text().lower();hashes[str(candidate.relative_to(ROOT))]=wanted;hashes[str((R/review).relative_to(ROOT))]=sha(R/review)
 payload=json.loads(candidate.read_text());payloads.append(payload);assert len(prepare_curated(candidate))==len(payload['sources']);dest=DATA/'raw'/f'{group}-wave5-2026-10-12'
 plans.append({'source':str(candidate.relative_to(ROOT)),'dest':str((dest/'candidate.json').relative_to(ROOT)),'sha256':wanted,'type':'candidate'})
 for s in payload['sources']:
  assert s['id'] not in sourceids;sourceids.add(s['id']);assert not con.execute('SELECT 1 FROM sources WHERE id=?',(s['id'],)).fetchone()
  for field in ['snapshot','text_sidecar']:
   if field not in s:continue
   rel=Path(s[field]);assert not rel.is_absolute() and '..' not in rel.parts;p=(parent/rel).resolve();target=(dest/rel).resolve();assert p.is_relative_to(parent.resolve()) and target.is_relative_to(dest.resolve()) and target.is_relative_to(DATA.resolve());h=sha(p);hashes[str(p.relative_to(ROOT))]=h
   if field=='snapshot':assert h==s['original_sha256']
   assert not target.exists() or sha(target)==h;plans.append({'source':str(p.relative_to(ROOT)),'dest':str(target.relative_to(ROOT)),'sha256':h,'type':field})
  for dep in s.get('evidence_dependencies',[]):
   rel=Path(dep['path']);assert not rel.is_absolute() and '..' not in rel.parts;p=(parent/rel).resolve();target=(dest/rel).resolve();assert p.is_relative_to(parent.resolve()) and target.is_relative_to(dest.resolve()) and target.is_relative_to(DATA.resolve());assert sha(p)==dep['sha256'];assert not target.exists() or sha(target)==dep['sha256'];hashes[str(p.relative_to(ROOT))]=dep['sha256'];plans.append({'source':str(p.relative_to(ROOT)),'dest':str(target.relative_to(ROOT)),'sha256':dep['sha256'],'type':'evidence_dependency'});deps.append(dep)
  if 'text_sidecar' in s:
   assert len(s['coverage'])<=4000;pdf=parent/s['snapshot'];text=parent/s['text_sidecar'];full,metadata=documents.read_capture(text.read_bytes(),sha(pdf));assert len(full)==s['extraction_metadata']['pdf_page_count'] and metadata==s['extraction_metadata'];pages+=len(full)
   for field in ['snapshot','text_sidecar']:
    p=parent/s[field];target=DATA/'raw'/('wave5-2026-10-12-'+p.name);assert not target.exists() or sha(target)==sha(p);plans.append({'source':str(p.relative_to(ROOT)),'dest':str(target.relative_to(ROOT)),'sha256':sha(p),'type':'reference_sibling'})
   newpdfs.append({'id':s['id'],'url':s['url'],'pdf':sha(pdf),'pages':len(full)})
 for d in payload['devices']:
  assert d['source'] in sourceids and not d.get('hyperscaler');assert all(role=='documentation_vendor' for _,role in d['affiliations']);roles.update(tuple(a) for a in d['affiliations'])
  did=hashlib.sha256(json.dumps([normalize(d['brand']),d['model'].casefold().strip(),'']).encode()).hexdigest()[:20];r=con.execute('SELECT * FROM devices WHERE id=?',(did,)).fetchone()
  if r:existingids.add(did);assert r['brand']==d['brand'] and r['model'].casefold()==d['model'].casefold()
  else:newids.add(did)
con.close();assert len(newids)==35 and len(existingids)==6 and len(sourceids)==4 and pages==6 and len(newpdfs)==2 and len(deps)==1
assert deps[0]['path']=='snapshots/parent-product.html' and deps[0]['sha256']=='d5b0fd941c02b83842157ce5e9a4ad8e188fe5abe4bd6e9e07a57fe927061455'
records=sum(len(p['devices']) for p in payloads);facts=sum(len(d['facts']) for p in payloads for d in p['devices']);assert records==41 and facts==235
expected={**counts,'devices':counts['devices']+len(newids),'records':counts['records']+records,'facts':counts['facts']+facts,'sources':counts['sources']+len(sourceids)+len(newpdfs),'documents':counts['documents']+len(newpdfs),'document_pages':counts['document_pages']+pages};assert expected=={'devices':5157,'records':7302,'facts':74837,'sources':2121,'documents':72,'document_pages':3804}
for key,newvalues,oldvalues in [('id',[p['id'] for p in newpdfs],[e['id'] for e in old['documents']]),('url',[p['url'] for p in newpdfs],[e['url'] for e in old['documents']]),('sha256',[p['pdf'] for p in newpdfs],[e['pdf']['sha256'] for e in old['documents']])]:assert len(set(newvalues))==2 and not set(newvalues)&set(oldvalues)
newpins={str(Path(p['dest']).relative_to('data')) for p in plans};newpins.add('raw/reference-documents-with-wave5-2026-10-12.json');assert len(newpins)==14 and not newpins&set(manifest['files']);assert len(manifest['files'])+len(newpins)==3984 and len(manifest['imports'])+2==152
for p in ['devices-candidate-wave5-2026-10-12.sqlite','import-manifest-candidate-wave5-2026-10-12.json','raw/reference-documents-with-wave5-2026-10-12.json']:assert not (DATA/p).exists()
assert not (I/'stage-report.json').exists();assert sha(script)==frozen;assert live=={name:sha(DATA/name) for name in live}
proof={'status':'PASS final frozen unexecuted stage helper; actual stage/rebuild remain pending','checked_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'stage_helper_sha256':frozen,'review_hashes_bound':reviews,'input_hashes':hashes,'copy_plan':plans,'auxiliary_evidence_dependencies_verified':deps,'frozen_code_files_verified':209,'pure_preparation_groups':2,'records':records,'facts':facts,'new_ids':sorted(newids),'existing_ids':sorted(existingids),'roles':{str(k):v for k,v in roles.items()},'expected_counts':expected,'bundle':{'old_entries':59,'new_entries':61,'old_pages':3724,'new_pages':3730},'manifest_expected_jobs':152,'manifest_expected_inputs':3984,'new_pins':sorted(newpins),'live_pair_unchanged':live,'limits':'Static helper+accepted input review/pure preparation only; no stage/import/temp DB/bundle/manifest creation, network, code or shared writes.'}
with (R/'wave5-stage-helper-check.json').open('x') as f:json.dump(proof,f,indent=2);f.write('\n')
print('PASS frozen helper41records235facts35new61bundle152jobs3984pins; QCT parent copied/pinned')
