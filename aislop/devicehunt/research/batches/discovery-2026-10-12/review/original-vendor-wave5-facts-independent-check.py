"""Independent finite source/row/routing audit; output only to reviewer directory."""
from pathlib import Path
from html.parser import HTMLParser
from collections import Counter
import datetime,hashlib,json,re,sqlite3,subprocess,sys
from devicehunt import documents
from devicehunt.catalog import normalize
ROOT=Path.cwd();B=ROOT/'research/batches/discovery-2026-10-12';C=B/'original-vendor-wave5-facts';O=B/'original-vendor-wave5';R=B/'review'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def tree():return {str(p.relative_to(C)):sha(p) for p in C.rglob('*') if p.is_file()}
def norm(t):return ' '.join(t.split())
class Article(HTMLParser):
 def __init__(self):super().__init__(convert_charrefs=True);self.active=False;self.frames=[];self.blocks=[];self.parts=[]
 def handle_starttag(self,tag,attrs):
  if tag=='article':self.active=True
  if self.active and tag in ('h2','p','li','tr'):self.frames.append({'tag':tag,'parts':[]})
  if self.active and tag=='br':
   self.parts.append(' ')
   for frame in self.frames:frame['parts'].append(' ')
 def handle_data(self,data):
  if self.active:
   self.parts.append(data)
   for frame in self.frames:frame['parts'].append(data)
 def handle_endtag(self,tag):
  if self.active and tag in ('h2','p','li','tr'):
   assert self.frames and self.frames[-1]['tag']==tag;frame=self.frames.pop();self.blocks.append({'tag':tag,'text':norm(''.join(frame['parts']))})
  if tag=='article':self.active=False
before=tree();candidate=json.loads((C/'candidate.json').read_text());assert sha(C/'candidate.json')=='687da4d58027a507cce2f2f4d31225548d3a57fd73ec6a12be5605c4fe5de86b'
findings=json.loads((O/'findings.json').read_text());obs={o['id']:o for o in findings['observations']};rows=findings['ordering_rows'];assert len(obs)==36 and len(rows)==38
for line in (C/'SHA256SUMS').read_text().splitlines():h,name=line.split('  ',1);assert sha(C/name)==h
manifest=json.loads((O/'artifact-manifest.json').read_text())['files'];assert len(manifest)==50
for f in manifest:assert sha(O/f['path'])==f['sha256'] and (O/f['path']).stat().st_size==f['bytes']
prov=json.loads((C/'provenance.json').read_text());assert len(prov['copies'])==5
for copy in prov['copies']:
 original=ROOT/copy['original'];snapshot=C/copy['snapshot'];assert original.read_bytes()==snapshot.read_bytes();assert sha(snapshot)==copy['sha256'] and snapshot.stat().st_size==copy['bytes'];assert snapshot.stat().st_mtime_ns==copy['original_mtime_ns']
for name,h in prov['accepted_review_and_boundary'].items():assert sha(R/name)==h
sources={s['id']:s for s in candidate['sources']};assert len(sources)==3;pages={}
requests=list(map(json.loads,(O/'requests.jsonl').read_text().splitlines()))
for sid,s in sources.items():
 assert s['tier']=='vendor' and sha(C/s['snapshot'])==s['original_sha256'];original=findings['sources'][Path(s['snapshot']).name];assert s['url']==original['url'] and s['captured_at_utc']==original['captured_at_utc']
 assert (C/s['snapshot']).resolve().is_relative_to(C.resolve())
 if 'text_sidecar' in s:
  captured,metadata=documents.read_capture((C/s['text_sidecar']).read_bytes(),s['original_sha256']);assert metadata==s['extraction_metadata'];pages[sid]={p['page']:p['text'].splitlines() for p in captured};assert metadata['extractor_version']=='26.06.0' and metadata['extraction_options']['remove_hyphens']=='none'
