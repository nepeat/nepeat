"""Read-only imported-fact/namespace and useful specification checks."""
from pathlib import Path
from collections import Counter
import json,sys
ROOT=Path(__file__).resolve().parents[4];sys.path.insert(0,str(ROOT))
from stage import CAND_DB,OUT,GROUPS,BATCH,sha,save
from devicehunt.catalog import Catalog
from devicehunt import specifications,buying

def main():
 before=sha(CAND_DB);c=Catalog(CAND_DB,read_only=True);checks=[];queries={}
 try:
  for group,(relative,wanted,review) in GROUPS.items():
   payload=json.loads((BATCH/relative/'candidate.json').read_text())
   for d in payload['devices']:
    rows=c.db.execute('SELECT r.*,v.brand,v.model FROM records r JOIN devices v ON v.id=r.device_id WHERE source_id=? AND source_key=?',(d['source'],d['key'])).fetchall()
    assert len(rows)==1;row=rows[0];rid=row['id']
    assert row['brand']==d['brand'] and row['model'].casefold()==d['model'].casefold()
    assert json.loads(row['raw_json'])==d
    assert Counter(tuple(r) for r in c.db.execute('SELECT name,value FROM facts WHERE record_id=?',(rid,)))==Counter(tuple(r) for r in d['facts'])
    assert Counter(tuple(r) for r in c.db.execute('SELECT company,role FROM affiliations WHERE record_id=?',(rid,)))==Counter(tuple(r) for r in d['affiliations'])
    assert Counter(tuple(r) for r in c.db.execute('SELECT kind,value FROM identifiers WHERE record_id=?',(rid,)))==Counter(tuple(r) for r in d.get('identifiers',[]))
    assert not c.db.execute('SELECT 1 FROM hyperscaler_evidence WHERE record_id=?',(rid,)).fetchone()
    checks.append({'source':d['source'],'key':d['key'],'device_id':row['device_id'],'stored_model':row['model'],'facts':len(d['facts'])})
  assert len(checks)==41 and sum(r['facts'] for r in checks)==235
  for model in ('Sun ZFS Storage 7420','AS6700-32X','AS6701-32X','AS5710-54X','AS5712-54X','QuantaGrid D52BQ-2U','PSU-AC-400-F','PSU-48V-400-B','PSU-12V-400-1','ORSA-1U'):
   queries[model]={'specifications':specifications.search(c,model,exact_model=True,include_source_facts=True,limit=-1),'buying_report':buying.run(c.db,model)}
  prohibited=('FAN-1U-1x1D-F','FAN-1U-1x1D-B','AN-1U-1x1D-F','AN-1U-1x1D-B','57xx-54X-SVC-RTF')
  for model in prohibited:
   assert not c.db.execute("SELECT 1 FROM records r JOIN devices d ON d.id=r.device_id WHERE r.source_id='edgecore-as5710-as5712-ds-r09-2026-10-12' AND d.model=?",(model,)).fetchone()
  assert not c.db.execute("SELECT 1 FROM records r JOIN devices d ON d.id=r.device_id WHERE r.source_id='oracle-sun-zfs-storage-7420-specifications-2026-10-12' AND d.model!='Sun ZFS Storage 7420'").fetchone()
 finally:c.db.close()
 assert sha(CAND_DB)==before
 save(OUT/'model-query-check.json',{'candidate_sha256_unchanged':before,'all_new_records_checked':checks,'scope':'Exact raw records/facts/identifiers/roles preserved, no hyperscaler assertions; source/model namespaces retained, disputed fan/service rows not hardware records, Oracle specifications not transferred to M2.','queries':queries})
 print(json.dumps({'records':len(checks),'facts':sum(r['facts'] for r in checks),'spec_counts':{k:len(v['specifications']) for k,v in queries.items()}}))
if __name__=='__main__':main()
