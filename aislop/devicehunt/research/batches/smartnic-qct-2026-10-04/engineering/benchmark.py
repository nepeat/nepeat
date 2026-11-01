import hashlib,importlib.util,json,time,resource
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt import lookup
root=Path.cwd(); here=root/'research/batches/smartnic-qct-2026-10-04/engineering'
spec=importlib.util.spec_from_file_location('devicehunt.lookup_before',here/'before/lookup.py'); old=importlib.util.module_from_spec(spec);spec.loader.exec_module(old)
golden=[]
for p in sorted((root/'data/exports').glob('lookup-*.json')):
 obj=json.loads(p.read_text())
 if isinstance(obj,dict) and 'query' in obj and 'limit_per_dataset' in obj:golden.append((p.name,obj))
c=Catalog(root/'data/devices.sqlite',read_only=True)
start=time.perf_counter(); expected=[old.run(c.db,o['query'],o['limit_per_dataset']) for _,o in golden]; before=time.perf_counter()-start
assert all(o==r for (_,o),r in zip(golden,expected))
start=time.perf_counter(); actual=lookup.run_many(c.db,[(o['query'],o['limit_per_dataset']) for _,o in golden]);after=time.perf_counter()-start
for (name,g),e,a in zip(golden,expected,actual):
 if a!=e:(here/(name+'.actual')).write_text(json.dumps(a));raise AssertionError(name)
proof=dict(reports=len(golden),before_seconds=before,after_seconds=after,speedup=before/after,exact_equal=True,max_rss=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,database_sha256=hashlib.sha256((root/'data/devices.sqlite').read_bytes()).hexdigest())
(here/'benchmark.json').write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps(proof),flush=True)
