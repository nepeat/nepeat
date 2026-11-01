import hashlib,json,subprocess,sys
from pathlib import Path
ROOT=Path.cwd();O=ROOT/'research/batches/server-evidence-2026-10-04/integration';candidate=ROOT/'data/candidate-with-server-evidence-2026-10-04.sqlite';live=ROOT/'data/devices.sqlite'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
before={'candidate':sha(candidate),'live':sha(live)};results=[];reports={}
def run(args,key,expected=0):
 cmd=[sys.executable,'-B','-m','devicehunt','--db',str(candidate)]+args
 p=subprocess.run(cmd,text=True,capture_output=True);(O/(key+'.json')).write_text(p.stdout);(O/(key+'.stderr')).write_text(p.stderr)
 assert p.returncode==expected,(cmd,p.returncode,p.stderr)
 d=json.loads(p.stdout);results.append({'command':cmd,'returncode':p.returncode,'output':str(O/(key+'.json'))});reports[key]=d;return d
vendorid='ingrasys-server-catalog-recovered-pages-2026-10-04';portfolio='ingrasys-recovered-variants-foxconn-portfolio-2026-10-04'
models=['SV1120A','SV1120I','SV1121A','SV1121I','SV2121A','SV2121I','SV2221A','SV2221I']
for model in models:
 d=run(['buying-report',model,'--format','json'],'buying-'+model)
 rows=[r for r in d['matched_records'] if r['source_id']==vendorid];assert len(rows)==1 and rows[0]['model']==model and rows[0]['context_facts']
 # Unfiltered exact-model output legitimately includes separate portfolio record.
for model in ['SV1120','SV1121','SV2121','SV2221']:
 d=run(['buying-report',model,'--format','json'],'buying-'+model)
 assert not any(r['source_id'] in [vendorid,portfolio] for r in d['matched_records'])
for model in models[:6]:
 d=run(['buying-report',model,'--vendor','Foxconn','--format','json'],'foxconn-'+model)
 assert len(d['matched_records'])==1 and d['matched_records'][0]['source_id']==portfolio
 assert all(not cat['statements'] for cat in d['categories'].values()),model
oracle=run(['buying-report','Oracle Server E4-2C','--vendor','Oracle','--format','json'],'buying-oracle-e4-2c')
row=[r for r in oracle['matched_records'] if r['source_id']=='nist-4506-e4-2c-reviewed-detail-2026-10-04'];assert len(row)==1
curated=json.loads((ROOT/'data/server-evidence-facts-2026-10-04.json').read_text());r=[r for r in curated['devices'] if r['source']=='nist-4506-e4-2c-reviewed-detail-2026-10-04'][0]
assert sorted((f['name'],f['value']) for f in row[0]['context_facts'])==sorted(map(tuple,r['facts']))
statements=[s for cat in oracle['categories'].values() for s in cat['statements'] if s['fact_name']=='cmvp_tested_configuration' and s['source_id']==r['source']]
assert sorted(s['statement'] for s in statements)==sorted(v for n,v in r['facts'] if n=='cmvp_tested_configuration')
base=run(['validate-inventory','--format','json'],'owned-baseline');assert base['summary']=={'total':10,'hits':10,'gaps':0}
byte=run(['validate-inventory','--inventory','validation/vendor-examples.json','--format','json'],'bytedance-examples',2);assert byte['summary']=={'total':2,'hits':0,'gaps':2}
doc=run(['documents','','--source','reference-pdf-ingrasys-servers-reconstructed-2026-10-04','--format','json','--limit','-1'],'image-only-archive');assert doc['matches']==10 and all(r['text_status']=='no_embedded_text' for r in doc['rows'])
assert before=={'candidate':sha(candidate),'live':sha(live)}
(O/'cli-verification.json').write_text(json.dumps({'commands':results,'database_hashes_before_after':before,'read_only':True,'eight_variants_literal_vendor_context':True,'six_foxconn_records_no_borrowed_vendor_specs':True,'four_unsuffixed_no_variant_borrowing':True,'oracle_two_literal_configurations_full_context':True,'ten_blank_pages_not_missing':True},indent=2)+'\n')
print('real CLI checks pass:',len(results),'read-only commands')
