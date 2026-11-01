"""Isolated actual-code fix probes; never execute stage.main or shared writes."""
import ast
import hashlib
import json
from pathlib import Path
import re
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch
OUT=Path(__file__).resolve().parent
PROJECT=OUT.parents[3]
sys.path.insert(0,str(PROJECT))
from devicehunt.importers import prepare_curated
import sqlite3
SRC=OUT.parent/'integration-breadth/stage.py'
CODE=SRC.read_text();TREE=ast.parse(CODE)
FUNCS=[n for n in TREE.body if isinstance(n,ast.FunctionDef) and n.name in {'sha','clone','checked_path'}]
MAIN=next(n for n in TREE.body if isinstance(n,ast.FunctionDef) and n.name=='main')
PREFLIGHT=next(n for n in MAIN.body if isinstance(n,ast.For) and isinstance(n.target,ast.Name) and n.target.id=='group')
COLS=next(n for n in ast.walk(MAIN) if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='cols' for t in n.targets))
NS={'Path':Path,'hashlib':hashlib,'shutil':shutil}
exec(compile(ast.Module(body=FUNCS,type_ignores=[]),str(SRC),'exec'),NS)
sha=NS['sha'];clone=NS['clone'];checked_path=NS['checked_path']

class FixTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(dir=OUT);self.addCleanup(self.tmp.cleanup);self.root=Path(self.tmp.name)
    def fixture(self):
        capture=self.root/'capture';capture.mkdir();(self.root/'data/raw').mkdir(parents=True)
        (capture/'cap.html').write_text('<html>exact source</html>');(capture/'sidecar.txt').write_text('exact sidecar')
        (self.root/'review.md').write_text('fixture review binding accepted candidate')
        payload={'sources':[{'id':'new-source','url':'https://example.invalid','tier':'vendor','license':'fixture','coverage':'fixture','snapshot':'cap.html','original_sha256':sha(capture/'cap.html')}],
                 'devices':[{'source':'new-source','key':'1','brand':'fixture','model':'X+','title':'X+','kind':'server','affiliations':[['fixture','listing_holder']],'facts':[['literal','value']]}]}
        path=capture/'candidate.json';path.write_text(json.dumps(payload))
        group={'name':'valid','candidate':'capture/candidate.json','sha256':sha(path),'accepted_candidate_sha256':sha(path),'review':'review.md','review_sha256':sha(self.root/'review.md'),'sources':1,'records':1,'facts':1}
        return payload,group
    def preflight(self,payload,groups):
        path=self.root/'capture/candidate.json';path.write_text(json.dumps(payload))
        for g in groups:g['sha256']=g['accepted_candidate_sha256']=sha(path)
        ns=dict(NS,ROOT=self.root,DATA=self.root/'data',re=re,json=json,prepare_curated=prepare_curated,
                plan={'groups':groups},names=set(),old_ids=set(),source_ids=set(),records=0,facts=0,groups=[],accepted_hashes={})
        exec(compile(ast.Module(body=[PREFLIGHT],type_ignores=[]),str(SRC),'exec'),ns)
        return ns
    def test_clone_good_bytes_match_accepted_hash(self):
        src=self.root/'original';src.write_text('accepted');dst=self.root/'copy'
        clone(src,dst,sha(src));self.assertEqual(src.read_bytes(),dst.read_bytes())
    def test_clone_rejects_changed_source_before_copy(self):
        src=self.root/'original';src.write_text('accepted');expected=sha(src);src.write_text('changed');dst=self.root/'copy'
        with self.assertRaises(AssertionError):clone(src,dst,expected)
        self.assertFalse(dst.exists())
    def test_clone_rejects_source_change_during_copy(self):
        src=self.root/'original';src.write_text('accepted');dst=self.root/'copy';expected=sha(src)
        original_copy=shutil.copyfileobj
        def changed(a,b):original_copy(a,b);src.write_text('changed after read')
        with patch.object(shutil,'copyfileobj',changed):
            with self.assertRaises(AssertionError):clone(src,dst,expected)
    def test_all_old_columns_now_detect_changed_values(self):
        db=sqlite3.connect(':memory:');self.addCleanup(db.close);db.execute("ATTACH DATABASE ':memory:' AS old")
        for prefix in ['main','old']:
            db.execute('CREATE TABLE '+prefix+'.records (id TEXT, observed_at TEXT, fetched_at TEXT, cache_path TEXT)')
        db.execute("INSERT INTO old.records VALUES ('id','old','old','old')");db.execute("INSERT INTO main.records VALUES ('id','changed','changed','changed')")
        ns={'db':db,'table':'records'};exec(compile(ast.Module(body=[COLS],type_ignores=[]),str(SRC),'exec'),ns)
        self.assertEqual(ns['cols'],['id','observed_at','fetched_at','cache_path'])
        fields=','.join('"'+c+'"' for c in ns['cols']);query=f'SELECT {fields},count(*) AS multiplicity FROM {{}}."records" GROUP BY {fields}'
        self.assertEqual(db.execute('SELECT count(*) FROM ('+query.format('old')+' EXCEPT '+query.format('main')+')').fetchone()[0],1)
    def test_valid_preflight_makes_no_staging_writes_and_pins_dependencies(self):
        payload,group=self.fixture();ns=self.preflight(payload,[group])
        self.assertEqual(ns['records'],1);self.assertEqual(len(ns['groups']),1)
        self.assertEqual(list((self.root/'data/raw').iterdir()),[])
        self.assertIn(self.root/'capture/cap.html',ns['accepted_hashes'])
        self.assertIn(self.root/'review.md',ns['accepted_hashes'])
    def test_unsafe_or_duplicate_group_names_fail_before_writes(self):
        payload,group=self.fixture()
        for invalid in ['../escape','a/../../escape','','UpperCase','-prefix']:
            with self.subTest(invalid=invalid):
                bad=dict(group,name=invalid)
                with self.assertRaises(AssertionError):self.preflight(payload,[bad])
        with self.assertRaises(AssertionError):self.preflight(payload,[dict(group),dict(group)])
        self.assertEqual(list((self.root/'data/raw').iterdir()),[])
    def test_unhashed_sidecar_fails_but_explicit_hash_passes(self):
        payload,group=self.fixture();payload['sources'][0]['text_sidecar']='sidecar.txt'
        with self.assertRaises(AssertionError):self.preflight(payload,[group])
        payload['sources'][0]['text_sidecar_sha256']=sha(self.root/'capture/sidecar.txt')
        ns=self.preflight(payload,[group]);self.assertIn(self.root/'capture/sidecar.txt',ns['accepted_hashes'])
    def test_sidecar_dependency_hash_passes_and_collision_rejected(self):
        payload,group=self.fixture();s=payload['sources'][0];s['text_sidecar']='sidecar.txt'
        s['evidence_dependencies']=[{'path':'sidecar.txt','sha256':sha(self.root/'capture/sidecar.txt')}]
        self.assertEqual(len(self.preflight(payload,[group])['groups']),1)
        s['evidence_dependencies'].append({'path':'candidate.json','sha256':'a'*64})
        with self.assertRaises(AssertionError):self.preflight(payload,[group])
    def test_group_count_mismatch_rejected_before_writes(self):
        payload,group=self.fixture()
        for field in ['sources','records','facts']:
            bad=dict(group);bad[field]+=1
            with self.subTest(field=field):
                with self.assertRaises(AssertionError):self.preflight(payload,[bad])
        self.assertEqual(list((self.root/'data/raw').iterdir()),[])
    def test_final_hash_guards_cover_plan_reviews_originals_staged(self):
        statements=[ast.unparse(n) for n in MAIN.body]
        self.assertIn('plan_bytes = plan_path.read_bytes()',statements)
        self.assertTrue(any('accepted_hashes = {plan_path: plan_hash}'==s for s in statements))
        checks=[n for n in MAIN.body if isinstance(n,ast.Assert)]
        self.assertEqual(sum('accepted_hashes.items()' in ast.unparse(n) for n in checks),2)
        self.assertEqual(sum('staged_hashes.items()' in ast.unparse(n) for n in checks),1)
        self.assertLess(MAIN.body.index(PREFLIGHT),next(i for i,n in enumerate(MAIN.body) if isinstance(n,ast.For) and 'dest_dir' in ast.unparse(n.target)))

if __name__=='__main__':
    (OUT/'stage-fixed-reviewed.py').write_text(CODE)
    suite=unittest.defaultTestLoader.loadTestsFromTestCase(FixTests)
    import io
    stream=io.StringIO();result=unittest.TextTestRunner(stream=stream,verbosity=2).run(suite)
    report={'helper_sha256':hashlib.sha256(CODE.encode()).hexdigest(),'tests_run':result.testsRun,'failures':len(result.failures),'errors':len(result.errors),'output':stream.getvalue(),
            'stage_main_invoked':False,'scope':'only extracted pure helpers, preflight-loop AST, all-column SQL, final guard inspection; fixture paths owned by this review','shared_paths_written':[],'network_requests':0}
    (OUT/'fix-probes.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
    raise SystemExit(0 if result.wasSuccessful() else 1)
