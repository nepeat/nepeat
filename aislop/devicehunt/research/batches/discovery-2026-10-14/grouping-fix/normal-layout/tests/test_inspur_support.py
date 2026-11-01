import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from devicehunt import inspur_support, lookup
from devicehunt.catalog import Catalog, digest
from devicehunt.pipeline import import_source
from devicehunt.rebuild import rebuild


def row(pid, name, parent='115', level='4'):
    return dict(typeTreeId=pid, typeTreeParentId=parent, typeTreeCname=name,
                typeTreeLevel=level, typeTreeLeaf='1', unreviewed_field={'preserve': True})


class InspurSupportTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.tmp.cleanup()

    def bundle(self, rows, change=None):
        def save(name, value):
            data = value.encode()
            (self.root / name).write_bytes(data)
            return dict(path=name, sha256=digest(data))
        data = dict(schema_version=1, source_url=inspur_support.API_URL,
                    index_url=inspur_support.INDEX_URL, expected_rows=len(rows),
                    index=save('index.html', inspur_support.API_PATH if change != 'index' else 'unrelated'),
                    products=save('products.json', json.dumps(rows)))
        if change == 'hash': data['products']['sha256'] = '0' * 64
        if change == 'count': data['expected_rows'] += 1
        if change == 'bool_count': data['expected_rows'] = True
        if change == 'url': data['source_url'] = 'https://example.org'
        path = self.root / 'bundle.json'
        path.write_text(json.dumps(data))
        return path

    def test_raw_groups_duplicates_parts_and_null_metadata_create_no_identities(self):
        rows = [row('1', 'i24&NS5162M5'), row('2', 'i24&NS5162M5', level=None),
                row('3', 'GPU Tesla-T4', '4511'), row('4', 'InCloud OS', '8320'),
                row('5', 'Straße_100%')]
        result = import_source(self.c, 'inspur-support', self.bundle(rows))
        self.assertEqual(result, dict(support_rows=5, distinct_names=4, catalog_assertions=0))
        stored = list(self.c.db.execute('SELECT * FROM inspur_support_entries ORDER BY row_number'))
        self.assertEqual([json.loads(r['raw_json']) for r in stored], rows)
        self.assertIsNone(stored[1]['level_raw'])
        for table in ('devices', 'records', 'affiliations', 'identifiers', 'hyperscaler_evidence'):
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM ' + table).fetchone()[0], 0)
        sections = {s['dataset']: s for s in lookup.run(self.c.db, 'i24&NS5162M5', 1)['datasets']}
        self.assertEqual(sections['inspur_support']['matches'], 2)
        self.assertTrue(sections['inspur_support']['truncated'])
        self.assertEqual(sections['catalog']['matches'], 0)
        section = next(s for s in lookup.run(self.c.db, 'STRASSE_100%')['datasets'] if s['dataset'] == 'inspur_support')
        self.assertEqual(section['matches'], 1)

    def test_invalid_captures_preserve_prior_rows(self):
        rows = [row('1', 'NF5280M5')]
        inspur_support.import_bundle(self.c, self.bundle(rows))
        before = {t: [tuple(r) for r in self.c.db.execute('SELECT * FROM ' + t)]
                  for t in ('sources', 'inspur_support_entries')}
        for change in ('hash', 'count', 'bool_count', 'url', 'index'):
            with self.subTest(change=change):
                with self.assertRaises(ValueError):
                    inspur_support.import_bundle(self.c, self.bundle(rows, change))
                for table, expected in before.items():
                    self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM ' + table)], expected)
        for invalid in ([], [rows[0], rows[0]], [row('2', '')], [row('2', 123)], [row('2', 'OK', level={})]):
            with self.assertRaises(ValueError):
                inspur_support.import_bundle(self.c, self.bundle(invalid))

    def test_reimport_is_idempotent_and_preserves_other_sources(self):
        self.c.source('other', 'https://example.org', 'vendor', 'test', 'test')
        self.c.add('other', 'kept', 'https://example.org', 'Oracle', 'KEPT', 'KEPT', 'server', {})
        self.c.finish('other', 1)
        path = self.bundle([row('1', 'OLD'), row('2', 'UNCHANGED')])
        inspur_support.import_bundle(self.c, path)
        path = self.bundle([row('3', 'NEW')])
        inspur_support.import_bundle(self.c, path)
        inspur_support.import_bundle(self.c, path)
        self.assertEqual([r[0] for r in self.c.db.execute('SELECT name FROM inspur_support_entries')], ['NEW'])
        self.assertEqual([r[0] for r in self.c.db.execute('SELECT model FROM devices')], ['KEPT'])

    def test_offline_rebuild_checks_raw_count_and_lookup_leaves_database_unchanged(self):
        bundle = self.bundle([row('1', 'NF5280M5')])
        manifest = self.root / 'manifest.json'
        data = dict(schema_version=1, imports=[dict(source='inspur-support', path=bundle.name)],
                    files={p.name: digest(p.read_bytes()) for p in (bundle, self.root/'index.html', self.root/'products.json')},
                    expected=dict(devices=0, records=0, sources={inspur_support.SOURCE: 1}, inspur_support_entries=1))
        manifest.write_text(json.dumps(data))
        output = self.root / 'rebuilt.sqlite'
        with patch('urllib.request.urlopen', side_effect=AssertionError('Network forbidden')):
            result = rebuild(manifest, output)
        self.assertEqual(result['inspur_support_entries'], 1)
        before = output.read_bytes()
        c = Catalog(output, read_only=True)
        self.assertEqual(c.db.execute('PRAGMA user_version').fetchone()[0], 29)
        self.assertTrue(next(s for s in lookup.run(c.db, 'NF5280M5')['datasets'] if s['dataset']=='inspur_support')['rows'])
        c.db.close()
        self.assertEqual(output.read_bytes(), before)
        data['expected']['inspur_support_entries'] = 2
        manifest.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, 'counts differ'):
            rebuild(manifest, self.root/'wrong.sqlite')
        self.assertFalse((self.root/'wrong.sqlite').exists())