assert sorted(len(v) for v in pages.values())==[2,4]
article=Article();article.feed((O/'oracle7420-specifications.html').read_text());articletext=norm(' '.join(article.parts));assert article.blocks[0]['text']=='Sun ZFS Storage 7420 Specifications' and 'M2' not in articletext
counts=Counter();observed=set();base=[];ordering=[];checked=[]
for d in candidate['devices']:
 s=sources[d['source']];assert d['key']==d['model'] and d['title']==d['brand']+' '+d['model'];company='Oracle Corporation' if d['brand']=='Oracle' else 'Edgecore Networks Corporation';assert d['affiliations']==[[company,'documentation_vendor']];assert d['hyperscaler']==[]
 assert len(d['facts'])==len(d['evidence'])==len(set(map(tuple,d['facts'])))
 isorder='original_discovery_ordering_row' in d
 counts['ordering_records' if isorder else 'base_records']+=1;counts['ordering_facts' if isorder else 'base_facts']+=len(d['facts'])
 if isorder:
  row=d['original_discovery_ordering_row'];assert row in rows;actual=pages[d['source']][row['pdf_page']][row['line_1based']-1];assert actual==row['raw_line'];cells=re.split(r'\s{2,}',actual.strip());assert cells==row['raw_cells_from_embedded_text'];assert cells[0]==d['model'];assert d['identifiers']==[['part_number',d['model']]]
  ordering.append(d['model']);expected=[['ordering_row_as_printed',actual.strip()]]
  if row['row_scope']=='family_configuration_order_row':
   assert not d['model'].startswith('AS');assert d['kind']=='switch configuration';assert len(cells) in (4,5)
   heading=row['base_model_line_as_printed'];assert heading in [l.strip() for l in pages[d['source']][3]];assert heading.startswith('Base Model: AS'+d['model'].split('-O-',1)[0]+' ')
   expected += [['base_model_heading_as_printed',heading],['cpu_module_as_printed',cells[1]],['power_description',cells[2]],['cooling_description',cells[3]]]
   if len(cells)==5:expected.append(['power_cord_region_as_printed',cells[4]])
  else:
   expected.append(['physical_description' if d['model'] in ('RKIT-85-6712','ORSA-1U') else 'power_description',cells[1]])
   if d['model'].startswith('PSU-') and len(cells)==4:expected += [['cooling_description',cells[2]],['power_cord_scope_as_printed',cells[3]]]
  assert d['facts']==[[n,v+' [PDF p. '+str(row['pdf_page'])+']'] for n,v in expected]
  if d['model']=='PSU-12V-400-1':assert d['kind']=='power input unit FRU' and len(d['facts'])==2 and all('400W' not in v and '400 W' not in v for _,v in d['facts'])
 else:
  assert d['identifiers']==[];base.append(d['model'])
 for idx,((name,value),e) in enumerate(zip(d['facts'],d['evidence'],strict=True)):
  assert e['fact_index']==idx and e['fact_name']==name and e['source_url']==s['url'] and e['capture_sha256']==s['original_sha256'];loc=e['locator'];q=e['literal_quote'];counts['evidence']+=1
  if 'pdf_page' in loc:
   actual='\n'.join(pages[d['source']][loc['pdf_page']][n-1] for n in loc['lines_1based']);assert actual==q or actual.strip()==q
   if e.get('visual_original'):assert sha(ROOT/e['visual_original'])==e['visual_sha256']
   if 'complete_sidecar_sha256' in e:assert sha(C/s['text_sidecar'])==e['complete_sidecar_sha256']
  elif 'article_block_ordinal_zero_based' in loc:
   block=article.blocks[loc['article_block_ordinal_zero_based']];assert block['tag']==loc['tag'] and block['text']==q and loc['text_phrase'] in q
  else:assert loc['article_text_exact_phrase']==q and q in articletext
  if 'original_observation_id' in e:
   o=obs[e['original_observation_id']];assert e['original_discovery_observation']==o;observed.add(o['id'])
   if o['id']=='as5712-22':
    actual_lines=q.splitlines();left=[re.split(r'\s{2,}',l.strip())[0] for l in actual_lines];assert left[0].startswith(d['model']+':');assert value=='\n'.join(left)+' [PDF p. 2]'
   else:assert value.startswith(o['summary'])
   if o['id'] in ('oracle-33','oracle-34','oracle-35'):assert e['mandatory_qualifying_note']==obs['oracle-rating-note']['raw_text'] and 'note' in value.lower()
  checked.append({'model':d['model'],'fact_index':idx,'fact_name':name,'locator':loc})
