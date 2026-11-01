import contextlib
import io
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from devicehunt.catalog import Catalog
from devicehunt.__main__ import main
from devicehunt.validation import run


class InventoryValidationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.path = self.root / 'catalog.sqlite'
        self.c = Catalog(self.path)
        self.fixture = self.root / 'inventory.json'
        self.fixture.write_text(json.dumps({'schema_version': 1, 'devices': [
            {'identifier': 'CS8210-32x-un-00', 'vendor': 'Celestica', 'claimed_operator': 'AWS'}]}))

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def add(self, model, *, sid='external', snapshot=True, tier='vendor', facts=(), identifiers=(), vendor='Celestica'):
        raw = self.root / (sid + '.html')
        raw.write_text(model)
        self.c.source(sid, 'https://example.org/' + sid, tier, '', '', raw if snapshot else None)
        self.c.add(sid, 'product', 'https://example.org/product', 'Celestica', model, model, 'switch', {},
                   [(vendor, 'brand')], identifiers, facts)
        self.c.finish(sid, 1)

    def evaluate(self):
        with sqlite3.connect(self.path.resolve().as_uri() + '?mode=ro', uri=True) as db:
            db.row_factory = sqlite3.Row
            return run(db, self.fixture)

    def test_exact_external_hit_preserves_owner_context_and_provenance(self):
        self.add('CS8210_32X_UN_00')
        result = self.evaluate()
        self.assertEqual(result['summary'], {'total': 1, 'hits': 1, 'gaps': 0})
        row = result['results'][0]
        self.assertEqual(row['identifier'], 'CS8210-32x-un-00')
        self.assertEqual(row['evidence'][0]['source_url'], 'https://example.org/product')
        self.assertTrue(row['evidence'][0]['source_sha256'])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0], 0)

    def test_family_suffix_and_replacement_mentions_cannot_pass(self):
        for model in ['CS8210', 'CS8210-32X', 'CS8210-32X-UN-01', 'CS8210-32X-UN-000']:
            self.add(model, facts=[('vendor_replacement_text', 'CS8210-32X-UN-00')])
            row = self.evaluate()['results'][0]
            self.assertTrue(row['search_hits'])
            self.assertEqual(row['status'], 'gap')
            self.assertEqual(row['evidence'], [])

    def test_owner_and_unsnapshotted_records_do_not_satisfy_fixture(self):
        self.add('CS8210-32X-UN-00', sid='owner', tier='user')
        self.add('CS8210-32X-UN-00', sid='reviewed', snapshot=False)
        self.add('CS8210-32X-UN-00', sid='wrong-vendor', vendor='Oracle')
        self.add('onie:celestica_cs8210', sid='build', identifiers=[('onie_platform', 'CS8210-32X-UN-00')])
        row = self.evaluate()['results'][0]
        self.assertEqual(row['status'], 'gap')
        self.assertEqual(len(row['leads']), 4)
        self.assertEqual(row['evidence'], [])

    def test_reviewed_label_photo_is_distinct_from_seller_text_and_not_manufacture(self):
        from devicehunt import vendor_inventory
        self.add('CS8210-32X-UN-00', sid='seller', tier='marketplace')
        self.assertEqual(self.evaluate()['summary']['hits'], 0)
        self.add('CS8210-32X-UN-00', sid='photo', tier='physical_label_photo')
        row = self.evaluate()['results'][0]
        self.assertEqual(row['status'], 'hit')
        self.assertEqual(self.evaluate()['evidence_summary'],
                         dict(document_snapshot_hits=0, label_photo_only_hits=1, unresolved_identity_gaps=0))
        self.assertEqual([p['source_tier'] for p in row['evidence']], ['physical_label_photo'])
        self.assertEqual(row['leads'][0]['source_tier'], 'marketplace')
        self.assertEqual(vendor_inventory.run(self.c.db, scope='manufacturer')['unique_identities'], 0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0], 0)
        self.add('CS8210-32X-UN-00', sid='photo', tier='physical_label_photo', snapshot=False)
        self.assertEqual(self.evaluate()['summary']['hits'], 0)

    def test_cli_exit_codes_read_only_and_search_regression_detection(self):
        self.add('CS8210-32X-UN-00')
        argv = ['--db', str(self.path), 'validate-inventory', '--inventory', str(self.fixture),
                '--output', str(self.root / 'report.json')]
        before = self.path.read_bytes()
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(main(argv), 0)
        self.assertEqual(self.path.read_bytes(), before)
        self.c.db.execute('DELETE FROM search_index')
        self.c.db.commit()
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(main(argv), 2)
        report = json.loads((self.root / 'report.json').read_text())
        self.assertEqual(report['results'][0]['gap_type'], 'search_index')
        self.fixture.write_text('{"schema_version":1,"devices":[]}')
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(main(argv), 1)


if __name__ == '__main__':
    unittest.main()
