import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import oracle_components as oracle
from devicehunt.catalog import Catalog, digest


class OracleComponentsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close(); self.temp.cleanup()

    def body(self, slug):
        system, suffix, count, total, mfg = oracle.PROFILES[slug]
        body = f'<h2>{system} PCIe I/O Cards</h2><div id="FIXTURE__{suffix}">'
        body += system + ' Controller NIC/HBA Options<table><tr><th>Mktg Part Number</th><th>Description</th>'
        body += '<th>Mfg Part Number</th>' if mfg else ''
        body += '</tr>'
        for i in range(count):
            part = f'<p>PART-{i}</p>'; desc = f'<p>{slug} option {i}</p>'
            if i == 0 and total > count:
                part += '<p>PART-EXTRA</p>'; desc += '<p>Second explicit paragraph</p>'
            body += f'<tr><td>{part}</td><td>{desc}</td>'
            if mfg: body += f'<td><p>MFG-{i}</p></td>'
            body += '</tr>'
        return body + '</table></div><table><tr><td>NOT-A-PART</td></tr></table>'

    def bundle(self, change=None):
        data = {'schema_version': 1, 'pages': {}}
        for slug in oracle.PROFILES:
            body = self.body(slug)
            if change: body = change(slug, body)
            p = self.root / (slug + '.html'); p.write_text(body)
            data['pages'][slug] = {'url': oracle.BASE + slug + '-pcie-io-cards.html',
                                    'html': {'path': p.name, 'sha256': digest(p.read_bytes())}}
        p = self.root / 'bundle.json'; p.write_text(json.dumps(data)); return p

    def test_paragraph_pairing_and_source_scoped_mapping(self):
        result = oracle.import_bundle(self.c, self.bundle())
        self.assertEqual(result, {'sources': 3, 'assertions': 30, 'marketing_parts': 11})
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.assertIsNone(self.c.db.execute("SELECT 1 FROM devices WHERE model='NOT-A-PART'").fetchone())
        rows = self.c.db.execute("SELECT r.raw_json,r.source_id FROM records r JOIN devices d ON d.id=r.device_id WHERE d.model='PART-0'").fetchall()
        self.assertEqual(len(rows), 3)
        values = [json.loads(r['raw_json']) for r in rows]
        self.assertEqual(sum(bool(r['manufacturing_part_number']) for r in values), 1)
        self.assertEqual(len({r['description'] for r in values}), 3)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM identifiers').fetchone()[0], 9)
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind='alias'").fetchone()[0], 0)
        extra = json.loads(self.c.db.execute("SELECT raw_json FROM records r JOIN devices d ON d.id=r.device_id WHERE d.model='PART-EXTRA'").fetchone()[0])
        self.assertEqual(extra['description'], 'Second explicit paragraph')
        self.assertEqual((extra['table_row'], extra['paragraph']), (1, 2))

    def test_bad_last_page_does_not_replace_prior_evidence(self):
        oracle.import_bundle(self.c, self.bundle())
        before = [tuple(r) for r in self.c.db.execute('SELECT * FROM records ORDER BY id')]
        def change(slug, body):
            return body.replace('Mktg Part Number', 'Unknown header') if slug.endswith('zs3-4') else body
        with self.assertRaises(ValueError): oracle.import_bundle(self.c, self.bundle(change))
        self.assertEqual(before, [tuple(r) for r in self.c.db.execute('SELECT * FROM records ORDER BY id')])

    def test_unpaired_duplicate_spanning_or_unparsed_cells_rejected(self):
        slug = 'oracle-zfs-storage-zs3-2'; body = self.body(slug)
        variants = [body.replace('<p>Second explicit paragraph</p>', ''),
                    body.replace('PART-EXTRA', 'PART-0'),
                    body.replace('<td>', '<td colspan="2">', 1),
                    body.replace('<p>PART-0</p>', 'unparsed<p>PART-0</p>'),
                    body.replace('<p>PART-0</p>', '<p>PART-0 PART-OTHER</p>'),
                    body.replace('PCIe I/O Cards', 'Different page')]
        for variant in variants:
            with self.subTest(variant=variant[:60]), self.assertRaises(ValueError): oracle.parse(variant, slug)

    def test_bundle_hash_scope_and_dependency_rejected(self):
        for failure in ['hash', 'missing', 'url', 'path']:
            path = self.bundle(); data = json.loads(path.read_text()); key = next(iter(data['pages']))
            if failure == 'hash': data['pages'][key]['html']['sha256'] = '0' * 64
            if failure == 'missing': del data['pages'][key]
            if failure == 'url': data['pages'][key]['url'] = 'https://other.example/test'
            if failure == 'path': data['pages'][key]['html']['path'] = '../test.html'
            path.write_text(json.dumps(data))
            with self.assertRaises(ValueError): oracle.import_bundle(self.c, path)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 0)

    def test_repeat_import_is_stable(self):
        path = self.bundle(); oracle.import_bundle(self.c, path)
        before = [tuple(r) for r in self.c.db.execute('SELECT * FROM search_index ORDER BY device_id')]
        oracle.import_bundle(self.c, path)
        self.assertEqual(before, [tuple(r) for r in self.c.db.execute('SELECT * FROM search_index ORDER BY device_id')])
        self.assertEqual(self.c.db.execute('PRAGMA foreign_key_check').fetchall(), [])
