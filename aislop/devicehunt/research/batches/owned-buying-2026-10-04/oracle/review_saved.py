"""Read-only, finite Oracle saved-evidence review; writes only owned findings."""
import sqlite3,json,hashlib,re,html
from pathlib import Path
ROOT=Path(__file__).resolve().parents[4]
OUT=Path(__file__).parent
DB=ROOT/'data/devices.sqlite'
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def plain(p): return re.sub(r'\s+',' ',html.unescape(re.sub('<[^>]+>',' ',p.read_text())))
before=sha(DB)
c=sqlite3.connect(f'file:{DB}?mode=ro',uri=True);c.row_factory=sqlite3.Row
rra=json.loads((ROOT/'data/raw/rra-oracle-official/bundle.json').read_text())
results=[]
for model in ['E4-2C','E5-2C','X7-2C','X10-2C']:
 d=c.execute('select * from devices where lower(model)=?',('oracle server '+model.lower(),)).fetchone();assert d
 result={'model':model,'exact_match':dict(d),'scope':'saved captures only; no imports or network','records':[],'positive_statements':[],'unknowns':{'power':['PSU part numbers, count, input AC/DC range, connectors and derating','measured idle/load draw'], 'cooling':['airflow direction, fan dependencies, acoustic output and thermal limits'], 'chassis':['exact owned-unit chassis variant, rails and rack depth','disk backplane, risers and proprietary cabling dependencies'], 'boot_firmware':['owned-unit BIOS/ILOM version, credentials and locks','installable firmware access and supported flash/recovery procedure','standalone OS bootability of the particular liquidated unit'], 'licensing':['resale software entitlement, support transfer and activation conditions']},'excluded_near_models':['Oracle Server X7-2','Oracle Server E5-2L','unmapped Oracle EcoDesign regulatory codes','E4-2c 1U/2U and Enclave/Blockstore suffix variants','E5-2c 1U/Enclave suffix variants']}
 for r in c.execute('select r.*,s.cache_path,s.sha256,s.fetched_at from records r join sources s on s.id=r.source_id where device_id=?',(d['id'],)):
  rr=dict(r);raw=json.loads(rr.pop('raw_json'));facts=[dict(x) for x in c.execute('select name,value from facts where record_id=?',(r['id'],))];rr['facts_already_imported']=facts;rr['capture_verification']='no saved capture path; benchmark facts excluded from reviewed positive statements'
  if r['cache_path']:
   p=Path(r['cache_path']);assert sha(p)==r['sha256'];rr['capture_verification']='hash matches source registry';rr['captures']=[{'path':str(p.relative_to(ROOT)),'sha256':sha(p),'url':r['source_url']}]
   if r['source_id']=='broadcom-servers':
    p=ROOT/f"data/raw/broadcom-servers-detail-{r['source_key']}.json";detail=json.loads(p.read_text())['data'];assert detail==raw['detail'];rr['captures'].append({'path':str(p.relative_to(ROOT)),'sha256':sha(p),'url':r['source_url'],'field_ref':'$.data.details'})
    for i,a in enumerate(detail['details']):
     for j,sub in enumerate(a['subsections']):
      if sub['type']=='table':
       result['positive_statements'].append({'category':'boot_firmware','status':'newly reviewed unimported structured fact (already preserved in raw record JSON)','record_id':r['id'],'source_id':r['source_id'],'capture':rr['captures'][-1],'field_ref':f'$.data.details[{i}].subsections[{j}].fieldValues','statement':'Submitted certification BIOS rows, with no inferred BIOS/release pairing or firmware availability','rows':sub['fieldValues']})
   elif r['source_id'].startswith('rra-'):
    entry=next(x for x in rra['details'] if x['certificate']==raw['certificate_number']);p=ROOT/'data/raw/rra-oracle-official'/entry['path'];assert sha(p)==entry['sha256'];assert model.lower() in plain(p).lower();rr['captures'].append({'path':str(p.relative_to(ROOT)),'sha256':sha(p),'url':entry['url'],'field_ref':'main model / derived models / manufacturer fields'})
   else: assert model.lower() in plain(p).lower()
   result['positive_statements'].append({'category':'identity_and_source_scoped_facts','status':'already imported','record_id':r['id'],'source_id':r['source_id'],'capture_refs':rr['captures'],'facts':facts,'field_ref':'source heading/tested configuration or model-specific section; exact fact names above'})
  result['records'].append(rr)
 if model=='E4-2C':
  p=ROOT/'data/raw/nist-4506.html';s=plain(p)
  texts=['Windows Server 2022 running on Oracle E4-2C server with AMD EPYC 7J13 with PAA','Windows Server 2022 running on Oracle E4-2C server with AMD EPYC 7J13 without PAA (single-user mode)']
  items=[re.sub(r'\s+',' ',html.unescape(re.sub('<[^>]+>',' ',x))).strip() for x in re.findall(r'<li\b[^>]*>(.*?)</li>',p.read_text(),flags=re.S|re.I)]
  relevant=[x for x in items if 'Windows Server 2022 running on Oracle E4-2C server' in x]
  assert relevant==texts, (relevant,texts)
  result['positive_statements'].append({'category':'boot_firmware','status':'newly reviewed unimported structured facts','source_id':'nist-4506','record_id':'86f9ea91a2a9ed89732d4962','capture':{'path':str(p.relative_to(ROOT)),'sha256':sha(p),'url':'https://csrc.nist.gov/projects/cryptographic-module-validation-program/certificate/4506'},'field_ref':'Tested Configuration(s); Software Versions; Validation History','statement':texts,'limits':'software cryptographic-module tested configurations only; not general/current OS compatibility or unit boot proof'})
 if model=='X7-2C':
  p=ROOT/'data/raw/oracle-x7-2c-vpat.html';s=plain(p);assert '23-Aug-2017' in s and 'Oracle Server X7-2c SM 1U' in s and 'Oracle Server X7-2c SM 2U' in s
  variants=[]
  for start,end in [('Oracle Server X7-2c SM 1U','Oracle Server X7-2c SM 2U'),('Oracle Server X7-2c SM 2U','Date:')]:
   section=s[s.index(start):s.index(end,s.index(start)+len(start))];variants.append({'variant':start,'literal_product_description':section})
  result['positive_statements'].append({'category':'chassis_variant_and_software','status':'newly reviewed unimported; provenance incomplete, not import-ready','source_id':None,'record_id':None,'capture':{'path':str(p.relative_to(ROOT)),'sha256':sha(p),'url':None,'url_status':'capture URL not recorded in available registry/bundles/notes; do not invent'},'field_ref':'Name of Product; Product Description; Date; Dependent Products','date':'23-Aug-2017','variants':variants,'dependent_products_literal':s[s.index('Dependent Products'):s.index('Evaluation Methods Used:')],'limits':'dated accessibility product description; variants stay separate; watt values describe CPU limits, not PSU ratings or wall draw; dependency list is not OS compatibility/entitlement; source memory-channel wording preserved literally'})
 if model=='X10-2C':
  p=ROOT/'data/raw/oracle-x10-2c-license.html';s=plain(p);assert 'edk2-stable202208' in s and 'BSD-2-Clause-Patent' in s
  result['positive_statements'].append({'category':'licensing_notices','status':'newly reviewed unimported structured fact','source_id':'oracle-x10-license','record_id':'f528b460dd0073172c59b3cd','capture':{'path':str(p.relative_to(ROOT)),'sha256':sha(p),'url':'https://docs.oracle.com/en/engineered-systems/private-cloud-appliance/3.0-latest/licensing-3.0-latest/licensing-component-x10-2c-license.html'},'field_ref':'Oracle Technology Notices and Licenses; Third Party Notices and Licenses, Provider/Component(s)/Licensing Information table','statement':'Notices identify edk2-stable202208 and BSD-2-Clause-Patent; page links ILOM and Hardware Management Pack notices as technologies that may be included or distributed.','limits':'bundled component copyright/license notice does not establish installable BIOS source, firmware availability, current support or resale entitlement'})
 result['finite_next_leads']=['one exact-model service/installation manual or label-backed Oracle part-number mapping to resolve PSU, fans, rails and chassis','one exact-model BIOS/ILOM release/readme and public download-access evidence, without gated/private requests','one official licensing/support-transfer statement applicable to a secondhand unit']
 if model=='X7-2C':result['finite_next_leads'].insert(0,'recover original URL/capture metadata for existing VPAT from acquisition provenance before considering import')
 (OUT/f'{model.lower()}-findings.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n');results.append(result)
assert sha(DB)==before
(OUT/'verification.json').write_text(json.dumps({'models':len(results),'database_sha256_before':before,'database_sha256_after':sha(DB),'all_registered_capture_hashes_match':True,'broadcom_detail_matches_raw_record':True,'network_requests':0,'imports':0,'limitations':['VPAT original URL missing','benchmark capture absent; excluded from positive review']},indent=2)+'\n')
print('four findings written; registered hashes and Broadcom raw detail equality verified; database unchanged')
