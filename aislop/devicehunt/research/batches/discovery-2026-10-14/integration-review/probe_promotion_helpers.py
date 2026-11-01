"""Static AST proof checks only; never execute promotion or export helpers."""
import ast
import hashlib
import json
from pathlib import Path
import unittest
import io
OUT=Path(__file__).resolve().parent
BASE=OUT.parent/'integration-breadth'
ROOT=OUT.parents[3]
CODES={name:(BASE/name).read_text() for name in ['promote.py','refresh_exports.py']}
TREES={name:ast.parse(code) for name,code in CODES.items()}
MAINS={name:next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name=='main') for name,tree in TREES.items()}

class PromotionHelperChecks(unittest.TestCase):
    def test_all_clone_calls_bind_explicit_expected_hash(self):
        for name,tree in TREES.items():
            calls=[n for n in ast.walk(tree) if isinstance(n,ast.Call) and isinstance(n.func,ast.Name) and n.func.id=='clone']
            self.assertEqual(len(calls),3 if name=='promote.py' else 1)
            self.assertTrue(all(len(n.args)==3 for n in calls))
    def test_required_proofs_include_all_new_helpers_reviews_and_rebuild(self):
        node=next(n for n in MAINS['promote.py'].body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='required' for t in n.targets))
        required=eval(compile(ast.Expression(node.value),'required-set','eval'),{'ROOT':ROOT,'OUT':BASE})
        self.assertIn('data/rebuilt-breadth-2026-10-14.sqlite',required)
        self.assertIn('data/exports/breadth-2026-10-14-comparison.json',required)
        self.assertIn('data/exports/breadth-2026-10-14-validation.json',required)
        for name in ['accepted-inputs.json','stage-report.json','stage.py','promote.py','refresh_exports.py','query-check.json']:
            self.assertIn(str((BASE/name).relative_to(ROOT)),required)
        for name in ['fix-review.md','plan-review.md','promotion-helper-review.md','final-review.md']:
            self.assertIn(str((OUT/name).relative_to(ROOT)),required)
    def test_proof_paths_are_confined_and_review_is_fixed(self):
        s=CODES['promote.py'];self.assertIn("not relative.is_absolute() and '..' not in relative.parts",s)
        self.assertIn('path.is_relative_to(ROOT)',s)
        self.assertIn("acceptance['review_path'] == str((OUT.parent / 'integration-review/final-review.md').relative_to(ROOT))",s)
    def test_candidate_and_manifest_bind_stage_and_acceptance(self):
        s=CODES['promote.py']
        self.assertIn("sha(CAND_DB) == report['candidate_database_sha256'] == acceptance['candidate_database_sha256']",s)
        self.assertIn("sha(CAND_MANIFEST) == report['candidate_manifest_sha256'] == acceptance['candidate_manifest_sha256']",s)
        self.assertIn("report['accepted_input_hashes'].items()",s);self.assertIn("report['new_input_hashes'].items()",s)
    def test_both_replacements_follow_complete_journal(self):
        body=MAINS['promote.py'].body
        journal=next(i for i,n in enumerate(body) if isinstance(n,ast.Expr) and isinstance(n.value,ast.Call) and isinstance(n.value.func,ast.Name) and n.value.func.id=='save' and 'promotion-journal.json' in ast.unparse(n))
        replace=[i for i,n in enumerate(body) if isinstance(n,ast.Expr) and isinstance(n.value,ast.Call) and ast.unparse(n.value.func)=='os.replace']
        self.assertEqual(len(replace),2);self.assertTrue(all(journal<i for i in replace))
        self.assertLess(replace[-1],next(i for i,n in enumerate(body) if isinstance(n,ast.Expr) and 'promotion-result.json' in ast.unparse(n)))
    def test_live_hash_guard_and_sidecar_guard_precede_mutations(self):
        body=MAINS['promote.py'].body;text=[ast.unparse(n) for n in body]
        guard=next(i for i,s in enumerate(text) if "report['live_unchanged']" in s and s.startswith('assert'))
        backups=next(i for i,n in enumerate(body) if isinstance(n,ast.For) and 'clone(source, dest' in ast.unparse(n))
        self.assertLess(guard,backups)
        self.assertIn("('-journal', '-wal', '-shm')",CODES['promote.py'])
    def test_export_catalog_read_only_and_queries_are_select(self):
        tree=TREES['refresh_exports.py']
        calls=[n for n in ast.walk(tree) if isinstance(n,ast.Call) and isinstance(n.func,ast.Name) and n.func.id=='Catalog']
        self.assertEqual(len(calls),1);self.assertTrue(any(k.arg=='read_only' and isinstance(k.value,ast.Constant) and k.value.value is True for k in calls[0].keywords))
        queries=[n for n in ast.walk(tree) if isinstance(n,ast.Call) and ast.unparse(n.func)=='c.db.execute']
        self.assertTrue(queries);self.assertTrue(all(isinstance(n.args[0],ast.Constant) and n.args[0].value.strip().upper().startswith('SELECT') for n in queries))
    def test_export_backups_precede_refresh_and_success_report(self):
        body=MAINS['refresh_exports.py'].body
        backup=next(i for i,n in enumerate(body) if isinstance(n,ast.For) and 'clone(' in ast.unparse(n))
        refresh=next(i for i,n in enumerate(body) if isinstance(n,ast.Try))
        report=next(i for i,n in enumerate(body) if isinstance(n,ast.Expr) and 'exports-report.json' in ast.unparse(n))
        self.assertLess(backup,refresh);self.assertLess(refresh,report)
        self.assertIn('assert sha(db) == before',CODES['refresh_exports.py'])
        self.assertIn("before == proof['new']['devices.sqlite']",CODES['refresh_exports.py'])
    def test_promote_checks_all_comparison_tables_and_full_baseline(self):
        s=CODES['promote.py'];self.assertIn('len(comparison) == len(expected_tables) == 52',s)
        self.assertIn("{'search_index', 'document_search'} <= set(comparison)",s)
        self.assertIn("r['only_original'] == r['only_rebuilt'] == 0",s)
        self.assertIn("{'total': 10, 'hits': 10, 'gaps': 0}",s)
        self.assertIn("validation['reference_database']",s);self.assertIn("validation['database']",s)

if __name__=='__main__':
    for name,code in CODES.items():(OUT/('reviewed-'+name)).write_text(code)
    stream=io.StringIO();result=unittest.TextTestRunner(stream=stream,verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(PromotionHelperChecks))
    report={'helper_sha256':{name:hashlib.sha256(code.encode()).hexdigest() for name,code in CODES.items()},'checks':result.testsRun,'failures':len(result.failures),'errors':len(result.errors),'output':stream.getvalue(),
            'mode':'static AST/source checks only; no helper main invocation','shared_paths_written':[],'network_requests':0}
    (OUT/'promotion-helper-probes.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2));raise SystemExit(0 if result.wasSuccessful() else 1)
