#!/usr/bin/env python3
"""Record private offline acceptance, including all acquisition receipt hashes."""
import json
from pathlib import Path
import subprocess
import sys
from extract_bulk import sha,dump
BASE=Path(__file__).resolve().parent

def main():
    receipts=[json.loads(s) for s in (BASE/'requests.jsonl').read_text().splitlines()]
    inputs=[]
    for r in receipts:
        for name,hashkey in [('body_path','body_sha256'),('header_path','header_sha256')]:
            p=BASE/r[name];actual=sha(p.read_bytes());expected=r[hashkey]
            if actual!=expected:raise ValueError('immutable receipt input changed: '+str(p))
            inputs.append({'path':r[name],'sha256':actual,'bytes':p.stat().st_size})
    run=subprocess.run([sys.executable,'-B',str(BASE/'test_bulk.py')],capture_output=True,text=True)
    report={'network_requests_in_verification':0,'acquisition_exchange_count':len(receipts),
            'immutable_capture_inputs_verified':inputs,'tests':{'argv':[sys.executable,'-B',str(BASE/'test_bulk.py')],
            'returncode':run.returncode,'stdout':run.stdout,'stderr':run.stderr},
            'production_paths_written':[]}
    dump(BASE/'checks.json',report)
    print(json.dumps({'receipt_inputs_verified':len(inputs),'tests_returncode':run.returncode}))
    raise SystemExit(run.returncode)
if __name__=='__main__':main()
