"""Bind actual final acceptance and proof files; never promote by itself."""
from pathlib import Path
import ast, hashlib, json
ROOT=Path(__file__).resolve().parents[4]
OUT=ROOT/'research/batches/discovery-2026-10-12/integration-wave5'
DATA=ROOT/'data'
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for chunk in iter(lambda:f.read(1048576),b''):h.update(chunk)
 return h.hexdigest()
def main():
 review=OUT.parent/'review/wave5-integration-review.md'
 assert review.exists()
 text=review.read_text().lower()
 assert 'accept' in text
 # Evaluate only the reviewed helper's declarative required set, without invoking it.
 tree=ast.parse((OUT/'promote.py').read_text())
 fn=next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name=='main')
 node=next(n.value for n in fn.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='required' for t in n.targets))
 names=eval(compile(ast.Expression(node),'<reviewed required set>','eval'),{'OUT':OUT,'ROOT':ROOT,'str':str})
 names.update(str((OUT/n).relative_to(ROOT)) for n in ('check_inventory.py','inventory-check.json','rebuild.log'))
 names.update(str(p.relative_to(ROOT)) for p in (ROOT/'research/batches/discovery-2026-10-14/review').iterdir() if p.is_file())
 names.add(str(Path(__file__).relative_to(ROOT)))
 stage=json.loads((OUT/'stage-report.json').read_text())
 receipt={'accepted':True,'integrator':'/root','accepted_at':'2026-10-15 UTC (2026-10-14 PDT)',
 'review_path':str(review.relative_to(ROOT)),'review_sha256':sha(review),
 'candidate_database_sha256':stage['candidate_database_sha256'],
 'candidate_manifest_sha256':stage['candidate_manifest_sha256'],
 'proof_hashes':{n:sha(ROOT/n) for n in sorted(names)}}
 with (OUT/'promotion-acceptance.json').open('x') as f:json.dump(receipt,f,indent=2);f.write('\n')
 print(json.dumps({'receipt':str((OUT/'promotion-acceptance.json').relative_to(ROOT)),'proofs':len(names),'review_sha256':sha(review)}))
if __name__=='__main__':main()
