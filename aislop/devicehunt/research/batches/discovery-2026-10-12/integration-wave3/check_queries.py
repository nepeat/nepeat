"""Read-only exact-model and source-scope smoke checks on staged wave3."""
from pathlib import Path
import hashlib,json,sys
ROOT=Path(__file__).resolve().parents[4];sys.path.insert(0,str(ROOT))
from devicehunt.catalog import Catalog
from devicehunt import specifications,buying
p=ROOT/'data/devices-candidate-wave3-2026-10-12.sqlite';sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();before=sha(p)
c=Catalog(p,read_only=True);models=('SD6200','Wedge 100-32X','Sun ZFS Storage 7420','Sun ZFS Storage 7420 M2');out={}
try:
 for m in models:
  specs=specifications.search(c,m,exact_model=True,limit=-1)
  out[m]={'specifications':specs,'buying_report':buying.run(c.db,m)}
 m2=c.db.execute("SELECT f.value FROM facts f JOIN records r ON r.id=f.record_id JOIN devices d ON d.id=r.device_id WHERE d.brand='Oracle' AND d.model='Sun ZFS Storage 7420 M2'").fetchall()
 original=json.loads((ROOT/'data/raw/oracle7420-facts-wave3-2026-10-12/candidate.json').read_text())
 expected=next(r for r in original['devices'] if r['model']=='Sun ZFS Storage 7420 M2')['facts']
 assert len(m2)==len(expected)==1 and m2[0][0]==expected[0][1]
 for m in ('ZFS STORAGE 7420','S7420'):
  assert not c.db.execute("SELECT 1 FROM records r JOIN devices d ON d.id=r.device_id WHERE d.model=? AND r.source_id LIKE 'oracle7420-%-2026-10-12'",(m,)).fetchone()
 assert not c.db.execute("SELECT 1 FROM identifiers WHERE record_id IN (SELECT id FROM records WHERE source_id IN ('edgecore-wedge100-32x-r04-2026-10-12','celestica-sd6200-datasheet-2026-10-12'))").fetchone()
finally:c.db.close()
assert sha(p)==before
q=Path(__file__).with_name('model-query-check.json')
with q.open('x') as f:json.dump({'candidate_sha256_unchanged':before,'scope_checks':'M2 one reviewed literal fact only; no transfer to registry spellings; ordering rows not identifiers','queries':out},f,indent=2,ensure_ascii=False);f.write('\n')
print(json.dumps({m:len(r['specifications']) for m,r in out.items()}))
