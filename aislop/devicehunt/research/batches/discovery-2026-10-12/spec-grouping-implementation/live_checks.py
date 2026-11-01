"""Four finite, hash-bound read-only cases; no persistent baseline claim."""
import collections
import datetime
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
from unittest.mock import patch
import devicehunt
from devicehunt import specifications, __main__
from devicehunt.catalog import Catalog
OWN=Path(__file__).resolve().parent
ROOT=OWN.parents[3]
assert Path.cwd()==OWN
assert Path(os.environ['PYTHONPATH']).resolve()==OWN/'overlay'
paths={m.__name__:str(Path(m.__file__).resolve()) for m in (devicehunt,specifications,__main__)}
assert all(Path(p).is_relative_to(OWN/'overlay') for p in paths.values())
spec=importlib.util.spec_from_file_location('devicehunt._parity_original', OWN/'originals/specifications.py')
legacy=importlib.util.module_from_spec(spec);spec.loader.exec_module(legacy)
db_path=ROOT/'data/devices.sqlite'
before=hashlib.sha256(db_path.read_bytes()).hexdigest()
results=[]
def reject(*a,**k):raise AssertionError('external access forbidden')
with patch('urllib.request.urlopen',reject),patch('socket.create_connection',reject),patch('subprocess.run',reject),patch('subprocess.Popen',reject):
    c=Catalog(db_path,read_only=True)
    try:
        for query,exact in [('NF5280M7',True),('NF5280',False),('AS7326-56X',True),('Sun ZFS Storage 7420',True)]:
            kwargs={'limit':-1,'exact_model':exact,'include_source_facts':True}
            flat=specifications.search(c,query,**kwargs)
            assert flat==legacy.search(c,query,**kwargs)
            groups=specifications.search(c,query,group_identical=True,**kwargs)
            if exact:
                wanted=query.strip().casefold()
                eligible={r['id'] for r in c.db.execute('SELECT r.id,d.model FROM records r JOIN devices d ON d.id=r.device_id') if r['model'].strip().casefold()==wanted}
                eligible.update(r['record_id'] for r in c.db.execute("SELECT record_id,value FROM identifiers WHERE kind IN ('alias','model_search','manufacturer_model')") if r['value'].strip().casefold()==wanted)
            else:
                ids={d['id'] for d in c.search(query,limit=-1)}
                eligible={r['id'] for r in c.db.execute('SELECT id,device_id FROM records') if r['device_id'] in ids}
            expected=collections.Counter()
            for r in c.db.execute('SELECT record_id,name,value FROM facts'):
                if r['record_id'] not in eligible:continue
                if r['name'] in specifications.CATEGORIES:
                    expected[(r['record_id'],r['name'],r['value'])]+=1
                elif specifications.source_fact_category(r['name']) and specifications.source_fact_statement(r['name'],r['value']) is not None:
                    expected[(r['record_id'],r['name'],r['value'])]+=1
            actual=collections.Counter((m['record_id'],m['raw_fact']['name'],m['raw_fact']['value']) for g in groups for m in g['evidence'])
            assert expected==actual,(query,len(expected),len(actual))
            assert specifications.search(c,query,group_identical=True,**(kwargs|{'limit':1}))==groups[:1]
            (OWN/(query.replace(' ','-')+'-live-output.json')).write_text(json.dumps({'flat':flat,'grouped':groups},ensure_ascii=False,indent=2)+'\n')
            results.append({'query':query,'exact_model':exact,'flat_rows':len(flat),'groups':len(groups),'evidence_rows':sum(actual.values()),'default_parity':True,'complete_raw_multiset_equal':True,'limit1_complete':True})
    finally:c.db.close()
after=hashlib.sha256(db_path.read_bytes()).hexdigest()
assert before==after
proof={'observed_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'database':str(db_path),'sha256_before':before,'sha256_after':after,'module_paths':paths,'cases':results}
(OWN/'live-checks.json').write_text(json.dumps(proof,indent=2)+'\n')
print(json.dumps(proof,indent=2))
