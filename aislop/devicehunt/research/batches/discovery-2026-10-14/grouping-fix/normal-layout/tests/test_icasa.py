import copy
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from devicehunt import icasa
from devicehunt.catalog import Catalog, digest


class IcasaTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')
        self.cells = [
            ['Ethernet Switch', 'AZ4444-52X-DC-10', 'Fixed Line Equipment', 'TE-2024/0223'],
            ['Console Terminal Server', 'DMC-12-48R-AS0 48V', 'Fixed Line Equipment', 'TE-2025/0183'],
            ['', 'Switch', 'CS8210-32X-DC-11, CS8211-32X-DC-11', 'Fixed Line Equipment'],
            ['Radio', 'TEST', 'Other', 'PA-2021/0001'],
        ]
        self.pages = {'pages': [{'page': 1, 'rows': self.cells}]}

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def test_shifted_rows_never_become_asserted_model_or_approval(self):
        rows = icasa.parse_pages(self.pages, expected_pages=1)
        self.assertEqual(rows[2][2:6], (None, None, None, None))
        self.assertEqual(rows[2][6], 'review_required')
        self.assertIn('CS8210', rows[2][7])
        self.assertEqual(rows[0][3], 'AZ4444-52X-DC-10')
        self.assertEqual(rows[1][3], 'DMC-12-48R-AS0 48V')
        self.assertEqual(rows[3][6], 'identified')
        with self.assertRaises(ValueError):
            icasa.parse_pages(self.pages, expected_pages=2)
        bad = copy.deepcopy(self.pages); bad['pages'][0]['page'] = 2
        with self.assertRaises(ValueError):
            icasa.parse_pages(bad, expected_pages=1)

    def test_import_checksum_and_exact_model_matching(self):
        # Synthetic PDF is explicitly substituted only for this unit fixture.
        pdf = self.root / 'source.pdf'; pdf.write_bytes(b'test PDF fixture')
        tables = self.root / 'tables.json'
        tables.write_text(json.dumps({'pages': [{'page': n, 'rows': self.cells} for n in range(1, 907)]}))
        data = {'schema_version': 1, 'source_url': icasa.URL,
                'capture_format': 'pdfplumber_positional_tables',
                'pdf': {'path': pdf.name, 'sha256': digest(pdf.read_bytes())},
                'tables': {'path': tables.name, 'sha256': digest(tables.read_bytes())},
                'expected': {'rows': 3624, 'identified': 2718, 'review_required': 906}}
        bundle = self.root / 'bundle.json'; bundle.write_text(json.dumps(data))
        with patch.object(icasa, 'PDF_SHA256', data['pdf']['sha256']):
            icasa.import_bundle(self.c, bundle)
            self.assertEqual(self.c.search(), [])
            self.assertEqual(len(icasa.search(self.c.db, 'CS8210', limit=-1)), 906)
            self.assertEqual(icasa.search(self.c.db, '%'), [])
            icasa.import_bundle(self.c, bundle)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM equipment_register').fetchone()[0], 3624)
            tables.write_text('corrupt')
            with self.assertRaisesRegex(ValueError, 'checksum'):
                icasa.import_bundle(self.c, bundle)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM equipment_register').fetchone()[0], 3624)
        self.c.source('fixture', 'https://example.org', 'test', 'test', 'test')
        exact = self.c.add('fixture', 'exact', 'https://example.org', 'Accton', 'AZ4444-52X-DC-10', 'test', 'switch', {})
        base = self.c.add('fixture', 'base', 'https://example.org', 'Edge-Core', 'AZ4444-52X', 'test', 'switch', {})
        no_suffix = self.c.add('fixture', 'dmc', 'https://example.org', 'Edge-Core', 'DMC-12-48R-AS0', 'test', 'server', {})
        self.c.finish('fixture', 3)
        self.assertEqual(len(self.c.detail(exact)['registry_matches']), 906)
        self.assertEqual(self.c.detail(base)['registry_matches'], [])
        self.assertEqual(self.c.detail(no_suffix)['registry_matches'], [])
        self.assertEqual(self.c.detail(exact)['affiliations'], [])
