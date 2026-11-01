"""Read-only candidate audit; writes only this batch's audit proof."""
import ast,collections,hashlib,json,re,sqlite3
from pathlib import Path
from devicehunt import validation
from devicehunt.catalog import normalize
ROOT=Path.cwd();B=ROOT/'research/batches/server-evidence-2026-10-04';O=B/'integration'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def connect(name):
 c=sqlite3.connect((ROOT/name).as_uri()+'?mode=ro',uri=True);c.row_factory=sqlite3.Row;return c
def count(c,t,cols=None):
 fields=','.join('"'+x+'"' for x in cols) if cols else '*'
 return collections.Counter(tuple(r) for r in c.execute('SELECT '+fields+' FROM '+t))
# Match exact script's43 logical tables without executing mutation/report code.
script=(ROOT/'scripts/compare_rebuild.py').read_text()
match=re.search(r"tables=('.*?')\.split\(\)",script);tables=ast.literal_eval(match[1]).split()+['inspur_support_entries'];assert len(tables)==43
old=connect('data/devices-before-server-evidence-2026-10-04.sqlite');new=connect('data/candidate-with-server-evidence-2026-10-04.sqlite');rebuiltpath=ROOT/'data/rebuilt-with-server-evidence-2026-10-04.sqlite'
reb=connect(str(rebuiltpath)) if rebuiltpath.exists() else None
bundle=json.loads((O/'old-complete-bundle.json').read_text());allowed={'reference-pdf-'+e['id'] for e in bundle['documents']};assert len(allowed)==45
preservation={};comparison={}
for t in tables:
 cols=[r['name'] for r in old.execute('PRAGMA table_info('+t+')')]
 if t=='sources':
  oldrows={r['id']:dict(r) for r in old.execute('SELECT * FROM sources')};newrows={r['id']:dict(r) for r in new.execute('SELECT * FROM sources')}
  changes={}
  for sid,row in oldrows.items():
   changed={k for k in row if row[k]!=newrows[sid][k]}
   assert changed <= ({'sha256','cache_path','fetched_at'} if sid in allowed else set()),(sid,changed)
   if changed:changes[sid]=sorted(changed)
  preservation[t]={'old':len(oldrows),'preserved':len(oldrows),'allowed_archive_metadata_changes':changes}
 elif t=='search_index':
  # Derived text changes separately checked by source reconstruction below.
  oldkeys={r[0] for r in old.execute('SELECT device_id FROM search_index')};newkeys={r[0] for r in new.execute('SELECT device_id FROM search_index')};assert oldkeys<=newkeys
  preservation[t]={'old_device_keys_preserved':len(oldkeys),'new_keys':len(newkeys-oldkeys)}
 else:
  a=count(old,t);b=count(new,t);assert not a-b,(t,list((a-b).items())[:2])
  preservation[t]={'old':sum(a.values()),'new':sum(b.values()),'missing_old_multiplicity':0}
 if reb:
  logical=[x for x in cols if x not in ('observed_at','fetched_at','cache_path')]
  if t in ('search_index','document_search'):logical=['rowid']+logical
  a=count(new,t,logical);b=count(reb,t,logical);assert a==b,(t,sum((a-b).values()),sum((b-a).values()))
  comparison[t]={'candidate':sum(a.values()),'rebuilt':sum(b.values()),'equal_duplicate_sensitive':True}
fts={}
for label,c in [('old',old),('candidate',new)]+([('rebuild',reb)] if reb else []):
 expected=[]
 for d in c.execute('SELECT * FROM devices ORDER BY id').fetchall():
  parts=[d['title'],d['brand'],d['model'],d['kind'],d['revision']]
  for table,expr in [('affiliations',"company || ' ' || role"),('identifiers','value'),('facts','value'),('hyperscaler_evidence',"operator || ' ' || relationship || ' ' || detail"),('fcc_exhibits',"title || ' ' || document_type || ' ' || availability")]:
   parts.extend(r[0] for r in c.execute(f'SELECT {expr} FROM {table} x JOIN records r ON r.id=x.record_id WHERE r.device_id=? ORDER BY r.id,1',(d['id'],)))
  text=' '.join(parts);text+=' '+' '.join(re.findall(r'\d+[x×](\d+(?:\.\d+)?[kmg](?:be?)?)',text,re.I));expected.append((d['id'],text))
 assert collections.Counter(expected)==count(c,'search_index')
 assert count(c,'document_search')==count(c,'document_pages',['document_key','page_number','text'])
 fts[label]={'device_search_reconstructs':True,'document_search_reconstructs':True}
for entry in bundle['documents']:
 for key in ['pdf','text']:assert sha(ROOT/'data/raw'/entry[key]['path'])==entry[key]['sha256']
