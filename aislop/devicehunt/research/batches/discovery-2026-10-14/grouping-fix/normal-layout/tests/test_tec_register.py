import html
import sqlite3
import tempfile
import unittest
from pathlib import Path

from devicehunt import lookup, tec, tec_register
from devicehunt.catalog import Catalog


def sample():
    return [
        ['1', 'Certificate Agent', 'LAN Switch', 'L2 LAN Switch', 'Accton Technology Corporation',
         'AZ4444-52X-DC-10', 'AZ4444-52X-AC-00', 'Mandatory', 'Regular',
         '20240104 04/01/2024', '20330826 26/08/2033', '000123'],
        ['2', 'Unrelated Company', 'Router', 'IPV4 Router', '', '', 'OTHER-01,OTHER-02',
         'Mandatory', 'Regular', '20240104 04/01/2024', '00/01/1900', '000123'],
    ]


def table(rows):
    return '<table><tr>' + ''.join('<th>' + html.escape(v) + '</th>' for v in tec.HEADER) + '</tr>' + ''.join(
        '<tr>' + ''.join('<td>' + html.escape(v) + '</td>' for v in row) + '</tr>' for row in rows) + '</table>'


class TECRegisterTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.path = self.root / 'table.html'
        self.db = self.root / 'db.sqlite'
        self.c = Catalog(self.db)

    def tearDown(self):
        self.c.db.close()
        self.tmp.cleanup()

    def save(self, rows=None):
        self.path.write_text(table(sample() if rows is None else rows))
        return self.path

    def test_all_rows_keep_duplicates_unknown_oems_and_invalid_dates(self):
        path = self.save()
        for _ in range(2):
            self.assertEqual(tec_register.import_table(self.c, path), {'rows': 2, 'review_required': 1})
        rows = [dict(r) for r in self.c.db.execute('SELECT * FROM tec_equipment ORDER BY row_number')]
        self.assertEqual(rows[0]['declared_oem'], 'Accton Technology Corporation')
        self.assertEqual(rows[0]['applicant'], 'Certificate Agent')
        self.assertEqual(rows[0]['issue_date'], '2024-01-04')
        self.assertEqual(rows[0]['certificate_id'], rows[1]['certificate_id'])
        self.assertEqual(rows[1]['declared_oem'], '')
        self.assertEqual(rows[1]['primary_model'], '')
        self.assertIsNone(rows[1]['valid_upto'])
        self.assertEqual(rows[1]['valid_upto_raw'], '00/01/1900')
        self.assertEqual(rows[1]['parse_status'], 'review_required')
        self.assertIn('valid_upto', rows[1]['issue'])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0], 0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 0)

    def test_lookup_searches_models_oems_and_applicants_without_writing(self):
        tec_register.import_table(self.c, self.save())
        before = self.db.read_bytes()
        with sqlite3.connect(self.db.as_uri() + '?mode=ro', uri=True) as db:
            db.row_factory = sqlite3.Row
            for query, expected in [('Certificate Agent', 1), ('Accton', 1), ('OTHER-02', 1), ('000123', 2)]:
                section = next(s for s in lookup.run(db, query, 1)['datasets'] if s['dataset'] == 'tec')
                self.assertEqual(section['state'], 'loaded')
                self.assertEqual(section['matches'], expected)
                self.assertEqual(section['truncated'], expected > 1)
                self.assertEqual(section['rows'][0]['source_url'], tec.URL)
                self.assertIn('raw_cells', section['rows'][0])
        self.assertEqual(before, self.db.read_bytes())

    def test_legacy_database_lookup_does_not_create_missing_register(self):
        self.c.db.execute('DROP TABLE tec_equipment'); self.c.db.commit()
        before = self.db.read_bytes()
        with sqlite3.connect(self.db.as_uri() + '?mode=ro', uri=True) as db:
            db.row_factory = sqlite3.Row
            section = next(s for s in lookup.run(db, 'test')['datasets'] if s['dataset'] == 'tec')
            self.assertEqual(section['state'], 'not_loaded')
        self.assertEqual(before, self.db.read_bytes())

    def test_source_refresh_fails_before_deletion_and_catalog_import_is_independent(self):
        path = self.save()
        tec_register.import_table(self.c, path)
        self.assertEqual(tec.import_table(self.c, path), {'scanned': 2, 'target_rows': 1, 'model_assertions': 2})
        before = [tuple(r) for r in self.c.db.execute('SELECT * FROM tec_equipment')]
        for rows in [sample()[1:], sample() + sample(), [sample()[0][:-1]], []]:
            self.save(rows)
            with self.assertRaises(ValueError): tec_register.import_table(self.c, path)
            self.assertEqual(before, [tuple(r) for r in self.c.db.execute('SELECT * FROM tec_equipment')])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 2)

    def test_conflicting_sort_date_is_retained_for_review(self):
        rows = sample(); rows[0][9] = '20200101 04/01/2024'
        parsed = tec_register.register_rows(table(rows))
        self.assertIsNone(parsed[0][13])
        self.assertEqual(parsed[0][15], 'review_required')
        self.assertIn('issue_date', parsed[0][16])
