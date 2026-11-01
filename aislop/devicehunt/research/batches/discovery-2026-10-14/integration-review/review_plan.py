"""Independent concrete-plan preflight only; does not invoke stage.main/import/rebuild."""
import ast
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import shutil
import sqlite3
import sys
OUT=Path(__file__).resolve().parent;BASE=OUT.parent/'integration-breadth';ROOT=OUT.parents[3]
sys.path.insert(0,str(ROOT))
from devicehunt.catalog import digest,normalize
from devicehunt.importers import prepare_curated
from devicehunt.rebuild import read_manifest

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
    plan_path=BASE/'accepted-inputs.json';planbytes=plan_path.read_bytes();plan=json.loads(planbytes)
    plan_sha=hashlib.sha256(planbytes).hexdigest();assert plan_sha=='d486dba2b9de90c32b3a497015e2b5c103de32a10721f483dc0813c4d2e38627'
    assert plan['accepted'] is True and [g['name'] for g in plan['groups']]==['redhat','bisdn','odm']
    data=ROOT/'data';before={name:sha(data/name) for name in plan['live_hashes']};assert before==plan['live_hashes']
    for path in [data/'devices-candidate-breadth-2026-10-14.sqlite',data/'import-manifest-candidate-breadth-2026-10-14.json',BASE/'stage-report.json']:assert not path.exists()
    with sqlite3.connect((data/'devices.sqlite').as_uri()+'?mode=ro',uri=True) as live:
        live.execute('BEGIN');old_ids={r[0] for r in live.execute('SELECT id FROM sources')};old_devices={r[0] for r in live.execute('SELECT id FROM devices')}
        counts={t:live.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['devices','records','facts','sources','identifiers','affiliations','hyperscaler_evidence','documents','document_pages']}
    manifest_root,manifest=read_manifest(data/'import-manifest.json')
    freeze=json.loads((ROOT/'research/batches/discovery-2026-10-12/integration-wave5/code-under-test.json').read_text())['files']
    assert len(freeze)==209 and all(sha(ROOT/name)==expected for name,expected in freeze.items())
    stage=(BASE/'stage.py').read_text();assert hashlib.sha256(stage.encode()).hexdigest()=='e795eac0ec21a9315adfc13d0f18d45515ffdd9764a558a127747627ad86e6cc'
    tree=ast.parse(stage);helper_funcs=[n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name in {'sha','clone','checked_path'}]
    function=next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name=='main')
    preflight=next(n for n in function.body if isinstance(n,ast.For) and isinstance(n.target,ast.Name) and n.target.id=='group')
    ns={'ROOT':ROOT,'DATA':data,'Path':Path,'re':re,'hashlib':hashlib,'shutil':shutil,'json':json,'prepare_curated':prepare_curated,
        'plan':plan,'names':set(),'old_ids':old_ids,'source_ids':set(),'records':0,'facts':0,'groups':[],
        'accepted_hashes':{plan_path:plan_sha}}
    exec(compile(ast.Module(body=helper_funcs+[preflight],type_ignores=[]),str(BASE/'stage.py'),'exec'),ns)
    assert len(ns['groups'])==3 and len(ns['source_ids'])==15 and ns['records']==467 and ns['facts']==5720
    rows=[];raw_hashes={};identity_hashes=[];affiliations=[];identifier_count=0;keys=[]
    for group in plan['groups']:
        path=ROOT/group['candidate'];payload=json.loads(path.read_text());review_path=ROOT/group['review']
        assert sha(path)==group['sha256']==group['accepted_candidate_sha256']
        assert sha(review_path)==group['review_sha256'];assert group['sha256'] in review_path.read_text()
        prepared=prepare_curated(path)
        for source,snapshot,metadata,records in prepared:
            assert metadata.sha256==source['original_sha256'];raw_hashes[str(snapshot.relative_to(ROOT))]=metadata.sha256
        identities=[]
        for d in payload['devices']:
            assert not d.get('hyperscaler');assert all(a[1]!='manufacturer' for a in d.get('affiliations',[]))
            did=digest(json.dumps([normalize(d['brand']),d['model'].casefold().strip(),'']).encode())[:20]
            identity_hashes.append(did);identities.append(did);keys.append((d['source'],d['key']))
            affiliations.extend(d.get('affiliations',[]));identifier_count+=len(d.get('identifiers',[]))
        rows.append({'name':group['name'],'candidate_sha256':group['sha256'],'review_sha256':group['review_sha256'],'sources':len(payload['sources']),'records':len(payload['devices']),
                     'facts':sum(len(d['facts']) for d in payload['devices']),'unique_identity_hashes':len(set(identities)),'existing_identity_reuse':len(set(identities)&old_devices)})
    assert len(keys)==len(set(keys))==467
    assert len(set(identity_hashes)-old_devices)==398
    assert len(set(identity_hashes)&old_devices)==9
    ref=[job for job in manifest['imports'] if job['source']=='reference-documents'];assert len(ref)==1
    reference=data/ref[0]['path'];bundle=json.loads(reference.read_text())
    print('reference bundle top keys',list(bundle) if isinstance(bundle,dict) else 'list')
    assert len(bundle['documents'])==61
    assert all(not d.exists() for _,_,d,_ in ns['groups'])
    assert before=={name:sha(data/name) for name in before};assert sha(plan_path)==plan_sha
    assert all(sha(p)==expected for p,expected in ns['accepted_hashes'].items())
    expected={**counts,'devices':counts['devices']+398,'records':counts['records']+467,'sources':counts['sources']+15,'facts':counts['facts']+5720,
              'identifiers':counts['identifiers']+identifier_count,'affiliations':counts['affiliations']+len(affiliations)}
    report={'decision':'accept exact concrete plan for staging','plan_sha256':plan_sha,'accepted_stage_helper_sha256':hashlib.sha256(stage.encode()).hexdigest(),
            'groups':rows,'source_count':15,'record_count':467,'fact_count':5720,'pinned_raw_snapshots':raw_hashes,
            'preflight_hashes':{str(p.relative_to(ROOT)):h for p,h in ns['accepted_hashes'].items()},'unique_new_identity_count':398,'existing_identity_reuse':9,
            'new_affiliation_roles':dict(Counter(a[1] for a in affiliations)),'new_identifiers':identifier_count,
            'live_hashes_unchanged':before,'expected_staged_counts':expected,'reference_bundle':str(reference.relative_to(ROOT)),
            'reference_bundle_sha256':sha(reference),'reference_bundle_entries':len(bundle['documents']),'manifest_jobs_before':len(manifest['imports']),'manifest_inputs_before':len(manifest['files']),
            'frozen_code_files':209,'stage_main_invoked':False,'preflight_executed_only':True,'shared_paths_written':[],'network_requests':0}
    (OUT/'plan-checks.json').write_text(json.dumps(report,indent=2)+'\n');(OUT/'accepted-plan-reviewed.json').write_bytes(planbytes)
    print(json.dumps({k:v for k,v in report.items() if k not in {'preflight_hashes','pinned_raw_snapshots','groups'}},indent=2))
if __name__=='__main__':main()
