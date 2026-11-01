import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import snowcone as s
from devicehunt.catalog import Catalog, digest


class SnowconeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close(); self.temp.cleanup()

    def bundle(self):
        def dep(p): return {'path': p.name, 'sha256': digest(p.read_bytes())}
        b = dict(schema_version=1, index_url=s.INDEX_URL, guide_url=s.GUIDE_URL, family_url=s.FAMILY_URL)
        for key, count in [('guide', 10), ('family', 3)]:
            pdf = self.root / (key + '.pdf'); pdf.write_bytes(b'%PDF-1.4\nFixture only ' + key.encode())
            texts = [''] * count
            if key == 'guide':
                texts[0] = "Informazioni sulla sicurezza e la conformità di AWS Snowcone l'apparecchiatura radio tipo AF1 è conforme"
                for pn, (model, kind) in s.MODELS.items():
                    texts[pn-1] = ('1. Products in scope of this Declaration of Conformity: ' + model +
                                  ' 2. Manufacturer Identification Name: Amazon Data Services, Inc. Address: Seattle '
                                  '3. This declaration is issued under the sole responsibility of the manufacturer '
                                  '4. Object of the Declaration: ' + kind + ' 5. Brand: AWS 6. Standards')
            else:
                texts[0] = 'AWS Snowball and Snowcone Safety and Compliance Information'
                texts[2] = 'September 2022 ' + s.RETURN_TEXT
            side = self.root / (key + '.json')
            side.write_text(json.dumps({'pdf_sha256': digest(pdf.read_bytes()), 'extraction_method': 'pypdf embedded text; no OCR',
                                       'pages': [{'page': n, 'text': t} for n, t in enumerate(texts, 1)]}))
            b[key] = {'pdf': dep(pdf), 'text': dep(side)}
        p = self.root / 'index.html'
        p.write_text('<h1>AWS Snow Family Device Safety and Compliance Guide</h1><a href="' + s.FAMILY_URL + '">EU / English</a>')
        b['index'] = dep(p)
        p = self.root / 'bundle.json'; p.write_text(json.dumps(b)); return p

    def change(self, path, key, before, after):
        d = json.loads(path.read_text()); entry = d[key] if key == 'index' else d[key]['text']
        p = self.root / entry['path']; p.write_text(p.read_text().replace(before, after))
        entry['sha256'] = digest(p.read_bytes()); path.write_text(json.dumps(d))

    def test_full_scope_not_split_and_family_context_not_manufacturer_evidence(self):
        path = self.bundle()
        for _ in range(2): self.assertEqual(s.import_bundle(self.c, path), {'sources': 2, 'models': 2, 'model_assertions': 3})
        self.assertEqual({r['model'] for r in self.c.search(target='Amazon Data Services', role='manufacturer')}, {'AF1', 'AF1 BD1'})
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind IN ('alias','fcc_id')").fetchone()[0], 0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0], 2)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 3)
        context = self.c.db.execute("SELECT d.model,a.company,a.target,a.role FROM records r JOIN devices d ON d.id=r.device_id JOIN affiliations a ON a.record_id=r.id WHERE r.source_id='aws-snow-family-return-context'").fetchall()
        self.assertEqual([tuple(r) for r in context], [('AF1', 'AWS', None, 'documentation_vendor')])

    def test_changed_declaration_role_object_brand_or_spacing_rejected_before_mutation(self):
        for before, after in [('Name: Amazon Data Services, Inc.', 'Name: Amazon Web Services, Inc.'),
                              ('Data storage device', 'Dongle'), ('AF1 BD1', 'AF1, BD1'), ('Brand: AWS', 'Brand: Other')]:
            path = self.bundle(); s.import_bundle(self.c, path)
            old = [tuple(r) for r in self.c.db.execute('SELECT * FROM sources ORDER BY id')]
            self.change(path, 'guide', before, after)
            with self.assertRaises(ValueError): s.import_bundle(self.c, path)
            self.assertEqual(old, [tuple(r) for r in self.c.db.execute('SELECT * FROM sources ORDER BY id')])

    def test_guide_mapping_and_dated_return_context_required(self):
        for key, before, after in [('index', 'EU / English', 'Unrelated PDF'),
                                   ('family', 'September 2022', 'September 2026'),
                                   ('family', 'must be returned', 'may be retained'),
                                   ('guide', 'tipo AF1', 'tipo AF2')]:
            path = self.bundle(); self.change(path, key, before, after)
            with self.assertRaises(ValueError): s.prepare(path)

    def test_dependencies_and_text_binding(self):
        path = self.bundle(); d = json.loads(path.read_text()); d['family']['pdf']['sha256'] = '0' * 64
        path.write_text(json.dumps(d))
        with self.assertRaisesRegex(ValueError, 'checksum'): s.prepare(path)
        path = self.bundle(); self.change(path, 'guide', digest((self.root / 'guide.pdf').read_bytes()), '0' * 64)
        with self.assertRaisesRegex(ValueError, 'association'): s.prepare(path)
