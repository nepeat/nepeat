import copy
import csv
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from devicehunt.catalog import Catalog, digest
from devicehunt import grantees as g


class GranteeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'test.sqlite')
        self.rows = [dict.fromkeys(g.FIELDS, '') for _ in range(3)]
        for row, code, name in zip(self.rows, ['2AVPL', 'HED', 'OAF'],
                                  ['Amazon Data Services, Inc.', 'Accton Technology Corp', '']):
            row.update(grantee_code=code, grantee_name=name,
                       date_received='02/06/2020 12:00:00 AM', country='United States')
        self.rows[1]['mailing_address'] = 'one line\nsecond line'
        self.meta = {'id': g.DATASET, 'name': 'EAS Equipment Authorization Grantee Registrations',
                     'columns': [{'fieldName': f} for f in g.FIELDS], 'rowsUpdatedAt': 1616396691}
        self.bundle = self.root / 'bundle.json'

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def save(self, rows=None, count=None, after=None):
        rows = self.rows if rows is None else rows
        out = io.StringIO(); writer = csv.DictWriter(out, fieldnames=g.FIELDS)
        writer.writeheader(); writer.writerows(rows)
        bodies = {'metadata_before': json.dumps(self.meta),
                  'metadata_after': json.dumps(after or self.meta),
                  'count_before': json.dumps([{'count': str(count if count is not None else len(rows))}]),
                  'count_after': json.dumps([{'count': str(count if count is not None else len(rows))}]),
                  'csv': out.getvalue()}
        urls = {'metadata_before': g.METADATA_URL, 'metadata_after': g.METADATA_URL,
                'count_before': g.COUNT_URL, 'count_after': g.COUNT_URL, 'csv': g.CSV_URL}
        files = {}
        for k, body in bodies.items():
            p = self.root / (k + '.txt'); p.write_text(body)
            files[k] = {'path': p.name, 'sha256': digest(p.read_bytes()), 'url': urls[k]}
        self.bundle.write_text(json.dumps({'schema_version': 1, 'dataset': g.DATASET, 'files': files}))
        return self.bundle

    def test_full_register_keeps_empty_names_and_multiline_addresses_without_devices(self):
        g.import_bundle(self.c, self.save())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_grantees').fetchone()[0], 3)
        self.assertEqual(self.c.search(), [])
        self.assertEqual(g.search(self.c.db, 'Amazon Data')[0]['grantee_code'], '2AVPL')
        self.assertEqual(g.search(self.c.db, 'hed')[0]['grantee_name'], 'Accton Technology Corp')
        self.assertEqual(g.search(self.c.db, '%'), [])
        self.assertEqual(g.search(self.c.db, '_'), [])
        self.assertEqual(self.c.db.execute('SELECT mailing_address FROM fcc_grantees WHERE grantee_code="HED"').fetchone()[0], 'one line\nsecond line')
        g.import_bundle(self.c, self.save())
        self.assertEqual(len(g.search(self.c.db)), 3)
        g.import_bundle(self.c, self.save(rows=self.rows[:1]))
        self.assertEqual(len(g.search(self.c.db)), 1)

    def test_bad_snapshot_preserves_previous_register(self):
        g.import_bundle(self.c, self.save())
        bad_cases = [dict(rows=self.rows[:1], count=3), dict(rows=self.rows + [self.rows[0]]),
                     dict(after={**self.meta, 'rowsUpdatedAt': 1616396692})]
        for case in bad_cases:
            with self.subTest(case=case):
                with self.assertRaises(ValueError):
                    g.import_bundle(self.c, self.save(**case))
                self.assertEqual(len(g.search(self.c.db)), 3)
        self.save(); (self.root / 'csv.txt').write_text('corrupt')
        with self.assertRaisesRegex(ValueError, 'checksum'):
            g.import_bundle(self.c, self.bundle)
        self.assertEqual(len(g.search(self.c.db)), 3)

    def test_collection_checks_before_and_after(self):
        self.save()
        fixture = json.loads(self.bundle.read_text())
        calls = []
        def fetch(url, path, refresh, max_bytes):
            calls.append(url)
            key = ('csv' if url == g.CSV_URL else 'count_before' if url == g.COUNT_URL else 'metadata_before')
            Path(path).write_bytes((self.root / fixture['files'][key]['path']).read_bytes())
            return Path(path)
        bundle = g.collect(self.root, fetch)
        self.assertEqual(calls, [g.METADATA_URL, g.COUNT_URL, g.CSV_URL, g.COUNT_URL, g.METADATA_URL])
        with patch('urllib.request.urlopen', side_effect=AssertionError('Network forbidden')):
            g.import_bundle(self.c, bundle)
        self.assertEqual(len(g.search(self.c.db)), 3)