prov=json.loads((ROOT/'data/raw/ingrasys-servers-reconstructed-2026-10-04-provenance.json').read_text())
body=(ROOT/prov['original_snapshot']).read_bytes();start,stop=prov['source_span_half_open']
assert body[start:stop]+bytes.fromhex(prov['appended_bytes_hex'])==(ROOT/'data/raw/ingrasys-servers-reconstructed-2026-10-04.pdf').read_bytes()
assert sha(ROOT/prov['original_snapshot'])==prov['original_sha256']
oldfts=dict(old.execute('SELECT device_id,text FROM search_index'));newfts=dict(new.execute('SELECT device_id,text FROM search_index'))
changes=[k for k,v in oldfts.items() if newfts[k]!=v]
changedmodels=[dict(new.execute('SELECT brand,model FROM devices WHERE id=?',(k,)).fetchone()) for k in changes]
assert sorted((r['brand'],r['model']) for r in changedmodels)==[('Ingrasys','SV2221A'),('Ingrasys','SV2221I'),('Oracle','Oracle Server E4-2C')],changedmodels
newdoc='reference-pdf-ingrasys-servers-reconstructed-2026-10-04'
blank=[dict(r) for r in new.execute('SELECT p.* FROM document_pages p JOIN documents d USING(document_key) WHERE d.source_id=? ORDER BY page_number',(newdoc,))]
assert len(blank)==10 and [r['page_number'] for r in blank]==list(range(1,11)) and all(r['text']=='' and r['text_status']=='no_embedded_text' for r in blank)
newrows=json.loads((ROOT/'data/server-evidence-facts-2026-10-04.json').read_text())
sourceproof={}
for source in newrows['sources']:
 sid=source['id'];rows=[r for r in newrows['devices'] if r['source']==sid]
 assert new.execute('SELECT records_seen FROM sources WHERE id=?',(sid,)).fetchone()[0]==len(rows)
 for r in rows:
  rid=hashlib.sha256(json.dumps([sid,r['key']]).encode()).hexdigest()[:24]
  actual=new.execute('SELECT * FROM records WHERE id=?',(rid,)).fetchone();assert json.loads(actual['raw_json'])==r
  actualfacts=collections.Counter((x[0],x[1]) for x in new.execute('SELECT name,value FROM facts WHERE record_id=?',(rid,)))
  expectedfacts=collections.Counter(set((n,str(v)) for n,v in r['facts'] if v is not None and str(v)));assert actualfacts==expectedfacts
  roles={(x[0],x[1]) for x in new.execute('SELECT company,role FROM affiliations WHERE record_id=?',(rid,))};assert roles==set(map(tuple,r['affiliations']))
 sourceproof[sid]={'records':len(rows),'literal_raw_facts_roles_equal':True}
for model in ['SV1120','SV1121','SV2121','SV2221']:
 assert new.execute('SELECT count(*) FROM records r JOIN devices d ON d.id=r.device_id WHERE d.model=?',(model,)).fetchone()[0]==old.execute('SELECT count(*) FROM records r JOIN devices d ON d.id=r.device_id WHERE d.model=?',(model,)).fetchone()[0]
base=validation.run(new,ROOT/'validation/owned-hardware.json');previous=validation.run(old,ROOT/'validation/owned-hardware.json')
for k in ['summary','results','search_summary']:assert base[k]==previous[k],k
byte=validation.run(new,ROOT/'validation/vendor-examples.json'); assert byte['summary']=={'total':2,'hits':0,'gaps':2}
print(json.dumps(byte['summary']))
for c in [new]+([reb] if reb else []):assert [r[0] for r in c.execute('PRAGMA integrity_check')]==['ok'];assert not c.execute('PRAGMA foreign_key_check').fetchall()
proof={'phase':'candidate/rebuild ready for independent QA; not promoted','preservation':preservation,'duplicate_sensitive_43_table_comparison':comparison,'fts':fts,'intentionally_changed_old_device_fts_models':changedmodels,'derivative_blank_pages':blank,'source_rows_literal_proof':sourceproof,'owned_baseline':base,'bytedance_examples':byte,'old_pdf_dependency_hashes':{entry[k]['path']:sha(ROOT/'data/raw'/entry[k]['path']) for entry in bundle['documents'] for k in ['pdf','text']},'candidate_sha256':sha(ROOT/'data/candidate-with-server-evidence-2026-10-04.sqlite'),'rebuild_sha256':sha(rebuiltpath) if reb else None,'live_sha256':sha(ROOT/'data/devices.sqlite'),'manifest_sha256':sha(ROOT/'data/import-manifest.json'),'integrity_and_foreign_keys':'pass'}
(O/'candidate-verification.json').write_text(json.dumps(proof,indent=2,ensure_ascii=False)+'\n')
print('old evidence preserved;43 Counter tables equal' if reb else 'old evidence preserved; rebuild pending')
