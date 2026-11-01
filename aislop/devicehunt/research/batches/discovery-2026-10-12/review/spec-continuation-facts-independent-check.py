"""Independent bounded read-only SPEC candidate audit; output only to reviewer path."""
from pathlib import Path
from html.parser import HTMLParser
import collections, datetime, hashlib, json, re, sqlite3, subprocess, sys
sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parents[4]
B=ROOT/'research/batches/discovery-2026-10-12/spec-continuation-facts'
O=B.parent/'spec-continuation'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def norm(t):return ' '.join(t.split())
class Cells(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True);self.stack=[];self.rows=[];self.ignored=0
    def handle_starttag(self,t,a):
        if t in ('script','style'):self.ignored+=1
        if t=='tr':self.stack.append([])
        if t in ('td','th') and self.stack:self.stack[-1].append([])
        if t=='br' and self.stack and self.stack[-1]:self.stack[-1][-1].append(' ')
    def handle_endtag(self,t):
        if t in ('script','style'):self.ignored-=1
        if t=='tr' and self.stack:self.rows.append([norm(''.join(c)) for c in self.stack.pop()])
    def handle_data(self,t):
        if not self.ignored and self.stack and self.stack[-1]:self.stack[-1][-1].append(t)
def tree():return {str(p.relative_to(B)):sha(p) for p in B.rglob('*') if p.is_file()}
before=tree()
dbs=[ROOT/'data/devices.sqlite',ROOT/'data/devices-candidate-wave3-2026-10-12.sqlite']
dbhash={str(p.relative_to(ROOT)):sha(p) for p in dbs}
c=json.loads((B/'candidate.json').read_text());sources={s['id']:s for s in c['sources']}
obs={d['url']:d for d in json.loads((O/'observations.json').read_text())['observed_pages']}
req={r['url']:r for r in map(json.loads,(O/'requests.jsonl').read_text().splitlines())}
provenance=json.loads((B/'provenance.json').read_text());assert len(provenance['artifacts'])==18
for a in provenance['artifacts']:
 p=ROOT/a['original'];s=B/a['snapshot'];assert p.read_bytes()==s.read_bytes();assert sha(p)==a['sha256'];assert s.stat().st_size==a['bytes'];assert s.stat().st_mtime_ns==a['original_mtime_ns']
for line in (O/'SHA256SUMS').read_text().splitlines():
 h,name=line.split('  ',1);assert sha(O/name)==h
