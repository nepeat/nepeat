import copy
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt.catalog import Catalog
from devicehunt.fcc import import_manifest
from devicehunt.__main__ import report


class FCCTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'catalog.sqlite')
        self.data = json.loads((Path(__file__).resolve().parents[1] / 'data/fcc-manifest.json').read_text())

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def load(self, data):
        path = self.root / 'manifest.json'
        path.write_text(json.dumps(data))
        return import_manifest(self.c, path)

    def test_application_ids_and_restricted_exhibits_survive_refresh(self):
        for _ in range(2):
            self.assertEqual(self.load(self.data), {'applications': 2, 'exhibits': 32})
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0], 2)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_exhibits').fetchone()[0], 32)
        row = self.c.db.execute("SELECT * FROM fcc_applications WHERE fcc_id='2AVPL-CUBSCOUT'").fetchone()
        self.assertEqual(row['application_id'], 'oaLSKGa9an/UwzeojNZCsg==')
        restricted = list(self.c.db.execute("SELECT * FROM fcc_exhibits WHERE availability='restricted'"))
        self.assertEqual(len(restricted), 2)
        self.assertTrue(all(r['document_url'] is None for r in restricted))
        self.assertFalse(self.c.search(role='manufacturer'))
        self.assertEqual(len(self.c.search('NFC')), 1)

    def test_later_applications_do_not_create_new_device(self):
        data = copy.deepcopy(self.data)
        later = copy.deepcopy(data['applications'][0])
        later['application_id'] = 'another-application'
        later['purpose'] = 'Class II Permissive Change'
        data['applications'].append(later)
        data['expected_applications'] = 3
        self.load(data)
        self.assertEqual(len(self.c.search()), 2)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_applications').fetchone()[0], 3)
        self.load(self.data)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_exhibits').fetchone()[0], 32)
        self.assertFalse(self.c.db.execute('PRAGMA foreign_key_check').fetchall())

    def test_incomplete_duplicate_or_wrong_entity_manifest_cannot_replace_good_data(self):
        self.load(self.data)
        changes = [lambda d: d.update(complete_for_declared_scope=False),
                   lambda d: d['applications'].pop(),
                   lambda d: d['applications'][0]['exhibits'].pop(),
                   lambda d: d['applications'].__setitem__(1, d['applications'][0]),
                   lambda d: d['applications'][0].update(grantee='Amazon Technologies Inc.'),
                   lambda d: d['applications'][0]['exhibits'][0].update(url='javascript:alert(1)')]
        for change in changes:
            data = copy.deepcopy(self.data)
            change(data)
            with self.assertRaises(ValueError):
                self.load(data)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_exhibits').fetchone()[0], 32)

    def test_report_contains_exhibit_details_and_no_pdf_archive_claim(self):
        self.load(self.data)
        path = self.root / 'catalog.html'
        report(self.c, path)
        body = path.read_text()
        self.assertIn('5661528', body)
        self.assertIn('PDF contents are not stored', body)
        self.assertIn('fcc_exhibits', body)


if __name__ == '__main__':
    unittest.main()
