import html
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import picos
from devicehunt.catalog import Catalog, digest
from devicehunt.pipeline import import_source


class PicosTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')
        self.rows = [
            ['100G Switch Portfolio', 'AS7726-32X (DCS204)', '32 x 100G', 'Trident 3×7', 'Intel x86'],
            ['1G Switch Portfolio', 'AS4610-30T', '24 x 1G-T 4 x 10G', 'Helix4', 'ARM Cortex A9'],
        ]

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def save(self, mutate_html=None, mutate_bundle=None):
        rows = [picos.HEADERS, *self.rows]
        body = ('<title>' + html.escape(picos.TITLE) + '</title><h1>' + picos.HEADING + '</h1>'
                '<nav>AS7760-32X AZ4444-52X navigation-only models</nav>'
                '<table data-testid="renderer-table">' + ''.join(
                    '<tr>' + ''.join('<td rowspan="1" colspan="1">' + html.escape(v) + '</td>'
                                    for v in row) + '</tr>' for row in rows) + '</table>')
        if mutate_html:
            body = mutate_html(body)
        p = self.root / 'table.html'; p.write_text(body)
        bundle = dict(schema_version=1, url=picos.URL, guide='PICOS-4.5_Configuration_Guide',
                      capture_method='original public HTML', retrieved_at='2026-09-27 05:59:43 UTC',
                      expected_rows=len(self.rows), html=dict(path=p.name, sha256=digest(p.read_bytes())))
        if mutate_bundle:
            mutate_bundle(bundle)
        path = self.root / 'bundle.json';path.write_text(json.dumps(bundle))
        return path

    def test_table_only_identity_aliases_and_row_scoped_specs(self):
        path = self.save()
        for _ in range(2):
            self.assertEqual(import_source(self.c, 'picos-hardware', path), {'platforms': 2})
        self.assertEqual(len(self.c.search('DCS204')), 1)
        self.assertEqual(self.c.search('AS7760-32X'), [])
        self.assertEqual(self.c.search('AZ4444-52X'), [])
        for target in ['Edge-Core', 'Accton']:
            self.assertEqual(len(self.c.search(target=target, role='compatibility_partner')), 2)
            self.assertEqual(self.c.search(target=target, role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        facts = dict(self.c.db.execute("SELECT f.name,f.value FROM facts f JOIN records r ON r.id=f.record_id WHERE r.source_key='AS4610-30T'"))
        self.assertEqual(facts['compatibility_cpu'], 'ARM Cortex A9')
        self.assertEqual(facts['compatibility_asic'], 'Helix4')
        self.assertEqual(facts['compatibility_ports'], '24 x 1G-T 4 x 10G')
        self.rows.pop()
        picos.import_bundle(self.c, self.save())
        self.assertEqual(self.c.search('AS4610'), [])

    def test_structural_drift_and_ambiguous_models_preserve_existing_records(self):
        picos.import_bundle(self.c, self.save())
        before = [tuple(r) for r in self.c.db.execute('SELECT * FROM records')]
        for change in [lambda s: s.replace('Switch ASIC', 'New column'),
                       lambda s: s.replace('rowspan="1"', 'rowspan="2"', 1),
                       lambda s: s.replace('AS4610-30T', 'AS7726-32X'),
                       lambda s: s.replace('AS4610-30T', 'AS4610-30T / AS4610-30P'),
                       lambda s: s.replace('DCS204', 'unknown code'),
                       lambda s: s.replace(picos.TITLE, 'Login')]:
            with self.assertRaises(ValueError):
                picos.import_bundle(self.c, self.save(mutate_html=change))
            self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM records')], before)

    def test_bundle_count_url_and_hash_are_checked_before_replacement(self):
        picos.import_bundle(self.c, self.save())
        for change in [lambda b: b.update(expected_rows=3), lambda b: b.update(expected_rows=True),
                       lambda b: b.update(url='https://example.com'),
                       lambda b: b['html'].update(sha256='0' * 64),
                       lambda b: b.update(capture_method='search extract')]:
            with self.assertRaises(ValueError):
                picos.import_bundle(self.c, self.save(mutate_bundle=change))
            self.assertEqual(len(self.c.search()), 2)


if __name__ == '__main__':
    unittest.main()
