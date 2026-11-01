from pathlib import Path
import tempfile
import unittest
import unittest.mock
from devicehunt.artifacts import Archive,canonical
from devicehunt.derivations import Derivations


def _recipe_ids(_):
    from devicehunt.derivations import recipe_code_sha256 as h
    return [h('page-text-flatten'), h('xml-locations')]



class DerivationTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        self.archive=Archive(Path(self.tmp.name)/'archive',create=True)
        self.engine=Derivations(self.archive)
        self.sha=self.archive.store_bytes(canonical({'pages':[{'page':1,'text':'first'},{'page':2,'text':''},{'page':3,'text':'first'}]}))
        self.inputs=[{'role':'page-text','sha256':self.sha}]
    def tearDown(self):
        self.archive.close()
        self.tmp.cleanup()
    def test_reuse_two_consumers_changed_recipe_and_rerun(self):
        recipe=self.engine.recipe('page-text-flatten','1')
        first=self.engine.run(recipe,self.inputs,consumer='search')
        second=self.engine.run(recipe,self.inputs,consumer='review')
        self.assertEqual(first,second)
        self.assertEqual(len(self.archive.receipts('derivation-consumer')),2)
        changed=self.engine.recipe('page-text-flatten','1',{'separator':' | '})
        third=self.engine.run(changed,self.inputs)
        self.assertNotEqual(first['id'],third['id'])
        self.assertNotEqual(first['id'],self.engine.run(recipe,self.inputs,force=True)['id'])
    def test_saved_unknown_never_matches_and_corrupt_output_reruns(self):
        saved=self.engine.register_saved(self.inputs,self.sha,'pypdf embedded text; no OCR')
        self.assertIsNone(self.archive.get_receipt(saved)['payload']['recipe_id'])
        recipe=self.engine.recipe('page-text-flatten','1')
        run=self.engine.run(recipe,self.inputs)
        self.assertNotEqual(saved,run['id'])
        output=self.archive.resolve(run['payload']['outputs'][0]['sha256'])
        output.unlink()
        fresh=self.engine.run(recipe,self.inputs)
        self.assertNotEqual(run['id'],fresh['id'])
    def test_failures_preserve_runs(self):
        sha=self.archive.store_bytes(b'not-json')
        recipe=self.engine.recipe('page-text-flatten','1')
        for _ in range(2):
            with self.assertRaises(ValueError): self.engine.run(recipe,[{'role':'raw','sha256':sha}])
        runs=self.archive.receipts('derivation-run')
        self.assertEqual(len(runs),2)
        self.assertTrue(all(r['payload']['status']=='failed' for r in runs))
    def test_xml_preserves_duplicate_order(self):
        sha=self.archive.store_bytes(b'<urlset><url><loc>a</loc></url><url><loc>a</loc></url><url><loc/></url></urlset>')
        recipe=self.engine.recipe('xml-locations','1')
        run=self.engine.run(recipe,[{'role':'sitemap','sha256':sha}])
        import json
        output=json.loads(self.archive.resolve(run['payload']['outputs'][0]['sha256']).read_bytes())
        self.assertEqual(output['locations'],['a','a',''])


class RecipeIdentityTests(unittest.TestCase):
    """S5: a recipe id binds only the code that recipe depends on."""
    def source(self):
        from pathlib import Path as P
        import devicehunt.derivations as d
        return P(d.__file__).read_text(encoding='utf-8')
    def test_unrelated_edits_keep_the_recipe_id(self):
        from devicehunt.derivations import recipe_code_sha256 as h
        base = self.source()
        edited = base + '\n\ndef _unrelated():\n    return 1\n'
        edited = edited.replace("def _consumer(self, run_id, consumer):", "def _consumer(self, run_id, consumer):\n        pass  # edited", 1)
        edited = '# comment\n' + edited
        self.assertIn('pass  # edited', edited)
        for name in ('page-text-flatten', 'xml-locations'):
            self.assertEqual(h(name), h(name, base))
            self.assertEqual(h(name, base), h(name, edited))
        # editing the OTHER recipe's function does not move this one
        other = base.replace("'locations': [node.text or ''", "'locations': [(node.text or '') + ''")
        self.assertNotEqual(other, base)
        self.assertEqual(h('page-text-flatten', other), h('page-text-flatten', base))
        self.assertNotEqual(h('xml-locations', other), h('xml-locations', base))
    def test_own_code_or_version_changes_the_recipe_id(self):
        import devicehunt.derivations as d
        base = self.source()
        changed = base.replace("configuration.get('separator', '\\n')", "configuration.get('separator', ' ')")
        self.assertNotEqual(changed, base)
        self.assertNotEqual(d.recipe_code_sha256('page-text-flatten', changed), d.recipe_code_sha256('page-text-flatten', base))
        before = d.recipe_code_sha256('xml-locations', base)
        with unittest.mock.patch.dict(d.RECIPE_CODE_VERSION, {'xml-locations': '2'}):
            self.assertNotEqual(d.recipe_code_sha256('xml-locations', base), before)
    def test_recipe_id_is_deterministic_across_processes(self):
        # Spawned interpreters each get a fresh hash seed; no subprocess entrypoint is used.
        import multiprocessing
        from devicehunt.derivations import recipe_code_sha256 as h
        with multiprocessing.get_context('spawn').Pool(2) as pool:
            out = pool.map(_recipe_ids, range(2))
        self.assertEqual(out[0], out[1])
        self.assertEqual(out[0], [h('page-text-flatten'), h('xml-locations')])
