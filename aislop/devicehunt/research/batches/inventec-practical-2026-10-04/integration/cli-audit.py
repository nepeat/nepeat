import hashlib,json,subprocess,sys,collections
from pathlib import Path
R=Path.cwd();O=R/'research/batches/inventec-practical-2026-10-04/integration';cp=R/'data/candidate-with-inventec-practical-2026-10-04.sqlite'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
before={str(p):sha(p) for p in [cp,R/'data/devices.sqlite']};commands=[];reports={}
def run(args,key,expected=0,jsonout=True):
 cmd=[sys.executable,'-B','-m','devicehunt','--db',str(cp)]+args;p=subprocess.run(cmd,capture_output=True,text=True);(O/(key+'.json')).write_text(p.stdout);(O/(key+'.stderr')).write_text(p.stderr);assert p.returncode==expected,(cmd,p.returncode,p.stderr);commands.append({'command':cmd,'returncode':p.returncode});d=json.loads(p.stdout) if jsonout else None;reports[key]=d;return d
cur=json.loads((R/'data/inventec-practical-facts-2026-10-04.json').read_text())
for r in cur['devices']:
 model=r['model'];sid=r['source']
 for vendor in [[],['--vendor','Inventec']]:
  d=run(['buying-report',model,'--format','json']+vendor,model+('-Inventec' if vendor else ''))
  row=[x for x in d['matched_records'] if x['source_id']==sid];assert len(row)==1
  assert collections.Counter((f['name'],f['value']) for f in row[0]['context_facts'])==collections.Counter(map(tuple,r['facts']))
  assert {x['role'] for x in row[0]['company_roles']}=={'brand','platform_vendor'}
  manufacturer=[x for x in d['matched_records'] if x['source_id']=='inventec-dongguan-manufacturer-2020'];assert len(manufacturer)==1
  assert all(s['source_id']==sid for cat in d['categories'].values() for s in cat['statements'] if s['fact_name'] in {'power_supply','cooling_description','drive_bays','memory','expansion'})
 d=run(['buying-report',model,'--vendor','Foxconn','--format','json'],model+'-Foxconn');assert not d['matched_records']
for q in ['EnteiX','GolduckX','Horsea']:
 d=run(['buying-report',q,'--format','json'],'noalias-'+q);assert not any(x['source_id'] in [r['source'] for r in cur['devices']] for x in d['matched_records'])
run(['buying-report','Golduck','--vendor','NoSuchVendor','--format','json'],'unsupported-vendor',1,False)
d=run(['documents','','--source','reference-pdf-inventec-golduck-product-brief-v3-2020','--format','json','--limit','-1'],'golduck-archive');assert d['matches']==2 and all(x['text_status']=='embedded_text' for x in d['rows'])
a=run(['validate-inventory','--format','json'],'owned-baseline');assert a['summary']=={'total':10,'hits':10,'gaps':0}
b=run(['validate-inventory','--inventory','validation/vendor-examples.json','--format','json'],'bytedance-examples',2);assert b['summary']=={'total':2,'hits':0,'gaps':2}
assert before=={str(p):sha(p) for p in [cp,R/'data/devices.sqlite']}
(O/'cli-verification.json').write_text(json.dumps({'commands':commands,'database_hashes_before_after':before,'read_only':True,'literal_context_roles_and_noalias':'pass','archive_two_pages':'pass'},indent=2)+'\n')
(R/'data/exports/inventec-practical-2026-10-04-buying-reports.json').write_text(json.dumps(reports,indent=2,ensure_ascii=False)+'\n')
print('CLI pass',len(commands))
