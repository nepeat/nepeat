import copy
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from devicehunt import lookup, ntra_historical as ntra, ntra_register
from devicehunt.catalog import Catalog, digest


class HistoricalNtraTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')
        self.pdf = self.root / 'register.pdf'; self.pdf.write_bytes(b'%PDF-1.4 synthetic test')
        self.pages = {'pages': [
            {'page': 1, 'tables': [[ntra.HEADER, ['1', 'SERVER', 'AMAZON DATA SERVICES Model-1']]]},
            {'page': 2, 'tables': [[['2', 'SWITCH', 'EDGE-CORE AS7760-32X'],
                                   ['3', 'SWITCH', '(cid:123) Unreadable']]]}]}

    def tearDown(self):
        self.c.db.close(); self.temp.cleanup()

    def bundle(self):
        table = self.root / 'tables.json'; table.write_text(json.dumps(self.pages))
        data = {'schema_version': 1, 'source_url': ntra.URL, 'capture_format': 'pdfplumber_positional_tables',
                'expected': ntra.summary(ntra.parse_pages(self.pages, 2, 3))}
        for key, p in [('pdf', self.pdf), ('tables', table)]:
            data[key] = {'path': p.name, 'sha256': digest(p.read_bytes())}
        p = self.root / 'bundle.json'; p.write_text(json.dumps(data)); return p

    def ingest(self, path):
        with patch.object(ntra, 'PDF_SHA256', digest(self.pdf.read_bytes())), patch.object(ntra, 'PAGE_COUNT', 2), patch.object(ntra, 'ROW_COUNT', 3):
            return ntra.import_bundle(self.c, path)

    def test_combined_cells_are_searchable_without_invented_brand_boundaries(self):
        for _ in range(2): result = self.ingest(self.bundle())
        self.assertEqual(result, {'pages': 2, 'rows': 3, 'combined_brand_model': 2, 'review_required': 1})
        self.assertEqual(self.c.search(), [])
        rows = ntra_register.search(self.c.db, 'AMAZON DATA SERVICES')
        self.assertEqual(len(rows), 1)
        self.assertIsNone(rows[0]['brand']); self.assertIsNone(rows[0]['model'])
        self.assertEqual(rows[0]['parse_status'], 'combined_brand_model')
        self.assertEqual(json.loads(rows[0]['cells_json'])[2], 'AMAZON DATA SERVICES Model-1')
        section = next(d for d in lookup.run(self.c.db, 'AS7760')['datasets'] if d['dataset'] == 'ntra')
        self.assertEqual(section['matches'], 1)
        self.assertEqual(section['rows'][0]['raw_cells'][2], 'EDGE-CORE AS7760-32X')
        self.assertEqual(ntra_register.search(self.c.db, 'Unreadable')[0]['parse_status'], 'review_required')

    def test_sequence_shape_header_and_page_fail_closed(self):
        changes = [lambda d: d['pages'].pop(),
                   lambda d: d['pages'][1].update(page=1),
                   lambda d: d['pages'][0]['tables'][0][0].__setitem__(2, 'MODEL'),
                   lambda d: d['pages'][1]['tables'][0][0].__setitem__(0, '1'),
                   lambda d: d['pages'][1]['tables'][0][0].__setitem__(0, '4'),
                   lambda d: d['pages'][1]['tables'][0][0].pop(),
                   lambda d: d['pages'][1]['tables'][0][0].__setitem__(1, 10)]
        for change in changes:
            data = copy.deepcopy(self.pages); change(data)
            with self.assertRaises(ValueError): ntra.parse_pages(data, 2, 3)
        with self.assertRaises(ValueError): ntra.parse_pages(self.pages, 2, 4)

    def test_corrupt_dependency_and_counts_preserve_prior_rows(self):
        path = self.bundle(); self.ingest(path)
        before = [tuple(r) for r in self.c.db.execute('SELECT * FROM ntra_equipment')]
        for field in ('checksum', 'counts', 'path'):
            path = self.bundle(); data = json.loads(path.read_text())
            if field == 'checksum': data['tables']['sha256'] = '0' * 64
            if field == 'counts': data['expected']['rows'] = 4
            if field == 'path': data['tables']['path'] = '../tables.json'
            path.write_text(json.dumps(data))
            with self.assertRaises(ValueError): self.ingest(path)
            self.assertEqual(before, [tuple(r) for r in self.c.db.execute('SELECT * FROM ntra_equipment')])

    def test_refresh_keeps_another_snapshot(self):
        path = self.bundle()
        self.c.source('another-snapshot', 'https://example.org', 'regulatory', '', '', path)
        self.c.db.execute('INSERT INTO ntra_equipment VALUES (?,?,?,?,?,?,?,?,?,?,?,?)',
                          ('another-snapshot', 1, 1, 1, '1', 'SWITCH', 'Edge-Core', 'AS7760-32X', '',
                           'identified', '["1","SWITCH","Edge-Core","AS7760-32X",""]', 'https://example.org'))
        self.c.db.commit()
        for _ in range(2): self.ingest(path)
        self.assertEqual(len(ntra_register.search(self.c.db, 'AS7760')), 2)
        self.assertEqual(self.c.db.execute('PRAGMA foreign_key_check').fetchall(), [])