assert len(sources)==len(c['devices'])==18
counts=collections.Counter();reviewed=[]
label_to_name={'Vendor':'spec_hardware_vendor','Name':'spec_system_name','Model':'spec_model','Form Factor':'spec_form_factor','CPU Name':'spec_cpu_name','CPU Characteristics':'spec_cpu_characteristics','CPU Frequency (MHz)':'spec_cpu_frequency_mhz','Nodes Per System':'spec_nodes_per_system','Number of Systems':'spec_number_of_systems','Chips Per System':'spec_chips_per_system','Cores Per System':'spec_cores_per_system','Cores Per Chip':'spec_cores_per_chip','Threads Per System':'spec_threads_per_system','Memory Amount (GB)':'spec_memory_amount_gb','# and size of DIMM(s)':'spec_number_and_size_of_dimms','Memory Details':'spec_memory_details','Power Supply Quantity and Rating (W)':'spec_power_supply_quantity_and_rating_w','Available':'spec_available'}
for d in c['devices']:
 s=sources[d['source']];p=B/s['snapshot'];raw=obs[s['url']];assert d['raw_reviewed_observation']==raw
 assert p.resolve().is_relative_to(B.resolve());assert sha(p)==s['original_sha256']==raw['capture_sha256'];assert s['tier']=='vendor'
 assert s['captured_at_utc']==req[s['url']]['finished_at_utc'];assert req[s['url']]['status']==200
 assert d['brand']=='Inspur Corporation' and d['affiliations']==[['Inspur Corporation','brand']]
 assert d['identifiers']==[['spec_system_name',d['key']]] and ['spec_system_name',d['key']] in d['facts']
 assert not d.get('hyperscaler');assert d['kind']=='server';assert len(d['facts'])==len(d['evidence'])==len(set(map(tuple,d['facts'])))
 facts=dict(d['facts']);assert facts['spec_test_sponsor']==facts['spec_hardware_vendor']=='Inspur Corporation'
 iscpu=p.suffix=='.txt';counts['cpu2017' if iscpu else 'jbb2015']+=len(d['facts'])
 if iscpu:
  lines=p.read_text().splitlines();assert not re.search(r'^\s*POWER-RELEVANT HARDWARE\s*$',p.read_text(),re.M)
  assert not any('power_supply' in n for n in facts)
  printed=raw['saved_index_row']['hw_model'];assert d['key']==printed
  header=norm(' '.join(lines[1:lines.index('')]));assert header=='Inspur Corporation '+printed
  removed=re.search(r'\s*\(([^()]*)\)\s*$',printed);assert removed and re.search(r'Intel|AMD|Xeon|EPYC',removed[1]);assert d['model']==printed[:removed.start()]
  assert d['model'].startswith('Inspur ')
  hw=next(i for i,l in enumerate(lines) if l.strip()=='HARDWARE');sw=next(i for i,l in enumerate(lines) if l.strip()=='SOFTWARE');assert hw<sw
 else:
  parser=Cells();parser.feed(p.read_text());rows=parser.rows
  blocks=[i for i,r in enumerate(rows) if len(r)==1 and re.fullmatch(r'Hardware hw_\d+',r[0])];assert len(blocks)==1 and rows[blocks[0]]==['Hardware hw_1']
  assert d['model']==d['key'];assert facts['spec_jbb_mode']=='MultiJVM'
 for idx,((name,value),e) in enumerate(zip(d['facts'],d['evidence'],strict=True)):
  assert e['fact_index']==idx and e['fact_name']==name and e['source_url']==s['url'] and e['capture_sha256']==sha(p)
  assert value not in ('None','Not Run','--','');q=e['literal_quote'];loc=e['locator']
  if name=='spec_result_url':
   assert value==q==s['url'] and loc=={'request_sequence':req[s['url']]['sequence'],'request_field':'url'};assert e['evidence_kind']=='saved request input, not document quote';counts['request_locators']+=1
  elif iscpu:
   assert q=='\n'.join(lines[n-1] for n in loc['lines_1based']);counts['source_quotes']+=1
   if loc.get('section')=='HARDWARE':
    assert all(hw<n-1<sw for n in loc['lines_1based']);v=norm(q.split(':',1)[1]);assert value==v
   if name.startswith('spec_result_'):
    metric,v=q.strip().split(None,1);assert value==v.strip();assert name=='spec_result_'+re.sub('[^a-z0-9]+','_',metric.lower()).strip('_')
  else:
   if 'header_label' in loc:
    cell=next(cell for row in rows[:3] for cell in row if cell.startswith(loc['header_label']+':'));assert q==cell;assert value==cell.split(':',1)[1].strip()
   else:
    row=rows[loc['table_row_index_zero_based']];cell=row[loc.get('cell',1)];assert q in cell
    if loc.get('section')=='Hardware hw_1':
     assert blocks[0]<loc['table_row_index_zero_based'];assert label_to_name[row[0]]==name;assert value==q==cell
   counts['source_quotes']+=1
  assert norm(value) in norm(q)
 reviewed.append({'source':d['source'],'printed':d['key'],'model':d['model'],'facts':len(d['facts']),'metrics':[f for f in d['facts'] if f[0].startswith('spec_result_') and f[0]!='spec_result_url'],'power':[f for f in d['facts'] if 'power_supply' in f[0]]})
assert counts=={'jbb2015':162,'cpu2017':204,'source_quotes':348,'request_locators':18}
routing=[]
for path in dbs:
 con=sqlite3.connect(path.resolve().as_uri()+'?mode=ro',uri=True);con.row_factory=sqlite3.Row
 existing=set();new=set();cases=[]
 for d in c['devices']:
  did=hashlib.sha256(json.dumps([re.sub('[^a-z0-9]','',d['brand'].lower()),d['model'].casefold().strip(),'']).encode()).hexdigest()[:20]
  assert did==d['printed_identity_mapping']['device_id'];r=con.execute('SELECT * FROM devices WHERE id=?',(did,)).fetchone()
  assert not con.execute('SELECT 1 FROM sources WHERE id=?',(d['source'],)).fetchone()
  if r:
   assert r['brand']==d['brand'] and r['model'].casefold()==d['model'].casefold();existing.add(did)
   if r['model']!=d['model']:cases.append({'source':d['source'],'printed_model':d['model'],'catalog_model':r['model'],'id':did})
  else:new.add(did)
 con.close();assert len(existing)==8 and len(new)==4 and len(cases)==3 and len({x['id'] for x in cases})==2
 routing.append({'path':str(path.relative_to(ROOT)),'existing':sorted(existing),'new':sorted(new),'case_differences':cases})
run=subprocess.run([sys.executable,'-B',str(B/'build_candidate.py'),'--check'],cwd=ROOT,capture_output=True,text=True,timeout=60)
assert run.returncode==0,(run.stdout,run.stderr);assert before==tree();assert dbhash=={str(p.relative_to(ROOT)):sha(p) for p in dbs}
report={'verdict':'PASS candidate audit; no import','checked_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'candidate_sha256':sha(B/'candidate.json'),'counts':dict(counts),'records':reviewed,'routing':routing,'snapshot_copies_verified':18,'original_artifact_hashes_verified':len((O/'SHA256SUMS').read_text().splitlines()),'builder_check':{'returncode':run.returncode,'stdout':run.stdout,'stderr':run.stderr,'candidate_tree_unchanged':True},'live_and_staged_database_hashes_unchanged':dbhash,'input_hashes':before}
out=Path(__file__).with_name('spec-continuation-facts-independent-checks.json')
with out.open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print('PASS:',dict(counts),'18 snapshots, 12 identities, 8 existing / 4 new; no imports or source writes')
