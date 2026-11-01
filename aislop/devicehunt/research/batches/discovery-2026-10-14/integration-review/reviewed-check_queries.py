"""Read-only imported-fact/namespace and useful specification checks."""
from pathlib import Path
from collections import Counter
import json,sys
ROOT=Path(__file__).resolve().parents[4];sys.path.insert(0,str(ROOT))
from stage import CAND_DB,OUT,ROOT,sha,save
from devicehunt.catalog import Catalog
from devicehunt import specifications,buying

def main():
 before=sha(CAND_DB);c=Catalog(CAND_DB,read_only=True);checks=[];queries={}
 try:
  plan=json.loads((OUT/'accepted-inputs.json').read_text())
  for group in plan['groups']:
   payload=json.loads((ROOT/group['candidate']).read_text())
   for d in payload['devices']:
    rows=c.db.execute('SELECT r.*,v.brand,v.model FROM records r JOIN devices v ON v.id=r.device_id WHERE source_id=? AND source_key=?',(d['source'],d['key'])).fetchall()
    assert len(rows)==1;row=rows[0];rid=row['id']
    assert row['brand']==d['brand'] and row['model'].strip().casefold()==d['model'].strip().casefold()
    assert json.loads(row['raw_json'])==d
    assert Counter(tuple(r) for r in c.db.execute('SELECT name,value FROM facts WHERE record_id=?',(rid,)))==Counter(tuple(r) for r in d['facts'])
    assert Counter(tuple(r) for r in c.db.execute('SELECT company,role FROM affiliations WHERE record_id=?',(rid,)))==Counter(tuple(r) for r in d.get('affiliations',[]))
    assert Counter(tuple(r) for r in c.db.execute('SELECT kind,value FROM identifiers WHERE record_id=?',(rid,)))==Counter(tuple(r) for r in d.get('identifiers',[]))
    assert not c.db.execute('SELECT 1 FROM hyperscaler_evidence WHERE record_id=?',(rid,)).fetchone()
    checks.append({'source':d['source'],'key':d['key'],'device_id':row['device_id'],'stored_model':row['model'],'facts':len(d['facts'])})
  assert len(checks)==467 and sum(r['facts'] for r in checks)==5720
  for model in ('P8000G6','C2810Z5D2E3XD','Miramar -ICEMan','Miramar - ICEMan','EPC612D4I','D4040','AS5812-54X'):
   queries[model]={'specifications':specifications.search(c,model,exact_model=True,include_source_facts=True,limit=-1),'buying_report':buying.run(c.db,model)}
  # These imports add source associations; manufacturing/deployment inventories remain exact.
  from contextlib import closing
  import sqlite3
  with closing(sqlite3.connect((ROOT/'data/devices.sqlite').as_uri()+'?mode=ro',uri=True)) as old:
   for table,where in [('affiliations'," WHERE role='manufacturer'"),('hyperscaler_evidence','')]:
    assert Counter(tuple(r) for r in c.db.execute('SELECT * FROM '+table+where))==Counter(old.execute('SELECT * FROM '+table+where))
 finally:c.db.close()
 assert sha(CAND_DB)==before
 save(OUT/'query-check.json',{'candidate_sha256_unchanged':before,'all_new_records_checked':checks,'scope':'All 467 raw records, 5720 facts, identifiers and conservative source roles preserved. Raw identity strings remain in source JSON; catalog identity normalization unchanged. Manufacturing and hyperscaler rows unchanged.','queries':queries})
 print(json.dumps({'records':len(checks),'facts':sum(r['facts'] for r in checks),'spec_counts':{k:len(v['specifications']) for k,v in queries.items()}}))
if __name__=='__main__':main()
