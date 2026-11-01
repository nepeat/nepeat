from pathlib import Path
import sqlite3,json,re,hashlib,sys
ROOT=Path.cwd(); OUT=ROOT/'research/batches/server-evidence-2026-10-04/oracle'
sys.path.insert(0,str(ROOT))
from devicehunt.oracle_evidence import plain
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
db=ROOT/'data/devices.sqlite'; before=sha(db)
p=ROOT/'data/raw/nist-4506.html'; body=p.read_text(); source_hash=sha(p)
c=sqlite3.connect(f'file:{db}?mode=ro',uri=True); c.row_factory=sqlite3.Row
source=dict(c.execute("select * from sources where id='nist-4506'").fetchone())
assert source['sha256']==source_hash
records=[dict(r) for r in c.execute("select * from records where source_id='nist-4506' order by source_key")]
for record in records:
 for table in ['facts','affiliations','identifiers']:
  record[table]=[dict(r) for r in c.execute(f'select * from {table} where record_id=? order by 1,2,3',(record['id'],))]
e4=next(r for r in records if r['source_key']=='Oracle Server E4-2C')
identity=dict(c.execute('select * from devices where id=?',(e4['device_id'],)).fetchone())
match=re.search(r'Tested Configuration\(s\)</span>.*?<ul\b[^>]*>(.*?)</ul>',body,re.S)
raw_rows=re.findall(r'<li\b[^>]*>(.*?)</li>',match[1],re.S)
rows=[plain(s) for s in raw_rows]; e4rows=[s for s in rows if 'Oracle E4-2C server' in s]
assert len(records)==4 and len(rows)==15 and len(e4rows)==2
assert json.loads(e4['raw_json'])['tested_configurations']==e4rows
history=re.search(r'<table[^>]*id="validation-history-table"[^>]*>(.*?)</table>',body,re.S).group(1)
historyrows=[[plain(x) for x in re.findall(r'<td[^>]*>(.*?)</td>',r,re.S)] for r in re.findall(r'<tr[^>]*>(.*?)</tr>',history,re.S) if '<td' in r]
facts=[['cmvp_tested_configuration',x] for x in e4rows]+[
 ['cmvp_module_name','Oracle OpenSSL FIPS Provider'],['cmvp_module_type','Software'],['cmvp_module_standard','FIPS 140-2'],
 ['cmvp_module_software_versions','3.0.15'],['cmvp_module_status_at_capture','Historical'],
 ['cmvp_module_historical_reason','Moved to historical list due to sunsetting'],
 ['cmvp_module_caveat','When operated in FIPS mode. No assurance of the minimum strength of generated keys.'],
 ['cmvp_module_vendor_literal','Oracle Corporation'],
 ['cmvp_module_validation_history_row',json.dumps(historyrows[0],ensure_ascii=False)],
 ['cmvp_module_validation_history_row',json.dumps(historyrows[1],ensure_ascii=False)],
 ['source_sha256',source_hash],['source_location','Details panel: #module-name; #module-standard; Tested Configuration(s) list items 14-15, HTML lines 583-584; Software Versions lines 685-689; #validation-history-table rows 1-2'],
 ['source_scope','Saved NIST CMVP certificate 4506 software-module tested-platform detail; two exact E4-2C rows, preserving PAA and single-user-mode modifiers. Status/date are snapshot/module statements.'],
 ['configuration_limit','No assertion of general/current OS support, installed CPU or OS, hardware certification, manufacturing, deployment, bootability, firmware access or secondhand software entitlement. Validation history dates are module events and are not separately assigned to either platform row.']]
sid='nist-4506-e4-2c-reviewed-detail-2026-10-04'
candidate={'sources':[{'id':sid,'url':source['url'],'tier':'regulatory','license':source['license'],'snapshot':'raw/nist-4506.html','coverage':'Additive reviewed detail from the existing immutable NIST 4506 capture; one exact Oracle E4-2C tested-platform identity. Preserves original nist-4506 source and all four records. Module vendor is not asserted as hardware manufacturer.'}],'devices':[{'source':sid,'key':'Oracle Server E4-2C tested software-module detail','brand':identity['brand'],'model':identity['model'],'title':identity['title'],'kind':identity['kind'],'affiliations':[['Oracle','tested_platform_vendor']],'facts':facts}]}
(OUT/'candidate-curated.json').write_text(json.dumps(candidate,indent=2)+'\n')
review={'outcome':'complete','network_requests':0,'capture':{'path':'data/raw/nist-4506.html','sha256':source_hash,'url':source['url'],'fetched_at':source['fetched_at']},'existing_source':source,'existing_records':records,'exact_identity':identity,'existing_vs_proposed':'Existing raw_json already contains both literal E4-2C rows; newly useful structured facts expose them and their software-module context without importing new identities or general OS support. Certificate number and original evidence_scope facts are not duplicated.','tested_configuration_rows':[{'ordinal':i+1,'raw_inner_html':raw,'text':row} for i,(raw,row) in enumerate(zip(raw_rows,rows))],'validation_history':{'html_id':'validation-history-table','raw_inner_html':history,'rows':historyrows},'proposed_fact_count':len(facts),'excluded':'Other platform rows retained for context only; no cross-platform ILOM/OS/CPU transfer. PAA remains literal, unexpanded. Oracle Corporation is the module vendor; tested_platform_vendor remains Oracle; no manufacturer/operator role added. No claim that module-wide version/status/date establishes availability or support of an auction unit.','checks':{'source_registry_hash_match':True,'existing_records':len(records),'literal_e4_rows_match_existing_raw_json':True,'all_platform_rows_preserved':len(rows),'exact_identity_match':True,'db_sha256_before':before}}
c.close(); after=sha(db); assert before==after
review['checks']['db_sha256_after']=after; review['checks']['database_unchanged']=True
(OUT/'review.json').write_text(json.dumps(review,indent=2)+'\n')
print(json.dumps({'facts':len(facts),'records':len(records),'sha256':source_hash,'database_unchanged':True}))
