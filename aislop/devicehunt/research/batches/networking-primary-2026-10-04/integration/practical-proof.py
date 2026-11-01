"""Validate real unfiltered CLI outputs and record-local evidence boundaries."""
from collections import Counter
import hashlib
import json
from pathlib import Path
import subprocess
import sys
HERE=Path(__file__).resolve().parent
DB=Path('data/networking-primary-candidate-2026-10-04.sqlite')
def sha(p):
 with p.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
before=sha(DB)
payload=json.loads(Path('data/networking-primary-facts-2026-10-04.json').read_text())
proof={'database_sha256':before,'reports':[]}
for d,name,query in zip(payload['devices'],['as7772','oracle'],['AS7772-32X','Oracle Server X7-2C']):
 args=[sys.executable,'-m','devicehunt','--db',str(DB),'buying-report',query,'--format','json']
 actual=json.loads(subprocess.run(args,check=True,capture_output=True,text=True).stdout)
 saved=json.loads((HERE/(name+'-cli-report.json')).read_text())
 assert actual==saved
 record=[r for r in actual['matched_records'] if r['source_id']==d['source']]
 assert len(record)==1
 record=record[0]
 assert Counter((f['name'],f['value']) for f in record['context_facts'])==Counter(tuple(f) for f in d['facts'])
 assert Counter((a['company'],a['role']) for a in record['company_roles'])==Counter(tuple(a) for a in d['affiliations'])
 scoped={k:[s for s in v['statements'] if s['source_id']==d['source']] for k,v in actual['categories'].items()}
 assert not scoped['power'] and not scoped['cooling'] and not scoped['licensing']
 if name=='oracle':
  assert {s['statement'] for s in scoped['chassis_physical']}=={'SM 1U','SM 2U'}
  assert {s['fact_name'] for s in scoped['compute_network_storage']} >= {'sm_1u_processor_power_raw','sm_2u_processor_power_raw','sm_1u_memory_description','sm_2u_memory_description','sm_1u_drive_bays_description','sm_2u_drive_bays_description'}
  assert any(s['fact_name']=='software_evidence' for s in scoped['firmware_boot_software'])
 else:
  assert record['brand']=='' and record['company_roles']==[] and record['tier']=='community'
  assert {s['fact_name'] for s in scoped['firmware_boot_software']}=={'artifact_model_declaration','artifact_compatible_declaration','artifact_serial0_alias','artifact_partition_labels'}
  assert not scoped['compute_network_storage'] and not scoped['chassis_physical']
  filtered=json.loads(subprocess.run(args+['--vendor','Accton'],check=True,capture_output=True,text=True).stdout)
  assert not any(r['source_id']==d['source'] for r in filtered['matched_records'])
 proof['reports'].append({'query':query,'file':name+'-cli-report.json','sha256':sha(HERE/(name+'-cli-report.json')),'context_facts':len(d['facts']),'source_id':d['source'],'category_counts':{k:len(v) for k,v in scoped.items()}})
assert sha(DB)==before
proof['readonly_hash_unchanged']=True
with (HERE/'candidate-practical-proof.json').open('x') as stream:json.dump(proof,stream,indent=2);stream.write('\n')
print('real CLI output, all 35 facts, Oracle variants/TDP and community role boundary pass')
