import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import panorama as p
from devicehunt.catalog import Catalog, digest


class PanoramaTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def bundle(self):
        def entry(path): return {'path': path.name, 'sha256': digest(path.read_bytes())}
        pdf = self.root / 'guide.pdf'; pdf.write_bytes(b'%PDF-1.4\nTest fixture, not an original document')
        pages = [{'page': n, 'text': ''} for n in range(1, 12)]
        pages[2]['text'] = ('The party responsible for FCC compliance is Amazon Development Center US, Inc., '
                           '410 Terry Ave North Device Name: AWS Panorama Appliance (Hub) Model: AMHT-1')
        pages[9]['text'] = ('CC SVC 00233019 2022 TR Declaration of Conformity '
                           '1. Product in scope of this Declaration of Conformity: Appliance, model: AMHT-1 '
                           '2. Manufacturer: Amazon Data Services, Inc. Address: 410 Terry Avenue North, '
                           'Seattle, Washington, 98109, United States of America '
                           '3. This Declaration is issued under the sole responsibility of the manufacturer. '
                           '4. Object of the Declaration: Appliance, Model: AMHT-1 Brand: AWS')
        side = self.root / 'text.json'
        side.write_text(json.dumps({'pdf_sha256': digest(pdf.read_bytes()),
                                   'extraction_method': 'pypdf embedded text; no OCR', 'pages': pages}))
        notice = self.root / 'notice.html'
        notice.write_text('<h1>AWS Panorama end of support</h1>' + ''.join(
            '<p>' + t + '</p>' for t in [p.INTRO, p.OFFLINE, p.PLATFORM + ' Consider replacement hardware.']))
        bundle = self.root / 'bundle.json'
        bundle.write_text(json.dumps({'schema_version': 1, 'pdf_url': p.PDF_URL, 'notice_url': p.NOTICE_URL,
                                      'pdf': entry(pdf), 'text': entry(side), 'notice': entry(notice)}))
        return bundle

    def replace(self, path, key, before, after):
        data = json.loads(path.read_text()); f = path.parent / data[key]['path']
        f.write_text(f.read_text().replace(before, after))
        data[key]['sha256'] = digest(f.read_bytes()); path.write_text(json.dumps(data))

    def test_idempotent_roles_identity_and_service_context(self):
        path = self.bundle()
        for _ in range(2):
            self.assertEqual(p.import_bundle(self.c, path), {'sources': 2, 'models': 1, 'model_assertions': 3})
        self.assertEqual([d['model'] for d in self.c.search(target='Amazon Data Services', role='manufacturer')], ['AMHT-1'])
        rows = [tuple(r) for r in self.c.db.execute('SELECT company,target,role FROM affiliations ORDER BY company,role')]
        self.assertIn((p.MANUFACTURER, 'Amazon Data Services', 'manufacturer'), rows)
        self.assertIn((p.FCC_PARTY, None, 'fcc_responsible_party'), rows)
        self.assertIn(('AWS', None, 'brand'), rows)
        self.assertEqual(sum(target is not None for _, target, _ in rows), 1)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 3)
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind IN ('fcc_id','alias')").fetchone()[0], 0)
        facts = dict(self.c.db.execute("SELECT name,value FROM facts WHERE name LIKE 'service_%'"))
        self.assertEqual(facts['service_end_date'], '2026-05-31')
        self.assertEqual(facts['service_new_customer_end_date'], '2025-05-20')
        self.assertEqual(facts['service_cloud_dependency'], p.OFFLINE)

    def test_changed_model_manufacturer_or_fcc_party_rejects_before_replacement(self):
        for before, after in [('AMHT-1', 'AMHT-2'), ('Manufacturer: Amazon Data Services, Inc.', 'Manufacturer: AWS'),
                              ('responsible for FCC compliance is Amazon Development Center US, Inc.',
                               'responsible for FCC compliance is Amazon Data Services, Inc.'), ('Brand: AWS', 'Brand: Other')]:
            path = self.bundle(); p.import_bundle(self.c, path)
            old = [tuple(r) for r in self.c.db.execute('SELECT * FROM sources ORDER BY id')]
            self.replace(path, 'text', before, after)
            with self.assertRaises(ValueError): p.import_bundle(self.c, path)
            self.assertEqual(old, [tuple(r) for r in self.c.db.execute('SELECT * FROM sources ORDER BY id')])

    def test_changed_notice_or_lost_cloud_dependency_rejected(self):
        for before, after in [('May 31, 2026', 'May 31, 2027'), (p.OFFLINE, ''),
                              (p.PLATFORM, 'The hardware is independently usable.'), (p.INTRO, p.INTRO + '</p><p>' + p.INTRO)]:
            path = self.bundle(); p.import_bundle(self.c, path)
            self.replace(path, 'notice', before, after)
            with self.assertRaises(ValueError): p.import_bundle(self.c, path)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 3)

    def test_pdf_association_checksum_page_order_and_scope(self):
        path = self.bundle(); self.replace(path, 'text', digest((self.root / 'guide.pdf').read_bytes()), '0' * 64)
        with self.assertRaisesRegex(ValueError, 'association'): p.prepare(path)
        path = self.bundle(); self.replace(path, 'text', '"page": 10', '"page": 11')
        with self.assertRaises(ValueError): p.prepare(path)
        path = self.bundle(); data = json.loads(path.read_text()); data['pdf']['sha256'] = '0' * 64
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, 'checksum'): p.prepare(path)
        path = self.bundle(); data = json.loads(path.read_text()); data['notice_url'] += '?other'
        path.write_text(json.dumps(data))
        with self.assertRaises(ValueError): p.prepare(path)