assert counts=={'base_records':5,'base_facts':61,'ordering_records':35,'ordering_facts':166,'evidence':227};assert len(candidate['devices'])==40 and len({(d['brand'],d['model']) for d in candidate['devices']})==40 and observed==set(obs)
assert set(base)=={'Sun ZFS Storage 7420','AS6700-32X','AS6701-32X','AS5710-54X','AS5712-54X'}
excluded=candidate['excluded_ordering_rows'];assert len(excluded)==3 and all(r in rows for r in excluded);assert {r['raw_cells_from_embedded_text'][0] for r in excluded}=={'FAN-1U-1x1D-F','FAN-1U-1x1D-B','57xx-54X-SVC-RTF'};assert not set(ordering)&{r['raw_cells_from_embedded_text'][0] for r in excluded}
for d in candidate['devices']:
 if d['model']=='AS5710-54X':assert len(d['facts'])==7 and all('Cumulus' not in v and 'PicOS' not in v and 'Atom C2538' not in v for _,v in d['facts'])
 if d['model']=='AS5712-54X':assert len(d['facts'])==8 and all('P2041' not in v for _,v in d['facts'])
 if d['model'] in ('AS6700-32X','AS6701-32X'):assert len(d['facts'])==18
 if d['model']=='Sun ZFS Storage 7420':assert len(d['facts'])==10 and not any('M2' in v for _,v in d['facts'])
routing=[];dbhash={}
for path in [ROOT/'data/devices.sqlite',ROOT/'data/devices-candidate-wave4-2026-10-12.sqlite',ROOT/'data/devices-candidate-wave3-2026-10-12.sqlite']:
 beforedb=sha(path);con=sqlite3.connect(path.resolve().as_uri()+'?mode=ro',uri=True);con.row_factory=sqlite3.Row;existing=0;new=0;others=[]
 for s in sources:assert not con.execute('SELECT 1 FROM sources WHERE id=?',(s,)).fetchone()
 for d in candidate['devices']:
  did=hashlib.sha256(json.dumps([normalize(d['brand']),d['model'].casefold().strip(),'']).encode()).hexdigest()[:20];r=con.execute('SELECT * FROM devices WHERE id=?',(did,)).fetchone()
  if r:existing+=1;assert d['model'] in base and r['brand']==d['brand'] and r['model']==d['model']
  else:new+=1;assert d['model'] in ordering
  other=[dict(r) for r in con.execute('SELECT id,brand,model,kind FROM devices WHERE model=? COLLATE NOCASE AND brand<>?',(d['model'],d['brand']))]
  if other:others.append({'model':d['model'],'other_namespaces':other})
 con.close();assert existing==5 and new==35 and sha(path)==beforedb;dbhash[str(path.relative_to(ROOT))]=beforedb;routing.append({'path':str(path.relative_to(ROOT)),'existing':existing,'new':new,'other_namespaces_kept_separate':others})
run=subprocess.run([sys.executable,'-B',str(C/'build_candidate.py'),'--check'],cwd=ROOT,capture_output=True,text=True,timeout=60);assert run.returncode==0,(run.stdout,run.stderr);assert tree()==before
for path,h in dbhash.items():assert sha(ROOT/path)==h
proof={'status':'PASS all227 facts/source scopes/ordering rows/routing; candidate only','checked_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'candidate_sha256':sha(C/'candidate.json'),'counts':dict(counts),'observations_verified':36,'ordering_rows_accepted':35,'ordering_rows_excluded':3,'original_artifacts_verified':50,'copies_verified':5,'complete_pdf_pages':6,'all_six_original_PDF_pages_visually_inspected_by_reviewer':True,'routing':routing,'all227locators':checked,'candidate_file_hashes':before,'readonly_database_hashes_unchanged':dbhash,'builder_check':{'returncode':run.returncode,'stdout':run.stdout,'stderr':run.stderr,'candidate_tree_unchanged':True},'limits':'No import/temp DB/network/extraction/render/code/shared writes; saved original images inspected only.'}
with (R/'original-vendor-wave5-facts-independent-checks.json').open('x') as f:json.dump(proof,f,indent=2);f.write('\n')
print('PASS3sources40records227facts;61base166ownrow;5existing35new; buildercheck no writes')
